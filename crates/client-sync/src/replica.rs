use std::{
    collections::VecDeque,
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use async_trait::async_trait;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use synveil_core::{NodeId, Sha256Digest};
use uuid::Uuid;

use crate::{
    ClientSyncError, ContentByteStream, MAX_CONTENT_CHUNK_BYTES, MAX_DOWNLOAD_BYTES,
    ManagedRelativePath, ReplicaScope, ServerProfileId, local_collision_key,
};

const CONTROL_DIRECTORY: &str = ".synveil";
const MARKER_FILE: &str = "root-id";
const STAGING_DIRECTORY: &str = "staging";
const QUARANTINE_DIRECTORY: &str = "quarantine";
const MARKER_VERSION: &str = "SYNVEIL_MANAGED_ROOT_V1";
const PROFILED_MARKER_VERSION: &str = "SYNVEIL_MANAGED_ROOT_V2";
const OPERATION_RECEIPT_VERSION: &str = "SYNVEIL_OPERATION_APPLIED_V1";
const COPY_BUFFER_BYTES: usize = 64 * 1024;
const MAX_LOCAL_SCAN_ENTRIES: usize = 100_000;

/// Random local identity shared by the root marker and SQLite binding row.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RootBindingId(Uuid);

impl RootBindingId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    pub fn parse(value: &str) -> Result<Self, ClientSyncError> {
        let uuid = Uuid::parse_str(value).map_err(|_| ClientSyncError::InvalidRoot)?;
        if uuid.get_version_num() != 7 || uuid.to_string() != value {
            return Err(ClientSyncError::InvalidRoot);
        }
        Ok(Self(uuid))
    }
}

impl Default for RootBindingId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for RootBindingId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("RootBindingId")
            .field(&self.0)
            .finish()
    }
}

impl fmt::Display for RootBindingId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for RootBindingId {
    type Err = ClientSyncError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalObjectKind {
    File,
    Directory,
}

impl LocalObjectKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::File => "FILE",
            Self::Directory => "DIRECTORY",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalFingerprint {
    kind: LocalObjectKind,
    length: Option<u64>,
    sha256: Option<Sha256Digest>,
}

impl LocalFingerprint {
    #[must_use]
    pub const fn directory() -> Self {
        Self {
            kind: LocalObjectKind::Directory,
            length: None,
            sha256: None,
        }
    }

    #[must_use]
    pub const fn file(length: u64, sha256: Sha256Digest) -> Self {
        Self {
            kind: LocalObjectKind::File,
            length: Some(length),
            sha256: Some(sha256),
        }
    }

    #[must_use]
    pub const fn kind(self) -> LocalObjectKind {
        self.kind
    }

    #[must_use]
    pub const fn length(self) -> Option<u64> {
        self.length
    }

