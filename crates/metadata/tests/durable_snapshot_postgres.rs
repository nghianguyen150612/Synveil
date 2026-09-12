//! PostgreSQL 17 verification for Prompt 82 durable rebaseline artifacts.
//!
//! The suite is intentionally ignored unless `SYNVEIL_TEST_DATABASE_URL`
//! identifies a fresh disposable PostgreSQL 17 database. Run serially because
//! it installs one deterministic failure-injection trigger for the rollback
//! proof and each fixture applies the forward migration chain.

use std::{sync::Arc, time::Duration};

use sqlx::PgPool;
use synveil_core::{
    ChangeEvent, ChangeKind, DedupDomainId, Device, DeviceId, DeviceStatus, Library, LibraryId,
    LogicalName, LogicalSnapshotNode, Node, NodeId, NodeState, RebaselineSnapshotId, Timestamp,
    User, UserId, UserStatus,
};
use synveil_metadata::{
    ChangeJournalService, DEFAULT_REBASELINE_SNAPSHOT_PAGE_SIZE, DatabaseConfig, DatabasePool,
    DeviceSyncService, DomainRepository, FileMetadataService, LogicalSnapshotService,
    MAX_ACTIVE_REBASELINE_SNAPSHOTS_PER_OWNER_LIBRARY, MAX_REBASELINE_SNAPSHOT_PAGE_SIZE,
    MigrationRunner, RebaselineSnapshotDescriptor, SnapshotError,
};
use uuid::Uuid;

struct Fixture {
    pool: DatabasePool,
    inspection: PgPool,
    owner_id: UserId,
    library_id: LibraryId,
    root: Node,
    metadata: FileMetadataService,
    snapshots: LogicalSnapshotService,
    journal: ChangeJournalService,
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
        insert_user(&pool, owner_id, "durable-snapshot-owner").await;
        let (library_id, root) = insert_library(&pool, owner_id, "durable-snapshot-library").await;
        let metadata = FileMetadataService::new(pool.clone());
        let snapshots = LogicalSnapshotService::new(pool.clone());
        let journal = ChangeJournalService::new(pool.clone());
        Self {
            pool,
            inspection,
            owner_id,
            library_id,
            root,
            metadata,
            snapshots,
            journal,
        }
    }

    async fn add_library(&self, owner_id: UserId, label: &str) -> (LibraryId, Node) {
        insert_library(&self.pool, owner_id, label).await
    }

    async fn close(self) {
        self.pool.close().await;
        self.inspection.close().await;
    }
}

fn observed_at() -> Timestamp {
    Timestamp::parse("2026-09-08T00:00:00.123456Z").expect("fixture timestamp is valid")
}

fn after(value: Timestamp, seconds: u64) -> Timestamp {
    value
        .checked_add_std(Duration::from_secs(seconds))
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
        observed_at(),
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
        observed_at(),
    );
    let library = Library::new(
        library_id,
        owner_id,
        name(label),
        &root,
        DedupDomainId::new(),
        observed_at(),
    )
    .expect("fixture library must satisfy domain invariants");
    DomainRepository::new(pool)
        .insert_library_with_root(&library, &root)
        .await
        .expect("fixture library must persist");
    (library_id, root)
}

async fn read_all(
    snapshots: &LogicalSnapshotService,
    owner_id: UserId,
    snapshot_id: RebaselineSnapshotId,
    page_size: u32,
    at: Timestamp,
) -> (Vec<LogicalSnapshotNode>, usize) {
    let mut entries = Vec::new();
    let mut cursor = None;
    let mut page_count = 0;
    loop {
        let page = snapshots
            .read_rebaseline_snapshot_page(owner_id, snapshot_id, cursor, page_size, at)
            .await
            .expect("durable snapshot page must load");
        assert_eq!(page.descriptor().snapshot_id(), snapshot_id);
        page_count += 1;
        cursor = page.next_cursor();
        entries.extend(page.into_entries());
        if cursor.is_none() {
            return (entries, page_count);
        }
    }
}

fn has_node(entries: &[LogicalSnapshotNode], node_id: NodeId) -> bool {
    entries.iter().any(|entry| entry.node_id() == node_id)
}

async fn event_for(
    journal: &ChangeJournalService,
    owner_id: UserId,
    library_id: LibraryId,
    node_id: NodeId,
    kind: ChangeKind,
) -> ChangeEvent {
    journal
        .list_changes(owner_id, library_id, None, 500)
        .await
        .expect("fixture journal page must load")
        .into_events()
        .into_iter()
        .find(|event| event.resource_id() == node_id && event.change_kind() == kind)
        .expect("expected mutation event must exist")
}

fn node_entry(entries: &[LogicalSnapshotNode], node_id: NodeId) -> &LogicalSnapshotNode {
    entries
        .iter()
        .find(|entry| entry.node_id() == node_id)
        .expect("mutation target must remain in the logical snapshot")
}

fn event_is_in_descriptor_cut(
    descriptor: RebaselineSnapshotDescriptor,
    event: ChangeEvent,
) -> bool {
    assert_eq!(descriptor.boundary().library_id(), event.library_id());
    assert_eq!(descriptor.boundary().journal_epoch(), event.journal_epoch());
    event.sequence().get() <= descriptor.boundary().sequence().get()
}

