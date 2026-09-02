//! Durable PostgreSQL boundary for physical Object garbage collection.
//!
//! This module owns only canonical metadata fencing and recovery state. It has
//! no ObjectStore or filesystem dependency. The storage application service
//! must obtain one short action fence, perform at most one external delete,
//! inspect the exact key, and persist the resulting observation before it may
//! advance to another replica.

use std::{fmt, str::FromStr};

use async_trait::async_trait;
use sqlx::{FromRow, Postgres, Transaction};
use synveil_core::{
    DedupDomainId, GcWorkerRetryPolicy, ObjectGcOperationId, ObjectGcPolicy, ObjectId,
    ObjectReplicaId, Revision, Sha256Digest, Timestamp,
};
use uuid::Uuid;

use crate::{
    DatabaseError, DatabasePool, ObjectGcCandidateState, ObjectGcError, ObjectGcLease,
    ObjectGcPlanningService,
};

/// Canonical lifecycle of a durable physical-GC operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectGcExecutionState {
    Active,
    RecoveryRequired,
    NeedsAttention,
    Completed,
}

impl ObjectGcExecutionState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::RecoveryRequired => "RECOVERY_REQUIRED",
            Self::NeedsAttention => "NEEDS_ATTENTION",
            Self::Completed => "COMPLETED",
        }
    }

    fn parse(value: &str) -> Result<Self, ObjectGcExecutionMetadataError> {
        match value {
            "ACTIVE" => Ok(Self::Active),
            "RECOVERY_REQUIRED" => Ok(Self::RecoveryRequired),
            "NEEDS_ATTENTION" => Ok(Self::NeedsAttention),
            "COMPLETED" => Ok(Self::Completed),
            _ => Err(ObjectGcExecutionMetadataError::InvalidPersistedData),
        }
    }
}

/// Durable state of one exact replica action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectGcReplicaActionState {
    Pending,
    DeleteFenced,
    ReconciliationRequired,
    Retryable,
    Deleted,
    Failed,
}

impl ObjectGcReplicaActionState {
    fn parse(value: &str) -> Result<Self, ObjectGcExecutionMetadataError> {
        match value {
            "PENDING" => Ok(Self::Pending),
            "DELETE_FENCED" => Ok(Self::DeleteFenced),
            "RECONCILIATION_REQUIRED" => Ok(Self::ReconciliationRequired),
            "RETRYABLE" => Ok(Self::Retryable),
            "DELETED" => Ok(Self::Deleted),
            "FAILED" => Ok(Self::Failed),
            _ => Err(ObjectGcExecutionMetadataError::InvalidPersistedData),
        }
    }
}

/// Safe persisted outcome of one external delete/reconciliation observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectGcReplicaObservation {
    Deleted,
    AlreadyAbsent,
    ReconciledAbsent,
    StillPresent,
    DeleteInProgress,
    Unknown,
    Mismatch,
}

impl ObjectGcReplicaObservation {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Deleted => "DELETED",
            Self::AlreadyAbsent => "ALREADY_ABSENT",
            Self::ReconciledAbsent => "RECONCILED_ABSENT",
            Self::StillPresent => "STILL_PRESENT",
            Self::DeleteInProgress => "DELETE_IN_PROGRESS",
            Self::Unknown => "UNKNOWN",
            Self::Mismatch => "MISMATCH",
        }
    }

    const fn error_code(self) -> Option<&'static str> {
        match self {
            Self::Deleted | Self::AlreadyAbsent | Self::ReconciledAbsent => None,
            Self::StillPresent => Some("replica_still_present"),
            Self::DeleteInProgress => Some("replica_delete_in_progress"),
            Self::Unknown => Some("replica_outcome_unknown"),
            Self::Mismatch => Some("replica_evidence_mismatch"),
        }
    }

    const fn action_state(self) -> ObjectGcReplicaActionState {
        match self {
            Self::Deleted | Self::AlreadyAbsent | Self::ReconciledAbsent => {
                ObjectGcReplicaActionState::Deleted
            }
            Self::StillPresent | Self::DeleteInProgress => ObjectGcReplicaActionState::Retryable,
            Self::Unknown => ObjectGcReplicaActionState::ReconciliationRequired,
            Self::Mismatch => ObjectGcReplicaActionState::Failed,
        }
    }
}

/// Safe operation projection. Lease tokens, storage keys, hashes, and backend
/// versions are deliberately absent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectGcOperation {
    operation_id: ObjectGcOperationId,
    object_id: ObjectId,
    dedup_domain_id: DedupDomainId,
    candidate_generation: u64,
    lease_generation: u64,
    state: ObjectGcExecutionState,
    started_at: Timestamp,
    updated_at: Timestamp,
    completed_at: Option<Timestamp>,
    replica_count: u32,
    deleted_replica_count: u32,
}

impl ObjectGcOperation {
    #[must_use]
    pub const fn operation_id(self) -> ObjectGcOperationId {
        self.operation_id
    }

    #[must_use]
    pub const fn object_id(self) -> ObjectId {
        self.object_id
    }

    #[must_use]
    pub const fn dedup_domain_id(self) -> DedupDomainId {
        self.dedup_domain_id
    }

    #[must_use]
    pub const fn candidate_generation(self) -> u64 {
        self.candidate_generation
    }

    #[must_use]
    pub const fn lease_generation(self) -> u64 {
        self.lease_generation
    }

    #[must_use]
    pub const fn state(self) -> ObjectGcExecutionState {
        self.state
    }

    #[must_use]
    pub const fn started_at(self) -> Timestamp {
        self.started_at
    }

    #[must_use]
    pub const fn updated_at(self) -> Timestamp {
        self.updated_at
    }

    #[must_use]
    pub const fn completed_at(self) -> Option<Timestamp> {
        self.completed_at
    }

    #[must_use]
    pub const fn replica_count(self) -> u32 {
        self.replica_count
    }

    #[must_use]
    pub const fn deleted_replica_count(self) -> u32 {
        self.deleted_replica_count
    }
}

/// Internal action evidence handed only to the storage application boundary.
/// Debug formatting redacts the opaque key, checksum, and backend version.
#[derive(Clone, Eq, PartialEq)]
pub struct ObjectGcReplicaAction {
    operation_id: ObjectGcOperationId,
    replica_id: ObjectReplicaId,
    ordinal: u32,
    backend_kind: String,
    storage_key: String,
    expected_stored_length: u64,
    expected_stored_sha256: Sha256Digest,
    expected_backend_version: Option<String>,
    state: ObjectGcReplicaActionState,
    action_generation: u64,
}

impl fmt::Debug for ObjectGcReplicaAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ObjectGcReplicaAction")
            .field("operation_id", &self.operation_id)
            .field("replica_id", &self.replica_id)
            .field("ordinal", &self.ordinal)
            .field("backend_kind", &self.backend_kind)
            .field("storage_key", &"<redacted>")
            .field("expected_stored_length", &self.expected_stored_length)
            .field("expected_stored_sha256", &"<redacted>")
            .field(
                "expected_backend_version",
                &self.expected_backend_version.as_ref().map(|_| "<redacted>"),
            )
            .field("state", &self.state)
            .field("action_generation", &self.action_generation)
            .finish()
    }
}

impl ObjectGcReplicaAction {
    #[must_use]
    pub const fn operation_id(&self) -> ObjectGcOperationId {
        self.operation_id
    }

    #[must_use]
    pub const fn replica_id(&self) -> ObjectReplicaId {
        self.replica_id
    }

    #[must_use]
    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }

    #[must_use]
    pub fn backend_kind(&self) -> &str {
        &self.backend_kind
    }

    #[must_use]
    pub fn storage_key(&self) -> &str {
        &self.storage_key
    }

    #[must_use]
    pub const fn expected_stored_length(&self) -> u64 {
        self.expected_stored_length
    }

    #[must_use]
    pub const fn expected_stored_sha256(&self) -> Sha256Digest {
        self.expected_stored_sha256
    }

    #[must_use]
    pub fn expected_backend_version(&self) -> Option<&str> {
        self.expected_backend_version.as_deref()
    }

    #[must_use]
    pub const fn state(&self) -> ObjectGcReplicaActionState {
        self.state
    }

    #[must_use]
    pub const fn action_generation(&self) -> u64 {
        self.action_generation
    }
}

/// Directive returned by the final short transaction immediately before an
/// external action. A reconciliation directive never authorizes deletion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectGcReplicaDirective {
    Delete(ObjectGcReplicaAction),
    Reconcile(ObjectGcReplicaAction),
    /// All remaining actions are intentionally deferred until their durable
    /// PostgreSQL-clock retry deadline. This never authorizes a deletion.
    Deferred(ObjectGcOperation),
    NoRemaining(ObjectGcOperation),
}

