use std::time::Duration;

use sqlx::PgPool;
use synveil_core::BackupRestorePreflightIssue;
use synveil_core::{
    BackupRestorePlanState, BackupSetId, DedupDomainId, FileVersion, FileVersionId, Library,
    LibraryId, LogicalName, Node, NodeId, NodeKind, ObjectId, ObjectReference, Sha256Digest,
    SnapshotId, Timestamp, TrashRetentionPolicy, User, UserId, UserStatus,
};
use synveil_metadata::{
    BackupError, BackupService, DatabaseConfig, DatabasePool, DomainRepository,
    FileMetadataService, MigrationRunner, PurgeExecutionResult, TrashRetentionService,
};
use uuid::Uuid;

fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse(value).expect("test timestamp is valid")
}

fn name(value: &str) -> LogicalName {
    LogicalName::new(value).expect("test logical name is valid")
}

struct RestoreFixture {
    pool: DatabasePool,
    inspection: PgPool,
    user_id: UserId,
    library: Library,
    root: Node,
    directory: Node,
    file: Node,
    object_v1: ObjectReference,
    version_v1: FileVersion,
}

async fn fixture() -> RestoreFixture {
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
    assert_eq!(status.applied_versions().len(), 34);
    let inspection = PgPool::connect(&url)
        .await
        .expect("inspection connection must succeed");

    let observed_at = timestamp("2026-08-29T00:00:00.123456Z");
    let repository = DomainRepository::new(&pool);
    let user_id = UserId::new();
    repository
        .insert_user(&User::new(
            user_id,
            synveil_core::LoginIdentifier::new(
                "restore-owner",
                format!("restore-owner-{}", user_id),
            )
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
        name("Restore target"),
        &root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("fixture library is valid");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("fixture library and root must persist");

    let directory = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::Directory,
        name("reports"),
        observed_at,
    )
    .expect("fixture directory is valid");
    repository
        .insert_node(&directory)
        .await
        .expect("fixture directory must persist");
    let file = Node::new_child(
        NodeId::new(),
        library_id,
        &directory,
        NodeKind::File,
        name("annual.pdf"),
        observed_at,
    )
    .expect("fixture file is valid");
    repository
        .insert_node(&file)
        .await
        .expect("fixture file must persist");

    let object_v1 = ObjectReference::new(
        ObjectId::new(),
        library.dedup_domain_id(),
        Sha256Digest::from_bytes([0xab; 32]),
        2_048,
    );
    repository
        .insert_object(object_v1, observed_at)
        .await
        .expect("V1 Object must persist");
    let version_v1 = FileVersion::new(
        FileVersionId::new(),
        &library,
        &file,
        object_v1,
        None,
        observed_at,
    )
    .expect("V1 FileVersion is valid");
    repository
        .insert_file_version(version_v1)
        .await
        .expect("V1 FileVersion must persist");
    let file = file
        .with_current_version(&version_v1, observed_at)
        .expect("file accepts V1");
    repository
        .update_node(&file)
        .await
        .expect("file head must persist");
    insert_verified_replica(&inspection, object_v1).await;

    RestoreFixture {
        pool,
        inspection,
        user_id,
        library,
        root,
        directory,
        file,
        object_v1,
        version_v1,
    }
}

async fn new_fixture() -> RestoreFixture {
    fixture().await
}

async fn insert_verified_replica(pool: &PgPool, object: ObjectReference) {
    let key = format!("restore-replica-{}", Uuid::now_v7().simple());
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
    .bind(key)
    .bind(object.plaintext_length().to_string())
    .bind(object.canonical_hash().as_bytes().to_vec())
    .execute(pool)
    .await
    .expect("verified replica metadata must persist");
}

async fn create_snapshot(fixture: &RestoreFixture, label: &str) -> (BackupSetId, SnapshotId) {
    let set_id = BackupSetId::new();
    let backup = BackupService::new(fixture.pool.clone());
    backup
        .create_backup_set(
            fixture.user_id,
            set_id,
            name(&format!("set-{label}")),
            fixture.library.id(),
            Some(30),
            timestamp("2026-08-29T00:00:01.123456Z"),
        )
        .await
        .expect("backup set must persist");
    let snapshot_id = SnapshotId::new();
    backup
        .capture_snapshot(
            fixture.user_id,
            set_id,
            snapshot_id,
            format!("capture-{label}-{}", Uuid::now_v7().simple()),
        )
        .await
        .expect("fixture snapshot must complete");
    (set_id, snapshot_id)
}

async fn create_plan(
    fixture: &RestoreFixture,
    set_id: BackupSetId,
    snapshot_id: SnapshotId,
    operation_id: &str,
    destination: &str,
) -> Result<synveil_core::BackupRestorePlan, BackupError> {
    BackupService::new(fixture.pool.clone())
        .create_restore_plan(
            fixture.user_id,
            operation_id.to_owned(),
            set_id,
            snapshot_id,
            fixture.library.id(),
            fixture.root.id(),
            name(destination),
        )
        .await
}

async fn snapshot_evidence(
    fixture: &RestoreFixture,
    snapshot_id: SnapshotId,
) -> (String, i64, i64, i64, i64, String) {
    sqlx::query_as(
        "SELECT snapshot.state, snapshot.manifest_item_count,
                snapshot.content_reference_count,
                (SELECT count(*) FROM backup_snapshot_nodes
                 WHERE snapshot_id = $1),
                (SELECT count(*) FROM backup_snapshot_content_pins
                 WHERE snapshot_id = $1),
                object.lifecycle_state
         FROM backup_snapshots AS snapshot
         INNER JOIN backup_snapshot_content_pins AS pin
           ON pin.snapshot_id = snapshot.id
         INNER JOIN objects AS object
           ON object.id = pin.object_id
          AND object.dedup_domain_id = pin.object_dedup_domain_id
         WHERE snapshot.id = $1
         LIMIT 1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("snapshot evidence must load")
}

async fn target_evidence(fixture: &RestoreFixture) -> (i64, i64, i64, i64, i64, i64) {
    sqlx::query_as(
        "SELECT
            (SELECT count(*) FROM nodes WHERE library_id = $1),
            (SELECT count(*) FROM file_versions WHERE library_id = $1),
            (SELECT journal_epoch FROM libraries WHERE id = $1),
            (SELECT sync_head FROM libraries WHERE id = $1),
            (SELECT count(*) FROM change_journal WHERE library_id = $1),
            (SELECT count(*) FROM objects WHERE dedup_domain_id = $2)",
    )
    .bind(fixture.library.id().into_uuid())
    .bind(fixture.library.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("target evidence must load")
}

async fn plan_counts(fixture: &RestoreFixture, owner_user_id: UserId) -> (i64, i64) {
    sqlx::query_as(
        "SELECT count(*)::BIGINT,
                (SELECT count(*) FROM backup_restore_plan_entries AS entry
                 INNER JOIN backup_restore_plans AS plan ON plan.id = entry.plan_id
                 WHERE plan.owner_user_id = $1)
         FROM backup_restore_plans
         WHERE owner_user_id = $1",
    )
    .bind(owner_user_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("restore plan counts must load")
}

fn assert_preflight(
    result: Result<synveil_core::BackupRestorePlan, BackupError>,
    issue: BackupRestorePreflightIssue,
) {
    assert!(
        matches!(result, Err(BackupError::RestorePreflight(actual)) if actual == issue),
        "unexpected restore preflight result: {result:?}"
    );
}

async fn insert_ineligible_snapshot(
    fixture: &RestoreFixture,
    set_id: BackupSetId,
    state: &str,
    ordinal: u32,
) -> SnapshotId {
    let snapshot_id = SnapshotId::new();
    let operation_id = format!("ineligible-{state}-{ordinal}-{}", Uuid::now_v7().simple());
    let shape = if state == "EXPIRED" {
        "CURRENT_TIMESTAMP, CURRENT_TIMESTAMP"
    } else {
        "NULL, NULL"
    };
    sqlx::query(&format!(
        "INSERT INTO backup_snapshots
            (id, backup_set_id, owner_user_id, source_library_id, operation_id,
             snapshot_epoch, snapshot_resume_sequence, manifest_item_count,
             terminal_node_id, content_reference_count, state, created_at,
             committed_at, expired_at)
         VALUES ($1, $2, $3, $4, $5, $6, 0, 0, NULL, 0, $7,
                 CURRENT_TIMESTAMP, {shape})"
    ))
    .bind(snapshot_id.into_uuid())
    .bind(set_id.into_uuid())
    .bind(fixture.user_id.into_uuid())
    .bind(fixture.library.id().into_uuid())
    .bind(operation_id)
    .bind(i64::from(1_000 + ordinal))
    .bind(state)
    .execute(&fixture.inspection)
    .await
    .expect("ineligible snapshot fixture must persist");
    snapshot_id
}

/// A successful plan records only logical provenance, is retry-idempotent,
/// and leaves the source retention graph and target namespace untouched.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_backup_restore_plan_is_durable_non_destructive_and_idempotent() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "durable").await;
    let backup = BackupService::new(fixture.pool.clone());
    let source_before = snapshot_evidence(&fixture, snapshot_id).await;
    let target_before = target_evidence(&fixture).await;
    let counts_before = plan_counts(&fixture, fixture.user_id).await;

    let first = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-plan-durable-0001",
        "Recovered 2026-08-29",
    )
    .await
    .expect("restore plan must succeed");
    assert_eq!(first.state(), BackupRestorePlanState::Planned);
    assert_eq!(first.item_count(), 3);
    assert_eq!(first.content_item_count(), 1);
    assert_eq!(first.base_journal_epoch().get(), 1);
    assert_eq!(first.base_journal_head().get(), 0);

    let (entries, has_more) = backup
        .list_restore_plan_entries(fixture.user_id, first.id(), None, 100)
        .await
        .expect("plan entries must load");
    assert!(!has_more);
    assert_eq!(entries.len(), 3);
    assert!(entries[0].is_wrapper());
    assert_eq!(entries[0].planned_parent_node_id(), fixture.root.id());
    assert_eq!(entries[0].name().as_str(), "Recovered 2026-08-29");
    let file_entry = entries
        .iter()
        .find(|entry| entry.kind() == NodeKind::File)
        .expect("file entry must exist");
    let content = file_entry
        .content()
        .expect("file entry has logical content");
    assert_eq!(content.file_version_id(), fixture.version_v1.id());
    assert_eq!(content.byte_length(), 2_048);
    assert_eq!(content.sha256(), fixture.object_v1.canonical_hash());
    assert!(
        !format!("{file_entry:?}").contains("object_id"),
        "logical plan debug representation must not expose Object identity"
    );

    assert_eq!(
        snapshot_evidence(&fixture, snapshot_id).await,
        source_before
    );
    assert_eq!(target_evidence(&fixture).await, target_before);
    assert_eq!(
        plan_counts(&fixture, fixture.user_id).await,
        (counts_before.0 + 1, counts_before.1 + 3)
    );

    let replay = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-plan-durable-0001",
        "Recovered 2026-08-29",
    )
    .await
    .expect("same restore operation must replay canonically");
    assert_eq!(replay.id(), first.id());
    let (replayed_entries, _) = backup
        .list_restore_plan_entries(fixture.user_id, replay.id(), None, 100)
        .await
        .expect("replayed plan entries must load");
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.planned_node_id())
            .collect::<Vec<_>>(),
        replayed_entries
            .iter()
            .map(|entry| entry.planned_node_id())
            .collect::<Vec<_>>()
    );
    assert_eq!(plan_counts(&fixture, fixture.user_id).await, (1, 3));

    let conflict = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-plan-durable-0001",
        "Different destination",
    )
    .await;
    assert!(matches!(conflict, Err(BackupError::RestorePlanConflict)));
    for (target_library_id, target_parent_node_id, candidate_snapshot_id) in [
        (fixture.library.id(), fixture.directory.id(), snapshot_id),
        (LibraryId::new(), fixture.root.id(), snapshot_id),
        (fixture.library.id(), fixture.root.id(), SnapshotId::new()),
    ] {
        let conflict = BackupService::new(fixture.pool.clone())
            .create_restore_plan(
                fixture.user_id,
                "restore-plan-durable-0001".to_owned(),
                set_id,
                candidate_snapshot_id,
                target_library_id,
                target_parent_node_id,
                name("Recovered 2026-08-29"),
            )
            .await;
        assert!(
            matches!(conflict, Err(BackupError::RestorePlanConflict)),
            "every semantic request change must conflict with the operation identity"
        );
    }
    assert_eq!(plan_counts(&fixture, fixture.user_id).await, (1, 3));

    let validated = backup
        .validate_restore_plan(fixture.user_id, first.id())
        .await
        .expect("unchanged target plan must validate");
    assert_eq!(validated.state(), BackupRestorePlanState::Planned);
    assert_eq!(
        snapshot_evidence(&fixture, snapshot_id).await,
        source_before
    );
    assert_eq!(target_evidence(&fixture).await, target_before);
}

