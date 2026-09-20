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
    fs::OpenOptions,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use synveil_client_sync::{
    ClientSyncError, DesktopSyncHostConfig, DesktopSyncLibraryConfig, FilesystemLocalReplica,
    LocalStateStore, ServerProfileId, canonical_root_for_comparison, roots_overlap,
};
use synveil_core::LibraryId;
use synveil_platform::PlatformRuntime;

const MAX_CONFIG_BYTES: usize = 64 * 1024;
const MAX_SYNC_STATE_BYTES: usize = 32;

/// Default non-secret process manifest name below the platform config root.
pub const DEFAULT_DESKTOP_CLIENT_CONFIG_FILE: &str = "client.conf";

/// Default non-secret process-owned sync-control state file. It is separate
/// from the SQLite sync schema and from credentials; the file contains only
/// the words `running` or `paused`.
pub const DEFAULT_DESKTOP_CLIENT_SYNC_STATE_FILE: &str = "sync-state.conf";

/// Durable process-owned storage for the global user sync pause state.
///
/// This is intentionally a tiny non-secret file beside the existing client
/// manifest. It is scoped to the one profile-bound `synveil-client` process,
/// so it does not become canonical sync data or a per-library preference.
#[derive(Clone)]
pub struct DesktopSyncPauseStore {
    path: PathBuf,
}

impl fmt::Debug for DesktopSyncPauseStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopSyncPauseStore")
            .field("path", &"[REDACTED]")
            .finish()
    }
}

impl DesktopSyncPauseStore {
    pub fn for_platform(platform: &dyn PlatformRuntime) -> Result<Self, DesktopClientConfigError> {
        let manifest = config_path(platform)?;
        Self::from_manifest_path(manifest)
    }

    pub fn from_manifest_path(
        manifest: impl Into<PathBuf>,
    ) -> Result<Self, DesktopClientConfigError> {
        let manifest = manifest.into();
        if !manifest.is_absolute() {
            return Err(DesktopClientConfigError::ConfigPathNotAbsolute);
        }
        let parent = manifest
            .parent()
            .ok_or(DesktopClientConfigError::PlatformPaths)?;
        Ok(Self {
            path: parent.join(DEFAULT_DESKTOP_CLIENT_SYNC_STATE_FILE),
        })
    }

    #[must_use]
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn is_paused(&self) -> Result<bool, DesktopClientConfigError> {
        let file = match fs::File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(DesktopClientConfigError::SyncStateUnreadable),
        };
        let mut bytes = Vec::new();
        file.take((MAX_SYNC_STATE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| DesktopClientConfigError::SyncStateUnreadable)?;
        if bytes.len() > MAX_SYNC_STATE_BYTES {
            return Err(DesktopClientConfigError::SyncStateMalformed);
        }
        match bytes.as_slice() {
            b"paused" | b"paused\n" => Ok(true),
            b"running" | b"running\n" => Ok(false),
            _ => Err(DesktopClientConfigError::SyncStateMalformed),
        }
    }

    pub fn persist(&self, paused: bool) -> Result<(), DesktopClientConfigError> {
        let parent = self
            .path
            .parent()
            .ok_or(DesktopClientConfigError::ConfigurationWriteFailed)?;
        fs::create_dir_all(parent).map_err(|_| DesktopClientConfigError::SyncStateWriteFailed)?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let temporary = parent.join(format!(
            ".{}.tmp-{}-{nonce}",
            DEFAULT_DESKTOP_CLIENT_SYNC_STATE_FILE,
            std::process::id()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| DesktopClientConfigError::SyncStateWriteFailed)?;
        let value: &[u8] = if paused { b"paused\n" } else { b"running\n" };
        if file
            .write_all(value)
            .and_then(|()| file.sync_all())
            .is_err()
        {
            let _ = fs::remove_file(&temporary);
            return Err(DesktopClientConfigError::SyncStateWriteFailed);
        }
        drop(file);

        #[cfg(windows)]
        if self.path.exists() {
            fs::remove_file(&self.path)
                .map_err(|_| DesktopClientConfigError::SyncStateWriteFailed)?;
        }
        if fs::rename(&temporary, &self.path).is_err() {
            let _ = fs::remove_file(&temporary);
            return Err(DesktopClientConfigError::SyncStateWriteFailed);
        }

        #[cfg(unix)]
        if let Ok(directory) = fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    }
}

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
    pending_libraries: Vec<DesktopClientLibrary>,
    sync_paused: bool,
    host: DesktopSyncHostConfig,
    network_hint_interval: std::time::Duration,
}