/// Sanitized failures at the PostgreSQL physical-GC metadata boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectGcExecutionMetadataError {
    CandidateNotFound,
    ReadyRequired,
    StaleLease,
    LeaseExpired,
    ReferenceExists,
    HoldExists,
    OperationNotFound,
    InvalidState,
    ReplicaMetadataMismatch,
    StaleAction,
    NeedsAttention,
    Database(DatabaseError),
    InvalidPersistedData,
}

impl fmt::Display for ObjectGcExecutionMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::CandidateNotFound => "physical GC candidate was not found",
            Self::ReadyRequired => "physical GC candidate is not ready",
            Self::StaleLease => "physical GC lease is stale",
            Self::LeaseExpired => "physical GC lease has expired",
            Self::ReferenceExists => "physical GC object has a committed reference",
            Self::HoldExists => "physical GC object has an active hold",
            Self::OperationNotFound => "physical GC operation was not found",
            Self::InvalidState => "physical GC resource is in an invalid state",
            Self::ReplicaMetadataMismatch => "physical GC replica evidence is inconsistent",
            Self::StaleAction => "physical GC replica action is stale",
            Self::NeedsAttention => "physical GC operation needs operator attention",
            Self::Database(error) => return error.fmt(formatter),
            Self::InvalidPersistedData => "physical GC persisted metadata is invalid",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ObjectGcExecutionMetadataError {}

/// Mockable metadata port used by the storage application service.
#[async_trait]
pub trait ObjectGcExecutionMetadataBackend: Send + Sync {
    async fn renew_execution_lease(
        &self,
        lease: ObjectGcLease,
    ) -> Result<ObjectGcLease, ObjectGcExecutionMetadataError>;

    async fn start_gc_execution(
        &self,
        lease: ObjectGcLease,
    ) -> Result<ObjectGcOperation, ObjectGcExecutionMetadataError>;

    async fn resume_gc_execution(
        &self,
        lease: ObjectGcLease,
    ) -> Result<ObjectGcOperation, ObjectGcExecutionMetadataError>;

    async fn fence_next_replica(
        &self,
        operation_id: ObjectGcOperationId,
        lease: ObjectGcLease,
    ) -> Result<ObjectGcReplicaDirective, ObjectGcExecutionMetadataError>;

    /// Repeat the canonical candidate/Object/reference/hold/action proof for
    /// the exact fenced action immediately before the caller invokes the
    /// external delete. This does not create a new action generation.
    async fn authorize_replica_delete(
        &self,
        action: &ObjectGcReplicaAction,
        lease: ObjectGcLease,
    ) -> Result<(), ObjectGcExecutionMetadataError>;

    async fn record_replica_observation(
        &self,
        action: &ObjectGcReplicaAction,
        lease: ObjectGcLease,
        observation: ObjectGcReplicaObservation,
        retry_policy: Option<GcWorkerRetryPolicy>,
    ) -> Result<ObjectGcOperation, ObjectGcExecutionMetadataError>;

    /// Persist an operation-level manual-intervention state when the worker
    /// can prove an unsafe, non-retryable composition failure before an action
    /// observation exists. A recovery worker may bind a previously leased
    /// unfinished operation to its currently claimed READY lease as part of
    /// this terminal transition. The error code must be a stable,
    /// non-sensitive internal classification.
    async fn mark_gc_execution_needs_attention(
        &self,
        operation_id: ObjectGcOperationId,
        lease: ObjectGcLease,
        error_code: &'static str,
    ) -> Result<ObjectGcOperation, ObjectGcExecutionMetadataError>;

    async fn complete_gc_execution(
        &self,
        operation_id: ObjectGcOperationId,
        lease: ObjectGcLease,
    ) -> Result<ObjectGcOperation, ObjectGcExecutionMetadataError>;

    async fn load_gc_execution(
        &self,
        operation_id: ObjectGcOperationId,
    ) -> Result<ObjectGcOperation, ObjectGcExecutionMetadataError>;
}

/// PostgreSQL implementation of the durable physical-GC metadata port.
#[derive(Clone)]
pub struct PostgresObjectGcExecutionRepository {
    pool: DatabasePool,
    policy: ObjectGcPolicy,
}

impl PostgresObjectGcExecutionRepository {
    #[must_use]
    pub fn new(pool: DatabasePool, policy: ObjectGcPolicy) -> Self {
        Self { pool, policy }
    }

    async fn start_or_resume(
        &self,
        lease: ObjectGcLease,
        allow_rebind: bool,
    ) -> Result<ObjectGcOperation, ObjectGcExecutionMetadataError> {
        if lease.state() != ObjectGcCandidateState::Ready {
            return Err(ObjectGcExecutionMetadataError::ReadyRequired);
        }

        let mut transaction = self.pool.sqlx_pool().begin().await.map_err(db_error)?;

        if let Some(existing) = load_operation_by_object(
            &mut transaction,
            lease.object_id(),
            lease.dedup_domain_id(),
            false,
        )
        .await?
            && existing.state == "COMPLETED"
        {
            let operation = map_operation(&mut transaction, existing).await?;
            transaction.commit().await.map_err(db_error)?;
            return Ok(operation);
        }

        lock_candidate(&mut transaction, lease).await?;
        lock_object_identity(&mut transaction, lease.object_id(), lease.dedup_domain_id()).await?;
        let object =
            load_object_for_update(&mut transaction, lease.object_id(), lease.dedup_domain_id())
                .await?;
        let now = database_now(&mut transaction).await?;
        validate_candidate(&mut transaction, lease, now).await?;
        ensure_no_references_or_holds(
            &mut transaction,
            lease.object_id(),
            lease.dedup_domain_id(),
            now,
        )
        .await?;

        if let Some(existing) = load_operation_by_object(
            &mut transaction,
            lease.object_id(),
            lease.dedup_domain_id(),
            true,
        )
        .await?
        {
            if existing.state == "COMPLETED" {
                let operation = map_operation(&mut transaction, existing).await?;
                transaction.commit().await.map_err(db_error)?;
                return Ok(operation);
            }
            if object.lifecycle_state != "GC_DELETING" {
                return Err(ObjectGcExecutionMetadataError::InvalidState);
            }
            if !allow_rebind
                && (existing.lease_id != lease.lease_id().into_uuid()
                    || parse_u64(
                        &existing.lease_generation,
                        "object_gc_operations.lease_generation",
                    )? != lease.lease_generation())
            {
                return Err(ObjectGcExecutionMetadataError::StaleLease);
            }
            validate_operation_replicas(&mut transaction, &existing).await?;
            if allow_rebind {
                sqlx::query(
                    "UPDATE object_gc_operations
                     SET lease_id = $2, lease_generation = $3::NUMERIC,
                         updated_at = $4
                     WHERE operation_id = $1 AND state <> 'COMPLETED'",
                )
                .bind(existing.operation_id)
                .bind(lease.lease_id().into_uuid())
                .bind(lease.lease_generation().to_string())
                .bind(now.as_offset_datetime())
                .execute(&mut *transaction)
                .await
                .map_err(db_error)?;
            }
            let row = load_operation_by_id(&mut transaction, existing.operation_id, false)
                .await?
                .ok_or(ObjectGcExecutionMetadataError::OperationNotFound)?;
            let operation = map_operation(&mut transaction, row).await?;
            transaction.commit().await.map_err(db_error)?;
            return Ok(operation);
        }

        if object.lifecycle_state != "AVAILABLE" {
            return Err(ObjectGcExecutionMetadataError::InvalidState);
        }
        let canonical_length = parse_u64(&object.plaintext_length, "objects.plaintext_length")?;
        let canonical_sha256 = Sha256Digest::try_from(object.canonical_hash.as_slice())
            .map_err(|_| ObjectGcExecutionMetadataError::InvalidPersistedData)?;
        let replicas =
            load_replicas_for_update(&mut transaction, lease.object_id(), lease.dedup_domain_id())
                .await?;
        for replica in &replicas {
            validate_start_replica(replica, canonical_length, canonical_sha256)?;
        }

        let operation_id = ObjectGcOperationId::new();
        sqlx::query(
            "INSERT INTO object_gc_operations
                (operation_id, object_id, object_dedup_domain_id,
                 candidate_generation, lease_id, lease_generation, state,
                 started_at, updated_at)
             VALUES ($1, $2, $3, $4::NUMERIC, $5, $6::NUMERIC,
                     'ACTIVE', $7, $7)",
        )
        .bind(operation_id.into_uuid())
        .bind(lease.object_id().into_uuid())
        .bind(lease.dedup_domain_id().into_uuid())
        .bind(lease.lease_generation().to_string())
        .bind(lease.lease_id().into_uuid())
        .bind(lease.lease_generation().to_string())
        .bind(now.as_offset_datetime())
        .execute(&mut *transaction)
        .await
        .map_err(db_error)?;

        for (ordinal, replica) in replicas.iter().enumerate() {
            let ordinal = i32::try_from(ordinal)
                .map_err(|_| ObjectGcExecutionMetadataError::InvalidPersistedData)?;
            sqlx::query(
                "INSERT INTO object_gc_replica_actions
                    (operation_id, replica_id, ordinal, backend_kind, storage_key,
                     expected_stored_length, expected_stored_sha256,
                     expected_backend_version, state, action_generation, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6::NUMERIC, $7, $8,
                         'PENDING', 0, $9)",
            )
            .bind(operation_id.into_uuid())
            .bind(replica.id)
            .bind(ordinal)
            .bind(&replica.backend_kind)
            .bind(&replica.storage_key)
            .bind(&replica.stored_length)
            .bind(&replica.stored_sha256)
            .bind(&replica.backend_version)
            .bind(now.as_offset_datetime())
            .execute(&mut *transaction)
            .await
            .map_err(db_error)?;
        }

        sqlx::query(
            "UPDATE object_replicas
             SET state = 'DELETING'
             WHERE object_id = $1 AND object_dedup_domain_id = $2
               AND state = 'VERIFIED'",
        )
        .bind(lease.object_id().into_uuid())
        .bind(lease.dedup_domain_id().into_uuid())
        .execute(&mut *transaction)
        .await
        .map_err(db_error)?;
        sqlx::query(
            "UPDATE objects SET lifecycle_state = 'GC_DELETING'
             WHERE id = $1 AND dedup_domain_id = $2
               AND lifecycle_state = 'AVAILABLE'",
        )
        .bind(lease.object_id().into_uuid())
        .bind(lease.dedup_domain_id().into_uuid())
        .execute(&mut *transaction)
        .await
        .map_err(db_error)?;

        let row = load_operation_by_id(&mut transaction, operation_id.into_uuid(), false)
            .await?
            .ok_or(ObjectGcExecutionMetadataError::OperationNotFound)?;
        let operation = map_operation(&mut transaction, row).await?;
        transaction.commit().await.map_err(db_error)?;
        Ok(operation)
    }
}

