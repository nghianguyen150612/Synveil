//! PostgreSQL-backed per-device checkpoint and one-way journal feed service.
//!
//! This module owns consumer progress only. It reuses the Prompt 31 journal
//! reader for ordered event pages and keeps checkpoint advancement separate
//! from logical mutation authority. The API layer supplies a verified signed
//! delivery evidence value before this service can advance a checkpoint.

use std::fmt;

use sqlx::{FromRow, Postgres, Transaction};
use synveil_core::{
    ChangeEvent, DeviceId, DeviceSyncCheckpoint, LibraryId, RebaselineSnapshotId, Sequence,
    Timestamp, UserId,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    ChangeJournalService, DatabaseError, DatabaseErrorKind, DatabasePool, DeviceSyncCheckpointRow,
    JournalCursor, JournalError, JournalHighWatermark, MetadataError,
};

/// The transport-authoritative default feed size.
pub const DEFAULT_SYNC_FEED_LIMIT: u32 = crate::DEFAULT_JOURNAL_PAGE_LIMIT;

/// The transport-authoritative maximum feed size.
pub const MAX_SYNC_FEED_LIMIT: u32 = crate::MAX_JOURNAL_PAGE_LIMIT;

/// Safe failures for checkpoint and one-way feed operations. Inaccessible
/// devices/libraries intentionally share `NotFound` so cross-owner existence
/// cannot be enumerated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncError {
    NotFound,
    InvalidLimit,
    InvalidAckToken,
    CheckpointConflict,
    CheckpointAheadOfSnapshot,
    CheckpointEpochConflict,
    RebaselineRequired {
        reason: RebaselineReason,
        current_epoch: Sequence,
        minimum_retained_sequence: Sequence,
    },
    DependencyUnavailable,
    Database(DatabaseError),
    InvalidPersistedData,
    InternalError,
}

impl fmt::Display for SyncError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotFound => "sync device or library was not found",
            Self::InvalidLimit => "sync feed limit is invalid",
            Self::InvalidAckToken => "sync acknowledgment token is invalid",
            Self::CheckpointConflict => "sync checkpoint progress conflicts",
            Self::CheckpointAheadOfSnapshot => {
                "sync checkpoint is already ahead of the rebaseline snapshot"
            }
            Self::CheckpointEpochConflict => "sync checkpoint epoch conflicts with the snapshot",
            Self::RebaselineRequired { .. } => "sync rebaseline is required",
            Self::DependencyUnavailable => "sync dependency is unavailable",
            Self::Database(error) => return error.fmt(formatter),
            Self::InvalidPersistedData => "sync persisted data is invalid",
            Self::InternalError => "sync internal error",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SyncError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RebaselineReason {
    EpochMismatch,
    HistoryUnavailable,
}

/// Server-verified claims bound to one delivered feed page. The API layer
/// verifies the signature and scope before constructing this value; the
/// metadata service still validates its relationship to durable state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncAckEvidence {
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
    journal_epoch: Sequence,
    from_sequence: Sequence,
    through_sequence: Sequence,
    high_watermark: Sequence,
}

/// The canonical result of one authenticated durable-snapshot handoff.
///
/// The snapshot ID and library ID are returned so a transport adapter can
/// verify that the response belongs to the request it made. The checkpoint is
/// the server-installed per-device cursor; no internal checkpoint row identity
/// or client-supplied boundary is part of this result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RebaselineHandoffResult {
    snapshot_id: RebaselineSnapshotId,
    library_id: LibraryId,
    installed_checkpoint: DeviceSyncCheckpoint,
}

impl RebaselineHandoffResult {
    #[must_use]
    pub const fn new(
        snapshot_id: RebaselineSnapshotId,
        library_id: LibraryId,
        installed_checkpoint: DeviceSyncCheckpoint,
    ) -> Self {
        Self {
            snapshot_id,
            library_id,
            installed_checkpoint,
        }
    }

