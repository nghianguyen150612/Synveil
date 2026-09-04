//! Manual, bounded policy resolution for durable backup schedules.
//!
//! One explicit tick derives at most one globally ordered action. Progress is
//! durable only through a successful Prompt 63 handoff or an immutable
//! expired-prefix skip. There is no process-local cursor, loop, worker, lease,
//! retry, maintenance advancement, snapshot, or ObjectStore interaction.

use std::{fmt, str::FromStr, time::Duration as StdDuration};

use sqlx::{FromRow, Postgres, Transaction};
use synveil_core::{
    BACKUP_SCHEDULE_FINGERPRINT_VERSION, BACKUP_SCHEDULE_LEGACY_FINGERPRINT_VERSION,
    BackupSchedule, BackupScheduleId, BackupScheduleIdempotencyFingerprint,
    BackupScheduleLocalTime, BackupScheduleMisfireMode, BackupScheduleMisfireSkip,
    BackupScheduleMisfireSkipId, BackupScheduleOccurrenceHandoffResult,
    BackupScheduleOccurrenceMaterializationResult, BackupScheduleOccurrenceNotEffectiveReason,
    BackupScheduleRecurrenceKind, BackupScheduleRevision, BackupScheduleRevisionId,
    BackupScheduleRevisionNumber, BackupScheduleTimezone, BackupScheduleWeekday,
    BackupSchedulerSkipOutcome, BackupSchedulerTickOutcome, BackupSchedulerTickResult, BackupSetId,
    BackupSetState, PlannedScheduleOccurrence, Timestamp, UserId, last_occurrence_before,
    latest_occurrence_in_window, oldest_occurrence_in_window,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    BackupScheduleError, BackupScheduleHandoffError, BackupScheduleService, BackupService,
    DatabaseError, DatabasePool, scheduling::lock_owned_backup_set,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackupSchedulerError {
    NotEffective(BackupScheduleOccurrenceNotEffectiveReason),
    NotDue,
    InvalidTarget,
    Schedule(BackupScheduleError),
    Handoff(BackupScheduleHandoffError),
    Database(DatabaseError),
    InvalidPersistedData,
}

impl fmt::Display for BackupSchedulerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotEffective(reason) => {
                write!(
                    formatter,
                    "scheduler candidate is not effective: {reason:?}"
                )
            }
            Self::NotDue => formatter.write_str("scheduler candidate is no longer due"),
            Self::InvalidTarget => formatter.write_str("scheduler candidate is invalid"),
            Self::Schedule(error) => error.fmt(formatter),
            Self::Handoff(error) => error.fmt(formatter),
            Self::Database(error) => error.fmt(formatter),
            Self::InvalidPersistedData => {
                formatter.write_str("scheduler persisted scheduling data is invalid")
            }
        }
    }
}

impl std::error::Error for BackupSchedulerError {}

impl From<sqlx::Error> for BackupSchedulerError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(DatabaseError::from(error))
    }
}

impl From<BackupScheduleError> for BackupSchedulerError {
    fn from(error: BackupScheduleError) -> Self {
        match error {
            BackupScheduleError::Database(error) => Self::Database(error),
            BackupScheduleError::InvalidPersistedData => Self::InvalidPersistedData,
            other => Self::Schedule(other),
        }
    }
}

impl From<BackupScheduleHandoffError> for BackupSchedulerError {
    fn from(error: BackupScheduleHandoffError) -> Self {
        match error {
            BackupScheduleHandoffError::Database(error) => Self::Database(error),
            BackupScheduleHandoffError::InvalidPersistedData => Self::InvalidPersistedData,
            other => Self::Handoff(other),
        }
    }
}

