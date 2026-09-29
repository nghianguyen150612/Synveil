//! Linux native-package reconciliation policy.
//!
//! This module describes policy; it never invokes a package manager. dpkg/rpm
//! remain transaction authorities and adapters report their observed outcome.

use crate::{EffectAuthority, ResourceClass};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};

pub const LINUX_PACKAGE_INTEGRATION_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LinuxPackageFormat {
    Deb,
    Rpm,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LinuxPackageOperation {
    Install,
    Upgrade,
    Repair,
    Uninstall,
    Verify,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LinuxPackageIntegrationState {
    NotInstalled,
    Installed,
    NeedsRepair,
    PackageManagerBusy,
    Inconsistent,
    Unsupported,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageSurface {
    pub path: String,
    pub owner: ResourceClass,
    pub verification: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FormatContract {
    pub format: LinuxPackageFormat,
    pub package_identity: String,
    pub artifact_type: String,
    pub architectures: Vec<String>,
    pub runtime_dependencies: Vec<String>,
    pub hooks: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LinuxPackageContract {
    pub schema_version: u32,
    pub product_version: String,
    pub binary_identity: String,
    pub desktop_application_id: String,
    pub autostart: bool,
    pub formats: Vec<FormatContract>,
    pub surfaces: Vec<PackageSurface>,
    pub preservation: Vec<ResourceClass>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinuxPackageError {
    InvalidJson,
    UnsupportedSchema,
    InvalidIdentity,
    DuplicateFormat,
    DuplicatePath,
    InvalidPath,
    UnauthorizedOwnership,
    MissingPreservation,
    AutostartForbidden,
    MissingVerification,
    UnsupportedFormat,
    WrongAuthority,
    FinalVerificationRequired,
    IncompatibleUpgrade,
    SymlinkEscape,
    InvalidFormatMetadata,
    DuplicatePreservation,
    InvalidRoot,
    InvalidSurfaceSet,
}

const PROTECTED: [ResourceClass; 8] = [
    ResourceClass::ApplicationConfig,
    ResourceClass::CredentialState,
    ResourceClass::ClientSyncState,
    ResourceClass::UserLibraryContent,
    ResourceClass::ServerConfig,
    ResourceClass::ServerDatabase,
    ResourceClass::ServerObjectData,
    ResourceClass::ExternalDependency,
];

impl LinuxPackageContract {
    pub fn from_json(input: &str) -> Result<Self, LinuxPackageError> {
        let value: Self =
            serde_json::from_str(input).map_err(|_| LinuxPackageError::InvalidJson)?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), LinuxPackageError> {
        if self.schema_version != LINUX_PACKAGE_INTEGRATION_SCHEMA_VERSION {
            return Err(LinuxPackageError::UnsupportedSchema);
        }
        if self.product_version != PRODUCT_VERSION
            || self.binary_identity != BINARY_IDENTITY
            || self.desktop_application_id != DESKTOP_APPLICATION_ID
            || !valid_identity(&self.binary_identity)
            || !valid_identity(&self.desktop_application_id)
        {
            return Err(LinuxPackageError::InvalidIdentity);
        }
        if self.autostart {
            return Err(LinuxPackageError::AutostartForbidden);
        }
        let mut formats = BTreeSet::new();
        for (index, format) in self.formats.iter().enumerate() {
            if !formats.insert(format.format as u8) {
                return Err(LinuxPackageError::DuplicateFormat);
            }
            if format.package_identity != "synveil"
                || !matches!(
                    (format.format, format.artifact_type.as_str()),
                    (LinuxPackageFormat::Deb, "deb") | (LinuxPackageFormat::Rpm, "rpm")
                )
            {
                return Err(LinuxPackageError::InvalidIdentity);
            }
            let (expected_format, architectures, dependencies, hooks) = match index {
                0 => (
                    LinuxPackageFormat::Deb,
                    DEB_ARCHITECTURES,
                    DEB_RUNTIME_DEPENDENCIES,
                    DEB_HOOKS,
                ),
                1 => (
                    LinuxPackageFormat::Rpm,
                    RPM_ARCHITECTURES,
                    RPM_RUNTIME_DEPENDENCIES,
                    RPM_HOOKS,
                ),
                _ => return Err(LinuxPackageError::UnsupportedFormat),
            };
            if format.format != expected_format
                || !metadata_is_exact(&format.architectures, architectures)
                || !metadata_is_exact(&format.runtime_dependencies, dependencies)
                || !metadata_is_exact(&format.hooks, hooks)
            {
                return Err(LinuxPackageError::InvalidFormatMetadata);
            }
        }
        if formats.len() != 2 {
            return Err(LinuxPackageError::UnsupportedFormat);
        }
        let mut paths = BTreeSet::new();
        for surface in &self.surfaces {
            validate_path(&surface.path)?;
            if !paths.insert(&surface.path) {
                return Err(LinuxPackageError::DuplicatePath);
            }
            if !matches!(
                surface.owner,
                ResourceClass::PackageOwned | ResourceClass::PlatformIntegrationOwned
            ) {
                return Err(LinuxPackageError::UnauthorizedOwnership);
            }
            if surface.verification.trim().is_empty() {
                return Err(LinuxPackageError::MissingVerification);
            }
        }
        if self.surfaces.len() != EXPECTED_SURFACES.len()
            || self.surfaces.iter().zip(EXPECTED_SURFACES.iter()).any(
                |(surface, (path, verification))| {
                    surface.path != *path
                        || surface.owner != ResourceClass::PackageOwned
                        || surface.verification != *verification
                },
            )
        {
            return Err(LinuxPackageError::InvalidSurfaceSet);
        }
        if self.preservation.iter().collect::<BTreeSet<_>>().len() != self.preservation.len() {
            return Err(LinuxPackageError::DuplicatePreservation);
        }
        if self.preservation.len() != PROTECTED.len()
            || PROTECTED
                .iter()
                .any(|item| !self.preservation.contains(item))
        {
            return Err(LinuxPackageError::MissingPreservation);
        }
        Ok(())
    }

    pub fn authorize(
        &self,
        operation: LinuxPackageOperation,
        authority: EffectAuthority,
        resource: ResourceClass,
        final_verification: bool,
        compatible_upgrade: bool,
    ) -> Result<(), LinuxPackageError> {
        if !final_verification {
            return Err(LinuxPackageError::FinalVerificationRequired);
        }
        if operation == LinuxPackageOperation::Upgrade && !compatible_upgrade {
            return Err(LinuxPackageError::IncompatibleUpgrade);
        }
        let expected = match resource {
            ResourceClass::NativePackageState | ResourceClass::PackageOwned => {
                EffectAuthority::NativePackageManager
            }
            ResourceClass::PlatformIntegrationOwned => EffectAuthority::PlatformIntegrationAdapter,
            _ => return Err(LinuxPackageError::UnauthorizedOwnership),
        };
        if authority != expected {
            return Err(LinuxPackageError::WrongAuthority);
        }
        Ok(())
    }
}

fn valid_identity(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.'))
}

const PRODUCT_VERSION: &str = "0.1.0";
const BINARY_IDENTITY: &str = "synveil-desktop";
const DESKTOP_APPLICATION_ID: &str = "synveil.desktop";
const EXPECTED_SURFACES: &[(&str, &str)] = &[
    ("/usr/bin/synveil-scheduled-maintenance-once", "executable"),
    ("/usr/bin/synveil-client", "executable"),
    ("/usr/bin/synveil-desktop", "executable"),
    (
        "/usr/lib/systemd/user/synveil-client.service",
        "static-user-unit",
    ),
    (
        "/usr/lib/systemd/system/synveil-scheduled-maintenance.service",
        "static-system-unit",
    ),
    (
        "/usr/lib/systemd/system/synveil-scheduled-maintenance.timer",
        "static-system-unit",
    ),
    ("/usr/lib/sysusers.d/synveil.conf", "sysusers-config"),
    ("/usr/lib/tmpfiles.d/synveil.conf", "tmpfiles-config"),
    ("/usr/share/applications/synveil.desktop", "desktop-entry"),
    ("/usr/share/icons/hicolor/scalable/apps/synveil.svg", "icon"),
    ("/usr/share/doc/synveil/LICENSE", "regular-file"),
    ("/usr/share/doc/synveil/NOTICE", "regular-file"),
    (
        "/usr/share/synveil/synveil-scheduled-maintenance.env.example",
        "regular-file",
    ),
];

const DEB_ARCHITECTURES: &[&str] = &["amd64", "arm64"];
const RPM_ARCHITECTURES: &[&str] = &["x86_64", "aarch64"];
const DEB_HOOKS: &[&str] = &["postinst", "prerm", "postrm"];
const RPM_HOOKS: &[&str] = &["post", "preun", "postun"];
const DEB_RUNTIME_DEPENDENCIES: &[&str] = &[
    "systemd",
    "libc6 (>= 2.34)",
    "libgcc-s1",
    "libstdc++6",
    "libdbus-1-3",
    "libsystemd0",
    "libqt6core6",
    "libqt6gui6",
    "libqt6widgets6",
    "libqt6qml6",
    "libqt6quick6",
    "libqt6quickcontrols2-6",
    "libqt6network6",
    "qt6-qpa-plugins",
    "qml6-module-qtqml",
    "qml6-module-qtqml-models",
    "qml6-module-qtqml-workerscript",
    "qml6-module-qtquick",
    "qml6-module-qtquick-controls",
    "qml6-module-qtquick-dialogs",
    "qml6-module-qtquick-layouts",
    "qml6-module-qtquick-templates",
    "qml6-module-qtquick-window",
];
const RPM_RUNTIME_DEPENDENCIES: &[&str] = &[
    "systemd",
    "systemd-libs",
    "glibc",
    "libgcc",
    "libstdc++",
    "dbus-libs",
    "qt6-qtbase",
    "qt6-qtdeclarative",
    "qt6-qtquickcontrols2",
];

fn metadata_is_exact(actual: &[String], expected: &[&str]) -> bool {
    !actual.is_empty()
        && actual.len() == expected.len()
        && actual.iter().zip(expected).all(|(actual, expected)| {
            actual == expected
                && actual.len() <= 128
                && actual.bytes().all(|byte| !byte.is_ascii_control())
        })
}

fn validate_path(path: &str) -> Result<(), LinuxPackageError> {
    if !path.starts_with('/')
        || path.contains('\\')
        || path.bytes().any(|byte| byte.is_ascii_control())
        || path
            .split('/')
            .skip(1)
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(LinuxPackageError::InvalidPath);
    }
    let components: Vec<_> = path.trim_start_matches('/').split('/').collect();
    if components.len() < 3
        || components[0] != "usr"
        || !matches!(components[1], "bin" | "lib" | "share")
    {
        return Err(LinuxPackageError::InvalidPath);
    }
    Ok(())
}

/// Reject a symlink in an existing staged destination's ancestry. Missing
/// suffixes are allowed because package managers create them transactionally.
pub fn validate_live_package_path(root: &Path, path: &str) -> Result<(), LinuxPackageError> {
    validate_path(path)?;
    let root_metadata =
        std::fs::symlink_metadata(root).map_err(|_| LinuxPackageError::InvalidRoot)?;
    if root_metadata.file_type().is_symlink() {
        return Err(LinuxPackageError::SymlinkEscape);
    }
    if !root_metadata.is_dir() {
        return Err(LinuxPackageError::InvalidRoot);
    }
    let mut candidate = root.to_path_buf();
    for component in path.trim_start_matches('/').split('/') {
        candidate.push(component);
        match std::fs::symlink_metadata(&candidate) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(LinuxPackageError::SymlinkEscape);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return Err(LinuxPackageError::InvalidPath),
        }
    }
    Ok(())
}
