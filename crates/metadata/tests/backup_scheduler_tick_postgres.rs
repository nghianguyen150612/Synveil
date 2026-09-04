//! PostgreSQL verification for Prompt 64's manual, bounded scheduler tick.
//!
//! The suite is intentionally ignored unless the caller supplies a fresh
//! PostgreSQL 17 database through `SYNVEIL_TEST_DATABASE_URL`. Run it with
//! `--test-threads=1` so the durable count assertions are isolated while each
//! test uses a distinct owner/BackupSet scope.

use sqlx::PgPool;
use synveil_core::{
    BackupMaintenanceRunState, BackupSchedule, BackupScheduleConfig, BackupScheduleLocalTime,
    BackupScheduleMisfireMode, BackupScheduleOccurrenceMaterializationResult,
    BackupScheduleOccurrenceNotEffectiveReason, BackupScheduleRecurrenceKind,
    BackupScheduleRevision, BackupScheduleTimezone, BackupSchedulerTickResult, BackupSetId,
    DedupDomainId, Library, LibraryId, LogicalName, Node, NodeId, Timestamp, User, UserId,
    UserStatus,
};
use synveil_metadata::{
    BackupScheduleHandoffError, BackupScheduleService, BackupSchedulerError,
    BackupSchedulerService, BackupService, DatabaseConfig, DatabasePool, DomainRepository,
    MigrationRunner,
};
use time::{Date, Duration};

fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse(value).expect("test timestamp must be valid")
}

fn name(value: impl AsRef<str>) -> LogicalName {
    LogicalName::new(value.as_ref()).expect("test logical name must be valid")
}

fn daily(time: &str) -> BackupScheduleConfig {
    BackupScheduleConfig::daily(
        "UTC".parse::<BackupScheduleTimezone>().unwrap(),
        time.parse::<BackupScheduleLocalTime>().unwrap(),
    )
    .expect("daily schedule configuration must be valid")
}

fn daily_with_policy(
    time: &str,
    mode: BackupScheduleMisfireMode,
    max_lateness_seconds: u32,
) -> BackupScheduleConfig {
    BackupScheduleConfig::new_with_misfire_policy(
        BackupScheduleRecurrenceKind::Daily,
        "UTC".parse::<BackupScheduleTimezone>().unwrap(),
        time.parse::<BackupScheduleLocalTime>().unwrap(),
        Vec::new(),
        mode,
        max_lateness_seconds,
    )
    .expect("policy-aware daily schedule must be valid")
}

fn observed_after(planned: synveil_core::PlannedScheduleOccurrence) -> Timestamp {
    planned
        .scheduled_for_utc()
        .checked_add_std(std::time::Duration::from_secs(1))
        .expect("test observed time must be representable")
}

/// Deterministic anchor for all schedule effective_from values.
/// This removes wall-clock nondeterminism: the production planner
/// legitimately rolls a local time like 09:00 to the next day when
/// effective_from is past that wall time, which otherwise makes
/// tests asserting 09:00 < 10:00 < 11:00 depend on the hour the
/// suite is executed. Pinning effective_from to a fixed midnight
/// ensures the same calendar-day ordering at 00:00, 06:00, 10:30,
/// 15:00 and 23:59 UTC.
const FIXED_EFFECTIVE_FROM: &str = "2026-09-02T00:00:00Z";

