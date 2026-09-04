//! Durable backup-scheduling metadata service.
//!
//! This adapter persists only logical schedule state and immutable revision
//! provenance. It never starts a worker, creates a snapshot, appends a
//! journal event, touches an ObjectStore, or creates an execution row. The
//! pure recurrence calculation remains in `synveil-core`.

use std::{fmt, str::FromStr};

use async_trait::async_trait;
use sqlx::{FromRow, Postgres, Transaction};
use synveil_core::{
    BACKUP_SCHEDULE_FINGERPRINT_VERSION, BACKUP_SCHEDULE_LEGACY_FINGERPRINT_VERSION,
    BackupSchedule, BackupScheduleConfig, BackupScheduleId, BackupScheduleIdempotencyFingerprint,
    BackupScheduleLocalTime, BackupScheduleMisfireMode, BackupScheduleRecurrenceKind,
    BackupScheduleRequest, BackupScheduleRevision, BackupScheduleRevisionId,
    BackupScheduleRevisionNumber, BackupScheduleTimezone, BackupScheduleWeekday, BackupSetId,
    BackupSetState, DomainError, PlannedScheduleOccurrence, Timestamp, UserId,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{DatabaseError, DatabasePool};

pub const MIN_BACKUP_SCHEDULE_OPERATION_KEY_BYTES: usize = 8;
pub const MAX_BACKUP_SCHEDULE_OPERATION_KEY_BYTES: usize = 256;

/// Safe application failures for the owner-scoped scheduling domain.
///
/// Unknown and cross-owner identities are deliberately concealed as
/// `NotFound`. A valid but disabled schedule is represented by `None` from the
/// effective-occurrence query, not by an error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackupScheduleError {
    NotFound,
    ScheduleNotConfigured,
    InvalidTimezone,
    InvalidLocalTime,
    InvalidWeeklyDays,
    InvalidMisfireMode,
    InvalidMaxLateness,
    InvalidOperationId,
    SemanticIdempotencyConflict,
    Database(DatabaseError),
    InvalidPersistedData,
}

impl fmt::Display for BackupScheduleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotFound => "backup schedule resource was not found",
            Self::ScheduleNotConfigured => "backup schedule is not configured",
            Self::InvalidTimezone => "backup schedule timezone is invalid",
            Self::InvalidLocalTime => "backup schedule local time is invalid",
            Self::InvalidWeeklyDays => "backup schedule weekly days are invalid",
            Self::InvalidMisfireMode => "backup schedule misfire mode is invalid",
            Self::InvalidMaxLateness => "backup schedule maximum lateness is invalid",
            Self::InvalidOperationId => "backup schedule operation identity is invalid",
            Self::SemanticIdempotencyConflict => {
                "backup schedule operation identity conflicts with the request"
            }
            Self::Database(error) => return error.fmt(formatter),
            Self::InvalidPersistedData => "backup schedule persisted data is invalid",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for BackupScheduleError {}

impl From<sqlx::Error> for BackupScheduleError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(DatabaseError::from(error))
    }
}

/// Metadata port for the scheduling foundation. It deliberately exposes no
/// execution, worker, HTTP, ObjectStore, journal, or sync operation.
#[async_trait]
pub trait BackupSchedulingBackend: Send + Sync {
    async fn configure_backup_schedule(
        &self,
        owner_user_id: UserId,
        operation_id: String,
        backup_set_id: BackupSetId,
        config: BackupScheduleConfig,
    ) -> Result<BackupScheduleRevision, BackupScheduleError>;

    async fn set_backup_schedule_enabled(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        enabled: bool,
    ) -> Result<BackupSchedule, BackupScheduleError>;

    async fn get_backup_schedule(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
    ) -> Result<BackupSchedule, BackupScheduleError>;

    async fn get_effective_next_backup_occurrence(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        exclusive_utc_instant: Timestamp,
    ) -> Result<Option<PlannedScheduleOccurrence>, BackupScheduleError>;
}

/// PostgreSQL-backed owner-scoped scheduling service.
#[derive(Clone)]
pub struct BackupScheduleService {
    pool: DatabasePool,
}

impl BackupScheduleService {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    #[must_use]
    pub const fn pool(&self) -> &DatabasePool {
        &self.pool
    }

