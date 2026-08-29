//! Transport-neutral resumable upload application service.
//!
//! The service coordinates PostgreSQL upload intent/progress with the
//! backend-neutral ObjectStore port. It never opens a filesystem path, talks to
//! Axum, or exposes a staging handle/key in its public session projection.

use std::{fmt, pin::Pin, sync::Arc, time::Duration};

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use sha2::{Digest, Sha256};
use synveil_core::{
    LibraryId, LogicalName, NodeId, ObjectId, ObjectReplicaId, OutboundIntentId, Revision,
    Sha256Digest, Timestamp, UploadOperation, UploadSessionId, UploadSessionState, UserId,
};
use synveil_metadata::{
    MetadataError, NewUploadSession, UploadClaim, UploadCleanupCandidate, UploadCompletion,
    UploadDurabilityReceipt, UploadFinalization, UploadMetadataBackend, UploadSessionRecord,
};
use synveil_object_store::{
    CapabilitySupport, IntegrityExpectation, ObjectKey, ObjectMetadata, ObjectStore,
    ObjectStoreError, StagingHandle, StorageAvailability, StorageCapabilities, StorageCapability,
};
use time::Duration as TimeDuration;

const DEFAULT_MAX_OBJECT_SIZE: u64 = 1 << 40;
const DEFAULT_MAX_CHUNK_SIZE: u64 = 8 << 20;
const DEFAULT_MAX_ACTIVE_SESSIONS: u32 = 8;
const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const DEFAULT_LEASE_TTL: Duration = Duration::from_secs(30);

/// Centralized limits for the upload application boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadLimits {
    pub max_object_size: u64,
    pub max_chunk_size: u64,
    pub max_active_sessions: u32,
    pub session_ttl: Duration,
    pub lease_ttl: Duration,
}

impl Default for UploadLimits {
    fn default() -> Self {
        Self {
            max_object_size: DEFAULT_MAX_OBJECT_SIZE,
            max_chunk_size: DEFAULT_MAX_CHUNK_SIZE,
            max_active_sessions: DEFAULT_MAX_ACTIVE_SESSIONS,
            session_ttl: DEFAULT_SESSION_TTL,
            lease_ttl: DEFAULT_LEASE_TTL,
        }
    }
}