/// Owner concealment, active-directory target rules, exact-name collision
/// preflight, and source-state eligibility all fail closed.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_backup_restore_plan_enforces_scope_parent_collision_and_eligibility() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "scope").await;
    let backup = BackupService::new(fixture.pool.clone());
    let valid = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-plan-scope-valid",
        "Scope-valid",
    )
    .await
    .expect("valid plan must persist");

    assert!(matches!(
        backup.get_restore_plan(UserId::new(), valid.id()).await,
        Err(BackupError::NotFound)
    ));

    let foreign_fixture = new_fixture().await;
    let (foreign_set_id, foreign_snapshot_id) = create_snapshot(&foreign_fixture, "foreign").await;
    assert!(matches!(
        BackupService::new(fixture.pool.clone())
            .create_restore_plan(
                fixture.user_id,
                "restore-plan-foreign-target".to_owned(),
                set_id,
                snapshot_id,
                foreign_fixture.library.id(),
                foreign_fixture.root.id(),
                name("Foreign target"),
            )
            .await,
        Err(BackupError::NotFound)
    ));
    assert!(matches!(
        BackupService::new(fixture.pool.clone())
            .create_restore_plan(
                fixture.user_id,
                "restore-plan-foreign-source".to_owned(),
                foreign_set_id,
                foreign_snapshot_id,
                fixture.library.id(),
                fixture.root.id(),
                name("Foreign source"),
            )
            .await,
        Err(BackupError::NotFound)
    ));
    assert!(matches!(
        BackupService::new(fixture.pool.clone())
            .create_restore_plan(
                fixture.user_id,
                "restore-plan-foreign-parent".to_owned(),
                set_id,
                snapshot_id,
                fixture.library.id(),
                foreign_fixture.root.id(),
                name("Foreign parent"),
            )
            .await,
        Err(BackupError::NotFound)
    ));

    let file_parent_result = BackupService::new(fixture.pool.clone())
        .create_restore_plan(
            fixture.user_id,
            "restore-plan-file-parent-actual".to_owned(),
            set_id,
            snapshot_id,
            fixture.library.id(),
            fixture.file.id(),
            name("File parent actual"),
        )
        .await;
    assert_preflight(
        file_parent_result,
        BackupRestorePreflightIssue::InvalidTargetParent,
    );

    let trashed_parent = FileMetadataService::new(fixture.pool.clone())
        .create_directory(
            fixture.user_id,
            fixture.library.id(),
            Some(fixture.root.id()),
            name("trashed-parent"),
        )
        .await
        .expect("trash fixture directory must persist");
    let trashed_parent = FileMetadataService::new(fixture.pool.clone())
        .delete_node(
            fixture.user_id,
            trashed_parent.id(),
            trashed_parent.revision(),
        )
        .await
        .expect("trash fixture directory must enter Trash");
    assert_preflight(
        BackupService::new(fixture.pool.clone())
            .create_restore_plan(
                fixture.user_id,
                "restore-plan-trashed-parent".to_owned(),
                set_id,
                snapshot_id,
                fixture.library.id(),
                trashed_parent.id(),
                name("Trashed parent"),
            )
            .await,
        BackupRestorePreflightIssue::InvalidTargetParent,
    );

    FileMetadataService::new(fixture.pool.clone())
        .create_directory(
            fixture.user_id,
            fixture.library.id(),
            Some(fixture.root.id()),
            name("already-there"),
        )
        .await
        .expect("collision fixture must persist");
    assert_preflight(
        create_plan(
            &fixture,
            set_id,
            snapshot_id,
            "restore-plan-collision",
            "already-there",
        )
        .await,
        BackupRestorePreflightIssue::DestinationNameConflict,
    );

    for (ordinal, state) in [(1, "BUILDING"), (2, "FAILED"), (3, "EXPIRED")] {
        let ineligible = insert_ineligible_snapshot(&fixture, set_id, state, ordinal).await;
        assert_preflight(
            create_plan(
                &fixture,
                set_id,
                ineligible,
                &format!("restore-plan-ineligible-{state}"),
                &format!("ineligible-{state}"),
            )
            .await,
            BackupRestorePreflightIssue::SnapshotNotRestorable,
        );
    }
}

