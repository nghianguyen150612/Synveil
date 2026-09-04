//! Exactly-once binding from a materialized schedule occurrence to the
//! existing metadata-only backup maintenance orchestration.
//!
//! The handoff is deliberately a small immutable relation. It never advances
//! a maintenance run, captures a snapshot, evaluates expiry, touches an
//! ObjectStore, or emits journal/sync evidence. A committed relation is the
//! provenance fact that lets every later retry replay the same maintenance-run
//! identity.

use std::{fmt, time::Duration as StdDuration};

use sqlx::{FromRow, Postgres, Transaction};
use synveil_core::{
    BackupMaintenanceRun, BackupMaintenanceRunId, BackupMaintenanceRunPreflightIssue,
    BackupScheduleId, BackupScheduleOccurrence, BackupScheduleOccurrenceHandoff,
    BackupScheduleOccurrenceHandoffResult, BackupScheduleOccurrenceId,
    BackupScheduleOccurrenceNotEffectiveReason, BackupSetId, BackupSetState, Timestamp, UserId,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    BackupError, BackupScheduleError, BackupScheduleService as MetadataBackupScheduleService,
    BackupService, DatabaseError, DatabasePool,
    backup::{
        load_maintenance_run_for_update, load_maintenance_run_in_transaction,
        lock_owned_backup_set, validate_operation_key,
    },
    occurrences::load_occurrence_in_transaction,
};

/// Bounded failures for the scheduled handoff boundary. The `Backup` variant
/// is retained only for unexpected canonical maintenance errors; expected
/// retention-policy and active-run preflights remain typed and replay-safe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackupScheduleHandoffError {
    NotFound,
    NotEffective(BackupScheduleOccurrenceNotEffectiveReason),
    MaintenanceRunPreflight(BackupMaintenanceRunPreflightIssue),
    InvalidRequest,
    Database(DatabaseError),
    InvalidPersistedData,
    Backup(BackupError),
}

impl fmt::Display for BackupScheduleHandoffError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("backup schedule occurrence was not found"),
            Self::NotEffective(reason) => match reason {
                BackupScheduleOccurrenceNotEffectiveReason::ScheduleDisabled => formatter
                    .write_str(
                        "scheduled handoff is not effective because the schedule is disabled",
                    ),
                BackupScheduleOccurrenceNotEffectiveReason::BackupSetInactive => formatter
                    .write_str(
                        "scheduled handoff is not effective because the backup set is inactive",
                    ),
                _ => formatter.write_str("scheduled handoff is not effective"),
            },
            Self::MaintenanceRunPreflight(issue) => issue.fmt(formatter),
            Self::InvalidRequest => formatter.write_str("scheduled handoff request is invalid"),
            Self::Database(error) => error.fmt(formatter),
            Self::InvalidPersistedData => {
                formatter.write_str("scheduled handoff persisted data is invalid")
            }
            Self::Backup(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for BackupScheduleHandoffError {}

impl From<sqlx::Error> for BackupScheduleHandoffError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(DatabaseError::from(error))
    }
}

impl From<BackupError> for BackupScheduleHandoffError {
    fn from(error: BackupError) -> Self {
        match error {
            BackupError::NotFound => Self::NotFound,
            BackupError::InvalidRequest => Self::InvalidRequest,
            BackupError::InvalidPersistedData => Self::InvalidPersistedData,
            BackupError::Database(error) => Self::Database(error),
            BackupError::MaintenanceRunPreflight(issue) => Self::MaintenanceRunPreflight(issue),
            other => Self::Backup(other),
        }
    }
}

impl From<BackupScheduleError> for BackupScheduleHandoffError {
    fn from(error: BackupScheduleError) -> Self {
        match error {
            BackupScheduleError::NotFound => Self::NotFound,
            BackupScheduleError::Database(error) => Self::Database(error),
            BackupScheduleError::InvalidPersistedData => Self::InvalidPersistedData,
            _ => Self::InvalidPersistedData,
        }
    }
}

#[derive(Clone, Debug, FromRow, PartialEq)]
struct OccurrenceScopeRow {
    backup_set_id: Uuid,
    schedule_id: Uuid,
}