async fn pin_fixed_effective_from(fixture: &Fixture, schedule: BackupSchedule) -> BackupSchedule {
    let fixed = timestamp(FIXED_EFFECTIVE_FROM);
    // The schedule integrity trigger enforces monotonic effective_from and
    // requires a semantic transition for any change. To obtain a wall-clock-
    // independent fixture we temporarily disable that trigger and coerce all
    // three schedule time columns to the same deterministic midnight. This
    // satisfies the `created_at <= effective_from <= updated_at` check and
    // allows the post-pin value to be earlier than the original
    // clock_timestamp() without violating the production invariant for later
    // semantic transitions (which will again advance from this deterministic
    // base).
    sqlx::query("ALTER TABLE backup_schedules DISABLE TRIGGER backup_schedules_integrity")
        .execute(&fixture.inspection)
        .await
        .expect("disable schedule integrity trigger must succeed");
    sqlx::query(
        "UPDATE backup_schedules
          SET created_at = $1, effective_from = $1, updated_at = $1
          WHERE id = $2 AND owner_user_id = $3",
    )
    .bind(fixed.as_offset_datetime())
    .bind(schedule.id().into_uuid())
    .bind(fixture.owner_user_id.into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("deterministic effective_from pin must succeed");
    sqlx::query("ALTER TABLE backup_schedules ENABLE TRIGGER backup_schedules_integrity")
        .execute(&fixture.inspection)
        .await
        .expect("enable schedule integrity trigger must succeed");
    BackupScheduleService::new(fixture.pool.clone())
        .get_backup_schedule(fixture.owner_user_id, schedule.backup_set_id())
        .await
        .expect("pinned schedule must be readable")
}

struct Fixture {
    pool: DatabasePool,
    inspection: PgPool,
    owner_user_id: UserId,
    library_id: LibraryId,
    backup_set_id: BackupSetId,
}

impl Fixture {
    async fn close(self) {
        // The scheduler is intentionally global across owners. Keep this
        // integration suite isolated when several ignored tests share the
        // same disposable database by disabling this fixture's schedules
        // through the same effective-from transition used by the service.
        sqlx::query(
            "WITH observed AS (SELECT clock_timestamp() AS value)
             UPDATE backup_schedules
             SET enabled = FALSE,
                 effective_from = GREATEST(backup_schedules.effective_from, observed.value)
                     + INTERVAL '1 microsecond',
                 updated_at = GREATEST(backup_schedules.effective_from, observed.value)
                     + INTERVAL '1 microsecond'
             FROM observed
             WHERE owner_user_id = $1 AND enabled = TRUE",
        )
        .bind(self.owner_user_id.into_uuid())
        .execute(&self.inspection)
        .await
        .expect("fixture schedule cleanup must persist");
        self.pool.close().await;
        self.inspection.close().await;
    }

    async fn create_active_backup_set(&self, label: &str) -> BackupSetId {
        let backup_set_id = BackupSetId::new();
        BackupService::new(self.pool.clone())
            .create_backup_set(
                self.owner_user_id,
                backup_set_id,
                name(format!("backup-{label}-{backup_set_id}")),
                self.library_id,
                None,
                timestamp("2026-09-02T00:00:01Z"),
            )
            .await
            .expect("additional backup set must persist");
        sqlx::query(
            "UPDATE backup_sets
             SET state = 'ACTIVE', revision = revision + 1,
                 updated_at = clock_timestamp()
             WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(backup_set_id.into_uuid())
        .bind(self.owner_user_id.into_uuid())
        .execute(&self.inspection)
        .await
        .expect("additional backup set must become active");
        backup_set_id
    }
}

async fn fixture(label: &str) -> Fixture {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a fresh disposable PostgreSQL database");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    let inspection = PgPool::connect(&url)
        .await
        .expect("inspection connection must succeed");
    let status = MigrationRunner::new()
        .run(&pool)
        .await
        .expect("all forward migrations must apply");
    assert!(status.is_current(), "all migrations must be current");
    assert_eq!(status.applied_versions().len(), 34);
    assert_eq!(status.latest_applied_version(), Some(20260903000001));

    let observed_at = timestamp("2026-09-02T00:00:00.123456Z");
    let owner_user_id = UserId::new();
    let repository = DomainRepository::new(&pool);
    repository
        .insert_user(&User::new(
            owner_user_id,
            synveil_core::LoginIdentifier::new(
                format!("scheduler-{label}-{owner_user_id}"),
                format!("scheduler-key-{owner_user_id}"),
            )
            .expect("fixture login must be valid"),
            UserStatus::Active,
            observed_at,
        ))
        .await
        .expect("fixture user must persist");

    let library_id = LibraryId::new();
    let root = Node::new_root(NodeId::new(), library_id, name("root"), observed_at);
    let library = Library::new(
        library_id,
        owner_user_id,
        name(format!("library-{label}-{library_id}")),
        &root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("fixture library must be valid");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("fixture library must persist");

    let backup_set_id = BackupSetId::new();
    BackupService::new(pool.clone())
        .create_backup_set(
            owner_user_id,
            backup_set_id,
            name(format!("backup-{label}-{backup_set_id}")),
            library_id,
            None,
            observed_at,
        )
        .await
        .expect("fixture backup set must persist");
    sqlx::query(
        "UPDATE backup_sets
         SET state = 'ACTIVE', revision = revision + 1,
             updated_at = clock_timestamp()
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(backup_set_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .execute(&inspection)
    .await
    .expect("fixture backup set must become active");

    Fixture {
        pool,
        inspection,
        owner_user_id,
        library_id,
        backup_set_id,
    }
}

async fn configure_policy(fixture: &Fixture, backup_set_id: BackupSetId, label: &str) {
    BackupService::new(fixture.pool.clone())
        .configure_snapshot_retention_policy(
            fixture.owner_user_id,
            format!("policy-{label}-{backup_set_id}"),
            backup_set_id,
            1,
            86_400,
        )
        .await
        .expect("retention policy must persist");
}

async fn configure_schedule(
    fixture: &Fixture,
    backup_set_id: BackupSetId,
    label: &str,
    local_time: &str,
) -> (
    BackupScheduleService,
    BackupScheduleRevision,
    BackupSchedule,
) {
    let service = BackupScheduleService::new(fixture.pool.clone());
    let revision = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            format!("schedule-{label}-{backup_set_id}"),
            backup_set_id,
            daily(local_time),
        )
        .await
        .expect("schedule revision must persist");
    let schedule = service
        .get_backup_schedule(fixture.owner_user_id, backup_set_id)
        .await
        .expect("schedule must be readable");
    let schedule = pin_fixed_effective_from(fixture, schedule).await;
    (service, revision, schedule)
}

async fn configure_schedule_with_policy(
    fixture: &Fixture,
    backup_set_id: BackupSetId,
    label: &str,
    local_time: &str,
    mode: BackupScheduleMisfireMode,
    max_lateness_seconds: u32,
) -> (
    BackupScheduleService,
    BackupScheduleRevision,
    BackupSchedule,
) {
    let service = BackupScheduleService::new(fixture.pool.clone());
    let revision = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            format!("schedule-{label}-{backup_set_id}"),
            backup_set_id,
            daily_with_policy(local_time, mode, max_lateness_seconds),
        )
        .await
        .expect("policy-aware schedule revision must persist");
    let schedule = service
        .get_backup_schedule(fixture.owner_user_id, backup_set_id)
        .await
        .expect("policy-aware schedule must be readable");
    let schedule = pin_fixed_effective_from(fixture, schedule).await;
    (service, revision, schedule)
}

fn first_planned(schedule: &BackupSchedule) -> (Date, synveil_core::PlannedScheduleOccurrence) {
    let date = (schedule.effective_from().as_offset_datetime() + Duration::days(1)).date();
    let planned = schedule
        .current_revision()
        .occurrence_on_local_date(date)
        .expect("daily schedule must plan the next local date");
    (date, planned)
}

async fn materialize_first(
    fixture: &Fixture,
    service: &BackupScheduleService,
    schedule: &BackupSchedule,
    date: Date,
    planned: synveil_core::PlannedScheduleOccurrence,
) -> synveil_core::BackupScheduleOccurrence {
    match service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            schedule.id(),
            schedule.current_revision_id(),
            date,
            observed_after(planned),
        )
        .await
        .expect("occurrence materialization must succeed")
    {
        BackupScheduleOccurrenceMaterializationResult::Created(occurrence)
        | BackupScheduleOccurrenceMaterializationResult::Existing(occurrence) => occurrence,
        other => panic!("expected a materialized occurrence, got {other:?}"),
    }
}

async fn count_for_set(pool: &PgPool, table: &str, backup_set_id: BackupSetId) -> i64 {
    let query = format!("SELECT count(*) FROM {table} WHERE backup_set_id = $1");
    sqlx::query_scalar::<_, i64>(&query)
        .bind(backup_set_id.into_uuid())
        .fetch_one(pool)
        .await
        .expect("scoped count must succeed")
}

async fn count_table(pool: &PgPool, table: &str) -> i64 {
    let query = format!("SELECT count(*) FROM {table}");
    sqlx::query_scalar::<_, i64>(&query)
        .fetch_one(pool)
        .await
        .expect("table count must succeed")
}

async fn side_effect_counts(pool: &PgPool) -> [i64; 9] {
    [
        count_table(pool, "backup_snapshots").await,
        count_table(pool, "backup_snapshot_content_pins").await,
        count_table(pool, "backup_snapshot_expiry_plans").await,
        count_table(pool, "backup_snapshot_expiry_executions").await,
        count_table(pool, "backup_prune_plans").await,
        count_table(pool, "backup_prune_executions").await,
        count_table(pool, "object_gc_candidates").await,
        count_table(pool, "change_journal").await,
        count_table(pool, "device_sync_checkpoints").await,
    ]
}

fn outcome(result: BackupSchedulerTickResult) -> synveil_core::BackupSchedulerTickOutcome {
    result.outcome().expect("tick must have a non-idle outcome")
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_scheduler_tick_schema_is_current_on_postgresql_17() {
    let fixture = fixture("schema").await;
    let version: String = sqlx::query_scalar("SHOW server_version")
        .fetch_one(&fixture.inspection)
        .await
        .expect("server version query must succeed");
    assert!(
        version.starts_with("17."),
        "expected PostgreSQL 17, got {version}"
    );
    assert_eq!(
        count_table(&fixture.inspection, "_sqlx_migrations").await,
        34
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_scheduler_tick_returns_idle_without_schedule_or_future_work() {
    let fixture = fixture("idle").await;
    let scheduler = BackupSchedulerService::new(fixture.pool.clone());
    assert_eq!(
        scheduler
            .run_scheduler_tick(timestamp("2026-09-02T00:00:00Z"))
            .await
            .expect("idle tick must succeed"),
        BackupSchedulerTickResult::Idle
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrences",
            fixture.backup_set_id,
        )
        .await,
        0
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrence_handoffs",
            fixture.backup_set_id,
        )
        .await,
        0
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_maintenance_runs",
            fixture.backup_set_id,
        )
        .await,
        0
    );

    let (_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "future", "23:59").await;
    let future_scheduler = BackupSchedulerService::new(fixture.pool.clone());
    assert_eq!(
        future_scheduler
            .run_scheduler_tick(schedule.effective_from())
            .await
            .expect("future-only tick must succeed"),
        BackupSchedulerTickResult::Idle
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_scheduler_tick_materializes_and_handoffs_exactly_one_due_occurrence() {
    let fixture = fixture("due").await;
    configure_policy(&fixture, fixture.backup_set_id, "due").await;
    let (_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "due", "09:00").await;
    let (_date, planned) = first_planned(&schedule);
    let result = BackupSchedulerService::new(fixture.pool.clone())
        .run_scheduler_tick(observed_after(planned))
        .await
        .expect("due tick must succeed");
    let outcome = match result {
        BackupSchedulerTickResult::MaterializedAndHandedOff(outcome) => outcome,
        other => panic!("expected materialized-and-handed-off result, got {other:?}"),
    };
    assert_eq!(outcome.schedule_id(), schedule.id());
    assert_eq!(
        outcome.schedule_revision_id(),
        schedule.current_revision_id()
    );
    assert_eq!(outcome.backup_set_id(), fixture.backup_set_id);
    assert_eq!(outcome.scheduled_for_utc(), planned.scheduled_for_utc());
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrences",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrence_handoffs",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_maintenance_runs",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    let run = BackupService::new(fixture.pool.clone())
        .get_backup_maintenance_run(fixture.owner_user_id, outcome.maintenance_run_id())
        .await
        .expect("scheduler-created maintenance run must be readable");
    assert_eq!(run.state(), BackupMaintenanceRunState::Created);
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_scheduler_tick_recovers_materialized_unhanded_after_service_restart() {
    let fixture = fixture("restart").await;
    configure_policy(&fixture, fixture.backup_set_id, "restart").await;
    let (schedule_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "restart", "09:00").await;
    let (date, planned) = first_planned(&schedule);
    let occurrence = materialize_first(&fixture, &schedule_service, &schedule, date, planned).await;
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrence_handoffs",
            fixture.backup_set_id,
        )
        .await,
        0
    );

    let fresh_scheduler = BackupSchedulerService::new(fixture.pool.clone());
    let result = fresh_scheduler
        .run_scheduler_tick(observed_after(planned))
        .await
        .expect("restart tick must recover the unhanded occurrence");
    let outcome = match result {
        BackupSchedulerTickResult::HandedOffExisting(outcome) => outcome,
        other => panic!("expected existing-occurrence handoff, got {other:?}"),
    };
    assert_eq!(outcome.occurrence_id(), occurrence.id());
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrences",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrence_handoffs",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_maintenance_runs",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_scheduler_tick_does_not_duplicate_handoff_and_respects_next_due_state() {
    let fixture = fixture("replay").await;
    configure_policy(&fixture, fixture.backup_set_id, "replay").await;
    let (_schedule_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "replay", "09:00").await;
    let (_date, first) = first_planned(&schedule);
    let scheduler = BackupSchedulerService::new(fixture.pool.clone());
    let first_result = scheduler
        .run_scheduler_tick(observed_after(first))
        .await
        .expect("first tick must succeed");
    let first_outcome = outcome(first_result);
    assert!(matches!(
        first_result,
        BackupSchedulerTickResult::MaterializedAndHandedOff(_)
    ));

    assert_eq!(
        scheduler
            .run_scheduler_tick(observed_after(first))
            .await
            .expect("already-handed-off tick must succeed"),
        BackupSchedulerTickResult::Idle
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrences",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrence_handoffs",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_maintenance_runs",
            fixture.backup_set_id,
        )
        .await,
        1
    );

    let next = schedule
        .current_revision()
        .next_occurrence_after(first.scheduled_for_utc())
        .expect("daily schedule must have a next occurrence");
    assert!(next.scheduled_for_utc() > first.scheduled_for_utc());
    let completed = BackupService::new(fixture.pool.clone())
        .advance_backup_maintenance_run(fixture.owner_user_id, first_outcome.maintenance_run_id())
        .await
        .expect("explicit Prompt 49 advance must complete the first run");
    assert_eq!(completed.state(), BackupMaintenanceRunState::Completed);
    let second_result = BackupSchedulerService::new(fixture.pool.clone())
        .run_scheduler_tick(observed_after(next))
        .await
        .expect("the next due occurrence must succeed");
    let second_outcome = match second_result {
        BackupSchedulerTickResult::MaterializedAndHandedOff(outcome) => outcome,
        other => {
            panic!("expected next occurrence to be materialized and handed off, got {other:?}")
        }
    };
    assert_eq!(second_outcome.schedule_id(), schedule.id());
    assert_ne!(
        second_outcome.occurrence_id(),
        first_outcome.occurrence_id()
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrences",
            fixture.backup_set_id,
        )
        .await,
        2
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrence_handoffs",
            fixture.backup_set_id,
        )
        .await,
        2
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_maintenance_runs",
            fixture.backup_set_id,
        )
        .await,
        2
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_scheduler_tick_chooses_oldest_and_deterministic_tie_candidate() {
    let fixture = fixture("ordering").await;
    let first_set = fixture.backup_set_id;
    let second_set = fixture.create_active_backup_set("ordering-second").await;
    configure_policy(&fixture, first_set, "ordering-first").await;
    configure_policy(&fixture, second_set, "ordering-second").await;
    let (_first_service, _first_revision, first_schedule) =
        configure_schedule(&fixture, first_set, "ordering-first", "08:00").await;
    let (_second_service, _second_revision, second_schedule) =
        configure_schedule(&fixture, second_set, "ordering-second", "07:00").await;
    let (_first_date, first_occurrence_planned) = first_planned(&first_schedule);
    let (_second_date, second_occurrence_planned) = first_planned(&second_schedule);
    let latest_planned = if first_occurrence_planned.scheduled_for_utc()
        >= second_occurrence_planned.scheduled_for_utc()
    {
        first_occurrence_planned
    } else {
        second_occurrence_planned
    };
    let observed = observed_after(latest_planned);
    let expected_oldest = if first_occurrence_planned.scheduled_for_utc()
        < second_occurrence_planned.scheduled_for_utc()
    {
        first_schedule.id()
    } else if second_occurrence_planned.scheduled_for_utc()
        < first_occurrence_planned.scheduled_for_utc()
    {
        second_schedule.id()
    } else {
        first_schedule.id().min(second_schedule.id())
    };
    let first_tick = BackupSchedulerService::new(fixture.pool.clone())
        .run_scheduler_tick(observed)
        .await
        .expect("oldest candidate tick must succeed");
    assert_eq!(outcome(first_tick).schedule_id(), expected_oldest);

    let second_tick = BackupSchedulerService::new(fixture.pool.clone())
        .run_scheduler_tick(observed)
        .await
        .expect("second due candidate tick must succeed");
    assert_ne!(
        outcome(second_tick).occurrence_id(),
        outcome(first_tick).occurrence_id(),
        "two due schedules must not hand off the same occurrence"
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrences",
            first_set
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrences",
            second_set
        )
        .await,
        1
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_scheduler_tick_uses_stable_schedule_id_tie_break() {
    let fixture = fixture("tie").await;
    let first_set = fixture.backup_set_id;
    let second_set = fixture.create_active_backup_set("tie-second").await;
    configure_policy(&fixture, first_set, "tie-first").await;
    configure_policy(&fixture, second_set, "tie-second").await;
    let (_first_service, _first_revision, first_schedule) =
        configure_schedule(&fixture, first_set, "tie-first", "08:00").await;
    let (_second_service, _second_revision, second_schedule) =
        configure_schedule(&fixture, second_set, "tie-second", "08:00").await;
    let (_first_date, first_planned_occurrence) = first_planned(&first_schedule);
    let (_second_date, second_planned_occurrence) = first_planned(&second_schedule);
    assert_eq!(
        first_planned_occurrence.scheduled_for_utc(),
        second_planned_occurrence.scheduled_for_utc()
    );
    let result = BackupSchedulerService::new(fixture.pool.clone())
        .run_scheduler_tick(observed_after(first_planned_occurrence))
        .await
        .expect("tie candidate tick must succeed");
    assert_eq!(
        outcome(result).schedule_id(),
        first_schedule.id().min(second_schedule.id()),
        "same-instant schedules must use the stable schedule-ID tie-break"
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_scheduler_tick_skips_disabled_reenabled_and_superseded_unhanded_work() {
    let fixture = fixture("policy-fences").await;
    configure_policy(&fixture, fixture.backup_set_id, "policy-fences").await;
    let (service, first_revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "policy-fences", "09:00").await;
    let (first_date, first_occurrence_planned) = first_planned(&schedule);
    let old_occurrence = materialize_first(
        &fixture,
        &service,
        &schedule,
        first_date,
        first_occurrence_planned,
    )
    .await;

    service
        .set_backup_schedule_enabled(fixture.owner_user_id, fixture.backup_set_id, false)
        .await
        .expect("schedule disable must persist");
    let scheduler = BackupSchedulerService::new(fixture.pool.clone());
    assert_eq!(
        scheduler
            .run_scheduler_tick(observed_after(first_occurrence_planned))
            .await
            .expect("disabled-only tick must be idle"),
        BackupSchedulerTickResult::Idle
    );
    let schedule_after_disable = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .expect("disabled schedule must be readable");
    let reenabled_after_old_occurrence = old_occurrence
        .scheduled_for_utc()
        .checked_add_std(std::time::Duration::from_secs(1))
        .expect("reenable boundary must be representable")
        .max(
            schedule_after_disable
                .effective_from()
                .checked_add_std(std::time::Duration::from_micros(1))
                .expect("reenable must advance effective_from"),
        );
    sqlx::query(
        "UPDATE backup_schedules
         SET enabled = TRUE, effective_from = $1, updated_at = $1
         WHERE id = $2 AND owner_user_id = $3 AND enabled = FALSE",
    )
    .bind(reenabled_after_old_occurrence.as_offset_datetime())
    .bind(schedule.id().into_uuid())
    .bind(fixture.owner_user_id.into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("re-enable-after-occurrence transition must persist");
    assert_eq!(
        BackupSchedulerService::new(fixture.pool.clone())
            .run_scheduler_tick(observed_after(first_occurrence_planned))
            .await
            .expect("re-enabled old occurrence must remain skipped"),
        BackupSchedulerTickResult::Idle
    );
    assert_eq!(
        service
            .get_backup_schedule_occurrence_by_logical_key(
                fixture.owner_user_id,
                first_revision.id(),
                old_occurrence.local_calendar_date(),
            )
            .await
            .expect("old occurrence lookup must succeed")
            .expect("old occurrence must remain durable")
            .id(),
        old_occurrence.id()
    );

    let revision_set = fixture.create_active_backup_set("revision-fence").await;
    configure_policy(&fixture, revision_set, "revision-fence").await;
    let (revision_service, _old_revision, revision_schedule) =
        configure_schedule(&fixture, revision_set, "revision-fence", "09:00").await;
    let (revision_date, revision_planned) = first_planned(&revision_schedule);
    let superseded_occurrence = materialize_first(
        &fixture,
        &revision_service,
        &revision_schedule,
        revision_date,
        revision_planned,
    )
    .await;
    let new_revision = revision_service
        .configure_backup_schedule(
            fixture.owner_user_id,
            "schedule-policy-fences-edit".to_owned(),
            revision_set,
            daily("11:00"),
        )
        .await
        .expect("schedule edit must persist");
    let current = revision_service
        .get_backup_schedule(fixture.owner_user_id, revision_set)
        .await
        .expect("edited schedule must be readable");
    let current = pin_fixed_effective_from(&fixture, current).await;
    let (_new_date, new_occurrence_planned) = first_planned(&current);
    let new_result = BackupSchedulerService::new(fixture.pool.clone())
        .run_scheduler_tick(observed_after(new_occurrence_planned))
        .await
        .expect("current revision due work must succeed");
    let new_outcome = outcome(new_result);
    assert_eq!(new_outcome.schedule_revision_id(), new_revision.id());
    assert_ne!(new_outcome.occurrence_id(), superseded_occurrence.id());
    assert_eq!(
        revision_service
            .get_handoff_for_occurrence(fixture.owner_user_id, superseded_occurrence.id())
            .await
            .expect("old handoff lookup must succeed"),
        None
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrence_handoffs",
            revision_set,
        )
        .await,
        1
    );
    let _ = scheduler;
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_scheduler_tick_concurrent_same_schedule_is_exactly_once() {
    let fixture = fixture("concurrent-one").await;
    configure_policy(&fixture, fixture.backup_set_id, "concurrent-one").await;
    let (_schedule_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "concurrent-one", "09:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed_at = observed_after(planned);
    let mut handles = Vec::new();
    for _ in 0..12 {
        let service = BackupSchedulerService::new(fixture.pool.clone());
        handles.push(tokio::spawn(async move {
            service.run_scheduler_tick(observed_at).await
        }));
    }
    let mut outcomes = Vec::new();
    for handle in handles {
        let result = handle.await.expect("concurrent tick task must not panic");
        outcomes.push(outcome(result.expect("concurrent tick must converge")));
    }
    assert!(
        outcomes
            .windows(2)
            .all(|pair| pair[0].occurrence_id() == pair[1].occurrence_id())
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrences",
            fixture.backup_set_id
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrence_handoffs",
            fixture.backup_set_id
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_maintenance_runs",
            fixture.backup_set_id
        )
        .await,
        1
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_scheduler_tick_concurrent_multiple_schedules_has_no_duplicates() {
    let fixture = fixture("concurrent-many").await;
    let first_set = fixture.backup_set_id;
    let second_set = fixture.create_active_backup_set("concurrent-second").await;
    configure_policy(&fixture, first_set, "concurrent-first").await;
    configure_policy(&fixture, second_set, "concurrent-second").await;
    let (_first_service, _first_revision, first_schedule) =
        configure_schedule(&fixture, first_set, "concurrent-first", "08:00").await;
    let (_second_service, _second_revision, second_schedule) =
        configure_schedule(&fixture, second_set, "concurrent-second", "09:00").await;
    let (_first_date, first_occurrence_planned) = first_planned(&first_schedule);
    let (_second_date, second_occurrence_planned) = first_planned(&second_schedule);
    let latest_planned = if first_occurrence_planned.scheduled_for_utc()
        >= second_occurrence_planned.scheduled_for_utc()
    {
        first_occurrence_planned
    } else {
        second_occurrence_planned
    };
    let observed_at = observed_after(latest_planned);
    let mut handles = Vec::new();
    for _ in 0..12 {
        let service = BackupSchedulerService::new(fixture.pool.clone());
        handles.push(tokio::spawn(async move {
            service.run_scheduler_tick(observed_at).await
        }));
    }
    for handle in handles {
        let result = handle.await.expect("concurrent tick task must not panic");
        result.expect("concurrent multi-schedule tick must remain bounded");
    }
    for backup_set_id in [first_set, second_set] {
        let occurrences = count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrences",
            backup_set_id,
        )
        .await;
        let handoffs = count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrence_handoffs",
            backup_set_id,
        )
        .await;
        let runs = count_for_set(
            &fixture.inspection,
            "backup_maintenance_runs",
            backup_set_id,
        )
        .await;
        assert!(
            occurrences <= 1,
            "one tick cannot duplicate a schedule occurrence"
        );
        assert_eq!(handoffs, occurrences);
        assert_eq!(runs, occurrences);
    }
    assert!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrences",
            first_set
        )
        .await
            + count_for_set(
                &fixture.inspection,
                "backup_schedule_occurrences",
                second_set
            )
            .await
            >= 1
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_misfire_global_concurrent_replay_latest_and_expired_actions_are_durable() {
    let fixture = fixture("misfire-concurrent-mixed-policies").await;
    let expired_set = fixture.backup_set_id;
    let replay_set = fixture
        .create_active_backup_set("misfire-concurrent-replay")
        .await;
    let latest_set = fixture
        .create_active_backup_set("misfire-concurrent-latest")
        .await;
    for (backup_set_id, label) in [
        (expired_set, "misfire-concurrent-expired"),
        (replay_set, "misfire-concurrent-replay"),
        (latest_set, "misfire-concurrent-latest"),
    ] {
        configure_policy(&fixture, backup_set_id, label).await;
    }
    let (_, _, expired_schedule) = configure_schedule_with_policy(
        &fixture,
        expired_set,
        "misfire-concurrent-expired",
        "09:00",
        BackupScheduleMisfireMode::LatestOnly,
        60,
    )
    .await;
    let (_, _, replay_schedule) = configure_schedule_with_policy(
        &fixture,
        replay_set,
        "misfire-concurrent-replay",
        "10:00",
        BackupScheduleMisfireMode::ReplayOneByOne,
        604_800,
    )
    .await;
    let (_, _, latest_schedule) = configure_schedule_with_policy(
        &fixture,
        latest_set,
        "misfire-concurrent-latest",
        "11:00",
        BackupScheduleMisfireMode::LatestOnly,
        604_800,
    )
    .await;
    let expired_due = planned_sequence(&expired_schedule, 1)[0];
    let replay_due = planned_sequence(&replay_schedule, 1)[0];
    let latest_due = planned_sequence(&latest_schedule, 1)[0];
    assert!(expired_due.scheduled_for_utc() < replay_due.scheduled_for_utc());
    assert!(replay_due.scheduled_for_utc() < latest_due.scheduled_for_utc());
    let observed = observed_after(latest_due);

    let mut handles = Vec::new();
    for _ in 0..12 {
        let scheduler = BackupSchedulerService::new(fixture.pool.clone());
        handles.push(tokio::spawn(async move {
            scheduler.run_scheduler_tick(observed).await
        }));
    }
    for handle in handles {
        handle
            .await
            .expect("concurrent mixed-policy tick must not panic")
            .expect("concurrent mixed-policy tick must not deadlock or fail");
    }

    // A caller may discover the next global action after another caller has
    // committed. A fixed number of explicit ticks settles all three one-action
    // decisions without adding any production loop.
    let scheduler = BackupSchedulerService::new(fixture.pool.clone());
    for _ in 0..3 {
        scheduler
            .run_scheduler_tick(observed)
            .await
            .expect("bounded follow-up tick must converge");
    }
    assert_eq!(
        scheduler.run_scheduler_tick(observed).await.unwrap(),
        BackupSchedulerTickResult::Idle
    );

    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_misfire_skips",
            expired_set,
        )
        .await,
        1
    );
    for table in [
        "backup_schedule_occurrences",
        "backup_schedule_occurrence_handoffs",
        "backup_maintenance_runs",
    ] {
        assert_eq!(
            count_for_set(&fixture.inspection, table, expired_set).await,
            0
        );
        assert_eq!(
            count_for_set(&fixture.inspection, table, replay_set).await,
            1
        );
        assert_eq!(
            count_for_set(&fixture.inspection, table, latest_set).await,
            1
        );
    }
    for backup_set_id in [replay_set, latest_set] {
        assert_eq!(
            count_for_set(
                &fixture.inspection,
                "backup_schedule_misfire_skips",
                backup_set_id,
            )
            .await,
            0
        );
    }
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_scheduler_tick_has_no_snapshot_expiry_prune_gc_journal_or_sync_side_effects() {
    let fixture = fixture("side-effects").await;
    configure_policy(&fixture, fixture.backup_set_id, "side-effects").await;
    let (_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "side-effects", "09:00").await;
    let (_date, planned) = first_planned(&schedule);
    let before = side_effect_counts(&fixture.inspection).await;
    let result = BackupSchedulerService::new(fixture.pool.clone())
        .run_scheduler_tick(observed_after(planned))
        .await
        .expect("side-effect audit tick must succeed");
    assert!(matches!(
        result,
        BackupSchedulerTickResult::MaterializedAndHandedOff(_)
    ));
    let after = side_effect_counts(&fixture.inspection).await;
    assert_eq!(after, before);
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrences",
            fixture.backup_set_id
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrence_handoffs",
            fixture.backup_set_id
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_maintenance_runs",
            fixture.backup_set_id
        )
        .await,
        1
    );
    fixture.close().await;
}

fn planned_sequence(
    schedule: &BackupSchedule,
    count: usize,
) -> Vec<synveil_core::PlannedScheduleOccurrence> {
    let first = schedule
        .current_revision()
        .next_occurrence_after(schedule.effective_from())
        .expect("the first activation occurrence must remain representable");
    let mut values = vec![first];
    while values.len() < count {
        values.push(
            schedule
                .current_revision()
                .next_occurrence_after(values.last().unwrap().scheduled_for_utc())
                .expect("daily sequence must remain representable"),
        );
    }
    values
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_misfire_replay_processes_oldest_eligible_one_per_tick() {
    let fixture = fixture("misfire-replay").await;
    configure_policy(&fixture, fixture.backup_set_id, "misfire-replay").await;
    let (_, _, schedule) = configure_schedule_with_policy(
        &fixture,
        fixture.backup_set_id,
        "misfire-replay",
        "09:00",
        BackupScheduleMisfireMode::ReplayOneByOne,
        604_800,
    )
    .await;
    let planned = planned_sequence(&schedule, 3);
    let observed = observed_after(planned[2]);
    let scheduler = BackupSchedulerService::new(fixture.pool.clone());

    for expected in &planned {
        let result = scheduler
            .run_scheduler_tick(observed)
            .await
            .expect("replay tick must succeed");
        let outcome = outcome(result);
        assert_eq!(outcome.scheduled_for_utc(), expected.scheduled_for_utc());
        BackupService::new(fixture.pool.clone())
            .advance_backup_maintenance_run(fixture.owner_user_id, outcome.maintenance_run_id())
            .await
            .expect("test must complete the active run before the next replay tick");
    }
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrence_handoffs",
            fixture.backup_set_id,
        )
        .await,
        3
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_misfire_skips",
            fixture.backup_set_id,
        )
        .await,
        0
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_misfire_latest_only_collapses_backlog_and_second_tick_is_idle() {
    let fixture = fixture("misfire-latest").await;
    configure_policy(&fixture, fixture.backup_set_id, "misfire-latest").await;
    let (_, _, schedule) = configure_schedule_with_policy(
        &fixture,
        fixture.backup_set_id,
        "misfire-latest",
        "09:00",
        BackupScheduleMisfireMode::LatestOnly,
        604_800,
    )
    .await;
    let planned = planned_sequence(&schedule, 3);
    let observed = observed_after(planned[2]);
    let scheduler = BackupSchedulerService::new(fixture.pool.clone());
    let result = scheduler.run_scheduler_tick(observed).await.unwrap();
    assert_eq!(
        outcome(result).scheduled_for_utc(),
        planned[2].scheduled_for_utc()
    );
    assert_eq!(
        scheduler.run_scheduler_tick(observed).await.unwrap(),
        BackupSchedulerTickResult::Idle
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrences",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrence_handoffs",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_misfire_fully_expired_is_one_restart_safe_immutable_skip() {
    let fixture = fixture("misfire-expired").await;
    let (_, _, schedule) = configure_schedule_with_policy(
        &fixture,
        fixture.backup_set_id,
        "misfire-expired",
        "09:00",
        BackupScheduleMisfireMode::LatestOnly,
        60,
    )
    .await;
    let planned = planned_sequence(&schedule, 2);
    let observed = planned[1]
        .scheduled_for_utc()
        .checked_add_std(std::time::Duration::from_secs(61))
        .unwrap();
    let side_effects_before = side_effect_counts(&fixture.inspection).await;
    let result = BackupSchedulerService::new(fixture.pool.clone())
        .run_scheduler_tick(observed)
        .await
        .unwrap();
    let skip = match result {
        BackupSchedulerTickResult::SkippedExpired(value) => value,
        other => panic!("expected expired skip, got {other:?}"),
    };
    assert_eq!(skip.resolved_through_utc(), planned[1].scheduled_for_utc());
    assert_eq!(skip.misfire_mode(), BackupScheduleMisfireMode::LatestOnly);
    assert_eq!(
        BackupSchedulerService::new(fixture.pool.clone())
            .run_scheduler_tick(observed)
            .await
            .unwrap(),
        BackupSchedulerTickResult::Idle
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_misfire_skips",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    for table in [
        "backup_schedule_occurrences",
        "backup_schedule_occurrence_handoffs",
        "backup_maintenance_runs",
    ] {
        assert_eq!(
            count_for_set(&fixture.inspection, table, fixture.backup_set_id).await,
            0
        );
    }
    assert_eq!(
        side_effect_counts(&fixture.inspection).await,
        side_effects_before
    );
    assert!(
        sqlx::query("UPDATE backup_schedule_misfire_skips SET observed_at_utc = clock_timestamp() WHERE backup_set_id = $1")
            .bind(fixture.backup_set_id.into_uuid())
            .execute(&fixture.inspection)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM backup_schedule_misfire_skips WHERE backup_set_id = $1")
            .bind(fixture.backup_set_id.into_uuid())
            .execute(&fixture.inspection)
            .await
            .is_err()
    );
    let forged_policy = sqlx::query(
        "INSERT INTO backup_schedule_misfire_skips
            (id, owner_user_id, backup_set_id, schedule_id,
             schedule_revision_id, activation_effective_from,
             resolved_from_exclusive_utc, resolved_through_utc,
             observed_at_utc, misfire_mode, max_lateness_seconds, created_at)
         SELECT $1, owner_user_id, backup_set_id, schedule_id,
                schedule_revision_id, activation_effective_from,
                resolved_through_utc, resolved_through_utc + INTERVAL '1 second',
                observed_at_utc, 'REPLAY_ONE_BY_ONE', max_lateness_seconds, created_at
         FROM backup_schedule_misfire_skips WHERE backup_set_id = $2",
    )
    .bind(synveil_core::BackupScheduleMisfireSkipId::new().into_uuid())
    .bind(fixture.backup_set_id.into_uuid())
    .execute(&fixture.inspection)
    .await;
    assert!(
        forged_policy.is_err(),
        "skip policy snapshot must be enforced"
    );
    let non_monotonic = sqlx::query(
        "INSERT INTO backup_schedule_misfire_skips
            (id, owner_user_id, backup_set_id, schedule_id,
             schedule_revision_id, activation_effective_from,
             resolved_from_exclusive_utc, resolved_through_utc,
             observed_at_utc, misfire_mode, max_lateness_seconds, created_at)
         SELECT $1, owner_user_id, backup_set_id, schedule_id,
                schedule_revision_id, activation_effective_from,
                activation_effective_from, resolved_through_utc + INTERVAL '1 second',
                observed_at_utc, misfire_mode, max_lateness_seconds, created_at
         FROM backup_schedule_misfire_skips WHERE backup_set_id = $2",
    )
    .bind(synveil_core::BackupScheduleMisfireSkipId::new().into_uuid())
    .bind(fixture.backup_set_id.into_uuid())
    .execute(&fixture.inspection)
    .await;
    assert!(
        non_monotonic.is_err(),
        "skip progress must not move backward"
    );
    let foreign_scope = sqlx::query(
        "INSERT INTO backup_schedule_misfire_skips
            (id, owner_user_id, backup_set_id, schedule_id,
             schedule_revision_id, activation_effective_from,
             resolved_from_exclusive_utc, resolved_through_utc,
             observed_at_utc, misfire_mode, max_lateness_seconds, created_at)
         SELECT $1, $2, backup_set_id, schedule_id,
                schedule_revision_id, activation_effective_from,
                resolved_through_utc, resolved_through_utc + INTERVAL '1 second',
                observed_at_utc, misfire_mode, max_lateness_seconds, created_at
         FROM backup_schedule_misfire_skips WHERE backup_set_id = $3",
    )
    .bind(synveil_core::BackupScheduleMisfireSkipId::new().into_uuid())
    .bind(UserId::new().into_uuid())
    .bind(fixture.backup_set_id.into_uuid())
    .execute(&fixture.inspection)
    .await;
    assert!(foreign_scope.is_err(), "skip owner scope must be enforced");
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_misfire_replay_skips_expired_prefix_then_replays_eligible() {
    let fixture = fixture("misfire-mixed-replay").await;
    configure_policy(&fixture, fixture.backup_set_id, "misfire-mixed-replay").await;
    let (_, _, schedule) = configure_schedule_with_policy(
        &fixture,
        fixture.backup_set_id,
        "misfire-mixed-replay",
        "09:00",
        BackupScheduleMisfireMode::ReplayOneByOne,
        172_800,
    )
    .await;
    let planned = planned_sequence(&schedule, 4);
    let observed = observed_after(planned[3]);
    let scheduler = BackupSchedulerService::new(fixture.pool.clone());
    let skipped = scheduler.run_scheduler_tick(observed).await.unwrap();
    let skip = skipped
        .skip_outcome()
        .expect("first action must skip expired prefix");
    assert_eq!(skip.resolved_through_utc(), planned[1].scheduled_for_utc());
    for expected in &planned[2..] {
        let result = scheduler.run_scheduler_tick(observed).await.unwrap();
        let handoff = outcome(result);
        assert_eq!(handoff.scheduled_for_utc(), expected.scheduled_for_utc());
        BackupService::new(fixture.pool.clone())
            .advance_backup_maintenance_run(fixture.owner_user_id, handoff.maintenance_run_id())
            .await
            .unwrap();
    }
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_misfire_skips",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_misfire_materialized_evidence_obeys_replay_and_latest_policies() {
    let fixture = fixture("misfire-materialized").await;
    configure_policy(&fixture, fixture.backup_set_id, "misfire-materialized").await;
    let (service, _, schedule) = configure_schedule_with_policy(
        &fixture,
        fixture.backup_set_id,
        "misfire-materialized",
        "09:00",
        BackupScheduleMisfireMode::LatestOnly,
        604_800,
    )
    .await;
    let planned = planned_sequence(&schedule, 3);
    let mut materialized = Vec::new();
    for target in &planned {
        materialized.push(
            materialize_first(
                &fixture,
                &service,
                &schedule,
                target.local_calendar_date(),
                *target,
            )
            .await,
        );
    }
    let result = BackupSchedulerService::new(fixture.pool.clone())
        .run_scheduler_tick(observed_after(planned[2]))
        .await
        .unwrap();
    let selected = outcome(result);
    assert_eq!(selected.occurrence_id(), materialized[2].id());
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrence_handoffs",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_misfire_exact_cutoff_is_eligible_and_before_cutoff_expires() {
    let cutoff_fixture = fixture("misfire-cutoff").await;
    configure_policy(
        &cutoff_fixture,
        cutoff_fixture.backup_set_id,
        "misfire-cutoff",
    )
    .await;
    let (_, _, schedule) = configure_schedule_with_policy(
        &cutoff_fixture,
        cutoff_fixture.backup_set_id,
        "misfire-cutoff",
        "09:00",
        BackupScheduleMisfireMode::LatestOnly,
        60,
    )
    .await;
    let planned = planned_sequence(&schedule, 1)[0];
    let exact = planned
        .scheduled_for_utc()
        .checked_add_std(std::time::Duration::from_secs(60))
        .unwrap();
    let result = BackupSchedulerService::new(cutoff_fixture.pool.clone())
        .run_scheduler_tick(exact)
        .await
        .unwrap();
    assert_eq!(
        outcome(result).scheduled_for_utc(),
        planned.scheduled_for_utc()
    );
    cutoff_fixture.close().await;

    let expired_fixture = fixture("misfire-cutoff-before").await;
    let (_, _, expired_schedule) = configure_schedule_with_policy(
        &expired_fixture,
        expired_fixture.backup_set_id,
        "misfire-cutoff-before",
        "09:00",
        BackupScheduleMisfireMode::LatestOnly,
        60,
    )
    .await;
    let expired = planned_sequence(&expired_schedule, 1)[0];
    let after = expired
        .scheduled_for_utc()
        .checked_add_std(std::time::Duration::from_micros(60_000_001))
        .unwrap();
    assert!(matches!(
        BackupSchedulerService::new(expired_fixture.pool.clone())
            .run_scheduler_tick(after)
            .await
            .unwrap(),
        BackupSchedulerTickResult::SkippedExpired(_)
    ));
    expired_fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_misfire_twelve_concurrent_expired_ticks_converge() {
    let fixture = fixture("misfire-concurrent-skip").await;
    let (_, _, schedule) = configure_schedule_with_policy(
        &fixture,
        fixture.backup_set_id,
        "misfire-concurrent-skip",
        "09:00",
        BackupScheduleMisfireMode::LatestOnly,
        60,
    )
    .await;
    let due = planned_sequence(&schedule, 2)[1];
    let observed = due
        .scheduled_for_utc()
        .checked_add_std(std::time::Duration::from_secs(61))
        .unwrap();
    let mut handles = Vec::new();
    for _ in 0..12 {
        let scheduler = BackupSchedulerService::new(fixture.pool.clone());
        handles.push(tokio::spawn(async move {
            scheduler.run_scheduler_tick(observed).await
        }));
    }
    for handle in handles {
        let result = handle.await.unwrap().unwrap();
        assert!(matches!(
            result,
            BackupSchedulerTickResult::SkippedExpired(_) | BackupSchedulerTickResult::Idle
        ));
    }
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_misfire_skips",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_maintenance_runs",
            fixture.backup_set_id,
        )
        .await,
        0
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_misfire_skip_vs_handoff_race_commits_one_progress_authority() {
    let fixture = fixture("misfire-skip-handoff-race").await;
    configure_policy(&fixture, fixture.backup_set_id, "misfire-skip-handoff-race").await;
    let (_, _, schedule) = configure_schedule_with_policy(
        &fixture,
        fixture.backup_set_id,
        "misfire-skip-handoff-race",
        "09:00",
        BackupScheduleMisfireMode::LatestOnly,
        60,
    )
    .await;
    let due = planned_sequence(&schedule, 1)[0];
    let eligible_observed = due
        .scheduled_for_utc()
        .checked_add_std(std::time::Duration::from_secs(60))
        .unwrap();
    let expired_observed = due
        .scheduled_for_utc()
        .checked_add_std(std::time::Duration::from_secs(61))
        .unwrap();
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
    let eligible_scheduler = BackupSchedulerService::new(fixture.pool.clone());
    let eligible_barrier = barrier.clone();
    let handoff_attempt = tokio::spawn(async move {
        eligible_barrier.wait().await;
        eligible_scheduler
            .run_scheduler_tick(eligible_observed)
            .await
    });
    let expired_scheduler = BackupSchedulerService::new(fixture.pool.clone());
    let skip_attempt = tokio::spawn(async move {
        barrier.wait().await;
        expired_scheduler.run_scheduler_tick(expired_observed).await
    });
    let handoff_result = handoff_attempt.await.expect("handoff task must not panic");
    let skip_result = skip_attempt.await.expect("skip task must not panic");
    for result in [&handoff_result, &skip_result] {
        assert!(matches!(
            result,
            Ok(BackupSchedulerTickResult::Idle
                | BackupSchedulerTickResult::SkippedExpired(_)
                | BackupSchedulerTickResult::HandedOffExisting(_)
                | BackupSchedulerTickResult::MaterializedAndHandedOff(_))
                | Err(BackupSchedulerError::Handoff(
                    BackupScheduleHandoffError::NotEffective(
                        BackupScheduleOccurrenceNotEffectiveReason::ExpiredForAutomaticExecution
                            | BackupScheduleOccurrenceNotEffectiveReason::SchedulerProgressResolved
                    )
                ))
        ));
    }

    let skips = count_for_set(
        &fixture.inspection,
        "backup_schedule_misfire_skips",
        fixture.backup_set_id,
    )
    .await;
    let handoffs = count_for_set(
        &fixture.inspection,
        "backup_schedule_occurrence_handoffs",
        fixture.backup_set_id,
    )
    .await;
    let runs = count_for_set(
        &fixture.inspection,
        "backup_maintenance_runs",
        fixture.backup_set_id,
    )
    .await;
    assert_eq!(skips + handoffs, 1, "exactly one progress path may win");
    assert_eq!(runs, handoffs, "skip must never create a maintenance run");
    assert!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrences",
            fixture.backup_set_id,
        )
        .await
            <= 1
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_misfire_expired_materialized_is_preserved_not_handed_off() {
    let fixture = fixture("misfire-materialized-expired").await;
    let (service, _, schedule) = configure_schedule_with_policy(
        &fixture,
        fixture.backup_set_id,
        "misfire-materialized-expired",
        "09:00",
        BackupScheduleMisfireMode::ReplayOneByOne,
        60,
    )
    .await;
    let planned = planned_sequence(&schedule, 1)[0];
    let occurrence = materialize_first(
        &fixture,
        &service,
        &schedule,
        planned.local_calendar_date(),
        planned,
    )
    .await;
    let observed = planned
        .scheduled_for_utc()
        .checked_add_std(std::time::Duration::from_secs(61))
        .unwrap();
    assert!(matches!(
        BackupSchedulerService::new(fixture.pool.clone())
            .run_scheduler_tick(observed)
            .await
            .unwrap(),
        BackupSchedulerTickResult::SkippedExpired(_)
    ));
    assert_eq!(
        service
            .get_handoff_for_occurrence(fixture.owner_user_id, occurrence.id())
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrences",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_maintenance_runs",
            fixture.backup_set_id,
        )
        .await,
        0
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_misfire_replay_recovers_oldest_materialized_eligible_occurrence() {
    let fixture = fixture("misfire-materialized-replay").await;
    configure_policy(
        &fixture,
        fixture.backup_set_id,
        "misfire-materialized-replay",
    )
    .await;
    let (service, _, schedule) = configure_schedule_with_policy(
        &fixture,
        fixture.backup_set_id,
        "misfire-materialized-replay",
        "09:00",
        BackupScheduleMisfireMode::ReplayOneByOne,
        604_800,
    )
    .await;
    let planned = planned_sequence(&schedule, 3);
    let occurrence = materialize_first(
        &fixture,
        &service,
        &schedule,
        planned[0].local_calendar_date(),
        planned[0],
    )
    .await;
    let result = BackupSchedulerService::new(fixture.pool.clone())
        .run_scheduler_tick(observed_after(planned[2]))
        .await
        .unwrap();
    match result {
        BackupSchedulerTickResult::HandedOffExisting(outcome) => {
            assert_eq!(outcome.occurrence_id(), occurrence.id());
            assert_eq!(outcome.scheduled_for_utc(), planned[0].scheduled_for_utc());
        }
        other => panic!("expected existing oldest replay occurrence, got {other:?}"),
    }
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_misfire_mixed_latest_handoffs_newest_without_cleanup_rows() {
    let fixture = fixture("misfire-mixed-latest").await;
    configure_policy(&fixture, fixture.backup_set_id, "misfire-mixed-latest").await;
    let (_, _, schedule) = configure_schedule_with_policy(
        &fixture,
        fixture.backup_set_id,
        "misfire-mixed-latest",
        "09:00",
        BackupScheduleMisfireMode::LatestOnly,
        172_800,
    )
    .await;
    let planned = planned_sequence(&schedule, 4);
    let result = BackupSchedulerService::new(fixture.pool.clone())
        .run_scheduler_tick(observed_after(planned[3]))
        .await
        .unwrap();
    assert_eq!(
        outcome(result).scheduled_for_utc(),
        planned[3].scheduled_for_utc()
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_occurrences",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_misfire_skips",
            fixture.backup_set_id,
        )
        .await,
        0
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_misfire_disable_vs_skip_race_has_only_serialized_outcomes() {
    let fixture = fixture("misfire-disable-skip-race").await;
    let (schedule_service, _, schedule) = configure_schedule_with_policy(
        &fixture,
        fixture.backup_set_id,
        "misfire-disable-skip-race",
        "09:00",
        BackupScheduleMisfireMode::LatestOnly,
        60,
    )
    .await;
    let due = planned_sequence(&schedule, 1)[0];
    let observed = due
        .scheduled_for_utc()
        .checked_add_std(std::time::Duration::from_secs(61))
        .unwrap();
    let scheduler = BackupSchedulerService::new(fixture.pool.clone());
    let owner = fixture.owner_user_id;
    let set_id = fixture.backup_set_id;
    let tick = tokio::spawn(async move { scheduler.run_scheduler_tick(observed).await });
    let disable = tokio::spawn(async move {
        schedule_service
            .set_backup_schedule_enabled(owner, set_id, false)
            .await
    });
    let tick_result = tick.await.unwrap().unwrap();
    let disabled = disable.await.unwrap().unwrap();
    assert!(!disabled.enabled());
    assert!(matches!(
        tick_result,
        BackupSchedulerTickResult::SkippedExpired(_) | BackupSchedulerTickResult::Idle
    ));
    assert!(
        count_for_set(
            &fixture.inspection,
            "backup_schedule_misfire_skips",
            fixture.backup_set_id,
        )
        .await
            <= 1
    );
    assert_eq!(
        count_for_set(
            &fixture.inspection,
            "backup_maintenance_runs",
            fixture.backup_set_id,
        )
        .await,
        0
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_misfire_policy_edit_vs_skip_never_mixes_activation_epochs() {
    let fixture = fixture("misfire-edit-skip-race").await;
    let (schedule_service, old_revision, schedule) = configure_schedule_with_policy(
        &fixture,
        fixture.backup_set_id,
        "misfire-edit-skip-race",
        "09:00",
        BackupScheduleMisfireMode::LatestOnly,
        60,
    )
    .await;
    let due = planned_sequence(&schedule, 1)[0];
    let observed = due
        .scheduled_for_utc()
        .checked_add_std(std::time::Duration::from_secs(61))
        .unwrap();
    let scheduler = BackupSchedulerService::new(fixture.pool.clone());
    let owner = fixture.owner_user_id;
    let set_id = fixture.backup_set_id;
    let tick = tokio::spawn(async move { scheduler.run_scheduler_tick(observed).await });
    let edit = tokio::spawn(async move {
        schedule_service
            .configure_backup_schedule(
                owner,
                format!("misfire-edit-race-{set_id}"),
                set_id,
                daily_with_policy("09:00", BackupScheduleMisfireMode::ReplayOneByOne, 172_800),
            )
            .await
    });
    let tick_result = tick.await.unwrap().unwrap();
    let new_revision = edit.await.unwrap().unwrap();
    assert_ne!(new_revision.id(), old_revision.id());
    assert!(matches!(
        tick_result,
        BackupSchedulerTickResult::SkippedExpired(_) | BackupSchedulerTickResult::Idle
    ));
    let skip_revisions = sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT schedule_revision_id FROM backup_schedule_misfire_skips
         WHERE backup_set_id = $1",
    )
    .bind(fixture.backup_set_id.into_uuid())
    .fetch_all(&fixture.inspection)
    .await
    .unwrap();
    assert!(
        skip_revisions
            .iter()
            .all(|revision| *revision == old_revision.id().into_uuid())
    );
    fixture.close().await;
}
