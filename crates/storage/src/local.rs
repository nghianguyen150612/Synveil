//! Local-filesystem implementation of the backend-neutral `ObjectStore` port.
//!
//! The adapter owns a private, opaque layout below an explicitly supplied
//! root. Logical names never enter this module. Final objects are represented
//! by a committed marker, metadata record, and immutable content file inside a
//! hashed object directory; the marker is the last visibility transition.

use std::{
    collections::BTreeMap,
    fs,
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::{StreamExt, stream};
use sha2::{Digest, Sha256};
use synveil_core::Sha256Digest;
use synveil_object_store::{
    ByteRange, ByteStream, CapabilityEvidence, CapabilitySupport, DeleteOutcome,
    DeleteReconciliation, IntegrityExpectation, ObjectKey, ObjectMetadata, ObjectRead, ObjectStore,
    ObjectStoreError, ObjectVersion, PromotionReceipt, PutRequest, StagedMetadata, StagingHandle,
    StagingProgress, StorageAvailability, StorageBackendKind, StorageCapabilities,
    StorageCapability, boxed_stream,
};
use tokio::{
    fs as async_fs,
    fs::{File, OpenOptions},
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom},
    sync::Mutex,
};
use uuid::Uuid;

const ROOT_MARKER_NAME: &str = ".synveil-storage-root";
const ROOT_MARKER_CONTENT: &[u8] = b"synveil-local-object-store-v1\n";
const OBJECTS_DIRECTORY: &str = "objects";
const STAGING_DIRECTORY: &str = "staging";
const OBJECT_LAYOUT_VERSION: &str = "v1";
const OBJECT_CONTENT_NAME: &str = "content";
const OBJECT_METADATA_NAME: &str = "metadata";
const OBJECT_COMMITTED_NAME: &str = "committed";
const OBJECT_COMMITTED_CONTENT: &[u8] = b"synveil-object-committed-v1\n";
const OBJECT_DELETING_SUFFIX: &str = ".deleting";
const STAGING_UPLOAD_SUFFIX: &str = ".upload";
const STAGING_VERIFIED_SUFFIX: &str = ".verified";
const STAGING_METADATA_SUFFIX: &str = ".metadata";
const STAGING_METADATA_TEMP_SUFFIX: &str = ".metadata.tmp";
const STAGING_RECORD_KIND: &str = "synveil-staging-v1";
const OBJECT_RECORD_KIND: &str = "synveil-object-v1";
const MAX_METADATA_BYTES: u64 = 4 * 1024;
const STREAM_BUFFER_BYTES: usize = 64 * 1024;

/// Production local-filesystem object-store adapter.
///
/// Construction is explicit and initializes only Synveil's marker and managed
/// directories. No platform default path is selected and no existing content
/// is recursively removed.
#[derive(Clone)]
pub struct LocalFilesystemObjectStore {
    root: PathBuf,
    objects_dir: PathBuf,
    staging_dir: PathBuf,
    object_layout_dir: PathBuf,
    capabilities: StorageCapabilities,
    directory_sync_supported: bool,
    mutation_lock: Arc<Mutex<()>>,
}

struct ObjectLocator {
    directory: PathBuf,
    deleting_directory: PathBuf,
    content: PathBuf,
    metadata: PathBuf,
    committed: PathBuf,
}

struct StagingPaths {
    upload: PathBuf,
    verified: PathBuf,
    metadata: PathBuf,
    metadata_temp: PathBuf,
}

struct StagingRecord {
    length: u64,
    sha256: Sha256Digest,
}

struct ObjectRecord {
    key: ObjectKey,
    length: u64,
    sha256: Sha256Digest,
    version: ObjectVersion,
}

struct FileReadState {
    file: File,
    remaining: u64,
    hasher: Option<Sha256>,
    expected_sha256: Option<Sha256Digest>,
}

impl LocalFilesystemObjectStore {
    /// Initialize or open a Synveil-managed local object-store root.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, ObjectStoreError> {
        let requested_root = root.as_ref().to_path_buf();
        if requested_root.as_os_str().is_empty() {
            return Err(ObjectStoreError::InvalidRequest);
        }
        if !requested_root.is_absolute() {
            return Err(ObjectStoreError::InvalidRequest);
        }

        ensure_root_directory_sync(&requested_root)?;
        let root = validate_storage_root_sync(&requested_root)?;
        initialize_root_marker_sync(&root)?;

        let objects_dir = root.join(OBJECTS_DIRECTORY);
        let staging_dir = root.join(STAGING_DIRECTORY);
        let object_layout_dir = objects_dir.join(OBJECT_LAYOUT_VERSION);
        ensure_directory_sync(&objects_dir)?;
        ensure_directory_sync(&staging_dir)?;
        ensure_directory_sync(&object_layout_dir)?;

        let file_fsync = probe_file_fsync(&staging_dir);
        let directory_fsync = probe_directory_fsync(&[
            root.as_path(),
            objects_dir.as_path(),
            staging_dir.as_path(),
            object_layout_dir.as_path(),
        ]);
        let atomic_rename = probe_atomic_rename(&staging_dir);
        let atomic_promotion = probe_hard_link(&staging_dir, &object_layout_dir);

        let capabilities =
            capabilities_for(file_fsync, directory_fsync, atomic_rename, atomic_promotion);