    #[must_use]
    pub const fn snapshot_id(self) -> RebaselineSnapshotId {
        self.snapshot_id
    }

    #[must_use]
    pub const fn library_id(self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn installed_checkpoint(self) -> DeviceSyncCheckpoint {
        self.installed_checkpoint
    }
}

impl SyncAckEvidence {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        journal_epoch: Sequence,
        from_sequence: Sequence,
        through_sequence: Sequence,
        high_watermark: Sequence,
    ) -> Self {
        Self {
            owner_user_id,
            device_id,
            library_id,
            journal_epoch,
            from_sequence,
            through_sequence,
            high_watermark,
        }
    }

    #[must_use]
    pub const fn owner_user_id(self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn device_id(self) -> DeviceId {
        self.device_id
    }

    #[must_use]
    pub const fn library_id(self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn journal_epoch(self) -> Sequence {
        self.journal_epoch
    }

    #[must_use]
    pub const fn from_sequence(self) -> Sequence {
        self.from_sequence
    }

    #[must_use]
    pub const fn through_sequence(self) -> Sequence {
        self.through_sequence
    }

    #[must_use]
    pub const fn high_watermark(self) -> Sequence {
        self.high_watermark
    }
}

/// One bounded feed response before transport-specific serialization and
/// acknowledgment-token issuance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncFeedPage {
    checkpoint: DeviceSyncCheckpoint,
    from_sequence: Sequence,
    through_sequence: Sequence,
    high_watermark: JournalHighWatermark,
    changes: Vec<ChangeEvent>,
    has_more: bool,
}

impl SyncFeedPage {
    #[must_use]
    pub fn new(
        checkpoint: DeviceSyncCheckpoint,
        from_sequence: Sequence,
        through_sequence: Sequence,
        high_watermark: JournalHighWatermark,
        changes: Vec<ChangeEvent>,
        has_more: bool,
    ) -> Self {
        Self {
            checkpoint,
            from_sequence,
            through_sequence,
            high_watermark,
            changes,
            has_more,
        }
    }

    #[must_use]
    pub fn checkpoint(&self) -> DeviceSyncCheckpoint {
        self.checkpoint
    }

    #[must_use]
    pub const fn from_sequence(&self) -> Sequence {
        self.from_sequence
    }

    #[must_use]
    pub const fn through_sequence(&self) -> Sequence {
        self.through_sequence
    }

    #[must_use]
    pub const fn high_watermark(&self) -> JournalHighWatermark {
        self.high_watermark
    }

    #[must_use]
    pub fn changes(&self) -> &[ChangeEvent] {
        &self.changes
    }

    #[must_use]
    pub fn into_changes(self) -> Vec<ChangeEvent> {
        self.changes
    }

    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }
}

#[derive(Clone, Debug, FromRow)]
struct SyncScopeRow {
    device_owner_user_id: Uuid,
    device_status: String,
    library_owner_user_id: Uuid,
    library_status: String,
    journal_epoch: i64,
    sync_head: i64,
    minimum_retained_sequence: i64,
}

#[derive(Clone, Debug, FromRow)]
struct CheckpointRow {
    owner_user_id: Uuid,
    device_id: Uuid,
    library_id: Uuid,
    journal_epoch: i64,
    acknowledged_sequence: i64,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    last_seen_high_watermark: Option<i64>,
}

#[derive(Clone, Debug, FromRow)]
struct RebaselineSnapshotHandoffRow {
    library_id: Uuid,
    journal_epoch: i64,
    snapshot_resume_sequence: i64,
}