#[async_trait]
impl ObjectGcExecutionMetadataBackend for PostgresObjectGcExecutionRepository {
    async fn renew_execution_lease(
        &self,
        lease: ObjectGcLease,
    ) -> Result<ObjectGcLease, ObjectGcExecutionMetadataError> {
        ObjectGcPlanningService::new(self.pool.clone(), self.policy)
            .renew_lease(lease)
            .await
            .map_err(map_planning_error)
    }

    async fn start_gc_execution(
        &self,
        lease: ObjectGcLease,
    ) -> Result<ObjectGcOperation, ObjectGcExecutionMetadataError> {
        self.start_or_resume(lease, false).await
    }

    async fn resume_gc_execution(
        &self,
        lease: ObjectGcLease,
    ) -> Result<ObjectGcOperation, ObjectGcExecutionMetadataError> {
        self.start_or_resume(lease, true).await
    }

    async fn fence_next_replica(
        &self,
        operation_id: ObjectGcOperationId,
        lease: ObjectGcLease,
    ) -> Result<ObjectGcReplicaDirective, ObjectGcExecutionMetadataError> {
        if lease.state() != ObjectGcCandidateState::Ready {
            return Err(ObjectGcExecutionMetadataError::ReadyRequired);
        }
        let mut transaction = self.pool.sqlx_pool().begin().await.map_err(db_error)?;
        lock_candidate(&mut transaction, lease).await?;
        lock_object_identity(&mut transaction, lease.object_id(), lease.dedup_domain_id()).await?;
        let object =
            load_object_for_update(&mut transaction, lease.object_id(), lease.dedup_domain_id())
                .await?;
        let now = database_now(&mut transaction).await?;
        validate_candidate(&mut transaction, lease, now).await?;
        ensure_no_references_or_holds(
            &mut transaction,
            lease.object_id(),
            lease.dedup_domain_id(),
            now,
        )
        .await?;
        if object.lifecycle_state != "GC_DELETING" {
            return Err(ObjectGcExecutionMetadataError::InvalidState);
        }

        let operation_row = load_operation_by_id(&mut transaction, operation_id.into_uuid(), true)
            .await?
            .ok_or(ObjectGcExecutionMetadataError::OperationNotFound)?;
        validate_operation_identity(&operation_row, lease)?;
        if operation_row.state == "COMPLETED" {
            let operation = map_operation(&mut transaction, operation_row).await?;
            transaction.commit().await.map_err(db_error)?;
            return Ok(ObjectGcReplicaDirective::NoRemaining(operation));
        }
        if operation_row.state == "NEEDS_ATTENTION" {
            transaction.commit().await.map_err(db_error)?;
            return Err(ObjectGcExecutionMetadataError::NeedsAttention);
        }

        let Some(mut action_row) = sqlx::query_as::<_, ReplicaActionRow>(
            "SELECT operation_id, replica_id, ordinal, backend_kind, storage_key,
                    expected_stored_length::TEXT AS expected_stored_length,
                    expected_stored_sha256, expected_backend_version, state,
                    action_generation::TEXT AS action_generation, last_outcome,
                    attempt_count, next_attempt_at
             FROM object_gc_replica_actions
             WHERE operation_id = $1 AND state <> 'DELETED'
             ORDER BY ordinal ASC
             LIMIT 1
             FOR UPDATE",
        )
        .bind(operation_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(db_error)?
        else {
            let operation = map_operation(&mut transaction, operation_row).await?;
            transaction.commit().await.map_err(db_error)?;
            return Ok(ObjectGcReplicaDirective::NoRemaining(operation));
        };

        validate_action_replica(&mut transaction, &action_row).await?;
        let state = ObjectGcReplicaActionState::parse(&action_row.state)?;
        if matches!(
            state,
            ObjectGcReplicaActionState::Retryable
                | ObjectGcReplicaActionState::ReconciliationRequired
        ) && action_row
            .next_attempt_at
            .is_some_and(|next_attempt_at| next_attempt_at > now.as_offset_datetime())
        {
            let operation = map_operation(&mut transaction, operation_row).await?;
            transaction.commit().await.map_err(db_error)?;
            return Ok(ObjectGcReplicaDirective::Deferred(operation));
        }
        let directive = match state {
            ObjectGcReplicaActionState::Pending | ObjectGcReplicaActionState::Retryable => {
                let generation = parse_u64(
                    &action_row.action_generation,
                    "object_gc_replica_actions.action_generation",
                )?
                .checked_add(1)
                .ok_or(ObjectGcExecutionMetadataError::InvalidPersistedData)?;
                sqlx::query(
                    "UPDATE object_gc_replica_actions
                     SET state = 'DELETE_FENCED', action_generation = $3::NUMERIC,
                         last_attempt_at = $4, updated_at = $4,
                         attempt_count = attempt_count + 1, next_attempt_at = NULL,
                         last_outcome = NULL, last_error_code = NULL
                     WHERE operation_id = $1 AND replica_id = $2",
                )
                .bind(operation_id.into_uuid())
                .bind(action_row.replica_id)
                .bind(generation.to_string())
                .bind(now.as_offset_datetime())
                .execute(&mut *transaction)
                .await
                .map_err(db_error)?;
                sqlx::query(
                    "UPDATE object_gc_operations
                     SET state = 'ACTIVE', updated_at = $2, last_error_code = NULL
                     WHERE operation_id = $1",
                )
                .bind(operation_id.into_uuid())
                .bind(now.as_offset_datetime())
                .execute(&mut *transaction)
                .await
                .map_err(db_error)?;
                action_row.state = "DELETE_FENCED".to_owned();
                action_row.action_generation = generation.to_string();
                action_row.attempt_count = action_row
                    .attempt_count
                    .checked_add(1)
                    .ok_or(ObjectGcExecutionMetadataError::InvalidPersistedData)?;
                action_row.next_attempt_at = None;
                ObjectGcReplicaDirective::Delete(map_action(action_row)?)
            }
            ObjectGcReplicaActionState::DeleteFenced
            | ObjectGcReplicaActionState::ReconciliationRequired => {
                let updated = sqlx::query(
                    "UPDATE object_gc_replica_actions
                     SET last_attempt_at = $3, updated_at = $3,
                         attempt_count = attempt_count + 1, next_attempt_at = NULL
                     WHERE operation_id = $1 AND replica_id = $2",
                )
                .bind(operation_id.into_uuid())
                .bind(action_row.replica_id)
                .bind(now.as_offset_datetime())
                .execute(&mut *transaction)
                .await
                .map_err(db_error)?;
                if updated.rows_affected() != 1 {
                    return Err(ObjectGcExecutionMetadataError::InvalidPersistedData);
                }
                sqlx::query(
                    "UPDATE object_gc_operations
                     SET state = 'ACTIVE', updated_at = $2, last_error_code = NULL
                     WHERE operation_id = $1 AND state <> 'COMPLETED'",
                )
                .bind(operation_id.into_uuid())
                .bind(now.as_offset_datetime())
                .execute(&mut *transaction)
                .await
                .map_err(db_error)?;
                action_row.attempt_count = action_row
                    .attempt_count
                    .checked_add(1)
                    .ok_or(ObjectGcExecutionMetadataError::InvalidPersistedData)?;
                action_row.next_attempt_at = None;
                ObjectGcReplicaDirective::Reconcile(map_action(action_row)?)
            }
            ObjectGcReplicaActionState::Failed => {
                return Err(ObjectGcExecutionMetadataError::NeedsAttention);
            }
            ObjectGcReplicaActionState::Deleted => {
                return Err(ObjectGcExecutionMetadataError::InvalidPersistedData);
            }
        };
        transaction.commit().await.map_err(db_error)?;
        Ok(directive)
    }

    async fn authorize_replica_delete(
        &self,
        action: &ObjectGcReplicaAction,
        lease: ObjectGcLease,
    ) -> Result<(), ObjectGcExecutionMetadataError> {
        if lease.state() != ObjectGcCandidateState::Ready {
            return Err(ObjectGcExecutionMetadataError::ReadyRequired);
        }

        let mut transaction = self.pool.sqlx_pool().begin().await.map_err(db_error)?;
        lock_candidate(&mut transaction, lease).await?;
        lock_object_identity(&mut transaction, lease.object_id(), lease.dedup_domain_id()).await?;
        let object =
            load_object_for_update(&mut transaction, lease.object_id(), lease.dedup_domain_id())
                .await?;
        let now = database_now(&mut transaction).await?;
        validate_candidate(&mut transaction, lease, now).await?;
        ensure_no_references_or_holds(
            &mut transaction,
            lease.object_id(),
            lease.dedup_domain_id(),
            now,
        )
        .await?;
        if object.lifecycle_state != "GC_DELETING" {
            return Err(ObjectGcExecutionMetadataError::InvalidState);
        }

        let operation_row =
            load_operation_by_id(&mut transaction, action.operation_id().into_uuid(), true)
                .await?
                .ok_or(ObjectGcExecutionMetadataError::OperationNotFound)?;
        validate_operation_identity(&operation_row, lease)?;
        if operation_row.state == "COMPLETED" {
            return Err(ObjectGcExecutionMetadataError::StaleAction);
        }

        let action_row = sqlx::query_as::<_, ReplicaActionRow>(
            "SELECT operation_id, replica_id, ordinal, backend_kind, storage_key,
                    expected_stored_length::TEXT AS expected_stored_length,
                    expected_stored_sha256, expected_backend_version, state,
                    action_generation::TEXT AS action_generation, last_outcome,
                    attempt_count, next_attempt_at
             FROM object_gc_replica_actions
             WHERE operation_id = $1 AND replica_id = $2
             FOR UPDATE",
        )
        .bind(action.operation_id().into_uuid())
        .bind(action.replica_id().into_uuid())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(db_error)?
        .ok_or(ObjectGcExecutionMetadataError::StaleAction)?;
        if ObjectGcReplicaActionState::parse(&action_row.state)?
            != ObjectGcReplicaActionState::DeleteFenced
            || parse_u64(
                &action_row.action_generation,
                "object_gc_replica_actions.action_generation",
            )? != action.action_generation()
        {
            return Err(ObjectGcExecutionMetadataError::StaleAction);
        }
        if !same_action(&action_row, action)? {
            return Err(ObjectGcExecutionMetadataError::ReplicaMetadataMismatch);
        }
        validate_action_replica(&mut transaction, &action_row).await?;

        transaction.commit().await.map_err(db_error)
    }

    async fn record_replica_observation(
        &self,
        action: &ObjectGcReplicaAction,
        lease: ObjectGcLease,
        observation: ObjectGcReplicaObservation,
        retry_policy: Option<GcWorkerRetryPolicy>,
    ) -> Result<ObjectGcOperation, ObjectGcExecutionMetadataError> {
        let mut transaction = self.pool.sqlx_pool().begin().await.map_err(db_error)?;
        // Completed replay is intentionally lease-independent: its Object and
        // candidate rows no longer exist. Do this non-locking look-up before
        // taking the canonical candidate -> Object -> operation lock order.
        let existing =
            load_operation_by_id(&mut transaction, action.operation_id().into_uuid(), false)
                .await?
                .ok_or(ObjectGcExecutionMetadataError::OperationNotFound)?;
        if existing.state == "COMPLETED" {
            let operation = map_operation(&mut transaction, existing).await?;
            transaction.commit().await.map_err(db_error)?;
            return Ok(operation);
        }
        if lease.state() != ObjectGcCandidateState::Ready {
            return Err(ObjectGcExecutionMetadataError::ReadyRequired);
        }
        lock_candidate(&mut transaction, lease).await?;
        lock_object_identity(&mut transaction, lease.object_id(), lease.dedup_domain_id()).await?;
        let object =
            load_object_for_update(&mut transaction, lease.object_id(), lease.dedup_domain_id())
                .await?;
        let now = database_now(&mut transaction).await?;
        validate_candidate(&mut transaction, lease, now).await?;
        ensure_no_references_or_holds(
            &mut transaction,
            lease.object_id(),
            lease.dedup_domain_id(),
            now,
        )
        .await?;
        if object.lifecycle_state != "GC_DELETING" {
            return Err(ObjectGcExecutionMetadataError::InvalidState);
        }
        let operation_row =
            load_operation_by_id(&mut transaction, action.operation_id().into_uuid(), true)
                .await?
                .ok_or(ObjectGcExecutionMetadataError::OperationNotFound)?;
        if operation_row.state == "COMPLETED" {
            let operation = map_operation(&mut transaction, operation_row).await?;
            transaction.commit().await.map_err(db_error)?;
            return Ok(operation);
        }
        validate_operation_identity(&operation_row, lease)?;
        let action_row = sqlx::query_as::<_, ReplicaActionRow>(
            "SELECT operation_id, replica_id, ordinal, backend_kind, storage_key,
                    expected_stored_length::TEXT AS expected_stored_length,
                    expected_stored_sha256, expected_backend_version, state,
                    action_generation::TEXT AS action_generation, last_outcome,
                    attempt_count, next_attempt_at
             FROM object_gc_replica_actions
             WHERE operation_id = $1 AND replica_id = $2
             FOR UPDATE",
        )
        .bind(action.operation_id().into_uuid())
        .bind(action.replica_id().into_uuid())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(db_error)?
        .ok_or(ObjectGcExecutionMetadataError::StaleAction)?;
        let stored_generation = parse_u64(
            &action_row.action_generation,
            "object_gc_replica_actions.action_generation",
        )?;
        if stored_generation != action.action_generation() {
            return Err(ObjectGcExecutionMetadataError::StaleAction);
        }
        let stored_state = ObjectGcReplicaActionState::parse(&action_row.state)?;
        if stored_state == ObjectGcReplicaActionState::Deleted {
            let operation = map_operation(&mut transaction, operation_row).await?;
            transaction.commit().await.map_err(db_error)?;
            return Ok(operation);
        }
        if !matches!(
            stored_state,
            ObjectGcReplicaActionState::DeleteFenced
                | ObjectGcReplicaActionState::ReconciliationRequired
        ) {
            return Err(ObjectGcExecutionMetadataError::StaleAction);
        }
        if !same_action(&action_row, action)? {
            return Err(ObjectGcExecutionMetadataError::ReplicaMetadataMismatch);
        }
        validate_action_replica(&mut transaction, &action_row).await?;
        let attempt_count = u32::try_from(action_row.attempt_count)
            .map_err(|_| ObjectGcExecutionMetadataError::InvalidPersistedData)?;
        let retryable_outcome = matches!(
            observation,
            ObjectGcReplicaObservation::StillPresent
                | ObjectGcReplicaObservation::DeleteInProgress
                | ObjectGcReplicaObservation::Unknown
        );
        let retry_exhausted = retryable_outcome
            && retry_policy.is_some_and(|policy| attempt_count >= policy.max_attempts());
        let next_state = if retry_exhausted {
            ObjectGcReplicaActionState::Failed
        } else {
            observation.action_state()
        };
        let next_attempt_at = if matches!(
            next_state,
            ObjectGcReplicaActionState::Retryable
                | ObjectGcReplicaActionState::ReconciliationRequired
        ) {
            retry_policy
                .map(|policy| {
                    let salt = retry_salt(action_row.operation_id, action_row.replica_id);
                    policy.delay_for_attempt(attempt_count, salt)
                })
                .map(|delay| {
                    now.checked_add_std(delay)
                        .ok_or(ObjectGcExecutionMetadataError::InvalidPersistedData)
                })
                .transpose()?
                .map(|timestamp| timestamp.as_offset_datetime())
        } else {
            None
        };
        let error_code = if retry_exhausted {
            Some("gc_retry_exhausted")
        } else {
            observation.error_code()
        };
        if next_state == ObjectGcReplicaActionState::Deleted {
            let deleted = sqlx::query(
                "DELETE FROM object_replicas
                 WHERE id = $1
                   AND object_id = $2
                   AND object_dedup_domain_id = $3
                   AND backend_kind = $4
                   AND storage_key = $5
                   AND stored_length = $6::NUMERIC
                   AND stored_sha256 = $7
                   AND backend_version IS NOT DISTINCT FROM $8
                   AND state = 'DELETING'",
            )
            .bind(action.replica_id().into_uuid())
            .bind(operation_row.object_id)
            .bind(operation_row.object_dedup_domain_id)
            .bind(action.backend_kind())
            .bind(action.storage_key())
            .bind(action.expected_stored_length().to_string())
            .bind(action.expected_stored_sha256().as_bytes())
            .bind(action.expected_backend_version())
            .execute(&mut *transaction)
            .await
            .map_err(db_error)?;
            if deleted.rows_affected() != 1 {
                return Err(ObjectGcExecutionMetadataError::ReplicaMetadataMismatch);
            }
        }

        let deleted_at =
            (next_state == ObjectGcReplicaActionState::Deleted).then_some(now.as_offset_datetime());
        sqlx::query(
            "UPDATE object_gc_replica_actions
             SET state = $3, updated_at = $4, deleted_at = $5,
                 last_outcome = $6, last_error_code = $7, next_attempt_at = $8
             WHERE operation_id = $1 AND replica_id = $2
               AND action_generation = $9::NUMERIC",
        )
        .bind(action.operation_id().into_uuid())
        .bind(action.replica_id().into_uuid())
        .bind(match next_state {
            ObjectGcReplicaActionState::Pending => "PENDING",
            ObjectGcReplicaActionState::DeleteFenced => "DELETE_FENCED",
            ObjectGcReplicaActionState::ReconciliationRequired => "RECONCILIATION_REQUIRED",
            ObjectGcReplicaActionState::Retryable => "RETRYABLE",
            ObjectGcReplicaActionState::Deleted => "DELETED",
            ObjectGcReplicaActionState::Failed => "FAILED",
        })
        .bind(now.as_offset_datetime())
        .bind(deleted_at)
        .bind(observation.as_str())
        .bind(error_code)
        .bind(next_attempt_at)
        .bind(action.action_generation().to_string())
        .execute(&mut *transaction)
        .await
        .map_err(db_error)?;

        let operation_state = match next_state {
            ObjectGcReplicaActionState::Failed => ObjectGcExecutionState::NeedsAttention,
            ObjectGcReplicaActionState::ReconciliationRequired
            | ObjectGcReplicaActionState::Retryable => ObjectGcExecutionState::RecoveryRequired,
            ObjectGcReplicaActionState::Pending
            | ObjectGcReplicaActionState::DeleteFenced
            | ObjectGcReplicaActionState::Deleted => ObjectGcExecutionState::Active,
        };
        sqlx::query(
            "UPDATE object_gc_operations
             SET state = $2, updated_at = $3, last_error_code = $4
             WHERE operation_id = $1 AND state <> 'COMPLETED'",
        )
        .bind(action.operation_id().into_uuid())
        .bind(operation_state.as_str())
        .bind(now.as_offset_datetime())
        .bind(error_code)
        .execute(&mut *transaction)
        .await
        .map_err(db_error)?;

        let row = load_operation_by_id(&mut transaction, action.operation_id().into_uuid(), false)
            .await?
            .ok_or(ObjectGcExecutionMetadataError::OperationNotFound)?;
        let operation = map_operation(&mut transaction, row).await?;
        transaction.commit().await.map_err(db_error)?;
        Ok(operation)
    }

    async fn mark_gc_execution_needs_attention(
        &self,
        operation_id: ObjectGcOperationId,
        lease: ObjectGcLease,
        error_code: &'static str,
    ) -> Result<ObjectGcOperation, ObjectGcExecutionMetadataError> {
        let mut transaction = self.pool.sqlx_pool().begin().await.map_err(db_error)?;
        if let Some(row) =
            load_operation_by_id(&mut transaction, operation_id.into_uuid(), false).await?
            && row.state == "COMPLETED"
        {
            let operation = map_operation(&mut transaction, row).await?;
            transaction.commit().await.map_err(db_error)?;
            return Ok(operation);
        }
        if lease.state() != ObjectGcCandidateState::Ready {
            return Err(ObjectGcExecutionMetadataError::ReadyRequired);
        }
        lock_candidate(&mut transaction, lease).await?;
        lock_object_identity(&mut transaction, lease.object_id(), lease.dedup_domain_id()).await?;
        let _object =
            load_object_for_update(&mut transaction, lease.object_id(), lease.dedup_domain_id())
                .await?;
        let now = database_now(&mut transaction).await?;
        validate_candidate(&mut transaction, lease, now).await?;
        let operation_row = load_operation_by_id(&mut transaction, operation_id.into_uuid(), true)
            .await?
            .ok_or(ObjectGcExecutionMetadataError::OperationNotFound)?;
        // A terminal recovery error can occur before `resume_gc_execution`
        // has committed the operation's replacement lease binding (for
        // example, while validating persisted replica evidence). The current
        // candidate lease has already been locked and validated above, so
        // require the canonical object identity but atomically rebind an
        // unfinished operation as it enters the safer terminal state.
        if operation_row.object_id != lease.object_id().into_uuid()
            || operation_row.object_dedup_domain_id != lease.dedup_domain_id().into_uuid()
        {
            return Err(ObjectGcExecutionMetadataError::InvalidState);
        }
        let updated = sqlx::query(
            "UPDATE object_gc_operations
             SET lease_id = $2, lease_generation = $3::NUMERIC,
                 state = 'NEEDS_ATTENTION', updated_at = $4, last_error_code = $5
             WHERE operation_id = $1 AND state <> 'COMPLETED'",
        )
        .bind(operation_id.into_uuid())
        .bind(lease.lease_id().into_uuid())
        .bind(lease.lease_generation().to_string())
        .bind(now.as_offset_datetime())
        .bind(error_code)
        .execute(&mut *transaction)
        .await
        .map_err(db_error)?;
        if updated.rows_affected() != 1 {
            return Err(ObjectGcExecutionMetadataError::InvalidPersistedData);
        }
        let row = load_operation_by_id(&mut transaction, operation_id.into_uuid(), false)
            .await?
            .ok_or(ObjectGcExecutionMetadataError::OperationNotFound)?;
        let operation = map_operation(&mut transaction, row).await?;
        transaction.commit().await.map_err(db_error)?;
        Ok(operation)
    }

    async fn complete_gc_execution(
        &self,
        operation_id: ObjectGcOperationId,
        lease: ObjectGcLease,
    ) -> Result<ObjectGcOperation, ObjectGcExecutionMetadataError> {
        let mut transaction = self.pool.sqlx_pool().begin().await.map_err(db_error)?;
        if let Some(row) =
            load_operation_by_id(&mut transaction, operation_id.into_uuid(), false).await?
            && row.state == "COMPLETED"
        {
            let operation = map_operation(&mut transaction, row).await?;
            transaction.commit().await.map_err(db_error)?;
            return Ok(operation);
        }
        if lease.state() != ObjectGcCandidateState::Ready {
            return Err(ObjectGcExecutionMetadataError::ReadyRequired);
        }
        lock_candidate(&mut transaction, lease).await?;
        lock_object_identity(&mut transaction, lease.object_id(), lease.dedup_domain_id()).await?;
        let object =
            load_object_for_update(&mut transaction, lease.object_id(), lease.dedup_domain_id())
                .await?;
        let now = database_now(&mut transaction).await?;
        validate_candidate(&mut transaction, lease, now).await?;
        ensure_no_references_or_holds(
            &mut transaction,
            lease.object_id(),
            lease.dedup_domain_id(),
            now,
        )
        .await?;
        if object.lifecycle_state != "GC_DELETING" {
            return Err(ObjectGcExecutionMetadataError::InvalidState);
        }
        let operation_row = load_operation_by_id(&mut transaction, operation_id.into_uuid(), true)
            .await?
            .ok_or(ObjectGcExecutionMetadataError::OperationNotFound)?;
        validate_operation_identity(&operation_row, lease)?;

        let remaining: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM object_gc_replica_actions
             WHERE operation_id = $1 AND state <> 'DELETED'",
        )
        .bind(operation_id.into_uuid())
        .fetch_one(&mut *transaction)
        .await
        .map_err(db_error)?;
        let replicas: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM object_replicas
             WHERE object_id = $1 AND object_dedup_domain_id = $2",
        )
        .bind(lease.object_id().into_uuid())
        .bind(lease.dedup_domain_id().into_uuid())
        .fetch_one(&mut *transaction)
        .await
        .map_err(db_error)?;
        if remaining != 0 || replicas != 0 {
            return Err(ObjectGcExecutionMetadataError::InvalidState);
        }