        Ok(Self {
            root,
            objects_dir,
            staging_dir,
            object_layout_dir,
            capabilities,
            directory_sync_supported: directory_fsync,
            mutation_lock: Arc::new(Mutex::new(())),
        })
    }

    /// Compatibility constructor spelling for runtime composition code.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, ObjectStoreError> {
        Self::open(root)
    }

    fn marker_path(&self) -> PathBuf {
        self.root.join(ROOT_MARKER_NAME)
    }

    async fn verify_layout(&self) -> Result<(), ObjectStoreError> {
        require_directory_async(&self.root).await?;
        let marker_path = self.marker_path();
        require_file_async(&marker_path).await?;
        let marker_content = read_small_file(&marker_path).await?;
        if marker_content.as_slice() != ROOT_MARKER_CONTENT {
            return Err(ObjectStoreError::StorageUnavailable);
        }
        require_directory_async(&self.objects_dir).await?;
        require_directory_async(&self.staging_dir).await?;
        require_directory_async(&self.object_layout_dir).await
    }

    fn object_locator(&self, key: &ObjectKey) -> Result<ObjectLocator, ObjectStoreError> {
        validate_object_key_at_adapter_boundary(key)?;
        let hash = Sha256::digest(key.as_str().as_bytes());
        let encoded = hex_digest(&hash);
        let prefix = &encoded[..2];
        let directory = self.object_layout_dir.join(prefix).join(&encoded);
        let deleting_directory = self
            .object_layout_dir
            .join(prefix)
            .join(format!("{encoded}{OBJECT_DELETING_SUFFIX}"));
        Ok(ObjectLocator {
            content: directory.join(OBJECT_CONTENT_NAME),
            metadata: directory.join(OBJECT_METADATA_NAME),
            committed: directory.join(OBJECT_COMMITTED_NAME),
            directory,
            deleting_directory,
        })
    }

    fn deletion_locator(locator: &ObjectLocator) -> ObjectLocator {
        let directory = locator.deleting_directory.clone();
        ObjectLocator {
            content: directory.join(OBJECT_CONTENT_NAME),
            metadata: directory.join(OBJECT_METADATA_NAME),
            committed: directory.join(OBJECT_COMMITTED_NAME),
            deleting_directory: directory.clone(),
            directory,
        }
    }

    fn staging_paths(&self, handle: &StagingHandle) -> Result<StagingPaths, ObjectStoreError> {
        validate_staging_handle_at_adapter_boundary(handle)?;
        let stem = self.staging_dir.join(handle.as_str());
        Ok(StagingPaths {
            upload: with_suffix(&stem, STAGING_UPLOAD_SUFFIX),
            verified: with_suffix(&stem, STAGING_VERIFIED_SUFFIX),
            metadata: with_suffix(&stem, STAGING_METADATA_SUFFIX),
            metadata_temp: with_suffix(&stem, STAGING_METADATA_TEMP_SUFFIX),
        })
    }

    async fn ensure_object_parent(&self, locator: &ObjectLocator) -> Result<(), ObjectStoreError> {
        require_directory_async(&self.object_layout_dir).await?;
        let prefix = locator
            .directory
            .parent()
            .ok_or(ObjectStoreError::InvalidKey)?;
        ensure_directory_async(prefix).await
    }

    async fn read_staging_record(
        &self,
        paths: &StagingPaths,
    ) -> Result<StagingRecord, ObjectStoreError> {
        match async_fs::symlink_metadata(&paths.verified).await {
            Ok(_) => {
                require_file_async(&paths.verified).await?;
                require_file_async(&paths.metadata).await.map_err(|error| {
                    if error == ObjectStoreError::NotFound {
                        ObjectStoreError::StagingConflict
                    } else {
                        error
                    }
                })?;
                let fields = parse_record(&read_small_file(&paths.metadata).await?)?;
                if fields.get("kind").map(String::as_str) != Some(STAGING_RECORD_KIND) {
                    return Err(ObjectStoreError::StorageUnavailable);
                }
                let length = parse_length(fields.get("length"))?;
                let sha256 = parse_digest(fields.get("sha256"))?;
                Ok(StagingRecord { length, sha256 })
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                if async_fs::symlink_metadata(&paths.upload).await.is_ok()
                    || async_fs::symlink_metadata(&paths.metadata).await.is_ok()
                    || async_fs::symlink_metadata(&paths.metadata_temp)
                        .await
                        .is_ok()
                {
                    Err(ObjectStoreError::StagingConflict)
                } else {
                    Err(ObjectStoreError::StagingNotFound)
                }
            }
            Err(error) => Err(map_io_error(error)),
        }
    }

    async fn read_object_record(&self, key: &ObjectKey) -> Result<ObjectRecord, ObjectStoreError> {
        let locator = self.object_locator(key)?;
        let prefix = locator
            .directory
            .parent()
            .ok_or(ObjectStoreError::InvalidKey)?;
        match require_directory_async(prefix).await {
            Ok(()) => {}
            Err(ObjectStoreError::NotFound) => return Err(ObjectStoreError::NotFound),
            Err(error) => return Err(error),
        }
        match async_fs::symlink_metadata(&locator.directory).await {
            Ok(metadata) => {
                if is_redirected(&metadata) || !metadata.is_dir() {
                    return Err(ObjectStoreError::StorageUnavailable);
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return match async_fs::symlink_metadata(&locator.deleting_directory).await {
                    Ok(_) => Err(ObjectStoreError::StorageUnavailable),
                    Err(tombstone_error) if tombstone_error.kind() == ErrorKind::NotFound => {
                        Err(ObjectStoreError::NotFound)
                    }
                    Err(tombstone_error) => Err(map_io_error(tombstone_error)),
                };
            }
            Err(error) => return Err(map_io_error(error)),
        }

        if async_fs::symlink_metadata(&locator.deleting_directory)
            .await
            .is_ok()
        {
            return Err(ObjectStoreError::StorageUnavailable);
        }

        require_file_async(&locator.committed).await?;
        let marker = read_small_file(&locator.committed).await?;
        if marker.as_slice() != OBJECT_COMMITTED_CONTENT {
            return Err(ObjectStoreError::StorageUnavailable);
        }
        let content_metadata = require_file_async(&locator.content)
            .await
            .map_err(|error| {
                if error == ObjectStoreError::NotFound {
                    ObjectStoreError::StorageUnavailable
                } else {
                    error
                }
            })?;
        require_file_async(&locator.metadata)
            .await
            .map_err(|error| {
                if error == ObjectStoreError::NotFound {
                    ObjectStoreError::StorageUnavailable
                } else {
                    error
                }
            })?;
        let fields = parse_record(&read_small_file(&locator.metadata).await?)?;
        if fields.get("kind").map(String::as_str) != Some(OBJECT_RECORD_KIND) {
            return Err(ObjectStoreError::StorageUnavailable);
        }

        let stored_key = ObjectKey::new(
            fields
                .get("key")
                .ok_or(ObjectStoreError::StorageUnavailable)?
                .clone(),
        )
        .map_err(|_| ObjectStoreError::StorageUnavailable)?;
        if stored_key != *key {
            return Err(ObjectStoreError::StorageUnavailable);
        }
        let length = parse_length(fields.get("length"))?;
        if content_metadata.len() != length {
            return Err(ObjectStoreError::IntegrityMismatch);
        }
        let sha256 = parse_digest(fields.get("sha256"))?;
        let version = ObjectVersion::new(
            fields
                .get("version")
                .ok_or(ObjectStoreError::StorageUnavailable)?
                .clone(),
        )
        .map_err(|_| ObjectStoreError::StorageUnavailable)?;

        Ok(ObjectRecord {
            key: stored_key,
            length,
            sha256,
            version,
        })
    }

    async fn verify_file_contents(
        &self,
        path: &Path,
        expected_length: u64,
        expected_sha256: Sha256Digest,
    ) -> Result<(), ObjectStoreError> {
        require_file_async(path).await?;
        let mut file = File::open(path).await.map_err(map_io_error)?;
        verify_open_file_contents(&mut file, expected_length, expected_sha256).await
    }

    async fn sync_managed_directory(&self, path: &Path) -> Result<(), ObjectStoreError> {
        if self.directory_sync_supported {
            sync_directory(path).await?;
        }
        Ok(())
    }

    async fn sync_staging_directory(&self) -> Result<(), ObjectStoreError> {
        self.sync_managed_directory(&self.staging_dir).await
    }

    async fn remove_staging_path(&self, path: &Path) -> Result<(), ObjectStoreError> {
        match async_fs::symlink_metadata(path).await {
            Ok(_) => {
                require_file_async(path).await?;
                async_fs::remove_file(path).await.map_err(map_io_error)
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(map_io_error(error)),
        }
    }

    async fn remove_staging_files(&self, paths: &StagingPaths) -> Result<(), ObjectStoreError> {
        self.remove_staging_path(&paths.upload).await?;
        self.remove_staging_path(&paths.verified).await?;
        self.remove_staging_path(&paths.metadata).await?;
        self.remove_staging_path(&paths.metadata_temp).await?;
        self.sync_staging_directory().await
    }

    async fn remove_committed_object(
        &self,
        key: &ObjectKey,
        locator: &ObjectLocator,
        expected_version: Option<&ObjectVersion>,
    ) -> Result<DeleteOutcome, ObjectStoreError> {
        let deleting = Self::deletion_locator(locator);
        match async_fs::symlink_metadata(&deleting.directory).await {
            Ok(_) => {
                if async_fs::symlink_metadata(&locator.directory).await.is_ok() {
                    return Err(ObjectStoreError::StorageUnavailable);
                }
                self.finish_deletion_tombstone(key, &deleting, expected_version)
                    .await?;
                return Ok(DeleteOutcome::Deleted);
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(map_io_error(error)),
        }

        match async_fs::symlink_metadata(&locator.directory).await {
            Ok(metadata) => {
                if is_redirected(&metadata) || !metadata.is_dir() {
                    return Err(ObjectStoreError::StorageUnavailable);
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(DeleteOutcome::AlreadyAbsent);
            }
            Err(error) => return Err(map_io_error(error)),
        }

        self.validate_object_directory_entries(locator).await?;
        let record = self.read_object_record(key).await?;
        if expected_version.is_some_and(|expected| record.version != *expected) {
            return Err(ObjectStoreError::PreconditionFailed);
        }

        async_fs::rename(&locator.directory, &deleting.directory)
            .await
            .map_err(map_io_error)?;
        let prefix = locator
            .directory
            .parent()
            .ok_or(ObjectStoreError::InvalidKey)?;
        self.sync_managed_directory(prefix).await?;
        self.finish_deletion_tombstone(key, &deleting, expected_version)
            .await?;
        Ok(DeleteOutcome::Deleted)
    }

    async fn validate_object_directory_entries(
        &self,
        locator: &ObjectLocator,
    ) -> Result<(), ObjectStoreError> {
        let metadata = async_fs::symlink_metadata(&locator.directory)
            .await
            .map_err(map_io_error)?;
        if is_redirected(&metadata) || !metadata.is_dir() {
            return Err(ObjectStoreError::StorageUnavailable);
        }
        let mut entries = async_fs::read_dir(&locator.directory)
            .await
            .map_err(map_io_error)?;
        while let Some(entry) = entries.next_entry().await.map_err(map_io_error)? {
            let name = entry.file_name();
            if name != OBJECT_CONTENT_NAME
                && name != OBJECT_METADATA_NAME
                && name != OBJECT_COMMITTED_NAME
            {
                return Err(ObjectStoreError::StorageUnavailable);
            }
        }
        Ok(())
    }

    async fn deletion_record(
        &self,
        key: &ObjectKey,
        locator: &ObjectLocator,
    ) -> Result<Option<ObjectRecord>, ObjectStoreError> {
        match async_fs::symlink_metadata(&locator.metadata).await {
            Ok(_) => require_file_async(&locator.metadata).await?,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(map_io_error(error)),
        };
        let fields = parse_record(&read_small_file(&locator.metadata).await?)?;
        if fields.get("kind").map(String::as_str) != Some(OBJECT_RECORD_KIND) {
            return Err(ObjectStoreError::StorageUnavailable);
        }
        let stored_key = ObjectKey::new(
            fields
                .get("key")
                .ok_or(ObjectStoreError::StorageUnavailable)?
                .clone(),
        )
        .map_err(|_| ObjectStoreError::StorageUnavailable)?;
        if stored_key != *key {
            return Err(ObjectStoreError::StorageUnavailable);
        }
        let length = parse_length(fields.get("length"))?;
        let sha256 = parse_digest(fields.get("sha256"))?;
        let version = ObjectVersion::new(
            fields
                .get("version")
                .ok_or(ObjectStoreError::StorageUnavailable)?
                .clone(),
        )
        .map_err(|_| ObjectStoreError::StorageUnavailable)?;
        Ok(Some(ObjectRecord {
            key: stored_key,
            length,
            sha256,
            version,
        }))
    }

    async fn finish_deletion_tombstone(
        &self,
        key: &ObjectKey,
        locator: &ObjectLocator,
        expected_version: Option<&ObjectVersion>,
    ) -> Result<(), ObjectStoreError> {
        self.validate_object_directory_entries(locator).await?;
        let stored_record = self.deletion_record(key, locator).await?;
        if let (Some(expected), Some(stored)) = (expected_version, stored_record.as_ref())
            && &stored.version != expected
        {
            return Err(ObjectStoreError::PreconditionFailed);
        }
        if stored_record.is_none()
            && (async_fs::symlink_metadata(&locator.content).await.is_ok()
                || async_fs::symlink_metadata(&locator.committed).await.is_ok())
        {
            return Err(ObjectStoreError::StorageUnavailable);
        }

        match async_fs::symlink_metadata(&locator.content).await {
            Ok(_) => {
                require_file_async(&locator.content).await?;
                async_fs::remove_file(&locator.content)
                    .await
                    .map_err(map_io_error)?;
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(map_io_error(error)),
        }
        match async_fs::symlink_metadata(&locator.committed).await {
            Ok(_) => {
                require_file_async(&locator.committed).await?;
                if read_small_file(&locator.committed).await?.as_slice() != OBJECT_COMMITTED_CONTENT
                {
                    return Err(ObjectStoreError::StorageUnavailable);
                }
                async_fs::remove_file(&locator.committed)
                    .await
                    .map_err(map_io_error)?;
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(map_io_error(error)),
        }
        self.sync_managed_directory(&locator.directory).await?;
        match async_fs::symlink_metadata(&locator.metadata).await {
            Ok(_) => {
                require_file_async(&locator.metadata).await?;
                async_fs::remove_file(&locator.metadata)
                    .await
                    .map_err(map_io_error)?;
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(map_io_error(error)),
        }
        async_fs::remove_dir(&locator.directory)
            .await
            .map_err(map_io_error)?;
        let prefix = locator
            .directory
            .parent()
            .ok_or(ObjectStoreError::InvalidKey)?;
        self.sync_managed_directory(prefix).await
    }

    async fn destination_is_committed(
        &self,
        key: &ObjectKey,
        locator: &ObjectLocator,
    ) -> Result<bool, ObjectStoreError> {
        match async_fs::symlink_metadata(&locator.deleting_directory).await {
            Ok(_) => return Err(ObjectStoreError::StorageUnavailable),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(map_io_error(error)),
        }
        match async_fs::symlink_metadata(&locator.directory).await {
            Ok(metadata) => {
                if is_redirected(&metadata) || !metadata.is_dir() {
                    return Err(ObjectStoreError::StorageUnavailable);
                }
                match self.read_object_record(key).await {
                    Ok(_) => Ok(true),
                    Err(ObjectStoreError::NotFound) => Err(ObjectStoreError::StorageUnavailable),
                    Err(error) => Err(error),
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(error) => Err(map_io_error(error)),
        }
    }

    async fn cleanup_incomplete_destination(
        &self,
        locator: &ObjectLocator,
    ) -> Result<(), ObjectStoreError> {
        let marker_exists = async_fs::symlink_metadata(&locator.committed).await.is_ok();
        if marker_exists {
            return Ok(());
        }

        for path in [&locator.content, &locator.metadata, &locator.committed] {
            match async_fs::symlink_metadata(path).await {
                Ok(_) => {
                    require_file_async(path).await?;
                    async_fs::remove_file(path).await.map_err(map_io_error)?;
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(map_io_error(error)),
            }
        }
        match async_fs::remove_dir(&locator.directory).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(map_io_error(error)),
        }
    }

    async fn sync_promotion_directories(
        &self,
        locator: &ObjectLocator,
    ) -> Result<(), ObjectStoreError> {
        if !self.directory_sync_supported {
            return Ok(());
        }

        let prefix = locator
            .directory
            .parent()
            .ok_or(ObjectStoreError::InvalidKey)?;
        let layout = prefix.parent().ok_or(ObjectStoreError::InvalidKey)?;
        let directories = [
            locator.directory.as_path(),
            prefix,
            layout,
            self.objects_dir.as_path(),
            self.root.as_path(),
        ];
        for directory in directories {
            sync_directory(directory).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl ObjectStore for LocalFilesystemObjectStore {
    fn capabilities(&self) -> StorageCapabilities {
        self.capabilities.clone()
    }

    async fn put(&self, request: PutRequest) -> Result<ObjectMetadata, ObjectStoreError> {
        let (key, body, integrity) = request.into_parts();
        let handle = self.begin_staged_write().await?;
        let staged = match self.write_staged(&handle, body, integrity).await {
            Ok(staged) => staged,
            Err(error) => {
                let _ = self.abort_staged(&handle).await;
                return Err(error);
            }
        };

        match self.promote_temp(&handle, &key).await {
            Ok(receipt) => Ok(receipt.metadata().clone()),
            Err(error) => {
                let _ = self.abort_staged(&handle).await;
                let _ = staged;
                Err(error)
            }
        }
    }

    async fn begin_staged_write(&self) -> Result<StagingHandle, ObjectStoreError> {
        let _guard = self.mutation_lock.lock().await;
        self.verify_layout().await?;

        for _ in 0..8 {
            let handle = StagingHandle::new(format!("v1-{}", Uuid::now_v7()))
                .map_err(|_| ObjectStoreError::StorageUnavailable)?;
            let paths = self.staging_paths(&handle)?;
            match OpenOptions::new()
                .write(true)
                .read(true)
                .create_new(true)
                .open(&paths.upload)
                .await
            {
                Ok(_) => return Ok(handle),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(map_io_error(error)),
            }
        }

        Err(ObjectStoreError::StorageUnavailable)
    }

    async fn write_staged(
        &self,
        handle: &StagingHandle,
        mut body: ByteStream,
        integrity: IntegrityExpectation,
    ) -> Result<StagedMetadata, ObjectStoreError> {
        let _guard = self.mutation_lock.lock().await;
        self.verify_layout().await?;
        let paths = self.staging_paths(handle)?;

        if async_fs::symlink_metadata(&paths.verified).await.is_ok()
            || async_fs::symlink_metadata(&paths.metadata).await.is_ok()
            || async_fs::symlink_metadata(&paths.metadata_temp)
                .await
                .is_ok()
        {
            return Err(ObjectStoreError::StagingConflict);
        }
        require_file_async(&paths.upload).await.map_err(|error| {
            if error == ObjectStoreError::NotFound {
                ObjectStoreError::StagingNotFound
            } else {
                error
            }
        })?;

        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&paths.upload)
            .await
            .map_err(map_io_error)?;
        let mut length = 0_u64;
        let mut hasher = Sha256::new();

        while let Some(chunk) = body.next().await {
            let chunk = chunk?;
            let chunk_length =
                u64::try_from(chunk.len()).map_err(|_| ObjectStoreError::IntegrityMismatch)?;
            let next_length = length
                .checked_add(chunk_length)
                .ok_or(ObjectStoreError::IntegrityMismatch)?;
            if integrity
                .expected_length()
                .is_some_and(|expected| next_length > expected)
            {
                return Err(ObjectStoreError::IntegrityMismatch);
            }
            file.write_all(&chunk).await.map_err(map_io_error)?;
            hasher.update(&chunk);
            length = next_length;
        }

        file.sync_all().await.map_err(map_io_error)?;
        let sha256 = digest_from_hasher(hasher);
        if integrity
            .expected_length()
            .is_some_and(|expected| expected != length)
            || integrity
                .expected_sha256()
                .is_some_and(|expected| expected != sha256)
        {
            return Err(ObjectStoreError::IntegrityMismatch);
        }
        drop(file);

        let staging_record =
            format!("kind={STAGING_RECORD_KIND}\nlength={length}\nsha256={sha256}\n");
        write_new_file(&paths.metadata_temp, staging_record.as_bytes()).await?;
        if let Err(error) = async_fs::hard_link(&paths.metadata_temp, &paths.metadata).await {
            let _ = async_fs::remove_file(&paths.metadata_temp).await;
            return Err(if error.kind() == ErrorKind::AlreadyExists {
                ObjectStoreError::StagingConflict
            } else {
                map_io_error(error)
            });
        }
        let _ = async_fs::remove_file(&paths.metadata_temp).await;

        if let Err(error) = async_fs::hard_link(&paths.upload, &paths.verified).await {
            let _ = async_fs::remove_file(&paths.metadata).await;
            return Err(if error.kind() == ErrorKind::AlreadyExists {
                ObjectStoreError::StagingConflict
            } else {
                map_io_error(error)
            });
        }
        let _ = async_fs::remove_file(&paths.upload).await;

        self.verify_file_contents(&paths.verified, length, sha256)
            .await?;
        self.sync_staging_directory().await?;
        Ok(StagedMetadata::new(handle.clone(), length, Some(sha256)))
    }

    async fn append_staged(
        &self,
        handle: &StagingHandle,
        expected_offset: u64,
        chunk: Bytes,
        maximum_length: u64,
    ) -> Result<StagingProgress, ObjectStoreError> {
        let _guard = self.mutation_lock.lock().await;
        self.verify_layout().await?;
        let paths = self.staging_paths(handle)?;

        if async_fs::symlink_metadata(&paths.verified).await.is_ok()
            || async_fs::symlink_metadata(&paths.metadata).await.is_ok()
            || async_fs::symlink_metadata(&paths.metadata_temp)
                .await
                .is_ok()
        {
            return Err(ObjectStoreError::StagingConflict);
        }

        let current = require_file_async(&paths.upload).await.map_err(|error| {
            if error == ObjectStoreError::NotFound {
                ObjectStoreError::StagingNotFound
            } else {
                error
            }
        })?;
        if current.len() != expected_offset {
            return Err(ObjectStoreError::PreconditionFailed);
        }
        let chunk_length =
            u64::try_from(chunk.len()).map_err(|_| ObjectStoreError::InvalidRequest)?;
        let next_length = expected_offset
            .checked_add(chunk_length)
            .ok_or(ObjectStoreError::IntegrityMismatch)?;
        if next_length > maximum_length {
            return Err(ObjectStoreError::IntegrityMismatch);
        }

        let mut file = OpenOptions::new()
            .write(true)
            .append(true)
            .open(&paths.upload)
            .await
            .map_err(map_io_error)?;
        file.write_all(&chunk).await.map_err(map_io_error)?;
        file.sync_all().await.map_err(map_io_error)?;
        self.sync_staging_directory().await?;
        Ok(StagingProgress::partial(handle.clone(), next_length))
    }

    async fn staging_progress(
        &self,
        handle: &StagingHandle,
    ) -> Result<StagingProgress, ObjectStoreError> {
        self.verify_layout().await?;
        let paths = self.staging_paths(handle)?;
        if async_fs::symlink_metadata(&paths.verified).await.is_ok()
            || async_fs::symlink_metadata(&paths.metadata).await.is_ok()
        {
            let record = self.read_staging_record(&paths).await?;
            self.verify_file_contents(&paths.verified, record.length, record.sha256)
                .await?;
            return Ok(StagingProgress::verified(
                handle.clone(),
                record.length,
                record.sha256,
            ));
        }

        let metadata = require_file_async(&paths.upload).await.map_err(|error| {
            if error == ObjectStoreError::NotFound {
                ObjectStoreError::StagingNotFound
            } else {
                error
            }
        })?;
        Ok(StagingProgress::partial(handle.clone(), metadata.len()))
    }

    async fn finalize_staged(
        &self,
        handle: &StagingHandle,
        integrity: IntegrityExpectation,
    ) -> Result<StagedMetadata, ObjectStoreError> {
        let _guard = self.mutation_lock.lock().await;
        self.verify_layout().await?;
        let paths = self.staging_paths(handle)?;

        if async_fs::symlink_metadata(&paths.verified).await.is_ok()
            || async_fs::symlink_metadata(&paths.metadata).await.is_ok()
        {
            let record = self.read_staging_record(&paths).await?;
            if integrity
                .expected_length()
                .is_some_and(|expected| expected != record.length)
                || integrity
                    .expected_sha256()
                    .is_some_and(|expected| expected != record.sha256)
            {
                return Err(ObjectStoreError::IntegrityMismatch);
            }
            self.verify_file_contents(&paths.verified, record.length, record.sha256)
                .await?;
            return Ok(StagedMetadata::new(
                handle.clone(),
                record.length,
                Some(record.sha256),
            ));
        }

        if async_fs::symlink_metadata(&paths.metadata_temp)
            .await
            .is_ok()
        {
            return Err(ObjectStoreError::StagingConflict);
        }
        require_file_async(&paths.upload).await.map_err(|error| {
            if error == ObjectStoreError::NotFound {
                ObjectStoreError::StagingNotFound
            } else {
                error
            }
        })?;

        let mut file = File::open(&paths.upload).await.map_err(map_io_error)?;
        let (length, sha256) = hash_open_file_contents(&mut file).await?;
        if integrity
            .expected_length()
            .is_some_and(|expected| expected != length)
            || integrity
                .expected_sha256()
                .is_some_and(|expected| expected != sha256)
        {
            return Err(ObjectStoreError::IntegrityMismatch);
        }
        drop(file);

        let staging_record =
            format!("kind={STAGING_RECORD_KIND}\nlength={length}\nsha256={sha256}\n");
        write_new_file(&paths.metadata_temp, staging_record.as_bytes()).await?;
        if let Err(error) = async_fs::hard_link(&paths.metadata_temp, &paths.metadata).await {
            let _ = async_fs::remove_file(&paths.metadata_temp).await;
            return Err(if error.kind() == ErrorKind::AlreadyExists {
                ObjectStoreError::StagingConflict
            } else {
                map_io_error(error)
            });
        }
        let _ = async_fs::remove_file(&paths.metadata_temp).await;

        if let Err(error) = async_fs::hard_link(&paths.upload, &paths.verified).await {
            let _ = async_fs::remove_file(&paths.metadata).await;
            return Err(if error.kind() == ErrorKind::AlreadyExists {
                ObjectStoreError::StagingConflict
            } else {
                map_io_error(error)
            });
        }
        let _ = async_fs::remove_file(&paths.upload).await;
        self.verify_file_contents(&paths.verified, length, sha256)
            .await?;
        self.sync_staging_directory().await?;
        Ok(StagedMetadata::new(handle.clone(), length, Some(sha256)))
    }

    async fn promote_temp(
        &self,
        handle: &StagingHandle,
        destination: &ObjectKey,
    ) -> Result<PromotionReceipt, ObjectStoreError> {
        let _guard = self.mutation_lock.lock().await;
        self.verify_layout().await?;
        if !self
            .capabilities
            .supports(StorageCapability::AtomicPromotion)
        {
            return Err(ObjectStoreError::UnsupportedCapability(
                StorageCapability::AtomicPromotion,
            ));
        }

        let locator = self.object_locator(destination)?;
        if self.destination_is_committed(destination, &locator).await? {
            return Err(ObjectStoreError::AlreadyExists);
        }
        let staging_paths = self.staging_paths(handle)?;
        let staging_record = self.read_staging_record(&staging_paths).await?;
        self.verify_file_contents(
            &staging_paths.verified,
            staging_record.length,
            staging_record.sha256,
        )
        .await?;
        self.ensure_object_parent(&locator).await?;

        if let Err(error) = async_fs::create_dir(&locator.directory).await {
            if error.kind() != ErrorKind::AlreadyExists {
                return Err(map_io_error(error));
            }
            if self.destination_is_committed(destination, &locator).await? {
                return Err(ObjectStoreError::AlreadyExists);
            }
            return Err(ObjectStoreError::StorageUnavailable);
        }

        let promotion_result = async {
            if let Err(error) = async_fs::hard_link(&staging_paths.verified, &locator.content).await
            {
                return Err(map_io_error(error));
            }

            let version = ObjectVersion::new(format!("v1-{}", Uuid::now_v7()))
                .map_err(|_| ObjectStoreError::StorageUnavailable)?;
            let metadata = format!(
                "kind={OBJECT_RECORD_KIND}\nkey={}\nlength={}\nsha256={}\nversion={}\n",
                destination.as_str(),
                staging_record.length,
                staging_record.sha256,
                version.as_str()
            );
            write_new_file(&locator.metadata, metadata.as_bytes()).await?;
            write_new_file(&locator.committed, OBJECT_COMMITTED_CONTENT).await?;
            self.sync_promotion_directories(&locator).await?;

            Ok::<ObjectMetadata, ObjectStoreError>(ObjectMetadata::new(
                destination.clone(),
                staging_record.length,
                Some(staging_record.sha256),
                Some(version),
            ))
        }
        .await;

        let metadata = match promotion_result {
            Ok(metadata) => metadata,
            Err(error) => {
                let _ = self.cleanup_incomplete_destination(&locator).await;
                return Err(error);
            }
        };

        // The final object is already committed and durable at this point.
        // Staging cleanup is best-effort so a cleanup failure cannot turn a
        // successful create-only promotion into an ambiguous write result.
        let _ = self.remove_staging_files(&staging_paths).await;
        Ok(PromotionReceipt::new(metadata))
    }

    async fn abort_staged(&self, handle: &StagingHandle) -> Result<(), ObjectStoreError> {
        let _guard = self.mutation_lock.lock().await;
        self.verify_layout().await?;
        let paths = self.staging_paths(handle)?;
        self.remove_staging_files(&paths).await
    }

    async fn get(&self, key: &ObjectKey) -> Result<ObjectRead, ObjectStoreError> {
        self.verify_layout().await?;
        let record = self.read_object_record(key).await?;
        let locator = self.object_locator(key)?;
        let mut file = File::open(&locator.content).await.map_err(map_io_error)?;
        // A full HTTP response advertises the verified length and SHA-256
        // before body framing begins. Verify the already-open descriptor
        // first, rather than emitting an entire same-length corrupt body and
        // discovering the mismatch only at EOF. This uses one fixed-size
        // buffer and rewinds the same descriptor, so it preserves bounded
        // streaming memory and avoids a close/reopen race. Keep the streaming
        // digest below as a defense against a later external modification.
        verify_open_file_contents(&mut file, record.length, record.sha256).await?;
        file.seek(SeekFrom::Start(0)).await.map_err(map_io_error)?;
        let body = file_stream(file, record.length, Some(record.sha256));
        Ok(ObjectRead::new(
            ObjectMetadata::new(
                record.key,
                record.length,
                Some(record.sha256),
                Some(record.version),
            ),
            None,
            body,
        ))
    }

    async fn range_read(
        &self,
        key: &ObjectKey,
        range: ByteRange,
    ) -> Result<ObjectRead, ObjectStoreError> {
        self.verify_layout().await?;
        let record = self.read_object_record(key).await?;
        if range.end_exclusive() > record.length {
            return Err(ObjectStoreError::InvalidRange);
        }
        let locator = self.object_locator(key)?;
        let mut file = File::open(&locator.content).await.map_err(map_io_error)?;
        verify_open_file_contents(&mut file, record.length, record.sha256).await?;
        file.seek(SeekFrom::Start(range.start()))
            .await
            .map_err(map_io_error)?;
        let body = file_stream(file, range.length(), None);
        Ok(ObjectRead::new(
            ObjectMetadata::new(
                record.key,
                record.length,
                Some(record.sha256),
                Some(record.version),
            ),
            Some(range),
            body,
        ))
    }

    async fn exists(&self, key: &ObjectKey) -> Result<bool, ObjectStoreError> {
        self.verify_layout().await?;
        match self.read_object_record(key).await {
            Ok(_) => Ok(true),
            Err(ObjectStoreError::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }

    async fn metadata(&self, key: &ObjectKey) -> Result<ObjectMetadata, ObjectStoreError> {
        self.verify_layout().await?;
        let record = self.read_object_record(key).await?;
        Ok(ObjectMetadata::new(
            record.key,
            record.length,
            Some(record.sha256),
            Some(record.version),
        ))
    }

    async fn delete(&self, key: &ObjectKey) -> Result<DeleteOutcome, ObjectStoreError> {
        let _guard = self.mutation_lock.lock().await;
        self.verify_layout().await?;
        let locator = self.object_locator(key)?;
        self.remove_committed_object(key, &locator, None).await
    }

    async fn conditional_delete(
        &self,
        key: &ObjectKey,
        expected_version: &ObjectVersion,
    ) -> Result<DeleteOutcome, ObjectStoreError> {
        let _guard = self.mutation_lock.lock().await;
        self.verify_layout().await?;
        let locator = self.object_locator(key)?;
        self.remove_committed_object(key, &locator, Some(expected_version))
            .await
    }

    async fn reconcile_delete(
        &self,
        key: &ObjectKey,
    ) -> Result<DeleteReconciliation, ObjectStoreError> {
        let _guard = self.mutation_lock.lock().await;
        self.verify_layout().await?;
        let locator = self.object_locator(key)?;
        let live = async_fs::symlink_metadata(&locator.directory).await;
        let deleting = async_fs::symlink_metadata(&locator.deleting_directory).await;
        match (live, deleting) {
            (Ok(_), Ok(_)) => Err(ObjectStoreError::StorageUnavailable),
            (Ok(_), Err(error)) if error.kind() == ErrorKind::NotFound => {
                self.read_object_record(key).await.map(|record| {
                    DeleteReconciliation::Present(ObjectMetadata::new(
                        record.key,
                        record.length,
                        Some(record.sha256),
                        Some(record.version),
                    ))
                })
            }
            (Err(error), Ok(_)) if error.kind() == ErrorKind::NotFound => {
                let deleting = Self::deletion_locator(&locator);
                self.validate_object_directory_entries(&deleting).await?;
                let record = self.deletion_record(key, &deleting).await?;
                if record.is_none()
                    && (async_fs::symlink_metadata(&deleting.content).await.is_ok()
                        || async_fs::symlink_metadata(&deleting.committed)
                            .await
                            .is_ok())
                {
                    return Err(ObjectStoreError::StorageUnavailable);
                }
                Ok(DeleteReconciliation::InProgress(record.map(|record| {
                    ObjectMetadata::new(
                        record.key,
                        record.length,
                        Some(record.sha256),
                        Some(record.version),
                    )
                })))
            }
            (Err(live_error), Err(deleting_error))
                if live_error.kind() == ErrorKind::NotFound
                    && deleting_error.kind() == ErrorKind::NotFound =>
            {
                Ok(DeleteReconciliation::Absent)
            }
            (Err(error), _) | (_, Err(error)) => Err(map_io_error(error)),
        }
    }
}

fn capabilities_for(
    file_fsync: bool,
    directory_fsync: bool,
    atomic_rename: bool,
    atomic_promotion: bool,
) -> StorageCapabilities {
    let supported = |value| {
        if value {
            CapabilitySupport::Supported
        } else {
            CapabilitySupport::Unsupported
        }
    };
    StorageCapabilities::for_location(
        StorageBackendKind::LocalFilesystem,
        StorageAvailability::Available,
        CapabilityEvidence::AdapterProbe { version: 1 },
    )
    .with_support(StorageCapability::Reflink, CapabilitySupport::Unsupported)
    .with_support(
        StorageCapability::BlockClone,
        CapabilitySupport::Unsupported,
    )
    .with_support(
        StorageCapability::CopyOnWriteClone,
        CapabilitySupport::Unsupported,
    )
    .with_support(
        StorageCapability::NativeSnapshot,
        CapabilitySupport::Unsupported,
    )
    .with_support(
        StorageCapability::Compression,
        CapabilitySupport::Unsupported,
    )
    .with_support(
        StorageCapability::Checksumming,
        CapabilitySupport::Supported,
    )
    .with_support(
        StorageCapability::SparseFiles,
        CapabilitySupport::Unsupported,
    )
    .with_support(StorageCapability::AtomicRename, supported(atomic_rename))
    .with_support(StorageCapability::DurableFsync, supported(file_fsync))
    .with_support(StorageCapability::RangeReads, CapabilitySupport::Supported)
    .with_support(
        StorageCapability::FilesystemHealth,
        CapabilitySupport::Unsupported,
    )
    .with_support(
        StorageCapability::AtomicPromotion,
        supported(atomic_promotion),
    )
    .with_support(
        StorageCapability::ExclusiveCreate,
        CapabilitySupport::Supported,
    )
    .with_support(
        StorageCapability::DurableFlush,
        supported(file_fsync && directory_fsync && atomic_promotion),
    )
    .with_support(
        StorageCapability::ReadAfterWrite,
        CapabilitySupport::Supported,
    )
    .with_support(
        StorageCapability::ConditionalDelete,
        CapabilitySupport::Supported,
    )
}

fn ensure_root_directory_sync(path: &Path) -> Result<(), ObjectStoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if is_redirected(&metadata) || !metadata.is_dir() {
                return Err(ObjectStoreError::StorageUnavailable);
            }
            Ok(())
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(map_io_error)?;
            let metadata = fs::symlink_metadata(path).map_err(map_io_error)?;
            if is_redirected(&metadata) || !metadata.is_dir() {
                return Err(ObjectStoreError::StorageUnavailable);
            }
            Ok(())
        }
        Err(error) => Err(map_io_error(error)),
    }
}

fn ensure_directory_sync(path: &Path) -> Result<(), ObjectStoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if is_redirected(&metadata) || !metadata.is_dir() {
                return Err(ObjectStoreError::StorageUnavailable);
            }
            Ok(())
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            fs::create_dir(path).map_err(map_io_error)?;
            let metadata = fs::symlink_metadata(path).map_err(map_io_error)?;
            if is_redirected(&metadata) || !metadata.is_dir() {
                return Err(ObjectStoreError::StorageUnavailable);
            }
            Ok(())
        }
        Err(error) => Err(map_io_error(error)),
    }
}

fn initialize_root_marker_sync(root: &Path) -> Result<(), ObjectStoreError> {
    let marker = root.join(ROOT_MARKER_NAME);
    match fs::symlink_metadata(&marker) {
        Ok(metadata) => {
            if is_redirected(&metadata) || !metadata.is_file() {
                return Err(ObjectStoreError::StorageUnavailable);
            }
            let file = fs::File::open(&marker).map_err(map_io_error)?;
            let mut content = Vec::new();
            file.take((ROOT_MARKER_CONTENT.len() + 1) as u64)
                .read_to_end(&mut content)
                .map_err(map_io_error)?;
            if content == ROOT_MARKER_CONTENT {
                Ok(())
            } else {
                Err(ObjectStoreError::StorageUnavailable)
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&marker)
                .map_err(|error| {
                    if error.kind() == ErrorKind::AlreadyExists {
                        ObjectStoreError::StorageUnavailable
                    } else {
                        map_io_error(error)
                    }
                })?;
            file.write_all(ROOT_MARKER_CONTENT).map_err(map_io_error)?;
            file.sync_all().map_err(map_io_error)
        }
        Err(error) => Err(map_io_error(error)),
    }
}

fn probe_file_fsync(staging_dir: &Path) -> bool {
    let path = probe_path(staging_dir, "fsync");
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(b"probe")?;
        file.sync_all()
    })()
    .is_ok();
    let _ = fs::remove_file(path);
    result
}

fn probe_directory_fsync(paths: &[&Path]) -> bool {
    paths.iter().all(|path| sync_directory_sync(path).is_ok())
}

fn probe_atomic_rename(staging_dir: &Path) -> bool {
    let source = probe_path(staging_dir, "rename-source");
    let destination = probe_path(staging_dir, "rename-destination");
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&source)?;
        file.write_all(b"probe")?;
        fs::rename(&source, &destination)?;
        Ok::<_, std::io::Error>(fs::metadata(&destination)?.is_file())
    })()
    .unwrap_or(false);
    let _ = fs::remove_file(source);
    let _ = fs::remove_file(destination);
    result
}

fn probe_hard_link(staging_dir: &Path, object_layout_dir: &Path) -> bool {
    let source = probe_path(staging_dir, "link-source");
    let destination = probe_path(object_layout_dir, "link-destination");
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&source)?;
        file.write_all(b"probe")?;
        file.sync_all()?;
        fs::hard_link(&source, &destination)?;
        Ok::<_, std::io::Error>(fs::metadata(&destination)?.is_file())
    })()
    .unwrap_or(false);
    let _ = fs::remove_file(source);
    let _ = fs::remove_file(destination);
    result
}

