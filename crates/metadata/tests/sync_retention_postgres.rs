//! PostgreSQL 17 acceptance matrix for Prompt 86 synchronization retention.
//!
//! Run serially against a fresh disposable database. The cases cover the
//! retained-floor, immutable-proof, bounded-cleanup, rollback, restart, and
//! concurrency contracts without introducing a scheduler or retry loop.

use std::{sync::Arc, time::Duration};

use sqlx::PgPool;
use synveil_core::{
    DedupDomainId, Device, DeviceId, DeviceStatus, Library, LibraryId, LogicalName, Node, NodeId,
    RebaselineSnapshotId, Sequence, Timestamp, User, UserId, UserStatus,
};
use synveil_metadata::{
    ChangeJournalService, DatabaseConfig, DatabasePool, DeviceSyncService, DomainRepository,
    FileMetadataService, JournalCursor, JournalError, LogicalSnapshotService, MigrationRunner,
    RebaselineReason, SnapshotError, SyncAckEvidence, SyncError, SyncRetentionError,
    SyncRetentionPolicy, SyncRetentionService,
};
use tokio::sync::Barrier;
use uuid::Uuid;

const DAY: u64 = 24 * 60 * 60;

struct Fixture {
    base_url: String,
    db_name: String,
    url: String,
    pool: DatabasePool,
    inspection: PgPool,
    owner_id: UserId,
    library_id: LibraryId,
    root: Node,
    metadata: FileMetadataService,
    snapshots: LogicalSnapshotService,
    sync: DeviceSyncService,
    retention: SyncRetentionService,
}

impl Fixture {
    async fn new() -> Self {
        let base_url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
            .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable PostgreSQL 17 database");
        let db_name = format!("p86_case_{}", Uuid::now_v7().simple());
        let maintenance = PgPool::connect(&base_url)
            .await
            .expect("maintenance connection must succeed");
        sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
            .execute(&maintenance)
            .await
            .expect("isolated retention database must be created");
        maintenance.close().await;
        let url = format!(
            "{}/{}",
            base_url
                .rsplit_once('/')
                .expect("test URL must have a database path")
                .0,
            db_name
        );
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
        insert_user(&pool, owner_id, "retention-owner").await;
        let (library_id, root) = insert_library(&pool, owner_id, "retention-library").await;
        Self {
            base_url,
            db_name,
            url,
            metadata: FileMetadataService::new(pool.clone()),
            snapshots: LogicalSnapshotService::new(pool.clone()),
            sync: DeviceSyncService::new(pool.clone()),
            retention: SyncRetentionService::new(pool.clone()),
            pool,
            inspection,
            owner_id,
            library_id,
            root,
        }
    }

    async fn add_library(&self, label: &str) -> (LibraryId, Node) {
        insert_library(&self.pool, self.owner_id, label).await
    }

    async fn add_device(&self, label: &str) -> DeviceId {
        insert_device(&self.pool, self.owner_id, label).await
    }

    async fn close(self) {
        self.pool.close().await;
        self.inspection.close().await;
        let maintenance = PgPool::connect(&self.base_url)
            .await
            .expect("maintenance reconnect must succeed");
        sqlx::query(&format!("DROP DATABASE \"{}\" WITH (FORCE)", self.db_name))
            .execute(&maintenance)
            .await
            .expect("isolated retention database must remove");
        maintenance.close().await;
    }
}

fn at() -> Timestamp {
    Timestamp::parse("2026-09-10T00:00:00.123456Z").expect("fixture timestamp is valid")
}

fn after(value: Timestamp, seconds: u64) -> Timestamp {
    value
        .checked_add_std(Duration::from_secs(seconds))
        .expect("fixture time arithmetic must be representable")
}

fn before(value: Timestamp, seconds: u64) -> Timestamp {
    value
        .checked_sub_std(Duration::from_secs(seconds))
        .expect("fixture time arithmetic must be representable")
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
        at(),
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
        at(),
    );
    let library = Library::new(
        library_id,
        owner_id,
        name(label),
        &root,
        DedupDomainId::new(),
        at(),
    )
    .expect("fixture library must satisfy domain invariants");
    DomainRepository::new(pool)
        .insert_library_with_root(&library, &root)
        .await
        .expect("fixture library must persist");
    (library_id, root)
}

async fn insert_device(pool: &DatabasePool, owner_id: UserId, label: &str) -> DeviceId {
    let mut device = Device::new(DeviceId::new(), owner_id, name(label), at());
    device
        .transition_status(DeviceStatus::Active, at())
        .expect("fixture device must activate");
    DomainRepository::new(pool)
        .insert_device(&device)
        .await
        .expect("fixture device must persist");
    device.id()
}

async fn seed_journal_range(
    inspection: &PgPool,
    owner_id: UserId,
    library_id: LibraryId,
    root_id: NodeId,
    epoch: i64,
    sequence_range: std::ops::RangeInclusive<i64>,
    occurred_at: Timestamp,
) {
    let first = *sequence_range.start();
    let last = *sequence_range.end();
    assert!(first > 0 && last >= first);
    let mut transaction = inspection
        .begin()
        .await
        .expect("journal fixture transaction must begin");
    let (current_epoch, current_head): (i64, i64) = sqlx::query_as(
        "SELECT journal_epoch, sync_head
         FROM libraries
         WHERE id = $1 AND owner_user_id = $2
         FOR UPDATE",
    )
    .bind(library_id.into_uuid())
    .bind(owner_id.into_uuid())
    .fetch_one(&mut *transaction)
    .await
    .expect("journal fixture head must lock");
    assert_eq!(current_epoch, epoch);
    assert_eq!(current_head + 1, first);
    sqlx::query("UPDATE libraries SET sync_head = $2 WHERE id = $1")
        .bind(library_id.into_uuid())
        .bind(last)
        .execute(&mut *transaction)
        .await
        .expect("journal fixture head must advance");
    let inserted = sqlx::query(
        "INSERT INTO change_journal
            (entry_id, owner_user_id, library_id, journal_epoch, sequence,
             schema_version, resource_kind, resource_id, change_kind,
             occurred_at, resource_revision, parent_node_id, node_kind,
             node_state, current_version_id)
         SELECT (
                    substr(md5($1::UUID::TEXT || ':' || value::TEXT), 1, 12)
                    || '7'
                    || substr(md5($1::UUID::TEXT || ':' || value::TEXT), 14, 3)
                    || '8'
                    || substr(md5($1::UUID::TEXT || ':' || value::TEXT), 18, 15)
                )::UUID,
                $2, $3, $4, value, 1, 'NODE', $5, 'NODE_RENAMED',
                $6, value::NUMERIC, NULL, 'DIRECTORY', 'ACTIVE', NULL
         FROM generate_series($7::BIGINT, $8::BIGINT) AS value",
    )
    .bind(Uuid::now_v7())
    .bind(owner_id.into_uuid())
    .bind(library_id.into_uuid())
    .bind(epoch)
    .bind(root_id.into_uuid())
    .bind(occurred_at.as_offset_datetime())
    .bind(first)
    .bind(last)
    .execute(&mut *transaction)
    .await
    .expect("journal fixture rows must insert")
    .rows_affected();
    assert_eq!(inserted, u64::try_from(last - first + 1).unwrap());
    transaction
        .commit()
        .await
        .expect("journal fixture transaction must commit");
}

