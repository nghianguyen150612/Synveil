use std::time::Duration;

use sqlx::PgPool;
use synveil_core::{
    BackupSetId, DedupDomainId, FileVersion, FileVersionId, Library, LibraryId, LogicalName, Node,
    NodeId, NodeKind, ObjectGcPolicy, ObjectId, ObjectReference, Sha256Digest, SnapshotId,
    Timestamp, TrashRetentionPolicy, User, UserId, UserStatus,
};
use synveil_metadata::{
    BackupService, DatabaseConfig, DatabasePool, DomainRepository, FileMetadataService,
    MigrationRunner, ObjectGcExecutionMetadataBackend, ObjectGcPlanResult, ObjectGcPlanningService,
    PostgresObjectGcExecutionRepository, PostgresObjectGcWorkerRepository, PurgeExecutionResult,
    TrashRetentionService,
};
use uuid::Uuid;

fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse(value).expect("test timestamp is valid")
}

fn name(value: &str) -> LogicalName {
    LogicalName::new(value).expect("test logical name is valid")
}

struct BackupFixture {
    pool: DatabasePool,
    inspection: PgPool,
    user_id: UserId,
    library: Library,
    root: Node,
    file: Node,
    object_v1: ObjectReference,
    version_v1: FileVersion,
}

async fn fixture() -> BackupFixture {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable test database");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    let status = MigrationRunner::new()
        .run(&pool)
        .await
        .expect("SQLx migration execution must succeed");
    assert!(status.is_current(), "all migrations must be current");
    let inspection = PgPool::connect(&url)
        .await
        .expect("inspection connection must succeed");

    let observed_at = timestamp("2026-08-29T00:00:00.123456Z");
    let repository = DomainRepository::new(&pool);
    let user_id = UserId::new();
    repository
        .insert_user(&User::new(
            user_id,
            synveil_core::LoginIdentifier::new("backup-retention-owner", user_id.to_string())
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
        name("Backup retention"),
        &root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("fixture library is valid");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("fixture library and root must persist");

    let (file, object_v1, version_v1) = insert_live_file(
        &pool,
        &library,
        &root,
        "annual.pdf",
        Sha256Digest::from_bytes([0xab; 32]),
        2_048,
        observed_at,
    )
    .await;

    BackupFixture {
        pool,
        inspection,
        user_id,
        library,
        root,
        file,
        object_v1,
        version_v1,
    }
}

async fn insert_live_file(
    pool: &DatabasePool,
    library: &Library,
    parent: &Node,
    file_name: &str,
    digest: Sha256Digest,
    byte_length: u64,
    observed_at: Timestamp,
) -> (Node, ObjectReference, FileVersion) {
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
    let object = ObjectReference::new(
        ObjectId::new(),
        library.dedup_domain_id(),
        digest,
        byte_length,
    );
    repository
        .insert_object(object, observed_at)
        .await
        .expect("fixture object must persist");
    let version = FileVersion::new(
        FileVersionId::new(),
        library,
        &file,
        object,
        None,
        observed_at,
    )
    .expect("fixture file version is valid");
    repository
        .insert_file_version(version)
        .await
        .expect("fixture file version must persist");
    let file = file
        .with_current_version(&version, observed_at)
        .expect("fixture file accepts its version");
    repository
        .update_node(&file)
        .await
        .expect("fixture file head must persist");
    (file, object, version)
}

async fn create_set(fixture: &BackupFixture, label: &str) -> BackupSetId {
    let set_id = BackupSetId::new();
    BackupService::new(fixture.pool.clone())
        .create_backup_set(
            fixture.user_id,
            set_id,
            name(label),
            fixture.library.id(),
            Some(30),
            timestamp("2026-08-29T00:00:01.123456Z"),
        )
        .await
        .expect("backup set must persist");
    set_id
}

async fn count_snapshot_pins(pool: &PgPool, snapshot_id: SnapshotId) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*)
         FROM backup_snapshot_content_pins
         WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(pool)
    .await
    .expect("pin count query must succeed")
}

async fn count_object_pins(pool: &PgPool, object: ObjectReference) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*)
         FROM backup_snapshot_content_pins
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .fetch_one(pool)
    .await
    .expect("object pin count query must succeed")
}

async fn count_gc_candidates(pool: &PgPool, object: ObjectReference) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*)
         FROM object_gc_candidates
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .fetch_one(pool)
    .await
    .expect("GC candidate count query must succeed")
}

