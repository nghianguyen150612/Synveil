use synveil_core::{
    ChangeEvent, ChangeKind, DedupDomainId, Device, DeviceId, DeviceStatus, Library, LibraryId,
    LogicalName, Node, NodeId, NodeState, Timestamp, User, UserId, UserStatus,
};
use synveil_metadata::{
    ChangeJournalService, DatabaseConfig, DatabasePool, DeviceSyncService, DomainRepository,
    FileMetadataService, LogicalSnapshotService, MigrationRunner, SnapshotError,
};

struct Fixture {
    pool: DatabasePool,
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
        MigrationRunner::new()
            .run(&pool)
            .await
            .expect("the current migration set must apply");

        let owner_id = UserId::new();
        insert_user(&pool, owner_id, "snapshot-owner").await;
        let (library_id, root) = insert_library(&pool, owner_id, "snapshot-library").await;
        let metadata = FileMetadataService::new(pool.clone());
        let snapshots = LogicalSnapshotService::new(pool.clone());
        let journal = ChangeJournalService::new(pool.clone());
        Self {
            pool,
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
    }
}

fn timestamp() -> Timestamp {
    Timestamp::parse("2026-09-08T00:00:00.123456Z").expect("fixture timestamp is valid")
}

fn name(value: &str) -> LogicalName {
    LogicalName::new(value).expect("fixture name is valid")
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
        name(&format!("{label}-root")),
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

fn node_entry(
    snapshot: &synveil_metadata::RebaselineSnapshot,
    node_id: NodeId,
) -> Option<&synveil_core::LogicalSnapshotNode> {
    snapshot
        .state()
        .entries()
        .iter()
        .find(|entry| entry.node_id() == node_id)
}

fn assert_atomic_event_cut(
    snapshot: &synveil_metadata::RebaselineSnapshot,
    event: ChangeEvent,
) -> bool {
    assert_eq!(snapshot.boundary().library_id(), event.library_id());
    assert_eq!(snapshot.boundary().journal_epoch(), event.journal_epoch());
    event.sequence().get() <= snapshot.boundary().sequence().get()
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_snapshot_empty_library_has_root_and_typed_boundary() {
    let fixture = Fixture::new().await;
    let snapshot = fixture
        .snapshots
        .build(fixture.owner_id, fixture.library_id)
        .await
        .expect("empty library snapshot must build");

    assert_eq!(snapshot.state().library_id(), fixture.library_id);
    assert_eq!(snapshot.state().len(), 1);
    assert_eq!(snapshot.state().root_node_id(), fixture.root.id());
    assert_eq!(snapshot.state().root().state(), NodeState::Active);
    assert_eq!(snapshot.boundary().sequence().get(), 0);
    assert_eq!(snapshot.cursor(), snapshot.boundary().cursor());

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_snapshot_preserves_single_node_hierarchy_and_trash_state() {
    let fixture = Fixture::new().await;
    let metadata = fixture.metadata.clone();
    let directory = metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("directory"),
        )
        .await
        .expect("directory must be created");
    let leaf = metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(directory.id()),
            name("leaf"),
        )
        .await
        .expect("leaf must be created");
    let trashed = metadata
        .delete_node(fixture.owner_id, leaf.id(), leaf.revision())
        .await
        .expect("leaf must be trashed");

    let snapshot = fixture
        .snapshots
        .build(fixture.owner_id, fixture.library_id)
        .await
        .expect("hierarchical snapshot must build");
    assert_eq!(snapshot.state().len(), 3);
    assert_eq!(
        node_entry(&snapshot, directory.id())
            .unwrap()
            .parent_node_id(),
        Some(fixture.root.id())
    );
    assert_eq!(
        node_entry(&snapshot, leaf.id()).unwrap().parent_node_id(),
        Some(directory.id())
    );
    assert_eq!(
        node_entry(&snapshot, leaf.id()).unwrap().state(),
        trashed.state()
    );
    assert_eq!(trashed.state(), NodeState::Trashed);
    assert!(
        snapshot
            .state()
            .entries()
            .windows(2)
            .all(|window| window[0].node_id() < window[1].node_id())
    );

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_snapshot_isolated_by_owner_and_library() {
    let fixture = Fixture::new().await;
    let other_owner = UserId::new();
    insert_user(&fixture.pool, other_owner, "snapshot-other-owner").await;
    let (other_library, other_root) = fixture
        .add_library(other_owner, "other-owner-library")
        .await;
    let other_child = fixture
        .metadata
        .create_directory(
            other_owner,
            other_library,
            Some(other_root.id()),
            name("foreign"),
        )
        .await
        .expect("other owner child must be created");
    let (second_library, second_root) = fixture
        .add_library(fixture.owner_id, "second-library")
        .await;
    let second_child = fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            second_library,
            Some(second_root.id()),
            name("second-child"),
        )
        .await
        .expect("second library child must be created");

    let owner_snapshot = fixture
        .snapshots
        .build(fixture.owner_id, fixture.library_id)
        .await
        .expect("owner snapshot must build");
    assert!(node_entry(&owner_snapshot, other_child.id()).is_none());
    assert!(node_entry(&owner_snapshot, second_child.id()).is_none());
    assert_eq!(
        fixture
            .snapshots
            .build(fixture.owner_id, other_library)
            .await,
        Err(SnapshotError::NotFound)
    );
    let other_snapshot = fixture
        .snapshots
        .build(other_owner, other_library)
        .await
        .expect("other owner snapshot must build");
    assert!(node_entry(&other_snapshot, other_child.id()).is_some());
    assert!(node_entry(&other_snapshot, fixture.root.id()).is_none());

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_snapshot_order_is_deterministic_for_a_large_fixture() {
    let fixture = Fixture::new().await;
    let mut parent = fixture.root.clone();
    for index in 0..250 {
        parent = fixture
            .metadata
            .create_directory(
                fixture.owner_id,
                fixture.library_id,
                Some(parent.id()),
                name(&format!("node-{index:03}")),
            )
            .await
            .expect("large snapshot fixture node must be created");
    }

    let first = fixture
        .snapshots
        .build(fixture.owner_id, fixture.library_id)
        .await
        .expect("first large snapshot must build");
    let second = fixture
        .snapshots
        .build(fixture.owner_id, fixture.library_id)
        .await
        .expect("second large snapshot must build");
    assert_eq!(first, second);
    assert_eq!(first.state().len(), 251);
    assert!(
        first
            .state()
            .entries()
            .windows(2)
            .all(|window| window[0].node_id() < window[1].node_id())
    );

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_snapshot_reflects_authoritative_rename_and_move_state() {
    let fixture = Fixture::new().await;
    let source = fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("source"),
        )
        .await
        .expect("source must be created");
    let destination = fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("destination"),
        )
        .await
        .expect("destination must be created");
    let renamed = fixture
        .metadata
        .rename_node(
            fixture.owner_id,
            source.id(),
            name("renamed"),
            source.revision(),
        )
        .await
        .expect("rename must commit");
    let moved = fixture
        .metadata
        .move_node(
            fixture.owner_id,
            renamed.id(),
            destination.id(),
            renamed.revision(),
        )
        .await
        .expect("move must commit");
    let snapshot = fixture
        .snapshots
        .build(fixture.owner_id, fixture.library_id)
        .await
        .expect("current-state snapshot must build");
    let entry = node_entry(&snapshot, moved.id()).expect("moved node must be present");
    assert_eq!(entry.name(), moved.name());
    assert_eq!(entry.parent_node_id(), moved.parent_node_id());
    assert_eq!(entry.revision(), moved.revision());

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_snapshot_reflects_current_trash_and_restore_state() {
    let fixture = Fixture::new().await;
    let node = fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("trashable"),
        )
        .await
        .expect("trashable node must be created");
    let trashed = fixture
        .metadata
        .delete_node(fixture.owner_id, node.id(), node.revision())
        .await
        .expect("trash must commit");
    let trashed_snapshot = fixture
        .snapshots
        .build(fixture.owner_id, fixture.library_id)
        .await
        .expect("trashed snapshot must build");
    assert_eq!(
        node_entry(&trashed_snapshot, node.id()).unwrap().state(),
        trashed.state()
    );
    assert_eq!(trashed.state(), NodeState::Trashed);

    let restored = fixture
        .metadata
        .restore_node(fixture.owner_id, node.id(), trashed.revision())
        .await
        .expect("restore must commit");
    let restored_snapshot = fixture
        .snapshots
        .build(fixture.owner_id, fixture.library_id)
        .await
        .expect("restored snapshot must build");
    assert_eq!(
        node_entry(&restored_snapshot, node.id()).unwrap().state(),
        restored.state()
    );
    assert_eq!(restored.state(), NodeState::Active);

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_snapshot_concurrent_create_is_before_or_after_the_journal_cut() {
    let fixture = Fixture::new().await;
    let snapshots = fixture.snapshots.clone();
    let metadata = fixture.metadata.clone();
    let (snapshot_result, created_result) = tokio::join!(
        snapshots.build(fixture.owner_id, fixture.library_id),
        metadata.create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("concurrent-create"),
        )
    );
    let snapshot = snapshot_result.expect("concurrent snapshot must build");
    let created = created_result.expect("concurrent create must commit");
    let event = event_for(
        &fixture.journal,
        fixture.owner_id,
        fixture.library_id,
        created.id(),
        ChangeKind::NodeCreated,
    )
    .await;
    let included = assert_atomic_event_cut(&snapshot, event);
    assert_eq!(node_entry(&snapshot, created.id()).is_some(), included);

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_snapshot_concurrent_rename_is_before_or_after_the_journal_cut() {
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
        snapshots.build(fixture.owner_id, fixture.library_id),
        metadata.rename_node(
            fixture.owner_id,
            node.id(),
            name("rename-after"),
            node.revision(),
        )
    );
    let snapshot = snapshot_result.expect("concurrent rename snapshot must build");
    let renamed = renamed_result.expect("concurrent rename must commit");
    let event = event_for(
        &fixture.journal,
        fixture.owner_id,
        fixture.library_id,
        node.id(),
        ChangeKind::NodeRenamed,
    )
    .await;
    let included = assert_atomic_event_cut(&snapshot, event);
    let entry = node_entry(&snapshot, node.id()).expect("rename target remains in snapshot");
    assert_eq!(
        entry.name(),
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
async fn postgres_snapshot_concurrent_move_is_before_or_after_the_journal_cut() {
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
        snapshots.build(fixture.owner_id, fixture.library_id),
        metadata.move_node(
            fixture.owner_id,
            source.id(),
            destination.id(),
            source.revision(),
        )
    );
    let snapshot = snapshot_result.expect("concurrent move snapshot must build");
    let moved = moved_result.expect("concurrent move must commit");
    let event = event_for(
        &fixture.journal,
        fixture.owner_id,
        fixture.library_id,
        source.id(),
        ChangeKind::NodeMoved,
    )
    .await;
    let included = assert_atomic_event_cut(&snapshot, event);
    let entry = node_entry(&snapshot, source.id()).expect("move target remains in snapshot");
    assert_eq!(
        entry.parent_node_id(),
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
async fn postgres_snapshot_concurrent_trash_is_before_or_after_the_journal_cut() {
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
        snapshots.build(fixture.owner_id, fixture.library_id),
        metadata.delete_node(fixture.owner_id, node.id(), node.revision())
    );
    let snapshot = snapshot_result.expect("concurrent trash snapshot must build");
    let trashed = trashed_result.expect("concurrent trash must commit");
    let event = event_for(
        &fixture.journal,
        fixture.owner_id,
        fixture.library_id,
        node.id(),
        ChangeKind::NodeTrashed,
    )
    .await;
    let included = assert_atomic_event_cut(&snapshot, event);
    let entry = node_entry(&snapshot, node.id()).expect("trashed node remains in snapshot");
    assert_eq!(
        entry.state(),
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
async fn postgres_snapshot_concurrent_restore_is_before_or_after_the_journal_cut() {
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
        .expect("restore fixture trash must commit");
    let snapshots = fixture.snapshots.clone();
    let metadata = fixture.metadata.clone();
    let (snapshot_result, restored_result) = tokio::join!(
        snapshots.build(fixture.owner_id, fixture.library_id),
        metadata.restore_node(fixture.owner_id, node.id(), trashed.revision())
    );
    let snapshot = snapshot_result.expect("concurrent restore snapshot must build");
    let restored = restored_result.expect("concurrent restore must commit");
    let event = event_for(
        &fixture.journal,
        fixture.owner_id,
        fixture.library_id,
        node.id(),
        ChangeKind::NodeRestored,
    )
    .await;
    let included = assert_atomic_event_cut(&snapshot, event);
    let entry = node_entry(&snapshot, node.id()).expect("restored node remains in snapshot");
    assert_eq!(
        entry.state(),
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
async fn postgres_snapshot_boundary_continues_without_gaps_or_duplicates() {
    let fixture = Fixture::new().await;
    let initial = fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("before-cut"),
        )
        .await
        .expect("pre-snapshot node must be created");
    let snapshot = fixture
        .snapshots
        .build(fixture.owner_id, fixture.library_id)
        .await
        .expect("continuation snapshot must build");
    assert!(node_entry(&snapshot, initial.id()).is_some());
    let after = fixture
        .metadata
        .create_directory(
            fixture.owner_id,
            fixture.library_id,
            Some(fixture.root.id()),
            name("after-cut"),
        )
        .await
        .expect("post-snapshot node must be created");
    let page = fixture
        .journal
        .list_changes(
            fixture.owner_id,
            fixture.library_id,
            Some(snapshot.cursor().encode()),
            500,
        )
        .await
        .expect("post-boundary feed must load");
    assert_eq!(page.events().len(), 1);
    assert_eq!(page.events()[0].resource_id(), after.id());
    assert_eq!(page.events()[0].change_kind(), ChangeKind::NodeCreated);
    assert!(
        page.events()
            .iter()
            .all(|event| { event.sequence().get() > snapshot.boundary().sequence().get() })
    );
    assert!(
        page.events()
            .iter()
            .all(|event| event.resource_id() != initial.id())
    );

    fixture.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_snapshot_generation_does_not_advance_device_checkpoint() {
    let fixture = Fixture::new().await;
    let mut device = Device::new(
        DeviceId::new(),
        fixture.owner_id,
        name("snapshot-device"),
        timestamp(),
    );
    device
        .transition_status(DeviceStatus::Active, timestamp())
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
    fixture
        .snapshots
        .build(fixture.owner_id, fixture.library_id)
        .await
        .expect("checkpoint-neutral snapshot must build");
    let after = sync
        .ensure_checkpoint(fixture.owner_id, device.id(), fixture.library_id)
        .await
        .expect("checkpoint must remain readable");
    assert_eq!(after, before);

    fixture.close().await;
}