async fn snapshot_counts(
    inspection: &PgPool,
    owner_id: UserId,
    library_id: LibraryId,
    observed_at: Timestamp,
) -> (i64, i64, i64) {
    let header_count: i64 = sqlx::query_scalar(
        "SELECT count(*)
         FROM rebaseline_snapshots
         WHERE owner_user_id = $1 AND library_id = $2",
    )
    .bind(owner_id.into_uuid())
    .bind(library_id.into_uuid())
    .fetch_one(inspection)
    .await
    .expect("snapshot header count must load");
    let active_count: i64 = sqlx::query_scalar(
        "SELECT count(*)
         FROM rebaseline_snapshots
         WHERE owner_user_id = $1
           AND library_id = $2
           AND expires_at > $3",
    )
    .bind(owner_id.into_uuid())
    .bind(library_id.into_uuid())
    .bind(observed_at.as_offset_datetime())
    .fetch_one(inspection)
    .await
    .expect("active snapshot count must load");
    let entry_count: i64 = sqlx::query_scalar(
        "SELECT count(*)
         FROM rebaseline_snapshot_entries AS e
         INNER JOIN rebaseline_snapshots AS s ON s.id = e.snapshot_id
         WHERE s.owner_user_id = $1 AND s.library_id = $2",
    )
    .bind(owner_id.into_uuid())
    .bind(library_id.into_uuid())
    .fetch_one(inspection)
    .await
    .expect("snapshot entry count must load");
    (header_count, entry_count, active_count)
}

async fn namespace_state(
    inspection: &PgPool,
    owner_id: UserId,
    library_id: LibraryId,
) -> (i64, i64, i64, i64) {
    let (journal_epoch, sync_head): (i64, i64) = sqlx::query_as(
        "SELECT journal_epoch, sync_head FROM libraries WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(library_id.into_uuid())
    .bind(owner_id.into_uuid())
    .fetch_one(inspection)
    .await
    .expect("library journal head must load");
    let journal_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM change_journal WHERE owner_user_id = $1 AND library_id = $2",
    )
    .bind(owner_id.into_uuid())
    .bind(library_id.into_uuid())
    .fetch_one(inspection)
    .await
    .expect("journal count must load");
    let checkpoint_count: i64 = sqlx::query_scalar(
        "SELECT count(*)
         FROM device_sync_checkpoints
         WHERE owner_user_id = $1 AND library_id = $2",
    )
    .bind(owner_id.into_uuid())
    .bind(library_id.into_uuid())
    .fetch_one(inspection)
    .await
    .expect("checkpoint count must load");
    (journal_epoch, sync_head, journal_count, checkpoint_count)
}