async fn journal_state(inspection: &PgPool, library_id: LibraryId) -> (i64, i64, i64, i64) {
    sqlx::query_as(
        "SELECT journal_epoch, sync_head, minimum_retained_sequence,
                (SELECT count(*) FROM change_journal WHERE library_id = l.id)
         FROM libraries AS l WHERE l.id = $1",
    )
    .bind(library_id.into_uuid())
    .fetch_one(inspection)
    .await
    .expect("journal state must load")
}

async fn snapshot_counts(
    inspection: &PgPool,
    snapshot_id: RebaselineSnapshotId,
) -> (i64, i64, i64) {
    let payloads = sqlx::query_scalar("SELECT count(*) FROM rebaseline_snapshots WHERE id = $1")
        .bind(snapshot_id.into_uuid())
        .fetch_one(inspection)
        .await
        .expect("payload count must load");
    let entries = sqlx::query_scalar(
        "SELECT count(*) FROM rebaseline_snapshot_entries WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(inspection)
    .await
    .expect("entry count must load");
    let proofs = sqlx::query_scalar(
        "SELECT count(*) FROM rebaseline_snapshot_handoff_proofs WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(inspection)
    .await
    .expect("proof count must load");
    (payloads, entries, proofs)
}

async fn deadlocks(inspection: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT deadlocks FROM pg_stat_database WHERE datname = current_database()")
        .fetch_one(inspection)
        .await
        .expect("deadlock count must load")
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_retention_upgrade_35_to_36_backfills_proofs_and_preserves_state() {
    let base = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable PostgreSQL 17 server");
    let db_name = format!("p86_upgrade_{}", Uuid::now_v7().simple());
    let maintenance = PgPool::connect(&base)
        .await
        .expect("maintenance connection must succeed");
    sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
        .execute(&maintenance)
        .await
        .expect("upgrade fixture database must be created");
    let url = format!(
        "{}/{}",
        base.rsplit_once('/').expect("test URL must have a path").0,
        db_name
    );
    let stage = std::env::temp_dir().join(format!("p86_migrations_{}", Uuid::now_v7().simple()));
    std::fs::create_dir_all(&stage).expect("migration staging directory must exist");
    let migration_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
    let mut migrations = std::fs::read_dir(&migration_root)
        .expect("migrations directory must be readable")
        .map(|entry| {
            entry
                .expect("migration entry must read")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| {
            name.ends_with(".sql")
                && name.as_str() < "20260910000000_sync_retention_handoff_proofs.sql"
        })
        .collect::<Vec<_>>();
    migrations.sort();
    assert_eq!(migrations.len(), 35);
    for migration in &migrations {
        std::fs::copy(migration_root.join(migration), stage.join(migration))
            .expect("historical migration must stage byte-for-byte");
    }

    let pool = DatabasePool::connect(&DatabaseConfig::from_url(&url).unwrap())
        .await
        .expect("upgrade pool must connect");
    let inspection = PgPool::connect(&url)
        .await
        .expect("upgrade inspection pool must connect");
    let historical = MigrationRunner::from_path(&stage)
        .run(&pool)
        .await
        .expect("historical 35 migrations must apply");
    assert_eq!(historical.applied_versions().len(), 35);
    assert_eq!(historical.latest_applied_version(), Some(20260908000000));

    let owner_id = UserId::new();
    insert_user(&pool, owner_id, "upgrade-owner").await;
    let (library_id, root) = insert_library(&pool, owner_id, "upgrade-library").await;
    let device_id = insert_device(&pool, owner_id, "upgrade-device").await;
    DeviceSyncService::new(pool.clone())
        .ensure_checkpoint(owner_id, device_id, library_id)
        .await
        .expect("pre-upgrade checkpoint must persist");
    seed_journal_range(
        &inspection,
        owner_id,
        library_id,
        root.id(),
        1,
        1..=3,
        before(at(), 40 * DAY),
    )
    .await;
    let snapshot_id = RebaselineSnapshotId::new();
    sqlx::query(
        "INSERT INTO rebaseline_snapshots
            (id, owner_user_id, library_id, journal_epoch,
             snapshot_resume_sequence, entry_count, created_at, expires_at)
         VALUES ($1, $2, $3, 1, 3, 1, $4, $5)",
    )
    .bind(snapshot_id.into_uuid())
    .bind(owner_id.into_uuid())
    .bind(library_id.into_uuid())
    .bind(at().as_offset_datetime())
    .bind(after(at(), DAY).as_offset_datetime())
    .execute(&inspection)
    .await
    .expect("pre-upgrade snapshot header must persist");
    sqlx::query(
        "INSERT INTO rebaseline_snapshot_entries
            (snapshot_id, node_id, parent_node_id, name, kind, state,
             revision, current_version_id, content_length, content_sha256)
         VALUES ($1, $2, NULL, 'upgrade-root', 'DIRECTORY', 'ACTIVE',
                 0, NULL, NULL, NULL)",
    )
    .bind(snapshot_id.into_uuid())
    .bind(root.id().into_uuid())
    .execute(&inspection)
    .await
    .expect("pre-upgrade snapshot entry must persist");

    let upgraded = MigrationRunner::new()
        .run(&pool)
        .await
        .expect("migration 36 must upgrade schema 35");
    assert!(upgraded.is_current());
    assert_eq!(upgraded.applied_versions().len(), 36);
    assert_eq!(upgraded.latest_applied_version(), Some(20260910000000));
    let proof: (
        Uuid,
        Uuid,
        Uuid,
        i64,
        i64,
        time::OffsetDateTime,
        time::OffsetDateTime,
    ) = sqlx::query_as(
        "SELECT snapshot_id, owner_user_id, library_id, journal_epoch,
                    snapshot_resume_sequence, snapshot_expires_at, proof_expires_at
             FROM rebaseline_snapshot_handoff_proofs
             WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(&inspection)
    .await
    .expect("existing snapshot proof must backfill");
    assert_eq!(proof.0, snapshot_id.into_uuid());
    assert_eq!(proof.1, owner_id.into_uuid());
    assert_eq!(proof.2, library_id.into_uuid());
    assert_eq!((proof.3, proof.4), (1, 3));
    assert_eq!(proof.6 - proof.5, time::Duration::days(30));
    assert_eq!(snapshot_counts(&inspection, snapshot_id).await, (1, 1, 1));
    assert_eq!(journal_state(&inspection, library_id).await, (1, 3, 0, 3));
    let checkpoints: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM device_sync_checkpoints
         WHERE device_id = $1 AND library_id = $2",
    )
    .bind(device_id.into_uuid())
    .bind(library_id.into_uuid())
    .fetch_one(&inspection)
    .await
    .unwrap();
    assert_eq!(checkpoints, 1);

    pool.close().await;
    inspection.close().await;
    std::fs::remove_dir_all(&stage).expect("migration stage must remove");
    sqlx::query(&format!("DROP DATABASE \"{db_name}\" WITH (FORCE)"))
        .execute(&maintenance)
        .await
        .expect("upgrade fixture database must remove");
    maintenance.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_snapshot_proof_atomicity_payload_handoff_and_proof_lifecycle() {
    let fixture = Fixture::new().await;
    let foreign_owner = UserId::new();
    insert_user(&fixture.pool, foreign_owner, "foreign-retention-owner").await;
    let first = fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("before-snapshot"),
        )
        .await
        .expect("pre-snapshot event must commit");
    let descriptor = fixture
        .snapshots
        .create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, at())
        .await
        .expect("snapshot and proof must commit atomically");
    let snapshot_id = descriptor.snapshot_id();
    assert_eq!(descriptor.boundary().sequence(), Sequence::new(1));
    assert_eq!(
        snapshot_counts(&fixture.inspection, snapshot_id).await,
        (1, 2, 1)
    );
    let proof: (i64, i64, time::OffsetDateTime, time::OffsetDateTime) = sqlx::query_as(
        "SELECT journal_epoch, snapshot_resume_sequence,
                snapshot_expires_at, proof_expires_at
         FROM rebaseline_snapshot_handoff_proofs WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("new snapshot proof must exist");
    assert_eq!((proof.0, proof.1), (1, 1));
    assert_eq!(proof.3 - proof.2, time::Duration::days(30));

    sqlx::query(
        "CREATE FUNCTION synveil_test_fail_proof_insert() RETURNS trigger
         LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'proof insert failure'; END $$",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER synveil_test_fail_proof_insert
         BEFORE INSERT ON rebaseline_snapshot_handoff_proofs
         FOR EACH ROW EXECUTE FUNCTION synveil_test_fail_proof_insert()",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    assert!(matches!(
        fixture
            .snapshots
            .create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, at())
            .await,
        Err(SnapshotError::Database(_))
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM rebaseline_snapshots")
            .fetch_one(&fixture.inspection)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM rebaseline_snapshot_handoff_proofs")
            .fetch_one(&fixture.inspection)
            .await
            .unwrap(),
        1
    );
    sqlx::query(
        "DROP TRIGGER synveil_test_fail_proof_insert
         ON rebaseline_snapshot_handoff_proofs",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    sqlx::query("DROP FUNCTION synveil_test_fail_proof_insert()")
        .execute(&fixture.inspection)
        .await
        .unwrap();

    let immutable = sqlx::query(
        "UPDATE rebaseline_snapshot_handoff_proofs
         SET snapshot_resume_sequence = snapshot_resume_sequence + 1
         WHERE snapshot_id = $1",
    )
    .bind(snapshot_id.into_uuid())
    .execute(&fixture.inspection)
    .await;
    assert!(
        immutable.is_err(),
        "proof boundary updates must be rejected"
    );

    let second = fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("after-snapshot"),
        )
        .await
        .expect("post-snapshot event must commit");
    assert_ne!(first.id(), second.id());
    let expiry = after(at(), DAY);
    assert_eq!(
        fixture
            .retention
            .cleanup_snapshot_payloads_step(before(expiry, 1))
            .await
            .unwrap()
            .payloads_deleted(),
        0
    );
    let cleanup = fixture
        .retention
        .cleanup_snapshot_payloads_step(expiry)
        .await
        .expect("payload is eligible exactly at expiry");
    assert_eq!(cleanup.payloads_deleted(), 1);
    assert_eq!(cleanup.entries_deleted(), 2);
    assert_eq!(
        snapshot_counts(&fixture.inspection, snapshot_id).await,
        (0, 0, 1)
    );
    assert_eq!(
        fixture
            .snapshots
            .get_rebaseline_snapshot(fixture.owner_id, snapshot_id, expiry)
            .await,
        Err(SnapshotError::Expired)
    );
    assert_eq!(
        fixture
            .snapshots
            .read_rebaseline_snapshot_page(fixture.owner_id, snapshot_id, None, 32, expiry)
            .await,
        Err(SnapshotError::Expired)
    );
    assert_eq!(
        fixture
            .snapshots
            .get_rebaseline_snapshot(foreign_owner, snapshot_id, expiry)
            .await,
        Err(SnapshotError::NotFound)
    );

    let device_id = fixture.add_device("payload-pruned-handoff").await;
    let handoff = fixture
        .sync
        .complete_rebaseline_handoff(fixture.owner_id, device_id, snapshot_id)
        .await
        .expect("proof must authorize handoff after payload deletion");
    assert_eq!(
        handoff.installed_checkpoint().acknowledged_sequence(),
        descriptor.boundary().sequence()
    );
    let feed = fixture
        .sync
        .fetch_feed(fixture.owner_id, device_id, fixture.library_id, 100)
        .await
        .expect("feed after proved boundary must have no gap");
    assert_eq!(feed.from_sequence(), Sequence::new(1));
    assert_eq!(feed.changes().len(), 1);
    assert_eq!(feed.changes()[0].sequence(), Sequence::new(2));

    let proof_deadline = after(expiry, 30 * DAY);
    assert_eq!(
        fixture
            .retention
            .cleanup_handoff_proofs_step(before(proof_deadline, 1))
            .await
            .unwrap()
            .proofs_deleted(),
        0
    );
    assert_eq!(
        fixture
            .retention
            .cleanup_handoff_proofs_step(proof_deadline)
            .await
            .expect("proof is eligible exactly at its deadline")
            .proofs_deleted(),
        1
    );
    assert_eq!(
        snapshot_counts(&fixture.inspection, snapshot_id).await,
        (0, 0, 0)
    );
    let missing_device = fixture.add_device("missing-proof-handoff").await;
    assert_eq!(
        fixture
            .sync
            .complete_rebaseline_handoff(fixture.owner_id, missing_device, snapshot_id)
            .await,
        Err(SyncError::NotFound)
    );
    assert_eq!(
        fixture
            .snapshots
            .get_rebaseline_snapshot(fixture.owner_id, snapshot_id, proof_deadline)
            .await,
        Err(SnapshotError::NotFound)
    );
    let installed: i64 = sqlx::query_scalar(
        "SELECT acknowledged_sequence FROM device_sync_checkpoints
         WHERE device_id = $1 AND library_id = $2",
    )
    .bind(device_id.into_uuid())
    .bind(fixture.library_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(installed, 1, "proof cleanup must not mutate checkpoints");
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_journal_compaction_is_contiguous_bounded_durable_and_stale_safe() {
    let fixture = Fixture::new().await;
    let old = before(at(), 40 * DAY);
    seed_journal_range(
        &fixture.inspection,
        fixture.owner_id,
        fixture.library_id,
        fixture.root.id(),
        1,
        1..=25_005,
        old,
    )
    .await;
    let stale_device = fixture.add_device("stale-device").await;
    let checkpoint = fixture
        .sync
        .ensure_checkpoint(fixture.owner_id, stale_device, fixture.library_id)
        .await
        .expect("initial checkpoint must persist");
    assert_eq!(checkpoint.acknowledged_sequence(), Sequence::new(0));

    sqlx::query(
        "CREATE FUNCTION synveil_test_fail_journal_cleanup() RETURNS trigger
         LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'journal cleanup failure'; END $$",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER synveil_test_fail_journal_cleanup
         AFTER DELETE ON change_journal FOR EACH STATEMENT
         EXECUTE FUNCTION synveil_test_fail_journal_cleanup()",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    assert!(matches!(
        fixture
            .retention
            .compact_journal_step(fixture.owner_id, fixture.library_id, at())
            .await,
        Err(SyncRetentionError::Database(_))
    ));
    assert_eq!(
        journal_state(&fixture.inspection, fixture.library_id).await,
        (1, 25_005, 0, 25_005)
    );
    sqlx::query("DROP TRIGGER synveil_test_fail_journal_cleanup ON change_journal")
        .execute(&fixture.inspection)
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION synveil_test_fail_journal_cleanup()")
        .execute(&fixture.inspection)
        .await
        .unwrap();

    let step1 = fixture
        .retention
        .compact_journal_step(fixture.owner_id, fixture.library_id, at())
        .await
        .expect("first bounded journal step must succeed");
    assert_eq!(step1.rows_deleted(), 10_000);
    assert_eq!(step1.previous_compacted_through(), Sequence::new(0));
    assert_eq!(step1.new_compacted_through(), Sequence::new(10_000));
    assert!(!step1.blocked_by_age());
    assert!(!step1.blocked_by_handoff_proof());
    assert_eq!(
        journal_state(&fixture.inspection, fixture.library_id).await,
        (1, 25_005, 10_000, 15_005)
    );

    assert_eq!(
        fixture
            .sync
            .fetch_feed(fixture.owner_id, stale_device, fixture.library_id, 100)
            .await,
        Err(SyncError::RebaselineRequired {
            reason: RebaselineReason::HistoryUnavailable,
            current_epoch: Sequence::new(1),
            minimum_retained_sequence: Sequence::new(10_000),
        })
    );
    let late_ack = SyncAckEvidence::new(
        fixture.owner_id,
        stale_device,
        fixture.library_id,
        Sequence::new(1),
        Sequence::new(0),
        Sequence::new(1),
        Sequence::new(25_005),
    );
    assert_eq!(
        fixture
            .sync
            .acknowledge(fixture.owner_id, stale_device, fixture.library_id, late_ack,)
            .await,
        Err(SyncError::RebaselineRequired {
            reason: RebaselineReason::HistoryUnavailable,
            current_epoch: Sequence::new(1),
            minimum_retained_sequence: Sequence::new(10_000),
        })
    );
    let journal = ChangeJournalService::new(fixture.pool.clone());
    assert_eq!(
        journal
            .list_changes(fixture.owner_id, fixture.library_id, None, 100)
            .await,
        Err(JournalError::CursorExpired)
    );
    let at_floor =
        JournalCursor::new(fixture.library_id, Sequence::new(1), Sequence::new(10_000)).encode();
    let floor_page = journal
        .list_changes(fixture.owner_id, fixture.library_id, Some(at_floor), 2)
        .await
        .expect("cursor exactly at floor remains valid");
    assert_eq!(
        floor_page
            .events()
            .iter()
            .map(|event| event.sequence().get())
            .collect::<Vec<_>>(),
        vec![10_001, 10_002]
    );
    let above_floor =
        JournalCursor::new(fixture.library_id, Sequence::new(1), Sequence::new(10_001)).encode();
    assert_eq!(
        journal
            .list_changes(fixture.owner_id, fixture.library_id, Some(above_floor), 1,)
            .await
            .unwrap()
            .events()[0]
            .sequence(),
        Sequence::new(10_002)
    );

    let reconnected = DatabasePool::connect(&DatabaseConfig::from_url(&fixture.url).unwrap())
        .await
        .expect("restart-equivalent pool must connect");
    let persisted = SyncRetentionService::new(reconnected.clone())
        .compact_journal_step(fixture.owner_id, fixture.library_id, at())
        .await
        .expect("second bounded step after reconnect must succeed");
    assert_eq!(
        persisted.previous_compacted_through(),
        Sequence::new(10_000)
    );
    assert_eq!(persisted.rows_deleted(), 10_000);
    let step3 = SyncRetentionService::new(reconnected.clone())
        .compact_journal_step(fixture.owner_id, fixture.library_id, at())
        .await
        .expect("third bounded step must delete remainder");
    assert_eq!(step3.rows_deleted(), 5_005);
    assert_eq!(step3.new_compacted_through(), Sequence::new(25_005));
    assert_eq!(
        journal_state(&fixture.inspection, fixture.library_id).await,
        (1, 25_005, 25_005, 0)
    );
    let empty_at_floor =
        JournalCursor::new(fixture.library_id, Sequence::new(1), Sequence::new(25_005)).encode();
    assert!(
        ChangeJournalService::new(reconnected.clone())
            .list_changes(
                fixture.owner_id,
                fixture.library_id,
                Some(empty_at_floor),
                100,
            )
            .await
            .expect("empty journal remains valid at exact floor")
            .events()
            .is_empty()
    );
    assert_eq!(
        ChangeJournalService::new(reconnected.clone())
            .list_changes(fixture.owner_id, fixture.library_id, None, 100)
            .await,
        Err(JournalError::CursorExpired)
    );
    let unchanged_checkpoint: (i64, i64) = sqlx::query_as(
        "SELECT journal_epoch, acknowledged_sequence
         FROM device_sync_checkpoints WHERE device_id = $1 AND library_id = $2",
    )
    .bind(stale_device.into_uuid())
    .bind(fixture.library_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(unchanged_checkpoint, (1, 0));
    reconnected.close().await;
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_journal_age_cutoff_keeps_a_nonmonotonic_suffix_contiguous() {
    let fixture = Fixture::new().await;
    let cutoff = before(at(), 30 * DAY);
    seed_journal_range(
        &fixture.inspection,
        fixture.owner_id,
        fixture.library_id,
        fixture.root.id(),
        1,
        1..=3,
        cutoff,
    )
    .await;
    sqlx::query(
        "UPDATE change_journal
         SET occurred_at = CASE sequence
             WHEN 1 THEN $2
             WHEN 2 THEN $3
             ELSE $4
         END
         WHERE library_id = $1",
    )
    .bind(fixture.library_id.into_uuid())
    .bind(cutoff.as_offset_datetime())
    .bind(after(cutoff, 1).as_offset_datetime())
    .bind(before(cutoff, DAY).as_offset_datetime())
    .execute(&fixture.inspection)
    .await
    .expect_err("journal rows remain immutable outside retention cleanup");
    // Seed the nonmonotonic fixture through the same transaction-local marker
    // used only by internal maintenance; this test-only setup does not expose a
    // production mutation path.
    let mut transaction = fixture.inspection.begin().await.unwrap();
    sqlx::query("SELECT set_config('synveil.retention_cleanup', 'on', true)")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE change_journal
         SET occurred_at = CASE sequence
             WHEN 1 THEN $2
             WHEN 2 THEN $3
             ELSE $4
         END
         WHERE library_id = $1",
    )
    .bind(fixture.library_id.into_uuid())
    .bind(cutoff.as_offset_datetime())
    .bind(after(cutoff, 1).as_offset_datetime())
    .bind(before(cutoff, DAY).as_offset_datetime())
    .execute(&mut *transaction)
    .await
    .expect_err("retention marker permits DELETE only, never journal UPDATE");
    transaction.rollback().await.unwrap();

    // Create the intended timestamp order without weakening the immutable-row
    // trigger: the seed helper accepts one timestamp, so use three contiguous
    // ranges in a fresh library.
    let (library_id, root) = fixture.add_library("nonmonotonic-age-library").await;
    seed_journal_range(
        &fixture.inspection,
        fixture.owner_id,
        library_id,
        root.id(),
        1,
        1..=1,
        cutoff,
    )
    .await;
    seed_journal_range(
        &fixture.inspection,
        fixture.owner_id,
        library_id,
        root.id(),
        1,
        2..=2,
        after(cutoff, 1),
    )
    .await;
    seed_journal_range(
        &fixture.inspection,
        fixture.owner_id,
        library_id,
        root.id(),
        1,
        3..=3,
        before(cutoff, DAY),
    )
    .await;
    let result = fixture
        .retention
        .compact_journal_step(fixture.owner_id, library_id, at())
        .await
        .unwrap();
    assert_eq!(result.rows_deleted(), 1);
    assert_eq!(result.new_compacted_through(), Sequence::new(1));
    assert!(result.blocked_by_age());
    assert_eq!(
        journal_state(&fixture.inspection, library_id).await,
        (1, 3, 1, 2)
    );
    let remaining = sqlx::query_scalar::<_, Vec<i64>>(
        "SELECT array_agg(sequence ORDER BY sequence)
         FROM change_journal WHERE library_id = $1",
    )
    .bind(library_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(remaining, vec![2, 3]);
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_oldest_proof_pins_then_retirement_releases_library_and_epoch_scoped_floor() {
    let fixture = Fixture::new().await;
    let old = before(at(), 60 * DAY);
    seed_journal_range(
        &fixture.inspection,
        fixture.owner_id,
        fixture.library_id,
        fixture.root.id(),
        1,
        1..=3,
        old,
    )
    .await;
    let first = fixture
        .snapshots
        .create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, at())
        .await
        .unwrap();
    seed_journal_range(
        &fixture.inspection,
        fixture.owner_id,
        fixture.library_id,
        fixture.root.id(),
        1,
        4..=6,
        old,
    )
    .await;
    let second_created = after(at(), 60 * 60);
    let second = fixture
        .snapshots
        .create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, second_created)
        .await
        .unwrap();
    seed_journal_range(
        &fixture.inspection,
        fixture.owner_id,
        fixture.library_id,
        fixture.root.id(),
        1,
        7..=10,
        old,
    )
    .await;
    assert_eq!(first.boundary().sequence(), Sequence::new(3));
    assert_eq!(second.boundary().sequence(), Sequence::new(6));

    let (other_library, other_root) = fixture.add_library("other-pin-library").await;
    fixture
        .snapshots
        .create_rebaseline_snapshot(fixture.owner_id, other_library, at())
        .await
        .unwrap();
    seed_journal_range(
        &fixture.inspection,
        fixture.owner_id,
        other_library,
        other_root.id(),
        1,
        1..=4,
        old,
    )
    .await;

    let incompatible_id = RebaselineSnapshotId::new();
    sqlx::query(
        "INSERT INTO rebaseline_snapshot_handoff_proofs
            (snapshot_id, owner_user_id, library_id, journal_epoch,
             snapshot_resume_sequence, snapshot_created_at,
             snapshot_expires_at, proof_expires_at)
         VALUES ($1, $2, $3, 2, 0, $4, $5, $6)",
    )
    .bind(incompatible_id.into_uuid())
    .bind(fixture.owner_id.into_uuid())
    .bind(fixture.library_id.into_uuid())
    .bind(at().as_offset_datetime())
    .bind(after(at(), DAY).as_offset_datetime())
    .bind(after(at(), 365 * DAY).as_offset_datetime())
    .execute(&fixture.inspection)
    .await
    .unwrap();

    let pinned = fixture
        .retention
        .compact_journal_step(fixture.owner_id, fixture.library_id, after(at(), 40 * DAY))
        .await
        .unwrap();
    assert_eq!(pinned.new_compacted_through(), Sequence::new(3));
    assert!(pinned.blocked_by_handoff_proof());
    let other = fixture
        .retention
        .compact_journal_step(fixture.owner_id, other_library, after(at(), 40 * DAY))
        .await
        .unwrap();
    assert_eq!(other.new_compacted_through(), Sequence::new(0));
    assert!(other.blocked_by_handoff_proof());
    assert_eq!(
        journal_state(&fixture.inspection, fixture.library_id)
            .await
            .2,
        3
    );

    let first_deadline = after(at(), 31 * DAY);
    let blocked = fixture
        .retention
        .cleanup_handoff_proofs_step(first_deadline)
        .await
        .unwrap();
    assert_eq!(blocked.proofs_deleted(), 0);
    assert!(blocked.blocked_by_payload());
    fixture
        .retention
        .cleanup_snapshot_payloads_step(first_deadline)
        .await
        .unwrap();
    let retired_first = fixture
        .retention
        .cleanup_handoff_proofs_step(first_deadline)
        .await
        .unwrap();
    assert!(retired_first.proofs_deleted() >= 1);
    let next_pin = fixture
        .retention
        .compact_journal_step(fixture.owner_id, fixture.library_id, after(at(), 40 * DAY))
        .await
        .unwrap();
    assert_eq!(next_pin.new_compacted_through(), Sequence::new(6));
    assert!(next_pin.blocked_by_handoff_proof());

    let second_deadline = after(second_created, 31 * DAY);
    fixture
        .retention
        .cleanup_handoff_proofs_step(second_deadline)
        .await
        .unwrap();
    let released = fixture
        .retention
        .compact_journal_step(fixture.owner_id, fixture.library_id, after(at(), 40 * DAY))
        .await
        .unwrap();
    assert_eq!(released.new_compacted_through(), Sequence::new(10));
    assert!(!released.blocked_by_handoff_proof());
    assert_eq!(
        journal_state(&fixture.inspection, fixture.library_id).await,
        (1, 10, 10, 0)
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM rebaseline_snapshot_handoff_proofs
             WHERE snapshot_id = $1"
        )
        .bind(incompatible_id.into_uuid())
        .fetch_one(&fixture.inspection)
        .await
        .unwrap(),
        1,
        "different-epoch proof remains but must not pin current epoch"
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_snapshot_and_proof_cleanup_are_bounded_and_rollback_atomically() {
    let fixture = Fixture::new().await;
    let policy = SyncRetentionPolicy::new(Duration::from_secs(30 * DAY), 100, 2, 2).unwrap();
    let retention = SyncRetentionService::with_policy(fixture.pool.clone(), policy);
    const ENTRIES_PER_PAYLOAD: i64 = 301;
    for index in 0..300 {
        fixture
            .metadata
            .create_directory(
                fixture.owner_id,
                fixture.library_id,
                Some(fixture.root.id()),
                name(format!("retention-payload-node-{index:03}")),
            )
            .await
            .expect("large snapshot cleanup fixture node must persist");
    }
    let mut snapshot_ids = Vec::new();
    for offset in [0, 2 * DAY, 4 * DAY, 6 * DAY, 8 * DAY] {
        snapshot_ids.push(
            fixture
                .snapshots
                .create_rebaseline_snapshot(
                    fixture.owner_id,
                    fixture.library_id,
                    after(at(), offset),
                )
                .await
                .unwrap()
                .snapshot_id(),
        );
    }
    let cleanup_at = after(at(), 60 * DAY);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM rebaseline_snapshots")
            .fetch_one(&fixture.inspection)
            .await
            .unwrap(),
        5
    );

    sqlx::query(
        "CREATE FUNCTION synveil_test_fail_payload_cleanup() RETURNS trigger
         LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'payload cleanup failure'; END $$",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER synveil_test_fail_payload_cleanup
         AFTER DELETE ON rebaseline_snapshot_entries FOR EACH STATEMENT
         EXECUTE FUNCTION synveil_test_fail_payload_cleanup()",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    assert!(matches!(
        retention.cleanup_snapshot_payloads_step(cleanup_at).await,
        Err(SyncRetentionError::Database(_))
    ));
    for snapshot_id in &snapshot_ids {
        assert_eq!(
            snapshot_counts(&fixture.inspection, *snapshot_id).await,
            (1, ENTRIES_PER_PAYLOAD, 1)
        );
    }
    sqlx::query(
        "DROP TRIGGER synveil_test_fail_payload_cleanup
         ON rebaseline_snapshot_entries",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    sqlx::query("DROP FUNCTION synveil_test_fail_payload_cleanup()")
        .execute(&fixture.inspection)
        .await
        .unwrap();

    let payload_steps = [
        retention
            .cleanup_snapshot_payloads_step(cleanup_at)
            .await
            .unwrap(),
        retention
            .cleanup_snapshot_payloads_step(cleanup_at)
            .await
            .unwrap(),
        retention
            .cleanup_snapshot_payloads_step(cleanup_at)
            .await
            .unwrap(),
    ];
    assert_eq!(
        payload_steps
            .iter()
            .map(|step| step.payloads_deleted())
            .collect::<Vec<_>>(),
        vec![2, 2, 1]
    );
    assert_eq!(
        payload_steps
            .iter()
            .map(|step| step.entries_deleted())
            .collect::<Vec<_>>(),
        vec![602, 602, 301]
    );

    sqlx::query(
        "CREATE FUNCTION synveil_test_fail_proof_cleanup() RETURNS trigger
         LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'proof cleanup failure'; END $$",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER synveil_test_fail_proof_cleanup
         AFTER DELETE ON rebaseline_snapshot_handoff_proofs FOR EACH STATEMENT
         EXECUTE FUNCTION synveil_test_fail_proof_cleanup()",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    assert!(matches!(
        retention.cleanup_handoff_proofs_step(cleanup_at).await,
        Err(SyncRetentionError::Database(_))
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM rebaseline_snapshot_handoff_proofs")
            .fetch_one(&fixture.inspection)
            .await
            .unwrap(),
        5
    );
    sqlx::query(
        "DROP TRIGGER synveil_test_fail_proof_cleanup
         ON rebaseline_snapshot_handoff_proofs",
    )
    .execute(&fixture.inspection)
    .await
    .unwrap();
    sqlx::query("DROP FUNCTION synveil_test_fail_proof_cleanup()")
        .execute(&fixture.inspection)
        .await
        .unwrap();
    let proof_steps = [
        retention
            .cleanup_handoff_proofs_step(cleanup_at)
            .await
            .unwrap(),
        retention
            .cleanup_handoff_proofs_step(cleanup_at)
            .await
            .unwrap(),
        retention
            .cleanup_handoff_proofs_step(cleanup_at)
            .await
            .unwrap(),
    ];
    assert_eq!(
        proof_steps
            .iter()
            .map(|step| step.proofs_deleted())
            .collect::<Vec<_>>(),
        vec![2, 2, 1]
    );

    let orphan_payload = RebaselineSnapshotId::new();
    sqlx::query(
        "INSERT INTO rebaseline_snapshots
            (id, owner_user_id, library_id, journal_epoch,
             snapshot_resume_sequence, entry_count, created_at, expires_at)
         VALUES ($1, $2, $3, 1, 0, 0, $4, $5)",
    )
    .bind(orphan_payload.into_uuid())
    .bind(fixture.owner_id.into_uuid())
    .bind(fixture.library_id.into_uuid())
    .bind(at().as_offset_datetime())
    .bind(after(at(), DAY).as_offset_datetime())
    .execute(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(
        retention.cleanup_snapshot_payloads_step(cleanup_at).await,
        Err(SyncRetentionError::InvariantViolation),
        "cleanup must fail closed when the last handoff authority is missing"
    );
    assert_eq!(
        snapshot_counts(&fixture.inspection, orphan_payload).await,
        (1, 0, 0)
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_retention_concurrency_is_gap_free_linearizable_and_deadlock_free() {
    let fixture = Fixture::new().await;
    let deadlocks_before = deadlocks(&fixture.inspection).await;
    let old = before(at(), 40 * DAY);

    let (feed_library, feed_root) = fixture.add_library("feed-race-library").await;
    seed_journal_range(
        &fixture.inspection,
        fixture.owner_id,
        feed_library,
        feed_root.id(),
        1,
        1..=100,
        old,
    )
    .await;
    let feed_device = fixture.add_device("feed-race-device").await;
    let barrier = Arc::new(Barrier::new(2));
    let feed_barrier = barrier.clone();
    let cleanup_barrier = barrier.clone();
    let sync = fixture.sync.clone();
    let retention = fixture.retention.clone();
    let feed_race = async move {
        feed_barrier.wait().await;
        sync.fetch_feed(fixture.owner_id, feed_device, feed_library, 100)
            .await
    };
    let cleanup_race = async move {
        cleanup_barrier.wait().await;
        retention
            .compact_journal_step(fixture.owner_id, feed_library, at())
            .await
    };
    let (feed_result, cleanup_result) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(feed_race, cleanup_race)
    })
    .await
    .expect("feed/cleanup race must not time out");
    let cleanup_result = cleanup_result.expect("feed/cleanup maintenance must succeed");
    assert_eq!(cleanup_result.new_compacted_through(), Sequence::new(100));
    match feed_result {
        Ok(page) => {
            assert_eq!(page.changes().len(), 100);
            assert_eq!(page.changes().first().unwrap().sequence(), Sequence::new(1));
            assert_eq!(
                page.changes().last().unwrap().sequence(),
                Sequence::new(100)
            );
        }
        Err(error) => assert_eq!(
            error,
            SyncError::RebaselineRequired {
                reason: RebaselineReason::HistoryUnavailable,
                current_epoch: Sequence::new(1),
                minimum_retained_sequence: Sequence::new(100),
            }
        ),
    }

    let (append_library, append_root) = fixture.add_library("append-race-library").await;
    seed_journal_range(
        &fixture.inspection,
        fixture.owner_id,
        append_library,
        append_root.id(),
        1,
        1..=100,
        old,
    )
    .await;
    let barrier = Arc::new(Barrier::new(2));
    let append_barrier = barrier.clone();
    let cleanup_barrier = barrier.clone();
    let metadata = fixture.metadata.clone();
    let retention = fixture.retention.clone();
    let append = async move {
        append_barrier.wait().await;
        metadata
            .create_directory(
                fixture.owner_id,
                append_library,
                Some(append_root.id()),
                name("concurrent-append"),
            )
            .await
    };
    let cleanup = async move {
        cleanup_barrier.wait().await;
        retention
            .compact_journal_step(fixture.owner_id, append_library, at())
            .await
    };
    let (append_result, cleanup_result) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(append, cleanup)
    })
    .await
    .expect("append/cleanup race must not time out");
    let appended = append_result.expect("concurrent append must commit");
    cleanup_result.expect("concurrent cleanup must commit");
    assert_eq!(
        journal_state(&fixture.inspection, append_library).await.1,
        101
    );
    let appended_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM change_journal
         WHERE library_id = $1 AND sequence = 101 AND resource_id = $2",
    )
    .bind(append_library.into_uuid())
    .bind(appended.id().into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .unwrap();
    assert_eq!(
        appended_rows, 1,
        "cleanup must preserve a concurrent append"
    );

    let (pin_library, pin_root) = fixture.add_library("proof-pin-race-library").await;
    seed_journal_range(
        &fixture.inspection,
        fixture.owner_id,
        pin_library,
        pin_root.id(),
        1,
        1..=40,
        old,
    )
    .await;
    let pin_snapshot = fixture
        .snapshots
        .create_rebaseline_snapshot(fixture.owner_id, pin_library, at())
        .await
        .expect("proof-pin race snapshot must persist");
    seed_journal_range(
        &fixture.inspection,
        fixture.owner_id,
        pin_library,
        pin_root.id(),
        1,
        41..=100,
        old,
    )
    .await;
    let pin_device = fixture.add_device("proof-pin-race-device").await;
    let barrier = Arc::new(Barrier::new(2));
    let handoff_barrier = barrier.clone();
    let cleanup_barrier = barrier.clone();
    let sync = fixture.sync.clone();
    let retention = fixture.retention.clone();
    let handoff = async move {
        handoff_barrier.wait().await;
        sync.complete_rebaseline_handoff(fixture.owner_id, pin_device, pin_snapshot.snapshot_id())
            .await
    };
    let cleanup = async move {
        cleanup_barrier.wait().await;
        retention
            .compact_journal_step(fixture.owner_id, pin_library, at())
            .await
    };
    let (handoff_result, cleanup_result) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(handoff, cleanup)
    })
    .await
    .expect("handoff/proof-pin cleanup race must not time out");
    assert_eq!(
        handoff_result
            .expect("retained proof must authorize the concurrent handoff")
            .installed_checkpoint()
            .acknowledged_sequence(),
        Sequence::new(40)
    );
    let pinned_cleanup = cleanup_result.expect("proof-pinned cleanup must commit");
    assert_eq!(pinned_cleanup.new_compacted_through(), Sequence::new(40));
    assert!(pinned_cleanup.blocked_by_handoff_proof());
    assert_eq!(
        journal_state(&fixture.inspection, pin_library).await,
        (1, 100, 40, 60)
    );
    assert_eq!(
        fixture
            .retention
            .cleanup_snapshot_payloads_step(after(at(), DAY))
            .await
            .expect("proof-pin race payload cleanup must commit")
            .payloads_deleted(),
        1
    );
    assert_eq!(
        snapshot_counts(&fixture.inspection, pin_snapshot.snapshot_id()).await,
        (0, 0, 1),
        "isolating the later payload race must retain its journal pin proof"
    );

    let page_one = fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("page-race-one"),
        )
        .await
        .unwrap();
    fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("page-race-two"),
        )
        .await
        .unwrap();
    let descriptor = fixture
        .snapshots
        .create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, at())
        .await
        .unwrap();
    let page_snapshot = descriptor.snapshot_id();
    let page_device = fixture.add_device("payload-race-handoff").await;
    let barrier = Arc::new(Barrier::new(3));
    let page_barrier = barrier.clone();
    let handoff_barrier = barrier.clone();
    let payload_barrier = barrier.clone();
    let snapshots = fixture.snapshots.clone();
    let sync = fixture.sync.clone();
    let retention = fixture.retention.clone();
    let page = async move {
        page_barrier.wait().await;
        snapshots
            .read_rebaseline_snapshot_page(
                fixture.owner_id,
                page_snapshot,
                None,
                100,
                before(after(at(), DAY), 1),
            )
            .await
    };
    let handoff = async move {
        handoff_barrier.wait().await;
        sync.complete_rebaseline_handoff(fixture.owner_id, page_device, page_snapshot)
            .await
    };
    let payload_cleanup = async move {
        payload_barrier.wait().await;
        retention
            .cleanup_snapshot_payloads_step(after(at(), DAY))
            .await
    };
    let (page_result, handoff_result, cleanup_result) =
        tokio::time::timeout(Duration::from_secs(10), async {
            tokio::join!(page, handoff, payload_cleanup)
        })
        .await
        .expect("page/handoff/payload cleanup race must not time out");
    match page_result {
        Ok(page) => {
            assert_eq!(page.entries().len(), 3);
            assert!(
                page.entries()
                    .iter()
                    .any(|entry| entry.node_id() == page_one.id())
            );
        }
        Err(error) => assert_eq!(error, SnapshotError::Expired),
    }
    assert_eq!(
        handoff_result
            .expect("handoff must survive payload cleanup")
            .installed_checkpoint()
            .acknowledged_sequence(),
        descriptor.boundary().sequence()
    );
    assert_eq!(cleanup_result.unwrap().payloads_deleted(), 1);
    assert_eq!(
        snapshot_counts(&fixture.inspection, page_snapshot).await.2,
        1
    );

    let proof_deadline = after(at(), 31 * DAY);
    let race_device = fixture.add_device("proof-race-handoff").await;
    let barrier = Arc::new(Barrier::new(2));
    let handoff_barrier = barrier.clone();
    let proof_barrier = barrier.clone();
    let sync = fixture.sync.clone();
    let retention = fixture.retention.clone();
    let handoff = async move {
        handoff_barrier.wait().await;
        sync.complete_rebaseline_handoff(fixture.owner_id, race_device, page_snapshot)
            .await
    };
    let proof_cleanup = async move {
        proof_barrier.wait().await;
        retention.cleanup_handoff_proofs_step(proof_deadline).await
    };
    let (handoff_result, proof_result) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(handoff, proof_cleanup)
    })
    .await
    .expect("handoff/proof cleanup race must not time out");
    proof_result.expect("proof cleanup race must commit");
    match handoff_result {
        Ok(result) => assert_eq!(
            result.installed_checkpoint().acknowledged_sequence(),
            descriptor.boundary().sequence()
        ),
        Err(error) => {
            assert_eq!(error, SyncError::NotFound);
            let rows: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM device_sync_checkpoints
                 WHERE device_id = $1 AND library_id = $2",
            )
            .bind(race_device.into_uuid())
            .bind(fixture.library_id.into_uuid())
            .fetch_one(&fixture.inspection)
            .await
            .unwrap();
            assert_eq!(
                rows, 0,
                "losing handoff must not partially install a checkpoint"
            );
        }
    }

    // Eight bounded four-caller rounds exercise the established lock order.
    // No caller retries; every individual result must be a documented terminal
    // result, and the database deadlock counter must remain unchanged.
    for round in 0..8 {
        let device = fixture.add_device(&format!("stress-device-{round}")).await;
        let retention_a = fixture.retention.clone();
        let retention_b = fixture.retention.clone();
        let sync_a = fixture.sync.clone();
        let sync_b = fixture.sync.clone();
        let results = tokio::time::timeout(Duration::from_secs(10), async {
            tokio::join!(
                retention_a.compact_journal_step(fixture.owner_id, append_library, at()),
                retention_b.cleanup_snapshot_payloads_step(proof_deadline),
                sync_a.fetch_feed(fixture.owner_id, device, append_library, 100),
                sync_b.complete_rebaseline_handoff(fixture.owner_id, device, page_snapshot),
            )
        })
        .await
        .expect("stress round must not time out");
        results.0.expect("stress compaction must succeed");
        results.1.expect("stress payload cleanup must succeed");
        assert!(
            results.2.is_ok() || matches!(results.2, Err(SyncError::RebaselineRequired { .. }))
        );
        assert!(results.3.is_ok() || results.3 == Err(SyncError::NotFound));
    }
    assert_eq!(deadlocks(&fixture.inspection).await, deadlocks_before);
    fixture.close().await;
}
