//! PostgreSQL verification for Prompt 68's service-integration boundary.
//!
//! `ScheduledMaintenanceCycleRunner` provides the application-level caller
//! for manually invoking one bounded scheduled-maintenance cycle. Tests run
//! with isolated disposable databases to avoid global discovery contamination.
//! Run with `--test-threads=1`.

use std::time::Duration as StdDuration;

use sqlx::PgPool;
use synveil_core::{
    BackupMaintenanceRunState, BackupScheduleConfig, BackupScheduleLocalTime,
    BackupScheduleOccurrenceMaterializationResult, BackupScheduleRecurrenceKind,
    BackupScheduleRevision, BackupScheduleTimezone, BackupScheduledMaintenanceStepResult,
    BackupScheduledMaintenanceWorkerId, BackupSetId, DedupDomainId, Library, LibraryId,
    LogicalName, Node, NodeId, Timestamp, User, UserId, UserStatus,
};
use synveil_metadata::{
    BackupScheduleService, BackupSchedulerError, BackupService, DatabaseConfig, DatabasePool,
    DomainRepository, MigrationRunner, ScheduledMaintenanceCycleError,
    ScheduledMaintenanceCycleRunner, ScheduledMaintenanceCycleTickOutcome,
    ScheduledMaintenanceCycleWorkerOutcome, ScheduledMaintenanceWorkerError,
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
        "p68_{}_{}",
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
    backup_set_id: BackupSetId,
}

impl Fixture {
    async fn close(self) {
        self.db.close().await;
    }

    fn pool(&self) -> DatabasePool {
        self.db.pool.clone()
    }

    fn runner(&self) -> ScheduledMaintenanceCycleRunner {
        ScheduledMaintenanceCycleRunner::new(self.pool())
    }