    #[must_use]
    pub const fn sha256(self) -> Option<Sha256Digest> {
        self.sha256
    }
}

/// Narrow filesystem boundary used by the orchestration engine.
#[async_trait]
pub trait LocalReplica: Send + Sync {
    fn scope(&self) -> ReplicaScope;
    /// Canonical physical root used only to establish one native watcher and
    /// bounded local reconciliation. Callers must validate the root before
    /// acting on events and must never log this path by default.
    fn root_path(&self) -> &Path;
    /// Legacy/offline fixtures return None. A production replica must carry
    /// the same durable profile identity as its HTTP remote and SQLite row.
    fn server_profile_id(&self) -> Option<ServerProfileId> {
        None
    }
    fn binding_id(&self) -> RootBindingId;
    fn validate_root(&self) -> Result<(), ClientSyncError>;
    fn inspect(
        &self,
        relative: &ManagedRelativePath,
    ) -> Result<Option<LocalFingerprint>, ClientSyncError>;
    fn open_visible_file(&self, relative: &ManagedRelativePath) -> Result<File, ClientSyncError>;
    fn read_staged_chunk(
        &self,
        staging: &ManagedRelativePath,
        offset: u64,
        max: usize,
    ) -> Result<bytes::Bytes, ClientSyncError>;
    fn ensure_directory(&self, relative: &ManagedRelativePath) -> Result<(), ClientSyncError>;
    fn has_portable_name_collision(
        &self,
        relative: &ManagedRelativePath,
    ) -> Result<bool, ClientSyncError>;
    fn stage_directory(&self, operation_id: Uuid) -> Result<ManagedRelativePath, ClientSyncError>;
    fn directory_staging_location(
        &self,
        operation_id: Uuid,
    ) -> Result<ManagedRelativePath, ClientSyncError>;
    async fn stage_content(
        &self,
        operation_id: Uuid,
        expected_length: u64,
        expected_sha256: Sha256Digest,
        content: ContentByteStream,
    ) -> Result<ManagedRelativePath, ClientSyncError>;
    fn staging_location(&self, operation_id: Uuid) -> Result<ManagedRelativePath, ClientSyncError>;
    fn quarantine_location(
        &self,
        node_id: NodeId,
        operation_id: Uuid,
    ) -> Result<ManagedRelativePath, ClientSyncError>;
    fn expose_staged_file(
        &self,
        staging: &ManagedRelativePath,
        destination: &ManagedRelativePath,
        operation_id: Uuid,
    ) -> Result<(), ClientSyncError>;
    fn rename_path(
        &self,
        source: &ManagedRelativePath,
        destination: &ManagedRelativePath,
    ) -> Result<(), ClientSyncError>;
    fn quarantine_path(
        &self,
        source: &ManagedRelativePath,
        node_id: NodeId,
        operation_id: Uuid,
    ) -> Result<ManagedRelativePath, ClientSyncError>;
    fn restore_quarantined(
        &self,
        quarantine: &ManagedRelativePath,
        destination: &ManagedRelativePath,
    ) -> Result<(), ClientSyncError>;
    fn list_descendants(
        &self,
        relative: &ManagedRelativePath,
    ) -> Result<Vec<ManagedRelativePath>, ClientSyncError>;
    fn remove_owned_staging(&self, staging: &ManagedRelativePath) -> Result<(), ClientSyncError>;
    fn write_operation_receipt(&self, operation_id: Uuid) -> Result<(), ClientSyncError>;
    fn has_operation_receipt(&self, operation_id: Uuid) -> Result<bool, ClientSyncError>;
    fn remove_operation_receipt(&self, operation_id: Uuid) -> Result<(), ClientSyncError>;
}

/// Standard Windows/Linux managed-root implementation.
pub struct FilesystemLocalReplica {
    root: PathBuf,
    scope: ReplicaScope,
    binding_id: RootBindingId,
    server_profile_id: Option<ServerProfileId>,
}

impl FilesystemLocalReplica {
    /// Explicitly initialize an empty user-approved root, or reopen an already
    /// initialized root for the same authenticated logical scope.
    pub fn initialize(
        root: impl AsRef<Path>,
        scope: ReplicaScope,
    ) -> Result<Self, ClientSyncError> {
        Self::initialize_inner(root.as_ref(), scope, None)
    }

    /// Initialize a production managed root with an immutable profile marker.
    /// Existing legacy markers require a future explicit rebind workflow.
    pub fn initialize_for_profile(
        root: impl AsRef<Path>,
        scope: ReplicaScope,
        profile_id: ServerProfileId,
    ) -> Result<Self, ClientSyncError> {
        Self::initialize_inner(root.as_ref(), scope, Some(profile_id))
    }

    fn initialize_inner(
        root: &Path,
        scope: ReplicaScope,
        server_profile_id: Option<ServerProfileId>,
    ) -> Result<Self, ClientSyncError> {
        reject_unsafe_root_choice(root)?;
        reject_redirect(root)?;
        if !root.is_dir() {
            return Err(ClientSyncError::InvalidRoot);
        }
        let root = fs::canonicalize(root).map_err(|_| ClientSyncError::InvalidRoot)?;
        let control = root.join(CONTROL_DIRECTORY);
        let marker = control.join(MARKER_FILE);

        if marker.exists() {
            return Self::open_inner(&root, scope, server_profile_id);
        }
        if fs::read_dir(&root)
            .map_err(|_| ClientSyncError::InvalidRoot)?
            .next()
            .is_some()
        {
            return Err(ClientSyncError::InvalidRoot);
        }

        fs::create_dir(&control)?;
        fs::create_dir(control.join(STAGING_DIRECTORY))?;
        fs::create_dir(control.join(QUARANTINE_DIRECTORY))?;
        let binding_id = RootBindingId::new();
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)?;
        write!(
            file,
            "{}\n{binding_id}\n{}\n{}\n{}\n",
            if server_profile_id.is_some() {
                PROFILED_MARKER_VERSION
            } else {
                MARKER_VERSION
            },
            scope.owner_user_id(),
            scope.device_id(),
            scope.library_id()
        )?;
        if let Some(profile_id) = server_profile_id {
            writeln!(file, "{profile_id}")?;
        }
        file.sync_all()?;
        sync_directory(&control)?;
        sync_directory(&root)?;
        Ok(Self {
            root,
            scope,
            binding_id,
            server_profile_id,
        })
    }

