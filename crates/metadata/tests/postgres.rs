use synveil_core::{
    DedupDomainId, Device, DeviceId, FileVersion, FileVersionId, Library, LibraryId, LogicalName,
    Node, NodeId, NodeKind, ObjectId, ObjectReference, Sha256Digest, Timestamp, User, UserId,
    UserStatus,
};
use synveil_metadata::{
    DatabaseConfig, DatabaseError, DatabaseErrorKind, DatabasePool, DomainRepository,
    MetadataError, MigrationRunner, ObjectRow, UserRow,
};

fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse(value).expect("test timestamp is valid")
}

fn name(value: &str) -> LogicalName {
    LogicalName::new(value).expect("test logical name is valid")
}

fn assert_query_failed(result: Result<(), MetadataError>) {
    assert_eq!(
        result,
        Err(MetadataError::Database(DatabaseError::Failure(
            DatabaseErrorKind::QueryFailed,
        )))
    );
}

/// This test is intentionally ignored unless a caller supplies an explicitly
/// disposable PostgreSQL database through `SYNVEIL_TEST_DATABASE_URL`.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to an explicitly disposable PostgreSQL database"]
async fn postgres_initial_schema_and_domain_round_trip() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable test database");
    let config = DatabaseConfig::from_url(url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    let runner = MigrationRunner::new();

    let status = runner
        .run(&pool)
        .await
        .expect("SQLx migration execution must succeed");
    assert!(status.is_current());
    assert!(status.pending_versions().is_empty());
    assert!(
        pool.readiness(&runner)
            .await
            .expect("database readiness probe must succeed")
            .is_ready()
    );

    for table in [
        "users",
        "devices",
        "libraries",
        "nodes",
        "objects",
        "file_versions",
    ] {
        assert!(
            pool.table_exists(table)
                .await
                .expect("table existence probe must succeed"),
            "expected canonical table {table}"
        );
    }

    let repository = DomainRepository::new(&pool);
    let observed_at = timestamp("2026-08-22T00:00:00.123456Z");
    let user_id = UserId::new();
    let user = User::new(
        user_id,
        synveil_core::LoginIdentifier::new("alice", "alice-key")
            .expect("test login identifier is valid"),
        UserStatus::Active,
        observed_at,
    );
    repository
        .insert_user(&user)
        .await
        .expect("user row must persist");

    let device = Device::new(DeviceId::new(), user_id, name("Workstation"), observed_at);
    repository
        .insert_device(&device)
        .await
        .expect("device row must persist");

    let library_id = LibraryId::new();
    let dedup_domain_id = DedupDomainId::new();
    let root = Node::new_root(NodeId::new(), library_id, name("root"), observed_at);
    let library = Library::new(
        library_id,
        user_id,
        name("Primary"),
        &root,
        dedup_domain_id,
        observed_at,
    )
    .expect("library root must satisfy domain invariants");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("library and root must persist as one pair");

    let directory = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::Directory,
        name("folder"),
        observed_at,
    )
    .expect("directory child must satisfy domain invariants");
    repository
        .insert_node(&directory)
        .await
        .expect("directory row must persist");

    let file = Node::new_child(
        NodeId::new(),
        library_id,
        &directory,
        NodeKind::File,
        name("A/B.txt"),
        observed_at,
    )
    .expect("file child must satisfy domain invariants");
    repository
        .insert_node(&file)
        .await
        .expect("file row must persist");

    let object = ObjectReference::new(
        ObjectId::new(),
        dedup_domain_id,
        Sha256Digest::from_bytes([0x42; 32]),
        42,
    );
    repository
        .insert_object(object, observed_at)
        .await
        .expect("object row must persist independently of the filename");

    let version = FileVersion::new(
        FileVersionId::new(),
        &library,
        &file,
        object,
        None,
        observed_at,
    )
    .expect("file version must satisfy domain invariants");
    repository
        .insert_file_version(version)
        .await
        .expect("file version relationship must persist");

    let file_with_version = file
        .with_current_version(&version, observed_at)
        .expect("file node accepts its own version");
    repository
        .update_node(&file_with_version)
        .await
        .expect("node head must persist");
    let mut trashed_file = file_with_version.clone();
    trashed_file
        .transition_state(
            synveil_core::NodeState::Trashed,
            timestamp("2026-08-22T00:00:01.123456Z"),
        )
        .expect("file can be logically deleted");
    repository
        .update_node(&trashed_file)
        .await
        .expect("logical deletion metadata must persist");

    assert_eq!(
        repository.find_user(user_id).await.unwrap(),
        Some(user.clone())
    );
    assert_eq!(
        repository.find_device(device.id()).await.unwrap(),
        Some(device)
    );
    assert_eq!(
        repository.find_library(library_id).await.unwrap(),
        Some(library)
    );
    assert_eq!(
        repository.find_node(file.id()).await.unwrap(),
        Some(trashed_file)
    );
    assert_eq!(
        repository.find_object(object.object_id()).await.unwrap(),
        Some(object)
    );
    assert_eq!(
        repository.find_file_version(version.id()).await.unwrap(),
        Some(version)
    );

    let missing_owner = Device::new(DeviceId::new(), UserId::new(), name("orphan"), observed_at);
    assert_query_failed(repository.insert_device(&missing_owner).await);

    let mut negative_length = ObjectRow::from_reference(object, observed_at).unwrap();
    negative_length.id = ObjectId::new().into_uuid();
    negative_length.plaintext_length = "-1".to_owned();
    assert_query_failed(repository.insert_object_row(&negative_length).await);

    let mut overflow_length = ObjectRow::from_reference(object, observed_at).unwrap();
    overflow_length.id = ObjectId::new().into_uuid();
    overflow_length.plaintext_length = "18446744073709551616".to_owned();
    assert_query_failed(repository.insert_object_row(&overflow_length).await);

    let mut negative_revision = UserRow::from_domain(&user).unwrap();
    negative_revision.id = UserId::new().into_uuid();
    negative_revision.login_value = "negative-revision".to_owned();
    negative_revision.login_key = "negative-revision-key".to_owned();
    negative_revision.revision = "-1".to_owned();
    assert_query_failed(repository.insert_user_row(&negative_revision).await);

    let mut overflow_revision = UserRow::from_domain(&user).unwrap();
    overflow_revision.id = UserId::new().into_uuid();
    overflow_revision.login_value = "overflow-revision".to_owned();
    overflow_revision.login_key = "overflow-revision-key".to_owned();
    overflow_revision.revision = "18446744073709551616".to_owned();
    assert_query_failed(repository.insert_user_row(&overflow_revision).await);

    pool.close().await;
}
