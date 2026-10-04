use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};

use serde::{Deserialize, Serialize};
use synveil_server_config::{ServerConfig, StorageId};

pub(super) const MARKER_NAME: &str = ".synveil-server-storage.json";
const MARKER_SCHEMA_VERSION: u32 = 1;
const MARKER_KIND: &str = "synveil-server-object-storage";
const OBJECT_LAYOUT_VERSION: u32 = 1;
const MAX_MARKER_BYTES: usize = 4096;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerStorageIdentity {
    schema_version: u32,
    kind: String,
    server_installation_id: String,
    storage_id: StorageId,
    backend: LocalStorageBackend,
    object_layout_version: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LocalStorageBackend {
    LocalFilesystem,
}

impl ServerStorageIdentity {
    #[must_use]
    pub fn server_installation_id(&self) -> &str {
        &self.server_installation_id
    }

    #[must_use]
    pub const fn storage_id(&self) -> &StorageId {
        &self.storage_id
    }

    fn for_config(config: &ServerConfig, storage_id: StorageId) -> Self {
        Self {
            schema_version: MARKER_SCHEMA_VERSION,
            kind: MARKER_KIND.to_owned(),
            server_installation_id: config.server_installation_id.clone(),
            storage_id,
            backend: LocalStorageBackend::LocalFilesystem,
            object_layout_version: OBJECT_LAYOUT_VERSION,
        }
    }

    fn canonical_bytes(&self) -> Result<Vec<u8>, MarkerError> {
        let mut bytes = serde_json::to_vec_pretty(self).map_err(|_| MarkerError::Malformed)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_MARKER_BYTES {
            return Err(MarkerError::Malformed);
        }
        Ok(bytes)
    }

    fn validate(&self) -> Result<(), MarkerError> {
        if self.schema_version != MARKER_SCHEMA_VERSION
            || self.kind != MARKER_KIND
            || self.object_layout_version != OBJECT_LAYOUT_VERSION
            || uuid::Uuid::parse_str(&self.server_installation_id)
                .ok()
                .is_none_or(|id| {
                    id.to_string() != self.server_installation_id || id.get_version_num() != 7
                })
            || StorageId::parse(self.storage_id.as_str().to_owned()).is_err()
        {
            return Err(MarkerError::Malformed);
        }
        Ok(())
    }

    fn matches(&self, config: &ServerConfig, storage_id: &StorageId) -> bool {
        self.server_installation_id == config.server_installation_id
            && &self.storage_id == storage_id
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MarkerError {
    Absent,
    Foreign,
    Malformed,
    SymlinkOrRedirected,
    WrongType,
    Io,
}

pub(super) fn marker_path(root: &Path) -> std::path::PathBuf {
    root.join(MARKER_NAME)
}

pub(super) fn read_identity(root: &Path) -> Result<ServerStorageIdentity, MarkerError> {
    let path = marker_path(root);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(MarkerError::Absent);
        }
        Err(_) => return Err(MarkerError::Io),
    };
    if metadata.file_type().is_symlink() {
        return Err(MarkerError::SymlinkOrRedirected);
    }
    if !metadata.is_file() {
        return Err(MarkerError::WrongType);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(MarkerError::Malformed);
        }
    }
    if metadata.len() > MAX_MARKER_BYTES as u64 {
        return Err(MarkerError::Malformed);
    }
    let mut options = OpenOptions::new();
    options.read(true);
    set_no_follow(&mut options);
    let file = options
        .open(&path)
        .map_err(|_| MarkerError::SymlinkOrRedirected)?;
    let opened = file.metadata().map_err(|_| MarkerError::Io)?;
    if !opened.is_file() || opened.len() > MAX_MARKER_BYTES as u64 || !same_file(&metadata, &opened)
    {
        return Err(MarkerError::WrongType);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened.nlink() != 1 {
            return Err(MarkerError::Malformed);
        }
    }
    let mut bytes = Vec::with_capacity(opened.len() as usize);
    file.take(MAX_MARKER_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| MarkerError::Io)?;
    if bytes.len() > MAX_MARKER_BYTES {
        return Err(MarkerError::Malformed);
    }
    let identity: ServerStorageIdentity =
        serde_json::from_slice(&bytes).map_err(|_| MarkerError::Malformed)?;
    identity.validate()?;
    if identity.canonical_bytes()? != bytes {
        return Err(MarkerError::Malformed);
    }
    Ok(identity)
}

