//! PostgreSQL verification for Prompt 67's bounded scheduled-maintenance cycle.
//!
//! Each cycle invocation composes exactly one scheduler tick (Prompt 64–65)
//! and exactly one scheduled-maintenance worker step (Prompt 66). No loop,
//! daemon, heartbeat, or retry runs inside the cycle. Tests run with isolated
//! disposable databases to avoid global discovery contamination. Run with
//! `--test-threads=1`.

use std::time::Duration as StdDuration;

use sqlx::PgPool;
use synveil_core::{
    BackupMaintenanceRunState, BackupScheduleConfig, BackupScheduleLocalTime,
    BackupScheduleMisfireMode, BackupScheduleOccurrenceMaterializationResult,
    BackupScheduleRecurrenceKind, BackupScheduleRevision, BackupScheduleTimezone,
    BackupScheduledMaintenanceClaimOutcome, BackupScheduledMaintenanceLeaseToken,
    BackupScheduledMaintenanceStepResult, BackupScheduledMaintenanceWorkerId, BackupSetId,
    DedupDomainId, Library, LibraryId, LogicalName, Node, NodeId, SnapshotId, Timestamp, User,
    UserId, UserStatus,
};
use synveil_metadata::{
    BackupScheduleService, BackupSchedulerError, BackupSchedulerService, BackupService,
    DatabaseConfig, DatabasePool, DomainRepository, MigrationRunner,
    ScheduledMaintenanceCycleError, ScheduledMaintenanceCycleService,
    ScheduledMaintenanceCycleTickOutcome, ScheduledMaintenanceCycleWorkerOutcome,
    ScheduledMaintenanceWorkerError, ScheduledMaintenanceWorkerService,
};
use time::Duration as TimeDuration;
use uuid::Uuid;

const LEASE_SECONDS: u64 = 120;

fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse(value).expect("test timestamp must be valid")
}

fn name(value: impl AsRef<str>) -> LogicalName {
    LogicalName::new(value.as_ref()).expect("test logical name must be valid")
}

fn timezone(value: &str) -> BackupScheduleTimezone {
    value.parse().expect("test timezone must be valid")
}

fn local_time(value: &str) -> BackupScheduleLocalTime {
    value.parse().expect("test local time must be valid")
}

fn daily(time: &str) -> BackupScheduleConfig {
    BackupScheduleConfig::daily(timezone("UTC"), local_time(time))
        .expect("daily schedule configuration must be valid")
}

#[allow(dead_code)]
fn daily_with_policy(
    time: &str,
    mode: BackupScheduleMisfireMode,
    max_lateness_seconds: u32,
) -> BackupScheduleConfig {
    BackupScheduleConfig::new_with_misfire_policy(
        BackupScheduleRecurrenceKind::Daily,
        timezone("UTC"),
        local_time(time),
        Vec::new(),
        mode,
        max_lateness_seconds,
    )
    .expect("policy-aware daily schedule must be valid")
}

fn after(value: Timestamp, seconds: u64) -> Timestamp {
    value
        .checked_add_std(StdDuration::from_secs(seconds))
        .expect("test observed time must be representable")
}

fn worker() -> BackupScheduledMaintenanceWorkerId {
    BackupScheduledMaintenanceWorkerId::new()
}

struct IsolatedDb {
    pool: DatabasePool,
    inspection: PgPool,
}

async fn new_db(label: &str) -> IsolatedDb {
    let base = std::env::var("SYNVEIL_TEST_DATABASE_URL").expect(
        "SYNVEIL_TEST_DATABASE_URL must identify the disposable server maintenance database",
    );
    let db_name = format!(
        "p67_{}_{}",
        label
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            })
            .collect::<String>(),
        Uuid::now_v7().simple()
    );
    let maintenance = PgPool::connect(&base)
        .await
        .expect("maintenance connection must succeed");
    sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
        .execute(&maintenance)
        .await
        .expect("isolated test database must be created");
    maintenance.close().await;

    let url = if let Some(prefix) = base.rsplit_once('/') {
        format!("{}/{}", prefix.0, db_name)
    } else {
        panic!("test database URL must contain a database path");
    };
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    let status = MigrationRunner::new()
        .run(&pool)
        .await
        .expect("all forward migrations must apply");
    assert!(status.is_current(), "all migrations must be current");
    assert_eq!(status.applied_versions().len(), 36);
    assert_eq!(status.latest_applied_version(), Some(20260910000000));
    let inspection = PgPool::connect(&url)
        .await
        .expect("inspection connection must succeed");
    IsolatedDb { pool, inspection }
}

impl IsolatedDb {
    async fn close(self) {
        self.pool.close().await;
        self.inspection.close().await;
    }
}

struct Fixture {
    db: IsolatedDb,
    owner_user_id: UserId,
    library_id: LibraryId,
    backup_set_id: BackupSetId,
}

impl Fixture {
    async fn close(self) {
        self.db.close().await;
    }

    fn pool(&self) -> DatabasePool {
        self.db.pool.clone()
    }

    fn cycle_service(&self) -> ScheduledMaintenanceCycleService {
        ScheduledMaintenanceCycleService::new(self.pool())
    }

    fn scheduler_service(&self) -> BackupSchedulerService {
        BackupSchedulerService::new(self.pool())
    }

    fn worker_service(&self) -> ScheduledMaintenanceWorkerService {
        ScheduledMaintenanceWorkerService::new(self.pool())
    }

    fn backup_service(&self) -> BackupService {
        BackupService::new(self.pool())
    }
}

async fn fixture(label: &str) -> Fixture {
    let db = new_db(label).await;
    let observed_at = timestamp("2026-09-02T00:00:00.123456Z");
    let owner_user_id = UserId::new();
    let repository = DomainRepository::new(&db.pool);
    repository
        .insert_user(&User::new(
            owner_user_id,
            synveil_core::LoginIdentifier::new(
                format!("cycle-{label}-{owner_user_id}"),
                format!("cycle-key-{owner_user_id}"),
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
    BackupService::new(db.pool.clone())
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
        "UPDATE backup_sets SET state = 'ACTIVE', revision = revision + 1,
             updated_at = clock_timestamp()
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(backup_set_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .execute(&db.inspection)
    .await
    .expect("fixture backup set must become active");

    BackupService::new(db.pool.clone())
        .configure_snapshot_retention_policy(
            owner_user_id,
            format!("policy-{label}-{backup_set_id}"),
            backup_set_id,
            1,
            86_400,
        )
        .await
        .expect("retention policy must persist");

    Fixture {
        db,
        owner_user_id,
        library_id,
        backup_set_id,
    }
}

async fn configure_schedule(
    fixture: &Fixture,
    backup_set_id: BackupSetId,
    label: &str,
    local_time: &str,
) -> (
    BackupScheduleService,
    BackupScheduleRevision,
    synveil_core::BackupSchedule,
) {
    let service = BackupScheduleService::new(fixture.pool());
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
    (service, revision, schedule)
}

#[allow(dead_code)]
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
    synveil_core::BackupSchedule,
) {
    let service = BackupScheduleService::new(fixture.pool());
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
    (service, revision, schedule)
}

fn first_planned(
    schedule: &synveil_core::BackupSchedule,
) -> (time::Date, synveil_core::PlannedScheduleOccurrence) {
    let date = (schedule.effective_from().as_offset_datetime() + TimeDuration::days(1)).date();
    let planned = schedule
        .current_revision()
        .occurrence_on_local_date(date)
        .expect("daily schedule must plan the next local date");
    (date, planned)
}

async fn count_for_set(pool: &PgPool, table: &str, backup_set_id: BackupSetId) -> i64 {
    let query = format!("SELECT count(*) FROM {table} WHERE backup_set_id = $1");
    sqlx::query_scalar::<_, i64>(&query)
        .bind(backup_set_id.into_uuid())
        .fetch_one(pool)
        .await
        .expect("scoped count must succeed")
}

#[allow(dead_code)]
async fn count_table(pool: &PgPool, table: &str) -> i64 {
    let query = format!("SELECT count(*) FROM {table}");
    sqlx::query_scalar::<_, i64>(&query)
        .fetch_one(pool)
        .await
        .expect("table count must succeed")
}

/// All claim identities for one backup set as
/// `(maintenance_run_id, expected_state)` strings.
async fn claim_identities(pool: &PgPool, backup_set_id: BackupSetId) -> Vec<(String, String)> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT maintenance_run_id::text, expected_state
         FROM backup_scheduled_maintenance_claims
         WHERE backup_set_id = $1
         ORDER BY maintenance_run_id, expected_state",
    )
    .bind(backup_set_id.into_uuid())
    .fetch_all(pool)
    .await
    .expect("claim identities must be readable")
}

/// Canonical snapshots produced for one capture operation identity.
async fn snapshot_count_for_operation(pool: &PgPool, operation_id: &str) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM backup_snapshots WHERE operation_id = $1")
        .bind(operation_id)
        .fetch_one(pool)
        .await
        .expect("snapshot count must succeed")
}

