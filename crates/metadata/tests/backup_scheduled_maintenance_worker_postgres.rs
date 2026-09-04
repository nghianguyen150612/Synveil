//! PostgreSQL verification for Prompt 66's durable scheduled-maintenance
//! claim/lease with fenced exactly-one-transition worker primitive.
//!
//! Every test uses an isolated disposable database created on the same
//! server identified by `SYNVEIL_TEST_DATABASE_URL` (which must address the
//! maintenance database, e.g. `.../postgres`). The global worker discovery
//! scans all owners, so sharing one database across tests would contaminate
//! deterministic ordering assertions. Run with `--test-threads=1`.

use std::time::Duration as StdDuration;

use sqlx::PgPool;
use synveil_core::{
    BackupMaintenanceRunState, BackupScheduleConfig, BackupScheduleLocalTime,
    BackupScheduleMisfireMode, BackupScheduleOccurrenceMaterializationResult,
    BackupScheduleRevision, BackupScheduleTimezone, BackupScheduledMaintenanceClaim,
    BackupScheduledMaintenanceClaimOutcome, BackupScheduledMaintenanceLeaseToken,
    BackupScheduledMaintenanceStepResult, BackupScheduledMaintenanceWorkerId, BackupSetId,
    DedupDomainId, Library, LibraryId, LogicalName, Node, NodeId, Timestamp, User, UserId,
    UserStatus,
};
use synveil_metadata::{
    BackupScheduleService, BackupSchedulerService, BackupService, DatabaseConfig, DatabasePool,
    DomainRepository, MigrationRunner, ScheduledMaintenanceWorkerError,
    ScheduledMaintenanceWorkerService,
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
        "p66_{}_{}",
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
    assert_eq!(status.applied_versions().len(), 34);
    assert_eq!(status.latest_applied_version(), Some(20260903000001));
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
                format!("sched-worker-{label}-{owner_user_id}"),
                format!("sched-worker-key-{owner_user_id}"),
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
        "UPDATE backup_sets
         SET state = 'ACTIVE', revision = revision + 1,
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

struct ScheduledRun {
    #[allow(dead_code)]
    schedule_revision: BackupScheduleRevision,
    occurrence_id: synveil_core::BackupScheduleOccurrenceId,
    scheduled_for_utc: Timestamp,
    run_id: synveil_core::BackupMaintenanceRunId,
}

async fn schedule_and_handoff(
    fixture: &Fixture,
    backup_set_id: BackupSetId,
    label: &str,
    local_time_value: &str,
) -> ScheduledRun {
    let service = BackupScheduleService::new(fixture.pool());
    let revision = service
        .configure_backup_schedule(
            fixture.owner_user_id,
            format!("schedule-{label}-{backup_set_id}"),
            backup_set_id,
            daily(local_time_value),
        )
        .await
        .expect("schedule revision must persist");
    let schedule = service
        .get_backup_schedule(fixture.owner_user_id, backup_set_id)
        .await
        .expect("schedule must be readable");
    let date = (schedule.effective_from().as_offset_datetime() + TimeDuration::days(1)).date();
    let planned = schedule
        .current_revision()
        .occurrence_on_local_date(date)
        .expect("daily schedule must plan the next local date");
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
    ScheduledRun {
        schedule_revision: revision,
        occurrence_id: occurrence.id(),
        scheduled_for_utc: occurrence.scheduled_for_utc(),
        run_id: handoff.maintenance_run().id(),
    }
}

async fn run_state(
    fixture: &Fixture,
    run_id: synveil_core::BackupMaintenanceRunId,
) -> BackupMaintenanceRunState {
    fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, run_id)
        .await
        .expect("maintenance run must be readable")
        .state()
}

async fn claim_count(inspection: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM backup_scheduled_maintenance_claims")
        .fetch_one(inspection)
        .await
        .expect("claim count must succeed")
}

async fn snapshot_count_for_operation(inspection: &PgPool, operation_id: &str) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM backup_snapshots WHERE operation_id = $1")
        .bind(operation_id)
        .fetch_one(inspection)
        .await
        .expect("snapshot count must succeed")
}

fn claimed(outcome: BackupScheduledMaintenanceClaimOutcome) -> BackupScheduledMaintenanceClaim {
    match outcome {
        BackupScheduledMaintenanceClaimOutcome::Claimed(claim)
        | BackupScheduledMaintenanceClaimOutcome::ExistingCurrentLease(claim)
        | BackupScheduledMaintenanceClaimOutcome::TakenOver(claim)
        | BackupScheduledMaintenanceClaimOutcome::RecoveredCompletion(claim) => claim,
        BackupScheduledMaintenanceClaimOutcome::Idle => panic!("expected a claim, got IDLE"),
    }
}