    /// Open a previously initialized root. Marker identity and every scope ID
    /// must match before any mutation method can be used.
    pub fn open(root: impl AsRef<Path>, scope: ReplicaScope) -> Result<Self, ClientSyncError> {
        Self::open_inner(root.as_ref(), scope, None)
    }

    /// Reopen only the exact profile recorded in the physical managed root.
    pub fn open_for_profile(
        root: impl AsRef<Path>,
        scope: ReplicaScope,
        profile_id: ServerProfileId,
    ) -> Result<Self, ClientSyncError> {
        Self::open_inner(root.as_ref(), scope, Some(profile_id))
    }

    fn open_inner(
        root: &Path,
        scope: ReplicaScope,
        server_profile_id: Option<ServerProfileId>,
    ) -> Result<Self, ClientSyncError> {
        reject_unsafe_root_choice(root)?;
        reject_redirect(root)?;
        let root = fs::canonicalize(root).map_err(|_| ClientSyncError::InvalidRoot)?;
        let binding_id = Self::open_marker_only(
            &root.join(CONTROL_DIRECTORY).join(MARKER_FILE),
            scope,
            server_profile_id,
        )?;
        let replica = Self {
            root,
            scope,
            binding_id,
            server_profile_id,
        };
        replica.validate_root()?;
        Ok(replica)
    }

    fn resolve(&self, relative: &ManagedRelativePath) -> Result<PathBuf, ClientSyncError> {
        self.validate_root()?;
        if relative
            .as_path()
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
            && !relative.is_root()
        {
            return Err(ClientSyncError::InvalidRelativePath);
        }
        let candidate = self.root.join(relative.as_path());
        reject_excessive_windows_path(&candidate)?;
        reject_existing_redirects(&self.root, relative)?;
        Ok(candidate)
    }

    fn internal_relative(parts: &[&str]) -> Result<ManagedRelativePath, ClientSyncError> {
        ManagedRelativePath::new(parts.join("/"))
    }

    fn operation_receipt_location(
        operation_id: Uuid,
    ) -> Result<ManagedRelativePath, ClientSyncError> {
        Self::internal_relative(&[
            CONTROL_DIRECTORY,
            STAGING_DIRECTORY,
            &format!("{operation_id}.applied"),
        ])
    }

    fn expected_operation_receipt(operation_id: Uuid) -> String {
        format!("{OPERATION_RECEIPT_VERSION}\n{operation_id}\n")
    }
}

#[async_trait]
impl LocalReplica for FilesystemLocalReplica {
    fn scope(&self) -> ReplicaScope {
        self.scope
    }

    fn root_path(&self) -> &Path {
        &self.root
    }

    fn binding_id(&self) -> RootBindingId {
        self.binding_id
    }

    fn server_profile_id(&self) -> Option<ServerProfileId> {
        self.server_profile_id
    }

    fn validate_root(&self) -> Result<(), ClientSyncError> {
        reject_redirect(&self.root)?;
        let current = fs::canonicalize(&self.root).map_err(|_| ClientSyncError::InvalidRoot)?;
        if current != self.root {
            return Err(ClientSyncError::RootRedirected);
        }
        let marker = self.root.join(CONTROL_DIRECTORY).join(MARKER_FILE);
        reject_redirect(&marker)?;
        let reopened = Self::open_marker_only(&marker, self.scope, self.server_profile_id)?;
        if reopened != self.binding_id {
            return Err(ClientSyncError::WrongRootBinding);
        }
        Ok(())
    }