/// Target namespace evidence is conservative: a library head/epoch change or
/// a collision appearing after planning makes the durable plan STALE.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_backup_restore_plan_staleness_detects_target_changes() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "stale").await;
    let backup = BackupService::new(fixture.pool.clone());

    let head_plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-plan-stale-head",
        "head-change",
    )
    .await
    .expect("head staleness plan must persist");
    let base_head = head_plan.base_journal_head();
    assert_eq!(
        backup
            .validate_restore_plan(fixture.user_id, head_plan.id())
            .await
            .expect("fresh plan must validate")
            .state(),
        BackupRestorePlanState::Planned
    );
    FileMetadataService::new(fixture.pool.clone())
        .create_directory(
            fixture.user_id,
            fixture.library.id(),
            Some(fixture.root.id()),
            name("unrelated-target-change"),
        )
        .await
        .expect("target journal mutation must persist");
    let stale_head = backup
        .validate_restore_plan(fixture.user_id, head_plan.id())
        .await
        .expect("head change must return a stale plan");
    assert_eq!(stale_head.state(), BackupRestorePlanState::Stale);
    assert_eq!(stale_head.base_journal_head(), base_head);
    assert!(stale_head.stale_at().is_some());
    assert_eq!(
        backup
            .get_restore_plan(fixture.user_id, head_plan.id())
            .await
            .expect("stale plan must remain readable")
            .base_journal_head(),
        base_head
    );

    let collision_plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-plan-stale-collision",
        "late-collision",
    )
    .await
    .expect("collision staleness plan must persist");
    let late_node = Node::new_child(
        NodeId::new(),
        fixture.library.id(),
        &fixture.root,
        NodeKind::Directory,
        name("late-collision"),
        timestamp("2026-08-29T00:00:03.123456Z"),
    )
    .expect("late collision node is valid");
    DomainRepository::new(&fixture.pool)
        .insert_node(&late_node)
        .await
        .expect("late collision fixture must persist");
    let stale_collision = backup
        .validate_restore_plan(fixture.user_id, collision_plan.id())
        .await
        .expect("late destination collision must stale the plan");
    assert_eq!(stale_collision.state(), BackupRestorePlanState::Stale);

    let epoch_plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-plan-stale-epoch",
        "epoch-change",
    )
    .await
    .expect("epoch staleness plan must persist");
    sqlx::query("UPDATE libraries SET journal_epoch = journal_epoch + 1 WHERE id = $1")
        .bind(fixture.library.id().into_uuid())
        .execute(&fixture.inspection)
        .await
        .expect("epoch fixture mutation must persist");
    let stale_epoch = backup
        .validate_restore_plan(fixture.user_id, epoch_plan.id())
        .await
        .expect("epoch change must stale the plan");
    assert_eq!(stale_epoch.state(), BackupRestorePlanState::Stale);
    assert_eq!(
        stale_epoch.base_journal_epoch(),
        epoch_plan.base_journal_epoch()
    );
}

/// Entries use ordinal keyset pagination and the database refuses every
/// current attempt to edit or delete immutable plan provenance.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_backup_restore_plan_entries_are_immutable_and_bounded() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "entries").await;
    let backup = BackupService::new(fixture.pool.clone());
    let plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-plan-entries-0001",
        "entry-pages",
    )
    .await
    .expect("plan must persist");

    let mut after = None;
    let mut paged = Vec::new();
    loop {
        let (page, has_more) = backup
            .list_restore_plan_entries(fixture.user_id, plan.id(), after, 1)
            .await
            .expect("bounded entry page must load");
        assert_eq!(page.len(), 1);
        after = page.last().map(|entry| entry.ordinal());
        paged.extend(page);
        if !has_more {
            break;
        }
    }
    assert_eq!(paged.len(), 3);
    assert_eq!(paged[0].ordinal(), 0);
    assert_eq!(paged[1].ordinal(), 1);
    assert_eq!(paged[2].ordinal(), 2);

    let update_entry = sqlx::query(
        "UPDATE backup_restore_plan_entries
         SET name = 'mutated'
         WHERE plan_id = $1 AND ordinal = 1",
    )
    .bind(plan.id().into_uuid())
    .execute(&fixture.inspection)
    .await;
    assert!(update_entry.is_err());
    let delete_entry = sqlx::query(
        "DELETE FROM backup_restore_plan_entries
         WHERE plan_id = $1 AND ordinal = 1",
    )
    .bind(plan.id().into_uuid())
    .execute(&fixture.inspection)
    .await;
    assert!(delete_entry.is_err());
    let insert_entry = sqlx::query(
        "INSERT INTO backup_restore_plan_entries
            (plan_id, ordinal, planned_node_id, planned_parent_node_id,
             source_snapshot_node_id, source_parent_node_id, source_state,
             action, kind, name, source_revision, file_version_id,
             content_length, content_sha256)
         SELECT plan_id, 3, $2, planned_parent_node_id,
                $3, source_parent_node_id, source_state,
                action, kind, name, source_revision, file_version_id,
                content_length, content_sha256
         FROM backup_restore_plan_entries
         WHERE plan_id = $1 AND ordinal = 1",
    )
    .bind(plan.id().into_uuid())
    .bind(NodeId::new().into_uuid())
    .bind(NodeId::new().into_uuid())
    .execute(&fixture.inspection)
    .await;
    assert!(insert_entry.is_err());
    let update_plan = sqlx::query(
        "UPDATE backup_restore_plans
         SET destination_name = 'mutated'
         WHERE id = $1",
    )
    .bind(plan.id().into_uuid())
    .execute(&fixture.inspection)
    .await;
    assert!(update_plan.is_err());

    let (unchanged, has_more) = backup
        .list_restore_plan_entries(fixture.user_id, plan.id(), None, 100)
        .await
        .expect("immutable entries must remain readable");
    assert!(!has_more);
    assert_eq!(unchanged, paged);
}

