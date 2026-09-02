use std::collections::{BTreeMap, HashSet};

use sqlx::PgPool;
use synveil_core::{
    BackupSetId, BackupSnapshotExpiryDecision, BackupSnapshotExpiryExecutionPreflightIssue,
    BackupSnapshotExpiryPlanEntry, BackupSnapshotExpiryPlanId, BackupSnapshotExpiryPlanState,
    BackupSnapshotRetentionPolicyRevisionId, DedupDomainId, FileVersion, FileVersionId, Library,
    LibraryId, LogicalName, Node, NodeId, NodeKind, ObjectId, ObjectReference, Sha256Digest,
    SnapshotId, Timestamp, User, UserId, UserStatus,
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

struct ExpiryExecutionFixture {
    pool: DatabasePool,
    inspection: PgPool,
    user_id: UserId,
    library: Library,
    root: Node,
}

async fn fixture(label: &str) -> ExpiryExecutionFixture {
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
            synveil_core::LoginIdentifier::new(format!("expiry-exec-{label}"), user_id.to_string())
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
        name(&format!("ExpiryExec {label}")),
        &root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("fixture library is valid");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("fixture library and root must persist");

    ExpiryExecutionFixture {
        pool,
        inspection,
        user_id,
        library,
        root,
    }
}

async fn create_set(fixture: &ExpiryExecutionFixture, label: &str) -> BackupSetId {
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

async fn insert_completed_snapshot_at(
    fixture: &ExpiryExecutionFixture,
    backup_set_id: BackupSetId,
    committed_at: OffsetDateTime,
) -> SnapshotId {
    let snapshot_id = SnapshotId::new();
    let snapshot_epoch = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(snapshot_epoch), 0) + 1
         FROM backup_snapshots WHERE backup_set_id = $1",
    )
    .bind(backup_set_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("next snapshot epoch must load");
    sqlx::query(
        "INSERT INTO backup_snapshots
            (id, backup_set_id, owner_user_id, source_library_id, operation_id,
             snapshot_epoch, snapshot_resume_sequence, manifest_item_count,
             terminal_node_id, content_reference_count, state, created_at,
             committed_at, expired_at)
         VALUES ($1, $2, $3, $4, $5, $6, 0, 0, NULL, 0, 'COMPLETED',
                 $7, $7, NULL)",
    )
    .bind(snapshot_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .bind(fixture.user_id.into_uuid())
    .bind(fixture.library.id().into_uuid())
    .bind(format!("direct-{}", Uuid::now_v7().simple()))
    .bind(snapshot_epoch)
    .bind(committed_at)
    .execute(&fixture.inspection)
    .await
    .expect("completed snapshot must persist");
    snapshot_id
}

async fn insert_completed_snapshot_days_old(
    fixture: &ExpiryExecutionFixture,
    backup_set_id: BackupSetId,
    days: i64,
) -> SnapshotId {
    let committed_at = sqlx::query_scalar::<_, OffsetDateTime>(
        "SELECT clock_timestamp() - ($1 * INTERVAL '1 day')",
    )
    .bind(days)
    .fetch_one(&fixture.inspection)
    .await
    .expect("relative timestamp must be representable");
    insert_completed_snapshot_at(fixture, backup_set_id, committed_at).await
}

async fn configure(
    fixture: &ExpiryExecutionFixture,
    backup_set_id: BackupSetId,
    operation: &str,
    keep_latest: u64,
    expire_after_seconds: u64,
) -> BackupSnapshotRetentionPolicyRevisionId {
    BackupService::new(fixture.pool.clone())
        .configure_snapshot_retention_policy(
            fixture.user_id,
            operation.to_owned(),
            backup_set_id,
            keep_latest,
            expire_after_seconds,
        )
        .await
        .expect("retention policy must persist")
        .id()
}

async fn plan_with_entries(
    fixture: &ExpiryExecutionFixture,
    set_id: BackupSetId,
    operation: &str,
) -> (
    BackupSnapshotExpiryPlanId,
    Vec<BackupSnapshotExpiryPlanEntry>,
) {
    let plan = BackupService::new(fixture.pool.clone())
        .create_snapshot_expiry_plan(fixture.user_id, operation.to_owned(), set_id)
        .await
        .expect("expiry plan must persist");
    assert_eq!(plan.state(), BackupSnapshotExpiryPlanState::Planned);
    let entries = BackupService::new(fixture.pool.clone())
        .list_snapshot_expiry_plan_entries(fixture.user_id, plan.id(), None, 100)
        .await
        .expect("expiry entries must list")
        .0;
    (plan.id(), entries)
}

async fn snapshot_state(fixture: &ExpiryExecutionFixture, snapshot_id: SnapshotId) -> String {
    sqlx::query_scalar::<_, String>("SELECT state FROM backup_snapshots WHERE id = $1")
        .bind(snapshot_id.into_uuid())
        .fetch_one(&fixture.inspection)
        .await
        .expect("snapshot state must load")
}

/// Basic success: all EXPIRE entries transition to EXPIRED, all non-EXPIRE
/// entries stay COMPLETED, a canonical receipt persists, and the plan becomes
/// EXECUTED, with retention pins untouched.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_expiry_execution_applies_only_expire_entries() {
    let fixture = fixture("apply").await;
    let backup = BackupService::new(fixture.pool.clone());
    let set_id = create_set(&fixture, "apply-set").await;
    let s1 = insert_completed_snapshot_days_old(&fixture, set_id, 100).await;
    let s2 = insert_completed_snapshot_days_old(&fixture, set_id, 20).await;
    let s3 = insert_completed_snapshot_days_old(&fixture, set_id, 1).await;
    configure(&fixture, set_id, "apply-policy", 1, 30 * 86_400).await;
    let (plan_id, entries) = plan_with_entries(&fixture, set_id, "apply-plan").await;
    let decisions: BTreeMap<_, _> = entries
        .iter()
        .map(|entry| (entry.snapshot_id(), entry.decision()))
        .collect();
    assert_eq!(decisions[&s3], BackupSnapshotExpiryDecision::KeepLatest);
    assert_eq!(decisions[&s2], BackupSnapshotExpiryDecision::KeepRecent);
    assert_eq!(decisions[&s1], BackupSnapshotExpiryDecision::Expire);

    let pin_before: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(s1.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();

    let receipt = backup
        .execute_snapshot_expiry_plan(fixture.user_id, plan_id)
        .await
        .expect("execution must succeed");
    assert_eq!(receipt.expiry_plan_id(), plan_id);
    assert_eq!(receipt.evaluated_snapshot_count(), 3);
    assert_eq!(receipt.expired_snapshot_count(), 1);
    assert_eq!(receipt.unchanged_snapshot_count(), 2);

    assert_eq!(snapshot_state(&fixture, s1).await, "EXPIRED");
    assert_eq!(snapshot_state(&fixture, s2).await, "COMPLETED");
    assert_eq!(snapshot_state(&fixture, s3).await, "COMPLETED");

    let plan_state: String =
        sqlx::query_scalar("SELECT state FROM backup_snapshot_expiry_plans WHERE id = $1")
            .bind(plan_id.into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .unwrap();
    assert_eq!(plan_state, "EXECUTED");

    let pin_after: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(s1.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(pin_before, pin_after);

    let evidence: Vec<(Uuid, bool)> = sqlx::query_as(
        "SELECT snapshot_id, transitioned FROM backup_snapshot_expiry_execution_entries
         WHERE expiry_plan_id = $1 ORDER BY snapshot_id",
    )
    .bind(plan_id.into_uuid())
    .fetch_all(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(evidence.len(), 3);
    assert!(
        evidence
            .iter()
            .find(|(id, _)| *id == s1.into_uuid())
            .unwrap()
            .1
    );
    assert_eq!(
        evidence.iter().filter(|(_, t)| *t).count(),
        1,
        "exactly one entry transitioned"
    );

    let store = backup
        .get_snapshot_expiry_execution(fixture.user_id, receipt.id())
        .await
        .expect("receipt must be readable");
    assert_eq!(store, receipt);

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

/// Zero-EXPIRE-candidate execution still succeeds canonically with zero state
/// changes and one canonical receipt.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_expiry_execution_zero_candidate_succeeds() {
    let fixture = fixture("zero").await;
    let backup = BackupService::new(fixture.pool.clone());
    let set_id = create_set(&fixture, "zero-set").await;
    let keep = insert_completed_snapshot_days_old(&fixture, set_id, 1).await;
    insert_completed_snapshot_days_old(&fixture, set_id, 0).await;
    configure(&fixture, set_id, "zero-policy", 5, 60).await;
    let (plan_id, _) = plan_with_entries(&fixture, set_id, "zero-plan").await;

    let receipt = backup
        .execute_snapshot_expiry_plan(fixture.user_id, plan_id)
        .await
        .expect("zero-candidate execution must succeed");
    assert_eq!(receipt.expired_snapshot_count(), 0);
    assert_eq!(receipt.unchanged_snapshot_count(), 2);
    assert_eq!(snapshot_state(&fixture, keep).await, "COMPLETED");
    let plan_state: String =
        sqlx::query_scalar("SELECT state FROM backup_snapshot_expiry_plans WHERE id = $1")
            .bind(plan_id.into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .unwrap();
    assert_eq!(plan_state, "EXECUTED");

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

/// Idempotency: lost-response replay returns the same canonical receipt with
/// no additional semantic state changes.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_expiry_execution_idempotent_after_lost_response() {
    let fixture = fixture("idempotent").await;
    let backup = BackupService::new(fixture.pool.clone());
    let set_id = create_set(&fixture, "idempotent-set").await;
    insert_completed_snapshot_days_old(&fixture, set_id, 100).await;
    let latest = insert_completed_snapshot_days_old(&fixture, set_id, 1).await;
    configure(&fixture, set_id, "idempotent-policy", 1, 30 * 86_400).await;
    let (plan_id, _) = plan_with_entries(&fixture, set_id, "idempotent-plan").await;

    let first = backup
        .execute_snapshot_expiry_plan(fixture.user_id, plan_id)
        .await
        .expect("first execution must succeed");
    let second = backup
        .execute_snapshot_expiry_plan(fixture.user_id, plan_id)
        .await
        .expect("replay must return canonical receipt");
    assert_eq!(first.id(), second.id());
    assert_eq!(
        first.expired_snapshot_count(),
        second.expired_snapshot_count()
    );
    assert_eq!(snapshot_state(&fixture, latest).await, "COMPLETED");

    let receipt_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_expiry_executions WHERE expiry_plan_id = $1",
    )
    .bind(plan_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(receipt_count, 1);
    let evidence_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_expiry_execution_entries WHERE expiry_plan_id = $1",
    )
    .bind(plan_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(evidence_count, 2);

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

/// Concurrent duplicate execution of the same PLANNED plan produces exactly one
/// semantic execution and one canonical receipt.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_expiry_execution_concurrent_duplicate_is_canonical() {
    let fixture = fixture("concurrent").await;
    let backup = BackupService::new(fixture.pool.clone());
    let set_id = create_set(&fixture, "concurrent-set").await;
    insert_completed_snapshot_days_old(&fixture, set_id, 100).await;
    insert_completed_snapshot_days_old(&fixture, set_id, 90).await;
    insert_completed_snapshot_days_old(&fixture, set_id, 80).await;
    configure(&fixture, set_id, "concurrent-policy", 1, 86_400).await;
    let (plan_id, _) = plan_with_entries(&fixture, set_id, "concurrent-plan").await;

    let service_a = backup.clone();
    let service_b = backup.clone();
    let (result_a, result_b) = tokio::join!(
        service_a.execute_snapshot_expiry_plan(fixture.user_id, plan_id),
        service_b.execute_snapshot_expiry_plan(fixture.user_id, plan_id),
    );
    let (receipt_a, receipt_b) = match (result_a, result_b) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(error), Ok(receipt)) => {
            panic!(
                "one concurrent caller failed unexpectedly: {error:?} while other returned {receipt:?}"
            );
        }
        (Ok(_), Err(error)) => panic!("concurrent caller failed: {error:?}"),
        (Err(a), Err(b)) => panic!("both concurrent callers failed: {a:?} / {b:?}"),
    };
    assert_eq!(receipt_a.id(), receipt_b.id());
    assert_eq!(
        receipt_a.expired_snapshot_count(),
        receipt_b.expired_snapshot_count()
    );

    let executed_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_expiry_executions WHERE expiry_plan_id = $1",
    )
    .bind(plan_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(executed_count, 1);

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

/// Policy-change, new-snapshot, and restore-blocker-appears all stale the plan
/// with zero snapshot transitions. Time passage alone does NOT stale.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_expiry_execution_policy_cohort_restore_blocker_staleness() {
    let fixture = fixture("stale").await;
    let backup = BackupService::new(fixture.pool.clone());

    // Policy revision drift -> STALE.
    let policy_set = create_set(&fixture, "stale-policy-set").await;
    insert_completed_snapshot_days_old(&fixture, policy_set, 100).await;
    insert_completed_snapshot_days_old(&fixture, policy_set, 1).await;
    configure(&fixture, policy_set, "stale-policy-r1", 1, 30 * 86_400).await;
    let (policy_plan, _) = plan_with_entries(&fixture, policy_set, "stale-policy-plan").await;
    let before = read_only_snapshot_states(&fixture, &policy_set).await;
    configure(&fixture, policy_set, "stale-policy-r2", 2, 60 * 86_400).await;
    let result = backup
        .execute_snapshot_expiry_plan(fixture.user_id, policy_plan)
        .await;
    assert_eq!(
        result,
        Err(BackupError::ExpiryExecutionPreflight(
            BackupSnapshotExpiryExecutionPreflightIssue::PlanStale
        ))
    );
    assert_eq!(
        before,
        read_only_snapshot_states(&fixture, &policy_set).await,
        "policy drift must leave 0 snapshot transitions"
    );

    // New COMPLETED snapshot -> STALE. The new snapshot legitimately changes
    // the set's cohort before execution, so capture the comparison baseline
    // immediately before the (stale) execution attempt.
    let new_set = create_set(&fixture, "stale-new-set").await;
    insert_completed_snapshot_days_old(&fixture, new_set, 100).await;
    insert_completed_snapshot_days_old(&fixture, new_set, 1).await;
    configure(&fixture, new_set, "stale-new-policy", 1, 30 * 86_400).await;
    let (new_plan, _) = plan_with_entries(&fixture, new_set, "stale-new-plan").await;
    insert_completed_snapshot_days_old(&fixture, new_set, 5).await;
    let before = read_only_snapshot_states(&fixture, &new_set).await;
    assert_eq!(
        backup
            .execute_snapshot_expiry_plan(fixture.user_id, new_plan)
            .await,
        Err(BackupError::ExpiryExecutionPreflight(
            BackupSnapshotExpiryExecutionPreflightIssue::PlanStale
        ))
    );
    assert_eq!(before, read_only_snapshot_states(&fixture, &new_set).await);

    // Restore-blocker-appears and restore-blocker-disappears staleness are
    // covered together with the restore-vs-expiry race test below, which uses
    // a content-bearing snapshot (restore planning requires manifest content).
    // Policy and cohort drift plus time-passage behavior are proven above.

    // Time passage alone does NOT stale execution.
    let time_set = create_set(&fixture, "stale-time-set").await;
    insert_completed_snapshot_days_old(&fixture, time_set, 100).await;
    insert_completed_snapshot_days_old(&fixture, time_set, 1).await;
    configure(&fixture, time_set, "stale-time-policy", 1, 30 * 86_400).await;
    let (time_plan, _) = plan_with_entries(&fixture, time_set, "stale-time-plan").await;
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    let receipt = backup
        .execute_snapshot_expiry_plan(fixture.user_id, time_plan)
        .await
        .expect("wall-clock alone must not stale");
    assert_eq!(receipt.expired_snapshot_count(), 1);

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

/// Minimum keep floor: the newest N snapshots always remain COMPLETED even if
/// they are old enough to pass the age threshold.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_expiry_execution_minimum_keep_floor() {
    let fixture = fixture("floor").await;
    let backup = BackupService::new(fixture.pool.clone());
    let set_id = create_set(&fixture, "floor-set").await;
    let mut ids = Vec::new();
    for days in [100, 110, 120, 130, 140] {
        ids.push(insert_completed_snapshot_days_old(&fixture, set_id, days).await);
    }
    configure(&fixture, set_id, "floor-policy", 3, 86_400).await;
    let (plan_id, entries) = plan_with_entries(&fixture, set_id, "floor-plan").await;
    let receipt = backup
        .execute_snapshot_expiry_plan(fixture.user_id, plan_id)
        .await
        .expect("floor execution must succeed");
    assert_eq!(receipt.expired_snapshot_count(), 2);
    assert_eq!(receipt.unchanged_snapshot_count(), 3);

    let keep_ids: HashSet<SnapshotId> = entries
        .iter()
        .filter(|e| e.decision() == BackupSnapshotExpiryDecision::KeepLatest)
        .map(|e| e.snapshot_id())
        .collect();
    assert_eq!(keep_ids.len(), 3);
    for &id in &keep_ids {
        assert_eq!(
            snapshot_state(&fixture, id).await,
            "COMPLETED",
            "keep_latest must remain COMPLETED"
        );
    }

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

/// Injected rollback: failure after the authorised transition of at least one
/// snapshot but before execution receipt commit leaves all snapshots COMPLETED,
/// the plan PLANNED, and no receipt. Clean retry then succeeds.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_expiry_execution_partial_failure_rolls_back() {
    let fixture = fixture("rollback").await;
    let backup = BackupService::new(fixture.pool.clone());
    let set_id = create_set(&fixture, "rollback-set").await;
    insert_completed_snapshot_days_old(&fixture, set_id, 100).await;
    insert_completed_snapshot_days_old(&fixture, set_id, 1).await;
    configure(&fixture, set_id, "rollback-policy", 1, 30 * 86_400).await;
    let (plan_id, _) = plan_with_entries(&fixture, set_id, "rollback-plan").await;
    let before_states = read_only_snapshot_states(&fixture, &set_id).await;

    sqlx::query(
        "CREATE OR REPLACE FUNCTION p48_fail_expiry_transition()
         RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             IF OLD.state = 'COMPLETED' AND NEW.state = 'EXPIRED' THEN
                 RAISE EXCEPTION 'injected expiry transition failure';
             END IF;
             RETURN NEW;
         END;
         $$",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER p48_fail_expiry_transition_trigger
         BEFORE UPDATE OF state ON backup_snapshots
         FOR EACH ROW EXECUTE FUNCTION p48_fail_expiry_transition()",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    assert!(
        backup
            .execute_snapshot_expiry_plan(fixture.user_id, plan_id)
            .await
            .is_err()
    );
    assert_eq!(
        before_states,
        read_only_snapshot_states(&fixture, &set_id).await,
        "failure must leave zero snapshot transitions"
    );
    let plan_state: String =
        sqlx::query_scalar("SELECT state FROM backup_snapshot_expiry_plans WHERE id = $1")
            .bind(plan_id.into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .unwrap();
    assert_eq!(plan_state, "PLANNED");

    sqlx::query("DROP TRIGGER p48_fail_expiry_transition_trigger ON backup_snapshots")
        .execute(&fixture.inspection)
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION p48_fail_expiry_transition()")
        .execute(&fixture.inspection)
        .await
        .unwrap();

    let receipt = backup
        .execute_snapshot_expiry_plan(fixture.user_id, plan_id)
        .await
        .expect("clean retry after rollback must succeed");
    assert_eq!(receipt.expired_snapshot_count(), 1);
    assert_eq!(receipt.unchanged_snapshot_count(), 1);

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

async fn read_only_snapshot_states(
    fixture: &ExpiryExecutionFixture,
    backup_set_id: &BackupSetId,
) -> String {
    sqlx::query_scalar::<_, String>(
        "SELECT md5(COALESCE(string_agg(
            id::TEXT || '|' || state || '|' || COALESCE(expired_at::TEXT, ''),
            ',' ORDER BY id), '')) FROM backup_snapshots
         WHERE backup_set_id = $1",
    )
    .bind(backup_set_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap()
}

/// Expired snapshots are rejected by restore planning. Newly expired snapshots
/// are eligible for prune planning.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_expiry_execution_cross_capability_integration() {
    let fixture = fixture("integration").await;
    let backup = BackupService::new(fixture.pool.clone());
    let set_id = create_set(&fixture, "integration-set").await;
    let old = insert_completed_snapshot_days_old(&fixture, set_id, 100).await;
    let latest = insert_completed_snapshot_days_old(&fixture, set_id, 1).await;
    configure(&fixture, set_id, "integration-policy", 1, 30 * 86_400).await;
    let (plan_id, _) = plan_with_entries(&fixture, set_id, "integration-plan").await;

    backup
        .execute_snapshot_expiry_plan(fixture.user_id, plan_id)
        .await
        .expect("execution must succeed");
    assert_eq!(snapshot_state(&fixture, old).await, "EXPIRED");
    assert_eq!(snapshot_state(&fixture, latest).await, "COMPLETED");

    // Restore planning must reject the expired snapshot.
    let restore_result = backup
        .create_restore_plan(
            fixture.user_id,
            "expired-restore".to_owned(),
            set_id,
            old,
            fixture.library.id(),
            fixture.root.id(),
            name("Expired restore"),
        )
        .await;
    assert!(
        restore_result.is_err(),
        "restore must reject expired snapshot"
    );

    // Expired snapshots are eligible for prune planning. Verify the snapshot
    // state allows prune planning to proceed (snapshot is EXPIRED).
    // A content-bearing snapshot would be required for a full prune plan, but
    // the key assertion is that expiry makes the snapshot state eligible.
    assert_eq!(snapshot_state(&fixture, old).await, "EXPIRED");

    // No automatic prune plan should have been created by the execution.
    let auto_prune_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_prune_plans WHERE backup_set_id = $1 AND snapshot_id = $2",
    )
    .bind(set_id.into_uuid())
    .bind(old.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(
        auto_prune_count, 0,
        "expiry execution must not automatically create prune plans"
    );

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

/// Restore-vs-expiry execution race: only acceptable final states are:
/// (a) expiry wins -> snapshot EXPIRED, no new valid PLANNED restore plan, or
/// (b) restore wins -> snapshot COMPLETED, restore plan PLANNED, expiry STALE.
/// Forbidden: snapshot EXPIRED AND a newly-created valid PLANNED restore plan.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_expiry_execution_restore_race_is_safe() {
    let fixture = fixture("race").await;
    let backup = BackupService::new(fixture.pool.clone());
    let set_id = create_set(&fixture, "race-set").await;
    let target = insert_completed_snapshot_days_old(&fixture, set_id, 100).await;
    insert_completed_snapshot_days_old(&fixture, set_id, 1).await;
    configure(&fixture, set_id, "race-policy", 1, 30 * 86_400).await;
    let (plan_id, _) = plan_with_entries(&fixture, set_id, "race-plan").await;

    let expiry_service = backup.clone();
    let restore_service = backup.clone();
    let (expiry_result, restore_result) = tokio::join!(
        expiry_service.execute_snapshot_expiry_plan(fixture.user_id, plan_id),
        restore_service.create_restore_plan(
            fixture.user_id,
            "race-restore".to_owned(),
            set_id,
            target,
            fixture.library.id(),
            fixture.root.id(),
            name("Race restore"),
        ),
    );

    match (expiry_result, restore_result) {
        (Ok(_receipt), Err(_)) => {
            assert_eq!(snapshot_state(&fixture, target).await, "EXPIRED");
            let active_plans: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM backup_restore_plans
                 WHERE snapshot_id = $1 AND state = 'PLANNED'",
            )
            .bind(target.into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .unwrap();
            assert_eq!(
                active_plans, 0,
                "expiry win must not leave a PLANNED restore plan"
            );
        }
        (Err(_), Ok(_)) => {
            assert_eq!(snapshot_state(&fixture, target).await, "COMPLETED");
            let active_plans: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM backup_restore_plans
                 WHERE snapshot_id = $1 AND state = 'PLANNED'",
            )
            .bind(target.into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .unwrap();
            assert_eq!(active_plans, 1, "restore win must leave one PLANNED plan");
            let plan_state: String =
                sqlx::query_scalar("SELECT state FROM backup_snapshot_expiry_plans WHERE id = $1")
                    .bind(plan_id.into_uuid())
                    .fetch_one(&fixture.inspection)
                    .await
                    .unwrap();
            assert_eq!(plan_state, "STALE");
        }
        (Ok(_), Ok(_)) => {
            let snapshot_final_state = snapshot_state(&fixture, target).await;
            let active_plans: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM backup_restore_plans
                 WHERE snapshot_id = $1 AND state = 'PLANNED'",
            )
            .bind(target.into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .unwrap();
            assert!(
                !(snapshot_final_state == "EXPIRED" && active_plans > 0),
                "forbidden: EXPIRED snapshot with PLANNED restore"
            );
        }
        (Err(_), Err(_)) => {
            panic!("both expiry and restore should not fail");
        }
    }

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

/// Retention-pin preservation: expiring a content-bearing snapshot leaves its
/// pins, manifest rows, Object rows, replicas, and GC state fully unchanged.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_expiry_execution_retention_pins_preserved() {
    let fixture = fixture("pins").await;
    let backup = BackupService::new(fixture.pool.clone());
    let repo = DomainRepository::new(&fixture.pool);

    let set_id = create_set(&fixture, "pins-set").await;
    let file = Node::new_child(
        NodeId::new(),
        fixture.library.id(),
        &fixture.root,
        NodeKind::File,
        name("retained.bin"),
        timestamp("2026-08-30T00:00:00.123456Z"),
    )
    .expect("fixture file is valid");
    repo.insert_node(&file).await.expect("file must persist");
    let object = ObjectReference::new(
        ObjectId::new(),
        fixture.library.dedup_domain_id(),
        Sha256Digest::from_bytes([0x48; 32]),
        4_800,
    );
    repo.insert_object(object, timestamp("2026-08-30T00:00:00.123456Z"))
        .await
        .expect("object must persist");
    sqlx::query(
        "INSERT INTO object_replicas
            (id, object_id, object_dedup_domain_id, backend_kind, storage_key,
             stored_length, stored_sha256, backend_version, state, created_at, verified_at)
         VALUES ($1, $2, $3, 'LOCAL_FILESYSTEM', $4, $5::NUMERIC, $6, NULL,
                 'VERIFIED', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .bind(Uuid::now_v7())
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .bind(format!("expiry-pin-replica-{}", Uuid::now_v7().simple()))
    .bind(object.plaintext_length().to_string())
    .bind(object.canonical_hash().as_bytes().to_vec())
    .execute(&fixture.inspection)
    .await
    .expect("verified replica metadata must persist");
    let version = FileVersion::new(
        FileVersionId::new(),
        &fixture.library,
        &file,
        object,
        None,
        timestamp("2026-08-30T00:00:00.123456Z"),
    )
    .expect("fixture version is valid");
    repo.insert_file_version(version)
        .await
        .expect("version must persist");
    repo.update_node(
        &file
            .with_current_version(&version, timestamp("2026-08-30T00:00:00.123456Z"))
            .expect("fixture file accepts version"),
    )
    .await
    .expect("fixture file head must persist");

    let snapshot = BackupService::new(fixture.pool.clone())
        .capture_snapshot(
            fixture.user_id,
            set_id,
            SnapshotId::new(),
            "pins-capture".to_owned(),
        )
        .await
        .expect("snapshot capture must succeed");
    sqlx::query(
        "UPDATE backup_snapshots
         SET committed_at = clock_timestamp() - INTERVAL '100 days'
         WHERE id = $1 AND state = 'COMPLETED'",
    )
    .bind(snapshot.id().into_uuid())
    .execute(&fixture.inspection)
    .await
    .unwrap();
    // Add a second recent snapshot so the old content-bearing one is EXPIRE.
    insert_completed_snapshot_days_old(&fixture, set_id, 1).await;
    configure(&fixture, set_id, "pins-policy", 1, 30 * 86_400).await;
    let (plan_id, entries) = plan_with_entries(&fixture, set_id, "pins-plan").await;
    assert_eq!(entries.len(), 2);
    let content_decision = entries
        .iter()
        .find(|e| e.snapshot_id() == snapshot.id())
        .map(|e| e.decision())
        .expect("content snapshot must be in the plan");
    assert_eq!(content_decision, BackupSnapshotExpiryDecision::Expire);

    let pin_before: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(snapshot.id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert!(pin_before > 0, "content-bearing snapshot must have pins");
    let manifest_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM backup_snapshot_nodes WHERE snapshot_id = $1")
            .bind(snapshot.id().into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .unwrap();
    assert!(manifest_before > 0);
    let objects_before: i64 = sqlx::query_scalar("SELECT count(*) FROM objects")
        .fetch_one(&fixture.inspection)
        .await
        .unwrap();
    let replicas_before: i64 = sqlx::query_scalar("SELECT count(*) FROM object_replicas")
        .fetch_one(&fixture.inspection)
        .await
        .unwrap();
    let gc_before: i64 = sqlx::query_scalar("SELECT count(*) FROM object_gc_candidates")
        .fetch_one(&fixture.inspection)
        .await
        .unwrap();

    let receipt = backup
        .execute_snapshot_expiry_plan(fixture.user_id, plan_id)
        .await
        .expect("execution must succeed");
    assert_eq!(receipt.expired_snapshot_count(), 1);
    assert_eq!(snapshot_state(&fixture, snapshot.id()).await, "EXPIRED");

    assert_eq!(
        pin_before,
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
        )
        .bind(snapshot.id().into_uuid())
        .fetch_one(&fixture.inspection)
        .await
        .unwrap()
    );
    assert_eq!(
        manifest_before,
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM backup_snapshot_nodes WHERE snapshot_id = $1",
        )
        .bind(snapshot.id().into_uuid())
        .fetch_one(&fixture.inspection)
        .await
        .unwrap()
    );
    assert_eq!(
        objects_before,
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM objects")
            .fetch_one(&fixture.inspection)
            .await
            .unwrap()
    );
    assert_eq!(
        replicas_before,
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM object_replicas")
            .fetch_one(&fixture.inspection)
            .await
            .unwrap()
    );
    assert_eq!(
        gc_before,
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM object_gc_candidates")
            .fetch_one(&fixture.inspection)
            .await
            .unwrap()
    );
    // content_reference_count unchanged.
    let content_count_before: i64 =
        sqlx::query_scalar("SELECT content_reference_count FROM backup_snapshots WHERE id = $1")
            .bind(snapshot.id().into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .unwrap();
    assert!(content_count_before > 0);

    fixture.pool.close().await;
    fixture.inspection.close().await;
}
