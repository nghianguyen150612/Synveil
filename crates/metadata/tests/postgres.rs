use std::time::Duration;

use sqlx::PgPool;
use synveil_core::{
    DedupDomainId, Device, DeviceId, FileVersion, FileVersionId, Library, LibraryId, LogicalName,
    Node, NodeId, NodeKind, ObjectId, ObjectReference, ObjectReplicaId, Revision, Sha256Digest,
    Timestamp, TrashRetentionPolicy, UploadOperation, UploadSessionId, User, UserId, UserStatus,
};
use synveil_metadata::{
    DatabaseConfig, DatabaseError, DatabaseErrorKind, DatabasePool, DomainRepository,
    FileMetadataError, FileMetadataService, MetadataError, MigrationRunner, NewUploadSession,
    ObjectRow, PostgresUploadRepository, PurgeError, PurgeExecutionResult, TrashRetentionService,
    UploadClaim, UploadDurabilityReceipt, UploadFinalization, UploadMetadataBackend, UserRow,
    VersionHistoryError, VersionHistoryService, VersionRestoreError, VersionRestoreService,
};

fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse(value).expect("test timestamp is valid")
}

fn name(value: &str) -> LogicalName {
    LogicalName::new(value).expect("test logical name is valid")
}

async fn count_file_versions(pool: &PgPool, library_id: LibraryId, node_id: NodeId) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM file_versions
         WHERE library_id = $1 AND node_id = $2",
    )
    .bind(library_id.into_uuid())
    .bind(node_id.into_uuid())
    .fetch_one(pool)
    .await
    .expect("file-version count query must succeed")
}

async fn restore_operation_counts(pool: &PgPool, owner_id: UserId, key: &str) -> (i64, i64) {
    sqlx::query_as::<_, (i64, i64)>(
        "SELECT count(*), count(*) FILTER (WHERE result_version_id IS NOT NULL)
         FROM file_version_restore_operations
         WHERE owner_user_id = $1 AND idempotency_key = $2",
    )
    .bind(owner_id.into_uuid())
    .bind(key)
    .fetch_one(pool)
    .await
    .expect("restore-operation count query must succeed")
}

async fn count_verified_replicas(pool: &PgPool, object: ObjectReference) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM object_replicas
         WHERE object_id = $1
           AND object_dedup_domain_id = $2
           AND state = 'VERIFIED'
           AND stored_length = $3::NUMERIC
           AND stored_sha256 = $4",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .bind(object.plaintext_length().to_string())
    .bind(object.canonical_hash().as_bytes())
    .fetch_one(pool)
    .await
    .expect("verified-replica count query must succeed")
}

async fn count_object_references(pool: &PgPool, object: ObjectReference) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM file_versions
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .fetch_one(pool)
    .await
    .expect("object-reference count query must succeed")
}

async fn count_gc_candidates(pool: &PgPool, object: ObjectReference) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM object_gc_candidates
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .fetch_one(pool)
    .await
    .expect("object GC-candidate count query must succeed")
}

fn assert_query_failed(result: Result<(), MetadataError>) {
    assert_eq!(
        result,
        Err(MetadataError::Database(DatabaseError::Failure(
            DatabaseErrorKind::QueryFailed,
        )))
    );
}

#[derive(Clone, Copy)]
struct ReplaceUploadInput {
    owner_id: UserId,
    library_id: LibraryId,
    node_id: NodeId,
    expected_revision: Revision,
    object_id: ObjectId,
    object_replica_id: ObjectReplicaId,
    length: u64,
    sha256: Sha256Digest,
    observed_at: Timestamp,
}