    /// Configure the one logical schedule belonging to a backup set. The
    /// operation is durable and replay-safe: a new semantic revision is
    /// appended only when the normalized configuration differs from current.
    /// A no-op operation is recorded in the small idempotency relation so a
    /// later retry cannot silently change meaning under the same key.
    pub async fn configure_backup_schedule(
        &self,
        owner_user_id: UserId,
        operation_id: String,
        backup_set_id: BackupSetId,
        config: BackupScheduleConfig,
    ) -> Result<BackupScheduleRevision, BackupScheduleError> {
        validate_operation_id(&operation_id)?;
        let request = BackupScheduleRequest::new(backup_set_id, config.clone());
        let fingerprint = request.fingerprint();
        let mut transaction = self.pool.sqlx_pool().begin().await?;

        // The existing BackupSet row is the serialization fence for every
        // schedule mutation. This makes first creation and revision-number
        // allocation linearizable per set without a new scheduler lock.
        lock_owned_backup_set(&mut transaction, owner_user_id, backup_set_id).await?;

        if let Some(operation) =
            load_schedule_operation(&mut transaction, owner_user_id, &operation_id).await?
        {
            let revision = load_operation_result(
                &mut transaction,
                operation,
                owner_user_id,
                backup_set_id,
                &request,
            )
            .await?;
            transaction.commit().await?;
            return Ok(revision);
        }

        let existing_schedule =
            load_schedule_row_for_update(&mut transaction, owner_user_id, backup_set_id).await?;

        let revision = if let Some(schedule) = existing_schedule {
            let current = load_revision_row(
                &mut transaction,
                owner_user_id,
                schedule.id,
                schedule.current_revision_id,
            )
            .await?
            .ok_or(BackupScheduleError::InvalidPersistedData)?;
            let current = current.try_into_domain()?;

            if current.config() == &config {
                insert_schedule_operation(
                    &mut transaction,
                    owner_user_id,
                    &operation_id,
                    backup_set_id,
                    schedule.id,
                    current.id().into_uuid(),
                    fingerprint,
                )
                .await?;
                current
            } else {
                let revision_number = current
                    .revision_number()
                    .checked_next()
                    .map_err(|_| BackupScheduleError::InvalidPersistedData)?;
                let revision_id = BackupScheduleRevisionId::new();
                let row = insert_schedule_revision(
                    &mut transaction,
                    owner_user_id,
                    backup_set_id,
                    BackupScheduleId::try_from_uuid(schedule.id)
                        .map_err(|_| BackupScheduleError::InvalidPersistedData)?,
                    revision_id,
                    revision_number,
                    &operation_id,
                    fingerprint,
                    &config,
                )
                .await?;
                update_current_revision(
                    &mut transaction,
                    owner_user_id,
                    BackupScheduleId::try_from_uuid(schedule.id)
                        .map_err(|_| BackupScheduleError::InvalidPersistedData)?,
                    revision_id,
                )
                .await?;
                insert_schedule_operation(
                    &mut transaction,
                    owner_user_id,
                    &operation_id,
                    backup_set_id,
                    schedule.id,
                    revision_id.into_uuid(),
                    fingerprint,
                )
                .await?;
                row.try_into_domain()?
            }
        } else {
            let schedule_id = BackupScheduleId::new();
            let revision_id = BackupScheduleRevisionId::new();
            let revision_number = BackupScheduleRevisionNumber::new(1)
                .map_err(|_| BackupScheduleError::InvalidPersistedData)?;

            // The current-revision FK is deferred so the stable schedule row
            // can be inserted with its preallocated revision identity, then
            // the revision and idempotency evidence can be inserted in this
            // same transaction before commit.
            sqlx::query(
                "WITH observed AS (SELECT clock_timestamp() AS value)
                 INSERT INTO backup_schedules
                    (id, owner_user_id, backup_set_id, current_revision_id,
                     enabled, effective_from, created_at, updated_at)
                 SELECT $1, $2, $3, $4, TRUE, value, value, value
                 FROM observed",
            )
            .bind(schedule_id.into_uuid())
            .bind(owner_user_id.into_uuid())
            .bind(backup_set_id.into_uuid())
            .bind(revision_id.into_uuid())
            .execute(&mut *transaction)
            .await?;

            let row = insert_schedule_revision(
                &mut transaction,
                owner_user_id,
                backup_set_id,
                schedule_id,
                revision_id,
                revision_number,
                &operation_id,
                fingerprint,
                &config,
            )
            .await?;
            insert_schedule_operation(
                &mut transaction,
                owner_user_id,
                &operation_id,
                backup_set_id,
                schedule_id.into_uuid(),
                revision_id.into_uuid(),
                fingerprint,
            )
            .await?;
            row.try_into_domain()?
        };

        transaction.commit().await?;
        Ok(revision)
    }

    /// Convenience boundary for callers that still have wire-shaped values.
    /// Validation happens before any transaction starts.
    #[allow(clippy::too_many_arguments)]
    pub async fn configure_backup_schedule_from_values(
        &self,
        owner_user_id: UserId,
        operation_id: String,
        backup_set_id: BackupSetId,
        recurrence_kind: BackupScheduleRecurrenceKind,
        timezone: &str,
        local_time: &str,
        weekly_days: Vec<BackupScheduleWeekday>,
    ) -> Result<BackupScheduleRevision, BackupScheduleError> {
        let timezone = timezone
            .parse::<BackupScheduleTimezone>()
            .map_err(|_| BackupScheduleError::InvalidTimezone)?;
        let local_time = local_time
            .parse::<BackupScheduleLocalTime>()
            .map_err(|_| BackupScheduleError::InvalidLocalTime)?;
        let config = BackupScheduleConfig::new(recurrence_kind, timezone, local_time, weekly_days)
            .map_err(map_domain_error)?;
        self.configure_backup_schedule(owner_user_id, operation_id, backup_set_id, config)
            .await
    }

    /// Policy-aware value boundary for backend callers introduced by Prompt
    /// 65. The older convenience method remains the safe-default spelling.
    #[allow(clippy::too_many_arguments)]
    pub async fn configure_backup_schedule_with_misfire_policy_from_values(
        &self,
        owner_user_id: UserId,
        operation_id: String,
        backup_set_id: BackupSetId,
        recurrence_kind: BackupScheduleRecurrenceKind,
        timezone: &str,
        local_time: &str,
        weekly_days: Vec<BackupScheduleWeekday>,
        misfire_mode: &str,
        max_lateness_seconds: u32,
    ) -> Result<BackupScheduleRevision, BackupScheduleError> {
        let timezone = timezone
            .parse::<BackupScheduleTimezone>()
            .map_err(|_| BackupScheduleError::InvalidTimezone)?;
        let local_time = local_time
            .parse::<BackupScheduleLocalTime>()
            .map_err(|_| BackupScheduleError::InvalidLocalTime)?;
        let misfire_mode = misfire_mode
            .parse::<BackupScheduleMisfireMode>()
            .map_err(|_| BackupScheduleError::InvalidMisfireMode)?;
        let config = BackupScheduleConfig::new_with_misfire_policy(
            recurrence_kind,
            timezone,
            local_time,
            weekly_days,
            misfire_mode,
            max_lateness_seconds,
        )
        .map_err(map_domain_error)?;
        self.configure_backup_schedule(owner_user_id, operation_id, backup_set_id, config)
            .await
    }

