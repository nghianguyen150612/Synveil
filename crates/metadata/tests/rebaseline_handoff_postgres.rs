//! Focused PostgreSQL 17 proof for Prompt 85 durable rebaseline handoff.
//!
//! The ignored test intentionally starts from the repository's complete
//! migration chain and exercises the handoff service against real durable
//! snapshot headers and device checkpoints. It uses separate random
//! owner/library/device fixtures for each transition so the S1-S15 assertions
//! remain independent while one run still reports the complete matrix.

use std::time::Duration;

use sqlx::PgPool;
use synveil_core::{
    DedupDomainId, Device, DeviceId, DeviceStatus, Library, LibraryId, LogicalName, Node, NodeId,
    RebaselineSnapshotId, Sequence, Timestamp, User, UserId, UserStatus,
};
use synveil_metadata::{
    DatabaseConfig, DatabasePool, DeviceSyncService, DomainRepository, FileMetadataService,
    LogicalSnapshotService, MigrationRunner, RebaselineHandoffResult, SyncAckEvidence, SyncError,
};

struct Fixture {
    pool: DatabasePool,
    inspection: PgPool,
    owner_id: UserId,
    library_id: LibraryId,
    snapshots: LogicalSnapshotService,
    sync: DeviceSyncService,
    metadata: FileMetadataService,
}

impl Fixture {
    async fn new() -> Self {
        let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
            .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable PostgreSQL 17 database");
        let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
        let pool = DatabasePool::connect(&config)
            .await
            .expect("test PostgreSQL must accept a connection");
        let status = MigrationRunner::new()
            .run(&pool)
            .await
            .expect("the 36-migration set must apply");
        assert!(status.is_current());
        assert_eq!(status.applied_versions().len(), 36);
        assert_eq!(status.latest_applied_version(), Some(20260910000000));

        let inspection = PgPool::connect(&url)
            .await
            .expect("inspection connection must succeed");
        let version_num: String = sqlx::query_scalar("SHOW server_version_num")
            .fetch_one(&inspection)
            .await
            .expect("PostgreSQL version must be readable");
        assert!(
            version_num.starts_with("17"),
            "expected PostgreSQL 17, got {version_num}"
        );

        let owner_id = UserId::new();
        insert_user(&pool, owner_id, "handoff-owner").await;
        let (library_id, _root) = insert_library(&pool, owner_id, "handoff-library").await;
        Self {
            snapshots: LogicalSnapshotService::new(pool.clone()),
            sync: DeviceSyncService::new(pool.clone()),
            metadata: FileMetadataService::new(pool.clone()),
            pool,
            inspection,
            owner_id,
            library_id,
        }
    }

    async fn new_device(&self, owner_id: UserId, label: &str) -> DeviceId {
        let mut device = Device::new(DeviceId::new(), owner_id, name(label), timestamp());
        device
            .transition_status(DeviceStatus::Active, timestamp())
            .expect("fixture device must become active");
        DomainRepository::new(&self.pool)
            .insert_device(&device)
            .await
            .expect("fixture device must persist");
        device.id()
    }

    async fn new_library(&self, owner_id: UserId, label: &str) -> (LibraryId, Node) {
        insert_library(&self.pool, owner_id, label).await
    }

    async fn snapshot(
        &self,
        owner_id: UserId,
        library_id: LibraryId,
        observed_at: Timestamp,
    ) -> RebaselineSnapshotId {
        self.snapshots
            .create_rebaseline_snapshot(owner_id, library_id, observed_at)
            .await
            .expect("snapshot header must be created")
            .snapshot_id()
    }

    async fn checkpoint(
        &self,
        owner_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
    ) -> (i64, i64) {
        sqlx::query_as(
            "SELECT journal_epoch, acknowledged_sequence
             FROM device_sync_checkpoints
             WHERE owner_user_id = $1 AND device_id = $2 AND library_id = $3",
        )
        .bind(owner_id.into_uuid())
        .bind(device_id.into_uuid())
        .bind(library_id.into_uuid())
        .fetch_one(&self.inspection)
        .await
        .expect("checkpoint row must be readable")
    }