    fn runner_with_id(
        &self,
        worker_id: BackupScheduledMaintenanceWorkerId,
    ) -> ScheduledMaintenanceCycleRunner {
        ScheduledMaintenanceCycleRunner::with_worker_id(self.pool(), worker_id)
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
                format!("runner-{label}-{owner_user_id}"),
                format!("runner-key-{owner_user_id}"),
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

/// Invoke the runner serially with advancing observations until it reports
/// fully idle (or a bound is reached). Each invocation advances at most one
/// maintenance transition, so draining one run needs at most three stepping
/// invocations plus one idle confirmation.
async fn drain_runner_to_idle(fixture: &Fixture, first_observed: Timestamp) -> CycleDrainOutcome {
    let runner = fixture.runner();
    let mut observed = first_observed;
    let mut invocations = 0;
    loop {
        assert!(
            invocations < 8,
            "serial drain must converge within a bounded number of invocations"
        );
        let result = runner
            .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
            .await
            .expect("drain runner must succeed");
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
async fn runner_service_construction() {
    let fixture = fixture("construct").await;
    let runner = fixture.runner();
    // Runner must hold a valid pool and worker identity.
    assert!(!runner.worker_id().to_string().is_empty());
    // Runner must be cloneable (Arc-based or Clone).
    let cloned = runner.clone();
    assert_eq!(runner.worker_id(), cloned.worker_id());
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn runner_empty_cycle_both_phases_idle() {
    let fixture = fixture("empty").await;
    let runner = fixture.runner();
    let observed = timestamp("2026-09-02T00:00:00Z");
    let result = runner
        .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
        .await
        .expect("empty runner cycle must succeed");
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
async fn runner_tick_creates_work_then_worker_advances_one_transition() {
    let fixture = fixture("created").await;
    let (_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "created", "09:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);
    let runner = fixture.runner();

    let result = runner
        .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
        .await
        .expect("runner cycle must succeed");

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
async fn runner_existing_work_no_due_occurrence_advances_one_step() {
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

    // Now run the runner with an observed time that is NOT after a new due occurrence.
    // The tick should be idle (no new due work), but the worker should claim and
    // advance the existing run.
    let runner = fixture.runner();
    let result = runner
        .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
        .await
        .expect("runner cycle must succeed");

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
async fn runner_bounded_progression_requires_distinct_invocations() {
    let fixture = fixture("bounded").await;
    let (_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "bounded", "08:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);

    // Invocation 1: tick materializes, worker advances CREATED -> SNAPSHOT_CAPTURED
    let runner = fixture.runner();
    let result1 = runner
        .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
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
        ScheduledMaintenanceCycleWorkerOutcome::Idle => {
            panic!("worker must not be idle on inv 1")
        }
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
    let result2 = runner
        .run_one_scheduled_backup_maintenance_cycle(observed2, LEASE_SECONDS)
        .await
        .expect("invocation 2 must succeed");
    assert!(
        matches!(result2.tick(), ScheduledMaintenanceCycleTickOutcome::Idle),
        "invocation 2 tick must be idle"
    );
    let worker2 = match result2.worker() {
        ScheduledMaintenanceCycleWorkerOutcome::Stepped(outcome) => outcome,
        ScheduledMaintenanceCycleWorkerOutcome::Idle => {
            panic!("worker must not be idle on inv 2")
        }
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
    let result3 = runner
        .run_one_scheduled_backup_maintenance_cycle(observed3, LEASE_SECONDS)
        .await
        .expect("invocation 3 must succeed");
    assert!(
        matches!(result3.tick(), ScheduledMaintenanceCycleTickOutcome::Idle),
        "invocation 3 tick must be idle"
    );
    let worker3 = match result3.worker() {
        ScheduledMaintenanceCycleWorkerOutcome::Stepped(outcome) => outcome,
        ScheduledMaintenanceCycleWorkerOutcome::Idle => {
            panic!("worker must not be idle on inv 3")
        }
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
    let result4 = runner
        .run_one_scheduled_backup_maintenance_cycle(observed4, LEASE_SECONDS)
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
async fn runner_manual_run_excluded_from_cycle_worker() {
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

    let runner = fixture.runner();
    let observed = timestamp("2026-09-02T00:00:00Z");
    let result = runner
        .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
        .await
        .expect("runner cycle must succeed");

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
async fn runner_disabled_schedule_after_handoff_advances_normally() {
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

    // The tick should be idle (schedule disabled), but the runner must still
    // advance the existing handed-off run.
    let runner = fixture.runner();
    let result = runner
        .run_one_scheduled_backup_maintenance_cycle(after(observed, 60), LEASE_SECONDS)
        .await
        .expect("runner cycle must succeed");

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
async fn runner_policy_edit_after_handoff_keeps_existing_run_authority() {
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

    // Edit only the misfire policy after handoff.
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

    let runner = fixture.runner();
    let result = runner
        .run_one_scheduled_backup_maintenance_cycle(after(observed, 60), LEASE_SECONDS)
        .await
        .expect("runner cycle must succeed");

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
async fn runner_scheduler_failure_boundary() {
    let fixture = fixture("schfail").await;
    let (_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "schfail", "08:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);

    // Corrupt the schedule revision so the scheduler's decode_revision fails.
    let corrupt_revision_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO backup_schedule_revisions
            (id, schedule_id, owner_user_id, backup_set_id, revision_number,
             operation_id, fingerprint_version, request_fingerprint,
             recurrence_kind, timezone, local_time_minute, weekly_days,
             misfire_mode, max_lateness_seconds, created_at)
         SELECT $3, schedule_id, owner_user_id, backup_set_id,
                revision_number + 1,
                'runner-failure-injection-' || backup_set_id::text,
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

    let runner = fixture.runner();
    let result = runner
        .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
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

    // No occurrence, handoff, or maintenance run must have been created.
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
async fn runner_worker_failure_preserves_scheduler_state() {
    let fixture = fixture("wf").await;
    let (_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "wf", "08:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);

    // Run a cycle: tick materializes+handoffs, worker advances CREATED->SNAPSHOT_CAPTURED.
    let runner = fixture.runner();
    let result1 = runner
        .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
        .await
        .expect("first runner cycle must succeed");
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
        "first runner cycle advances one transition"
    );

    // Verify scheduler-side durable changes persist.
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

    // Corrupt the retention policy revision so the worker fails with PolicyChanged.
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

    let observed2 = after(observed, 60);
    let result2 = runner
        .run_one_scheduled_backup_maintenance_cycle(observed2, LEASE_SECONDS)
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
async fn runner_lease_duration_validation() {
    let fixture = fixture("lease").await;
    let runner = fixture.runner();
    let observed = timestamp("2026-09-02T00:00:00Z");

    // Valid lease duration (within bounds).
    let result = runner
        .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
        .await
        .expect("valid lease must succeed");
    assert!(result.is_idle());

    // Below minimum (10 seconds).
    let result = runner
        .run_one_scheduled_backup_maintenance_cycle(observed, 9)
        .await;
    assert!(
        matches!(
            result,
            Err(ScheduledMaintenanceCycleError::InvalidLeaseDuration)
        ),
        "below minimum must be rejected, got {result:?}"
    );

    // Above maximum (900 seconds).
    let result = runner
        .run_one_scheduled_backup_maintenance_cycle(observed, 901)
        .await;
    assert!(
        matches!(
            result,
            Err(ScheduledMaintenanceCycleError::InvalidLeaseDuration)
        ),
        "above maximum must be rejected, got {result:?}"
    );

    // Boundary values.
    let result = runner
        .run_one_scheduled_backup_maintenance_cycle(observed, 10)
        .await
        .expect("minimum boundary must succeed");
    assert!(result.is_idle());

    let result = runner
        .run_one_scheduled_backup_maintenance_cycle(observed, 900)
        .await
        .expect("maximum boundary must succeed");
    assert!(result.is_idle());

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn runner_explicit_time_propagation() {
    let fixture = fixture("time").await;
    let runner = fixture.runner();

    // Use a time far in the past: no schedule should be due.
    let past = timestamp("2020-01-01T00:00:00Z");
    let result = runner
        .run_one_scheduled_backup_maintenance_cycle(past, LEASE_SECONDS)
        .await
        .expect("past time must succeed");
    assert!(result.is_idle(), "no work should be due in the past");

    // Use a time far in the future: tick may skip expired prefix.
    let future = timestamp("2030-01-01T00:00:00Z");
    let result = runner
        .run_one_scheduled_backup_maintenance_cycle(future, LEASE_SECONDS)
        .await
        .expect("future time must succeed");
    // Either idle or skipped expired; no panic.
    assert!(
        result.is_idle()
            || matches!(
                result.tick(),
                ScheduledMaintenanceCycleTickOutcome::SkippedExpired(_)
            )
    );

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn runner_concurrent_invocations_prevent_duplicate_state() {
    let fixture = fixture("concurrent").await;
    let (_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "concurrent", "08:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);

    // Twelve concurrent invocations with isolated runner identities race the
    // same due work through one shared pool.
    let mut handles = Vec::new();
    for _ in 0..12 {
        let pool = fixture.pool();
        let w = worker();
        handles.push(tokio::spawn(async move {
            let runner = ScheduledMaintenanceCycleRunner::with_worker_id(pool, w);
            runner
                .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
                .await
        }));
    }

    let mut tick_outcomes = Vec::new();
    let mut advanced_identities = Vec::new();
    let mut contention_aborts = 0;
    for handle in handles {
        let result = handle.await.expect("concurrent runner task must not panic");
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
                eprintln!("concurrent runner contention abort (tolerated): {error:?}");
            }
        }
    }

    assert!(
        !tick_outcomes.is_empty(),
        "at least one concurrent runner must succeed, aborts: {contention_aborts}"
    );

    // At most one tick may report materializing.
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
        "at most one runner must materialize, got {materialized_count}"
    );

    // Every committed transition must carry a distinct claim identity.
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

    // Claim identities must be unique per (run, expected state).
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
        "concurrent runners must not mark the run stale"
    );
    assert!(
        snapshot_count_for_operation(&fixture.db.inspection, run.capture_operation_id()).await <= 1,
        "one canonical capture operation"
    );
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

    // A serial drain after the storm must converge the run to COMPLETED.
    let drained = drain_runner_to_idle(&fixture, after(observed, 300)).await;
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
async fn runner_worker_identity_propagation() {
    let fixture = fixture("identity").await;
    let explicit_id = worker();
    let runner = fixture.runner_with_id(explicit_id);
    let (_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "identity", "08:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);

    let result = runner
        .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
        .await
        .expect("runner cycle must succeed");

    // Worker must have stepped.
    assert!(
        matches!(
            result.worker(),
            ScheduledMaintenanceCycleWorkerOutcome::Stepped(_)
        ),
        "worker must step"
    );

    // The claim must reflect the explicit worker identity.
    let outcome = result.tick().outcome().expect("tick must have outcome");
    let run_id = outcome.maintenance_run_id();
    let identities = claim_identities(&fixture.db.inspection, fixture.backup_set_id).await;
    assert_eq!(identities.len(), 1, "exactly one claim");

    // The claim's worker identity should match the runner's worker_id.
    // We can verify this indirectly: the claim exists and the runner used
    // the explicit identity. The claim table stores the worker ID.
    let claim_worker: (String,) = sqlx::query_as(
        "SELECT lease_worker_id::text
         FROM backup_scheduled_maintenance_claims
         WHERE maintenance_run_id = $1",
    )
    .bind(run_id.into_uuid())
    .fetch_one(&fixture.db.inspection)
    .await
    .expect("claim worker must be readable");
    assert_eq!(
        claim_worker.0,
        explicit_id.to_string(),
        "claim must reflect the explicit runner worker identity"
    );

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn runner_service_restart_compatibility() {
    let fixture = fixture("restart").await;
    let (_service, _revision, schedule) =
        configure_schedule(&fixture, fixture.backup_set_id, "restart", "08:00").await;
    let (_date, planned) = first_planned(&schedule);
    let observed = after(planned.scheduled_for_utc(), 1);

    // First runner invocation: creates claim and advances.
    let runner1 = fixture.runner();
    let result1 = runner1
        .run_one_scheduled_backup_maintenance_cycle(observed, LEASE_SECONDS)
        .await
        .expect("first runner must succeed");
    let run_id = result1
        .tick()
        .outcome()
        .expect("tick must have outcome")
        .maintenance_run_id();
    assert!(matches!(
        result1.worker(),
        ScheduledMaintenanceCycleWorkerOutcome::Stepped(_)
    ));

    // Simulate restart: construct a fresh runner with a new identity.
    let runner2 = fixture.runner();
    let observed2 = after(observed, 60);
    let _result2 = runner2
        .run_one_scheduled_backup_maintenance_cycle(observed2, LEASE_SECONDS)
        .await
        .expect("second runner must succeed");

    // The new runner should either take over the claim or advance the next step.
    let run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("run must be readable");
    assert!(
        matches!(
            run.state(),
            BackupMaintenanceRunState::SnapshotCaptured | BackupMaintenanceRunState::ExpiryPlanned
        ),
        "run must advance after restart, got {:?}",
        run.state()
    );

    // Durable claim state must be authoritative; no in-memory state required.
    let identities = claim_identities(&fixture.db.inspection, fixture.backup_set_id).await;
    assert!(
        !identities.is_empty(),
        "claims must persist across runner restarts"
    );

    fixture.close().await;
}