    fn inspect(
        &self,
        relative: &ManagedRelativePath,
    ) -> Result<Option<LocalFingerprint>, ClientSyncError> {
        let path = self.resolve(relative)?;
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(ClientSyncError::LocalIo),
        };
        if is_redirect(&metadata) {
            return Err(ClientSyncError::RootRedirected);
        }
        if metadata.is_dir() {
            return Ok(Some(LocalFingerprint::directory()));
        }
        if !metadata.is_file() {
            return Err(ClientSyncError::InvalidState);
        }
        let length = metadata.len();
        let modified_before = metadata.modified().ok();
        let mut file = File::open(path)?;
        let mut digest = Sha256::new();
        let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            digest.update(&buffer[..read]);
        }
        let bytes: [u8; 32] = digest.finalize().into();
        let after = fs::symlink_metadata(self.resolve(relative)?)?;
        if is_redirect(&after)
            || !after.is_file()
            || after.len() != length
            || after.modified().ok() != modified_before
        {
            return Err(ClientSyncError::ContentUnstable);
        }
        Ok(Some(LocalFingerprint::file(
            length,
            Sha256Digest::from_bytes(bytes),
        )))
    }

    fn open_visible_file(&self, relative: &ManagedRelativePath) -> Result<File, ClientSyncError> {
        let path = self.resolve(relative)?;
        let before = fs::symlink_metadata(&path)?;
        if is_redirect(&before) || !before.is_file() {
            return Err(ClientSyncError::RootRedirected);
        }
        let file = open_file_without_following_redirect(&path)?;
        let after = fs::symlink_metadata(&path)?;
        if is_redirect(&after) || !after.is_file() || after.len() != before.len() {
            return Err(ClientSyncError::ContentUnstable);
        }
        Ok(file)
    }

    fn read_staged_chunk(
        &self,
        staging: &ManagedRelativePath,
        offset: u64,
        max: usize,
    ) -> Result<bytes::Bytes, ClientSyncError> {
        let expected_prefix = format!("{CONTROL_DIRECTORY}/{STAGING_DIRECTORY}/");
        if !staging.as_str().starts_with(&expected_prefix) {
            return Err(ClientSyncError::InvalidRelativePath);
        }
        let mut file = self.open_visible_file(staging)?;
        file.seek(SeekFrom::Start(offset))?;
        let mut buffer = vec![0_u8; max];
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Err(ClientSyncError::ContentIntegrityMismatch);
        }
        buffer.truncate(read);
        Ok(bytes::Bytes::from(buffer))
    }

    fn ensure_directory(&self, relative: &ManagedRelativePath) -> Result<(), ClientSyncError> {
        if relative.is_root() {
            return self.validate_root();
        }
        let path = self.resolve(relative)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && !is_redirect(&metadata) => return Ok(()),
            Ok(_) => return Err(ClientSyncError::InvalidState),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(ClientSyncError::LocalIo),
        }
        let parent = path.parent().ok_or(ClientSyncError::InvalidRelativePath)?;
        reject_redirect(parent)?;
        fs::create_dir(&path)?;
        sync_directory(parent)?;
        Ok(())
    }

    fn has_portable_name_collision(
        &self,
        relative: &ManagedRelativePath,
    ) -> Result<bool, ClientSyncError> {
        if relative.is_root() {
            return Ok(false);
        }
        let parent = relative
            .parent()
            .ok_or(ClientSyncError::InvalidRelativePath)?;
        let parent_path = self.resolve(&parent)?;
        let target_name = relative
            .as_str()
            .rsplit('/')
            .next()
            .ok_or(ClientSyncError::InvalidRelativePath)?;
        let target_key = local_collision_key(target_name);
        let mut seen = 0_usize;
        for entry in fs::read_dir(parent_path)? {
            let entry = entry?;
            seen = seen.checked_add(1).ok_or(ClientSyncError::ResourceLimit)?;
            if seen > MAX_LOCAL_SCAN_ENTRIES {
                return Err(ClientSyncError::ResourceLimit);
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                return Ok(true);
            };
            if name != target_name && local_collision_key(&name) == target_key {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn stage_directory(&self, operation_id: Uuid) -> Result<ManagedRelativePath, ClientSyncError> {
        let staging = self.directory_staging_location(operation_id)?;
        match self.inspect(&staging)? {
            Some(fingerprint) if fingerprint == LocalFingerprint::directory() => {
                return Ok(staging);
            }
            Some(_) => return Err(ClientSyncError::InvalidState),
            None => {}
        }
        let path = self.resolve(&staging)?;
        fs::create_dir(&path)?;
        sync_directory(path.parent().ok_or(ClientSyncError::InvalidRelativePath)?)?;
        Ok(staging)
    }

    fn directory_staging_location(
        &self,
        operation_id: Uuid,
    ) -> Result<ManagedRelativePath, ClientSyncError> {
        Self::internal_relative(&[
            CONTROL_DIRECTORY,
            STAGING_DIRECTORY,
            &format!("{operation_id}.dir"),
        ])
    }

    async fn stage_content(
        &self,
        operation_id: Uuid,
        expected_length: u64,
        expected_sha256: Sha256Digest,
        mut content: ContentByteStream,
    ) -> Result<ManagedRelativePath, ClientSyncError> {
        if expected_length > MAX_DOWNLOAD_BYTES {
            return Err(ClientSyncError::ResourceLimit);
        }
        let staging = self.staging_location(operation_id)?;
        let path = self.resolve(&staging)?;
        if path.exists() {
            if self.inspect(&staging)?
                == Some(LocalFingerprint::file(expected_length, expected_sha256))
            {
                return Ok(staging);
            }
            fs::remove_file(&path)?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let mut digest = Sha256::new();
        let mut received = 0_u64;
        while let Some(chunk) = content.next().await {
            let chunk = chunk.map_err(ClientSyncError::Remote)?;
            if chunk.len() > MAX_CONTENT_CHUNK_BYTES {
                drop(file);
                let _ = fs::remove_file(&path);
                return Err(ClientSyncError::ResourceLimit);
            }
            let chunk_length =
                u64::try_from(chunk.len()).map_err(|_| ClientSyncError::ResourceLimit)?;
            received = received
                .checked_add(chunk_length)
                .ok_or(ClientSyncError::ResourceLimit)?;
            if received > expected_length || received > MAX_DOWNLOAD_BYTES {
                drop(file);
                let _ = fs::remove_file(&path);
                return Err(ClientSyncError::ContentIntegrityMismatch);
            }
            file.write_all(&chunk)?;
            digest.update(&chunk);
        }
        file.flush()?;
        file.sync_all()?;
        drop(file);
        let actual_hash = Sha256Digest::from_bytes(digest.finalize().into());
        if received != expected_length || actual_hash != expected_sha256 {
            let _ = fs::remove_file(&path);
            return Err(ClientSyncError::ContentIntegrityMismatch);
        }
        sync_directory(path.parent().ok_or(ClientSyncError::InvalidRelativePath)?)?;
        Ok(staging)
    }

    fn staging_location(&self, operation_id: Uuid) -> Result<ManagedRelativePath, ClientSyncError> {
        Self::internal_relative(&[
            CONTROL_DIRECTORY,
            STAGING_DIRECTORY,
            &format!("{operation_id}.part"),
        ])
    }

    fn quarantine_location(
        &self,
        node_id: NodeId,
        operation_id: Uuid,
    ) -> Result<ManagedRelativePath, ClientSyncError> {
        Self::internal_relative(&[
            CONTROL_DIRECTORY,
            QUARANTINE_DIRECTORY,
            &format!("{node_id}-{operation_id}"),
        ])
    }

    fn expose_staged_file(
        &self,
        staging: &ManagedRelativePath,
        destination: &ManagedRelativePath,
        operation_id: Uuid,
    ) -> Result<(), ClientSyncError> {
        let source = self.resolve(staging)?;
        let destination = self.resolve(destination)?;
        let parent = destination
            .parent()
            .ok_or(ClientSyncError::InvalidRelativePath)?;
        reject_redirect(parent)?;

        #[cfg(not(target_os = "windows"))]
        {
            fs::rename(&source, &destination)?;
        }

        #[cfg(target_os = "windows")]
        {
            let backup = self
                .root
                .join(CONTROL_DIRECTORY)
                .join(QUARANTINE_DIRECTORY)
                .join(format!("replace-{operation_id}"));
            let had_destination = destination.exists();
            if had_destination {
                fs::rename(&destination, &backup)?;
            }
            if let Err(error) = fs::rename(&source, &destination) {
                if had_destination {
                    let _ = fs::rename(&backup, &destination);
                }
                return Err(error.into());
            }
        }

        let _ = operation_id;
        sync_directory(parent)?;
        Ok(())
    }

    fn rename_path(
        &self,
        source: &ManagedRelativePath,
        destination: &ManagedRelativePath,
    ) -> Result<(), ClientSyncError> {
        if source == destination {
            return Ok(());
        }
        let source_path = self.resolve(source)?;
        let destination_path = self.resolve(destination)?;
        if destination_path.exists() {
            return Err(ClientSyncError::InvalidState);
        }
        let destination_parent = destination_path
            .parent()
            .ok_or(ClientSyncError::InvalidRelativePath)?;
        reject_redirect(destination_parent)?;
        fs::rename(&source_path, &destination_path)?;
        if let Some(source_parent) = source_path.parent() {
            sync_directory(source_parent)?;
        }
        sync_directory(destination_parent)?;
        Ok(())
    }

    fn quarantine_path(
        &self,
        source: &ManagedRelativePath,
        node_id: NodeId,
        operation_id: Uuid,
    ) -> Result<ManagedRelativePath, ClientSyncError> {
        let quarantine = self.quarantine_location(node_id, operation_id)?;
        let source_path = self.resolve(source)?;
        let quarantine_path = self.resolve(&quarantine)?;
        if quarantine_path.exists() {
            if !source_path.exists() {
                return Ok(quarantine);
            }
            return Err(ClientSyncError::InvalidState);
        }
        fs::rename(&source_path, &quarantine_path)?;
        if let Some(parent) = source_path.parent() {
            sync_directory(parent)?;
        }
        sync_directory(
            quarantine_path
                .parent()
                .ok_or(ClientSyncError::InvalidRelativePath)?,
        )?;
        Ok(quarantine)
    }

    fn restore_quarantined(
        &self,
        quarantine: &ManagedRelativePath,
        destination: &ManagedRelativePath,
    ) -> Result<(), ClientSyncError> {
        let source = self.resolve(quarantine)?;
        let destination = self.resolve(destination)?;
        if destination.exists() {
            return Err(ClientSyncError::InvalidState);
        }
        fs::rename(&source, &destination)?;
        sync_directory(
            destination
                .parent()
                .ok_or(ClientSyncError::InvalidRelativePath)?,
        )?;
        Ok(())
    }

    fn list_descendants(
        &self,
        relative: &ManagedRelativePath,
    ) -> Result<Vec<ManagedRelativePath>, ClientSyncError> {
        let root = self.resolve(relative)?;
        let mut pending = VecDeque::from([(root, relative.clone())]);
        let mut result = Vec::new();
        while let Some((directory, directory_relative)) = pending.pop_front() {
            for entry in fs::read_dir(directory)? {
                let entry = entry?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| ClientSyncError::InvalidState)?;
                if directory_relative.is_root() && name == CONTROL_DIRECTORY {
                    continue;
                }
                let child = directory_relative.child(&name)?;
                let metadata = fs::symlink_metadata(entry.path())?;
                if is_redirect(&metadata) {
                    return Err(ClientSyncError::RootRedirected);
                }
                result.push(child.clone());
                if result.len() > MAX_LOCAL_SCAN_ENTRIES {
                    return Err(ClientSyncError::ResourceLimit);
                }
                if metadata.is_dir() {
                    pending.push_back((entry.path(), child));
                }
            }
        }
        Ok(result)
    }

    fn remove_owned_staging(&self, staging: &ManagedRelativePath) -> Result<(), ClientSyncError> {
        let expected_prefix = format!("{CONTROL_DIRECTORY}/{STAGING_DIRECTORY}/");
        if !staging.as_str().starts_with(&expected_prefix) {
            return Err(ClientSyncError::InvalidRelativePath);
        }
        let path = self.resolve(staging)?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn write_operation_receipt(&self, operation_id: Uuid) -> Result<(), ClientSyncError> {
        let relative = Self::operation_receipt_location(operation_id)?;
        let path = self.resolve(&relative)?;
        let expected = Self::expected_operation_receipt(operation_id);
        match fs::read_to_string(&path) {
            Ok(actual) if actual == expected => return Ok(()),
            Ok(_) => return Err(ClientSyncError::InvalidState),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(ClientSyncError::LocalIo),
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(expected.as_bytes())?;
        file.sync_all()?;
        drop(file);
        sync_directory(path.parent().ok_or(ClientSyncError::InvalidRelativePath)?)
    }

    fn has_operation_receipt(&self, operation_id: Uuid) -> Result<bool, ClientSyncError> {
        let relative = Self::operation_receipt_location(operation_id)?;
        let path = self.resolve(&relative)?;
        match fs::read_to_string(path) {
            Ok(actual) => {
                if actual == Self::expected_operation_receipt(operation_id) {
                    Ok(true)
                } else {
                    Err(ClientSyncError::InvalidState)
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err(ClientSyncError::LocalIo),
        }
    }

    fn remove_operation_receipt(&self, operation_id: Uuid) -> Result<(), ClientSyncError> {
        let relative = Self::operation_receipt_location(operation_id)?;
        let path = self.resolve(&relative)?;
        match fs::remove_file(&path) {
            Ok(()) => sync_directory(path.parent().ok_or(ClientSyncError::InvalidRelativePath)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

impl FilesystemLocalReplica {
    fn open_marker_only(
        marker_path: &Path,
        scope: ReplicaScope,
        server_profile_id: Option<ServerProfileId>,
    ) -> Result<RootBindingId, ClientSyncError> {
        reject_redirect(marker_path)?;
        let mut marker = String::new();
        File::open(marker_path)
            .map_err(|_| ClientSyncError::WrongRootBinding)?
            .take(513)
            .read_to_string(&mut marker)
            .map_err(|_| ClientSyncError::WrongRootBinding)?;
        if marker.len() > 512 {
            return Err(ClientSyncError::InvalidRoot);
        }
        let mut lines = marker.lines();
        let expected_version = if server_profile_id.is_some() {
            PROFILED_MARKER_VERSION
        } else {
            MARKER_VERSION
        };
        if lines.next() != Some(expected_version) {
            return Err(ClientSyncError::WrongServerProfile);
        }
        let binding = RootBindingId::parse(lines.next().ok_or(ClientSyncError::InvalidRoot)?)?;
        let owner = scope.owner_user_id().to_string();
        let device = scope.device_id().to_string();
        let library = scope.library_id().to_string();
        if lines.next() != Some(owner.as_str())
            || lines.next() != Some(device.as_str())
            || lines.next() != Some(library.as_str())
        {
            return Err(ClientSyncError::WrongRootBinding);
        }
        if let Some(profile_id) = server_profile_id
            && lines.next() != Some(profile_id.to_string().as_str())
        {
            return Err(ClientSyncError::WrongServerProfile);
        }
        if lines.next().is_some() {
            return Err(ClientSyncError::WrongRootBinding);
        }
        Ok(binding)
    }
}

fn reject_unsafe_root_choice(root: &Path) -> Result<(), ClientSyncError> {
    if !root.is_absolute() || root.parent().is_none() {
        return Err(ClientSyncError::InvalidRoot);
    }
    let canonical = fs::canonicalize(root).map_err(|_| ClientSyncError::InvalidRoot)?;
    if canonical.parent().is_none() {
        return Err(ClientSyncError::InvalidRoot);
    }
    let is_home = ["HOME", "USERPROFILE"].iter().any(|variable| {
        std::env::var_os(variable)
            .map(PathBuf::from)
            .and_then(|path| fs::canonicalize(path).ok())
            .as_ref()
            == Some(&canonical)
    });
    if is_home
        || std::env::current_dir()
            .ok()
            .and_then(|path| fs::canonicalize(path).ok())
            .as_ref()
            == Some(&canonical)
    {
        return Err(ClientSyncError::InvalidRoot);
    }
    Ok(())
}

fn reject_existing_redirects(
    root: &Path,
    relative: &ManagedRelativePath,
) -> Result<(), ClientSyncError> {
    reject_redirect(root)?;
    let mut current = root.to_path_buf();
    for component in relative.as_path().components() {
        let Component::Normal(component) = component else {
            return Err(ClientSyncError::InvalidRelativePath);
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if is_redirect(&metadata) => {
                return Err(ClientSyncError::RootRedirected);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return Err(ClientSyncError::LocalIo),
        }
    }
    Ok(())
}

fn reject_redirect(path: &Path) -> Result<(), ClientSyncError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ClientSyncError::InvalidRoot)?;
    if is_redirect(&metadata) {
        return Err(ClientSyncError::RootRedirected);
    }
    Ok(())
}

#[cfg(unix)]
fn open_file_without_following_redirect(path: &Path) -> Result<File, ClientSyncError> {
    use std::os::unix::fs::OpenOptionsExt;

    const O_NOFOLLOW: i32 = 0o400000;
    const ELOOP: i32 = 40;

    OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW)
        .open(path)
        .map_err(|error| {
            if error.raw_os_error() == Some(ELOOP) {
                ClientSyncError::RootRedirected
            } else {
                error.into()
            }
        })
}

#[cfg(windows)]
fn open_file_without_following_redirect(path: &Path) -> Result<File, ClientSyncError> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(Into::into)
}

#[cfg(windows)]
fn reject_excessive_windows_path(path: &Path) -> Result<(), ClientSyncError> {
    use std::os::windows::ffi::OsStrExt;

    // The Win32 extended-length ceiling is 32,767 UTF-16 code units including
    // prefixes and the terminating NUL. Keep explicit headroom for the latter
    // and for standard-library normalization rather than relying on a late,
    // operation-specific OS failure.
    const MAX_MANAGED_PATH_UTF16_UNITS: usize = 32_000;
    if path.as_os_str().encode_wide().count() > MAX_MANAGED_PATH_UTF16_UNITS {
        return Err(ClientSyncError::InvalidRelativePath);
    }
    Ok(())
}

#[cfg(not(windows))]
fn reject_excessive_windows_path(_path: &Path) -> Result<(), ClientSyncError> {
    Ok(())
}

#[cfg(unix)]
fn is_redirect(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn is_redirect(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(any(unix, windows)))]
fn is_redirect(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), ClientSyncError> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), ClientSyncError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use futures_util::stream;
    use sha2::{Digest, Sha256};
    use synveil_core::{DeviceId, LibraryId, UserId};

    use super::{FilesystemLocalReplica, LocalFingerprint, LocalReplica};
    use crate::{
        ManagedRelativePath, ReplicaScope, boxed_content_stream,
        test_support::remove_dir_all_bounded,
    };

    #[cfg(unix)]
    use crate::ClientSyncError;

    fn temporary_directory(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "synveil-client-sync-{label}-{}",
            uuid::Uuid::now_v7()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    fn scope() -> ReplicaScope {
        ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new())
    }

    #[tokio::test]
    async fn root_marker_binding_and_verified_staging_are_durable() {
        let root = temporary_directory("replica");
        let replica_scope = scope();
        let replica = FilesystemLocalReplica::initialize(&root, replica_scope).unwrap();
        assert!(FilesystemLocalReplica::open(&root, replica_scope).is_ok());
        assert!(FilesystemLocalReplica::open(&root, scope()).is_err());

        let parent = ManagedRelativePath::new("folder").unwrap();
        replica.ensure_directory(&parent).unwrap();
        let bytes = bytes::Bytes::from_static(b"verified bytes");
        let digest = synveil_core::Sha256Digest::from_bytes(Sha256::digest(&bytes).into());
        let staging = replica
            .stage_content(
                uuid::Uuid::now_v7(),
                bytes.len() as u64,
                digest,
                boxed_content_stream(stream::iter([Ok(bytes.clone())])),
            )
            .await
            .unwrap();
        let destination = parent.child("file.txt").unwrap();
        replica
            .expose_staged_file(&staging, &destination, uuid::Uuid::now_v7())
            .unwrap();
        assert_eq!(
            replica.inspect(&destination).unwrap(),
            Some(LocalFingerprint::file(bytes.len() as u64, digest))
        );
        remove_dir_all_bounded(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_redirect_is_rejected() {
        use std::os::unix::fs::symlink;

        let root = temporary_directory("symlink-root");
        let outside = temporary_directory("symlink-outside");
        let replica = FilesystemLocalReplica::initialize(&root, scope()).unwrap();
        symlink(&outside, root.join("redirect")).unwrap();
        assert!(
            replica
                .inspect(&ManagedRelativePath::new("redirect/secret").unwrap())
                .is_err()
        );
        fs::remove_file(root.join("redirect")).unwrap();
        remove_dir_all_bounded(&root).unwrap();
        remove_dir_all_bounded(&outside).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn owned_staging_chunk_rejects_symlink_redirect() {
        use std::os::unix::fs::symlink;

        let root = temporary_directory("staging-symlink");
        let outside = temporary_directory("staging-symlink-outside");
        let replica = FilesystemLocalReplica::initialize(&root, scope()).unwrap();
        let staging = replica.staging_location(uuid::Uuid::now_v7()).unwrap();
        fs::write(outside.join("secret.part"), b"outside").unwrap();
        symlink(outside.join("secret.part"), root.join(staging.as_path())).unwrap();

        assert!(matches!(
            replica
                .read_staged_chunk(&staging, 0, 4)
                .expect_err("symlink staging must not be opened"),
            ClientSyncError::RootRedirected
        ));

        fs::remove_file(root.join(staging.as_path())).unwrap();
        remove_dir_all_bounded(&root).unwrap();
        remove_dir_all_bounded(&outside).unwrap();
    }

    #[tokio::test]
    async fn corrupt_stream_never_becomes_visible() {
        let root = temporary_directory("corrupt");
        let replica = FilesystemLocalReplica::initialize(&root, scope()).unwrap();
        let destination = ManagedRelativePath::new("file.txt").unwrap();
        fs::write(root.join("file.txt"), b"old").unwrap();
        let expected = synveil_core::Sha256Digest::from_bytes([7; 32]);
        let result = replica
            .stage_content(
                uuid::Uuid::now_v7(),
                8,
                expected,
                boxed_content_stream(stream::iter([Ok(bytes::Bytes::from_static(b"short"))])),
            )
            .await;
        assert!(result.is_err());
        assert_eq!(fs::read(root.join(destination.as_path())).unwrap(), b"old");

        let same_length_wrong_hash = replica
            .stage_content(
                uuid::Uuid::now_v7(),
                8,
                expected,
                boxed_content_stream(stream::iter([Ok(bytes::Bytes::from_static(b"12345678"))])),
            )
            .await;
        assert!(same_length_wrong_hash.is_err());
        assert_eq!(fs::read(root.join(destination.as_path())).unwrap(), b"old");
        remove_dir_all_bounded(&root).unwrap();
    }
}
