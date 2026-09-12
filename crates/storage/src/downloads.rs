//! Transport-neutral owner-authorized immutable content reads.
//!
//! This service resolves logical metadata before handing one trusted opaque key
//! to the existing `ObjectStore` port. It deliberately contains no HTTP range
//! parsing, response headers, public-link behavior, or local-filesystem calls.

use std::{fmt, pin::Pin, sync::Arc};

use futures_util::{Stream, StreamExt};
use synveil_core::{FileVersionId, NodeId, ObjectId, Revision, Sha256Digest, Timestamp, UserId};
use synveil_metadata::{
    AuthorizedContent, ContentReadMetadataBackend, ContentReadResolution, MappingError,
    MetadataError,
};

use crate::{
    ByteRange, ObjectKey, ObjectRead, ObjectStore, ObjectStoreError, StorageAvailability,
    StorageBackendKind, StorageCapabilities, StorageCapability,
};

/// A bounded content stream whose failures remain in the application error
/// vocabulary rather than exposing object-store details to a transport layer.
pub type ContentByteStream =
    Pin<Box<dyn Stream<Item = Result<bytes::Bytes, ContentReadError>> + Send + 'static>>;

/// Box a runtime-neutral content stream for a test or a future transport
/// adapter. This helper does not buffer bytes.
pub fn boxed_content_stream<S>(stream: S) -> ContentByteStream
where
    S: Stream<Item = Result<bytes::Bytes, ContentReadError>> + Send + 'static,
{
    Box::pin(stream)
}

/// Safe immutable content metadata resolved before a byte stream is opened.
///
/// This projection lets a transport validate a requested range and conditional
/// validator without opening storage bytes. It deliberately contains no
/// storage key, path, staging handle, replica state, or backend credential.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentMetadata {
    node_id: NodeId,
    file_version_id: FileVersionId,
    object_id: ObjectId,
    length: u64,
    sha256: Sha256Digest,
    file_version_revision: Revision,
    committed_at: Timestamp,
}

impl ContentMetadata {
    fn from_authorized(content: &AuthorizedContent) -> Self {
        Self {
            node_id: content.node_id(),
            file_version_id: content.file_version_id(),
            object_id: content.object_id(),
            length: content.length(),
            sha256: content.sha256(),
            file_version_revision: content.file_version_revision(),
            committed_at: content.committed_at(),
        }
    }

    #[must_use]
    pub const fn node_id(self) -> NodeId {
        self.node_id
    }

    #[must_use]
    pub const fn file_version_id(self) -> FileVersionId {
        self.file_version_id
    }

    #[must_use]
    pub const fn object_id(self) -> ObjectId {
        self.object_id
    }

    #[must_use]
    pub const fn length(self) -> u64 {
        self.length
    }

    #[must_use]
    pub const fn sha256(self) -> Sha256Digest {
        self.sha256
    }

    #[must_use]
    pub const fn file_version_revision(self) -> Revision {
        self.file_version_revision
    }

    #[must_use]
    pub const fn committed_at(self) -> Timestamp {
        self.committed_at
    }
}

/// Safe immutable content metadata plus a bounded byte stream. Physical keys,
/// backend versions, staging handles, and paths are intentionally absent.
pub struct ContentDescriptor {
    node_id: NodeId,
    file_version_id: FileVersionId,
    object_id: ObjectId,
    length: u64,
    sha256: Sha256Digest,
    file_version_revision: Revision,
    committed_at: Timestamp,
    range: Option<ByteRange>,
    body: ContentByteStream,
}

impl ContentDescriptor {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    fn new(content: &AuthorizedContent, range: Option<ByteRange>, body: ContentByteStream) -> Self {
        Self {
            node_id: content.node_id(),
            file_version_id: content.file_version_id(),
            object_id: content.object_id(),
            length: content.length(),
            sha256: content.sha256(),
            file_version_revision: content.file_version_revision(),
            committed_at: content.committed_at(),
            range,
            body,
        }
    }

    #[must_use]
    pub const fn node_id(&self) -> NodeId {
        self.node_id
    }

    #[must_use]
    pub const fn file_version_id(&self) -> FileVersionId {
        self.file_version_id
    }

    #[must_use]
    pub const fn object_id(&self) -> ObjectId {
        self.object_id
    }

    /// Canonical full-object length, even when the stream represents a range.
    #[must_use]
    pub const fn length(&self) -> u64 {
        self.length
    }

    #[must_use]
    pub const fn sha256(&self) -> Sha256Digest {
        self.sha256
    }

    #[must_use]
    pub const fn file_version_revision(&self) -> Revision {
        self.file_version_revision
    }

    #[must_use]
    pub const fn committed_at(&self) -> Timestamp {
        self.committed_at
    }

    #[must_use]
    pub const fn range(&self) -> Option<ByteRange> {
        self.range
    }

    /// Number of bytes the returned stream represents.
    #[must_use]
    pub const fn stream_length(&self) -> u64 {
        match self.range {
            Some(range) => range.length(),
            None => self.length,
        }
    }

    #[must_use]
    pub fn into_stream(self) -> ContentByteStream {
        self.body
    }
}

