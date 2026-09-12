//! Bounded, transport-neutral synchronization retention primitives.
//!
//! Prompt 86 intentionally exposes one-shot operations only. It does not own
//! a timer, worker, retry loop, HTTP endpoint, or device recovery policy.

use std::{fmt, time::Duration};

use sqlx::{FromRow, Postgres, Transaction};
use synveil_core::{LibraryId, Sequence, Timestamp, UserId};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    DatabaseError, DatabaseErrorKind, DatabasePool, MetadataError, journal::acquire_namespace_guard,
};

pub const DEFAULT_JOURNAL_RETENTION_SECONDS: u64 = 30 * 24 * 60 * 60;
pub const DEFAULT_HANDOFF_PROOF_RETENTION_SECONDS: u64 = 30 * 24 * 60 * 60;
pub const DEFAULT_JOURNAL_COMPACTION_BATCH_LIMIT: u32 = 10_000;
pub const DEFAULT_SNAPSHOT_PAYLOAD_CLEANUP_BATCH_LIMIT: u32 = 32;
pub const DEFAULT_HANDOFF_PROOF_CLEANUP_BATCH_LIMIT: u32 = 128;

pub const MAX_JOURNAL_COMPACTION_BATCH_LIMIT: u32 = 100_000;
pub const MAX_SNAPSHOT_PAYLOAD_CLEANUP_BATCH_LIMIT: u32 = 1_024;
pub const MAX_HANDOFF_PROOF_CLEANUP_BATCH_LIMIT: u32 = 4_096;

/// Validated Gen-1 retention policy used by one-shot maintenance calls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncRetentionPolicy {
    journal_retention: Duration,
    journal_batch_limit: u32,
    snapshot_batch_limit: u32,
    proof_batch_limit: u32,
}

impl SyncRetentionPolicy {
    pub fn new(
        journal_retention: Duration,
        journal_batch_limit: u32,
        snapshot_batch_limit: u32,
        proof_batch_limit: u32,
    ) -> Result<Self, SyncRetentionError> {
        if journal_retention.is_zero()
            || !(1..=MAX_JOURNAL_COMPACTION_BATCH_LIMIT).contains(&journal_batch_limit)
            || !(1..=MAX_SNAPSHOT_PAYLOAD_CLEANUP_BATCH_LIMIT).contains(&snapshot_batch_limit)
            || !(1..=MAX_HANDOFF_PROOF_CLEANUP_BATCH_LIMIT).contains(&proof_batch_limit)
        {
            return Err(SyncRetentionError::InvalidPolicy);
        }
        Ok(Self {
            journal_retention,
            journal_batch_limit,
            snapshot_batch_limit,
            proof_batch_limit,
        })
    }

    #[must_use]
    pub const fn journal_retention(self) -> Duration {
        self.journal_retention
    }

    #[must_use]
    pub const fn journal_batch_limit(self) -> u32 {
        self.journal_batch_limit
    }

    #[must_use]
    pub const fn snapshot_batch_limit(self) -> u32 {
        self.snapshot_batch_limit
    }

    #[must_use]
    pub const fn proof_batch_limit(self) -> u32 {
        self.proof_batch_limit
    }
}

impl Default for SyncRetentionPolicy {
    fn default() -> Self {
        Self {
            journal_retention: Duration::from_secs(DEFAULT_JOURNAL_RETENTION_SECONDS),
            journal_batch_limit: DEFAULT_JOURNAL_COMPACTION_BATCH_LIMIT,
            snapshot_batch_limit: DEFAULT_SNAPSHOT_PAYLOAD_CLEANUP_BATCH_LIMIT,
            proof_batch_limit: DEFAULT_HANDOFF_PROOF_CLEANUP_BATCH_LIMIT,
        }
    }
}

/// Safe, transport-neutral failures for bounded retention maintenance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncRetentionError {
    NotFound,
    InvalidPolicy,
    InvalidObservedTime,
    InvariantViolation,
    DependencyUnavailable,
    Database(DatabaseError),
}

