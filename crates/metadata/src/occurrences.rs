//! Durable backup-schedule occurrence ledger.
//!
//! Materialization recognizes one exact due logical target under the same
//! `BackupSet -> BackupSchedule` lock order used by schedule configuration.
//! It does not discover targets, run work, touch an ObjectStore, create a
//! snapshot/maintenance run, or mutate journal/sync/restore/prune/GC state.

use std::str::FromStr;

use sqlx::{FromRow, Postgres, Transaction};
use synveil_core::{
    BackupScheduleId, BackupScheduleLocalTime, BackupScheduleOccurrence,
    BackupScheduleOccurrenceId, BackupScheduleOccurrenceMaterializationResult,
    BackupScheduleOccurrenceNotEffectiveReason, BackupScheduleRevision, BackupScheduleRevisionId,
    BackupSetId, BackupSetState, Timestamp, UserId,
};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::{BackupScheduleError, BackupScheduleService};

use crate::scheduling::{load_revision_row, lock_owned_backup_set};

#[derive(Clone, Debug, FromRow, PartialEq)]
struct BackupScheduleOccurrenceRow {
    id: Uuid,
    owner_user_id: Uuid,
    backup_set_id: Uuid,
    schedule_id: Uuid,
    schedule_revision_id: Uuid,
    local_calendar_date: Date,
    resolved_local_time_minute: i16,
    scheduled_for_utc: OffsetDateTime,
    materialized_at: OffsetDateTime,
}

impl BackupScheduleOccurrenceRow {
    fn try_into_domain(
        self,
        revision: &BackupScheduleRevision,
    ) -> Result<BackupScheduleOccurrence, BackupScheduleError> {
        if self.owner_user_id != revision.owner_user_id().into_uuid()
            || self.backup_set_id != revision.backup_set_id().into_uuid()
            || self.schedule_id != revision.schedule_id().into_uuid()
            || self.schedule_revision_id != revision.id().into_uuid()
        {
            return Err(BackupScheduleError::InvalidPersistedData);
        }
        let resolved_local_wall_time = BackupScheduleLocalTime::from_minute(
            u16::try_from(self.resolved_local_time_minute)
                .map_err(|_| BackupScheduleError::InvalidPersistedData)?,
        )
        .map_err(|_| BackupScheduleError::InvalidPersistedData)?;
        BackupScheduleOccurrence::new(
            BackupScheduleOccurrenceId::try_from_uuid(self.id)
                .map_err(|_| BackupScheduleError::InvalidPersistedData)?,
            revision,
            self.local_calendar_date,
            resolved_local_wall_time,
            Timestamp::from_offset_datetime(self.scheduled_for_utc),
            Timestamp::from_offset_datetime(self.materialized_at),
        )
        .map_err(|_| BackupScheduleError::InvalidPersistedData)
    }
}

#[derive(Clone, Debug, FromRow, PartialEq)]
struct BackupScheduleMaterializationFenceRow {
    backup_set_id: Uuid,
    current_revision_id: Uuid,
    enabled: bool,
    effective_from: OffsetDateTime,
}