impl From<CheckpointRow> for DeviceSyncCheckpointRow {
    fn from(row: CheckpointRow) -> Self {
        Self {
            owner_user_id: row.owner_user_id,
            device_id: row.device_id,
            library_id: row.library_id,
            journal_epoch: row.journal_epoch,
            acknowledged_sequence: row.acknowledged_sequence,
            created_at: row.created_at,
            updated_at: row.updated_at,
            last_seen_high_watermark: row.last_seen_high_watermark,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SyncScope {
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
    journal_epoch: Sequence,
    sync_head: Sequence,
    minimum_retained_sequence: Sequence,
}

/// PostgreSQL-backed checkpoint and one-way feed application service.
#[derive(Clone)]
pub struct DeviceSyncService {
    pool: DatabasePool,
}

impl DeviceSyncService {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    #[must_use]
    pub const fn pool(&self) -> &DatabasePool {
        &self.pool
    }

    /// Create the initial checkpoint if necessary and return the durable row.
    /// The device and library must already exist, belong to the owner, and be
    /// in sync-eligible states. Initial progress is always sequence zero in the
    /// library's current epoch; history is never silently skipped.
    pub async fn ensure_checkpoint(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
    ) -> Result<DeviceSyncCheckpoint, SyncError> {
        let mut transaction = self.begin_transaction(false).await?;
        let scope = load_scope(&mut transaction, owner_user_id, device_id, library_id).await?;
        let row = ensure_checkpoint_in_transaction(&mut transaction, scope).await?;
        let checkpoint = map_checkpoint(row.into())?;
        validate_checkpoint(&checkpoint, scope)?;
        transaction
            .commit()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;
        Ok(checkpoint)
    }

    /// Install the canonical boundary proved by a durable rebaseline snapshot
    /// into exactly one authenticated device checkpoint.
    ///
    /// This is intentionally separate from [`Self::acknowledge`]. Ordinary
    /// acknowledgment is authorized by feed-delivery evidence and therefore
    /// cannot be reused to jump over a page. Handoff authorization comes from
    /// the immutable, owner-scoped handoff proof instead. The transaction
    /// first reads that proof without checking logical download expiry, then
    /// validates the device/library scope, locks the checkpoint row, and
    /// installs the proved boundary or returns the typed conflict required by
    /// the monotone transition rules. It never reads snapshot entries and it
    /// does not take the namespace mutation guard.
    pub async fn complete_rebaseline_handoff(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        snapshot_id: RebaselineSnapshotId,
    ) -> Result<RebaselineHandoffResult, SyncError> {
        let mut transaction = self.begin_transaction(false).await?;
        let snapshot = sqlx::query_as::<_, RebaselineSnapshotHandoffRow>(
            "SELECT library_id, journal_epoch, snapshot_resume_sequence
             FROM rebaseline_snapshot_handoff_proofs
             WHERE snapshot_id = $1
               AND owner_user_id = $2
             FOR SHARE",
        )
        .bind(snapshot_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?
        .ok_or(SyncError::NotFound)?;

        // The durable proof is the only accepted source for the handoff
        // boundary. A missing/foreign proof has already collapsed to
        // NotFound above; malformed persisted values fail closed.
        let snapshot_library_id = LibraryId::try_from_uuid(snapshot.library_id)
            .map_err(|_| SyncError::InvalidPersistedData)?;
        let snapshot_epoch = positive_sequence(snapshot.journal_epoch)?;
        let snapshot_resume_sequence = nonnegative_sequence(snapshot.snapshot_resume_sequence)?;

        // `load_scope` takes the same short library/device share lock used by
        // ordinary feed/checkpoint operations. This prevents a concurrent
        // owner-scoped library deletion from invalidating the scope after the
        // header has been observed, while avoiding the namespace advisory
        // guard used only for live projection materialization.
        let scope = load_scope(
            &mut transaction,
            owner_user_id,
            device_id,
            snapshot_library_id,
        )
        .await?;
        if snapshot_epoch != scope.journal_epoch {
            return Err(SyncError::CheckpointEpochConflict);
        }
        if snapshot_resume_sequence.get() > scope.sync_head.get() {
            return Err(SyncError::InvalidPersistedData);
        }
        if snapshot_resume_sequence.get() < scope.minimum_retained_sequence.get() {
            return Err(SyncError::InvalidPersistedData);
        }

        // A missing row is installed at the snapshot boundary, not at the
        // current head. ON CONFLICT closes the race with first-use checkpoint
        // creation and lets the row lock below serialize competing handoffs.
        sqlx::query(
            "INSERT INTO device_sync_checkpoints
                (owner_user_id, device_id, library_id, journal_epoch,
                 acknowledged_sequence, created_at, updated_at,
                 last_seen_high_watermark)
             VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, $5)
             ON CONFLICT (device_id, library_id) DO NOTHING",
        )
        .bind(owner_user_id.into_uuid())
        .bind(device_id.into_uuid())
        .bind(snapshot_library_id.into_uuid())
        .bind(i64_from_sequence(snapshot_epoch)?)
        .bind(i64_from_sequence(snapshot_resume_sequence)?)
        .execute(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;

        let row = load_checkpoint_for_update(
            &mut transaction,
            owner_user_id,
            device_id,
            snapshot_library_id,
        )
        .await?
        .ok_or(SyncError::InvalidPersistedData)?;
        let checkpoint = map_checkpoint(row.into())?;
        if checkpoint.owner_user_id() != scope.owner_user_id
            || checkpoint.device_id() != scope.device_id
            || checkpoint.library_id() != scope.library_id
        {
            return Err(SyncError::InvalidPersistedData);
        }

        let current_epoch = checkpoint.journal_epoch();
        let current_sequence = checkpoint.acknowledged_sequence();
        let installed_checkpoint = if current_epoch > snapshot_epoch {
            return Err(SyncError::CheckpointEpochConflict);
        } else if current_epoch == snapshot_epoch {
            if current_sequence > snapshot_resume_sequence {
                return Err(SyncError::CheckpointAheadOfSnapshot);
            }
            if current_sequence == snapshot_resume_sequence {
                checkpoint
            } else {
                update_handoff_checkpoint(
                    &mut transaction,
                    scope,
                    snapshot_epoch,
                    snapshot_resume_sequence,
                    false,
                )
                .await?
            }
        } else {
            update_handoff_checkpoint(
                &mut transaction,
                scope,
                snapshot_epoch,
                snapshot_resume_sequence,
                true,
            )
            .await?
        };

        if installed_checkpoint.journal_epoch() != snapshot_epoch
            || installed_checkpoint.acknowledged_sequence() != snapshot_resume_sequence
        {
            return Err(SyncError::InvalidPersistedData);
        }
        transaction
            .commit()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;
        Ok(RebaselineHandoffResult::new(
            snapshot_id,
            snapshot_library_id,
            installed_checkpoint,
        ))
    }

    /// Fetch a bounded, ordered page after the durable acknowledged sequence.
    /// Checkpoint creation and the journal page share one short transaction.
    /// Its library share lock prevents both append and compaction from changing
    /// the head, retained floor, or rows until the page is complete, so READ
    /// COMMITTED avoids a cleanup-race serialization failure without a retry.
    /// This operation never advances acknowledged progress.
    pub async fn fetch_feed(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        limit: u32,
    ) -> Result<SyncFeedPage, SyncError> {
        validate_limit(limit)?;
        let mut transaction = self.begin_transaction(false).await?;
        let scope = load_scope(&mut transaction, owner_user_id, device_id, library_id).await?;
        let checkpoint_row = ensure_checkpoint_in_transaction(&mut transaction, scope).await?;
        let checkpoint = map_checkpoint(checkpoint_row.into())?;
        validate_checkpoint(&checkpoint, scope)?;
        if checkpoint.journal_epoch() != scope.journal_epoch {
            return Err(rebaseline_error(scope, RebaselineReason::EpochMismatch));
        }

        let from_sequence = checkpoint.acknowledged_sequence();
        if from_sequence.get() < scope.minimum_retained_sequence.get() {
            return Err(rebaseline_error(
                scope,
                RebaselineReason::HistoryUnavailable,
            ));
        }

        let cursor = JournalCursor::new(library_id, scope.journal_epoch, from_sequence);
        let journal_page = ChangeJournalService::list_changes_in_transaction(
            &mut transaction,
            owner_user_id,
            library_id,
            Some(cursor),
            limit,
        )
        .await
        .map_err(|error| map_journal_error(error, scope))?;
        let has_more = journal_page.has_more();
        let through_sequence = journal_page
            .events()
            .last()
            .map_or(from_sequence, |change| change.sequence());
        let high_watermark = journal_page.high_watermark();
        let changes = journal_page.into_events();

        transaction
            .commit()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;

        Ok(SyncFeedPage::new(
            checkpoint,
            from_sequence,
            through_sequence,
            high_watermark,
            changes,
            has_more,
        ))
    }

    /// Advance the durable checkpoint after the API has verified a signed
    /// delivery evidence token. The token's start must equal the current
    /// checkpoint, so a client cannot skip an unapplied page. Already-applied
    /// older tokens converge to the current row without rewinding it.
    pub async fn acknowledge(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        evidence: SyncAckEvidence,
    ) -> Result<DeviceSyncCheckpoint, SyncError> {
        if evidence.owner_user_id() != owner_user_id
            || evidence.device_id() != device_id
            || evidence.library_id() != library_id
        {
            return Err(SyncError::InvalidAckToken);
        }
        if evidence.through_sequence().get() < evidence.from_sequence().get()
            || evidence.high_watermark().get() < evidence.through_sequence().get()
            || evidence
                .through_sequence()
                .get()
                .saturating_sub(evidence.from_sequence().get())
                > u64::from(MAX_SYNC_FEED_LIMIT)
        {
            return Err(SyncError::InvalidAckToken);
        }

        let mut transaction = self.begin_transaction(false).await?;
        let scope = load_scope(&mut transaction, owner_user_id, device_id, library_id).await?;
        let Some(row) =
            load_checkpoint_for_update(&mut transaction, owner_user_id, device_id, library_id)
                .await?
        else {
            return Err(SyncError::CheckpointConflict);
        };
        let checkpoint = map_checkpoint(row.into())?;
        validate_checkpoint(&checkpoint, scope)?;

        if checkpoint.journal_epoch() != evidence.journal_epoch()
            || evidence.journal_epoch() != scope.journal_epoch
        {
            return Err(rebaseline_error(scope, RebaselineReason::EpochMismatch));
        }
        if checkpoint.acknowledged_sequence().get() < scope.minimum_retained_sequence.get() {
            return Err(rebaseline_error(
                scope,
                RebaselineReason::HistoryUnavailable,
            ));
        }
        if evidence.high_watermark().get() > scope.sync_head.get() {
            return Err(SyncError::InvalidAckToken);
        }

        let current = checkpoint.acknowledged_sequence().get();
        let from = evidence.from_sequence().get();
        let through = evidence.through_sequence().get();
        if current > through {
            transaction
                .commit()
                .await
                .map_err(MetadataError::from)
                .map_err(map_metadata_error)?;
            return Ok(checkpoint);
        }
        if current == through {
            transaction
                .commit()
                .await
                .map_err(MetadataError::from)
                .map_err(map_metadata_error)?;
            return Ok(checkpoint);
        }
        if current != from {
            return Err(SyncError::CheckpointConflict);
        }

        if through > from {
            let expected_count = through - from;
            let actual_count: i64 = sqlx::query_scalar(
                "SELECT count(*)
                 FROM change_journal
                 WHERE owner_user_id = $1
                   AND library_id = $2
                   AND journal_epoch = $3
                   AND sequence > $4
                   AND sequence <= $5",
            )
            .bind(owner_user_id.into_uuid())
            .bind(library_id.into_uuid())
            .bind(i64_from_sequence(evidence.journal_epoch())?)
            .bind(i64::try_from(from).map_err(|_| SyncError::InvalidAckToken)?)
            .bind(i64::try_from(through).map_err(|_| SyncError::InvalidAckToken)?)
            .fetch_one(&mut *transaction)
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;
            if actual_count != i64::try_from(expected_count).unwrap_or(i64::MAX) {
                return Err(SyncError::InvalidAckToken);
            }
        }

        let row = sqlx::query_as::<_, CheckpointRow>(
            "UPDATE device_sync_checkpoints
             SET acknowledged_sequence = $4,
                 last_seen_high_watermark = GREATEST(
                     COALESCE(last_seen_high_watermark, 0), $5
                 ),
                 updated_at = CURRENT_TIMESTAMP
             WHERE owner_user_id = $1
               AND device_id = $2
               AND library_id = $3
               AND acknowledged_sequence = $6
             RETURNING owner_user_id, device_id, library_id, journal_epoch,
                       acknowledged_sequence, created_at, updated_at,
                       last_seen_high_watermark",
        )
        .bind(owner_user_id.into_uuid())
        .bind(device_id.into_uuid())
        .bind(library_id.into_uuid())
        .bind(i64::try_from(through).map_err(|_| SyncError::InvalidAckToken)?)
        .bind(
            i64::try_from(evidence.high_watermark().get())
                .map_err(|_| SyncError::InvalidAckToken)?,
        )
        .bind(i64::try_from(from).map_err(|_| SyncError::InvalidAckToken)?)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?
        .ok_or(SyncError::CheckpointConflict)?;

        transaction
            .commit()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;
        map_checkpoint(row.into())
    }

    async fn begin_transaction(
        &self,
        repeatable_read: bool,
    ) -> Result<Transaction<'_, Postgres>, SyncError> {
        let mut transaction = self
            .pool
            .sqlx_pool()
            .begin()
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;
        let statement = if repeatable_read {
            "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ"
        } else {
            "SET TRANSACTION ISOLATION LEVEL READ COMMITTED"
        };
        sqlx::query(statement)
            .execute(&mut *transaction)
            .await
            .map_err(MetadataError::from)
            .map_err(map_metadata_error)?;
        Ok(transaction)
    }
}

async fn load_scope(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
) -> Result<SyncScope, SyncError> {
    let row = sqlx::query_as::<_, SyncScopeRow>(
        "SELECT d.owner_user_id AS device_owner_user_id,
                d.status AS device_status,
                l.owner_user_id AS library_owner_user_id,
                l.status AS library_status,
                l.journal_epoch,
                l.sync_head,
                l.minimum_retained_sequence
         FROM devices AS d
         CROSS JOIN libraries AS l
         WHERE d.id = $1
           AND l.id = $2
         FOR SHARE",
    )
    .bind(device_id.into_uuid())
    .bind(library_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map_err(map_metadata_error)?
    .ok_or(SyncError::NotFound)?;

    let owner_matches = row.device_owner_user_id == owner_user_id.into_uuid()
        && row.library_owner_user_id == owner_user_id.into_uuid();
    if !owner_matches || row.device_status != "ACTIVE" || row.library_status == "DELETING" {
        return Err(SyncError::NotFound);
    }

    let journal_epoch = positive_sequence(row.journal_epoch)?;
    let sync_head = nonnegative_sequence(row.sync_head)?;
    let minimum_retained_sequence = nonnegative_sequence(row.minimum_retained_sequence)?;
    if minimum_retained_sequence.get() > sync_head.get() {
        return Err(SyncError::InvalidPersistedData);
    }

    Ok(SyncScope {
        owner_user_id,
        device_id,
        library_id,
        journal_epoch,
        sync_head,
        minimum_retained_sequence,
    })
}

async fn ensure_checkpoint_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    scope: SyncScope,
) -> Result<CheckpointRow, SyncError> {
    sqlx::query(
        "INSERT INTO device_sync_checkpoints
            (owner_user_id, device_id, library_id, journal_epoch,
             acknowledged_sequence, created_at, updated_at,
             last_seen_high_watermark)
         VALUES ($1, $2, $3, $4, 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, NULL)
         ON CONFLICT (device_id, library_id) DO NOTHING",
    )
    .bind(scope.owner_user_id.into_uuid())
    .bind(scope.device_id.into_uuid())
    .bind(scope.library_id.into_uuid())
    .bind(i64_from_sequence(scope.journal_epoch)?)
    .execute(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map_err(map_metadata_error)?;

    load_checkpoint_for_update(
        transaction,
        scope.owner_user_id,
        scope.device_id,
        scope.library_id,
    )
    .await?
    .ok_or(SyncError::InvalidPersistedData)
}

async fn load_checkpoint_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
) -> Result<Option<CheckpointRow>, SyncError> {
    sqlx::query_as::<_, CheckpointRow>(
        "SELECT owner_user_id, device_id, library_id, journal_epoch,
                acknowledged_sequence, created_at, updated_at,
                last_seen_high_watermark
         FROM device_sync_checkpoints
         WHERE owner_user_id = $1
           AND device_id = $2
           AND library_id = $3
         FOR UPDATE",
    )
    .bind(owner_user_id.into_uuid())
    .bind(device_id.into_uuid())
    .bind(library_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(MetadataError::from)
    .map_err(map_metadata_error)
}

async fn update_handoff_checkpoint(
    transaction: &mut Transaction<'_, Postgres>,
    scope: SyncScope,
    journal_epoch: Sequence,
    acknowledged_sequence: Sequence,
    reset_high_watermark: bool,
) -> Result<DeviceSyncCheckpoint, SyncError> {
    let statement = if reset_high_watermark {
        "UPDATE device_sync_checkpoints
         SET journal_epoch = $4,
             acknowledged_sequence = $5,
             last_seen_high_watermark = $5,
             updated_at = CURRENT_TIMESTAMP
         WHERE owner_user_id = $1
           AND device_id = $2
           AND library_id = $3
         RETURNING owner_user_id, device_id, library_id, journal_epoch,
                   acknowledged_sequence, created_at, updated_at,
                   last_seen_high_watermark"
    } else {
        "UPDATE device_sync_checkpoints
         SET journal_epoch = $4,
             acknowledged_sequence = $5,
             last_seen_high_watermark = GREATEST(
                 COALESCE(last_seen_high_watermark, 0), $5
             ),
             updated_at = CURRENT_TIMESTAMP
         WHERE owner_user_id = $1
           AND device_id = $2
           AND library_id = $3
         RETURNING owner_user_id, device_id, library_id, journal_epoch,
                   acknowledged_sequence, created_at, updated_at,
                   last_seen_high_watermark"
    };
    let row = sqlx::query_as::<_, CheckpointRow>(statement)
        .bind(scope.owner_user_id.into_uuid())
        .bind(scope.device_id.into_uuid())
        .bind(scope.library_id.into_uuid())
        .bind(i64_from_sequence(journal_epoch)?)
        .bind(i64_from_sequence(acknowledged_sequence)?)
        .fetch_one(&mut **transaction)
        .await
        .map_err(MetadataError::from)
        .map_err(map_metadata_error)?;
    map_checkpoint(row.into())
}

fn map_checkpoint(row: DeviceSyncCheckpointRow) -> Result<DeviceSyncCheckpoint, SyncError> {
    let owner_user_id =
        UserId::try_from_uuid(row.owner_user_id).map_err(|_| SyncError::InvalidPersistedData)?;
    let device_id =
        DeviceId::try_from_uuid(row.device_id).map_err(|_| SyncError::InvalidPersistedData)?;
    let library_id =
        LibraryId::try_from_uuid(row.library_id).map_err(|_| SyncError::InvalidPersistedData)?;
    let journal_epoch = positive_sequence(row.journal_epoch)?;
    let acknowledged_sequence = nonnegative_sequence(row.acknowledged_sequence)?;
    let last_seen_high_watermark = row
        .last_seen_high_watermark
        .map(nonnegative_sequence)
        .transpose()?;
    Ok(DeviceSyncCheckpoint::new(
        owner_user_id,
        device_id,
        library_id,
        journal_epoch,
        acknowledged_sequence,
        Timestamp::from_offset_datetime(row.created_at),
        Timestamp::from_offset_datetime(row.updated_at),
        last_seen_high_watermark,
    ))
}

fn validate_checkpoint(
    checkpoint: &DeviceSyncCheckpoint,
    scope: SyncScope,
) -> Result<(), SyncError> {
    if checkpoint.owner_user_id() != scope.owner_user_id
        || checkpoint.device_id() != scope.device_id
        || checkpoint.library_id() != scope.library_id
    {
        return Err(SyncError::InvalidPersistedData);
    }
    if checkpoint.acknowledged_sequence().get() > scope.sync_head.get() {
        return Err(SyncError::InvalidPersistedData);
    }
    if checkpoint
        .last_seen_high_watermark()
        .is_some_and(|watermark| watermark.get() > scope.sync_head.get())
    {
        return Err(SyncError::InvalidPersistedData);
    }
    Ok(())
}

fn validate_limit(limit: u32) -> Result<(), SyncError> {
    if (1..=MAX_SYNC_FEED_LIMIT).contains(&limit) {
        Ok(())
    } else {
        Err(SyncError::InvalidLimit)
    }
}

fn positive_sequence(value: i64) -> Result<Sequence, SyncError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .map(Sequence::new)
        .ok_or(SyncError::InvalidPersistedData)
}

fn nonnegative_sequence(value: i64) -> Result<Sequence, SyncError> {
    u64::try_from(value)
        .map(Sequence::new)
        .map_err(|_| SyncError::InvalidPersistedData)
}

fn i64_from_sequence(value: Sequence) -> Result<i64, SyncError> {
    i64::try_from(value.get()).map_err(|_| SyncError::InvalidPersistedData)
}

fn rebaseline_error(scope: SyncScope, reason: RebaselineReason) -> SyncError {
    SyncError::RebaselineRequired {
        reason,
        current_epoch: scope.journal_epoch,
        minimum_retained_sequence: scope.minimum_retained_sequence,
    }
}

fn map_journal_error(error: JournalError, scope: SyncScope) -> SyncError {
    match error {
        JournalError::NotFound => SyncError::NotFound,
        JournalError::InvalidLimit => SyncError::InvalidLimit,
        JournalError::CursorEpochMismatch => {
            rebaseline_error(scope, RebaselineReason::EpochMismatch)
        }
        JournalError::CursorExpired => {
            rebaseline_error(scope, RebaselineReason::HistoryUnavailable)
        }
        JournalError::DependencyUnavailable => SyncError::DependencyUnavailable,
        JournalError::Database(error) => SyncError::Database(error),
        JournalError::InvalidCursor
        | JournalError::InvalidPersistedData
        | JournalError::InternalError => SyncError::InvalidPersistedData,
    }
}

fn map_metadata_error(error: MetadataError) -> SyncError {
    match error {
        MetadataError::Database(DatabaseError::Failure(
            DatabaseErrorKind::ConnectionUnavailable,
        )) => SyncError::DependencyUnavailable,
        MetadataError::Database(error) => SyncError::Database(error),
        MetadataError::Mapping(_) => SyncError::InvalidPersistedData,
        MetadataError::CapacityUnavailable => SyncError::DependencyUnavailable,
    }
}