    /// Enable or disable a configured logical schedule without creating a new
    /// timing revision. The schedule and its history remain durable either
    /// way. An existing disabled BackupSet remains ineffective regardless of
    /// this flag.
    pub async fn set_backup_schedule_enabled(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        enabled: bool,
    ) -> Result<BackupSchedule, BackupScheduleError> {
        let mut transaction = self.pool.sqlx_pool().begin().await?;
        lock_owned_backup_set(&mut transaction, owner_user_id, backup_set_id).await?;
        let schedule = load_schedule_row_for_update(&mut transaction, owner_user_id, backup_set_id)
            .await?
            .ok_or(BackupScheduleError::ScheduleNotConfigured)?;

        if schedule.enabled != enabled {
            sqlx::query(
                "WITH observed AS (SELECT clock_timestamp() AS value)
                 UPDATE backup_schedules
                 SET enabled = $1, effective_from = observed.value,
                     updated_at = observed.value
                 FROM observed
                 WHERE id = $2 AND owner_user_id = $3",
            )
            .bind(enabled)
            .bind(schedule.id)
            .bind(owner_user_id.into_uuid())
            .execute(&mut *transaction)
            .await?;
        }

        let current =
            load_current_schedule_in_transaction(&mut transaction, owner_user_id, backup_set_id)
                .await?
                .ok_or(BackupScheduleError::InvalidPersistedData)?;
        transaction.commit().await?;
        Ok(current)
    }

    /// Same state transition addressed by stable schedule identity. The
    /// service resolves the owning BackupSet before taking the mutation fence.
    pub async fn set_backup_schedule_enabled_by_id(
        &self,
        owner_user_id: UserId,
        schedule_id: BackupScheduleId,
        enabled: bool,
    ) -> Result<BackupSchedule, BackupScheduleError> {
        let schedule = self
            .get_backup_schedule_by_id(owner_user_id, schedule_id)
            .await?;
        self.set_backup_schedule_enabled(owner_user_id, schedule.backup_set_id(), enabled)
            .await
    }

    /// Read the current revision for an owned BackupSet. An owned set with no
    /// configured schedule is distinct from a missing/cross-owner set.
    pub async fn get_backup_schedule(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
    ) -> Result<BackupSchedule, BackupScheduleError> {
        if !owned_backup_set_exists(&self.pool, owner_user_id, backup_set_id).await? {
            return Err(BackupScheduleError::NotFound);
        }
        self.get_backup_schedule_optional(owner_user_id, backup_set_id)
            .await?
            .ok_or(BackupScheduleError::ScheduleNotConfigured)
    }

    /// Read by logical schedule identity while preserving owner concealment.
    pub async fn get_backup_schedule_by_id(
        &self,
        owner_user_id: UserId,
        schedule_id: BackupScheduleId,
    ) -> Result<BackupSchedule, BackupScheduleError> {
        load_current_schedule_by_id(&self.pool, owner_user_id, schedule_id)
            .await?
            .ok_or(BackupScheduleError::NotFound)
    }