impl fmt::Debug for ContentDescriptor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContentDescriptor")
            .field("node_id", &self.node_id)
            .field("file_version_id", &self.file_version_id)
            .field("object_id", &self.object_id)
            .field("length", &self.length)
            .field("sha256", &"<redacted>")
            .field("file_version_revision", &self.file_version_revision)
            .field("committed_at", &self.committed_at)
            .field("range", &self.range)
            .finish_non_exhaustive()
    }
}

/// Stable, transport-neutral failures for authorized content reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentReadError {
    ContentNotFound,
    NotAFile,
    VersionNotFound,
    ContentUnavailable,
    IntegrityMismatch,
    InvalidRange,
    StorageUnavailable,
}

impl ContentReadError {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ContentNotFound => "content_not_found",
            Self::NotAFile => "not_a_file",
            Self::VersionNotFound => "version_not_found",
            Self::ContentUnavailable => "content_unavailable",
            Self::IntegrityMismatch => "integrity_mismatch",
            Self::InvalidRange => "invalid_range",
            Self::StorageUnavailable => "storage_unavailable",
        }
    }

    #[must_use]
    pub const fn retryable(self) -> bool {
        matches!(self, Self::ContentUnavailable | Self::StorageUnavailable)
    }
}

impl fmt::Display for ContentReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::error::Error for ContentReadError {}

/// PostgreSQL/ObjectStore-coordinating immutable content-read service.
#[derive(Clone)]
pub struct ContentReadApplicationService {
    metadata: Arc<dyn ContentReadMetadataBackend>,
    object_store: Arc<dyn ObjectStore>,
}

impl ContentReadApplicationService {
    #[must_use]
    pub fn new(
        metadata: Arc<dyn ContentReadMetadataBackend>,
        object_store: Arc<dyn ObjectStore>,
    ) -> Self {
        Self {
            metadata,
            object_store,
        }
    }

    /// Return whether the configured verified backend can serve logical byte
    /// ranges. Full reads may still be supported when this is false.
    #[must_use]
    pub fn supports_range_reads(&self) -> bool {
        let capabilities = self.object_store.capabilities();
        capabilities.availability() == StorageAvailability::Available
            && capabilities.supports(StorageCapability::Checksumming)
            && capabilities.supports(StorageCapability::RangeReads)
            && backend_kind(capabilities).is_some()
    }

    /// Resolve current content metadata without opening storage bytes.
    pub async fn current_content_metadata(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
    ) -> Result<ContentMetadata, ContentReadError> {
        let content = self
            .resolve_current_content(owner_user_id, node_id, false)
            .await?;
        Ok(ContentMetadata::from_authorized(&content))
    }

    /// Resolve immutable historical content metadata without opening storage
    /// bytes.
    pub async fn file_version_content_metadata(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
    ) -> Result<ContentMetadata, ContentReadError> {
        let content = self
            .resolve_file_version_content(owner_user_id, file_version_id, false)
            .await?;
        Ok(ContentMetadata::from_authorized(&content))
    }

    /// Open the current immutable content version of an active owner-visible
    /// file. The resolved version is bound before any bytes are streamed.
    pub async fn open_current_file_content(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
    ) -> Result<ContentDescriptor, ContentReadError> {
        let content = self
            .resolve_current_content(owner_user_id, node_id, false)
            .await?;
        self.open_resolved_content(content, None).await
    }

    /// Open an immutable historical version after owner and active-node
    /// authorization are resolved from canonical metadata.
    pub async fn open_file_version_content(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
    ) -> Result<ContentDescriptor, ContentReadError> {
        let content = self
            .resolve_file_version_content(owner_user_id, file_version_id, false)
            .await?;
        self.open_resolved_content(content, None).await
    }

    /// Open an existing logical plaintext range of the current immutable file
    /// content. Callers provide the already-parsed `ByteRange`; no HTTP range
    /// syntax is accepted or interpreted here.
    pub async fn open_file_content_range(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
        range: ByteRange,
    ) -> Result<ContentDescriptor, ContentReadError> {
        let content = self
            .resolve_current_content(owner_user_id, node_id, true)
            .await?;
        if range.end_exclusive() > content.length() {
            return Err(ContentReadError::InvalidRange);
        }
        self.open_resolved_content(content, Some(range)).await
    }

    /// Open one logical range from an immutable historical version after owner
    /// and active-node authorization have been resolved.
    pub async fn open_file_version_content_range(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
        range: ByteRange,
    ) -> Result<ContentDescriptor, ContentReadError> {
        let content = self
            .resolve_file_version_content(owner_user_id, file_version_id, true)
            .await?;
        if range.end_exclusive() > content.length() {
            return Err(ContentReadError::InvalidRange);
        }
        self.open_resolved_content(content, Some(range)).await
    }