#[derive(Clone, Debug, FromRow, PartialEq)]
struct CurrentScheduleRow {
    backup_set_state: String,
    schedule_id: Uuid,
    schedule_owner_user_id: Uuid,
    schedule_backup_set_id: Uuid,
    current_revision_id: Uuid,
    enabled: bool,
    schedule_effective_from: OffsetDateTime,
    schedule_created_at: OffsetDateTime,
    schedule_updated_at: OffsetDateTime,
    revision_id: Uuid,
    revision_schedule_id: Uuid,
    revision_owner_user_id: Uuid,
    revision_backup_set_id: Uuid,
    revision_number: i64,
    operation_id: String,
    fingerprint_version: i16,
    request_fingerprint: Vec<u8>,
    recurrence_kind: String,
    timezone: String,
    local_time_minute: i16,
    weekly_days: Vec<i16>,
    misfire_mode: String,
    max_lateness_seconds: i32,
    revision_created_at: OffsetDateTime,
    last_handed_off_utc: Option<OffsetDateTime>,
    last_skipped_through_utc: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, FromRow, PartialEq)]
struct MisfireSkipRow {
    id: Uuid,
    owner_user_id: Uuid,
    backup_set_id: Uuid,
    schedule_id: Uuid,
    schedule_revision_id: Uuid,
    activation_effective_from: OffsetDateTime,
    resolved_from_exclusive_utc: OffsetDateTime,
    resolved_through_utc: OffsetDateTime,
    observed_at_utc: OffsetDateTime,
    misfire_mode: String,
    max_lateness_seconds: i32,
    created_at: OffsetDateTime,
}

#[derive(Clone, Debug, FromRow)]
struct ScheduleFenceRow {
    owner_user_id: Uuid,
    backup_set_id: Uuid,
    current_revision_id: Uuid,
    enabled: bool,
    effective_from: OffsetDateTime,
}

#[derive(Clone, Debug)]
struct ScheduleDescriptor {
    owner_user_id: UserId,
    schedule: BackupSchedule,
    resolution_reference: Timestamp,
}

impl ScheduleDescriptor {
    fn try_from_row(row: CurrentScheduleRow) -> Result<Option<Self>, BackupSchedulerError> {
        let state = BackupSetState::from_str(&row.backup_set_state)
            .map_err(|_| BackupSchedulerError::InvalidPersistedData)?;
        if state != BackupSetState::Active || !row.enabled {
            return Ok(None);
        }
        let owner_user_id = UserId::try_from_uuid(row.schedule_owner_user_id)
            .map_err(|_| BackupSchedulerError::InvalidPersistedData)?;
        let backup_set_id = BackupSetId::try_from_uuid(row.schedule_backup_set_id)
            .map_err(|_| BackupSchedulerError::InvalidPersistedData)?;
        let schedule_id = BackupScheduleId::try_from_uuid(row.schedule_id)
            .map_err(|_| BackupSchedulerError::InvalidPersistedData)?;
        let revision_id = BackupScheduleRevisionId::try_from_uuid(row.revision_id)
            .map_err(|_| BackupSchedulerError::InvalidPersistedData)?;
        if row.current_revision_id != row.revision_id
            || row.revision_schedule_id != row.schedule_id
            || row.revision_owner_user_id != row.schedule_owner_user_id
            || row.revision_backup_set_id != row.schedule_backup_set_id
        {
            return Err(BackupSchedulerError::InvalidPersistedData);
        }
        let revision = decode_revision(
            revision_id,
            schedule_id,
            owner_user_id,
            backup_set_id,
            row.revision_number,
            row.operation_id,
            row.fingerprint_version,
            row.request_fingerprint,
            row.recurrence_kind,
            row.timezone,
            row.local_time_minute,
            row.weekly_days,
            row.misfire_mode,
            row.max_lateness_seconds,
            row.revision_created_at,
        )?;
        let schedule = BackupSchedule::new(
            schedule_id,
            owner_user_id,
            backup_set_id,
            revision,
            row.enabled,
            Timestamp::from_offset_datetime(row.schedule_effective_from),
            Timestamp::from_offset_datetime(row.schedule_created_at),
            Timestamp::from_offset_datetime(row.schedule_updated_at),
        )
        .map_err(|_| BackupSchedulerError::InvalidPersistedData)?;
        let mut resolution_reference = schedule.effective_from();
        if let Some(value) = row.last_handed_off_utc {
            resolution_reference = resolution_reference.max(Timestamp::from_offset_datetime(value));
        }
        if let Some(value) = row.last_skipped_through_utc {
            resolution_reference = resolution_reference.max(Timestamp::from_offset_datetime(value));
        }
        Ok(Some(Self {
            owner_user_id,
            schedule,
            resolution_reference,
        }))
    }