#[derive(Clone, Debug, FromRow, PartialEq)]
struct ScheduleHandoffRow {
    id: Uuid,
    owner_user_id: Uuid,
    backup_set_id: Uuid,
    current_revision_id: Uuid,
    enabled: bool,
    effective_from: OffsetDateTime,
}

#[derive(Clone, Debug, FromRow, PartialEq)]
struct BackupScheduleOccurrenceHandoffRow {
    occurrence_id: Uuid,
    owner_user_id: Uuid,
    backup_set_id: Uuid,
    schedule_id: Uuid,
    maintenance_run_id: Uuid,
    created_at: OffsetDateTime,
}

impl BackupScheduleOccurrenceHandoffRow {
    fn try_into_domain(
        self,
        occurrence: &BackupScheduleOccurrence,
        maintenance_run: &BackupMaintenanceRun,
    ) -> Result<BackupScheduleOccurrenceHandoff, BackupScheduleHandoffError> {
        if self.occurrence_id != occurrence.id().into_uuid()
            || self.owner_user_id != occurrence.owner_user_id().into_uuid()
            || self.backup_set_id != occurrence.backup_set_id().into_uuid()
            || self.schedule_id != occurrence.schedule_id().into_uuid()
            || self.maintenance_run_id != maintenance_run.id().into_uuid()
        {
            return Err(BackupScheduleHandoffError::InvalidPersistedData);
        }
        BackupScheduleOccurrenceHandoff::new(
            occurrence,
            maintenance_run,
            Timestamp::from_offset_datetime(self.created_at),
        )
        .map_err(|_| BackupScheduleHandoffError::InvalidPersistedData)
    }
}

impl BackupService {
    /// Atomically bind one already-materialized occurrence to one canonical
    /// Prompt 49 maintenance run. The transaction lock order is:
    /// `BackupSet -> BackupSchedule -> Occurrence -> MaintenanceRun/policy ->
    /// Handoff`. Existing handoff replay is checked before new-handoff
    /// effectivity checks, so later schedule/set disablement cannot invalidate
    /// a committed execution identity.
    pub async fn handoff_backup_schedule_occurrence(
        &self,
        owner_user_id: UserId,
        occurrence_id: BackupScheduleOccurrenceId,
        observed_at_utc: Timestamp,
    ) -> Result<BackupScheduleOccurrenceHandoffResult, BackupScheduleHandoffError> {
        handoff_backup_schedule_occurrence(
            self.pool(),
            owner_user_id,
            occurrence_id,
            observed_at_utc,
            false,
        )
        .await
    }

    /// Scheduler-only handoff entry point. It uses the same Prompt 63
    /// transaction and maintenance creator, but adds the scheduler policy
    /// that the occurrence must still belong to the current activation epoch.
    /// The method is crate-visible because no scheduler HTTP/API boundary is
    /// intended by Prompt 64.
    pub(crate) async fn handoff_backup_schedule_occurrence_for_scheduler(
        &self,
        owner_user_id: UserId,
        occurrence_id: BackupScheduleOccurrenceId,
        observed_at_utc: Timestamp,
    ) -> Result<BackupScheduleOccurrenceHandoffResult, BackupScheduleHandoffError> {
        handoff_backup_schedule_occurrence(
            self.pool(),
            owner_user_id,
            occurrence_id,
            observed_at_utc,
            true,
        )
        .await
    }

    /// Owner-scoped read of the immutable handoff evidence and its canonical
    /// maintenance run. `None` is the valid result for an ordinary manual run.
    pub async fn get_backup_schedule_occurrence_handoff(
        &self,
        owner_user_id: UserId,
        occurrence_id: BackupScheduleOccurrenceId,
    ) -> Result<Option<BackupScheduleOccurrenceHandoff>, BackupScheduleHandoffError> {
        get_handoff_for_occurrence(self.pool(), owner_user_id, occurrence_id).await
    }

    /// Owner-scoped reverse provenance lookup for future workers and audit.
    /// Manual maintenance runs return `None` because they have no scheduled
    /// occurrence relation.
    pub async fn get_scheduled_occurrence_for_maintenance_run(
        &self,
        owner_user_id: UserId,
        maintenance_run_id: BackupMaintenanceRunId,
    ) -> Result<Option<BackupScheduleOccurrence>, BackupScheduleHandoffError> {
        get_scheduled_occurrence_for_maintenance_run(self.pool(), owner_user_id, maintenance_run_id)
            .await
    }
}