impl fmt::Display for SyncRetentionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("sync retention scope was not found"),
            Self::InvalidPolicy => formatter.write_str("sync retention policy is invalid"),
            Self::InvalidObservedTime => {
                formatter.write_str("sync retention observed time is invalid")
            }
            Self::InvariantViolation => {
                formatter.write_str("sync retention persisted invariant is invalid")
            }
            Self::DependencyUnavailable => {
                formatter.write_str("sync retention dependency is unavailable")
            }
            Self::Database(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SyncRetentionError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JournalCompactionStepResult {
    library_id: LibraryId,
    journal_epoch: Sequence,
    previous_compacted_through: Sequence,
    new_compacted_through: Sequence,
    rows_deleted: u32,
    blocked_by_age: bool,
    blocked_by_handoff_proof: bool,
}

impl JournalCompactionStepResult {
    #[must_use]
    pub const fn library_id(self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn journal_epoch(self) -> Sequence {
        self.journal_epoch
    }

    #[must_use]
    pub const fn previous_compacted_through(self) -> Sequence {
        self.previous_compacted_through
    }

    #[must_use]
    pub const fn new_compacted_through(self) -> Sequence {
        self.new_compacted_through
    }

    #[must_use]
    pub const fn rows_deleted(self) -> u32 {
        self.rows_deleted
    }

    #[must_use]
    pub const fn blocked_by_age(self) -> bool {
        self.blocked_by_age
    }

    #[must_use]
    pub const fn blocked_by_handoff_proof(self) -> bool {
        self.blocked_by_handoff_proof
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotPayloadCleanupStepResult {
    payloads_deleted: u32,
    entries_deleted: u64,
}

impl SnapshotPayloadCleanupStepResult {
    #[must_use]
    pub const fn payloads_deleted(self) -> u32 {
        self.payloads_deleted
    }

    #[must_use]
    pub const fn entries_deleted(self) -> u64 {
        self.entries_deleted
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HandoffProofCleanupStepResult {
    proofs_deleted: u32,
    blocked_by_payload: bool,
}

impl HandoffProofCleanupStepResult {
    #[must_use]
    pub const fn proofs_deleted(self) -> u32 {
        self.proofs_deleted
    }

    #[must_use]
    pub const fn blocked_by_payload(self) -> bool {
        self.blocked_by_payload
    }
}

#[derive(Clone, Debug, FromRow)]
struct RetentionScopeRow {
    journal_epoch: i64,
    sync_head: i64,
    minimum_retained_sequence: i64,
}

#[derive(Clone, Debug, FromRow)]
struct JournalRetentionCandidateRow {
    sequence: i64,
    occurred_at: OffsetDateTime,
}

#[derive(Clone, Debug, FromRow)]
struct SnapshotCleanupCandidateRow {
    id: Uuid,
    entry_count: i64,
}

/// PostgreSQL-backed bounded retention service.
#[derive(Clone)]
pub struct SyncRetentionService {
    pool: DatabasePool,
    policy: SyncRetentionPolicy,
}

impl SyncRetentionService {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self {
            pool,
            policy: SyncRetentionPolicy::default(),
        }
    }

    #[must_use]
    pub const fn with_policy(pool: DatabasePool, policy: SyncRetentionPolicy) -> Self {
        Self { pool, policy }
    }

    #[must_use]
    pub const fn pool(&self) -> &DatabasePool {
        &self.pool
    }

    #[must_use]
    pub const fn policy(&self) -> SyncRetentionPolicy {
        self.policy
    }

    /// Delete at most one policy batch from the oldest contiguous, sufficiently
    /// old prefix of one library's current epoch.
    ///
    /// Canonical lock order is namespace advisory guard, library clock/floor
    /// row, proof observation, then journal rows. Snapshot creation takes the
    /// same first guard, so no new proof can publish a boundary already crossed
    /// by this transaction. Feed takes a short share lock on the library row;
    /// it therefore sees either the complete old prefix or the committed new
    /// floor. Append also takes the library row before assigning new sequences.
    pub async fn compact_journal_step(
        &self,
        owner_user_id: UserId,
        library_id: LibraryId,
        observed_at: Timestamp,
    ) -> Result<JournalCompactionStepResult, SyncRetentionError> {
        let cutoff = observed_at
            .checked_sub_std(self.policy.journal_retention)
            .ok_or(SyncRetentionError::InvalidObservedTime)?;
        let mut transaction = self.begin_transaction().await?;
        acquire_namespace_guard(&mut transaction, library_id)
            .await
            .map_err(map_metadata_error)?;

        let scope = sqlx::query_as::<_, RetentionScopeRow>(
            "SELECT journal_epoch, sync_head, minimum_retained_sequence
             FROM libraries
             WHERE id = $1
               AND owner_user_id = $2
               AND status <> 'DELETING'
             FOR UPDATE",
        )
        .bind(library_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?
        .ok_or(SyncRetentionError::NotFound)?;
        validate_scope(&scope)?;

        let proof_boundary = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT MIN(snapshot_resume_sequence)
             FROM rebaseline_snapshot_handoff_proofs
             WHERE library_id = $1
               AND journal_epoch = $2",
        )
        .bind(library_id.into_uuid())
        .bind(scope.journal_epoch)
        .fetch_one(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;
        if proof_boundary.is_some_and(|boundary| {
            boundary < scope.minimum_retained_sequence || boundary > scope.sync_head
        }) {
            return Err(SyncRetentionError::InvariantViolation);
        }

        let candidates = sqlx::query_as::<_, JournalRetentionCandidateRow>(
            "SELECT sequence, occurred_at
             FROM change_journal
             WHERE library_id = $1
               AND journal_epoch = $2
               AND sequence > $3
               AND sequence <= $4
             ORDER BY sequence ASC
             LIMIT $5",
        )
        .bind(library_id.into_uuid())
        .bind(scope.journal_epoch)
        .bind(scope.minimum_retained_sequence)
        .bind(scope.sync_head)
        .bind(i64::from(self.policy.journal_batch_limit))
        .fetch_all(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;

        if candidates.is_empty() && scope.minimum_retained_sequence < scope.sync_head {
            return Err(SyncRetentionError::InvariantViolation);
        }

        let mut age_target = scope.minimum_retained_sequence;
        let mut expected_sequence = scope
            .minimum_retained_sequence
            .checked_add(1)
            .ok_or(SyncRetentionError::InvariantViolation)?;
        let mut blocked_by_age = false;
        for candidate in &candidates {
            if candidate.sequence != expected_sequence {
                return Err(SyncRetentionError::InvariantViolation);
            }
            if candidate.occurred_at > cutoff.as_offset_datetime() {
                blocked_by_age = true;
                break;
            }
            age_target = candidate.sequence;
            expected_sequence = expected_sequence
                .checked_add(1)
                .ok_or(SyncRetentionError::InvariantViolation)?;
        }

        let safe_target = proof_boundary.map_or(age_target, |boundary| age_target.min(boundary));
        let blocked_by_handoff_proof = proof_boundary.is_some_and(|boundary| boundary < age_target);
        let expected_deleted = safe_target
            .checked_sub(scope.minimum_retained_sequence)
            .ok_or(SyncRetentionError::InvariantViolation)?;
        let rows_deleted = if expected_deleted == 0 {
            0
        } else {
            enable_retention_deletes(&mut transaction).await?;
            let deleted = sqlx::query(
                "DELETE FROM change_journal
                 WHERE library_id = $1
                   AND journal_epoch = $2
                   AND sequence > $3
                   AND sequence <= $4",
            )
            .bind(library_id.into_uuid())
            .bind(scope.journal_epoch)
            .bind(scope.minimum_retained_sequence)
            .bind(safe_target)
            .execute(&mut *transaction)
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?
            .rows_affected();
            if deleted != u64::try_from(expected_deleted).unwrap_or(u64::MAX) {
                return Err(SyncRetentionError::InvariantViolation);
            }
            let updated = sqlx::query(
                "UPDATE libraries
                 SET minimum_retained_sequence = $3
                 WHERE id = $1
                   AND journal_epoch = $2
                   AND minimum_retained_sequence = $4",
            )
            .bind(library_id.into_uuid())
            .bind(scope.journal_epoch)
            .bind(safe_target)
            .bind(scope.minimum_retained_sequence)
            .execute(&mut *transaction)
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?
            .rows_affected();
            if updated != 1 {
                return Err(SyncRetentionError::InvariantViolation);
            }
            u32::try_from(deleted).map_err(|_| SyncRetentionError::InvariantViolation)?
        };

        transaction
            .commit()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;

        Ok(JournalCompactionStepResult {
            library_id,
            journal_epoch: positive_sequence(scope.journal_epoch)?,
            previous_compacted_through: nonnegative_sequence(scope.minimum_retained_sequence)?,
            new_compacted_through: nonnegative_sequence(safe_target)?,
            rows_deleted,
            blocked_by_age,
            blocked_by_handoff_proof,
        })
    }

    /// Delete at most one bounded batch of logically expired snapshot payloads.
    /// Every selected header has an independent proof before any cascade runs.
    pub async fn cleanup_snapshot_payloads_step(
        &self,
        observed_at: Timestamp,
    ) -> Result<SnapshotPayloadCleanupStepResult, SyncRetentionError> {
        let mut transaction = self.begin_transaction().await?;
        let missing_proof = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1
                FROM rebaseline_snapshots AS s
                WHERE s.expires_at <= $1
                  AND NOT EXISTS (
                      SELECT 1
                      FROM rebaseline_snapshot_handoff_proofs AS p
                      WHERE p.snapshot_id = s.id
                  )
                LIMIT 1
             )",
        )
        .bind(observed_at.as_offset_datetime())
        .fetch_one(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;
        if missing_proof {
            return Err(SyncRetentionError::InvariantViolation);
        }

        let candidates = sqlx::query_as::<_, SnapshotCleanupCandidateRow>(
            "SELECT s.id, s.entry_count
             FROM rebaseline_snapshots AS s
             WHERE s.expires_at <= $1
               AND EXISTS (
                   SELECT 1
                   FROM rebaseline_snapshot_handoff_proofs AS p
                   WHERE p.snapshot_id = s.id
               )
             ORDER BY s.expires_at ASC, s.id ASC
             LIMIT $2
             FOR UPDATE OF s SKIP LOCKED",
        )
        .bind(observed_at.as_offset_datetime())
        .bind(i64::from(self.policy.snapshot_batch_limit))
        .fetch_all(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;
        let ids = candidates.iter().map(|row| row.id).collect::<Vec<_>>();
        let entries_deleted = candidates.iter().try_fold(0_u64, |total, row| {
            let count = u64::try_from(row.entry_count)
                .map_err(|_| SyncRetentionError::InvariantViolation)?;
            total
                .checked_add(count)
                .ok_or(SyncRetentionError::InvariantViolation)
        })?;
        if !ids.is_empty() {
            enable_retention_deletes(&mut transaction).await?;
            let deleted = sqlx::query("DELETE FROM rebaseline_snapshots WHERE id = ANY($1)")
                .bind(&ids)
                .execute(&mut *transaction)
                .await
                .map_err(MetadataError::from)
                .map_err(map_metadata_error)?
                .rows_affected();
            if deleted != ids.len() as u64 {
                return Err(SyncRetentionError::InvariantViolation);
            }
        }
        transaction
            .commit()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;
        Ok(SnapshotPayloadCleanupStepResult {
            payloads_deleted: u32::try_from(ids.len())
                .map_err(|_| SyncRetentionError::InvariantViolation)?,
            entries_deleted,
        })
    }

    /// Delete at most one bounded batch of proofs whose deadline has arrived,
    /// but only after the corresponding large payload is already absent.
    pub async fn cleanup_handoff_proofs_step(
        &self,
        observed_at: Timestamp,
    ) -> Result<HandoffProofCleanupStepResult, SyncRetentionError> {
        let mut transaction = self.begin_transaction().await?;
        let blocked_by_payload = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1
                FROM rebaseline_snapshot_handoff_proofs AS p
                WHERE p.proof_expires_at <= $1
                  AND EXISTS (
                      SELECT 1 FROM rebaseline_snapshots AS s
                      WHERE s.id = p.snapshot_id
                  )
                LIMIT 1
             )",
        )
        .bind(observed_at.as_offset_datetime())
        .fetch_one(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;
        let ids = sqlx::query_scalar::<_, Uuid>(
            "SELECT p.snapshot_id
             FROM rebaseline_snapshot_handoff_proofs AS p
             WHERE p.proof_expires_at <= $1
               AND NOT EXISTS (
                   SELECT 1 FROM rebaseline_snapshots AS s
                   WHERE s.id = p.snapshot_id
               )
             ORDER BY p.proof_expires_at ASC, p.snapshot_id ASC
             LIMIT $2
             FOR UPDATE OF p SKIP LOCKED",
        )
        .bind(observed_at.as_offset_datetime())
        .bind(i64::from(self.policy.proof_batch_limit))
        .fetch_all(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;
        if !ids.is_empty() {
            enable_retention_deletes(&mut transaction).await?;
            let deleted = sqlx::query(
                "DELETE FROM rebaseline_snapshot_handoff_proofs
                 WHERE snapshot_id = ANY($1)",
            )
            .bind(&ids)
            .execute(&mut *transaction)
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?
            .rows_affected();
            if deleted != ids.len() as u64 {
                return Err(SyncRetentionError::InvariantViolation);
            }
        }
        transaction
            .commit()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;
        Ok(HandoffProofCleanupStepResult {
            proofs_deleted: u32::try_from(ids.len())
                .map_err(|_| SyncRetentionError::InvariantViolation)?,
            blocked_by_payload,
        })
    }

    async fn begin_transaction(&self) -> Result<Transaction<'_, Postgres>, SyncRetentionError> {
        self.pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)
    }
}

async fn enable_retention_deletes(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), SyncRetentionError> {
    sqlx::query("SELECT set_config('synveil.retention_cleanup', 'on', true)")
        .execute(&mut **transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)
        .map(|_| ())
}

fn validate_scope(scope: &RetentionScopeRow) -> Result<(), SyncRetentionError> {
    if scope.journal_epoch <= 0
        || scope.sync_head < 0
        || scope.minimum_retained_sequence < 0
        || scope.minimum_retained_sequence > scope.sync_head
    {
        Err(SyncRetentionError::InvariantViolation)
    } else {
        Ok(())
    }
}

fn positive_sequence(value: i64) -> Result<Sequence, SyncRetentionError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .map(Sequence::new)
        .ok_or(SyncRetentionError::InvariantViolation)
}

fn nonnegative_sequence(value: i64) -> Result<Sequence, SyncRetentionError> {
    u64::try_from(value)
        .map(Sequence::new)
        .map_err(|_| SyncRetentionError::InvariantViolation)
}

fn map_metadata_error(error: MetadataError) -> SyncRetentionError {
    match error {
        MetadataError::Database(DatabaseError::Failure(
            DatabaseErrorKind::ConnectionUnavailable,
        )) => SyncRetentionError::DependencyUnavailable,
        MetadataError::Database(error) => SyncRetentionError::Database(error),
        MetadataError::Mapping(_) | MetadataError::CapacityUnavailable => {
            SyncRetentionError::InvariantViolation
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_HANDOFF_PROOF_CLEANUP_BATCH_LIMIT, MAX_JOURNAL_COMPACTION_BATCH_LIMIT,
        MAX_SNAPSHOT_PAYLOAD_CLEANUP_BATCH_LIMIT, SyncRetentionError, SyncRetentionPolicy,
    };
    use std::time::Duration;

    #[test]
    fn policy_rejects_zero_and_unbounded_limits() {
        assert_eq!(
            SyncRetentionPolicy::new(Duration::ZERO, 1, 1, 1),
            Err(SyncRetentionError::InvalidPolicy)
        );
        assert_eq!(
            SyncRetentionPolicy::new(Duration::from_secs(1), 0, 1, 1),
            Err(SyncRetentionError::InvalidPolicy)
        );
        assert_eq!(
            SyncRetentionPolicy::new(
                Duration::from_secs(1),
                MAX_JOURNAL_COMPACTION_BATCH_LIMIT + 1,
                1,
                1,
            ),
            Err(SyncRetentionError::InvalidPolicy)
        );
        assert_eq!(
            SyncRetentionPolicy::new(
                Duration::from_secs(1),
                1,
                MAX_SNAPSHOT_PAYLOAD_CLEANUP_BATCH_LIMIT + 1,
                1,
            ),
            Err(SyncRetentionError::InvalidPolicy)
        );
        assert_eq!(
            SyncRetentionPolicy::new(
                Duration::from_secs(1),
                1,
                1,
                MAX_HANDOFF_PROOF_CLEANUP_BATCH_LIMIT + 1,
            ),
            Err(SyncRetentionError::InvalidPolicy)
        );
    }
}