    fn action(
        &self,
        observed_at_utc: Timestamp,
    ) -> Result<Option<SchedulerAction>, BackupSchedulerError> {
        if observed_at_utc <= self.resolution_reference {
            return Ok(None);
        }
        let revision = self.schedule.current_revision();
        let cutoff = observed_at_utc
            .checked_sub_std(StdDuration::from_secs(u64::from(
                revision.max_lateness_seconds(),
            )))
            .ok_or(BackupSchedulerError::InvalidTarget)?;
        let expired = last_occurrence_before(revision, cutoff)
            .filter(|planned| planned.scheduled_for_utc() > self.resolution_reference);

        match revision.misfire_mode() {
            BackupScheduleMisfireMode::ReplayOneByOne => {
                if let Some(planned_through) = expired {
                    return Ok(Some(SchedulerAction::SkipExpired {
                        owner_user_id: self.owner_user_id,
                        schedule: Box::new(self.schedule.clone()),
                        resolved_from_exclusive_utc: self.resolution_reference,
                        planned_through,
                    }));
                }
                Ok(oldest_occurrence_in_window(
                    revision,
                    self.resolution_reference,
                    cutoff,
                    observed_at_utc,
                )
                .map(|planned| SchedulerAction::Fire {
                    owner_user_id: self.owner_user_id,
                    schedule: Box::new(self.schedule.clone()),
                    planned,
                }))
            }
            BackupScheduleMisfireMode::LatestOnly => {
                if let Some(planned) = latest_occurrence_in_window(
                    revision,
                    self.resolution_reference,
                    cutoff,
                    observed_at_utc,
                ) {
                    return Ok(Some(SchedulerAction::Fire {
                        owner_user_id: self.owner_user_id,
                        schedule: Box::new(self.schedule.clone()),
                        planned,
                    }));
                }
                Ok(expired.map(|planned_through| SchedulerAction::SkipExpired {
                    owner_user_id: self.owner_user_id,
                    schedule: Box::new(self.schedule.clone()),
                    resolved_from_exclusive_utc: self.resolution_reference,
                    planned_through,
                }))
            }
        }
    }
}

enum SchedulerAction {
    SkipExpired {
        owner_user_id: UserId,
        schedule: Box<BackupSchedule>,
        resolved_from_exclusive_utc: Timestamp,
        planned_through: PlannedScheduleOccurrence,
    },
    Fire {
        owner_user_id: UserId,
        schedule: Box<BackupSchedule>,
        planned: PlannedScheduleOccurrence,
    },
}

impl SchedulerAction {
    fn key(&self) -> (Timestamp, BackupScheduleId, BackupScheduleRevisionId) {
        match self {
            Self::SkipExpired {
                schedule,
                planned_through,
                ..
            } => (
                planned_through.scheduled_for_utc(),
                schedule.id(),
                schedule.current_revision_id(),
            ),
            Self::Fire {
                schedule, planned, ..
            } => (
                planned.scheduled_for_utc(),
                schedule.id(),
                schedule.current_revision_id(),
            ),
        }
    }
}

#[derive(Clone)]
pub struct BackupSchedulerService {
    pool: DatabasePool,
}

impl BackupSchedulerService {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    #[must_use]
    pub const fn pool(&self) -> &DatabasePool {
        &self.pool
    }

