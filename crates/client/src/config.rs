//! Non-secret desktop process configuration.
//!
//! The process configuration deliberately contains only profile and library
//! references. Profile/device credentials stay in the existing
//! `synveil-platform::SecretStore`, while durable scope and root bindings stay
//! in the existing client-sync SQLite state. The small line-oriented file is
//! an input manifest, not a second synchronization database.

use std::{
    collections::BTreeMap,
    env, fmt, fs,
    path::{Component, Path, PathBuf},
};

use synveil_client_sync::{
    ClientSyncError, DesktopSyncHostConfig, DesktopSyncLibraryConfig, FilesystemLocalReplica,
    LocalStateStore, ServerProfileId,
};
use synveil_core::LibraryId;
use synveil_platform::PlatformRuntime;

const MAX_CONFIG_BYTES: usize = 64 * 1024;

/// Default non-secret process manifest name below the platform config root.
pub const DEFAULT_DESKTOP_CLIENT_CONFIG_FILE: &str = "client.conf";

/// A configured library/root pair. The root is never logged by the process
/// status surface; it is used only to reopen the existing durable binding.
#[derive(Clone, Eq, PartialEq)]
pub struct DesktopClientLibrary {
    library_id: LibraryId,
    root: PathBuf,
}

impl fmt::Debug for DesktopClientLibrary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopClientLibrary")
            .field("library_id", &self.library_id)
            .field("root", &"[REDACTED]")
            .finish()
    }
}

impl DesktopClientLibrary {
    /// Construct a non-secret library manifest entry.
    pub fn new(
        library_id: LibraryId,
        root: impl Into<PathBuf>,
    ) -> Result<Self, DesktopClientConfigError> {
        let root = root.into();
        validate_root_manifest(&root)?;
        Ok(Self { library_id, root })
    }

    #[must_use]
    pub const fn library_id(&self) -> LibraryId {
        self.library_id
    }

    /// The configured path is available to the composition boundary, but is
    /// intentionally absent from process status and diagnostic formatting.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Validated non-secret inputs to one production desktop process.
pub struct DesktopClientConfig {
    profile_id: ServerProfileId,
    libraries: Vec<DesktopClientLibrary>,
    host: DesktopSyncHostConfig,
    network_hint_interval: std::time::Duration,
}

impl fmt::Debug for DesktopClientConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopClientConfig")
            .field("profile_id", &self.profile_id)
            .field("libraries", &self.libraries)
            .field("host", &self.host)
            .field("network_hint_interval", &self.network_hint_interval)
            .finish()
    }
}

impl DesktopClientConfig {
    /// Build process configuration for an embedding application or test.
    pub fn new(
        profile_id: ServerProfileId,
        libraries: impl IntoIterator<Item = DesktopClientLibrary>,
    ) -> Result<Self, DesktopClientConfigError> {
        let libraries = libraries.into_iter().collect::<Vec<_>>();
        validate_libraries(&libraries)?;
        Ok(Self {
            profile_id,
            libraries,
            host: DesktopSyncHostConfig::default(),
            network_hint_interval: crate::DEFAULT_DESKTOP_NETWORK_HINT_INTERVAL,
        })
    }

    /// Load the canonical process manifest below the platform config root.
    /// `SYNVEIL_CLIENT_CONFIG` is an absolute-path, non-secret development/test
    /// override; it never carries a credential.
    pub fn from_platform(platform: &dyn PlatformRuntime) -> Result<Self, DesktopClientConfigError> {
        let path = config_path(platform)?;
        Self::from_path(path)
    }

    /// Load only the non-secret profile identity for a controller-only
    /// embedding. Library/root values are treated as opaque and are never
    /// materialized into this result.
    pub fn configured_profile_id() -> Result<ServerProfileId, DesktopClientConfigError> {
        let platform: std::sync::Arc<dyn PlatformRuntime> =
            std::sync::Arc::from(synveil_platform::current());
        Self::profile_id_from_platform(platform.as_ref())
    }