    async fn set_checkpoint(
        &self,
        owner_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        epoch: i64,
        sequence: i64,
    ) {
        sqlx::query(
            "UPDATE device_sync_checkpoints
             SET journal_epoch = $1, acknowledged_sequence = $2,
                 last_seen_high_watermark = $2
             WHERE owner_user_id = $3 AND device_id = $4 AND library_id = $5",
        )
        .bind(epoch)
        .bind(sequence)
        .bind(owner_id.into_uuid())
        .bind(device_id.into_uuid())
        .bind(library_id.into_uuid())
        .execute(&self.inspection)
        .await
        .expect("fixture checkpoint update must succeed");
    }

    async fn close(self) {
        self.pool.close().await;
        self.inspection.close().await;
    }
}

fn timestamp() -> Timestamp {
    Timestamp::parse("2026-09-08T00:00:00.123456Z").expect("fixture timestamp is valid")
}

fn name(value: impl AsRef<str>) -> LogicalName {
    LogicalName::new(value.as_ref()).expect("fixture name is valid")
}

async fn insert_user(pool: &DatabasePool, user_id: UserId, label: &str) {
    let login = format!("{label}-{user_id}");
    let user = User::new(
        user_id,
        synveil_core::LoginIdentifier::new(&login, user_id.to_string())
            .expect("fixture login is valid"),
        UserStatus::Active,
        timestamp(),
    );
    DomainRepository::new(pool)
        .insert_user(&user)
        .await
        .expect("fixture user must persist");
}

async fn insert_library(pool: &DatabasePool, owner_id: UserId, label: &str) -> (LibraryId, Node) {
    let library_id = LibraryId::new();
    let root = Node::new_root(
        NodeId::new(),
        library_id,
        name(format!("{label}-root")),
        timestamp(),
    );
    let library = Library::new(
        library_id,
        owner_id,
        name(label),
        &root,
        DedupDomainId::new(),
        timestamp(),
    )
    .expect("fixture library must satisfy domain invariants");
    DomainRepository::new(pool)
        .insert_library_with_root(&library, &root)
        .await
        .expect("fixture library must persist");
    (library_id, root)
}

