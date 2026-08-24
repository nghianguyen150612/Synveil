use synveil_core::{
    DedupDomainId, Device, DeviceId, FileVersion, FileVersionId, Library, LibraryId, LogicalName,
    Node, NodeId, NodeKind, ObjectId, ObjectReference, ObjectReplicaId, Sha256Digest, Timestamp,
    UploadOperation, UploadSessionId, User, UserId, UserStatus,
};
use synveil_metadata::{
    DatabaseConfig, DatabaseError, DatabaseErrorKind, DatabasePool, DomainRepository,
    FileMetadataError, FileMetadataService, MetadataError, MigrationRunner, NewUploadSession,
    ObjectRow, PostgresUploadRepository, UploadClaim, UploadDurabilityReceipt, UploadFinalization,
    UploadMetadataBackend, UserRow,
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
        Some(trashed_file.clone())
    );
    assert_eq!(
        repository.find_object(object.object_id()).await.unwrap(),
        Some(object)
    );
    assert_eq!(
        repository.find_file_version(version.id()).await.unwrap(),
        Some(version)
    );

    let service = FileMetadataService::new(pool.clone());
    let managed_a = service
        .create_directory(user_id, library_id, Some(root.id()), name("managed-a"))
        .await
        .expect("metadata service must create a directory");
    let managed_b = service
        .create_directory(user_id, library_id, Some(root.id()), name("managed-b"))
        .await
        .expect("metadata service must create a second directory");
    assert_eq!(managed_a.revision(), synveil_core::Revision::new(0));
    assert_eq!(
        service
            .create_directory(user_id, library_id, Some(file.id()), name("invalid"))
            .await,
        Err(FileMetadataError::InvalidState)
    );
    assert_eq!(
        service
            .create_directory(user_id, library_id, Some(NodeId::new()), name("missing"))
            .await,
        Err(FileMetadataError::NotFound)
    );

    let first_page = service
        .list_children(user_id, library_id, Some(root.id()), None, 1)
        .await
        .expect("metadata child listing must be bounded");
    assert_eq!(first_page.nodes().len(), 1);
    assert!(first_page.has_more());
    let next_cursor = first_page
        .next_cursor()
        .map(str::to_owned)
        .expect("bounded listing must issue a cursor when more rows exist");
    let second_page = service
        .list_children(user_id, library_id, Some(root.id()), Some(next_cursor), 2)
        .await
        .expect("metadata cursor must resume the same listing");
    assert!(!second_page.nodes().is_empty());
    assert_eq!(
        service
            .list_children(
                user_id,
                library_id,
                Some(root.id()),
                Some("v0.node.invalid".to_owned()),
                2,
            )
            .await,
        Err(FileMetadataError::InvalidCursor)
    );
    assert_eq!(
        service.get_node(UserId::new(), managed_a.id()).await,
        Err(FileMetadataError::NotFound)
    );
    assert_eq!(
        service.get_node(user_id, managed_a.id()).await.unwrap(),
        managed_a
    );

    let renamed = service
        .rename_node(
            user_id,
            managed_a.id(),
            name("managed-renamed"),
            managed_a.revision(),
        )
        .await
        .expect("metadata service must rename a node");
    assert_eq!(renamed.revision(), synveil_core::Revision::new(1));
    assert_eq!(
        service
            .rename_node(user_id, managed_a.id(), name("stale"), managed_a.revision(),)
            .await,
        Err(FileMetadataError::VersionConflict {
            current_revision: synveil_core::Revision::new(1),
        })
    );

    let moved = service
        .move_node(user_id, managed_a.id(), managed_b.id(), renamed.revision())
        .await
        .expect("metadata service must move a node");
    assert_eq!(moved.parent_node_id(), Some(managed_b.id()));
    assert_eq!(moved.revision(), synveil_core::Revision::new(2));
    let nested = service
        .create_directory(user_id, library_id, Some(moved.id()), name("nested"))
        .await
        .expect("nested metadata directory must be created");
    assert_eq!(
        service
            .move_node(user_id, managed_b.id(), nested.id(), managed_b.revision())
            .await,
        Err(FileMetadataError::InvalidState)
    );

    let other_library_id = LibraryId::new();
    let other_root = Node::new_root(
        NodeId::new(),
        other_library_id,
        name("other-root"),
        observed_at,
    );
    let other_library = Library::new(
        other_library_id,
        user_id,
        name("Other"),
        &other_root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("second test library must be valid");
    repository
        .insert_library_with_root(&other_library, &other_root)
        .await
        .expect("second test library must persist");
    assert_eq!(
        service
            .move_node(user_id, moved.id(), other_root.id(), moved.revision())
            .await,
        Err(FileMetadataError::PermissionDenied)
    );
    assert_eq!(
        service
            .delete_node(user_id, root.id(), root.revision())
            .await,
        Err(FileMetadataError::InvalidState)
    );
    assert_eq!(
        service
            .move_node(user_id, root.id(), managed_b.id(), root.revision())
            .await,
        Err(FileMetadataError::InvalidState)
    );

    let restored_file = service
        .restore_node(user_id, file.id(), trashed_file.revision())
        .await
        .expect("metadata service must restore a valid trashed node");
    assert_eq!(restored_file.state(), synveil_core::NodeState::Active);
    assert_eq!(restored_file.revision(), synveil_core::Revision::new(3));
    let deleted_file = service
        .delete_node(user_id, file.id(), restored_file.revision())
        .await
        .expect("metadata service must logically delete a file");
    assert_eq!(deleted_file.state(), synveil_core::NodeState::Trashed);
    assert_eq!(deleted_file.revision(), synveil_core::Revision::new(4));
    assert_eq!(
        repository.find_object(object.object_id()).await.unwrap(),
        Some(object)
    );
    assert_eq!(
        repository.find_file_version(version.id()).await.unwrap(),
        Some(version)
    );
    assert_eq!(
        service
            .delete_node(user_id, directory.id(), directory.revision())
            .await,
        Err(FileMetadataError::InvalidState)
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

/// This test is intentionally ignored unless a caller supplies an explicitly
/// disposable PostgreSQL database through `SYNVEIL_TEST_DATABASE_URL`.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_upload_session_progress_and_finalization_are_atomic() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable test database");
    let config = DatabaseConfig::from_url(url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    let runner = MigrationRunner::new();
    runner
        .run(&pool)
        .await
        .expect("SQLx migration execution must succeed");

    let observed_at = timestamp("2026-08-24T00:00:00.123456Z");
    let owner = UserId::new();
    let repository = DomainRepository::new(&pool);
    let user = User::new(
        owner,
        synveil_core::LoginIdentifier::new("upload-owner", owner.to_string())
            .expect("test login identifier is valid"),
        UserStatus::Active,
        observed_at,
    );
    repository
        .insert_user(&user)
        .await
        .expect("upload owner must persist");

    let library_id = LibraryId::new();
    let root = Node::new_root(NodeId::new(), library_id, name("upload-root"), observed_at);
    let library = Library::new(
        library_id,
        owner,
        name("Upload Library"),
        &root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("upload library must satisfy domain invariants");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("upload library must persist");

    let upload_repository = PostgresUploadRepository::new(pool.clone());
    let session_id = UploadSessionId::new();
    let object_id = ObjectId::new();
    let object_replica_id = ObjectReplicaId::new();
    let object_key = format!("objects/v1/{object_id}");
    let staging_handle = format!("v1-test-{session_id}");
    let expected_hash = Sha256Digest::from_bytes([0x55; 32]);
    let input = NewUploadSession {
        id: session_id,
        owner_user_id: owner,
        library_id,
        operation: UploadOperation::CreateFile,
        target_node_id: NodeId::new(),
        target_parent_node_id: Some(root.id()),
        target_name: Some(name("uploaded.txt")),
        expected_node_revision: None,
        expected_length: 4,
        expected_sha256: Some(expected_hash),
        object_id,
        object_replica_id,
        object_key: object_key.clone(),
        staging_handle,
        max_active_sessions: 2,
        created_at: observed_at,
        expires_at: timestamp("2026-08-24T01:00:00.123456Z"),
    };
    let created = upload_repository
        .create_upload_session(input)
        .await
        .expect("upload session must persist");
    assert_eq!(created.state, synveil_core::UploadSessionState::Open);
    assert_eq!(
        upload_repository
            .find_upload_session(UserId::new(), session_id)
            .await
            .expect("wrong-owner lookup must be safe"),
        None
    );

    let progressed = upload_repository
        .record_upload_progress(owner, session_id, 0, 4, observed_at)
        .await
        .expect("upload progress must persist");
    assert_eq!(progressed.bytes_received, 4);
    let lease_until = timestamp("2026-08-24T00:01:00.123456Z");
    let claim = upload_repository
        .claim_upload(owner, session_id, observed_at, lease_until)
        .await
        .expect("upload verification claim must persist");
    let claimed = match claim {
        UploadClaim::Acquired(record) => record,
        other => panic!("unexpected upload claim: {other:?}"),
    };
    let durable = upload_repository
        .record_upload_durable(
            owner,
            session_id,
            claimed.lease_generation,
            UploadDurabilityReceipt {
                backend_kind: "LOCAL_FILESYSTEM".to_owned(),
                storage_key: object_key,
                backend_version: Some("v1".to_owned()),
                length: 4,
                sha256: expected_hash,
                verified_at: observed_at,
            },
            observed_at,
        )
        .await
        .expect("durability receipt must persist");
    assert_eq!(durable.state, synveil_core::UploadSessionState::Committing);

    let finalized = upload_repository
        .finalize_upload(owner, session_id, durable.lease_generation, observed_at)
        .await
        .expect("logical upload finalization must commit");
    let completion = match finalized {
        UploadFinalization::Completed(completion) => completion,
        other => panic!("unexpected upload finalization: {other:?}"),
    };
    assert_eq!(completion.object_id, object_id);
    assert_eq!(completion.object_replica_id, object_replica_id);
    assert_eq!(completion.length, 4);
    assert_eq!(completion.sha256, expected_hash);

    let retry = upload_repository
        .finalize_upload(owner, session_id, durable.lease_generation, observed_at)
        .await
        .expect("committed finalization retry must be safe");
    assert_eq!(retry, UploadFinalization::Completed(completion.clone()));
    let stored = upload_repository
        .find_upload_session(owner, session_id)
        .await
        .expect("stored upload lookup must succeed")
        .expect("stored upload must exist");
    assert_eq!(stored.state, synveil_core::UploadSessionState::Committed);
    assert_eq!(stored.completion, Some(completion));

    pool.close().await;
}