        let candidate_deleted = sqlx::query(
            "DELETE FROM object_gc_candidates
             WHERE object_id = $1 AND object_dedup_domain_id = $2
               AND source = 'METADATA_PURGE' AND state = 'READY'
               AND lease_id = $3 AND lease_generation = $4::NUMERIC",
        )
        .bind(lease.object_id().into_uuid())
        .bind(lease.dedup_domain_id().into_uuid())
        .bind(lease.lease_id().into_uuid())
        .bind(lease.lease_generation().to_string())
        .execute(&mut *transaction)
        .await
        .map_err(db_error)?;
        if candidate_deleted.rows_affected() != 1 {
            return Err(ObjectGcExecutionMetadataError::StaleLease);
        }
        let object_deleted = sqlx::query(
            "DELETE FROM objects
             WHERE id = $1 AND dedup_domain_id = $2
               AND lifecycle_state = 'GC_DELETING'",
        )
        .bind(lease.object_id().into_uuid())
        .bind(lease.dedup_domain_id().into_uuid())
        .execute(&mut *transaction)
        .await
        .map_err(db_error)?;
        if object_deleted.rows_affected() != 1 {
            return Err(ObjectGcExecutionMetadataError::InvalidState);
        }
        sqlx::query(
            "UPDATE object_gc_operations
             SET state = 'COMPLETED', updated_at = $2, completed_at = $2,
                 last_error_code = NULL
             WHERE operation_id = $1 AND state <> 'COMPLETED'",
        )
        .bind(operation_id.into_uuid())
        .bind(now.as_offset_datetime())
        .execute(&mut *transaction)
        .await
        .map_err(db_error)?;

