//! Durable PostgreSQL discovery and recovery claims for the internal GC worker.
//!
//! This module never opens an ObjectStore and never deletes physical bytes. It
//! owns only bounded, metadata-driven inspection plus reclamation of an expired
//! or released planning lease for an already durable physical-GC operation.

use std::fmt;

use sqlx::FromRow;
use synveil_core::{DedupDomainId, ObjectGcOperationId, ObjectGcPolicy, ObjectId, Timestamp};
use uuid::Uuid;

use crate::{
    DatabaseError, DatabasePool, GcLeaseId, MetadataError, ObjectGcCandidateState, ObjectGcLease,
    gc::{ObjectGcCandidateRow, map_object_gc_candidate_row},
};

/// The maximum number of rows any one metadata-only reconciliation category
/// may inspect in a cycle. The worker config can choose a smaller limit.
pub const MAX_GC_WORKER_RECONCILIATION_LIMIT: u32 = 100;

/// An opaque recovery claim binds an incomplete operation identity to the new
/// candidate lease that must be revalidated and rebound by Prompt 28 before
/// any replica action can proceed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectGcRecoveryClaim {
    operation_id: ObjectGcOperationId,
    lease: ObjectGcLease,
}

impl ObjectGcRecoveryClaim {
    #[must_use]
    pub const fn operation_id(self) -> ObjectGcOperationId {
        self.operation_id
    }

    #[must_use]
    pub const fn lease(self) -> ObjectGcLease {
        self.lease
    }
}

/// Bounded, metadata-only anomaly counts. The report intentionally contains
/// no storage keys, provider data, paths, checksums, or credentials.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ObjectGcReconciliationReport {
    inspected_limit: u32,
    gc_deleting_without_operation: u32,
    operation_without_candidate: u32,
    ready_candidate_without_operation: u32,
    terminal_cleanup_pending: u32,
    needs_attention: u32,
}

impl ObjectGcReconciliationReport {
    #[must_use]
    pub const fn inspected_limit(self) -> u32 {
        self.inspected_limit
    }

    #[must_use]
    pub const fn gc_deleting_without_operation(self) -> u32 {
        self.gc_deleting_without_operation
    }

    #[must_use]
    pub const fn operation_without_candidate(self) -> u32 {
        self.operation_without_candidate
    }

    #[must_use]
    pub const fn ready_candidate_without_operation(self) -> u32 {
        self.ready_candidate_without_operation
    }

    #[must_use]
    pub const fn terminal_cleanup_pending(self) -> u32 {
        self.terminal_cleanup_pending
    }

    #[must_use]
    pub const fn needs_attention(self) -> u32 {
        self.needs_attention
    }

    #[must_use]
    pub const fn total_findings(self) -> u32 {
        self.gc_deleting_without_operation
            + self.operation_without_candidate
            + self.ready_candidate_without_operation
            + self.terminal_cleanup_pending
            + self.needs_attention
    }
}

/// Stable, redacted failures at the worker metadata boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectGcWorkerMetadataError {
    InvalidRequest,
    CapacityUnavailable,
    Database(DatabaseError),
    InvalidPersistedData,
}

impl fmt::Display for ObjectGcWorkerMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidRequest => "GC worker metadata request is invalid",
            Self::CapacityUnavailable => "GC worker metadata capacity is unavailable",
            Self::Database(error) => return error.fmt(formatter),
            Self::InvalidPersistedData => "GC worker persisted metadata is invalid",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ObjectGcWorkerMetadataError {}

/// PostgreSQL-backed metadata port used only by the internal GC worker.
#[derive(Clone)]
pub struct PostgresObjectGcWorkerRepository {
    pool: DatabasePool,
    policy: ObjectGcPolicy,
}

impl PostgresObjectGcWorkerRepository {
    #[must_use]
    pub fn new(pool: DatabasePool, policy: ObjectGcPolicy) -> Self {
        Self { pool, policy }
    }