impl UploadLimits {
    pub fn validate(self) -> Result<Self, UploadConfigurationError> {
        if self.max_object_size == 0
            || self.max_chunk_size == 0
            || self.max_chunk_size > self.max_object_size
            || self.max_active_sessions == 0
            || self.session_ttl.is_zero()
            || self.lease_ttl.is_zero()
        {
            return Err(UploadConfigurationError::InvalidLimits);
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadConfigurationError {
    InvalidLimits,
}

impl fmt::Display for UploadConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("upload limits are invalid")
    }
}

impl std::error::Error for UploadConfigurationError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UploadTargetRequest {
    CreateFile {
        library_id: LibraryId,
        parent_node_id: NodeId,
        name: LogicalName,
    },
    ReplaceContent {
        library_id: LibraryId,
        node_id: NodeId,
        expected_revision: Revision,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateUploadSessionRequest {
    pub idempotency_key: OutboundIntentId,
    pub owner_user_id: UserId,
    pub target: UploadTargetRequest,
    pub expected_length: u64,
    pub expected_sha256: Option<Sha256Digest>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UploadTargetView {
    CreateFile {
        library_id: LibraryId,
        parent_node_id: NodeId,
        node_id: NodeId,
        name: LogicalName,
    },
    ReplaceContent {
        library_id: LibraryId,
        node_id: NodeId,
        expected_revision: Revision,
    },
}

/// Safe reconnect/status projection. It contains no physical path, staging
/// filename, ObjectKey, or backend credential.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadSessionView {
    pub id: UploadSessionId,
    pub owner_user_id: UserId,
    pub target: UploadTargetView,
    pub expected_length: u64,
    pub expected_sha256: Option<Sha256Digest>,
    pub received_bytes: u64,
    pub state: UploadSessionState,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub expires_at: Timestamp,
    pub last_error_code: Option<String>,
    pub terminal_failure_code: Option<String>,
    pub completion: Option<UploadCompletion>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadProgress {
    pub session_id: UploadSessionId,
    pub received_bytes: u64,
}

/// Transport-neutral byte stream accepted by the upload application service.
///
/// HTTP adapters may map their bounded body stream into this type without
/// aggregating the complete request body. Stream errors deliberately use the
/// same safe upload error vocabulary as the rest of the application boundary.
pub type UploadByteStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, UploadError>> + Send + 'static>>;

#[must_use]
pub fn boxed_upload_stream<S>(stream: S) -> UploadByteStream
where
    S: Stream<Item = Result<Bytes, UploadError>> + Send + 'static,
{
    Box::pin(stream)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadCleanupWork {
    pub session_id: UploadSessionId,
    pub state: UploadSessionState,
    pub staging_handle: StagingHandle,
    pub object_key: ObjectKey,
    pub lease_generation: u64,
}

/// A deployment may provide a stronger quota/disk/capacity admission adapter.
/// The default is intentionally explicit and reports that strong capacity
/// admission is not yet implemented by this phase.
#[async_trait]
pub trait CapacityAdmission: Send + Sync {
    async fn admit(
        &self,
        owner_user_id: UserId,
        library_id: LibraryId,
        expected_length: u64,
    ) -> Result<(), UploadError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoCapacityAdmission;

#[async_trait]
impl CapacityAdmission for NoCapacityAdmission {
    async fn admit(
        &self,
        _owner_user_id: UserId,
        _library_id: LibraryId,
        _expected_length: u64,
    ) -> Result<(), UploadError> {
        Ok(())
    }
}

/// Stable application errors. Values never contain SQL, host paths, raw
/// storage handles, object bodies, or client-controlled names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UploadError {
    NotFound,
    InvalidRequest,
    InvalidOffset { current_offset: u64 },
    ChunkTooLarge,
    SizeMismatch,
    HashMismatch,
    InvalidState { state: UploadSessionState },
    UploadExpired,
    VersionConflict { current_revision: Option<Revision> },
    CompletionConflict,
    InProgress,
    StorageUnavailable,
    CapacityUnavailable,
    DatabaseUnavailable,
    InvalidPersistedData,
    Failed { code: String },
}

impl UploadError {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotFound => "upload_not_found",
            Self::InvalidRequest => "invalid_upload_request",
            Self::InvalidOffset { .. } => "invalid_offset",
            Self::ChunkTooLarge => "chunk_too_large",
            Self::SizeMismatch => "size_mismatch",
            Self::HashMismatch => "hash_mismatch",
            Self::InvalidState { .. } => "invalid_state",
            Self::UploadExpired => "upload_expired",
            Self::VersionConflict { .. } => "version_conflict",
            Self::CompletionConflict => "completion_conflict",
            Self::InProgress => "upload_in_progress",
            Self::StorageUnavailable => "storage_unavailable",
            Self::CapacityUnavailable => "capacity_unavailable",
            Self::DatabaseUnavailable => "database_unavailable",
            Self::InvalidPersistedData => "invalid_persisted_upload_state",
            Self::Failed { .. } => "upload_failed",
        }
    }

    #[must_use]
    pub const fn retryable(&self) -> bool {
        matches!(
            self,
            Self::InProgress
                | Self::StorageUnavailable
                | Self::DatabaseUnavailable
                | Self::CapacityUnavailable
        )
    }
}

impl fmt::Display for UploadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::error::Error for UploadError {}

/// PostgreSQL/ObjectStore-coordinating upload application service.
#[derive(Clone)]
pub struct UploadApplicationService {
    metadata: Arc<dyn UploadMetadataBackend>,
    object_store: Arc<dyn ObjectStore>,
    capacity: Arc<dyn CapacityAdmission>,
    limits: UploadLimits,
}

impl UploadApplicationService {
    pub fn new(
        metadata: Arc<dyn UploadMetadataBackend>,
        object_store: Arc<dyn ObjectStore>,
        limits: UploadLimits,
    ) -> Result<Self, UploadConfigurationError> {
        Ok(Self {
            metadata,
            object_store,
            capacity: Arc::new(NoCapacityAdmission),
            limits: limits.validate()?,
        })
    }

    #[must_use]
    pub fn with_capacity_admission(mut self, capacity: Arc<dyn CapacityAdmission>) -> Self {
        self.capacity = capacity;
        self
    }

    #[must_use]
    pub const fn limits(&self) -> UploadLimits {
        self.limits
    }

    pub async fn create_upload_session(
        &self,
        request: CreateUploadSessionRequest,
    ) -> Result<UploadSessionView, UploadError> {
        let session_id = UploadSessionId::try_from_uuid(*request.idempotency_key.as_uuid())
            .map_err(|_| UploadError::InvalidRequest)?;
        if let Some(existing) = self
            .metadata
            .find_upload_session(request.owner_user_id, session_id)
            .await
            .map_err(map_metadata_error)?
        {
            if !upload_request_matches_record(&request, &existing) {
                return Err(UploadError::CompletionConflict);
            }
            return view_from_record(&existing);
        }
        self.validate_capabilities(self.object_store.capabilities())?;
        if request.expected_length > self.limits.max_object_size {
            return Err(UploadError::SizeMismatch);
        }
        self.capacity
            .admit(
                request.owner_user_id,
                request.target.library_id(),
                request.expected_length,
            )
            .await?;
        let object_id = ObjectId::new();
        let object_replica_id = ObjectReplicaId::new();
        let object_key = ObjectKey::new(format!("objects/v1/{object_id}"))
            .map_err(|_| UploadError::InvalidPersistedData)?;
        let staging_handle = self
            .object_store
            .begin_staged_write()
            .await
            .map_err(map_storage_error)?;
        let now = now();
        let expires_at = add_duration(now, self.limits.session_ttl)?;
        let (
            operation,
            target_node_id,
            target_parent_node_id,
            target_name,
            expected_revision,
            library_id,
        ) = match request.target {
            UploadTargetRequest::CreateFile {
                library_id,
                parent_node_id,
                name,
            } => (
                UploadOperation::CreateFile,
                NodeId::new(),
                Some(parent_node_id),
                Some(name),
                None,
                library_id,
            ),
            UploadTargetRequest::ReplaceContent {
                library_id,
                node_id,
                expected_revision,
            } => (
                UploadOperation::ReplaceContent,
                node_id,
                None,
                None,
                Some(expected_revision),
                library_id,
            ),
        };
        let input = NewUploadSession {
            id: session_id,
            owner_user_id: request.owner_user_id,
            library_id,
            operation,
            target_node_id,
            target_parent_node_id,
            target_name,
            expected_node_revision: expected_revision,
            expected_length: request.expected_length,
            expected_sha256: request.expected_sha256,
            object_id,
            object_replica_id,
            object_key: object_key.as_str().to_owned(),
            staging_handle: staging_handle.as_str().to_owned(),
            max_active_sessions: self.limits.max_active_sessions,
            created_at: now,
            expires_at,
        };
        let record = match self.metadata.create_upload_session(input).await {
            Ok(record) => {
                if record.staging_handle != staging_handle.as_str() {
                    let _ = self.object_store.abort_staged(&staging_handle).await;
                }
                record
            }
            Err(error) => {
                let _ = self.object_store.abort_staged(&staging_handle).await;
                return Err(map_metadata_error(error));
            }
        };
        view_from_record(&record)
    }

    pub async fn get_upload_session(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
    ) -> Result<UploadSessionView, UploadError> {
        let record = self.load_owned(owner_user_id, session_id).await?;
        let record = self.expire_if_needed(record).await?;
        let record = if record.state == UploadSessionState::Open {
            let handle = parse_staging_handle(&record.staging_handle)?;
            match self.object_store.staging_progress(&handle).await {
                Ok(progress) => {
                    self.reconcile_progress(owner_user_id, record, progress.length())
                        .await?
                }
                // This is the narrow crash boundary after create-only
                // promotion and before its durability/finalization metadata.
                // Completion owns that reconciliation; a complete recorded
                // length remains the only safe status projection here.
                Err(ObjectStoreError::StagingNotFound)
                    if record.bytes_received == record.expected_length =>
                {
                    record
                }
                Err(error) => return Err(map_storage_error(error)),
            }
        } else {
            record
        };
        view_from_record(&record)
    }

    pub async fn append_upload_chunk(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        expected_offset: u64,
        chunk: Bytes,
    ) -> Result<UploadProgress, UploadError> {
        if chunk.is_empty() {
            return Err(UploadError::InvalidRequest);
        }
        let chunk_length = u64::try_from(chunk.len()).map_err(|_| UploadError::ChunkTooLarge)?;
        if chunk_length > self.limits.max_chunk_size {
            return Err(UploadError::ChunkTooLarge);
        }
        let record = self
            .expire_if_needed(self.load_owned(owner_user_id, session_id).await?)
            .await?;
        ensure_open(&record)?;
        let handle = parse_staging_handle(&record.staging_handle)?;
        let progress = self
            .object_store
            .staging_progress(&handle)
            .await
            .map_err(map_storage_error)?;
        let record = self
            .reconcile_progress(owner_user_id, record, progress.length())
            .await?;
        ensure_open(&record)?;
        if expected_offset != record.bytes_received {
            return Err(UploadError::InvalidOffset {
                current_offset: record.bytes_received,
            });
        }
        let progress = self
            .object_store
            .append_staged(&handle, expected_offset, chunk, record.expected_length)
            .await;
        let progress = match progress {
            Ok(progress) => progress,
            Err(ObjectStoreError::PreconditionFailed) => {
                let current = self
                    .object_store
                    .staging_progress(&handle)
                    .await
                    .map_err(map_storage_error)?;
                return Err(UploadError::InvalidOffset {
                    current_offset: current.length(),
                });
            }
            Err(ObjectStoreError::IntegrityMismatch) => return Err(UploadError::SizeMismatch),
            Err(error) => return Err(map_storage_error(error)),
        };
        let updated = self
            .metadata
            .record_upload_progress(
                owner_user_id,
                session_id,
                expected_offset,
                progress.length(),
                now(),
            )
            .await
            .map_err(map_metadata_error)?;
        if updated.state != UploadSessionState::Open {
            return Err(UploadError::InvalidState {
                state: updated.state,
            });
        }
        if updated.bytes_received < progress.length() {
            return Err(UploadError::DatabaseUnavailable);
        }
        Ok(UploadProgress {
            session_id,
            received_bytes: updated.bytes_received,
        })
    }

    /// Append one HTTP/request chunk from a streaming transport.
    ///
    /// Each non-empty transport frame is durably appended through the existing
    /// exact-offset state machine, so a disconnect after any accepted frame is
    /// recoverable by querying the persisted session offset. The aggregate
    /// request size is capped by the service's configured chunk limit; no
    /// transport frame or complete request is collected into an unbounded
    /// buffer.
    pub async fn append_upload_stream(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        expected_offset: u64,
        mut stream: UploadByteStream,
    ) -> Result<UploadProgress, UploadError> {
        let mut request_bytes = 0_u64;
        let mut current_offset = expected_offset;
        let mut last_progress = None;

        while let Some(frame) = stream.next().await {
            let frame = frame?;
            if frame.is_empty() {
                continue;
            }

            let frame_length =
                u64::try_from(frame.len()).map_err(|_| UploadError::ChunkTooLarge)?;
            request_bytes = request_bytes
                .checked_add(frame_length)
                .ok_or(UploadError::ChunkTooLarge)?;
            if request_bytes > self.limits.max_chunk_size {
                return Err(UploadError::ChunkTooLarge);
            }

            let progress = self
                .append_upload_chunk(owner_user_id, session_id, current_offset, frame)
                .await?;
            current_offset = progress.received_bytes;
            last_progress = Some(progress);
        }

        last_progress.ok_or(UploadError::InvalidRequest)
    }

    pub async fn complete_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
    ) -> Result<UploadCompletion, UploadError> {
        let initial = self.load_owned(owner_user_id, session_id).await?;
        if let Some(completion) = initial.completion.clone()
            && initial.state == UploadSessionState::Committed
        {
            return Ok(completion);
        }
        let initial = if initial.state == UploadSessionState::Open {
            self.expire_if_needed(initial).await?
        } else {
            initial
        };
        if initial.state == UploadSessionState::Committed {
            return initial.completion.ok_or(UploadError::InvalidPersistedData);
        }
        if initial.state == UploadSessionState::Expired {
            return Err(UploadError::UploadExpired);
        }
        if initial.state == UploadSessionState::Aborted {
            return Err(UploadError::InvalidState {
                state: initial.state,
            });
        }
        if initial.state == UploadSessionState::Failed {
            return Err(failed_error(&initial));
        }

        let initial = if initial.state == UploadSessionState::Open {
            let handle = parse_staging_handle(&initial.staging_handle)?;
            match self.object_store.staging_progress(&handle).await {
                Ok(progress) => {
                    self.reconcile_progress(owner_user_id, initial, progress.length())
                        .await?
                }
                Err(ObjectStoreError::StagingNotFound) => {
                    // Completion recovery below will inspect the preassigned
                    // final key. This is the crash boundary after promotion
                    // and before the metadata durability receipt.
                    if initial.bytes_received != initial.expected_length {
                        return Err(UploadError::StorageUnavailable);
                    }
                    initial
                }
                Err(error) => return Err(map_storage_error(error)),
            }
        } else {
            initial
        };
        if initial.state == UploadSessionState::Open
            && initial.bytes_received != initial.expected_length
        {
            return Err(UploadError::SizeMismatch);
        }

        let lease_until = add_duration(now(), self.limits.lease_ttl)?;
        let claim = self
            .metadata
            .claim_upload(owner_user_id, session_id, now(), lease_until)
            .await
            .map_err(map_metadata_error)?;
        let record = match claim {
            UploadClaim::Busy(_) => return Err(UploadError::InProgress),
            UploadClaim::Terminal(record) => {
                if record.state == UploadSessionState::Committed {
                    return record.completion.ok_or(UploadError::InvalidPersistedData);
                }
                if record.state == UploadSessionState::Expired {
                    return Err(UploadError::UploadExpired);
                }
                if record.state == UploadSessionState::Failed {
                    return Err(failed_error(&record));
                }
                return Err(UploadError::InvalidState {
                    state: record.state,
                });
            }
            UploadClaim::Acquired(record) => record,
        };

        let claimed_committing = record.state == UploadSessionState::Committing;
        let record = if record.state == UploadSessionState::Verifying && record.durability.is_none()
        {
            self.verify_and_promote(owner_user_id, record).await?
        } else {
            record
        };
        if record.state != UploadSessionState::Committing {
            return Err(UploadError::InvalidState {
                state: record.state,
            });
        }
        let commit_record = if claimed_committing {
            record
        } else {
            let lease_until = add_duration(now(), self.limits.lease_ttl)?;
            let commit_claim = self
                .metadata
                .claim_upload(owner_user_id, session_id, now(), lease_until)
                .await
                .map_err(map_metadata_error)?;
            match commit_claim {
                UploadClaim::Busy(_) => return Err(UploadError::InProgress),
                UploadClaim::Terminal(record) => {
                    if record.state == UploadSessionState::Committed {
                        return record.completion.ok_or(UploadError::InvalidPersistedData);
                    }
                    return Err(failed_error(&record));
                }
                UploadClaim::Acquired(record) => record,
            }
        };
        if commit_record.state != UploadSessionState::Committing {
            return Err(UploadError::InvalidState {
                state: commit_record.state,
            });
        }
        match self
            .metadata
            .finalize_upload(
                owner_user_id,
                session_id,
                commit_record.lease_generation,
                now(),
            )
            .await
            .map_err(map_metadata_error)?
        {
            UploadFinalization::Completed(completion) => Ok(completion),
            UploadFinalization::VersionConflict {
                current_revision, ..
            } => {
                let _ = self.abort_staging_if_safe(&commit_record).await;
                Err(UploadError::VersionConflict {
                    current_revision: Some(current_revision),
                })
            }
            UploadFinalization::Terminal(record) => Err(failed_error(&record)),
            UploadFinalization::NotReady(_) => Err(UploadError::InProgress),
        }
    }

    pub async fn abort_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
    ) -> Result<UploadSessionView, UploadError> {
        let record = self
            .metadata
            .abort_upload(owner_user_id, session_id, now())
            .await
            .map_err(map_metadata_error)?;
        // Promotion may have succeeded even if the caller never observed its
        // receipt. Abort only touches staging; the committed key is deliberately
        // retained for later reconciliation/GC policy.
        let _ = self.abort_staging_if_safe(&record).await;
        view_from_record(&record)
    }

