use sqlx::PgPool;
use synveil_core::{
    BackupMaintenanceRunId, BackupMaintenanceRunState, BackupSetId, BackupSnapshotExpiryDecision,
    BackupSnapshotExpiryPlanState, DedupDomainId, FileVersion, FileVersionId, Library, LibraryId,
    LogicalName, Node, NodeId, NodeKind, ObjectId, ObjectReference, Sha256Digest, SnapshotId,
    Timestamp, User, UserId, UserStatus,
};
use synveil_metadata::{
    BackupError, BackupService, DatabaseConfig, DatabasePool, DomainRepository, MigrationRunner,
};
use time::OffsetDateTime;
use uuid::Uuid;

fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse(value).expect("test timestamp is valid")
}

fn name(value: &str) -> LogicalName {
    LogicalName::new(value).expect("test logical name is valid")
}

struct Fixture {
    pool: DatabasePool,
    inspection: PgPool,
    user_id: UserId,
    library: Library,
    root: Node,
    file: Node,
    object: ObjectReference,
    version: FileVersion,
}

async fn fixture(label: &str) -> Fixture {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a fresh disposable PostgreSQL database");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    let status = MigrationRunner::new()
        .run(&pool)
        .await
        .expect("full migration chain must apply");
    assert!(status.is_current(), "all migrations must be current");
    assert_eq!(status.applied_versions().len(), 29);
    let inspection = PgPool::connect(&url)
        .await
        .expect("inspection connection must succeed");

    let observed_at = timestamp("2026-08-30T00:00:00.123456Z");
    let repository = DomainRepository::new(&pool);
    let user_id = UserId::new();
    repository
        .insert_user(&User::new(
            user_id,
            synveil_core::LoginIdentifier::new(format!("maintenance-{label}"), user_id.to_string())
                .expect("fixture login is valid"),
            UserStatus::Active,
            observed_at,
        ))
        .await
        .expect("fixture owner must persist");

    let library_id = LibraryId::new();
    let root = Node::new_root(NodeId::new(), library_id, name("root"), observed_at);
    let library = Library::new(
        library_id,
        user_id,
        name(&format!("Maintenance {label}")),
        &root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("fixture library is valid");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("fixture library and root must persist");

    let file = Node::new_child(
        NodeId::new(),
        library.id(),
        &root,
        NodeKind::File,
        name("file.txt"),
        observed_at,
    )
    .expect("fixture file is valid");
    repository
        .insert_node(&file)
        .await
        .expect("fixture file must persist");
    let object = ObjectReference::new(
        ObjectId::new(),
        library.dedup_domain_id(),
        Sha256Digest::from_bytes([7_u8; 32]),
        10,
    );
    repository
        .insert_object(object, observed_at)
        .await
        .expect("fixture object must persist");
    let version = FileVersion::new(
        FileVersionId::new(),
        &library,
        &file,
        object,
        None,
        observed_at,
    )
    .expect("fixture version is valid");
    repository
        .insert_file_version(version)
        .await
        .expect("fixture version must persist");
    let file = file
        .with_current_version(&version, observed_at)
        .expect("fixture file accepts current version");
    repository
        .update_node(&file)
        .await
        .expect("fixture file current version must update");
    sqlx::query(
        "INSERT INTO object_replicas
            (id, object_id, object_dedup_domain_id, backend_kind, storage_key,
             state, stored_length, stored_sha256, created_at, verified_at)
         VALUES ($1, $2, $3, 'LOCAL_FILESYSTEM', $4, 'VERIFIED', $5::NUMERIC, $6, $7, $7)",
    )
    .bind(Uuid::now_v7())
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .bind(format!("fixtures/{label}/object"))
    .bind(object.plaintext_length().to_string())
    .bind(object.canonical_hash().as_bytes())
    .bind(observed_at.as_offset_datetime())
    .execute(&inspection)
    .await
    .expect("fixture verified replica must persist");

    Fixture {
        pool,
        inspection,
        user_id,
        library,
        root,
        file,
        object,
        version,
    }
}

async fn create_set(fixture: &Fixture, label: &str) -> BackupSetId {
    let set_id = BackupSetId::new();
    BackupService::new(fixture.pool.clone())
        .create_backup_set(
            fixture.user_id,
            set_id,
            name(label),
            fixture.library.id(),
            None,
            timestamp("2026-08-30T00:00:01.123456Z"),
        )
        .await
        .expect("backup set must persist");
    set_id
}

async fn configure(fixture: &Fixture, set_id: BackupSetId, operation: &str, keep: u64, age: u64) {
    BackupService::new(fixture.pool.clone())
        .configure_snapshot_retention_policy(
            fixture.user_id,
            operation.to_owned(),
            set_id,
            keep,
            age,
        )
        .await
        .expect("retention policy must persist");
}

async fn insert_completed_snapshot_at(
    fixture: &Fixture,
    backup_set_id: BackupSetId,
    committed_at: OffsetDateTime,
    with_content: bool,
) -> SnapshotId {
    let snapshot_id = SnapshotId::new();
    let snapshot_epoch = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(snapshot_epoch), 999) + 1
         FROM backup_snapshots WHERE backup_set_id = $1",
    )
    .bind(backup_set_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("next snapshot epoch must load");
    let manifest_count = if with_content { 2 } else { 0 };
    let terminal_node_id = if with_content {
        Some(fixture.file.id().into_uuid())
    } else {
        None
    };
    let content_count = if with_content { 1 } else { 0 };
    sqlx::query(
        "INSERT INTO backup_snapshots
            (id, backup_set_id, owner_user_id, source_library_id, operation_id,
             snapshot_epoch, snapshot_resume_sequence, manifest_item_count,
             terminal_node_id, content_reference_count, state, created_at,
             committed_at, expired_at)
         VALUES ($1, $2, $3, $4, $5, $6, 0, $7, $8, $9, 'BUILDING',
                 $10, NULL, NULL)",
    )
    .bind(snapshot_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .bind(fixture.user_id.into_uuid())
    .bind(fixture.library.id().into_uuid())
    .bind(format!("direct-{}", Uuid::now_v7().simple()))
    .bind(snapshot_epoch)
    .bind(manifest_count)
    .bind(terminal_node_id)
    .bind(content_count)
    .bind(committed_at)
    .execute(&fixture.inspection)
    .await
    .expect("completed snapshot must persist");

    if with_content {
        sqlx::query(
            "INSERT INTO backup_snapshot_nodes
                (snapshot_id, node_id, parent_node_id, name, kind, state, revision,
                 current_version_id, content_length, content_sha256,
                 node_created_at, node_updated_at)
             VALUES
                ($1, $2, NULL, 'root', 'DIRECTORY', 'ACTIVE', 0::NUMERIC,
                 NULL, NULL, NULL, $6, $6),
                ($1, $3, $2, 'file.txt', 'FILE', 'ACTIVE', 0::NUMERIC,
                 $4, $7::NUMERIC, $8, $6, $6)",
        )
        .bind(snapshot_id.into_uuid())
        .bind(fixture.root.id().into_uuid())
        .bind(fixture.file.id().into_uuid())
        .bind(fixture.version.id().into_uuid())
        .bind(fixture.object.object_id().into_uuid())
        .bind(committed_at)
        .bind(fixture.object.plaintext_length().to_string())
        .bind(fixture.object.canonical_hash().as_bytes())
        .execute(&fixture.inspection)
        .await
        .expect("manifest rows must persist");
        sqlx::query(
            "INSERT INTO backup_snapshot_content_pins
                (snapshot_id, manifest_node_id, file_version_id, object_id,
                 object_dedup_domain_id, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(snapshot_id.into_uuid())
        .bind(fixture.file.id().into_uuid())
        .bind(fixture.version.id().into_uuid())
        .bind(fixture.object.object_id().into_uuid())
        .bind(fixture.object.dedup_domain_id().into_uuid())
        .bind(committed_at)
        .execute(&fixture.inspection)
        .await
        .expect("content pin must persist");
    }

    sqlx::query(
        "UPDATE backup_snapshots
         SET state = 'COMPLETED', committed_at = $2
         WHERE id = $1 AND state = 'BUILDING'",
    )
    .bind(snapshot_id.into_uuid())
    .bind(committed_at)
    .execute(&fixture.inspection)
    .await
    .expect("snapshot must complete after fixture pins exist");

    snapshot_id
}

async fn insert_completed_snapshot_days_old(
    fixture: &Fixture,
    backup_set_id: BackupSetId,
    days: i64,
    with_content: bool,
) -> SnapshotId {
    let committed_at = sqlx::query_scalar::<_, OffsetDateTime>(
        "SELECT clock_timestamp() - ($1 * INTERVAL '1 day')",
    )
    .bind(days)
    .fetch_one(&fixture.inspection)
    .await
    .expect("committed_at timestamp must load");
    insert_completed_snapshot_at(fixture, backup_set_id, committed_at, with_content).await
}

async fn count_table(fixture: &Fixture, table: &str) -> i64 {
    let sql = format!("SELECT count(*) FROM {table}");
    sqlx::query_scalar(&sql)
        .fetch_one(&fixture.inspection)
        .await
        .expect("count must load")
}

async fn snapshot_count_for_operation(fixture: &Fixture, operation_id: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshots
         WHERE owner_user_id = $1 AND operation_id = $2",
    )
    .bind(fixture.user_id.into_uuid())
    .bind(operation_id)
    .fetch_one(&fixture.inspection)
    .await
    .expect("snapshot operation count must load")
}

async fn pin_count(fixture: &Fixture, snapshot_id: SnapshotId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1")
        .bind(snapshot_id.into_uuid())
        .fetch_one(&fixture.inspection)
        .await
        .expect("pin count must load")
}

async fn snapshot_state(fixture: &Fixture, snapshot_id: SnapshotId) -> String {
    sqlx::query_scalar("SELECT state FROM backup_snapshots WHERE id = $1")
        .bind(snapshot_id.into_uuid())
        .fetch_one(&fixture.inspection)
        .await
        .expect("snapshot state must load")
}

async fn run_state(fixture: &Fixture, run_id: BackupMaintenanceRunId) -> String {
    sqlx::query_scalar("SELECT state FROM backup_maintenance_runs WHERE id = $1")
        .bind(run_id.into_uuid())
        .fetch_one(&fixture.inspection)
        .await
        .expect("run state must load")
}

async fn expiry_entry_decision(
    fixture: &Fixture,
    plan_id: synveil_core::BackupSnapshotExpiryPlanId,
    snapshot_id: SnapshotId,
) -> BackupSnapshotExpiryDecision {
    let decision: String = sqlx::query_scalar(
        "SELECT decision FROM backup_snapshot_expiry_plan_entries
         WHERE plan_id = $1 AND snapshot_id = $2",
    )
    .bind(plan_id.into_uuid())
    .bind(snapshot_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("plan entry decision must load");
    decision.parse().expect("decision must parse")
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_maintenance_happy_path_expires_old_snapshot_without_prune_or_gc() {
    let fixture = fixture("happy").await;
    let backup = BackupService::new(fixture.pool.clone());
    let set_id = create_set(&fixture, "happy-set").await;
    let old = insert_completed_snapshot_days_old(&fixture, set_id, 100, true).await;
    let latest = insert_completed_snapshot_days_old(&fixture, set_id, 1, true).await;
    let old_pins = pin_count(&fixture, old).await;
    configure(&fixture, set_id, "happy-policy", 1, 30 * 86_400).await;
    let prune_before = count_table(&fixture, "backup_prune_plans").await;
    let prune_exec_before = count_table(&fixture, "backup_prune_executions").await;
    let gc_before = count_table(&fixture, "object_gc_candidates").await;

    let run = backup
        .create_backup_maintenance_run(fixture.user_id, "happy-maintenance".to_owned(), set_id)
        .await
        .expect("maintenance run must be created");
    let completed = backup
        .advance_backup_maintenance_run(fixture.user_id, run.id())
        .await
        .expect("maintenance run must complete");

    assert_eq!(completed.state(), BackupMaintenanceRunState::Completed);
    assert!(completed.captured_snapshot_id().is_some());
    assert!(completed.expiry_plan_id().is_some());
    assert!(completed.expiry_execution_id().is_some());
    assert_eq!(snapshot_state(&fixture, old).await, "EXPIRED");
    assert_eq!(snapshot_state(&fixture, latest).await, "COMPLETED");
    assert_eq!(pin_count(&fixture, old).await, old_pins);
    assert_eq!(
        pin_count(&fixture, completed.captured_snapshot_id().unwrap()).await,
        1
    );
    assert_eq!(
        count_table(&fixture, "backup_prune_plans").await,
        prune_before
    );
    assert_eq!(
        count_table(&fixture, "backup_prune_executions").await,
        prune_exec_before
    );
    assert_eq!(
        count_table(&fixture, "object_gc_candidates").await,
        gc_before
    );
    assert_eq!(
        backup
            .get_snapshot_expiry_execution(
                fixture.user_id,
                completed.expiry_execution_id().unwrap()
            )
            .await
            .unwrap()
            .expired_snapshot_count(),
        1
    );

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_maintenance_zero_expiry_completes() {
    let fixture = fixture("zero").await;
    let backup = BackupService::new(fixture.pool.clone());
    let set_id = create_set(&fixture, "zero-set").await;
    insert_completed_snapshot_days_old(&fixture, set_id, 1, false).await;
    configure(&fixture, set_id, "zero-policy", 5, 30 * 86_400).await;

    let run = backup
        .create_backup_maintenance_run(fixture.user_id, "zero-maintenance".to_owned(), set_id)
        .await
        .expect("maintenance run must be created");
    let completed = backup
        .advance_backup_maintenance_run(fixture.user_id, run.id())
        .await
        .expect("zero-expiry maintenance must complete");

    assert_eq!(completed.state(), BackupMaintenanceRunState::Completed);
    assert_eq!(
        backup
            .get_snapshot_expiry_execution(
                fixture.user_id,
                completed.expiry_execution_id().unwrap()
            )
            .await
            .unwrap()
            .expired_snapshot_count(),
        0
    );

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_maintenance_active_restore_blocker_completes_without_mutating_restore() {
    let fixture = fixture("blocker").await;
    let backup = BackupService::new(fixture.pool.clone());
    let set_id = create_set(&fixture, "blocker-set").await;
    let blocked = insert_completed_snapshot_days_old(&fixture, set_id, 100, true).await;
    insert_completed_snapshot_days_old(&fixture, set_id, 1, true).await;
    configure(&fixture, set_id, "blocker-policy", 1, 30 * 86_400).await;
    let restore = backup
        .create_restore_plan(
            fixture.user_id,
            "blocker-restore".to_owned(),
            set_id,
            blocked,
            fixture.library.id(),
            fixture.root.id(),
            name("Restore Blocker"),
        )
        .await
        .expect("restore blocker plan must persist");

    let run = backup
        .create_backup_maintenance_run(fixture.user_id, "blocker-maintenance".to_owned(), set_id)
        .await
        .expect("maintenance run must be created");
    let completed = backup
        .advance_backup_maintenance_run(fixture.user_id, run.id())
        .await
        .expect("blocked candidate is a successful KEEP decision");

    assert_eq!(completed.state(), BackupMaintenanceRunState::Completed);
    assert_eq!(snapshot_state(&fixture, blocked).await, "COMPLETED");
    assert_eq!(
        expiry_entry_decision(&fixture, completed.expiry_plan_id().unwrap(), blocked).await,
        BackupSnapshotExpiryDecision::BlockedActiveRestorePlan
    );
    let restore_state: String =
        sqlx::query_scalar("SELECT state FROM backup_restore_plans WHERE id = $1")
            .bind(restore.id().into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .unwrap();
    assert_eq!(restore_state, "PLANNED");

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_maintenance_crash_recovery_replays_canonical_children() {
    let fixture = fixture("crash").await;
    let backup = BackupService::new(fixture.pool.clone());
    let set_id = create_set(&fixture, "crash-set").await;
    insert_completed_snapshot_days_old(&fixture, set_id, 100, true).await;
    configure(&fixture, set_id, "crash-policy", 1, 30 * 86_400).await;
    let run = backup
        .create_backup_maintenance_run(fixture.user_id, "crash-maintenance".to_owned(), set_id)
        .await
        .expect("maintenance run must persist");

    let capture = backup
        .capture_snapshot(
            fixture.user_id,
            set_id,
            SnapshotId::new(),
            run.capture_operation_id().to_owned(),
        )
        .await
        .expect("child capture commits before simulated coordinator crash");
    assert_eq!(run_state(&fixture, run.id()).await, "CREATED");
    let after_capture_replay = backup
        .advance_backup_maintenance_run(fixture.user_id, run.id())
        .await
        .expect("replay must finish maintenance");
    assert_eq!(
        after_capture_replay.captured_snapshot_id(),
        Some(capture.id()),
        "coordinator must record canonical capture replay"
    );
    assert_eq!(
        snapshot_count_for_operation(&fixture, run.capture_operation_id()).await,
        1
    );
    assert_eq!(pin_count(&fixture, capture.id()).await, 1);

    let plan_replay = backup
        .create_snapshot_expiry_plan(
            fixture.user_id,
            run.expiry_plan_operation_id().to_owned(),
            set_id,
        )
        .await
        .expect("canonical plan replay after completion returns same plan");
    assert_eq!(
        Some(plan_replay.id()),
        after_capture_replay.expiry_plan_id()
    );

    let execution_replay = backup
        .execute_snapshot_expiry_plan(fixture.user_id, plan_replay.id())
        .await
        .expect("canonical execution replay returns same receipt");
    assert_eq!(
        Some(execution_replay.id()),
        after_capture_replay.expiry_execution_id()
    );

    let plan_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_expiry_plans
         WHERE owner_user_id = $1 AND operation_id = $2",
    )
    .bind(fixture.user_id.into_uuid())
    .bind(run.expiry_plan_operation_id())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    let execution_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_expiry_executions WHERE expiry_plan_id = $1",
    )
    .bind(plan_replay.id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(plan_rows, 1);
    assert_eq!(execution_rows, 1);

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_maintenance_plan_and_execution_crash_windows_resume() {
    let fixture = fixture("planexec-crash").await;
    let backup = BackupService::new(fixture.pool.clone());
    let set_id = create_set(&fixture, "planexec-set").await;
    insert_completed_snapshot_days_old(&fixture, set_id, 100, false).await;
    configure(&fixture, set_id, "planexec-policy", 1, 30 * 86_400).await;
    let run = backup
        .create_backup_maintenance_run(fixture.user_id, "planexec-maintenance".to_owned(), set_id)
        .await
        .expect("maintenance run must persist");
    let captured = backup
        .capture_snapshot(
            fixture.user_id,
            set_id,
            SnapshotId::new(),
            run.capture_operation_id().to_owned(),
        )
        .await
        .unwrap();
    sqlx::query(
        "UPDATE backup_maintenance_runs
         SET state = 'SNAPSHOT_CAPTURED', captured_snapshot_id = $2, snapshot_captured_at = $3
         WHERE id = $1",
    )
    .bind(run.id().into_uuid())
    .bind(captured.id().into_uuid())
    .bind(captured.committed_at().unwrap().as_offset_datetime())
    .execute(&fixture.inspection)
    .await
    .expect("test setup persists capture progress");

    let plan = backup
        .create_snapshot_expiry_plan(
            fixture.user_id,
            run.expiry_plan_operation_id().to_owned(),
            set_id,
        )
        .await
        .expect("expiry plan commits before simulated coordinator crash");
    assert_eq!(run_state(&fixture, run.id()).await, "SNAPSHOT_CAPTURED");
    let completed = backup
        .advance_backup_maintenance_run(fixture.user_id, run.id())
        .await
        .expect("plan replay resumes and completes");
    assert_eq!(completed.expiry_plan_id(), Some(plan.id()));

    let second_set = create_set(&fixture, "execution-crash-set").await;
    insert_completed_snapshot_days_old(&fixture, second_set, 100, false).await;
    configure(
        &fixture,
        second_set,
        "execution-crash-policy",
        1,
        30 * 86_400,
    )
    .await;
    let run = backup
        .create_backup_maintenance_run(
            fixture.user_id,
            "execution-crash-maintenance".to_owned(),
            second_set,
        )
        .await
        .unwrap();
    let captured = backup
        .capture_snapshot(
            fixture.user_id,
            second_set,
            SnapshotId::new(),
            run.capture_operation_id().to_owned(),
        )
        .await
        .unwrap();
    let plan = backup
        .create_snapshot_expiry_plan(
            fixture.user_id,
            run.expiry_plan_operation_id().to_owned(),
            second_set,
        )
        .await
        .unwrap();
    sqlx::query(
        "UPDATE backup_maintenance_runs
         SET state = 'SNAPSHOT_CAPTURED', captured_snapshot_id = $2, snapshot_captured_at = $3
         WHERE id = $1",
    )
    .bind(run.id().into_uuid())
    .bind(captured.id().into_uuid())
    .bind(captured.committed_at().unwrap().as_offset_datetime())
    .execute(&fixture.inspection)
    .await
    .expect("test setup persists capture progress");
    sqlx::query(
        "UPDATE backup_maintenance_runs
         SET state = 'EXPIRY_PLANNED', expiry_plan_id = $2, expiry_planned_at = $3
         WHERE id = $1",
    )
    .bind(run.id().into_uuid())
    .bind(plan.id().into_uuid())
    .bind(plan.created_at().as_offset_datetime())
    .execute(&fixture.inspection)
    .await
    .expect("test setup persists plan progress");
    let execution = backup
        .execute_snapshot_expiry_plan(fixture.user_id, plan.id())
        .await
        .expect("expiry execution commits before simulated coordinator crash");
    assert_eq!(run_state(&fixture, run.id()).await, "EXPIRY_PLANNED");
    let completed = backup
        .advance_backup_maintenance_run(fixture.user_id, run.id())
        .await
        .expect("execution replay completes maintenance");
    assert_eq!(completed.expiry_execution_id(), Some(execution.id()));
    let execution_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_expiry_executions WHERE expiry_plan_id = $1",
    )
    .bind(plan.id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(execution_rows, 1);

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_maintenance_policy_change_before_planning_marks_stale() {
    let fixture = fixture("policy-stale").await;
    let backup = BackupService::new(fixture.pool.clone());
    let set_id = create_set(&fixture, "policy-stale-set").await;
    configure(&fixture, set_id, "policy-stale-r1", 1, 30 * 86_400).await;
    let run = backup
        .create_backup_maintenance_run(fixture.user_id, "policy-stale-maint".to_owned(), set_id)
        .await
        .unwrap();
    let captured = backup
        .capture_snapshot(
            fixture.user_id,
            set_id,
            SnapshotId::new(),
            run.capture_operation_id().to_owned(),
        )
        .await
        .unwrap();
    sqlx::query(
        "UPDATE backup_maintenance_runs
         SET state = 'SNAPSHOT_CAPTURED', captured_snapshot_id = $2, snapshot_captured_at = $3
         WHERE id = $1",
    )
    .bind(run.id().into_uuid())
    .bind(captured.id().into_uuid())
    .bind(captured.committed_at().unwrap().as_offset_datetime())
    .execute(&fixture.inspection)
    .await
    .unwrap();
    configure(&fixture, set_id, "policy-stale-r2", 2, 60 * 86_400).await;

    let result = backup
        .advance_backup_maintenance_run(fixture.user_id, run.id())
        .await;
    assert!(matches!(
        result,
        Err(BackupError::MaintenanceRunPreflight(_))
    ));
    assert_eq!(run_state(&fixture, run.id()).await, "STALE");
    let plan_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_expiry_plans WHERE operation_id = $1",
    )
    .bind(run.expiry_plan_operation_id())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(plan_count, 0);
    assert_eq!(snapshot_state(&fixture, captured.id()).await, "COMPLETED");

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_maintenance_stale_child_plan_marks_run_stale_without_replan() {
    let fixture = fixture("child-stale").await;
    let backup = BackupService::new(fixture.pool.clone());
    let set_id = create_set(&fixture, "child-stale-set").await;
    let target = insert_completed_snapshot_days_old(&fixture, set_id, 100, true).await;
    configure(&fixture, set_id, "child-stale-policy", 1, 30 * 86_400).await;
    let run = backup
        .create_backup_maintenance_run(fixture.user_id, "child-stale-maint".to_owned(), set_id)
        .await
        .unwrap();
    let captured = backup
        .capture_snapshot(
            fixture.user_id,
            set_id,
            SnapshotId::new(),
            run.capture_operation_id().to_owned(),
        )
        .await
        .unwrap();
    let plan = backup
        .create_snapshot_expiry_plan(
            fixture.user_id,
            run.expiry_plan_operation_id().to_owned(),
            set_id,
        )
        .await
        .unwrap();
    sqlx::query(
        "UPDATE backup_maintenance_runs
         SET state = 'SNAPSHOT_CAPTURED', captured_snapshot_id = $2, snapshot_captured_at = $3
         WHERE id = $1",
    )
    .bind(run.id().into_uuid())
    .bind(captured.id().into_uuid())
    .bind(captured.committed_at().unwrap().as_offset_datetime())
    .execute(&fixture.inspection)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE backup_maintenance_runs
         SET state = 'EXPIRY_PLANNED', expiry_plan_id = $2, expiry_planned_at = $3
         WHERE id = $1",
    )
    .bind(run.id().into_uuid())
    .bind(plan.id().into_uuid())
    .bind(plan.created_at().as_offset_datetime())
    .execute(&fixture.inspection)
    .await
    .unwrap();
    backup
        .create_restore_plan(
            fixture.user_id,
            "child-stale-restore".to_owned(),
            set_id,
            target,
            fixture.library.id(),
            fixture.root.id(),
            name("Child Stale Restore"),
        )
        .await
        .expect("new blocker creates Prompt 48 stale cause");

    let result = backup
        .advance_backup_maintenance_run(fixture.user_id, run.id())
        .await;
    assert!(matches!(
        result,
        Err(BackupError::MaintenanceRunPreflight(_))
    ));
    assert_eq!(run_state(&fixture, run.id()).await, "STALE");
    assert_eq!(snapshot_state(&fixture, target).await, "COMPLETED");
    let plan_state: String =
        sqlx::query_scalar("SELECT state FROM backup_snapshot_expiry_plans WHERE id = $1")
            .bind(plan.id().into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .unwrap();
    assert_eq!(plan_state, BackupSnapshotExpiryPlanState::Stale.as_str());
    let plan_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_expiry_plans WHERE backup_set_id = $1",
    )
    .bind(set_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(plan_rows, 1, "maintenance must not create replacement plan");

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_maintenance_idempotent_and_one_active_creation() {
    let fixture = fixture("idempotency").await;
    let backup = BackupService::new(fixture.pool.clone());
    let set_id = create_set(&fixture, "idempotency-set").await;
    let second_set = create_set(&fixture, "idempotency-other-set").await;
    configure(&fixture, set_id, "idempotency-policy", 1, 30 * 86_400).await;
    configure(
        &fixture,
        second_set,
        "idempotency-other-policy",
        1,
        30 * 86_400,
    )
    .await;

    let first = backup
        .create_backup_maintenance_run(fixture.user_id, "idempotency-maint".to_owned(), set_id)
        .await
        .unwrap();
    let same = backup
        .create_backup_maintenance_run(fixture.user_id, "idempotency-maint".to_owned(), set_id)
        .await
        .unwrap();
    assert_eq!(first.id(), same.id());
    let conflict = backup
        .create_backup_maintenance_run(fixture.user_id, "idempotency-maint".to_owned(), second_set)
        .await;
    assert!(matches!(
        conflict,
        Err(BackupError::MaintenanceRunPreflight(_))
    ));
    let active_conflict = backup
        .create_backup_maintenance_run(fixture.user_id, "idempotency-maint-2".to_owned(), set_id)
        .await;
    assert!(matches!(
        active_conflict,
        Err(BackupError::MaintenanceRunPreflight(_))
    ));

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_maintenance_concurrent_creation_and_advance_are_idempotent() {
    let fixture = fixture("concurrency").await;
    let backup = BackupService::new(fixture.pool.clone());
    let set_id = create_set(&fixture, "concurrency-set").await;
    insert_completed_snapshot_days_old(&fixture, set_id, 100, false).await;
    configure(&fixture, set_id, "concurrency-policy", 1, 30 * 86_400).await;

    let create_a = backup.clone();
    let create_b = backup.clone();
    let (a, b) = tokio::join!(
        create_a.create_backup_maintenance_run(
            fixture.user_id,
            "concurrent-create".to_owned(),
            set_id
        ),
        create_b.create_backup_maintenance_run(
            fixture.user_id,
            "concurrent-create".to_owned(),
            set_id
        )
    );
    let run_a = a.expect("first creator must return run");
    let run_b = b.expect("second creator must return same run");
    assert_eq!(run_a.id(), run_b.id());

    let advance_a = backup.clone();
    let advance_b = backup.clone();
    let (a, b) = tokio::join!(
        advance_a.advance_backup_maintenance_run(fixture.user_id, run_a.id()),
        advance_b.advance_backup_maintenance_run(fixture.user_id, run_a.id())
    );
    let completed = match (a, b) {
        (Ok(run), _) | (_, Ok(run)) => run,
        (Err(left), Err(right)) => panic!("both advance calls failed: {left:?}; {right:?}"),
    };
    assert_eq!(completed.state(), BackupMaintenanceRunState::Completed);
    let fresh = backup
        .get_backup_maintenance_run(fixture.user_id, run_a.id())
        .await
        .unwrap();
    assert_eq!(fresh.state(), BackupMaintenanceRunState::Completed);
    assert_eq!(
        snapshot_count_for_operation(&fixture, run_a.capture_operation_id()).await,
        1
    );
    let plan_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_expiry_plans WHERE operation_id = $1",
    )
    .bind(run_a.expiry_plan_operation_id())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(plan_count, 1);
    let execution_count: i64 = sqlx::query_scalar(
        "SELECT count(*)
         FROM backup_snapshot_expiry_executions AS execution
         INNER JOIN backup_snapshot_expiry_plans AS plan
            ON plan.id = execution.expiry_plan_id
         WHERE plan.operation_id = $1",
    )
    .bind(run_a.expiry_plan_operation_id())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(execution_count, 1);

    fixture.pool.close().await;
    fixture.inspection.close().await;
}
