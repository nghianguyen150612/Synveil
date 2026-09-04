//! Durable backup-scheduling semantics.
//!
//! This module owns the logical schedule contract and the pure recurrence
//! planner. It deliberately has no persistence, worker, snapshot, journal, or
//! object-store dependency. A schedule stores a named IANA timezone and a
//! local wall-clock minute; the planner resolves that wall time against the
//! compiled timezone database for each candidate local calendar date.

use std::{fmt, str::FromStr};

use chrono::{DateTime, Datelike, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use sha2::{Digest, Sha256};
use time::{Date, Month, OffsetDateTime};

use super::backup::{BackupMaintenanceRun, BackupMaintenanceRunState};
use super::errors::DomainError;
use crate::{
    BackupMaintenanceRunId, BackupScheduleId, BackupScheduleMisfireSkipId,
    BackupScheduleOccurrenceId, BackupScheduleRevisionId, BackupScheduledMaintenanceClaimId,
    BackupScheduledMaintenanceLeaseToken, BackupScheduledMaintenanceWorkerId, BackupSetId,
    Timestamp, UserId,
};

/// Current policy-aware semantic configuration fingerprint version.
pub const BACKUP_SCHEDULE_FINGERPRINT_VERSION: u16 = 2;
/// Historical Prompt 61 projection, retained for durable operation replay.
pub const BACKUP_SCHEDULE_LEGACY_FINGERPRINT_VERSION: u16 = 1;

pub const DEFAULT_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS: u32 = 604_800;
pub const MIN_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS: u32 = 60;
pub const MAX_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS: u32 = 2_678_400;

const MINUTES_PER_DAY: u16 = 24 * 60;
const MAX_RECURRENCE_DATE_SCAN_DAYS: usize =
    (MAX_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS / 86_400) as usize + 3;

/// Closed missed-occurrence policy vocabulary.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BackupScheduleMisfireMode {
    ReplayOneByOne,
    LatestOnly,
}

impl BackupScheduleMisfireMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReplayOneByOne => "REPLAY_ONE_BY_ONE",
            Self::LatestOnly => "LATEST_ONLY",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupScheduleMisfireModeParseError;

impl fmt::Display for BackupScheduleMisfireModeParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("backup schedule misfire mode is unknown")
    }
}

impl std::error::Error for BackupScheduleMisfireModeParseError {}

impl FromStr for BackupScheduleMisfireMode {
    type Err = BackupScheduleMisfireModeParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.eq_ignore_ascii_case("REPLAY_ONE_BY_ONE") {
            Ok(Self::ReplayOneByOne)
        } else if value.eq_ignore_ascii_case("LATEST_ONLY") {
            Ok(Self::LatestOnly)
        } else {
            Err(BackupScheduleMisfireModeParseError)
        }
    }
}

/// The closed recurrence vocabulary in Prompt 61.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BackupScheduleRecurrenceKind {
    Daily,
    Weekly,
}

impl BackupScheduleRecurrenceKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "DAILY",
            Self::Weekly => "WEEKLY",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupScheduleRecurrenceKindParseError;

impl fmt::Display for BackupScheduleRecurrenceKindParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("backup schedule recurrence kind is unknown")
    }
}

impl std::error::Error for BackupScheduleRecurrenceKindParseError {}

impl FromStr for BackupScheduleRecurrenceKind {
    type Err = BackupScheduleRecurrenceKindParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.eq_ignore_ascii_case("DAILY") {
            Ok(Self::Daily)
        } else if value.eq_ignore_ascii_case("WEEKLY") {
            Ok(Self::Weekly)
        } else {
            Err(BackupScheduleRecurrenceKindParseError)
        }
    }
}

/// Canonical, locale-independent weekday identity. Monday is always ordinal
/// one and Sunday is always ordinal seven.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BackupScheduleWeekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl BackupScheduleWeekday {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Monday => "MONDAY",
            Self::Tuesday => "TUESDAY",
            Self::Wednesday => "WEDNESDAY",
            Self::Thursday => "THURSDAY",
            Self::Friday => "FRIDAY",
            Self::Saturday => "SATURDAY",
            Self::Sunday => "SUNDAY",
        }
    }

    #[must_use]
    pub const fn ordinal(self) -> u8 {
        match self {
            Self::Monday => 1,
            Self::Tuesday => 2,
            Self::Wednesday => 3,
            Self::Thursday => 4,
            Self::Friday => 5,
            Self::Saturday => 6,
            Self::Sunday => 7,
        }
    }

    #[must_use]
    pub const fn from_ordinal(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Monday),
            2 => Some(Self::Tuesday),
            3 => Some(Self::Wednesday),
            4 => Some(Self::Thursday),
            5 => Some(Self::Friday),
            6 => Some(Self::Saturday),
            7 => Some(Self::Sunday),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupScheduleWeekdayParseError;

impl fmt::Display for BackupScheduleWeekdayParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("backup schedule weekday is unknown")
    }
}

impl std::error::Error for BackupScheduleWeekdayParseError {}

impl FromStr for BackupScheduleWeekday {
    type Err = BackupScheduleWeekdayParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_uppercase().as_str() {
            "MONDAY" => Ok(Self::Monday),
            "TUESDAY" => Ok(Self::Tuesday),
            "WEDNESDAY" => Ok(Self::Wednesday),
            "THURSDAY" => Ok(Self::Thursday),
            "FRIDAY" => Ok(Self::Friday),
            "SATURDAY" => Ok(Self::Saturday),
            "SUNDAY" => Ok(Self::Sunday),
            _ => Err(BackupScheduleWeekdayParseError),
        }
    }
}

/// A validated IANA timezone identity. The value is the timezone database's
/// canonical spelling, not a fixed UTC offset and not the host's local zone.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BackupScheduleTimezone(Tz);

impl BackupScheduleTimezone {
    pub fn new(value: &str) -> Result<Self, DomainError> {
        value
            .parse::<Tz>()
            .map(Self)
            .map_err(|_| DomainError::InvalidTimezone)
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        self.0.name()
    }

    #[must_use]
    const fn as_tz(self) -> Tz {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupScheduleTimezoneParseError;

impl fmt::Display for BackupScheduleTimezoneParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("backup schedule timezone is not a valid IANA timezone")
    }
}

impl std::error::Error for BackupScheduleTimezoneParseError {}

impl FromStr for BackupScheduleTimezone {
    type Err = BackupScheduleTimezoneParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<Tz>()
            .map(Self)
            .map_err(|_| BackupScheduleTimezoneParseError)
    }
}

impl fmt::Display for BackupScheduleTimezone {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A local wall-clock minute with one-minute precision. Seconds and
/// sub-minute values cannot enter the scheduling domain.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BackupScheduleLocalTime(u16);

impl BackupScheduleLocalTime {
    pub fn new(hour: u8, minute: u8) -> Result<Self, DomainError> {
        if hour >= 24 || minute >= 60 {
            return Err(DomainError::InvalidLocalTime);
        }
        Self::from_minute(u16::from(hour) * 60 + u16::from(minute))
    }

    pub fn from_minute(minute: u16) -> Result<Self, DomainError> {
        if minute >= MINUTES_PER_DAY {
            return Err(DomainError::InvalidLocalTime);
        }
        Ok(Self(minute))
    }

    #[must_use]
    pub const fn minute(self) -> u16 {
        self.0
    }

    #[must_use]
    pub const fn hour(self) -> u8 {
        (self.0 / 60) as u8
    }

    #[must_use]
    pub const fn minute_of_hour(self) -> u8 {
        (self.0 % 60) as u8
    }
}

impl FromStr for BackupScheduleLocalTime {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let bytes = value.as_bytes();
        if bytes.len() != 5
            || bytes[2] != b':'
            || !bytes[0].is_ascii_digit()
            || !bytes[1].is_ascii_digit()
            || !bytes[3].is_ascii_digit()
            || !bytes[4].is_ascii_digit()
        {
            return Err(DomainError::InvalidLocalTime);
        }
        let hour = (bytes[0] - b'0') * 10 + bytes[1] - b'0';
        let minute = (bytes[3] - b'0') * 10 + bytes[4] - b'0';
        Self::new(hour, minute)
    }
}

impl fmt::Display for BackupScheduleLocalTime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:02}:{:02}", self.hour(), self.minute_of_hour())
    }
}

/// Immutable semantic configuration for one schedule revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupScheduleConfig {
    recurrence_kind: BackupScheduleRecurrenceKind,
    timezone: BackupScheduleTimezone,
    local_time: BackupScheduleLocalTime,
    weekly_days: Vec<BackupScheduleWeekday>,
    misfire_mode: BackupScheduleMisfireMode,
    max_lateness_seconds: u32,
}

impl BackupScheduleConfig {
    pub fn new(
        recurrence_kind: BackupScheduleRecurrenceKind,
        timezone: BackupScheduleTimezone,
        local_time: BackupScheduleLocalTime,
        weekly_days: Vec<BackupScheduleWeekday>,
    ) -> Result<Self, DomainError> {
        Self::new_with_misfire_policy(
            recurrence_kind,
            timezone,
            local_time,
            weekly_days,
            BackupScheduleMisfireMode::LatestOnly,
            DEFAULT_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS,
        )
    }

    pub fn new_with_misfire_policy(
        recurrence_kind: BackupScheduleRecurrenceKind,
        timezone: BackupScheduleTimezone,
        local_time: BackupScheduleLocalTime,
        mut weekly_days: Vec<BackupScheduleWeekday>,
        misfire_mode: BackupScheduleMisfireMode,
        max_lateness_seconds: u32,
    ) -> Result<Self, DomainError> {
        weekly_days.sort_unstable();
        weekly_days.dedup();

        match recurrence_kind {
            BackupScheduleRecurrenceKind::Daily if !weekly_days.is_empty() => {
                return Err(DomainError::InvalidWeeklyDays);
            }
            BackupScheduleRecurrenceKind::Weekly if weekly_days.is_empty() => {
                return Err(DomainError::InvalidWeeklyDays);
            }
            BackupScheduleRecurrenceKind::Daily | BackupScheduleRecurrenceKind::Weekly => {}
        }
        if !(MIN_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS..=MAX_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS)
            .contains(&max_lateness_seconds)
        {
            return Err(DomainError::InvalidBackupScheduleMaxLateness);
        }

        Ok(Self {
            recurrence_kind,
            timezone,
            local_time,
            weekly_days,
            misfire_mode,
            max_lateness_seconds,
        })
    }