impl MetadataBackupScheduleService {
    /// Schedule-service convenience boundary for the cross-domain handoff.
    /// The implementation delegates to `BackupService`; it does not introduce
    /// a second maintenance state machine.
    pub async fn handoff_backup_schedule_occurrence(
        &self,
        owner_user_id: UserId,
        occurrence_id: BackupScheduleOccurrenceId,
        observed_at_utc: Timestamp,
    ) -> Result<BackupScheduleOccurrenceHandoffResult, BackupScheduleHandoffError> {
        BackupService::new(self.pool().clone())
            .handoff_backup_schedule_occurrence(owner_user_id, occurrence_id, observed_at_utc)
            .await
    }

    pub async fn get_handoff_for_occurrence(
        &self,
        owner_user_id: UserId,
        occurrence_id: BackupScheduleOccurrenceId,
    ) -> Result<Option<BackupScheduleOccurrenceHandoff>, BackupScheduleHandoffError> {
        get_handoff_for_occurrence(self.pool(), owner_user_id, occurrence_id).await
    }

    pub async fn get_scheduled_occurrence_for_maintenance_run(
        &self,
        owner_user_id: UserId,
        maintenance_run_id: BackupMaintenanceRunId,
    ) -> Result<Option<BackupScheduleOccurrence>, BackupScheduleHandoffError> {
        get_scheduled_occurrence_for_maintenance_run(self.pool(), owner_user_id, maintenance_run_id)
            .await
    }
}

