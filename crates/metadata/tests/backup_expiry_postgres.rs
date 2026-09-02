use std::collections::BTreeMap;

use sqlx::PgPool;
use synveil_core::{
    BackupSetId, BackupSnapshotExpiryBasisEntry, BackupSnapshotExpiryBasisFingerprint,
    BackupSnapshotExpiryDecision, BackupSnapshotExpiryPlanId, BackupSnapshotExpiryPlanRequest,
    BackupSnapshotExpiryPlanState, BackupSnapshotExpiryPreflightIssue,
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

struct ExpiryFixture {
    pool: DatabasePool,
    inspection: PgPool,
    user_id: UserId,
    library: Library,
    root: Node,
}

async fn fixture(label: &str) -> ExpiryFixture {
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
    assert_eq!(status.applied_versions().len(), 27);
    let inspection = PgPool::connect(&url)
        .await
        .expect("inspection connection must succeed");

    let observed_at = timestamp("2026-08-30T00:00:00.123456Z");
    let repository = DomainRepository::new(&pool);
    let user_id = UserId::new();
    repository
        .insert_user(&User::new(
            user_id,
            synveil_core::LoginIdentifier::new(format!("expiry-{label}"), user_id.to_string())
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
        name(&format!("Expiry {label}")),
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
        name("retained.bin"),
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
        Sha256Digest::from_bytes([0x47; 32]),
        4_700,
    );
    repository
        .insert_object(object, observed_at)
        .await
        .expect("fixture object must persist");
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
    .bind(format!("expiry-replica-{}", Uuid::now_v7().simple()))
    .bind(object.plaintext_length().to_string())
    .bind(object.canonical_hash().as_bytes().to_vec())
    .execute(&inspection)
    .await
    .expect("verified replica metadata must persist");
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
    repository
        .update_node(
            &file
                .with_current_version(&version, observed_at)
                .expect("fixture file accepts version"),
        )
        .await
        .expect("fixture file head must persist");

    ExpiryFixture {
        pool,
        inspection,
        user_id,
        library,
        root,
    }
}

async fn create_set(fixture: &ExpiryFixture, label: &str) -> BackupSetId {
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
    fixture: &ExpiryFixture,
    backup_set_id: BackupSetId,
    committed_at: OffsetDateTime,
) -> SnapshotId {
    let snapshot_id = SnapshotId::new();
    let snapshot_epoch = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(snapshot_epoch), 0) + 1
         FROM backup_snapshots
         WHERE backup_set_id = $1",
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
    fixture: &ExpiryFixture,
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

async fn insert_noncompleted_snapshot(
    fixture: &ExpiryFixture,
    backup_set_id: BackupSetId,
    state: &str,
) -> SnapshotId {
    assert!(matches!(state, "BUILDING" | "FAILED" | "EXPIRED"));
    let snapshot_id = SnapshotId::new();
    let snapshot_epoch = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(snapshot_epoch), 0) + 1
         FROM backup_snapshots WHERE backup_set_id = $1",
    )
    .bind(backup_set_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    let (committed_at, expired_at) = if state == "EXPIRED" {
        let committed_at = sqlx::query_scalar::<_, OffsetDateTime>(
            "SELECT clock_timestamp() - INTERVAL '200 days'",
        )
        .fetch_one(&fixture.inspection)
        .await
        .unwrap();
        (Some(committed_at), Some(OffsetDateTime::now_utc()))
    } else {
        (None, None)
    };
    sqlx::query(
        "INSERT INTO backup_snapshots
            (id, backup_set_id, owner_user_id, source_library_id, operation_id,
             snapshot_epoch, snapshot_resume_sequence, manifest_item_count,
             terminal_node_id, content_reference_count, state, created_at,
             committed_at, expired_at)
         VALUES ($1, $2, $3, $4, $5, $6, 0, 0, NULL, 0, $7,
                 clock_timestamp(), $8, $9)",
    )
    .bind(snapshot_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .bind(fixture.user_id.into_uuid())
    .bind(fixture.library.id().into_uuid())
    .bind(format!("noncompleted-{}", Uuid::now_v7().simple()))
    .bind(snapshot_epoch)
    .bind(state)
    .bind(committed_at)
    .bind(expired_at)
    .execute(&fixture.inspection)
    .await
    .expect("non-COMPLETED cohort fixture must persist");
    snapshot_id
}

async fn capture_days_old(
    fixture: &ExpiryFixture,
    backup_set_id: BackupSetId,
    days: i64,
    operation: &str,
) -> SnapshotId {
    let snapshot = BackupService::new(fixture.pool.clone())
        .capture_snapshot(
            fixture.user_id,
            backup_set_id,
            SnapshotId::new(),
            operation.to_owned(),
        )
        .await
        .expect("canonical snapshot capture must succeed");
    sqlx::query(
        "UPDATE backup_snapshots
         SET committed_at = clock_timestamp() - ($2 * INTERVAL '1 day')
         WHERE id = $1 AND state = 'COMPLETED'",
    )
    .bind(snapshot.id().into_uuid())
    .bind(days)
    .execute(&fixture.inspection)
    .await
    .expect("test snapshot age adjustment must succeed");
    snapshot.id()
}

async fn configure(
    fixture: &ExpiryFixture,
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

async fn digest(pool: &PgPool, query: &str) -> String {
    sqlx::query_scalar::<_, String>(query)
        .fetch_one(pool)
        .await
        .expect("read-only evidence digest query must succeed")
}

#[derive(Debug, Eq, PartialEq)]
struct ReadOnlyEvidence {
    snapshots: String,
    manifests: String,
    pins: String,
    live_nodes: String,
    file_versions: String,
    journal: String,
    sync: String,
    restore: String,
    restore_entries: String,
    restore_executions: String,
    prune: String,
    prune_executions: String,
    objects: String,
    replicas: String,
    gc: String,
}

async fn read_only_evidence(pool: &PgPool) -> ReadOnlyEvidence {
    ReadOnlyEvidence {
        snapshots: digest(
            pool,
            "SELECT md5(COALESCE(string_agg(
                id::TEXT || '|' || state || '|' || COALESCE(committed_at::TEXT, '') ||
                '|' || manifest_item_count::TEXT || '|' || content_reference_count::TEXT,
                ',' ORDER BY id), '')) FROM backup_snapshots",
        )
        .await,
        manifests: digest(
            pool,
            "SELECT md5(COALESCE(string_agg(
                snapshot_id::TEXT || '|' || node_id::TEXT || '|' || revision::TEXT ||
                '|' || COALESCE(current_version_id::TEXT, ''), ',' ORDER BY snapshot_id, node_id),
                '')) FROM backup_snapshot_nodes",
        )
        .await,
        pins: digest(
            pool,
            "SELECT md5(COALESCE(string_agg(
                snapshot_id::TEXT || '|' || manifest_node_id::TEXT || '|' ||
                file_version_id::TEXT || '|' || object_id::TEXT,
                ',' ORDER BY snapshot_id, manifest_node_id), ''))
             FROM backup_snapshot_content_pins",
        )
        .await,
        live_nodes: digest(
            pool,
            "SELECT md5(COALESCE(string_agg(
                id::TEXT || '|' || state || '|' || revision::TEXT || '|' ||
                COALESCE(current_version_id::TEXT, ''), ',' ORDER BY id), '')) FROM nodes",
        )
        .await,
        file_versions: digest(
            pool,
            "SELECT md5(COALESCE(string_agg(
                id::TEXT || '|' || node_id::TEXT || '|' || object_id::TEXT,
                ',' ORDER BY id), '')) FROM file_versions",
        )
        .await,
        journal: digest(
            pool,
            "SELECT md5(COALESCE(string_agg(
                entry_id::TEXT || '|' || sequence::TEXT || '|' || change_kind,
                ',' ORDER BY entry_id), '')) FROM change_journal",
        )
        .await,
        sync: digest(
            pool,
            "SELECT md5(COALESCE(string_agg(
                device_id::TEXT || '|' || library_id::TEXT || '|' ||
                acknowledged_sequence::TEXT, ',' ORDER BY device_id, library_id), ''))
             FROM device_sync_checkpoints",
        )
        .await,
        restore: digest(
            pool,
            "SELECT md5(COALESCE(string_agg(
                id::TEXT || '|' || state || '|' || snapshot_id::TEXT,
                ',' ORDER BY id), '')) FROM backup_restore_plans",
        )
        .await,
        restore_entries: digest(
            pool,
            "SELECT md5(COALESCE(string_agg(
                plan_id::TEXT || '|' || ordinal::TEXT || '|' || planned_node_id::TEXT,
                ',' ORDER BY plan_id, ordinal), '')) FROM backup_restore_plan_entries",
        )
        .await,
        restore_executions: digest(
            pool,
            "SELECT md5(COALESCE(string_agg(
                id::TEXT || '|' || state || '|' || restore_plan_id::TEXT,
                ',' ORDER BY id), '')) FROM backup_restore_executions",
        )
        .await,
        prune: digest(
            pool,
            "SELECT md5(COALESCE(string_agg(
                id::TEXT || '|' || state || '|' || snapshot_id::TEXT,
                ',' ORDER BY id), '')) FROM backup_prune_plans",
        )
        .await,
        prune_executions: digest(
            pool,
            "SELECT md5(COALESCE(string_agg(
                id::TEXT || '|' || state || '|' || prune_plan_id::TEXT,
                ',' ORDER BY id), '')) FROM backup_prune_executions",
        )
        .await,
        objects: digest(
            pool,
            "SELECT md5(COALESCE(string_agg(
                id::TEXT || '|' || lifecycle_state, ',' ORDER BY id), '')) FROM objects",
        )
        .await,
        replicas: digest(
            pool,
            "SELECT md5(COALESCE(string_agg(
                id::TEXT || '|' || state, ',' ORDER BY id), '')) FROM object_replicas",
        )
        .await,
        gc: digest(
            pool,
            "SELECT md5(
                (SELECT count(*)::TEXT FROM object_gc_candidates) || '|' ||
                (SELECT count(*)::TEXT FROM object_gc_operations) || '|' ||
                (SELECT count(*)::TEXT FROM object_gc_holds))",
        )
        .await,
    }
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_retention_policy_revisions_are_idempotent_serialized_and_immutable() {
    let fixture = fixture("policy").await;
    let backup = BackupService::new(fixture.pool.clone());
    let set_id = create_set(&fixture, "policy-set").await;

    assert_eq!(
        backup
            .create_snapshot_expiry_plan(fixture.user_id, "no-policy-plan".to_owned(), set_id,)
            .await,
        Err(BackupError::ExpiryPreflight(
            BackupSnapshotExpiryPreflightIssue::RetentionPolicyNotConfigured,
        ))
    );
    assert_eq!(
        backup
            .configure_snapshot_retention_policy(
                fixture.user_id,
                "invalid-keep-zero".to_owned(),
                set_id,
                0,
                60,
            )
            .await,
        Err(BackupError::InvalidRequest)
    );
    assert_eq!(
        backup
            .configure_snapshot_retention_policy(
                fixture.user_id,
                "invalid-age-zero".to_owned(),
                set_id,
                1,
                0,
            )
            .await,
        Err(BackupError::InvalidRequest)
    );
    assert_eq!(
        backup
            .configure_snapshot_retention_policy(
                fixture.user_id,
                "invalid-age-overflow".to_owned(),
                set_id,
                1,
                (i64::MAX as u64) + 1,
            )
            .await,
        Err(BackupError::InvalidRequest)
    );

    let product_before = read_only_evidence(&fixture.inspection).await;
    let first = backup
        .configure_snapshot_retention_policy(
            fixture.user_id,
            "policy-operation-one".to_owned(),
            set_id,
            2,
            2_592_000,
        )
        .await
        .expect("first policy revision must persist");
    assert_eq!(first.revision_number().get(), 1);
    assert_eq!(
        product_before,
        read_only_evidence(&fixture.inspection).await
    );

    let replay = backup
        .configure_snapshot_retention_policy(
            fixture.user_id,
            "policy-operation-one".to_owned(),
            set_id,
            2,
            2_592_000,
        )
        .await
        .expect("same operation must replay");
    assert_eq!(replay, first);
    assert_eq!(
        backup
            .configure_snapshot_retention_policy(
                fixture.user_id,
                "policy-operation-one".to_owned(),
                set_id,
                3,
                2_592_000,
            )
            .await,
        Err(BackupError::RetentionPolicyConflict)
    );
    let different_set = create_set(&fixture, "policy-different-set").await;
    assert_eq!(
        backup
            .configure_snapshot_retention_policy(
                fixture.user_id,
                "policy-operation-one".to_owned(),
                different_set,
                2,
                2_592_000,
            )
            .await,
        Err(BackupError::RetentionPolicyConflict)
    );
    assert_eq!(
        backup
            .configure_snapshot_retention_policy(
                UserId::new(),
                "foreign-policy-set".to_owned(),
                set_id,
                2,
                2_592_000,
            )
            .await,
        Err(BackupError::NotFound)
    );

    let second = backup
        .configure_snapshot_retention_policy(
            fixture.user_id,
            "policy-operation-two".to_owned(),
            set_id,
            3,
            604_800,
        )
        .await
        .expect("second policy revision must persist");
    assert_eq!(second.revision_number().get(), 2);
    let historical: (i64, i64, i64) = sqlx::query_as(
        "SELECT revision_number, keep_latest_completed, expire_after_seconds
         FROM backup_snapshot_retention_policy_revisions WHERE id = $1",
    )
    .bind(first.id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("historical policy must remain readable");
    assert_eq!(historical, (1, 2, 2_592_000));

    let service_a = backup.clone();
    let service_b = backup.clone();
    let (r3, r4) = tokio::join!(
        service_a.configure_snapshot_retention_policy(
            fixture.user_id,
            "policy-race-three".to_owned(),
            set_id,
            4,
            400,
        ),
        service_b.configure_snapshot_retention_policy(
            fixture.user_id,
            "policy-race-four".to_owned(),
            set_id,
            5,
            500,
        ),
    );
    let mut raced = [r3.unwrap(), r4.unwrap()];
    raced.sort_by_key(|revision| revision.revision_number());
    assert_eq!(raced[0].revision_number().get(), 3);
    assert_eq!(raced[1].revision_number().get(), 4);
    assert_eq!(
        backup
            .get_current_snapshot_retention_policy(fixture.user_id, set_id)
            .await
            .unwrap()
            .revision_number()
            .get(),
        4
    );

    let service_a = backup.clone();
    let service_b = backup.clone();
    let (same_a, same_b) = tokio::join!(
        service_a.configure_snapshot_retention_policy(
            fixture.user_id,
            "policy-same-race".to_owned(),
            set_id,
            6,
            600,
        ),
        service_b.configure_snapshot_retention_policy(
            fixture.user_id,
            "policy-same-race".to_owned(),
            set_id,
            6,
            600,
        ),
    );
    assert_eq!(same_a.unwrap().id(), same_b.unwrap().id());
    let semantic_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_retention_policy_revisions
         WHERE owner_user_id = $1 AND operation_id = 'policy-same-race'",
    )
    .bind(fixture.user_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(semantic_count, 1);

    let foreign = UserId::new();
    assert_eq!(
        backup
            .get_current_snapshot_retention_policy(foreign, set_id)
            .await,
        Err(BackupError::NotFound)
    );
    assert!(
        sqlx::query(
            "UPDATE backup_snapshot_retention_policy_revisions
             SET keep_latest_completed = 99 WHERE id = $1",
        )
        .bind(first.id().into_uuid())
        .execute(&fixture.inspection)
        .await
        .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM backup_snapshot_retention_policy_revisions WHERE id = $1")
            .bind(first.id().into_uuid())
            .execute(&fixture.inspection)
            .await
            .is_err()
    );

    // A failed revision insert rolls back without a phantom current revision;
    // after removing the injected failure, the same request succeeds cleanly.
    sqlx::query(
        "CREATE OR REPLACE FUNCTION p47_fail_policy_insert()
         RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             IF NEW.operation_id = 'policy-injected-failure' THEN
                 RAISE EXCEPTION 'injected policy insertion failure';
             END IF;
             RETURN NEW;
         END;
         $$",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER p47_fail_policy_insert_trigger
         BEFORE INSERT ON backup_snapshot_retention_policy_revisions
         FOR EACH ROW EXECUTE FUNCTION p47_fail_policy_insert()",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    let revision_count_before: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_retention_policy_revisions
         WHERE backup_set_id = $1",
    )
    .bind(set_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert!(
        backup
            .configure_snapshot_retention_policy(
                fixture.user_id,
                "policy-injected-failure".to_owned(),
                set_id,
                8,
                800,
            )
            .await
            .is_err()
    );
    let revision_count_after: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_retention_policy_revisions
         WHERE backup_set_id = $1",
    )
    .bind(set_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(revision_count_after, revision_count_before);
    sqlx::query(
        "DROP TRIGGER p47_fail_policy_insert_trigger
         ON backup_snapshot_retention_policy_revisions",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    sqlx::query("DROP FUNCTION p47_fail_policy_insert()")
        .execute(&fixture.inspection)
        .await
        .unwrap();
    backup
        .configure_snapshot_retention_policy(
            fixture.user_id,
            "policy-injected-failure".to_owned(),
            set_id,
            8,
            800,
        )
        .await
        .expect("clean retry after rollback must succeed");

    // Disabled sets remain eligible for historical lifecycle management.
    sqlx::query(
        "UPDATE backup_sets SET state = 'DISABLED', updated_at = clock_timestamp(),
                revision = revision + 1
         WHERE id = $1",
    )
    .bind(set_id.into_uuid())
    .execute(&fixture.inspection)
    .await
    .unwrap();
    backup
        .configure_snapshot_retention_policy(
            fixture.user_id,
            "policy-disabled-set".to_owned(),
            set_id,
            7,
            700,
        )
        .await
        .expect("disabled set accepts a new historical retention revision");

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_expiry_planning_is_deterministic_idempotent_and_read_only() {
    let fixture = fixture("decisions").await;
    let backup = BackupService::new(fixture.pool.clone());
    let set_id = create_set(&fixture, "decision-set").await;
    let oldest = capture_days_old(&fixture, set_id, 80, "decision-capture-oldest").await;
    let old = insert_completed_snapshot_days_old(&fixture, set_id, 40).await;
    let recent = insert_completed_snapshot_days_old(&fixture, set_id, 20).await;
    let latest_two = insert_completed_snapshot_days_old(&fixture, set_id, 10).await;
    let latest_one = insert_completed_snapshot_days_old(&fixture, set_id, 1).await;
    insert_noncompleted_snapshot(&fixture, set_id, "BUILDING").await;
    insert_noncompleted_snapshot(&fixture, set_id, "FAILED").await;
    insert_noncompleted_snapshot(&fixture, set_id, "EXPIRED").await;

    let restore = backup
        .create_restore_plan(
            fixture.user_id,
            "decision-restore-blocker".to_owned(),
            set_id,
            oldest,
            fixture.library.id(),
            fixture.root.id(),
            name("Blocked recovery"),
        )
        .await
        .expect("active restore intent must persist");
    assert_eq!(
        restore.state(),
        synveil_core::BackupRestorePlanState::Planned
    );
    configure(&fixture, set_id, "decision-policy", 2, 30 * 86_400).await;

    let before = read_only_evidence(&fixture.inspection).await;
    let plan = backup
        .create_snapshot_expiry_plan(fixture.user_id, "decision-expiry-plan".to_owned(), set_id)
        .await
        .expect("expiry plan must persist");
    assert_eq!(plan.state(), BackupSnapshotExpiryPlanState::Planned);
    assert_eq!(plan.evaluated_completed_snapshot_count(), 5);
    assert_eq!(plan.keep_latest_count(), 2);
    assert_eq!(plan.keep_recent_count(), 1);
    assert_eq!(plan.expire_candidate_count(), 1);
    assert_eq!(plan.blocked_active_restore_count(), 1);
    assert_eq!(before, read_only_evidence(&fixture.inspection).await);

    let (entries, has_more) = backup
        .list_snapshot_expiry_plan_entries(fixture.user_id, plan.id(), None, 100)
        .await
        .expect("expiry entries must list");
    assert!(!has_more);
    assert_eq!(entries.len(), 5);
    let decisions: BTreeMap<_, _> = entries
        .iter()
        .map(|entry| (entry.snapshot_id(), entry.decision()))
        .collect();
    assert_eq!(
        decisions[&latest_one],
        BackupSnapshotExpiryDecision::KeepLatest
    );
    assert_eq!(
        decisions[&latest_two],
        BackupSnapshotExpiryDecision::KeepLatest
    );
    assert_eq!(decisions[&recent], BackupSnapshotExpiryDecision::KeepRecent);
    assert_eq!(decisions[&old], BackupSnapshotExpiryDecision::Expire);
    assert_eq!(
        decisions[&oldest],
        BackupSnapshotExpiryDecision::BlockedActiveRestorePlan
    );
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.recency_rank())
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );

    let replay = backup
        .create_snapshot_expiry_plan(fixture.user_id, "decision-expiry-plan".to_owned(), set_id)
        .await
        .expect("same operation must replay canonical plan");
    assert_eq!(replay, plan);
    assert_eq!(replay.evaluated_at(), plan.evaluated_at());
    assert_eq!(
        backup
            .list_snapshot_expiry_plan_entries(fixture.user_id, replay.id(), None, 100)
            .await
            .unwrap()
            .0,
        entries
    );
    assert_eq!(
        backup
            .create_snapshot_expiry_plan(
                fixture.user_id,
                "decision-competing-plan".to_owned(),
                set_id,
            )
            .await,
        Err(BackupError::ExpiryPreflight(
            BackupSnapshotExpiryPreflightIssue::ExpiryAlreadyPlanned,
        ))
    );
    let replay_conflict_set = create_set(&fixture, "expiry-replay-conflict-set").await;
    configure(
        &fixture,
        replay_conflict_set,
        "expiry-replay-conflict-policy",
        1,
        60,
    )
    .await;
    assert_eq!(
        backup
            .create_snapshot_expiry_plan(
                fixture.user_id,
                "decision-expiry-plan".to_owned(),
                replay_conflict_set,
            )
            .await,
        Err(BackupError::ExpiryPlanConflict)
    );
    assert_eq!(
        backup
            .get_snapshot_expiry_plan(UserId::new(), plan.id())
            .await,
        Err(BackupError::NotFound)
    );
    let (first_page, more) = backup
        .list_snapshot_expiry_plan_entries(fixture.user_id, plan.id(), None, 2)
        .await
        .unwrap();
    assert!(more);
    let (second_page, _) = backup
        .list_snapshot_expiry_plan_entries(
            fixture.user_id,
            plan.id(),
            Some(first_page.last().unwrap().recency_rank()),
            2,
        )
        .await
        .unwrap();
    assert_eq!(second_page[0].recency_rank(), 3);

    // Equal committed_at values use snapshot identity DESC, never insertion
    // or PostgreSQL query-plan order.
    let tie_set = create_set(&fixture, "equal-timestamp-set").await;
    let tied_at =
        sqlx::query_scalar::<_, OffsetDateTime>("SELECT clock_timestamp() - INTERVAL '90 days'")
            .fetch_one(&fixture.inspection)
            .await
            .unwrap();
    let tied_a = insert_completed_snapshot_at(&fixture, tie_set, tied_at).await;
    let tied_b = insert_completed_snapshot_at(&fixture, tie_set, tied_at).await;
    let tied_c = insert_completed_snapshot_at(&fixture, tie_set, tied_at).await;
    configure(&fixture, tie_set, "tie-policy", 1, 30 * 86_400).await;
    let tie_plan = backup
        .create_snapshot_expiry_plan(fixture.user_id, "tie-expiry-plan".to_owned(), tie_set)
        .await
        .unwrap();
    let tied_entries = backup
        .list_snapshot_expiry_plan_entries(fixture.user_id, tie_plan.id(), None, 10)
        .await
        .unwrap()
        .0;
    let mut expected_ids = vec![tied_a, tied_b, tied_c];
    expected_ids.sort_by(|left, right| right.cmp(left));
    assert_eq!(
        tied_entries
            .iter()
            .map(|entry| entry.snapshot_id())
            .collect::<Vec<_>>(),
        expected_ids
    );
    assert_eq!(
        tied_entries[0].decision(),
        BackupSnapshotExpiryDecision::KeepLatest
    );
    assert!(
        tied_entries[1..]
            .iter()
            .all(|entry| entry.decision() == BackupSnapshotExpiryDecision::Expire)
    );

    // An all-old cohort still protects the newest N.
    let floor_set = create_set(&fixture, "minimum-floor-set").await;
    for age in [100, 110, 120, 130, 140] {
        insert_completed_snapshot_days_old(&fixture, floor_set, age).await;
    }
    configure(&fixture, floor_set, "floor-policy", 3, 86_400).await;
    let floor_plan = backup
        .create_snapshot_expiry_plan(fixture.user_id, "floor-expiry-plan".to_owned(), floor_set)
        .await
        .unwrap();
    assert_eq!(floor_plan.keep_latest_count(), 3);
    assert_eq!(floor_plan.expire_candidate_count(), 2);

    // Fewer snapshots than the keep floor protects every snapshot.
    let small_set = create_set(&fixture, "small-cohort-set").await;
    for age in [100, 110, 120, 130] {
        insert_completed_snapshot_days_old(&fixture, small_set, age).await;
    }
    configure(&fixture, small_set, "small-policy", 10, 1).await;
    let small_plan = backup
        .create_snapshot_expiry_plan(fixture.user_id, "small-expiry-plan".to_owned(), small_set)
        .await
        .unwrap();
    assert_eq!(small_plan.keep_latest_count(), 4);
    assert_eq!(small_plan.expire_candidate_count(), 0);

    // A disabled set with no COMPLETED snapshots produces a valid empty plan.
    let empty_set = create_set(&fixture, "empty-disabled-set").await;
    sqlx::query(
        "UPDATE backup_sets SET state = 'DISABLED', updated_at = clock_timestamp(),
                revision = revision + 1 WHERE id = $1",
    )
    .bind(empty_set.into_uuid())
    .execute(&fixture.inspection)
    .await
    .unwrap();
    configure(&fixture, empty_set, "empty-policy", 2, 60).await;
    let empty_plan = backup
        .create_snapshot_expiry_plan(fixture.user_id, "empty-expiry-plan".to_owned(), empty_set)
        .await
        .unwrap();
    assert_eq!(empty_plan.evaluated_completed_snapshot_count(), 0);
    assert_eq!(empty_plan.keep_latest_count(), 0);
    assert_eq!(empty_plan.keep_recent_count(), 0);
    assert_eq!(empty_plan.blocked_active_restore_count(), 0);
    assert_eq!(empty_plan.expire_candidate_count(), 0);
    assert!(
        backup
            .list_snapshot_expiry_plan_entries(fixture.user_id, empty_plan.id(), None, 10,)
            .await
            .unwrap()
            .0
            .is_empty()
    );

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_expiry_validation_detects_policy_cohort_and_restore_drift_only() {
    let fixture = fixture("staleness").await;
    let backup = BackupService::new(fixture.pool.clone());

    // No change, including wall-clock passage, leaves the original evaluated
    // basis valid. A later policy revision stales it without rewriting any
    // immutable plan field or entry.
    let policy_set = create_set(&fixture, "stale-policy-set").await;
    insert_completed_snapshot_days_old(&fixture, policy_set, 100).await;
    insert_completed_snapshot_days_old(&fixture, policy_set, 1).await;
    configure(&fixture, policy_set, "stale-policy-r1", 1, 30 * 86_400).await;
    let policy_plan = backup
        .create_snapshot_expiry_plan(fixture.user_id, "stale-policy-plan".to_owned(), policy_set)
        .await
        .unwrap();
    let original_entries = backup
        .list_snapshot_expiry_plan_entries(fixture.user_id, policy_plan.id(), None, 100)
        .await
        .unwrap()
        .0;
    assert_eq!(
        backup
            .validate_snapshot_expiry_plan(fixture.user_id, policy_plan.id())
            .await
            .unwrap()
            .state(),
        BackupSnapshotExpiryPlanState::Planned
    );
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    assert_eq!(
        backup
            .validate_snapshot_expiry_plan(fixture.user_id, policy_plan.id())
            .await
            .unwrap()
            .state(),
        BackupSnapshotExpiryPlanState::Planned
    );
    configure(&fixture, policy_set, "stale-policy-r2", 2, 60 * 86_400).await;
    let policy_stale = backup
        .validate_snapshot_expiry_plan(fixture.user_id, policy_plan.id())
        .await
        .unwrap();
    assert_eq!(policy_stale.state(), BackupSnapshotExpiryPlanState::Stale);
    assert_eq!(
        policy_stale.policy_revision_id(),
        policy_plan.policy_revision_id()
    );
    assert_eq!(
        policy_stale.policy_revision_number(),
        policy_plan.policy_revision_number()
    );
    assert_eq!(policy_stale.evaluated_at(), policy_plan.evaluated_at());
    assert_eq!(
        policy_stale.snapshot_basis_fingerprint(),
        policy_plan.snapshot_basis_fingerprint()
    );
    assert_eq!(
        backup
            .list_snapshot_expiry_plan_entries(fixture.user_id, policy_plan.id(), None, 100)
            .await
            .unwrap()
            .0,
        original_entries
    );

    // A new COMPLETED snapshot changes ranks/cohort and stales the plan.
    let new_set = create_set(&fixture, "stale-new-snapshot-set").await;
    insert_completed_snapshot_days_old(&fixture, new_set, 100).await;
    configure(&fixture, new_set, "stale-new-policy", 1, 30 * 86_400).await;
    let new_plan = backup
        .create_snapshot_expiry_plan(fixture.user_id, "stale-new-plan".to_owned(), new_set)
        .await
        .unwrap();
    insert_completed_snapshot_days_old(&fixture, new_set, 1).await;
    assert_eq!(
        backup
            .validate_snapshot_expiry_plan(fixture.user_id, new_plan.id())
            .await
            .unwrap()
            .state(),
        BackupSnapshotExpiryPlanState::Stale
    );

    // A represented snapshot leaving COMPLETED also changes the cohort.
    let lifecycle_set = create_set(&fixture, "stale-lifecycle-set").await;
    let lifecycle_snapshot = insert_completed_snapshot_days_old(&fixture, lifecycle_set, 100).await;
    configure(
        &fixture,
        lifecycle_set,
        "stale-lifecycle-policy",
        1,
        30 * 86_400,
    )
    .await;
    let lifecycle_plan = backup
        .create_snapshot_expiry_plan(
            fixture.user_id,
            "stale-lifecycle-plan".to_owned(),
            lifecycle_set,
        )
        .await
        .unwrap();
    sqlx::query(
        "UPDATE backup_snapshots
         SET state = 'EXPIRED', expired_at = clock_timestamp()
         WHERE id = $1 AND state = 'COMPLETED'",
    )
    .bind(lifecycle_snapshot.into_uuid())
    .execute(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(
        backup
            .validate_snapshot_expiry_plan(fixture.user_id, lifecycle_plan.id())
            .await
            .unwrap()
            .state(),
        BackupSnapshotExpiryPlanState::Stale
    );

    // A new active restore intent for an EXPIRE candidate changes its blocker
    // bit and stales the old decision rather than mutating it in place.
    let appears_set = create_set(&fixture, "stale-blocker-appears-set").await;
    let appears_old = capture_days_old(&fixture, appears_set, 100, "appears-source-capture").await;
    insert_completed_snapshot_days_old(&fixture, appears_set, 1).await;
    configure(&fixture, appears_set, "appears-policy", 1, 30 * 86_400).await;
    let appears_plan = backup
        .create_snapshot_expiry_plan(
            fixture.user_id,
            "appears-expiry-plan".to_owned(),
            appears_set,
        )
        .await
        .unwrap();
    let appears_before = backup
        .list_snapshot_expiry_plan_entries(fixture.user_id, appears_plan.id(), None, 10)
        .await
        .unwrap()
        .0;
    assert_eq!(
        appears_before
            .iter()
            .find(|entry| entry.snapshot_id() == appears_old)
            .unwrap()
            .decision(),
        BackupSnapshotExpiryDecision::Expire
    );
    backup
        .create_restore_plan(
            fixture.user_id,
            "appears-restore-plan".to_owned(),
            appears_set,
            appears_old,
            fixture.library.id(),
            fixture.root.id(),
            name("Appearing blocker"),
        )
        .await
        .unwrap();
    assert_eq!(
        backup
            .validate_snapshot_expiry_plan(fixture.user_id, appears_plan.id())
            .await
            .unwrap()
            .state(),
        BackupSnapshotExpiryPlanState::Stale
    );
    assert_eq!(
        backup
            .list_snapshot_expiry_plan_entries(fixture.user_id, appears_plan.id(), None, 10)
            .await
            .unwrap()
            .0,
        appears_before
    );

    // A blocker disappearing likewise stales the old BLOCKED entry. After an
    // explicit replan, a STALE restore plan no longer blocks expiry.
    let disappears_set = create_set(&fixture, "stale-blocker-disappears-set").await;
    let disappears_old =
        capture_days_old(&fixture, disappears_set, 100, "disappears-source-capture").await;
    insert_completed_snapshot_days_old(&fixture, disappears_set, 1).await;
    let blocker = backup
        .create_restore_plan(
            fixture.user_id,
            "disappears-restore-plan".to_owned(),
            disappears_set,
            disappears_old,
            fixture.library.id(),
            fixture.root.id(),
            name("Disappearing blocker"),
        )
        .await
        .unwrap();
    configure(
        &fixture,
        disappears_set,
        "disappears-policy",
        1,
        30 * 86_400,
    )
    .await;
    let disappears_plan = backup
        .create_snapshot_expiry_plan(
            fixture.user_id,
            "disappears-expiry-plan".to_owned(),
            disappears_set,
        )
        .await
        .unwrap();
    let blocked_entry = backup
        .list_snapshot_expiry_plan_entries(fixture.user_id, disappears_plan.id(), None, 10)
        .await
        .unwrap()
        .0
        .into_iter()
        .find(|entry| entry.snapshot_id() == disappears_old)
        .unwrap();
    assert_eq!(
        blocked_entry.decision(),
        BackupSnapshotExpiryDecision::BlockedActiveRestorePlan
    );
    sqlx::query(
        "UPDATE backup_restore_plans
         SET state = 'STALE', stale_at = clock_timestamp()
         WHERE id = $1 AND state = 'PLANNED'",
    )
    .bind(blocker.id().into_uuid())
    .execute(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(
        backup
            .validate_snapshot_expiry_plan(fixture.user_id, disappears_plan.id())
            .await
            .unwrap()
            .state(),
        BackupSnapshotExpiryPlanState::Stale
    );
    let replanned = backup
        .create_snapshot_expiry_plan(
            fixture.user_id,
            "disappears-replan".to_owned(),
            disappears_set,
        )
        .await
        .unwrap();
    assert_eq!(
        backup
            .list_snapshot_expiry_plan_entries(fixture.user_id, replanned.id(), None, 10)
            .await
            .unwrap()
            .0
            .into_iter()
            .find(|entry| entry.snapshot_id() == disappears_old)
            .unwrap()
            .decision(),
        BackupSnapshotExpiryDecision::Expire
    );

    // An EXECUTED restore plan is history, not an active blocker.
    let executed_set = create_set(&fixture, "executed-restore-set").await;
    let executed_old =
        capture_days_old(&fixture, executed_set, 100, "executed-source-capture").await;
    insert_completed_snapshot_days_old(&fixture, executed_set, 1).await;
    let executed_restore = backup
        .create_restore_plan(
            fixture.user_id,
            "executed-restore-plan".to_owned(),
            executed_set,
            executed_old,
            fixture.library.id(),
            fixture.root.id(),
            name("Executed recovery"),
        )
        .await
        .unwrap();
    backup
        .execute_restore_plan(fixture.user_id, executed_restore.id())
        .await
        .expect("restore plan must execute before retention planning");
    configure(&fixture, executed_set, "executed-policy", 1, 30 * 86_400).await;
    let executed_expiry = backup
        .create_snapshot_expiry_plan(
            fixture.user_id,
            "executed-expiry-plan".to_owned(),
            executed_set,
        )
        .await
        .unwrap();
    assert_eq!(
        backup
            .list_snapshot_expiry_plan_entries(fixture.user_id, executed_expiry.id(), None, 10,)
            .await
            .unwrap()
            .0
            .into_iter()
            .find(|entry| entry.snapshot_id() == executed_old)
            .unwrap()
            .decision(),
        BackupSnapshotExpiryDecision::Expire
    );

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_expiry_plan_rollback_immutability_and_concurrency_are_atomic() {
    let fixture = fixture("atomicity").await;
    let backup = BackupService::new(fixture.pool.clone());
    sqlx::query(
        "DROP TRIGGER IF EXISTS p47_fail_first_expiry_entry_trigger
         ON backup_snapshot_expiry_plan_entries",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    sqlx::query("DROP FUNCTION IF EXISTS p47_fail_first_expiry_entry()")
        .execute(&fixture.inspection)
        .await
        .unwrap();
    sqlx::query(
        "DROP TRIGGER IF EXISTS p47_fail_second_expiry_entry_trigger
         ON backup_snapshot_expiry_plan_entries",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    sqlx::query("DROP FUNCTION IF EXISTS p47_fail_second_expiry_entry()")
        .execute(&fixture.inspection)
        .await
        .unwrap();

    // Failure after the plan row exists but before the first entry commits.
    let row_failure_operation = format!("row-failure-{}", Uuid::now_v7().simple());
    let row_failure_set = create_set(&fixture, "row-failure-set").await;
    insert_completed_snapshot_days_old(&fixture, row_failure_set, 100).await;
    configure(&fixture, row_failure_set, "row-failure-policy", 1, 86_400).await;
    sqlx::query(
        "CREATE OR REPLACE FUNCTION p47_fail_first_expiry_entry()
         RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             IF EXISTS (
                 SELECT 1 FROM backup_snapshot_expiry_plans
                 WHERE id = NEW.plan_id AND operation_id LIKE 'row-failure-%'
             ) THEN
                 RAISE EXCEPTION 'injected first-entry failure';
             END IF;
             RETURN NEW;
         END;
         $$",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER p47_fail_first_expiry_entry_trigger
         BEFORE INSERT ON backup_snapshot_expiry_plan_entries
         FOR EACH ROW EXECUTE FUNCTION p47_fail_first_expiry_entry()",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    let row_failure_evidence = read_only_evidence(&fixture.inspection).await;
    assert!(
        backup
            .create_snapshot_expiry_plan(
                fixture.user_id,
                row_failure_operation.clone(),
                row_failure_set,
            )
            .await
            .is_err()
    );
    let partial: (i64, i64) = sqlx::query_as(
        "SELECT
            (SELECT count(*) FROM backup_snapshot_expiry_plans
             WHERE operation_id = $1),
            (SELECT count(*) FROM backup_snapshot_expiry_plan_entries AS entry
             INNER JOIN backup_snapshot_expiry_plans AS plan ON plan.id = entry.plan_id
             WHERE plan.operation_id = $1)",
    )
    .bind(&row_failure_operation)
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(partial, (0, 0));
    assert_eq!(
        row_failure_evidence,
        read_only_evidence(&fixture.inspection).await
    );
    sqlx::query(
        "DROP TRIGGER p47_fail_first_expiry_entry_trigger
         ON backup_snapshot_expiry_plan_entries",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    sqlx::query("DROP FUNCTION p47_fail_first_expiry_entry()")
        .execute(&fixture.inspection)
        .await
        .unwrap();
    let row_failure_retry = backup
        .create_snapshot_expiry_plan(fixture.user_id, row_failure_operation, row_failure_set)
        .await
        .expect("clean retry after first-entry rollback must succeed");

    // Failure after one of several entries has been inserted also rolls the
    // entire transaction back.
    let entry_failure_operation = format!("entry-failure-{}", Uuid::now_v7().simple());
    let entry_failure_set = create_set(&fixture, "entry-failure-set").await;
    insert_completed_snapshot_days_old(&fixture, entry_failure_set, 100).await;
    insert_completed_snapshot_days_old(&fixture, entry_failure_set, 90).await;
    insert_completed_snapshot_days_old(&fixture, entry_failure_set, 80).await;
    configure(
        &fixture,
        entry_failure_set,
        "entry-failure-policy",
        1,
        86_400,
    )
    .await;
    sqlx::query(
        "CREATE OR REPLACE FUNCTION p47_fail_second_expiry_entry()
         RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             IF NEW.recency_rank = 2 AND EXISTS (
                 SELECT 1 FROM backup_snapshot_expiry_plans
                 WHERE id = NEW.plan_id AND operation_id LIKE 'entry-failure-%'
             ) THEN
                 RAISE EXCEPTION 'injected partial-entry failure';
             END IF;
             RETURN NEW;
         END;
         $$",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER p47_fail_second_expiry_entry_trigger
         BEFORE INSERT ON backup_snapshot_expiry_plan_entries
         FOR EACH ROW EXECUTE FUNCTION p47_fail_second_expiry_entry()",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    let entry_failure_evidence = read_only_evidence(&fixture.inspection).await;
    assert!(
        backup
            .create_snapshot_expiry_plan(
                fixture.user_id,
                entry_failure_operation.clone(),
                entry_failure_set,
            )
            .await
            .is_err()
    );
    let partial: (i64, i64) = sqlx::query_as(
        "SELECT
            (SELECT count(*) FROM backup_snapshot_expiry_plans
             WHERE operation_id = $1),
            (SELECT count(*) FROM backup_snapshot_expiry_plan_entries AS entry
             INNER JOIN backup_snapshot_expiry_plans AS plan ON plan.id = entry.plan_id
             WHERE plan.operation_id = $1)",
    )
    .bind(&entry_failure_operation)
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(partial, (0, 0));
    assert_eq!(
        entry_failure_evidence,
        read_only_evidence(&fixture.inspection).await
    );
    sqlx::query(
        "DROP TRIGGER p47_fail_second_expiry_entry_trigger
         ON backup_snapshot_expiry_plan_entries",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    sqlx::query("DROP FUNCTION p47_fail_second_expiry_entry()")
        .execute(&fixture.inspection)
        .await
        .unwrap();
    backup
        .create_snapshot_expiry_plan(fixture.user_id, entry_failure_operation, entry_failure_set)
        .await
        .expect("clean retry after partial-entry rollback must succeed");

    // Sealed plan provenance and decisions reject arbitrary mutation.
    assert!(
        sqlx::query(
            "UPDATE backup_snapshot_expiry_plans
             SET evaluated_at = evaluated_at + INTERVAL '1 second' WHERE id = $1",
        )
        .bind(row_failure_retry.id().into_uuid())
        .execute(&fixture.inspection)
        .await
        .is_err()
    );
    assert!(
        sqlx::query(
            "UPDATE backup_snapshot_expiry_plans
             SET expire_candidate_count = expire_candidate_count + 1 WHERE id = $1",
        )
        .bind(row_failure_retry.id().into_uuid())
        .execute(&fixture.inspection)
        .await
        .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM backup_snapshot_expiry_plans WHERE id = $1")
            .bind(row_failure_retry.id().into_uuid())
            .execute(&fixture.inspection)
            .await
            .is_err()
    );
    assert!(
        sqlx::query(
            "UPDATE backup_snapshot_expiry_plan_entries
             SET decision = 'EXPIRE' WHERE plan_id = $1",
        )
        .bind(row_failure_retry.id().into_uuid())
        .execute(&fixture.inspection)
        .await
        .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM backup_snapshot_expiry_plan_entries WHERE plan_id = $1")
            .bind(row_failure_retry.id().into_uuid())
            .execute(&fixture.inspection)
            .await
            .is_err()
    );
    let existing_entry = backup
        .list_snapshot_expiry_plan_entries(fixture.user_id, row_failure_retry.id(), None, 10)
        .await
        .unwrap()
        .0[0];
    assert!(
        sqlx::query(
            "INSERT INTO backup_snapshot_expiry_plan_entries
                (plan_id, snapshot_id, committed_at, recency_rank, decision)
             VALUES ($1, $2, $3, 99, 'EXPIRE')",
        )
        .bind(row_failure_retry.id().into_uuid())
        .bind(existing_entry.snapshot_id().into_uuid())
        .bind(existing_entry.committed_at().as_offset_datetime())
        .execute(&fixture.inspection)
        .await
        .is_err()
    );

    // Same-operation planning races converge on one canonical plan.
    let same_set = create_set(&fixture, "same-plan-race-set").await;
    configure(&fixture, same_set, "same-plan-policy", 1, 60).await;
    let service_a = backup.clone();
    let service_b = backup.clone();
    let (same_a, same_b) = tokio::join!(
        service_a.create_snapshot_expiry_plan(
            fixture.user_id,
            "same-plan-race".to_owned(),
            same_set,
        ),
        service_b.create_snapshot_expiry_plan(
            fixture.user_id,
            "same-plan-race".to_owned(),
            same_set,
        ),
    );
    let same_a = same_a.unwrap();
    let same_b = same_b.unwrap();
    assert_eq!(same_a, same_b);
    assert_eq!(same_a.evaluated_at(), same_b.evaluated_at());

    // Distinct operation identities serialize to exactly one active plan and
    // a typed product error for the loser.
    let distinct_set = create_set(&fixture, "distinct-plan-race-set").await;
    configure(&fixture, distinct_set, "distinct-plan-policy", 1, 60).await;
    let service_a = backup.clone();
    let service_b = backup.clone();
    let (distinct_a, distinct_b) = tokio::join!(
        service_a.create_snapshot_expiry_plan(
            fixture.user_id,
            "distinct-plan-race-a".to_owned(),
            distinct_set,
        ),
        service_b.create_snapshot_expiry_plan(
            fixture.user_id,
            "distinct-plan-race-b".to_owned(),
            distinct_set,
        ),
    );
    assert_eq!(
        usize::from(distinct_a.is_ok()) + usize::from(distinct_b.is_ok()),
        1
    );
    let loser = distinct_a.err().or_else(|| distinct_b.err()).unwrap();
    assert_eq!(
        loser,
        BackupError::ExpiryPreflight(BackupSnapshotExpiryPreflightIssue::ExpiryAlreadyPlanned)
    );

    // Snapshot completion and planning share the backup-set fence. Either the
    // capture is in the original cohort or validation observes it as drift.
    let capture_race_set = create_set(&fixture, "capture-race-set").await;
    configure(&fixture, capture_race_set, "capture-race-policy", 1, 60).await;
    let plan_service = backup.clone();
    let capture_service = backup.clone();
    let (raced_plan, raced_snapshot) = tokio::join!(
        plan_service.create_snapshot_expiry_plan(
            fixture.user_id,
            "capture-race-plan".to_owned(),
            capture_race_set,
        ),
        capture_service.capture_snapshot(
            fixture.user_id,
            capture_race_set,
            SnapshotId::new(),
            "capture-race-snapshot".to_owned(),
        ),
    );
    let raced_plan = raced_plan.unwrap();
    raced_snapshot.unwrap();
    let raced_entries = backup
        .list_snapshot_expiry_plan_entries(fixture.user_id, raced_plan.id(), None, 10)
        .await
        .unwrap()
        .0;
    let validated = backup
        .validate_snapshot_expiry_plan(fixture.user_id, raced_plan.id())
        .await
        .unwrap();
    if raced_entries.is_empty() {
        assert_eq!(validated.state(), BackupSnapshotExpiryPlanState::Stale);
    } else {
        assert_eq!(raced_entries.len(), 1);
        assert_eq!(validated.state(), BackupSnapshotExpiryPlanState::Planned);
    }

    // Restore-plan insertion uses the set FK key lock. If it commits first the
    // decision is BLOCKED; if the expiry observation commits first, the new
    // blocker makes validation STALE.
    let restore_race_set = create_set(&fixture, "restore-race-set").await;
    let restore_race_old = capture_days_old(
        &fixture,
        restore_race_set,
        100,
        "restore-race-source-capture",
    )
    .await;
    insert_completed_snapshot_days_old(&fixture, restore_race_set, 1).await;
    configure(
        &fixture,
        restore_race_set,
        "restore-race-policy",
        1,
        30 * 86_400,
    )
    .await;
    let expiry_service = backup.clone();
    let restore_service = backup.clone();
    let (restore_raced_expiry, restore_raced_restore) = tokio::join!(
        expiry_service.create_snapshot_expiry_plan(
            fixture.user_id,
            "restore-race-expiry".to_owned(),
            restore_race_set,
        ),
        restore_service.create_restore_plan(
            fixture.user_id,
            "restore-race-restore".to_owned(),
            restore_race_set,
            restore_race_old,
            fixture.library.id(),
            fixture.root.id(),
            name("Restore race target"),
        ),
    );
    let restore_raced_expiry = restore_raced_expiry.unwrap();
    restore_raced_restore.unwrap();
    let source_decision = backup
        .list_snapshot_expiry_plan_entries(fixture.user_id, restore_raced_expiry.id(), None, 10)
        .await
        .unwrap()
        .0
        .into_iter()
        .find(|entry| entry.snapshot_id() == restore_race_old)
        .unwrap()
        .decision();
    let validated = backup
        .validate_snapshot_expiry_plan(fixture.user_id, restore_raced_expiry.id())
        .await
        .unwrap();
    match source_decision {
        BackupSnapshotExpiryDecision::BlockedActiveRestorePlan => {
            assert_eq!(validated.state(), BackupSnapshotExpiryPlanState::Planned);
        }
        BackupSnapshotExpiryDecision::Expire => {
            assert_eq!(validated.state(), BackupSnapshotExpiryPlanState::Stale);
        }
        other => panic!("unexpected restore-race decision: {other:?}"),
    }

    fixture.pool.close().await;
    fixture.inspection.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_expiry_plan_database_seal_enforces_boundary_and_rejects_partial_commit() {
    let fixture = fixture("database-seal").await;
    let backup = BackupService::new(fixture.pool.clone());

    // Build an exact committed_at == cutoff case at the database seal. The
    // service algorithm has the same equality unit test; this proves the
    // PostgreSQL decision fence independently accepts EXPIRE at equality.
    let boundary_set = create_set(&fixture, "boundary-seal-set").await;
    let evaluated_at = timestamp("2026-08-30T00:00:00Z");
    let cutoff_at = timestamp("2026-07-31T00:00:00Z");
    let newest_at = timestamp("2026-08-29T00:00:00Z");
    let newest =
        insert_completed_snapshot_at(&fixture, boundary_set, newest_at.as_offset_datetime()).await;
    let boundary =
        insert_completed_snapshot_at(&fixture, boundary_set, cutoff_at.as_offset_datetime()).await;
    let policy = backup
        .configure_snapshot_retention_policy(
            fixture.user_id,
            "boundary-seal-policy".to_owned(),
            boundary_set,
            1,
            30 * 86_400,
        )
        .await
        .unwrap();
    let request = BackupSnapshotExpiryPlanRequest::new(boundary_set);
    let basis = BackupSnapshotExpiryBasisFingerprint::calculate(
        boundary_set,
        policy.id(),
        evaluated_at,
        &[
            BackupSnapshotExpiryBasisEntry::new(newest, newest_at, false),
            BackupSnapshotExpiryBasisEntry::new(boundary, cutoff_at, false),
        ],
    );
    let plan_id = BackupSnapshotExpiryPlanId::new();
    let mut transaction = fixture.inspection.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO backup_snapshot_expiry_plans
            (id, owner_user_id, backup_set_id, policy_revision_id,
             policy_revision_number, operation_id, fingerprint_version,
             request_fingerprint, evaluated_at, cutoff_at,
             snapshot_basis_fingerprint_version, snapshot_basis_fingerprint,
             evaluated_completed_snapshot_count, expire_candidate_count,
             keep_latest_count, keep_recent_count, blocked_active_restore_count,
             state, created_at, stale_at)
         VALUES ($1, $2, $3, $4, $5, 'boundary-seal-plan', $6, $7, $8, $9,
                 $10, $11, 2, 1, 1, 0, 0, 'ASSEMBLING', $8, NULL)",
    )
    .bind(plan_id.into_uuid())
    .bind(fixture.user_id.into_uuid())
    .bind(boundary_set.into_uuid())
    .bind(policy.id().into_uuid())
    .bind(i64::try_from(policy.revision_number().get()).unwrap())
    .bind(i16::try_from(request.fingerprint().version()).unwrap())
    .bind(request.fingerprint().as_bytes().as_slice())
    .bind(evaluated_at.as_offset_datetime())
    .bind(cutoff_at.as_offset_datetime())
    .bind(i16::try_from(basis.version()).unwrap())
    .bind(basis.as_bytes().as_slice())
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO backup_snapshot_expiry_plan_entries
            (plan_id, snapshot_id, committed_at, recency_rank, decision)
         VALUES ($1, $2, $3, 1, 'KEEP_LATEST'),
                ($1, $4, $5, 2, 'EXPIRE')",
    )
    .bind(plan_id.into_uuid())
    .bind(newest.into_uuid())
    .bind(newest_at.as_offset_datetime())
    .bind(boundary.into_uuid())
    .bind(cutoff_at.as_offset_datetime())
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE backup_snapshot_expiry_plans SET state = 'PLANNED'
         WHERE id = $1 AND state = 'ASSEMBLING'",
    )
    .bind(plan_id.into_uuid())
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction
        .commit()
        .await
        .expect("exact boundary decision must satisfy deferred seal");
    assert_eq!(
        backup
            .list_snapshot_expiry_plan_entries(fixture.user_id, plan_id, Some(1), 10)
            .await
            .unwrap()
            .0[0]
            .decision(),
        BackupSnapshotExpiryDecision::Expire
    );

    // An ASSEMBLING row cannot become durable, even for a valid empty cohort.
    let unsealed_set = create_set(&fixture, "unsealed-empty-set").await;
    let unsealed_policy = backup
        .configure_snapshot_retention_policy(
            fixture.user_id,
            "unsealed-policy".to_owned(),
            unsealed_set,
            1,
            60,
        )
        .await
        .unwrap();
    let unsealed_request = BackupSnapshotExpiryPlanRequest::new(unsealed_set);
    let unsealed_evaluated = Timestamp::from_offset_datetime(
        sqlx::query_scalar::<_, OffsetDateTime>("SELECT clock_timestamp()")
            .fetch_one(&fixture.inspection)
            .await
            .unwrap(),
    );
    let unsealed_cutoff = unsealed_evaluated
        .checked_sub_std(std::time::Duration::from_secs(60))
        .unwrap();
    let unsealed_basis = BackupSnapshotExpiryBasisFingerprint::calculate(
        unsealed_set,
        unsealed_policy.id(),
        unsealed_evaluated,
        &[],
    );
    let unsealed_id = BackupSnapshotExpiryPlanId::new();
    let mut transaction = fixture.inspection.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO backup_snapshot_expiry_plans
            (id, owner_user_id, backup_set_id, policy_revision_id,
             policy_revision_number, operation_id, fingerprint_version,
             request_fingerprint, evaluated_at, cutoff_at,
             snapshot_basis_fingerprint_version, snapshot_basis_fingerprint,
             evaluated_completed_snapshot_count, expire_candidate_count,
             keep_latest_count, keep_recent_count, blocked_active_restore_count,
             state, created_at, stale_at)
         VALUES ($1, $2, $3, $4, $5, 'unsealed-expiry-plan', $6, $7, $8, $9,
                 $10, $11, 0, 0, 0, 0, 0, 'ASSEMBLING', $8, NULL)",
    )
    .bind(unsealed_id.into_uuid())
    .bind(fixture.user_id.into_uuid())
    .bind(unsealed_set.into_uuid())
    .bind(unsealed_policy.id().into_uuid())
    .bind(i64::try_from(unsealed_policy.revision_number().get()).unwrap())
    .bind(i16::try_from(unsealed_request.fingerprint().version()).unwrap())
    .bind(unsealed_request.fingerprint().as_bytes().as_slice())
    .bind(unsealed_evaluated.as_offset_datetime())
    .bind(unsealed_cutoff.as_offset_datetime())
    .bind(i16::try_from(unsealed_basis.version()).unwrap())
    .bind(unsealed_basis.as_bytes().as_slice())
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert!(transaction.commit().await.is_err());
    let durable_unsealed: i64 =
        sqlx::query_scalar("SELECT count(*) FROM backup_snapshot_expiry_plans WHERE id = $1")
            .bind(unsealed_id.into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .unwrap();
    assert_eq!(durable_unsealed, 0);

    fixture.pool.close().await;
    fixture.inspection.close().await;
}