    /// Read only the profile identity from the canonical process manifest.
    /// This keeps platform path resolution and manifest I/O below the client
    /// application boundary rather than making a UI crate depend on them.
    pub fn profile_id_from_platform(
        platform: &dyn PlatformRuntime,
    ) -> Result<ServerProfileId, DesktopClientConfigError> {
        let bytes = fs::read(config_path(platform)?).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DesktopClientConfigError::MissingConfiguration
            } else {
                DesktopClientConfigError::ConfigurationUnreadable
            }
        })?;
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(DesktopClientConfigError::ConfigurationTooLarge);
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| DesktopClientConfigError::ConfigurationMalformed)?;
        Self::parse_profile_id(text)
    }

    /// Parse only the profile identity from a non-secret manifest. Known
    /// `library.*` entries are deliberately skipped as opaque values.
    pub fn parse_profile_id(text: &str) -> Result<ServerProfileId, DesktopClientConfigError> {
        if text.len() > MAX_CONFIG_BYTES {
            return Err(DesktopClientConfigError::ConfigurationTooLarge);
        }
        let mut profile_id = None;
        for (line_number, raw_line) in text.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(DesktopClientConfigError::MalformedLine(line_number + 1));
            };
            let key = key.trim();
            let value = value.trim();
            if value.is_empty() {
                return Err(DesktopClientConfigError::MalformedLine(line_number + 1));
            }
            if key == "profile_id" {
                if profile_id.is_some() {
                    return Err(DesktopClientConfigError::DuplicateProfile);
                }
                profile_id = Some(
                    ServerProfileId::parse_str(value)
                        .map_err(|_| DesktopClientConfigError::InvalidProfileId)?,
                );
            } else if !key.starts_with("library.") {
                return Err(DesktopClientConfigError::UnknownKey);
            }
        }
        profile_id.ok_or(DesktopClientConfigError::MissingProfileId)
    }

    /// Parse a small non-secret manifest. Supported keys are:
    ///
    /// `profile_id=<canonical profile UUIDv7>`
    /// `library.<canonical library UUIDv7>=<absolute root path>`
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, DesktopClientConfigError> {
        let bytes = fs::read(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DesktopClientConfigError::MissingConfiguration
            } else {
                DesktopClientConfigError::ConfigurationUnreadable
            }
        })?;
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(DesktopClientConfigError::ConfigurationTooLarge);
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| DesktopClientConfigError::ConfigurationMalformed)?;
        Self::parse(text)
    }

    /// Parse without touching the filesystem. This is the testable boundary
    /// used by the production file loader.
    pub fn parse(text: &str) -> Result<Self, DesktopClientConfigError> {
        if text.len() > MAX_CONFIG_BYTES {
            return Err(DesktopClientConfigError::ConfigurationTooLarge);
        }
        let mut profile_id = None;
        let mut libraries = BTreeMap::new();
        for (line_number, raw_line) in text.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(DesktopClientConfigError::MalformedLine(line_number + 1));
            };
            let key = key.trim();
            let value = value.trim();
            if value.is_empty() {
                return Err(DesktopClientConfigError::MalformedLine(line_number + 1));
            }
            if key == "profile_id" {
                if profile_id.is_some() {
                    return Err(DesktopClientConfigError::DuplicateProfile);
                }
                profile_id = Some(
                    ServerProfileId::parse_str(value)
                        .map_err(|_| DesktopClientConfigError::InvalidProfileId)?,
                );
                continue;
            }
            let Some(library_id_text) = key.strip_prefix("library.") else {
                return Err(DesktopClientConfigError::UnknownKey);
            };
            let library_id = library_id_text
                .parse::<LibraryId>()
                .map_err(|_| DesktopClientConfigError::InvalidLibraryId)?;
            let root = PathBuf::from(value);
            validate_root_manifest(&root)?;
            if libraries.insert(library_id, root).is_some() {
                return Err(DesktopClientConfigError::DuplicateLibrary);
            }
        }
        let profile_id = profile_id.ok_or(DesktopClientConfigError::MissingProfileId)?;
        let libraries = libraries
            .into_iter()
            .map(|(library_id, root)| DesktopClientLibrary { library_id, root })
            .collect::<Vec<_>>();
        validate_libraries(&libraries)?;
        Ok(Self {
            profile_id,
            libraries,
            host: DesktopSyncHostConfig::default(),
            network_hint_interval: crate::DEFAULT_DESKTOP_NETWORK_HINT_INTERVAL,
        })
    }

    #[must_use]
    pub const fn profile_id(&self) -> ServerProfileId {
        self.profile_id
    }

    #[must_use]
    pub fn libraries(&self) -> &[DesktopClientLibrary] {
        &self.libraries
    }

    #[must_use]
    pub const fn host(&self) -> DesktopSyncHostConfig {
        self.host
    }

    #[must_use]
    pub const fn network_hint_interval(&self) -> std::time::Duration {
        self.network_hint_interval
    }

    #[must_use]
    pub const fn with_host(mut self, host: DesktopSyncHostConfig) -> Self {
        self.host = host;
        self
    }

    pub fn with_network_hint_interval(
        mut self,
        interval: std::time::Duration,
    ) -> Result<Self, DesktopClientConfigError> {
        crate::network::validate_network_hint_interval(interval)?;
        self.network_hint_interval = interval;
        Ok(self)
    }

    /// Reopen configured roots against the durable profile/replica records and
    /// produce the host's canonical HTTP library declarations. A missing root
    /// uses the existing durable binding ID in a deferred replica; it never
    /// creates a directory or binds a replacement marker.
    pub(crate) async fn materialize_libraries(
        &self,
        state: &LocalStateStore,
    ) -> Result<Vec<DesktopSyncLibraryConfig>, DesktopClientConfigError> {
        let profile = state
            .server_profile(self.profile_id)
            .await?
            .ok_or(DesktopClientConfigError::ProfileNotFound)?;
        let enrollment = state.profile_enrollment(self.profile_id).await?;
        let mut process_scope = None;
        let mut result = Vec::with_capacity(self.libraries.len());
        for configured in &self.libraries {
            let record = state
                .replica(configured.library_id())
                .await?
                .ok_or(DesktopClientConfigError::LibraryNotConfigured)?;
            if record.server_profile_id() != Some(self.profile_id) {
                return Err(DesktopClientConfigError::WrongProfileBinding);
            }
            let scope = record.scope();
            if let Some(enrollment) = enrollment
                && (enrollment.owner_user_id() != scope.owner_user_id()
                    || enrollment.device_id() != scope.device_id())
            {
                return Err(DesktopClientConfigError::MixedProcessScope);
            }
            if process_scope.replace(scope).is_some_and(|previous| {
                previous.owner_user_id() != scope.owner_user_id()
                    || previous.device_id() != scope.device_id()
            }) {
                return Err(DesktopClientConfigError::MixedProcessScope);
            }

            let replica = match FilesystemLocalReplica::open_for_profile(
                configured.root(),
                scope,
                self.profile_id,
            ) {
                Ok(replica) => replica,
                Err(_error) if root_is_temporarily_unavailable(configured.root()) => {
                    FilesystemLocalReplica::open_deferred_for_profile(
                        configured.root(),
                        scope,
                        self.profile_id,
                        record.root_binding_id(),
                    )?
                }
                Err(error) => return Err(error.into()),
            };
            result.push(
                DesktopSyncLibraryConfig::http(
                    scope,
                    profile.clone(),
                    std::sync::Arc::new(replica),
                )
                .with_native_watcher(),
            );
        }
        Ok(result)
    }
}