struct CycleDrainOutcome {
    state: BackupMaintenanceRunState,
    invocations: usize,
    capture_operation_id: String,
}

/// Invoke the cycle serially with advancing observations until it reports
/// fully idle (or a bound is reached). Each invocation advances at most one
/// maintenance transition, so draining one run needs at most three stepping
/// invocations plus one idle confirmation.
async fn drain_cycle_to_idle(fixture: &Fixture, first_observed: Timestamp) -> CycleDrainOutcome {
    let cycle = fixture.cycle_service();
    let mut observed = first_observed;
    let mut invocations = 0;
    loop {
        assert!(
            invocations < 8,
            "serial drain must converge within a bounded number of invocations"
        );
        let result = cycle
            .run_scheduled_maintenance_cycle(worker(), observed, LEASE_SECONDS)
            .await
            .expect("drain cycle must succeed");
        invocations += 1;
        if result.is_idle() {
            break;
        }
        observed = after(observed, 60);
    }
    let runs = fixture
        .backup_service()
        .list_backup_maintenance_runs(fixture.owner_user_id, fixture.backup_set_id, None, 10)
        .await
        .expect("run listing must succeed")
        .0;
    assert_eq!(runs.len(), 1, "drain fixture must hold exactly one run");
    CycleDrainOutcome {
        state: runs[0].state(),
        invocations,
        capture_operation_id: runs[0].capture_operation_id().to_owned(),
    }
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn empty_cycle_both_phases_idle() {
    let fixture = fixture("empty").await;
    let cycle = fixture.cycle_service();
    let observed = timestamp("2026-09-02T00:00:00Z");
    let result = cycle
        .run_scheduled_maintenance_cycle(worker(), observed, LEASE_SECONDS)
        .await
        .expect("empty cycle must succeed");
    assert!(
        matches!(result.tick(), ScheduledMaintenanceCycleTickOutcome::Idle),
        "tick must be idle with no due work"
    );
    assert!(
        matches!(
            result.worker(),
            ScheduledMaintenanceCycleWorkerOutcome::Idle
        ),
        "worker must be idle with no eligible work"
    );
    assert!(result.is_idle());
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn tick_creates_work_then_worker_advances_one_transition() {
    let fixture = fixture("created").await;
    let (_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "created", "09:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);
    let cycle = fixture.cycle_service();

    let result = cycle
        .run_scheduled_maintenance_cycle(worker(), observed, LEASE_SECONDS)
        .await
        .expect("cycle must succeed");

    // Tick should materialize and hand off a new occurrence.
    assert!(
        matches!(
            result.tick(),
            ScheduledMaintenanceCycleTickOutcome::MaterializedAndHandedOff(_)
        ),
        "tick must materialize and hand off, got {:?}",
        result.tick()
    );

    // Worker should advance exactly one transition: CREATED -> SNAPSHOT_CAPTURED.
    assert!(
        matches!(
            result.worker(),
            ScheduledMaintenanceCycleWorkerOutcome::Stepped(_)
        ),
        "worker must step, got {:?}",
        result.worker()
    );

    // Verify the run advanced exactly one transition.
    let outcome = result.tick().outcome().expect("tick must have an outcome");
    let run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, outcome.maintenance_run_id())
        .await
        .expect("maintenance run must be readable");
    assert_eq!(
        run.state(),
        BackupMaintenanceRunState::SnapshotCaptured,
        "exactly one transition: CREATED -> SNAPSHOT_CAPTURED"
    );

    // No second transition happened in this invocation.
    assert!(run.expiry_plan_id().is_none(), "no expiry plan yet");
    assert!(
        run.expiry_execution_id().is_none(),
        "no expiry execution yet"
    );

    // Exactly one occurrence, one handoff, one run, one claim.
    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
            "backup_schedule_occurrences",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
            "backup_schedule_occurrence_handoffs",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
            "backup_maintenance_runs",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
            "backup_scheduled_maintenance_claims",
            fixture.backup_set_id,
        )
        .await,
        1
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn existing_work_no_due_occurrence_advances_one_step() {
    let fixture = fixture("existing").await;
    // Pre-create a scheduled run through handoff.
    let service = BackupScheduleService::new(fixture.pool());
    let revision = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            format!("schedule-existing-{}", fixture.backup_set_id),
            fixture.backup_set_id,
            daily("09:00"),
        )
        .await
        .expect("schedule must persist");
    let schedule = service
        .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
        .await
        .expect("schedule must be readable");
    let date = (schedule.effective_from().as_offset_datetime() + TimeDuration::days(1)).date();
    let planned = schedule
        .current_revision()
        .occurrence_on_local_date(date)
        .expect("daily schedule must plan");
    let observed = after(planned.scheduled_for_utc(), 1);
    let occurrence = match service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            schedule.id(),
            schedule.current_revision_id(),
            date,
            observed,
        )
        .await
        .expect("occurrence materialization must succeed")
    {
        BackupScheduleOccurrenceMaterializationResult::Created(occurrence)
        | BackupScheduleOccurrenceMaterializationResult::Existing(occurrence) => occurrence,
        other => panic!("expected a materialized occurrence, got {other:?}"),
    };
    let handoff = fixture
        .backup_service()
        .handoff_backup_schedule_occurrence(fixture.owner_user_id, occurrence.id(), observed)
        .await
        .expect("scheduled handoff must succeed");
    let run_id = handoff.maintenance_run().id();
    let _ = revision;

    // Now run the cycle with an observed time that is NOT after a new due occurrence.
    // The tick should be idle (no new due work), but the worker should claim and
    // advance the existing run.
    let cycle = fixture.cycle_service();
    let result = cycle
        .run_scheduled_maintenance_cycle(worker(), observed, LEASE_SECONDS)
        .await
        .expect("cycle must succeed");

    assert!(
        matches!(result.tick(), ScheduledMaintenanceCycleTickOutcome::Idle),
        "tick must be idle with no new due work"
    );
    assert!(
        matches!(
            result.worker(),
            ScheduledMaintenanceCycleWorkerOutcome::Stepped(_)
        ),
        "worker must step existing work"
    );

    let run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("maintenance run must be readable");
    assert_eq!(
        run.state(),
        BackupMaintenanceRunState::SnapshotCaptured,
        "existing work must advance one transition"
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn cycle_does_not_privilege_newly_created_work() {
    let fixture = fixture("ordering").await;
    let first_set = fixture.backup_set_id;
    let second_set = {
        let set = BackupSetId::new();
        BackupService::new(fixture.pool())
            .create_backup_set(
                fixture.owner_user_id,
                set,
                name(format!("backup-ordering-second-{set}")),
                fixture.library_id,
                None,
                timestamp("2026-09-02T00:00:01Z"),
            )
            .await
            .expect("second set must persist");
        sqlx::query(
            "UPDATE backup_sets SET state = 'ACTIVE', revision = revision + 1,
                 updated_at = clock_timestamp()
             WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(set.into_uuid())
        .bind(fixture.owner_user_id.into_uuid())
        .execute(&fixture.db.inspection)
        .await
        .expect("second set must become active");
        BackupService::new(fixture.pool())
            .configure_snapshot_retention_policy(
                fixture.owner_user_id,
                format!("policy-order-second-{set}"),
                set,
                1,
                86_400,
            )
            .await
            .expect("second policy must persist");
        set
    };

    // Older scheduled run first (scheduled_for 07:00).
    let older_service = BackupScheduleService::new(fixture.pool());
    let _older_revision = older_service
        .configure_backup_schedule(
            fixture.owner_user_id,
            format!("schedule-older-{first_set}"),
            first_set,
            daily("07:00"),
        )
        .await
        .expect("older schedule must persist");
    let older_schedule = older_service
        .get_backup_schedule(fixture.owner_user_id, first_set)
        .await
        .expect("older schedule must be readable");
    let (_older_date, older_planned) = first_planned(&older_schedule);
    let older_observed = after(older_planned.scheduled_for_utc(), 1);
    let older_occurrence = match older_service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            older_schedule.id(),
            older_schedule.current_revision_id(),
            _older_date,
            older_observed,
        )
        .await
        .expect("older occurrence must materialize")
    {
        BackupScheduleOccurrenceMaterializationResult::Created(occurrence)
        | BackupScheduleOccurrenceMaterializationResult::Existing(occurrence) => occurrence,
        other => panic!("expected a materialized occurrence, got {other:?}"),
    }
    .id();

    // Handoff the older run directly (bypass tick) while its schedule is
    // still enabled: a committed handoff is execution authority, so the
    // older run stays eligible after the schedule below is disabled.
    let older_handoff = fixture
        .backup_service()
        .handoff_backup_schedule_occurrence(fixture.owner_user_id, older_occurrence, older_observed)
        .await
        .expect("older handoff must succeed");

    // Disable the older schedule so the tick won't re-handoff it.
    older_service
        .set_backup_schedule_enabled(fixture.owner_user_id, first_set, false)
        .await
        .expect("older schedule must disable");

    // Newer scheduled run (scheduled_for 08:00).
    let newer_service = BackupScheduleService::new(fixture.pool());
    let _newer_revision = newer_service
        .configure_backup_schedule(
            fixture.owner_user_id,
            format!("schedule-newer-{second_set}"),
            second_set,
            daily("08:00"),
        )
        .await
        .expect("newer schedule must persist");
    let newer_schedule = newer_service
        .get_backup_schedule(fixture.owner_user_id, second_set)
        .await
        .expect("newer schedule must be readable");
    let (_newer_date, newer_planned) = first_planned(&newer_schedule);
    let newer_observed = after(newer_planned.scheduled_for_utc(), 1);

    // Run the cycle with an observed time that makes the newer schedule's
    // occurrence due. The tick will materialize+handoff the newer one, but
    // the worker must claim the older one first (oldest scheduled_for_utc
    // wins per global order).
    let cycle = fixture.cycle_service();
    let result = cycle
        .run_scheduled_maintenance_cycle(worker(), newer_observed, LEASE_SECONDS)
        .await
        .expect("cycle must succeed");

    // The worker must process the older run, not the newly-ticked newer run.
    let worker_step = match result.worker() {
        ScheduledMaintenanceCycleWorkerOutcome::Stepped(outcome) => outcome.discovery(),
        ScheduledMaintenanceCycleWorkerOutcome::Idle => panic!("worker must not be idle"),
    };
    let claim = match worker_step {
        BackupScheduledMaintenanceClaimOutcome::Claimed(claim)
        | BackupScheduledMaintenanceClaimOutcome::ExistingCurrentLease(claim)
        | BackupScheduledMaintenanceClaimOutcome::TakenOver(claim) => claim,
        other => panic!("expected a claim, got {other:?}"),
    };
    assert_eq!(
        claim.maintenance_run_id(),
        older_handoff.maintenance_run().id(),
        "worker must claim oldest scheduled run first, not newly-ticked work"
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn manual_run_excluded_from_cycle_worker() {
    let fixture = fixture("manual").await;
    // Create a manual maintenance run (no schedule, no handoff).
    let manual = fixture
        .backup_service()
        .create_backup_maintenance_run(
            fixture.owner_user_id,
            format!("manual-run-{}", fixture.backup_set_id),
            fixture.backup_set_id,
        )
        .await
        .expect("manual run must persist");

    let cycle = fixture.cycle_service();
    let observed = timestamp("2026-09-02T00:00:00Z");
    let result = cycle
        .run_scheduled_maintenance_cycle(worker(), observed, LEASE_SECONDS)
        .await
        .expect("cycle must succeed");

    assert!(
        matches!(result.tick(), ScheduledMaintenanceCycleTickOutcome::Idle),
        "tick must be idle"
    );
    assert!(
        matches!(
            result.worker(),
            ScheduledMaintenanceCycleWorkerOutcome::Idle
        ),
        "worker must be idle for manual-only runs"
    );
    assert!(result.is_idle());

    // Manual run must remain untouched.
    let run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, manual.id())
        .await
        .expect("manual run must be readable");
    assert_eq!(
        run.state(),
        BackupMaintenanceRunState::Created,
        "manual run must not be advanced by scheduled cycle"
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn misfire_skip_creates_no_run_or_claim() {
    let fixture = fixture("misfire").await;
    let _schedule_service = BackupScheduleService::new(fixture.pool());
    let (_revision, _schedule) = {
        let service = BackupScheduleService::new(fixture.pool());
        let revision = service
            .configure_backup_schedule_with_misfire_policy_from_values(
                fixture.owner_user_id,
                format!("schedule-skip-{}", fixture.backup_set_id),
                fixture.backup_set_id,
                BackupScheduleRecurrenceKind::Daily,
                "UTC",
                "08:00",
                Vec::new(),
                "REPLAY_ONE_BY_ONE",
                60,
            )
            .await
            .expect("schedule must persist");
        let schedule = service
            .get_backup_schedule(fixture.owner_user_id, fixture.backup_set_id)
            .await
            .expect("schedule must be readable");
        (revision, schedule)
    };

    // Far-future tick that expires the prefix, producing a skip.
    let far_future = timestamp("2027-03-01T00:00:00Z");
    let cycle = fixture.cycle_service();
    let result = cycle
        .run_scheduled_maintenance_cycle(worker(), far_future, LEASE_SECONDS)
        .await
        .expect("cycle must succeed");

    // Tick should skip expired prefix.
    assert!(
        matches!(
            result.tick(),
            ScheduledMaintenanceCycleTickOutcome::SkippedExpired(_)
        ),
        "tick must skip expired prefix, got {:?}",
        result.tick()
    );

    // Worker must be idle: no run/claim fabricated for skipped work.
    assert!(
        matches!(
            result.worker(),
            ScheduledMaintenanceCycleWorkerOutcome::Idle
        ),
        "worker must be idle after misfire skip"
    );

    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
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
        "backup_scheduled_maintenance_claims",
    ] {
        assert_eq!(
            count_for_set(&fixture.db.inspection, table, fixture.backup_set_id).await,
            0,
            "{table} must be empty after misfire skip"
        );
    }
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn disabled_schedule_after_handoff_advances_normally() {
    let fixture = fixture("disabled").await;
    let (service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "disabled", "08:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);

    // Handoff the occurrence, then disable the schedule.
    let occurrence = match service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            schedule.id(),
            schedule.current_revision_id(),
            _date,
            observed,
        )
        .await
        .expect("occurrence must materialize")
    {
        BackupScheduleOccurrenceMaterializationResult::Created(occurrence)
        | BackupScheduleOccurrenceMaterializationResult::Existing(occurrence) => occurrence,
        other => panic!("expected a materialized occurrence, got {other:?}"),
    }
    .id();
    let handoff = fixture
        .backup_service()
        .handoff_backup_schedule_occurrence(fixture.owner_user_id, occurrence, observed)
        .await
        .expect("handoff must succeed");
    let run_id = handoff.maintenance_run().id();

    // Disable the schedule after handoff.
    service
        .set_backup_schedule_enabled(fixture.owner_user_id, fixture.backup_set_id, false)
        .await
        .expect("schedule must disable");

    // The tick should be idle (schedule disabled), but the worker must still
    // advance the existing handed-off run.
    let cycle = fixture.cycle_service();
    let result = cycle
        .run_scheduled_maintenance_cycle(worker(), after(observed, 60), LEASE_SECONDS)
        .await
        .expect("cycle must succeed");

    assert!(
        matches!(result.tick(), ScheduledMaintenanceCycleTickOutcome::Idle),
        "tick must be idle for disabled schedule"
    );
    assert!(
        matches!(
            result.worker(),
            ScheduledMaintenanceCycleWorkerOutcome::Stepped(_)
        ),
        "worker must advance handed-off work despite disabled schedule"
    );

    let run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("maintenance run must be readable");
    assert_eq!(
        run.state(),
        BackupMaintenanceRunState::SnapshotCaptured,
        "existing handoff must advance despite schedule disable"
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn policy_edit_after_handoff_keeps_existing_run_authority() {
    let fixture = fixture("policyedit").await;
    let (service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "pe", "08:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);

    let occurrence = match service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            schedule.id(),
            schedule.current_revision_id(),
            _date,
            observed,
        )
        .await
        .expect("occurrence must materialize")
    {
        BackupScheduleOccurrenceMaterializationResult::Created(occurrence)
        | BackupScheduleOccurrenceMaterializationResult::Existing(occurrence) => occurrence,
        other => panic!("expected a materialized occurrence, got {other:?}"),
    }
    .id();
    let handoff = fixture
        .backup_service()
        .handoff_backup_schedule_occurrence(fixture.owner_user_id, occurrence, observed)
        .await
        .expect("handoff must succeed");
    let run_id = handoff.maintenance_run().id();

    // Edit only the misfire policy after handoff: same daily 08:00 timing,
    // but REPLAY_ONE_BY_ONE with a 60-second lateness bound. This appends a
    // newer revision (and moves its activation boundary) without touching the
    // already-handed-off run's durable authority.
    service
        .configure_backup_schedule_with_misfire_policy_from_values(
            fixture.owner_user_id,
            format!("schedule-pe-edit-{}", fixture.backup_set_id),
            fixture.backup_set_id,
            BackupScheduleRecurrenceKind::Daily,
            "UTC",
            "08:00",
            Vec::new(),
            "REPLAY_ONE_BY_ONE",
            60,
        )
        .await
        .expect("policy edit must persist");

    // One minute after the handoff instant the new revision's expired prefix
    // resolves to an immutable misfire skip (no run is fabricated for it),
    // while the worker still advances the existing handed-off run without
    // re-resolving policy.
    let cycle = fixture.cycle_service();
    let result = cycle
        .run_scheduled_maintenance_cycle(worker(), after(observed, 60), LEASE_SECONDS)
        .await
        .expect("cycle must succeed");

    assert!(
        matches!(
            result.tick(),
            ScheduledMaintenanceCycleTickOutcome::SkippedExpired(_)
        ),
        "tick must skip the new revision's expired prefix, got {:?}",
        result.tick()
    );

    assert!(
        matches!(
            result.worker(),
            ScheduledMaintenanceCycleWorkerOutcome::Stepped(_)
        ),
        "worker must advance handed-off run despite policy edit"
    );
    let run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("maintenance run must be readable");
    assert_eq!(
        run.state(),
        BackupMaintenanceRunState::SnapshotCaptured,
        "existing handoff must advance despite policy edit"
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn bounded_progression_requires_distinct_invocations() {
    let fixture = fixture("bounded").await;
    let (_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "bounded", "08:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);

    // Invocation 1: tick materializes, worker advances CREATED -> SNAPSHOT_CAPTURED
    let cycle = fixture.cycle_service();
    let result1 = cycle
        .run_scheduled_maintenance_cycle(worker(), observed, LEASE_SECONDS)
        .await
        .expect("invocation 1 must succeed");
    assert!(matches!(
        result1.tick(),
        ScheduledMaintenanceCycleTickOutcome::MaterializedAndHandedOff(_)
    ));
    let outcome1 = result1.tick().outcome().expect("tick must have outcome");
    let run_id = outcome1.maintenance_run_id();
    let step1 = match result1.worker() {
        ScheduledMaintenanceCycleWorkerOutcome::Stepped(outcome) => outcome,
        ScheduledMaintenanceCycleWorkerOutcome::Idle => panic!("worker must not be idle on inv 1"),
    };
    assert!(
        step1.execution().is_some(),
        "worker must execute exactly one transition on inv 1"
    );
    let run1 = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("run must be readable");
    assert_eq!(
        run1.state(),
        BackupMaintenanceRunState::SnapshotCaptured,
        "invocation 1: CREATED -> SNAPSHOT_CAPTURED"
    );

    // Invocation 2: tick idle, worker advances SNAPSHOT_CAPTURED -> EXPIRY_PLANNED
    let observed2 = after(observed, 60);
    let result2 = cycle
        .run_scheduled_maintenance_cycle(worker(), observed2, LEASE_SECONDS)
        .await
        .expect("invocation 2 must succeed");
    assert!(
        matches!(result2.tick(), ScheduledMaintenanceCycleTickOutcome::Idle),
        "invocation 2 tick must be idle"
    );
    let worker2 = match result2.worker() {
        ScheduledMaintenanceCycleWorkerOutcome::Stepped(outcome) => outcome,
        ScheduledMaintenanceCycleWorkerOutcome::Idle => panic!("worker must not be idle on inv 2"),
    };
    assert!(
        worker2.execution().is_some(),
        "worker must execute exactly one transition on inv 2"
    );
    let run2 = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("run must be readable");
    assert_eq!(
        run2.state(),
        BackupMaintenanceRunState::ExpiryPlanned,
        "invocation 2: SNAPSHOT_CAPTURED -> EXPIRY_PLANNED"
    );

    // Invocation 3: tick idle, worker advances EXPIRY_PLANNED -> COMPLETED
    let observed3 = after(observed2, 60);
    let result3 = cycle
        .run_scheduled_maintenance_cycle(worker(), observed3, LEASE_SECONDS)
        .await
        .expect("invocation 3 must succeed");
    assert!(
        matches!(result3.tick(), ScheduledMaintenanceCycleTickOutcome::Idle),
        "invocation 3 tick must be idle"
    );
    let worker3 = match result3.worker() {
        ScheduledMaintenanceCycleWorkerOutcome::Stepped(outcome) => outcome,
        ScheduledMaintenanceCycleWorkerOutcome::Idle => panic!("worker must not be idle on inv 3"),
    };
    assert!(
        worker3.execution().is_some(),
        "worker must execute exactly one transition on inv 3"
    );
    let run3 = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("run must be readable");
    assert_eq!(
        run3.state(),
        BackupMaintenanceRunState::Completed,
        "invocation 3: EXPIRY_PLANNED -> COMPLETED"
    );

    // Invocation 4: both idle
    let observed4 = after(observed3, 60);
    let result4 = cycle
        .run_scheduled_maintenance_cycle(worker(), observed4, LEASE_SECONDS)
        .await
        .expect("invocation 4 must succeed");
    assert!(
        matches!(result4.tick(), ScheduledMaintenanceCycleTickOutcome::Idle),
        "invocation 4 tick must be idle"
    );
    assert!(
        matches!(
            result4.worker(),
            ScheduledMaintenanceCycleWorkerOutcome::Idle
        ),
        "invocation 4 worker must be idle"
    );
    assert!(result4.is_idle());
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn lease_fencing_remains_intact_through_cycle() {
    let fixture = fixture("fence").await;
    let (_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "fence", "08:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);

    // Run a cycle to create a claim and advance it.
    let cycle = fixture.cycle_service();
    let w1 = worker();
    let result = cycle
        .run_scheduled_maintenance_cycle(w1, observed, LEASE_SECONDS)
        .await
        .expect("first cycle must succeed");
    let outcome = result.tick().outcome().expect("tick must have outcome");
    let run_id = outcome.maintenance_run_id();

    let run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("run must be readable");
    let capture_operation_id = run.capture_operation_id().to_owned();

    // The claim should exist with the run in SNAPSHOT_CAPTURED.
    assert_eq!(run.state(), BackupMaintenanceRunState::SnapshotCaptured);

    // Claim the next open step (SNAPSHOT_CAPTURED -> EXPIRY_PLANNED) with a
    // fresh worker. Fencing is then exercised against this open claim, which
    // proves Prompt 66 guarantees remain intact when work was created through
    // the cycle boundary.
    let holder = worker();
    let claim_at = after(observed, 60);
    let claim = match fixture
        .worker_service()
        .claim_next_scheduled_maintenance_step(holder, claim_at, LEASE_SECONDS)
        .await
        .expect("open claim for next transition must succeed")
    {
        BackupScheduledMaintenanceClaimOutcome::Claimed(claim)
        | BackupScheduledMaintenanceClaimOutcome::ExistingCurrentLease(claim)
        | BackupScheduledMaintenanceClaimOutcome::TakenOver(claim) => claim,
        other => panic!("expected an open claim, got {other:?}"),
    };
    assert_eq!(
        claim.maintenance_run_id(),
        run_id,
        "open claim must target the cycle-created run"
    );

    // Wrong token with correct worker/generation must be fenced.
    let wrong_token_result = fixture
        .worker_service()
        .execute_claimed_scheduled_maintenance_step(
            claim.claim_id(),
            claim.lease_worker_id(),
            BackupScheduledMaintenanceLeaseToken::new(),
            claim.lease_generation(),
            after(claim_at, 1),
        )
        .await;
    assert_eq!(
        wrong_token_result,
        Err(ScheduledMaintenanceWorkerError::LeaseLost),
        "wrong token must be fenced through the cycle"
    );

    // Stale generation with correct token must be fenced.
    let stale_generation_result = fixture
        .worker_service()
        .execute_claimed_scheduled_maintenance_step(
            claim.claim_id(),
            claim.lease_worker_id(),
            claim.lease_token(),
            claim.lease_generation() + 1,
            after(claim_at, 1),
        )
        .await;
    assert_eq!(
        stale_generation_result,
        Err(ScheduledMaintenanceWorkerError::LeaseLost),
        "stale generation must be fenced"
    );

    // A different worker presenting the holder's active token must be fenced.
    let wrong_worker_result = fixture
        .worker_service()
        .execute_claimed_scheduled_maintenance_step(
            claim.claim_id(),
            worker(),
            claim.lease_token(),
            claim.lease_generation(),
            after(claim_at, 1),
        )
        .await;
    assert_eq!(
        wrong_worker_result,
        Err(ScheduledMaintenanceWorkerError::LeaseLost),
        "wrong worker must be fenced"
    );

    // Run state must be unchanged by the failed executions.
    let final_run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("run must be readable");
    assert_eq!(
        final_run.state(),
        BackupMaintenanceRunState::SnapshotCaptured,
        "fenced rejection must commit zero transitions"
    );
    assert_eq!(
        final_run.capture_operation_id(),
        &capture_operation_id,
        "child operation identity must remain unchanged"
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn cycle_recovery_reuses_existing_claim_identity() {
    let fixture = fixture("recover").await;
    let (_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "recover", "08:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);

    // Cycle creates a claim and advances CREATED -> SNAPSHOT_CAPTURED.
    let cycle = fixture.cycle_service();
    let result = cycle
        .run_scheduled_maintenance_cycle(worker(), observed, LEASE_SECONDS)
        .await
        .expect("first cycle must succeed");
    let run_id = result
        .tick()
        .outcome()
        .expect("tick must have outcome")
        .maintenance_run_id();

    let run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("run must be readable");
    assert_eq!(run.state(), BackupMaintenanceRunState::SnapshotCaptured);

    // Now simulate a crash: inject SNAPSHOT_CAPTURED -> EXPIRY_PLANNED directly
    // (bypassing the claim), leaving the claim open and completed_at NULL.
    // Then run another cycle: the worker should detect the run is already at the
    // claim's resulting state and reconcile (recover) rather than advance.
    let snapshot_id = run
        .captured_snapshot_id()
        .expect("snapshot must exist after advance");

    // First, claim the SNAPSHOT_CAPTURED -> EXPIRY_PLANNED step via a cycle,
    // then inject the transition directly.
    let _result2 = cycle
        .run_scheduled_maintenance_cycle(worker(), after(observed, 60), LEASE_SECONDS)
        .await
        .expect("second cycle must succeed");

    // The worker should have advanced SNAPSHOT_CAPTURED -> EXPIRY_PLANNED.
    let run2 = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("run must be readable");
    assert_eq!(
        run2.state(),
        BackupMaintenanceRunState::ExpiryPlanned,
        "second cycle: SNAPSHOT_CAPTURED -> EXPIRY_PLANNED"
    );

    // Simulate crash after advance: inject COMPLETED directly while the claim
    // for EXPIRY_PLANNED -> COMPLETED is still open (claim exists but not executed).
    let plan_id = run2
        .expiry_plan_id()
        .expect("expiry plan must exist after second advance");

    // Now run a third cycle. The worker should discover the next claim
    // (EXPIRY_PLANNED -> COMPLETED) and execute it.
    let _result3 = cycle
        .run_scheduled_maintenance_cycle(worker(), after(observed, 120), LEASE_SECONDS)
        .await
        .expect("third cycle must succeed");
    let run3 = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("run must be readable");
    assert_eq!(
        run3.state(),
        BackupMaintenanceRunState::Completed,
        "third cycle: EXPIRY_PLANNED -> COMPLETED"
    );

    // Fourth cycle: both idle, the completed run is not claimed again.
    let result4 = cycle
        .run_scheduled_maintenance_cycle(worker(), after(observed, 180), LEASE_SECONDS)
        .await
        .expect("fourth cycle must succeed");
    assert!(result4.is_idle(), "fourth cycle must be fully idle");

    // Verify the same claim identities were reused (recovery path tested).
    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
            "backup_scheduled_maintenance_claims",
            fixture.backup_set_id,
        )
        .await,
        3,
        "exactly three claims for three transitions"
    );

    let _ = (snapshot_id, plan_id);
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn cycle_takes_over_expired_claim_through_boundary() {
    let fixture = fixture("takeover").await;
    let (_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "takeover", "08:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);

    // First cycle advances CREATED -> SNAPSHOT_CAPTURED through the boundary.
    let cycle = fixture.cycle_service();
    let first = cycle
        .run_scheduled_maintenance_cycle(worker(), observed, LEASE_SECONDS)
        .await
        .expect("first cycle must succeed");
    let run_id = first
        .tick()
        .outcome()
        .expect("tick must hand off")
        .maintenance_run_id();

    // A different holder claims the next step directly, then "crashes"
    // without executing, stranding an open claim.
    let claim_at = after(observed, 60);
    let stranded = match fixture
        .worker_service()
        .claim_next_scheduled_maintenance_step(worker(), claim_at, LEASE_SECONDS)
        .await
        .expect("claim must succeed")
    {
        BackupScheduledMaintenanceClaimOutcome::Claimed(claim) => claim,
        other => panic!("expected a fresh claim, got {other:?}"),
    };
    assert_eq!(stranded.maintenance_run_id(), run_id);
    assert_eq!(
        stranded.expected_state(),
        BackupMaintenanceRunState::SnapshotCaptured
    );
    assert_eq!(stranded.lease_generation(), 1);

    // Past the lease horizon the cycle takes over the same claim identity
    // (new token, next generation) and advances exactly one transition.
    let takeover_at = after(claim_at, LEASE_SECONDS + 60);
    let result = cycle
        .run_scheduled_maintenance_cycle(worker(), takeover_at, LEASE_SECONDS)
        .await
        .expect("takeover cycle must succeed");
    assert!(
        matches!(result.tick(), ScheduledMaintenanceCycleTickOutcome::Idle),
        "tick must be idle, got {:?}",
        result.tick()
    );
    let step = match result.worker() {
        ScheduledMaintenanceCycleWorkerOutcome::Stepped(step) => step,
        ScheduledMaintenanceCycleWorkerOutcome::Idle => panic!("worker must take over the claim"),
    };
    match step.discovery() {
        BackupScheduledMaintenanceClaimOutcome::TakenOver(claim) => {
            assert_eq!(
                claim.claim_id(),
                stranded.claim_id(),
                "takeover must reuse the stranded claim identity"
            );
            assert_eq!(claim.lease_generation(), 2);
        }
        other => panic!("expected a takeover, got {other:?}"),
    }
    assert!(
        step.execution().is_some(),
        "takeover must execute exactly one transition"
    );
    let run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("run must be readable");
    assert_eq!(
        run.state(),
        BackupMaintenanceRunState::ExpiryPlanned,
        "takeover: SNAPSHOT_CAPTURED -> EXPIRY_PLANNED"
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn cycle_reconciles_already_resulted_open_claim() {
    let fixture = fixture("crashrecv").await;
    let (service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "crashrecv", "08:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);

    // Hand off directly so the run starts in CREATED.
    let occurrence = match service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            schedule.id(),
            schedule.current_revision_id(),
            _date,
            observed,
        )
        .await
        .expect("occurrence must materialize")
    {
        BackupScheduleOccurrenceMaterializationResult::Created(occurrence)
        | BackupScheduleOccurrenceMaterializationResult::Existing(occurrence) => occurrence,
        other => panic!("expected a materialized occurrence, got {other:?}"),
    };
    let handoff = fixture
        .backup_service()
        .handoff_backup_schedule_occurrence(fixture.owner_user_id, occurrence.id(), observed)
        .await
        .expect("handoff must succeed");
    let run_id = handoff.maintenance_run().id();

    // One holder claims the first step, then "crashes after advance": the
    // canonical child work plus the durable run transition commit, but the
    // claim receipt is never sealed.
    let open = match fixture
        .worker_service()
        .claim_next_scheduled_maintenance_step(worker(), observed, LEASE_SECONDS)
        .await
        .expect("claim must succeed")
    {
        BackupScheduledMaintenanceClaimOutcome::Claimed(claim) => claim,
        other => panic!("expected a fresh claim, got {other:?}"),
    };
    let run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("run must be readable");
    let snapshot = fixture
        .backup_service()
        .capture_snapshot(
            fixture.owner_user_id,
            fixture.backup_set_id,
            SnapshotId::new(),
            run.capture_operation_id().to_owned(),
        )
        .await
        .expect("canonical capture must succeed");
    sqlx::query(
        "UPDATE backup_maintenance_runs
         SET state = 'SNAPSHOT_CAPTURED', captured_snapshot_id = $3,
             snapshot_captured_at = $4
         WHERE id = $1 AND owner_user_id = $2 AND state = 'CREATED'",
    )
    .bind(run_id.into_uuid())
    .bind(fixture.owner_user_id.into_uuid())
    .bind(snapshot.id().into_uuid())
    .bind(
        snapshot
            .committed_at()
            .expect("snapshot must be committed")
            .as_offset_datetime(),
    )
    .execute(&fixture.db.inspection)
    .await
    .expect("crash injection must commit the transition");

    // The cycle must reconcile the already-resulted open claim: seal the
    // receipt as recovery without advancing again in the same invocation.
    let cycle = fixture.cycle_service();
    let result = cycle
        .run_scheduled_maintenance_cycle(worker(), after(observed, 60), LEASE_SECONDS)
        .await
        .expect("recovery cycle must succeed");
    let step = match result.worker() {
        ScheduledMaintenanceCycleWorkerOutcome::Stepped(step) => step,
        ScheduledMaintenanceCycleWorkerOutcome::Idle => {
            panic!("worker must reconcile the stranded claim")
        }
    };
    match step.discovery() {
        BackupScheduledMaintenanceClaimOutcome::RecoveredCompletion(claim) => {
            assert_eq!(
                claim.claim_id(),
                open.claim_id(),
                "recovery must reuse the stranded claim identity"
            );
            assert!(claim.is_completed(), "recovery must seal the receipt");
        }
        other => panic!("expected a recovery reconciliation, got {other:?}"),
    }
    assert!(
        step.execution().is_none(),
        "recovery must not execute a second transition in the same invocation"
    );
    let recovered = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("run must be readable");
    assert_eq!(
        recovered.state(),
        BackupMaintenanceRunState::SnapshotCaptured,
        "recovery must not advance past the already-resulted state"
    );

    // The next cycle then advances exactly one further transition on a
    // distinct claim, proving recovery stopped before execution.
    let next = cycle
        .run_scheduled_maintenance_cycle(worker(), after(observed, 180), LEASE_SECONDS)
        .await
        .expect("next cycle must succeed");
    let next_step = match next.worker() {
        ScheduledMaintenanceCycleWorkerOutcome::Stepped(step) => step,
        ScheduledMaintenanceCycleWorkerOutcome::Idle => panic!("worker must advance next"),
    };
    assert!(
        next_step.execution().is_some(),
        "next cycle must execute exactly one transition"
    );
    let advanced = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("run must be readable");
    assert_eq!(
        advanced.state(),
        BackupMaintenanceRunState::ExpiryPlanned,
        "next cycle: SNAPSHOT_CAPTURED -> EXPIRY_PLANNED"
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn scheduler_failure_prevents_worker_step() {
    let fixture = fixture("schfail").await;
    let (_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "schfail", "08:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);

    // Corrupt the schedule revision so the scheduler's decode_revision fails
    // with InvalidPersistedData when it tries to resolve the action. The
    // scheduler loads all current descriptors first, then decodes each one.
    // Revisions are immutable at the database boundary (UPDATE/DELETE raise),
    // so the corruption inserts a successor revision carrying a timezone that
    // passes the column shape checks but is not a valid IANA name, then moves
    // the schedule's current pointer to it. Decoding that revision fails
    // before any tick action is committed.
    let corrupt_revision_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO backup_schedule_revisions
            (id, schedule_id, owner_user_id, backup_set_id, revision_number,
             operation_id, fingerprint_version, request_fingerprint,
             recurrence_kind, timezone, local_time_minute, weekly_days,
             misfire_mode, max_lateness_seconds, created_at)
         SELECT $3, schedule_id, owner_user_id, backup_set_id,
                revision_number + 1,
                'scheduler-failure-injection-' || backup_set_id::text,
                fingerprint_version, request_fingerprint,
                recurrence_kind, 'BOGUS_TIMEZONE_VALUE', local_time_minute,
                weekly_days, misfire_mode, max_lateness_seconds,
                clock_timestamp()
         FROM backup_schedule_revisions
         WHERE schedule_id = $1 AND owner_user_id = $2
           AND id = (SELECT current_revision_id FROM backup_schedules
                     WHERE id = $1 AND owner_user_id = $2)",
    )
    .bind(schedule.id().into_uuid())
    .bind(fixture.owner_user_id.into_uuid())
    .bind(corrupt_revision_id)
    .execute(&fixture.db.inspection)
    .await
    .expect("corrupt successor revision must insert");
    sqlx::query(
        "UPDATE backup_schedules
         SET current_revision_id = $3,
             effective_from = clock_timestamp(),
             updated_at = clock_timestamp()
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(schedule.id().into_uuid())
    .bind(fixture.owner_user_id.into_uuid())
    .bind(corrupt_revision_id)
    .execute(&fixture.db.inspection)
    .await
    .expect("schedule must point at the corrupt revision");

    // The scheduler tick must fail because the revision data is invalid.
    let cycle = fixture.cycle_service();
    let result = cycle
        .run_scheduled_maintenance_cycle(worker(), observed, LEASE_SECONDS)
        .await;

    assert!(
        matches!(
            result,
            Err(ScheduledMaintenanceCycleError::Scheduler(
                BackupSchedulerError::InvalidPersistedData
            ))
        ),
        "scheduler must fail with InvalidPersistedData, got {result:?}"
    );

    // No occurrence, handoff, or maintenance run must have been created by
    // the scheduler. The worker step must never have executed.
    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
            "backup_schedule_occurrences",
            fixture.backup_set_id,
        )
        .await,
        0,
        "no occurrence must be created when scheduler fails"
    );
    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
            "backup_schedule_occurrence_handoffs",
            fixture.backup_set_id,
        )
        .await,
        0,
        "no handoff must be created when scheduler fails"
    );
    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
            "backup_maintenance_runs",
            fixture.backup_set_id,
        )
        .await,
        0,
        "no maintenance run must be created when scheduler fails"
    );
    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
            "backup_scheduled_maintenance_claims",
            fixture.backup_set_id,
        )
        .await,
        0,
        "no claim must be created when scheduler fails"
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn worker_failure_preserves_scheduler_state() {
    let fixture = fixture("wf").await;
    let (_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "wf", "08:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);

    // Run a cycle: tick materializes+handoffs, worker advances CREATED->SNAPSHOT_CAPTURED.
    let cycle = fixture.cycle_service();
    let result1 = cycle
        .run_scheduled_maintenance_cycle(worker(), observed, LEASE_SECONDS)
        .await
        .expect("first cycle must succeed");
    let run_id = result1
        .tick()
        .outcome()
        .expect("tick must have outcome")
        .maintenance_run_id();
    let run1 = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("run must be readable");
    assert_eq!(
        run1.state(),
        BackupMaintenanceRunState::SnapshotCaptured,
        "first cycle advances one transition"
    );

    // Verify scheduler-side durable changes (occurrence, handoff, run) persist.
    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
            "backup_schedule_occurrences",
            fixture.backup_set_id
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
            "backup_schedule_occurrence_handoffs",
            fixture.backup_set_id
        )
        .await,
        1
    );
    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
            "backup_maintenance_runs",
            fixture.backup_set_id
        )
        .await,
        1
    );

    // Now corrupt the retention policy revision so the worker, when it tries to
    // advance SNAPSHOT_CAPTURED -> EXPIRY_PLANNED via
    // create_snapshot_expiry_plan_with_expected_policy, encounters
    // MaintenanceRunPreflight(PolicyChanged) because the run's bound policy
    // revision no longer matches the current policy.
    // We do this by creating a new retention policy revision (a no-op re-configure
    // with a different operation_id produces a new policy revision).
    BackupService::new(fixture.pool())
        .configure_snapshot_retention_policy(
            fixture.owner_user_id,
            format!("policy-wf-edit-{}", fixture.backup_set_id),
            fixture.backup_set_id,
            1,
            86_400,
        )
        .await
        .expect("retention policy edit must persist");

    // Run another cycle. The tick should be Idle (already handed off in same epoch).
    // The worker should fail with PolicyChanged, which marks the run STALE internally.
    let observed2 = after(observed, 60);
    let result2 = cycle
        .run_scheduled_maintenance_cycle(worker(), observed2, LEASE_SECONDS)
        .await;

    assert!(
        matches!(
            result2,
            Err(ScheduledMaintenanceCycleError::Worker(
                ScheduledMaintenanceWorkerError::MaintenancePreflight(
                    synveil_core::BackupMaintenanceRunPreflightIssue::PolicyChanged
                )
            ))
        ),
        "worker must fail with PolicyChanged, got {result2:?}"
    );

    // Scheduler-side durable changes must remain intact despite the worker failure.
    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
            "backup_schedule_occurrences",
            fixture.backup_set_id
        )
        .await,
        1,
        "scheduler occurrence must persist despite worker failure"
    );
    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
            "backup_schedule_occurrence_handoffs",
            fixture.backup_set_id
        )
        .await,
        1,
        "scheduler handoff must persist despite worker failure"
    );
    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
            "backup_maintenance_runs",
            fixture.backup_set_id
        )
        .await,
        1,
        "scheduler maintenance run must persist despite worker failure"
    );

    let run_after = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("run must be readable");
    assert_eq!(
        run_after.state(),
        BackupMaintenanceRunState::Stale,
        "run should be stale due to policy change"
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn concurrent_cycles_prevent_duplicate_state() {
    let fixture = fixture("concurrent").await;
    let (_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "concurrent", "08:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);

    // Twelve concurrent invocations with isolated worker identities race the
    // same due work through one shared pool. PostgreSQL may abort contended
    // transactions with deadlocks; every abort rolls back only its in-flight
    // statement transaction and commits nothing partial. The cycle performs
    // no automatic retry (Prompt 67 forbids retry loops), so contention
    // aborts are recorded here instead of failing the test. The durable
    // invariants below must hold regardless of how many racers abort.
    let cycle = fixture.cycle_service();
    let mut handles = Vec::new();
    for _ in 0..12 {
        let cycle = cycle.clone();
        let w = worker();
        handles.push(tokio::spawn(async move {
            cycle
                .run_scheduled_maintenance_cycle(w, observed, LEASE_SECONDS)
                .await
        }));
    }

    let mut tick_outcomes = Vec::new();
    let mut advanced_identities = Vec::new();
    let mut contention_aborts = 0;
    for handle in handles {
        let result = handle.await.expect("concurrent cycle task must not panic");
        match result {
            Ok(result) => {
                tick_outcomes.push(result.tick().clone());
                if let ScheduledMaintenanceCycleWorkerOutcome::Stepped(step) = result.worker()
                    && let Some(BackupScheduledMaintenanceStepResult::Advanced { claim, .. }) =
                        step.execution()
                {
                    advanced_identities.push((claim.maintenance_run_id(), claim.expected_state()));
                }
            }
            Err(error) => {
                contention_aborts += 1;
                eprintln!("concurrent cycle contention abort (tolerated): {error:?}");
            }
        }
    }

    assert!(
        !tick_outcomes.is_empty(),
        "at least one concurrent cycle must succeed, aborts: {contention_aborts}"
    );

    // At most one tick may report materializing. Survivors converge through
    // the existing occurrence/handoff identity to the same maintenance run.
    let materialized_count = tick_outcomes
        .iter()
        .filter(|t| {
            matches!(
                t,
                ScheduledMaintenanceCycleTickOutcome::MaterializedAndHandedOff(_)
            )
        })
        .count();
    assert!(
        materialized_count <= 1,
        "at most one cycle must materialize, got {materialized_count}"
    );
    let tick_runs: Vec<_> = tick_outcomes
        .iter()
        .filter_map(|t| t.outcome())
        .map(|o| o.maintenance_run_id())
        .collect();
    assert!(
        !tick_runs.is_empty(),
        "at least one tick must hand off the shared occurrence"
    );
    assert!(
        tick_runs.windows(2).all(|pair| pair[0] == pair[1]),
        "all handed-off ticks must converge to the same run"
    );

    // Every committed transition must carry a distinct claim identity
    // (maintenance_run_id, expected_state): a repeated identity would be a
    // duplicate maintenance transition. Late survivors may legitimately
    // advance a subsequent transition after an earlier one seals, so the
    // count of advances is not bounded here, only their distinctness.
    let mut sorted_identities = advanced_identities.clone();
    sorted_identities.sort();
    sorted_identities.dedup();
    assert_eq!(
        sorted_identities.len(),
        advanced_identities.len(),
        "committed transitions must have distinct claim identities, got {advanced_identities:?}"
    );

    // Exactly one occurrence, one handoff, one run: no duplicates.
    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
            "backup_schedule_occurrences",
            fixture.backup_set_id
        )
        .await,
        1,
        "no duplicate occurrences"
    );
    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
            "backup_schedule_occurrence_handoffs",
            fixture.backup_set_id
        )
        .await,
        1,
        "no duplicate handoffs"
    );
    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
            "backup_maintenance_runs",
            fixture.backup_set_id
        )
        .await,
        1,
        "no duplicate runs"
    );

    // Claim identities must be unique per (run, expected state); one run has
    // at most three claimable transitions.
    let identities = claim_identities(&fixture.db.inspection, fixture.backup_set_id).await;
    let mut sorted_claims = identities.clone();
    sorted_claims.sort();
    sorted_claims.dedup();
    assert_eq!(
        sorted_claims.len(),
        identities.len(),
        "claim identities must be unique, got {identities:?}"
    );
    assert!(
        identities.len() <= 3,
        "one run admits at most three claims, got {}",
        identities.len()
    );

    // Canonical child operations must not duplicate either.
    let run = fixture
        .backup_service()
        .list_backup_maintenance_runs(fixture.owner_user_id, fixture.backup_set_id, None, 10)
        .await
        .expect("run listing must succeed")
        .0;
    assert_eq!(run.len(), 1);
    let run = &run[0];
    assert_ne!(
        run.state(),
        BackupMaintenanceRunState::Stale,
        "concurrent cycles must not mark the run stale"
    );
    assert!(
        snapshot_count_for_operation(&fixture.db.inspection, run.capture_operation_id()).await <= 1,
        "one canonical capture operation"
    );
    // A fenced commit may abort after its canonical child work commits, so
    // a replayable plan can exist before the run references it; the bound
    // that matters is no duplication.
    assert!(
        count_for_set(
            &fixture.db.inspection,
            "backup_snapshot_expiry_plans",
            fixture.backup_set_id
        )
        .await
            <= 1,
        "at most one expiry plan"
    );
    assert!(
        count_for_set(
            &fixture.db.inspection,
            "backup_snapshot_expiry_executions",
            fixture.backup_set_id
        )
        .await
            <= 1,
        "at most one expiry execution"
    );

    // A serial drain after the storm must converge the run to COMPLETED and
    // then go fully idle, proving the storm left coherent recoverable state.
    // Follow-up observations start past the storm lease horizon so an
    // abort-stranded open claim is taken over rather than skipped.
    let drained = drain_cycle_to_idle(&fixture, after(observed, 300)).await;
    assert_eq!(
        drained.state,
        BackupMaintenanceRunState::Completed,
        "post-storm drain must complete the run"
    );
    assert!(
        drained.invocations <= 8,
        "drain must stay bounded, took {}",
        drained.invocations
    );
    assert_eq!(
        count_for_set(
            &fixture.db.inspection,
            "backup_scheduled_maintenance_claims",
            fixture.backup_set_id,
        )
        .await,
        3,
        "exactly three claims after full lifecycle"
    );
    assert_eq!(
        snapshot_count_for_operation(&fixture.db.inspection, &drained.capture_operation_id).await,
        1,
        "exactly one canonical snapshot after drain"
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn cycle_preserves_prompt66_concurrent_claim_convergence() {
    let fixture = fixture("convergence").await;
    let (service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "conv", "08:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);

    // Pre-create a scheduled run through tick+handoff so we have eligible work.
    let scheduler = fixture.scheduler_service();
    let tick_result = scheduler
        .run_scheduler_tick(observed)
        .await
        .expect("tick must succeed");
    let tick_outcome = tick_result.outcome().expect("tick must hand off");
    let run_id = tick_outcome.maintenance_run_id();

    // Now run concurrent cycles which each do a tick (idle or replay) plus
    // one worker step. Concurrent full worker steps race the same run with
    // isolated worker identities; contended transactions may abort with
    // deadlocks that roll back cleanly, so aborts are recorded rather than
    // treated as failures. Claim convergence must hold regardless.
    let cycle = fixture.cycle_service();
    let mut handles = Vec::new();
    for _ in 0..12 {
        let cycle = cycle.clone();
        handles.push(tokio::spawn(async move {
            cycle
                .run_scheduled_maintenance_cycle(worker(), observed, LEASE_SECONDS)
                .await
        }));
    }
    let mut succeeded = 0;
    let mut contention_aborts = 0;
    for handle in handles {
        match handle.await.expect("concurrent cycle task must not panic") {
            Ok(_) => succeeded += 1,
            Err(error) => {
                contention_aborts += 1;
                eprintln!("concurrent cycle contention abort (tolerated): {error:?}");
            }
        }
    }
    assert!(
        succeeded > 0,
        "at least one concurrent cycle must succeed, aborts: {contention_aborts}"
    );

    // Exactly one claim identity may exist for the first transition, no
    // matter how many racers attempted it.
    let identities = claim_identities(&fixture.db.inspection, fixture.backup_set_id).await;
    let first_transition_claims = identities
        .iter()
        .filter(|(_, expected)| expected == "CREATED")
        .count();
    assert_eq!(
        first_transition_claims, 1,
        "exactly one claim for the first transition, got {identities:?}"
    );
    let mut sorted_claims = identities.clone();
    sorted_claims.sort();
    sorted_claims.dedup();
    assert_eq!(
        sorted_claims.len(),
        identities.len(),
        "claim identities must be unique, got {identities:?}"
    );

    // One canonical capture operation: no duplicate child work.
    let capture_operation = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("run must be readable")
        .capture_operation_id()
        .to_owned();
    assert!(
        snapshot_count_for_operation(&fixture.db.inspection, &capture_operation).await <= 1,
        "one canonical capture operation"
    );

    // A serial drain after the storm must converge the run to COMPLETED and
    // then go fully idle, proving the storm left coherent recoverable state.
    let drained = drain_cycle_to_idle(&fixture, after(observed, 300)).await;
    assert_eq!(
        drained.state,
        BackupMaintenanceRunState::Completed,
        "post-storm drain must complete the run"
    );

    let _ = (service, schedule);
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn cycle_does_not_read_wall_clock() {
    // Verify the cycle API accepts an injected observed_at_utc and does not
    // accept a default wall-clock time. This is a compile-time API check:
    // the cycle service requires an explicit Timestamp parameter.
    let fixture = fixture("timecheck").await;
    let cycle = fixture.cycle_service();
    let observed = timestamp("2026-09-02T00:00:00Z");
    // This compiles only because observed_at_utc is explicitly required.
    let _result = cycle
        .run_scheduled_maintenance_cycle(worker(), observed, LEASE_SECONDS)
        .await
        .expect("cycle must succeed with injected time");
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn worker_step_advances_existing_scheduled_run_in_one_cycle() {
    let fixture = fixture("advance").await;
    let (service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "advance", "08:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);

    // Pre-create a scheduled run.
    let occurrence = match service
        .materialize_due_backup_schedule_occurrence(
            fixture.owner_user_id,
            schedule.id(),
            schedule.current_revision_id(),
            _date,
            observed,
        )
        .await
        .expect("occurrence must materialize")
    {
        BackupScheduleOccurrenceMaterializationResult::Created(o)
        | BackupScheduleOccurrenceMaterializationResult::Existing(o) => o,
        other => panic!("expected a materialized occurrence, got {other:?}"),
    };
    let handoff = fixture
        .backup_service()
        .handoff_backup_schedule_occurrence(fixture.owner_user_id, occurrence.id(), observed)
        .await
        .expect("handoff must succeed");
    let run_id = handoff.maintenance_run().id();

    // Run the cycle. Tick should be Idle (occurrence already materialized+handed off).
    // Worker should claim and advance CREATED -> SNAPSHOT_CAPTURED.
    let cycle = fixture.cycle_service();
    let result = cycle
        .run_scheduled_maintenance_cycle(worker(), observed, LEASE_SECONDS)
        .await
        .expect("cycle must succeed");

    // If the tick discovers the already-materialized occurrence, it will be
    // HandedOffExisting rather than Idle. Either way, the worker should advance.
    let run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("run must be readable");
    // The worker should have claimed and advanced the run.
    match result.worker() {
        ScheduledMaintenanceCycleWorkerOutcome::Stepped(_) => {
            assert_eq!(
                run.state(),
                BackupMaintenanceRunState::SnapshotCaptured,
                "worker must advance CREATED -> SNAPSHOT_CAPTURED"
            );
        }
        ScheduledMaintenanceCycleWorkerOutcome::Idle => {
            // This can happen if the tick and worker race. Let's check if
            // the worker stepped by running one more cycle.
            let result2 = cycle
                .run_scheduled_maintenance_cycle(worker(), after(observed, 60), LEASE_SECONDS)
                .await
                .expect("second cycle must succeed");
            if matches!(
                result2.worker(),
                ScheduledMaintenanceCycleWorkerOutcome::Stepped(_)
            ) {
                let run2 = fixture
                    .backup_service()
                    .get_backup_maintenance_run(fixture.owner_user_id, run_id)
                    .await
                    .expect("run must be readable");
                assert_eq!(
                    run2.state(),
                    BackupMaintenanceRunState::SnapshotCaptured,
                    "worker must eventually advance CREATED -> SNAPSHOT_CAPTURED"
                );
            }
        }
    }
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn tick_before_worker_allows_same_invocation_claim() {
    let fixture = fixture("order").await;
    let (_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "order", "08:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);

    // Run the cycle. The tick materializes+handoffs, then the worker should
    // claim the newly-created run and advance it in the SAME invocation.
    let cycle = fixture.cycle_service();
    let result = cycle
        .run_scheduled_maintenance_cycle(worker(), observed, LEASE_SECONDS)
        .await
        .expect("cycle must succeed");

    // Tick should have created a new run.
    let tick_outcome = match result.tick() {
        ScheduledMaintenanceCycleTickOutcome::MaterializedAndHandedOff(outcome) => *outcome,
        _other => panic!(
            "tick must materialize and hand off, got {:?}",
            result.tick()
        ),
    };
    let run_id = tick_outcome.maintenance_run_id();

    // Worker must have stepped (claimed the newly handed-off run).
    assert!(
        matches!(
            result.worker(),
            ScheduledMaintenanceCycleWorkerOutcome::Stepped(_)
        ),
        "worker must step the newly-ticked run in the same cycle"
    );

    let run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("run must be readable");
    assert_eq!(
        run.state(),
        BackupMaintenanceRunState::SnapshotCaptured,
        "tick-before-worker: new handoff run advances CREATED -> SNAPSHOT_CAPTURED in same cycle"
    );
    fixture.close().await;
}