async fn execute(
    fixture: &Fixture,
    claim: &BackupScheduledMaintenanceClaim,
    observed: Timestamp,
) -> Result<BackupScheduledMaintenanceStepResult, ScheduledMaintenanceWorkerError> {
    fixture
        .worker_service()
        .execute_claimed_scheduled_maintenance_step(
            claim.claim_id(),
            claim.lease_worker_id(),
            claim.lease_token(),
            claim.lease_generation(),
            observed,
        )
        .await
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn fresh_migration_applies_all_34_from_empty() {
    let db = new_db("freshcount").await;
    let version: String = sqlx::query_scalar("SELECT version()")
        .fetch_one(&db.inspection)
        .await
        .expect("version query must succeed");
    assert!(
        version.contains("PostgreSQL 17."),
        "worker tests require PostgreSQL 17, got {version}"
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn scheduled_run_is_discovered_and_manual_run_is_excluded() {
    let fixture = fixture("discovery").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "sched", "08:00").await;

    let w = worker();
    let t0 = after(scheduled.scheduled_for_utc, 60);
    let outcome = fixture
        .worker_service()
        .claim_next_scheduled_maintenance_step(w, t0, LEASE_SECONDS)
        .await
        .expect("discovery must succeed");
    let claim = claimed(outcome);
    assert_eq!(claim.maintenance_run_id(), scheduled.run_id);
    assert_eq!(claim.expected_state(), BackupMaintenanceRunState::Created);
    assert_eq!(claim.lease_generation(), 1);
    assert_eq!(claim.lease_worker_id(), w);

    // Claim-only acquisition leaves maintenance state untouched.
    assert_eq!(
        run_state(&fixture, scheduled.run_id).await,
        BackupMaintenanceRunState::Created
    );

    // Finish the scheduled run through the worker, then prove a lone manual
    // run is invisible to discovery.
    let mut observed = after(t0, 1);
    for _ in 0..3 {
        let step = fixture
            .worker_service()
            .run_scheduled_maintenance_worker_step(w, observed, LEASE_SECONDS)
            .await
            .expect("worker step must succeed");
        assert!(!step.is_idle());
        observed = after(observed, 5);
    }
    assert_eq!(
        run_state(&fixture, scheduled.run_id).await,
        BackupMaintenanceRunState::Completed
    );

    let manual_run = fixture
        .backup_service()
        .create_backup_maintenance_run(
            fixture.owner_user_id,
            format!("manual-after-{}", fixture.backup_set_id),
            fixture.backup_set_id,
        )
        .await
        .expect("manual run must persist after scheduled completion");
    let idle = fixture
        .worker_service()
        .claim_next_scheduled_maintenance_step(worker(), after(observed, 5), LEASE_SECONDS)
        .await
        .expect("discovery must succeed");
    assert!(idle.is_idle(), "manual runs must never be claimed");
    assert_eq!(
        run_state(&fixture, manual_run.id()).await,
        BackupMaintenanceRunState::Created
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn claim_only_has_no_backup_side_effects() {
    let fixture = fixture("claimside").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "side", "08:00").await;
    let before_snapshots: i64 =
        sqlx::query_scalar("SELECT count(*) FROM backup_snapshots WHERE owner_user_id = $1")
            .bind(fixture.owner_user_id.into_uuid())
            .fetch_one(&fixture.db.inspection)
            .await
            .expect("snapshot count must succeed");
    let before_plans: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_expiry_plans WHERE owner_user_id = $1",
    )
    .bind(fixture.owner_user_id.into_uuid())
    .fetch_one(&fixture.db.inspection)
    .await
    .expect("plan count must succeed");

    let t0 = after(scheduled.scheduled_for_utc, 60);
    let claim_side_effects = claimed(
        fixture
            .worker_service()
            .claim_next_scheduled_maintenance_step(worker(), t0, LEASE_SECONDS)
            .await
            .expect("claim must succeed"),
    );
    assert_eq!(
        claim_side_effects.expected_state(),
        BackupMaintenanceRunState::Created
    );
    assert_eq!(claim_count(&fixture.db.inspection).await, 1);
    assert_eq!(
        run_state(&fixture, scheduled.run_id).await,
        BackupMaintenanceRunState::Created
    );
    let after_snapshots: i64 =
        sqlx::query_scalar("SELECT count(*) FROM backup_snapshots WHERE owner_user_id = $1")
            .bind(fixture.owner_user_id.into_uuid())
            .fetch_one(&fixture.db.inspection)
            .await
            .expect("snapshot count must succeed");
    let after_plans: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_expiry_plans WHERE owner_user_id = $1",
    )
    .bind(fixture.owner_user_id.into_uuid())
    .fetch_one(&fixture.db.inspection)
    .await
    .expect("plan count must succeed");
    assert_eq!(
        before_snapshots, after_snapshots,
        "claim must capture no snapshot"
    );
    assert_eq!(before_plans, after_plans, "claim must plan no expiry");
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn three_explicit_steps_complete_one_run_and_fourth_is_idle() {
    let fixture = fixture("threestep").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "three", "08:00").await;
    let service = fixture.worker_service();
    let w = worker();
    let mut observed = after(scheduled.scheduled_for_utc, 60);

    let expected = [
        (
            BackupMaintenanceRunState::Created,
            BackupMaintenanceRunState::SnapshotCaptured,
        ),
        (
            BackupMaintenanceRunState::SnapshotCaptured,
            BackupMaintenanceRunState::ExpiryPlanned,
        ),
        (
            BackupMaintenanceRunState::ExpiryPlanned,
            BackupMaintenanceRunState::Completed,
        ),
    ];
    for (from, to) in expected {
        let discovery = service
            .claim_next_scheduled_maintenance_step(w, observed, LEASE_SECONDS)
            .await
            .expect("discovery must succeed");
        let claim = claimed(discovery);
        assert_eq!(claim.expected_state(), from);
        // Same-holder replay before expiry returns the canonical open claim.
        let replay = service
            .claim_next_scheduled_maintenance_step(w, after(observed, 1), LEASE_SECONDS)
            .await
            .expect("replay must succeed");
        match replay {
            BackupScheduledMaintenanceClaimOutcome::ExistingCurrentLease(existing) => {
                assert_eq!(existing.claim_id(), claim.claim_id());
                assert_eq!(existing.lease_token(), claim.lease_token());
            }
            other => panic!("expected same-holder replay, got {other:?}"),
        }
        let result = execute(&fixture, &claim, after(observed, 2))
            .await
            .expect("execution must succeed");
        match result {
            BackupScheduledMaintenanceStepResult::Advanced {
                claim: completed,
                maintenance_run,
            } => {
                assert_eq!(completed.claim_id(), claim.claim_id());
                assert!(completed.is_completed());
                assert_eq!(completed.resulting_state(), Some(to));
                assert_eq!(maintenance_run.state(), to);
            }
            other => panic!("expected ADVANCED, got {other:?}"),
        }
        assert_eq!(run_state(&fixture, scheduled.run_id).await, to);
        observed = after(observed, 10);
    }
    let idle = service
        .claim_next_scheduled_maintenance_step(w, observed, LEASE_SECONDS)
        .await
        .expect("final discovery must succeed");
    assert!(idle.is_idle(), "completed runs are not claimable");
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn invalid_lease_durations_are_rejected_without_clamping() {
    let fixture = fixture("leasedur").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "dur", "08:00").await;
    let t0 = after(scheduled.scheduled_for_utc, 60);
    for invalid in [0, 1, 9, 901, 3_600, u64::MAX] {
        let result = fixture
            .worker_service()
            .claim_next_scheduled_maintenance_step(worker(), t0, invalid)
            .await;
        assert_eq!(
            result,
            Err(ScheduledMaintenanceWorkerError::InvalidLeaseDuration),
            "duration {invalid} must be rejected"
        );
    }
    assert_eq!(claim_count(&fixture.db.inspection).await, 0);
    // Boundary durations are accepted; the minimum is exercised functionally
    // by the takeover tests that use short leases.
    let outcome = fixture
        .worker_service()
        .claim_next_scheduled_maintenance_step(worker(), t0, 10)
        .await
        .expect("minimum bounded duration must be accepted");
    let minimum = claimed(outcome);
    assert_eq!(minimum.expected_state(), BackupMaintenanceRunState::Created);
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn exact_expiry_boundary_governs_takeover() {
    let fixture = fixture("expiry").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "exp", "08:00").await;
    let service = fixture.worker_service();
    let a = worker();
    let b = worker();
    let t0 = after(scheduled.scheduled_for_utc, 60);
    let first = claimed(
        service
            .claim_next_scheduled_maintenance_step(a, t0, 60)
            .await
            .expect("first claim must succeed"),
    );
    assert_eq!(first.lease_generation(), 1);

    // One second before expiry another worker cannot steal the claim.
    let early = service
        .claim_next_scheduled_maintenance_step(b, after(t0, 59), 60)
        .await
        .expect("early discovery must succeed");
    assert!(early.is_idle(), "early steal must be forbidden");
    assert_eq!(claim_count(&fixture.db.inspection).await, 1);

    // Exactly at expiry the takeover is authorized.
    let at_expiry = after(t0, 60);
    let taken = service
        .claim_next_scheduled_maintenance_step(b, at_expiry, 60)
        .await
        .expect("takeover must succeed");
    match taken {
        BackupScheduledMaintenanceClaimOutcome::TakenOver(next) => {
            assert_eq!(next.claim_id(), first.claim_id());
            assert_eq!(next.lease_generation(), 2);
            assert_eq!(next.lease_worker_id(), b);
            assert_ne!(next.lease_token(), first.lease_token());
        }
        other => panic!("expected takeover at exact expiry, got {other:?}"),
    }
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn wrong_token_generation_and_worker_are_fenced() {
    let fixture = fixture("fence").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "fence", "08:00").await;
    let service = fixture.worker_service();
    let t0 = after(scheduled.scheduled_for_utc, 60);
    let claim = claimed(
        service
            .claim_next_scheduled_maintenance_step(worker(), t0, LEASE_SECONDS)
            .await
            .expect("claim must succeed"),
    );

    // Wrong token with correct worker/generation.
    let wrong_token = service
        .execute_claimed_scheduled_maintenance_step(
            claim.claim_id(),
            claim.lease_worker_id(),
            BackupScheduledMaintenanceLeaseToken::new(),
            claim.lease_generation(),
            after(t0, 1),
        )
        .await;
    assert_eq!(wrong_token, Err(ScheduledMaintenanceWorkerError::LeaseLost));

    // Stale generation with correct token.
    let wrong_generation = service
        .execute_claimed_scheduled_maintenance_step(
            claim.claim_id(),
            claim.lease_worker_id(),
            claim.lease_token(),
            claim.lease_generation() + 1,
            after(t0, 1),
        )
        .await;
    assert_eq!(
        wrong_generation,
        Err(ScheduledMaintenanceWorkerError::LeaseLost)
    );

    // Different worker presenting another holder's active token.
    let wrong_worker = service
        .execute_claimed_scheduled_maintenance_step(
            claim.claim_id(),
            worker(),
            claim.lease_token(),
            claim.lease_generation(),
            after(t0, 1),
        )
        .await;
    assert_eq!(
        wrong_worker,
        Err(ScheduledMaintenanceWorkerError::LeaseLost)
    );

    assert_eq!(
        run_state(&fixture, scheduled.run_id).await,
        BackupMaintenanceRunState::Created,
        "fenced rejections must commit zero transitions"
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn stale_worker_cannot_commit_after_takeover() {
    let fixture = fixture("stalew").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "stale", "08:00").await;
    let service = fixture.worker_service();
    let t0 = after(scheduled.scheduled_for_utc, 60);
    let stale = claimed(
        service
            .claim_next_scheduled_maintenance_step(worker(), t0, 60)
            .await
            .expect("first claim must succeed"),
    );
    let taken = claimed(
        service
            .claim_next_scheduled_maintenance_step(worker(), after(t0, 60), 60)
            .await
            .expect("takeover must succeed"),
    );
    assert_eq!(taken.lease_generation(), 2);

    let stale_attempt = service
        .execute_claimed_scheduled_maintenance_step(
            stale.claim_id(),
            stale.lease_worker_id(),
            stale.lease_token(),
            stale.lease_generation(),
            after(t0, 61),
        )
        .await;
    assert_eq!(
        stale_attempt,
        Err(ScheduledMaintenanceWorkerError::LeaseLost)
    );
    assert_eq!(
        run_state(&fixture, scheduled.run_id).await,
        BackupMaintenanceRunState::Created
    );

    let fresh = execute(&fixture, &taken, after(t0, 62))
        .await
        .expect("current holder must advance");
    assert!(
        matches!(fresh, BackupScheduledMaintenanceStepResult::Advanced { .. }),
        "current generation must commit one transition, got {fresh:?}"
    );
    assert_eq!(
        run_state(&fixture, scheduled.run_id).await,
        BackupMaintenanceRunState::SnapshotCaptured
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn stale_worker_after_child_work_leaves_canonical_child_operation() {
    let fixture = fixture("stalechild").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "child", "08:00").await;
    let service = fixture.worker_service();
    let t0 = after(scheduled.scheduled_for_utc, 60);
    let stale = claimed(
        service
            .claim_next_scheduled_maintenance_step(worker(), t0, 60)
            .await
            .expect("first claim must succeed"),
    );
    // Worker A performs the canonical idempotent child work, then stalls
    // while its lease expires: the snapshot replays under the run's durable
    // capture operation identity.
    let run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, scheduled.run_id)
        .await
        .expect("run must be readable");
    fixture
        .backup_service()
        .capture_snapshot(
            fixture.owner_user_id,
            fixture.backup_set_id,
            synveil_core::SnapshotId::new(),
            run.capture_operation_id().to_owned(),
        )
        .await
        .expect("canonical child capture must succeed");

    let taken = claimed(
        service
            .claim_next_scheduled_maintenance_step(worker(), after(t0, 60), 60)
            .await
            .expect("takeover must succeed"),
    );
    let stale_attempt = service
        .execute_claimed_scheduled_maintenance_step(
            stale.claim_id(),
            stale.lease_worker_id(),
            stale.lease_token(),
            stale.lease_generation(),
            after(t0, 61),
        )
        .await;
    assert_eq!(
        stale_attempt,
        Err(ScheduledMaintenanceWorkerError::LeaseLost)
    );

    let fresh = execute(&fixture, &taken, after(t0, 62))
        .await
        .expect("current holder must advance");
    assert!(matches!(
        fresh,
        BackupScheduledMaintenanceStepResult::Advanced { .. }
    ));
    assert_eq!(
        snapshot_count_for_operation(&fixture.db.inspection, run.capture_operation_id()).await,
        1,
        "canonical child operation must exist exactly once"
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn crash_before_advance_recovers_with_single_transition() {
    let fixture = fixture("crashbefore").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "cba", "08:00").await;
    let service = fixture.worker_service();
    let t0 = after(scheduled.scheduled_for_utc, 60);
    let first = claimed(
        service
            .claim_next_scheduled_maintenance_step(worker(), t0, 60)
            .await
            .expect("claim must succeed"),
    );
    assert_eq!(claim_count(&fixture.db.inspection).await, 1);
    assert_eq!(
        run_state(&fixture, scheduled.run_id).await,
        BackupMaintenanceRunState::Created
    );

    // Worker crashes before any execution; after expiry a new worker takes
    // over the same claim identity and advances exactly once.
    let taken = claimed(
        service
            .claim_next_scheduled_maintenance_step(worker(), after(t0, 60), 60)
            .await
            .expect("takeover must succeed"),
    );
    assert_eq!(taken.claim_id(), first.claim_id());
    assert_eq!(taken.lease_generation(), 2);
    assert_eq!(claim_count(&fixture.db.inspection).await, 1);
    let result = execute(&fixture, &taken, after(t0, 61))
        .await
        .expect("takeover execution must succeed");
    assert!(matches!(
        result,
        BackupScheduledMaintenanceStepResult::Advanced { .. }
    ));
    assert_eq!(
        run_state(&fixture, scheduled.run_id).await,
        BackupMaintenanceRunState::SnapshotCaptured
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn crash_after_advance_reconciles_without_second_transition() {
    let fixture = fixture("crashafter").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "caa", "08:00").await;
    let service = fixture.worker_service();
    let t0 = after(scheduled.scheduled_for_utc, 60);
    let claim = claimed(
        service
            .claim_next_scheduled_maintenance_step(worker(), t0, LEASE_SECONDS)
            .await
            .expect("claim must succeed"),
    );
    // Faithful crash injection: canonical child work plus the durable Prompt
    // 49 transition commit, without the claim completion receipt.
    let run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, scheduled.run_id)
        .await
        .expect("run must be readable");
    let snapshot = fixture
        .backup_service()
        .capture_snapshot(
            fixture.owner_user_id,
            fixture.backup_set_id,
            synveil_core::SnapshotId::new(),
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
    .bind(scheduled.run_id.into_uuid())
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

    let result = execute(&fixture, &claim, after(t0, 1))
        .await
        .expect("recovery must succeed");
    match result {
        BackupScheduledMaintenanceStepResult::RecoveredCompletion {
            claim: completed,
            maintenance_run,
        } => {
            assert_eq!(completed.claim_id(), claim.claim_id());
            assert!(completed.is_completed());
            assert_eq!(
                completed.resulting_state(),
                Some(BackupMaintenanceRunState::SnapshotCaptured)
            );
            assert_eq!(
                maintenance_run.state(),
                BackupMaintenanceRunState::SnapshotCaptured
            );
        }
        other => panic!("expected RECOVERED_COMPLETION, got {other:?}"),
    }
    // Recovery stops: the run must not advance again in any follow-up claim
    // for the reconciled step, and the next step is a distinct claim.
    assert_eq!(
        run_state(&fixture, scheduled.run_id).await,
        BackupMaintenanceRunState::SnapshotCaptured
    );
    let next = claimed(
        service
            .claim_next_scheduled_maintenance_step(worker(), after(t0, 2), LEASE_SECONDS)
            .await
            .expect("next discovery must succeed"),
    );
    assert_ne!(next.claim_id(), claim.claim_id());
    assert_eq!(
        next.expected_state(),
        BackupMaintenanceRunState::SnapshotCaptured
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn crash_after_final_advance_recovers_completed_receipt() {
    let fixture = fixture("crashfinal").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "cf", "08:00").await;
    let service = fixture.worker_service();
    // Drive the run to EXPIRY_PLANNED through two worker steps.
    let mut observed = after(scheduled.scheduled_for_utc, 60);
    for _ in 0..2 {
        let outcome = service
            .run_scheduled_maintenance_worker_step(worker(), observed, LEASE_SECONDS)
            .await
            .expect("worker step must succeed");
        assert!(!outcome.is_idle());
        observed = after(observed, 10);
    }
    assert_eq!(
        run_state(&fixture, scheduled.run_id).await,
        BackupMaintenanceRunState::ExpiryPlanned
    );

    // Claim the final step, then inject the crash: transition committed,
    // receipt open, run terminal.
    let claim = claimed(
        service
            .claim_next_scheduled_maintenance_step(worker(), observed, LEASE_SECONDS)
            .await
            .expect("final claim must succeed"),
    );
    assert_eq!(
        claim.expected_state(),
        BackupMaintenanceRunState::ExpiryPlanned
    );
    let run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, scheduled.run_id)
        .await
        .expect("run must be readable");
    let execution = fixture
        .backup_service()
        .execute_snapshot_expiry_plan(
            fixture.owner_user_id,
            run.expiry_plan_id().expect("plan must exist"),
        )
        .await
        .expect("canonical expiry execution must succeed");
    sqlx::query(
        "UPDATE backup_maintenance_runs
         SET state = 'COMPLETED', expiry_execution_id = $3,
             maintenance_completed_at = $4
         WHERE id = $1 AND owner_user_id = $2 AND state = 'EXPIRY_PLANNED'",
    )
    .bind(scheduled.run_id.into_uuid())
    .bind(fixture.owner_user_id.into_uuid())
    .bind(execution.id().into_uuid())
    .bind(execution.executed_at().as_offset_datetime())
    .execute(&fixture.db.inspection)
    .await
    .expect("crash injection must commit the final transition");

    let result = execute(&fixture, &claim, after(observed, 1))
        .await
        .expect("final recovery must succeed");
    assert!(
        matches!(
            result,
            BackupScheduledMaintenanceStepResult::RecoveredCompletion { .. }
        ),
        "terminal recovery must reconcile, got {result:?}"
    );
    let idle = service
        .claim_next_scheduled_maintenance_step(worker(), after(observed, 2), LEASE_SECONDS)
        .await
        .expect("post-recovery discovery must succeed");
    assert!(idle.is_idle());
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn lost_completion_response_replays_canonical_receipt() {
    let fixture = fixture("lostresp").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "lr", "08:00").await;
    let service = fixture.worker_service();
    let t0 = after(scheduled.scheduled_for_utc, 60);
    let claim = claimed(
        service
            .claim_next_scheduled_maintenance_step(worker(), t0, LEASE_SECONDS)
            .await
            .expect("claim must succeed"),
    );
    let first = execute(&fixture, &claim, after(t0, 1))
        .await
        .expect("execution must succeed");
    assert!(matches!(
        first,
        BackupScheduledMaintenanceStepResult::Advanced { .. }
    ));

    // The committed receipt replays canonically; no next-state work happens.
    let retry = execute(&fixture, &claim, after(t0, 2))
        .await
        .expect("retry must succeed");
    match retry {
        BackupScheduledMaintenanceStepResult::AlreadyCompleted { claim: completed } => {
            assert_eq!(completed.claim_id(), claim.claim_id());
            assert!(completed.is_completed());
        }
        other => panic!("expected ALREADY_COMPLETED, got {other:?}"),
    }
    assert_eq!(
        run_state(&fixture, scheduled.run_id).await,
        BackupMaintenanceRunState::SnapshotCaptured,
        "replay must not advance to the next state"
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn unexpected_run_state_fails_closed() {
    let fixture = fixture("unexpected").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "ux", "08:00").await;
    let service = fixture.worker_service();
    let t0 = after(scheduled.scheduled_for_utc, 60);
    let claim = claimed(
        service
            .claim_next_scheduled_maintenance_step(worker(), t0, LEASE_SECONDS)
            .await
            .expect("claim must succeed"),
    );
    // Move the run two legitimate transitions ahead (CREATED ->
    // SNAPSHOT_CAPTURED -> EXPIRY_PLANNED) with canonical child references
    // while the CREATED claim stays open: the worker must not guess that
    // both happened legitimately under this claim.
    let run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, scheduled.run_id)
        .await
        .expect("run must be readable");
    let snapshot = fixture
        .backup_service()
        .capture_snapshot(
            fixture.owner_user_id,
            fixture.backup_set_id,
            synveil_core::SnapshotId::new(),
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
    .bind(scheduled.run_id.into_uuid())
    .bind(fixture.owner_user_id.into_uuid())
    .bind(snapshot.id().into_uuid())
    .bind(
        snapshot
            .committed_at()
            .expect("snapshot must commit")
            .as_offset_datetime(),
    )
    .execute(&fixture.db.inspection)
    .await
    .expect("first injected transition must apply");
    let plan = fixture
        .backup_service()
        .create_snapshot_expiry_plan(
            fixture.owner_user_id,
            run.expiry_plan_operation_id().to_owned(),
            fixture.backup_set_id,
        )
        .await
        .expect("canonical expiry plan must succeed");
    sqlx::query(
        "UPDATE backup_maintenance_runs
         SET state = 'EXPIRY_PLANNED', expiry_plan_id = $3,
             expiry_planned_at = $4
         WHERE id = $1 AND owner_user_id = $2 AND state = 'SNAPSHOT_CAPTURED'",
    )
    .bind(scheduled.run_id.into_uuid())
    .bind(fixture.owner_user_id.into_uuid())
    .bind(plan.id().into_uuid())
    .bind(plan.created_at().as_offset_datetime())
    .execute(&fixture.db.inspection)
    .await
    .expect("second injected transition must apply");

    let result = execute(&fixture, &claim, after(t0, 1)).await;
    assert_eq!(
        result,
        Err(ScheduledMaintenanceWorkerError::InconsistentState),
        "unexpected state must fail closed"
    );
    let stored = service
        .get_scheduled_maintenance_claim(fixture.owner_user_id, claim.claim_id())
        .await
        .expect("claim read must succeed")
        .expect("claim must still exist");
    assert!(!stored.is_completed(), "no fake success may be recorded");
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn stale_run_is_not_claimable_and_execution_reports_stale() {
    let fixture = fixture("stalerun").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "sr", "08:00").await;
    let service = fixture.worker_service();
    let t0 = after(scheduled.scheduled_for_utc, 60);
    let claim = claimed(
        service
            .claim_next_scheduled_maintenance_step(worker(), t0, LEASE_SECONDS)
            .await
            .expect("claim must succeed"),
    );
    sqlx::query(
        "UPDATE backup_maintenance_runs
         SET state = 'STALE', stale_at = clock_timestamp()
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(scheduled.run_id.into_uuid())
    .bind(fixture.owner_user_id.into_uuid())
    .execute(&fixture.db.inspection)
    .await
    .expect("run must become stale");
    let result = execute(&fixture, &claim, after(t0, 1)).await;
    assert_eq!(result, Err(ScheduledMaintenanceWorkerError::RunStale));
    let stored = service
        .get_scheduled_maintenance_claim(fixture.owner_user_id, claim.claim_id())
        .await
        .expect("claim read must succeed")
        .expect("claim must still exist");
    assert!(
        !stored.is_completed(),
        "stale runs never earn a normal receipt"
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn twelve_concurrent_claims_converge_to_one_row() {
    let fixture = fixture("concclaim").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "cc", "08:00").await;
    let t0 = after(scheduled.scheduled_for_utc, 60);
    let service = fixture.worker_service();
    let mut handles = Vec::new();
    for _ in 0..12 {
        let service = service.clone();
        handles.push(tokio::spawn(async move {
            service
                .claim_next_scheduled_maintenance_step(worker(), t0, LEASE_SECONDS)
                .await
        }));
    }
    let mut claimed_count = 0;
    for handle in handles {
        let outcome = handle
            .await
            .expect("claim task must join")
            .expect("claim must succeed");
        if !outcome.is_idle() {
            claimed_count += 1;
        }
    }
    assert_eq!(claim_count(&fixture.db.inspection).await, 1);
    assert_eq!(claimed_count, 1, "exactly one worker must hold the claim");
    assert_eq!(
        run_state(&fixture, scheduled.run_id).await,
        BackupMaintenanceRunState::Created
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn twelve_concurrent_executions_commit_one_transition() {
    let fixture = fixture("concexec").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "ce", "08:00").await;
    let service = fixture.worker_service();
    let t0 = after(scheduled.scheduled_for_utc, 60);
    let claim = claimed(
        service
            .claim_next_scheduled_maintenance_step(worker(), t0, LEASE_SECONDS)
            .await
            .expect("claim must succeed"),
    );
    let observed = after(t0, 1);
    let mut handles = Vec::new();
    for _ in 0..12 {
        let service = service.clone();
        let (claim_id, worker_id, token, generation) = (
            claim.claim_id(),
            claim.lease_worker_id(),
            claim.lease_token(),
            claim.lease_generation(),
        );
        handles.push(tokio::spawn(async move {
            service
                .execute_claimed_scheduled_maintenance_step(
                    claim_id, worker_id, token, generation, observed,
                )
                .await
        }));
    }
    let mut progressed = 0;
    let mut replayed = 0;
    for handle in handles {
        match handle
            .await
            .expect("execution task must join")
            .expect("execution must succeed")
        {
            BackupScheduledMaintenanceStepResult::Advanced { .. }
            | BackupScheduledMaintenanceStepResult::RecoveredCompletion { .. } => progressed += 1,
            BackupScheduledMaintenanceStepResult::AlreadyCompleted { .. } => replayed += 1,
            BackupScheduledMaintenanceStepResult::LeaseLost => {
                panic!("shared fence must not report lease lost")
            }
        }
    }
    // Exactly one semantic transition commits and exactly one receipt seals;
    // racers either share the recovery or replay the canonical receipt.
    assert!(
        progressed >= 1,
        "at least one caller must progress the step"
    );
    assert_eq!(progressed + replayed, 12);
    let run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, scheduled.run_id)
        .await
        .expect("run must be readable");
    assert!(run.captured_snapshot_id().is_some());
    let snapshots_for_operation =
        snapshot_count_for_operation(&fixture.db.inspection, run.capture_operation_id()).await;
    assert_eq!(snapshots_for_operation, 1, "one canonical child operation");
    assert_eq!(
        run_state(&fixture, scheduled.run_id).await,
        BackupMaintenanceRunState::SnapshotCaptured
    );
    let stored = service
        .get_scheduled_maintenance_claim(fixture.owner_user_id, claim.claim_id())
        .await
        .expect("claim read must succeed")
        .expect("claim must exist");
    assert!(stored.is_completed());
    assert_eq!(
        stored.resulting_state(),
        Some(BackupMaintenanceRunState::SnapshotCaptured)
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn global_order_is_oldest_first_with_active_lease_skip() {
    let fixture = fixture("ordering").await;
    // Three independent BackupSets so each run owns its active-run slot.
    let mut sets = Vec::new();
    for label in ["ord-a", "ord-b", "ord-c"] {
        let set = BackupSetId::new();
        BackupService::new(fixture.pool())
            .create_backup_set(
                fixture.owner_user_id,
                set,
                name(format!("backup-{label}-{set}")),
                fixture.library_id,
                None,
                timestamp("2026-09-02T00:00:01Z"),
            )
            .await
            .expect("set must persist");
        sqlx::query(
            "UPDATE backup_sets SET state = 'ACTIVE', revision = revision + 1,
                 updated_at = clock_timestamp()
             WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(set.into_uuid())
        .bind(fixture.owner_user_id.into_uuid())
        .execute(&fixture.db.inspection)
        .await
        .expect("set must become active");
        BackupService::new(fixture.pool())
            .configure_snapshot_retention_policy(
                fixture.owner_user_id,
                format!("policy-{label}-{set}"),
                set,
                1,
                86_400,
            )
            .await
            .expect("policy must persist");
        sets.push(set);
    }
    let run_a = schedule_and_handoff(&fixture, sets[0], "ord-a", "08:00").await;
    let run_b = schedule_and_handoff(&fixture, sets[1], "ord-b", "07:00").await;
    let run_c = schedule_and_handoff(&fixture, sets[2], "ord-c", "09:00").await;
    assert!(
        run_b.scheduled_for_utc < run_a.scheduled_for_utc
            && run_a.scheduled_for_utc < run_c.scheduled_for_utc,
        "fixture must order B < A < C"
    );

    let service = fixture.worker_service();
    let t0 = after(run_c.scheduled_for_utc, 60);
    let first = claimed(
        service
            .claim_next_scheduled_maintenance_step(worker(), t0, LEASE_SECONDS)
            .await
            .expect("discovery must succeed"),
    );
    assert_eq!(
        first.maintenance_run_id(),
        run_b.run_id,
        "oldest work first"
    );

    // The oldest run now carries another worker's active lease, so the next
    // worker skips it and claims the second-oldest run.
    let second = claimed(
        service
            .claim_next_scheduled_maintenance_step(worker(), after(t0, 1), LEASE_SECONDS)
            .await
            .expect("skip discovery must succeed"),
    );
    assert_eq!(second.maintenance_run_id(), run_a.run_id);

    // After the oldest lease expires, takeover of the oldest claim wins over
    // claiming still-newer work.
    let retake = service
        .claim_next_scheduled_maintenance_step(worker(), after(t0, LEASE_SECONDS), LEASE_SECONDS)
        .await
        .expect("priority discovery must succeed");
    match retake {
        BackupScheduledMaintenanceClaimOutcome::TakenOver(retaken) => {
            assert_eq!(retaken.maintenance_run_id(), run_b.run_id);
            assert_eq!(retaken.lease_generation(), 2);
        }
        other => panic!("expected expired-lease priority takeover, got {other:?}"),
    }
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn recovery_has_priority_when_oldest() {
    let fixture = fixture("recprior").await;
    let mut sets = Vec::new();
    for label in ["rp-a", "rp-b"] {
        let set = BackupSetId::new();
        BackupService::new(fixture.pool())
            .create_backup_set(
                fixture.owner_user_id,
                set,
                name(format!("backup-{label}-{set}")),
                fixture.library_id,
                None,
                timestamp("2026-09-02T00:00:01Z"),
            )
            .await
            .expect("set must persist");
        sqlx::query(
            "UPDATE backup_sets SET state = 'ACTIVE', revision = revision + 1,
                 updated_at = clock_timestamp()
             WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(set.into_uuid())
        .bind(fixture.owner_user_id.into_uuid())
        .execute(&fixture.db.inspection)
        .await
        .expect("set must become active");
        BackupService::new(fixture.pool())
            .configure_snapshot_retention_policy(
                fixture.owner_user_id,
                format!("policy-{label}-{set}"),
                set,
                1,
                86_400,
            )
            .await
            .expect("policy must persist");
        sets.push(set);
    }
    let older = schedule_and_handoff(&fixture, sets[0], "rp-a", "07:00").await;
    let newer = schedule_and_handoff(&fixture, sets[1], "rp-b", "08:00").await;
    let service = fixture.worker_service();
    let t0 = after(newer.scheduled_for_utc, 60);

    // Crash injection on the oldest run: transition committed, receipt open.
    let stale_claim = claimed(
        service
            .claim_next_scheduled_maintenance_step(worker(), t0, LEASE_SECONDS)
            .await
            .expect("oldest claim must succeed"),
    );
    assert_eq!(stale_claim.maintenance_run_id(), older.run_id);
    let run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, older.run_id)
        .await
        .expect("run must be readable");
    let snapshot = fixture
        .backup_service()
        .capture_snapshot(
            fixture.owner_user_id,
            sets[0],
            synveil_core::SnapshotId::new(),
            run.capture_operation_id().to_owned(),
        )
        .await
        .expect("canonical capture must succeed");
    sqlx::query(
        "UPDATE backup_maintenance_runs
         SET state = 'SNAPSHOT_CAPTURED', captured_snapshot_id = $3,
             snapshot_captured_at = $4
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(older.run_id.into_uuid())
    .bind(fixture.owner_user_id.into_uuid())
    .bind(snapshot.id().into_uuid())
    .bind(
        snapshot
            .committed_at()
            .expect("snapshot must commit")
            .as_offset_datetime(),
    )
    .execute(&fixture.db.inspection)
    .await
    .expect("crash injection must apply");

    // Even though the newer run is claimable from scratch, the oldest
    // eligible step is the recovery, which reconciles first.
    let next = service
        .claim_next_scheduled_maintenance_step(worker(), after(t0, 1), LEASE_SECONDS)
        .await
        .expect("recovery discovery must succeed");
    match next {
        BackupScheduledMaintenanceClaimOutcome::RecoveredCompletion(recovered) => {
            assert_eq!(recovered.claim_id(), stale_claim.claim_id());
            assert!(recovered.is_completed());
        }
        other => panic!("expected recovery priority, got {other:?}"),
    }
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn cross_scope_forgeries_are_rejected_by_the_database() {
    let fixture = fixture("forgery").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "fg", "08:00").await;

    // A second BackupSet gives the forged cross-set run scope.
    let other_set = BackupSetId::new();
    BackupService::new(fixture.pool())
        .create_backup_set(
            fixture.owner_user_id,
            other_set,
            name(format!("backup-forged-{other_set}")),
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
    .bind(other_set.into_uuid())
    .bind(fixture.owner_user_id.into_uuid())
    .execute(&fixture.db.inspection)
    .await
    .expect("second set must become active");
    BackupService::new(fixture.pool())
        .configure_snapshot_retention_policy(
            fixture.owner_user_id,
            format!("policy-forged-{other_set}"),
            other_set,
            1,
            86_400,
        )
        .await
        .expect("second policy must persist");
    let other = schedule_and_handoff(&fixture, other_set, "fg-other", "08:00").await;

    // Cross-BackupSet forgery: occurrence from A with run from B.
    let forged = sqlx::query(
        "INSERT INTO backup_scheduled_maintenance_claims
            (claim_id, owner_user_id, backup_set_id, schedule_id, occurrence_id,
             maintenance_run_id, expected_state, resulting_state, lease_worker_id,
             lease_token, lease_generation, lease_acquired_at, lease_expires_at,
             completed_at, created_at, updated_at)
          VALUES (gen_random_uuid(), $1, $2,
                  (SELECT schedule_id FROM backup_schedule_occurrence_handoffs
                    WHERE maintenance_run_id = $4),
                  $3, $4, 'CREATED', NULL, gen_random_uuid(), gen_random_uuid(),
                  1, now(), now() + INTERVAL '120 seconds', NULL, now(), now())",
    )
    .bind(fixture.owner_user_id.into_uuid())
    .bind(fixture.backup_set_id.into_uuid())
    .bind(scheduled.occurrence_id.into_uuid())
    .bind(other.run_id.into_uuid())
    .execute(&fixture.db.inspection)
    .await;
    assert!(
        forged.is_err(),
        "cross-BackupSet claim must be rejected, got {forged:?}"
    );

    // Cross-handoff forgery: a manual run has no handoff row at all.
    // First finish the scheduled run so a manual run can exist.
    let service = fixture.worker_service();
    let mut observed = after(other.scheduled_for_utc, 120);
    for _ in 0..3 {
        let step = service
            .run_scheduled_maintenance_worker_step(worker(), observed, LEASE_SECONDS)
            .await
            .expect("worker step must succeed");
        let _ = step;
        observed = after(observed, 5);
    }
    let manual = fixture
        .backup_service()
        .create_backup_maintenance_run(
            fixture.owner_user_id,
            format!("manual-forge-{}", fixture.backup_set_id),
            fixture.backup_set_id,
        )
        .await
        .expect("manual run must persist");
    let forged_manual = sqlx::query(
        "INSERT INTO backup_scheduled_maintenance_claims
            (claim_id, owner_user_id, backup_set_id, schedule_id, occurrence_id,
             maintenance_run_id, expected_state, resulting_state, lease_worker_id,
             lease_token, lease_generation, lease_acquired_at, lease_expires_at,
             completed_at, created_at, updated_at)
          VALUES (gen_random_uuid(), $1, $2,
                  (SELECT schedule_id FROM backup_schedule_occurrence_handoffs
                    WHERE maintenance_run_id = $3),
                  $4, $5, 'CREATED', NULL, gen_random_uuid(), gen_random_uuid(),
                  1, now(), now() + INTERVAL '120 seconds', NULL, now(), now())",
    )
    .bind(fixture.owner_user_id.into_uuid())
    .bind(fixture.backup_set_id.into_uuid())
    .bind(scheduled.run_id.into_uuid())
    .bind(scheduled.occurrence_id.into_uuid())
    .bind(manual.id().into_uuid())
    .execute(&fixture.db.inspection)
    .await;
    assert!(
        forged_manual.is_err(),
        "claim on a manual run must be rejected, got {forged_manual:?}"
    );

    // Cross-owner forgery: another owner's identity on this execution.
    let stranger = UserId::new();
    let forged_owner = sqlx::query(
        "INSERT INTO backup_scheduled_maintenance_claims
            (claim_id, owner_user_id, backup_set_id, schedule_id, occurrence_id,
             maintenance_run_id, expected_state, resulting_state, lease_worker_id,
             lease_token, lease_generation, lease_acquired_at, lease_expires_at,
             completed_at, created_at, updated_at)
          VALUES (gen_random_uuid(), $1, $2,
                  (SELECT schedule_id FROM backup_schedule_occurrence_handoffs
                    WHERE maintenance_run_id = $3),
                  $4, $3, 'EXPIRY_PLANNED', NULL, gen_random_uuid(),
                  gen_random_uuid(), 1, now(), now() + INTERVAL '120 seconds',
                  NULL, now(), now())",
    )
    .bind(stranger.into_uuid())
    .bind(fixture.backup_set_id.into_uuid())
    .bind(scheduled.run_id.into_uuid())
    .bind(scheduled.occurrence_id.into_uuid())
    .execute(&fixture.db.inspection)
    .await;
    assert!(forged_owner.is_err(), "cross-owner claim must be rejected");
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn completed_claims_are_immutable_and_undeletable() {
    let fixture = fixture("immutable").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "im", "08:00").await;
    let service = fixture.worker_service();
    let t0 = after(scheduled.scheduled_for_utc, 60);
    let claim = claimed(
        service
            .claim_next_scheduled_maintenance_step(worker(), t0, LEASE_SECONDS)
            .await
            .expect("claim must succeed"),
    );
    let advanced = execute(&fixture, &claim, after(t0, 1))
        .await
        .expect("execution must succeed");
    assert!(matches!(
        advanced,
        BackupScheduledMaintenanceStepResult::Advanced { .. }
    ));

    let update = sqlx::query(
        "UPDATE backup_scheduled_maintenance_claims
         SET lease_generation = lease_generation + 1,
             lease_token = gen_random_uuid(),
             updated_at = clock_timestamp()
         WHERE claim_id = $1",
    )
    .bind(claim.claim_id().into_uuid())
    .execute(&fixture.db.inspection)
    .await;
    assert!(
        update.is_err(),
        "completed claim UPDATE must be rejected, got {update:?}"
    );

    let delete = sqlx::query("DELETE FROM backup_scheduled_maintenance_claims WHERE claim_id = $1")
        .bind(claim.claim_id().into_uuid())
        .execute(&fixture.db.inspection)
        .await;
    assert!(
        delete.is_err(),
        "claim DELETE must be rejected, got {delete:?}"
    );
    assert_eq!(claim_count(&fixture.db.inspection).await, 1);
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn incomplete_claim_takeover_rules_are_database_fenced() {
    let fixture = fixture("takerules").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "tr", "08:00").await;
    let service = fixture.worker_service();
    let t0 = after(scheduled.scheduled_for_utc, 60);
    let claim = claimed(
        service
            .claim_next_scheduled_maintenance_step(worker(), t0, LEASE_SECONDS)
            .await
            .expect("claim must succeed"),
    );

    // Early takeover before expiry is rejected even with a fresh token.
    let early = sqlx::query(
        "UPDATE backup_scheduled_maintenance_claims
         SET lease_worker_id = gen_random_uuid(), lease_token = gen_random_uuid(),
             lease_generation = lease_generation + 1,
             lease_acquired_at = $2, lease_expires_at = $2 + INTERVAL '120 seconds',
             updated_at = clock_timestamp()
         WHERE claim_id = $1",
    )
    .bind(claim.claim_id().into_uuid())
    .bind(after(t0, 10).as_offset_datetime())
    .execute(&fixture.db.inspection)
    .await;
    assert!(
        early.is_err(),
        "early steal must be rejected, got {early:?}"
    );

    // Generation jumps are rejected.
    let jump = sqlx::query(
        "UPDATE backup_scheduled_maintenance_claims
         SET lease_worker_id = gen_random_uuid(), lease_token = gen_random_uuid(),
             lease_generation = lease_generation + 2,
             lease_acquired_at = $2, lease_expires_at = $2 + INTERVAL '120 seconds',
             updated_at = clock_timestamp()
         WHERE claim_id = $1",
    )
    .bind(claim.claim_id().into_uuid())
    .bind(after(t0, LEASE_SECONDS).as_offset_datetime())
    .execute(&fixture.db.inspection)
    .await;
    assert!(
        jump.is_err(),
        "generation jump must be rejected, got {jump:?}"
    );

    // Token reuse is rejected.
    let reuse = sqlx::query(
        "UPDATE backup_scheduled_maintenance_claims
         SET lease_worker_id = gen_random_uuid(),
             lease_generation = lease_generation + 1,
             lease_acquired_at = $2, lease_expires_at = $2 + INTERVAL '120 seconds',
             updated_at = clock_timestamp()
         WHERE claim_id = $1",
    )
    .bind(claim.claim_id().into_uuid())
    .bind(after(t0, LEASE_SECONDS).as_offset_datetime())
    .execute(&fixture.db.inspection)
    .await;
    assert!(
        reuse.is_err(),
        "token reuse must be rejected, got {reuse:?}"
    );

    // Provenance rebinding is rejected.
    let rebind = sqlx::query(
        "UPDATE backup_scheduled_maintenance_claims
         SET expected_state = 'SNAPSHOT_CAPTURED', updated_at = clock_timestamp()
         WHERE claim_id = $1",
    )
    .bind(claim.claim_id().into_uuid())
    .execute(&fixture.db.inspection)
    .await;
    assert!(
        rebind.is_err(),
        "provenance mutation must be rejected, got {rebind:?}"
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn schedule_disable_after_handoff_still_advances() {
    let fixture = fixture("disable").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "dis", "08:00").await;
    BackupScheduleService::new(fixture.pool())
        .set_backup_schedule_enabled(fixture.owner_user_id, fixture.backup_set_id, false)
        .await
        .expect("schedule must disable");

    let service = fixture.worker_service();
    let mut observed = after(scheduled.scheduled_for_utc, 60);
    for _ in 0..3 {
        let step = service
            .run_scheduled_maintenance_worker_step(worker(), observed, LEASE_SECONDS)
            .await
            .expect("disabled-schedule work must still advance");
        assert!(!step.is_idle());
        observed = after(observed, 10);
    }
    assert_eq!(
        run_state(&fixture, scheduled.run_id).await,
        BackupMaintenanceRunState::Completed
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn policy_edit_after_handoff_keeps_existing_run_eligible() {
    let fixture = fixture("policyedit").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "pe", "08:00").await;
    // A new revision (policy edit) after the handoff must not revoke the
    // already-handed-off execution authority.
    BackupScheduleService::new(fixture.pool())
        .configure_backup_schedule(
            fixture.owner_user_id,
            format!("schedule-edit-{}", fixture.backup_set_id),
            fixture.backup_set_id,
            BackupScheduleConfig::new_with_misfire_policy(
                synveil_core::BackupScheduleRecurrenceKind::Daily,
                timezone("UTC"),
                local_time("09:30"),
                Vec::new(),
                BackupScheduleMisfireMode::ReplayOneByOne,
                3_600,
            )
            .expect("edited config must be valid"),
        )
        .await
        .expect("policy edit must persist");

    let service = fixture.worker_service();
    let mut observed = after(scheduled.scheduled_for_utc, 60);
    for _ in 0..3 {
        let step = service
            .run_scheduled_maintenance_worker_step(worker(), observed, LEASE_SECONDS)
            .await
            .expect("edited-policy work must still advance");
        assert!(!step.is_idle());
        observed = after(observed, 10);
    }
    assert_eq!(
        run_state(&fixture, scheduled.run_id).await,
        BackupMaintenanceRunState::Completed
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn misfire_skip_creates_no_run_and_no_claim() {
    let fixture = fixture("misfire").await;
    let service = BackupScheduleService::new(fixture.pool());
    service
        .configure_backup_schedule_with_misfire_policy_from_values(
            fixture.owner_user_id,
            format!("schedule-skip-{}", fixture.backup_set_id),
            fixture.backup_set_id,
            synveil_core::BackupScheduleRecurrenceKind::Daily,
            "UTC",
            "08:00",
            Vec::new(),
            "REPLAY_ONE_BY_ONE",
            60,
        )
        .await
        .expect("schedule must persist");
    // A far-future tick expires the whole prefix: the oldest due occurrence
    // is sixty seconds stale, so the tick records an immutable skip and
    // hands off nothing.
    let tick = BackupSchedulerService::new(fixture.pool())
        .run_scheduler_tick(timestamp("2027-03-01T00:00:00Z"))
        .await
        .expect("tick must succeed");
    assert!(
        tick.is_skipped(),
        "expected an expired-prefix skip, got {tick:?}"
    );

    let runs = fixture
        .backup_service()
        .list_backup_maintenance_runs(fixture.owner_user_id, fixture.backup_set_id, None, 10)
        .await
        .expect("run listing must succeed");
    assert!(runs.0.is_empty(), "skipped occurrences create no run");
    let idle = fixture
        .worker_service()
        .claim_next_scheduled_maintenance_step(
            worker(),
            timestamp("2027-03-01T00:00:01Z"),
            LEASE_SECONDS,
        )
        .await
        .expect("discovery must succeed");
    assert!(idle.is_idle());
    assert_eq!(claim_count(&fixture.db.inspection).await, 0);
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn restart_recovers_the_durable_claim() {
    let fixture = fixture("restart").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "rs", "08:00").await;
    let t0 = after(scheduled.scheduled_for_utc, 60);
    let first = claimed(
        fixture
            .worker_service()
            .claim_next_scheduled_maintenance_step(worker(), t0, 60)
            .await
            .expect("claim must succeed"),
    );
    // Destroy the service instance: no process-local state may be required.
    drop(fixture.worker_service());
    let fresh = ScheduledMaintenanceWorkerService::new(fixture.pool());
    let taken = fresh
        .claim_next_scheduled_maintenance_step(worker(), after(t0, 60), 60)
        .await
        .expect("fresh instance must take over");
    match taken {
        BackupScheduledMaintenanceClaimOutcome::TakenOver(next) => {
            assert_eq!(next.claim_id(), first.claim_id());
            assert_eq!(next.lease_generation(), 2);
        }
        other => panic!("expected durable takeover after restart, got {other:?}"),
    }
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn end_to_end_tick_then_three_worker_steps() {
    let fixture = fixture("e2e").await;
    let service = BackupScheduleService::new(fixture.pool());
    service
        .configure_backup_schedule(
            fixture.owner_user_id,
            format!("schedule-e2e-{}", fixture.backup_set_id),
            fixture.backup_set_id,
            daily("08:00"),
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
    let tick = BackupSchedulerService::new(fixture.pool())
        .run_scheduler_tick(after(planned.scheduled_for_utc(), 1))
        .await
        .expect("tick must succeed");
    let outcome = tick.outcome().expect("tick must hand off");
    assert_eq!(
        run_state(&fixture, outcome.maintenance_run_id()).await,
        BackupMaintenanceRunState::Created
    );

    let worker_service = fixture.worker_service();
    let mut observed = after(planned.scheduled_for_utc(), 60);
    for _ in 0..3 {
        let step = worker_service
            .run_scheduled_maintenance_worker_step(worker(), observed, LEASE_SECONDS)
            .await
            .expect("worker step must succeed");
        assert!(!step.is_idle());
        assert!(
            step.execution().is_some(),
            "claimed steps must execute exactly one transition"
        );
        observed = after(observed, 10);
    }
    assert_eq!(
        run_state(&fixture, outcome.maintenance_run_id()).await,
        BackupMaintenanceRunState::Completed
    );
    let idle = worker_service
        .run_scheduled_maintenance_worker_step(worker(), observed, LEASE_SECONDS)
        .await
        .expect("idle step must succeed");
    assert!(idle.is_idle());
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn worker_driven_run_matches_manual_prompt49_semantics() {
    let fixture = fixture("equiv").await;
    let scheduled = schedule_and_handoff(&fixture, fixture.backup_set_id, "eq", "08:00").await;
    let service = fixture.worker_service();
    let mut observed = after(scheduled.scheduled_for_utc, 60);
    for _ in 0..3 {
        service
            .run_scheduled_maintenance_worker_step(worker(), observed, LEASE_SECONDS)
            .await
            .expect("worker step must succeed");
        observed = after(observed, 10);
    }
    let worker_run = fixture
        .backup_service()
        .get_backup_maintenance_run(fixture.owner_user_id, scheduled.run_id)
        .await
        .expect("worker run must be readable");
    assert_eq!(worker_run.state(), BackupMaintenanceRunState::Completed);
    assert!(worker_run.captured_snapshot_id().is_some());
    assert!(worker_run.expiry_plan_id().is_some());
    assert!(worker_run.expiry_execution_id().is_some());
    assert!(worker_run.snapshot_captured_at().is_some());
    assert!(worker_run.expiry_planned_at().is_some());
    assert!(worker_run.maintenance_completed_at().is_some());
    assert!(worker_run.stale_at().is_none());

    // One MAINTENANCE operation and no worker-claim duplicates in the public
    // operation feed.
    let (operations, _) = fixture
        .backup_service()
        .list_backup_operations(
            fixture.owner_user_id,
            fixture.backup_set_id,
            None,
            None,
            100,
        )
        .await
        .expect("operation feed must be readable");
    assert_eq!(
        operations.len(),
        1,
        "exactly one public operation, got {operations:?}"
    );
    assert_eq!(
        operations[0].operation_kind(),
        synveil_core::BackupOperationKind::Maintenance
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn claim_schema_audit() {
    let fixture = fixture("schema").await;
    let constraints: Vec<(String, String)> = sqlx::query_as(
        "SELECT conname, contype::TEXT FROM pg_constraint
          WHERE conrelid = 'backup_scheduled_maintenance_claims'::regclass",
    )
    .fetch_all(&fixture.db.inspection)
    .await
    .expect("constraint audit must succeed");
    let names: Vec<&str> = constraints.iter().map(|(name, _)| name.as_str()).collect();
    for required in [
        "backup_scheduled_maintenance_claims_pkey",
        "backup_scheduled_maintenance_claims_run_state_unique",
        "backup_scheduled_maintenance_claims_lease_token_unique",
        "backup_scheduled_maintenance_claims_expected_value",
        "backup_scheduled_maintenance_claims_result_shape",
        "backup_scheduled_maintenance_claims_completion_consistency",
        "backup_scheduled_maintenance_claims_generation_value",
        "backup_scheduled_maintenance_claims_lease_order",
        "backup_scheduled_maintenance_claims_set_owner_fk",
        "backup_scheduled_maintenance_claims_schedule_scope_fk",
        "backup_scheduled_maintenance_claims_occurrence_scope_fk",
        "backup_scheduled_maintenance_claims_run_scope_fk",
    ] {
        assert!(
            names.contains(&required),
            "missing constraint {required}: {names:?}"
        );
    }
    let indexes: Vec<String> = sqlx::query_scalar(
        "SELECT indexname FROM pg_indexes WHERE tablename = 'backup_scheduled_maintenance_claims'",
    )
    .fetch_all(&fixture.db.inspection)
    .await
    .expect("index audit must succeed");
    for required in [
        "backup_scheduled_maintenance_claims_run_incomplete_idx",
        "backup_scheduled_maintenance_claims_lease_expiry_idx",
        "backup_scheduled_maintenance_claims_owner_created_idx",
        "backup_scheduled_maintenance_claims_occurrence_idx",
    ] {
        assert!(
            indexes.iter().any(|index| index == required),
            "missing index {required}: {indexes:?}"
        );
    }
    let triggers: Vec<String> = sqlx::query_scalar(
        "SELECT tgname FROM pg_trigger WHERE tgrelid = 'backup_scheduled_maintenance_claims'::regclass
          AND NOT tgisinternal",
    )
    .fetch_all(&fixture.db.inspection)
    .await
    .expect("trigger audit must succeed");
    for required in [
        "backup_scheduled_maintenance_claims_validate",
        "backup_scheduled_maintenance_claims_enforce_update",
        "backup_scheduled_maintenance_claims_no_delete",
    ] {
        assert!(
            triggers.iter().any(|trigger| trigger == required),
            "missing trigger {required}: {triggers:?}"
        );
    }
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to the disposable server maintenance database"]
async fn forward_migration_33_to_34_preserves_prompt65_data() {
    let base = std::env::var("SYNVEIL_TEST_DATABASE_URL").expect(
        "SYNVEIL_TEST_DATABASE_URL must identify the disposable server maintenance database",
    );
    let db_name = format!("p66_fwd_{}", Uuid::now_v7().simple());
    let maintenance = PgPool::connect(&base)
        .await
        .expect("maintenance must connect");
    sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
        .execute(&maintenance)
        .await
        .expect("forward-migration database must be created");
    maintenance.close().await;
    let url = format!(
        "{}/{}",
        base.rsplit_once('/').expect("URL must have path").0,
        db_name
    );
    let raw = PgPool::connect(&url)
        .await
        .expect("forward database must connect");

    // Stage migrations 1-33 in a temporary directory and apply them through
    // the production runner, so `_sqlx_migrations` bookkeeping (including
    // checksums) matches a real Prompt 65 deployment.
    let mut entries: Vec<_> = std::fs::read_dir("../../migrations")
        .expect("migrations directory must be readable")
        .map(|entry| {
            entry
                .expect("entry must be readable")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.ends_with(".sql"))
        .collect();
    entries.sort();
    assert!(
        entries.len() >= 34,
        "all migrations must be present, got {}",
        entries.len()
    );
    let stage = std::env::temp_dir().join(format!("p66_fwd_{}", Uuid::now_v7().simple()));
    std::fs::create_dir_all(&stage).expect("staging directory must be created");
    for name in entries
        .iter()
        .filter(|name| !name.starts_with("20260903000001"))
    {
        std::fs::copy(format!("../../migrations/{name}"), stage.join(name))
            .unwrap_or_else(|_| panic!("migration {name} must stage"));
    }
    let config = DatabaseConfig::from_url(&url).expect("forward URL must use PostgreSQL");
    let pool_33 = DatabasePool::connect(&config)
        .await
        .expect("33-state database must connect");
    let staged = MigrationRunner::from_path(&stage)
        .run(&pool_33)
        .await
        .expect("migrations 1-33 must apply");
    assert_eq!(staged.applied_versions().len(), 33);
    assert_eq!(staged.latest_applied_version(), Some(20260903000000));

    // Populate the Prompt 65 surface through the canonical services so the
    // pre-migration rows carry valid fingerprints and provenance: a schedule
    // with revision, a materialized occurrence, a handoff with its scheduled
    // run, plus a manual run on a second BackupSet.
    let repository = DomainRepository::new(&pool_33);
    let observed_at = timestamp("2026-09-02T00:00:00.123456Z");
    let owner_user_id = UserId::new();
    repository
        .insert_user(&User::new(
            owner_user_id,
            synveil_core::LoginIdentifier::new(
                format!("fwd-owner-{owner_user_id}"),
                format!("fwd-key-{owner_user_id}"),
            )
            .expect("forward login must be valid"),
            UserStatus::Active,
            observed_at,
        ))
        .await
        .expect("forward user must persist");
    let library_id = LibraryId::new();
    let root = Node::new_root(NodeId::new(), library_id, name("root"), observed_at);
    let library = Library::new(
        library_id,
        owner_user_id,
        name(format!("fwd-library-{library_id}")),
        &root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("forward library must be valid");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("forward library must persist");
    let backups = BackupService::new(pool_33.clone());
    let mut sets = Vec::new();
    for label in ["fwd-a", "fwd-b"] {
        let set = BackupSetId::new();
        backups
            .create_backup_set(
                owner_user_id,
                set,
                name(format!("backup-{label}-{set}")),
                library_id,
                None,
                observed_at,
            )
            .await
            .expect("forward set must persist");
        sqlx::query(
            "UPDATE backup_sets SET state = 'ACTIVE', revision = revision + 1,
                 updated_at = clock_timestamp()
             WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(set.into_uuid())
        .bind(owner_user_id.into_uuid())
        .execute(&raw)
        .await
        .expect("forward set must become active");
        backups
            .configure_snapshot_retention_policy(
                owner_user_id,
                format!("policy-{label}-{set}"),
                set,
                1,
                86_400,
            )
            .await
            .expect("forward policy must persist");
        sets.push(set);
    }
    let schedules = BackupScheduleService::new(pool_33.clone());
    let revision = schedules
        .configure_backup_schedule(
            owner_user_id,
            format!("schedule-fwd-{}", sets[0]),
            sets[0],
            daily("08:00"),
        )
        .await
        .expect("forward schedule must persist");
    let schedule = schedules
        .get_backup_schedule(owner_user_id, sets[0])
        .await
        .expect("forward schedule must be readable");
    let date = (schedule.effective_from().as_offset_datetime() + time::Duration::days(1)).date();
    let planned = schedule
        .current_revision()
        .occurrence_on_local_date(date)
        .expect("forward occurrence must plan");
    let occurrence = match schedules
        .materialize_due_backup_schedule_occurrence(
            owner_user_id,
            schedule.id(),
            schedule.current_revision_id(),
            date,
            after(planned.scheduled_for_utc(), 1),
        )
        .await
        .expect("forward occurrence must materialize")
    {
        BackupScheduleOccurrenceMaterializationResult::Created(occurrence)
        | BackupScheduleOccurrenceMaterializationResult::Existing(occurrence) => occurrence,
        other => panic!("expected a forward occurrence, got {other:?}"),
    };
    let handoff = backups
        .handoff_backup_schedule_occurrence(
            owner_user_id,
            occurrence.id(),
            after(planned.scheduled_for_utc(), 1),
        )
        .await
        .expect("forward handoff must succeed");
    let manual = backups
        .create_backup_maintenance_run(owner_user_id, format!("manual-fwd-{}", sets[1]), sets[1])
        .await
        .expect("forward manual run must persist");
    let scheduled_run_id = handoff.maintenance_run().id();
    let _ = (revision, manual);

    let counts_before: Vec<i64> = {
        let mut counts = Vec::new();
        for table in [
            "backup_schedules",
            "backup_schedule_revisions",
            "backup_schedule_occurrences",
            "backup_schedule_occurrence_handoffs",
            "backup_maintenance_runs",
        ] {
            counts.push(
                sqlx::query_scalar::<_, i64>(&format!("SELECT count(*) FROM {table}"))
                    .fetch_one(&raw)
                    .await
                    .expect("count must succeed"),
            );
        }
        counts
    };

    // Apply migration 34 through the production runner: the staged 33
    // rows share checksums with the workspace set, so exactly one pending
    // migration applies on the populated Prompt 65 data.
    let upgraded_status = MigrationRunner::new()
        .run(&pool_33)
        .await
        .expect("migration 34 must apply on populated Prompt 65 data");
    assert!(upgraded_status.is_current());
    assert_eq!(upgraded_status.applied_versions().len(), 34);
    assert_eq!(
        upgraded_status.latest_applied_version(),
        Some(20260903000001)
    );

    for (table, before) in [
        "backup_schedules",
        "backup_schedule_revisions",
        "backup_schedule_occurrences",
        "backup_schedule_occurrence_handoffs",
        "backup_maintenance_runs",
    ]
    .into_iter()
    .zip(counts_before)
    {
        let after_count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&raw)
            .await
            .expect("recount must succeed");
        assert_eq!(after_count, before, "migration 34 must preserve {table}");
    }
    let run_state: String =
        sqlx::query_scalar("SELECT state FROM backup_maintenance_runs WHERE id = $1")
            .bind(scheduled_run_id.into_uuid())
            .fetch_one(&raw)
            .await
            .expect("run must still exist");
    assert_eq!(
        run_state, "CREATED",
        "migration must change zero maintenance states"
    );
    let handoffs: i64 =
        sqlx::query_scalar("SELECT count(*) FROM backup_schedule_occurrence_handoffs")
            .fetch_one(&raw)
            .await
            .expect("handoff recount must succeed");
    assert_eq!(handoffs, 1, "migration must fabricate zero handoffs");
    let claims: i64 =
        sqlx::query_scalar("SELECT count(*) FROM backup_scheduled_maintenance_claims")
            .fetch_one(&raw)
            .await
            .expect("claim count must succeed");
    assert_eq!(claims, 0, "migration must fabricate zero claims");

    // The upgraded database immediately serves worker claims on the
    // preserved scheduled run.
    let discovered = ScheduledMaintenanceWorkerService::new(pool_33.clone())
        .claim_next_scheduled_maintenance_step(
            BackupScheduledMaintenanceWorkerId::new(),
            after(planned.scheduled_for_utc(), 60),
            120,
        )
        .await
        .expect("upgraded database must serve claims");
    assert_eq!(
        claimed(discovered).maintenance_run_id(),
        scheduled_run_id,
        "preserved handoff run must be claimable after upgrade"
    );
    pool_33.close().await;
    raw.close().await;
    std::fs::remove_dir_all(&stage).expect("staging directory must be removed");
}