/// Typed, non-secret configuration failures. Line numbers are safe to expose;
/// line contents and paths are intentionally omitted.
#[derive(Debug)]
pub enum DesktopClientConfigError {
    MissingConfiguration,
    ConfigurationUnreadable,
    ConfigurationTooLarge,
    ConfigurationMalformed,
    MalformedLine(usize),
    UnknownKey,
    DuplicateProfile,
    DuplicateLibrary,
    MissingProfileId,
    InvalidProfileId,
    InvalidLibraryId,
    InvalidRootPath,
    ConfigPathNotAbsolute,
    PlatformPaths,
    ProfileNotFound,
    LibraryNotConfigured,
    WrongProfileBinding,
    MixedProcessScope,
    InvalidNetworkHintInterval,
    Client(ClientSyncError),
}

impl fmt::Display for DesktopClientConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingConfiguration => formatter.write_str("DESKTOP_CONFIG_MISSING"),
            Self::ConfigurationUnreadable => formatter.write_str("DESKTOP_CONFIG_UNREADABLE"),
            Self::ConfigurationTooLarge => formatter.write_str("DESKTOP_CONFIG_TOO_LARGE"),
            Self::ConfigurationMalformed => formatter.write_str("DESKTOP_CONFIG_MALFORMED"),
            Self::MalformedLine(line) => write!(formatter, "DESKTOP_CONFIG_MALFORMED_LINE_{line}"),
            Self::UnknownKey => formatter.write_str("DESKTOP_CONFIG_UNKNOWN_KEY"),
            Self::DuplicateProfile => formatter.write_str("DESKTOP_CONFIG_DUPLICATE_PROFILE"),
            Self::DuplicateLibrary => formatter.write_str("DESKTOP_CONFIG_DUPLICATE_LIBRARY"),
            Self::MissingProfileId => formatter.write_str("DESKTOP_CONFIG_PROFILE_REQUIRED"),
            Self::InvalidProfileId => formatter.write_str("DESKTOP_CONFIG_PROFILE_INVALID"),
            Self::InvalidLibraryId => formatter.write_str("DESKTOP_CONFIG_LIBRARY_INVALID"),
            Self::InvalidRootPath => formatter.write_str("DESKTOP_CONFIG_ROOT_INVALID"),
            Self::ConfigPathNotAbsolute => formatter.write_str("DESKTOP_CONFIG_PATH_INVALID"),
            Self::PlatformPaths => formatter.write_str("DESKTOP_CONFIG_PLATFORM_PATHS_INVALID"),
            Self::ProfileNotFound => formatter.write_str("DESKTOP_CONFIG_PROFILE_NOT_FOUND"),
            Self::LibraryNotConfigured => {
                formatter.write_str("DESKTOP_CONFIG_LIBRARY_NOT_CONFIGURED")
            }
            Self::WrongProfileBinding => {
                formatter.write_str("DESKTOP_CONFIG_PROFILE_BINDING_MISMATCH")
            }
            Self::MixedProcessScope => formatter.write_str("DESKTOP_CONFIG_SCOPE_MIXED"),
            Self::InvalidNetworkHintInterval => {
                formatter.write_str("DESKTOP_CONFIG_NETWORK_INTERVAL_INVALID")
            }
            Self::Client(error) => formatter.write_str(error.code()),
        }
    }
}