    pub fn daily(
        timezone: BackupScheduleTimezone,
        local_time: BackupScheduleLocalTime,
    ) -> Result<Self, DomainError> {
        Self::new(
            BackupScheduleRecurrenceKind::Daily,
            timezone,
            local_time,
            Vec::new(),
        )
    }

    pub fn weekly(
        timezone: BackupScheduleTimezone,
        local_time: BackupScheduleLocalTime,
        weekly_days: Vec<BackupScheduleWeekday>,
    ) -> Result<Self, DomainError> {
        Self::new(
            BackupScheduleRecurrenceKind::Weekly,
            timezone,
            local_time,
            weekly_days,
        )
    }

    #[must_use]
    pub const fn recurrence_kind(&self) -> BackupScheduleRecurrenceKind {
        self.recurrence_kind
    }

    #[must_use]
    pub const fn timezone(&self) -> BackupScheduleTimezone {
        self.timezone
    }

    #[must_use]
    pub const fn local_time(&self) -> BackupScheduleLocalTime {
        self.local_time
    }

    #[must_use]
    pub fn weekly_days(&self) -> &[BackupScheduleWeekday] {
        &self.weekly_days
    }

    #[must_use]
    pub const fn misfire_mode(&self) -> BackupScheduleMisfireMode {
        self.misfire_mode
    }

    #[must_use]
    pub const fn max_lateness_seconds(&self) -> u32 {
        self.max_lateness_seconds
    }
}

/// The semantic input used by the durable configuration idempotency fence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupScheduleRequest {
    backup_set_id: BackupSetId,
    config: BackupScheduleConfig,
}

impl BackupScheduleRequest {
    #[must_use]
    pub const fn new(backup_set_id: BackupSetId, config: BackupScheduleConfig) -> Self {
        Self {
            backup_set_id,
            config,
        }
    }

    #[must_use]
    pub const fn backup_set_id(&self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn config(&self) -> &BackupScheduleConfig {
        &self.config
    }

    #[must_use]
    pub fn fingerprint(&self) -> BackupScheduleIdempotencyFingerprint {
        self.fingerprint_for_version(BACKUP_SCHEDULE_FINGERPRINT_VERSION)
            .expect("current schedule fingerprint version must be supported")
    }

    /// Compute the exact semantic projection used by persisted evidence.
    /// Version 1 excludes policy fields; version 2 includes them.
    #[must_use]
    pub fn fingerprint_for_version(
        &self,
        version: u16,
    ) -> Option<BackupScheduleIdempotencyFingerprint> {
        if version != BACKUP_SCHEDULE_LEGACY_FINGERPRINT_VERSION
            && version != BACKUP_SCHEDULE_FINGERPRINT_VERSION
        {
            return None;
        }
        let timezone = self.config.timezone.as_str().as_bytes();
        let mut canonical = Vec::with_capacity(96);
        canonical.extend_from_slice(b"synveil/backup-schedule\0");
        canonical.extend_from_slice(&version.to_be_bytes());
        canonical.extend_from_slice(self.backup_set_id.as_bytes());
        canonical.push(match self.config.recurrence_kind {
            BackupScheduleRecurrenceKind::Daily => 1,
            BackupScheduleRecurrenceKind::Weekly => 2,
        });
        canonical.extend_from_slice(&(timezone.len() as u16).to_be_bytes());
        canonical.extend_from_slice(timezone);
        canonical.extend_from_slice(&self.config.local_time.minute().to_be_bytes());
        canonical.push(self.config.weekly_days.len() as u8);
        for day in &self.config.weekly_days {
            canonical.push(day.ordinal());
        }
        if version == BACKUP_SCHEDULE_FINGERPRINT_VERSION {
            canonical.push(match self.config.misfire_mode {
                BackupScheduleMisfireMode::ReplayOneByOne => 1,
                BackupScheduleMisfireMode::LatestOnly => 2,
            });
            canonical.extend_from_slice(&self.config.max_lateness_seconds.to_be_bytes());
        }
        let digest = Sha256::digest(canonical);
        let mut sha256 = [0_u8; 32];
        sha256.copy_from_slice(&digest);
        Some(BackupScheduleIdempotencyFingerprint::new(version, sha256))
    }
}

/// Persisted semantic request fingerprint. It contains no physical storage
/// identity and is safe to compare at the metadata idempotency boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BackupScheduleIdempotencyFingerprint {
    version: u16,
    sha256: [u8; 32],
}

impl BackupScheduleIdempotencyFingerprint {
    #[must_use]
    pub const fn new(version: u16, sha256: [u8; 32]) -> Self {
        Self { version, sha256 }
    }

    #[must_use]
    pub const fn version(self) -> u16 {
        self.version
    }

    #[must_use]
    pub const fn sha256(self) -> [u8; 32] {
        self.sha256
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.sha256
    }
}

/// Positive, monotonically increasing revision number within one logical
/// backup schedule. PostgreSQL allocation is serialized by the owning set row.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BackupScheduleRevisionNumber(u64);