/// A live V1 FileVersion can be purged after capture; planning still succeeds
/// through the immutable manifest, retention pin, Object, and verified replica.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_backup_restore_plan_uses_retention_after_live_v1_purge() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "purged-v1").await;
    let repository = DomainRepository::new(&fixture.pool);
    let file_service = FileMetadataService::new(fixture.pool.clone());
    let current = file_service
        .get_node(fixture.user_id, fixture.file.id())
        .await
        .expect("live V1 file must load");
    let object_v2 = ObjectReference::new(
        ObjectId::new(),
        fixture.library.dedup_domain_id(),
        Sha256Digest::from_bytes([0xbc; 32]),
        4_096,
    );
    repository
        .insert_object(object_v2, timestamp("2026-08-29T00:00:02.123456Z"))
        .await
        .expect("V2 Object must persist");
    let version_v2 = FileVersion::new(
        FileVersionId::new(),
        &fixture.library,
        &current,
        object_v2,
        Some(fixture.version_v1.id()),
        timestamp("2026-08-29T00:00:02.123456Z"),
    )
    .expect("V2 FileVersion is valid");
    repository
        .insert_file_version(version_v2)
        .await
        .expect("V2 FileVersion must persist");
    let advanced = current
        .with_current_version(&version_v2, timestamp("2026-08-29T00:00:02.123456Z"))
        .expect("live file accepts V2");
    repository
        .update_node(&advanced)
        .await
        .expect("V2 head must persist");
    let trashed = file_service
        .delete_node(fixture.user_id, advanced.id(), advanced.revision())
        .await
        .expect("live file must enter Trash");
    sqlx::query(
        "UPDATE nodes
         SET trashed_at = clock_timestamp() - INTERVAL '5 seconds'
         WHERE id = $1",
    )
    .bind(trashed.id().into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("test trash age must persist");
    let retention = TrashRetentionService::new(
        fixture.pool.clone(),
        TrashRetentionPolicy::new(Duration::from_secs(1)).expect("short policy is valid"),
    );
    let purging = retention
        .begin_node_purge(fixture.user_id, trashed.id(), trashed.revision())
        .await
        .expect("purge must begin through the valid lifecycle");
    assert_eq!(
        retention
            .execute_metadata_purge(fixture.user_id, purging.id(), purging.revision())
            .await
            .expect("metadata purge must complete"),
        PurgeExecutionResult::Completed
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM file_versions WHERE id = $1")
            .bind(fixture.version_v1.id().into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .expect("purged V1 count must load"),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM backup_snapshot_content_pins
             WHERE snapshot_id = $1 AND file_version_id = $2",
        )
        .bind(snapshot_id.into_uuid())
        .bind(fixture.version_v1.id().into_uuid())
        .fetch_one(&fixture.inspection)
        .await
        .expect("V1 pin count must load"),
        1
    );

    let plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-plan-purged-v1-0001",
        "Recovered V1",
    )
    .await
    .expect("retention-backed plan must succeed without live V1 metadata");
    let (entries, _) = BackupService::new(fixture.pool.clone())
        .list_restore_plan_entries(fixture.user_id, plan.id(), None, 100)
        .await
        .expect("purged V1 plan entries must load");
    let file_entry = entries
        .iter()
        .find(|entry| entry.kind() == NodeKind::File)
        .expect("purged V1 file entry must exist");
    let content = file_entry.content().expect("purged V1 content must exist");
    assert_eq!(content.file_version_id(), fixture.version_v1.id());
    assert_eq!(content.byte_length(), fixture.object_v1.plaintext_length());
    assert_eq!(content.sha256(), fixture.object_v1.canonical_hash());

    let execution = BackupService::new(fixture.pool.clone())
        .execute_restore_plan(fixture.user_id, plan.id())
        .await
        .expect("purged V1 restore must execute through retained content");
    assert_eq!(execution.created_file_version_count(), 1);
    let execution_entries = BackupService::new(fixture.pool.clone())
        .list_restore_execution_entries(fixture.user_id, execution.id())
        .await
        .expect("purged V1 execution evidence must load");
    let restored_file_version_id = execution_entries
        .iter()
        .find_map(|entry| entry.destination_file_version_id())
        .expect("purged V1 execution must create a live destination version");
    assert_ne!(restored_file_version_id, fixture.version_v1.id());
}

/// Missing retained content and a broken manifest tree fail before a plan row
/// is committed. The fixture repairs both mutations only through a tiny
/// disposable-test trigger window.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_backup_restore_plan_fails_closed_for_corrupt_retention_and_manifest() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "corruption").await;
    let original_pin = sqlx::query_as::<_, (Uuid, Uuid, Uuid, Uuid)>(
        "SELECT manifest_node_id, file_version_id, object_id, object_dedup_domain_id
         FROM backup_snapshot_content_pins
         WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("original pin must load");
    sqlx::query("ALTER TABLE backup_snapshot_content_pins DISABLE TRIGGER USER")
        .execute(&fixture.inspection)
        .await
        .expect("test-only retention corruption window must open");
    sqlx::query(
        "DELETE FROM backup_snapshot_content_pins
         WHERE snapshot_id = $1 AND manifest_node_id = $2",
    )
    .bind(snapshot_id.into_uuid())
    .bind(original_pin.0)
    .execute(&fixture.inspection)
    .await
    .expect("test-only pin removal must persist");
    sqlx::query("ALTER TABLE backup_snapshot_content_pins ENABLE TRIGGER USER")
        .execute(&fixture.inspection)
        .await
        .expect("test-only retention corruption window must close");
    assert_preflight(
        create_plan(
            &fixture,
            set_id,
            snapshot_id,
            "restore-plan-missing-pin",
            "missing-pin",
        )
        .await,
        BackupRestorePreflightIssue::MissingRetentionReference,
    );
    assert_eq!(plan_counts(&fixture, fixture.user_id).await, (0, 0));

    sqlx::query("ALTER TABLE backup_snapshot_content_pins DISABLE TRIGGER USER")
        .execute(&fixture.inspection)
        .await
        .expect("test-only retention repair window must open");
    sqlx::query(
        "INSERT INTO backup_snapshot_content_pins
            (snapshot_id, manifest_node_id, file_version_id, object_id,
             object_dedup_domain_id, created_at)
         VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP)",
    )
    .bind(snapshot_id.into_uuid())
    .bind(original_pin.0)
    .bind(original_pin.1)
    .bind(original_pin.2)
    .bind(original_pin.3)
    .execute(&fixture.inspection)
    .await
    .expect("test-only pin repair must persist");
    sqlx::query("ALTER TABLE backup_snapshot_content_pins ENABLE TRIGGER USER")
        .execute(&fixture.inspection)
        .await
        .expect("test-only retention repair window must close");

    let broken_parent = NodeId::new();
    sqlx::query("ALTER TABLE backup_snapshot_nodes DISABLE TRIGGER USER")
        .execute(&fixture.inspection)
        .await
        .expect("test-only manifest corruption window must open");
    sqlx::query(
        "UPDATE backup_snapshot_nodes
         SET parent_node_id = $3
         WHERE snapshot_id = $1 AND node_id = $2",
    )
    .bind(snapshot_id.into_uuid())
    .bind(fixture.directory.id().into_uuid())
    .bind(broken_parent.into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("test-only manifest break must persist");
    sqlx::query("ALTER TABLE backup_snapshot_nodes ENABLE TRIGGER USER")
        .execute(&fixture.inspection)
        .await
        .expect("test-only manifest corruption window must close");
    assert_preflight(
        create_plan(
            &fixture,
            set_id,
            snapshot_id,
            "restore-plan-corrupt-manifest",
            "corrupt-manifest",
        )
        .await,
        BackupRestorePreflightIssue::SnapshotCorrupt,
    );
    assert_eq!(plan_counts(&fixture, fixture.user_id).await, (0, 0));

    sqlx::query("ALTER TABLE backup_snapshot_nodes DISABLE TRIGGER USER")
        .execute(&fixture.inspection)
        .await
        .expect("test-only manifest repair window must open");
    sqlx::query(
        "UPDATE backup_snapshot_nodes
         SET parent_node_id = $3
         WHERE snapshot_id = $1 AND node_id = $2",
    )
    .bind(snapshot_id.into_uuid())
    .bind(fixture.directory.id().into_uuid())
    .bind(fixture.root.id().into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("test-only manifest repair must persist");
    sqlx::query("ALTER TABLE backup_snapshot_nodes ENABLE TRIGGER USER")
        .execute(&fixture.inspection)
        .await
        .expect("test-only manifest repair window must close");

    let repaired = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-plan-corruption-repaired",
        "repaired",
    )
    .await
    .expect("repaired isolated fixture must plan normally");
    assert_eq!(repaired.state(), BackupRestorePlanState::Planned);
}