async fn complete_replace_upload(
    upload_repository: &PostgresUploadRepository,
    input: ReplaceUploadInput,
) -> synveil_metadata::UploadCompletion {
    let ReplaceUploadInput {
        owner_id,
        library_id,
        node_id,
        expected_revision,
        object_id,
        object_replica_id,
        length,
        sha256,
        observed_at,
    } = input;
    let session_id = UploadSessionId::new();
    let object_key = format!("objects/v1/{object_id}");
    let input = NewUploadSession {
        id: session_id,
        owner_user_id: owner_id,
        library_id,
        operation: UploadOperation::ReplaceContent,
        target_node_id: node_id,
        target_parent_node_id: None,
        target_name: None,
        expected_node_revision: Some(expected_revision),
        expected_length: length,
        expected_sha256: Some(sha256),
        object_id,
        object_replica_id,
        object_key: object_key.clone(),
        staging_handle: format!("restore-test-{session_id}"),
        max_active_sessions: 2,
        created_at: observed_at,
        expires_at: timestamp("2026-08-24T01:00:00.123456Z"),
    };
    upload_repository
        .create_upload_session(input)
        .await
        .expect("replace upload session must persist");
    upload_repository
        .record_upload_progress(owner_id, session_id, 0, length, observed_at)
        .await
        .expect("replace upload progress must persist");
    let lease_until = timestamp("2026-08-24T00:10:00.123456Z");
    let claim = upload_repository
        .claim_upload(owner_id, session_id, observed_at, lease_until)
        .await
        .expect("replace upload claim must persist");
    let lease_generation = match claim {
        UploadClaim::Acquired(record) => record.lease_generation,
        other => panic!("replace upload claim was not acquired: {other:?}"),
    };
    upload_repository
        .record_upload_durable(
            owner_id,
            session_id,
            lease_generation,
            UploadDurabilityReceipt {
                backend_kind: "LOCAL_FILESYSTEM".to_owned(),
                storage_key: object_key,
                backend_version: Some("v1".to_owned()),
                length,
                sha256,
                verified_at: observed_at,
            },
            observed_at,
        )
        .await
        .expect("replace upload durability receipt must persist");
    match upload_repository
        .finalize_upload(owner_id, session_id, lease_generation, observed_at)
        .await
        .expect("replace upload finalization must persist")
    {
        UploadFinalization::Completed(completion) => completion,
        other => panic!("replace upload was not completed: {other:?}"),
    }
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
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to an explicitly disposable PostgreSQL database"]
async fn postgres_version_history_is_owner_scoped_keyset_paged_and_pointer_based() {
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

    let owner_id = UserId::new();
    let observed_at = timestamp("2026-08-24T00:00:00.123456Z");
    let owner = User::new(
        owner_id,
        synveil_core::LoginIdentifier::new("version-owner", owner_id.to_string())
            .expect("test login identifier is valid"),
        UserStatus::Active,
        observed_at,
    );
    let repository = DomainRepository::new(&pool);
    repository
        .insert_user(&owner)
        .await
        .expect("version-history owner must persist");

    let library_id = LibraryId::new();
    let dedup_domain_id = DedupDomainId::new();
    let root = Node::new_root(NodeId::new(), library_id, name("version-root"), observed_at);
    let library = Library::new(
        library_id,
        owner_id,
        name("Version Library"),
        &root,
        dedup_domain_id,
        observed_at,
    )
    .expect("version-history library must satisfy domain invariants");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("version-history library must persist");

    let file = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::File,
        name("history.bin"),
        observed_at,
    )
    .expect("version-history file must satisfy domain invariants");
    repository
        .insert_node(&file)
        .await
        .expect("version-history file must persist");

    let object_one = ObjectReference::new(
        ObjectId::new(),
        dedup_domain_id,
        Sha256Digest::from_bytes([0x11; 32]),
        11,
    );
    let object_two = ObjectReference::new(
        ObjectId::new(),
        dedup_domain_id,
        Sha256Digest::from_bytes([0x22; 32]),
        22,
    );
    let object_three = ObjectReference::new(
        ObjectId::new(),
        dedup_domain_id,
        Sha256Digest::from_bytes([0x33; 32]),
        33,
    );
    for object in [object_one, object_two, object_three] {
        repository
            .insert_object(object, observed_at)
            .await
            .expect("version-history object must persist");
    }

    let first_at = timestamp("2026-08-24T00:00:01.123456Z");
    let tied_at = timestamp("2026-08-24T00:00:02.123456Z");
    let first = FileVersion::new(
        FileVersionId::new(),
        &library,
        &file,
        object_one,
        None,
        first_at,
    )
    .expect("first version must satisfy domain invariants");
    repository
        .insert_file_version(first)
        .await
        .expect("first version must persist");
    let file_with_first = file
        .with_current_version(&first, first_at)
        .expect("file must accept first version");
    repository
        .update_node(&file_with_first)
        .await
        .expect("first version head must persist");

    let second = FileVersion::new(
        FileVersionId::new(),
        &library,
        &file_with_first,
        object_two,
        Some(first.id()),
        tied_at,
    )
    .expect("second version must satisfy domain invariants");
    repository
        .insert_file_version(second)
        .await
        .expect("second version must persist");
    let file_with_second = file_with_first
        .with_current_version(&second, tied_at)
        .expect("file must accept second version");
    repository
        .update_node(&file_with_second)
        .await
        .expect("second version head must persist");

    let third = FileVersion::new(
        FileVersionId::new(),
        &library,
        &file_with_second,
        object_three,
        Some(second.id()),
        tied_at,
    )
    .expect("third version must satisfy domain invariants");
    repository
        .insert_file_version(third)
        .await
        .expect("third version must persist");
    // Keep the authoritative head at the middle version even though the
    // newest timestamp/ID row is the third version. The read contract must
    // follow the pointer, not recompute currentness from ordering.
    repository
        .update_node(&file_with_second)
        .await
        .expect("authoritative version pointer must persist");

    let service = VersionHistoryService::new(pool.clone());
    let first_page = service
        .list_file_versions(owner_id, file.id(), None, 2)
        .await
        .expect("version-history page must load");
    assert_eq!(first_page.versions().len(), 2);
    assert!(first_page.has_more());
    assert_eq!(
        first_page
            .versions()
            .iter()
            .filter(|version| version.is_current())
            .count(),
        1
    );
    assert_eq!(
        first_page
            .versions()
            .iter()
            .find(|version| version.is_current())
            .expect("current version must be on the first page")
            .id(),
        second.id()
    );

    let cursor = first_page
        .next_cursor()
        .map(str::to_owned)
        .expect("version-history page must issue a cursor");
    let second_page = service
        .list_file_versions(owner_id, file.id(), Some(cursor), 2)
        .await
        .expect("version-history continuation must load");
    assert!(!second_page.has_more());
    let listed_ids = first_page
        .versions()
        .iter()
        .chain(second_page.versions().iter())
        .map(|version| version.id())
        .collect::<Vec<_>>();
    let mut expected_ids = vec![first.id(), second.id(), third.id()];
    expected_ids.sort_by(|left, right| {
        let left_time = match *left {
            id if id == first.id() => first.committed_at(),
            id if id == second.id() => second.committed_at(),
            _ => third.committed_at(),
        };
        let right_time = match *right {
            id if id == first.id() => first.committed_at(),
            id if id == second.id() => second.committed_at(),
            _ => third.committed_at(),
        };
        right_time.cmp(&left_time).then_with(|| right.cmp(left))
    });
    assert_eq!(listed_ids.len(), expected_ids.len());
    assert_eq!(listed_ids, expected_ids);

    let direct = service
        .get_file_version_metadata(owner_id, third.id())
        .await
        .expect("direct version metadata lookup must load");
    assert_eq!(direct.node_id(), file.id());
    assert_eq!(direct.byte_length(), 33);
    assert_eq!(direct.sha256(), object_three.canonical_hash());
    assert!(!direct.is_current());
    assert_eq!(
        service
            .get_file_version_metadata(UserId::new(), third.id())
            .await,
        Err(VersionHistoryError::NotFound)
    );
    assert_eq!(
        service
            .list_file_versions(owner_id, root.id(), None, 2)
            .await,
        Err(VersionHistoryError::InvalidState)
    );
    assert_eq!(
        service
            .list_file_versions(owner_id, file.id(), Some("v0.invalid".to_owned()), 2)
            .await,
        Err(VersionHistoryError::InvalidCursor)
    );

    let mut trashed = file_with_second;
    trashed
        .transition_state(
            synveil_core::NodeState::Trashed,
            timestamp("2026-08-24T00:00:03.123456Z"),
        )
        .expect("version-history file can be trashed");
    repository
        .update_node(&trashed)
        .await
        .expect("trashed version-history file must persist");
    assert_eq!(
        service
            .list_file_versions(owner_id, file.id(), None, 2)
            .await,
        Err(VersionHistoryError::NotFound)
    );
    assert_eq!(
        service
            .get_file_version_metadata(owner_id, third.id())
            .await,
        Err(VersionHistoryError::NotFound)
    );

    pool.close().await;
}