async fn database_deadlocks(inspection: &PgPool) -> i64 {
    sqlx::query_scalar(
        "SELECT deadlocks
         FROM pg_stat_database
         WHERE datname = current_database()",
    )
    .fetch_one(inspection)
    .await
    .expect("database deadlock counter must load")
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_durable_snapshot_matches_prompt81_logical_snapshot_and_descriptor_count() {
    let fixture = Fixture::new().await;
    let directory = fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("directory"),
        )
        .await
        .expect("directory must be created");
    let leaf = fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(directory.id()),
            name("leaf"),
        )
        .await
        .expect("leaf must be created");
    let trashed = fixture
        .metadata
        .delete_node(fixture.owner_id, leaf.id(), leaf.revision())
        .await
        .expect("leaf must be trashed");

    let prompt81 = fixture
        .snapshots
        .build(fixture.owner_id, fixture.library_id)
        .await
        .expect("Prompt 81 snapshot must build");
    let descriptor = fixture
        .snapshots
        .create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, observed_at())
        .await
        .expect("durable snapshot must materialize");
    let loaded = fixture
        .snapshots
        .get_rebaseline_snapshot(fixture.owner_id, descriptor.snapshot_id(), observed_at())
        .await
        .expect("descriptor must be readable");
    let (entries, page_count) = read_all(
        &fixture.snapshots,
        fixture.owner_id,
        descriptor.snapshot_id(),
        DEFAULT_REBASELINE_SNAPSHOT_PAGE_SIZE,
        observed_at(),
    )
    .await;

    assert_eq!(descriptor, loaded);
    assert_eq!(descriptor.boundary(), prompt81.boundary());
    assert_eq!(descriptor.entry_count(), 3);
    assert_eq!(entries, prompt81.state().entries());
    assert_eq!(page_count, 1);
    assert!(
        entries
            .windows(2)
            .all(|pair| pair[0].node_id() < pair[1].node_id())
    );
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.node_id() == leaf.id())
            .map(LogicalSnapshotNode::state),
        Some(trashed.state())
    );

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_durable_snapshot_keyset_pages_are_stable_for_a_thousand_node_fixture() {
    let fixture = Fixture::new().await;
    let mut parent = fixture.root.clone();
    for index in 0..1_000 {
        parent = fixture
            .metadata
            .create_directory(
                fixture.owner_id,
                fixture.library_id,
                Some(parent.id()),
                name(format!("large-{index:04}")),
            )
            .await
            .expect("large fixture node must be created");
    }
    let descriptor = fixture
        .snapshots
        .create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, observed_at())
        .await
        .expect("large durable snapshot must materialize");
    let (entries, page_count) = read_all(
        &fixture.snapshots,
        fixture.owner_id,
        descriptor.snapshot_id(),
        128,
        observed_at(),
    )
    .await;

    assert_eq!(descriptor.entry_count(), 1_001);
    assert_eq!(entries.len(), 1_001);
    assert_eq!(page_count, 8);
    assert!(
        entries
            .windows(2)
            .all(|pair| pair[0].node_id() < pair[1].node_id())
    );
    assert_eq!(
        fixture
            .snapshots
            .read_rebaseline_snapshot_page(
                fixture.owner_id,
                descriptor.snapshot_id(),
                None,
                0,
                observed_at(),
            )
            .await,
        Err(SnapshotError::InvalidPageSize)
    );
    assert_eq!(
        fixture
            .snapshots
            .read_rebaseline_snapshot_page(
                fixture.owner_id,
                descriptor.snapshot_id(),
                None,
                MAX_REBASELINE_SNAPSHOT_PAGE_SIZE + 1,
                observed_at(),
            )
            .await,
        Err(SnapshotError::InvalidPageSize)
    );

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_durable_snapshot_owner_library_and_cursor_scopes_are_concealed_or_rejected() {
    let fixture = Fixture::new().await;
    fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("cursor-child"),
        )
        .await
        .expect("cursor child must be created");
    let first = fixture
        .snapshots
        .create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, observed_at())
        .await
        .expect("first artifact must materialize");
    let second = fixture
        .snapshots
        .create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, observed_at())
        .await
        .expect("second artifact must materialize");
    let first_page = fixture
        .snapshots
        .read_rebaseline_snapshot_page(
            fixture.owner_id,
            first.snapshot_id(),
            None,
            1,
            observed_at(),
        )
        .await
        .expect("first page must load");
    let first_cursor = first_page
        .next_cursor()
        .expect("two-entry snapshot must have a continuation");
    assert_eq!(
        fixture
            .snapshots
            .read_rebaseline_snapshot_page(
                fixture.owner_id,
                second.snapshot_id(),
                Some(first_cursor),
                1,
                observed_at(),
            )
            .await,
        Err(SnapshotError::InvalidPageCursor)
    );

    let foreign_owner = UserId::new();
    insert_user(&fixture.pool, foreign_owner, "durable-snapshot-foreign").await;
    let (foreign_library, foreign_root) = fixture
        .add_library(foreign_owner, "durable-snapshot-foreign-library")
        .await;
    let foreign_child = fixture
        .metadata
        .create_directory(
            foreign_owner,
            foreign_library,
            Some(foreign_root.id()),
            name("foreign-child"),
        )
        .await
        .expect("foreign child must be created");
    let foreign_snapshot = fixture
        .snapshots
        .create_rebaseline_snapshot(foreign_owner, foreign_library, observed_at())
        .await
        .expect("foreign snapshot must materialize");
    assert_eq!(
        fixture
            .snapshots
            .get_rebaseline_snapshot(
                fixture.owner_id,
                foreign_snapshot.snapshot_id(),
                observed_at()
            )
            .await,
        Err(SnapshotError::NotFound)
    );
    assert_eq!(
        fixture
            .snapshots
            .read_rebaseline_snapshot_page(
                fixture.owner_id,
                foreign_snapshot.snapshot_id(),
                None,
                1,
                observed_at(),
            )
            .await,
        Err(SnapshotError::NotFound)
    );
    let (foreign_entries, _) = read_all(
        &fixture.snapshots,
        foreign_owner,
        foreign_snapshot.snapshot_id(),
        10,
        observed_at(),
    )
    .await;
    assert!(has_node(&foreign_entries, foreign_child.id()));
    assert!(!has_node(&foreign_entries, fixture.root.id()));

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_durable_snapshot_expires_at_the_exact_injected_boundary() {
    let fixture = Fixture::new().await;
    let descriptor = fixture
        .snapshots
        .create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, observed_at())
        .await
        .expect("artifact must materialize");
    assert!(descriptor.expires_at() > observed_at());
    assert!(
        fixture
            .snapshots
            .get_rebaseline_snapshot(
                fixture.owner_id,
                descriptor.snapshot_id(),
                after(descriptor.expires_at(), 0),
            )
            .await
            .is_err_and(|error| error == SnapshotError::Expired)
    );
    assert_eq!(
        fixture
            .snapshots
            .read_rebaseline_snapshot_page(
                fixture.owner_id,
                descriptor.snapshot_id(),
                None,
                1,
                descriptor.expires_at(),
            )
            .await,
        Err(SnapshotError::Expired)
    );
    let foreign_owner = UserId::new();
    insert_user(&fixture.pool, foreign_owner, "expired-foreign-owner").await;
    assert_eq!(
        fixture
            .snapshots
            .get_rebaseline_snapshot(
                foreign_owner,
                descriptor.snapshot_id(),
                descriptor.expires_at(),
            )
            .await,
        Err(SnapshotError::NotFound)
    );

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_durable_snapshot_active_admission_is_scoped_and_expired_rows_re_admit() {
    let fixture = Fixture::new().await;
    let limit = MAX_ACTIVE_REBASELINE_SNAPSHOTS_PER_OWNER_LIBRARY;
    for _ in 0..limit {
        fixture
            .snapshots
            .create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, observed_at())
            .await
            .expect("active snapshot must be admitted below the bound");
    }

    let counts_before = snapshot_counts(
        &fixture.inspection,
        fixture.owner_id,
        fixture.library_id,
        observed_at(),
    )
    .await;
    assert_eq!(
        counts_before,
        (i64::from(limit), i64::from(limit), i64::from(limit))
    );

    let mut device = Device::new(
        DeviceId::new(),
        fixture.owner_id,
        name("admission-invariance-device"),
        observed_at(),
    );
    device
        .transition_status(DeviceStatus::Active, observed_at())
        .expect("fixture device must activate");
    DomainRepository::new(&fixture.pool)
        .insert_device(&device)
        .await
        .expect("fixture device must persist");
    let sync = DeviceSyncService::new(fixture.pool.clone());
    let checkpoint_before = sync
        .ensure_checkpoint(fixture.owner_id, device.id(), fixture.library_id)
        .await
        .expect("checkpoint must initialize");
    let namespace_before =
        namespace_state(&fixture.inspection, fixture.owner_id, fixture.library_id).await;

    assert_eq!(
        fixture
            .snapshots
            .create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, observed_at())
            .await,
        Err(SnapshotError::ActiveArtifactLimitReached)
    );

    let counts_after_rejection = snapshot_counts(
        &fixture.inspection,
        fixture.owner_id,
        fixture.library_id,
        observed_at(),
    )
    .await;
    assert_eq!(counts_after_rejection, counts_before);
    let checkpoint_after = sync
        .ensure_checkpoint(fixture.owner_id, device.id(), fixture.library_id)
        .await
        .expect("checkpoint must remain readable");
    assert_eq!(checkpoint_after, checkpoint_before);
    assert_eq!(
        namespace_state(&fixture.inspection, fixture.owner_id, fixture.library_id,).await,
        namespace_before,
        "rejected creation must not change journal head, journal rows, or checkpoints"
    );

    let (other_library, _) = fixture
        .add_library(fixture.owner_id, "admission-other-library")
        .await;
    fixture
        .snapshots
        .create_rebaseline_snapshot(fixture.owner_id, other_library, observed_at())
        .await
        .expect("same owner may create in another library");

    let foreign_owner = UserId::new();
    insert_user(&fixture.pool, foreign_owner, "admission-foreign-owner").await;
    let (foreign_library, _) = fixture
        .add_library(foreign_owner, "admission-foreign-library")
        .await;
    fixture
        .snapshots
        .create_rebaseline_snapshot(foreign_owner, foreign_library, observed_at())
        .await
        .expect("another owner may create in another library");

    let exact_expiry = after(
        observed_at(),
        synveil_metadata::DEFAULT_REBASELINE_SNAPSHOT_LIFETIME_SECONDS,
    );
    fixture
        .snapshots
        .create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, exact_expiry)
        .await
        .expect("equality with expiry must re-admit a new artifact");
    assert_eq!(
        snapshot_counts(
            &fixture.inspection,
            fixture.owner_id,
            fixture.library_id,
            exact_expiry,
        )
        .await,
        (i64::from(limit) + 1, i64::from(limit) + 1, 1),
        "expired rows remain durable but do not count as active"
    );

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_durable_snapshot_concurrent_admission_never_exceeds_active_limit() {
    let fixture = Fixture::new().await;
    let limit = MAX_ACTIVE_REBASELINE_SNAPSHOTS_PER_OWNER_LIBRARY;
    for _ in 0..(limit - 1) {
        fixture
            .snapshots
            .create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, observed_at())
            .await
            .expect("seed snapshot must be admitted");
    }

    let callers = usize::try_from(limit).expect("snapshot limit fits usize") + 4;
    let owner_id = fixture.owner_id;
    let library_id = fixture.library_id;
    let deadlocks_before = database_deadlocks(&fixture.inspection).await;
    let barrier = Arc::new(tokio::sync::Barrier::new(callers));
    let mut handles = Vec::with_capacity(callers);
    for _ in 0..callers {
        let snapshots = fixture.snapshots.clone();
        let barrier = barrier.clone();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            tokio::time::timeout(
                Duration::from_secs(10),
                snapshots.create_rebaseline_snapshot(owner_id, library_id, observed_at()),
            )
            .await
        }));
    }

    let mut admitted = 0_u32;
    let mut rejected = 0_u32;
    let mut unexpected_errors = 0_u32;
    let mut timeouts = 0_u32;
    for handle in handles {
        match handle.await.expect("admission task must join") {
            Ok(Ok(_descriptor)) => admitted += 1,
            Ok(Err(SnapshotError::ActiveArtifactLimitReached)) => rejected += 1,
            Ok(Err(error)) => {
                unexpected_errors += 1;
                eprintln!("unexpected concurrent admission error: {error:?}");
            }
            Err(_) => timeouts += 1,
        }
    }
    let deadlocks_after = database_deadlocks(&fixture.inspection).await;

    println!(
        "durable snapshot admission boundary: callers={callers} admitted={admitted} rejected={rejected} unexpected_errors={unexpected_errors} timeouts={timeouts} sqlstate_40p01_delta={}",
        deadlocks_after - deadlocks_before
    );
    assert_eq!(admitted, 1);
    assert_eq!(
        rejected,
        u32::try_from(callers - 1).expect("callers fit u32")
    );
    assert_eq!(unexpected_errors, 0);
    assert_eq!(timeouts, 0);
    assert_eq!(deadlocks_after, deadlocks_before);
    assert_eq!(
        snapshot_counts(
            &fixture.inspection,
            fixture.owner_id,
            fixture.library_id,
            observed_at(),
        )
        .await,
        (i64::from(limit), i64::from(limit), i64::from(limit)),
        "concurrent admission must leave exactly the durable active bound"
    );

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_durable_snapshot_pages_survive_new_pool_and_cross_connection() {
    let fixture = Fixture::new().await;
    for label in ["restart-a", "restart-b", "restart-c"] {
        fixture
            .metadata
            .create_directory(
                fixture.owner_id,
                fixture.library_id,
                Some(fixture.root.id()),
                name(label),
            )
            .await
            .expect("fixture child must be created");
    }
    let descriptor = fixture
        .snapshots
        .create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, observed_at())
        .await
        .expect("artifact must materialize");
    let first_page = fixture
        .snapshots
        .read_rebaseline_snapshot_page(
            fixture.owner_id,
            descriptor.snapshot_id(),
            None,
            1,
            observed_at(),
        )
        .await
        .expect("first page must load");
    let first_entry = first_page.entries()[0].clone();
    let cursor = first_page
        .next_cursor()
        .expect("four-entry fixture requires a next page");
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL").expect("test URL must be available");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let cross_pool = DatabasePool::connect(&config)
        .await
        .expect("second service pool must connect");
    let cross_service = LogicalSnapshotService::new(cross_pool.clone());
    let cross_page = cross_service
        .read_rebaseline_snapshot_page(
            fixture.owner_id,
            descriptor.snapshot_id(),
            Some(cursor),
            1,
            observed_at(),
        )
        .await
        .expect("second connection must continue the same artifact");
    assert_eq!(cross_page.descriptor(), descriptor);
    assert_ne!(cross_page.entries()[0], first_entry);
    cross_pool.close().await;

    let owner_id = fixture.owner_id;
    let snapshot_id = descriptor.snapshot_id();
    fixture.close().await;
    let restarted_pool = DatabasePool::connect(&config)
        .await
        .expect("fresh service pool must connect after original close");
    let restarted_service = LogicalSnapshotService::new(restarted_pool.clone());
    let resumed_page = restarted_service
        .read_rebaseline_snapshot_page(owner_id, snapshot_id, Some(cursor), 2, observed_at())
        .await
        .expect("fresh service must resume from the original page cursor");
    let resumed_count = resumed_page.entries().len();
    let final_count = match resumed_page.next_cursor() {
        Some(next_cursor) => restarted_service
            .read_rebaseline_snapshot_page(
                owner_id,
                snapshot_id,
                Some(next_cursor),
                2,
                observed_at(),
            )
            .await
            .expect("fresh service must read the terminal remaining page")
            .entries()
            .len(),
        None => 0,
    };
    assert_eq!(resumed_count + final_count, 3);
    restarted_pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_durable_snapshot_is_immutable_and_continues_feed_strictly_after_boundary() {
    let fixture = Fixture::new().await;
    let before_cut = fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("before-cut"),
        )
        .await
        .expect("pre-cut node must be created");
    let descriptor = fixture
        .snapshots
        .create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, observed_at())
        .await
        .expect("artifact must materialize");
    let (original_entries, _) = read_all(
        &fixture.snapshots,
        fixture.owner_id,
        descriptor.snapshot_id(),
        2,
        observed_at(),
    )
    .await;
    assert!(has_node(&original_entries, before_cut.id()));

    let after_cut = fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("after-cut"),
        )
        .await
        .expect("post-cut node must be created");
    let renamed = fixture
        .metadata
        .rename_node(
            fixture.owner_id,
            before_cut.id(),
            name("before-cut-renamed"),
            before_cut.revision(),
        )
        .await
        .expect("post-cut rename must commit");
    let _trashed = fixture
        .metadata
        .delete_node(fixture.owner_id, renamed.id(), renamed.revision())
        .await
        .expect("post-cut trash must commit");
    let (after_entries, _) = read_all(
        &fixture.snapshots,
        fixture.owner_id,
        descriptor.snapshot_id(),
        2,
        observed_at(),
    )
    .await;
    assert_eq!(after_entries, original_entries);
    assert!(!has_node(&after_entries, after_cut.id()));

    let feed = fixture
        .journal
        .list_changes(
            fixture.owner_id,
            fixture.library_id,
            Some(descriptor.boundary().cursor().encode()),
            500,
        )
        .await
        .expect("post-boundary feed must load");
    assert!(feed.events().iter().all(|event| {
        event.journal_epoch() == descriptor.boundary().journal_epoch()
            && event.sequence().get() > descriptor.boundary().sequence().get()
    }));
    assert!(feed.events().iter().any(|event| {
        event.resource_id() == after_cut.id() && event.change_kind() == ChangeKind::NodeCreated
    }));
    assert!(feed.events().iter().all(|event| {
        !(event.resource_id() == before_cut.id() && event.change_kind() == ChangeKind::NodeCreated)
    }));

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_durable_snapshot_create_and_pages_do_not_advance_checkpoint() {
    let fixture = Fixture::new().await;
    let mut device = Device::new(
        DeviceId::new(),
        fixture.owner_id,
        name("durable-snapshot-device"),
        observed_at(),
    );
    device
        .transition_status(DeviceStatus::Active, observed_at())
        .expect("fixture device must activate");
    DomainRepository::new(&fixture.pool)
        .insert_device(&device)
        .await
        .expect("fixture device must persist");
    let sync = DeviceSyncService::new(fixture.pool.clone());
    let before = sync
        .ensure_checkpoint(fixture.owner_id, device.id(), fixture.library_id)
        .await
        .expect("checkpoint must initialize");
    let descriptor = fixture
        .snapshots
        .create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, observed_at())
        .await
        .expect("artifact must materialize");
    fixture
        .snapshots
        .read_rebaseline_snapshot_page(
            fixture.owner_id,
            descriptor.snapshot_id(),
            None,
            1,
            observed_at(),
        )
        .await
        .expect("artifact page must load");
    let after = sync
        .ensure_checkpoint(fixture.owner_id, device.id(), fixture.library_id)
        .await
        .expect("checkpoint must remain readable");
    assert_eq!(after, before);

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_durable_snapshot_rolls_back_header_and_entries_when_copy_fails() {
    let fixture = Fixture::new().await;
    let suffix = Uuid::now_v7().simple().to_string();
    let function_name = format!("p82_snapshot_fail_{suffix}");
    let trigger_name = format!("p82_snapshot_fail_trigger_{suffix}");
    sqlx::query(&format!(
        "CREATE FUNCTION {function_name}() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN RAISE EXCEPTION 'injected durable snapshot entry failure'; END;
         $$"
    ))
    .execute(&fixture.inspection)
    .await
    .expect("failure function must install");
    sqlx::query(&format!(
        "CREATE TRIGGER {trigger_name}
         BEFORE INSERT ON rebaseline_snapshot_entries
         FOR EACH ROW EXECUTE FUNCTION {function_name}()"
    ))
    .execute(&fixture.inspection)
    .await
    .expect("failure trigger must install");

    let result = fixture
        .snapshots
        .create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, observed_at())
        .await;
    assert!(matches!(result, Err(SnapshotError::Database(_))));
    let header_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM rebaseline_snapshots WHERE owner_user_id = $1 AND library_id = $2",
    )
    .bind(fixture.owner_id.into_uuid())
    .bind(fixture.library_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("header count must load");
    let entry_count: i64 = sqlx::query_scalar(
        "SELECT count(*)
         FROM rebaseline_snapshot_entries AS e
         INNER JOIN rebaseline_snapshots AS s ON s.id = e.snapshot_id
         WHERE s.owner_user_id = $1 AND s.library_id = $2",
    )
    .bind(fixture.owner_id.into_uuid())
    .bind(fixture.library_id.into_uuid())
    .fetch_one(&fixture.inspection)
    .await
    .expect("entry count must load");
    assert_eq!(header_count, 0);
    assert_eq!(entry_count, 0);

    sqlx::query(&format!(
        "DROP TRIGGER {trigger_name} ON rebaseline_snapshot_entries"
    ))
    .execute(&fixture.inspection)
    .await
    .expect("failure trigger must remove");
    sqlx::query(&format!("DROP FUNCTION {function_name}()"))
        .execute(&fixture.inspection)
        .await
        .expect("failure function must remove");
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_durable_snapshot_concurrent_create_and_namespace_mutation_remain_one_cut_without_deadlock()
 {
    let fixture = Fixture::new().await;
    let mut deadlock_count = 0_u32;
    let mut unexpected_error_count = 0_u32;
    let mut timeout_count = 0_u32;
    let mut admission_rejection_count = 0_u32;

    // Exercise every lock-taking mutation shape that this artifact can race:
    // creation, rename, move, trash, and restore. All are paired with durable
    // materialization across 15 bounded rounds, without retries or a global
    // serialization mechanism.
    for round in 0..15 {
        let snapshots = fixture.snapshots.clone();
        let metadata = fixture.metadata.clone();
        let result = match round % 5 {
            0 => tokio::time::timeout(Duration::from_secs(10), async {
                tokio::join!(
                    snapshots.create_rebaseline_snapshot(
                        fixture.owner_id,
                        fixture.library_id,
                        observed_at(),
                    ),
                    metadata.create_directory(
                        fixture.owner_id,
                        fixture.library_id,
                        Some(fixture.root.id()),
                        name(format!("concurrent-create-{round}")),
                    )
                )
            })
            .await
            .map(|result| (result, ChangeKind::NodeCreated)),
            1 => {
                let node = fixture
                    .metadata
                    .create_directory(
                        fixture.owner_id,
                        fixture.library_id,
                        Some(fixture.root.id()),
                        name(format!("concurrent-rename-before-{round}")),
                    )
                    .await
                    .expect("rename stress fixture node must be created");
                tokio::time::timeout(Duration::from_secs(10), async {
                    tokio::join!(
                        snapshots.create_rebaseline_snapshot(
                            fixture.owner_id,
                            fixture.library_id,
                            observed_at(),
                        ),
                        metadata.rename_node(
                            fixture.owner_id,
                            node.id(),
                            name(format!("concurrent-rename-after-{round}")),
                            node.revision(),
                        )
                    )
                })
                .await
                .map(|result| (result, ChangeKind::NodeRenamed))
            }
            2 => {
                let source = fixture
                    .metadata
                    .create_directory(
                        fixture.owner_id,
                        fixture.library_id,
                        Some(fixture.root.id()),
                        name(format!("concurrent-move-source-{round}")),
                    )
                    .await
                    .expect("move stress source must be created");
                let destination = fixture
                    .metadata
                    .create_directory(
                        fixture.owner_id,
                        fixture.library_id,
                        Some(fixture.root.id()),
                        name(format!("concurrent-move-destination-{round}")),
                    )
                    .await
                    .expect("move stress destination must be created");
                tokio::time::timeout(Duration::from_secs(10), async {
                    tokio::join!(
                        snapshots.create_rebaseline_snapshot(
                            fixture.owner_id,
                            fixture.library_id,
                            observed_at(),
                        ),
                        metadata.move_node(
                            fixture.owner_id,
                            source.id(),
                            destination.id(),
                            source.revision(),
                        )
                    )
                })
                .await
                .map(|result| (result, ChangeKind::NodeMoved))
            }
            3 => {
                let node = fixture
                    .metadata
                    .create_directory(
                        fixture.owner_id,
                        fixture.library_id,
                        Some(fixture.root.id()),
                        name(format!("concurrent-trash-{round}")),
                    )
                    .await
                    .expect("trash stress fixture node must be created");
                tokio::time::timeout(Duration::from_secs(10), async {
                    tokio::join!(
                        snapshots.create_rebaseline_snapshot(
                            fixture.owner_id,
                            fixture.library_id,
                            observed_at(),
                        ),
                        metadata.delete_node(fixture.owner_id, node.id(), node.revision())
                    )
                })
                .await
                .map(|result| (result, ChangeKind::NodeTrashed))
            }
            _ => {
                let node = fixture
                    .metadata
                    .create_directory(
                        fixture.owner_id,
                        fixture.library_id,
                        Some(fixture.root.id()),
                        name(format!("concurrent-restore-{round}")),
                    )
                    .await
                    .expect("restore stress fixture node must be created");
                let trashed = fixture
                    .metadata
                    .delete_node(fixture.owner_id, node.id(), node.revision())
                    .await
                    .expect("restore stress fixture node must be trashed");
                tokio::time::timeout(Duration::from_secs(10), async {
                    tokio::join!(
                        snapshots.create_rebaseline_snapshot(
                            fixture.owner_id,
                            fixture.library_id,
                            observed_at(),
                        ),
                        metadata.restore_node(fixture.owner_id, node.id(), trashed.revision())
                    )
                })
                .await
                .map(|result| (result, ChangeKind::NodeRestored))
            }
        };

        let Ok(((snapshot_result, mutation_result), mutation_kind)) = result else {
            timeout_count += 1;
            continue;
        };
        match (snapshot_result, mutation_result) {
            (Ok(descriptor), Ok(node)) => {
                let (entries, _) = read_all(
                    &fixture.snapshots,
                    fixture.owner_id,
                    descriptor.snapshot_id(),
                    32,
                    observed_at(),
                )
                .await;
                let event = event_for(
                    &fixture.journal,
                    fixture.owner_id,
                    fixture.library_id,
                    node.id(),
                    mutation_kind,
                )
                .await;
                let included = event_is_in_descriptor_cut(descriptor, event);
                if mutation_kind == ChangeKind::NodeCreated {
                    assert_eq!(has_node(&entries, node.id()), included);
                } else {
                    assert!(has_node(&entries, node.id()));
                }
            }
            (Err(SnapshotError::ActiveArtifactLimitReached), Ok(_node)) => {
                admission_rejection_count += 1;
            }
            (snapshot_result, mutation_result) => {
                let errors = format!("snapshot={snapshot_result:?}; mutation={mutation_result:?}");
                if errors.contains("40P01") {
                    deadlock_count += 1;
                } else {
                    unexpected_error_count += 1;
                }
            }
        }
    }

    println!(
        "durable snapshot concurrency stress: rounds=15 callers=2 admitted=8 admission_rejections={admission_rejection_count} sqlstate_40p01={deadlock_count} unexpected_errors={unexpected_error_count} timeouts={timeout_count}"
    );
    assert_eq!(admission_rejection_count, 7);
    assert_eq!(deadlock_count, 0);
    assert_eq!(unexpected_error_count, 0);
    assert_eq!(timeout_count, 0);
    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_durable_snapshot_concurrent_rename_is_before_or_after_its_persisted_cut() {
    let fixture = Fixture::new().await;
    let node = fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("rename-before"),
        )
        .await
        .expect("rename fixture node must be created");
    let snapshots = fixture.snapshots.clone();
    let metadata = fixture.metadata.clone();
    let (snapshot_result, renamed_result) = tokio::join!(
        snapshots.create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, observed_at()),
        metadata.rename_node(
            fixture.owner_id,
            node.id(),
            name("rename-after"),
            node.revision(),
        )
    );
    let descriptor = snapshot_result.expect("concurrent durable snapshot must materialize");
    let renamed = renamed_result.expect("concurrent rename must commit");
    let event = event_for(
        &fixture.journal,
        fixture.owner_id,
        fixture.library_id,
        node.id(),
        ChangeKind::NodeRenamed,
    )
    .await;
    let included = event_is_in_descriptor_cut(descriptor, event);
    let (entries, _) = read_all(
        &fixture.snapshots,
        fixture.owner_id,
        descriptor.snapshot_id(),
        32,
        observed_at(),
    )
    .await;
    assert_eq!(
        node_entry(&entries, node.id()).name(),
        if included {
            renamed.name()
        } else {
            node.name()
        }
    );

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_durable_snapshot_concurrent_move_is_before_or_after_its_persisted_cut() {
    let fixture = Fixture::new().await;
    let source = fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("move-before"),
        )
        .await
        .expect("move source must be created");
    let destination = fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("move-destination"),
        )
        .await
        .expect("move destination must be created");
    let snapshots = fixture.snapshots.clone();
    let metadata = fixture.metadata.clone();
    let (snapshot_result, moved_result) = tokio::join!(
        snapshots.create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, observed_at()),
        metadata.move_node(
            fixture.owner_id,
            source.id(),
            destination.id(),
            source.revision(),
        )
    );
    let descriptor = snapshot_result.expect("concurrent durable snapshot must materialize");
    let moved = moved_result.expect("concurrent move must commit");
    let event = event_for(
        &fixture.journal,
        fixture.owner_id,
        fixture.library_id,
        source.id(),
        ChangeKind::NodeMoved,
    )
    .await;
    let included = event_is_in_descriptor_cut(descriptor, event);
    let (entries, _) = read_all(
        &fixture.snapshots,
        fixture.owner_id,
        descriptor.snapshot_id(),
        32,
        observed_at(),
    )
    .await;
    assert_eq!(
        node_entry(&entries, source.id()).parent_node_id(),
        if included {
            moved.parent_node_id()
        } else {
            source.parent_node_id()
        }
    );

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_durable_snapshot_concurrent_trash_is_before_or_after_its_persisted_cut() {
    let fixture = Fixture::new().await;
    let node = fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("trash-before"),
        )
        .await
        .expect("trash fixture node must be created");
    let snapshots = fixture.snapshots.clone();
    let metadata = fixture.metadata.clone();
    let (snapshot_result, trashed_result) = tokio::join!(
        snapshots.create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, observed_at()),
        metadata.delete_node(fixture.owner_id, node.id(), node.revision())
    );
    let descriptor = snapshot_result.expect("concurrent durable snapshot must materialize");
    let trashed = trashed_result.expect("concurrent trash must commit");
    let event = event_for(
        &fixture.journal,
        fixture.owner_id,
        fixture.library_id,
        node.id(),
        ChangeKind::NodeTrashed,
    )
    .await;
    let included = event_is_in_descriptor_cut(descriptor, event);
    let (entries, _) = read_all(
        &fixture.snapshots,
        fixture.owner_id,
        descriptor.snapshot_id(),
        32,
        observed_at(),
    )
    .await;
    assert_eq!(
        node_entry(&entries, node.id()).state(),
        if included {
            trashed.state()
        } else {
            NodeState::Active
        }
    );

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_durable_snapshot_concurrent_restore_is_before_or_after_its_persisted_cut() {
    let fixture = Fixture::new().await;
    let node = fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("restore-before"),
        )
        .await
        .expect("restore fixture node must be created");
    let trashed = fixture
        .metadata
        .delete_node(fixture.owner_id, node.id(), node.revision())
        .await
        .expect("restore fixture node must be trashed");
    let snapshots = fixture.snapshots.clone();
    let metadata = fixture.metadata.clone();
    let (snapshot_result, restored_result) = tokio::join!(
        snapshots.create_rebaseline_snapshot(fixture.owner_id, fixture.library_id, observed_at()),
        metadata.restore_node(fixture.owner_id, node.id(), trashed.revision())
    );
    let descriptor = snapshot_result.expect("concurrent durable snapshot must materialize");
    let restored = restored_result.expect("concurrent restore must commit");
    let event = event_for(
        &fixture.journal,
        fixture.owner_id,
        fixture.library_id,
        node.id(),
        ChangeKind::NodeRestored,
    )
    .await;
    let included = event_is_in_descriptor_cut(descriptor, event);
    let (entries, _) = read_all(
        &fixture.snapshots,
        fixture.owner_id,
        descriptor.snapshot_id(),
        32,
        observed_at(),
    )
    .await;
    assert_eq!(
        node_entry(&entries, node.id()).state(),
        if included {
            restored.state()
        } else {
            NodeState::Trashed
        }
    );

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_durable_snapshot_migration_from_34_preserves_owner_library_node_journal_and_checkpoint()
 {
    let base = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable PostgreSQL 17 database");
    let db_name = format!("p82_upgrade_{}", Uuid::now_v7().simple());
    let maintenance = PgPool::connect(&base)
        .await
        .expect("maintenance connection must succeed");
    sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
        .execute(&maintenance)
        .await
        .expect("upgrade fixture database must be created");
    maintenance.close().await;
    let url = format!(
        "{}/{}",
        base.rsplit_once('/').expect("test URL must have a path").0,
        db_name
    );
    let stage = std::env::temp_dir().join(format!("p82_migrations_{}", Uuid::now_v7().simple()));
    std::fs::create_dir_all(&stage).expect("migration staging directory must exist");
    let mut migrations: Vec<_> = std::fs::read_dir("../../migrations")
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
                && name.as_str() < "20260908000000_rebaseline_durable_snapshots.sql"
        })
        .collect();
    migrations.sort();
    assert_eq!(migrations.len(), 34);
    for migration in &migrations {
        std::fs::copy(
            format!("../../migrations/{migration}"),
            stage.join(migration),
        )
        .expect("historical migration must stage byte-for-byte");
    }

    let config = DatabaseConfig::from_url(&url).expect("upgrade URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("upgrade pool must connect");
    let historical = MigrationRunner::from_path(&stage)
        .run(&pool)
        .await
        .expect("historical 34-migration state must apply");
    assert_eq!(historical.applied_versions().len(), 34);
    let owner_id = UserId::new();
    insert_user(&pool, owner_id, "upgrade-owner").await;
    let (library_id, root) = insert_library(&pool, owner_id, "upgrade-library").await;
    let metadata = FileMetadataService::new(pool.clone());
    let node = metadata
        .create_directory(owner_id, library_id, Some(root.id()), name("upgrade-node"))
        .await
        .expect("pre-upgrade node must persist with journal event");
    let mut device = Device::new(
        DeviceId::new(),
        owner_id,
        name("upgrade-device"),
        observed_at(),
    );
    device
        .transition_status(DeviceStatus::Active, observed_at())
        .expect("pre-upgrade device must activate");
    DomainRepository::new(&pool)
        .insert_device(&device)
        .await
        .expect("pre-upgrade device must persist");
    let sync = DeviceSyncService::new(pool.clone());
    let checkpoint = sync
        .ensure_checkpoint(owner_id, device.id(), library_id)
        .await
        .expect("pre-upgrade checkpoint must persist");

    let upgraded = MigrationRunner::new()
        .run(&pool)
        .await
        .expect("Prompt 82 migration must apply on 34-state data");
    assert!(upgraded.is_current());
    assert_eq!(upgraded.applied_versions().len(), 36);
    let journal = ChangeJournalService::new(pool.clone());
    let feed = journal
        .list_changes(owner_id, library_id, None, 500)
        .await
        .expect("pre-upgrade journal must remain readable");
    assert!(
        feed.events()
            .iter()
            .any(|event| event.resource_id() == node.id())
    );
    assert_eq!(
        sync.ensure_checkpoint(owner_id, device.id(), library_id)
            .await
            .expect("pre-upgrade checkpoint must remain readable"),
        checkpoint
    );
    let snapshots = LogicalSnapshotService::new(pool.clone());
    let descriptor = snapshots
        .create_rebaseline_snapshot(owner_id, library_id, observed_at())
        .await
        .expect("new artifact must materialize after upgrade");
    assert_eq!(descriptor.entry_count(), 2);

    pool.close().await;
    std::fs::remove_dir_all(&stage).expect("migration staging directory must remove");
}