    /// Reclaim at most `limit` due incomplete operations in oldest-update
    /// order. A candidate with a live lease is never stolen. A released
    /// retryable/reconciliation action is claimed only after its durable,
    /// PostgreSQL-clock `next_attempt_at` deadline.
    pub async fn claim_recoverable_operations(
        &self,
        limit: u32,
    ) -> Result<Vec<ObjectGcRecoveryClaim>, ObjectGcWorkerMetadataError> {
        if !(1..=self.policy.max_batch_size()).contains(&limit) {
            return Err(ObjectGcWorkerMetadataError::InvalidRequest);
        }

        let mut transaction = self.pool.sqlx_pool().begin().await.map_err(db_error)?;
        let now = database_now(&mut transaction).await?;
        let grace_cutoff = now
            .checked_sub_std(self.policy.grace_period())
            .ok_or(ObjectGcWorkerMetadataError::InvalidPersistedData)?;
        let rows = sqlx::query_as::<_, RecoveryCandidateRow>(
            "SELECT candidate.object_id, candidate.object_dedup_domain_id,
                    candidate.unreferenced_at, candidate.source, candidate.state,
                    candidate.lease_id,
                    candidate.lease_generation::TEXT AS lease_generation,
                    candidate.lease_acquired_at, candidate.lease_expires_at,
                    candidate.validated_at, operation.operation_id
             FROM object_gc_candidates AS candidate
             JOIN object_gc_operations AS operation
               ON operation.object_id = candidate.object_id
              AND operation.object_dedup_domain_id = candidate.object_dedup_domain_id
             WHERE candidate.source = 'METADATA_PURGE'
               AND candidate.unreferenced_at <= $1
               AND operation.state IN ('ACTIVE', 'RECOVERY_REQUIRED')
               AND (
                    candidate.state = 'ELIGIBLE'
                    OR (
                        candidate.state IN ('LEASED', 'READY')
                        AND candidate.lease_expires_at <= $2
                    )
               )
               AND NOT EXISTS (
                    SELECT 1
                    FROM object_gc_replica_actions AS failed_action
                    WHERE failed_action.operation_id = operation.operation_id
                      AND failed_action.state = 'FAILED'
               )
               AND (
                    NOT EXISTS (
                        SELECT 1
                        FROM object_gc_replica_actions AS remaining_action
                        WHERE remaining_action.operation_id = operation.operation_id
                          AND remaining_action.state <> 'DELETED'
                    )
                    OR EXISTS (
                        SELECT 1
                        FROM object_gc_replica_actions AS due_action
                        WHERE due_action.operation_id = operation.operation_id
                          AND (
                              due_action.state IN ('PENDING', 'DELETE_FENCED')
                              OR (
                                  due_action.state IN (
                                      'RETRYABLE', 'RECONCILIATION_REQUIRED'
                                  )
                                  AND (
                                      due_action.next_attempt_at IS NULL
                                      OR due_action.next_attempt_at <= $2
                                  )
                              )
                          )
                    )
               )
             ORDER BY operation.updated_at ASC, operation.operation_id ASC
             LIMIT $3
             FOR UPDATE OF candidate SKIP LOCKED",
        )
        .bind(grace_cutoff.as_offset_datetime())
        .bind(now.as_offset_datetime())
        .bind(i64::from(limit))
        .fetch_all(&mut *transaction)
        .await
        .map_err(db_error)?;

        let mut claims = Vec::with_capacity(rows.len());
        for row in rows {
            let operation_id = ObjectGcOperationId::try_from_uuid(row.operation_id)
                .map_err(|_| ObjectGcWorkerMetadataError::InvalidPersistedData)?;
            let candidate = map_object_gc_candidate_row(ObjectGcCandidateRow {
                object_id: row.object_id,
                object_dedup_domain_id: row.object_dedup_domain_id,
                unreferenced_at: row.unreferenced_at,
                source: row.source,
                state: row.state,
                lease_id: row.lease_id,
                lease_generation: row.lease_generation,
                lease_acquired_at: row.lease_acquired_at,
                lease_expires_at: row.lease_expires_at,
                validated_at: row.validated_at,
            })
            .map_err(map_metadata_error)?;

            if !lock_object_identity(
                &mut transaction,
                candidate.object_id(),
                candidate.dedup_domain_id(),
            )
            .await?
            {
                mark_needs_attention(&mut transaction, operation_id, now, "gc_object_missing")
                    .await?;
                continue;
            }

            // Preserve the canonical lock order used by Prompt 28:
            // candidate -> Object -> operation/action. The initial candidate
            // selection intentionally does not lock `operation`; after any
            // wait on the Object row we lock and re-evaluate it under this
            // same order before issuing a replacement lease.
            let Some(operation) = lock_recovery_operation(&mut transaction, operation_id).await?
            else {
                continue;
            };
            let rechecked_at = database_now(&mut transaction).await?;
            if !matches!(operation.state.as_str(), "ACTIVE" | "RECOVERY_REQUIRED")
                || !operation_is_due(&mut transaction, operation_id, rechecked_at).await?
            {
                continue;
            }

            // A new reference is impossible through normal FileVersion/pin
            // lifecycle triggers after `GC_DELETING`, but persisted drift (or
            // a fault-injected backup pin) must stop rather than be reclaimed
            // and retried forever.
            if has_content_retention_reference(
                &mut transaction,
                candidate.object_id(),
                candidate.dedup_domain_id(),
            )
            .await?
            {
                mark_needs_attention(&mut transaction, operation_id, now, "gc_reference_conflict")
                    .await?;
                continue;
            }

            let claimed_at = database_now(&mut transaction).await?;
            let lease_expires_at = claimed_at
                .checked_add_std(self.policy.lease_duration())
                .ok_or(ObjectGcWorkerMetadataError::InvalidPersistedData)?;
            let lease_generation = candidate
                .lease_generation()
                .checked_add(1)
                .ok_or(ObjectGcWorkerMetadataError::InvalidPersistedData)?;
            let lease_id = GcLeaseId::new();
            let updated = sqlx::query(
                "UPDATE object_gc_candidates
                 SET state = 'LEASED', lease_id = $4,
                     lease_generation = $5::NUMERIC,
                     lease_acquired_at = $6, lease_expires_at = $7,
                     validated_at = $6
                 WHERE object_id = $1
                   AND object_dedup_domain_id = $2
                   AND source = 'METADATA_PURGE'
                   AND state = $3
                   AND lease_generation = $8::NUMERIC",
            )
            .bind(candidate.object_id().into_uuid())
            .bind(candidate.dedup_domain_id().into_uuid())
            .bind(candidate.state().as_str())
            .bind(lease_id.into_uuid())
            .bind(lease_generation.to_string())
            .bind(claimed_at.as_offset_datetime())
            .bind(lease_expires_at.as_offset_datetime())
            .bind(candidate.lease_generation().to_string())
            .execute(&mut *transaction)
            .await
            .map_err(db_error)?;
            if updated.rows_affected() != 1 {
                return Err(ObjectGcWorkerMetadataError::InvalidPersistedData);
            }
            claims.push(ObjectGcRecoveryClaim {
                operation_id,
                lease: ObjectGcLease::from_parts(
                    candidate.object_id(),
                    candidate.dedup_domain_id(),
                    lease_id,
                    lease_generation,
                    claimed_at,
                    lease_expires_at,
                    ObjectGcCandidateState::Leased,
                ),
            });
        }

        transaction.commit().await.map_err(db_error)?;
        Ok(claims)
    }