pub(super) fn verify_identity(
    root: &Path,
    config: &ServerConfig,
    storage_id: &StorageId,
) -> Result<(), MarkerError> {
    match read_identity(root)? {
        identity if identity.matches(config, storage_id) => Ok(()),
        _ => Err(MarkerError::Foreign),
    }
}

pub(super) fn initialize_identity(
    root: &Path,
    config: &ServerConfig,
    storage_id: StorageId,
) -> Result<(), MarkerError> {
    let identity = ServerStorageIdentity::for_config(config, storage_id);
    identity.validate()?;
    let expected = identity.canonical_bytes()?;
    let path = marker_path(root);
    match read_identity(root) {
        Ok(existing) if existing == identity => return Ok(()),
        Ok(_) => return Err(MarkerError::Foreign),
        Err(MarkerError::Absent) => {}
        Err(error) => return Err(error),
    }

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    set_create_mode(&mut options, 0o644);
    let mut file = options.open(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            MarkerError::Foreign
        } else {
            MarkerError::Io
        }
    })?;
    file.write_all(&expected).map_err(|_| MarkerError::Io)?;
    file.sync_all().map_err(|_| MarkerError::Io)?;
    sync_directory(root).map_err(|_| MarkerError::Io)?;
    verify_identity(root, config, &identity.storage_id)
}

fn set_no_follow(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(not(unix))]
    let _ = options;
}

fn set_create_mode(options: &mut OpenOptions, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(mode)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(not(unix))]
    let _ = (options, mode);
}

fn sync_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
        options.open(path)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        std::fs::File::open(path)?.sync_all()
    }
}

#[cfg(unix)]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.len() == right.len()
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use synveil_server_config::{
        DeploymentProfile, ExistingServerEvidence, InitializeResult, LinuxConfigLayout,
        NewServerConfig, ServerConfigStore,
    };

    fn managed_config(temp: &tempfile::TempDir) -> ServerConfig {
        let layout = LinuxConfigLayout::at_root(temp.path().join("etc/synveil")).unwrap();
        let store = ServerConfigStore::new(layout);
        match store
            .initialize_managed(
                NewServerConfig {
                    deployment_profile: DeploymentProfile::PersonalHomeManaged,
                },
                ExistingServerEvidence::NoKnownServerState,
            )
            .unwrap()
        {
            InitializeResult::Created(config) | InitializeResult::Reused(config) => config,
        }
    }

    #[test]
    fn identity_marker_is_small_deterministic_and_non_secret() {
        let temp = tempfile::tempdir().unwrap();
        let config = managed_config(&temp);
        let root = temp.path().join("storage");
        fs::create_dir(&root).unwrap();
        let storage_id = StorageId::new_v7();
        let identity = ServerStorageIdentity::for_config(&config, storage_id);
        let bytes = identity.canonical_bytes().unwrap();
        assert!(bytes.len() <= MAX_MARKER_BYTES);
        assert_eq!(identity.canonical_bytes().unwrap(), bytes);
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.to_ascii_lowercase().contains("password"));
        assert!(!text.to_ascii_lowercase().contains("credential"));
        assert!(!text.to_ascii_lowercase().contains("token"));
    }

    #[test]
    fn unknown_marker_fields_and_versions_fail_closed_without_rewrite() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("storage");
        fs::create_dir(&root).unwrap();
        let marker = marker_path(&root);
        let unknown = br#"{"schema_version":2,"kind":"synveil-server-object-storage","server_installation_id":"018f2ed0-44c2-7c00-8000-000000000001","storage_id":"018f2ed0-44c2-7c00-8000-000000000002","backend":"local_filesystem","object_layout_version":1,"future_field":true}"#;
        fs::write(&marker, unknown).unwrap();
        assert_eq!(read_identity(&root), Err(MarkerError::Malformed));
        assert_eq!(fs::read(marker).unwrap(), unknown);
    }

    #[cfg(unix)]
    #[test]
    fn identity_marker_symlink_is_not_followed() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("storage");
        fs::create_dir(&root).unwrap();
        let target = temp.path().join("target.json");
        fs::write(&target, b"{}").unwrap();
        symlink(&target, marker_path(&root)).unwrap();
        assert_eq!(read_identity(&root), Err(MarkerError::SymlinkOrRedirected));
    }
}