        let row = load_operation_by_id(&mut transaction, operation_id.into_uuid(), false)
            .await?
            .ok_or(ObjectGcExecutionMetadataError::OperationNotFound)?;
        let operation = map_operation(&mut transaction, row).await?;
        transaction.commit().await.map_err(db_error)?;
        Ok(operation)
    }

    async fn load_gc_execution(
        &self,
        operation_id: ObjectGcOperationId,
    ) -> Result<ObjectGcOperation, ObjectGcExecutionMetadataError> {
        let mut transaction = self.pool.sqlx_pool().begin().await.map_err(db_error)?;
        let row = load_operation_by_id(&mut transaction, operation_id.into_uuid(), false)
            .await?
            .ok_or(ObjectGcExecutionMetadataError::OperationNotFound)?;
        let operation = map_operation(&mut transaction, row).await?;
        transaction.commit().await.map_err(db_error)?;
        Ok(operation)
    }
}

#[derive(Debug, FromRow)]
struct CandidateFenceRow {
    state: String,
    lease_id: Option<Uuid>,
    lease_generation: String,
    lease_expires_at: Option<time::OffsetDateTime>,
}

#[derive(Debug, FromRow)]
struct ObjectFenceRow {
    canonical_hash: Vec<u8>,
    plaintext_length: String,
    lifecycle_state: String,
}