fn probe_path(directory: &Path, purpose: &str) -> PathBuf {
    directory.join(format!(".synveil-probe-{purpose}-{}", Uuid::now_v7()))
}

fn validate_storage_root_sync(root: &Path) -> Result<PathBuf, ObjectStoreError> {
    let canonical = fs::canonicalize(root).map_err(map_io_error)?;
    let is_current_directory = std::env::current_dir()
        .ok()
        .is_some_and(|current| canonical_path_matches(&canonical, &current));
    let is_user_profile = ["HOME", "USERPROFILE"]
        .into_iter()
        .filter_map(std::env::var_os)
        .any(|profile| canonical_path_matches(&canonical, Path::new(&profile)));
    let is_build_workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .is_some_and(|workspace| canonical_path_matches(&canonical, workspace));

    if canonical.parent().is_none()
        || canonical.parent().is_some_and(|parent| parent == canonical)
        || is_current_directory
        || is_user_profile
        || is_build_workspace
    {
        return Err(ObjectStoreError::InvalidRequest);
    }
    Ok(canonical)
}

fn canonical_path_matches(canonical: &Path, candidate: &Path) -> bool {
    fs::canonicalize(candidate)
        .ok()
        .is_some_and(|candidate| candidate == canonical)
}

async fn require_directory_async(path: &Path) -> Result<(), ObjectStoreError> {
    let metadata = async_fs::symlink_metadata(path)
        .await
        .map_err(map_io_error)?;
    if is_redirected(&metadata) || !metadata.is_dir() {
        return Err(ObjectStoreError::StorageUnavailable);
    }
    Ok(())
}