impl fmt::Debug for DesktopClientConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopClientConfig")
            .field("profile_id", &self.profile_id)
            .field("libraries", &self.libraries)
            .field("pending_libraries", &self.pending_libraries)
            .field("sync_paused", &self.sync_paused)
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
            pending_libraries: Vec::new(),
            sync_paused: false,
            host: DesktopSyncHostConfig::default(),
            network_hint_interval: crate::DEFAULT_DESKTOP_NETWORK_HINT_INTERVAL,
        })
    }

    /// Load the canonical process manifest below the platform config root.
    /// `SYNVEIL_CLIENT_CONFIG` is an absolute-path, non-secret development/test
    /// override; it never carries a credential.
    pub fn from_platform(platform: &dyn PlatformRuntime) -> Result<Self, DesktopClientConfigError> {
        let path = ensure_profile_manifest(platform)?;
        let mut config = Self::from_path(&path)?;
        config.sync_paused = DesktopSyncPauseStore::from_manifest_path(path)?.is_paused()?;
        Ok(config)
    }

    /// Load only the non-secret profile identity for a controller-only
    /// embedding. Library/root values are treated as opaque and are never
    /// materialized into this result.
    pub fn configured_profile_id() -> Result<ServerProfileId, DesktopClientConfigError> {
        let platform: std::sync::Arc<dyn PlatformRuntime> =
            std::sync::Arc::from(synveil_platform::current());
        ensure_profile_id_from_platform(platform.as_ref())
    }

    /// Read only the profile identity from the canonical process manifest.
    /// This keeps platform path resolution and manifest I/O below the client
    /// application boundary rather than making a UI crate depend on them.
    pub fn profile_id_from_platform(
        platform: &dyn PlatformRuntime,
    ) -> Result<ServerProfileId, DesktopClientConfigError> {
        let bytes = fs::read(ensure_profile_manifest(platform)?).map_err(|error| {
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
            } else if !key.starts_with("library.") && !key.starts_with("pending.") {
                return Err(DesktopClientConfigError::UnknownKey);
            }
        }
        profile_id.ok_or(DesktopClientConfigError::MissingProfileId)
    }

    /// Parse a small non-secret manifest. Supported keys are:
    ///
    /// `profile_id=<canonical profile UUIDv7>`
    /// `library.<canonical library UUIDv7>=<absolute root path>`
    /// `pending.<canonical library UUIDv7>=<absolute root path>`
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
        let mut pending_libraries = BTreeMap::new();
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
            let (target, library_id_text) = if let Some(value) = key.strip_prefix("library.") {
                (&mut libraries, value)
            } else if let Some(value) = key.strip_prefix("pending.") {
                (&mut pending_libraries, value)
            } else {
                return Err(DesktopClientConfigError::UnknownKey);
            };
            let library_id = library_id_text
                .parse::<LibraryId>()
                .map_err(|_| DesktopClientConfigError::InvalidLibraryId)?;
            let root = PathBuf::from(value);
            validate_root_manifest(&root)?;
            if target.insert(library_id, root).is_some() {
                return Err(DesktopClientConfigError::DuplicateLibrary);
            }
        }
        let profile_id = profile_id.ok_or(DesktopClientConfigError::MissingProfileId)?;
        let libraries = libraries
            .into_iter()
            .map(|(library_id, root)| DesktopClientLibrary { library_id, root })
            .collect::<Vec<_>>();
        let pending_libraries = pending_libraries
            .into_iter()
            .map(|(library_id, root)| DesktopClientLibrary { library_id, root })
            .collect::<Vec<_>>();
        validate_libraries(&libraries)?;
        validate_libraries(&pending_libraries)?;
        Ok(Self {
            profile_id,
            libraries,
            pending_libraries,
            sync_paused: false,
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
    pub fn pending_libraries(&self) -> &[DesktopClientLibrary] {
        &self.pending_libraries
    }

    #[must_use]
    pub const fn sync_paused(&self) -> bool {
        self.sync_paused
    }

    pub fn sync_pause_store(
        platform: &dyn PlatformRuntime,
    ) -> Result<DesktopSyncPauseStore, DesktopClientConfigError> {
        DesktopSyncPauseStore::for_platform(platform)
    }

    /// Persist a pending root choice before making a network or filesystem
    /// side effect. The append is fsynced and the manifest remains bounded.
    pub fn append_pending_library_binding(
        platform: &dyn PlatformRuntime,
        profile_id: ServerProfileId,
        library_id: LibraryId,
        root: &Path,
    ) -> Result<(), DesktopClientConfigError> {
        append_library_binding(platform, profile_id, library_id, root, "pending")
    }

    /// Promote a pending root choice to an active process binding after the
    /// server and local SQLite/root marker have both committed.
    pub fn append_library_binding(
        platform: &dyn PlatformRuntime,
        profile_id: ServerProfileId,
        library_id: LibraryId,
        root: &Path,
    ) -> Result<(), DesktopClientConfigError> {
        append_library_binding(platform, profile_id, library_id, root, "library")
    }

    /// Find the pending UUID for an equivalent root without following raw
    /// string prefixes. This is the restart/retry identity for an interrupted
    /// onboarding operation.
    pub fn pending_library_for_root(
        platform: &dyn PlatformRuntime,
        profile_id: ServerProfileId,
        root: &Path,
    ) -> Result<Option<LibraryId>, DesktopClientConfigError> {
        let config = Self::from_platform(platform)?;
        if config.profile_id != profile_id {
            return Err(DesktopClientConfigError::ProfileMismatch);
        }
        let canonical = canonical_root_for_comparison(root)?;
        for pending in &config.pending_libraries {
            if canonical_root_for_comparison(pending.root())? == canonical {
                return Ok(Some(pending.library_id()));
            }
        }
        Ok(None)
    }

    /// Check all active and pending bindings for a component-aware overlap.
    pub fn root_overlaps_existing(
        platform: &dyn PlatformRuntime,
        profile_id: ServerProfileId,
        root: &Path,
    ) -> Result<bool, DesktopClientConfigError> {
        let config = Self::from_platform(platform)?;
        if config.profile_id != profile_id {
            return Err(DesktopClientConfigError::ProfileMismatch);
        }
        for configured in config
            .libraries
            .iter()
            .chain(config.pending_libraries.iter())
        {
            if roots_overlap(configured.root(), root)? {
                return Ok(true);
            }
        }
        Ok(false)
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
        if self.libraries.is_empty() {
            return Ok(Vec::new());
        }
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
    SyncStateUnreadable,
    SyncStateMalformed,
    SyncStateWriteFailed,
    ProfileMismatch,
    ConfigurationWriteFailed,
    LibraryBindingConflict,
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
            Self::SyncStateUnreadable => formatter.write_str("DESKTOP_SYNC_STATE_UNREADABLE"),
            Self::SyncStateMalformed => formatter.write_str("DESKTOP_SYNC_STATE_MALFORMED"),
            Self::SyncStateWriteFailed => formatter.write_str("DESKTOP_SYNC_STATE_WRITE_FAILED"),
            Self::ProfileMismatch => formatter.write_str("DESKTOP_CONFIG_PROFILE_MISMATCH"),
            Self::ConfigurationWriteFailed => formatter.write_str("DESKTOP_CONFIG_WRITE_FAILED"),
            Self::LibraryBindingConflict => {
                formatter.write_str("DESKTOP_CONFIG_LIBRARY_BINDING_CONFLICT")
            }
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
    if libraries.len() > 4_096 {
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

fn append_library_binding(
    platform: &dyn PlatformRuntime,
    profile_id: ServerProfileId,
    library_id: LibraryId,
    root: &Path,
    kind: &str,
) -> Result<(), DesktopClientConfigError> {
    if kind != "pending" && kind != "library" {
        return Err(DesktopClientConfigError::ConfigurationWriteFailed);
    }
    validate_root_manifest(root)?;
    let path = ensure_profile_manifest(platform)?;
    let bytes = fs::read(&path).map_err(|_| DesktopClientConfigError::ConfigurationUnreadable)?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(DesktopClientConfigError::ConfigurationTooLarge);
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| DesktopClientConfigError::ConfigurationMalformed)?;
    let config = DesktopClientConfig::parse(text)?;
    if config.profile_id != profile_id {
        return Err(DesktopClientConfigError::ProfileMismatch);
    }
    let active = config
        .libraries
        .iter()
        .find(|library| library.library_id() == library_id);
    let pending = config
        .pending_libraries
        .iter()
        .find(|library| library.library_id() == library_id);
    let equivalent_root = |configured: &DesktopClientLibrary| {
        canonical_root_for_comparison(configured.root())
            .ok()
            .zip(canonical_root_for_comparison(root).ok())
            .is_some_and(|(configured, requested)| configured == requested)
    };
    if active.is_some_and(|library| !equivalent_root(library))
        || pending.is_some_and(|library| !equivalent_root(library))
    {
        return Err(DesktopClientConfigError::LibraryBindingConflict);
    }
    if (kind == "library" && active.is_some()) || (kind == "pending" && pending.is_some()) {
        return Ok(());
    }
    let root = root
        .to_str()
        .ok_or(DesktopClientConfigError::InvalidRootPath)?;
    let line = format!("{kind}.{library_id}={root}\n");
    if bytes.len().saturating_add(line.len()).saturating_add(1) > MAX_CONFIG_BYTES {
        return Err(DesktopClientConfigError::ConfigurationTooLarge);
    }
    let mut file = OpenOptions::new()
        .append(true)
        .open(&path)
        .map_err(|_| DesktopClientConfigError::ConfigurationWriteFailed)?;
    if !bytes.ends_with(b"\n") {
        file.write_all(b"\n")
            .map_err(|_| DesktopClientConfigError::ConfigurationWriteFailed)?;
    }
    file.write_all(line.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|_| DesktopClientConfigError::ConfigurationWriteFailed)
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

/// Return the canonical manifest path, creating only the process-owned
/// non-secret profile identity on first run. The create-new file operation is
/// the single-writer race boundary; another concurrent starter simply reads
/// the identity it won.
fn ensure_profile_manifest(
    platform: &dyn PlatformRuntime,
) -> Result<PathBuf, DesktopClientConfigError> {
    let path = config_path(platform)?;
    match fs::read(&path) {
        Ok(bytes) => {
            if bytes.len() > MAX_CONFIG_BYTES {
                return Err(DesktopClientConfigError::ConfigurationTooLarge);
            }
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| DesktopClientConfigError::ConfigurationMalformed)?;
            DesktopClientConfig::parse_profile_id(text)?;
            return Ok(path);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(DesktopClientConfigError::ConfigurationUnreadable),
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| DesktopClientConfigError::ConfigurationUnreadable)?;
    }
    let profile_id = ServerProfileId::new();
    let contents = format!("# non-secret desktop profile identity\nprofile_id={profile_id}\n");
    let temp_path = path.with_extension(format!("client.conf.{profile_id}.tmp"));
    let create_result = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path);
    match create_result {
        Ok(mut file) => {
            file.write_all(contents.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|_| DesktopClientConfigError::ConfigurationUnreadable)?;
            drop(file);
            match fs::hard_link(&temp_path, &path) {
                Ok(()) => {
                    let _ = fs::remove_file(&temp_path);
                    Ok(path)
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let _ = fs::remove_file(&temp_path);
                    Ok(path)
                }
                Err(_) => {
                    let _ = fs::remove_file(&temp_path);
                    Err(DesktopClientConfigError::ConfigurationUnreadable)
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(path),
        Err(_) => Err(DesktopClientConfigError::ConfigurationUnreadable),
    }
}

fn ensure_profile_id_from_platform(
    platform: &dyn PlatformRuntime,
) -> Result<ServerProfileId, DesktopClientConfigError> {
    let path = ensure_profile_manifest(platform)?;
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
    DesktopClientConfig::parse_profile_id(text)
}

fn validate_root_manifest(root: &Path) -> Result<(), DesktopClientConfigError> {
    if root.as_os_str().to_string_lossy().len() > 16 * 1024
        || root
            .as_os_str()
            .to_string_lossy()
            .chars()
            .any(char::is_control)
        || !root.is_absolute()
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
    use std::{fs, path::PathBuf, time::Duration};

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
    fn profile_only_manifest_is_a_valid_zero_library_process_configuration() {
        let profile_id = ServerProfileId::new();
        let config = DesktopClientConfig::parse(&format!(
            "# first-run non-secret manifest\nprofile_id={profile_id}\n"
        ))
        .expect("profile-only configuration");
        assert_eq!(config.profile_id(), profile_id);
        assert!(config.libraries().is_empty());
        assert!(config.pending_libraries().is_empty());
    }

    #[test]
    fn pending_library_bindings_are_non_secret_and_bounded() {
        let profile_id = ServerProfileId::new();
        let library_id = LibraryId::new();
        let config = DesktopClientConfig::parse(&format!(
            "profile_id={profile_id}\npending.{library_id}=/tmp/synveil-pending-root\n"
        ))
        .expect("pending manifest");
        assert!(config.libraries().is_empty());
        assert_eq!(config.pending_libraries().len(), 1);
        assert_eq!(config.pending_libraries()[0].library_id(), library_id);
        assert!(!format!("{config:?}").contains("synveil-pending-root"));
    }

    #[test]
    fn manifest_rejects_control_bearing_root_values() {
        let profile_id = ServerProfileId::new();
        let library_id = LibraryId::new();
        assert!(matches!(
            DesktopClientConfig::parse(&format!(
                "profile_id={profile_id}\nlibrary.{library_id}=/tmp/bad\nroot"
            )),
            Err(DesktopClientConfigError::MalformedLine(_))
        ));
        assert!(matches!(
            DesktopClientLibrary::new(library_id, "/tmp/bad\nroot"),
            Err(DesktopClientConfigError::InvalidRootPath)
        ));
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

    #[test]
    fn sync_pause_store_persists_only_the_bounded_state_and_reopens() {
        let directory = std::env::temp_dir().join(format!(
            "synveil-sync-state-test-{}-{}",
            std::process::id(),
            ServerProfileId::new()
        ));
        fs::create_dir_all(&directory).expect("test state directory");
        let store = DesktopSyncPauseStore::from_path(directory.join("sync-state.conf"));
        assert!(
            !store
                .is_paused()
                .expect("missing state defaults to running")
        );
        store.persist(true).expect("pause state persists");
        assert!(store.is_paused().expect("paused state reads"));
        store.persist(false).expect("resume state persists");
        assert!(!store.is_paused().expect("running state reads"));
        assert_eq!(
            fs::read_to_string(directory.join("sync-state.conf")).expect("state file"),
            "running\n"
        );
        fs::remove_dir_all(directory).expect("test state cleanup");
    }

    #[test]
    fn sync_pause_store_rejects_unknown_state_without_changing_runtime_contract() {
        let directory = std::env::temp_dir().join(format!(
            "synveil-sync-state-malformed-{}-{}",
            std::process::id(),
            ServerProfileId::new()
        ));
        fs::create_dir_all(&directory).expect("test state directory");
        let path = directory.join("sync-state.conf");
        fs::write(&path, "maybe\n").expect("malformed state");
        let store = DesktopSyncPauseStore::from_path(path);
        assert!(matches!(
            store.is_paused(),
            Err(DesktopClientConfigError::SyncStateMalformed)
        ));
        fs::remove_dir_all(directory).expect("test state cleanup");
    }
}