    /// Inspect a small bounded page of suspicious durable states. This is
    /// deliberately metadata-driven only: it never lists a storage root and
    /// never turns unknown physical bytes into delete work.
    pub async fn inspect_reconciliation(
        &self,
        limit: u32,
    ) -> Result<ObjectGcReconciliationReport, ObjectGcWorkerMetadataError> {
        if !(1..=MAX_GC_WORKER_RECONCILIATION_LIMIT).contains(&limit) {
            return Err(ObjectGcWorkerMetadataError::InvalidRequest);
        }
        let pool = self.pool.sqlx_pool();
        let now: time::OffsetDateTime = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(pool)
            .await
            .map_err(db_error)?;
        let limit = i64::from(limit);
        let gc_deleting_without_operation = bounded_count(
            pool,
            "SELECT 1
             FROM objects AS object
             WHERE object.lifecycle_state = 'GC_DELETING'
               AND NOT EXISTS (
                    SELECT 1 FROM object_gc_operations AS operation
                    WHERE operation.object_id = object.id
                      AND operation.object_dedup_domain_id = object.dedup_domain_id
               )
             ORDER BY object.id ASC
             LIMIT $1",
            limit,
        )
        .await?;
        let operation_without_candidate = bounded_count(
            pool,
            "SELECT 1
             FROM object_gc_operations AS operation
             WHERE operation.state IN ('ACTIVE', 'RECOVERY_REQUIRED')
               AND NOT EXISTS (
                    SELECT 1 FROM object_gc_candidates AS candidate
                    WHERE candidate.object_id = operation.object_id
                      AND candidate.object_dedup_domain_id = operation.object_dedup_domain_id
                      AND candidate.source = 'METADATA_PURGE'
               )
             ORDER BY operation.updated_at ASC, operation.operation_id ASC
             LIMIT $1",
            limit,
        )
        .await?;
        let ready_candidate_without_operation = bounded_count_with_now(
            pool,
            "SELECT 1
             FROM object_gc_candidates AS candidate
             WHERE candidate.source = 'METADATA_PURGE'
               AND candidate.state = 'READY'
               AND candidate.lease_expires_at <= $1
               AND NOT EXISTS (
                    SELECT 1 FROM object_gc_operations AS operation
                    WHERE operation.object_id = candidate.object_id
                      AND operation.object_dedup_domain_id = candidate.object_dedup_domain_id
                      AND operation.state <> 'COMPLETED'
               )
             ORDER BY candidate.lease_expires_at ASC, candidate.object_id ASC
             LIMIT $2",
            now,
            limit,
        )
        .await?;
        let terminal_cleanup_pending = bounded_count(
            pool,
            "SELECT 1
             FROM object_gc_operations AS operation
             WHERE operation.state IN ('ACTIVE', 'RECOVERY_REQUIRED')
               AND NOT EXISTS (
                    SELECT 1 FROM object_gc_replica_actions AS action
                    WHERE action.operation_id = operation.operation_id
                      AND action.state <> 'DELETED'
               )
             ORDER BY operation.updated_at ASC, operation.operation_id ASC
             LIMIT $1",
            limit,
        )
        .await?;
        let needs_attention = bounded_count(
            pool,
            "SELECT 1
             FROM object_gc_operations AS operation
             WHERE operation.state = 'NEEDS_ATTENTION'
                OR EXISTS (
                    SELECT 1 FROM object_gc_replica_actions AS action
                    WHERE action.operation_id = operation.operation_id
                      AND action.state = 'FAILED'
                )
             ORDER BY operation.updated_at ASC, operation.operation_id ASC
             LIMIT $1",
            limit,
        )
        .await?;

        Ok(ObjectGcReconciliationReport {
            inspected_limit: u32::try_from(limit)
                .map_err(|_| ObjectGcWorkerMetadataError::InvalidPersistedData)?,
            gc_deleting_without_operation,
            operation_without_candidate,
            ready_candidate_without_operation,
            terminal_cleanup_pending,
            needs_attention,
        })
    }
}