#[derive(Clone, Debug, FromRow)]
struct OperationRow {
    operation_id: Uuid,
    object_id: Uuid,
    object_dedup_domain_id: Uuid,
    candidate_generation: String,
    lease_id: Uuid,
    lease_generation: String,
    state: String,
    started_at: time::OffsetDateTime,
    updated_at: time::OffsetDateTime,
    completed_at: Option<time::OffsetDateTime>,
}

#[derive(Clone, Debug, FromRow)]
struct ReplicaRow {
    id: Uuid,
    object_id: Uuid,
    object_dedup_domain_id: Uuid,
    backend_kind: String,
    storage_key: String,
    stored_length: String,
    stored_sha256: Vec<u8>,
    backend_version: Option<String>,
    state: String,
}

#[derive(Clone, Debug, FromRow)]
struct ReplicaActionRow {
    operation_id: Uuid,
    replica_id: Uuid,
    ordinal: i32,
    backend_kind: String,
    storage_key: String,
    expected_stored_length: String,
    expected_stored_sha256: Vec<u8>,
    expected_backend_version: Option<String>,
    state: String,
    action_generation: String,
    last_outcome: Option<String>,
    attempt_count: i32,
    next_attempt_at: Option<time::OffsetDateTime>,
}

async fn lock_candidate(
    transaction: &mut Transaction<'_, Postgres>,
    lease: ObjectGcLease,
) -> Result<(), ObjectGcExecutionMetadataError> {
    let exists = sqlx::query(
        "SELECT 1 FROM object_gc_candidates
         WHERE object_id = $1 AND object_dedup_domain_id = $2
         FOR UPDATE",
    )
    .bind(lease.object_id().into_uuid())
    .bind(lease.dedup_domain_id().into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(db_error)?
    .is_some();
    if !exists {
        return Err(ObjectGcExecutionMetadataError::CandidateNotFound);
    }
    Ok(())
}

async fn lock_object_identity(
    transaction: &mut Transaction<'_, Postgres>,
    object_id: ObjectId,
    dedup_domain_id: DedupDomainId,
) -> Result<(), ObjectGcExecutionMetadataError> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock(
            hashtextextended($1::UUID::TEXT || ':' || $2::UUID::TEXT, 0)
         )",
    )
    .bind(object_id.into_uuid())
    .bind(dedup_domain_id.into_uuid())
    .execute(&mut **transaction)
    .await
    .map_err(db_error)?;
    Ok(())
}

