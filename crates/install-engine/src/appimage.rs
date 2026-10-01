//! Current-user AppImage integration. Portable execution never calls this module.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    process::Command,
};

pub const APPIMAGE_INTEGRATION_SCHEMA_VERSION: u32 = 1;
const DESKTOP_NAME: &str = "synveil-appimage.desktop";
const ICON_RELATIVE: &str = "icons/hicolor/scalable/apps/synveil.svg";
const UNIT_NAME: &str = "synveil-appimage-client.service";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AppImageIntegrationStatus {
    NotIntegrated,
    Healthy,
    StaleAppImagePath,
    NeedsRepair,
    Incomplete,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppImageIntegrationRecord {
    pub schema_version: u32,
    pub appimage_path: PathBuf,
    pub artifact_sha256: String,
    pub desktop_entry_path: PathBuf,
    pub icon_path: PathBuf,
    pub user_unit_path: PathBuf,
    pub startup_enabled: bool,
}

#[derive(Debug)]
pub enum AppImageIntegrationError {
    InvalidEnvironment,
    InvalidArtifact,
    UnsafePath,
    UnownedSurface,
    Io(io::Error),
    InvalidRecord,
    UserSystemdUnavailable,
    SystemctlFailed,
}

impl From<io::Error> for AppImageIntegrationError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Clone, Debug)]
pub struct AppImageIntegration {
    data_home: PathBuf,
    state_home: PathBuf,
    config_home: PathBuf,
}

impl AppImageIntegration {
    pub fn from_environment() -> Result<Self, AppImageIntegrationError> {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or(AppImageIntegrationError::InvalidEnvironment)?;
        if !home.is_absolute() {
            return Err(AppImageIntegrationError::InvalidEnvironment);
        }
        Ok(Self {
            data_home: xdg("XDG_DATA_HOME", &home, ".local/share")?,
            state_home: xdg("XDG_STATE_HOME", &home, ".local/state")?,
            config_home: xdg("XDG_CONFIG_HOME", &home, ".config")?,
        })
    }