/// A database failure after the wrapper entry is inserted rolls the plan and
/// every partial entry back; retrying after the disposable trigger is removed
/// starts from a clean operation identity.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_backup_restore_plan_partial_entry_failure_rolls_back() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "rollback").await;
    let suffix = Uuid::now_v7().simple().to_string();
    let function = format!("synveil_test_restore_plan_fail_{suffix}");
    let trigger = format!("synveil_test_restore_plan_fail_trigger_{suffix}");
    sqlx::query(&format!(
        "CREATE FUNCTION {function}() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             IF NEW.ordinal = 1 THEN
                 RAISE EXCEPTION 'injected restore plan entry failure';
             END IF;
             RETURN NEW;
         END;
         $$"
    ))
    .execute(&fixture.inspection)
    .await
    .expect("restore-plan failure function must persist");
    sqlx::query(&format!(
        "CREATE TRIGGER {trigger}
         BEFORE INSERT ON backup_restore_plan_entries
         FOR EACH ROW EXECUTE FUNCTION {function}()"
    ))
    .execute(&fixture.inspection)
    .await
    .expect("restore-plan failure trigger must persist");

    let failed = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-plan-rollback-0001",
        "rollback-destination",
    )
    .await;
    assert!(
        failed.is_err(),
        "injected entry failure must abort planning"
    );

    sqlx::query(&format!(
        "DROP TRIGGER {trigger} ON backup_restore_plan_entries"
    ))
    .execute(&fixture.inspection)
    .await
    .expect("restore-plan failure trigger must drop");
    sqlx::query(&format!("DROP FUNCTION {function}()"))
        .execute(&fixture.inspection)
        .await
        .expect("restore-plan failure function must drop");
    assert_eq!(plan_counts(&fixture, fixture.user_id).await, (0, 0));

    let retry = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-plan-rollback-0001",
        "rollback-destination",
    )
    .await
    .expect("clean retry must create the canonical plan");
    assert_eq!(retry.state(), BackupRestorePlanState::Planned);
    assert_eq!(plan_counts(&fixture, fixture.user_id).await, (1, 3));
}