impl BackupScheduleService {
    /// Materialize one exact local-date target. The caller supplies no UTC
    /// instant, offset, resolved local time, or DST decision; those values are
    /// recomputed from the immutable revision by `synveil-core`.
    pub async fn materialize_due_backup_schedule_occurrence(
        &self,
        owner_user_id: UserId,
        schedule_id: BackupScheduleId,
        schedule_revision_id: BackupScheduleRevisionId,
        local_calendar_date: Date,
        observed_at_utc: Timestamp,
    ) -> Result<BackupScheduleOccurrenceMaterializationResult, BackupScheduleError> {
        let mut transaction = self.pool().sqlx_pool().begin().await?;

        // Schedule identity is immutable, so this nonlocking owner-scoped read
        // safely discovers the canonical BackupSet fence. The mutation lock
        // order then remains BackupSet first, schedule second, exactly as in
        // Prompt 61 configuration and enable/disable operations.
        let backup_set_uuid = sqlx::query_scalar::<_, Uuid>(
            "SELECT backup_set_id
             FROM backup_schedules
             WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(schedule_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BackupScheduleError::NotFound)?;
        let backup_set_id = BackupSetId::try_from_uuid(backup_set_uuid)
            .map_err(|_| BackupScheduleError::InvalidPersistedData)?;

        lock_owned_backup_set(&mut transaction, owner_user_id, backup_set_id).await?;
        let backup_set_state = sqlx::query_scalar::<_, String>(
            "SELECT state
             FROM backup_sets
             WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(backup_set_uuid)
        .bind(owner_user_id.into_uuid())
        .fetch_one(&mut *transaction)
        .await?;
        let backup_set_state = BackupSetState::from_str(&backup_set_state)
            .map_err(|_| BackupScheduleError::InvalidPersistedData)?;

        let schedule = sqlx::query_as::<_, BackupScheduleMaterializationFenceRow>(
            "SELECT backup_set_id, current_revision_id, enabled, effective_from
             FROM backup_schedules
             WHERE id = $1 AND owner_user_id = $2
             FOR UPDATE",
        )
        .bind(schedule_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BackupScheduleError::NotFound)?;
        if schedule.backup_set_id != backup_set_uuid {
            return Err(BackupScheduleError::InvalidPersistedData);
        }

        // Replay must precede current/enabled/effectivity rejection. This is
        // what recovers a committed result after a lost response even when a
        // later edit or disable has made the requested revision historical.
        if let Some(existing) = load_occurrence_by_logical_key(
            &mut transaction,
            owner_user_id,
            schedule_id,
            schedule_revision_id,
            local_calendar_date,
        )
        .await?
        {
            let revision = load_revision_row(
                &mut transaction,
                owner_user_id,
                schedule_id.into_uuid(),
                schedule_revision_id.into_uuid(),
            )
            .await?
            .ok_or(BackupScheduleError::InvalidPersistedData)?
            .try_into_domain()?;
            let existing = existing.try_into_domain(&revision)?;
            transaction.commit().await?;
            return Ok(BackupScheduleOccurrenceMaterializationResult::Existing(
                existing,
            ));
        }

        if backup_set_state != BackupSetState::Active {
            transaction.commit().await?;
            return Ok(BackupScheduleOccurrenceMaterializationResult::NotEffective(
                BackupScheduleOccurrenceNotEffectiveReason::BackupSetInactive,
            ));
        }
        if !schedule.enabled {
            transaction.commit().await?;
            return Ok(BackupScheduleOccurrenceMaterializationResult::NotEffective(
                BackupScheduleOccurrenceNotEffectiveReason::ScheduleDisabled,
            ));
        }
        if schedule.current_revision_id != schedule_revision_id.into_uuid() {
            transaction.commit().await?;
            return Ok(BackupScheduleOccurrenceMaterializationResult::NotEffective(
                BackupScheduleOccurrenceNotEffectiveReason::RevisionSuperseded,
            ));
        }

        let revision = load_revision_row(
            &mut transaction,
            owner_user_id,
            schedule_id.into_uuid(),
            schedule_revision_id.into_uuid(),
        )
        .await?
        .ok_or(BackupScheduleError::InvalidPersistedData)?
        .try_into_domain()?;
        let Some(planned) = revision.occurrence_on_local_date(local_calendar_date) else {
            transaction.commit().await?;
            return Ok(BackupScheduleOccurrenceMaterializationResult::InvalidTarget);
        };

        if planned.scheduled_for_utc() > observed_at_utc {
            transaction.commit().await?;
            return Ok(BackupScheduleOccurrenceMaterializationResult::NotDue);
        }
        if planned.scheduled_for_utc() <= Timestamp::from_offset_datetime(schedule.effective_from) {
            transaction.commit().await?;
            return Ok(BackupScheduleOccurrenceMaterializationResult::NotEffective(
                BackupScheduleOccurrenceNotEffectiveReason::BeforeEffectiveFrom,
            ));
        }

        if schedule_instant_exists(
            &mut transaction,
            owner_user_id,
            schedule_id,
            planned.scheduled_for_utc(),
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(BackupScheduleOccurrenceMaterializationResult::NotEffective(
                BackupScheduleOccurrenceNotEffectiveReason::ScheduleInstantAlreadyMaterialized,
            ));
        }

        let occurrence_id = BackupScheduleOccurrenceId::new();
        let inserted = sqlx::query_as::<_, BackupScheduleOccurrenceRow>(
            "INSERT INTO backup_schedule_occurrences
                (id, owner_user_id, backup_set_id, schedule_id,
                 schedule_revision_id, local_calendar_date,
                 resolved_local_time_minute, scheduled_for_utc, materialized_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, clock_timestamp())
             ON CONFLICT DO NOTHING
             RETURNING id, owner_user_id, backup_set_id, schedule_id,
                       schedule_revision_id, local_calendar_date,
                       resolved_local_time_minute, scheduled_for_utc,
                       materialized_at",
        )
        .bind(occurrence_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .bind(backup_set_id.into_uuid())
        .bind(schedule_id.into_uuid())
        .bind(schedule_revision_id.into_uuid())
        .bind(local_calendar_date)
        .bind(
            i16::try_from(planned.local_wall_time().minute())
                .map_err(|_| BackupScheduleError::InvalidPersistedData)?,
        )
        .bind(planned.scheduled_for_utc().as_offset_datetime())
        .fetch_optional(&mut *transaction)
        .await?;

        if let Some(inserted) = inserted {
            let occurrence = inserted.try_into_domain(&revision)?;
            transaction.commit().await?;
            return Ok(BackupScheduleOccurrenceMaterializationResult::Created(
                occurrence,
            ));
        }

        // A direct writer could still race the service's canonical lock. Hide
        // both uniqueness fences behind bounded domain outcomes rather than
        // exposing a raw PostgreSQL unique-violation string.
        if let Some(existing) = load_occurrence_by_logical_key(
            &mut transaction,
            owner_user_id,
            schedule_id,
            schedule_revision_id,
            local_calendar_date,
        )
        .await?
        {
            let occurrence = existing.try_into_domain(&revision)?;
            transaction.commit().await?;
            return Ok(BackupScheduleOccurrenceMaterializationResult::Existing(
                occurrence,
            ));
        }
        if schedule_instant_exists(
            &mut transaction,
            owner_user_id,
            schedule_id,
            planned.scheduled_for_utc(),
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(BackupScheduleOccurrenceMaterializationResult::NotEffective(
                BackupScheduleOccurrenceNotEffectiveReason::ScheduleInstantAlreadyMaterialized,
            ));
        }
        Err(BackupScheduleError::InvalidPersistedData)
    }

    /// Read one occurrence by opaque identity with cross-owner concealment.
    pub async fn get_backup_schedule_occurrence(
        &self,
        owner_user_id: UserId,
        occurrence_id: BackupScheduleOccurrenceId,
    ) -> Result<BackupScheduleOccurrence, BackupScheduleError> {
        let mut transaction = self.pool().sqlx_pool().begin().await?;
        let occurrence =
            load_occurrence_in_transaction(&mut transaction, owner_user_id, occurrence_id, false)
                .await?
                .ok_or(BackupScheduleError::NotFound)?;
        transaction.commit().await?;
        Ok(occurrence)
    }

    /// Owner-scoped canonical replay lookup by the logical occurrence key.
    pub async fn get_backup_schedule_occurrence_by_logical_key(
        &self,
        owner_user_id: UserId,
        schedule_revision_id: BackupScheduleRevisionId,
        local_calendar_date: Date,
    ) -> Result<Option<BackupScheduleOccurrence>, BackupScheduleError> {
        let mut transaction = self.pool().sqlx_pool().begin().await?;
        let row = sqlx::query_as::<_, BackupScheduleOccurrenceRow>(
            "SELECT id, owner_user_id, backup_set_id, schedule_id,
                    schedule_revision_id, local_calendar_date,
                    resolved_local_time_minute, scheduled_for_utc,
                    materialized_at
             FROM backup_schedule_occurrences
             WHERE owner_user_id = $1 AND schedule_revision_id = $2
               AND local_calendar_date = $3",
        )
        .bind(owner_user_id.into_uuid())
        .bind(schedule_revision_id.into_uuid())
        .bind(local_calendar_date)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(None);
        };
        let revision = load_revision_row(
            &mut transaction,
            owner_user_id,
            row.schedule_id,
            row.schedule_revision_id,
        )
        .await?
        .ok_or(BackupScheduleError::InvalidPersistedData)?
        .try_into_domain()?;
        let occurrence = row.try_into_domain(&revision)?;
        transaction.commit().await?;
        Ok(Some(occurrence))
    }
}

async fn load_occurrence_by_logical_key(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    schedule_id: BackupScheduleId,
    schedule_revision_id: BackupScheduleRevisionId,
    local_calendar_date: Date,
) -> Result<Option<BackupScheduleOccurrenceRow>, BackupScheduleError> {
    Ok(sqlx::query_as::<_, BackupScheduleOccurrenceRow>(
        "SELECT id, owner_user_id, backup_set_id, schedule_id,
                schedule_revision_id, local_calendar_date,
                resolved_local_time_minute, scheduled_for_utc,
                materialized_at
         FROM backup_schedule_occurrences
         WHERE owner_user_id = $1 AND schedule_id = $2
           AND schedule_revision_id = $3 AND local_calendar_date = $4",
    )
    .bind(owner_user_id.into_uuid())
    .bind(schedule_id.into_uuid())
    .bind(schedule_revision_id.into_uuid())
    .bind(local_calendar_date)
    .fetch_optional(&mut **transaction)
    .await?)
}

/// Load one owner-scoped occurrence while keeping its historical revision
/// validation at the adapter boundary. A handoff uses the locking form only
/// after acquiring the owning BackupSet and schedule locks, preserving the
/// canonical `BackupSet -> BackupSchedule -> Occurrence` order.
pub(crate) async fn load_occurrence_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    occurrence_id: BackupScheduleOccurrenceId,
    lock: bool,
) -> Result<Option<BackupScheduleOccurrence>, BackupScheduleError> {
    let query = if lock {
        "SELECT id, owner_user_id, backup_set_id, schedule_id,
                schedule_revision_id, local_calendar_date,
                resolved_local_time_minute, scheduled_for_utc,
                materialized_at
         FROM backup_schedule_occurrences
         WHERE id = $1 AND owner_user_id = $2
         FOR UPDATE"
    } else {
        "SELECT id, owner_user_id, backup_set_id, schedule_id,
                schedule_revision_id, local_calendar_date,
                resolved_local_time_minute, scheduled_for_utc,
                materialized_at
         FROM backup_schedule_occurrences
         WHERE id = $1 AND owner_user_id = $2"
    };
    let Some(row) = sqlx::query_as::<_, BackupScheduleOccurrenceRow>(query)
        .bind(occurrence_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut **transaction)
        .await?
    else {
        return Ok(None);
    };
    let revision = load_revision_row(
        transaction,
        owner_user_id,
        row.schedule_id,
        row.schedule_revision_id,
    )
    .await?
    .ok_or(BackupScheduleError::InvalidPersistedData)?
    .try_into_domain()?;
    row.try_into_domain(&revision).map(Some)
}

async fn schedule_instant_exists(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    schedule_id: BackupScheduleId,
    scheduled_for_utc: Timestamp,
) -> Result<bool, BackupScheduleError> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(
            SELECT 1
            FROM backup_schedule_occurrences
            WHERE owner_user_id = $1 AND schedule_id = $2
              AND scheduled_for_utc = $3
        )",
    )
    .bind(owner_user_id.into_uuid())
    .bind(schedule_id.into_uuid())
    .bind(scheduled_for_utc.as_offset_datetime())
    .fetch_one(&mut **transaction)
    .await?)
}