    pub async fn expire_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
    ) -> Result<UploadSessionView, UploadError> {
        let record = self
            .metadata
            .expire_upload(owner_user_id, session_id, now())
            .await
            .map_err(map_metadata_error)?;
        if record.state == UploadSessionState::Expired {
            let _ = self.abort_staging_if_safe(&record).await;
        }
        view_from_record(&record)
    }

    pub async fn list_cleanup_candidates(
        &self,
        limit: u32,
    ) -> Result<Vec<UploadCleanupWork>, UploadError> {
        let candidates = self
            .metadata
            .list_upload_cleanup_candidates(limit.min(100))
            .await
            .map_err(map_metadata_error)?;
        candidates.into_iter().map(cleanup_work).collect()
    }

    async fn verify_and_promote(
        &self,
        owner_user_id: UserId,
        record: UploadSessionRecord,
    ) -> Result<UploadSessionRecord, UploadError> {
        let handle = parse_staging_handle(&record.staging_handle)?;
        let key = parse_object_key(&record.object_key)?;
        let progress = match self.object_store.staging_progress(&handle).await {
            Ok(progress) => Some(progress),
            Err(ObjectStoreError::StagingNotFound) => None,
            Err(error) => {
                let mapped = map_storage_error(error);
                let _ = self
                    .metadata
                    .release_upload_lease(
                        owner_user_id,
                        record.id,
                        record.lease_generation,
                        mapped.as_str(),
                        now(),
                    )
                    .await;
                return Err(mapped);
            }
        };
        if let Some(progress) = progress
            && progress.length() != record.bytes_received
        {
            if progress.length() > record.bytes_received {
                let _ = self
                    .metadata
                    .fail_upload(
                        owner_user_id,
                        record.id,
                        record.lease_generation,
                        "concurrent_append",
                        now(),
                    )
                    .await;
                let _ = self.object_store.abort_staged(&handle).await;
                return Err(UploadError::Failed {
                    code: "concurrent_append".to_owned(),
                });
            }
            let _ = self
                .metadata
                .release_upload_lease(
                    owner_user_id,
                    record.id,
                    record.lease_generation,
                    "storage_unavailable",
                    now(),
                )
                .await;
            return Err(UploadError::StorageUnavailable);
        }
        let integrity = IntegrityExpectation::none()
            .with_length(record.expected_length)
            .with_sha256_option(record.expected_sha256);
        let metadata = match self.object_store.finalize_staged(&handle, integrity).await {
            Ok(_staged) => {
                self.promote_and_reconcile(owner_user_id, &record, &handle, &key)
                    .await?
            }
            Err(ObjectStoreError::StagingNotFound) => {
                self.reconcile_promoted_object(owner_user_id, &record, &key)
                    .await?
            }
            Err(ObjectStoreError::IntegrityMismatch) => {
                let error = if record
                    .expected_sha256
                    .is_some_and(|_| record.bytes_received == record.expected_length)
                {
                    UploadError::HashMismatch
                } else {
                    UploadError::SizeMismatch
                };
                let _ = self
                    .metadata
                    .fail_upload(
                        owner_user_id,
                        record.id,
                        record.lease_generation,
                        error.as_str(),
                        now(),
                    )
                    .await;
                let _ = self.object_store.abort_staged(&handle).await;
                return Err(error);
            }
            Err(error) => {
                let mapped = map_storage_error(error);
                let _ = self
                    .metadata
                    .release_upload_lease(
                        owner_user_id,
                        record.id,
                        record.lease_generation,
                        mapped.as_str(),
                        now(),
                    )
                    .await;
                return Err(mapped);
            }
        };
        if metadata.key() != &key {
            let _ = self
                .metadata
                .fail_upload(
                    owner_user_id,
                    record.id,
                    record.lease_generation,
                    "object_key_mismatch",
                    now(),
                )
                .await;
            return Err(UploadError::InvalidPersistedData);
        }
        let verified_sha256 = match self
            .verify_object_metadata(&metadata, record.expected_length, record.expected_sha256)
            .await
        {
            Ok(digest) => digest,
            Err(error @ (UploadError::HashMismatch | UploadError::SizeMismatch)) => {
                let _ = self
                    .metadata
                    .fail_upload(
                        owner_user_id,
                        record.id,
                        record.lease_generation,
                        "object_corrupt",
                        now(),
                    )
                    .await;
                return Err(error);
            }
            Err(error @ UploadError::StorageUnavailable) => {
                let _ = self
                    .metadata
                    .release_upload_lease(
                        owner_user_id,
                        record.id,
                        record.lease_generation,
                        error.as_str(),
                        now(),
                    )
                    .await;
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        let receipt = UploadDurabilityReceipt {
            backend_kind: backend_kind(self.object_store.capabilities()),
            storage_key: metadata.key().as_str().to_owned(),
            backend_version: metadata
                .version()
                .map(|version| version.as_str().to_owned()),
            length: metadata.length(),
            sha256: verified_sha256,
            verified_at: now(),
        };
        let updated = self
            .metadata
            .record_upload_durable(
                owner_user_id,
                record.id,
                record.lease_generation,
                receipt,
                now(),
            )
            .await
            .map_err(map_metadata_error)?;
        if updated.state != UploadSessionState::Committing {
            let _ = self.object_store.abort_staged(&handle).await;
            return Err(UploadError::InvalidState {
                state: updated.state,
            });
        }
        Ok(updated)
    }

    async fn promote_and_reconcile(
        &self,
        owner_user_id: UserId,
        record: &UploadSessionRecord,
        handle: &StagingHandle,
        key: &ObjectKey,
    ) -> Result<ObjectMetadata, UploadError> {
        match self.object_store.promote_temp(handle, key).await {
            Ok(receipt) => Ok(receipt.metadata().clone()),
            Err(ObjectStoreError::AlreadyExists | ObjectStoreError::StorageUnavailable) => {
                self.reconcile_promoted_object(owner_user_id, record, key)
                    .await
            }
            Err(error) => {
                let mapped = map_storage_error(error);
                let _ = self
                    .metadata
                    .release_upload_lease(
                        owner_user_id,
                        record.id,
                        record.lease_generation,
                        mapped.as_str(),
                        now(),
                    )
                    .await;
                Err(mapped)
            }
        }
    }

    async fn reconcile_promoted_object(
        &self,
        owner_user_id: UserId,
        record: &UploadSessionRecord,
        key: &ObjectKey,
    ) -> Result<ObjectMetadata, UploadError> {
        match self
            .verify_committed_object(key, record.expected_length, record.expected_sha256)
            .await
        {
            Ok(metadata) => Ok(metadata),
            Err(UploadError::StorageUnavailable) => {
                let _ = self
                    .metadata
                    .release_upload_lease(
                        owner_user_id,
                        record.id,
                        record.lease_generation,
                        "storage_unavailable",
                        now(),
                    )
                    .await;
                Err(UploadError::StorageUnavailable)
            }
            Err(error @ (UploadError::HashMismatch | UploadError::SizeMismatch)) => {
                let _ = self
                    .metadata
                    .fail_upload(
                        owner_user_id,
                        record.id,
                        record.lease_generation,
                        "object_corrupt",
                        now(),
                    )
                    .await;
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    async fn verify_committed_object(
        &self,
        key: &ObjectKey,
        expected_length: u64,
        expected_sha256: Option<Sha256Digest>,
    ) -> Result<ObjectMetadata, UploadError> {
        let metadata = self
            .object_store
            .metadata(key)
            .await
            .map_err(map_storage_error)?;
        self.verify_object_metadata(&metadata, expected_length, expected_sha256)
            .await?;
        Ok(metadata)
    }

    async fn verify_object_metadata(
        &self,
        metadata: &ObjectMetadata,
        expected_length: u64,
        expected_sha256: Option<Sha256Digest>,
    ) -> Result<Sha256Digest, UploadError> {
        if metadata.length() != expected_length {
            return Err(UploadError::SizeMismatch);
        }
        if expected_sha256
            .is_some_and(|expected| metadata.sha256().is_some_and(|actual| *actual != expected))
        {
            return Err(UploadError::HashMismatch);
        }
        let read = self
            .object_store
            .get(metadata.key())
            .await
            .map_err(map_storage_error)?;
        let mut stream = read.into_stream();
        let mut length = 0_u64;
        let mut hasher = Sha256::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(map_storage_error)?;
            length = length
                .checked_add(chunk.len() as u64)
                .ok_or(UploadError::SizeMismatch)?;
            hasher.update(&chunk);
        }
        if length != expected_length {
            return Err(UploadError::SizeMismatch);
        }
        let mut digest = [0_u8; 32];
        digest.copy_from_slice(&hasher.finalize());
        let digest = Sha256Digest::from_bytes(digest);
        if expected_sha256.is_some_and(|expected| expected != digest)
            || metadata
                .sha256()
                .is_some_and(|expected| *expected != digest)
        {
            return Err(UploadError::HashMismatch);
        }
        Ok(digest)
    }

    async fn reconcile_progress(
        &self,
        owner_user_id: UserId,
        record: UploadSessionRecord,
        observed_length: u64,
    ) -> Result<UploadSessionRecord, UploadError> {
        if observed_length == record.bytes_received {
            return Ok(record);
        }
        if observed_length < record.bytes_received {
            return Err(UploadError::StorageUnavailable);
        }
        if observed_length > record.expected_length {
            return Err(UploadError::SizeMismatch);
        }
        let updated = self
            .metadata
            .record_upload_progress(
                owner_user_id,
                record.id,
                record.bytes_received,
                observed_length,
                now(),
            )
            .await
            .map_err(map_metadata_error)?;
        if updated.bytes_received < observed_length {
            return Err(UploadError::DatabaseUnavailable);
        }
        Ok(updated)
    }

    async fn load_owned(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
    ) -> Result<UploadSessionRecord, UploadError> {
        self.metadata
            .find_upload_session(owner_user_id, session_id)
            .await
            .map_err(map_metadata_error)?
            .ok_or(UploadError::NotFound)
    }

    async fn expire_if_needed(
        &self,
        record: UploadSessionRecord,
    ) -> Result<UploadSessionRecord, UploadError> {
        if record.state == UploadSessionState::Open
            && record.expires_at.as_offset_datetime() <= now().as_offset_datetime()
        {
            let updated = self
                .metadata
                .expire_upload(record.owner_user_id, record.id, now())
                .await
                .map_err(map_metadata_error)?;
            if updated.state == UploadSessionState::Expired {
                let _ = self.abort_staging_if_safe(&updated).await;
            }
            return Ok(updated);
        }
        Ok(record)
    }

    async fn abort_staging_if_safe(&self, record: &UploadSessionRecord) -> Result<(), UploadError> {
        let handle = parse_staging_handle(&record.staging_handle)?;
        self.object_store
            .abort_staged(&handle)
            .await
            .map_err(map_storage_error)
    }

    fn validate_capabilities(&self, capabilities: StorageCapabilities) -> Result<(), UploadError> {
        if capabilities.availability() != StorageAvailability::Available
            || !capabilities.supports(StorageCapability::AtomicPromotion)
            || !capabilities.supports(StorageCapability::Checksumming)
            || !capabilities.supports(StorageCapability::ReadAfterWrite)
            || capabilities.support(StorageCapability::DurableFlush) != CapabilitySupport::Supported
        {
            return Err(UploadError::StorageUnavailable);
        }
        Ok(())
    }
}

fn ensure_open(record: &UploadSessionRecord) -> Result<(), UploadError> {
    match record.state {
        UploadSessionState::Open => Ok(()),
        UploadSessionState::Expired => Err(UploadError::UploadExpired),
        UploadSessionState::Committed => Err(UploadError::InvalidState {
            state: record.state,
        }),
        UploadSessionState::Failed => Err(failed_error(record)),
        state => Err(UploadError::InvalidState { state }),
    }
}

fn failed_error(record: &UploadSessionRecord) -> UploadError {
    UploadError::Failed {
        code: safe_error_code(record.terminal_failure_code.as_deref())
            .or_else(|| safe_error_code(record.last_error_code.as_deref()))
            .unwrap_or_else(|| "upload_failed".to_owned()),
    }
}

fn map_metadata_error(error: MetadataError) -> UploadError {
    match error {
        MetadataError::Database(_) => UploadError::DatabaseUnavailable,
        MetadataError::CapacityUnavailable => UploadError::CapacityUnavailable,
        MetadataError::Mapping(MappingError::Domain(DomainError::LibraryNotWritable)) => {
            UploadError::CapacityUnavailable
        }
        MetadataError::Mapping(MappingError::Domain(DomainError::InvalidNodeStateTransition {
            ..
        })) => UploadError::InvalidState {
            state: UploadSessionState::Failed,
        },
        MetadataError::Mapping(MappingError::Domain(_)) => UploadError::InvalidRequest,
        MetadataError::Mapping(MappingError::RelationMismatch { .. }) => UploadError::NotFound,
        MetadataError::Mapping(_) => UploadError::InvalidPersistedData,
    }
}

fn map_storage_error(error: ObjectStoreError) -> UploadError {
    match error {
        ObjectStoreError::StorageUnavailable
        | ObjectStoreError::StagingNotFound
        | ObjectStoreError::StagingConflict
        | ObjectStoreError::NotFound => UploadError::StorageUnavailable,
        ObjectStoreError::IntegrityMismatch => UploadError::HashMismatch,
        ObjectStoreError::UnsupportedCapability(_) => UploadError::StorageUnavailable,
        ObjectStoreError::AlreadyExists | ObjectStoreError::PreconditionFailed => {
            UploadError::CompletionConflict
        }
        ObjectStoreError::InvalidKey | ObjectStoreError::InvalidRequest => {
            UploadError::InvalidPersistedData
        }
        ObjectStoreError::InvalidRange => UploadError::InvalidRequest,
    }
}

fn upload_request_matches_record(
    request: &CreateUploadSessionRequest,
    record: &UploadSessionRecord,
) -> bool {
    let expected_operation = match &request.target {
        UploadTargetRequest::CreateFile { .. } => UploadOperation::CreateFile,
        UploadTargetRequest::ReplaceContent { .. } => UploadOperation::ReplaceContent,
    };
    record.operation == expected_operation
        && record.expected_length == request.expected_length
        && record.expected_sha256 == request.expected_sha256
        && match (&request.target, record.operation) {
            (
                UploadTargetRequest::CreateFile {
                    library_id,
                    parent_node_id,
                    name,
                },
                UploadOperation::CreateFile,
            ) => {
                record.library_id == *library_id
                    && record.target_parent_node_id == Some(*parent_node_id)
                    && record.target_name.as_ref() == Some(name)
            }
            (
                UploadTargetRequest::ReplaceContent {
                    library_id,
                    node_id,
                    expected_revision,
                },
                UploadOperation::ReplaceContent,
            ) => {
                record.library_id == *library_id
                    && record.target_node_id == *node_id
                    && record.expected_node_revision == Some(*expected_revision)
            }
            _ => false,
        }
}

fn parse_staging_handle(value: &str) -> Result<StagingHandle, UploadError> {
    StagingHandle::new(value.to_owned()).map_err(|_| UploadError::InvalidPersistedData)
}

fn parse_object_key(value: &str) -> Result<ObjectKey, UploadError> {
    ObjectKey::new(value.to_owned()).map_err(|_| UploadError::InvalidPersistedData)
}

fn add_duration(now: Timestamp, duration: Duration) -> Result<Timestamp, UploadError> {
    let duration = TimeDuration::try_from(duration).map_err(|_| UploadError::InvalidRequest)?;
    now.as_offset_datetime()
        .checked_add(duration)
        .map(Timestamp::from_offset_datetime)
        .ok_or(UploadError::InvalidRequest)
}

fn now() -> Timestamp {
    let value = Timestamp::now().as_offset_datetime();
    Timestamp::from_offset_datetime(
        value
            .replace_nanosecond(value.nanosecond() / 1_000 * 1_000)
            .expect("microsecond truncation remains valid"),
    )
}

fn backend_kind(capabilities: StorageCapabilities) -> String {
    match capabilities.backend() {
        synveil_object_store::StorageBackendKind::LocalFilesystem => "LOCAL_FILESYSTEM",
        synveil_object_store::StorageBackendKind::ObjectStore => "OBJECT_STORE",
        synveil_object_store::StorageBackendKind::Unknown => "UNKNOWN",
    }
    .to_owned()
}

fn view_from_record(record: &UploadSessionRecord) -> Result<UploadSessionView, UploadError> {
    let target = match record.operation {
        UploadOperation::CreateFile => {
            let parent_node_id = record
                .target_parent_node_id
                .ok_or(UploadError::InvalidPersistedData)?;
            let name = record
                .target_name
                .clone()
                .ok_or(UploadError::InvalidPersistedData)?;
            UploadTargetView::CreateFile {
                library_id: record.library_id,
                parent_node_id,
                node_id: record.target_node_id,
                name,
            }
        }
        UploadOperation::ReplaceContent => UploadTargetView::ReplaceContent {
            library_id: record.library_id,
            node_id: record.target_node_id,
            expected_revision: record
                .expected_node_revision
                .ok_or(UploadError::InvalidPersistedData)?,
        },
    };
    Ok(UploadSessionView {
        id: record.id,
        owner_user_id: record.owner_user_id,
        target,
        expected_length: record.expected_length,
        expected_sha256: record.expected_sha256,
        received_bytes: record.bytes_received,
        state: record.state,
        created_at: record.created_at,
        updated_at: record.updated_at,
        expires_at: record.expires_at,
        last_error_code: safe_error_code(record.last_error_code.as_deref()),
        terminal_failure_code: safe_error_code(record.terminal_failure_code.as_deref()),
        completion: record.completion.clone(),
    })
}

fn safe_error_code(value: Option<&str>) -> Option<String> {
    value
        .filter(|value| {
            matches!(
                *value,
                "upload_expired"
                    | "invalid_offset"
                    | "size_mismatch"
                    | "hash_mismatch"
                    | "version_conflict"
                    | "object_corrupt"
                    | "object_key_mismatch"
                    | "concurrent_append"
                    | "storage_unavailable"
                    | "database_unavailable"
                    | "capacity_unavailable"
                    | "completion_conflict"
            )
        })
        .map(str::to_owned)
}

fn cleanup_work(candidate: UploadCleanupCandidate) -> Result<UploadCleanupWork, UploadError> {
    Ok(UploadCleanupWork {
        session_id: candidate.session_id,
        state: candidate.state,
        staging_handle: parse_staging_handle(&candidate.staging_handle)?,
        object_key: parse_object_key(&candidate.object_key)?,
        lease_generation: candidate.lease_generation,
    })
}

trait UploadTargetLibrary {
    fn library_id(&self) -> LibraryId;
}

impl UploadTargetLibrary for UploadTargetRequest {
    fn library_id(&self) -> LibraryId {
        match self {
            Self::CreateFile { library_id, .. } | Self::ReplaceContent { library_id, .. } => {
                *library_id
            }
        }
    }
}

trait IntegrityExpectationExt {
    fn with_sha256_option(self, expected: Option<Sha256Digest>) -> Self;
}

impl IntegrityExpectationExt for IntegrityExpectation {
    fn with_sha256_option(self, expected: Option<Sha256Digest>) -> Self {
        expected.map_or(self, |value| self.with_sha256(value))
    }
}

use synveil_core::DomainError;
use synveil_metadata::MappingError;
