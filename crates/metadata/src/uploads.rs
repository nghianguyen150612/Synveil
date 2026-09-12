//! Persisted upload-session records and their PostgreSQL concurrency boundary.
//!
//! This module owns no byte transport and no filesystem/provider behavior. It
//! persists opaque storage identities, leases, progress, and the short
//! logical finalization transaction used by the storage application service.

use std::{fmt, str::FromStr};

use async_trait::async_trait;
use sqlx::{Postgres, Transaction};
use synveil_core::{
    ChangeKind, FileVersion, FileVersionId, Library, LibraryId, LibraryStatus, LogicalName, Node,
    NodeId, NodeKind, ObjectId, ObjectReference, ObjectReplicaId, Revision, Sha256Digest,
    Timestamp, UploadOperation, UploadSessionId, UploadSessionState, UserId,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::journal::{JournalChange, acquire_namespace_guard, append_changes};
use crate::{
    DatabasePool, DomainRepository, FileVersionRow, LibraryRow, MappingError, MetadataError,
    NodeRow, ObjectReplicaRow, ObjectRow, UploadSessionRow,
};

/// Verified storage evidence that is safe to persist and later reconcile.
/// `storage_key` is an opaque server-generated key and is never a user path.
#[derive(Clone, Eq, PartialEq)]
pub struct UploadDurabilityReceipt {
    pub backend_kind: String,
    pub storage_key: String,
    pub backend_version: Option<String>,
    pub length: u64,
    pub sha256: Sha256Digest,
    pub verified_at: Timestamp,
}

impl fmt::Debug for UploadDurabilityReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UploadDurabilityReceipt")
            .field("backend_kind", &self.backend_kind)
            .field("storage_key", &"<redacted>")
            .field(
                "backend_version",
                &self.backend_version.as_ref().map(|_| "<redacted>"),
            )
            .field("length", &self.length)
            .field("sha256", &"<redacted>")
            .field("verified_at", &self.verified_at)
            .finish()
    }
}

/// Canonical metadata returned after the logical commit transaction wins.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadCompletion {
    pub session_id: UploadSessionId,
    pub node_id: NodeId,
    pub file_version_id: FileVersionId,
    pub object_id: ObjectId,
    pub object_replica_id: ObjectReplicaId,
    pub node_revision: Revision,
    pub length: u64,
    pub sha256: Sha256Digest,
    pub committed_at: Timestamp,
}

/// Fully persisted session state used by the application service. The public
/// status projection is deliberately separate so opaque staging/key values do
/// not escape the application boundary.
#[derive(Clone, Eq, PartialEq)]
pub struct UploadSessionRecord {
    pub id: UploadSessionId,
    pub owner_user_id: UserId,
    pub library_id: LibraryId,
    pub operation: UploadOperation,
    pub target_node_id: NodeId,
    pub target_parent_node_id: Option<NodeId>,
    pub target_name: Option<LogicalName>,
    pub expected_node_revision: Option<Revision>,
    pub expected_length: u64,
    pub expected_sha256: Option<Sha256Digest>,
    pub object_id: ObjectId,
    pub object_replica_id: ObjectReplicaId,
    pub object_key: String,
    pub staging_handle: String,
    pub bytes_received: u64,
    pub state: UploadSessionState,
    pub lease_generation: u64,
    pub lease_expires_at: Option<Timestamp>,
    pub cancel_requested_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub expires_at: Timestamp,
    pub last_error_code: Option<String>,
    pub terminal_failure_code: Option<String>,
    pub durability: Option<UploadDurabilityReceipt>,
    pub completion: Option<UploadCompletion>,
}

impl fmt::Debug for UploadSessionRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UploadSessionRecord")
            .field("id", &self.id)
            .field("owner_user_id", &self.owner_user_id)
            .field("library_id", &self.library_id)
            .field("operation", &self.operation)
            .field("target_node_id", &self.target_node_id)
            .field("target_parent_node_id", &self.target_parent_node_id)
            .field("target_name", &self.target_name)
            .field("expected_node_revision", &self.expected_node_revision)
            .field("expected_length", &self.expected_length)
            .field(
                "expected_sha256",
                &self.expected_sha256.as_ref().map(|_| "<redacted>"),
            )
            .field("object_id", &self.object_id)
            .field("object_replica_id", &self.object_replica_id)
            .field("object_key", &"<redacted>")
            .field("staging_handle", &"<redacted>")
            .field("bytes_received", &self.bytes_received)
            .field("state", &self.state)
            .field("lease_generation", &self.lease_generation)
            .field("lease_expires_at", &self.lease_expires_at)
            .field("cancel_requested_at", &self.cancel_requested_at)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("expires_at", &self.expires_at)
            .field("last_error_code", &self.last_error_code)
            .field("terminal_failure_code", &self.terminal_failure_code)
            .field("durability", &self.durability)
            .field("completion", &self.completion)
            .finish()
    }
}

/// Server-generated/persistence-checked input for session creation.
#[derive(Clone)]
pub struct NewUploadSession {
    pub id: UploadSessionId,
    pub owner_user_id: UserId,
    pub library_id: LibraryId,
    pub operation: UploadOperation,
    pub target_node_id: NodeId,
    pub target_parent_node_id: Option<NodeId>,
    pub target_name: Option<LogicalName>,
    pub expected_node_revision: Option<Revision>,
    pub expected_length: u64,
    pub expected_sha256: Option<Sha256Digest>,
    pub object_id: ObjectId,
    pub object_replica_id: ObjectReplicaId,
    pub object_key: String,
    pub staging_handle: String,
    pub max_active_sessions: u32,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}