async fn ensure_directory_async(path: &Path) -> Result<(), ObjectStoreError> {
    match async_fs::symlink_metadata(path).await {
        Ok(metadata) => {
            if is_redirected(&metadata) || !metadata.is_dir() {
                return Err(ObjectStoreError::StorageUnavailable);
            }
            Ok(())
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            async_fs::create_dir(path).await.map_err(map_io_error)?;
            require_directory_async(path).await
        }
        Err(error) => Err(map_io_error(error)),
    }
}

async fn require_file_async(path: &Path) -> Result<fs::Metadata, ObjectStoreError> {
    let metadata = async_fs::symlink_metadata(path)
        .await
        .map_err(map_io_error)?;
    if is_redirected(&metadata) || !metadata.is_file() {
        return Err(ObjectStoreError::StorageUnavailable);
    }
    Ok(metadata)
}

async fn read_small_file(path: &Path) -> Result<Vec<u8>, ObjectStoreError> {
    let file = File::open(path).await.map_err(map_io_error)?;
    let mut bytes = Vec::new();
    let mut limited = file.take(MAX_METADATA_BYTES + 1);
    limited
        .read_to_end(&mut bytes)
        .await
        .map_err(map_io_error)?;
    if bytes.len() as u64 > MAX_METADATA_BYTES {
        return Err(ObjectStoreError::StorageUnavailable);
    }
    Ok(bytes)
}