    async fn resolve_current_content(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
        needs_range: bool,
    ) -> Result<AuthorizedContent, ContentReadError> {
        let backend_kind = self.require_read_capabilities(needs_range)?;
        let resolution = self
            .metadata
            .resolve_current_content(owner_user_id, node_id, backend_kind)
            .await
            .map_err(map_metadata_error)?;
        match resolution {
            ContentReadResolution::NotFound => Err(ContentReadError::ContentNotFound),
            ContentReadResolution::NotAFile => Err(ContentReadError::NotAFile),
            ContentReadResolution::ContentUnavailable => Err(ContentReadError::ContentUnavailable),
            ContentReadResolution::Found(content) => Ok(content),
        }
    }

    async fn resolve_file_version_content(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
        needs_range: bool,
    ) -> Result<AuthorizedContent, ContentReadError> {
        let backend_kind = self.require_read_capabilities(needs_range)?;
        let resolution = self
            .metadata
            .resolve_file_version_content(owner_user_id, file_version_id, backend_kind)
            .await
            .map_err(map_metadata_error)?;
        match resolution {
            ContentReadResolution::NotFound | ContentReadResolution::NotAFile => {
                Err(ContentReadError::VersionNotFound)
            }
            ContentReadResolution::ContentUnavailable => Err(ContentReadError::ContentUnavailable),
            ContentReadResolution::Found(content) => Ok(content),
        }
    }

    async fn open_resolved_content(
        &self,
        content: AuthorizedContent,
        range: Option<ByteRange>,
    ) -> Result<ContentDescriptor, ContentReadError> {
        let key = ObjectKey::new(content.storage_key().to_owned())
            .map_err(|_| ContentReadError::IntegrityMismatch)?;
        let read = match range {
            Some(range) => self.object_store.range_read(&key, range).await,
            None => self.object_store.get(&key).await,
        }
        .map_err(map_storage_error)?;
        validate_object_read(&content, &key, &read, range)?;

        let body = boxed_content_stream(
            read.into_stream()
                .map(|chunk| chunk.map_err(map_storage_error)),
        );
        Ok(ContentDescriptor::new(&content, range, body))
    }

    fn require_read_capabilities(
        &self,
        needs_range: bool,
    ) -> Result<&'static str, ContentReadError> {
        let capabilities = self.object_store.capabilities();
        if capabilities.availability() != StorageAvailability::Available
            || !capabilities.supports(StorageCapability::Checksumming)
            || (needs_range && !capabilities.supports(StorageCapability::RangeReads))
        {
            return Err(ContentReadError::StorageUnavailable);
        }
        backend_kind(capabilities).ok_or(ContentReadError::StorageUnavailable)
    }
}

fn validate_object_read(
    content: &AuthorizedContent,
    expected_key: &ObjectKey,
    read: &ObjectRead,
    expected_range: Option<ByteRange>,
) -> Result<(), ContentReadError> {
    let metadata = read.metadata();
    let expected_sha256 = content.sha256();
    if metadata.key() != expected_key
        || metadata.length() != content.length()
        || metadata.sha256() != Some(&expected_sha256)
        || read.range() != expected_range
    {
        return Err(ContentReadError::IntegrityMismatch);
    }
    Ok(())
}

fn backend_kind(capabilities: StorageCapabilities) -> Option<&'static str> {
    match capabilities.backend() {
        StorageBackendKind::LocalFilesystem => Some("LOCAL_FILESYSTEM"),
        StorageBackendKind::ObjectStore => Some("OBJECT_STORE"),
        StorageBackendKind::Unknown => None,
    }
}

fn map_metadata_error(error: MetadataError) -> ContentReadError {
    match error {
        MetadataError::Database(_) | MetadataError::CapacityUnavailable => {
            ContentReadError::StorageUnavailable
        }
        MetadataError::Mapping(MappingError::RelationMismatch { .. })
        | MetadataError::Mapping(MappingError::InvalidDigest { .. })
        | MetadataError::Mapping(MappingError::InvalidId { .. })
        | MetadataError::Mapping(MappingError::InvalidDecimal { .. })
        | MetadataError::Mapping(MappingError::InvalidEnum { .. })
        | MetadataError::Mapping(MappingError::InvalidTimestamp { .. })
        | MetadataError::Mapping(MappingError::TimestampPrecisionLoss { .. })
        | MetadataError::Mapping(MappingError::InvalidName { .. })
        | MetadataError::Mapping(MappingError::InvalidLogin { .. })
        | MetadataError::Mapping(MappingError::RevisionConflict { .. })
        | MetadataError::Mapping(MappingError::Domain(_)) => ContentReadError::IntegrityMismatch,
    }
}

fn map_storage_error(error: ObjectStoreError) -> ContentReadError {
    match error {
        ObjectStoreError::NotFound | ObjectStoreError::StagingNotFound => {
            ContentReadError::ContentUnavailable
        }
        ObjectStoreError::IntegrityMismatch
        | ObjectStoreError::InvalidKey
        | ObjectStoreError::InvalidRequest
        | ObjectStoreError::AlreadyExists
        | ObjectStoreError::PreconditionFailed
        | ObjectStoreError::StagingConflict => ContentReadError::IntegrityMismatch,
        ObjectStoreError::InvalidRange => ContentReadError::InvalidRange,
        ObjectStoreError::StorageUnavailable | ObjectStoreError::UnsupportedCapability(_) => {
            ContentReadError::StorageUnavailable
        }
    }
}