impl fmt::Debug for NewUploadSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NewUploadSession")
            .field("id", &self.id)
            .field("owner_user_id", &self.owner_user_id)
            .field("library_id", &self.library_id)
            .field("operation", &self.operation)
            .field("target_node_id", &self.target_node_id)
            .field("target_parent_node_id", &self.target_parent_node_id)
            .field("target_name", &self.target_name)
            .field("expected_node_revision", &self.expected_node_revision)
            .field("expected_length", &self.expected_length)
            .field(
                "expected_sha256",
                &self.expected_sha256.as_ref().map(|_| "<redacted>"),
            )
            .field("object_id", &self.object_id)
            .field("object_replica_id", &self.object_replica_id)
            .field("object_key", &"<redacted>")
            .field("staging_handle", &"<redacted>")
            .field("max_active_sessions", &self.max_active_sessions)
            .field("created_at", &self.created_at)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UploadClaim {
    Acquired(UploadSessionRecord),
    Busy(UploadSessionRecord),
    Terminal(UploadSessionRecord),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UploadFinalization {
    Completed(UploadCompletion),
    VersionConflict {
        current_revision: Revision,
        session: UploadSessionRecord,
    },
    NotReady(UploadSessionRecord),
    Terminal(UploadSessionRecord),
}

#[derive(Clone, Eq, PartialEq)]
pub struct UploadCleanupCandidate {
    pub session_id: UploadSessionId,
    pub state: UploadSessionState,
    pub staging_handle: String,
    pub object_key: String,
    pub lease_generation: u64,
}

impl fmt::Debug for UploadCleanupCandidate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UploadCleanupCandidate")
            .field("session_id", &self.session_id)
            .field("state", &self.state)
            .field("staging_handle", &"<redacted>")
            .field("object_key", &"<redacted>")
            .field("lease_generation", &self.lease_generation)
            .finish()
    }
}

/// Metadata operations required by the transport-neutral upload service.
/// Implementations must keep row locks/conditional transitions in PostgreSQL;
/// callers must not replace this with an in-process mutex.
#[async_trait]
pub trait UploadMetadataBackend: Send + Sync {
    async fn create_upload_session(
        &self,
        input: NewUploadSession,
    ) -> Result<UploadSessionRecord, MetadataError>;

    async fn find_upload_session(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
    ) -> Result<Option<UploadSessionRecord>, MetadataError>;

    async fn record_upload_progress(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        expected_offset: u64,
        new_offset: u64,
        observed_at: Timestamp,
    ) -> Result<UploadSessionRecord, MetadataError>;

    async fn claim_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        observed_at: Timestamp,
        lease_until: Timestamp,
    ) -> Result<UploadClaim, MetadataError>;

    async fn record_upload_durable(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        lease_generation: u64,
        receipt: UploadDurabilityReceipt,
        observed_at: Timestamp,
    ) -> Result<UploadSessionRecord, MetadataError>;

    async fn release_upload_lease(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        lease_generation: u64,
        safe_error_code: &'static str,
        observed_at: Timestamp,
    ) -> Result<(), MetadataError>;

    async fn fail_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        lease_generation: u64,
        safe_error_code: &'static str,
        observed_at: Timestamp,
    ) -> Result<UploadSessionRecord, MetadataError>;

    async fn finalize_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        lease_generation: u64,
        observed_at: Timestamp,
    ) -> Result<UploadFinalization, MetadataError>;

    async fn abort_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        observed_at: Timestamp,
    ) -> Result<UploadSessionRecord, MetadataError>;

    async fn expire_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        observed_at: Timestamp,
    ) -> Result<UploadSessionRecord, MetadataError>;

    async fn list_upload_cleanup_candidates(
        &self,
        limit: u32,
    ) -> Result<Vec<UploadCleanupCandidate>, MetadataError>;
}

fn now_timestamp(value: OffsetDateTime) -> Timestamp {
    Timestamp::from_offset_datetime(value)
}

fn encode_timestamp(value: Timestamp) -> OffsetDateTime {
    value.as_offset_datetime()
}

fn decode_u64(value: &str, field: &'static str) -> Result<u64, MappingError> {
    Revision::from_str(value)
        .map(Revision::get)
        .map_err(|_| MappingError::InvalidDecimal { field })
}