async fn write_new_file(path: &Path, content: &[u8]) -> Result<(), ObjectStoreError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await
        .map_err(map_io_error)?;
    file.write_all(content).await.map_err(map_io_error)?;
    file.sync_all().await.map_err(map_io_error)
}

/// Flush a managed directory using the platform's namespace-durability
/// primitive. Windows requires a directory handle opened with backup
/// semantics, and `FlushFileBuffers` requires write access to that handle.
/// `FILE_FLAG_OPEN_REPARSE_POINT` keeps this barrier from following a junction
/// or symlink if a managed path is replaced between validation and the flush.
fn sync_directory_sync(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

        return fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?
            .sync_all();
    }

    #[cfg(not(windows))]
    {
        fs::File::open(path)?.sync_all()
    }
}

async fn sync_directory(path: &Path) -> Result<(), ObjectStoreError> {
    #[cfg(windows)]
    {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || sync_directory_sync(&path))
            .await
            .map_err(|_| ObjectStoreError::StorageUnavailable)?
            .map_err(map_io_error)
    }

    #[cfg(not(windows))]
    {
        let file = File::open(path).await.map_err(map_io_error)?;
        file.sync_all().await.map_err(map_io_error)
    }
}

async fn verify_open_file_contents(
    file: &mut File,
    expected_length: u64,
    expected_sha256: Sha256Digest,
) -> Result<(), ObjectStoreError> {
    let (length, sha256) = hash_open_file_contents(file).await?;
    if length != expected_length || sha256 != expected_sha256 {
        return Err(ObjectStoreError::IntegrityMismatch);
    }
    Ok(())
}