/// A valid plan creates exactly its persisted subtree, reattaches retained
/// canonical content through a new live FileVersion, and returns the same
/// receipt and entry identities on replay.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_backup_restore_execution_is_atomic_and_retry_idempotent() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-success").await;
    let backup = BackupService::new(fixture.pool.clone());
    let plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-execution-success-0001",
        "Recovered execution",
    )
    .await
    .expect("execution plan must persist");
    let source_before = snapshot_evidence(&fixture, snapshot_id).await;
    let target_before = target_evidence(&fixture).await;
    let object_before = sqlx::query_as::<_, (String, String, Vec<u8>, i64)>(
        "SELECT object.lifecycle_state, object.plaintext_length::TEXT,
                object.canonical_hash,
                (SELECT count(*) FROM object_replicas AS replica
                 WHERE replica.object_id = object.id
                   AND replica.object_dedup_domain_id = object.dedup_domain_id)
         FROM objects AS object
         WHERE object.id = $1 AND object.dedup_domain_id = $2",
    )
    .bind(fixture.object_v1.object_id().into_uuid())
    .bind(fixture.object_v1.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("source Object evidence must load");
    let checkpoints_before = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM device_sync_checkpoints WHERE library_id = $1",
    )
    .bind(fixture.library.id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("sync checkpoint evidence must load");

    let execution = backup
        .execute_restore_plan(fixture.user_id, plan.id())
        .await
        .expect("planned restore must execute");
    assert!(matches!(
        backup.execute_restore_plan(UserId::new(), plan.id()).await,
        Err(BackupError::NotFound)
    ));
    assert_eq!(execution.plan_id(), plan.id());
    assert_eq!(execution.target_library_id(), fixture.library.id());
    assert!(matches!(
        backup
            .get_restore_execution(UserId::new(), execution.id())
            .await,
        Err(BackupError::NotFound)
    ));
    assert_eq!(execution.created_node_count(), 3);
    assert_eq!(execution.created_file_version_count(), 1);
    assert_eq!(execution.journal_first_sequence().get(), 1);
    assert_eq!(execution.journal_last_sequence().get(), 3);

    let executed_plan = backup
        .get_restore_plan(fixture.user_id, plan.id())
        .await
        .expect("executed plan must remain readable");
    assert_eq!(executed_plan.state(), BackupRestorePlanState::Executed);
    let execution_entries = backup
        .list_restore_execution_entries(fixture.user_id, execution.id())
        .await
        .expect("execution evidence must load");
    let (plan_entries, has_more) = backup
        .list_restore_plan_entries(fixture.user_id, plan.id(), None, 100)
        .await
        .expect("persisted plan entries must load");
    assert!(!has_more);
    assert_eq!(execution_entries.len(), 3);
    assert_eq!(plan_entries.len(), execution_entries.len());
    for (plan_entry, execution_entry) in plan_entries.iter().zip(&execution_entries) {
        assert_eq!(plan_entry.ordinal(), execution_entry.ordinal());
        assert_eq!(
            plan_entry.planned_node_id(),
            execution_entry.destination_node_id()
        );
    }
    assert_eq!(
        execution_entries[0]
            .node_created_journal_sequence()
            .expect("wrapper has a NodeCreated event")
            .get(),
        1
    );
    assert_eq!(
        execution_entries[1]
            .node_created_journal_sequence()
            .expect("directory has a NodeCreated event")
            .get(),
        2
    );
    let restored_file_version_id = execution_entries[2]
        .destination_file_version_id()
        .expect("restored file has a new live FileVersion");
    assert_ne!(restored_file_version_id, fixture.version_v1.id());
    let restored_content = sqlx::query_as::<_, (Uuid, String, Vec<u8>)>(
        "SELECT object.id, object.plaintext_length::TEXT, object.canonical_hash
         FROM file_versions AS version
         INNER JOIN objects AS object
           ON object.id = version.object_id
          AND object.dedup_domain_id = version.object_dedup_domain_id
         WHERE version.id = $1 AND version.library_id = $2",
    )
    .bind(restored_file_version_id.into_uuid())
    .bind(fixture.library.id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("restored content metadata must load");
    assert_eq!(
        restored_content.0,
        fixture.object_v1.object_id().into_uuid()
    );
    assert_eq!(restored_content.1, "2048");
    assert_eq!(
        restored_content.2,
        fixture.object_v1.canonical_hash().as_bytes()
    );
    assert_eq!(
        execution_entries[2]
            .file_content_committed_journal_sequence()
            .expect("file has a content commit event")
            .get(),
        3
    );

    let after = target_evidence(&fixture).await;
    assert_eq!(after.0, target_before.0 + 3);
    assert_eq!(after.1, target_before.1 + 1);
    assert_eq!(after.2, target_before.2);
    assert_eq!(after.3, target_before.3 + 3);
    assert_eq!(after.4, target_before.4 + 3);
    assert_eq!(after.5, target_before.5);
    assert_eq!(
        snapshot_evidence(&fixture, snapshot_id).await,
        source_before
    );
    assert_eq!(
        sqlx::query_as::<_, (String, String, Vec<u8>, i64)>(
            "SELECT object.lifecycle_state, object.plaintext_length::TEXT,
                    object.canonical_hash,
                    (SELECT count(*) FROM object_replicas AS replica
                     WHERE replica.object_id = object.id
                       AND replica.object_dedup_domain_id = object.dedup_domain_id)
             FROM objects AS object
             WHERE object.id = $1 AND object.dedup_domain_id = $2",
        )
        .bind(fixture.object_v1.object_id().into_uuid())
        .bind(fixture.object_v1.dedup_domain_id().into_uuid())
        .fetch_one(&fixture.inspection)
        .await
        .expect("source Object evidence must remain readable"),
        object_before
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM device_sync_checkpoints WHERE library_id = $1",
        )
        .bind(fixture.library.id().into_uuid())
        .fetch_one(&fixture.inspection)
        .await
        .expect("sync checkpoint evidence must remain readable"),
        checkpoints_before
    );
    let metadata = FileMetadataService::new(fixture.pool.clone());
    assert_eq!(
        metadata
            .get_node(fixture.user_id, fixture.directory.id())
            .await
            .expect("existing directory must remain readable"),
        fixture.directory
    );
    assert_eq!(
        metadata
            .get_node(fixture.user_id, fixture.file.id())
            .await
            .expect("existing file must remain readable"),
        fixture.file
    );

    let replay = backup
        .execute_restore_plan(fixture.user_id, plan.id())
        .await
        .expect("lost-response retry must return the canonical receipt");
    assert_eq!(replay, execution);
    assert_eq!(
        backup
            .list_restore_execution_entries(fixture.user_id, replay.id())
            .await
            .expect("replayed execution evidence must load"),
        execution_entries
    );
    assert_eq!(target_evidence(&fixture).await, after);
    assert!(
        sqlx::query(
            "UPDATE backup_restore_executions
         SET journal_last_sequence = journal_last_sequence + 1
         WHERE id = $1",
        )
        .bind(execution.id().into_uuid())
        .execute(&fixture.inspection)
        .await
        .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM backup_restore_execution_entries WHERE execution_id = $1")
            .bind(execution.id().into_uuid())
            .execute(&fixture.inspection)
            .await
            .is_err()
    );

    let journal = sqlx::query_as::<_, (i64, String, Uuid)>(
        "SELECT sequence, change_kind, resource_id
         FROM change_journal
         WHERE library_id = $1 AND sequence > $2
         ORDER BY sequence ASC",
    )
    .bind(fixture.library.id().into_uuid())
    .bind(target_before.3)
    .fetch_all(&fixture.inspection)
    .await
    .expect("restore journal facts must load");
    assert_eq!(
        journal.iter().map(|row| row.1.as_str()).collect::<Vec<_>>(),
        vec!["NODE_CREATED", "NODE_CREATED", "FILE_CONTENT_COMMITTED"]
    );
    assert_eq!(
        journal.iter().map(|row| row.0).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
}

/// Execution repeats the Prompt 43 fail-closed checks at the write fence:
/// target clock drift, late collision, planned-ID occupation, invalid parent,
/// expired source, and missing retained content never create a receipt or a
/// destination namespace row.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_backup_restore_execution_revalidates_all_safety_fences() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-fences").await;
    let backup = BackupService::new(fixture.pool.clone());

    let head_plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-execution-fence-head",
        "head-drift",
    )
    .await
    .expect("head-drift plan must persist");
    sqlx::query("UPDATE libraries SET sync_head = sync_head + 1 WHERE id = $1")
        .bind(fixture.library.id().into_uuid())
        .execute(&fixture.inspection)
        .await
        .expect("test head drift must persist");
    assert!(matches!(
        backup
            .execute_restore_plan(fixture.user_id, head_plan.id())
            .await,
        Err(BackupError::RestorePreflight(
            BackupRestorePreflightIssue::TargetChanged
        ))
    ));
    assert_eq!(
        backup
            .get_restore_plan(fixture.user_id, head_plan.id())
            .await
            .expect("head-drift plan remains readable")
            .state(),
        BackupRestorePlanState::Stale
    );

    let collision_plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-execution-fence-collision",
        "late execution collision",
    )
    .await
    .expect("collision plan must persist");
    let collision_node = Node::new_child(
        NodeId::new(),
        fixture.library.id(),
        &fixture.root,
        NodeKind::Directory,
        name("late execution collision"),
        timestamp("2026-08-29T00:00:03.123456Z"),
    )
    .expect("late collision node is valid");
    DomainRepository::new(&fixture.pool)
        .insert_node(&collision_node)
        .await
        .expect("late collision must persist");
    assert!(matches!(
        backup
            .execute_restore_plan(fixture.user_id, collision_plan.id())
            .await,
        Err(BackupError::RestorePreflight(
            BackupRestorePreflightIssue::DestinationNameConflict
        ))
    ));
    assert_eq!(
        backup
            .get_restore_plan(fixture.user_id, collision_plan.id())
            .await
            .expect("collision plan remains readable")
            .state(),
        BackupRestorePlanState::Stale
    );

    let id_plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-execution-fence-planned-id",
        "planned id occupation",
    )
    .await
    .expect("planned-id plan must persist");
    let (id_entries, _) = backup
        .list_restore_plan_entries(fixture.user_id, id_plan.id(), None, 100)
        .await
        .expect("planned-id entries must load");
    let occupied = Node::new_child(
        id_entries[0].planned_node_id(),
        fixture.library.id(),
        &fixture.root,
        NodeKind::Directory,
        name("unrelated planned identity occupant"),
        timestamp("2026-08-29T00:00:04.123456Z"),
    )
    .expect("planned identity occupant is valid");
    DomainRepository::new(&fixture.pool)
        .insert_node(&occupied)
        .await
        .expect("planned identity occupant must persist");
    assert!(matches!(
        backup
            .execute_restore_plan(fixture.user_id, id_plan.id())
            .await,
        Err(BackupError::RestorePreflight(
            BackupRestorePreflightIssue::TargetChanged
        ))
    ));
    assert_eq!(
        backup
            .get_restore_plan(fixture.user_id, id_plan.id())
            .await
            .expect("planned-id plan remains readable")
            .state(),
        BackupRestorePlanState::Planned
    );

    let parent_plan = BackupService::new(fixture.pool.clone())
        .create_restore_plan(
            fixture.user_id,
            "restore-execution-fence-parent".to_owned(),
            set_id,
            snapshot_id,
            fixture.library.id(),
            fixture.directory.id(),
            name("invalidated parent"),
        )
        .await
        .expect("parent plan must persist");
    sqlx::query(
        "UPDATE nodes
         SET state = 'TRASHED', trashed_at = CURRENT_TIMESTAMP
         WHERE id = $1",
    )
    .bind(fixture.directory.id().into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("test parent invalidation must persist");
    assert!(matches!(
        backup
            .execute_restore_plan(fixture.user_id, parent_plan.id())
            .await,
        Err(BackupError::RestorePreflight(
            BackupRestorePreflightIssue::InvalidTargetParent
        ))
    ));
    assert_eq!(
        backup
            .get_restore_plan(fixture.user_id, parent_plan.id())
            .await
            .expect("invalid-parent plan remains readable")
            .state(),
        BackupRestorePlanState::Stale
    );

    let expired_plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-execution-fence-expired",
        "expired source",
    )
    .await
    .expect("expired-source plan must persist");
    sqlx::query(
        "UPDATE backup_snapshots
         SET state = 'EXPIRED', expired_at = CURRENT_TIMESTAMP
         WHERE id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("snapshot expiry must persist");
    assert!(matches!(
        backup
            .execute_restore_plan(fixture.user_id, expired_plan.id())
            .await,
        Err(BackupError::RestorePreflight(
            BackupRestorePreflightIssue::SnapshotNotRestorable
        ))
    ));
    assert_eq!(
        backup
            .get_restore_plan(fixture.user_id, expired_plan.id())
            .await
            .expect("expired-source plan remains readable")
            .state(),
        BackupRestorePlanState::Stale
    );

    let retained_fixture = new_fixture().await;
    let (retained_set_id, retained_snapshot_id) =
        create_snapshot(&retained_fixture, "execution-retention-missing").await;
    let retained_backup = BackupService::new(retained_fixture.pool.clone());
    let retained_plan = create_plan(
        &retained_fixture,
        retained_set_id,
        retained_snapshot_id,
        "restore-execution-fence-retention",
        "missing retention",
    )
    .await
    .expect("retention plan must persist");
    sqlx::query("ALTER TABLE backup_snapshot_content_pins DISABLE TRIGGER backup_snapshot_content_pins_reject_direct_delete")
        .execute(&retained_fixture.inspection)
        .await
        .expect("test-only retention corruption gate must open");
    sqlx::query("DELETE FROM backup_snapshot_content_pins WHERE snapshot_id = $1")
        .bind(retained_snapshot_id.into_uuid())
        .execute(&retained_fixture.inspection)
        .await
        .expect("test-only retained pin deletion must persist");
    sqlx::query("ALTER TABLE backup_snapshot_content_pins ENABLE TRIGGER backup_snapshot_content_pins_reject_direct_delete")
        .execute(&retained_fixture.inspection)
        .await
        .expect("test-only retention corruption gate must close");
    assert!(matches!(
        retained_backup
            .execute_restore_plan(retained_fixture.user_id, retained_plan.id())
            .await,
        Err(BackupError::RestorePreflight(
            BackupRestorePreflightIssue::MissingRetentionReference
        ))
    ));
    assert_eq!(
        retained_backup
            .get_restore_plan(retained_fixture.user_id, retained_plan.id())
            .await
            .expect("retention plan remains readable")
            .state(),
        BackupRestorePlanState::Planned
    );
}