fn decode_id<T>(value: Uuid, field: &'static str) -> Result<T, MappingError>
where
    T: TryFrom<Uuid, Error = synveil_core::IdParseError>,
{
    T::try_from(value).map_err(|reason| MappingError::InvalidId { field, reason })
}

fn decode_digest(value: &[u8], field: &'static str) -> Result<Sha256Digest, MappingError> {
    Sha256Digest::try_from(value).map_err(|_| MappingError::InvalidDigest { field })
}

fn decode_optional_timestamp(value: Option<OffsetDateTime>) -> Option<Timestamp> {
    value.map(now_timestamp)
}

impl UploadSessionRow {
    fn try_into_record(self) -> Result<UploadSessionRecord, MappingError> {
        let id = decode_id(self.id, "upload_sessions.id")?;
        let owner_user_id = decode_id(self.owner_user_id, "upload_sessions.owner_user_id")?;
        let library_id = decode_id(self.library_id, "upload_sessions.library_id")?;
        let operation =
            UploadOperation::from_str(&self.operation).map_err(|_| MappingError::InvalidEnum {
                field: "upload_sessions.operation",
            })?;
        let state =
            UploadSessionState::from_str(&self.state).map_err(|_| MappingError::InvalidEnum {
                field: "upload_sessions.state",
            })?;
        let target_node_id = decode_id(self.target_node_id, "upload_sessions.target_node_id")?;
        let target_parent_node_id = self
            .target_parent_node_id
            .map(|value| decode_id(value, "upload_sessions.target_parent_node_id"))
            .transpose()?;
        let target_name = self
            .target_name
            .map(|value| {
                LogicalName::new(value).map_err(|_| MappingError::InvalidName {
                    field: "upload_sessions.target_name",
                })
            })
            .transpose()?;
        let expected_node_revision = self
            .expected_node_revision
            .as_deref()
            .map(|value| decode_u64(value, "upload_sessions.expected_node_revision"))
            .transpose()?
            .map(Revision::new);
        let expected_length = decode_u64(&self.expected_length, "upload_sessions.expected_length")?;
        let expected_sha256 = self
            .expected_sha256
            .as_deref()
            .map(|value| decode_digest(value, "upload_sessions.expected_sha256"))
            .transpose()?;
        let object_id = decode_id(self.object_id, "upload_sessions.object_id")?;
        let object_replica_id =
            decode_id(self.object_replica_id, "upload_sessions.object_replica_id")?;
        let bytes_received = decode_u64(&self.bytes_received, "upload_sessions.bytes_received")?;
        let lease_generation =
            decode_u64(&self.lease_generation, "upload_sessions.lease_generation")?;
        let durability = match (
            self.durability_backend_kind,
            self.durability_backend_version,
            self.durability_length,
            self.durability_sha256,
            self.durable_at,
        ) {
            (None, None, None, None, None) => None,
            (
                Some(backend_kind),
                backend_version,
                Some(length),
                Some(sha256),
                Some(verified_at),
            ) => Some(UploadDurabilityReceipt {
                backend_kind,
                storage_key: self.object_key.clone(),
                backend_version,
                length: decode_u64(&length, "upload_sessions.durability_length")?,
                sha256: decode_digest(&sha256, "upload_sessions.durability_sha256")?,
                verified_at: now_timestamp(verified_at),
            }),
            _ => {
                return Err(MappingError::RelationMismatch {
                    relation: "upload_sessions.durability",
                });
            }
        };
        let completion = match (
            self.completed_node_id,
            self.completed_file_version_id,
            self.completed_object_id,
            self.completed_object_replica_id,
            self.completed_node_revision,
            self.completed_at,
        ) {
            (None, None, None, None, None, None) => None,
            (
                Some(node_id),
                Some(file_version_id),
                Some(object_id),
                Some(object_replica_id),
                Some(node_revision),
                Some(committed_at),
            ) => Some(UploadCompletion {
                session_id: id,
                node_id: decode_id(node_id, "upload_sessions.completed_node_id")?,
                file_version_id: decode_id(
                    file_version_id,
                    "upload_sessions.completed_file_version_id",
                )?,
                object_id: decode_id(object_id, "upload_sessions.completed_object_id")?,
                object_replica_id: decode_id(
                    object_replica_id,
                    "upload_sessions.completed_object_replica_id",
                )?,
                node_revision: Revision::new(decode_u64(
                    &node_revision,
                    "upload_sessions.completed_node_revision",
                )?),
                length: durability
                    .as_ref()
                    .map_or(expected_length, |receipt| receipt.length),
                sha256: durability
                    .as_ref()
                    .map(|receipt| receipt.sha256)
                    .or(expected_sha256)
                    .ok_or(MappingError::RelationMismatch {
                        relation: "upload_sessions.completed_integrity",
                    })?,
                committed_at: now_timestamp(committed_at),
            }),
            _ => {
                return Err(MappingError::RelationMismatch {
                    relation: "upload_sessions.completion",
                });
            }
        };

        Ok(UploadSessionRecord {
            id,
            owner_user_id,
            library_id,
            operation,
            target_node_id,
            target_parent_node_id,
            target_name,
            expected_node_revision,
            expected_length,
            expected_sha256,
            object_id,
            object_replica_id,
            object_key: self.object_key,
            staging_handle: self.staging_handle,
            bytes_received,
            state,
            lease_generation,
            lease_expires_at: decode_optional_timestamp(self.lease_expires_at),
            cancel_requested_at: decode_optional_timestamp(self.cancel_requested_at),
            created_at: now_timestamp(self.created_at),
            updated_at: now_timestamp(self.updated_at),
            expires_at: now_timestamp(self.expires_at),
            last_error_code: self.last_error_code,
            terminal_failure_code: self.terminal_failure_code,
            durability,
            completion,
        })
    }
}

fn record_from_new(input: &NewUploadSession) -> UploadSessionRecord {
    UploadSessionRecord {
        id: input.id,
        owner_user_id: input.owner_user_id,
        library_id: input.library_id,
        operation: input.operation,
        target_node_id: input.target_node_id,
        target_parent_node_id: input.target_parent_node_id,
        target_name: input.target_name.clone(),
        expected_node_revision: input.expected_node_revision,
        expected_length: input.expected_length,
        expected_sha256: input.expected_sha256,
        object_id: input.object_id,
        object_replica_id: input.object_replica_id,
        object_key: input.object_key.clone(),
        staging_handle: input.staging_handle.clone(),
        bytes_received: 0,
        state: UploadSessionState::Open,
        lease_generation: 0,
        lease_expires_at: None,
        cancel_requested_at: None,
        created_at: input.created_at,
        updated_at: input.created_at,
        expires_at: input.expires_at,
        last_error_code: None,
        terminal_failure_code: None,
        durability: None,
        completion: None,
    }
}

const UPLOAD_SESSION_COLUMNS: &str = "
    id, owner_user_id, library_id, operation, target_node_id,
    target_parent_node_id, target_name, expected_node_revision::TEXT AS expected_node_revision,
    expected_length::TEXT AS expected_length, expected_sha256, object_id, object_replica_id,
    object_key, staging_handle, bytes_received::TEXT AS bytes_received, state,
    lease_generation::TEXT AS lease_generation, lease_expires_at, cancel_requested_at,
    created_at, updated_at, expires_at, last_error_code, terminal_failure_code,
    durability_backend_kind, durability_backend_version,
    durability_length::TEXT AS durability_length, durability_sha256, durable_at,
    completed_node_id, completed_file_version_id, completed_object_id,
    completed_object_replica_id, completed_node_revision::TEXT AS completed_node_revision,
    completed_at";

fn upload_select(where_clause: &str) -> String {
    format!("SELECT {UPLOAD_SESSION_COLUMNS} FROM upload_sessions {where_clause}")
}

/// PostgreSQL implementation of the upload metadata port.
#[derive(Clone)]
pub struct PostgresUploadRepository {
    pool: DatabasePool,
}

impl PostgresUploadRepository {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    #[must_use]
    pub const fn pool(&self) -> &DatabasePool {
        &self.pool
    }
}

async fn load_upload(
    pool: &DatabasePool,
    owner_user_id: UserId,
    session_id: UploadSessionId,
) -> Result<Option<UploadSessionRecord>, MetadataError> {
    let row = sqlx::query_as::<_, UploadSessionRow>(&upload_select(
        "WHERE owner_user_id = $1 AND id = $2",
    ))
    .bind(owner_user_id.into_uuid())
    .bind(session_id.into_uuid())
    .fetch_optional(pool.sqlx_pool())
    .await
    .map_err(MetadataError::from)?;
    row.map(UploadSessionRow::try_into_record)
        .transpose()
        .map_err(Into::into)
}

async fn load_upload_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    session_id: UploadSessionId,
) -> Result<Option<UploadSessionRecord>, MetadataError> {
    let row = sqlx::query_as::<_, UploadSessionRow>(&upload_select(
        "WHERE owner_user_id = $1 AND id = $2 FOR UPDATE",
    ))
    .bind(owner_user_id.into_uuid())
    .bind(session_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(MetadataError::from)?;
    row.map(UploadSessionRow::try_into_record)
        .transpose()
        .map_err(Into::into)
}

async fn load_owned_library(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    library_id: LibraryId,
) -> Result<Option<Library>, MetadataError> {
    let row = sqlx::query_as::<_, LibraryRow>(
        "SELECT id, owner_user_id, name, root_node_id, dedup_domain_id, status,
                created_at, updated_at, revision::TEXT AS revision
         FROM libraries WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(library_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(MetadataError::from)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let root = sqlx::query_as::<_, NodeRow>(
        "SELECT id, library_id, parent_node_id, kind, name, current_version_id,
                state, trashed_at, created_at, updated_at, revision::TEXT AS revision
         FROM nodes WHERE id = $1 AND library_id = $2",
    )
    .bind(row.root_node_id)
    .bind(row.id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(MetadataError::from)?
    .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
        relation: "libraries.root_node_id",
    }))?
    .try_into_domain()?;
    row.try_into_domain(&root).map(Some).map_err(Into::into)
}

async fn load_node_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    library_id: LibraryId,
    node_id: NodeId,
) -> Result<Option<Node>, MetadataError> {
    let row = sqlx::query_as::<_, NodeRow>(
        "SELECT id, library_id, parent_node_id, kind, name, current_version_id,
                state, trashed_at, created_at, updated_at, revision::TEXT AS revision
         FROM nodes WHERE id = $1 AND library_id = $2 FOR UPDATE",
    )
    .bind(node_id.into_uuid())
    .bind(library_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(MetadataError::from)?;
    row.map(NodeRow::try_into_domain)
        .transpose()
        .map_err(Into::into)
}

async fn insert_node(
    transaction: &mut Transaction<'_, Postgres>,
    node: &Node,
) -> Result<(), MetadataError> {
    let row = NodeRow::from_domain(node)?;
    sqlx::query(
        "INSERT INTO nodes
                (id, library_id, parent_node_id, kind, name, current_version_id,
             state, trashed_at, created_at, updated_at, revision)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::NUMERIC)",
    )
    .bind(row.id)
    .bind(row.library_id)
    .bind(row.parent_node_id)
    .bind(&row.kind)
    .bind(&row.name)
    .bind(row.current_version_id)
    .bind(&row.state)
    .bind(row.trashed_at)
    .bind(row.created_at)
    .bind(row.updated_at)
    .bind(&row.revision)
    .execute(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map(|_| ())
}

async fn update_node(
    transaction: &mut Transaction<'_, Postgres>,
    node: &Node,
) -> Result<(), MetadataError> {
    let row = NodeRow::from_domain(node)?;
    sqlx::query(
        "UPDATE nodes SET current_version_id = $2, updated_at = $3,
                revision = $4::NUMERIC WHERE id = $1 AND library_id = $5",
    )
    .bind(row.id)
    .bind(row.current_version_id)
    .bind(row.updated_at)
    .bind(&row.revision)
    .bind(row.library_id)
    .execute(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map(|_| ())
}

async fn insert_object(
    transaction: &mut Transaction<'_, Postgres>,
    object: ObjectReference,
    created_at: Timestamp,
) -> Result<(), MetadataError> {
    let row = ObjectRow::from_reference(object, created_at)?;
    sqlx::query(
        "INSERT INTO objects
            (id, dedup_domain_id, canonical_hash, plaintext_length, created_at)
         VALUES ($1, $2, $3, $4::NUMERIC, $5)",
    )
    .bind(row.id)
    .bind(row.dedup_domain_id)
    .bind(&row.canonical_hash)
    .bind(&row.plaintext_length)
    .bind(row.created_at)
    .execute(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map(|_| ())
}

async fn insert_replica(
    transaction: &mut Transaction<'_, Postgres>,
    replica_id: ObjectReplicaId,
    object: ObjectReference,
    receipt: &UploadDurabilityReceipt,
) -> Result<(), MetadataError> {
    let row = ObjectReplicaRow {
        id: replica_id.into_uuid(),
        object_id: object.object_id().into_uuid(),
        object_dedup_domain_id: object.dedup_domain_id().into_uuid(),
        backend_kind: receipt.backend_kind.clone(),
        storage_key: receipt.storage_key.clone(),
        stored_length: receipt.length.to_string(),
        stored_sha256: receipt.sha256.as_bytes().to_vec(),
        backend_version: receipt.backend_version.clone(),
        state: "VERIFIED".to_owned(),
        created_at: encode_timestamp(receipt.verified_at),
        verified_at: encode_timestamp(receipt.verified_at),
    };
    sqlx::query(
        "INSERT INTO object_replicas
            (id, object_id, object_dedup_domain_id, backend_kind, storage_key,
             stored_length, stored_sha256, backend_version, state, created_at, verified_at)
         VALUES ($1, $2, $3, $4, $5, $6::NUMERIC, $7, $8, $9, $10, $11)",
    )
    .bind(row.id)
    .bind(row.object_id)
    .bind(row.object_dedup_domain_id)
    .bind(&row.backend_kind)
    .bind(&row.storage_key)
    .bind(&row.stored_length)
    .bind(&row.stored_sha256)
    .bind(&row.backend_version)
    .bind(&row.state)
    .bind(row.created_at)
    .bind(row.verified_at)
    .execute(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map(|_| ())
}

async fn insert_file_version(
    transaction: &mut Transaction<'_, Postgres>,
    version: FileVersion,
) -> Result<(), MetadataError> {
    let object = version.object_reference();
    let row = FileVersionRow::from_domain(version)?;
    DomainRepository::clear_object_gc_candidate_in_transaction(
        transaction,
        object.object_id(),
        object.dedup_domain_id(),
    )
    .await?;
    sqlx::query(
        "INSERT INTO file_versions
            (id, library_id, node_id, object_id, object_dedup_domain_id,
             parent_version_id, committed_at, revision)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8::NUMERIC)",
    )
    .bind(row.id)
    .bind(row.library_id)
    .bind(row.node_id)
    .bind(row.object_id)
    .bind(row.object_dedup_domain_id)
    .bind(row.parent_version_id)
    .bind(row.committed_at)
    .bind(&row.revision)
    .execute(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map(|_| ())
}

fn completed_from_record(record: &UploadSessionRecord) -> Result<UploadCompletion, MetadataError> {
    record
        .completion
        .clone()
        .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
            relation: "upload_sessions.completion",
        }))
}

fn same_upload_create_semantics(record: &UploadSessionRecord, input: &NewUploadSession) -> bool {
    record.owner_user_id == input.owner_user_id
        && record.library_id == input.library_id
        && record.operation == input.operation
        && record.target_node_id == input.target_node_id
        && record.target_parent_node_id == input.target_parent_node_id
        && record.target_name == input.target_name
        && record.expected_node_revision == input.expected_node_revision
        && record.expected_length == input.expected_length
        && record.expected_sha256 == input.expected_sha256
}

#[async_trait]
impl UploadMetadataBackend for PostgresUploadRepository {
    async fn create_upload_session(
        &self,
        input: NewUploadSession,
    ) -> Result<UploadSessionRecord, MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let Some(library) =
            load_owned_library(&mut transaction, input.owner_user_id, input.library_id).await?
        else {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "upload_sessions.library",
            }));
        };
        if library.status() != LibraryStatus::Active {
            return Err(MetadataError::Mapping(MappingError::Domain(
                synveil_core::DomainError::LibraryNotWritable,
            )));
        }
        acquire_namespace_guard(&mut transaction, input.library_id).await?;
        let active_sessions: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM upload_sessions
             WHERE owner_user_id = $1 AND library_id = $2
               AND state IN ('OPEN', 'VERIFYING', 'COMMITTING')",
        )
        .bind(input.owner_user_id.into_uuid())
        .bind(input.library_id.into_uuid())
        .fetch_one(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;
        if active_sessions >= i64::from(input.max_active_sessions) {
            return Err(MetadataError::CapacityUnavailable);
        }
        match input.operation {
            UploadOperation::CreateFile => {
                let parent_id = input.target_parent_node_id.ok_or(MetadataError::Mapping(
                    MappingError::RelationMismatch {
                        relation: "upload_sessions.target_parent_node_id",
                    },
                ))?;
                let Some(parent) =
                    load_node_for_update(&mut transaction, input.library_id, parent_id).await?
                else {
                    return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                        relation: "upload_sessions.target_parent_node_id",
                    }));
                };
                if parent.kind() != NodeKind::Directory
                    || parent.state() != synveil_core::NodeState::Active
                {
                    return Err(MetadataError::Mapping(MappingError::Domain(
                        synveil_core::DomainError::ParentMustBeActive,
                    )));
                }
                if input.target_name.is_none() || input.expected_node_revision.is_some() {
                    return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                        relation: "upload_sessions.create_target",
                    }));
                }
            }
            UploadOperation::ReplaceContent => {
                let Some(node) =
                    load_node_for_update(&mut transaction, input.library_id, input.target_node_id)
                        .await?
                else {
                    return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                        relation: "upload_sessions.target_node_id",
                    }));
                };
                if node.kind() != NodeKind::File || node.state() != synveil_core::NodeState::Active
                {
                    return Err(MetadataError::Mapping(MappingError::Domain(
                        synveil_core::DomainError::InvalidNodeStateTransition {
                            from: node.state(),
                            to: node.state(),
                        },
                    )));
                }
                if input.target_parent_node_id.is_some() || input.target_name.is_some() {
                    return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                        relation: "upload_sessions.replace_target",
                    }));
                }
                if input.expected_node_revision != Some(node.revision()) {
                    return Err(MetadataError::Mapping(MappingError::RevisionConflict {
                        current_revision: node.revision(),
                    }));
                }
            }
        }

        let row = record_from_new(&input);
        let insert = sqlx::query(
            "INSERT INTO upload_sessions
                (id, owner_user_id, library_id, operation, target_node_id,
                 target_parent_node_id, target_name, expected_node_revision,
                 expected_length, expected_sha256, object_id, object_replica_id,
                 object_key, staging_handle, bytes_received, state, lease_generation,
                 created_at, updated_at, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8::NUMERIC,
                     $9::NUMERIC, $10, $11, $12, $13, $14, $15::NUMERIC,
                     $16, $17::NUMERIC, $18, $19, $20)",
        )
        .bind(row.id.into_uuid())
        .bind(row.owner_user_id.into_uuid())
        .bind(row.library_id.into_uuid())
        .bind(row.operation.as_str())
        .bind(row.target_node_id.into_uuid())
        .bind(row.target_parent_node_id.map(NodeId::into_uuid))
        .bind(row.target_name.as_ref().map(LogicalName::as_str))
        .bind(
            row.expected_node_revision
                .map(|value| value.get().to_string()),
        )
        .bind(row.expected_length.to_string())
        .bind(row.expected_sha256.map(|value| value.as_bytes().to_vec()))
        .bind(row.object_id.into_uuid())
        .bind(row.object_replica_id.into_uuid())
        .bind(&row.object_key)
        .bind(&row.staging_handle)
        .bind(row.bytes_received.to_string())
        .bind(row.state.as_str())
        .bind(row.lease_generation.to_string())
        .bind(encode_timestamp(row.created_at))
        .bind(encode_timestamp(row.updated_at))
        .bind(encode_timestamp(row.expires_at))
        .execute(&mut *transaction)
        .await;
        if let Err(error) = insert {
            let duplicate = matches!(
                error.as_database_error().and_then(|database| database.code()),
                Some(code) if code.as_ref() == "23505"
            );
            transaction.rollback().await.map_err(MetadataError::from)?;
            if !duplicate {
                return Err(MetadataError::from(error));
            }
            let existing = load_upload(&self.pool, input.owner_user_id, input.id)
                .await?
                .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                    relation: "upload_sessions.id",
                }))?;
            if !same_upload_create_semantics(&existing, &input) {
                return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                    relation: "upload_sessions.idempotency_key",
                }));
            }
            return Ok(existing);
        }
        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(row)
    }

    async fn find_upload_session(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
    ) -> Result<Option<UploadSessionRecord>, MetadataError> {
        load_upload(&self.pool, owner_user_id, session_id).await
    }

    async fn record_upload_progress(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        expected_offset: u64,
        new_offset: u64,
        observed_at: Timestamp,
    ) -> Result<UploadSessionRecord, MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let Some(record) =
            load_upload_for_update(&mut transaction, owner_user_id, session_id).await?
        else {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "upload_sessions.id",
            }));
        };
        if record.state == UploadSessionState::Open && record.bytes_received == expected_offset {
            UploadSessionState::Open
                .transition(UploadSessionState::Open)
                .map_err(|_| {
                    MetadataError::Mapping(MappingError::InvalidEnum {
                        field: "upload_sessions.state",
                    })
                })?;
            sqlx::query(
                "UPDATE upload_sessions SET bytes_received = $3::NUMERIC,
                        updated_at = $4, last_error_code = NULL
                 WHERE id = $1 AND owner_user_id = $2",
            )
            .bind(session_id.into_uuid())
            .bind(owner_user_id.into_uuid())
            .bind(new_offset.to_string())
            .bind(encode_timestamp(observed_at))
            .execute(&mut *transaction)
            .await
            .map_err(MetadataError::from)?;
        }
        let updated = load_upload_for_update(&mut transaction, owner_user_id, session_id)
            .await?
            .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "upload_sessions.id",
            }))?;
        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(updated)
    }

    async fn claim_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        observed_at: Timestamp,
        lease_until: Timestamp,
    ) -> Result<UploadClaim, MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let Some(mut record) =
            load_upload_for_update(&mut transaction, owner_user_id, session_id).await?
        else {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "upload_sessions.id",
            }));
        };
        if record.state == UploadSessionState::Open
            && record.expires_at.as_offset_datetime() <= observed_at.as_offset_datetime()
        {
            sqlx::query(
                "UPDATE upload_sessions SET state = 'EXPIRED', updated_at = $3,
                        lease_expires_at = NULL, last_error_code = 'upload_expired'
                 WHERE id = $1 AND owner_user_id = $2 AND state = 'OPEN'",
            )
            .bind(session_id.into_uuid())
            .bind(owner_user_id.into_uuid())
            .bind(encode_timestamp(observed_at))
            .execute(&mut *transaction)
            .await
            .map_err(MetadataError::from)?;
            record.state = UploadSessionState::Expired;
            record.last_error_code = Some("upload_expired".to_owned());
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(UploadClaim::Terminal(record));
        }

        if record.state.is_terminal() {
            if record.state == UploadSessionState::Committed {
                let _ = completed_from_record(&record)?;
            }
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(UploadClaim::Terminal(record));
        }

        let lease_active = record
            .lease_expires_at
            .is_some_and(|until| until.as_offset_datetime() > observed_at.as_offset_datetime());
        if lease_active {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(UploadClaim::Busy(record));
        }

        let next_state = match record.state {
            UploadSessionState::Open => UploadSessionState::Verifying,
            UploadSessionState::Verifying => UploadSessionState::Verifying,
            UploadSessionState::Committing => UploadSessionState::Committing,
            _ => record.state,
        };
        record.state.transition(next_state).map_err(|_| {
            MetadataError::Mapping(MappingError::InvalidEnum {
                field: "upload_sessions.state",
            })
        })?;
        let next_generation =
            record
                .lease_generation
                .checked_add(1)
                .ok_or(MetadataError::Mapping(MappingError::InvalidDecimal {
                    field: "upload_sessions.lease_generation",
                }))?;
        sqlx::query(
            "UPDATE upload_sessions SET state = $3, lease_generation = $4::NUMERIC,
                    lease_expires_at = $5, updated_at = $6
             WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(session_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(next_state.as_str())
        .bind(next_generation.to_string())
        .bind(encode_timestamp(lease_until))
        .bind(encode_timestamp(observed_at))
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;
        record.state = next_state;
        record.lease_generation = next_generation;
        record.lease_expires_at = Some(lease_until);
        record.updated_at = observed_at;
        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(UploadClaim::Acquired(record))
    }

    async fn record_upload_durable(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        lease_generation: u64,
        receipt: UploadDurabilityReceipt,
        observed_at: Timestamp,
    ) -> Result<UploadSessionRecord, MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let Some(record) =
            load_upload_for_update(&mut transaction, owner_user_id, session_id).await?
        else {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "upload_sessions.id",
            }));
        };
        if record.state != UploadSessionState::Verifying
            || record.lease_generation != lease_generation
            || receipt.storage_key != record.object_key
            || receipt.length != record.expected_length
            || record
                .expected_sha256
                .is_some_and(|expected| expected != receipt.sha256)
        {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(record);
        }
        record
            .state
            .transition(UploadSessionState::Committing)
            .map_err(|_| {
                MetadataError::Mapping(MappingError::InvalidEnum {
                    field: "upload_sessions.state",
                })
            })?;
        sqlx::query(
            "UPDATE upload_sessions SET state = 'COMMITTING',
                    durability_backend_kind = $3, durability_backend_version = $4,
                    durability_length = $5::NUMERIC, durability_sha256 = $6,
                    durable_at = $7, lease_expires_at = $7, updated_at = $7,
                    last_error_code = NULL
             WHERE id = $1 AND owner_user_id = $2 AND state = 'VERIFYING'
               AND lease_generation = $8::NUMERIC",
        )
        .bind(session_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(&receipt.backend_kind)
        .bind(&receipt.backend_version)
        .bind(receipt.length.to_string())
        .bind(receipt.sha256.as_bytes().to_vec())
        .bind(encode_timestamp(observed_at))
        .bind(lease_generation.to_string())
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;
        let updated = load_upload_for_update(&mut transaction, owner_user_id, session_id)
            .await?
            .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "upload_sessions.id",
            }))?;
        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(updated)
    }

    async fn release_upload_lease(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        lease_generation: u64,
        safe_error_code: &'static str,
        observed_at: Timestamp,
    ) -> Result<(), MetadataError> {
        sqlx::query(
            "UPDATE upload_sessions SET lease_expires_at = $4,
                    last_error_code = $3, updated_at = $4
             WHERE id = $1 AND owner_user_id = $2 AND lease_generation = $5::NUMERIC
               AND state IN ('VERIFYING', 'COMMITTING')",
        )
        .bind(session_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(safe_error_code)
        .bind(encode_timestamp(observed_at))
        .bind(lease_generation.to_string())
        .execute(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)
        .map(|_| ())
    }

    async fn fail_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        lease_generation: u64,
        safe_error_code: &'static str,
        observed_at: Timestamp,
    ) -> Result<UploadSessionRecord, MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let Some(record) =
            load_upload_for_update(&mut transaction, owner_user_id, session_id).await?
        else {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "upload_sessions.id",
            }));
        };
        if record.state.is_terminal() || record.lease_generation != lease_generation {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(record);
        }
        record
            .state
            .transition(UploadSessionState::Failed)
            .map_err(|_| {
                MetadataError::Mapping(MappingError::InvalidEnum {
                    field: "upload_sessions.state",
                })
            })?;
        sqlx::query(
            "UPDATE upload_sessions SET state = 'FAILED', terminal_failure_code = $3,
                    last_error_code = $3, lease_expires_at = NULL, updated_at = $4
             WHERE id = $1 AND owner_user_id = $2 AND lease_generation = $5::NUMERIC",
        )
        .bind(session_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(safe_error_code)
        .bind(encode_timestamp(observed_at))
        .bind(lease_generation.to_string())
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;
        let updated = load_upload_for_update(&mut transaction, owner_user_id, session_id)
            .await?
            .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "upload_sessions.id",
            }))?;
        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(updated)
    }

    async fn finalize_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        lease_generation: u64,
        observed_at: Timestamp,
    ) -> Result<UploadFinalization, MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let Some(record) =
            load_upload_for_update(&mut transaction, owner_user_id, session_id).await?
        else {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "upload_sessions.id",
            }));
        };
        if record.state == UploadSessionState::Committed {
            let completion = completed_from_record(&record)?;
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(UploadFinalization::Completed(completion));
        }
        if record.state.is_terminal() {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(UploadFinalization::Terminal(record));
        }
        if record.state != UploadSessionState::Committing
            || record.lease_generation != lease_generation
        {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(UploadFinalization::NotReady(record));
        }
        let receipt = record.durability.clone().ok_or(MetadataError::Mapping(
            MappingError::RelationMismatch {
                relation: "upload_sessions.durability",
            },
        ))?;
        let Some(library) =
            load_owned_library(&mut transaction, owner_user_id, record.library_id).await?
        else {
            return Ok(UploadFinalization::NotReady(record));
        };
        if library.status() != LibraryStatus::Active {
            transaction.commit().await.map_err(MetadataError::from)?;
            return Ok(UploadFinalization::Terminal(record));
        }
        acquire_namespace_guard(&mut transaction, record.library_id).await?;

        let node = match record.operation {
            UploadOperation::CreateFile => {
                let parent_id = record.target_parent_node_id.ok_or(MetadataError::Mapping(
                    MappingError::RelationMismatch {
                        relation: "upload_sessions.target_parent_node_id",
                    },
                ))?;
                let Some(parent) =
                    load_node_for_update(&mut transaction, record.library_id, parent_id).await?
                else {
                    return Ok(UploadFinalization::NotReady(record));
                };
                let name = record.target_name.clone().ok_or(MetadataError::Mapping(
                    MappingError::RelationMismatch {
                        relation: "upload_sessions.target_name",
                    },
                ))?;
                let node = Node::new_child(
                    record.target_node_id,
                    record.library_id,
                    &parent,
                    NodeKind::File,
                    name,
                    observed_at,
                )?;
                insert_node(&mut transaction, &node).await?;
                node
            }
            UploadOperation::ReplaceContent => {
                let Some(node) = load_node_for_update(
                    &mut transaction,
                    record.library_id,
                    record.target_node_id,
                )
                .await?
                else {
                    return Ok(UploadFinalization::NotReady(record));
                };
                if node.kind() != NodeKind::File || node.state() != synveil_core::NodeState::Active
                {
                    transaction.commit().await.map_err(MetadataError::from)?;
                    return Ok(UploadFinalization::Terminal(record));
                }
                if record.expected_node_revision != Some(node.revision()) {
                    let current_revision = node.revision();
                    sqlx::query(
                        "UPDATE upload_sessions SET state = 'FAILED',
                                terminal_failure_code = 'version_conflict',
                                last_error_code = 'version_conflict',
                                lease_expires_at = NULL, updated_at = $3
                         WHERE id = $1 AND owner_user_id = $2
                           AND state = 'COMMITTING' AND lease_generation = $4::NUMERIC",
                    )
                    .bind(session_id.into_uuid())
                    .bind(owner_user_id.into_uuid())
                    .bind(encode_timestamp(observed_at))
                    .bind(lease_generation.to_string())
                    .execute(&mut *transaction)
                    .await
                    .map_err(MetadataError::from)?;
                    let updated =
                        load_upload_for_update(&mut transaction, owner_user_id, session_id)
                            .await?
                            .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                                relation: "upload_sessions.id",
                            }))?;
                    transaction.commit().await.map_err(MetadataError::from)?;
                    return Ok(UploadFinalization::VersionConflict {
                        current_revision,
                        session: updated,
                    });
                }
                node
            }
        };

        let object = ObjectReference::verified(
            record.object_id,
            library.dedup_domain_id(),
            receipt.sha256,
            receipt.length,
        );
        let version = FileVersion::new(
            FileVersionId::new(),
            &library,
            &node,
            object,
            node.current_version_id(),
            observed_at,
        )?;
        let next_node = node.with_current_version(&version, observed_at)?;
        insert_object(&mut transaction, object, observed_at).await?;
        insert_replica(&mut transaction, record.object_replica_id, object, &receipt).await?;
        insert_file_version(&mut transaction, version).await?;
        update_node(&mut transaction, &next_node).await?;
        append_changes(
            &mut transaction,
            owner_user_id,
            record.library_id,
            &[JournalChange::from_node(
                ChangeKind::FileContentCommitted,
                &next_node,
            )],
        )
        .await?;

        let completion = UploadCompletion {
            session_id,
            node_id: next_node.id(),
            file_version_id: version.id(),
            object_id: object.object_id(),
            object_replica_id: record.object_replica_id,
            node_revision: next_node.revision(),
            length: receipt.length,
            sha256: receipt.sha256,
            committed_at: observed_at,
        };
        sqlx::query(
            "UPDATE upload_sessions SET state = 'COMMITTED', lease_expires_at = NULL,
                    last_error_code = NULL, completed_node_id = $3,
                    completed_file_version_id = $4, completed_object_id = $5,
                    completed_object_replica_id = $6, completed_node_revision = $7::NUMERIC,
                    completed_at = $8, updated_at = $8
             WHERE id = $1 AND owner_user_id = $2 AND state = 'COMMITTING'
               AND lease_generation = $9::NUMERIC",
        )
        .bind(session_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(completion.node_id.into_uuid())
        .bind(completion.file_version_id.into_uuid())
        .bind(completion.object_id.into_uuid())
        .bind(completion.object_replica_id.into_uuid())
        .bind(completion.node_revision.get().to_string())
        .bind(encode_timestamp(observed_at))
        .bind(lease_generation.to_string())
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)?;
        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(UploadFinalization::Completed(completion))
    }

    async fn abort_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        observed_at: Timestamp,
    ) -> Result<UploadSessionRecord, MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let Some(record) =
            load_upload_for_update(&mut transaction, owner_user_id, session_id).await?
        else {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "upload_sessions.id",
            }));
        };
        if !record.state.is_terminal() {
            record
                .state
                .transition(UploadSessionState::Aborted)
                .map_err(|_| {
                    MetadataError::Mapping(MappingError::InvalidEnum {
                        field: "upload_sessions.state",
                    })
                })?;
            sqlx::query(
                "UPDATE upload_sessions SET state = 'ABORTED', cancel_requested_at = $3,
                        lease_expires_at = NULL, updated_at = $3
                 WHERE id = $1 AND owner_user_id = $2",
            )
            .bind(session_id.into_uuid())
            .bind(owner_user_id.into_uuid())
            .bind(encode_timestamp(observed_at))
            .execute(&mut *transaction)
            .await
            .map_err(MetadataError::from)?;
        }
        let updated = load_upload_for_update(&mut transaction, owner_user_id, session_id)
            .await?
            .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "upload_sessions.id",
            }))?;
        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(updated)
    }

    async fn expire_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        observed_at: Timestamp,
    ) -> Result<UploadSessionRecord, MetadataError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)?;
        let Some(record) =
            load_upload_for_update(&mut transaction, owner_user_id, session_id).await?
        else {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "upload_sessions.id",
            }));
        };
        if record.state == UploadSessionState::Open
            && record.expires_at.as_offset_datetime() <= observed_at.as_offset_datetime()
        {
            record
                .state
                .transition(UploadSessionState::Expired)
                .map_err(|_| {
                    MetadataError::Mapping(MappingError::InvalidEnum {
                        field: "upload_sessions.state",
                    })
                })?;
            sqlx::query(
                "UPDATE upload_sessions SET state = 'EXPIRED', last_error_code = 'upload_expired',
                        lease_expires_at = NULL, updated_at = $3
                 WHERE id = $1 AND owner_user_id = $2 AND state = 'OPEN'",
            )
            .bind(session_id.into_uuid())
            .bind(owner_user_id.into_uuid())
            .bind(encode_timestamp(observed_at))
            .execute(&mut *transaction)
            .await
            .map_err(MetadataError::from)?;
        }
        let updated = load_upload_for_update(&mut transaction, owner_user_id, session_id)
            .await?
            .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "upload_sessions.id",
            }))?;
        transaction.commit().await.map_err(MetadataError::from)?;
        Ok(updated)
    }

    async fn list_upload_cleanup_candidates(
        &self,
        limit: u32,
    ) -> Result<Vec<UploadCleanupCandidate>, MetadataError> {
        let rows = sqlx::query_as::<_, UploadSessionRow>(&upload_select(
            "WHERE state IN ('ABORTED', 'EXPIRED', 'FAILED')
             ORDER BY updated_at ASC LIMIT $1",
        ))
        .bind(i64::from(limit.min(100)))
        .fetch_all(self.pool.sqlx_pool())
        .await
        .map_err(MetadataError::from)?;
        rows.into_iter()
            .map(|row| {
                let record = row.try_into_record()?;
                Ok(UploadCleanupCandidate {
                    session_id: record.id,
                    state: record.state,
                    staging_handle: record.staging_handle,
                    object_key: record.object_key,
                    lease_generation: record.lease_generation,
                })
            })
            .collect::<Result<Vec<_>, MappingError>>()
            .map_err(Into::into)
    }
}