impl std::error::Error for DesktopClientConfigError {}

impl From<ClientSyncError> for DesktopClientConfigError {
    fn from(error: ClientSyncError) -> Self {
        Self::Client(error)
    }
}

fn validate_libraries(libraries: &[DesktopClientLibrary]) -> Result<(), DesktopClientConfigError> {
    if libraries.is_empty() || libraries.len() > 4_096 {
        return Err(DesktopClientConfigError::LibraryNotConfigured);
    }
    let mut ids = std::collections::BTreeSet::new();
    for library in libraries {
        if !ids.insert(library.library_id()) {
            return Err(DesktopClientConfigError::DuplicateLibrary);
        }
    }
    Ok(())
}

fn config_path(platform: &dyn PlatformRuntime) -> Result<PathBuf, DesktopClientConfigError> {
    env::var_os("SYNVEIL_CLIENT_CONFIG")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map_or_else(
            || {
                let paths = platform
                    .resolve_paths()
                    .map_err(|_| DesktopClientConfigError::PlatformPaths)?;
                Ok(paths
                    .config_dir()
                    .as_path()
                    .join(DEFAULT_DESKTOP_CLIENT_CONFIG_FILE))
            },
            |path| {
                if !path.is_absolute() {
                    Err(DesktopClientConfigError::ConfigPathNotAbsolute)
                } else {
                    Ok(path)
                }
            },
        )
}