async fn hash_open_file_contents(file: &mut File) -> Result<(u64, Sha256Digest), ObjectStoreError> {
    let mut buffer = vec![0_u8; STREAM_BUFFER_BYTES];
    let mut length = 0_u64;
    let mut hasher = Sha256::new();

    loop {
        let read = file.read(&mut buffer).await.map_err(map_io_error)?;
        if read == 0 {
            break;
        }
        length = length
            .checked_add(read as u64)
            .ok_or(ObjectStoreError::IntegrityMismatch)?;
        hasher.update(&buffer[..read]);
    }

    Ok((length, digest_from_hasher(hasher)))
}

fn parse_record(content: &[u8]) -> Result<BTreeMap<String, String>, ObjectStoreError> {
    let content = std::str::from_utf8(content).map_err(|_| ObjectStoreError::StorageUnavailable)?;
    let mut fields = BTreeMap::new();
    for line in content.lines() {
        let (name, value) = line
            .split_once('=')
            .ok_or(ObjectStoreError::StorageUnavailable)?;
        if !matches!(name, "kind" | "key" | "length" | "sha256" | "version")
            || fields.insert(name.to_owned(), value.to_owned()).is_some()
        {
            return Err(ObjectStoreError::StorageUnavailable);
        }
    }
    Ok(fields)
}