    /// Read one historical revision by its logical immutable identity.
    pub async fn get_backup_schedule_revision(
        &self,
        owner_user_id: UserId,
        revision_id: BackupScheduleRevisionId,
    ) -> Result<BackupScheduleRevision, BackupScheduleError> {
        let row = sqlx::query_as::<_, BackupScheduleRevisionRow>(
            "SELECT id, schedule_id, owner_user_id, backup_set_id,
                    revision_number, operation_id, fingerprint_version,
                    request_fingerprint, recurrence_kind, timezone,
                    local_time_minute, weekly_days, misfire_mode,
                    max_lateness_seconds, created_at
             FROM backup_schedule_revisions
             WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(revision_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await?;
        row.ok_or(BackupScheduleError::NotFound)?.try_into_domain()
    }

    /// Compute the next occurrence only when the BackupSet is ACTIVE and its
    /// logical schedule is enabled. The query reads the set and current
    /// revision in one PostgreSQL statement, so the effective-state decision
    /// and revision snapshot share one database read boundary.
    pub async fn get_effective_next_backup_occurrence(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        exclusive_utc_instant: Timestamp,
    ) -> Result<Option<PlannedScheduleOccurrence>, BackupScheduleError> {
        let row = sqlx::query_as::<_, BackupScheduleEffectiveRow>(
            "SELECT backup_set.state AS backup_set_state,
                    schedule.id AS schedule_id,
                    schedule.owner_user_id AS schedule_owner_user_id,
                    schedule.backup_set_id AS schedule_backup_set_id,
                    schedule.current_revision_id,
                    schedule.enabled,
                    schedule.effective_from AS schedule_effective_from,
                    schedule.created_at AS schedule_created_at,
                    schedule.updated_at AS schedule_updated_at,
                    revision.id AS revision_id,
                    revision.schedule_id AS revision_schedule_id,
                    revision.owner_user_id AS revision_owner_user_id,
                    revision.backup_set_id AS revision_backup_set_id,
                    revision.revision_number,
                    revision.operation_id,
                    revision.fingerprint_version,
                    revision.request_fingerprint,
                    revision.recurrence_kind,
                    revision.timezone,
                    revision.local_time_minute,
                    revision.weekly_days,
                    revision.misfire_mode,
                    revision.max_lateness_seconds,
                    revision.created_at AS revision_created_at
             FROM backup_sets AS backup_set
             LEFT JOIN backup_schedules AS schedule
               ON schedule.backup_set_id = backup_set.id
              AND schedule.owner_user_id = backup_set.owner_user_id
             LEFT JOIN backup_schedule_revisions AS revision
               ON revision.id = schedule.current_revision_id
              AND revision.schedule_id = schedule.id
              AND revision.owner_user_id = schedule.owner_user_id
             WHERE backup_set.id = $1 AND backup_set.owner_user_id = $2",
        )
        .bind(backup_set_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await?
        .ok_or(BackupScheduleError::NotFound)?;

        let backup_set_state = BackupSetState::from_str(&row.backup_set_state)
            .map_err(|_| BackupScheduleError::InvalidPersistedData)?;
        let Some(schedule_id) = row.schedule_id else {
            return Ok(None);
        };
        let current = row.try_into_current_schedule(schedule_id)?;
        if backup_set_state != BackupSetState::Active || !current.enabled() {
            return Ok(None);
        }
        let effective_reference = exclusive_utc_instant.max(current.effective_from());
        Ok(current
            .current_revision()
            .next_occurrence_after(effective_reference))
    }

    async fn get_backup_schedule_optional(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
    ) -> Result<Option<BackupSchedule>, BackupScheduleError> {
        load_current_schedule(&self.pool, owner_user_id, backup_set_id).await
    }
}

#[async_trait]
impl BackupSchedulingBackend for BackupScheduleService {
    async fn configure_backup_schedule(
        &self,
        owner_user_id: UserId,
        operation_id: String,
        backup_set_id: BackupSetId,
        config: BackupScheduleConfig,
    ) -> Result<BackupScheduleRevision, BackupScheduleError> {
        Self::configure_backup_schedule(self, owner_user_id, operation_id, backup_set_id, config)
            .await
    }

    async fn set_backup_schedule_enabled(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        enabled: bool,
    ) -> Result<BackupSchedule, BackupScheduleError> {
        Self::set_backup_schedule_enabled(self, owner_user_id, backup_set_id, enabled).await
    }

    async fn get_backup_schedule(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
    ) -> Result<BackupSchedule, BackupScheduleError> {
        Self::get_backup_schedule(self, owner_user_id, backup_set_id).await
    }

    async fn get_effective_next_backup_occurrence(
        &self,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        exclusive_utc_instant: Timestamp,
    ) -> Result<Option<PlannedScheduleOccurrence>, BackupScheduleError> {
        Self::get_effective_next_backup_occurrence(
            self,
            owner_user_id,
            backup_set_id,
            exclusive_utc_instant,
        )
        .await
    }
}

#[derive(Clone, Debug, FromRow, PartialEq)]
struct BackupScheduleRow {
    id: Uuid,
    owner_user_id: Uuid,
    backup_set_id: Uuid,
    current_revision_id: Uuid,
    enabled: bool,
    effective_from: OffsetDateTime,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, FromRow, PartialEq)]
pub(crate) struct BackupScheduleRevisionRow {
    id: Uuid,
    schedule_id: Uuid,
    owner_user_id: Uuid,
    backup_set_id: Uuid,
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
}

impl BackupScheduleRevisionRow {
    pub(crate) fn try_into_domain(self) -> Result<BackupScheduleRevision, BackupScheduleError> {
        let id = BackupScheduleRevisionId::try_from_uuid(self.id)
            .map_err(|_| BackupScheduleError::InvalidPersistedData)?;
        let schedule_id = BackupScheduleId::try_from_uuid(self.schedule_id)
            .map_err(|_| BackupScheduleError::InvalidPersistedData)?;
        let owner_user_id = UserId::try_from_uuid(self.owner_user_id)
            .map_err(|_| BackupScheduleError::InvalidPersistedData)?;
        let backup_set_id = BackupSetId::try_from_uuid(self.backup_set_id)
            .map_err(|_| BackupScheduleError::InvalidPersistedData)?;
        let revision_number = BackupScheduleRevisionNumber::new(
            u64::try_from(self.revision_number)
                .map_err(|_| BackupScheduleError::InvalidPersistedData)?,
        )
        .map_err(|_| BackupScheduleError::InvalidPersistedData)?;
        let fingerprint = decode_fingerprint(self.fingerprint_version, &self.request_fingerprint)?;
        let recurrence_kind = BackupScheduleRecurrenceKind::from_str(&self.recurrence_kind)
            .map_err(|_| BackupScheduleError::InvalidPersistedData)?;
        let timezone = BackupScheduleTimezone::from_str(&self.timezone)
            .map_err(|_| BackupScheduleError::InvalidPersistedData)?;
        let local_time = BackupScheduleLocalTime::from_minute(
            u16::try_from(self.local_time_minute)
                .map_err(|_| BackupScheduleError::InvalidPersistedData)?,
        )
        .map_err(|_| BackupScheduleError::InvalidPersistedData)?;
        let weekly_days = self
            .weekly_days
            .into_iter()
            .map(|day| {
                BackupScheduleWeekday::from_ordinal(
                    u8::try_from(day).map_err(|_| BackupScheduleError::InvalidPersistedData)?,
                )
                .ok_or(BackupScheduleError::InvalidPersistedData)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let misfire_mode = BackupScheduleMisfireMode::from_str(&self.misfire_mode)
            .map_err(|_| BackupScheduleError::InvalidPersistedData)?;
        let max_lateness_seconds = u32::try_from(self.max_lateness_seconds)
            .map_err(|_| BackupScheduleError::InvalidPersistedData)?;
        let config = BackupScheduleConfig::new_with_misfire_policy(
            recurrence_kind,
            timezone,
            local_time,
            weekly_days,
            misfire_mode,
            max_lateness_seconds,
        )
        .map_err(|_| BackupScheduleError::InvalidPersistedData)?;
        BackupScheduleRevision::new(
            id,
            schedule_id,
            owner_user_id,
            backup_set_id,
            revision_number,
            self.operation_id,
            fingerprint,
            config,
            Timestamp::from_offset_datetime(self.created_at),
        )
        .map_err(|_| BackupScheduleError::InvalidPersistedData)
    }
}

#[derive(Clone, Debug, FromRow, PartialEq)]
struct BackupScheduleOperationRow {
    owner_user_id: Uuid,
    operation_id: String,
    backup_set_id: Uuid,
    schedule_id: Uuid,
    result_revision_id: Uuid,
    fingerprint_version: i16,
    request_fingerprint: Vec<u8>,
}

#[derive(Clone, Debug, FromRow, PartialEq)]
struct BackupScheduleCurrentRow {
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
}

impl BackupScheduleCurrentRow {
    fn try_into_domain(self) -> Result<BackupSchedule, BackupScheduleError> {
        let revision = self.revision_row().try_into_domain()?;
        if self.current_revision_id != self.revision_id
            || revision.schedule_id().into_uuid() != self.schedule_id
        {
            return Err(BackupScheduleError::InvalidPersistedData);
        }
        let schedule_id = BackupScheduleId::try_from_uuid(self.schedule_id)
            .map_err(|_| BackupScheduleError::InvalidPersistedData)?;
        let owner_user_id = UserId::try_from_uuid(self.schedule_owner_user_id)
            .map_err(|_| BackupScheduleError::InvalidPersistedData)?;
        let backup_set_id = BackupSetId::try_from_uuid(self.schedule_backup_set_id)
            .map_err(|_| BackupScheduleError::InvalidPersistedData)?;
        BackupSchedule::new(
            schedule_id,
            owner_user_id,
            backup_set_id,
            revision,
            self.enabled,
            Timestamp::from_offset_datetime(self.schedule_effective_from),
            Timestamp::from_offset_datetime(self.schedule_created_at),
            Timestamp::from_offset_datetime(self.schedule_updated_at),
        )
        .map_err(|_| BackupScheduleError::InvalidPersistedData)
    }

    fn revision_row(&self) -> BackupScheduleRevisionRow {
        BackupScheduleRevisionRow {
            id: self.revision_id,
            schedule_id: self.revision_schedule_id,
            owner_user_id: self.revision_owner_user_id,
            backup_set_id: self.revision_backup_set_id,
            revision_number: self.revision_number,
            operation_id: self.operation_id.clone(),
            fingerprint_version: self.fingerprint_version,
            request_fingerprint: self.request_fingerprint.clone(),
            recurrence_kind: self.recurrence_kind.clone(),
            timezone: self.timezone.clone(),
            local_time_minute: self.local_time_minute,
            weekly_days: self.weekly_days.clone(),
            misfire_mode: self.misfire_mode.clone(),
            max_lateness_seconds: self.max_lateness_seconds,
            created_at: self.revision_created_at,
        }
    }
}

#[derive(Clone, Debug, FromRow, PartialEq)]
struct BackupScheduleEffectiveRow {
    backup_set_state: String,
    schedule_id: Option<Uuid>,
    schedule_owner_user_id: Option<Uuid>,
    schedule_backup_set_id: Option<Uuid>,
    current_revision_id: Option<Uuid>,
    enabled: Option<bool>,
    schedule_effective_from: Option<OffsetDateTime>,
    schedule_created_at: Option<OffsetDateTime>,
    schedule_updated_at: Option<OffsetDateTime>,
    revision_id: Option<Uuid>,
    revision_schedule_id: Option<Uuid>,
    revision_owner_user_id: Option<Uuid>,
    revision_backup_set_id: Option<Uuid>,
    revision_number: Option<i64>,
    operation_id: Option<String>,
    fingerprint_version: Option<i16>,
    request_fingerprint: Option<Vec<u8>>,
    recurrence_kind: Option<String>,
    timezone: Option<String>,
    local_time_minute: Option<i16>,
    weekly_days: Option<Vec<i16>>,
    misfire_mode: Option<String>,
    max_lateness_seconds: Option<i32>,
    revision_created_at: Option<OffsetDateTime>,
}

impl BackupScheduleEffectiveRow {
    fn try_into_current_schedule(
        self,
        schedule_id: Uuid,
    ) -> Result<BackupSchedule, BackupScheduleError> {
        let current = BackupScheduleCurrentRow {
            schedule_id,
            schedule_owner_user_id: self
                .schedule_owner_user_id
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            schedule_backup_set_id: self
                .schedule_backup_set_id
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            current_revision_id: self
                .current_revision_id
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            enabled: self
                .enabled
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            schedule_effective_from: self
                .schedule_effective_from
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            schedule_created_at: self
                .schedule_created_at
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            schedule_updated_at: self
                .schedule_updated_at
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            revision_id: self
                .revision_id
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            revision_schedule_id: self
                .revision_schedule_id
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            revision_owner_user_id: self
                .revision_owner_user_id
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            revision_backup_set_id: self
                .revision_backup_set_id
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            revision_number: self
                .revision_number
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            operation_id: self
                .operation_id
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            fingerprint_version: self
                .fingerprint_version
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            request_fingerprint: self
                .request_fingerprint
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            recurrence_kind: self
                .recurrence_kind
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            timezone: self
                .timezone
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            local_time_minute: self
                .local_time_minute
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            weekly_days: self
                .weekly_days
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            misfire_mode: self
                .misfire_mode
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            max_lateness_seconds: self
                .max_lateness_seconds
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
            revision_created_at: self
                .revision_created_at
                .ok_or(BackupScheduleError::InvalidPersistedData)?,
        };
        current.try_into_domain()
    }
}

async fn owned_backup_set_exists(
    pool: &DatabasePool,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
) -> Result<bool, BackupScheduleError> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(
            SELECT 1 FROM backup_sets
            WHERE id = $1 AND owner_user_id = $2
        )",
    )
    .bind(backup_set_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_one(pool.sqlx_pool())
    .await?)
}

pub(crate) async fn lock_owned_backup_set(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
) -> Result<(), BackupScheduleError> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id
         FROM backup_sets
         WHERE id = $1 AND owner_user_id = $2
         FOR UPDATE",
    )
    .bind(backup_set_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(BackupScheduleError::NotFound)?;
    Ok(())
}

async fn load_schedule_row_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
) -> Result<Option<BackupScheduleRow>, BackupScheduleError> {
    Ok(sqlx::query_as::<_, BackupScheduleRow>(
        "SELECT id, owner_user_id, backup_set_id, current_revision_id,
                enabled, effective_from, created_at, updated_at
         FROM backup_schedules
         WHERE owner_user_id = $1 AND backup_set_id = $2
         FOR UPDATE",
    )
    .bind(owner_user_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await?)
}

pub(crate) async fn load_revision_row(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    schedule_id: Uuid,
    revision_id: Uuid,
) -> Result<Option<BackupScheduleRevisionRow>, BackupScheduleError> {
    Ok(sqlx::query_as::<_, BackupScheduleRevisionRow>(
        "SELECT id, schedule_id, owner_user_id, backup_set_id,
                revision_number, operation_id, fingerprint_version,
                request_fingerprint, recurrence_kind, timezone,
                local_time_minute, weekly_days, misfire_mode,
                max_lateness_seconds, created_at
         FROM backup_schedule_revisions
         WHERE id = $1 AND schedule_id = $2 AND owner_user_id = $3
         FOR SHARE",
    )
    .bind(revision_id)
    .bind(schedule_id)
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await?)
}

async fn load_current_schedule_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
) -> Result<Option<BackupSchedule>, BackupScheduleError> {
    let Some(schedule) =
        load_schedule_row_for_update(transaction, owner_user_id, backup_set_id).await?
    else {
        return Ok(None);
    };
    let revision = load_revision_row(
        transaction,
        owner_user_id,
        schedule.id,
        schedule.current_revision_id,
    )
    .await?
    .ok_or(BackupScheduleError::InvalidPersistedData)?;
    Ok(Some(schedule.try_into_domain(revision)?))
}