fn validate_root_manifest(root: &Path) -> Result<(), DesktopClientConfigError> {
    if !root.is_absolute()
        || root
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
        || root.file_name().is_none()
    {
        return Err(DesktopClientConfigError::InvalidRootPath);
    }
    Ok(())
}

fn root_is_temporarily_unavailable(root: &Path) -> bool {
    fs::symlink_metadata(root).is_err()
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use synveil_client_sync::ServerProfileId;
    use synveil_core::LibraryId;

    use super::*;

    #[test]
    fn parses_only_bounded_non_secret_profile_and_library_references() {
        let profile_id = ServerProfileId::new();
        let library_id = LibraryId::new();
        let root = "/tmp/synveil-config-test-root";
        let config = DesktopClientConfig::parse(&format!(
            "# non-secret manifest\nprofile_id={profile_id}\nlibrary.{library_id}={root}\n"
        ))
        .expect("manifest");
        assert_eq!(config.profile_id(), profile_id);
        assert_eq!(config.libraries().len(), 1);
        assert_eq!(config.libraries()[0].library_id(), library_id);
        assert_eq!(config.libraries()[0].root(), PathBuf::from(root).as_path());
        assert!(!format!("{config:?}").contains(root));
    }

    #[test]
    fn rejects_relative_roots_duplicates_and_credentials_as_unknown_keys() {
        let profile_id = ServerProfileId::new();
        let library_id = LibraryId::new();
        assert!(matches!(
            DesktopClientConfig::parse(&format!(
                "profile_id={profile_id}\nlibrary.{library_id}=relative/root"
            )),
            Err(DesktopClientConfigError::InvalidRootPath)
        ));
        assert!(matches!(
            DesktopClientConfig::parse(&format!(
                "profile_id={profile_id}\nprofile_id={profile_id}\nlibrary.{library_id}=/tmp/root"
            )),
            Err(DesktopClientConfigError::DuplicateProfile)
        ));
        assert!(matches!(
            DesktopClientConfig::parse(&format!(
                "profile_id={profile_id}\nlibrary.{library_id}=/tmp/root\ncredential=secret"
            )),
            Err(DesktopClientConfigError::UnknownKey)
        ));
    }

    #[test]
    fn profile_only_parser_does_not_materialize_library_roots() {
        let profile_id = ServerProfileId::new();
        let parsed = DesktopClientConfig::parse_profile_id(&format!(
            "profile_id={profile_id}\nlibrary.opaque=/private/root"
        ))
        .expect("profile identity");
        assert_eq!(parsed, profile_id);
    }

    #[test]
    fn network_hint_interval_is_bounded() {
        let profile_id = ServerProfileId::new();
        let library = DesktopClientLibrary::new(LibraryId::new(), "/tmp/synveil-root")
            .expect("library manifest");
        let config = DesktopClientConfig::new(profile_id, [library]).expect("config");
        assert!(matches!(
            config.with_network_hint_interval(Duration::ZERO),
            Err(DesktopClientConfigError::InvalidNetworkHintInterval)
        ));
    }
}