/// This test is intentionally ignored unless a caller supplies an explicitly
/// disposable PostgreSQL database through `SYNVEIL_TEST_DATABASE_URL`.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_version_restore_is_append_only_reuses_verified_content_and_replays() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable test database");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    let runner = MigrationRunner::new();
    runner
        .run(&pool)
        .await
        .expect("SQLx migration execution must succeed");
    let inspection_pool = PgPool::connect(&url)
        .await
        .expect("restore inspection connection must succeed");

    let owner_id = UserId::new();
    let observed_at = timestamp("2026-08-24T00:00:00.123456Z");
    let owner = User::new(
        owner_id,
        synveil_core::LoginIdentifier::new("restore-owner", owner_id.to_string())
            .expect("test login identifier is valid"),
        UserStatus::Active,
        observed_at,
    );
    let repository = DomainRepository::new(&pool);
    repository
        .insert_user(&owner)
        .await
        .expect("restore owner must persist");

    let library_id = LibraryId::new();
    let dedup_domain_id = DedupDomainId::new();
    let root = Node::new_root(NodeId::new(), library_id, name("restore-root"), observed_at);
    let library = Library::new(
        library_id,
        owner_id,
        name("Restore Library"),
        &root,
        dedup_domain_id,
        observed_at,
    )
    .expect("restore library must satisfy domain invariants");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("restore library must persist");

    let file = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::File,
        name("restore.bin"),
        observed_at,
    )
    .expect("restore file must satisfy domain invariants");
    repository
        .insert_node(&file)
        .await
        .expect("restore file must persist");

    let upload_repository = PostgresUploadRepository::new(pool.clone());
    let source_hash = Sha256Digest::from_bytes([0x51; 32]);
    let current_hash = Sha256Digest::from_bytes([0x61; 32]);
    let first = complete_replace_upload(
        &upload_repository,
        ReplaceUploadInput {
            owner_id,
            library_id,
            node_id: file.id(),
            expected_revision: Revision::new(0),
            object_id: ObjectId::new(),
            object_replica_id: ObjectReplicaId::new(),
            length: 51,
            sha256: source_hash,
            observed_at: timestamp("2026-08-24T00:00:01.123456Z"),
        },
    )
    .await;
    let source_before = repository
        .find_file_version(first.file_version_id)
        .await
        .expect("source version lookup must succeed")
        .expect("source version must exist");
    assert_eq!(
        count_verified_replicas(&inspection_pool, source_before.object_reference()).await,
        1
    );
    let second = complete_replace_upload(
        &upload_repository,
        ReplaceUploadInput {
            owner_id,
            library_id,
            node_id: file.id(),
            expected_revision: Revision::new(1),
            object_id: ObjectId::new(),
            object_replica_id: ObjectReplicaId::new(),
            length: 61,
            sha256: current_hash,
            observed_at: timestamp("2026-08-24T00:00:02.123456Z"),
        },
    )
    .await;
    let second_before = repository
        .find_file_version(second.file_version_id)
        .await
        .expect("current version lookup must succeed")
        .expect("current version must exist");
    let node_before = repository
        .find_node(file.id())
        .await
        .expect("current node lookup must succeed")
        .expect("current node must exist");
    assert_eq!(
        node_before.current_version_id(),
        Some(second.file_version_id)
    );
    assert_eq!(node_before.revision(), Revision::new(2));

    let history = VersionHistoryService::new(pool.clone());
    let before_restore = history
        .list_file_versions(owner_id, file.id(), None, 100)
        .await
        .expect("version history before restore must load");
    assert_eq!(before_restore.versions().len(), 2);

    let restore_service = VersionRestoreService::new(pool.clone());
    let restored = restore_service
        .restore_file_version(
            owner_id,
            file.id(),
            first.file_version_id,
            node_before.revision(),
            "restore-pg-key-001".to_owned(),
        )
        .await
        .expect("historical restore must commit");
    assert_ne!(restored.version().id(), first.file_version_id);
    assert_eq!(restored.version().node_id(), file.id());
    assert_eq!(
        restored.version().byte_length(),
        source_before.plaintext_length()
    );
    assert_eq!(restored.version().sha256(), source_before.canonical_hash());
    assert!(restored.version().committed_at() > source_before.committed_at());
    assert_eq!(restored.node_revision(), Revision::new(3));

    let restored_record = repository
        .find_file_version(restored.version().id())
        .await
        .expect("restored version lookup must succeed")
        .expect("restored version must exist");
    assert_eq!(
        restored_record.object_reference(),
        source_before.object_reference()
    );
    assert_eq!(
        restored_record.parent_version_id(),
        Some(second.file_version_id)
    );
    assert_eq!(
        repository
            .find_file_version(first.file_version_id)
            .await
            .expect("source version reread must succeed"),
        Some(source_before)
    );
    assert_eq!(
        repository
            .find_file_version(second.file_version_id)
            .await
            .expect("previous current version reread must succeed"),
        Some(second_before)
    );
    let node_after = repository
        .find_node(file.id())
        .await
        .expect("restored node lookup must succeed")
        .expect("restored node must exist");
    assert_eq!(
        node_after.current_version_id(),
        Some(restored.version().id())
    );
    assert_eq!(node_after.revision(), Revision::new(3));

    let after_restore = history
        .list_file_versions(owner_id, file.id(), None, 100)
        .await
        .expect("version history after restore must load");
    assert_eq!(after_restore.versions().len(), 3);
    assert_eq!(
        after_restore
            .versions()
            .iter()
            .filter(|version| version.is_current())
            .count(),
        1
    );
    assert!(
        after_restore
            .versions()
            .iter()
            .any(|version| version.id() == restored.version().id() && version.is_current())
    );
    assert!(
        after_restore
            .versions()
            .iter()
            .any(|version| version.id() == second.file_version_id && !version.is_current())
    );
    assert!(
        after_restore
            .versions()
            .iter()
            .any(|version| version.id() == first.file_version_id && !version.is_current())
    );
    assert_eq!(
        restore_operation_counts(&inspection_pool, owner_id, "restore-pg-key-001").await,
        (1, 1)
    );

    let replay = restore_service
        .restore_file_version(
            owner_id,
            file.id(),
            first.file_version_id,
            node_before.revision(),
            "restore-pg-key-001".to_owned(),
        )
        .await
        .expect("restore retry must replay");
    assert_eq!(replay, restored);
    let after_replay = history
        .list_file_versions(owner_id, file.id(), None, 100)
        .await
        .expect("version history after replay must load");
    assert_eq!(after_replay.versions().len(), 3);
    assert_eq!(
        restore_service
            .restore_file_version(
                owner_id,
                file.id(),
                first.file_version_id,
                node_after.revision(),
                "restore-pg-key-001".to_owned(),
            )
            .await,
        Err(VersionRestoreError::IdempotencyConflict)
    );

    drop(restore_service);
    drop(history);
    drop(upload_repository);
    pool.close().await;

    let reconnected = DatabasePool::connect(&config)
        .await
        .expect("restore reconnect must succeed");
    let reconnected_repository = DomainRepository::new(&reconnected);
    let reconnected_history = VersionHistoryService::new(reconnected.clone());
    let reconnected_restore_service = VersionRestoreService::new(reconnected.clone());
    let replay_after_reconnect = reconnected_restore_service
        .restore_file_version(
            owner_id,
            file.id(),
            first.file_version_id,
            node_before.revision(),
            "restore-pg-key-001".to_owned(),
        )
        .await
        .expect("persisted restore retry must replay after reconnect");
    assert_eq!(replay_after_reconnect, restored);
    assert_eq!(
        reconnected_history
            .list_file_versions(owner_id, file.id(), None, 100)
            .await
            .expect("version history after reconnect replay must load")
            .versions()
            .len(),
        3
    );
    assert_eq!(
        restore_operation_counts(&inspection_pool, owner_id, "restore-pg-key-001").await,
        (1, 1)
    );

    assert_eq!(
        reconnected_restore_service
            .restore_file_version(
                owner_id,
                file.id(),
                first.file_version_id,
                node_before.revision(),
                "restore-pg-key-002".to_owned(),
            )
            .await,
        Err(VersionRestoreError::VersionConflict {
            current_revision: Revision::new(3),
        })
    );
    assert_eq!(
        reconnected_restore_service
            .restore_file_version(
                owner_id,
                file.id(),
                restored.version().id(),
                node_after.revision(),
                "restore-pg-key-003".to_owned(),
            )
            .await,
        Err(VersionRestoreError::InvalidRequest)
    );
    let after_current_restore_rejection = reconnected_history
        .list_file_versions(owner_id, file.id(), None, 100)
        .await
        .expect("version history after current-version rejection must load");
    assert_eq!(after_current_restore_rejection.versions().len(), 3);

    let same_owner_file = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::File,
        name("restore-other-node.bin"),
        timestamp("2026-08-24T00:00:06.123456Z"),
    )
    .expect("same-owner source file must satisfy domain invariants");
    reconnected_repository
        .insert_node(&same_owner_file)
        .await
        .expect("same-owner source file must persist");
    let same_owner_upload_repository = PostgresUploadRepository::new(reconnected.clone());
    let same_owner_source = complete_replace_upload(
        &same_owner_upload_repository,
        ReplaceUploadInput {
            owner_id,
            library_id,
            node_id: same_owner_file.id(),
            expected_revision: Revision::new(0),
            object_id: ObjectId::new(),
            object_replica_id: ObjectReplicaId::new(),
            length: 81,
            sha256: Sha256Digest::from_bytes([0x81; 32]),
            observed_at: timestamp("2026-08-24T00:00:06.123456Z"),
        },
    )
    .await;
    assert_eq!(
        reconnected_restore_service
            .restore_file_version(
                owner_id,
                file.id(),
                same_owner_source.file_version_id,
                node_after.revision(),
                "restore-pg-key-005".to_owned(),
            )
            .await,
        Err(VersionRestoreError::NotFound)
    );

    let cross_owner_id = UserId::new();
    let cross_owner = User::new(
        cross_owner_id,
        synveil_core::LoginIdentifier::new("restore-cross-owner", cross_owner_id.to_string())
            .expect("cross-owner login identifier is valid"),
        UserStatus::Active,
        timestamp("2026-08-24T00:00:07.123456Z"),
    );
    reconnected_repository
        .insert_user(&cross_owner)
        .await
        .expect("cross-owner user must persist");
    let cross_library_id = LibraryId::new();
    let cross_dedup_domain_id = DedupDomainId::new();
    let cross_root = Node::new_root(
        NodeId::new(),
        cross_library_id,
        name("restore-cross-owner-root"),
        timestamp("2026-08-24T00:00:07.123456Z"),
    );
    let cross_library = Library::new(
        cross_library_id,
        cross_owner_id,
        name("Restore Cross Owner Library"),
        &cross_root,
        cross_dedup_domain_id,
        timestamp("2026-08-24T00:00:07.123456Z"),
    )
    .expect("cross-owner library must satisfy domain invariants");
    reconnected_repository
        .insert_library_with_root(&cross_library, &cross_root)
        .await
        .expect("cross-owner library must persist");
    let cross_file = Node::new_child(
        NodeId::new(),
        cross_library_id,
        &cross_root,
        NodeKind::File,
        name("restore-cross-owner.bin"),
        timestamp("2026-08-24T00:00:07.123456Z"),
    )
    .expect("cross-owner source file must satisfy domain invariants");
    reconnected_repository
        .insert_node(&cross_file)
        .await
        .expect("cross-owner source file must persist");
    let cross_owner_upload_repository = PostgresUploadRepository::new(reconnected.clone());
    let cross_owner_source = complete_replace_upload(
        &cross_owner_upload_repository,
        ReplaceUploadInput {
            owner_id: cross_owner_id,
            library_id: cross_library_id,
            node_id: cross_file.id(),
            expected_revision: Revision::new(0),
            object_id: ObjectId::new(),
            object_replica_id: ObjectReplicaId::new(),
            length: 91,
            sha256: Sha256Digest::from_bytes([0x91; 32]),
            observed_at: timestamp("2026-08-24T00:00:07.123456Z"),
        },
    )
    .await;
    assert_eq!(
        reconnected_restore_service
            .restore_file_version(
                owner_id,
                file.id(),
                cross_owner_source.file_version_id,
                node_after.revision(),
                "restore-pg-key-006".to_owned(),
            )
            .await,
        Err(VersionRestoreError::NotFound)
    );

    let unverified_object = ObjectReference::new(
        ObjectId::new(),
        dedup_domain_id,
        Sha256Digest::from_bytes([0x71; 32]),
        71,
    );
    reconnected_repository
        .insert_object(unverified_object, timestamp("2026-08-24T00:00:04.123456Z"))
        .await
        .expect("unverified restore object must persist");
    let unverified_version = FileVersion::new(
        FileVersionId::new(),
        &library,
        &node_after,
        unverified_object,
        Some(restored.version().id()),
        timestamp("2026-08-24T00:00:05.123456Z"),
    )
    .expect("unverified historical version must satisfy domain invariants");
    reconnected_repository
        .insert_file_version(unverified_version)
        .await
        .expect("unverified historical version must persist");
    assert_eq!(
        count_verified_replicas(&inspection_pool, unverified_object).await,
        0
    );
    let versions_before_failed_restore =
        count_file_versions(&inspection_pool, library_id, file.id()).await;
    assert_eq!(versions_before_failed_restore, 4);
    assert_eq!(
        restore_operation_counts(&inspection_pool, owner_id, "restore-pg-key-004").await,
        (0, 0)
    );
    assert_eq!(
        reconnected_restore_service
            .restore_file_version(
                owner_id,
                file.id(),
                unverified_version.id(),
                node_after.revision(),
                "restore-pg-key-004".to_owned(),
            )
            .await,
        Err(VersionRestoreError::ContentUnavailable)
    );
    assert_eq!(
        count_file_versions(&inspection_pool, library_id, file.id()).await,
        versions_before_failed_restore
    );
    assert_eq!(
        restore_operation_counts(&inspection_pool, owner_id, "restore-pg-key-004").await,
        (0, 0)
    );
    let node_after_failed_restore = reconnected_repository
        .find_node(file.id())
        .await
        .expect("node lookup after failed restore must succeed")
        .expect("node after failed restore must exist");
    assert_eq!(
        node_after_failed_restore.current_version_id(),
        Some(restored.version().id())
    );
    assert_eq!(node_after_failed_restore.revision(), Revision::new(3));
    let after_unverified_restore_rejection = reconnected_history
        .list_file_versions(owner_id, file.id(), None, 100)
        .await
        .expect("version history after unavailable-content rejection must load");
    assert_eq!(after_unverified_restore_rejection.versions().len(), 4);

    drop(cross_owner_upload_repository);
    drop(same_owner_upload_repository);
    drop(reconnected_restore_service);
    drop(reconnected_history);
    reconnected.close().await;
    inspection_pool.close().await;
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