/// Test-only failures after namespace insertion and after FileVersion/journal
/// work are recovered by PostgreSQL rollback, not compensating deletes. A clean
/// retry then performs the complete execution.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_backup_restore_execution_rolls_back_partial_metadata_and_journal() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-rollback").await;
    let backup = BackupService::new(fixture.pool.clone());
    let source_before = snapshot_evidence(&fixture, snapshot_id).await;
    let target_before = target_evidence(&fixture).await;

    let node_suffix = Uuid::now_v7().simple().to_string();
    let node_function = format!("synveil_test_restore_execution_node_fail_{node_suffix}");
    let node_trigger = format!("synveil_test_restore_execution_node_fail_trigger_{node_suffix}");
    sqlx::query(&format!(
        "CREATE FUNCTION {node_function}() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             IF NEW.name = 'reports' THEN
                 RAISE EXCEPTION 'injected restore node failure';
             END IF;
             RETURN NEW;
         END;
         $$"
    ))
    .execute(&fixture.inspection)
    .await
    .expect("restore node failure function must persist");
    sqlx::query(&format!(
        "CREATE TRIGGER {node_trigger}
         BEFORE INSERT ON nodes
         FOR EACH ROW EXECUTE FUNCTION {node_function}()"
    ))
    .execute(&fixture.inspection)
    .await
    .expect("restore node failure trigger must persist");

    let node_failure_plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-execution-rollback-node",
        "rollback after wrapper",
    )
    .await
    .expect("node rollback plan must persist");
    assert!(
        backup
            .execute_restore_plan(fixture.user_id, node_failure_plan.id())
            .await
            .is_err()
    );
    sqlx::query(&format!("DROP TRIGGER {node_trigger} ON nodes"))
        .execute(&fixture.inspection)
        .await
        .expect("restore node failure trigger must drop");
    sqlx::query(&format!("DROP FUNCTION {node_function}()"))
        .execute(&fixture.inspection)
        .await
        .expect("restore node failure function must drop");

    assert_eq!(
        backup
            .get_restore_plan(fixture.user_id, node_failure_plan.id())
            .await
            .expect("node rollback plan remains readable")
            .state(),
        BackupRestorePlanState::Planned
    );
    assert_eq!(target_evidence(&fixture).await, target_before);
    assert_eq!(
        snapshot_evidence(&fixture, snapshot_id).await,
        source_before
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM backup_restore_executions
             WHERE restore_plan_id = $1",
        )
        .bind(node_failure_plan.id().into_uuid())
        .fetch_one(&fixture.inspection)
        .await
        .expect("node rollback execution count must load"),
        0
    );

    let journal_suffix = Uuid::now_v7().simple().to_string();
    let journal_function = format!("synveil_test_restore_execution_journal_fail_{journal_suffix}");
    let journal_trigger =
        format!("synveil_test_restore_execution_journal_fail_trigger_{journal_suffix}");
    sqlx::query(&format!(
        "CREATE FUNCTION {journal_function}() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             IF NEW.change_kind = 'FILE_CONTENT_COMMITTED' THEN
                 RAISE EXCEPTION 'injected restore journal failure';
             END IF;
             RETURN NEW;
         END;
         $$"
    ))
    .execute(&fixture.inspection)
    .await
    .expect("restore journal failure function must persist");
    sqlx::query(&format!(
        "CREATE TRIGGER {journal_trigger}
         BEFORE INSERT ON change_journal
         FOR EACH ROW EXECUTE FUNCTION {journal_function}()"
    ))
    .execute(&fixture.inspection)
    .await
    .expect("restore journal failure trigger must persist");

    let journal_failure_plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-execution-rollback-journal",
        "rollback after journal",
    )
    .await
    .expect("journal rollback plan must persist");
    assert!(
        backup
            .execute_restore_plan(fixture.user_id, journal_failure_plan.id())
            .await
            .is_err()
    );
    sqlx::query(&format!("DROP TRIGGER {journal_trigger} ON change_journal"))
        .execute(&fixture.inspection)
        .await
        .expect("restore journal failure trigger must drop");
    sqlx::query(&format!("DROP FUNCTION {journal_function}()"))
        .execute(&fixture.inspection)
        .await
        .expect("restore journal failure function must drop");

    assert_eq!(
        backup
            .get_restore_plan(fixture.user_id, journal_failure_plan.id())
            .await
            .expect("journal rollback plan remains readable")
            .state(),
        BackupRestorePlanState::Planned
    );
    assert_eq!(target_evidence(&fixture).await, target_before);
    assert_eq!(
        snapshot_evidence(&fixture, snapshot_id).await,
        source_before
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM backup_restore_executions
             WHERE restore_plan_id = $1",
        )
        .bind(journal_failure_plan.id().into_uuid())
        .fetch_one(&fixture.inspection)
        .await
        .expect("journal rollback execution count must load"),
        0
    );

    let retry = backup
        .execute_restore_plan(fixture.user_id, journal_failure_plan.id())
        .await
        .expect("clean rollback retry must execute completely");
    assert_eq!(retry.created_node_count(), 3);
    assert_eq!(retry.created_file_version_count(), 1);
    assert_eq!(target_evidence(&fixture).await.0, target_before.0 + 3);
}