#[derive(Debug, FromRow)]
struct RecoveryCandidateRow {
    object_id: Uuid,
    object_dedup_domain_id: Uuid,
    unreferenced_at: time::OffsetDateTime,
    source: String,
    state: String,
    lease_id: Option<Uuid>,
    lease_generation: String,
    lease_acquired_at: Option<time::OffsetDateTime>,
    lease_expires_at: Option<time::OffsetDateTime>,
    validated_at: Option<time::OffsetDateTime>,
    operation_id: Uuid,
}

#[derive(Debug, FromRow)]
struct RecoveryOperationRow {
    state: String,
}

async fn lock_recovery_operation(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    operation_id: ObjectGcOperationId,
) -> Result<Option<RecoveryOperationRow>, ObjectGcWorkerMetadataError> {
    sqlx::query_as::<_, RecoveryOperationRow>(
        "SELECT state
         FROM object_gc_operations
         WHERE operation_id = $1
         FOR UPDATE",
    )
    .bind(operation_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(db_error)
}

async fn operation_is_due(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    operation_id: ObjectGcOperationId,
    now: Timestamp,
) -> Result<bool, ObjectGcWorkerMetadataError> {
    sqlx::query_scalar(
        "SELECT
            NOT EXISTS (
                SELECT 1
                FROM object_gc_replica_actions AS remaining_action
                WHERE remaining_action.operation_id = $1
                  AND remaining_action.state <> 'DELETED'
            )
            OR (
                NOT EXISTS (
                    SELECT 1
                    FROM object_gc_replica_actions AS failed_action
                    WHERE failed_action.operation_id = $1
                      AND failed_action.state = 'FAILED'
                )
                AND EXISTS (
                    SELECT 1
                    FROM object_gc_replica_actions AS due_action
                    WHERE due_action.operation_id = $1
                      AND (
                          due_action.state IN ('PENDING', 'DELETE_FENCED')
                          OR (
                              due_action.state IN ('RETRYABLE', 'RECONCILIATION_REQUIRED')
                              AND (
                                  due_action.next_attempt_at IS NULL
                                  OR due_action.next_attempt_at <= $2
                              )
                          )
                      )
                )
            )",
    )
    .bind(operation_id.into_uuid())
    .bind(now.as_offset_datetime())
    .fetch_one(&mut **transaction)
    .await
    .map_err(db_error)
}

async fn lock_object_identity(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    object_id: ObjectId,
    dedup_domain_id: DedupDomainId,
) -> Result<bool, ObjectGcWorkerMetadataError> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM objects
         WHERE id = $1 AND dedup_domain_id = $2
         FOR UPDATE",
    )
    .bind(object_id.into_uuid())
    .bind(dedup_domain_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map(|value| value.is_some())
    .map_err(db_error)
}