async fn validate_candidate(
    transaction: &mut Transaction<'_, Postgres>,
    lease: ObjectGcLease,
    now: Timestamp,
) -> Result<(), ObjectGcExecutionMetadataError> {
    let row = sqlx::query_as::<_, CandidateFenceRow>(
        "SELECT state, lease_id,
                lease_generation::TEXT AS lease_generation, lease_expires_at
         FROM object_gc_candidates
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(lease.object_id().into_uuid())
    .bind(lease.dedup_domain_id().into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(db_error)?
    .ok_or(ObjectGcExecutionMetadataError::CandidateNotFound)?;
    if row.state != "READY" {
        return Err(ObjectGcExecutionMetadataError::ReadyRequired);
    }
    if row.lease_id != Some(lease.lease_id().into_uuid())
        || parse_u64(
            &row.lease_generation,
            "object_gc_candidates.lease_generation",
        )? != lease.lease_generation()
    {
        return Err(ObjectGcExecutionMetadataError::StaleLease);
    }
    let expires_at = row
        .lease_expires_at
        .map(Timestamp::from_offset_datetime)
        .ok_or(ObjectGcExecutionMetadataError::InvalidPersistedData)?;
    if expires_at <= now {
        return Err(ObjectGcExecutionMetadataError::LeaseExpired);
    }
    Ok(())
}

async fn load_object_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    object_id: ObjectId,
    dedup_domain_id: DedupDomainId,
) -> Result<ObjectFenceRow, ObjectGcExecutionMetadataError> {
    sqlx::query_as::<_, ObjectFenceRow>(
        "SELECT canonical_hash, plaintext_length::TEXT AS plaintext_length,
                lifecycle_state
         FROM objects
         WHERE id = $1 AND dedup_domain_id = $2
         FOR UPDATE",
    )
    .bind(object_id.into_uuid())
    .bind(dedup_domain_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(db_error)?
    .ok_or(ObjectGcExecutionMetadataError::InvalidState)
}

async fn ensure_no_references_or_holds(
    transaction: &mut Transaction<'_, Postgres>,
    object_id: ObjectId,
    dedup_domain_id: DedupDomainId,
    now: Timestamp,
) -> Result<(), ObjectGcExecutionMetadataError> {
    let referenced: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM file_versions
            WHERE object_id = $1 AND object_dedup_domain_id = $2
         )",
    )
    .bind(object_id.into_uuid())
    .bind(dedup_domain_id.into_uuid())
    .fetch_one(&mut **transaction)
    .await
    .map_err(db_error)?;
    if referenced {
        return Err(ObjectGcExecutionMetadataError::ReferenceExists);
    }
    let held: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM object_gc_holds
            WHERE object_id = $1 AND object_dedup_domain_id = $2
              AND released_at IS NULL
              AND (expires_at IS NULL OR expires_at > $3)
         )",
    )
    .bind(object_id.into_uuid())
    .bind(dedup_domain_id.into_uuid())
    .bind(now.as_offset_datetime())
    .fetch_one(&mut **transaction)
    .await
    .map_err(db_error)?;
    if held {
        return Err(ObjectGcExecutionMetadataError::HoldExists);
    }
    Ok(())
}

async fn load_replicas_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    object_id: ObjectId,
    dedup_domain_id: DedupDomainId,
) -> Result<Vec<ReplicaRow>, ObjectGcExecutionMetadataError> {
    sqlx::query_as::<_, ReplicaRow>(
        "SELECT id, object_id, object_dedup_domain_id, backend_kind, storage_key,
                stored_length::TEXT AS stored_length, stored_sha256,
                backend_version, state
         FROM object_replicas
         WHERE object_id = $1 AND object_dedup_domain_id = $2
         ORDER BY backend_kind ASC, storage_key ASC, id ASC
         FOR UPDATE",
    )
    .bind(object_id.into_uuid())
    .bind(dedup_domain_id.into_uuid())
    .fetch_all(&mut **transaction)
    .await
    .map_err(db_error)
}

fn validate_start_replica(
    replica: &ReplicaRow,
    canonical_length: u64,
    canonical_sha256: Sha256Digest,
) -> Result<(), ObjectGcExecutionMetadataError> {
    ObjectReplicaId::try_from_uuid(replica.id)
        .map_err(|_| ObjectGcExecutionMetadataError::InvalidPersistedData)?;
    ObjectId::try_from_uuid(replica.object_id)
        .map_err(|_| ObjectGcExecutionMetadataError::InvalidPersistedData)?;
    DedupDomainId::try_from_uuid(replica.object_dedup_domain_id)
        .map_err(|_| ObjectGcExecutionMetadataError::InvalidPersistedData)?;
    if replica.state != "VERIFIED"
        || !matches!(
            replica.backend_kind.as_str(),
            "LOCAL_FILESYSTEM" | "OBJECT_STORE"
        )
        || !valid_storage_key(&replica.storage_key)
        || parse_u64(&replica.stored_length, "object_replicas.stored_length")? != canonical_length
        || Sha256Digest::try_from(replica.stored_sha256.as_slice())
            .map_err(|_| ObjectGcExecutionMetadataError::InvalidPersistedData)?
            != canonical_sha256
        || replica
            .backend_version
            .as_ref()
            .is_some_and(String::is_empty)
    {
        return Err(ObjectGcExecutionMetadataError::ReplicaMetadataMismatch);
    }
    Ok(())
}

async fn validate_operation_replicas(
    transaction: &mut Transaction<'_, Postgres>,
    operation: &OperationRow,
) -> Result<(), ObjectGcExecutionMetadataError> {
    let actions = sqlx::query_as::<_, ReplicaActionRow>(
        "SELECT operation_id, replica_id, ordinal, backend_kind, storage_key,
                expected_stored_length::TEXT AS expected_stored_length,
                expected_stored_sha256, expected_backend_version, state,
                action_generation::TEXT AS action_generation, last_outcome,
                attempt_count, next_attempt_at
         FROM object_gc_replica_actions
         WHERE operation_id = $1
         ORDER BY ordinal ASC
         FOR UPDATE",
    )
    .bind(operation.operation_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(db_error)?;
    for action in &actions {
        if ObjectGcReplicaActionState::parse(&action.state)? == ObjectGcReplicaActionState::Deleted
        {
            let still_present: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM object_replicas WHERE id = $1)")
                    .bind(action.replica_id)
                    .fetch_one(&mut **transaction)
                    .await
                    .map_err(db_error)?;
            if still_present {
                return Err(ObjectGcExecutionMetadataError::ReplicaMetadataMismatch);
            }
        } else {
            validate_action_replica(transaction, action).await?;
        }
    }
    let extras: i64 = sqlx::query_scalar(
        "SELECT count(*)
         FROM object_replicas AS replica
         WHERE replica.object_id = $1
           AND replica.object_dedup_domain_id = $2
           AND NOT EXISTS (
               SELECT 1 FROM object_gc_replica_actions AS action
               WHERE action.operation_id = $3 AND action.replica_id = replica.id
           )",
    )
    .bind(operation.object_id)
    .bind(operation.object_dedup_domain_id)
    .bind(operation.operation_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(db_error)?;
    if extras != 0 {
        return Err(ObjectGcExecutionMetadataError::ReplicaMetadataMismatch);
    }
    Ok(())
}

async fn validate_action_replica(
    transaction: &mut Transaction<'_, Postgres>,
    action: &ReplicaActionRow,
) -> Result<(), ObjectGcExecutionMetadataError> {
    let matches: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1
            FROM object_replicas AS replica
            JOIN object_gc_operations AS operation
              ON operation.operation_id = $1
            WHERE replica.id = $2
              AND replica.object_id = operation.object_id
              AND replica.object_dedup_domain_id = operation.object_dedup_domain_id
              AND replica.backend_kind = $3
              AND replica.storage_key = $4
              AND replica.stored_length = $5::NUMERIC
              AND replica.stored_sha256 = $6
              AND replica.backend_version IS NOT DISTINCT FROM $7
              AND replica.state = 'DELETING'
         )",
    )
    .bind(action.operation_id)
    .bind(action.replica_id)
    .bind(&action.backend_kind)
    .bind(&action.storage_key)
    .bind(&action.expected_stored_length)
    .bind(&action.expected_stored_sha256)
    .bind(&action.expected_backend_version)
    .fetch_one(&mut **transaction)
    .await
    .map_err(db_error)?;
    if !matches {
        return Err(ObjectGcExecutionMetadataError::ReplicaMetadataMismatch);
    }
    Ok(())
}

fn validate_operation_identity(
    operation: &OperationRow,
    lease: ObjectGcLease,
) -> Result<(), ObjectGcExecutionMetadataError> {
    if operation.object_id != lease.object_id().into_uuid()
        || operation.object_dedup_domain_id != lease.dedup_domain_id().into_uuid()
    {
        return Err(ObjectGcExecutionMetadataError::InvalidState);
    }
    if operation.lease_id != lease.lease_id().into_uuid()
        || parse_u64(
            &operation.lease_generation,
            "object_gc_operations.lease_generation",
        )? != lease.lease_generation()
    {
        return Err(ObjectGcExecutionMetadataError::StaleLease);
    }
    Ok(())
}

fn same_action(
    row: &ReplicaActionRow,
    action: &ObjectGcReplicaAction,
) -> Result<bool, ObjectGcExecutionMetadataError> {
    Ok(row.operation_id == action.operation_id().into_uuid()
        && row.replica_id == action.replica_id().into_uuid()
        && row.backend_kind == action.backend_kind()
        && row.storage_key == action.storage_key()
        && parse_u64(
            &row.expected_stored_length,
            "object_gc_replica_actions.expected_stored_length",
        )? == action.expected_stored_length()
        && Sha256Digest::try_from(row.expected_stored_sha256.as_slice())
            .map_err(|_| ObjectGcExecutionMetadataError::InvalidPersistedData)?
            == action.expected_stored_sha256()
        && row.expected_backend_version.as_deref() == action.expected_backend_version())
}