impl BackupScheduleRow {
    fn try_into_domain(
        self,
        revision: BackupScheduleRevisionRow,
    ) -> Result<BackupSchedule, BackupScheduleError> {
        let revision = revision.try_into_domain()?;
        if self.current_revision_id != revision.id().into_uuid()
            || self.id != revision.schedule_id().into_uuid()
        {
            return Err(BackupScheduleError::InvalidPersistedData);
        }
        BackupSchedule::new(
            BackupScheduleId::try_from_uuid(self.id)
                .map_err(|_| BackupScheduleError::InvalidPersistedData)?,
            UserId::try_from_uuid(self.owner_user_id)
                .map_err(|_| BackupScheduleError::InvalidPersistedData)?,
            BackupSetId::try_from_uuid(self.backup_set_id)
                .map_err(|_| BackupScheduleError::InvalidPersistedData)?,
            revision,
            self.enabled,
            Timestamp::from_offset_datetime(self.effective_from),
            Timestamp::from_offset_datetime(self.created_at),
            Timestamp::from_offset_datetime(self.updated_at),
        )
        .map_err(|_| BackupScheduleError::InvalidPersistedData)
    }
}

async fn load_current_schedule(
    pool: &DatabasePool,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
) -> Result<Option<BackupSchedule>, BackupScheduleError> {
    let row = sqlx::query_as::<_, BackupScheduleCurrentRow>(
        "SELECT schedule.id AS schedule_id,
                schedule.owner_user_id AS schedule_owner_user_id,
                schedule.backup_set_id AS schedule_backup_set_id,
                schedule.current_revision_id,
                schedule.enabled,
                schedule.effective_from AS schedule_effective_from,
                schedule.created_at AS schedule_created_at,
                schedule.updated_at AS schedule_updated_at,
                revision.id AS revision_id,
                revision.schedule_id AS revision_schedule_id,
                revision.owner_user_id AS revision_owner_user_id,
                revision.backup_set_id AS revision_backup_set_id,
                revision.revision_number,
                revision.operation_id,
                revision.fingerprint_version,
                revision.request_fingerprint,
                revision.recurrence_kind,
                revision.timezone,
                revision.local_time_minute,
                revision.weekly_days,
                revision.misfire_mode,
                revision.max_lateness_seconds,
                revision.created_at AS revision_created_at
         FROM backup_schedules AS schedule
         INNER JOIN backup_schedule_revisions AS revision
           ON revision.id = schedule.current_revision_id
          AND revision.schedule_id = schedule.id
          AND revision.owner_user_id = schedule.owner_user_id
         WHERE schedule.owner_user_id = $1 AND schedule.backup_set_id = $2",
    )
    .bind(owner_user_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .fetch_optional(pool.sqlx_pool())
    .await?;
    row.map(BackupScheduleCurrentRow::try_into_domain)
        .transpose()
}

async fn load_current_schedule_by_id(
    pool: &DatabasePool,
    owner_user_id: UserId,
    schedule_id: BackupScheduleId,
) -> Result<Option<BackupSchedule>, BackupScheduleError> {
    let row = sqlx::query_as::<_, BackupScheduleCurrentRow>(
        "SELECT schedule.id AS schedule_id,
                schedule.owner_user_id AS schedule_owner_user_id,
                schedule.backup_set_id AS schedule_backup_set_id,
                schedule.current_revision_id,
                schedule.enabled,
                schedule.effective_from AS schedule_effective_from,
                schedule.created_at AS schedule_created_at,
                schedule.updated_at AS schedule_updated_at,
                revision.id AS revision_id,
                revision.schedule_id AS revision_schedule_id,
                revision.owner_user_id AS revision_owner_user_id,
                revision.backup_set_id AS revision_backup_set_id,
                revision.revision_number,
                revision.operation_id,
                revision.fingerprint_version,
                revision.request_fingerprint,
                revision.recurrence_kind,
                revision.timezone,
                revision.local_time_minute,
                revision.weekly_days,
                revision.misfire_mode,
                revision.max_lateness_seconds,
                revision.created_at AS revision_created_at
         FROM backup_schedules AS schedule
         INNER JOIN backup_schedule_revisions AS revision
           ON revision.id = schedule.current_revision_id
          AND revision.schedule_id = schedule.id
          AND revision.owner_user_id = schedule.owner_user_id
         WHERE schedule.owner_user_id = $1 AND schedule.id = $2",
    )
    .bind(owner_user_id.into_uuid())
    .bind(schedule_id.into_uuid())
    .fetch_optional(pool.sqlx_pool())
    .await?;
    row.map(BackupScheduleCurrentRow::try_into_domain)
        .transpose()
}

async fn load_schedule_operation(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    operation_id: &str,
) -> Result<Option<BackupScheduleOperationRow>, BackupScheduleError> {
    Ok(sqlx::query_as::<_, BackupScheduleOperationRow>(
        "SELECT owner_user_id, operation_id, backup_set_id, schedule_id,
                result_revision_id, fingerprint_version, request_fingerprint
         FROM backup_schedule_operations
         WHERE owner_user_id = $1 AND operation_id = $2
         FOR UPDATE",
    )
    .bind(owner_user_id.into_uuid())
    .bind(operation_id)
    .fetch_optional(&mut **transaction)
    .await?)
}

async fn load_operation_result(
    transaction: &mut Transaction<'_, Postgres>,
    operation: BackupScheduleOperationRow,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
    expected_request: &BackupScheduleRequest,
) -> Result<BackupScheduleRevision, BackupScheduleError> {
    let persisted_fingerprint = decode_fingerprint(
        operation.fingerprint_version,
        &operation.request_fingerprint,
    )?;
    let expected_fingerprint = expected_request
        .fingerprint_for_version(persisted_fingerprint.version())
        .ok_or(BackupScheduleError::InvalidPersistedData)?;
    if operation.owner_user_id != owner_user_id.into_uuid()
        || operation.backup_set_id != backup_set_id.into_uuid()
        || persisted_fingerprint != expected_fingerprint
    {
        return Err(BackupScheduleError::SemanticIdempotencyConflict);
    }
    let revision = load_revision_row(
        transaction,
        owner_user_id,
        operation.schedule_id,
        operation.result_revision_id,
    )
    .await?
    .ok_or(BackupScheduleError::InvalidPersistedData)?
    .try_into_domain()?;
    let revision_fingerprint = revision.request_fingerprint();
    let compatible_result = if revision_fingerprint.version() == persisted_fingerprint.version() {
        revision_fingerprint == persisted_fingerprint
    } else if revision_fingerprint.version() == BACKUP_SCHEDULE_LEGACY_FINGERPRINT_VERSION
        && persisted_fingerprint.version() == BACKUP_SCHEDULE_FINGERPRINT_VERSION
    {
        // After migration, a new policy-aware no-op operation may point to the
        // unchanged v1 current revision. Preserve the no-op/effective_from
        // contract while validating the complete v2 semantics against the
        // safely defaulted immutable result revision.
        revision.config() == expected_request.config()
    } else {
        false
    };
    if !compatible_result || revision.backup_set_id().into_uuid() != operation.backup_set_id {
        return Err(BackupScheduleError::InvalidPersistedData);
    }
    Ok(revision)
}

async fn insert_schedule_operation(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    operation_id: &str,
    backup_set_id: BackupSetId,
    schedule_id: Uuid,
    revision_id: Uuid,
    fingerprint: BackupScheduleIdempotencyFingerprint,
) -> Result<(), BackupScheduleError> {
    let inserted = sqlx::query(
        "INSERT INTO backup_schedule_operations
            (owner_user_id, operation_id, backup_set_id, schedule_id,
             result_revision_id, fingerprint_version, request_fingerprint, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, clock_timestamp())
         ON CONFLICT (owner_user_id, operation_id) DO NOTHING",
    )
    .bind(owner_user_id.into_uuid())
    .bind(operation_id)
    .bind(backup_set_id.into_uuid())
    .bind(schedule_id)
    .bind(revision_id)
    .bind(
        i16::try_from(fingerprint.version())
            .map_err(|_| BackupScheduleError::InvalidPersistedData)?,
    )
    .bind(fingerprint.as_bytes().as_slice())
    .execute(&mut **transaction)
    .await?
    .rows_affected();
    if inserted != 1 {
        // A same-set duplicate is serialized by the BackupSet fence and is
        // handled by the initial replay lookup. A conflict here therefore
        // means another owner-scoped target won the operation identity race;
        // aborting the transaction preserves atomicity.
        return Err(BackupScheduleError::SemanticIdempotencyConflict);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_schedule_revision(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
    schedule_id: BackupScheduleId,
    revision_id: BackupScheduleRevisionId,
    revision_number: BackupScheduleRevisionNumber,
    operation_id: &str,
    fingerprint: BackupScheduleIdempotencyFingerprint,
    config: &BackupScheduleConfig,
) -> Result<BackupScheduleRevisionRow, BackupScheduleError> {
    let weekly_days = config
        .weekly_days()
        .iter()
        .map(|day| i16::from(day.ordinal()))
        .collect::<Vec<_>>();
    Ok(sqlx::query_as::<_, BackupScheduleRevisionRow>(
        "INSERT INTO backup_schedule_revisions
            (id, schedule_id, owner_user_id, backup_set_id, revision_number,
             operation_id, fingerprint_version, request_fingerprint,
             recurrence_kind, timezone, local_time_minute, weekly_days,
             misfire_mode, max_lateness_seconds, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                 clock_timestamp())
         RETURNING id, schedule_id, owner_user_id, backup_set_id,
                   revision_number, operation_id, fingerprint_version,
                   request_fingerprint, recurrence_kind, timezone,
                   local_time_minute, weekly_days, misfire_mode,
                   max_lateness_seconds, created_at",
    )
    .bind(revision_id.into_uuid())
    .bind(schedule_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .bind(
        i64::try_from(revision_number.get())
            .map_err(|_| BackupScheduleError::InvalidPersistedData)?,
    )
    .bind(operation_id)
    .bind(
        i16::try_from(fingerprint.version())
            .map_err(|_| BackupScheduleError::InvalidPersistedData)?,
    )
    .bind(fingerprint.as_bytes().as_slice())
    .bind(config.recurrence_kind().as_str())
    .bind(config.timezone().as_str())
    .bind(
        i16::try_from(config.local_time().minute())
            .map_err(|_| BackupScheduleError::InvalidPersistedData)?,
    )
    .bind(weekly_days)
    .bind(config.misfire_mode().as_str())
    .bind(
        i32::try_from(config.max_lateness_seconds())
            .map_err(|_| BackupScheduleError::InvalidPersistedData)?,
    )
    .fetch_one(&mut **transaction)
    .await?)
}

async fn update_current_revision(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    schedule_id: BackupScheduleId,
    revision_id: BackupScheduleRevisionId,
) -> Result<(), BackupScheduleError> {
    let changed = sqlx::query(
        "WITH observed AS (SELECT clock_timestamp() AS value)
         UPDATE backup_schedules
         SET current_revision_id = $1, effective_from = observed.value,
             updated_at = observed.value
         FROM observed
         WHERE id = $2 AND owner_user_id = $3",
    )
    .bind(revision_id.into_uuid())
    .bind(schedule_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .execute(&mut **transaction)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(BackupScheduleError::InvalidPersistedData);
    }
    Ok(())
}

fn decode_fingerprint(
    version: i16,
    bytes: &[u8],
) -> Result<BackupScheduleIdempotencyFingerprint, BackupScheduleError> {
    let version = u16::try_from(version).map_err(|_| BackupScheduleError::InvalidPersistedData)?;
    let bytes =
        <[u8; 32]>::try_from(bytes).map_err(|_| BackupScheduleError::InvalidPersistedData)?;
    if version != BACKUP_SCHEDULE_LEGACY_FINGERPRINT_VERSION
        && version != BACKUP_SCHEDULE_FINGERPRINT_VERSION
    {
        return Err(BackupScheduleError::InvalidPersistedData);
    }
    Ok(BackupScheduleIdempotencyFingerprint::new(version, bytes))
}

fn validate_operation_id(value: &str) -> Result<(), BackupScheduleError> {
    if (MIN_BACKUP_SCHEDULE_OPERATION_KEY_BYTES..=MAX_BACKUP_SCHEDULE_OPERATION_KEY_BYTES)
        .contains(&value.len())
    {
        Ok(())
    } else {
        Err(BackupScheduleError::InvalidOperationId)
    }
}

fn map_domain_error(error: DomainError) -> BackupScheduleError {
    match error {
        DomainError::InvalidTimezone => BackupScheduleError::InvalidTimezone,
        DomainError::InvalidLocalTime => BackupScheduleError::InvalidLocalTime,
        DomainError::InvalidWeeklyDays => BackupScheduleError::InvalidWeeklyDays,
        DomainError::InvalidBackupScheduleMaxLateness => BackupScheduleError::InvalidMaxLateness,
        _ => BackupScheduleError::InvalidPersistedData,
    }
}
