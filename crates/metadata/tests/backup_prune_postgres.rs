use std::time::Duration;

use sqlx::PgPool;
use synveil_core::{
    BackupPruneExecutionPreflightIssue, BackupPrunePlanState, BackupSetId, DedupDomainId,
    FileVersion, FileVersionId, Library, LibraryId, LogicalName, Node, NodeId, NodeKind, ObjectId,
    ObjectReference, Sha256Digest, SnapshotId, Timestamp, TrashRetentionPolicy, User, UserId,
    UserStatus,
};
use synveil_metadata::{
    BackupError, BackupService, DatabaseConfig, DatabasePool, DomainRepository,
    FileMetadataService, MigrationRunner, ObjectGcPlanningService, PurgeExecutionResult,
    TrashRetentionService,
};
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

async fn fixture() -> Fixture {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable PostgreSQL database");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    let status = MigrationRunner::new()
        .run(&pool)
        .await
        .expect("full migration chain must apply");
    assert!(status.is_current(), "migration chain must be current");
    assert_eq!(status.applied_versions().len(), 34);
    let inspection = PgPool::connect(&url)
        .await
        .expect("inspection connection must succeed");

    let observed_at = timestamp("2026-08-30T00:00:00.123456Z");
    let repository = DomainRepository::new(&pool);
    let user_id = UserId::new();
    repository
        .insert_user(&User::new(
            user_id,
            synveil_core::LoginIdentifier::new("prune-owner", format!("prune-owner-{user_id}"))
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
        name("Prune library"),
        &root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("fixture library is valid");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("fixture library must persist");
    let object = ObjectReference::new(
        ObjectId::new(),
        library.dedup_domain_id(),
        Sha256Digest::from_bytes([0x45; 32]),
        4_096,
    );
    repository
        .insert_object(object, observed_at)
        .await
        .expect("fixture Object must persist");
    let (file, version) =
        insert_live_reference(&pool, &library, &root, object, "source.bin", observed_at).await;
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

async fn insert_live_reference(
    pool: &DatabasePool,
    library: &Library,
    parent: &Node,
    object: ObjectReference,
    file_name: &str,
    observed_at: Timestamp,
) -> (Node, FileVersion) {
    let repository = DomainRepository::new(pool);
    let file = Node::new_child(
        NodeId::new(),
        library.id(),
        parent,
        NodeKind::File,
        name(file_name),
        observed_at,
    )
    .expect("fixture file is valid");
    repository
        .insert_node(&file)
        .await
        .expect("fixture file must persist");
    let version = FileVersion::new(
        FileVersionId::new(),
        library,
        &file,
        object,
        None,
        observed_at,
    )
    .expect("fixture FileVersion is valid");
    repository
        .insert_file_version(version)
        .await
        .expect("fixture FileVersion must persist");
    let file = file
        .with_current_version(&version, observed_at)
        .expect("fixture file accepts its version");
    repository
        .update_node(&file)
        .await
        .expect("fixture file head must persist");
    (file, version)
}

async fn create_set(fixture: &Fixture, label: &str) -> BackupSetId {
    let set_id = BackupSetId::new();
    BackupService::new(fixture.pool.clone())
        .create_backup_set(
            fixture.user_id,
            set_id,
            name(&format!("prune-{label}")),
            fixture.library.id(),
            Some(30),
            timestamp("2026-08-30T00:00:01.123456Z"),
        )
        .await
        .expect("backup set must persist");
    set_id
}

async fn create_snapshot(fixture: &Fixture, label: &str) -> (BackupSetId, SnapshotId) {
    let set_id = create_set(fixture, label).await;
    let snapshot_id = SnapshotId::new();
    BackupService::new(fixture.pool.clone())
        .capture_snapshot(
            fixture.user_id,
            set_id,
            snapshot_id,
            format!("prune-capture-{label}-{}", Uuid::now_v7().simple()),
        )
        .await
        .expect("fixture snapshot must complete");
    (set_id, snapshot_id)
}

async fn expire_snapshot(fixture: &Fixture, snapshot_id: SnapshotId) {
    sqlx::query(
        "UPDATE backup_snapshots
         SET state = 'EXPIRED', expired_at = CURRENT_TIMESTAMP
         WHERE id = $1 AND state = 'COMPLETED'",
    )
    .bind(snapshot_id.into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("completed snapshot must expire through its lifecycle transition");
}

async fn create_plan(
    fixture: &Fixture,
    set_id: BackupSetId,
    snapshot_id: SnapshotId,
    operation_id: &str,
) -> Result<synveil_core::BackupPrunePlan, BackupError> {
    BackupService::new(fixture.pool.clone())
        .create_prune_plan(
            fixture.user_id,
            operation_id.to_owned(),
            set_id,
            snapshot_id,
        )
        .await
}

async fn purge_file(fixture: &Fixture, file: Node) {
    let files = FileMetadataService::new(fixture.pool.clone());
    let trashed = files
        .delete_node(fixture.user_id, file.id(), file.revision())
        .await
        .expect("file must enter Trash through the canonical lifecycle");
    sqlx::query(
        "UPDATE nodes
         SET trashed_at = clock_timestamp() - INTERVAL '5 seconds'
         WHERE id = $1 AND library_id = $2",
    )
    .bind(trashed.id().into_uuid())
    .bind(fixture.library.id().into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("test trash age must persist");
    let retention = TrashRetentionService::new(
        fixture.pool.clone(),
        TrashRetentionPolicy::new(Duration::from_secs(1)).expect("short retention is valid"),
    );
    let purging = retention
        .begin_node_purge(fixture.user_id, trashed.id(), trashed.revision())
        .await
        .expect("expired file must begin metadata purge");
    assert_eq!(
        retention
            .execute_metadata_purge(fixture.user_id, purging.id(), purging.revision())
            .await
            .expect("metadata purge must complete"),
        PurgeExecutionResult::Completed
    );
}

async fn source_evidence(pool: &PgPool, snapshot_id: SnapshotId) -> (String, i64, i64, i64, i64) {
    sqlx::query_as(
        "SELECT state, manifest_item_count, content_reference_count,
                (SELECT count(*) FROM backup_snapshot_nodes WHERE snapshot_id = $1),
                (SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1)
         FROM backup_snapshots
         WHERE id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(pool)
    .await
    .expect("source evidence must load")
}

async fn impact_rows(
    pool: &PgPool,
    plan_id: synveil_core::BackupPrunePlanId,
) -> Vec<(i64, i64, i64, i64, String)> {
    sqlx::query_as(
        "SELECT target_snapshot_pin_count,
                surviving_live_file_version_reference_count,
                surviving_other_snapshot_pin_count,
                predicted_post_release_reference_count, impact
         FROM backup_prune_plan_object_impacts
         WHERE plan_id = $1
         ORDER BY object_id, object_dedup_domain_id",
    )
    .bind(plan_id.into_uuid())
    .fetch_all(pool)
    .await
    .expect("private accounting evidence must load")
}

async fn insert_verified_replica(pool: &PgPool, object: ObjectReference) {
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
    .bind(format!("prune-replica-{}", Uuid::now_v7().simple()))
    .bind(object.plaintext_length().to_string())
    .bind(object.canonical_hash().as_bytes())
    .execute(pool)
    .await
    .expect("verified replica metadata must persist");
}

async fn create_directory_only_expired_snapshot(
    fixture: &Fixture,
    label: &str,
) -> (BackupSetId, SnapshotId) {
    let set_id = create_set(fixture, label).await;
    let snapshot_id = SnapshotId::new();
    let (epoch, head): (i64, i64) = sqlx::query_as(
        "SELECT journal_epoch, sync_head FROM libraries WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(fixture.library.id().into_uuid())
    .bind(fixture.user_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("library clock must load");
    sqlx::query(
        "INSERT INTO backup_snapshots
            (id, backup_set_id, owner_user_id, source_library_id, operation_id,
             snapshot_epoch, snapshot_resume_sequence, manifest_item_count,
             terminal_node_id, content_reference_count, state, created_at,
             committed_at, expired_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, 1, $8, 0, 'BUILDING',
                 CURRENT_TIMESTAMP, NULL, NULL)",
    )
    .bind(snapshot_id.into_uuid())
    .bind(set_id.into_uuid())
    .bind(fixture.user_id.into_uuid())
    .bind(fixture.library.id().into_uuid())
    .bind(format!(
        "directory-prune-{label}-{}",
        Uuid::now_v7().simple()
    ))
    .bind(epoch)
    .bind(head)
    .bind(fixture.root.id().into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("building directory snapshot must persist");
    sqlx::query(
        "INSERT INTO backup_snapshot_nodes
            (snapshot_id, node_id, parent_node_id, name, kind, state, revision,
             current_version_id, content_length, content_sha256,
             node_created_at, node_updated_at)
         VALUES ($1, $2, NULL, $3, 'DIRECTORY', 'ACTIVE', $4::NUMERIC,
                 NULL, NULL, NULL, $5, $6)",
    )
    .bind(snapshot_id.into_uuid())
    .bind(fixture.root.id().into_uuid())
    .bind(fixture.root.name().as_str())
    .bind(fixture.root.revision().get().to_string())
    .bind(fixture.root.created_at().as_offset_datetime())
    .bind(fixture.root.updated_at().as_offset_datetime())
    .execute(&fixture.inspection)
    .await
    .expect("directory manifest must persist");
    sqlx::query(
        "UPDATE backup_snapshots
         SET state = 'COMPLETED', committed_at = CURRENT_TIMESTAMP
         WHERE id = $1 AND state = 'BUILDING'",
    )
    .bind(snapshot_id.into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("zero-content snapshot must complete");
    expire_snapshot(fixture, snapshot_id).await;
    (set_id, snapshot_id)
}

async fn create_empty_nonexpired_snapshot(
    fixture: &Fixture,
    label: &str,
    failed: bool,
) -> (BackupSetId, SnapshotId) {
    let set_id = create_set(fixture, label).await;
    let snapshot_id = SnapshotId::new();
    let (epoch, head): (i64, i64) = sqlx::query_as(
        "SELECT journal_epoch, sync_head FROM libraries WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(fixture.library.id().into_uuid())
    .bind(fixture.user_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("library clock must load");
    sqlx::query(
        "INSERT INTO backup_snapshots
            (id, backup_set_id, owner_user_id, source_library_id, operation_id,
             snapshot_epoch, snapshot_resume_sequence, manifest_item_count,
             terminal_node_id, content_reference_count, state, created_at,
             committed_at, expired_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, 0, NULL, 0, 'BUILDING',
                 CURRENT_TIMESTAMP, NULL, NULL)",
    )
    .bind(snapshot_id.into_uuid())
    .bind(set_id.into_uuid())
    .bind(fixture.user_id.into_uuid())
    .bind(fixture.library.id().into_uuid())
    .bind(format!(
        "nonexpired-prune-{label}-{}",
        Uuid::now_v7().simple()
    ))
    .bind(epoch)
    .bind(head)
    .execute(&fixture.inspection)
    .await
    .expect("building non-expired snapshot must persist");
    if failed {
        sqlx::query(
            "UPDATE backup_snapshots SET state = 'FAILED'
             WHERE id = $1 AND state = 'BUILDING'",
        )
        .bind(snapshot_id.into_uuid())
        .execute(&fixture.inspection)
        .await
        .expect("building snapshot must transition to failed");
    }
    (set_id, snapshot_id)
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_plan_is_read_only_idempotent_and_stales_when_a_live_reference_appears() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "sole-owner").await;
    purge_file(&fixture, fixture.file.clone()).await;
    expire_snapshot(&fixture, snapshot_id).await;
    let before_source = source_evidence(&fixture.inspection, snapshot_id).await;
    let before_object_count: i64 = sqlx::query_scalar("SELECT count(*) FROM objects")
        .fetch_one(&fixture.inspection)
        .await
        .expect("Object count must load");
    let before_gc_count: i64 = sqlx::query_scalar("SELECT count(*) FROM object_gc_candidates")
        .fetch_one(&fixture.inspection)
        .await
        .expect("GC count must load");

    let plan = create_plan(&fixture, set_id, snapshot_id, "prune-sole-owner-0001")
        .await
        .expect("expired sole-owner snapshot must plan");
    assert_eq!(plan.state(), BackupPrunePlanState::Planned);
    assert_eq!(plan.planned_pin_release_count(), 1);
    assert_eq!(plan.distinct_retained_content_count(), 1);
    assert_eq!(plan.retained_after_release_count(), 0);
    assert_eq!(plan.would_become_unreferenced_count(), 1);
    assert_eq!(
        impact_rows(&fixture.inspection, plan.id()).await,
        vec![(1, 0, 0, 0, "WOULD_BECOME_UNREFERENCED".to_owned())]
    );
    let entries = BackupService::new(fixture.pool.clone())
        .list_prune_plan_entries(fixture.user_id, plan.id(), None, 10)
        .await
        .expect("logical entries must list")
        .0;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].content().file_version_id(), fixture.version.id());
    assert_eq!(
        source_evidence(&fixture.inspection, snapshot_id).await,
        before_source
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM objects")
            .fetch_one(&fixture.inspection)
            .await
            .expect("Object count must reload"),
        before_object_count
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM object_gc_candidates")
            .fetch_one(&fixture.inspection)
            .await
            .expect("GC count must reload"),
        before_gc_count
    );

    let replay = create_plan(&fixture, set_id, snapshot_id, "prune-sole-owner-0001")
        .await
        .expect("same semantic operation must replay canonically");
    assert_eq!(replay.id(), plan.id());
    assert_eq!(
        create_plan(&fixture, set_id, snapshot_id, "prune-sole-owner-0002").await,
        Err(BackupError::PruneAlreadyPlanned)
    );

    let _new_live = insert_live_reference(
        &fixture.pool,
        &fixture.library,
        &fixture.root,
        fixture.object,
        "appeared-after-plan.bin",
        timestamp("2026-08-30T00:00:02.123456Z"),
    )
    .await;
    let stale = BackupService::new(fixture.pool.clone())
        .validate_prune_plan(fixture.user_id, plan.id())
        .await
        .expect("accounting drift must validate safely");
    assert_eq!(stale.state(), BackupPrunePlanState::Stale);
    assert_eq!(stale.would_become_unreferenced_count(), 1);
    assert_eq!(
        impact_rows(&fixture.inspection, plan.id()).await,
        vec![(1, 0, 0, 0, "WOULD_BECOME_UNREFERENCED".to_owned())]
    );
    assert_eq!(
        source_evidence(&fixture.inspection, snapshot_id).await,
        before_source
    );
    let replan = create_plan(&fixture, set_id, snapshot_id, "prune-sole-owner-replan")
        .await
        .expect("a stale plan must not prevent explicit replanning");
    assert_ne!(replan.id(), plan.id());
    assert_eq!(replan.state(), BackupPrunePlanState::Planned);
    assert_eq!(replan.retained_after_release_count(), 1);
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_plan_excludes_all_target_pins_and_keeps_other_snapshot_pins() {
    let fixture = fixture().await;
    // S2 captures one pin before a second live FileVersion is added. S1 then
    // captures two pins to the same Object, exercising the all-target-pins
    // prospective exclusion rather than per-manifest-row counting.
    let (_set_s2, s2) = create_snapshot(&fixture, "other-snapshot").await;
    let (second_file, _second_version) = insert_live_reference(
        &fixture.pool,
        &fixture.library,
        &fixture.root,
        fixture.object,
        "second-same-object.bin",
        timestamp("2026-08-30T00:00:02.123456Z"),
    )
    .await;
    let (set_s1, s1) = create_snapshot(&fixture, "two-target-pins").await;
    purge_file(&fixture, fixture.file.clone()).await;
    purge_file(&fixture, second_file).await;
    expire_snapshot(&fixture, s1).await;
    let s2_pins_before: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(s2.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("other snapshot pin count must load");
    assert_eq!(s2_pins_before, 1);

    let plan = create_plan(&fixture, set_s1, s1, "prune-multi-target-pins")
        .await
        .expect("multi-pin source must plan");
    assert_eq!(plan.planned_pin_release_count(), 2);
    assert_eq!(plan.distinct_retained_content_count(), 1);
    assert_eq!(plan.retained_after_release_count(), 1);
    assert_eq!(plan.would_become_unreferenced_count(), 0);
    assert_eq!(
        impact_rows(&fixture.inspection, plan.id()).await,
        vec![(2, 0, 1, 1, "RETAINED_BY_OTHER_REFERENCE".to_owned())]
    );
    let s1_pins_after: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(s1.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("target pins must remain inspectable");
    let s2_pins_after: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(s2.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("other pins must remain inspectable");
    assert_eq!((s1_pins_after, s2_pins_after), (2, s2_pins_before));
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_plan_excludes_all_multi_pins_for_a_sole_owner() {
    let fixture = fixture().await;
    let (second_file, _second_version) = insert_live_reference(
        &fixture.pool,
        &fixture.library,
        &fixture.root,
        fixture.object,
        "sole-owner-second-same-object.bin",
        timestamp("2026-08-30T00:00:02.123456Z"),
    )
    .await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "multi-pin-sole-owner").await;
    purge_file(&fixture, fixture.file.clone()).await;
    purge_file(&fixture, second_file).await;
    expire_snapshot(&fixture, snapshot_id).await;
    let before_source = source_evidence(&fixture.inspection, snapshot_id).await;
    let before_object_count: i64 = sqlx::query_scalar("SELECT count(*) FROM objects")
        .fetch_one(&fixture.inspection)
        .await
        .expect("Object count must load");
    let before_gc_count: i64 = sqlx::query_scalar("SELECT count(*) FROM object_gc_candidates")
        .fetch_one(&fixture.inspection)
        .await
        .expect("GC count must load");

    let plan = create_plan(&fixture, set_id, snapshot_id, "prune-multi-pin-sole-owner")
        .await
        .expect("sole-authority multi-pin snapshot must plan");
    assert_eq!(plan.planned_pin_release_count(), 2);
    assert_eq!(plan.distinct_retained_content_count(), 1);
    assert_eq!(plan.retained_after_release_count(), 0);
    assert_eq!(plan.would_become_unreferenced_count(), 1);
    assert_eq!(
        impact_rows(&fixture.inspection, plan.id()).await,
        vec![(2, 0, 0, 0, "WOULD_BECOME_UNREFERENCED".to_owned())]
    );
    assert_eq!(
        source_evidence(&fixture.inspection, snapshot_id).await,
        before_source,
        "planning leaves every source pin and manifest row intact"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM objects")
            .fetch_one(&fixture.inspection)
            .await
            .expect("Object count must reload"),
        before_object_count
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM object_gc_candidates")
            .fetch_one(&fixture.inspection)
            .await
            .expect("GC count must reload"),
        before_gc_count
    );
    let unchanged = BackupService::new(fixture.pool.clone())
        .validate_prune_plan(fixture.user_id, plan.id())
        .await
        .expect("unchanged accounting basis must remain valid");
    assert_eq!(unchanged.state(), BackupPrunePlanState::Planned);
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_plan_counts_live_and_other_snapshot_references_together() {
    let fixture = fixture().await;
    let (_other_set, _other_snapshot) = create_snapshot(&fixture, "both-other-snapshot").await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "both-target-snapshot").await;
    expire_snapshot(&fixture, snapshot_id).await;

    let plan = create_plan(&fixture, set_id, snapshot_id, "prune-retained-by-both")
        .await
        .expect("live and other-retention references must plan");
    assert_eq!(plan.planned_pin_release_count(), 1);
    assert_eq!(plan.retained_after_release_count(), 1);
    assert_eq!(plan.would_become_unreferenced_count(), 0);
    assert_eq!(
        impact_rows(&fixture.inspection, plan.id()).await,
        vec![(1, 1, 1, 2, "RETAINED_BY_OTHER_REFERENCE".to_owned())]
    );
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_plan_stales_when_a_live_reference_disappears_or_other_retention_appears() {
    let first_fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&first_fixture, "live-reference-disappears").await;
    expire_snapshot(&first_fixture, snapshot_id).await;
    let plan = create_plan(
        &first_fixture,
        set_id,
        snapshot_id,
        "prune-live-reference-disappears",
    )
    .await
    .expect("live-reference plan must seal");
    assert_eq!(
        impact_rows(&first_fixture.inspection, plan.id()).await,
        vec![(1, 1, 0, 1, "RETAINED_BY_OTHER_REFERENCE".to_owned())]
    );
    purge_file(&first_fixture, first_fixture.file.clone()).await;
    let stale = BackupService::new(first_fixture.pool.clone())
        .validate_prune_plan(first_fixture.user_id, plan.id())
        .await
        .expect("removed live reference must stale plan");
    assert_eq!(stale.state(), BackupPrunePlanState::Stale);
    assert_eq!(
        impact_rows(&first_fixture.inspection, plan.id()).await,
        vec![(1, 1, 0, 1, "RETAINED_BY_OTHER_REFERENCE".to_owned())]
    );

    let other_fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&other_fixture, "other-pin-appears").await;
    expire_snapshot(&other_fixture, snapshot_id).await;
    let plan = create_plan(
        &other_fixture,
        set_id,
        snapshot_id,
        "prune-other-pin-appears",
    )
    .await
    .expect("base plan must seal");
    assert_eq!(
        impact_rows(&other_fixture.inspection, plan.id()).await[0].2,
        0
    );
    let _other_snapshot = create_snapshot(&other_fixture, "other-pin-after-plan").await;
    let stale = BackupService::new(other_fixture.pool.clone())
        .validate_prune_plan(other_fixture.user_id, plan.id())
        .await
        .expect("new other snapshot retention must stale plan");
    assert_eq!(stale.state(), BackupPrunePlanState::Stale);
    assert_eq!(
        impact_rows(&other_fixture.inspection, plan.id()).await[0].2,
        0
    );

    let disappearing_fixture = fixture().await;
    let (_other_set, other_snapshot) =
        create_snapshot(&disappearing_fixture, "other-pin-before-plan").await;
    let (set_id, snapshot_id) =
        create_snapshot(&disappearing_fixture, "other-pin-disappears").await;
    expire_snapshot(&disappearing_fixture, snapshot_id).await;
    let plan = create_plan(
        &disappearing_fixture,
        set_id,
        snapshot_id,
        "prune-other-pin-disappears",
    )
    .await
    .expect("plan with surviving other retention must seal");
    assert_eq!(
        impact_rows(&disappearing_fixture.inspection, plan.id()).await[0].2,
        1
    );
    let mut injection = disappearing_fixture
        .inspection
        .begin()
        .await
        .expect("test-only retention corruption transaction must begin");
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *injection)
        .await
        .expect("test-only trigger bypass must enable");
    sqlx::query("DELETE FROM backup_snapshot_content_pins WHERE snapshot_id = $1")
        .bind(other_snapshot.into_uuid())
        .execute(&mut *injection)
        .await
        .expect("test-only other-pin disappearance must persist");
    injection
        .commit()
        .await
        .expect("test-only retention corruption must commit");
    let stale = BackupService::new(disappearing_fixture.pool.clone())
        .validate_prune_plan(disappearing_fixture.user_id, plan.id())
        .await
        .expect("removed other retention must stale the original basis");
    assert_eq!(stale.state(), BackupPrunePlanState::Stale);
    assert_eq!(
        impact_rows(&disappearing_fixture.inspection, plan.id()).await[0].2,
        1,
        "stored basis must not be rewritten after other-pin drift"
    );
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_plan_counts_restore_created_file_versions_as_live_references() {
    let fixture = fixture().await;
    insert_verified_replica(&fixture.inspection, fixture.object).await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "restore-created-reference").await;
    let backup = BackupService::new(fixture.pool.clone());
    let restore_plan = backup
        .create_restore_plan(
            fixture.user_id,
            "restore-for-prune-0001".to_owned(),
            set_id,
            snapshot_id,
            fixture.library.id(),
            fixture.root.id(),
            name("restored-source"),
        )
        .await
        .expect("restore planning must succeed");
    let receipt = backup
        .execute_restore_plan(fixture.user_id, restore_plan.id())
        .await
        .expect("restore execution must create destination FileVersions");
    assert_eq!(receipt.created_file_version_count(), 1);
    purge_file(&fixture, fixture.file.clone()).await;
    expire_snapshot(&fixture, snapshot_id).await;
    let restore_history_before: (String, i64, i64) = sqlx::query_as(
        "SELECT plan.state,
                (SELECT count(*) FROM backup_restore_executions
                 WHERE restore_plan_id = plan.id),
                (SELECT count(*) FROM backup_restore_execution_entries AS entry
                 INNER JOIN backup_restore_executions AS execution
                   ON execution.id = entry.execution_id
                 WHERE execution.restore_plan_id = plan.id)
         FROM backup_restore_plans AS plan
         WHERE plan.id = $1",
    )
    .bind(restore_plan.id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("restore history must load before prune planning");
    let replica_state_before: (i64, Option<String>) = sqlx::query_as(
        "SELECT count(*), string_agg(state, ',' ORDER BY id)
         FROM object_replicas
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("replica evidence must load before prune planning");

    let plan = create_plan(&fixture, set_id, snapshot_id, "prune-restored-reference")
        .await
        .expect("expired restored source must plan");
    assert_eq!(plan.retained_after_release_count(), 1);
    assert_eq!(plan.would_become_unreferenced_count(), 0);
    assert_eq!(
        impact_rows(&fixture.inspection, plan.id()).await,
        vec![(1, 1, 0, 1, "RETAINED_BY_OTHER_REFERENCE".to_owned())]
    );
    let restore_history_after: (String, i64, i64) = sqlx::query_as(
        "SELECT plan.state,
                (SELECT count(*) FROM backup_restore_executions
                 WHERE restore_plan_id = plan.id),
                (SELECT count(*) FROM backup_restore_execution_entries AS entry
                 INNER JOIN backup_restore_executions AS execution
                   ON execution.id = entry.execution_id
                 WHERE execution.restore_plan_id = plan.id)
         FROM backup_restore_plans AS plan
         WHERE plan.id = $1",
    )
    .bind(restore_plan.id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("restore history must reload after prune planning");
    let replica_state_after: (i64, Option<String>) = sqlx::query_as(
        "SELECT count(*), string_agg(state, ',' ORDER BY id)
         FROM object_replicas
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("replica evidence must reload after prune planning");
    assert_eq!(restore_history_after, restore_history_before);
    assert_eq!(replica_state_after, replica_state_before);
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_plan_enforces_eligibility_ownership_zero_content_and_immutability() {
    let fixture = fixture().await;
    let (completed_set, completed) = create_snapshot(&fixture, "completed-ineligible").await;
    assert_eq!(
        create_plan(
            &fixture,
            completed_set,
            completed,
            "prune-completed-ineligible",
        )
        .await,
        Err(BackupError::PrunePreflight(
            synveil_core::BackupPrunePreflightIssue::SnapshotNotExpired,
        ))
    );
    assert_eq!(
        BackupService::new(fixture.pool.clone())
            .create_prune_plan(
                UserId::new(),
                "prune-foreign-snapshot".to_owned(),
                completed_set,
                completed,
            )
            .await,
        Err(BackupError::NotFound)
    );
    let (building_set, building_snapshot) =
        create_empty_nonexpired_snapshot(&fixture, "building-ineligible", false).await;
    assert_eq!(
        create_plan(
            &fixture,
            building_set,
            building_snapshot,
            "prune-building-ineligible",
        )
        .await,
        Err(BackupError::PrunePreflight(
            synveil_core::BackupPrunePreflightIssue::SnapshotNotExpired,
        ))
    );
    let (failed_set, failed_snapshot) =
        create_empty_nonexpired_snapshot(&fixture, "failed-ineligible", true).await;
    assert_eq!(
        create_plan(
            &fixture,
            failed_set,
            failed_snapshot,
            "prune-failed-ineligible",
        )
        .await,
        Err(BackupError::PrunePreflight(
            synveil_core::BackupPrunePreflightIssue::SnapshotNotExpired,
        ))
    );

    let (directory_set, directory_snapshot) =
        create_directory_only_expired_snapshot(&fixture, "directory-only").await;
    let directory_plan = create_plan(
        &fixture,
        directory_set,
        directory_snapshot,
        "prune-directory-only",
    )
    .await
    .expect("zero-content expired snapshot must plan");
    assert_eq!(directory_plan.state(), BackupPrunePlanState::Planned);
    assert_eq!(directory_plan.planned_pin_release_count(), 0);
    assert_eq!(directory_plan.distinct_retained_content_count(), 0);
    assert!(
        BackupService::new(fixture.pool.clone())
            .list_prune_plan_entries(fixture.user_id, directory_plan.id(), None, 10)
            .await
            .expect("zero-content entries must list")
            .0
            .is_empty()
    );

    let immutable_result = sqlx::query(
        "UPDATE backup_prune_plans
         SET planned_pin_release_count = 1
         WHERE id = $1",
    )
    .bind(directory_plan.id().into_uuid())
    .execute(&fixture.inspection)
    .await;
    assert!(
        immutable_result.is_err(),
        "sealed plan provenance is immutable"
    );
    let plan_delete = sqlx::query("DELETE FROM backup_prune_plans WHERE id = $1")
        .bind(directory_plan.id().into_uuid())
        .execute(&fixture.inspection)
        .await;
    assert!(plan_delete.is_err(), "sealed plan delete is forbidden");

    let (different_set, different_snapshot) =
        create_directory_only_expired_snapshot(&fixture, "idempotency-conflict").await;
    assert_eq!(
        create_plan(
            &fixture,
            different_set,
            different_snapshot,
            "prune-directory-only",
        )
        .await,
        Err(BackupError::PrunePlanConflict)
    );

    let (content_set, content_snapshot) = create_snapshot(&fixture, "entry-immutability").await;
    expire_snapshot(&fixture, content_snapshot).await;
    let content_plan = create_plan(
        &fixture,
        content_set,
        content_snapshot,
        "prune-entry-immutability",
    )
    .await
    .expect("content plan must seal");
    let entry_update = sqlx::query(
        "UPDATE backup_prune_plan_entries
         SET ordinal = 99 WHERE plan_id = $1 AND ordinal = 0",
    )
    .bind(content_plan.id().into_uuid())
    .execute(&fixture.inspection)
    .await;
    let impact_delete =
        sqlx::query("DELETE FROM backup_prune_plan_object_impacts WHERE plan_id = $1")
            .bind(content_plan.id().into_uuid())
            .execute(&fixture.inspection)
            .await;
    assert!(
        entry_update.is_err(),
        "sealed logical entries are immutable"
    );
    assert!(
        impact_delete.is_err(),
        "sealed internal impacts are immutable"
    );
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_plan_corruption_and_assembly_failure_rollback_fail_closed() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "corrupt-missing-pin").await;
    expire_snapshot(&fixture, snapshot_id).await;
    // Test-only corruption injection: bypass production trigger enforcement on
    // a disposable DB, then prove planner validation commits no plan evidence.
    let mut injection = fixture
        .inspection
        .begin()
        .await
        .expect("corruption transaction must begin");
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *injection)
        .await
        .expect("test-only trigger bypass must enable");
    sqlx::query("DELETE FROM backup_snapshot_content_pins WHERE snapshot_id = $1")
        .bind(snapshot_id.into_uuid())
        .execute(&mut *injection)
        .await
        .expect("test-only missing-pin corruption must persist");
    injection
        .commit()
        .await
        .expect("corruption transaction must commit");
    assert_eq!(
        create_plan(&fixture, set_id, snapshot_id, "prune-corrupt-missing-pin").await,
        Err(BackupError::PrunePreflight(
            synveil_core::BackupPrunePreflightIssue::RetentionCorrupt,
        ))
    );
    let corrupt_plan_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_prune_plans WHERE owner_user_id = $1 AND operation_id = $2",
    )
    .bind(fixture.user_id.into_uuid())
    .bind("prune-corrupt-missing-pin")
    .fetch_one(&fixture.inspection)
    .await
    .expect("corrupt plan count must load");
    assert_eq!(corrupt_plan_count, 0);

    let (retry_set, retry_snapshot) = create_snapshot(&fixture, "assembly-rollback").await;
    expire_snapshot(&fixture, retry_snapshot).await;
    sqlx::query(
        "CREATE FUNCTION synveil_test_prune_fail_entry()
         RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             RAISE EXCEPTION 'injected prune entry failure';
         END;
         $$",
    )
    .execute(&fixture.inspection)
    .await
    .expect("failure injection function must persist");
    sqlx::query(
        "CREATE TRIGGER synveil_test_prune_fail_entry_trigger
         BEFORE INSERT ON backup_prune_plan_entries
         FOR EACH ROW EXECUTE FUNCTION synveil_test_prune_fail_entry()",
    )
    .execute(&fixture.inspection)
    .await
    .expect("failure injection trigger must persist");
    assert!(
        create_plan(
            &fixture,
            retry_set,
            retry_snapshot,
            "prune-assembly-rollback",
        )
        .await
        .is_err(),
        "entry failure after plan row insertion must abort the whole plan"
    );
    sqlx::query("DROP TRIGGER synveil_test_prune_fail_entry_trigger ON backup_prune_plan_entries")
        .execute(&fixture.inspection)
        .await
        .expect("failure injection trigger must drop");
    sqlx::query("DROP FUNCTION synveil_test_prune_fail_entry()")
        .execute(&fixture.inspection)
        .await
        .expect("failure injection function must drop");
    let rollback_counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT
            (SELECT count(*) FROM backup_prune_plans
             WHERE owner_user_id = $1 AND operation_id = 'prune-assembly-rollback'),
            (SELECT count(*) FROM backup_prune_plan_entries AS entry
             INNER JOIN backup_prune_plans AS plan ON plan.id = entry.plan_id
             WHERE plan.owner_user_id = $1
               AND plan.operation_id = 'prune-assembly-rollback'),
            (SELECT count(*) FROM backup_prune_plan_object_impacts AS impact
             INNER JOIN backup_prune_plans AS plan ON plan.id = impact.plan_id
             WHERE plan.owner_user_id = $1
               AND plan.operation_id = 'prune-assembly-rollback')",
    )
    .bind(fixture.user_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("rollback evidence must load");
    assert_eq!(rollback_counts.0, 0);
    assert_eq!(rollback_counts.1, 0);
    assert_eq!(rollback_counts.2, 0);
    create_plan(
        &fixture,
        retry_set,
        retry_snapshot,
        "prune-assembly-rollback",
    )
    .await
    .expect("clean retry after rollback must succeed");

    let _second = insert_live_reference(
        &fixture.pool,
        &fixture.library,
        &fixture.root,
        fixture.object,
        "partial-entry.bin",
        timestamp("2026-08-30T00:00:03.123456Z"),
    )
    .await;
    let (partial_set, partial_snapshot) = create_snapshot(&fixture, "partial-entry").await;
    expire_snapshot(&fixture, partial_snapshot).await;
    sqlx::query(
        "CREATE FUNCTION synveil_test_prune_fail_second_entry()
         RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             IF NEW.ordinal = 1 THEN
                 RAISE EXCEPTION 'injected second prune entry failure';
             END IF;
             RETURN NEW;
         END;
         $$",
    )
    .execute(&fixture.inspection)
    .await
    .expect("partial-entry failure function must persist");
    sqlx::query(
        "CREATE TRIGGER synveil_test_prune_fail_second_entry_trigger
         BEFORE INSERT ON backup_prune_plan_entries
         FOR EACH ROW EXECUTE FUNCTION synveil_test_prune_fail_second_entry()",
    )
    .execute(&fixture.inspection)
    .await
    .expect("partial-entry failure trigger must persist");
    assert!(
        create_plan(
            &fixture,
            partial_set,
            partial_snapshot,
            "prune-partial-entry-rollback",
        )
        .await
        .is_err(),
        "failure after one logical entry must roll back plan and prior entries"
    );
    sqlx::query(
        "DROP TRIGGER synveil_test_prune_fail_second_entry_trigger
         ON backup_prune_plan_entries",
    )
    .execute(&fixture.inspection)
    .await
    .expect("partial-entry failure trigger must drop");
    sqlx::query("DROP FUNCTION synveil_test_prune_fail_second_entry()")
        .execute(&fixture.inspection)
        .await
        .expect("partial-entry failure function must drop");
    let partial_entries: i64 = sqlx::query_scalar(
        "SELECT count(*)
         FROM backup_prune_plan_entries AS entry
         INNER JOIN backup_prune_plans AS plan ON plan.id = entry.plan_id
         WHERE plan.owner_user_id = $1
           AND plan.operation_id = 'prune-partial-entry-rollback'",
    )
    .bind(fixture.user_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("partial-entry rollback evidence must load");
    assert_eq!(partial_entries, 0);

    let different_object = ObjectReference::new(
        ObjectId::new(),
        fixture.library.dedup_domain_id(),
        Sha256Digest::from_bytes([0x99; 32]),
        1_024,
    );
    DomainRepository::new(&fixture.pool)
        .insert_object(different_object, timestamp("2026-08-30T00:00:04.123456Z"))
        .await
        .expect("second canonical Object must persist");
    let _different_object_file = insert_live_reference(
        &fixture.pool,
        &fixture.library,
        &fixture.root,
        different_object,
        "partial-impact-different-object.bin",
        timestamp("2026-08-30T00:00:04.123456Z"),
    )
    .await;
    let (impact_set, impact_snapshot) = create_snapshot(&fixture, "impact-rollback").await;
    expire_snapshot(&fixture, impact_snapshot).await;
    sqlx::query(
        "CREATE FUNCTION synveil_test_prune_fail_impact()
         RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             IF EXISTS (
                 SELECT 1 FROM backup_prune_plan_object_impacts
                 WHERE plan_id = NEW.plan_id
             ) THEN
                 RAISE EXCEPTION 'injected second prune impact failure';
             END IF;
             RETURN NEW;
         END;
         $$",
    )
    .execute(&fixture.inspection)
    .await
    .expect("impact failure function must persist");
    sqlx::query(
        "CREATE TRIGGER synveil_test_prune_fail_impact_trigger
         BEFORE INSERT ON backup_prune_plan_object_impacts
         FOR EACH ROW EXECUTE FUNCTION synveil_test_prune_fail_impact()",
    )
    .execute(&fixture.inspection)
    .await
    .expect("impact failure trigger must persist");
    assert!(
        create_plan(
            &fixture,
            impact_set,
            impact_snapshot,
            "prune-impact-rollback",
        )
        .await
        .is_err(),
        "failure after the first internal impact must roll back all plan evidence"
    );
    sqlx::query(
        "DROP TRIGGER synveil_test_prune_fail_impact_trigger
         ON backup_prune_plan_object_impacts",
    )
    .execute(&fixture.inspection)
    .await
    .expect("impact failure trigger must drop");
    sqlx::query("DROP FUNCTION synveil_test_prune_fail_impact()")
        .execute(&fixture.inspection)
        .await
        .expect("impact failure function must drop");
    let impact_rollback: (i64, i64, i64) = sqlx::query_as(
        "SELECT
            (SELECT count(*) FROM backup_prune_plans
             WHERE owner_user_id = $1 AND operation_id = 'prune-impact-rollback'),
            (SELECT count(*) FROM backup_prune_plan_entries AS entry
             INNER JOIN backup_prune_plans AS plan ON plan.id = entry.plan_id
             WHERE plan.owner_user_id = $1
               AND plan.operation_id = 'prune-impact-rollback'),
            (SELECT count(*) FROM backup_prune_plan_object_impacts AS impact
             INNER JOIN backup_prune_plans AS plan ON plan.id = impact.plan_id
             WHERE plan.owner_user_id = $1
               AND plan.operation_id = 'prune-impact-rollback')",
    )
    .bind(fixture.user_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("impact rollback evidence must load");
    assert_eq!(impact_rollback, (0, 0, 0));
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_plan_rejects_wrong_pin_mapping_and_missing_canonical_object() {
    let fixture = fixture().await;

    let (set_id, snapshot_id) = create_snapshot(&fixture, "wrong-pin-version").await;
    expire_snapshot(&fixture, snapshot_id).await;
    let mut injection = fixture.inspection.begin().await.expect("injection begins");
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *injection)
        .await
        .expect("test-only trigger bypass enables");
    sqlx::query(
        "UPDATE backup_snapshot_content_pins
         SET file_version_id = $2 WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .bind(FileVersionId::new().into_uuid())
    .execute(&mut *injection)
    .await
    .expect("wrong FileVersion corruption persists");
    injection.commit().await.expect("injection commits");
    assert_eq!(
        create_plan(&fixture, set_id, snapshot_id, "prune-wrong-pin-version").await,
        Err(BackupError::PrunePreflight(
            synveil_core::BackupPrunePreflightIssue::RetentionCorrupt,
        ))
    );

    let (set_id, snapshot_id) = create_snapshot(&fixture, "directory-pin").await;
    expire_snapshot(&fixture, snapshot_id).await;
    let mut injection = fixture.inspection.begin().await.expect("injection begins");
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *injection)
        .await
        .expect("test-only trigger bypass enables");
    sqlx::query(
        "UPDATE backup_snapshot_content_pins
         SET manifest_node_id = $2 WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .bind(fixture.root.id().into_uuid())
    .execute(&mut *injection)
    .await
    .expect("directory-pin corruption persists");
    injection.commit().await.expect("injection commits");
    assert_eq!(
        create_plan(&fixture, set_id, snapshot_id, "prune-directory-pin").await,
        Err(BackupError::PrunePreflight(
            synveil_core::BackupPrunePreflightIssue::RetentionCorrupt,
        ))
    );

    let (set_id, snapshot_id) = create_snapshot(&fixture, "wrong-object-domain").await;
    expire_snapshot(&fixture, snapshot_id).await;
    let mut injection = fixture.inspection.begin().await.expect("injection begins");
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *injection)
        .await
        .expect("test-only trigger bypass enables");
    sqlx::query(
        "UPDATE backup_snapshot_content_pins
         SET object_id = $2, object_dedup_domain_id = $3
         WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .bind(ObjectId::new().into_uuid())
    .bind(DedupDomainId::new().into_uuid())
    .execute(&mut *injection)
    .await
    .expect("wrong Object/domain corruption persists");
    injection.commit().await.expect("injection commits");
    assert_eq!(
        create_plan(&fixture, set_id, snapshot_id, "prune-wrong-object-domain").await,
        Err(BackupError::PrunePreflight(
            synveil_core::BackupPrunePreflightIssue::RetentionCorrupt,
        ))
    );

    // A target snapshot with no owned pin while another snapshot retains the
    // same Object cannot borrow that foreign pin; owner-local pin count fails.
    let (foreign_set, foreign_snapshot) = create_snapshot(&fixture, "foreign-pin-owner").await;
    let (target_set, target_snapshot) = create_snapshot(&fixture, "missing-target-pin").await;
    expire_snapshot(&fixture, target_snapshot).await;
    let mut injection = fixture.inspection.begin().await.expect("injection begins");
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *injection)
        .await
        .expect("test-only trigger bypass enables");
    sqlx::query("DELETE FROM backup_snapshot_content_pins WHERE snapshot_id = $1")
        .bind(target_snapshot.into_uuid())
        .execute(&mut *injection)
        .await
        .expect("target pin removal corruption persists");
    injection.commit().await.expect("injection commits");
    assert_eq!(
        create_plan(
            &fixture,
            target_set,
            target_snapshot,
            "prune-missing-target-foreign-pin",
        )
        .await,
        Err(BackupError::PrunePreflight(
            synveil_core::BackupPrunePreflightIssue::RetentionCorrupt,
        ))
    );
    let foreign_pin_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(foreign_snapshot.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("foreign pin must remain present");
    assert!(foreign_pin_count > 0);
    let _ = foreign_set;

    let (set_id, snapshot_id) = create_snapshot(&fixture, "missing-object").await;
    expire_snapshot(&fixture, snapshot_id).await;
    let mut injection = fixture.inspection.begin().await.expect("injection begins");
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *injection)
        .await
        .expect("test-only trigger bypass enables");
    sqlx::query("DELETE FROM objects WHERE id = $1 AND dedup_domain_id = $2")
        .bind(fixture.object.object_id().into_uuid())
        .bind(fixture.object.dedup_domain_id().into_uuid())
        .execute(&mut *injection)
        .await
        .expect("missing canonical Object corruption persists");
    injection.commit().await.expect("injection commits");
    assert_eq!(
        create_plan(
            &fixture,
            set_id,
            snapshot_id,
            "prune-missing-canonical-object"
        )
        .await,
        Err(BackupError::PrunePreflight(
            synveil_core::BackupPrunePreflightIssue::RetentionCorrupt,
        ))
    );
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_plan_marks_corrupt_persisted_evidence_stale_without_rewriting_basis() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "corrupt-plan-evidence").await;
    expire_snapshot(&fixture, snapshot_id).await;
    let plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "prune-corrupt-persisted-evidence",
    )
    .await
    .expect("base plan must seal");
    let source_before = source_evidence(&fixture.inspection, snapshot_id).await;
    let original_counts = (
        plan.planned_pin_release_count(),
        plan.distinct_retained_content_count(),
        plan.retained_after_release_count(),
        plan.would_become_unreferenced_count(),
    );
    let mut injection = fixture.inspection.begin().await.expect("injection begins");
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *injection)
        .await
        .expect("test-only trigger bypass enables");
    sqlx::query("DELETE FROM backup_prune_plan_object_impacts WHERE plan_id = $1")
        .bind(plan.id().into_uuid())
        .execute(&mut *injection)
        .await
        .expect("test-only impact corruption persists");
    injection.commit().await.expect("injection commits");
    let stale = BackupService::new(fixture.pool.clone())
        .validate_prune_plan(fixture.user_id, plan.id())
        .await
        .expect("corrupt evidence must stale rather than recalculate the plan");
    assert_eq!(stale.state(), BackupPrunePlanState::Stale);
    assert_eq!(
        (
            stale.planned_pin_release_count(),
            stale.distinct_retained_content_count(),
            stale.retained_after_release_count(),
            stale.would_become_unreferenced_count(),
        ),
        original_counts
    );
    assert_eq!(
        source_evidence(&fixture.inspection, snapshot_id).await,
        source_before
    );
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_plan_creation_is_canonical_under_concurrency_and_reference_races() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "concurrent-same-operation").await;
    expire_snapshot(&fixture, snapshot_id).await;
    let service_a = BackupService::new(fixture.pool.clone());
    let service_b = BackupService::new(fixture.pool.clone());
    let (left, right) = tokio::join!(
        service_a.create_prune_plan(
            fixture.user_id,
            "prune-concurrent-same-op".to_owned(),
            set_id,
            snapshot_id,
        ),
        service_b.create_prune_plan(
            fixture.user_id,
            "prune-concurrent-same-op".to_owned(),
            set_id,
            snapshot_id,
        )
    );
    let left = left.expect("first same operation must resolve");
    let right = right.expect("second same operation must resolve");
    assert_eq!(left.id(), right.id());

    let (different_set, different_snapshot) =
        create_snapshot(&fixture, "concurrent-different-operations").await;
    expire_snapshot(&fixture, different_snapshot).await;
    let service_a = BackupService::new(fixture.pool.clone());
    let service_b = BackupService::new(fixture.pool.clone());
    let (left, right) = tokio::join!(
        service_a.create_prune_plan(
            fixture.user_id,
            "prune-concurrent-different-a".to_owned(),
            different_set,
            different_snapshot,
        ),
        service_b.create_prune_plan(
            fixture.user_id,
            "prune-concurrent-different-b".to_owned(),
            different_set,
            different_snapshot,
        )
    );
    match (left, right) {
        (Ok(_), Err(BackupError::PruneAlreadyPlanned))
        | (Err(BackupError::PruneAlreadyPlanned), Ok(_)) => {}
        outcome => panic!("different concurrent operations must resolve canonically: {outcome:?}"),
    }
    let active_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_prune_plans
         WHERE snapshot_id = $1 AND state = 'PLANNED'",
    )
    .bind(different_snapshot.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("active plan count must load");
    assert_eq!(active_count, 1);

    let (race_set, race_snapshot) = create_snapshot(&fixture, "reference-race").await;
    expire_snapshot(&fixture, race_snapshot).await;
    let race_service = BackupService::new(fixture.pool.clone());
    let planning = race_service.create_prune_plan(
        fixture.user_id,
        "prune-reference-race".to_owned(),
        race_set,
        race_snapshot,
    );
    let mutation = insert_live_reference(
        &fixture.pool,
        &fixture.library,
        &fixture.root,
        fixture.object,
        "racing-reference.bin",
        timestamp("2026-08-30T00:00:04.123456Z"),
    );
    let (plan, _new_reference) = tokio::join!(planning, mutation);
    let plan = plan.expect("planning must observe one coherent reference basis");
    let impacts = impact_rows(&fixture.inspection, plan.id()).await;
    assert_eq!(impacts.len(), 1);
    let (target_pins, live, other_pins, post, impact) = &impacts[0];
    assert!(*target_pins > 0);
    assert_eq!(*post, *live + *other_pins);
    assert_eq!(
        impact.as_str(),
        if *post > 0 {
            "RETAINED_BY_OTHER_REFERENCE"
        } else {
            "WOULD_BECOME_UNREFERENCED"
        }
    );
    let validation = BackupService::new(fixture.pool.clone())
        .validate_prune_plan(fixture.user_id, plan.id())
        .await
        .expect("race plan must remain safely readable");
    assert!(matches!(
        validation.state(),
        BackupPrunePlanState::Planned | BackupPrunePlanState::Stale
    ));
}

async fn prune_execution_evidence(
    pool: &PgPool,
    execution_id: synveil_core::BackupPruneExecutionId,
) -> Vec<(i64, i64, i64, i64, bool, Option<Uuid>)> {
    sqlx::query_as(
        "SELECT target_snapshot_pin_count,
                post_release_live_file_version_count,
                post_release_other_snapshot_pin_count,
                post_release_authoritative_reference_count,
                gc_candidate_created_or_reused,
                gc_candidate_id
         FROM backup_prune_execution_object_results
         WHERE execution_id = $1
         ORDER BY object_id, object_dedup_domain_id",
    )
    .bind(execution_id.into_uuid())
    .fetch_all(pool)
    .await
    .expect("private execution evidence must load")
}

/// Critical sole-authority end-to-end: S1 owns ALL pins to O, no live
/// FileVersion survives, no other snapshot pins O. Execution releases every
/// S1 pin, hands O to one canonical GC candidate, preserves snapshot/manifest
/// audit history, and leaves Object/ObjectReplica/bytes untouched.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_execution_sole_authority_releases_all_pins_and_handsoff_gc() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-sole-authority").await;
    purge_file(&fixture, fixture.file.clone()).await;
    expire_snapshot(&fixture, snapshot_id).await;
    let plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "prune-execution-sole-authority",
    )
    .await
    .expect("sole-authority snapshot must plan");
    assert_eq!(plan.would_become_unreferenced_count(), 1);

    let before_object: (i64, String, i64) = sqlx::query_as(
        "SELECT
             (SELECT count(*) FROM objects WHERE id = $1 AND dedup_domain_id = $2),
             (SELECT max(lifecycle_state) FROM objects WHERE id = $1 AND dedup_domain_id = $2),
             (SELECT count(*) FROM object_replicas
              WHERE object_id = $1 AND object_dedup_domain_id = $2)",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("pre-execution Object evidence must load");
    let before_candidate_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM object_gc_candidates")
            .fetch_one(&fixture.inspection)
            .await
            .expect("candidate count must load");

    let receipt = BackupService::new(fixture.pool.clone())
        .execute_prune_plan(fixture.user_id, plan.id())
        .await
        .expect("sole-authority prune must execute");
    assert_eq!(receipt.released_pin_count(), 1);
    assert_eq!(receipt.distinct_object_count(), 1);
    assert_eq!(receipt.retained_by_other_reference_count(), 0);
    assert_eq!(receipt.gc_handoff_object_count(), 1);

    let pin_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("post-release pin count must load");
    assert_eq!(pin_count, 0);
    assert_eq!(plan.state(), BackupPrunePlanState::Planned);
    let plan_state: String =
        sqlx::query_scalar("SELECT state FROM backup_prune_plans WHERE id = $1")
            .bind(plan.id().into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .expect("plan state must load");
    assert_eq!(plan_state, "EXECUTED");

    let candidates: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT candidate.object_id, candidate.state
         FROM object_gc_candidates AS candidate
         WHERE candidate.object_id = $1
           AND candidate.object_dedup_domain_id = $2
           AND candidate.source = 'METADATA_PURGE'",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .fetch_all(&fixture.inspection)
    .await
    .expect("canonical GC candidate must exist");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].1, "ELIGIBLE");

    let after_object: (i64, String, i64) = sqlx::query_as(
        "SELECT
             (SELECT count(*) FROM objects WHERE id = $1 AND dedup_domain_id = $2),
             (SELECT max(lifecycle_state) FROM objects WHERE id = $1 AND dedup_domain_id = $2),
             (SELECT count(*) FROM object_replicas
              WHERE object_id = $1 AND object_dedup_domain_id = $2)",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("post-execution Object evidence must load");
    assert_eq!(
        after_object, before_object,
        "Object metadata must remain unchanged"
    );
    let after_candidate_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM object_gc_candidates")
            .fetch_one(&fixture.inspection)
            .await
            .expect("candidate count must reload");
    assert_eq!(
        after_candidate_count,
        before_candidate_count + 1,
        "exactly one new canonical candidate"
    );

    let snapshot_row: (String, i64, i64) = sqlx::query_as(
        "SELECT state,
                (SELECT count(*) FROM backup_snapshot_nodes WHERE snapshot_id = $1),
                content_reference_count
         FROM backup_snapshots
         WHERE id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("snapshot audit history must load");
    assert_eq!(snapshot_row.0, "EXPIRED");
    assert!(snapshot_row.1 > 0, "manifest rows remain");
    assert!(
        snapshot_row.2 > 0,
        "historical content_reference_count remains"
    );

    let evidence = prune_execution_evidence(&fixture.inspection, receipt.id()).await;
    assert_eq!(
        evidence,
        vec![(
            1,
            0,
            0,
            0,
            true,
            Some(fixture.object.object_id().into_uuid())
        )]
    );
    let stored = BackupService::new(fixture.pool.clone())
        .get_prune_execution(fixture.user_id, receipt.id())
        .await
        .expect("canonical receipt must be readable");
    assert_eq!(stored, receipt);
}

/// Shared-object: S1 owns two pins -> O, S2 owns one pin -> O. Execution
/// releases BOTH S1 pins together, leaves the S2 pin untouched, and O remains
/// retained (post-release count 1), so no GC handoff occurs.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_execution_multi_pin_same_object_releases_all_and_keeps_other() {
    let fixture = fixture().await;
    let (_set_s2, s2) = create_snapshot(&fixture, "execution-other-snapshot").await;
    let (second_file, _second_version) = insert_live_reference(
        &fixture.pool,
        &fixture.library,
        &fixture.root,
        fixture.object,
        "execution-second-same-object.bin",
        timestamp("2026-08-30T00:00:02.123456Z"),
    )
    .await;
    let (set_s1, s1) = create_snapshot(&fixture, "execution-multi-target-pins").await;
    purge_file(&fixture, fixture.file.clone()).await;
    purge_file(&fixture, second_file).await;
    expire_snapshot(&fixture, s1).await;
    let plan = create_plan(&fixture, set_s1, s1, "prune-execution-multi-target-pins")
        .await
        .expect("multi-pin source must plan");
    assert_eq!(plan.planned_pin_release_count(), 2);
    assert_eq!(plan.distinct_retained_content_count(), 1);
    assert_eq!(plan.retained_after_release_count(), 1);

    let s2_pins_before: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(s2.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("other snapshot pin count must load");

    let receipt = BackupService::new(fixture.pool.clone())
        .execute_prune_plan(fixture.user_id, plan.id())
        .await
        .expect("multi-pin execution must succeed");
    assert_eq!(receipt.released_pin_count(), 2);
    assert_eq!(receipt.distinct_object_count(), 1);
    assert_eq!(receipt.retained_by_other_reference_count(), 1);
    assert_eq!(receipt.gc_handoff_object_count(), 0);

    let (s1_after, s2_after): (i64, i64) = sqlx::query_as(
        "SELECT
            (SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1),
            (SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $2)",
    )
    .bind(s1.into_uuid())
    .bind(s2.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("post-execution pin counts must load");
    assert_eq!((s1_after, s2_after), (0, s2_pins_before));
    let object_candidate_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_gc_candidates
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("candidate count must load");
    assert_eq!(object_candidate_count, 0, "O stays backup-retained");
    let evidence = prune_execution_evidence(&fixture.inspection, receipt.id()).await;
    assert_eq!(evidence, vec![(2, 0, 1, 1, false, None)]);
}

/// Live FileVersion survives release: S1 pins O and a live FileVersion also
/// references O. Execution releases S1 pins but creates no new GC candidate.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_execution_live_file_version_keeps_object_referenced() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-live-reference").await;
    expire_snapshot(&fixture, snapshot_id).await;
    let plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "prune-execution-live-reference",
    )
    .await
    .expect("live-reference plan must seal");
    assert_eq!(plan.retained_after_release_count(), 1);

    let receipt = BackupService::new(fixture.pool.clone())
        .execute_prune_plan(fixture.user_id, plan.id())
        .await
        .expect("live-reference execution must succeed");
    assert_eq!(receipt.released_pin_count(), 1);
    assert_eq!(receipt.gc_handoff_object_count(), 0);
    let candidate_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_gc_candidates
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("candidate count must load");
    assert_eq!(
        candidate_count, 0,
        "live FileVersion keeps Object referenced"
    );
    let live_versions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM file_versions
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("live FileVersion count must load");
    assert!(live_versions > 0);
}

/// Prompt 44 restored FileVersion survives release: the restore-created
/// destination FileVersion remains an ordinary canonical live reference.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_execution_restored_file_version_survives_release() {
    let fixture = fixture().await;
    insert_verified_replica(&fixture.inspection, fixture.object).await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-restored-reference").await;
    let backup = BackupService::new(fixture.pool.clone());
    let restore_plan = backup
        .create_restore_plan(
            fixture.user_id,
            "restore-for-prune-execution-0001".to_owned(),
            set_id,
            snapshot_id,
            fixture.library.id(),
            fixture.root.id(),
            name("execution-restored-source"),
        )
        .await
        .expect("restore planning must succeed");
    let restore_receipt = backup
        .execute_restore_plan(fixture.user_id, restore_plan.id())
        .await
        .expect("restore execution must create destination FileVersions");
    assert_eq!(restore_receipt.created_file_version_count(), 1);
    purge_file(&fixture, fixture.file.clone()).await;
    expire_snapshot(&fixture, snapshot_id).await;
    let plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "prune-execution-restored-reference",
    )
    .await
    .expect("expired restored source must plan");
    assert_eq!(plan.retained_after_release_count(), 1);

    let receipt = BackupService::new(fixture.pool.clone())
        .execute_prune_plan(fixture.user_id, plan.id())
        .await
        .expect("restored-reference execution must succeed");
    assert_eq!(receipt.released_pin_count(), 1);
    assert_eq!(receipt.gc_handoff_object_count(), 0);
    let restored_version_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM file_versions
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("restored FileVersion count must load");
    assert!(restored_version_count > 0, "Vrestore still references O");
    let evidence = prune_execution_evidence(&fixture.inspection, receipt.id()).await;
    assert_eq!(evidence, vec![(1, 1, 0, 1, false, None)]);
    let restore_history: (String, i64) = sqlx::query_as(
        "SELECT plan.state,
                (SELECT count(*) FROM backup_restore_executions
                 WHERE restore_plan_id = plan.id)
         FROM backup_restore_plans AS plan WHERE plan.id = $1",
    )
    .bind(restore_plan.id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("restore history must remain intact");
    assert_eq!(restore_history, ("EXECUTED".to_owned(), 1));
}

/// Direct pin DELETE remains rejected even with a valid PLANNED plan and an
/// EXPIRED snapshot (Prompt 42B regression outside the authorized protocol).
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_execution_direct_delete_and_wrong_scope_still_fail() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-direct-delete").await;
    expire_snapshot(&fixture, snapshot_id).await;
    let plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "prune-execution-direct-delete",
    )
    .await
    .expect("plan must seal");
    let before = source_evidence(&fixture.inspection, snapshot_id).await;

    let direct_delete =
        sqlx::query("DELETE FROM backup_snapshot_content_pins WHERE snapshot_id = $1")
            .bind(snapshot_id.into_uuid())
            .execute(&fixture.inspection)
            .await;
    assert!(
        direct_delete.is_err(),
        "arbitrary direct pin DELETE must be rejected even with a PLANNED plan"
    );
    assert_eq!(
        source_evidence(&fixture.inspection, snapshot_id).await,
        before
    );

    // The authorized release function must reject a wrong snapshot identity.
    let (_other_set, other_snapshot) =
        create_snapshot(&fixture, "execution-wrong-snapshot-other").await;
    let wrong_snapshot =
        sqlx::query_scalar::<_, i64>("SELECT synveil_authorized_prune_pin_release($1, $2)")
            .bind(plan.id().into_uuid())
            .bind(other_snapshot.into_uuid())
            .fetch_one(&fixture.inspection)
            .await;
    assert!(
        wrong_snapshot.is_err(),
        "authorized function must reject a wrong snapshot"
    );
    assert_eq!(
        source_evidence(&fixture.inspection, snapshot_id).await,
        before
    );
    let other_after: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(other_snapshot.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("other snapshot pins must remain");
    assert!(other_after > 0);

    // A completed (non-expired) snapshot must also be rejected.
    let (completed_set, completed) = create_snapshot(&fixture, "execution-completed").await;
    let completed_plan = create_plan(
        &fixture,
        completed_set,
        completed,
        "prune-execution-completed",
    )
    .await;
    assert_eq!(
        completed_plan,
        Err(BackupError::PrunePreflight(
            synveil_core::BackupPrunePreflightIssue::SnapshotNotExpired,
        ))
    );
}

/// Wrong-plan / wrong-owner execution must fail closed with no release.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_execution_owner_and_plan_concealment() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-concealment").await;
    expire_snapshot(&fixture, snapshot_id).await;
    let plan = create_plan(&fixture, set_id, snapshot_id, "prune-execution-concealment")
        .await
        .expect("plan must seal");
    let foreign = BackupService::new(fixture.pool.clone())
        .execute_prune_plan(UserId::new(), plan.id())
        .await;
    assert_eq!(foreign, Err(BackupError::NotFound));
    let pin_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("pins must remain");
    assert!(pin_count > 0);
    let missing = BackupService::new(fixture.pool.clone())
        .execute_prune_plan(fixture.user_id, synveil_core::BackupPrunePlanId::new())
        .await;
    assert_eq!(missing, Err(BackupError::NotFound));
}

/// Reference drift from any class (new live version, new other-snapshot pin,
/// removed other retention) commits only STALE with 0 release / 0 handoff.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_execution_drift_stales_without_release() {
    let f1 = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&f1, "execution-live-appears").await;
    purge_file(&f1, f1.file.clone()).await;
    expire_snapshot(&f1, snapshot_id).await;
    let plan = create_plan(&f1, set_id, snapshot_id, "prune-execution-live-appears")
        .await
        .expect("sole-authority plan must seal");
    assert_eq!(plan.would_become_unreferenced_count(), 1);
    let _new_live = insert_live_reference(
        &f1.pool,
        &f1.library,
        &f1.root,
        f1.object,
        "execution-live-appeared.bin",
        timestamp("2026-08-30T00:00:05.123456Z"),
    )
    .await;
    let result = BackupService::new(f1.pool.clone())
        .execute_prune_plan(f1.user_id, plan.id())
        .await;
    assert_eq!(
        result,
        Err(BackupError::PruneExecutionPreflight(
            BackupPruneExecutionPreflightIssue::PlanReferenceDrift
        ))
    );
    let plan_state: String =
        sqlx::query_scalar("SELECT state FROM backup_prune_plans WHERE id = $1")
            .bind(plan.id().into_uuid())
            .fetch_one(&f1.inspection)
            .await
            .expect("plan state must load");
    assert_eq!(plan_state, "STALE");
    let pin_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(&f1.inspection)
    .await
    .expect("pin count must load");
    assert_eq!(pin_count, 1, "0 pins released on drift");

    // New other-snapshot pin appears after planning.
    let f2 = fixture().await;
    // Create snapshot S1 for f2's Object. The file is STILL LIVE here, so S1 pins O.
    let (other_set, other_snapshot) = create_snapshot(&f2, "execution-other-pin-appears").await;
    // Expire S1 before creating the extra snapshot.
    expire_snapshot(&f2, other_snapshot).await;
    let other_plan = create_plan(
        &f2,
        other_set,
        other_snapshot,
        "prune-execution-other-pin-appears",
    )
    .await
    .expect("base plan must seal");
    // S2 captures the same still-live Object, creating an other-snapshot pin.
    let (_extra_set, extra_snapshot) = create_snapshot(&f2, "execution-other-after-plan").await;
    let extra_pin_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(extra_snapshot.into_uuid())
    .fetch_one(&f2.inspection)
    .await
    .expect("extra snapshot pin count must load");
    assert!(
        extra_pin_count > 0,
        "S2 must pin the same Object to trigger drift"
    );
    let drift_result = BackupService::new(f2.pool.clone())
        .execute_prune_plan(f2.user_id, other_plan.id())
        .await;
    assert_eq!(
        drift_result,
        Err(BackupError::PruneExecutionPreflight(
            BackupPruneExecutionPreflightIssue::PlanReferenceDrift
        ))
    );
    let drift_state: String =
        sqlx::query_scalar("SELECT state FROM backup_prune_plans WHERE id = $1")
            .bind(other_plan.id().into_uuid())
            .fetch_one(&f2.inspection)
            .await
            .expect("drift plan state must load");
    assert_eq!(drift_state, "STALE");
}

/// Structural corruption fails closed: a broken target-snapshot pin mapping
/// releases 0 pins, creates 0 handoffs, and commits 0 receipts.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_execution_corruption_fails_closed_without_release() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-pin-corrupt").await;
    expire_snapshot(&fixture, snapshot_id).await;
    let plan = create_plan(&fixture, set_id, snapshot_id, "prune-execution-pin-corrupt")
        .await
        .expect("plan must seal");
    let before = source_evidence(&fixture.inspection, snapshot_id).await;
    let mut injection = fixture.inspection.begin().await.expect("injection begins");
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *injection)
        .await
        .expect("test-only trigger bypass enables");
    sqlx::query(
        "UPDATE backup_snapshot_content_pins
         SET file_version_id = $2 WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .bind(FileVersionId::new().into_uuid())
    .execute(&mut *injection)
    .await
    .expect("wrong FileVersion corruption persists");
    injection.commit().await.expect("injection commits");

    let result = BackupService::new(fixture.pool.clone())
        .execute_prune_plan(fixture.user_id, plan.id())
        .await;
    assert!(
        matches!(
            result,
            Err(BackupError::PruneExecutionPreflight(
                BackupPruneExecutionPreflightIssue::PlanReferenceDrift
                    | BackupPruneExecutionPreflightIssue::PlanCorruption
            )),
        ),
        "corrupt pin mapping must fail closed: {result:?}"
    );
    assert_eq!(
        source_evidence(&fixture.inspection, snapshot_id).await,
        before,
        "0 pins released under corruption"
    );
    let receipts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM backup_prune_executions WHERE prune_plan_id = $1")
            .bind(plan.id().into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .expect("receipt count must load");
    assert_eq!(receipts, 0);
    let candidates: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_gc_candidates WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("candidate count must load");
    assert_eq!(candidates, 0);
}

/// Transactionality: a failure injected after real pin release AND after a GC
/// handoff mutation rolls back everything; a clean retry then succeeds exactly
/// once with the pins restored between attempts.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_execution_rollback_after_release_and_handoff_then_retry_succeeds() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-rollback").await;
    purge_file(&fixture, fixture.file.clone()).await;
    expire_snapshot(&fixture, snapshot_id).await;
    let plan = create_plan(&fixture, set_id, snapshot_id, "prune-execution-rollback")
        .await
        .expect("plan must seal");
    let before_pins: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("pre-rollback pin count must load");
    assert_eq!(before_pins, 1);
    let before_candidates: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_gc_candidates WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("pre-rollback candidate count must load");
    assert_eq!(before_candidates, 0);

    // Fail on the GC candidate handoff insert: the pin release has already
    // happened inside the same transaction when this trigger fires.
    sqlx::query(
        "CREATE FUNCTION synveil_test_prune_execution_fail_handoff()
         RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             IF NEW.source = 'METADATA_PURGE' THEN
                 RAISE EXCEPTION 'injected prune execution gc handoff failure';
             END IF;
             RETURN NEW;
         END;
         $$",
    )
    .execute(&fixture.inspection)
    .await
    .expect("handoff failure function must persist");
    sqlx::query(
        "CREATE TRIGGER synveil_test_prune_execution_fail_handoff_trigger
         BEFORE INSERT ON object_gc_candidates
         FOR EACH ROW EXECUTE FUNCTION synveil_test_prune_execution_fail_handoff()",
    )
    .execute(&fixture.inspection)
    .await
    .expect("handoff failure trigger must persist");

    let failed = BackupService::new(fixture.pool.clone())
        .execute_prune_plan(fixture.user_id, plan.id())
        .await;
    assert!(
        failed.is_err(),
        "injected handoff failure must abort execution"
    );

    sqlx::query(
        "DROP TRIGGER synveil_test_prune_execution_fail_handoff_trigger ON object_gc_candidates",
    )
    .execute(&fixture.inspection)
    .await
    .expect("handoff failure trigger must drop");
    sqlx::query("DROP FUNCTION synveil_test_prune_execution_fail_handoff()")
        .execute(&fixture.inspection)
        .await
        .expect("handoff failure function must drop");

    let after_pins: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("post-rollback pin count must load");
    assert_eq!(
        after_pins, before_pins,
        "all original pins restored by rollback"
    );
    let after_candidates: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_gc_candidates WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("post-rollback candidate count must load");
    assert_eq!(after_candidates, before_candidates);
    let after_plan: String =
        sqlx::query_scalar("SELECT state FROM backup_prune_plans WHERE id = $1")
            .bind(plan.id().into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .expect("plan state must load");
    assert_eq!(after_plan, "PLANNED", "plan remains retryable");
    let after_receipts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM backup_prune_executions WHERE prune_plan_id = $1")
            .bind(plan.id().into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .expect("receipt count must load");
    assert_eq!(after_receipts, 0);

    let retry = BackupService::new(fixture.pool.clone())
        .execute_prune_plan(fixture.user_id, plan.id())
        .await
        .expect("clean retry after rollback must succeed exactly once");
    assert_eq!(retry.released_pin_count(), 1);
    assert_eq!(retry.gc_handoff_object_count(), 1);
}

/// Zero-content (directory-only) EXPIRED snapshots execute canonically with
/// 0/0/0 counts and a durable receipt.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_execution_zero_content_snapshot_executes() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) =
        create_directory_only_expired_snapshot(&fixture, "directory-exec").await;
    let plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "prune-execution-directory-only",
    )
    .await
    .expect("zero-content expired snapshot must plan");
    assert_eq!(plan.planned_pin_release_count(), 0);
    let receipt = BackupService::new(fixture.pool.clone())
        .execute_prune_plan(fixture.user_id, plan.id())
        .await
        .expect("zero-content execution must succeed canonically");
    assert_eq!(receipt.released_pin_count(), 0);
    assert_eq!(receipt.distinct_object_count(), 0);
    assert_eq!(receipt.gc_handoff_object_count(), 0);
    let plan_state: String =
        sqlx::query_scalar("SELECT state FROM backup_prune_plans WHERE id = $1")
            .bind(plan.id().into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .expect("plan state must load");
    assert_eq!(plan_state, "EXECUTED");
    let snapshot_state: String =
        sqlx::query_scalar("SELECT state FROM backup_snapshots WHERE id = $1")
            .bind(snapshot_id.into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .expect("snapshot state must load");
    assert_eq!(snapshot_state, "EXPIRED");
}

/// Lost-response replay: executing the same already-EXECUTED plan returns the
/// same canonical receipt with no additional releases or duplicate handoffs.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_execution_lost_response_replay_is_idempotent() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-replay").await;
    purge_file(&fixture, fixture.file.clone()).await;
    expire_snapshot(&fixture, snapshot_id).await;
    let plan = create_plan(&fixture, set_id, snapshot_id, "prune-execution-replay")
        .await
        .expect("plan must seal");
    let first = BackupService::new(fixture.pool.clone())
        .execute_prune_plan(fixture.user_id, plan.id())
        .await
        .expect("first execution must succeed");
    let pin_after_first: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("post-first pin count must load");
    assert_eq!(pin_after_first, 0);
    let candidate_after_first: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_gc_candidates WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("post-first candidate count must load");
    assert_eq!(candidate_after_first, 1);

    let replay = BackupService::new(fixture.pool.clone())
        .execute_prune_plan(fixture.user_id, plan.id())
        .await
        .expect("lost-response replay must resolve to canonical receipt");
    assert_eq!(replay.id(), first.id());
    assert_eq!(replay.released_pin_count(), first.released_pin_count());
    assert_eq!(
        replay.gc_handoff_object_count(),
        first.gc_handoff_object_count()
    );
    let executions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM backup_prune_executions WHERE prune_plan_id = $1")
            .bind(plan.id().into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .expect("execution count must load");
    assert_eq!(executions, 1);
    let pin_after_replay: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("post-replay pin count must load");
    assert_eq!(pin_after_replay, 0);
    let candidate_after_replay: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_gc_candidates WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("post-replay candidate count must load");
    assert_eq!(candidate_after_replay, 1, "no duplicate semantic handoff");
}

/// Concurrent duplicate execution of the same PLANNED plan produces exactly
/// one semantic execution, one receipt, and one release; both callers resolve.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_execution_concurrent_duplicates_resolve_to_one_execution() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-concurrent").await;
    purge_file(&fixture, fixture.file.clone()).await;
    expire_snapshot(&fixture, snapshot_id).await;
    let plan = create_plan(&fixture, set_id, snapshot_id, "prune-execution-concurrent")
        .await
        .expect("plan must seal");
    let service_a = BackupService::new(fixture.pool.clone());
    let service_b = BackupService::new(fixture.pool.clone());
    let (left, right) = tokio::join!(
        service_a.execute_prune_plan(fixture.user_id, plan.id()),
        service_b.execute_prune_plan(fixture.user_id, plan.id()),
    );
    let left = left.expect("first concurrent caller must resolve");
    let right = right.expect("second concurrent caller must resolve");
    assert_eq!(
        left.id(),
        right.id(),
        "both callers resolve to one canonical receipt"
    );
    let executions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM backup_prune_executions WHERE prune_plan_id = $1")
            .bind(plan.id().into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .expect("execution count must load");
    assert_eq!(executions, 1);
    let pins: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("pin count must load");
    assert_eq!(pins, 0);
    let plan_state: String =
        sqlx::query_scalar("SELECT state FROM backup_prune_plans WHERE id = $1")
            .bind(plan.id().into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .expect("plan state must load");
    assert_eq!(plan_state, "EXECUTED");
    let candidates: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_gc_candidates WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("candidate count must load");
    assert_eq!(candidates, 1, "exactly one canonical GC handoff");
}

/// Existing stale GC candidate is reused, never duplicated.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_execution_reuses_existing_gc_candidate_without_duplication() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-existing-candidate").await;
    purge_file(&fixture, fixture.file.clone()).await;
    expire_snapshot(&fixture, snapshot_id).await;
    let plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "prune-execution-existing-candidate",
    )
    .await
    .expect("plan must seal");
    sqlx::query(
        "INSERT INTO object_gc_candidates
            (object_id, object_dedup_domain_id, unreferenced_at, source)
         VALUES ($1, $2, clock_timestamp() - INTERVAL '60 seconds', 'METADATA_PURGE')",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("blocked existing candidate must persist");

    let receipt = BackupService::new(fixture.pool.clone())
        .execute_prune_plan(fixture.user_id, plan.id())
        .await
        .expect("execution must reuse the existing candidate");
    assert_eq!(receipt.gc_handoff_object_count(), 1);
    let candidates: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_gc_candidates WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("candidate count must load");
    assert_eq!(candidates, 1, "existing candidate is not duplicated");
}

/// Post-prune planning for an already-pruned snapshot is rejected explicitly
/// with SnapshotAlreadyPruned, not with generic corruption errors.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_execution_rejects_new_plan_for_already_pruned_snapshot() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-already-pruned").await;
    purge_file(&fixture, fixture.file.clone()).await;
    expire_snapshot(&fixture, snapshot_id).await;
    let plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "prune-execution-already-pruned",
    )
    .await
    .expect("plan must seal");
    BackupService::new(fixture.pool.clone())
        .execute_prune_plan(fixture.user_id, plan.id())
        .await
        .expect("execution must succeed");
    assert_eq!(
        create_plan(
            &fixture,
            set_id,
            snapshot_id,
            "prune-execution-already-pruned-replan",
        )
        .await,
        Err(BackupError::SnapshotAlreadyPruned)
    );
}

/// The GC final reference fence remains authoritative: after prune handoff, a
/// new surviving reference prevents physical deletion via the canonical GC
/// planning path.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_execution_gc_final_fence_still_blocks_new_reference() {
    use synveil_core::ObjectGcPolicy;
    let fixture = fixture().await;
    // Serial test isolation on a shared disposable database: remove candidates
    // created by earlier tests so the planner can only see this test's one.
    sqlx::query("DELETE FROM object_gc_candidates")
        .execute(&fixture.inspection)
        .await
        .expect("foreign candidate cleanup must succeed");
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-gc-fence").await;
    purge_file(&fixture, fixture.file.clone()).await;
    expire_snapshot(&fixture, snapshot_id).await;
    let plan = create_plan(&fixture, set_id, snapshot_id, "prune-execution-gc-fence")
        .await
        .expect("plan must seal");
    BackupService::new(fixture.pool.clone())
        .execute_prune_plan(fixture.user_id, plan.id())
        .await
        .expect("execution must hand off the candidate");

    // Age the handoff candidate past the grace period so the canonical GC
    // planner actually inspects it (mirroring the Prompt 42B stale-candidate
    // fixture pattern) instead of skipping a too-young candidate.
    sqlx::query(
        "UPDATE object_gc_candidates
         SET unreferenced_at = clock_timestamp() - INTERVAL '60 seconds'
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("candidate must become mature for planner inspection");

    // A new authoritative reference appears after prune (e.g. a restore or
    // upload). The canonical GC planner must then invalidate/clear the
    // candidate instead of authorizing physical deletion.
    let _survivor = insert_live_reference(
        &fixture.pool,
        &fixture.library,
        &fixture.root,
        fixture.object,
        "gc-fence-survivor.bin",
        timestamp("2026-08-30T00:00:06.123456Z"),
    )
    .await;
    let policy = ObjectGcPolicy::new(Duration::from_secs(1), Duration::from_secs(30), 1)
        .expect("focused GC policy is valid");
    let planner = ObjectGcPlanningService::new(fixture.pool.clone(), policy);
    let leases = planner
        .claim_candidates(1)
        .await
        .expect("planner must revalidate against the new reference");
    assert!(
        leases.is_empty(),
        "the surviving reference must block a destructive GC lease"
    );
    let candidates: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_gc_candidates WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("candidate count must load");
    assert_eq!(candidates, 0, "stale candidate was cleared, not executed");
    let object_state: String = sqlx::query_scalar(
        "SELECT lifecycle_state FROM objects WHERE id = $1 AND dedup_domain_id = $2",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("Object lifecycle must load");
    assert_eq!(object_state, "AVAILABLE", "no physical deletion authorized");
}

/// Execution receipt and plan-state immutability after EXECUTED.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_execution_receipts_and_executed_plans_are_immutable() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-immutability").await;
    purge_file(&fixture, fixture.file.clone()).await;
    expire_snapshot(&fixture, snapshot_id).await;
    let plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "prune-execution-immutability",
    )
    .await
    .expect("plan must seal");
    let receipt = BackupService::new(fixture.pool.clone())
        .execute_prune_plan(fixture.user_id, plan.id())
        .await
        .expect("execution must succeed");

    let receipt_update = sqlx::query(
        "UPDATE backup_prune_executions
         SET released_pin_count = 99 WHERE id = $1",
    )
    .bind(receipt.id().into_uuid())
    .execute(&fixture.inspection)
    .await;
    assert!(
        receipt_update.is_err(),
        "execution receipt rewrite must be rejected"
    );
    let receipt_delete = sqlx::query("DELETE FROM backup_prune_executions WHERE id = $1")
        .bind(receipt.id().into_uuid())
        .execute(&fixture.inspection)
        .await;
    assert!(
        receipt_delete.is_err(),
        "execution receipt delete must be rejected"
    );
    let evidence_update = sqlx::query(
        "UPDATE backup_prune_execution_object_results
         SET post_release_authoritative_reference_count = 99 WHERE execution_id = $1",
    )
    .bind(receipt.id().into_uuid())
    .execute(&fixture.inspection)
    .await;
    assert!(
        evidence_update.is_err(),
        "per-Object evidence rewrite must be rejected"
    );
    let plan_update = sqlx::query(
        "UPDATE backup_prune_plans
         SET planned_pin_release_count = 99 WHERE id = $1",
    )
    .bind(plan.id().into_uuid())
    .execute(&fixture.inspection)
    .await;
    assert!(
        plan_update.is_err(),
        "EXECUTED plan provenance must remain immutable"
    );
    let plan_delete = sqlx::query("DELETE FROM backup_prune_plans WHERE id = $1")
        .bind(plan.id().into_uuid())
        .execute(&fixture.inspection)
        .await;
    assert!(
        plan_delete.is_err(),
        "EXECUTED plan delete must be rejected"
    );
    let stale_rewrite = sqlx::query(
        "UPDATE backup_prune_plans
         SET state = 'STALE', stale_at = CURRENT_TIMESTAMP WHERE id = $1",
    )
    .bind(plan.id().into_uuid())
    .execute(&fixture.inspection)
    .await;
    assert!(
        stale_rewrite.is_err(),
        "EXECUTED cannot be rewritten to STALE"
    );
}

/// Execution vs live-reference creation race: either outcome is coherent, and
/// no mixed accounting state is persisted.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_prune_execution_races_live_reference_creation_cleanly() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-live-race").await;
    purge_file(&fixture, fixture.file.clone()).await;
    expire_snapshot(&fixture, snapshot_id).await;
    let plan = create_plan(&fixture, set_id, snapshot_id, "prune-execution-live-race")
        .await
        .expect("sole-authority plan must seal");
    let service = BackupService::new(fixture.pool.clone());
    let executing = service.execute_prune_plan(fixture.user_id, plan.id());
    let mutating = insert_live_reference(
        &fixture.pool,
        &fixture.library,
        &fixture.root,
        fixture.object,
        "racing-execution.bin",
        timestamp("2026-08-30T00:00:07.123456Z"),
    );
    let (execution, _reference) = tokio::join!(executing, mutating);
    let pins: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("pin count must load");
    let candidates: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_gc_candidates WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(fixture.object.object_id().into_uuid())
    .bind(fixture.object.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("candidate count must load");
    assert!(
        !(pins > 0 && candidates > 0),
        "no mixed accounting state may persist"
    );
    if let Ok(receipt) = execution {
        assert_eq!(pins, 0, "execution won: pins released");
        assert!(receipt.gc_handoff_object_count() <= 1);
    } else {
        assert_eq!(pins, 1, "reference won: prune stale, pins remain");
        assert_eq!(candidates, 0);
    }
    let plan_state: String =
        sqlx::query_scalar("SELECT state FROM backup_prune_plans WHERE id = $1")
            .bind(plan.id().into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .expect("plan state must load");
    assert!(matches!(
        plan_state.as_str(),
        "PLANNED" | "EXECUTED" | "STALE"
    ));
}