    pub async fn run_scheduler_tick(
        &self,
        observed_at_utc: Timestamp,
    ) -> Result<BackupSchedulerTickResult, BackupSchedulerError> {
        let Some(action) = self.discover_action(observed_at_utc).await? else {
            return Ok(BackupSchedulerTickResult::Idle);
        };
        match action {
            SchedulerAction::SkipExpired {
                owner_user_id,
                schedule,
                resolved_from_exclusive_utc,
                planned_through,
            } => {
                let skip = self
                    .resolve_misfire_skip(
                        owner_user_id,
                        &schedule,
                        resolved_from_exclusive_utc,
                        planned_through,
                        observed_at_utc,
                    )
                    .await?;
                Ok(skip.map_or(BackupSchedulerTickResult::Idle, |skip| {
                    BackupSchedulerTickResult::SkippedExpired(BackupSchedulerSkipOutcome::from(
                        &skip,
                    ))
                }))
            }
            SchedulerAction::Fire {
                owner_user_id,
                schedule,
                planned,
            } => {
                let materialization = BackupScheduleService::new(self.pool.clone())
                    .materialize_due_backup_schedule_occurrence(
                        owner_user_id,
                        schedule.id(),
                        schedule.current_revision_id(),
                        planned.local_calendar_date(),
                        observed_at_utc,
                    )
                    .await?;
                let (occurrence, created) = match materialization {
                    BackupScheduleOccurrenceMaterializationResult::Created(value) => (value, true),
                    BackupScheduleOccurrenceMaterializationResult::Existing(value) => {
                        (value, false)
                    }
                    BackupScheduleOccurrenceMaterializationResult::NotDue => {
                        return Err(BackupSchedulerError::NotDue);
                    }
                    BackupScheduleOccurrenceMaterializationResult::NotEffective(reason) => {
                        return Err(BackupSchedulerError::NotEffective(reason));
                    }
                    BackupScheduleOccurrenceMaterializationResult::InvalidTarget => {
                        return Err(BackupSchedulerError::InvalidTarget);
                    }
                };
                let handoff = BackupService::new(self.pool.clone())
                    .handoff_backup_schedule_occurrence_for_scheduler(
                        owner_user_id,
                        occurrence.id(),
                        observed_at_utc,
                    )
                    .await?;
                let outcome = tick_outcome(&handoff)?;
                Ok(if created {
                    BackupSchedulerTickResult::MaterializedAndHandedOff(outcome)
                } else {
                    BackupSchedulerTickResult::HandedOffExisting(outcome)
                })
            }
        }
    }

    pub async fn tick(
        &self,
        observed_at_utc: Timestamp,
    ) -> Result<BackupSchedulerTickResult, BackupSchedulerError> {
        self.run_scheduler_tick(observed_at_utc).await
    }

    async fn discover_action(
        &self,
        observed_at_utc: Timestamp,
    ) -> Result<Option<SchedulerAction>, BackupSchedulerError> {
        let mut selected = None;
        for descriptor in load_current_schedule_descriptors(&self.pool).await? {
            let Some(action) = descriptor.action(observed_at_utc)? else {
                continue;
            };
            if selected
                .as_ref()
                .is_none_or(|current: &SchedulerAction| action.key() < current.key())
            {
                selected = Some(action);
            }
        }
        Ok(selected)
    }