/// Two independent service instances contend on the real PostgreSQL plan and
/// namespace fences. Exactly one transaction creates the subtree; the other
/// resolves to its committed canonical receipt.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_backup_restore_execution_concurrent_duplicate_is_canonical() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-concurrent").await;
    let plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-execution-concurrent-0001",
        "concurrent recovery",
    )
    .await
    .expect("concurrent plan must persist");
    let target_before = target_evidence(&fixture).await;
    let first_service = BackupService::new(fixture.pool.clone());
    let second_service = BackupService::new(fixture.pool.clone());

    let (first, second) = tokio::join!(
        first_service.execute_restore_plan(fixture.user_id, plan.id()),
        second_service.execute_restore_plan(fixture.user_id, plan.id()),
    );
    let first = first.expect("first concurrent execution must resolve");
    let second = second.expect("duplicate concurrent execution must resolve");
    assert_eq!(first, second);
    assert_eq!(first.created_node_count(), 3);
    assert_eq!(first.created_file_version_count(), 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM backup_restore_executions
             WHERE restore_plan_id = $1 AND state = 'COMMITTED'",
        )
        .bind(plan.id().into_uuid())
        .fetch_one(&fixture.inspection)
        .await
        .expect("canonical execution count must load"),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM backup_restore_execution_entries
             WHERE execution_id = $1",
        )
        .bind(first.id().into_uuid())
        .fetch_one(&fixture.inspection)
        .await
        .expect("canonical execution-entry count must load"),
        3
    );
    let after = target_evidence(&fixture).await;
    assert_eq!(after.0, target_before.0 + 3);
    assert_eq!(after.1, target_before.1 + 1);
    assert_eq!(after.4, target_before.4 + 3);
}

/// The current schema has an explicit deduplication domain on libraries. A
/// cross-domain restore therefore fails closed without copying or rewriting an
/// ObjectStore object.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_backup_restore_execution_rejects_incompatible_target_domain() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-domain").await;
    let repository = DomainRepository::new(&fixture.pool);
    let target_library_id = LibraryId::new();
    let target_root = Node::new_root(
        NodeId::new(),
        target_library_id,
        name("other-domain-root"),
        timestamp("2026-08-29T00:00:01.123456Z"),
    );
    let target_library = Library::new(
        target_library_id,
        fixture.user_id,
        name("Other storage domain"),
        &target_root,
        DedupDomainId::new(),
        timestamp("2026-08-29T00:00:01.123456Z"),
    )
    .expect("other target library is valid");
    repository
        .insert_library_with_root(&target_library, &target_root)
        .await
        .expect("other target library must persist");

    let backup = BackupService::new(fixture.pool.clone());
    let plan = backup
        .create_restore_plan(
            fixture.user_id,
            "restore-execution-domain-incompatible".to_owned(),
            set_id,
            snapshot_id,
            target_library.id(),
            target_root.id(),
            name("incompatible restore"),
        )
        .await
        .expect("cross-domain plan is still valid planning provenance");
    assert!(matches!(
        backup
            .execute_restore_plan(fixture.user_id, plan.id())
            .await,
        Err(BackupError::RestorePreflight(
            BackupRestorePreflightIssue::TargetStorageIncompatible
        ))
    ));
    assert_eq!(
        backup
            .get_restore_plan(fixture.user_id, plan.id())
            .await
            .expect("incompatible-domain plan remains readable")
            .state(),
        BackupRestorePlanState::Planned
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM nodes
             WHERE library_id = $1 AND parent_node_id = $2",
        )
        .bind(target_library.id().into_uuid())
        .bind(target_root.id().into_uuid())
        .fetch_one(&fixture.inspection)
        .await
        .expect("other target child count must load"),
        0
    );
}

/// A normal namespace mutation and restore execution serialize on the same
/// library guard. Depending on which transaction acquires it first, restore
/// either commits first or observes the mutation's new journal head and stales
/// without writing any part of its subtree.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_backup_restore_execution_serializes_against_target_mutation() {
    let fixture = fixture().await;
    let (set_id, snapshot_id) = create_snapshot(&fixture, "execution-race").await;
    let backup = BackupService::new(fixture.pool.clone());
    let plan = create_plan(
        &fixture,
        set_id,
        snapshot_id,
        "restore-execution-race-0001",
        "race recovery",
    )
    .await
    .expect("race plan must persist");
    let target_before = target_evidence(&fixture).await;
    let mutation_service = FileMetadataService::new(fixture.pool.clone());

    let (execution, mutation) = tokio::join!(
        backup.execute_restore_plan(fixture.user_id, plan.id()),
        mutation_service.create_directory(
            fixture.user_id,
            fixture.library.id(),
            Some(fixture.root.id()),
            name("race mutation"),
        ),
    );
    let mutation = mutation.expect("normal target mutation must serialize and commit");
    match execution {
        Ok(execution) => {
            assert_eq!(execution.created_node_count(), 3);
            assert_eq!(
                backup
                    .get_restore_plan(fixture.user_id, plan.id())
                    .await
                    .expect("race plan must remain readable")
                    .state(),
                BackupRestorePlanState::Executed
            );
            assert_eq!(target_evidence(&fixture).await.0, target_before.0 + 4);
        }
        Err(BackupError::RestorePreflight(BackupRestorePreflightIssue::TargetChanged)) => {
            assert_eq!(
                backup
                    .get_restore_plan(fixture.user_id, plan.id())
                    .await
                    .expect("stale race plan must remain readable")
                    .state(),
                BackupRestorePlanState::Stale
            );
            assert_eq!(target_evidence(&fixture).await.0, target_before.0 + 1);
        }
        other => panic!("unexpected race outcome: {other:?}"),
    }
    assert!(mutation.state() == synveil_core::NodeState::Active);
}