async fn create_building_content_snapshot(
    fixture: &BackupFixture,
    label: &str,
    node: &Node,
    version: &FileVersion,
) -> SnapshotId {
    let set_id = create_set(fixture, label).await;
    let snapshot_id = SnapshotId::new();
    let operation_id = format!("building-{label}-{}", Uuid::now_v7().simple());
    let (snapshot_epoch, snapshot_resume_sequence) = sqlx::query_as::<_, (i64, i64)>(
        "SELECT journal_epoch, sync_head
         FROM libraries
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(fixture.library.id().into_uuid())
    .bind(fixture.user_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("library journal head must load");

    sqlx::query(
        "INSERT INTO backup_snapshots
            (id, backup_set_id, owner_user_id, source_library_id, operation_id,
             snapshot_epoch, snapshot_resume_sequence, manifest_item_count,
             terminal_node_id, content_reference_count, state, created_at,
             committed_at, expired_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, 1, $8, 1, 'BUILDING',
                 CURRENT_TIMESTAMP, NULL, NULL)",
    )
    .bind(snapshot_id.into_uuid())
    .bind(set_id.into_uuid())
    .bind(fixture.user_id.into_uuid())
    .bind(fixture.library.id().into_uuid())
    .bind(&operation_id)
    .bind(snapshot_epoch)
    .bind(snapshot_resume_sequence)
    .bind(node.id().into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("building content snapshot must persist");

    let object = version.object_reference();
    sqlx::query(
        "INSERT INTO backup_snapshot_nodes
            (snapshot_id, node_id, parent_node_id, name, kind, state, revision,
             current_version_id, content_length, content_sha256,
             node_created_at, node_updated_at)
         VALUES ($1, $2, $3, $4, 'FILE', 'ACTIVE', $5::NUMERIC, $6, $7::NUMERIC,
                 $8, $9, $10)",
    )
    .bind(snapshot_id.into_uuid())
    .bind(node.id().into_uuid())
    .bind(node.parent_node_id().map(NodeId::into_uuid))
    .bind(node.name().as_str())
    .bind(node.revision().get().to_string())
    .bind(version.id().into_uuid())
    .bind(object.plaintext_length().to_string())
    .bind(object.canonical_hash().as_bytes())
    .bind(node.created_at().as_offset_datetime())
    .bind(node.updated_at().as_offset_datetime())
    .execute(&fixture.inspection)
    .await
    .expect("building content manifest must persist");

    snapshot_id
}

async fn create_building_directory_snapshot(fixture: &BackupFixture, label: &str) -> SnapshotId {
    let set_id = create_set(fixture, label).await;
    let snapshot_id = SnapshotId::new();
    let operation_id = format!("directory-{label}-{}", Uuid::now_v7().simple());
    let (snapshot_epoch, snapshot_resume_sequence) = sqlx::query_as::<_, (i64, i64)>(
        "SELECT journal_epoch, sync_head
         FROM libraries
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(fixture.library.id().into_uuid())
    .bind(fixture.user_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("library journal head must load");

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
    .bind(&operation_id)
    .bind(snapshot_epoch)
    .bind(snapshot_resume_sequence)
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
    .expect("building directory manifest must persist");

    snapshot_id
}

async fn insert_pin(
    pool: &PgPool,
    snapshot_id: SnapshotId,
    manifest_node_id: NodeId,
    file_version_id: FileVersionId,
    object: ObjectReference,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO backup_snapshot_content_pins
            (snapshot_id, manifest_node_id, file_version_id,
             object_id, object_dedup_domain_id, created_at)
         VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP)",
    )
    .bind(snapshot_id.into_uuid())
    .bind(manifest_node_id.into_uuid())
    .bind(file_version_id.into_uuid())
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .execute(pool)
    .await
    .map(|_| ())
}

async fn snapshot_pin_state(
    pool: &PgPool,
    snapshot_id: SnapshotId,
) -> (i64, i64, i64, i64, Uuid, Uuid) {
    sqlx::query_as(
        "SELECT
            (SELECT count(*) FROM backup_snapshot_content_pins
             WHERE snapshot_id = $1),
            (SELECT count(*) FROM backup_snapshot_nodes WHERE snapshot_id = $1),
            snapshot.manifest_item_count,
            snapshot.content_reference_count,
            pin.object_id,
            pin.object_dedup_domain_id
         FROM backup_snapshots AS snapshot
         INNER JOIN backup_snapshot_content_pins AS pin
           ON pin.snapshot_id = snapshot.id
         WHERE snapshot.id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(pool)
    .await
    .expect("snapshot pin state query must succeed")
}

async fn assert_pin_mutation_rejected(
    pool: &PgPool,
    snapshot_id: SnapshotId,
    manifest_node_id: NodeId,
    object: ObjectReference,
    replacement_object: ObjectReference,
) {
    let before = snapshot_pin_state(pool, snapshot_id).await;

    let extra_insert = insert_pin(
        pool,
        snapshot_id,
        NodeId::new(),
        FileVersionId::new(),
        object,
    )
    .await;
    assert!(
        extra_insert.is_err(),
        "post-commit pin INSERT must be rejected"
    );
    assert_eq!(snapshot_pin_state(pool, snapshot_id).await, before);

    let identity_update = sqlx::query(
        "UPDATE backup_snapshot_content_pins
         SET object_id = $3, object_dedup_domain_id = $4
         WHERE snapshot_id = $1 AND manifest_node_id = $2",
    )
    .bind(snapshot_id.into_uuid())
    .bind(manifest_node_id.into_uuid())
    .bind(replacement_object.object_id().into_uuid())
    .bind(replacement_object.dedup_domain_id().into_uuid())
    .execute(pool)
    .await;
    assert!(
        identity_update.is_err(),
        "post-commit Object identity UPDATE must be rejected"
    );
    assert_eq!(snapshot_pin_state(pool, snapshot_id).await, before);

    let file_version_update = sqlx::query(
        "UPDATE backup_snapshot_content_pins
         SET file_version_id = $3
         WHERE snapshot_id = $1 AND manifest_node_id = $2",
    )
    .bind(snapshot_id.into_uuid())
    .bind(manifest_node_id.into_uuid())
    .bind(FileVersionId::new().into_uuid())
    .execute(pool)
    .await;
    assert!(
        file_version_update.is_err(),
        "post-commit FileVersion UPDATE must be rejected"
    );
    assert_eq!(snapshot_pin_state(pool, snapshot_id).await, before);

    let manifest_node_update = sqlx::query(
        "UPDATE backup_snapshot_content_pins
         SET manifest_node_id = $3
         WHERE snapshot_id = $1 AND manifest_node_id = $2",
    )
    .bind(snapshot_id.into_uuid())
    .bind(manifest_node_id.into_uuid())
    .bind(NodeId::new().into_uuid())
    .execute(pool)
    .await;
    assert!(
        manifest_node_update.is_err(),
        "post-commit manifest-node UPDATE must be rejected"
    );
    assert_eq!(snapshot_pin_state(pool, snapshot_id).await, before);

    let direct_delete = sqlx::query(
        "DELETE FROM backup_snapshot_content_pins
         WHERE snapshot_id = $1 AND manifest_node_id = $2",
    )
    .bind(snapshot_id.into_uuid())
    .bind(manifest_node_id.into_uuid())
    .execute(pool)
    .await;
    assert!(
        direct_delete.is_err(),
        "direct pin release must be rejected"
    );
    assert_eq!(snapshot_pin_state(pool, snapshot_id).await, before);
}

/// Pin insertion is a BUILDING-only operation and validates every part of the
/// manifest/FileVersion/Object mapping before the completion aggregate runs.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_backup_pin_insert_enforces_building_manifest_mapping_immediately() {
    let fixture = fixture().await;
    let repository = DomainRepository::new(&fixture.pool);
    let replacement_object = ObjectReference::new(
        ObjectId::new(),
        fixture.library.dedup_domain_id(),
        Sha256Digest::from_bytes([0xd1; 32]),
        512,
    );
    repository
        .insert_object(replacement_object, timestamp("2026-08-29T00:00:00.323456Z"))
        .await
        .expect("same-domain replacement Object must persist");
    let other_domain_object = ObjectReference::new(
        ObjectId::new(),
        DedupDomainId::new(),
        Sha256Digest::from_bytes([0xd2; 32]),
        256,
    );
    repository
        .insert_object(
            other_domain_object,
            timestamp("2026-08-29T00:00:00.423456Z"),
        )
        .await
        .expect("other-domain replacement Object must persist");
    let (_second_file, second_object, second_version) = insert_live_file(
        &fixture.pool,
        &fixture.library,
        &fixture.root,
        "mapping-second.bin",
        Sha256Digest::from_bytes([0xd3; 32]),
        768,
        timestamp("2026-08-29T00:00:00.523456Z"),
    )
    .await;

    let valid_snapshot = create_building_content_snapshot(
        &fixture,
        "mapping-valid",
        &fixture.file,
        &fixture.version_v1,
    )
    .await;
    insert_pin(
        &fixture.inspection,
        valid_snapshot,
        fixture.file.id(),
        fixture.version_v1.id(),
        fixture.object_v1,
    )
    .await
    .expect("a valid pin is allowed while the snapshot is BUILDING");
    sqlx::query(
        "UPDATE backup_snapshots
         SET state = 'COMPLETED', committed_at = CURRENT_TIMESTAMP
         WHERE id = $1 AND state = 'BUILDING'",
    )
    .bind(valid_snapshot.into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("complete transition with a valid pin must succeed");

    // The child DELETE guard does not break the existing cascade shape for an
    // unfinished snapshot. A future explicit pruning migration can therefore
    // retain the FK cascade while replacing the terminal snapshot-delete
    // protocol.
    let cascade_snapshot = create_building_content_snapshot(
        &fixture,
        "mapping-cascade",
        &fixture.file,
        &fixture.version_v1,
    )
    .await;
    insert_pin(
        &fixture.inspection,
        cascade_snapshot,
        fixture.file.id(),
        fixture.version_v1.id(),
        fixture.object_v1,
    )
    .await
    .expect("valid unfinished snapshot pin must persist before cascade test");
    sqlx::query("DELETE FROM backup_snapshots WHERE id = $1 AND state = 'BUILDING'")
        .bind(cascade_snapshot.into_uuid())
        .execute(&fixture.inspection)
        .await
        .expect("unfinished snapshot cascade must remain available");
    let cascade_counts = sqlx::query_as::<_, (i64, i64)>(
        "SELECT
            (SELECT count(*) FROM backup_snapshots WHERE id = $1),
            (SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1)",
    )
    .bind(cascade_snapshot.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("cascade result must be inspectable");
    assert_eq!(cascade_counts, (0, 0));

    let wrong_manifest_snapshot = create_building_content_snapshot(
        &fixture,
        "mapping-wrong-manifest",
        &fixture.file,
        &fixture.version_v1,
    )
    .await;
    assert!(
        insert_pin(
            &fixture.inspection,
            wrong_manifest_snapshot,
            NodeId::new(),
            fixture.version_v1.id(),
            fixture.object_v1,
        )
        .await
        .is_err(),
        "wrong manifest_node_id must be rejected at pin insert"
    );

    let wrong_version_snapshot = create_building_content_snapshot(
        &fixture,
        "mapping-wrong-version",
        &fixture.file,
        &fixture.version_v1,
    )
    .await;
    assert!(
        insert_pin(
            &fixture.inspection,
            wrong_version_snapshot,
            fixture.file.id(),
            FileVersionId::new(),
            fixture.object_v1,
        )
        .await
        .is_err(),
        "wrong file_version_id must be rejected at pin insert"
    );

    let wrong_object_snapshot = create_building_content_snapshot(
        &fixture,
        "mapping-wrong-object",
        &fixture.file,
        &fixture.version_v1,
    )
    .await;
    assert!(
        insert_pin(
            &fixture.inspection,
            wrong_object_snapshot,
            fixture.file.id(),
            fixture.version_v1.id(),
            replacement_object,
        )
        .await
        .is_err(),
        "wrong object_id must be rejected at pin insert"
    );

    let wrong_domain_snapshot = create_building_content_snapshot(
        &fixture,
        "mapping-wrong-domain",
        &fixture.file,
        &fixture.version_v1,
    )
    .await;
    assert!(
        insert_pin(
            &fixture.inspection,
            wrong_domain_snapshot,
            fixture.file.id(),
            fixture.version_v1.id(),
            other_domain_object,
        )
        .await
        .is_err(),
        "wrong dedup_domain_id must be rejected at pin insert"
    );

    let directory_snapshot =
        create_building_directory_snapshot(&fixture, "mapping-directory").await;
    assert!(
        insert_pin(
            &fixture.inspection,
            directory_snapshot,
            fixture.root.id(),
            fixture.version_v1.id(),
            fixture.object_v1,
        )
        .await
        .is_err(),
        "a directory/no-content manifest row must not be pinnable"
    );

    let other_snapshot = create_building_content_snapshot(
        &fixture,
        "mapping-other-snapshot",
        &_second_file,
        &second_version,
    )
    .await;
    let cross_snapshot = create_building_content_snapshot(
        &fixture,
        "mapping-cross-snapshot",
        &fixture.file,
        &fixture.version_v1,
    )
    .await;
    assert!(
        insert_pin(
            &fixture.inspection,
            cross_snapshot,
            second_version.node_id(),
            second_version.id(),
            second_object,
        )
        .await
        .is_err(),
        "a pin sourced from another snapshot manifest must be rejected"
    );

    for snapshot_id in [
        wrong_manifest_snapshot,
        wrong_version_snapshot,
        wrong_object_snapshot,
        wrong_domain_snapshot,
        directory_snapshot,
        other_snapshot,
        cross_snapshot,
    ] {
        sqlx::query("DELETE FROM backup_snapshots WHERE id = $1 AND state = 'BUILDING'")
            .bind(snapshot_id.into_uuid())
            .execute(&fixture.inspection)
            .await
            .expect("test-only building snapshot cleanup must succeed");
    }
}

/// A committed pin cannot be inserted, rewritten, or directly released. The
/// same retention ownership remains immutable after lifecycle-only expiry.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_backup_committed_and_expired_pins_are_immutable() {
    let fixture = fixture().await;
    let repository = DomainRepository::new(&fixture.pool);
    let replacement_object = ObjectReference::new(
        ObjectId::new(),
        fixture.library.dedup_domain_id(),
        Sha256Digest::from_bytes([0xe1; 32]),
        1_024,
    );
    repository
        .insert_object(replacement_object, timestamp("2026-08-29T00:00:00.623456Z"))
        .await
        .expect("replacement Object must persist");
    let set_id = create_set(&fixture, "post-commit-immutability").await;
    let snapshot = BackupService::new(fixture.pool.clone())
        .capture_snapshot(
            fixture.user_id,
            set_id,
            SnapshotId::new(),
            "post-commit-immutability-0001".to_owned(),
        )
        .await
        .expect("content snapshot must complete");
    let before = snapshot_pin_state(&fixture.inspection, snapshot.id()).await;
    assert_eq!(before.0, 1, "the valid content pin must exist");

    assert_pin_mutation_rejected(
        &fixture.inspection,
        snapshot.id(),
        fixture.file.id(),
        fixture.object_v1,
        replacement_object,
    )
    .await;
    assert_eq!(
        snapshot_pin_state(&fixture.inspection, snapshot.id()).await,
        before
    );

    let reopen_completed = sqlx::query(
        "UPDATE backup_snapshots
         SET state = 'BUILDING', committed_at = NULL
         WHERE id = $1",
    )
    .bind(snapshot.id().into_uuid())
    .execute(&fixture.inspection)
    .await;
    assert!(
        reopen_completed.is_err(),
        "a committed snapshot cannot be reopened to bypass the BUILDING pin gate"
    );
    let completed_state: String =
        sqlx::query_scalar("SELECT state FROM backup_snapshots WHERE id = $1")
            .bind(snapshot.id().into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .expect("completed state must remain inspectable");
    assert_eq!(completed_state, "COMPLETED");

    let completed_snapshot_delete = sqlx::query("DELETE FROM backup_snapshots WHERE id = $1")
        .bind(snapshot.id().into_uuid())
        .execute(&fixture.inspection)
        .await;
    assert!(
        completed_snapshot_delete.is_err(),
        "ordinary deletion of a committed snapshot must be rejected"
    );
    assert_eq!(
        snapshot_pin_state(&fixture.inspection, snapshot.id()).await,
        before
    );

    sqlx::query(
        "UPDATE backup_snapshots
         SET state = 'EXPIRED', expired_at = CURRENT_TIMESTAMP
         WHERE id = $1 AND state = 'COMPLETED'",
    )
    .bind(snapshot.id().into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("completed snapshot must transition to EXPIRED");
    let expired_before = snapshot_pin_state(&fixture.inspection, snapshot.id()).await;
    assert_eq!(expired_before, before);

    assert_pin_mutation_rejected(
        &fixture.inspection,
        snapshot.id(),
        fixture.file.id(),
        fixture.object_v1,
        replacement_object,
    )
    .await;
    assert_eq!(
        snapshot_pin_state(&fixture.inspection, snapshot.id()).await,
        expired_before
    );

    let reopen_expired = sqlx::query(
        "UPDATE backup_snapshots
         SET state = 'BUILDING', committed_at = NULL, expired_at = NULL
         WHERE id = $1",
    )
    .bind(snapshot.id().into_uuid())
    .execute(&fixture.inspection)
    .await;
    assert!(
        reopen_expired.is_err(),
        "an expired snapshot cannot be reopened to bypass the BUILDING pin gate"
    );

    let expired_snapshot_delete = sqlx::query("DELETE FROM backup_snapshots WHERE id = $1")
        .bind(snapshot.id().into_uuid())
        .execute(&fixture.inspection)
        .await;
    assert!(
        expired_snapshot_delete.is_err(),
        "ordinary deletion of an expired snapshot must be rejected"
    );
    assert_eq!(
        snapshot_pin_state(&fixture.inspection, snapshot.id()).await,
        expired_before
    );
}

/// Capture writes one private Object pin per manifest content row, replay from
/// a fresh pool cannot duplicate it, and different snapshots retain their own
/// independent rows even when they point at the same Object.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_backup_content_pins_capture_replay_and_independent_snapshot_ownership() {
    let fixture = fixture().await;
    let (_second_file, second_object, second_version) = insert_live_file(
        &fixture.pool,
        &fixture.library,
        &fixture.root,
        "inventory.csv",
        Sha256Digest::from_bytes([0xcd; 32]),
        4_096,
        timestamp("2026-08-29T00:00:00.223456Z"),
    )
    .await;
    let set_id = create_set(&fixture, "capture-and-replay").await;
    let backup = BackupService::new(fixture.pool.clone());

    let first = backup
        .capture_snapshot(
            fixture.user_id,
            set_id,
            SnapshotId::new(),
            "retention-capture-0001".to_owned(),
        )
        .await
        .expect("capture must complete");
    assert_eq!(first.content_reference_count(), 2);
    assert_eq!(
        count_snapshot_pins(&fixture.inspection, first.id()).await,
        2
    );
    let first_pins = sqlx::query_as::<_, (Uuid, Uuid, Uuid, Uuid)>(
        "SELECT manifest_node_id, file_version_id, object_id, object_dedup_domain_id
         FROM backup_snapshot_content_pins
         WHERE snapshot_id = $1
         ORDER BY manifest_node_id ASC",
    )
    .bind(first.id().into_uuid())
    .fetch_all(&fixture.inspection)
    .await
    .expect("first pin rows must load");
    assert_eq!(
        first_pins,
        vec![
            (
                fixture.file.id().into_uuid(),
                fixture.version_v1.id().into_uuid(),
                fixture.object_v1.object_id().into_uuid(),
                fixture.object_v1.dedup_domain_id().into_uuid(),
            ),
            (
                second_version.node_id().into_uuid(),
                second_version.id().into_uuid(),
                second_object.object_id().into_uuid(),
                second_object.dedup_domain_id().into_uuid(),
            ),
        ]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>(),
        "each logical manifest content row maps to its canonical private Object pin"
    );

    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must remain available for replay");
    let fresh_pool = DatabasePool::connect(
        &DatabaseConfig::from_url(url).expect("test URL must use PostgreSQL"),
    )
    .await
    .expect("fresh replay pool must connect");
    let replay = BackupService::new(fresh_pool.clone())
        .capture_snapshot(
            fixture.user_id,
            set_id,
            SnapshotId::new(),
            "retention-capture-0001".to_owned(),
        )
        .await
        .expect("fresh-pool replay must reuse the snapshot");
    assert_eq!(replay.id(), first.id());
    assert_eq!(
        count_snapshot_pins(&fixture.inspection, first.id()).await,
        2
    );
    fresh_pool.close().await;

    // Snapshot epochs are unique within a backup set, so a second independent
    // set intentionally captures the identical current Object without
    // inventing a live namespace mutation merely for this retention test.
    let second_set_id = create_set(&fixture, "capture-and-replay-second").await;
    let second = backup
        .capture_snapshot(
            fixture.user_id,
            second_set_id,
            SnapshotId::new(),
            "retention-capture-0002".to_owned(),
        )
        .await
        .expect("independent snapshot must complete");
    assert_ne!(second.id(), first.id());
    assert_eq!(
        count_snapshot_pins(&fixture.inspection, second.id()).await,
        2
    );
    assert_eq!(
        count_object_pins(&fixture.inspection, fixture.object_v1).await,
        2
    );
    assert_eq!(
        count_object_pins(&fixture.inspection, second_object).await,
        2
    );
}

/// A pin write failure after manifest work has begun rolls back the complete
/// capture transaction: no snapshot, manifest, pin, or retention reference is
/// left committed for the requested snapshot identity.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_backup_content_pin_capture_failure_rolls_back_everything() {
    let fixture = fixture().await;
    let (second_file, _, _) = insert_live_file(
        &fixture.pool,
        &fixture.library,
        &fixture.root,
        "failure-second.bin",
        Sha256Digest::from_bytes([0xee; 32]),
        32,
        timestamp("2026-08-29T00:00:00.223456Z"),
    )
    .await;
    let set_id = create_set(&fixture, "capture-rollback").await;
    let snapshot_id = SnapshotId::new();
    let fail_node_id = [fixture.file.id(), second_file.id()]
        .into_iter()
        .max_by_key(|node_id| node_id.into_uuid())
        .expect("two file nodes exist");
    let suffix = Uuid::now_v7().simple().to_string();
    let function = format!("synveil_test_fail_backup_pin_{suffix}");
    let trigger = format!("synveil_test_fail_backup_pin_trigger_{suffix}");
    let function_sql = format!(
        "CREATE FUNCTION {function}() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             IF NEW.manifest_node_id = '{}'::UUID THEN
                 RAISE EXCEPTION 'injected backup pin failure';
             END IF;
             RETURN NEW;
         END;
         $$",
        fail_node_id.into_uuid()
    );
    sqlx::query(&function_sql)
        .execute(&fixture.inspection)
        .await
        .expect("failure-injection function must persist");
    sqlx::query(&format!(
        "CREATE TRIGGER {trigger}
         BEFORE INSERT ON backup_snapshot_content_pins
         FOR EACH ROW EXECUTE FUNCTION {function}()"
    ))
    .execute(&fixture.inspection)
    .await
    .expect("failure-injection trigger must persist");

    let capture = BackupService::new(fixture.pool.clone())
        .capture_snapshot(
            fixture.user_id,
            set_id,
            snapshot_id,
            "retention-rollback-0001".to_owned(),
        )
        .await;
    assert!(capture.is_err(), "injected pin failure must abort capture");

    sqlx::query(&format!(
        "DROP TRIGGER {trigger} ON backup_snapshot_content_pins"
    ))
    .execute(&fixture.inspection)
    .await
    .expect("failure-injection trigger must drop");
    sqlx::query(&format!("DROP FUNCTION {function}()"))
        .execute(&fixture.inspection)
        .await
        .expect("failure-injection function must drop");

    let committed_counts = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT
            (SELECT count(*) FROM backup_snapshots WHERE id = $1),
            (SELECT count(*) FROM backup_snapshot_nodes WHERE snapshot_id = $1),
            (SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1)",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("rollback counts must load");
    assert_eq!(committed_counts, (0, 0, 0));
}

/// A retained V1 is independent from its live Node/FileVersion history. Once
/// V2 advances the live file and the real Trash-retention lifecycle purges the
/// node, V1's pin remains, no candidate is produced for V1, and lifecycle-only
/// expiry still leaves both the pin and the GC block intact.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_backup_pin_survives_live_purge_blocks_stale_gc_and_expiry() {
    let fixture = fixture().await;
    let set_id = create_set(&fixture, "purge-survival").await;
    let backup = BackupService::new(fixture.pool.clone());
    let snapshot = backup
        .capture_snapshot(
            fixture.user_id,
            set_id,
            SnapshotId::new(),
            "retention-purge-v1-0001".to_owned(),
        )
        .await
        .expect("V1 snapshot must complete");
    assert_eq!(
        count_snapshot_pins(&fixture.inspection, snapshot.id()).await,
        1
    );

    // Advance the live file through a valid immutable version/head update.
    let repository = DomainRepository::new(&fixture.pool);
    let file_service = FileMetadataService::new(fixture.pool.clone());
    let current_file = file_service
        .get_node(fixture.user_id, fixture.file.id())
        .await
        .expect("live file must load");
    let object_v2 = ObjectReference::new(
        ObjectId::new(),
        fixture.library.dedup_domain_id(),
        Sha256Digest::from_bytes([0xbc; 32]),
        4_096,
    );
    repository
        .insert_object(object_v2, timestamp("2026-08-29T00:00:02.123456Z"))
        .await
        .expect("V2 object must persist");
    let version_v2 = FileVersion::new(
        FileVersionId::new(),
        &fixture.library,
        &current_file,
        object_v2,
        Some(fixture.version_v1.id()),
        timestamp("2026-08-29T00:00:02.123456Z"),
    )
    .expect("V2 version is valid");
    repository
        .insert_file_version(version_v2)
        .await
        .expect("V2 version must persist");
    let advanced_file = current_file
        .with_current_version(&version_v2, timestamp("2026-08-29T00:00:02.123456Z"))
        .expect("live file must advance to V2");
    repository
        .update_node(&advanced_file)
        .await
        .expect("live V2 head must persist");

    let trashed = file_service
        .delete_node(
            fixture.user_id,
            advanced_file.id(),
            advanced_file.revision(),
        )
        .await
        .expect("live V2 file must enter Trash through the real lifecycle");
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
        TrashRetentionPolicy::new(Duration::from_secs(1)).expect("short policy is valid"),
    );
    let purging = retention
        .begin_node_purge(fixture.user_id, trashed.id(), trashed.revision())
        .await
        .expect("expired live file must begin purge");
    assert_eq!(
        retention
            .execute_metadata_purge(fixture.user_id, purging.id(), purging.revision())
            .await
            .expect("metadata purge must complete"),
        PurgeExecutionResult::Completed
    );

    let post_purge = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT
            (SELECT count(*) FROM file_versions WHERE id = $1),
            (SELECT count(*) FROM backup_snapshot_content_pins
             WHERE snapshot_id = $2 AND file_version_id = $1),
            (SELECT count(*) FROM objects WHERE id = $3 AND dedup_domain_id = $4)",
    )
    .bind(fixture.version_v1.id().into_uuid())
    .bind(snapshot.id().into_uuid())
    .bind(fixture.object_v1.object_id().into_uuid())
    .bind(fixture.object_v1.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("post-purge retention rows must load");
    assert_eq!(post_purge, (0, 1, 1));
    assert_eq!(
        count_gc_candidates(&fixture.inspection, fixture.object_v1).await,
        0
    );

    // Expiration is lifecycle-only. The private pin stays and still defeats a
    // deliberately injected stale candidate through the normal planner.
    sqlx::query(
        "UPDATE backup_snapshots
         SET state = 'EXPIRED', expired_at = CURRENT_TIMESTAMP
         WHERE id = $1 AND state = 'COMPLETED'",
    )
    .bind(snapshot.id().into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("snapshot expiry metadata must persist");
    let expiry_state = sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT state,
                (SELECT count(*) FROM backup_snapshot_nodes WHERE snapshot_id = $1),
                (SELECT count(*) FROM backup_snapshot_content_pins WHERE snapshot_id = $1)
         FROM backup_snapshots
         WHERE id = $1",
    )
    .bind(snapshot.id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("expired snapshot must remain inspectable internally");
    assert_eq!(expiry_state.0, "EXPIRED");
    assert_eq!(expiry_state.2, 1);
    assert!(expiry_state.1 > 0, "expiry must not delete the manifest");

    sqlx::query(
        "INSERT INTO object_gc_candidates
            (object_id, object_dedup_domain_id, unreferenced_at, source)
         VALUES ($1, $2, clock_timestamp() - INTERVAL '60 seconds', 'METADATA_PURGE')",
    )
    .bind(fixture.object_v1.object_id().into_uuid())
    .bind(fixture.object_v1.dedup_domain_id().into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("stale candidate fixture must persist");
    let planner = ObjectGcPlanningService::new(
        fixture.pool.clone(),
        ObjectGcPolicy::new(Duration::from_secs(1), Duration::from_secs(30), 1)
            .expect("focused GC policy is valid"),
    );
    assert!(
        planner
            .claim_candidates(1)
            .await
            .expect("planner must revalidate stale candidate")
            .is_empty(),
        "a retained backup Object cannot obtain a destructive lease"
    );
    assert_eq!(
        count_gc_candidates(&fixture.inspection, fixture.object_v1).await,
        0
    );
    assert_eq!(
        count_snapshot_pins(&fixture.inspection, snapshot.id()).await,
        1
    );
}

/// Recovery workers are another authoritative revalidation path. A persisted
/// backup pin that appears on an incomplete physical-GC operation is treated
/// as a reference conflict and escalated to the existing terminal
/// `NEEDS_ATTENTION` path; the worker must never issue a replacement lease.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_backup_pin_blocks_gc_worker_recovery_revalidation() {
    let fixture = fixture().await;
    let set_id = create_set(&fixture, "worker-revalidation").await;
    let snapshot = BackupService::new(fixture.pool.clone())
        .capture_snapshot(
            fixture.user_id,
            set_id,
            SnapshotId::new(),
            "retention-worker-revalidation-0001".to_owned(),
        )
        .await
        .expect("snapshot must complete");
    let policy = ObjectGcPolicy::new(Duration::from_secs(1), Duration::from_secs(30), 1)
        .expect("focused GC policy is valid");
    let repository = DomainRepository::new(&fixture.pool);
    let object = ObjectReference::new(
        ObjectId::new(),
        fixture.library.dedup_domain_id(),
        Sha256Digest::from_bytes([0xf1; 32]),
        0,
    );
    repository
        .insert_object(object, timestamp("2026-08-29T00:00:03.123456Z"))
        .await
        .expect("unreferenced worker object must persist");
    sqlx::query(
        "INSERT INTO object_gc_candidates
            (object_id, object_dedup_domain_id, unreferenced_at, source)
         VALUES ($1, $2, clock_timestamp() - INTERVAL '60 seconds', 'METADATA_PURGE')",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("candidate must persist");
    let planner = ObjectGcPlanningService::new(fixture.pool.clone(), policy);
    let lease = planner
        .claim_candidates(1)
        .await
        .expect("candidate claim must succeed")
        .pop()
        .expect("unreferenced candidate must be leased");
    let ready = match planner
        .mark_ready_for_deletion(lease)
        .await
        .expect("candidate ready transition must succeed")
    {
        ObjectGcPlanResult::Valid(candidate) => candidate.lease().expect("READY lease exists"),
        ObjectGcPlanResult::Invalidated => panic!("unreferenced candidate was invalidated"),
    };
    let operation = PostgresObjectGcExecutionRepository::new(fixture.pool.clone(), policy)
        .start_gc_execution(ready)
        .await
        .expect("zero-replica physical operation must start");

    // Normal backup capture cannot create this row after `GC_DELETING`; use a
    // disposable transaction that temporarily disables user triggers to model
    // persisted drift. The triggers are restored before the row commits, so
    // this cannot become a production maintenance or force-insert path.
    let mut injection = fixture
        .inspection
        .begin()
        .await
        .expect("test-only corruption transaction must begin");
    sqlx::query("ALTER TABLE backup_snapshot_content_pins DISABLE TRIGGER USER")
        .execute(&mut *injection)
        .await
        .expect("test-only pin invariant bypass must disable user triggers");
    sqlx::query(
        "INSERT INTO backup_snapshot_content_pins
            (snapshot_id, manifest_node_id, file_version_id,
             object_id, object_dedup_domain_id, created_at)
         VALUES ($1, $2, $3, $4, $5, clock_timestamp())",
    )
    .bind(snapshot.id().into_uuid())
    .bind(NodeId::new().into_uuid())
    .bind(FileVersionId::new().into_uuid())
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .execute(&mut *injection)
    .await
    .expect("fault-injected backup pin must persist");
    sqlx::query("ALTER TABLE backup_snapshot_content_pins ENABLE TRIGGER USER")
        .execute(&mut *injection)
        .await
        .expect("test-only pin invariant bypass must re-enable user triggers");
    injection
        .commit()
        .await
        .expect("test-only corruption transaction must restore invariants");
    sqlx::query(
        "UPDATE object_gc_candidates
         SET lease_acquired_at = clock_timestamp() - INTERVAL '2 seconds',
             lease_expires_at = clock_timestamp() - INTERVAL '1 second'
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("recovery lease must become due");

    let recovery = PostgresObjectGcWorkerRepository::new(fixture.pool.clone(), policy);
    assert!(
        recovery
            .claim_recoverable_operations(1)
            .await
            .expect("recovery recheck must succeed")
            .is_empty(),
        "backup pin must deny a replacement recovery lease"
    );
    let recovery_state = sqlx::query_as::<_, (String, Option<String>, i64)>(
        "SELECT operation.state, operation.last_error_code,
                (SELECT count(*) FROM backup_snapshot_content_pins
                 WHERE object_id = $2 AND object_dedup_domain_id = $3)
         FROM object_gc_operations AS operation
         WHERE operation.operation_id = $1",
    )
    .bind(operation.operation_id().into_uuid())
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("recovery result must persist");
    assert_eq!(
        recovery_state,
        (
            "NEEDS_ATTENTION".to_owned(),
            Some("gc_reference_conflict".to_owned()),
            1,
        )
    );
}