    #[allow(clippy::too_many_lines)]
    async fn resolve_misfire_skip(
        &self,
        owner_user_id: UserId,
        schedule: &BackupSchedule,
        resolved_from_exclusive_utc: Timestamp,
        planned_through: PlannedScheduleOccurrence,
        observed_at_utc: Timestamp,
    ) -> Result<Option<BackupScheduleMisfireSkip>, BackupSchedulerError> {
        let mut transaction = self.pool.sqlx_pool().begin().await?;
        lock_owned_backup_set(&mut transaction, owner_user_id, schedule.backup_set_id()).await?;
        let state = sqlx::query_scalar::<_, String>(
            "SELECT state FROM backup_sets
             WHERE id = $1 AND owner_user_id = $2 FOR UPDATE",
        )
        .bind(schedule.backup_set_id().into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_one(&mut *transaction)
        .await?;
        let state = BackupSetState::from_str(&state)
            .map_err(|_| BackupSchedulerError::InvalidPersistedData)?;
        let fence = sqlx::query_as::<_, ScheduleFenceRow>(
            "SELECT owner_user_id, backup_set_id, current_revision_id, enabled, effective_from
             FROM backup_schedules
             WHERE id = $1 AND owner_user_id = $2 FOR UPDATE",
        )
        .bind(schedule.id().into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BackupSchedulerError::InvalidPersistedData)?;
        if state != BackupSetState::Active
            || fence.owner_user_id != owner_user_id.into_uuid()
            || fence.backup_set_id != schedule.backup_set_id().into_uuid()
            || fence.current_revision_id != schedule.current_revision_id().into_uuid()
            || !fence.enabled
            || Timestamp::from_offset_datetime(fence.effective_from) != schedule.effective_from()
        {
            transaction.commit().await?;
            return Ok(None);
        }

        let current_reference =
            resolution_reference_in_transaction(&mut transaction, owner_user_id, schedule).await?;
        if current_reference != resolved_from_exclusive_utc {
            let existing = load_skip_by_boundary(
                &mut transaction,
                owner_user_id,
                schedule,
                planned_through.scheduled_for_utc(),
            )
            .await?;
            transaction.commit().await?;
            return existing
                .map(|row| skip_from_row(row, schedule.current_revision()))
                .transpose();
        }

        let cutoff = observed_at_utc
            .checked_sub_std(StdDuration::from_secs(u64::from(
                schedule.current_revision().max_lateness_seconds(),
            )))
            .ok_or(BackupSchedulerError::InvalidTarget)?;
        if planned_through.schedule_id() != schedule.id()
            || planned_through.schedule_revision_id() != schedule.current_revision_id()
            || planned_through.scheduled_for_utc() <= current_reference
            || planned_through.scheduled_for_utc() >= cutoff
            || planned_through.scheduled_for_utc() > observed_at_utc
        {
            return Err(BackupSchedulerError::InvalidTarget);
        }

        let skip_id = BackupScheduleMisfireSkipId::new();
        let row = sqlx::query_as::<_, MisfireSkipRow>(
            "INSERT INTO backup_schedule_misfire_skips
                (id, owner_user_id, backup_set_id, schedule_id,
                 schedule_revision_id, activation_effective_from,
                 resolved_from_exclusive_utc, resolved_through_utc,
                 observed_at_utc, misfire_mode, max_lateness_seconds, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $9)
             RETURNING id, owner_user_id, backup_set_id, schedule_id,
                       schedule_revision_id, activation_effective_from,
                       resolved_from_exclusive_utc, resolved_through_utc,
                       observed_at_utc, misfire_mode, max_lateness_seconds, created_at",
        )
        .bind(skip_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(schedule.backup_set_id().into_uuid())
        .bind(schedule.id().into_uuid())
        .bind(schedule.current_revision_id().into_uuid())
        .bind(schedule.effective_from().as_offset_datetime())
        .bind(resolved_from_exclusive_utc.as_offset_datetime())
        .bind(planned_through.scheduled_for_utc().as_offset_datetime())
        .bind(observed_at_utc.as_offset_datetime())
        .bind(schedule.current_revision().misfire_mode().as_str())
        .bind(
            i32::try_from(schedule.current_revision().max_lateness_seconds())
                .map_err(|_| BackupSchedulerError::InvalidPersistedData)?,
        )
        .fetch_one(&mut *transaction)
        .await?;
        let skip = skip_from_row(row, schedule.current_revision())?;
        transaction.commit().await?;
        Ok(Some(skip))
    }
}

impl BackupScheduleService {
    pub async fn run_scheduler_tick(
        &self,
        observed_at_utc: Timestamp,
    ) -> Result<BackupSchedulerTickResult, BackupSchedulerError> {
        BackupSchedulerService::new(self.pool().clone())
            .run_scheduler_tick(observed_at_utc)
            .await
    }
}

fn tick_outcome(
    handoff: &BackupScheduleOccurrenceHandoffResult,
) -> Result<BackupSchedulerTickOutcome, BackupSchedulerError> {
    BackupSchedulerTickOutcome::from_handoff_result(handoff)
        .map_err(|_| BackupSchedulerError::InvalidPersistedData)
}

async fn load_current_schedule_descriptors(
    pool: &DatabasePool,
) -> Result<Vec<ScheduleDescriptor>, BackupSchedulerError> {
    let rows = sqlx::query_as::<_, CurrentScheduleRow>(
        "SELECT backup_set.state AS backup_set_state,
                schedule.id AS schedule_id,
                schedule.owner_user_id AS schedule_owner_user_id,
                schedule.backup_set_id AS schedule_backup_set_id,
                schedule.current_revision_id, schedule.enabled,
                schedule.effective_from AS schedule_effective_from,
                schedule.created_at AS schedule_created_at,
                schedule.updated_at AS schedule_updated_at,
                revision.id AS revision_id,
                revision.schedule_id AS revision_schedule_id,
                revision.owner_user_id AS revision_owner_user_id,
                revision.backup_set_id AS revision_backup_set_id,
                revision.revision_number, revision.operation_id,
                revision.fingerprint_version, revision.request_fingerprint,
                revision.recurrence_kind, revision.timezone,
                revision.local_time_minute, revision.weekly_days,
                revision.misfire_mode, revision.max_lateness_seconds,
                revision.created_at AS revision_created_at,
                last_handoff.scheduled_for_utc AS last_handed_off_utc,
                last_skip.resolved_through_utc AS last_skipped_through_utc
         FROM backup_sets AS backup_set
         JOIN backup_schedules AS schedule
           ON schedule.backup_set_id = backup_set.id
          AND schedule.owner_user_id = backup_set.owner_user_id
         JOIN backup_schedule_revisions AS revision
           ON revision.id = schedule.current_revision_id
          AND revision.schedule_id = schedule.id
          AND revision.owner_user_id = schedule.owner_user_id
          AND revision.backup_set_id = schedule.backup_set_id
         LEFT JOIN LATERAL (
             SELECT occurrence.scheduled_for_utc
             FROM backup_schedule_occurrence_handoffs AS handoff
             JOIN backup_schedule_occurrences AS occurrence
               ON occurrence.id = handoff.occurrence_id
              AND occurrence.owner_user_id = handoff.owner_user_id
              AND occurrence.backup_set_id = handoff.backup_set_id
              AND occurrence.schedule_id = handoff.schedule_id
             WHERE handoff.owner_user_id = schedule.owner_user_id
               AND handoff.backup_set_id = schedule.backup_set_id
               AND handoff.schedule_id = schedule.id
               AND occurrence.schedule_revision_id = schedule.current_revision_id
               AND occurrence.scheduled_for_utc > schedule.effective_from
             ORDER BY occurrence.scheduled_for_utc DESC, occurrence.id DESC
             LIMIT 1
         ) AS last_handoff ON TRUE
         LEFT JOIN LATERAL (
             SELECT resolved_through_utc
             FROM backup_schedule_misfire_skips AS skip
             WHERE skip.owner_user_id = schedule.owner_user_id
               AND skip.backup_set_id = schedule.backup_set_id
               AND skip.schedule_id = schedule.id
               AND skip.schedule_revision_id = schedule.current_revision_id
               AND skip.activation_effective_from = schedule.effective_from
             ORDER BY skip.resolved_through_utc DESC, skip.id DESC
             LIMIT 1
         ) AS last_skip ON TRUE
         WHERE backup_set.state = 'ACTIVE' AND schedule.enabled = TRUE",
    )
    .fetch_all(pool.sqlx_pool())
    .await?;
    rows.into_iter()
        .map(ScheduleDescriptor::try_from_row)
        .filter_map(|result| match result {
            Ok(Some(value)) => Some(Ok(value)),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}

async fn resolution_reference_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    schedule: &BackupSchedule,
) -> Result<Timestamp, BackupSchedulerError> {
    let last_handoff = sqlx::query_scalar::<_, Option<OffsetDateTime>>(
        "SELECT max(occurrence.scheduled_for_utc)
         FROM backup_schedule_occurrence_handoffs AS handoff
         JOIN backup_schedule_occurrences AS occurrence
           ON occurrence.id = handoff.occurrence_id
          AND occurrence.owner_user_id = handoff.owner_user_id
          AND occurrence.backup_set_id = handoff.backup_set_id
          AND occurrence.schedule_id = handoff.schedule_id
         WHERE handoff.owner_user_id = $1 AND handoff.backup_set_id = $2
           AND handoff.schedule_id = $3
           AND occurrence.schedule_revision_id = $4
           AND occurrence.scheduled_for_utc > $5",
    )
    .bind(owner_user_id.into_uuid())
    .bind(schedule.backup_set_id().into_uuid())
    .bind(schedule.id().into_uuid())
    .bind(schedule.current_revision_id().into_uuid())
    .bind(schedule.effective_from().as_offset_datetime())
    .fetch_one(&mut **transaction)
    .await?;
    let last_skip = sqlx::query_scalar::<_, Option<OffsetDateTime>>(
        "SELECT max(resolved_through_utc)
         FROM backup_schedule_misfire_skips
         WHERE owner_user_id = $1 AND backup_set_id = $2 AND schedule_id = $3
           AND schedule_revision_id = $4 AND activation_effective_from = $5",
    )
    .bind(owner_user_id.into_uuid())
    .bind(schedule.backup_set_id().into_uuid())
    .bind(schedule.id().into_uuid())
    .bind(schedule.current_revision_id().into_uuid())
    .bind(schedule.effective_from().as_offset_datetime())
    .fetch_one(&mut **transaction)
    .await?;
    let mut reference = schedule.effective_from();
    if let Some(value) = last_handoff {
        reference = reference.max(Timestamp::from_offset_datetime(value));
    }
    if let Some(value) = last_skip {
        reference = reference.max(Timestamp::from_offset_datetime(value));
    }
    Ok(reference)
}

async fn load_skip_by_boundary(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    schedule: &BackupSchedule,
    resolved_through_utc: Timestamp,
) -> Result<Option<MisfireSkipRow>, BackupSchedulerError> {
    Ok(sqlx::query_as::<_, MisfireSkipRow>(
        "SELECT id, owner_user_id, backup_set_id, schedule_id,
                schedule_revision_id, activation_effective_from,
                resolved_from_exclusive_utc, resolved_through_utc,
                observed_at_utc, misfire_mode, max_lateness_seconds, created_at
         FROM backup_schedule_misfire_skips
         WHERE owner_user_id = $1 AND backup_set_id = $2 AND schedule_id = $3
           AND schedule_revision_id = $4 AND activation_effective_from = $5
           AND resolved_through_utc = $6",
    )
    .bind(owner_user_id.into_uuid())
    .bind(schedule.backup_set_id().into_uuid())
    .bind(schedule.id().into_uuid())
    .bind(schedule.current_revision_id().into_uuid())
    .bind(schedule.effective_from().as_offset_datetime())
    .bind(resolved_through_utc.as_offset_datetime())
    .fetch_optional(&mut **transaction)
    .await?)
}

fn skip_from_row(
    row: MisfireSkipRow,
    revision: &BackupScheduleRevision,
) -> Result<BackupScheduleMisfireSkip, BackupSchedulerError> {
    if row.owner_user_id != revision.owner_user_id().into_uuid()
        || row.backup_set_id != revision.backup_set_id().into_uuid()
        || row.schedule_id != revision.schedule_id().into_uuid()
        || row.schedule_revision_id != revision.id().into_uuid()
    {
        return Err(BackupSchedulerError::InvalidPersistedData);
    }
    let mode = BackupScheduleMisfireMode::from_str(&row.misfire_mode)
        .map_err(|_| BackupSchedulerError::InvalidPersistedData)?;
    BackupScheduleMisfireSkip::new(
        BackupScheduleMisfireSkipId::try_from_uuid(row.id)
            .map_err(|_| BackupSchedulerError::InvalidPersistedData)?,
        revision,
        Timestamp::from_offset_datetime(row.activation_effective_from),
        Timestamp::from_offset_datetime(row.resolved_from_exclusive_utc),
        Timestamp::from_offset_datetime(row.resolved_through_utc),
        Timestamp::from_offset_datetime(row.observed_at_utc),
        mode,
        u32::try_from(row.max_lateness_seconds)
            .map_err(|_| BackupSchedulerError::InvalidPersistedData)?,
        Timestamp::from_offset_datetime(row.created_at),
    )
    .map_err(|_| BackupSchedulerError::InvalidPersistedData)
}

#[allow(clippy::too_many_arguments)]
fn decode_revision(
    id: BackupScheduleRevisionId,
    schedule_id: BackupScheduleId,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
    revision_number: i64,
    operation_id: String,
    fingerprint_version: i16,
    request_fingerprint: Vec<u8>,
    recurrence_kind: String,
    timezone: String,
    local_time_minute: i16,
    weekly_days: Vec<i16>,
    misfire_mode: String,
    max_lateness_seconds: i32,
    created_at: OffsetDateTime,
) -> Result<BackupScheduleRevision, BackupSchedulerError> {
    let revision_number = BackupScheduleRevisionNumber::new(
        u64::try_from(revision_number).map_err(|_| BackupSchedulerError::InvalidPersistedData)?,
    )
    .map_err(|_| BackupSchedulerError::InvalidPersistedData)?;
    let version = u16::try_from(fingerprint_version)
        .map_err(|_| BackupSchedulerError::InvalidPersistedData)?;
    if version != BACKUP_SCHEDULE_LEGACY_FINGERPRINT_VERSION
        && version != BACKUP_SCHEDULE_FINGERPRINT_VERSION
    {
        return Err(BackupSchedulerError::InvalidPersistedData);
    }
    let bytes = <[u8; 32]>::try_from(request_fingerprint.as_slice())
        .map_err(|_| BackupSchedulerError::InvalidPersistedData)?;
    let recurrence_kind = BackupScheduleRecurrenceKind::from_str(&recurrence_kind)
        .map_err(|_| BackupSchedulerError::InvalidPersistedData)?;
    let timezone = BackupScheduleTimezone::from_str(&timezone)
        .map_err(|_| BackupSchedulerError::InvalidPersistedData)?;
    let local_time = BackupScheduleLocalTime::from_minute(
        u16::try_from(local_time_minute).map_err(|_| BackupSchedulerError::InvalidPersistedData)?,
    )
    .map_err(|_| BackupSchedulerError::InvalidPersistedData)?;
    let weekly_days = weekly_days
        .into_iter()
        .map(|day| {
            BackupScheduleWeekday::from_ordinal(
                u8::try_from(day).map_err(|_| BackupSchedulerError::InvalidPersistedData)?,
            )
            .ok_or(BackupSchedulerError::InvalidPersistedData)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mode = BackupScheduleMisfireMode::from_str(&misfire_mode)
        .map_err(|_| BackupSchedulerError::InvalidPersistedData)?;
    let config = synveil_core::BackupScheduleConfig::new_with_misfire_policy(
        recurrence_kind,
        timezone,
        local_time,
        weekly_days,
        mode,
        u32::try_from(max_lateness_seconds)
            .map_err(|_| BackupSchedulerError::InvalidPersistedData)?,
    )
    .map_err(|_| BackupSchedulerError::InvalidPersistedData)?;
    BackupScheduleRevision::new(
        id,
        schedule_id,
        owner_user_id,
        backup_set_id,
        revision_number,
        operation_id,
        BackupScheduleIdempotencyFingerprint::new(version, bytes),
        config,
        Timestamp::from_offset_datetime(created_at),
    )
    .map_err(|_| BackupSchedulerError::InvalidPersistedData)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_order_is_time_then_stable_logical_ids() {
        let first = (
            Timestamp::parse("2026-09-02T08:00:00Z").unwrap(),
            BackupScheduleId::new(),
            BackupScheduleRevisionId::new(),
        );
        let second = (
            Timestamp::parse("2026-09-02T07:30:00Z").unwrap(),
            BackupScheduleId::new(),
            BackupScheduleRevisionId::new(),
        );
        assert!(second < first);
    }
}