fn parse_length(value: Option<&String>) -> Result<u64, ObjectStoreError> {
    value
        .ok_or(ObjectStoreError::StorageUnavailable)?
        .parse()
        .map_err(|_| ObjectStoreError::StorageUnavailable)
}

fn parse_digest(value: Option<&String>) -> Result<Sha256Digest, ObjectStoreError> {
    value
        .ok_or(ObjectStoreError::StorageUnavailable)?
        .parse()
        .map_err(|_| ObjectStoreError::StorageUnavailable)
}

fn file_stream(file: File, length: u64, expected_sha256: Option<Sha256Digest>) -> ByteStream {
    boxed_stream(stream::unfold(
        Some(FileReadState {
            file,
            remaining: length,
            hasher: expected_sha256.map(|_| Sha256::new()),
            expected_sha256,
        }),
        |state| async move {
            let mut state = state?;
            if state.remaining == 0 {
                if let (Some(hasher), Some(expected)) =
                    (state.hasher.take(), state.expected_sha256.take())
                    && digest_from_hasher(hasher) != expected
                {
                    return Some((Err(ObjectStoreError::IntegrityMismatch), None));
                }
                return None;
            }

            let read_size = state.remaining.min(STREAM_BUFFER_BYTES as u64) as usize;
            let mut buffer = vec![0_u8; read_size];
            match state.file.read(&mut buffer).await {
                Ok(0) => Some((Err(ObjectStoreError::IntegrityMismatch), None)),
                Ok(read) => {
                    buffer.truncate(read);
                    state.remaining -= read as u64;
                    if let Some(hasher) = state.hasher.as_mut() {
                        hasher.update(&buffer);
                    }
                    Some((Ok(Bytes::from(buffer)), Some(state)))
                }
                Err(error) => Some((Err(map_io_error(error)), None)),
            }
        },
    ))
}