/// This test is intentionally ignored unless a caller supplies an explicitly
/// disposable PostgreSQL database through `SYNVEIL_TEST_DATABASE_URL`.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_trash_retention_candidates_and_begin_are_metadata_only() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable test database");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    MigrationRunner::new()
        .run(&pool)
        .await
        .expect("SQLx migration execution must succeed");
    let inspection_pool = PgPool::connect(&url)
        .await
        .expect("trash-retention inspection connection must succeed");

    let repository = DomainRepository::new(&pool);
    let owner_id = UserId::new();
    let other_owner_id = UserId::new();
    let observed_at = timestamp("2026-08-22T00:00:00.123456Z");
    let old_trash_at = timestamp("2026-08-22T00:00:01.123456Z");
    let owner = User::new(
        owner_id,
        synveil_core::LoginIdentifier::new("trash-owner", owner_id.to_string())
            .expect("test login identifier is valid"),
        UserStatus::Active,
        observed_at,
    );
    let other_owner = User::new(
        other_owner_id,
        synveil_core::LoginIdentifier::new("other-trash-owner", other_owner_id.to_string())
            .expect("test login identifier is valid"),
        UserStatus::Active,
        observed_at,
    );
    repository
        .insert_user(&owner)
        .await
        .expect("trash owner must persist");
    repository
        .insert_user(&other_owner)
        .await
        .expect("second trash owner must persist");

    let library_id = LibraryId::new();
    let dedup_domain_id = DedupDomainId::new();
    let root = Node::new_root(NodeId::new(), library_id, name("trash-root"), observed_at);
    let library = Library::new(
        library_id,
        owner_id,
        name("Trash Library"),
        &root,
        dedup_domain_id,
        observed_at,
    )
    .expect("trash library must satisfy domain invariants");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("trash library must persist");

    let other_library_id = LibraryId::new();
    let other_root = Node::new_root(
        NodeId::new(),
        other_library_id,
        name("other-trash-root"),
        observed_at,
    );
    let other_library = Library::new(
        other_library_id,
        other_owner_id,
        name("Other Trash Library"),
        &other_root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("second trash library must satisfy domain invariants");
    repository
        .insert_library_with_root(&other_library, &other_root)
        .await
        .expect("second trash library must persist");

    let file = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::File,
        name("eligible-with-history"),
        observed_at,
    )
    .expect("eligible file must satisfy domain invariants");
    repository
        .insert_node(&file)
        .await
        .expect("eligible file must persist");
    let object = ObjectReference::new(
        ObjectId::new(),
        dedup_domain_id,
        Sha256Digest::from_bytes([0x72; 32]),
        72,
    );
    repository
        .insert_object(object, observed_at)
        .await
        .expect("eligible object must persist");
    let version = FileVersion::new(
        FileVersionId::new(),
        &library,
        &file,
        object,
        None,
        observed_at,
    )
    .expect("eligible file version must satisfy domain invariants");
    repository
        .insert_file_version(version)
        .await
        .expect("eligible file version must persist");
    let file_with_version = file
        .with_current_version(&version, observed_at)
        .expect("eligible file must accept its version");
    repository
        .update_node(&file_with_version)
        .await
        .expect("eligible file head must persist");
    let mut trashed_file = file_with_version;
    trashed_file
        .transition_state(synveil_core::NodeState::Trashed, old_trash_at)
        .expect("eligible file can be trashed");
    repository
        .update_node(&trashed_file)
        .await
        .expect("eligible trash state must persist");

    let replica_id = ObjectReplicaId::new();
    sqlx::query(
        "INSERT INTO object_replicas
            (id, object_id, object_dedup_domain_id, backend_kind, storage_key,
             stored_length, stored_sha256, backend_version, state, created_at, verified_at)
         VALUES ($1, $2, $3, 'LOCAL_FILESYSTEM', 'trash-retention-test-key',
                 $4::NUMERIC, $5, 'v1', 'VERIFIED', $6, $6)",
    )
    .bind(replica_id.into_uuid())
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .bind(object.plaintext_length().to_string())
    .bind(object.canonical_hash().as_bytes().to_vec())
    .bind(observed_at.as_offset_datetime())
    .execute(&inspection_pool)
    .await
    .expect("eligible object replica must persist");
    assert_eq!(count_verified_replicas(&inspection_pool, object).await, 1);

    let second_file = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::File,
        name("eligible-to-restore"),
        observed_at,
    )
    .expect("second eligible file must satisfy domain invariants");
    repository
        .insert_node(&second_file)
        .await
        .expect("second eligible file must persist");
    let mut trashed_second = second_file;
    trashed_second
        .transition_state(synveil_core::NodeState::Trashed, old_trash_at)
        .expect("second file can be trashed");
    repository
        .update_node(&trashed_second)
        .await
        .expect("second trash state must persist");

    let empty_directory = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::Directory,
        name("empty-directory"),
        observed_at,
    )
    .expect("empty directory must satisfy domain invariants");
    repository
        .insert_node(&empty_directory)
        .await
        .expect("empty directory must persist");
    let mut trashed_empty_directory = empty_directory;
    trashed_empty_directory
        .transition_state(synveil_core::NodeState::Trashed, old_trash_at)
        .expect("empty directory can be trashed");
    repository
        .update_node(&trashed_empty_directory)
        .await
        .expect("empty-directory trash state must persist");

    let nonempty_directory = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::Directory,
        name("nonempty-directory"),
        observed_at,
    )
    .expect("nonempty directory must satisfy domain invariants");
    repository
        .insert_node(&nonempty_directory)
        .await
        .expect("nonempty directory must persist");
    let child = Node::new_child(
        NodeId::new(),
        library_id,
        &nonempty_directory,
        NodeKind::File,
        name("child"),
        observed_at,
    )
    .expect("directory child must satisfy domain invariants");
    repository
        .insert_node(&child)
        .await
        .expect("directory child must persist");
    let metadata = FileMetadataService::new(pool.clone());
    assert_eq!(
        metadata
            .delete_node(
                owner_id,
                nonempty_directory.id(),
                nonempty_directory.revision()
            )
            .await,
        Err(FileMetadataError::InvalidState)
    );

    let other_file = Node::new_child(
        NodeId::new(),
        other_library_id,
        &other_root,
        NodeKind::File,
        name("other-owner-file"),
        observed_at,
    )
    .expect("other-owner file must satisfy domain invariants");
    repository
        .insert_node(&other_file)
        .await
        .expect("other-owner file must persist");
    let mut trashed_other_file = other_file;
    trashed_other_file
        .transition_state(synveil_core::NodeState::Trashed, old_trash_at)
        .expect("other-owner file can be trashed");
    repository
        .update_node(&trashed_other_file)
        .await
        .expect("other-owner trash state must persist");

    let policy = TrashRetentionPolicy::new(Duration::from_secs(1)).unwrap();
    let retention = TrashRetentionService::new(pool.clone(), policy);
    let mut cursor = None;
    let mut candidates = Vec::new();
    loop {
        let page = retention
            .list_purge_candidates(cursor.take(), 1)
            .await
            .expect("purge candidates must be listed in bounded pages");
        candidates.extend(
            page.candidates()
                .iter()
                .map(|candidate| candidate.node_id()),
        );
        if !page.has_more() {
            break;
        }
        cursor = Some(
            page.next_cursor()
                .expect("a continued page must issue an opaque cursor")
                .to_owned(),
        );
    }
    let mut sorted_candidates = candidates.clone();
    sorted_candidates.sort();
    assert_eq!(candidates.len(), sorted_candidates.len());
    assert!(candidates.contains(&trashed_file.id()));
    assert!(candidates.contains(&trashed_second.id()));
    assert!(candidates.contains(&trashed_empty_directory.id()));
    assert!(candidates.contains(&trashed_other_file.id()));
    assert!(!candidates.contains(&nonempty_directory.id()));
    assert_eq!(
        candidates
            .iter()
            .filter(|id| **id == trashed_file.id())
            .count(),
        1
    );

    assert_eq!(
        retention
            .begin_node_purge(other_owner_id, trashed_file.id(), trashed_file.revision())
            .await,
        Err(PurgeError::NotFound)
    );
    let purged = retention
        .begin_node_purge(owner_id, trashed_file.id(), trashed_file.revision())
        .await
        .expect("expired owned file can begin metadata-only purge");
    assert_eq!(purged.state(), synveil_core::NodeState::Purging);
    assert_eq!(purged.trashed_at(), trashed_file.trashed_at());
    assert_eq!(
        repository.find_file_version(version.id()).await.unwrap(),
        Some(version)
    );
    assert_eq!(
        repository.find_object(object.object_id()).await.unwrap(),
        Some(object)
    );
    assert_eq!(count_verified_replicas(&inspection_pool, object).await, 1);
    assert_eq!(
        retention
            .begin_node_purge(owner_id, purged.id(), purged.revision())
            .await
            .expect("repeated purge begin with current revision is idempotent"),
        purged
    );
    assert_eq!(
        retention
            .begin_node_purge(owner_id, purged.id(), trashed_file.revision())
            .await,
        Err(PurgeError::VersionConflict {
            current_revision: purged.revision(),
        })
    );

    let restored = metadata
        .restore_node(owner_id, trashed_second.id(), trashed_second.revision())
        .await
        .expect("restore remains allowed after deadline before purge begins");
    assert_eq!(restored.state(), synveil_core::NodeState::Active);
    assert_eq!(restored.trashed_at(), None);
    let restored_page = retention
        .list_purge_candidates(None, 100)
        .await
        .expect("candidate scan after restore must succeed");
    assert!(
        !restored_page
            .candidates()
            .iter()
            .any(|candidate| candidate.node_id() == restored.id())
    );

    let fresh_file = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::File,
        name("not-yet-eligible"),
        observed_at,
    )
    .expect("fresh file must satisfy domain invariants");
    repository
        .insert_node(&fresh_file)
        .await
        .expect("fresh file must persist");
    let fresh_trashed = metadata
        .delete_node(owner_id, fresh_file.id(), fresh_file.revision())
        .await
        .expect("fresh file can be trashed");
    let long_retention = TrashRetentionService::new(
        pool.clone(),
        TrashRetentionPolicy::new(Duration::from_secs(86_400)).unwrap(),
    );
    assert_eq!(
        long_retention
            .begin_node_purge(owner_id, fresh_trashed.id(), fresh_trashed.revision())
            .await,
        Err(PurgeError::PurgeNotEligible)
    );

    let raced_file = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::File,
        name("restore-purge-race"),
        observed_at,
    )
    .expect("raced file must satisfy domain invariants");
    repository
        .insert_node(&raced_file)
        .await
        .expect("raced file must persist");
    let mut raced_trashed = raced_file;
    raced_trashed
        .transition_state(synveil_core::NodeState::Trashed, old_trash_at)
        .expect("raced file can be trashed");
    repository
        .update_node(&raced_trashed)
        .await
        .expect("raced trash state must persist");
    let restore_service = metadata.clone();
    let purge_service = retention.clone();
    let (restore_result, purge_result) = tokio::join!(
        restore_service.restore_node(owner_id, raced_trashed.id(), raced_trashed.revision()),
        purge_service.begin_node_purge(owner_id, raced_trashed.id(), raced_trashed.revision()),
    );
    assert_ne!(restore_result.is_ok(), purge_result.is_ok());
    match (restore_result, purge_result) {
        (Ok(restored), Err(PurgeError::VersionConflict { current_revision })) => {
            assert_eq!(restored.state(), synveil_core::NodeState::Active);
            assert_eq!(current_revision, restored.revision());
        }
        (Err(FileMetadataError::VersionConflict { current_revision }), Ok(purged)) => {
            assert_eq!(purged.state(), synveil_core::NodeState::Purging);
            assert_eq!(current_revision, purged.revision());
        }
        (restore_result, purge_result) => {
            panic!(
                "restore/purge race had unexpected outcomes: {restore_result:?}, {purge_result:?}"
            )
        }
    }

    inspection_pool.close().await;
    pool.close().await;
}