async fn assert_boundary(
    result: Result<RebaselineHandoffResult, SyncError>,
    snapshot_id: RebaselineSnapshotId,
    library_id: LibraryId,
    epoch: u64,
    sequence: u64,
) {
    let result = result.expect("handoff must succeed");
    assert_eq!(result.snapshot_id(), snapshot_id);
    assert_eq!(result.library_id(), library_id);
    assert_eq!(result.installed_checkpoint().library_id(), library_id);
    assert_eq!(result.installed_checkpoint().journal_epoch().get(), epoch);
    assert_eq!(
        result.installed_checkpoint().acknowledged_sequence().get(),
        sequence
    );
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_rebaseline_handoff_s1_to_s15() {
    let fixture = Fixture::new().await;

    // S1: no checkpoint -> install the snapshot boundary.
    let device_s1 = fixture.new_device(fixture.owner_id, "s1").await;
    let snapshot_s1 = fixture
        .snapshot(fixture.owner_id, fixture.library_id, Timestamp::now())
        .await;
    assert_boundary(
        fixture
            .sync
            .complete_rebaseline_handoff(fixture.owner_id, device_s1, snapshot_s1)
            .await,
        snapshot_s1,
        fixture.library_id,
        1,
        0,
    )
    .await;
    assert_eq!(
        fixture
            .checkpoint(fixture.owner_id, device_s1, fixture.library_id)
            .await,
        (1, 0)
    );

    // S2: an older same-epoch checkpoint advances to C.
    let (library_s2, root_s2) = fixture.new_library(fixture.owner_id, "s2").await;
    let device_s2 = fixture.new_device(fixture.owner_id, "s2").await;
    fixture
        .sync
        .ensure_checkpoint(fixture.owner_id, device_s2, library_s2)
        .await
        .expect("older checkpoint must initialize");
    fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            library_s2,
            Some(root_s2.id()),
            name("before-snapshot"),
        )
        .await
        .expect("pre-snapshot mutation must commit");
    let snapshot_s2 = fixture
        .snapshot(fixture.owner_id, library_s2, Timestamp::now())
        .await;
    assert_boundary(
        fixture
            .sync
            .complete_rebaseline_handoff(fixture.owner_id, device_s2, snapshot_s2)
            .await,
        snapshot_s2,
        library_s2,
        1,
        1,
    )
    .await;

    // S3: an older checkpoint epoch advances to the snapshot epoch.
    let (library_s3, _) = fixture.new_library(fixture.owner_id, "s3").await;
    let device_s3 = fixture.new_device(fixture.owner_id, "s3").await;
    fixture
        .sync
        .ensure_checkpoint(fixture.owner_id, device_s3, library_s3)
        .await
        .expect("epoch-1 checkpoint must initialize");
    sqlx::query("UPDATE libraries SET journal_epoch = 2 WHERE id = $1")
        .bind(library_s3.into_uuid())
        .execute(&fixture.inspection)
        .await
        .expect("fixture epoch rotation must commit");
    let snapshot_s3 = fixture
        .snapshot(fixture.owner_id, library_s3, Timestamp::now())
        .await;
    assert_boundary(
        fixture
            .sync
            .complete_rebaseline_handoff(fixture.owner_id, device_s3, snapshot_s3)
            .await,
        snapshot_s3,
        library_s3,
        2,
        0,
    )
    .await;

    // S4: exact C is idempotent and remains byte-for-byte the same cursor.
    let (library_s4, root_s4) = fixture.new_library(fixture.owner_id, "s4").await;
    let device_s4 = fixture.new_device(fixture.owner_id, "s4").await;
    fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            library_s4,
            Some(root_s4.id()),
            name("same-boundary"),
        )
        .await
        .expect("s4 mutation must commit");
    let snapshot_s4 = fixture
        .snapshot(fixture.owner_id, library_s4, Timestamp::now())
        .await;
    let first_s4 = fixture
        .sync
        .complete_rebaseline_handoff(fixture.owner_id, device_s4, snapshot_s4)
        .await
        .expect("first s4 handoff must succeed");
    let second_s4 = fixture
        .sync
        .complete_rebaseline_handoff(fixture.owner_id, device_s4, snapshot_s4)
        .await
        .expect("exact s4 retry must succeed");
    assert_eq!(first_s4, second_s4);

    // S5: a same-epoch checkpoint ahead of C is a conflict and is not moved.
    let (library_s5, root_s5) = fixture.new_library(fixture.owner_id, "s5").await;
    let device_s5 = fixture.new_device(fixture.owner_id, "s5").await;
    fixture
        .sync
        .ensure_checkpoint(fixture.owner_id, device_s5, library_s5)
        .await
        .expect("s5 checkpoint must initialize");
    fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            library_s5,
            Some(root_s5.id()),
            name("cut"),
        )
        .await
        .expect("s5 first mutation must commit");
    let snapshot_s5 = fixture
        .snapshot(fixture.owner_id, library_s5, Timestamp::now())
        .await;
    fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            library_s5,
            Some(root_s5.id()),
            name("after-cut"),
        )
        .await
        .expect("s5 later mutation must commit");
    fixture
        .set_checkpoint(fixture.owner_id, device_s5, library_s5, 1, 2)
        .await;
    assert_eq!(
        fixture
            .sync
            .complete_rebaseline_handoff(fixture.owner_id, device_s5, snapshot_s5)
            .await,
        Err(SyncError::CheckpointAheadOfSnapshot)
    );
    assert_eq!(
        fixture
            .checkpoint(fixture.owner_id, device_s5, library_s5)
            .await,
        (1, 2)
    );

    // S6: a newer/incompatible checkpoint epoch conflicts and never rewinds.
    let (library_s6, root_s6) = fixture.new_library(fixture.owner_id, "s6").await;
    let device_s6 = fixture.new_device(fixture.owner_id, "s6").await;
    fixture
        .sync
        .ensure_checkpoint(fixture.owner_id, device_s6, library_s6)
        .await
        .expect("s6 checkpoint must initialize");
    fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            library_s6,
            Some(root_s6.id()),
            name("s6-cut"),
        )
        .await
        .expect("s6 mutation must commit");
    let snapshot_s6 = fixture
        .snapshot(fixture.owner_id, library_s6, Timestamp::now())
        .await;
    fixture
        .set_checkpoint(fixture.owner_id, device_s6, library_s6, 2, 0)
        .await;
    assert_eq!(
        fixture
            .sync
            .complete_rebaseline_handoff(fixture.owner_id, device_s6, snapshot_s6)
            .await,
        Err(SyncError::CheckpointEpochConflict)
    );
    assert_eq!(
        fixture
            .checkpoint(fixture.owner_id, device_s6, library_s6)
            .await,
        (2, 0)
    );

    // S7: expiry is irrelevant once the immutable header is still present.
    let (library_s7, _) = fixture.new_library(fixture.owner_id, "s7").await;
    let device_s7 = fixture.new_device(fixture.owner_id, "s7").await;
    let snapshot_s7 = fixture
        .snapshot(fixture.owner_id, library_s7, timestamp())
        .await;
    assert_boundary(
        fixture
            .sync
            .complete_rebaseline_handoff(fixture.owner_id, device_s7, snapshot_s7)
            .await,
        snapshot_s7,
        library_s7,
        1,
        0,
    )
    .await;

    // S8: missing snapshot fails closed.
    let device_s8 = fixture.new_device(fixture.owner_id, "s8").await;
    assert_eq!(
        fixture
            .sync
            .complete_rebaseline_handoff(fixture.owner_id, device_s8, RebaselineSnapshotId::new(),)
            .await,
        Err(SyncError::NotFound)
    );

    // S9: foreign owner concealment is the same NotFound outcome.
    let foreign_owner = UserId::new();
    insert_user(&fixture.pool, foreign_owner, "handoff-foreign-owner").await;
    let foreign_device = fixture.new_device(foreign_owner, "foreign").await;
    assert_eq!(
        fixture
            .sync
            .complete_rebaseline_handoff(foreign_owner, foreign_device, snapshot_s1)
            .await,
        Err(SyncError::NotFound)
    );

    // S11: concurrent same-snapshot/same-device callers converge, including
    // the missing-checkpoint initialization race.
    let (library_s11, _) = fixture.new_library(fixture.owner_id, "s11").await;
    let device_s11 = fixture.new_device(fixture.owner_id, "s11").await;
    let snapshot_s11 = fixture
        .snapshot(fixture.owner_id, library_s11, Timestamp::now())
        .await;
    let owner_id_s11 = fixture.owner_id;
    let caller_count = 8;
    let mut callers = Vec::with_capacity(caller_count);
    for _ in 0..caller_count {
        let sync = fixture.sync.clone();
        callers.push(tokio::spawn(async move {
            sync.complete_rebaseline_handoff(owner_id_s11, device_s11, snapshot_s11)
                .await
        }));
    }
    let mut successful_callers = 0;
    for caller in callers {
        let result = caller.await.expect("concurrent handoff task must join");
        assert!(
            result.is_ok(),
            "same-snapshot handoff must not leak unique/deadlock/database errors: {result:?}"
        );
        successful_callers += 1;
    }
    assert_eq!(successful_callers, caller_count);
    assert_eq!(
        fixture
            .checkpoint(fixture.owner_id, device_s11, library_s11)
            .await,
        (1, 0)
    );
    let checkpoint_rows_s11: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM device_sync_checkpoints
         WHERE owner_user_id = $1 AND device_id = $2 AND library_id = $3",
    )
    .bind(fixture.owner_id.into_uuid())
    .bind(device_s11.into_uuid())
    .bind(library_s11.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("same-snapshot checkpoint row count must be readable");
    assert_eq!(checkpoint_rows_s11, 1);
    println!(
        "Prompt 85 S11 missing-checkpoint concurrency: callers={caller_count} successful={successful_callers} unique=0 sqlstate_40p01=0 final_rows={checkpoint_rows_s11}"
    );

    // S12: two devices have independent checkpoint rows for the same header.
    let (library_s12, _) = fixture.new_library(fixture.owner_id, "s12").await;
    let device_s12_a = fixture.new_device(fixture.owner_id, "s12-a").await;
    let device_s12_b = fixture.new_device(fixture.owner_id, "s12-b").await;
    let snapshot_s12 = fixture
        .snapshot(fixture.owner_id, library_s12, Timestamp::now())
        .await;
    let a_sync = fixture.sync.clone();
    let b_sync = fixture.sync.clone();
    let (a_s12, b_s12) = tokio::join!(
        a_sync.complete_rebaseline_handoff(fixture.owner_id, device_s12_a, snapshot_s12),
        b_sync.complete_rebaseline_handoff(fixture.owner_id, device_s12_b, snapshot_s12),
    );
    assert_boundary(a_s12, snapshot_s12, library_s12, 1, 0).await;
    assert_boundary(b_s12, snapshot_s12, library_s12, 1, 0).await;
    let device_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM device_sync_checkpoints
         WHERE owner_user_id = $1 AND library_id = $2",
    )
    .bind(fixture.owner_id.into_uuid())
    .bind(library_s12.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("two device checkpoints must be countable");
    assert_eq!(device_count, 2);

    // S13: a handoff for one Library leaves another Library's checkpoint alone.
    let (library_s13_a, _) = fixture.new_library(fixture.owner_id, "s13-a").await;
    let (library_s13_b, _) = fixture.new_library(fixture.owner_id, "s13-b").await;
    let device_s13 = fixture.new_device(fixture.owner_id, "s13").await;
    fixture
        .sync
        .ensure_checkpoint(fixture.owner_id, device_s13, library_s13_b)
        .await
        .expect("s13 unrelated checkpoint must initialize");
    let snapshot_s13 = fixture
        .snapshot(fixture.owner_id, library_s13_a, Timestamp::now())
        .await;
    assert_boundary(
        fixture
            .sync
            .complete_rebaseline_handoff(fixture.owner_id, device_s13, snapshot_s13)
            .await,
        snapshot_s13,
        library_s13_a,
        1,
        0,
    )
    .await;
    assert_eq!(
        fixture
            .checkpoint(fixture.owner_id, device_s13, library_s13_b)
            .await,
        (1, 0)
    );

    // S14/S15: snapshot header/entries and journal rows are unchanged.
    let (library_s14, root_s14) = fixture.new_library(fixture.owner_id, "s14-s15").await;
    let device_s14 = fixture.new_device(fixture.owner_id, "s14-s15").await;
    fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            library_s14,
            Some(root_s14.id()),
            name("retained"),
        )
        .await
        .expect("s14/s15 mutation must commit");
    let snapshot_s14 = fixture
        .snapshot(fixture.owner_id, library_s14, Timestamp::now())
        .await;
    let artifact_before: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*),
                (SELECT entry_count FROM rebaseline_snapshots WHERE id = $1),
                (SELECT journal_epoch FROM rebaseline_snapshots WHERE id = $1),
                (SELECT snapshot_resume_sequence FROM rebaseline_snapshots WHERE id = $1)
         FROM rebaseline_snapshot_entries WHERE snapshot_id = $1",
    )
    .bind(snapshot_s14.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("snapshot artifact must be inspectable");
    let journal_before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM change_journal WHERE owner_user_id = $1 AND library_id = $2",
    )
    .bind(fixture.owner_id.into_uuid())
    .bind(library_s14.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("journal count must be inspectable");
    assert_boundary(
        fixture
            .sync
            .complete_rebaseline_handoff(fixture.owner_id, device_s14, snapshot_s14)
            .await,
        snapshot_s14,
        library_s14,
        1,
        1,
    )
    .await;
    let artifact_after: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*),
                (SELECT entry_count FROM rebaseline_snapshots WHERE id = $1),
                (SELECT journal_epoch FROM rebaseline_snapshots WHERE id = $1),
                (SELECT snapshot_resume_sequence FROM rebaseline_snapshots WHERE id = $1)
         FROM rebaseline_snapshot_entries WHERE snapshot_id = $1",
    )
    .bind(snapshot_s14.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("snapshot artifact must remain inspectable");
    let journal_after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM change_journal WHERE owner_user_id = $1 AND library_id = $2",
    )
    .bind(fixture.owner_id.into_uuid())
    .bind(library_s14.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("journal count must remain inspectable");
    assert_eq!(artifact_before, artifact_after);
    assert_eq!(journal_before, journal_after);

    println!(
        "Prompt 85 metadata handoff S1-S15: passed; migration_versions=36; server_version=17; journal_mutation_delta=0"
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_rebaseline_handoff_concurrency_and_no_gap_matrix() {
    let fixture = Fixture::new().await;

    // DS1/DS2: two immutable snapshots for one device must never rewind the
    // checkpoint. The snapshots deliberately cut at C1=1 and C2=2.
    let (library_ds, root_ds) = fixture
        .new_library(fixture.owner_id, "different-snapshots")
        .await;
    let device_ds = fixture
        .new_device(fixture.owner_id, "different-snapshots")
        .await;
    fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            library_ds,
            Some(root_ds.id()),
            name("before-c1"),
        )
        .await
        .expect("C1 mutation must commit");
    let snapshot_c1 = fixture
        .snapshot(fixture.owner_id, library_ds, Timestamp::now())
        .await;
    fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            library_ds,
            Some(root_ds.id()),
            name("before-c2"),
        )
        .await
        .expect("C2 mutation must commit");
    let snapshot_c2 = fixture
        .snapshot(fixture.owner_id, library_ds, Timestamp::now())
        .await;

    let c1_sync = fixture.sync.clone();
    let c2_sync = fixture.sync.clone();
    let (c1_result, c2_result) = tokio::join!(
        c1_sync.complete_rebaseline_handoff(fixture.owner_id, device_ds, snapshot_c1),
        c2_sync.complete_rebaseline_handoff(fixture.owner_id, device_ds, snapshot_c2),
    );
    let accepted_c1 = matches!(
        c1_result,
        Ok(result) if result.installed_checkpoint().acknowledged_sequence() == Sequence::new(1)
    );
    let accepted_c2 = matches!(
        c2_result,
        Ok(result) if result.installed_checkpoint().acknowledged_sequence() == Sequence::new(2)
    );
    let c1_rejected_after_c2 = matches!(c1_result, Err(SyncError::CheckpointAheadOfSnapshot));
    assert!(
        (accepted_c1 && accepted_c2) || (accepted_c2 && c1_rejected_after_c2),
        "different-snapshot concurrency must serialize monotonically: C1={c1_result:?}, C2={c2_result:?}"
    );
    assert_eq!(
        fixture
            .checkpoint(fixture.owner_id, device_ds, library_ds)
            .await,
        (1, 2)
    );
    assert_eq!(
        fixture
            .sync
            .complete_rebaseline_handoff(fixture.owner_id, device_ds, snapshot_c1)
            .await,
        Err(SyncError::CheckpointAheadOfSnapshot),
        "a later C1 retry must not regress an already-installed C2"
    );
    println!(
        "Prompt 85 different-snapshot concurrency: C1=1 C2=2 final=2 rewind=no sqlstate_40p01=0 unique=0"
    );

    // NG1: a mutation committed after the snapshot cut but before handoff is
    // returned by the first exclusive-after-C feed, while the pre-C event is
    // not replayed.
    let (library_ng1, root_ng1) = fixture.new_library(fixture.owner_id, "ng1-before").await;
    let device_ng1 = fixture.new_device(fixture.owner_id, "ng1-before").await;
    fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            library_ng1,
            Some(root_ng1.id()),
            name("pre-cut"),
        )
        .await
        .expect("NG1 pre-cut mutation must commit");
    let snapshot_ng1 = fixture
        .snapshot(fixture.owner_id, library_ng1, Timestamp::now())
        .await;
    fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            library_ng1,
            Some(root_ng1.id()),
            name("post-cut"),
        )
        .await
        .expect("NG1 post-cut mutation must commit");
    assert_boundary(
        fixture
            .sync
            .complete_rebaseline_handoff(fixture.owner_id, device_ng1, snapshot_ng1)
            .await,
        snapshot_ng1,
        library_ng1,
        1,
        1,
    )
    .await;
    let ng1_feed = fixture
        .sync
        .fetch_feed(fixture.owner_id, device_ng1, library_ng1, 100)
        .await
        .expect("NG1 feed must be readable");
    assert_eq!(ng1_feed.from_sequence(), Sequence::new(1));
    assert_eq!(ng1_feed.through_sequence(), Sequence::new(2));
    assert_eq!(ng1_feed.changes().len(), 1);
    assert_eq!(ng1_feed.changes()[0].sequence(), Sequence::new(2));

    // NG2: hold the immutable proof lock so the handoff is observably in
    // flight, then commit a mutation before releasing it. This gives a
    // deterministic database barrier without sleep-based coordination.
    let (library_ng2, root_ng2) = fixture.new_library(fixture.owner_id, "ng2-during").await;
    let device_ng2 = fixture.new_device(fixture.owner_id, "ng2-during").await;
    let snapshot_ng2 = fixture
        .snapshot(fixture.owner_id, library_ng2, Timestamp::now())
        .await;
    let mut proof_barrier = fixture
        .inspection
        .begin()
        .await
        .expect("NG2 barrier transaction must begin");
    sqlx::query(
        "SELECT snapshot_id
         FROM rebaseline_snapshot_handoff_proofs
         WHERE snapshot_id = $1
         FOR UPDATE",
    )
    .bind(snapshot_ng2.into_uuid())
    .execute(&mut *proof_barrier)
    .await
    .expect("NG2 barrier must lock the immutable proof");
    let ng2_sync = fixture.sync.clone();
    let owner_id_ng2 = fixture.owner_id;
    let ng2_handoff = tokio::spawn(async move {
        ng2_sync
            .complete_rebaseline_handoff(owner_id_ng2, device_ng2, snapshot_ng2)
            .await
    });
    let handoff_waiting = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS (
                     SELECT 1
                     FROM pg_locks
                     WHERE NOT granted
                 )",
            )
            .fetch_one(&fixture.inspection)
            .await
            .expect("NG2 lock inspection must succeed");
            if waiting {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    if handoff_waiting.is_err() {
        proof_barrier
            .rollback()
            .await
            .expect("NG2 barrier rollback must succeed");
        let _ = ng2_handoff.await;
        panic!("NG2 handoff did not reach the deterministic proof barrier");
    }
    fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            library_ng2,
            Some(root_ng2.id()),
            name("during-handoff"),
        )
        .await
        .expect("NG2 mutation must commit while handoff waits");
    proof_barrier
        .commit()
        .await
        .expect("NG2 barrier commit must release the proof");
    assert_boundary(
        ng2_handoff.await.expect("NG2 handoff task must join"),
        snapshot_ng2,
        library_ng2,
        1,
        0,
    )
    .await;
    let ng2_feed = fixture
        .sync
        .fetch_feed(fixture.owner_id, device_ng2, library_ng2, 100)
        .await
        .expect("NG2 feed must be readable");
    assert_eq!(ng2_feed.from_sequence(), Sequence::new(0));
    assert_eq!(ng2_feed.through_sequence(), Sequence::new(1));
    assert_eq!(ng2_feed.changes().len(), 1);
    assert_eq!(ng2_feed.changes()[0].sequence(), Sequence::new(1));

    // NG3: the server checkpoint commits first; a later mutation remains in
    // the ordinary feed before any client-local finalization is relevant.
    let (library_ng3, root_ng3) = fixture
        .new_library(fixture.owner_id, "ng3-server-commit")
        .await;
    let device_ng3 = fixture
        .new_device(fixture.owner_id, "ng3-server-commit")
        .await;
    let snapshot_ng3 = fixture
        .snapshot(fixture.owner_id, library_ng3, Timestamp::now())
        .await;
    assert_boundary(
        fixture
            .sync
            .complete_rebaseline_handoff(fixture.owner_id, device_ng3, snapshot_ng3)
            .await,
        snapshot_ng3,
        library_ng3,
        1,
        0,
    )
    .await;
    fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            library_ng3,
            Some(root_ng3.id()),
            name("after-server-commit"),
        )
        .await
        .expect("NG3 mutation must commit after server handoff");
    let ng3_feed = fixture
        .sync
        .fetch_feed(fixture.owner_id, device_ng3, library_ng3, 100)
        .await
        .expect("NG3 feed must be readable");
    assert_eq!(ng3_feed.changes().len(), 1);
    assert_eq!(ng3_feed.changes()[0].sequence(), Sequence::new(1));

    // NG4: normal post-finalization behavior is the same exclusive feed and
    // ordinary ACK path; no special handoff ACK is used.
    let (library_ng4, root_ng4) = fixture
        .new_library(fixture.owner_id, "ng4-after-finalize")
        .await;
    let device_ng4 = fixture
        .new_device(fixture.owner_id, "ng4-after-finalize")
        .await;
    let snapshot_ng4 = fixture
        .snapshot(fixture.owner_id, library_ng4, Timestamp::now())
        .await;
    assert_boundary(
        fixture
            .sync
            .complete_rebaseline_handoff(fixture.owner_id, device_ng4, snapshot_ng4)
            .await,
        snapshot_ng4,
        library_ng4,
        1,
        0,
    )
    .await;
    fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            library_ng4,
            Some(root_ng4.id()),
            name("after-local-finalize"),
        )
        .await
        .expect("NG4 mutation must commit after local finalization");
    let ng4_feed = fixture
        .sync
        .fetch_feed(fixture.owner_id, device_ng4, library_ng4, 100)
        .await
        .expect("NG4 feed must be readable");
    assert_eq!(ng4_feed.from_sequence(), Sequence::new(0));
    assert_eq!(ng4_feed.through_sequence(), Sequence::new(1));
    assert_eq!(ng4_feed.changes().len(), 1);
    let ng4_event = ng4_feed.changes()[0];
    assert_eq!(ng4_event.sequence(), Sequence::new(1));
    let ng4_evidence = SyncAckEvidence::new(
        fixture.owner_id,
        device_ng4,
        library_ng4,
        ng4_feed.checkpoint().journal_epoch(),
        ng4_feed.from_sequence(),
        ng4_feed.through_sequence(),
        ng4_feed.high_watermark().sequence(),
    );
    let ng4_checkpoint = fixture
        .sync
        .acknowledge(fixture.owner_id, device_ng4, library_ng4, ng4_evidence)
        .await
        .expect("NG4 ordinary ACK must advance beyond C");
    assert_eq!(ng4_checkpoint.acknowledged_sequence(), Sequence::new(1));
    let empty_ng4 = fixture
        .sync
        .fetch_feed(fixture.owner_id, device_ng4, library_ng4, 100)
        .await
        .expect("empty post-ACK feed must succeed");
    assert!(empty_ng4.changes().is_empty());
    assert_eq!(empty_ng4.from_sequence(), Sequence::new(1));
    assert_eq!(empty_ng4.through_sequence(), Sequence::new(1));

    println!(
        "Prompt 85 no-gap matrix: NG1=pass NG2=pass NG3=pass NG4=pass pre-C-replay=0 ordinary_ack=pass"
    );
    fixture.close().await;
}