impl BackupScheduleRevisionNumber {
    pub fn new(value: u64) -> Result<Self, DomainError> {
        if value == 0 || i64::try_from(value).is_err() {
            return Err(DomainError::BackupScheduleInvalidRevision);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn checked_next(self) -> Result<Self, DomainError> {
        self.0
            .checked_add(1)
            .ok_or(DomainError::BackupScheduleInvalidRevision)
            .and_then(Self::new)
    }
}

impl FromStr for BackupScheduleRevisionNumber {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<u64>()
            .map_err(|_| DomainError::BackupScheduleInvalidRevision)
            .and_then(Self::new)
    }
}

impl fmt::Display for BackupScheduleRevisionNumber {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// One immutable schedule configuration revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupScheduleRevision {
    id: BackupScheduleRevisionId,
    schedule_id: BackupScheduleId,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
    revision_number: BackupScheduleRevisionNumber,
    operation_id: String,
    request_fingerprint: BackupScheduleIdempotencyFingerprint,
    config: BackupScheduleConfig,
    created_at: Timestamp,
}

impl BackupScheduleRevision {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: BackupScheduleRevisionId,
        schedule_id: BackupScheduleId,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        revision_number: BackupScheduleRevisionNumber,
        operation_id: String,
        request_fingerprint: BackupScheduleIdempotencyFingerprint,
        config: BackupScheduleConfig,
        created_at: Timestamp,
    ) -> Result<Self, DomainError> {
        let request = BackupScheduleRequest::new(backup_set_id, config.clone());
        if (request_fingerprint.version() == BACKUP_SCHEDULE_LEGACY_FINGERPRINT_VERSION
            && (config.misfire_mode() != BackupScheduleMisfireMode::LatestOnly
                || config.max_lateness_seconds() != DEFAULT_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS))
            || request.fingerprint_for_version(request_fingerprint.version())
                != Some(request_fingerprint)
        {
            return Err(DomainError::BackupScheduleInvalidRevision);
        }
        Ok(Self {
            id,
            schedule_id,
            owner_user_id,
            backup_set_id,
            revision_number,
            operation_id,
            request_fingerprint,
            config,
            created_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> BackupScheduleRevisionId {
        self.id
    }

    #[must_use]
    pub const fn schedule_id(&self) -> BackupScheduleId {
        self.schedule_id
    }

    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn backup_set_id(&self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn revision_number(&self) -> BackupScheduleRevisionNumber {
        self.revision_number
    }

    #[must_use]
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    #[must_use]
    pub const fn request_fingerprint(&self) -> BackupScheduleIdempotencyFingerprint {
        self.request_fingerprint
    }

    #[must_use]
    pub const fn config(&self) -> &BackupScheduleConfig {
        &self.config
    }

    #[must_use]
    pub const fn recurrence_kind(&self) -> BackupScheduleRecurrenceKind {
        self.config.recurrence_kind
    }

    #[must_use]
    pub const fn timezone(&self) -> BackupScheduleTimezone {
        self.config.timezone
    }

    #[must_use]
    pub const fn local_time(&self) -> BackupScheduleLocalTime {
        self.config.local_time
    }

    #[must_use]
    pub fn weekly_days(&self) -> &[BackupScheduleWeekday] {
        self.config.weekly_days()
    }

    #[must_use]
    pub const fn misfire_mode(&self) -> BackupScheduleMisfireMode {
        self.config.misfire_mode()
    }

    #[must_use]
    pub const fn max_lateness_seconds(&self) -> u32 {
        self.config.max_lateness_seconds()
    }

    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    /// Calculate the next occurrence strictly after an absolute UTC instant.
    #[must_use]
    pub fn next_occurrence_after(
        &self,
        exclusive_utc_instant: Timestamp,
    ) -> Option<PlannedScheduleOccurrence> {
        next_occurrence_after(self, exclusive_utc_instant)
    }

    /// Resolve this revision's one canonical occurrence on an exact local
    /// calendar date, if the recurrence includes that date.
    #[must_use]
    pub fn occurrence_on_local_date(
        &self,
        local_calendar_date: Date,
    ) -> Option<PlannedScheduleOccurrence> {
        occurrence_on_local_date(self, local_calendar_date)
    }
}

/// Stable logical schedule identity plus its one authoritative current
/// revision. Historical revisions remain available through their own IDs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupSchedule {
    id: BackupScheduleId,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
    current_revision_id: BackupScheduleRevisionId,
    enabled: bool,
    current_revision: BackupScheduleRevision,
    effective_from: Timestamp,
    created_at: Timestamp,
    updated_at: Timestamp,
}

impl BackupSchedule {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: BackupScheduleId,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        current_revision: BackupScheduleRevision,
        enabled: bool,
        effective_from: Timestamp,
        created_at: Timestamp,
        updated_at: Timestamp,
    ) -> Result<Self, DomainError> {
        if current_revision.schedule_id() != id
            || current_revision.owner_user_id() != owner_user_id
            || current_revision.backup_set_id() != backup_set_id
        {
            return Err(DomainError::BackupScheduleInvalidRevision);
        }
        Ok(Self {
            id,
            owner_user_id,
            backup_set_id,
            current_revision_id: current_revision.id(),
            enabled,
            current_revision,
            effective_from,
            created_at,
            updated_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> BackupScheduleId {
        self.id
    }

    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn backup_set_id(&self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn current_revision_id(&self) -> BackupScheduleRevisionId {
        self.current_revision_id
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub const fn current_revision(&self) -> &BackupScheduleRevision {
        &self.current_revision
    }

    /// Strict activation boundary for the current enabled configuration.
    /// Newly materialized occurrences must be later than this instant.
    #[must_use]
    pub const fn effective_from(&self) -> Timestamp {
        self.effective_from
    }

    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    #[must_use]
    pub const fn updated_at(&self) -> Timestamp {
        self.updated_at
    }
}

/// The pure planner's result. The local time is the actual valid local wall
/// time selected for the date; for a DST gap it can be later than the
/// configured time, while an overlap still produces one result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlannedScheduleOccurrence {
    schedule_id: BackupScheduleId,
    schedule_revision_id: BackupScheduleRevisionId,
    local_calendar_date: Date,
    local_wall_time: BackupScheduleLocalTime,
    timezone: BackupScheduleTimezone,
    scheduled_for_utc: Timestamp,
}

impl PlannedScheduleOccurrence {
    #[must_use]
    pub const fn schedule_id(self) -> BackupScheduleId {
        self.schedule_id
    }

    #[must_use]
    pub const fn schedule_revision_id(self) -> BackupScheduleRevisionId {
        self.schedule_revision_id
    }

    #[must_use]
    pub const fn local_calendar_date(self) -> Date {
        self.local_calendar_date
    }

    #[must_use]
    pub const fn local_wall_time(self) -> BackupScheduleLocalTime {
        self.local_wall_time
    }

    #[must_use]
    pub const fn timezone(self) -> BackupScheduleTimezone {
        self.timezone
    }

    #[must_use]
    pub const fn scheduled_for_utc(self) -> Timestamp {
        self.scheduled_for_utc
    }
}

/// Immutable durable evidence that Synveil recognized one due logical
/// schedule occurrence. This is control-plane identity only: it carries no
/// execution state, snapshot identity, maintenance run, or physical storage
/// identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupScheduleOccurrence {
    id: BackupScheduleOccurrenceId,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
    schedule_id: BackupScheduleId,
    schedule_revision_id: BackupScheduleRevisionId,
    local_calendar_date: Date,
    resolved_local_wall_time: BackupScheduleLocalTime,
    timezone: BackupScheduleTimezone,
    scheduled_for_utc: Timestamp,
    materialized_at: Timestamp,
}

impl BackupScheduleOccurrence {
    /// Reconstruct and validate a persisted occurrence against its immutable
    /// revision. Persisted local/UTC values must exactly match the shared pure
    /// recurrence and DST resolver.
    pub fn new(
        id: BackupScheduleOccurrenceId,
        revision: &BackupScheduleRevision,
        local_calendar_date: Date,
        resolved_local_wall_time: BackupScheduleLocalTime,
        scheduled_for_utc: Timestamp,
        materialized_at: Timestamp,
    ) -> Result<Self, DomainError> {
        let planned = occurrence_on_local_date(revision, local_calendar_date)
            .ok_or(DomainError::BackupScheduleInvalidOccurrence)?;
        if planned.local_wall_time() != resolved_local_wall_time
            || planned.scheduled_for_utc() != scheduled_for_utc
        {
            return Err(DomainError::BackupScheduleInvalidOccurrence);
        }
        Ok(Self {
            id,
            owner_user_id: revision.owner_user_id(),
            backup_set_id: revision.backup_set_id(),
            schedule_id: revision.schedule_id(),
            schedule_revision_id: revision.id(),
            local_calendar_date,
            resolved_local_wall_time,
            timezone: revision.timezone(),
            scheduled_for_utc,
            materialized_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> BackupScheduleOccurrenceId {
        self.id
    }

    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn backup_set_id(&self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn schedule_id(&self) -> BackupScheduleId {
        self.schedule_id
    }

    #[must_use]
    pub const fn schedule_revision_id(&self) -> BackupScheduleRevisionId {
        self.schedule_revision_id
    }

    #[must_use]
    pub const fn local_calendar_date(&self) -> Date {
        self.local_calendar_date
    }

    #[must_use]
    pub const fn resolved_local_wall_time(&self) -> BackupScheduleLocalTime {
        self.resolved_local_wall_time
    }

    #[must_use]
    pub const fn timezone(&self) -> BackupScheduleTimezone {
        self.timezone
    }

    #[must_use]
    pub const fn scheduled_for_utc(&self) -> Timestamp {
        self.scheduled_for_utc
    }

    #[must_use]
    pub const fn materialized_at(&self) -> Timestamp {
        self.materialized_at
    }
}

/// Immutable provenance binding between one already-materialized schedule
/// occurrence and one canonical maintenance run. The relation carries only
/// logical control-plane identity; execution progress remains owned by
/// `BackupMaintenanceRun`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupScheduleOccurrenceHandoff {
    occurrence_id: BackupScheduleOccurrenceId,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
    schedule_id: BackupScheduleId,
    maintenance_run_id: BackupMaintenanceRunId,
    created_at: Timestamp,
}

impl BackupScheduleOccurrenceHandoff {
    /// Create validated immutable handoff evidence from the two canonical
    /// domain objects. A run may be bound only within the same owner/BackupSet
    /// scope as the occurrence.
    pub fn new(
        occurrence: &BackupScheduleOccurrence,
        maintenance_run: &BackupMaintenanceRun,
        created_at: Timestamp,
    ) -> Result<Self, DomainError> {
        if occurrence.owner_user_id() != maintenance_run.owner_user_id()
            || occurrence.backup_set_id() != maintenance_run.backup_set_id()
        {
            return Err(DomainError::BackupScheduleInvalidHandoff);
        }
        Ok(Self {
            occurrence_id: occurrence.id(),
            owner_user_id: occurrence.owner_user_id(),
            backup_set_id: occurrence.backup_set_id(),
            schedule_id: occurrence.schedule_id(),
            maintenance_run_id: maintenance_run.id(),
            created_at,
        })
    }

    #[must_use]
    pub const fn occurrence_id(&self) -> BackupScheduleOccurrenceId {
        self.occurrence_id
    }

    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn backup_set_id(&self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn schedule_id(&self) -> BackupScheduleId {
        self.schedule_id
    }

    #[must_use]
    pub const fn maintenance_run_id(&self) -> BackupMaintenanceRunId {
        self.maintenance_run_id
    }

    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }
}

/// The canonical replay result for one occurrence-to-maintenance handoff.
/// `Created` is returned only by the transaction that inserted the relation;
/// `Existing` is the stable response for all later retries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackupScheduleOccurrenceHandoffResult {
    Created {
        occurrence: BackupScheduleOccurrence,
        maintenance_run: BackupMaintenanceRun,
        handoff: BackupScheduleOccurrenceHandoff,
    },
    Existing {
        occurrence: BackupScheduleOccurrence,
        maintenance_run: BackupMaintenanceRun,
        handoff: BackupScheduleOccurrenceHandoff,
    },
}

impl BackupScheduleOccurrenceHandoffResult {
    #[must_use]
    pub const fn occurrence(&self) -> &BackupScheduleOccurrence {
        match self {
            Self::Created { occurrence, .. } | Self::Existing { occurrence, .. } => occurrence,
        }
    }

    #[must_use]
    pub const fn maintenance_run(&self) -> &BackupMaintenanceRun {
        match self {
            Self::Created {
                maintenance_run, ..
            }
            | Self::Existing {
                maintenance_run, ..
            } => maintenance_run,
        }
    }

    #[must_use]
    pub const fn handoff(&self) -> &BackupScheduleOccurrenceHandoff {
        match self {
            Self::Created { handoff, .. } | Self::Existing { handoff, .. } => handoff,
        }
    }

    #[must_use]
    pub const fn is_created(&self) -> bool {
        matches!(self, Self::Created { .. })
    }

    #[must_use]
    pub const fn is_existing(&self) -> bool {
        matches!(self, Self::Existing { .. })
    }
}

/// Immutable evidence that one activation epoch intentionally advanced past
/// an expired prefix without granting execution authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupScheduleMisfireSkip {
    id: BackupScheduleMisfireSkipId,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
    schedule_id: BackupScheduleId,
    schedule_revision_id: BackupScheduleRevisionId,
    activation_effective_from: Timestamp,
    resolved_from_exclusive_utc: Timestamp,
    resolved_through_utc: Timestamp,
    observed_at_utc: Timestamp,
    misfire_mode: BackupScheduleMisfireMode,
    max_lateness_seconds: u32,
    created_at: Timestamp,
}

impl BackupScheduleMisfireSkip {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: BackupScheduleMisfireSkipId,
        revision: &BackupScheduleRevision,
        activation_effective_from: Timestamp,
        resolved_from_exclusive_utc: Timestamp,
        resolved_through_utc: Timestamp,
        observed_at_utc: Timestamp,
        misfire_mode: BackupScheduleMisfireMode,
        max_lateness_seconds: u32,
        created_at: Timestamp,
    ) -> Result<Self, DomainError> {
        if resolved_from_exclusive_utc < activation_effective_from
            || resolved_through_utc <= resolved_from_exclusive_utc
            || resolved_through_utc > observed_at_utc
            || misfire_mode != revision.misfire_mode()
            || max_lateness_seconds != revision.max_lateness_seconds()
        {
            return Err(DomainError::BackupScheduleInvalidMisfireSkip);
        }
        Ok(Self {
            id,
            owner_user_id: revision.owner_user_id(),
            backup_set_id: revision.backup_set_id(),
            schedule_id: revision.schedule_id(),
            schedule_revision_id: revision.id(),
            activation_effective_from,
            resolved_from_exclusive_utc,
            resolved_through_utc,
            observed_at_utc,
            misfire_mode,
            max_lateness_seconds,
            created_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> BackupScheduleMisfireSkipId {
        self.id
    }

    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn backup_set_id(&self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn schedule_id(&self) -> BackupScheduleId {
        self.schedule_id
    }

    #[must_use]
    pub const fn schedule_revision_id(&self) -> BackupScheduleRevisionId {
        self.schedule_revision_id
    }

    #[must_use]
    pub const fn activation_effective_from(&self) -> Timestamp {
        self.activation_effective_from
    }

    #[must_use]
    pub const fn resolved_from_exclusive_utc(&self) -> Timestamp {
        self.resolved_from_exclusive_utc
    }

    #[must_use]
    pub const fn resolved_through_utc(&self) -> Timestamp {
        self.resolved_through_utc
    }

    #[must_use]
    pub const fn observed_at_utc(&self) -> Timestamp {
        self.observed_at_utc
    }

    #[must_use]
    pub const fn misfire_mode(&self) -> BackupScheduleMisfireMode {
        self.misfire_mode
    }

    #[must_use]
    pub const fn max_lateness_seconds(&self) -> u32 {
        self.max_lateness_seconds
    }

    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }
}

/// Safe logical metadata returned for a durable expired-prefix action.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BackupSchedulerSkipOutcome {
    schedule_id: BackupScheduleId,
    schedule_revision_id: BackupScheduleRevisionId,
    backup_set_id: BackupSetId,
    resolved_from_exclusive_utc: Timestamp,
    resolved_through_utc: Timestamp,
    misfire_mode: BackupScheduleMisfireMode,
}

impl From<&BackupScheduleMisfireSkip> for BackupSchedulerSkipOutcome {
    fn from(skip: &BackupScheduleMisfireSkip) -> Self {
        Self {
            schedule_id: skip.schedule_id(),
            schedule_revision_id: skip.schedule_revision_id(),
            backup_set_id: skip.backup_set_id(),
            resolved_from_exclusive_utc: skip.resolved_from_exclusive_utc(),
            resolved_through_utc: skip.resolved_through_utc(),
            misfire_mode: skip.misfire_mode(),
        }
    }
}

impl BackupSchedulerSkipOutcome {
    #[must_use]
    pub const fn schedule_id(self) -> BackupScheduleId {
        self.schedule_id
    }

    #[must_use]
    pub const fn schedule_revision_id(self) -> BackupScheduleRevisionId {
        self.schedule_revision_id
    }

    #[must_use]
    pub const fn backup_set_id(self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn resolved_from_exclusive_utc(self) -> Timestamp {
        self.resolved_from_exclusive_utc
    }

    #[must_use]
    pub const fn resolved_through_utc(self) -> Timestamp {
        self.resolved_through_utc
    }

    #[must_use]
    pub const fn misfire_mode(self) -> BackupScheduleMisfireMode {
        self.misfire_mode
    }
}

/// The logical references returned by one successful manual scheduler tick.
///
/// This is deliberately smaller than either the occurrence or maintenance-run
/// records. It reports the durable control-plane handoff and exposes no
/// snapshot, object-store, journal, or other physical identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BackupSchedulerTickOutcome {
    occurrence_id: BackupScheduleOccurrenceId,
    schedule_id: BackupScheduleId,
    schedule_revision_id: BackupScheduleRevisionId,
    backup_set_id: BackupSetId,
    scheduled_for_utc: Timestamp,
    maintenance_run_id: BackupMaintenanceRunId,
}

impl BackupSchedulerTickOutcome {
    /// Build the result only from the canonical handoff response. The scope
    /// checks keep a malformed relation from being presented as scheduler
    /// progress above the domain boundary.
    pub fn from_handoff_result(
        result: &BackupScheduleOccurrenceHandoffResult,
    ) -> Result<Self, DomainError> {
        let occurrence = result.occurrence();
        let handoff = result.handoff();
        if handoff.occurrence_id() != occurrence.id()
            || handoff.owner_user_id() != occurrence.owner_user_id()
            || handoff.backup_set_id() != occurrence.backup_set_id()
            || handoff.schedule_id() != occurrence.schedule_id()
        {
            return Err(DomainError::BackupScheduleInvalidHandoff);
        }
        Ok(Self {
            occurrence_id: occurrence.id(),
            schedule_id: occurrence.schedule_id(),
            schedule_revision_id: occurrence.schedule_revision_id(),
            backup_set_id: occurrence.backup_set_id(),
            scheduled_for_utc: occurrence.scheduled_for_utc(),
            maintenance_run_id: handoff.maintenance_run_id(),
        })
    }

    #[must_use]
    pub const fn occurrence_id(self) -> BackupScheduleOccurrenceId {
        self.occurrence_id
    }

    #[must_use]
    pub const fn schedule_id(self) -> BackupScheduleId {
        self.schedule_id
    }

    #[must_use]
    pub const fn schedule_revision_id(self) -> BackupScheduleRevisionId {
        self.schedule_revision_id
    }

    #[must_use]
    pub const fn backup_set_id(self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn scheduled_for_utc(self) -> Timestamp {
        self.scheduled_for_utc
    }

    #[must_use]
    pub const fn maintenance_run_id(self) -> BackupMaintenanceRunId {
        self.maintenance_run_id
    }
}

/// Canonical outcome of one explicit scheduler invocation. A tick is either
/// idle, completes an already-materialized occurrence, or materializes and
/// hands off exactly one new occurrence.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BackupSchedulerTickResult {
    Idle,
    SkippedExpired(BackupSchedulerSkipOutcome),
    HandedOffExisting(BackupSchedulerTickOutcome),
    MaterializedAndHandedOff(BackupSchedulerTickOutcome),
}

impl BackupSchedulerTickResult {
    #[must_use]
    pub const fn is_idle(self) -> bool {
        matches!(self, Self::Idle)
    }

    #[must_use]
    pub const fn is_skipped(self) -> bool {
        matches!(self, Self::SkippedExpired(_))
    }

    #[must_use]
    pub const fn is_existing(self) -> bool {
        matches!(self, Self::HandedOffExisting(_))
    }

    #[must_use]
    pub const fn is_created(self) -> bool {
        matches!(self, Self::MaterializedAndHandedOff(_))
    }

    #[must_use]
    pub const fn outcome(self) -> Option<BackupSchedulerTickOutcome> {
        match self {
            Self::Idle | Self::SkippedExpired(_) => None,
            Self::HandedOffExisting(outcome) | Self::MaterializedAndHandedOff(outcome) => {
                Some(outcome)
            }
        }
    }

    #[must_use]
    pub const fn skip_outcome(self) -> Option<BackupSchedulerSkipOutcome> {
        match self {
            Self::SkippedExpired(outcome) => Some(outcome),
            Self::Idle | Self::HandedOffExisting(_) | Self::MaterializedAndHandedOff(_) => None,
        }
    }
}

/// Bounded reason why a valid-looking target cannot create a new durable
/// occurrence at the serialized configuration boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackupScheduleOccurrenceNotEffectiveReason {
    BackupSetInactive,
    ScheduleDisabled,
    RevisionSuperseded,
    BeforeEffectiveFrom,
    ScheduleInstantAlreadyMaterialized,
    ExpiredForAutomaticExecution,
    SchedulerProgressResolved,
}

/// Exact materialization result. Only `Created` and `Existing` carry firing
/// authority, and both carry the same canonical immutable occurrence shape.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackupScheduleOccurrenceMaterializationResult {
    Created(BackupScheduleOccurrence),
    Existing(BackupScheduleOccurrence),
    NotDue,
    NotEffective(BackupScheduleOccurrenceNotEffectiveReason),
    InvalidTarget,
}

impl BackupScheduleOccurrenceMaterializationResult {
    #[must_use]
    pub const fn occurrence(&self) -> Option<&BackupScheduleOccurrence> {
        match self {
            Self::Created(occurrence) | Self::Existing(occurrence) => Some(occurrence),
            Self::NotDue | Self::NotEffective(_) | Self::InvalidTarget => None,
        }
    }
}

/// Resolve the exact canonical occurrence for one local calendar date. DAILY
/// accepts every representable date; WEEKLY accepts only normalized selected
/// weekdays. DST resolution is shared with `next_occurrence_after`.
#[must_use]
pub fn occurrence_on_local_date(
    schedule_revision: &BackupScheduleRevision,
    local_calendar_date: Date,
) -> Option<PlannedScheduleOccurrence> {
    let date = NaiveDate::from_ymd_opt(
        local_calendar_date.year(),
        u32::from(u8::from(local_calendar_date.month())),
        u32::from(local_calendar_date.day()),
    )?;
    planned_occurrence_on_chrono_date(schedule_revision, date)
}

/// Pure recurrence calculation. It performs no I/O and uses only the
/// schedule's compiled IANA timezone plus the supplied UTC reference instant.
#[must_use]
pub fn next_occurrence_after(
    schedule_revision: &BackupScheduleRevision,
    exclusive_utc_instant: Timestamp,
) -> Option<PlannedScheduleOccurrence> {
    let reference = timestamp_to_chrono_utc(exclusive_utc_instant)?;
    let timezone = schedule_revision.timezone().as_tz();
    let local_reference = reference.with_timezone(&timezone);
    let mut date = local_reference.date_naive();

    for _ in 0..MAX_RECURRENCE_DATE_SCAN_DAYS {
        if let Some(candidate) = planned_occurrence_on_chrono_date(schedule_revision, date) {
            let candidate_utc = timestamp_to_chrono_utc(candidate.scheduled_for_utc())?;
            // This strict comparison is the duplicate-prevention boundary.
            // In an overlap, the earlier absolute instant was already chosen
            // by resolve_local_minute; the later copy is never considered.
            if candidate_utc > reference {
                return Some(candidate);
            }
        }

        date = date.succ_opt()?;
    }
    None
}

/// Select the oldest occurrence in a bounded automatic-execution window.
/// `resolved_after_utc` is strict scheduler progress; `not_before_utc` and
/// `through_utc` are the inclusive lateness window boundaries.
#[must_use]
pub fn oldest_occurrence_in_window(
    schedule_revision: &BackupScheduleRevision,
    resolved_after_utc: Timestamp,
    not_before_utc: Timestamp,
    through_utc: Timestamp,
) -> Option<PlannedScheduleOccurrence> {
    if not_before_utc > through_utc || !bounded_window(not_before_utc, through_utc) {
        return None;
    }
    let timezone = schedule_revision.timezone().as_tz();
    let reference = timestamp_to_chrono_utc(not_before_utc)?;
    let mut date = reference.with_timezone(&timezone).date_naive();
    for _ in 0..MAX_RECURRENCE_DATE_SCAN_DAYS {
        if let Some(candidate) = planned_occurrence_on_chrono_date(schedule_revision, date)
            && candidate.scheduled_for_utc() > resolved_after_utc
            && candidate.scheduled_for_utc() >= not_before_utc
            && candidate.scheduled_for_utc() <= through_utc
        {
            return Some(candidate);
        }
        date = date.succ_opt()?;
    }
    None
}

/// Select the newest occurrence in the same bounded execution window.
#[must_use]
pub fn latest_occurrence_in_window(
    schedule_revision: &BackupScheduleRevision,
    resolved_after_utc: Timestamp,
    not_before_utc: Timestamp,
    through_utc: Timestamp,
) -> Option<PlannedScheduleOccurrence> {
    if not_before_utc > through_utc || !bounded_window(not_before_utc, through_utc) {
        return None;
    }
    let timezone = schedule_revision.timezone().as_tz();
    let reference = timestamp_to_chrono_utc(through_utc)?;
    let mut date = reference.with_timezone(&timezone).date_naive();
    for _ in 0..MAX_RECURRENCE_DATE_SCAN_DAYS {
        if let Some(candidate) = planned_occurrence_on_chrono_date(schedule_revision, date)
            && candidate.scheduled_for_utc() > resolved_after_utc
            && candidate.scheduled_for_utc() >= not_before_utc
            && candidate.scheduled_for_utc() <= through_utc
        {
            return Some(candidate);
        }
        date = date.pred_opt()?;
    }
    None
}

/// Find the last canonical occurrence strictly before an instant. Recurrence
/// shape guarantees a result, when representable, within seven local days;
/// the shared 34-day hard limit is deliberately conservative around timezone
/// transitions and date bounds.
#[must_use]
pub fn last_occurrence_before(
    schedule_revision: &BackupScheduleRevision,
    exclusive_utc_instant: Timestamp,
) -> Option<PlannedScheduleOccurrence> {
    let timezone = schedule_revision.timezone().as_tz();
    let reference = timestamp_to_chrono_utc(exclusive_utc_instant)?;
    let mut date = reference.with_timezone(&timezone).date_naive();
    for _ in 0..MAX_RECURRENCE_DATE_SCAN_DAYS {
        if let Some(candidate) = planned_occurrence_on_chrono_date(schedule_revision, date)
            && candidate.scheduled_for_utc() < exclusive_utc_instant
        {
            return Some(candidate);
        }
        date = date.pred_opt()?;
    }
    None
}

fn bounded_window(not_before_utc: Timestamp, through_utc: Timestamp) -> bool {
    let width = through_utc.as_offset_datetime() - not_before_utc.as_offset_datetime();
    width.is_positive()
        && width.whole_seconds() <= i64::from(MAX_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS) + 86_400
}

fn planned_occurrence_on_chrono_date(
    schedule_revision: &BackupScheduleRevision,
    date: NaiveDate,
) -> Option<PlannedScheduleOccurrence> {
    if !occurs_on_date(schedule_revision, date) {
        return None;
    }
    let timezone = schedule_revision.timezone().as_tz();
    let (local_minute, local_instant) =
        resolve_local_minute(timezone, date, schedule_revision.local_time().minute())?;
    Some(PlannedScheduleOccurrence {
        schedule_id: schedule_revision.schedule_id(),
        schedule_revision_id: schedule_revision.id(),
        local_calendar_date: chrono_date_to_time_date(date)?,
        local_wall_time: BackupScheduleLocalTime::from_minute(local_minute).ok()?,
        timezone: schedule_revision.timezone(),
        scheduled_for_utc: chrono_utc_to_timestamp(local_instant.with_timezone(&Utc))?,
    })
}

fn occurs_on_date(schedule_revision: &BackupScheduleRevision, date: NaiveDate) -> bool {
    match schedule_revision.recurrence_kind() {
        BackupScheduleRecurrenceKind::Daily => true,
        BackupScheduleRecurrenceKind::Weekly => {
            let ordinal = date.weekday().number_from_monday() as u8;
            schedule_revision
                .weekly_days()
                .iter()
                .any(|day| day.ordinal() == ordinal)
        }
    }
}

/// Resolve one configured wall-clock minute. A nonexistent local minute is
/// advanced one minute at a time until the first valid minute on that same
/// local date. An ambiguous minute chooses the earlier absolute instant.
fn resolve_local_minute(
    timezone: Tz,
    date: NaiveDate,
    configured_minute: u16,
) -> Option<(u16, DateTime<Tz>)> {
    for minute in configured_minute..MINUTES_PER_DAY {
        let local_time =
            NaiveTime::from_hms_opt(u32::from(minute / 60), u32::from(minute % 60), 0)?;
        let local = NaiveDateTime::new(date, local_time);
        let instant = match timezone.from_local_datetime(&local) {
            LocalResult::Single(value) => value,
            LocalResult::Ambiguous(first, second) => {
                let first_utc = first.with_timezone(&Utc);
                let second_utc = second.with_timezone(&Utc);
                if first_utc <= second_utc {
                    first
                } else {
                    second
                }
            }
            LocalResult::None => continue,
        };
        return Some((minute, instant));
    }
    None
}

fn timestamp_to_chrono_utc(value: Timestamp) -> Option<DateTime<Utc>> {
    let value = value.as_offset_datetime();
    DateTime::<Utc>::from_timestamp(value.unix_timestamp(), value.nanosecond())
}

fn chrono_utc_to_timestamp(value: DateTime<Utc>) -> Option<Timestamp> {
    let value = OffsetDateTime::from_unix_timestamp(value.timestamp()).ok()?;
    let value = value.replace_nanosecond(value.nanosecond()).ok()?;
    Some(Timestamp::from_offset_datetime(value))
}

fn chrono_date_to_time_date(value: NaiveDate) -> Option<Date> {
    let month = Month::try_from(value.month() as u8).ok()?;
    Date::from_calendar_date(value.year(), month, value.day() as u8).ok()
}

/// Bounded lease duration for one explicit scheduled-maintenance worker step.
///
/// A lease covers a single bounded transition attempt. There is no heartbeat
/// renewal in Prompt 66; a future daemon prompt may add renewal if a bounded
/// attempt proves insufficient.
pub const DEFAULT_BACKUP_SCHEDULED_MAINTENANCE_LEASE_SECONDS: u64 = 120;
pub const MIN_BACKUP_SCHEDULED_MAINTENANCE_LEASE_SECONDS: u64 = 10;
pub const MAX_BACKUP_SCHEDULED_MAINTENANCE_LEASE_SECONDS: u64 = 900;

/// Validate one explicitly requested lease duration without silent clamping.
pub fn validate_scheduled_maintenance_lease_duration(
    lease_duration_seconds: u64,
) -> Result<std::time::Duration, DomainError> {
    if (MIN_BACKUP_SCHEDULED_MAINTENANCE_LEASE_SECONDS
        ..=MAX_BACKUP_SCHEDULED_MAINTENANCE_LEASE_SECONDS)
        .contains(&lease_duration_seconds)
    {
        Ok(std::time::Duration::from_secs(lease_duration_seconds))
    } else {
        Err(DomainError::BackupScheduleInvalidMaintenanceClaim)
    }
}

/// The predetermined maintenance transition for one claimable expected state.
/// `Created -> SnapshotCaptured`, `SnapshotCaptured -> ExpiryPlanned`,
/// `ExpiryPlanned -> Completed`. Terminal states are never claimable.
#[must_use]
pub fn scheduled_maintenance_resulting_state(
    expected_state: BackupMaintenanceRunState,
) -> Option<BackupMaintenanceRunState> {
    match expected_state {
        BackupMaintenanceRunState::Created => Some(BackupMaintenanceRunState::SnapshotCaptured),
        BackupMaintenanceRunState::SnapshotCaptured => {
            Some(BackupMaintenanceRunState::ExpiryPlanned)
        }
        BackupMaintenanceRunState::ExpiryPlanned => Some(BackupMaintenanceRunState::Completed),
        BackupMaintenanceRunState::Completed | BackupMaintenanceRunState::Stale => None,
    }
}

/// Whether a maintenance state may appear as a claim's expected state.
#[must_use]
pub const fn is_claimable_scheduled_maintenance_state(state: BackupMaintenanceRunState) -> bool {
    matches!(
        state,
        BackupMaintenanceRunState::Created
            | BackupMaintenanceRunState::SnapshotCaptured
            | BackupMaintenanceRunState::ExpiryPlanned
    )
}

/// Durable authorization to execute exactly one transition from one expected
/// maintenance state. Identity is `(maintenance_run_id, expected_state)`; the
/// lease triple `(worker, token, generation)` fences stale holders.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupScheduledMaintenanceClaim {
    claim_id: BackupScheduledMaintenanceClaimId,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
    schedule_id: BackupScheduleId,
    occurrence_id: BackupScheduleOccurrenceId,
    maintenance_run_id: BackupMaintenanceRunId,
    expected_state: BackupMaintenanceRunState,
    resulting_state: Option<BackupMaintenanceRunState>,
    lease_worker_id: BackupScheduledMaintenanceWorkerId,
    lease_token: BackupScheduledMaintenanceLeaseToken,
    lease_generation: u64,
    lease_acquired_at: Timestamp,
    lease_expires_at: Timestamp,
    completed_at: Option<Timestamp>,
    created_at: Timestamp,
}

impl BackupScheduledMaintenanceClaim {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        claim_id: BackupScheduledMaintenanceClaimId,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        schedule_id: BackupScheduleId,
        occurrence_id: BackupScheduleOccurrenceId,
        maintenance_run_id: BackupMaintenanceRunId,
        expected_state: BackupMaintenanceRunState,
        lease_worker_id: BackupScheduledMaintenanceWorkerId,
        lease_token: BackupScheduledMaintenanceLeaseToken,
        lease_generation: u64,
        lease_acquired_at: Timestamp,
        lease_expires_at: Timestamp,
        created_at: Timestamp,
    ) -> Result<Self, DomainError> {
        let resulting_state = scheduled_maintenance_resulting_state(expected_state)
            .ok_or(DomainError::BackupScheduleInvalidMaintenanceClaim)?;
        if lease_generation == 0 || lease_expires_at <= lease_acquired_at {
            return Err(DomainError::BackupScheduleInvalidMaintenanceClaim);
        }
        Ok(Self {
            claim_id,
            owner_user_id,
            backup_set_id,
            schedule_id,
            occurrence_id,
            maintenance_run_id,
            expected_state,
            resulting_state: Some(resulting_state),
            lease_worker_id,
            lease_token,
            lease_generation,
            lease_acquired_at,
            lease_expires_at,
            completed_at: None,
            created_at,
        })
    }

    /// Rehydrate validated persisted state, including the completed receipt.
    #[allow(clippy::too_many_arguments)]
    pub fn rehydrate(
        claim_id: BackupScheduledMaintenanceClaimId,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        schedule_id: BackupScheduleId,
        occurrence_id: BackupScheduleOccurrenceId,
        maintenance_run_id: BackupMaintenanceRunId,
        expected_state: BackupMaintenanceRunState,
        resulting_state: Option<BackupMaintenanceRunState>,
        lease_worker_id: BackupScheduledMaintenanceWorkerId,
        lease_token: BackupScheduledMaintenanceLeaseToken,
        lease_generation: u64,
        lease_acquired_at: Timestamp,
        lease_expires_at: Timestamp,
        completed_at: Option<Timestamp>,
        created_at: Timestamp,
    ) -> Result<Self, DomainError> {
        let expected_resulting = scheduled_maintenance_resulting_state(expected_state)
            .ok_or(DomainError::BackupScheduleInvalidMaintenanceClaim)?;
        if lease_generation == 0 || lease_expires_at <= lease_acquired_at {
            return Err(DomainError::BackupScheduleInvalidMaintenanceClaim);
        }
        match (completed_at, resulting_state) {
            (None, None) => {}
            (Some(_), Some(actual)) if actual == expected_resulting => {}
            _ => {
                return Err(DomainError::BackupScheduleInvalidMaintenanceClaim);
            }
        }
        Ok(Self {
            claim_id,
            owner_user_id,
            backup_set_id,
            schedule_id,
            occurrence_id,
            maintenance_run_id,
            expected_state,
            resulting_state,
            lease_worker_id,
            lease_token,
            lease_generation,
            lease_acquired_at,
            lease_expires_at,
            completed_at,
            created_at,
        })
    }

    #[must_use]
    pub const fn claim_id(&self) -> BackupScheduledMaintenanceClaimId {
        self.claim_id
    }

    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn backup_set_id(&self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn schedule_id(&self) -> BackupScheduleId {
        self.schedule_id
    }

    #[must_use]
    pub const fn occurrence_id(&self) -> BackupScheduleOccurrenceId {
        self.occurrence_id
    }

    #[must_use]
    pub const fn maintenance_run_id(&self) -> BackupMaintenanceRunId {
        self.maintenance_run_id
    }

    #[must_use]
    pub const fn expected_state(&self) -> BackupMaintenanceRunState {
        self.expected_state
    }

    #[must_use]
    pub const fn resulting_state(&self) -> Option<BackupMaintenanceRunState> {
        self.resulting_state
    }

    /// The predetermined resulting state for this claim's expected state.
    #[must_use]
    pub fn expected_resulting_state(&self) -> BackupMaintenanceRunState {
        scheduled_maintenance_resulting_state(self.expected_state)
            .expect("claim expected state must be claimable")
    }

    #[must_use]
    pub const fn lease_worker_id(&self) -> BackupScheduledMaintenanceWorkerId {
        self.lease_worker_id
    }

    #[must_use]
    pub const fn lease_token(&self) -> BackupScheduledMaintenanceLeaseToken {
        self.lease_token
    }

    #[must_use]
    pub const fn lease_generation(&self) -> u64 {
        self.lease_generation
    }

    #[must_use]
    pub const fn lease_acquired_at(&self) -> Timestamp {
        self.lease_acquired_at
    }

    #[must_use]
    pub const fn lease_expires_at(&self) -> Timestamp {
        self.lease_expires_at
    }

    #[must_use]
    pub const fn completed_at(&self) -> Option<Timestamp> {
        self.completed_at
    }

    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    #[must_use]
    pub const fn is_completed(&self) -> bool {
        self.completed_at.is_some()
    }

    /// A takeover is authorized exactly at or after expiry.
    #[must_use]
    pub fn is_takeover_eligible(&self, observed_at_utc: Timestamp) -> bool {
        !self.is_completed() && self.lease_expires_at <= observed_at_utc
    }

    /// An unexpired, incomplete lease held by any worker.
    #[must_use]
    pub fn is_lease_active(&self, observed_at_utc: Timestamp) -> bool {
        !self.is_completed() && self.lease_expires_at > observed_at_utc
    }
}

/// Canonical outcome of one explicit claim-discovery invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackupScheduledMaintenanceClaimOutcome {
    Idle,
    Claimed(BackupScheduledMaintenanceClaim),
    ExistingCurrentLease(BackupScheduledMaintenanceClaim),
    TakenOver(BackupScheduledMaintenanceClaim),
    RecoveredCompletion(BackupScheduledMaintenanceClaim),
}

impl BackupScheduledMaintenanceClaimOutcome {
    #[must_use]
    pub const fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    #[must_use]
    pub const fn claim(&self) -> Option<&BackupScheduledMaintenanceClaim> {
        match self {
            Self::Idle => None,
            Self::Claimed(claim)
            | Self::ExistingCurrentLease(claim)
            | Self::TakenOver(claim)
            | Self::RecoveredCompletion(claim) => Some(claim),
        }
    }
}

/// Canonical result of executing one fenced claimed step.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackupScheduledMaintenanceStepResult {
    Advanced {
        claim: BackupScheduledMaintenanceClaim,
        maintenance_run: BackupMaintenanceRun,
    },
    AlreadyCompleted {
        claim: BackupScheduledMaintenanceClaim,
    },
    RecoveredCompletion {
        claim: BackupScheduledMaintenanceClaim,
        maintenance_run: BackupMaintenanceRun,
    },
    LeaseLost,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::{
        BackupMaintenanceRunRequest, BackupMaintenanceRunState,
        BackupSnapshotRetentionPolicyRevisionId, BackupSnapshotRetentionPolicyRevisionNumber,
    };

    use super::*;

    fn timestamp(value: &str) -> Timestamp {
        Timestamp::parse(value).expect("test timestamp must be valid")
    }

    #[test]
    fn scheduled_maintenance_claim_contract_is_closed_and_bounded() {
        assert_eq!(
            scheduled_maintenance_resulting_state(BackupMaintenanceRunState::Created),
            Some(BackupMaintenanceRunState::SnapshotCaptured)
        );
        assert_eq!(
            scheduled_maintenance_resulting_state(BackupMaintenanceRunState::SnapshotCaptured),
            Some(BackupMaintenanceRunState::ExpiryPlanned)
        );
        assert_eq!(
            scheduled_maintenance_resulting_state(BackupMaintenanceRunState::ExpiryPlanned),
            Some(BackupMaintenanceRunState::Completed)
        );
        assert_eq!(
            scheduled_maintenance_resulting_state(BackupMaintenanceRunState::Completed),
            None
        );
        assert_eq!(
            scheduled_maintenance_resulting_state(BackupMaintenanceRunState::Stale),
            None
        );
        assert!(is_claimable_scheduled_maintenance_state(
            BackupMaintenanceRunState::Created
        ));
        assert!(!is_claimable_scheduled_maintenance_state(
            BackupMaintenanceRunState::Completed
        ));
        assert!(!is_claimable_scheduled_maintenance_state(
            BackupMaintenanceRunState::Stale
        ));

        assert_eq!(
            validate_scheduled_maintenance_lease_duration(
                DEFAULT_BACKUP_SCHEDULED_MAINTENANCE_LEASE_SECONDS
            )
            .unwrap(),
            Duration::from_secs(120)
        );
        assert!(
            validate_scheduled_maintenance_lease_duration(
                MIN_BACKUP_SCHEDULED_MAINTENANCE_LEASE_SECONDS
            )
            .is_ok()
        );
        assert!(
            validate_scheduled_maintenance_lease_duration(
                MAX_BACKUP_SCHEDULED_MAINTENANCE_LEASE_SECONDS
            )
            .is_ok()
        );
        for invalid in [0, 9, 901, u64::MAX] {
            assert_eq!(
                validate_scheduled_maintenance_lease_duration(invalid),
                Err(DomainError::BackupScheduleInvalidMaintenanceClaim)
            );
        }

        let acquired = timestamp("2026-09-03T10:00:00Z");
        let expires = timestamp("2026-09-03T10:02:00Z");
        let claim = BackupScheduledMaintenanceClaim::new(
            BackupScheduledMaintenanceClaimId::new(),
            UserId::new(),
            BackupSetId::new(),
            BackupScheduleId::new(),
            BackupScheduleOccurrenceId::new(),
            BackupMaintenanceRunId::new(),
            BackupMaintenanceRunState::Created,
            BackupScheduledMaintenanceWorkerId::new(),
            BackupScheduledMaintenanceLeaseToken::new(),
            1,
            acquired,
            expires,
            acquired,
        )
        .expect("claim must validate");
        assert_eq!(
            claim.expected_resulting_state(),
            BackupMaintenanceRunState::SnapshotCaptured
        );
        assert!(!claim.is_completed());
        assert!(claim.is_lease_active(acquired));
        assert!(!claim.is_takeover_eligible(acquired));
        assert!(claim.is_takeover_eligible(expires));
        assert!(
            BackupScheduledMaintenanceClaim::new(
                BackupScheduledMaintenanceClaimId::new(),
                UserId::new(),
                BackupSetId::new(),
                BackupScheduleId::new(),
                BackupScheduleOccurrenceId::new(),
                BackupMaintenanceRunId::new(),
                BackupMaintenanceRunState::Completed,
                BackupScheduledMaintenanceWorkerId::new(),
                BackupScheduledMaintenanceLeaseToken::new(),
                1,
                acquired,
                expires,
                acquired,
            )
            .is_err(),
            "terminal states are never claimable"
        );
    }

    fn timezone(value: &str) -> BackupScheduleTimezone {
        value.parse().expect("test timezone must be valid")
    }

    fn local_time(value: &str) -> BackupScheduleLocalTime {
        value.parse().expect("test local time must be valid")
    }

    fn date(year: i32, month: Month, day: u8) -> Date {
        Date::from_calendar_date(year, month, day).expect("test date must be valid")
    }

    fn revision(
        kind: BackupScheduleRecurrenceKind,
        zone: &str,
        time: &str,
        days: Vec<BackupScheduleWeekday>,
    ) -> BackupScheduleRevision {
        revision_with_policy(
            kind,
            zone,
            time,
            days,
            BackupScheduleMisfireMode::LatestOnly,
            DEFAULT_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS,
        )
    }

    fn revision_with_policy(
        kind: BackupScheduleRecurrenceKind,
        zone: &str,
        time: &str,
        days: Vec<BackupScheduleWeekday>,
        misfire_mode: BackupScheduleMisfireMode,
        max_lateness_seconds: u32,
    ) -> BackupScheduleRevision {
        let schedule_id = BackupScheduleId::new();
        let backup_set_id = BackupSetId::new();
        let config = BackupScheduleConfig::new_with_misfire_policy(
            kind,
            timezone(zone),
            local_time(time),
            days,
            misfire_mode,
            max_lateness_seconds,
        )
        .expect("test schedule config must be valid");
        let request = BackupScheduleRequest::new(backup_set_id, config.clone());
        BackupScheduleRevision::new(
            BackupScheduleRevisionId::new(),
            schedule_id,
            UserId::new(),
            backup_set_id,
            BackupScheduleRevisionNumber::new(1).unwrap(),
            "unit-test-operation".to_owned(),
            request.fingerprint(),
            config,
            timestamp("2026-01-01T00:00:00Z"),
        )
        .expect("test revision must be valid")
    }

    #[test]
    fn misfire_policy_defaults_are_safe_closed_and_bounded() {
        let default = BackupScheduleConfig::daily(timezone("UTC"), local_time("02:00")).unwrap();
        assert_eq!(
            default.misfire_mode(),
            BackupScheduleMisfireMode::LatestOnly
        );
        assert_eq!(
            default.max_lateness_seconds(),
            DEFAULT_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS
        );
        assert_eq!(
            "REPLAY_ONE_BY_ONE".parse(),
            Ok(BackupScheduleMisfireMode::ReplayOneByOne)
        );
        assert_eq!(
            "LATEST_ONLY".parse(),
            Ok(BackupScheduleMisfireMode::LatestOnly)
        );
        assert!("UNKNOWN".parse::<BackupScheduleMisfireMode>().is_err());
        for invalid in [
            MIN_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS - 1,
            MAX_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS + 1,
        ] {
            assert_eq!(
                BackupScheduleConfig::new_with_misfire_policy(
                    BackupScheduleRecurrenceKind::Daily,
                    timezone("UTC"),
                    local_time("02:00"),
                    Vec::new(),
                    BackupScheduleMisfireMode::LatestOnly,
                    invalid,
                ),
                Err(DomainError::InvalidBackupScheduleMaxLateness)
            );
        }
    }

    #[test]
    fn fingerprint_v1_projection_survives_policy_upgrade_and_v2_includes_policy() {
        let backup_set_id = BackupSetId::new();
        let latest = BackupScheduleConfig::daily(timezone("UTC"), local_time("02:00")).unwrap();
        let replay = BackupScheduleConfig::new_with_misfire_policy(
            BackupScheduleRecurrenceKind::Daily,
            timezone("UTC"),
            local_time("02:00"),
            Vec::new(),
            BackupScheduleMisfireMode::ReplayOneByOne,
            172_800,
        )
        .unwrap();
        let latest_request = BackupScheduleRequest::new(backup_set_id, latest);
        let replay_request = BackupScheduleRequest::new(backup_set_id, replay);
        assert_eq!(
            latest_request.fingerprint_for_version(1),
            replay_request.fingerprint_for_version(1)
        );
        assert_ne!(latest_request.fingerprint(), replay_request.fingerprint());
        assert_eq!(latest_request.fingerprint().version(), 2);
        assert_eq!(latest_request.fingerprint_for_version(3), None);
    }

    #[test]
    fn bounded_window_planners_keep_exact_cutoff_and_dst_canonicality() {
        let schedule = revision_with_policy(
            BackupScheduleRecurrenceKind::Daily,
            "America/New_York",
            "02:30",
            Vec::new(),
            BackupScheduleMisfireMode::ReplayOneByOne,
            604_800,
        );
        let cutoff = timestamp("2026-03-08T07:00:00Z");
        let observed = timestamp("2026-03-10T12:00:00Z");
        let at_cutoff = oldest_occurrence_in_window(
            &schedule,
            timestamp("2026-03-01T00:00:00Z"),
            cutoff,
            observed,
        )
        .unwrap();
        assert_eq!(at_cutoff.scheduled_for_utc(), cutoff);
        assert_eq!(at_cutoff.local_wall_time(), local_time("03:00"));
        assert_eq!(
            latest_occurrence_in_window(
                &schedule,
                timestamp("2026-03-01T00:00:00Z"),
                cutoff,
                observed,
            )
            .unwrap()
            .scheduled_for_utc(),
            timestamp("2026-03-10T06:30:00Z")
        );
        assert!(
            last_occurrence_before(&schedule, cutoff)
                .unwrap()
                .scheduled_for_utc()
                < cutoff
        );
    }

    #[test]
    fn bounded_window_planners_cover_weekly_overlap_and_reject_unbounded_ranges() {
        let weekly = revision_with_policy(
            BackupScheduleRecurrenceKind::Weekly,
            "Europe/Berlin",
            "21:30",
            vec![BackupScheduleWeekday::Monday, BackupScheduleWeekday::Friday],
            BackupScheduleMisfireMode::LatestOnly,
            MAX_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS,
        );
        let start = timestamp("2026-09-01T00:00:00Z");
        let through = timestamp("2026-10-01T00:00:00Z");
        let oldest =
            oldest_occurrence_in_window(&weekly, timestamp("2026-08-31T00:00:00Z"), start, through)
                .unwrap();
        let latest =
            latest_occurrence_in_window(&weekly, timestamp("2026-08-31T00:00:00Z"), start, through)
                .unwrap();
        assert_eq!(
            oldest.local_calendar_date().weekday(),
            time::Weekday::Friday
        );
        assert_eq!(
            latest.local_calendar_date().weekday(),
            time::Weekday::Monday
        );
        assert!(oldest.scheduled_for_utc() < latest.scheduled_for_utc());

        let overlap = revision(
            BackupScheduleRecurrenceKind::Daily,
            "America/New_York",
            "01:30",
            Vec::new(),
        );
        let overlap_occurrence = latest_occurrence_in_window(
            &overlap,
            timestamp("2026-10-31T00:00:00Z"),
            timestamp("2026-11-01T00:00:00Z"),
            timestamp("2026-11-01T23:59:59Z"),
        )
        .unwrap();
        assert_eq!(
            overlap_occurrence.scheduled_for_utc(),
            timestamp("2026-11-01T05:30:00Z")
        );

        assert_eq!(
            oldest_occurrence_in_window(
                &weekly,
                timestamp("2026-01-01T00:00:00Z"),
                timestamp("2026-01-01T00:00:00Z"),
                timestamp("2026-03-01T00:00:00Z"),
            ),
            None
        );
    }

    #[test]
    fn local_time_is_strict_hh_mm_and_bounded() {
        assert_eq!("00:00".parse(), Ok(local_time("00:00")));
        assert_eq!("23:59".parse(), Ok(local_time("23:59")));
        assert!("2:30".parse::<BackupScheduleLocalTime>().is_err());
        assert!("24:00".parse::<BackupScheduleLocalTime>().is_err());
        assert!("12:60".parse::<BackupScheduleLocalTime>().is_err());
        assert!("12:30:00".parse::<BackupScheduleLocalTime>().is_err());
    }

    #[test]
    fn timezone_validation_uses_named_iana_id_without_utc_fallback() {
        assert_eq!(timezone("UTC").as_str(), "UTC");
        assert_eq!(timezone("Asia/Ho_Chi_Minh").as_str(), "Asia/Ho_Chi_Minh");
        assert!("Not/A_Timezone".parse::<BackupScheduleTimezone>().is_err());
        assert_eq!(
            BackupScheduleTimezone::new("Not/A_Timezone"),
            Err(DomainError::InvalidTimezone)
        );
    }

    #[test]
    fn daily_recurrence_returns_before_exact_and_after_cases() {
        let schedule = revision(
            BackupScheduleRecurrenceKind::Daily,
            "UTC",
            "02:30",
            Vec::new(),
        );

        assert_eq!(
            schedule
                .next_occurrence_after(timestamp("2026-01-05T01:00:00Z"))
                .unwrap()
                .scheduled_for_utc(),
            timestamp("2026-01-05T02:30:00Z")
        );
        assert_eq!(
            schedule
                .next_occurrence_after(timestamp("2026-01-05T02:30:00Z"))
                .unwrap()
                .scheduled_for_utc(),
            timestamp("2026-01-06T02:30:00Z")
        );
        assert_eq!(
            schedule
                .next_occurrence_after(timestamp("2026-01-05T03:00:00Z"))
                .unwrap()
                .scheduled_for_utc(),
            timestamp("2026-01-06T02:30:00Z")
        );
    }

    #[test]
    fn weekly_recurrence_normalizes_days_and_moves_across_week() {
        let schedule = revision(
            BackupScheduleRecurrenceKind::Weekly,
            "UTC",
            "21:30",
            vec![
                BackupScheduleWeekday::Friday,
                BackupScheduleWeekday::Monday,
                BackupScheduleWeekday::Friday,
            ],
        );
        assert_eq!(
            schedule.weekly_days(),
            &[BackupScheduleWeekday::Monday, BackupScheduleWeekday::Friday]
        );

        let friday_after = schedule
            .next_occurrence_after(timestamp("2026-01-09T22:00:00Z"))
            .unwrap();
        assert_eq!(
            friday_after.scheduled_for_utc(),
            timestamp("2026-01-12T21:30:00Z")
        );
        assert_eq!(
            friday_after.local_calendar_date(),
            Date::from_calendar_date(2026, Month::January, 12).unwrap()
        );
    }

    #[test]
    fn weekly_same_day_before_and_after_time_are_distinct() {
        let schedule = revision(
            BackupScheduleRecurrenceKind::Weekly,
            "UTC",
            "09:00",
            vec![BackupScheduleWeekday::Monday],
        );
        assert_eq!(
            schedule
                .next_occurrence_after(timestamp("2026-01-05T08:59:59Z"))
                .unwrap()
                .scheduled_for_utc(),
            timestamp("2026-01-05T09:00:00Z")
        );
        assert_eq!(
            schedule
                .next_occurrence_after(timestamp("2026-01-05T09:00:00Z"))
                .unwrap()
                .scheduled_for_utc(),
            timestamp("2026-01-12T09:00:00Z")
        );
    }

    #[test]
    fn utc_and_non_dst_timezone_keep_local_wall_time() {
        let utc = revision(
            BackupScheduleRecurrenceKind::Daily,
            "UTC",
            "00:00",
            Vec::new(),
        );
        let utc_occurrence = utc
            .next_occurrence_after(timestamp("2026-02-28T23:59:00Z"))
            .unwrap();
        assert_eq!(
            utc_occurrence.scheduled_for_utc(),
            timestamp("2026-03-01T00:00:00Z")
        );
        assert_eq!(utc_occurrence.local_wall_time(), local_time("00:00"));

        let vietnam = revision(
            BackupScheduleRecurrenceKind::Daily,
            "Asia/Ho_Chi_Minh",
            "02:00",
            Vec::new(),
        );
        assert_eq!(
            vietnam
                .next_occurrence_after(timestamp("2026-03-01T18:59:00Z"))
                .unwrap()
                .scheduled_for_utc(),
            timestamp("2026-03-01T19:00:00Z")
        );
    }

    #[test]
    fn spring_forward_gap_chooses_first_valid_minute_on_same_date() {
        let schedule = revision(
            BackupScheduleRecurrenceKind::Daily,
            "America/New_York",
            "02:30",
            Vec::new(),
        );
        let occurrence = schedule
            .next_occurrence_after(timestamp("2026-03-08T05:00:00Z"))
            .unwrap();
        assert_eq!(occurrence.local_wall_time(), local_time("03:00"));
        assert_eq!(
            occurrence.scheduled_for_utc(),
            timestamp("2026-03-08T07:00:00Z")
        );
    }

    #[test]
    fn fall_back_overlap_chooses_earlier_absolute_instant_once() {
        let schedule = revision(
            BackupScheduleRecurrenceKind::Daily,
            "America/New_York",
            "01:30",
            Vec::new(),
        );
        let first = schedule
            .next_occurrence_after(timestamp("2026-11-01T00:00:00Z"))
            .unwrap();
        assert_eq!(first.local_wall_time(), local_time("01:30"));
        assert_eq!(first.scheduled_for_utc(), timestamp("2026-11-01T05:30:00Z"));

        let after_first = schedule
            .next_occurrence_after(timestamp("2026-11-01T05:30:00Z"))
            .unwrap();
        assert_eq!(
            after_first.scheduled_for_utc(),
            timestamp("2026-11-02T06:30:00Z")
        );
    }

    #[test]
    fn date_boundaries_and_leap_day_are_local_calendar_boundaries() {
        let schedule = revision(
            BackupScheduleRecurrenceKind::Daily,
            "UTC",
            "23:59",
            Vec::new(),
        );
        assert_eq!(
            schedule
                .next_occurrence_after(timestamp("2026-12-31T23:58:59Z"))
                .unwrap()
                .scheduled_for_utc(),
            timestamp("2026-12-31T23:59:00Z")
        );
        assert_eq!(
            schedule
                .next_occurrence_after(timestamp("2026-12-31T23:59:00Z"))
                .unwrap()
                .scheduled_for_utc(),
            timestamp("2027-01-01T23:59:00Z")
        );

        let leap = revision(
            BackupScheduleRecurrenceKind::Daily,
            "UTC",
            "00:00",
            Vec::new(),
        );
        assert_eq!(
            leap.next_occurrence_after(timestamp("2028-02-28T23:59:00Z"))
                .unwrap()
                .scheduled_for_utc(),
            timestamp("2028-02-29T00:00:00Z")
        );
    }

    #[test]
    fn invalid_weekday_combinations_are_rejected() {
        let zone = timezone("UTC");
        let time = local_time("02:00");
        assert_eq!(
            BackupScheduleConfig::new(
                BackupScheduleRecurrenceKind::Daily,
                zone,
                time,
                vec![BackupScheduleWeekday::Monday],
            ),
            Err(DomainError::InvalidWeeklyDays)
        );
        assert_eq!(
            BackupScheduleConfig::new(BackupScheduleRecurrenceKind::Weekly, zone, time, Vec::new(),),
            Err(DomainError::InvalidWeeklyDays)
        );
    }

    #[test]
    fn occurrence_on_date_handles_daily_weekly_and_wrong_weekday_targets() {
        let daily = revision(
            BackupScheduleRecurrenceKind::Daily,
            "UTC",
            "09:15",
            Vec::new(),
        );
        let daily_occurrence = daily
            .occurrence_on_local_date(date(2026, Month::September, 2))
            .expect("daily target must exist");
        assert_eq!(
            daily_occurrence.scheduled_for_utc(),
            timestamp("2026-09-02T09:15:00Z")
        );
        assert_eq!(daily_occurrence.local_wall_time(), local_time("09:15"));

        let weekly = revision(
            BackupScheduleRecurrenceKind::Weekly,
            "Asia/Ho_Chi_Minh",
            "21:30",
            vec![BackupScheduleWeekday::Wednesday],
        );
        let matching = weekly
            .occurrence_on_local_date(date(2026, Month::September, 2))
            .expect("selected Wednesday must exist");
        assert_eq!(
            matching.scheduled_for_utc(),
            timestamp("2026-09-02T14:30:00Z")
        );
        assert!(
            weekly
                .occurrence_on_local_date(date(2026, Month::September, 3))
                .is_none(),
            "unselected Thursday must not become a target"
        );
    }

    #[test]
    fn occurrence_on_date_reuses_gap_and_overlap_resolution() {
        let gap = revision(
            BackupScheduleRecurrenceKind::Daily,
            "America/New_York",
            "02:30",
            Vec::new(),
        )
        .occurrence_on_local_date(date(2026, Month::March, 8))
        .expect("gap date must resolve");
        assert_eq!(gap.local_wall_time(), local_time("03:00"));
        assert_eq!(gap.scheduled_for_utc(), timestamp("2026-03-08T07:00:00Z"));

        let overlap = revision(
            BackupScheduleRecurrenceKind::Daily,
            "America/New_York",
            "01:30",
            Vec::new(),
        )
        .occurrence_on_local_date(date(2026, Month::November, 1))
        .expect("overlap date must resolve once");
        assert_eq!(overlap.local_wall_time(), local_time("01:30"));
        assert_eq!(
            overlap.scheduled_for_utc(),
            timestamp("2026-11-01T05:30:00Z")
        );
    }

    #[test]
    fn occurrence_on_date_handles_leap_day_and_year_boundary() {
        let schedule = revision(
            BackupScheduleRecurrenceKind::Daily,
            "UTC",
            "00:00",
            Vec::new(),
        );
        assert_eq!(
            schedule
                .occurrence_on_local_date(date(2028, Month::February, 29))
                .unwrap()
                .scheduled_for_utc(),
            timestamp("2028-02-29T00:00:00Z")
        );
        assert_eq!(
            schedule
                .occurrence_on_local_date(date(2027, Month::January, 1))
                .unwrap()
                .scheduled_for_utc(),
            timestamp("2027-01-01T00:00:00Z")
        );
    }

    #[test]
    fn occurrence_on_date_and_strict_after_planner_are_consistent() {
        let revisions = [
            revision(
                BackupScheduleRecurrenceKind::Daily,
                "UTC",
                "00:00",
                Vec::new(),
            ),
            revision(
                BackupScheduleRecurrenceKind::Daily,
                "America/New_York",
                "02:30",
                Vec::new(),
            ),
            revision(
                BackupScheduleRecurrenceKind::Weekly,
                "Asia/Ho_Chi_Minh",
                "21:30",
                vec![BackupScheduleWeekday::Wednesday],
            ),
        ];
        let dates = [
            date(2026, Month::January, 1),
            date(2026, Month::March, 8),
            date(2026, Month::September, 2),
            date(2026, Month::November, 1),
            date(2028, Month::February, 29),
        ];

        for revision in &revisions {
            for local_date in dates {
                let Some(on_date) = revision.occurrence_on_local_date(local_date) else {
                    continue;
                };
                let reference = on_date
                    .scheduled_for_utc()
                    .checked_sub_std(Duration::from_secs(1))
                    .expect("representative timestamp supports subtraction");
                assert_eq!(
                    revision.next_occurrence_after(reference),
                    Some(on_date),
                    "exact-date and strict-after planners diverged for {local_date}"
                );
            }
        }
    }

    #[test]
    fn handoff_binds_only_same_owner_and_backup_set() {
        let revision = revision(
            BackupScheduleRecurrenceKind::Daily,
            "UTC",
            "09:00",
            Vec::new(),
        );
        let local_calendar_date = date(2026, Month::September, 2);
        let planned = revision
            .occurrence_on_local_date(local_calendar_date)
            .expect("test occurrence must be planned");
        let occurrence = BackupScheduleOccurrence::new(
            BackupScheduleOccurrenceId::new(),
            &revision,
            local_calendar_date,
            planned.local_wall_time(),
            planned.scheduled_for_utc(),
            timestamp("2026-09-02T10:00:00Z"),
        )
        .expect("test occurrence must be valid");
        let created_at = timestamp("2026-09-02T10:00:01Z");
        let run = BackupMaintenanceRun::new(
            BackupMaintenanceRunId::new(),
            revision.owner_user_id(),
            "test-maintenance-run".to_owned(),
            BackupMaintenanceRunRequest::new(revision.backup_set_id()).fingerprint(),
            revision.backup_set_id(),
            BackupSnapshotRetentionPolicyRevisionId::new(),
            BackupSnapshotRetentionPolicyRevisionNumber::new(1).unwrap(),
            "test-maintenance-capture".to_owned(),
            "test-maintenance-expiry".to_owned(),
            BackupMaintenanceRunState::Created,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            created_at,
        )
        .expect("test maintenance run must be valid");
        let handoff = BackupScheduleOccurrenceHandoff::new(&occurrence, &run, created_at)
            .expect("same-scope handoff must be valid");
        assert_eq!(handoff.occurrence_id(), occurrence.id());
        assert_eq!(handoff.maintenance_run_id(), run.id());
        assert_eq!(handoff.owner_user_id(), revision.owner_user_id());
        assert_eq!(handoff.backup_set_id(), revision.backup_set_id());

        let foreign_owner_run = BackupMaintenanceRun::new(
            BackupMaintenanceRunId::new(),
            UserId::new(),
            "foreign-maintenance-run".to_owned(),
            BackupMaintenanceRunRequest::new(revision.backup_set_id()).fingerprint(),
            revision.backup_set_id(),
            BackupSnapshotRetentionPolicyRevisionId::new(),
            BackupSnapshotRetentionPolicyRevisionNumber::new(1).unwrap(),
            "foreign-maintenance-capture".to_owned(),
            "foreign-maintenance-expiry".to_owned(),
            BackupMaintenanceRunState::Created,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            created_at,
        )
        .expect("foreign maintenance run must be valid on its own");
        assert_eq!(
            BackupScheduleOccurrenceHandoff::new(&occurrence, &foreign_owner_run, created_at),
            Err(DomainError::BackupScheduleInvalidHandoff)
        );
    }
}