async fn handoff_backup_schedule_occurrence(
    pool: &DatabasePool,
    owner_user_id: UserId,
    occurrence_id: BackupScheduleOccurrenceId,
    observed_at_utc: Timestamp,
    enforce_scheduler_epoch: bool,
) -> Result<BackupScheduleOccurrenceHandoffResult, BackupScheduleHandoffError> {
    let mut transaction = pool.sqlx_pool().begin().await?;

    // This initial read discovers the immutable BackupSet/schedule fence
    // without taking a child lock before the canonical parent locks.
    let scope = sqlx::query_as::<_, OccurrenceScopeRow>(
        "SELECT backup_set_id, schedule_id
         FROM backup_schedule_occurrences
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(occurrence_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(BackupScheduleHandoffError::NotFound)?;
    let backup_set_id = BackupSetId::try_from_uuid(scope.backup_set_id)
        .map_err(|_| BackupScheduleHandoffError::InvalidPersistedData)?;
    let schedule_id = BackupScheduleId::try_from_uuid(scope.schedule_id)
        .map_err(|_| BackupScheduleHandoffError::InvalidPersistedData)?;

    // Canonical mutation order begins at the existing BackupSet fence. This
    // serializes against schedule edits, schedule disablement, and set state
    // transitions before the occurrence can be claimed.
    lock_owned_backup_set(&mut transaction, owner_user_id, backup_set_id)
        .await
        .map_err(BackupScheduleHandoffError::from)?;
    let backup_set_state = sqlx::query_scalar::<_, String>(
        "SELECT state
         FROM backup_sets
         WHERE id = $1 AND owner_user_id = $2
         FOR UPDATE",
    )
    .bind(backup_set_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_one(&mut *transaction)
    .await?;

    let schedule = sqlx::query_as::<_, ScheduleHandoffRow>(
        "SELECT id, owner_user_id, backup_set_id, current_revision_id,
                enabled, effective_from
         FROM backup_schedules
         WHERE id = $1 AND owner_user_id = $2
         FOR UPDATE",
    )
    .bind(schedule_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(BackupScheduleHandoffError::InvalidPersistedData)?;
    if schedule.id != schedule_id.into_uuid()
        || schedule.owner_user_id != owner_user_id.into_uuid()
        || schedule.backup_set_id != backup_set_id.into_uuid()
    {
        return Err(BackupScheduleHandoffError::InvalidPersistedData);
    }

    // Occurrence is immutable, but taking its lock after its parents keeps
    // direct writers and future relation writers from creating a reverse lock
    // order. Rehydration also proves its historical revision is still valid.
    let occurrence =
        load_occurrence_in_transaction(&mut transaction, owner_user_id, occurrence_id, true)
            .await?
            .ok_or(BackupScheduleHandoffError::InvalidPersistedData)?;
    if occurrence.backup_set_id() != backup_set_id || occurrence.schedule_id() != schedule_id {
        return Err(BackupScheduleHandoffError::InvalidPersistedData);
    }

    // Replay is deliberately before the new-handoff state fence. A committed
    // relation remains authoritative after revision edits or disablement.
    if let Some(row) =
        load_handoff_row(&mut transaction, owner_user_id, occurrence_id, true).await?
    {
        let run_id = BackupMaintenanceRunId::try_from_uuid(row.maintenance_run_id)
            .map_err(|_| BackupScheduleHandoffError::InvalidPersistedData)?;
        let maintenance_run =
            load_maintenance_run_for_update(&mut transaction, owner_user_id, run_id)
                .await
                .map_err(BackupScheduleHandoffError::from)?
                .ok_or(BackupScheduleHandoffError::InvalidPersistedData)?
                .try_into_domain()
                .map_err(|_| BackupScheduleHandoffError::InvalidPersistedData)?;
        let handoff = row.try_into_domain(&occurrence, &maintenance_run)?;
        transaction.commit().await?;
        return Ok(BackupScheduleOccurrenceHandoffResult::Existing {
            occurrence,
            maintenance_run,
            handoff,
        });
    }

    let backup_set_state = backup_set_state
        .parse::<BackupSetState>()
        .map_err(|_| BackupScheduleHandoffError::InvalidPersistedData)?;
    if backup_set_state != BackupSetState::Active {
        transaction.commit().await?;
        return Err(BackupScheduleHandoffError::NotEffective(
            BackupScheduleOccurrenceNotEffectiveReason::BackupSetInactive,
        ));
    }
    if !schedule.enabled {
        transaction.commit().await?;
        return Err(BackupScheduleHandoffError::NotEffective(
            BackupScheduleOccurrenceNotEffectiveReason::ScheduleDisabled,
        ));
    }
    if enforce_scheduler_epoch {
        if schedule.current_revision_id != occurrence.schedule_revision_id().into_uuid() {
            transaction.commit().await?;
            return Err(BackupScheduleHandoffError::NotEffective(
                BackupScheduleOccurrenceNotEffectiveReason::RevisionSuperseded,
            ));
        }
        if occurrence.scheduled_for_utc()
            <= Timestamp::from_offset_datetime(schedule.effective_from)
        {
            transaction.commit().await?;
            return Err(BackupScheduleHandoffError::NotEffective(
                BackupScheduleOccurrenceNotEffectiveReason::BeforeEffectiveFrom,
            ));
        }

        let max_lateness_seconds = sqlx::query_scalar::<_, i32>(
            "SELECT max_lateness_seconds
             FROM backup_schedule_revisions
             WHERE id = $1 AND schedule_id = $2
               AND owner_user_id = $3 AND backup_set_id = $4",
        )
        .bind(schedule.current_revision_id)
        .bind(schedule.id)
        .bind(owner_user_id.into_uuid())
        .bind(backup_set_id.into_uuid())
        .fetch_one(&mut *transaction)
        .await?;
        let cutoff = observed_at_utc
            .checked_sub_std(StdDuration::from_secs(
                u64::try_from(max_lateness_seconds)
                    .map_err(|_| BackupScheduleHandoffError::InvalidPersistedData)?,
            ))
            .ok_or(BackupScheduleHandoffError::InvalidPersistedData)?;
        if occurrence.scheduled_for_utc() < cutoff {
            transaction.commit().await?;
            return Err(BackupScheduleHandoffError::NotEffective(
                BackupScheduleOccurrenceNotEffectiveReason::ExpiredForAutomaticExecution,
            ));
        }

        let latest_resolution = sqlx::query_scalar::<_, Option<OffsetDateTime>>(
            "SELECT max(progress.scheduled_for_utc)
             FROM (
                 SELECT prior.scheduled_for_utc
                 FROM backup_schedule_occurrence_handoffs AS prior_handoff
                 JOIN backup_schedule_occurrences AS prior
                   ON prior.id = prior_handoff.occurrence_id
                  AND prior.owner_user_id = prior_handoff.owner_user_id
                  AND prior.backup_set_id = prior_handoff.backup_set_id
                  AND prior.schedule_id = prior_handoff.schedule_id
                 WHERE prior_handoff.owner_user_id = $1
                   AND prior_handoff.backup_set_id = $2
                   AND prior_handoff.schedule_id = $3
                   AND prior.schedule_revision_id = $4
                   AND prior.scheduled_for_utc > $5
                 UNION ALL
                 SELECT skip.resolved_through_utc
                 FROM backup_schedule_misfire_skips AS skip
                 WHERE skip.owner_user_id = $1
                   AND skip.backup_set_id = $2
                   AND skip.schedule_id = $3
                   AND skip.schedule_revision_id = $4
                   AND skip.activation_effective_from = $5
             ) AS progress",
        )
        .bind(owner_user_id.into_uuid())
        .bind(backup_set_id.into_uuid())
        .bind(schedule.id)
        .bind(schedule.current_revision_id)
        .bind(schedule.effective_from)
        .fetch_one(&mut *transaction)
        .await?;
        if latest_resolution.is_some_and(|resolved| {
            Timestamp::from_offset_datetime(resolved) >= occurrence.scheduled_for_utc()
        }) {
            transaction.commit().await?;
            return Err(BackupScheduleHandoffError::NotEffective(
                BackupScheduleOccurrenceNotEffectiveReason::SchedulerProgressResolved,
            ));
        }
    }

    let operation_id = format!("schedule-occurrence:{occurrence_id}:maintenance");
    let run_id = BackupMaintenanceRunId::new();
    let capture_operation_id = format!("maint:{run_id}:capture");
    let expiry_plan_operation_id = format!("maint:{run_id}:expiry-plan");
    validate_operation_key(&operation_id).map_err(BackupScheduleHandoffError::from)?;
    validate_operation_key(&capture_operation_id).map_err(BackupScheduleHandoffError::from)?;
    validate_operation_key(&expiry_plan_operation_id).map_err(BackupScheduleHandoffError::from)?;

    // Prompt 49's canonical creator binds the current retention policy,
    // enforces one active run per BackupSet, creates CREATED state, and emits
    // all child operation identities. The false replay flag prevents a
    // deterministic operation-key collision from attaching an arbitrary
    // existing manual run.
    let maintenance_run = BackupService::create_backup_maintenance_run_in_transaction(
        &mut transaction,
        owner_user_id,
        &operation_id,
        backup_set_id,
        run_id,
        &capture_operation_id,
        &expiry_plan_operation_id,
        false,
    )
    .await
    .map_err(BackupScheduleHandoffError::from)?;
    if maintenance_run.state() != synveil_core::BackupMaintenanceRunState::Created {
        return Err(BackupScheduleHandoffError::InvalidPersistedData);
    }

    let inserted = sqlx::query_as::<_, BackupScheduleOccurrenceHandoffRow>(
        "INSERT INTO backup_schedule_occurrence_handoffs
            (occurrence_id, owner_user_id, backup_set_id, schedule_id,
             maintenance_run_id, created_at)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT DO NOTHING
         RETURNING occurrence_id, owner_user_id, backup_set_id, schedule_id,
                   maintenance_run_id, created_at",
    )
    .bind(occurrence_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .bind(schedule_id.into_uuid())
    .bind(maintenance_run.id().into_uuid())
    .bind(observed_at_utc.as_offset_datetime())
    .fetch_optional(&mut *transaction)
    .await?;

    let Some(inserted) = inserted else {
        // The new run is intentionally rolled back with the transaction. A
        // direct relation writer can never turn this path into an orphan or a
        // second run; the next canonical caller replays the committed winner.
        return Err(BackupScheduleHandoffError::InvalidPersistedData);
    };
    let handoff = inserted.try_into_domain(&occurrence, &maintenance_run)?;
    transaction.commit().await?;
    Ok(BackupScheduleOccurrenceHandoffResult::Created {
        occurrence,
        maintenance_run,
        handoff,
    })
}

async fn get_handoff_for_occurrence(
    pool: &DatabasePool,
    owner_user_id: UserId,
    occurrence_id: BackupScheduleOccurrenceId,
) -> Result<Option<BackupScheduleOccurrenceHandoff>, BackupScheduleHandoffError> {
    let mut transaction = pool.sqlx_pool().begin().await?;
    let Some(occurrence) =
        load_occurrence_in_transaction(&mut transaction, owner_user_id, occurrence_id, false)
            .await?
    else {
        transaction.commit().await?;
        return Ok(None);
    };
    let Some(row) = load_handoff_row(&mut transaction, owner_user_id, occurrence_id, false).await?
    else {
        transaction.commit().await?;
        return Ok(None);
    };
    let run_id = BackupMaintenanceRunId::try_from_uuid(row.maintenance_run_id)
        .map_err(|_| BackupScheduleHandoffError::InvalidPersistedData)?;
    let run = load_maintenance_run_in_transaction(&mut transaction, owner_user_id, run_id, false)
        .await
        .map_err(BackupScheduleHandoffError::from)?
        .ok_or(BackupScheduleHandoffError::InvalidPersistedData)?
        .try_into_domain()
        .map_err(|_| BackupScheduleHandoffError::InvalidPersistedData)?;
    let handoff = row.try_into_domain(&occurrence, &run)?;
    transaction.commit().await?;
    Ok(Some(handoff))
}

async fn get_scheduled_occurrence_for_maintenance_run(
    pool: &DatabasePool,
    owner_user_id: UserId,
    maintenance_run_id: BackupMaintenanceRunId,
) -> Result<Option<BackupScheduleOccurrence>, BackupScheduleHandoffError> {
    let mut transaction = pool.sqlx_pool().begin().await?;
    let Some(row) =
        load_handoff_row_by_maintenance_run(&mut transaction, owner_user_id, maintenance_run_id)
            .await?
    else {
        transaction.commit().await?;
        return Ok(None);
    };
    let occurrence_id = BackupScheduleOccurrenceId::try_from_uuid(row.occurrence_id)
        .map_err(|_| BackupScheduleHandoffError::InvalidPersistedData)?;
    let occurrence =
        load_occurrence_in_transaction(&mut transaction, owner_user_id, occurrence_id, false)
            .await?
            .ok_or(BackupScheduleHandoffError::InvalidPersistedData)?;
    let run = load_maintenance_run_in_transaction(
        &mut transaction,
        owner_user_id,
        maintenance_run_id,
        false,
    )
    .await
    .map_err(BackupScheduleHandoffError::from)?
    .ok_or(BackupScheduleHandoffError::InvalidPersistedData)?
    .try_into_domain()
    .map_err(|_| BackupScheduleHandoffError::InvalidPersistedData)?;
    row.try_into_domain(&occurrence, &run)?;
    transaction.commit().await?;
    Ok(Some(occurrence))
}

async fn load_handoff_row(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    occurrence_id: BackupScheduleOccurrenceId,
    lock: bool,
) -> Result<Option<BackupScheduleOccurrenceHandoffRow>, BackupScheduleHandoffError> {
    let query = if lock {
        "SELECT occurrence_id, owner_user_id, backup_set_id, schedule_id,
                maintenance_run_id, created_at
         FROM backup_schedule_occurrence_handoffs
         WHERE occurrence_id = $1 AND owner_user_id = $2
         FOR SHARE"
    } else {
        "SELECT occurrence_id, owner_user_id, backup_set_id, schedule_id,
                maintenance_run_id, created_at
         FROM backup_schedule_occurrence_handoffs
         WHERE occurrence_id = $1 AND owner_user_id = $2"
    };
    sqlx::query_as::<_, BackupScheduleOccurrenceHandoffRow>(query)
        .bind(occurrence_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(Into::into)
}

async fn load_handoff_row_by_maintenance_run(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    maintenance_run_id: BackupMaintenanceRunId,
) -> Result<Option<BackupScheduleOccurrenceHandoffRow>, BackupScheduleHandoffError> {
    sqlx::query_as::<_, BackupScheduleOccurrenceHandoffRow>(
        "SELECT occurrence_id, owner_user_id, backup_set_id, schedule_id,
                maintenance_run_id, created_at
         FROM backup_schedule_occurrence_handoffs
         WHERE maintenance_run_id = $1 AND owner_user_id = $2",
    )
    .bind(maintenance_run_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(Into::into)
}