    pub fn paths(&self) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        (
            self.data_home.join("applications").join(DESKTOP_NAME),
            self.data_home.join(ICON_RELATIVE),
            self.state_home.join("synveil/appimage-integration.json"),
            self.config_home.join("systemd/user").join(UNIT_NAME),
        )
    }

    pub fn inspect(&self) -> Result<AppImageIntegrationStatus, AppImageIntegrationError> {
        let (desktop, icon, record_path, unit) = self.paths();
        let existing = [
            desktop.exists(),
            icon.exists(),
            record_path.exists(),
            unit.exists(),
        ];
        if !existing.iter().any(|v| *v) {
            return Ok(AppImageIntegrationStatus::NotIntegrated);
        }
        if !record_path.is_file() {
            return Ok(AppImageIntegrationStatus::Incomplete);
        }
        let record = self.read_record()?;
        if !record.appimage_path.is_file() {
            return Ok(AppImageIntegrationStatus::StaleAppImagePath);
        }
        if !desktop.is_file() || !icon.is_file() || !unit.is_file() {
            return Ok(AppImageIntegrationStatus::NeedsRepair);
        }
        if fs::read_to_string(&desktop)? != desktop_entry(&record.appimage_path)?
            || fs::read_to_string(&unit)? != user_unit(&record.appimage_path)?
            || record.desktop_entry_path != desktop
            || record.icon_path != icon
            || record.user_unit_path != unit
        {
            return Ok(AppImageIntegrationStatus::NeedsRepair);
        }
        Ok(AppImageIntegrationStatus::Healthy)
    }

    /// Install or explicitly reconcile the one authoritative registration.
    pub fn install(
        &self,
        artifact: &Path,
        icon_source: &Path,
    ) -> Result<AppImageIntegrationRecord, AppImageIntegrationError> {
        let artifact = validate_artifact(artifact)?;
        let icon = fs::read(icon_source).map_err(|_| AppImageIntegrationError::InvalidArtifact)?;
        if icon.is_empty() {
            return Err(AppImageIntegrationError::InvalidArtifact);
        }
        let (desktop, icon_path, record_path, unit) = self.paths();
        atomic_owned_write(&desktop, desktop_entry(&artifact)?.as_bytes(), 0o644)?;
        atomic_owned_write(&icon_path, &icon, 0o644)?;
        atomic_owned_write(&unit, user_unit(&artifact)?.as_bytes(), 0o644)?;
        let enabled = self
            .read_record()
            .map(|r| r.startup_enabled)
            .unwrap_or(false);
        let record = AppImageIntegrationRecord {
            schema_version: APPIMAGE_INTEGRATION_SCHEMA_VERSION,
            artifact_sha256: sha256(&artifact)?,
            appimage_path: artifact,
            desktop_entry_path: desktop,
            icon_path,
            user_unit_path: unit,
            startup_enabled: enabled,
        };
        atomic_owned_write(
            &record_path,
            &serde_json::to_vec_pretty(&record)
                .map_err(|_| AppImageIntegrationError::InvalidRecord)?,
            0o600,
        )?;
        if self.inspect()? != AppImageIntegrationStatus::Healthy {
            return Err(AppImageIntegrationError::InvalidRecord);
        }
        Ok(record)
    }

    pub fn repair(
        &self,
        artifact: &Path,
        icon: &Path,
    ) -> Result<AppImageIntegrationRecord, AppImageIntegrationError> {
        self.install(artifact, icon)
    }

    pub fn remove(&self) -> Result<(), AppImageIntegrationError> {
        let (desktop, icon, record, unit) = self.paths();
        for path in [&desktop, &icon, &unit, &record] {
            remove_owned(path)?;
        }
        Ok(())
    }

    pub fn set_startup(&self, enable: bool) -> Result<(), AppImageIntegrationError> {
        let mut record = self.read_record()?;
        if !user_systemd_available() {
            return Err(AppImageIntegrationError::UserSystemdUnavailable);
        }
        let action = if enable { "enable" } else { "disable" };
        let status = Command::new("systemctl")
            .args(["--user", action, UNIT_NAME])
            .status()?;
        if !status.success() {
            return Err(AppImageIntegrationError::SystemctlFailed);
        }
        record.startup_enabled = enable;
        let (_, _, path, _) = self.paths();
        atomic_owned_write(
            &path,
            &serde_json::to_vec_pretty(&record)
                .map_err(|_| AppImageIntegrationError::InvalidRecord)?,
            0o600,
        )
    }

    fn read_record(&self) -> Result<AppImageIntegrationRecord, AppImageIntegrationError> {
        let (_, _, path, _) = self.paths();
        let record: AppImageIntegrationRecord = serde_json::from_slice(&fs::read(path)?)
            .map_err(|_| AppImageIntegrationError::InvalidRecord)?;
        if record.schema_version != APPIMAGE_INTEGRATION_SCHEMA_VERSION {
            return Err(AppImageIntegrationError::InvalidRecord);
        }
        Ok(record)
    }
}

fn xdg(name: &str, home: &Path, fallback: &str) -> Result<PathBuf, AppImageIntegrationError> {
    let value = env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(fallback));
    if !value.is_absolute() {
        return Err(AppImageIntegrationError::InvalidEnvironment);
    }
    Ok(value)
}

fn validate_artifact(path: &Path) -> Result<PathBuf, AppImageIntegrationError> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(AppImageIntegrationError::UnsafePath);
    }
    let metadata =
        fs::symlink_metadata(path).map_err(|_| AppImageIntegrationError::InvalidArtifact)?;
    if !metadata.file_type().is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(AppImageIntegrationError::InvalidArtifact);
    }
    let mut header = [0_u8; 11];
    fs::File::open(path)?
        .read_exact(&mut header)
        .map_err(|_| AppImageIntegrationError::InvalidArtifact)?;
    if &header[..4] != b"\x7fELF" || &header[8..11] != b"AI\x02" {
        return Err(AppImageIntegrationError::InvalidArtifact);
    }
    fs::canonicalize(path).map_err(Into::into)
}