async fn load_operation_by_object(
    transaction: &mut Transaction<'_, Postgres>,
    object_id: ObjectId,
    dedup_domain_id: DedupDomainId,
    for_update: bool,
) -> Result<Option<OperationRow>, ObjectGcExecutionMetadataError> {
    let suffix = if for_update { " FOR UPDATE" } else { "" };
    let query = format!(
        "SELECT operation_id, object_id, object_dedup_domain_id,
                candidate_generation::TEXT AS candidate_generation, lease_id,
                lease_generation::TEXT AS lease_generation, state, started_at,
                updated_at, completed_at
         FROM object_gc_operations
         WHERE object_id = $1 AND object_dedup_domain_id = $2{suffix}"
    );
    sqlx::query_as::<_, OperationRow>(&query)
        .bind(object_id.into_uuid())
        .bind(dedup_domain_id.into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(db_error)
}

async fn load_operation_by_id(
    transaction: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    for_update: bool,
) -> Result<Option<OperationRow>, ObjectGcExecutionMetadataError> {
    let suffix = if for_update { " FOR UPDATE" } else { "" };
    let query = format!(
        "SELECT operation_id, object_id, object_dedup_domain_id,
                candidate_generation::TEXT AS candidate_generation, lease_id,
                lease_generation::TEXT AS lease_generation, state, started_at,
                updated_at, completed_at
         FROM object_gc_operations
         WHERE operation_id = $1{suffix}"
    );
    sqlx::query_as::<_, OperationRow>(&query)
        .bind(operation_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(db_error)
}

async fn map_operation(
    transaction: &mut Transaction<'_, Postgres>,
    row: OperationRow,
) -> Result<ObjectGcOperation, ObjectGcExecutionMetadataError> {
    let counts = sqlx::query_as::<_, (i64, i64)>(
        "SELECT count(*), count(*) FILTER (WHERE state = 'DELETED')
         FROM object_gc_replica_actions
         WHERE operation_id = $1",
    )
    .bind(row.operation_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(db_error)?;
    let replica_count = u32::try_from(counts.0)
        .map_err(|_| ObjectGcExecutionMetadataError::InvalidPersistedData)?;
    let deleted_replica_count = u32::try_from(counts.1)
        .map_err(|_| ObjectGcExecutionMetadataError::InvalidPersistedData)?;
    if deleted_replica_count > replica_count {
        return Err(ObjectGcExecutionMetadataError::InvalidPersistedData);
    }
    let state = ObjectGcExecutionState::parse(&row.state)?;
    if (state == ObjectGcExecutionState::Completed) != row.completed_at.is_some() {
        return Err(ObjectGcExecutionMetadataError::InvalidPersistedData);
    }
    Ok(ObjectGcOperation {
        operation_id: ObjectGcOperationId::try_from_uuid(row.operation_id)
            .map_err(|_| ObjectGcExecutionMetadataError::InvalidPersistedData)?,
        object_id: ObjectId::try_from_uuid(row.object_id)
            .map_err(|_| ObjectGcExecutionMetadataError::InvalidPersistedData)?,
        dedup_domain_id: DedupDomainId::try_from_uuid(row.object_dedup_domain_id)
            .map_err(|_| ObjectGcExecutionMetadataError::InvalidPersistedData)?,
        candidate_generation: parse_u64(
            &row.candidate_generation,
            "object_gc_operations.candidate_generation",
        )?,
        lease_generation: parse_u64(
            &row.lease_generation,
            "object_gc_operations.lease_generation",
        )?,
        state,
        started_at: Timestamp::from_offset_datetime(row.started_at),
        updated_at: Timestamp::from_offset_datetime(row.updated_at),
        completed_at: row.completed_at.map(Timestamp::from_offset_datetime),
        replica_count,
        deleted_replica_count,
    })
}

fn map_action(
    row: ReplicaActionRow,
) -> Result<ObjectGcReplicaAction, ObjectGcExecutionMetadataError> {
    if row.ordinal < 0 || !valid_storage_key(&row.storage_key) {
        return Err(ObjectGcExecutionMetadataError::InvalidPersistedData);
    }
    let _ = &row.last_outcome;
    Ok(ObjectGcReplicaAction {
        operation_id: ObjectGcOperationId::try_from_uuid(row.operation_id)
            .map_err(|_| ObjectGcExecutionMetadataError::InvalidPersistedData)?,
        replica_id: ObjectReplicaId::try_from_uuid(row.replica_id)
            .map_err(|_| ObjectGcExecutionMetadataError::InvalidPersistedData)?,
        ordinal: u32::try_from(row.ordinal)
            .map_err(|_| ObjectGcExecutionMetadataError::InvalidPersistedData)?,
        backend_kind: row.backend_kind,
        storage_key: row.storage_key,
        expected_stored_length: parse_u64(
            &row.expected_stored_length,
            "object_gc_replica_actions.expected_stored_length",
        )?,
        expected_stored_sha256: Sha256Digest::try_from(row.expected_stored_sha256.as_slice())
            .map_err(|_| ObjectGcExecutionMetadataError::InvalidPersistedData)?,
        expected_backend_version: row.expected_backend_version,
        state: ObjectGcReplicaActionState::parse(&row.state)?,
        action_generation: parse_u64(
            &row.action_generation,
            "object_gc_replica_actions.action_generation",
        )?,
    })
}

fn retry_salt(operation_id: Uuid, replica_id: Uuid) -> u64 {
    let operation_bytes = operation_id.as_bytes();
    let replica_bytes = replica_id.as_bytes();
    let mut operation_prefix = [0_u8; 8];
    let mut replica_suffix = [0_u8; 8];
    operation_prefix.copy_from_slice(&operation_bytes[..8]);
    replica_suffix.copy_from_slice(&replica_bytes[8..]);
    u64::from_le_bytes(operation_prefix) ^ u64::from_le_bytes(replica_suffix)
}

fn valid_storage_key(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.contains("//")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_'))
        && value
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

fn parse_u64(value: &str, _field: &'static str) -> Result<u64, ObjectGcExecutionMetadataError> {
    Revision::from_str(value)
        .map(Revision::get)
        .map_err(|_| ObjectGcExecutionMetadataError::InvalidPersistedData)
}

async fn database_now(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<Timestamp, ObjectGcExecutionMetadataError> {
    sqlx::query_scalar::<_, time::OffsetDateTime>("SELECT clock_timestamp()")
        .fetch_one(&mut **transaction)
        .await
        .map(Timestamp::from_offset_datetime)
        .map_err(db_error)
}

fn db_error(_error: sqlx::Error) -> ObjectGcExecutionMetadataError {
    ObjectGcExecutionMetadataError::Database(DatabaseError::Failure(
        crate::DatabaseErrorKind::QueryFailed,
    ))
}

fn map_planning_error(error: ObjectGcError) -> ObjectGcExecutionMetadataError {
    match error {
        ObjectGcError::NotFound => ObjectGcExecutionMetadataError::CandidateNotFound,
        ObjectGcError::CandidateInvalidated => ObjectGcExecutionMetadataError::ReferenceExists,
        ObjectGcError::StaleLease => ObjectGcExecutionMetadataError::StaleLease,
        ObjectGcError::LeaseExpired => ObjectGcExecutionMetadataError::LeaseExpired,
        ObjectGcError::Database(error) => ObjectGcExecutionMetadataError::Database(error),
        ObjectGcError::InvalidRequest | ObjectGcError::InvalidPolicy => {
            ObjectGcExecutionMetadataError::InvalidState
        }
        ObjectGcError::InvalidPersistedData => ObjectGcExecutionMetadataError::InvalidPersistedData,
    }
}

#[cfg(test)]
mod tests {
    use super::{ObjectGcReplicaActionState, ObjectGcReplicaObservation, valid_storage_key};

    #[test]
    fn observations_have_closed_safe_state_and_error_mappings() {
        assert_eq!(
            ObjectGcReplicaObservation::Deleted.action_state(),
            ObjectGcReplicaActionState::Deleted
        );
        assert_eq!(
            ObjectGcReplicaObservation::Unknown.error_code(),
            Some("replica_outcome_unknown")
        );
        assert_eq!(ObjectGcReplicaObservation::Mismatch.as_str(), "MISMATCH");
    }

    #[test]
    fn persisted_storage_keys_remain_opaque_and_traversal_free() {
        assert!(valid_storage_key("objects/v1/server_generated_key"));
        for invalid in ["", "/absolute", "../outside", "a//b", "a\\b", "user name"] {
            assert!(!valid_storage_key(invalid));
        }
    }
}