async fn has_content_retention_reference(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    object_id: ObjectId,
    dedup_domain_id: DedupDomainId,
) -> Result<bool, ObjectGcWorkerMetadataError> {
    sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM file_versions
            WHERE object_id = $1 AND object_dedup_domain_id = $2
            UNION ALL
            SELECT 1 FROM backup_snapshot_content_pins
            WHERE object_id = $1 AND object_dedup_domain_id = $2
         )",
    )
    .bind(object_id.into_uuid())
    .bind(dedup_domain_id.into_uuid())
    .fetch_one(&mut **transaction)
    .await
    .map_err(db_error)
}

async fn mark_needs_attention(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    operation_id: ObjectGcOperationId,
    now: Timestamp,
    error_code: &'static str,
) -> Result<(), ObjectGcWorkerMetadataError> {
    let result = sqlx::query(
        "UPDATE object_gc_operations
         SET state = 'NEEDS_ATTENTION', updated_at = $2, last_error_code = $3
         WHERE operation_id = $1 AND state <> 'COMPLETED'",
    )
    .bind(operation_id.into_uuid())
    .bind(now.as_offset_datetime())
    .bind(error_code)
    .execute(&mut **transaction)
    .await
    .map_err(db_error)?;
    if result.rows_affected() != 1 {
        return Err(ObjectGcWorkerMetadataError::InvalidPersistedData);
    }
    Ok(())
}

async fn bounded_count(
    pool: &sqlx::PgPool,
    inner_query: &str,
    limit: i64,
) -> Result<u32, ObjectGcWorkerMetadataError> {
    let query = format!("SELECT count(*) FROM ({inner_query}) AS bounded_rows");
    let count: i64 = sqlx::query_scalar(&query)
        .bind(limit)
        .fetch_one(pool)
        .await
        .map_err(db_error)?;
    u32::try_from(count).map_err(|_| ObjectGcWorkerMetadataError::InvalidPersistedData)
}

async fn bounded_count_with_now(
    pool: &sqlx::PgPool,
    inner_query: &str,
    now: time::OffsetDateTime,
    limit: i64,
) -> Result<u32, ObjectGcWorkerMetadataError> {
    let query = format!("SELECT count(*) FROM ({inner_query}) AS bounded_rows");
    let count: i64 = sqlx::query_scalar(&query)
        .bind(now)
        .bind(limit)
        .fetch_one(pool)
        .await
        .map_err(db_error)?;
    u32::try_from(count).map_err(|_| ObjectGcWorkerMetadataError::InvalidPersistedData)
}

async fn database_now(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<Timestamp, ObjectGcWorkerMetadataError> {
    sqlx::query_scalar::<_, time::OffsetDateTime>("SELECT clock_timestamp()")
        .fetch_one(&mut **transaction)
        .await
        .map(Timestamp::from_offset_datetime)
        .map_err(db_error)
}

fn map_metadata_error(error: MetadataError) -> ObjectGcWorkerMetadataError {
    match error {
        MetadataError::Database(error) => ObjectGcWorkerMetadataError::Database(error),
        MetadataError::CapacityUnavailable => ObjectGcWorkerMetadataError::CapacityUnavailable,
        MetadataError::Mapping(_) => ObjectGcWorkerMetadataError::InvalidPersistedData,
    }
}

fn db_error(_error: sqlx::Error) -> ObjectGcWorkerMetadataError {
    ObjectGcWorkerMetadataError::Database(DatabaseError::Failure(
        crate::DatabaseErrorKind::QueryFailed,
    ))
}