fn quote_exec(path: &Path) -> Result<String, AppImageIntegrationError> {
    let value = path.to_str().ok_or(AppImageIntegrationError::UnsafePath)?;
    if value.chars().any(|c| c == '\n' || c == '\r' || c == '\0') {
        return Err(AppImageIntegrationError::UnsafePath);
    }
    Ok(format!(
        "\"{}\"",
        value
            .replace('%', "%%")
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    ))
}

fn desktop_entry(path: &Path) -> Result<String, AppImageIntegrationError> {
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=Synveil\nIcon=synveil\nExec={}\nTerminal=false\nCategories=Utility;\n",
        quote_exec(path)?
    ))
}

fn user_unit(path: &Path) -> Result<String, AppImageIntegrationError> {
    Ok(format!(
        "[Unit]\nDescription=Synveil background client (AppImage)\n\n[Service]\nType=simple\nExecStart={} --synveil-client\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
        quote_systemd(path)?
    ))
}

fn quote_systemd(path: &Path) -> Result<String, AppImageIntegrationError> {
    let value = path.to_str().ok_or(AppImageIntegrationError::UnsafePath)?;
    if value.chars().any(|c| c.is_control()) {
        return Err(AppImageIntegrationError::UnsafePath);
    }
    Ok(format!(
        "\"{}\"",
        value
            .replace('%', "%%")
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    ))
}

fn sha256(path: &Path) -> Result<String, AppImageIntegrationError> {
    let mut hasher = Sha256::new();
    hasher.update(fs::read(path)?);
    Ok(format!("{:x}", hasher.finalize()))
}

fn atomic_owned_write(
    path: &Path,
    bytes: &[u8],
    mode: u32,
) -> Result<(), AppImageIntegrationError> {
    let parent = path.parent().ok_or(AppImageIntegrationError::UnsafePath)?;
    fs::create_dir_all(parent)?;
    reject_symlink_ancestors(parent)?;
    let meta = fs::symlink_metadata(parent)?;
    if !meta.is_dir() || meta.uid() != unsafe_uid() {
        return Err(AppImageIntegrationError::UnownedSurface);
    }
    if let Ok(meta) = fs::symlink_metadata(path)
        && (!meta.file_type().is_file() || meta.uid() != unsafe_uid())
    {
        return Err(AppImageIntegrationError::UnownedSurface);
    }
    let tmp = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name().unwrap().to_string_lossy(),
        std::process::id()
    ));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true).mode(mode);
    let mut file = options.open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)?;
    fs::File::open(parent)?.sync_all()?;
    if fs::read(path)? != bytes {
        return Err(AppImageIntegrationError::InvalidRecord);
    }
    Ok(())
}

fn reject_symlink_ancestors(path: &Path) -> Result<(), AppImageIntegrationError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if fs::symlink_metadata(&current)?.file_type().is_symlink() {
            return Err(AppImageIntegrationError::UnsafePath);
        }
    }
    Ok(())
}

fn remove_owned(path: &Path) -> Result<(), AppImageIntegrationError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() && meta.uid() == unsafe_uid() => {
            fs::remove_file(path).map_err(Into::into)
        }
        Ok(_) => Err(AppImageIntegrationError::UnownedSurface),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn unsafe_uid() -> u32 {
    nix::unistd::getuid().as_raw()
}

fn user_systemd_available() -> bool {
    env::var_os("XDG_RUNTIME_DIR")
        .map(|p| PathBuf::from(p).join("systemd/private").exists())
        .unwrap_or(false)
}