fn digest_from_hasher(hasher: Sha256) -> Sha256Digest {
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 32];
    bytes.copy_from_slice(&digest);
    Sha256Digest::from_bytes(bytes)
}

fn hex_digest(digest: &[u8]) -> String {
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

#[cfg(windows)]
fn is_redirected(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_redirected(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn validate_object_key_at_adapter_boundary(key: &ObjectKey) -> Result<(), ObjectStoreError> {
    let value = key.as_str();
    if value.is_empty()
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains("//")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_'))
        || value
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(ObjectStoreError::InvalidKey);
    }
    Ok(())
}

fn validate_staging_handle_at_adapter_boundary(
    handle: &StagingHandle,
) -> Result<(), ObjectStoreError> {
    if handle.as_str().is_empty()
        || !handle
            .as_str()
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ObjectStoreError::InvalidKey);
    }
    Ok(())
}

fn map_io_error(error: std::io::Error) -> ObjectStoreError {
    match error.kind() {
        ErrorKind::AlreadyExists => ObjectStoreError::AlreadyExists,
        ErrorKind::NotFound => ObjectStoreError::NotFound,
        _ => ObjectStoreError::StorageUnavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_broad_roots_are_rejected_without_initialization() {
        let current = std::env::current_dir().expect("current directory");
        assert_eq!(
            validate_storage_root_sync(&current),
            Err(ObjectStoreError::InvalidRequest)
        );

        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace root");
        assert_eq!(
            validate_storage_root_sync(workspace),
            Err(ObjectStoreError::InvalidRequest)
        );

        let filesystem_root = fs::canonicalize(workspace)
            .expect("canonical workspace")
            .ancestors()
            .last()
            .expect("filesystem root")
            .to_path_buf();
        assert_eq!(
            validate_storage_root_sync(&filesystem_root),
            Err(ObjectStoreError::InvalidRequest)
        );

        for variable in ["HOME", "USERPROFILE"] {
            let Some(profile) = std::env::var_os(variable) else {
                continue;
            };
            if fs::canonicalize(&profile).is_ok() {
                assert_eq!(
                    validate_storage_root_sync(Path::new(&profile)),
                    Err(ObjectStoreError::InvalidRequest)
                );
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_directory_sync_flushes_a_writable_directory_handle() {
        let directory =
            std::env::temp_dir().join(format!("synveil-directory-sync-test-{}", Uuid::now_v7()));
        fs::create_dir_all(&directory).expect("create directory-sync fixture");
        let child = directory.join("child");
        fs::write(&child, b"directory flush fixture").expect("write directory-sync fixture");

        let sync_result = sync_directory_sync(&directory);
        let read_result = fs::read(&child);
        let cleanup_result = fs::remove_dir_all(&directory);

        sync_result.expect("flush directory metadata");
        assert_eq!(
            read_result.expect("read directory-sync fixture"),
            b"directory flush fixture"
        );
        cleanup_result.expect("remove directory-sync fixture");
    }
}