/// Prompt 26's destructive metadata phase is intentionally integration-only:
/// the assertions depend on PostgreSQL FK timing, row locks, set-based
/// FileVersion removal, and a second connection replaying the committed
/// operation.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_metadata_purge_releases_references_without_deleting_objects() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable test database");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    MigrationRunner::new()
        .run(&pool)
        .await
        .expect("SQLx migration execution must succeed");
    let inspection_pool = PgPool::connect(&url)
        .await
        .expect("purge inspection connection must succeed");

    let repository = DomainRepository::new(&pool);
    let owner_id = UserId::new();
    let other_owner_id = UserId::new();
    let observed_at = timestamp("2026-08-22T00:00:00.123456Z");
    let old_trash_at = timestamp("2026-08-22T00:00:01.123456Z");
    let owner = User::new(
        owner_id,
        synveil_core::LoginIdentifier::new("purge-owner", owner_id.to_string())
            .expect("test login identifier is valid"),
        UserStatus::Active,
        observed_at,
    );
    let other_owner = User::new(
        other_owner_id,
        synveil_core::LoginIdentifier::new("purge-other-owner", other_owner_id.to_string())
            .expect("test login identifier is valid"),
        UserStatus::Active,
        observed_at,
    );
    repository
        .insert_user(&owner)
        .await
        .expect("purge owner must persist");
    repository
        .insert_user(&other_owner)
        .await
        .expect("second purge owner must persist");

    let library_id = LibraryId::new();
    let dedup_domain_id = DedupDomainId::new();
    let root = Node::new_root(NodeId::new(), library_id, name("purge-root"), observed_at);
    let library = Library::new(
        library_id,
        owner_id,
        name("Purge Library"),
        &root,
        dedup_domain_id,
        observed_at,
    )
    .expect("purge library must satisfy domain invariants");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("purge library must persist");

    let shared_object = ObjectReference::new(
        ObjectId::new(),
        dedup_domain_id,
        Sha256Digest::from_bytes([0x51; 32]),
        51,
    );
    repository
        .insert_object(shared_object, observed_at)
        .await
        .expect("shared object must persist");
    sqlx::query(
        "INSERT INTO object_replicas
            (id, object_id, object_dedup_domain_id, backend_kind, storage_key,
             stored_length, stored_sha256, backend_version, state, created_at, verified_at)
         VALUES ($1, $2, $3, 'LOCAL_FILESYSTEM', 'prompt-26-shared-replica',
                 $4::NUMERIC, $5, 'v1', 'VERIFIED', $6, $6)",
    )
    .bind(ObjectReplicaId::new().into_uuid())
    .bind(shared_object.object_id().into_uuid())
    .bind(shared_object.dedup_domain_id().into_uuid())
    .bind(shared_object.plaintext_length().to_string())
    .bind(shared_object.canonical_hash().as_bytes().to_vec())
    .bind(observed_at.as_offset_datetime())
    .execute(&inspection_pool)
    .await
    .expect("shared verified replica must persist");

    let file_a = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::File,
        name("purge-shared-a"),
        observed_at,
    )
    .expect("first shared file must be valid");
    let file_b = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::File,
        name("purge-shared-b"),
        observed_at,
    )
    .expect("second shared file must be valid");
    repository
        .insert_node(&file_a)
        .await
        .expect("first shared file must persist");
    repository
        .insert_node(&file_b)
        .await
        .expect("second shared file must persist");

    let version_a1 = FileVersion::new(
        FileVersionId::new(),
        &library,
        &file_a,
        shared_object,
        None,
        observed_at,
    )
    .expect("first shared version must be valid");
    repository
        .insert_file_version(version_a1)
        .await
        .expect("first shared version must persist");
    let file_a_head = file_a
        .with_current_version(&version_a1, observed_at)
        .expect("first shared file must accept its first version");
    repository
        .update_node(&file_a_head)
        .await
        .expect("first shared head must persist");

    let version_a2 = FileVersion::new(
        FileVersionId::new(),
        &library,
        &file_a,
        shared_object,
        Some(version_a1.id()),
        timestamp("2026-08-22T00:00:02.123456Z"),
    )
    .expect("second shared version must be valid");
    repository
        .insert_file_version(version_a2)
        .await
        .expect("second shared version must persist");
    let file_a_head = file_a_head
        .with_current_version(&version_a2, timestamp("2026-08-22T00:00:02.123456Z"))
        .expect("first shared file must accept its second version");
    repository
        .update_node(&file_a_head)
        .await
        .expect("second shared head must persist");

    let version_b = FileVersion::new(
        FileVersionId::new(),
        &library,
        &file_b,
        shared_object,
        None,
        timestamp("2026-08-22T00:00:03.123456Z"),
    )
    .expect("surviving shared version must be valid");
    repository
        .insert_file_version(version_b)
        .await
        .expect("surviving shared version must persist");
    let file_b_head = file_b
        .with_current_version(&version_b, timestamp("2026-08-22T00:00:03.123456Z"))
        .expect("surviving shared file must accept its version");
    repository
        .update_node(&file_b_head)
        .await
        .expect("surviving shared head must persist");

    let version_b2 = FileVersion::new(
        FileVersionId::new(),
        &library,
        &file_b,
        shared_object,
        Some(version_b.id()),
        timestamp("2026-08-22T00:00:04.123456Z"),
    )
    .expect("second surviving shared version must be valid");
    repository
        .insert_file_version(version_b2)
        .await
        .expect("second surviving shared version must persist");
    let file_b_head = file_b_head
        .with_current_version(&version_b2, timestamp("2026-08-22T00:00:04.123456Z"))
        .expect("second surviving shared head must advance");
    repository
        .update_node(&file_b_head)
        .await
        .expect("second surviving shared head must persist");

    let restore_service = VersionRestoreService::new(pool.clone());
    let restored_b = restore_service
        .restore_file_version(
            owner_id,
            file_b.id(),
            version_b.id(),
            file_b_head.revision(),
            "purge-restore-created".to_owned(),
        )
        .await
        .expect("restore-created shared reference must persist");
    assert!(restored_b.version().is_current());
    assert_eq!(
        count_object_references(&inspection_pool, shared_object).await,
        5
    );

    let metadata = FileMetadataService::new(pool.clone());
    let retention = TrashRetentionService::new(
        pool.clone(),
        TrashRetentionPolicy::new(Duration::from_secs(1)).expect("short purge policy is valid"),
    );
    let trashed_a = metadata
        .delete_node(owner_id, file_a.id(), file_a_head.revision())
        .await
        .expect("shared file can enter Trash");
    assert_eq!(
        retention
            .execute_metadata_purge(owner_id, file_a.id(), trashed_a.revision())
            .await,
        Err(PurgeError::InvalidState)
    );
    let purging_a = retention
        .begin_node_purge(owner_id, file_a.id(), trashed_a.revision())
        .await
        .expect("expired shared file can enter PURGING");
    assert_eq!(
        retention
            .execute_metadata_purge(owner_id, file_a.id(), trashed_a.revision())
            .await,
        Err(PurgeError::VersionConflict {
            current_revision: purging_a.revision(),
        })
    );

    // Force a late uniqueness failure at the completion-record insert. The
    // operation must roll back the already-issued head clear, reference
    // release, and FileVersion deletion as one PostgreSQL transaction.
    sqlx::query(
        "INSERT INTO metadata_purge_operations
            (node_id, owner_user_id, library_id, purge_revision, completed_at)
         VALUES ($1, $2, $3, $4::NUMERIC, $5)",
    )
    .bind(file_a.id().into_uuid())
    .bind(owner_id.into_uuid())
    .bind(library_id.into_uuid())
    .bind(purging_a.revision().get().to_string())
    .bind(observed_at.as_offset_datetime())
    .execute(&inspection_pool)
    .await
    .expect("rollback sentinel must persist");
    assert_eq!(
        retention
            .execute_metadata_purge(owner_id, file_a.id(), purging_a.revision())
            .await,
        Err(PurgeError::Database(DatabaseError::Failure(
            DatabaseErrorKind::QueryFailed,
        )))
    );
    assert_eq!(
        repository.find_node(file_a.id()).await.unwrap(),
        Some(purging_a.clone())
    );
    assert_eq!(
        count_file_versions(&inspection_pool, library_id, file_a.id()).await,
        2
    );
    assert_eq!(
        count_object_references(&inspection_pool, shared_object).await,
        5
    );
    assert_eq!(
        count_gc_candidates(&inspection_pool, shared_object).await,
        0
    );
    sqlx::query(
        "DELETE FROM metadata_purge_operations
         WHERE node_id = $1 AND owner_user_id = $2 AND library_id = $3",
    )
    .bind(file_a.id().into_uuid())
    .bind(owner_id.into_uuid())
    .bind(library_id.into_uuid())
    .execute(&inspection_pool)
    .await
    .expect("rollback sentinel must be removable");

    let first_worker = retention.clone();
    let second_worker = retention.clone();
    let (first_result, second_result) = tokio::join!(
        first_worker.execute_metadata_purge(owner_id, file_a.id(), purging_a.revision()),
        second_worker.execute_metadata_purge(owner_id, file_a.id(), purging_a.revision()),
    );
    assert!(
        (first_result == Ok(PurgeExecutionResult::Completed)
            && second_result == Ok(PurgeExecutionResult::AlreadyCompleted))
            || (first_result == Ok(PurgeExecutionResult::AlreadyCompleted)
                && second_result == Ok(PurgeExecutionResult::Completed)),
        "duplicate workers must have one commit and one replay: {first_result:?}, {second_result:?}"
    );
    assert_eq!(repository.find_node(file_a.id()).await.unwrap(), None);
    assert_eq!(
        metadata.get_node(owner_id, file_a.id()).await,
        Err(FileMetadataError::NotFound)
    );
    assert_eq!(
        metadata.get_node(other_owner_id, file_a.id()).await,
        Err(FileMetadataError::NotFound)
    );
    assert_eq!(
        repository.find_file_version(version_a1.id()).await.unwrap(),
        None
    );
    assert_eq!(
        repository.find_file_version(version_a2.id()).await.unwrap(),
        None
    );
    assert_eq!(
        repository.find_file_version(version_b.id()).await.unwrap(),
        Some(version_b)
    );
    assert_eq!(
        repository.find_file_version(version_b2.id()).await.unwrap(),
        Some(version_b2)
    );
    assert!(
        repository
            .find_file_version(restored_b.version().id())
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        count_object_references(&inspection_pool, shared_object).await,
        3
    );
    assert_eq!(
        count_gc_candidates(&inspection_pool, shared_object).await,
        0
    );
    assert_eq!(
        repository
            .find_object(shared_object.object_id())
            .await
            .unwrap(),
        Some(shared_object)
    );
    assert_eq!(
        count_verified_replicas(&inspection_pool, shared_object).await,
        1
    );
    assert_eq!(
        retention
            .execute_metadata_purge(other_owner_id, file_a.id(), purging_a.revision())
            .await,
        Err(PurgeError::NotFound)
    );

    let reconnected = DatabasePool::connect(&config)
        .await
        .expect("reconnect after committed purge must succeed");
    MigrationRunner::new()
        .run(&reconnected)
        .await
        .expect("reconnected database must remain migration-current");
    let replay_service = TrashRetentionService::new(
        reconnected.clone(),
        TrashRetentionPolicy::new(Duration::from_secs(1)).expect("short purge policy is valid"),
    );
    assert_eq!(
        replay_service
            .execute_metadata_purge(owner_id, file_a.id(), purging_a.revision())
            .await,
        Ok(PurgeExecutionResult::AlreadyCompleted)
    );
    let history = VersionHistoryService::new(reconnected.clone());
    assert_eq!(
        history
            .get_file_version_metadata(owner_id, version_a2.id())
            .await,
        Err(VersionHistoryError::NotFound)
    );
    reconnected.close().await;

    let trashed_b = metadata
        .delete_node(owner_id, file_b.id(), restored_b.node_revision())
        .await
        .expect("restore-created shared file can enter Trash");
    let purging_b = retention
        .begin_node_purge(owner_id, file_b.id(), trashed_b.revision())
        .await
        .expect("restore-created shared file can enter PURGING");
    assert_eq!(
        retention
            .execute_metadata_purge(owner_id, file_b.id(), purging_b.revision())
            .await,
        Ok(PurgeExecutionResult::Completed)
    );
    assert_eq!(
        count_object_references(&inspection_pool, shared_object).await,
        0
    );
    assert_eq!(
        count_gc_candidates(&inspection_pool, shared_object).await,
        1
    );
    assert_eq!(
        count_verified_replicas(&inspection_pool, shared_object).await,
        1
    );
    assert_eq!(
        restore_operation_counts(&inspection_pool, owner_id, "purge-restore-created").await,
        (0, 0)
    );
    assert_eq!(
        retention
            .execute_metadata_purge(owner_id, file_b.id(), purging_b.revision())
            .await,
        Ok(PurgeExecutionResult::AlreadyCompleted)
    );
    assert_eq!(
        count_gc_candidates(&inspection_pool, shared_object).await,
        1
    );

    let active_file = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::File,
        name("purge-active-reject"),
        observed_at,
    )
    .expect("active rejection file must be valid");
    repository
        .insert_node(&active_file)
        .await
        .expect("active rejection file must persist");
    assert_eq!(
        retention
            .execute_metadata_purge(owner_id, active_file.id(), active_file.revision())
            .await,
        Err(PurgeError::InvalidState)
    );
    assert_eq!(
        retention
            .begin_node_purge(owner_id, root.id(), root.revision())
            .await,
        Err(PurgeError::PurgeNotEligible)
    );
    let trashed_active_file = metadata
        .delete_node(owner_id, active_file.id(), active_file.revision())
        .await
        .expect("active rejection file can enter Trash");
    assert_eq!(
        retention
            .execute_metadata_purge(
                owner_id,
                trashed_active_file.id(),
                trashed_active_file.revision(),
            )
            .await,
        Err(PurgeError::InvalidState)
    );

    let protected_directory = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::Directory,
        name("purge-child-recheck"),
        observed_at,
    )
    .expect("protected directory must be valid");
    let protected_child = Node::new_child(
        NodeId::new(),
        library_id,
        &protected_directory,
        NodeKind::File,
        name("protected-child"),
        observed_at,
    )
    .expect("protected child must be valid");
    repository
        .insert_node(&protected_directory)
        .await
        .expect("protected directory must persist");
    repository
        .insert_node(&protected_child)
        .await
        .expect("protected child must persist");
    sqlx::query(
        "UPDATE nodes
         SET state = 'PURGING', trashed_at = $2, revision = $3::NUMERIC
         WHERE id = $1 AND library_id = $4",
    )
    .bind(protected_directory.id().into_uuid())
    .bind(old_trash_at.as_offset_datetime())
    .bind("1")
    .bind(library_id.into_uuid())
    .execute(&inspection_pool)
    .await
    .expect("test must be able to construct a child-protection race state");
    assert_eq!(
        retention
            .execute_metadata_purge(owner_id, protected_directory.id(), Revision::new(1))
            .await,
        Err(PurgeError::PurgeNotEligible)
    );
    assert_eq!(
        repository.find_node(protected_child.id()).await.unwrap(),
        Some(protected_child)
    );

    let staging_object = ObjectReference::new(
        ObjectId::new(),
        dedup_domain_id,
        Sha256Digest::from_bytes([0x61; 32]),
        61,
    );
    repository
        .insert_object(staging_object, observed_at)
        .await
        .expect("staging object metadata must persist");
    let upload_repository = PostgresUploadRepository::new(pool.clone());
    upload_repository
        .create_upload_session(NewUploadSession {
            id: UploadSessionId::new(),
            owner_user_id: owner_id,
            library_id,
            operation: UploadOperation::CreateFile,
            target_node_id: NodeId::new(),
            target_parent_node_id: Some(root.id()),
            target_name: Some(name("staging-upload.bin")),
            expected_node_revision: None,
            expected_length: staging_object.plaintext_length(),
            expected_sha256: Some(staging_object.canonical_hash()),
            object_id: staging_object.object_id(),
            object_replica_id: ObjectReplicaId::new(),
            object_key: "objects/v1/staging-upload".to_owned(),
            staging_handle: "prompt-26-staging".to_owned(),
            max_active_sessions: 2,
            created_at: observed_at,
            expires_at: timestamp("2026-08-22T01:00:00.123456Z"),
        })
        .await
        .expect("staging upload session must persist");
    assert_eq!(
        count_object_references(&inspection_pool, staging_object).await,
        0
    );
    assert_eq!(
        count_gc_candidates(&inspection_pool, staging_object).await,
        0
    );

    // A persisted CREATE_FILE session retains its target parent through an
    // FK. It is not a committed reference, but purge must defer rather than
    // enter PURGING and discover the FK only at the final DELETE.
    let upload_blocked_directory = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::Directory,
        name("purge-upload-parent"),
        observed_at,
    )
    .expect("upload-parent directory must be valid");
    repository
        .insert_node(&upload_blocked_directory)
        .await
        .expect("upload-parent directory must persist");
    upload_repository
        .create_upload_session(NewUploadSession {
            id: UploadSessionId::new(),
            owner_user_id: owner_id,
            library_id,
            operation: UploadOperation::CreateFile,
            target_node_id: NodeId::new(),
            target_parent_node_id: Some(upload_blocked_directory.id()),
            target_name: Some(name("blocked-upload.bin")),
            expected_node_revision: None,
            expected_length: staging_object.plaintext_length(),
            expected_sha256: Some(staging_object.canonical_hash()),
            object_id: staging_object.object_id(),
            object_replica_id: ObjectReplicaId::new(),
            object_key: "objects/v1/blocked-upload".to_owned(),
            staging_handle: "prompt-26-blocked-parent".to_owned(),
            max_active_sessions: 3,
            created_at: observed_at,
            expires_at: timestamp("2026-08-22T01:00:00.123456Z"),
        })
        .await
        .expect("blocked-parent upload session must persist");
    let blocked_trashed = metadata
        .delete_node(
            owner_id,
            upload_blocked_directory.id(),
            upload_blocked_directory.revision(),
        )
        .await
        .expect("upload-parent directory can enter Trash");
    sqlx::query(
        "UPDATE nodes SET trashed_at = $2
         WHERE id = $1 AND library_id = $3",
    )
    .bind(upload_blocked_directory.id().into_uuid())
    .bind(old_trash_at.as_offset_datetime())
    .bind(library_id.into_uuid())
    .execute(&inspection_pool)
    .await
    .expect("test must age the upload-parent Trash timestamp");
    assert_eq!(
        retention
            .begin_node_purge(
                owner_id,
                upload_blocked_directory.id(),
                blocked_trashed.revision(),
            )
            .await,
        Err(PurgeError::PurgeNotEligible)
    );

    let zero_object = ObjectReference::new(
        ObjectId::new(),
        dedup_domain_id,
        Sha256Digest::from_bytes([0; 32]),
        0,
    );
    repository
        .insert_object(zero_object, observed_at)
        .await
        .expect("zero-byte object must persist");
    sqlx::query(
        "INSERT INTO object_replicas
            (id, object_id, object_dedup_domain_id, backend_kind, storage_key,
             stored_length, stored_sha256, backend_version, state, created_at, verified_at)
         VALUES ($1, $2, $3, 'LOCAL_FILESYSTEM', 'prompt-26-zero-replica',
                 0::NUMERIC, $4, 'v1', 'VERIFIED', $5, $5)",
    )
    .bind(ObjectReplicaId::new().into_uuid())
    .bind(zero_object.object_id().into_uuid())
    .bind(zero_object.dedup_domain_id().into_uuid())
    .bind(zero_object.canonical_hash().as_bytes().to_vec())
    .bind(observed_at.as_offset_datetime())
    .execute(&inspection_pool)
    .await
    .expect("zero-byte verified replica must persist");
    let zero_file = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::File,
        name("purge-zero-byte"),
        observed_at,
    )
    .expect("zero-byte file must be valid");
    repository
        .insert_node(&zero_file)
        .await
        .expect("zero-byte file must persist");
    let zero_version = FileVersion::new(
        FileVersionId::new(),
        &library,
        &zero_file,
        zero_object,
        None,
        observed_at,
    )
    .expect("zero-byte version must be valid");
    repository
        .insert_file_version(zero_version)
        .await
        .expect("zero-byte version must persist");
    let zero_head = zero_file
        .with_current_version(&zero_version, observed_at)
        .expect("zero-byte file must accept its version");
    repository
        .update_node(&zero_head)
        .await
        .expect("zero-byte head must persist");
    let zero_trashed = metadata
        .delete_node(owner_id, zero_file.id(), zero_head.revision())
        .await
        .expect("zero-byte file can enter Trash");
    let zero_purging = retention
        .begin_node_purge(owner_id, zero_file.id(), zero_trashed.revision())
        .await
        .expect("zero-byte file can enter PURGING");
    assert_eq!(
        retention
            .execute_metadata_purge(owner_id, zero_file.id(), zero_purging.revision())
            .await,
        Ok(PurgeExecutionResult::Completed)
    );
    assert_eq!(
        count_object_references(&inspection_pool, zero_object).await,
        0
    );
    assert_eq!(count_gc_candidates(&inspection_pool, zero_object).await, 1);
    assert_eq!(
        repository
            .find_object(zero_object.object_id())
            .await
            .unwrap(),
        Some(zero_object)
    );
    assert_eq!(
        count_verified_replicas(&inspection_pool, zero_object).await,
        1
    );

    // A later committed reference removes a stale candidate transactionally;
    // candidate metadata never acts as an independent reference counter.
    let reused_file = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::File,
        name("purge-zero-reused"),
        observed_at,
    )
    .expect("reused zero-byte file must be valid");
    repository
        .insert_node(&reused_file)
        .await
        .expect("reused zero-byte file must persist");
    let reused_version = FileVersion::new(
        FileVersionId::new(),
        &library,
        &reused_file,
        zero_object,
        None,
        observed_at,
    )
    .expect("reused zero-byte version must be valid");
    repository
        .insert_file_version(reused_version)
        .await
        .expect("reused zero-byte reference must persist");
    assert_eq!(count_gc_candidates(&inspection_pool, zero_object).await, 0);
    let reused_head = reused_file
        .with_current_version(&reused_version, observed_at)
        .expect("reused zero-byte file must accept its version");
    repository
        .update_node(&reused_head)
        .await
        .expect("reused zero-byte head must persist");
    let reused_trashed = metadata
        .delete_node(owner_id, reused_file.id(), reused_head.revision())
        .await
        .expect("reused zero-byte file can enter Trash");
    let reused_purging = retention
        .begin_node_purge(owner_id, reused_file.id(), reused_trashed.revision())
        .await
        .expect("reused zero-byte file can enter PURGING");
    retention
        .execute_metadata_purge(owner_id, reused_file.id(), reused_purging.revision())
        .await
        .expect("last reused zero-byte reference can be purged");
    assert_eq!(count_gc_candidates(&inspection_pool, zero_object).await, 1);

    inspection_pool.close().await;
    pool.close().await;
}
