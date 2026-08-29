use std::time::Duration;

use sqlx::PgPool;
use synveil_core::{
    ChangeEvent, ChangeEventId, ChangeKind, ClientMutation, ClientMutationId,
    ClientMutationRequest, ConflictLifecycle, ConflictResolutionAction, ConflictResolutionId,
    ConflictResolutionRequest, DedupDomainId, Device, DeviceId, DeviceStatus, FileVersion,
    FileVersionId, Library, LibraryId, LogicalName, LogicalSnapshotNode, Node, NodeId, NodeKind,
    NodeState, ObjectGcPolicy, ObjectId, ObjectReference, ObjectReplicaId, Revision, Sequence,
    Sha256Digest, SyncBootstrap, SyncConflictId, Timestamp, TrashRetentionPolicy, UploadOperation,
    UploadSessionId, User, UserId, UserStatus,
};
use synveil_metadata::{
    BootstrapPagePosition, BootstrapTerminalEvidence, ChangeJournalService, ClientMutationError,
    ClientMutationResult, ClientMutationService, ConflictManagementError,
    ConflictManagementService, ConflictResolutionResult, DatabaseConfig, DatabaseError,
    DatabaseErrorKind, DatabasePool, DeviceSyncService, DomainRepository, FileMetadataError,
    FileMetadataService, MetadataError, MigrationRunner, MutationConflictReason, NewUploadSession,
    ObjectGcCandidateState, ObjectGcError, ObjectGcLeaseReleaseResult, ObjectGcPlanResult,
    ObjectGcPlanningService, ObjectRow, PostgresUploadRepository, PurgeError, PurgeExecutionResult,
    RebaselineError, RebaselineReason, SyncAckEvidence, SyncBootstrapService, SyncError,
    TrashRetentionService, UploadClaim, UploadDurabilityReceipt, UploadFinalization,
    UploadMetadataBackend, UserRow, VersionHistoryError, VersionHistoryService,
    VersionRestoreError, VersionRestoreService,
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

async fn list_journal_events(
    journal: &ChangeJournalService,
    owner_id: UserId,
    library_id: LibraryId,
) -> Vec<ChangeEvent> {
    let mut cursor = None;
    let mut events = Vec::new();
    loop {
        let page = journal
            .list_changes(owner_id, library_id, cursor, 500)
            .await
            .expect("journal page must load");
        events.extend_from_slice(page.events());
        if !page.has_more() {
            return events;
        }
        cursor = Some(page.next_cursor().to_owned());
    }
}

fn client_mutation_request(
    mutation_id: ClientMutationId,
    base_sequence: u64,
    mutation: ClientMutation,
) -> ClientMutationRequest {
    ClientMutationRequest::new(
        mutation_id,
        Sequence::new(1),
        Sequence::new(base_sequence),
        mutation,
    )
}

#[derive(Clone, Copy, Debug)]
struct ClientMutationFixture {
    owner_id: UserId,
    other_owner_id: UserId,
    device_one_id: DeviceId,
    device_two_id: DeviceId,
    revoked_device_id: DeviceId,
    foreign_device_id: DeviceId,
    library_id: LibraryId,
    root_id: NodeId,
    other_library_id: LibraryId,
}

async fn insert_client_mutation_fixture(pool: &DatabasePool) -> ClientMutationFixture {
    let observed_at = timestamp("2026-08-27T00:00:00.123456Z");
    let owner_id = UserId::new();
    let other_owner_id = UserId::new();
    let owner_login = format!("mutation-owner-{owner_id}");
    let other_login = format!("mutation-other-{other_owner_id}");
    let owner = User::new(
        owner_id,
        synveil_core::LoginIdentifier::new(&owner_login, owner_id.to_string())
            .expect("mutation owner login must be valid"),
        UserStatus::Active,
        observed_at,
    );
    let other_owner = User::new(
        other_owner_id,
        synveil_core::LoginIdentifier::new(&other_login, other_owner_id.to_string())
            .expect("other mutation owner login must be valid"),
        UserStatus::Active,
        observed_at,
    );
    let repository = DomainRepository::new(pool);
    repository
        .insert_user(&owner)
        .await
        .expect("mutation owner must persist");
    repository
        .insert_user(&other_owner)
        .await
        .expect("other mutation owner must persist");

    let device_one =
        insert_active_sync_device(&repository, owner_id, "Mutation workstation", observed_at).await;
    let device_two =
        insert_active_sync_device(&repository, owner_id, "Mutation laptop", observed_at).await;
    let revoked_device = {
        let mut device = Device::new(
            DeviceId::new(),
            owner_id,
            name("Revoked mutation device"),
            observed_at,
        );
        device
            .transition_status(DeviceStatus::Active, observed_at)
            .expect("revoked mutation device must become active first");
        device
            .transition_status(DeviceStatus::Revoked, observed_at)
            .expect("revoked mutation device must become revoked");
        repository
            .insert_device(&device)
            .await
            .expect("revoked mutation device must persist");
        device
    };
    let foreign_device = insert_active_sync_device(
        &repository,
        other_owner_id,
        "Foreign mutation device",
        observed_at,
    )
    .await;

    let library_id = LibraryId::new();
    let root = Node::new_root(
        NodeId::new(),
        library_id,
        name("mutation-root"),
        observed_at,
    );
    let library = Library::new(
        library_id,
        owner_id,
        name("Mutation library"),
        &root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("mutation library must satisfy domain invariants");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("mutation library must persist");

    let other_library_id = LibraryId::new();
    let other_root = Node::new_root(
        NodeId::new(),
        other_library_id,
        name("other-mutation-root"),
        observed_at,
    );
    let other_library = Library::new(
        other_library_id,
        other_owner_id,
        name("Other mutation library"),
        &other_root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("other mutation library must satisfy domain invariants");
    repository
        .insert_library_with_root(&other_library, &other_root)
        .await
        .expect("other mutation library must persist");

    ClientMutationFixture {
        owner_id,
        other_owner_id,
        device_one_id: device_one.id(),
        device_two_id: device_two.id(),
        revoked_device_id: revoked_device.id(),
        foreign_device_id: foreign_device.id(),
        library_id,
        root_id: root.id(),
        other_library_id,
    }
}

fn applied_mutation_parts(result: &ClientMutationResult) -> (&Node, ChangeEventId, Sequence) {
    match result {
        ClientMutationResult::Applied {
            node,
            journal_event_id,
            journal_sequence,
            ..
        } => (node, *journal_event_id, *journal_sequence),
        ClientMutationResult::Conflict { .. } => panic!("expected an applied mutation result"),
    }
}

fn conflict_reason(result: &ClientMutationResult) -> MutationConflictReason {
    match result {
        ClientMutationResult::Conflict { conflict, .. } => conflict.reason(),
        ClientMutationResult::Applied { .. } => panic!("expected a conflict mutation result"),
    }
}

fn conflict_id(result: &ClientMutationResult) -> SyncConflictId {
    match result {
        ClientMutationResult::Conflict { conflict_id, .. } => *conflict_id,
        ClientMutationResult::Applied { .. } => panic!("expected a conflict mutation result"),
    }
}

async fn create_stale_rename_conflict(
    service: &ClientMutationService,
    inspection_pool: &PgPool,
    fixture: ClientMutationFixture,
    node: &Node,
    requested_name: &str,
) -> (ClientMutationRequest, SyncConflictId) {
    let head = library_sync_head(inspection_pool, fixture.library_id).await;
    let stale_revision = Revision::new(node.revision().get() + 1);
    let request = client_mutation_request(
        ClientMutationId::new(),
        head.get(),
        ClientMutation::rename_node(node.id(), stale_revision, name(requested_name)),
    );
    let result = service
        .submit(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            request.clone(),
        )
        .await
        .expect("stale rename must become a durable conflict");
    assert_eq!(
        conflict_reason(&result),
        MutationConflictReason::RevisionMismatch
    );
    (request, conflict_id(&result))
}

fn assert_one_applied_one_conflict(first: &ClientMutationResult, second: &ClientMutationResult) {
    assert_ne!(
        matches!(first, ClientMutationResult::Applied { .. }),
        matches!(second, ClientMutationResult::Applied { .. }),
        "a stale pair must have exactly one applied result"
    );
    assert_ne!(
        matches!(first, ClientMutationResult::Conflict { .. }),
        matches!(second, ClientMutationResult::Conflict { .. }),
        "a stale pair must have exactly one conflict result"
    );
}

async fn library_sync_head(pool: &PgPool, library_id: LibraryId) -> Sequence {
    let head = sqlx::query_scalar::<_, i64>("SELECT sync_head FROM libraries WHERE id = $1")
        .bind(library_id.into_uuid())
        .fetch_one(pool)
        .await
        .expect("library journal head query must succeed");
    Sequence::new(u64::try_from(head).expect("test journal head must be nonnegative"))
}

async fn insert_active_sync_device(
    repository: &DomainRepository<'_>,
    owner_user_id: UserId,
    label: &str,
    observed_at: Timestamp,
) -> Device {
    let mut device = Device::new(DeviceId::new(), owner_user_id, name(label), observed_at);
    device
        .transition_status(DeviceStatus::Active, observed_at)
        .expect("sync test device must become active");
    repository
        .insert_device(&device)
        .await
        .expect("sync test device must persist");
    device
}

async fn insert_verified_object(
    repository: &DomainRepository<'_>,
    inspection_pool: &PgPool,
    dedup_domain_id: DedupDomainId,
    marker: u8,
    length: u64,
    observed_at: Timestamp,
) -> ObjectReference {
    let object = ObjectReference::new(
        ObjectId::new(),
        dedup_domain_id,
        Sha256Digest::from_bytes([marker; 32]),
        length,
    );
    repository
        .insert_object(object, observed_at)
        .await
        .expect("bootstrap fixture object must persist");
    sqlx::query(
        "INSERT INTO object_replicas
            (id, object_id, object_dedup_domain_id, backend_kind, storage_key,
             stored_length, stored_sha256, backend_version, state, created_at, verified_at)
         VALUES ($1, $2, $3, 'LOCAL_FILESYSTEM', $4,
                 $5::NUMERIC, $6, 'v1', 'VERIFIED', $7, $7)",
    )
    .bind(ObjectReplicaId::new().into_uuid())
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .bind(format!("prompt-33-safe-fixture-{}", object.object_id()))
    .bind(length.to_string())
    .bind(object.canonical_hash().as_bytes().to_vec())
    .bind(observed_at.as_offset_datetime())
    .execute(inspection_pool)
    .await
    .expect("bootstrap fixture verified replica must persist");
    object
}

async fn drain_bootstrap(
    service: &SyncBootstrapService,
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
    bootstrap: SyncBootstrap,
    limit: u32,
) -> (Vec<LogicalSnapshotNode>, BootstrapTerminalEvidence) {
    let mut position: Option<BootstrapPagePosition> = None;
    let mut nodes = Vec::new();
    loop {
        let page = service
            .page_nodes(
                owner_user_id,
                device_id,
                library_id,
                bootstrap.id(),
                position,
                limit,
            )
            .await
            .expect("bootstrap manifest page must load");
        nodes.extend_from_slice(page.nodes());
        if let Some(evidence) = page.terminal_evidence() {
            return (nodes, evidence);
        }
        position = page.next_position();
        assert!(
            position.is_some(),
            "nonterminal page must carry a keyset position"
        );
    }
}

async fn count_gc_candidates_in_state(pool: &PgPool, object: ObjectReference, state: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM object_gc_candidates
         WHERE object_id = $1
           AND object_dedup_domain_id = $2
           AND state = $3",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .bind(state)
    .fetch_one(pool)
    .await
    .expect("object GC-candidate state count query must succeed")
}

async fn age_gc_candidate(pool: &PgPool, object: ObjectReference, age_seconds: i64) {
    sqlx::query(
        "UPDATE object_gc_candidates
         SET unreferenced_at = clock_timestamp()
             - ($3::BIGINT * INTERVAL '1 second')
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .bind(age_seconds)
    .execute(pool)
    .await
    .expect("object GC-candidate age update must succeed");
}

async fn expire_gc_lease(pool: &PgPool, object: ObjectReference) {
    sqlx::query(
        "UPDATE object_gc_candidates
         SET lease_acquired_at = clock_timestamp() - INTERVAL '2 seconds',
             lease_expires_at = clock_timestamp() - INTERVAL '1 second'
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .execute(pool)
    .await
    .expect("object GC lease expiry update must succeed");
}

async fn insert_gc_candidate(pool: &PgPool, object: ObjectReference, age_seconds: i64) {
    sqlx::query(
        "INSERT INTO object_gc_candidates
            (object_id, object_dedup_domain_id, unreferenced_at, source)
         VALUES ($1, $2, clock_timestamp()
                    - ($3::BIGINT * INTERVAL '1 second'), 'METADATA_PURGE')
         ON CONFLICT (object_id, object_dedup_domain_id) DO UPDATE
         SET unreferenced_at = EXCLUDED.unreferenced_at,
             state = 'ELIGIBLE', lease_id = NULL,
             lease_acquired_at = NULL, lease_expires_at = NULL,
             validated_at = NULL",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .bind(age_seconds)
    .execute(pool)
    .await
    .expect("object GC-candidate insert must succeed");
}

async fn count_objects_in_dedup_domain(pool: &PgPool, dedup_domain_id: DedupDomainId) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM objects WHERE dedup_domain_id = $1")
        .bind(dedup_domain_id.into_uuid())
        .fetch_one(pool)
        .await
        .expect("object count query must succeed")
}

async fn count_replicas_in_dedup_domain(pool: &PgPool, dedup_domain_id: DedupDomainId) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM object_replicas WHERE object_dedup_domain_id = $1",
    )
    .bind(dedup_domain_id.into_uuid())
    .fetch_one(pool)
    .await
    .expect("replica count query must succeed")
}

async fn insert_gc_object(
    repository: &DomainRepository<'_>,
    inspection_pool: &PgPool,
    dedup_domain_id: DedupDomainId,
    marker: u8,
) -> ObjectReference {
    let observed_at = timestamp("2026-08-26T00:00:00.123456Z");
    let object = ObjectReference::new(
        ObjectId::new(),
        dedup_domain_id,
        Sha256Digest::from_bytes([marker; 32]),
        u64::from(marker),
    );
    repository
        .insert_object(object, observed_at)
        .await
        .expect("GC object must persist");
    sqlx::query(
        "INSERT INTO object_replicas
            (id, object_id, object_dedup_domain_id, backend_kind, storage_key,
             stored_length, stored_sha256, backend_version, state, created_at, verified_at)
         VALUES ($1, $2, $3, 'LOCAL_FILESYSTEM', $4,
                 $5::NUMERIC, $6, 'v1', 'VERIFIED', $7, $7)",
    )
    .bind(ObjectReplicaId::new().into_uuid())
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .bind(format!("prompt-27-{marker}-{}", object.object_id()))
    .bind(object.plaintext_length().to_string())
    .bind(object.canonical_hash().as_bytes().to_vec())
    .bind(observed_at.as_offset_datetime())
    .execute(inspection_pool)
    .await
    .expect("GC object replica must persist");
    object
}

async fn insert_gc_file_version(
    repository: &DomainRepository<'_>,
    library: &Library,
    root: &Node,
    object: ObjectReference,
    label: &str,
) -> (Node, FileVersion) {
    let observed_at = timestamp("2026-08-26T00:00:00.123456Z");
    let node = Node::new_child(
        NodeId::new(),
        library.id(),
        root,
        NodeKind::File,
        name(label),
        observed_at,
    )
    .expect("GC reference node must satisfy domain invariants");
    repository
        .insert_node(&node)
        .await
        .expect("GC reference node must persist");
    let version = FileVersion::new(
        FileVersionId::new(),
        library,
        &node,
        object,
        None,
        observed_at,
    )
    .expect("GC reference version must satisfy domain invariants");
    repository
        .insert_file_version(version)
        .await
        .expect("GC reference version must persist");
    (node, version)
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
        "change_journal",
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
    let journal = ChangeJournalService::new(pool.clone());
    let empty_watermark = journal
        .get_current_high_watermark(user_id, library_id)
        .await
        .expect("new library journal watermark must load");
    assert_eq!(empty_watermark.sequence(), synveil_core::Sequence::new(0));
    let empty_page = journal
        .list_changes(user_id, library_id, None, 1)
        .await
        .expect("empty library journal page must load");
    assert!(empty_page.events().is_empty());
    assert!(!empty_page.has_more());
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

    let events = list_journal_events(&journal, user_id, library_id).await;
    assert_eq!(
        events
            .iter()
            .map(|event| event.change_kind())
            .collect::<Vec<_>>(),
        vec![
            ChangeKind::NodeCreated,
            ChangeKind::NodeCreated,
            ChangeKind::NodeRenamed,
            ChangeKind::NodeMoved,
            ChangeKind::NodeCreated,
            ChangeKind::NodeRestored,
            ChangeKind::NodeTrashed,
        ]
    );
    assert_eq!(
        events
            .iter()
            .map(|event| event.sequence().get())
            .collect::<Vec<_>>(),
        (1_u64..=7).collect::<Vec<_>>()
    );
    assert!(events.iter().all(|event| {
        event.owner_user_id() == user_id
            && event.library_id() == library_id
            && event.journal_epoch() == synveil_core::Sequence::new(1)
    }));
    assert_eq!(events[0].resource_id(), managed_a.id());
    assert_eq!(
        events[0].resource_revision(),
        synveil_core::Revision::new(0)
    );
    assert_eq!(events[2].resource_id(), managed_a.id());
    assert_eq!(
        events[2].resource_revision(),
        synveil_core::Revision::new(1)
    );
    assert_eq!(events[3].resource_id(), managed_a.id());
    assert_eq!(events[3].parent_node_id(), Some(managed_b.id()));
    assert_eq!(
        events[3].resource_revision(),
        synveil_core::Revision::new(2)
    );
    assert_eq!(events[5].resource_id(), file.id());
    assert_eq!(
        events[5].resource_revision(),
        synveil_core::Revision::new(3)
    );
    assert_eq!(events[6].resource_id(), file.id());
    assert_eq!(
        events[6].resource_revision(),
        synveil_core::Revision::new(4)
    );

    let first_journal_page = journal
        .list_changes(user_id, library_id, None, 2)
        .await
        .expect("first journal page must load");
    assert_eq!(first_journal_page.events().len(), 2);
    assert!(first_journal_page.has_more());
    let replayed_first_page = journal
        .list_changes(user_id, library_id, None, 2)
        .await
        .expect("re-reading the same journal page must load");
    assert_eq!(replayed_first_page.events(), first_journal_page.events());
    assert_eq!(first_journal_page.high_watermark().sequence().get(), 7);

    let mut paged_events = first_journal_page.events().to_vec();
    let mut has_more = first_journal_page.has_more();
    let mut cursor = Some(first_journal_page.next_cursor().to_owned());
    while has_more {
        let page = journal
            .list_changes(user_id, library_id, cursor.take(), 2)
            .await
            .expect("journal continuation page must load");
        paged_events.extend_from_slice(page.events());
        has_more = page.has_more();
        cursor = Some(page.next_cursor().to_owned());
    }
    assert_eq!(paged_events, events);
    assert_eq!(
        journal
            .list_changes(UserId::new(), library_id, None, 2)
            .await,
        Err(synveil_metadata::JournalError::NotFound)
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
/// disposable PostgreSQL database. It covers the journal's live ordering,
/// scoping, restart/resume, concurrent writers, and a late failure after the
/// journal INSERT has executed inside the mutation transaction.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_change_journal_is_ordered_scoped_resumable_and_atomic() {
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
        .expect("journal inspection connection must succeed");

    let owner_id = UserId::new();
    let observed_at = timestamp("2026-08-26T00:00:00.123456Z");
    let login_value = format!("journal-owner-{owner_id}");
    let owner = User::new(
        owner_id,
        synveil_core::LoginIdentifier::new(&login_value, owner_id.to_string())
            .expect("journal owner login identifier is valid"),
        UserStatus::Active,
        observed_at,
    );
    let repository = DomainRepository::new(&pool);
    repository
        .insert_user(&owner)
        .await
        .expect("journal owner must persist");

    let library_id = LibraryId::new();
    let root = Node::new_root(NodeId::new(), library_id, name("journal-root"), observed_at);
    let library = Library::new(
        library_id,
        owner_id,
        name("Journal Library"),
        &root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("journal library must satisfy domain invariants");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("journal library must persist");

    let metadata = FileMetadataService::new(pool.clone());
    let journal = ChangeJournalService::new(pool.clone());
    let empty = journal
        .list_changes(owner_id, library_id, None, 1)
        .await
        .expect("empty journal page must load");
    assert!(empty.events().is_empty());
    assert!(!empty.has_more());
    assert_eq!(
        empty.high_watermark().sequence(),
        synveil_core::Sequence::new(0)
    );
    assert_eq!(
        journal.list_changes(owner_id, library_id, None, 0).await,
        Err(synveil_metadata::JournalError::InvalidLimit)
    );

    let first_metadata = metadata.clone();
    let second_metadata = metadata.clone();
    let (first_result, second_result) = tokio::join!(
        first_metadata.create_directory(
            owner_id,
            library_id,
            Some(root.id()),
            name("concurrent-a"),
        ),
        second_metadata.create_directory(
            owner_id,
            library_id,
            Some(root.id()),
            name("concurrent-b"),
        ),
    );
    let first_node = first_result.expect("first concurrent directory must commit");
    let second_node = second_result.expect("second concurrent directory must commit");

    let first_page = journal
        .list_changes(owner_id, library_id, None, 1)
        .await
        .expect("first bounded journal page must load");
    assert_eq!(first_page.events().len(), 1);
    assert!(first_page.has_more());
    assert_eq!(first_page.high_watermark().sequence().get(), 2);
    let first_cursor = first_page.next_cursor().to_owned();
    let replayed_first_page = journal
        .list_changes(owner_id, library_id, None, 1)
        .await
        .expect("re-reading the first journal page must load");
    assert_eq!(replayed_first_page.events(), first_page.events());

    // Append after the first page was read. The continuation must include the
    // already committed second event and the later event without a duplicate.
    let third_node = metadata
        .create_directory(owner_id, library_id, Some(root.id()), name("reader-race"))
        .await
        .expect("reader-race directory must commit");
    let second_page = journal
        .list_changes(owner_id, library_id, Some(first_cursor), 1)
        .await
        .expect("journal continuation after concurrent append must load");
    assert_eq!(second_page.events().len(), 1);
    assert!(second_page.has_more());
    assert_eq!(second_page.events()[0].sequence().get(), 2);
    assert_eq!(second_page.high_watermark().sequence().get(), 3);
    let third_page = journal
        .list_changes(
            owner_id,
            library_id,
            Some(second_page.next_cursor().to_owned()),
            1,
        )
        .await
        .expect("final journal continuation must load");
    assert_eq!(third_page.events().len(), 1);
    assert!(!third_page.has_more());
    assert_eq!(third_page.events()[0].sequence().get(), 3);
    let paged_events = first_page
        .events()
        .iter()
        .chain(second_page.events().iter())
        .chain(third_page.events().iter())
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(
        paged_events
            .iter()
            .map(|event| event.sequence().get())
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(
        paged_events
            .iter()
            .all(|event| event.change_kind() == ChangeKind::NodeCreated)
    );
    assert!(
        paged_events
            .iter()
            .any(|event| event.resource_id() == first_node.id())
    );
    assert!(
        paged_events
            .iter()
            .any(|event| event.resource_id() == second_node.id())
    );
    assert!(
        paged_events
            .iter()
            .any(|event| event.resource_id() == third_node.id())
    );

    // History is immutable at the PostgreSQL boundary, not merely by
    // convention in the application writer. Both mutation forms must fail
    // without changing the committed event set.
    let immutable_entry_id = paged_events[0].id();
    let update_attempt = sqlx::query(
        "UPDATE change_journal
         SET change_kind = change_kind
         WHERE entry_id = $1",
    )
    .bind(immutable_entry_id.into_uuid())
    .execute(&inspection_pool)
    .await;
    assert!(update_attempt.is_err(), "journal UPDATE must be rejected");
    let delete_attempt = sqlx::query("DELETE FROM change_journal WHERE entry_id = $1")
        .bind(immutable_entry_id.into_uuid())
        .execute(&inspection_pool)
        .await;
    assert!(delete_attempt.is_err(), "journal DELETE must be rejected");

    let other_library_id = LibraryId::new();
    let other_root = Node::new_root(
        NodeId::new(),
        other_library_id,
        name("journal-other-root"),
        observed_at,
    );
    let other_library = Library::new(
        other_library_id,
        owner_id,
        name("Journal Other Library"),
        &other_root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("second journal library must satisfy domain invariants");
    repository
        .insert_library_with_root(&other_library, &other_root)
        .await
        .expect("second journal library must persist");
    let other_node = metadata
        .create_directory(
            owner_id,
            other_library_id,
            Some(other_root.id()),
            name("other-library-directory"),
        )
        .await
        .expect("other-library directory must commit");
    let other_page = journal
        .list_changes(owner_id, other_library_id, None, 1)
        .await
        .expect("other-library journal page must load");
    assert_eq!(other_page.events().len(), 1);
    assert_eq!(other_page.events()[0].resource_id(), other_node.id());
    assert_eq!(
        journal
            .list_changes(
                owner_id,
                library_id,
                Some(other_page.next_cursor().to_owned()),
                1,
            )
            .await,
        Err(synveil_metadata::JournalError::InvalidCursor)
    );
    assert_eq!(
        journal
            .list_changes(UserId::new(), library_id, None, 1)
            .await,
        Err(synveil_metadata::JournalError::NotFound)
    );

    // Force a failure after PostgreSQL has executed the journal INSERT. The
    // transaction-local writer must leave no node, journal row, or head bump.
    sqlx::query("DROP TRIGGER IF EXISTS synveil_test_fail_change_journal ON change_journal")
        .execute(&inspection_pool)
        .await
        .expect("old journal failure trigger must be removable");
    sqlx::query("DROP FUNCTION IF EXISTS synveil_test_fail_change_journal_insert()")
        .execute(&inspection_pool)
        .await
        .expect("old journal failure function must be removable");
    sqlx::query(
        "CREATE FUNCTION synveil_test_fail_change_journal_insert()
         RETURNS trigger
         LANGUAGE plpgsql
         AS $$
         BEGIN
             RAISE EXCEPTION 'prompt 31 injected journal failure';
             RETURN NEW;
         END;
         $$",
    )
    .execute(&inspection_pool)
    .await
    .expect("journal failure function must be created");
    sqlx::query(
        "CREATE TRIGGER synveil_test_fail_change_journal
         AFTER INSERT ON change_journal
         FOR EACH ROW EXECUTE FUNCTION synveil_test_fail_change_journal_insert()",
    )
    .execute(&inspection_pool)
    .await
    .expect("journal failure trigger must be created");

    let before_failure = journal
        .get_current_high_watermark(owner_id, library_id)
        .await
        .expect("pre-failure journal watermark must load");
    let failed = metadata
        .create_directory(
            owner_id,
            library_id,
            Some(root.id()),
            name("journal-rollback-directory"),
        )
        .await;
    assert!(
        failed.is_err(),
        "injected journal failure must reach caller"
    );

    sqlx::query("DROP TRIGGER synveil_test_fail_change_journal ON change_journal")
        .execute(&inspection_pool)
        .await
        .expect("journal failure trigger must be removed");
    sqlx::query("DROP FUNCTION synveil_test_fail_change_journal_insert()")
        .execute(&inspection_pool)
        .await
        .expect("journal failure function must be removed");

    let rollback_node_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM nodes
         WHERE library_id = $1 AND name = 'journal-rollback-directory'",
    )
    .bind(library_id.into_uuid())
    .fetch_one(&inspection_pool)
    .await
    .expect("rollback node count query must succeed");
    assert_eq!(rollback_node_count, 0);
    let after_failure = journal
        .get_current_high_watermark(owner_id, library_id)
        .await
        .expect("post-failure journal watermark must load");
    assert_eq!(after_failure, before_failure);
    assert_eq!(
        list_journal_events(&journal, owner_id, library_id)
            .await
            .len(),
        3
    );

    let recovered = metadata
        .create_directory(
            owner_id,
            library_id,
            Some(root.id()),
            name("journal-recovery-directory"),
        )
        .await
        .expect("post-rollback journal mutation must commit");
    assert_eq!(recovered.revision(), synveil_core::Revision::new(0));
    let recovered_events = list_journal_events(&journal, owner_id, library_id).await;
    assert_eq!(recovered_events.len(), 4);
    assert_eq!(recovered_events[3].sequence().get(), 4);
    assert_eq!(recovered_events[3].resource_id(), recovered.id());
    let (entry_count, distinct_sequence_count) = sqlx::query_as::<_, (i64, i64)>(
        "SELECT count(*), count(DISTINCT sequence)
         FROM change_journal
         WHERE owner_user_id = $1 AND library_id = $2",
    )
    .bind(owner_id.into_uuid())
    .bind(library_id.into_uuid())
    .fetch_one(&inspection_pool)
    .await
    .expect("journal sequence uniqueness query must succeed");
    assert_eq!(entry_count, distinct_sequence_count);

    let reconnected = DatabasePool::connect(&config)
        .await
        .expect("journal reconnect must succeed");
    runner
        .run(&reconnected)
        .await
        .expect("reconnected journal database must remain current");
    let reconnected_journal = ChangeJournalService::new(reconnected.clone());
    let resumed_after_restart = reconnected_journal
        .list_changes(
            owner_id,
            library_id,
            Some(first_page.next_cursor().to_owned()),
            500,
        )
        .await
        .expect("journal cursor must resume after reconnect");
    assert_eq!(
        resumed_after_restart
            .events()
            .iter()
            .map(|event| event.sequence().get())
            .collect::<Vec<_>>(),
        vec![2, 3, 4]
    );
    reconnected.close().await;
    inspection_pool.close().await;
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
    let journal = ChangeJournalService::new(pool.clone());
    let before_replay_journal = list_journal_events(&journal, owner_id, library_id).await;
    assert_eq!(
        before_replay_journal
            .iter()
            .filter(|event| event.resource_id() == file.id())
            .map(|event| event.change_kind())
            .collect::<Vec<_>>(),
        vec![
            ChangeKind::FileContentCommitted,
            ChangeKind::FileContentCommitted,
            ChangeKind::FileVersionRestored,
        ]
    );
    let restore_event = before_replay_journal
        .iter()
        .find(|event| event.change_kind() == ChangeKind::FileVersionRestored)
        .expect("version restore must have a journal event");
    assert_eq!(restore_event.resource_id(), file.id());
    assert_eq!(
        restore_event.current_version_id(),
        Some(restored.version().id())
    );
    assert_eq!(restore_event.resource_revision(), restored.node_revision());

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
    let after_replay_journal = list_journal_events(&journal, owner_id, library_id).await;
    assert_eq!(after_replay_journal, before_replay_journal);
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

    let first_upload_repository = upload_repository.clone();
    let second_upload_repository = upload_repository.clone();
    let (first_finalization, second_finalization) = tokio::join!(
        first_upload_repository.finalize_upload(
            owner,
            session_id,
            durable.lease_generation,
            observed_at,
        ),
        second_upload_repository.finalize_upload(
            owner,
            session_id,
            durable.lease_generation,
            observed_at,
        ),
    );
    let first_completion =
        match first_finalization.expect("first concurrent finalization must commit") {
            UploadFinalization::Completed(completion) => completion,
            other => panic!("unexpected first upload finalization: {other:?}"),
        };
    let second_completion =
        match second_finalization.expect("second concurrent finalization must replay") {
            UploadFinalization::Completed(completion) => completion,
            other => panic!("unexpected second upload finalization: {other:?}"),
        };
    assert_eq!(first_completion, second_completion);
    let completion = first_completion;
    assert_eq!(completion.object_id, object_id);
    assert_eq!(completion.object_replica_id, object_replica_id);
    assert_eq!(completion.length, 4);
    assert_eq!(completion.sha256, expected_hash);

    let retry = upload_repository
        .finalize_upload(owner, session_id, durable.lease_generation, observed_at)
        .await
        .expect("committed finalization retry must be safe");
    assert_eq!(retry, UploadFinalization::Completed(completion.clone()));
    let journal = ChangeJournalService::new(pool.clone());
    let events = list_journal_events(&journal, owner, library_id).await;
    assert_eq!(
        events
            .iter()
            .filter(|event| event.resource_id() == completion.node_id)
            .map(|event| event.change_kind())
            .collect::<Vec<_>>(),
        vec![ChangeKind::FileContentCommitted]
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.resource_id() == completion.node_id)
            .count(),
        1
    );
    let content_event = events
        .iter()
        .find(|event| event.resource_id() == completion.node_id)
        .expect("upload commit must have a journal event");
    assert_eq!(
        content_event.current_version_id(),
        Some(completion.file_version_id)
    );
    assert_eq!(content_event.resource_revision(), completion.node_revision);
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
    sqlx::query(
        "UPDATE nodes SET trashed_at = $2
         WHERE id = $1 AND library_id = $3",
    )
    .bind(file_a.id().into_uuid())
    .bind(old_trash_at.as_offset_datetime())
    .bind(library_id.into_uuid())
    .execute(&inspection_pool)
    .await
    .expect("test must age the shared-file Trash timestamp");
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
    let journal = ChangeJournalService::new(pool.clone());
    let purge_events = list_journal_events(&journal, owner_id, library_id).await;
    let purge_events = purge_events
        .iter()
        .filter(|event| event.resource_id() == file_a.id())
        .collect::<Vec<_>>();
    assert_eq!(
        purge_events
            .iter()
            .map(|event| event.change_kind())
            .collect::<Vec<_>>(),
        vec![ChangeKind::NodeTrashed, ChangeKind::NodePurged]
    );
    let purge_event = purge_events
        .iter()
        .find(|event| event.change_kind() == ChangeKind::NodePurged)
        .expect("metadata purge must leave a tombstone event");
    assert_eq!(purge_event.node_kind(), Some(NodeKind::File));
    assert_eq!(purge_event.node_state(), None);
    assert_eq!(purge_event.parent_node_id(), None);
    assert_eq!(purge_event.current_version_id(), None);
    assert_eq!(purge_event.resource_revision(), purging_a.revision());
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
    sqlx::query(
        "UPDATE nodes SET trashed_at = $2
         WHERE id = $1 AND library_id = $3",
    )
    .bind(file_b.id().into_uuid())
    .bind(old_trash_at.as_offset_datetime())
    .bind(library_id.into_uuid())
    .execute(&inspection_pool)
    .await
    .expect("test must age the restore-created Trash timestamp");
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
    sqlx::query(
        "UPDATE nodes SET trashed_at = $2
         WHERE id = $1 AND library_id = $3",
    )
    .bind(zero_file.id().into_uuid())
    .bind(old_trash_at.as_offset_datetime())
    .bind(library_id.into_uuid())
    .execute(&inspection_pool)
    .await
    .expect("test must age the zero-byte Trash timestamp");
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
    sqlx::query(
        "UPDATE nodes SET trashed_at = $2
         WHERE id = $1 AND library_id = $3",
    )
    .bind(reused_file.id().into_uuid())
    .bind(old_trash_at.as_offset_datetime())
    .bind(library_id.into_uuid())
    .execute(&inspection_pool)
    .await
    .expect("test must age the reused zero-byte Trash timestamp");
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

/// Prompt 27's planner test is intentionally ignored unless a caller supplies
/// a fresh disposable PostgreSQL database. It exercises the authoritative
/// candidate relation, bounded `SKIP LOCKED` claims, lease fencing and
/// metadata-only cancellation while leaving physical object storage intact.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_object_gc_planning_is_bounded_fenced_and_non_destructive() {
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
        .expect("GC inspection connection must succeed");

    let repository = DomainRepository::new(&pool);
    let owner_id = UserId::new();
    let observed_at = timestamp("2026-08-26T00:00:00.123456Z");
    let owner = User::new(
        owner_id,
        synveil_core::LoginIdentifier::new("gc-owner", owner_id.to_string())
            .expect("GC test login identifier is valid"),
        UserStatus::Active,
        observed_at,
    );
    repository
        .insert_user(&owner)
        .await
        .expect("GC owner must persist");

    let library_id = LibraryId::new();
    let dedup_domain_id = DedupDomainId::new();
    let root = Node::new_root(NodeId::new(), library_id, name("gc-root"), observed_at);
    let library = Library::new(
        library_id,
        owner_id,
        name("GC Library"),
        &root,
        dedup_domain_id,
        observed_at,
    )
    .expect("GC library must satisfy domain invariants");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("GC library must persist");

    let before = insert_gc_object(&repository, &inspection_pool, dedup_domain_id, 0x21).await;
    let exact = insert_gc_object(&repository, &inspection_pool, dedup_domain_id, 0x22).await;
    let first = insert_gc_object(&repository, &inspection_pool, dedup_domain_id, 0x23).await;
    let second = insert_gc_object(&repository, &inspection_pool, dedup_domain_id, 0x24).await;
    let third = insert_gc_object(&repository, &inspection_pool, dedup_domain_id, 0x25).await;
    let ready = insert_gc_object(&repository, &inspection_pool, dedup_domain_id, 0x26).await;
    let referenced = insert_gc_object(&repository, &inspection_pool, dedup_domain_id, 0x27).await;
    let blocked = insert_gc_object(&repository, &inspection_pool, dedup_domain_id, 0x28).await;
    let claim_race_a = insert_gc_object(&repository, &inspection_pool, dedup_domain_id, 0x29).await;
    let claim_race_b = insert_gc_object(&repository, &inspection_pool, dedup_domain_id, 0x2a).await;
    let reference_race =
        insert_gc_object(&repository, &inspection_pool, dedup_domain_id, 0x2b).await;
    let ready_race = insert_gc_object(&repository, &inspection_pool, dedup_domain_id, 0x2c).await;
    // The disposable database is intentionally shareable across integration
    // test binaries. Scope this non-destructive planner assertion to this
    // test's fresh dedup domain, rather than accidentally observing physical
    // GC fixtures owned by another test binary.
    let initial_object_count =
        count_objects_in_dedup_domain(&inspection_pool, dedup_domain_id).await;
    let initial_replica_count =
        count_replicas_in_dedup_domain(&inspection_pool, dedup_domain_id).await;

    insert_gc_candidate(&inspection_pool, before, 1).await;
    insert_gc_candidate(&inspection_pool, exact, 10).await;
    insert_gc_candidate(&inspection_pool, first, 60).await;
    insert_gc_candidate(&inspection_pool, second, 50).await;
    insert_gc_candidate(&inspection_pool, third, 40).await;
    insert_gc_candidate(&inspection_pool, ready, 30).await;
    let (_referenced_node, _referenced_version) = insert_gc_file_version(
        &repository,
        &library,
        &root,
        referenced,
        "already-referenced",
    )
    .await;
    // A candidate row alone is not authority: the planner must remove this
    // row after its locked `NOT EXISTS` reference recheck.
    insert_gc_candidate(&inspection_pool, referenced, 20).await;

    let policy = ObjectGcPolicy::new(Duration::from_secs(10), Duration::from_secs(5), 2)
        .expect("focused GC policy must be valid");
    let service = ObjectGcPlanningService::new(pool.clone(), policy);
    assert_eq!(
        service.claim_candidates(3).await,
        Err(ObjectGcError::InvalidRequest)
    );

    let first_claim = service
        .claim_candidates(2)
        .await
        .expect("first bounded GC claim must succeed");
    assert_eq!(first_claim.len(), 2);
    assert_eq!(first_claim[0].object_id(), first.object_id());
    assert_eq!(first_claim[1].object_id(), second.object_id());
    assert_eq!(
        count_gc_candidates_in_state(&inspection_pool, before, "ELIGIBLE").await,
        1
    );
    assert_eq!(
        count_gc_candidates_in_state(&inspection_pool, before, "LEASED").await,
        0
    );
    assert_eq!(count_gc_candidates(&inspection_pool, referenced).await, 1);

    let second_claim = service
        .claim_candidates(2)
        .await
        .expect("second bounded GC claim must succeed");
    assert_eq!(second_claim.len(), 2);
    assert!(
        second_claim
            .iter()
            .any(|lease| lease.object_id() == third.object_id())
    );
    assert!(
        second_claim
            .iter()
            .any(|lease| lease.object_id() == ready.object_id())
    );

    // The next bounded claim encounters the referenced row, cancels it, and
    // still leases the exact-grace candidate. The unit policy test proves the
    // mathematical boundary; this PostgreSQL assertion proves the query uses
    // the inclusive cutoff against server time.
    let third_claim = service
        .claim_candidates(2)
        .await
        .expect("third bounded GC claim must succeed");
    assert_eq!(third_claim.len(), 1);
    assert_eq!(third_claim[0].object_id(), exact.object_id());
    assert_eq!(count_gc_candidates(&inspection_pool, referenced).await, 0);

    // All mature rows are now leased and the one intentionally young row is
    // not claimable. This is the non-expired lease blocking assertion.
    assert!(
        service
            .claim_candidates(2)
            .await
            .expect("empty GC claim must succeed")
            .is_empty()
    );

    insert_gc_candidate(&inspection_pool, blocked, 60).await;
    let blocked_lease = service
        .claim_candidates(1)
        .await
        .expect("blocked candidate claim must succeed")
        .pop()
        .expect("blocked candidate must be claimed");
    assert_eq!(blocked_lease.object_id(), blocked.object_id());
    assert!(
        service
            .claim_candidates(1)
            .await
            .expect("non-expired blocked claim must succeed")
            .is_empty()
    );

    let reconnected = DatabasePool::connect(&config)
        .await
        .expect("GC worker reconnect must succeed");
    MigrationRunner::new()
        .run(&reconnected)
        .await
        .expect("reconnected GC worker must see current migrations");
    let reconnected_service = ObjectGcPlanningService::new(reconnected.clone(), policy);
    assert!(
        reconnected_service
            .claim_candidates(1)
            .await
            .expect("reconnected worker claim must succeed")
            .is_empty()
    );

    age_gc_candidate(&inspection_pool, blocked, 60).await;
    expire_gc_lease(&inspection_pool, blocked).await;
    let reclaimed_lease = reconnected_service
        .claim_candidates(1)
        .await
        .expect("expired GC lease must be reclaimable")
        .pop()
        .expect("expired blocked candidate must be reclaimed");
    assert_eq!(reclaimed_lease.object_id(), blocked.object_id());
    assert_eq!(
        reclaimed_lease.lease_generation(),
        blocked_lease.lease_generation() + 1
    );
    assert_ne!(reclaimed_lease.lease_id(), blocked_lease.lease_id());
    assert_eq!(
        service.renew_lease(blocked_lease).await,
        Err(ObjectGcError::StaleLease)
    );

    let renewed = reconnected_service
        .renew_lease(reclaimed_lease)
        .await
        .expect("matching unexpired GC lease must renew");
    assert_eq!(renewed.lease_id(), reclaimed_lease.lease_id());
    assert_eq!(
        renewed.lease_generation(),
        reclaimed_lease.lease_generation()
    );
    assert!(renewed.lease_expires_at() > reclaimed_lease.lease_expires_at());
    assert_eq!(
        reconnected_service
            .release_lease(renewed)
            .await
            .expect("matching GC lease release must succeed"),
        ObjectGcLeaseReleaseResult::Released
    );
    assert_eq!(
        reconnected_service
            .release_lease(renewed)
            .await
            .expect("GC lease release retry must be safe"),
        ObjectGcLeaseReleaseResult::AlreadyReleased
    );

    // A committed FileVersion reference cancels an eligible candidate in the
    // same candidate-first/object-second lock order.
    let (blocked_reference_node, _blocked_reference_version) = insert_gc_file_version(
        &repository,
        &library,
        &root,
        blocked,
        "reference-after-release",
    )
    .await;
    assert_eq!(count_gc_candidates(&inspection_pool, blocked).await, 0);

    // Purging that reference creates a fresh metadata candidate lifecycle.
    // Its generation starts from the new row's zero baseline, while the old
    // opaque lease IDs remain unusable even if a future row reuses a number.
    let metadata = FileMetadataService::new(pool.clone());
    let retention = TrashRetentionService::new(
        pool.clone(),
        TrashRetentionPolicy::new(Duration::from_secs(1)).expect("GC purge policy must be valid"),
    );
    let trashed_reference = metadata
        .delete_node(
            owner_id,
            blocked_reference_node.id(),
            blocked_reference_node.revision(),
        )
        .await
        .expect("reference node must enter Trash");
    sqlx::query(
        "UPDATE nodes
         SET trashed_at = clock_timestamp() - INTERVAL '60 seconds'
         WHERE id = $1 AND library_id = $2",
    )
    .bind(blocked_reference_node.id().into_uuid())
    .bind(library_id.into_uuid())
    .execute(&inspection_pool)
    .await
    .expect("reference Trash timestamp must be aged");
    let purging_reference = retention
        .begin_node_purge(
            owner_id,
            blocked_reference_node.id(),
            trashed_reference.revision(),
        )
        .await
        .expect("reference node must enter PURGING");
    assert_eq!(
        retention
            .execute_metadata_purge(
                owner_id,
                blocked_reference_node.id(),
                purging_reference.revision(),
            )
            .await,
        Ok(PurgeExecutionResult::Completed)
    );
    age_gc_candidate(&inspection_pool, blocked, 60).await;
    let fresh_lease = reconnected_service
        .claim_candidates(1)
        .await
        .expect("fresh GC lifecycle must be claimable")
        .pop()
        .expect("freshly unreferenced object must be claimed");
    assert_eq!(fresh_lease.lease_generation(), 1);
    assert_ne!(fresh_lease.lease_id(), reclaimed_lease.lease_id());
    assert_eq!(
        reconnected_service.renew_lease(reclaimed_lease).await,
        Err(ObjectGcError::StaleLease)
    );

    let ready_result = reconnected_service
        .mark_ready_for_deletion(fresh_lease)
        .await
        .expect("ready transition must revalidate references");
    let ready_candidate = match ready_result {
        ObjectGcPlanResult::Valid(candidate) => candidate,
        ObjectGcPlanResult::Invalidated => {
            panic!("unreferenced candidate was unexpectedly invalidated")
        }
    };
    assert_eq!(ready_candidate.state(), ObjectGcCandidateState::Ready);
    assert_eq!(
        ready_candidate
            .lease()
            .expect("ready candidate retains its lease")
            .state(),
        ObjectGcCandidateState::Ready
    );
    let revalidated = reconnected_service
        .revalidate_candidate(fresh_lease)
        .await
        .expect("ready candidate must remain revalidatable");
    assert!(matches!(
        revalidated,
        ObjectGcPlanResult::Valid(candidate)
            if candidate.state() == ObjectGcCandidateState::Ready
    ));

    let (_ready_reference_node, _ready_reference_version) = insert_gc_file_version(
        &repository,
        &library,
        &root,
        blocked,
        "reference-after-ready",
    )
    .await;
    assert_eq!(count_gc_candidates(&inspection_pool, blocked).await, 0);
    assert_eq!(
        reconnected_service.revalidate_candidate(fresh_lease).await,
        Err(ObjectGcError::NotFound)
    );

    // Two workers claiming two disjoint candidates concurrently must not
    // double-lease a row.
    insert_gc_candidate(&inspection_pool, claim_race_a, 60).await;
    insert_gc_candidate(&inspection_pool, claim_race_b, 60).await;
    let claim_worker_a = ObjectGcPlanningService::new(pool.clone(), policy);
    let claim_worker_b = ObjectGcPlanningService::new(pool.clone(), policy);
    let (claim_a, claim_b) = tokio::join!(
        claim_worker_a.claim_candidates(1),
        claim_worker_b.claim_candidates(1),
    );
    let claim_a = claim_a.expect("first concurrent GC claim must succeed");
    let claim_b = claim_b.expect("second concurrent GC claim must succeed");
    assert_eq!(claim_a.len(), 1);
    assert_eq!(claim_b.len(), 1);
    assert_ne!(claim_a[0].object_id(), claim_b[0].object_id());
    assert!([claim_race_a.object_id(), claim_race_b.object_id()].contains(&claim_a[0].object_id()));
    assert!([claim_race_a.object_id(), claim_race_b.object_id()].contains(&claim_b[0].object_id()));

    // Claim versus committed reference: whichever transaction wins, the
    // final candidate relation is cancelled and cannot be treated as ready.
    insert_gc_candidate(&inspection_pool, reference_race, 60).await;
    let reference_race_service = ObjectGcPlanningService::new(pool.clone(), policy);
    let (claim_result, (_race_reference_node, _race_reference_version)) = tokio::join!(
        reference_race_service.claim_candidates(1),
        insert_gc_file_version(
            &repository,
            &library,
            &root,
            reference_race,
            "claim-reference-race",
        ),
    );
    let claim_result = claim_result.expect("claim/reference race must not fail");
    assert!(claim_result.len() <= 1);
    assert_eq!(
        count_gc_candidates(&inspection_pool, reference_race).await,
        0
    );

    // Ready transition versus committed reference has the same final safety
    // invariant: no candidate survives with a committed FileVersion ref.
    insert_gc_candidate(&inspection_pool, ready_race, 60).await;
    let ready_race_lease = reference_race_service
        .claim_candidates(1)
        .await
        .expect("ready/reference candidate claim must succeed")
        .pop()
        .expect("ready/reference candidate must be claimed");
    let (ready_result, (_ready_race_node, _ready_race_version)) = tokio::join!(
        reference_race_service.mark_ready_for_deletion(ready_race_lease),
        insert_gc_file_version(
            &repository,
            &library,
            &root,
            ready_race,
            "ready-reference-race",
        ),
    );
    let ready_result = ready_result.expect("ready/reference race must not fail");
    assert!(matches!(
        ready_result,
        ObjectGcPlanResult::Valid(_) | ObjectGcPlanResult::Invalidated
    ));
    assert_eq!(count_gc_candidates(&inspection_pool, ready_race).await, 0);

    // No planner operation is permitted to delete physical metadata. The
    // Object and ObjectReplica row counts therefore remain exactly stable.
    assert_eq!(
        count_objects_in_dedup_domain(&inspection_pool, dedup_domain_id).await,
        initial_object_count
    );
    assert_eq!(
        count_replicas_in_dedup_domain(&inspection_pool, dedup_domain_id).await,
        initial_replica_count
    );
    assert!(
        repository
            .find_object(blocked.object_id())
            .await
            .expect("preserved object lookup must succeed")
            .is_some()
    );

    drop(metadata);
    drop(retention);
    drop(service);
    drop(reconnected_service);
    drop(claim_worker_a);
    drop(claim_worker_b);
    drop(reference_race_service);
    inspection_pool.close().await;
    reconnected.close().await;
    pool.close().await;
}

/// This test is intentionally ignored unless a caller supplies an explicitly
/// disposable PostgreSQL database. It covers the complete Prompt 32
/// checkpoint/feed contract against the live schema, including real
/// PostgreSQL row-lock races.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_device_sync_checkpoints_and_feed_are_bounded_monotonic_and_scoped() {
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
        .expect("sync inspection connection must succeed");
    assert!(
        pool.table_exists("device_sync_checkpoints")
            .await
            .expect("checkpoint table probe must succeed")
    );

    let observed_at = timestamp("2026-08-27T00:00:00.123456Z");
    let owner_id = UserId::new();
    let other_owner_id = UserId::new();
    let owner_login = format!("sync-owner-{owner_id}");
    let other_login = format!("sync-other-{other_owner_id}");
    let owner = User::new(
        owner_id,
        synveil_core::LoginIdentifier::new(&owner_login, owner_id.to_string())
            .expect("sync owner login identifier is valid"),
        UserStatus::Active,
        observed_at,
    );
    let other_owner = User::new(
        other_owner_id,
        synveil_core::LoginIdentifier::new(&other_login, other_owner_id.to_string())
            .expect("other sync owner login identifier is valid"),
        UserStatus::Active,
        observed_at,
    );
    let repository = DomainRepository::new(&pool);
    repository
        .insert_user(&owner)
        .await
        .expect("sync owner must persist");
    repository
        .insert_user(&other_owner)
        .await
        .expect("other sync owner must persist");

    let mut device_one = Device::new(
        DeviceId::new(),
        owner_id,
        name("Sync workstation"),
        observed_at,
    );
    device_one
        .transition_status(DeviceStatus::Active, observed_at)
        .expect("device one must become active");
    let mut device_two = Device::new(DeviceId::new(), owner_id, name("Sync laptop"), observed_at);
    device_two
        .transition_status(DeviceStatus::Active, observed_at)
        .expect("device two must become active");
    let mut device_three = Device::new(DeviceId::new(), owner_id, name("Sync tablet"), observed_at);
    device_three
        .transition_status(DeviceStatus::Active, observed_at)
        .expect("device three must become active");
    let mut foreign_device = Device::new(
        DeviceId::new(),
        other_owner_id,
        name("Foreign device"),
        observed_at,
    );
    foreign_device
        .transition_status(DeviceStatus::Active, observed_at)
        .expect("foreign device must become active");
    for device in [&device_one, &device_two, &device_three, &foreign_device] {
        repository
            .insert_device(device)
            .await
            .expect("sync device must persist");
    }

    let library_id = LibraryId::new();
    let root = Node::new_root(NodeId::new(), library_id, name("sync-root"), observed_at);
    let library = Library::new(
        library_id,
        owner_id,
        name("Sync library"),
        &root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("sync library must satisfy domain invariants");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("sync library must persist");

    let other_library_id = LibraryId::new();
    let other_root = Node::new_root(
        NodeId::new(),
        other_library_id,
        name("other-sync-root"),
        observed_at,
    );
    let other_library = Library::new(
        other_library_id,
        other_owner_id,
        name("Other sync library"),
        &other_root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("other sync library must satisfy domain invariants");
    repository
        .insert_library_with_root(&other_library, &other_root)
        .await
        .expect("other sync library must persist");

    let metadata = FileMetadataService::new(pool.clone());
    let sync = DeviceSyncService::new(pool.clone());

    let first_checkpoint = sync
        .ensure_checkpoint(owner_id, device_one.id(), library_id)
        .await
        .expect("first checkpoint must be created");
    assert_eq!(first_checkpoint.journal_epoch(), Sequence::new(1));
    assert_eq!(first_checkpoint.acknowledged_sequence(), Sequence::new(0));
    let repeated_checkpoint = sync
        .ensure_checkpoint(owner_id, device_one.id(), library_id)
        .await
        .expect("checkpoint read must be repeatable");
    assert_eq!(repeated_checkpoint, first_checkpoint);

    let sync_for_first_create = sync.clone();
    let sync_for_second_create = sync.clone();
    let (created_a, created_b) = tokio::join!(
        sync_for_first_create.ensure_checkpoint(owner_id, device_two.id(), library_id),
        sync_for_second_create.ensure_checkpoint(owner_id, device_two.id(), library_id),
    );
    let created_a = created_a.expect("first concurrent checkpoint create must succeed");
    let created_b = created_b.expect("second concurrent checkpoint create must succeed");
    assert_eq!(created_a, created_b);
    let checkpoint_count: i64 = sqlx::query_scalar(
        "SELECT count(*)
         FROM device_sync_checkpoints
         WHERE device_id = $1 AND library_id = $2",
    )
    .bind(device_two.id().into_uuid())
    .bind(library_id.into_uuid())
    .fetch_one(&inspection_pool)
    .await
    .expect("checkpoint uniqueness query must succeed");
    assert_eq!(checkpoint_count, 1);

    assert_eq!(
        sync.ensure_checkpoint(other_owner_id, device_one.id(), library_id)
            .await,
        Err(SyncError::NotFound)
    );
    assert_eq!(
        sync.ensure_checkpoint(owner_id, foreign_device.id(), library_id)
            .await,
        Err(SyncError::NotFound)
    );
    assert_eq!(
        sync.ensure_checkpoint(owner_id, device_one.id(), other_library_id)
            .await,
        Err(SyncError::NotFound)
    );

    let first_node = metadata
        .create_directory(owner_id, library_id, Some(root.id()), name("first"))
        .await
        .expect("first sync mutation must commit");
    let second_node = metadata
        .create_directory(owner_id, library_id, Some(root.id()), name("second"))
        .await
        .expect("second sync mutation must commit");
    let third_node = metadata
        .create_directory(owner_id, library_id, Some(root.id()), name("third"))
        .await
        .expect("third sync mutation must commit");

    let first_page = sync
        .fetch_feed(owner_id, device_one.id(), library_id, 2)
        .await
        .expect("first bounded sync page must load");
    assert_eq!(
        first_page
            .changes()
            .iter()
            .map(|event| event.sequence().get())
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(first_page.from_sequence(), Sequence::new(0));
    assert_eq!(first_page.through_sequence(), Sequence::new(2));
    assert_eq!(first_page.high_watermark().sequence(), Sequence::new(3));
    assert!(first_page.has_more());
    assert_eq!(
        first_page.checkpoint().acknowledged_sequence(),
        Sequence::new(0)
    );
    let repeated_page = sync
        .fetch_feed(owner_id, device_one.id(), library_id, 2)
        .await
        .expect("repeated unacknowledged page must load");
    assert_eq!(repeated_page, first_page);

    let device_two_page = sync
        .fetch_feed(owner_id, device_two.id(), library_id, 500)
        .await
        .expect("second device feed must load independently");
    assert_eq!(device_two_page.changes().len(), 3);
    assert_eq!(
        device_two_page
            .changes()
            .iter()
            .map(|event| event.sequence().get())
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    let device_two_checkpoint = sync
        .ensure_checkpoint(owner_id, device_two.id(), library_id)
        .await
        .expect("second device checkpoint must remain readable");
    assert_eq!(
        device_two_checkpoint.acknowledged_sequence(),
        Sequence::new(0)
    );

    let sync_for_reader_a = sync.clone();
    let sync_for_reader_b = sync.clone();
    let (reader_a, reader_b) = tokio::join!(
        sync_for_reader_a.fetch_feed(owner_id, device_two.id(), library_id, 2),
        sync_for_reader_b.fetch_feed(owner_id, device_two.id(), library_id, 2),
    );
    let reader_a = reader_a.expect("first concurrent feed reader must succeed");
    let reader_b = reader_b.expect("second concurrent feed reader must succeed");
    assert_eq!(reader_a, reader_b);
    assert_eq!(reader_a.high_watermark(), device_two_page.high_watermark());

    let first_evidence = SyncAckEvidence::new(
        owner_id,
        device_one.id(),
        library_id,
        first_page.checkpoint().journal_epoch(),
        first_page.from_sequence(),
        first_page.through_sequence(),
        first_page.high_watermark().sequence(),
    );
    let acknowledged_first = sync
        .acknowledge(owner_id, device_one.id(), library_id, first_evidence)
        .await
        .expect("first page acknowledgment must succeed");
    assert_eq!(acknowledged_first.acknowledged_sequence(), Sequence::new(2));
    assert_eq!(
        acknowledged_first.last_seen_high_watermark(),
        Some(Sequence::new(3))
    );
    let replayed_first = sync
        .acknowledge(owner_id, device_one.id(), library_id, first_evidence)
        .await
        .expect("same acknowledgment must be idempotent");
    assert_eq!(replayed_first, acknowledged_first);

    let second_page = sync
        .fetch_feed(owner_id, device_one.id(), library_id, 2)
        .await
        .expect("following sync page must load after acknowledgment");
    assert_eq!(second_page.changes().len(), 1);
    assert_eq!(second_page.changes()[0].resource_id(), third_node.id());
    assert_eq!(second_page.from_sequence(), Sequence::new(2));
    assert_eq!(second_page.through_sequence(), Sequence::new(3));
    assert!(!second_page.has_more());
    let second_evidence = SyncAckEvidence::new(
        owner_id,
        device_one.id(),
        library_id,
        second_page.checkpoint().journal_epoch(),
        second_page.from_sequence(),
        second_page.through_sequence(),
        second_page.high_watermark().sequence(),
    );
    let acknowledged_second = sync
        .acknowledge(owner_id, device_one.id(), library_id, second_evidence)
        .await
        .expect("second page acknowledgment must succeed");
    assert_eq!(
        acknowledged_second.acknowledged_sequence(),
        Sequence::new(3)
    );
    assert_eq!(
        acknowledged_second.last_seen_high_watermark(),
        Some(Sequence::new(3))
    );
    let empty_page = sync
        .fetch_feed(owner_id, device_one.id(), library_id, 100)
        .await
        .expect("fully acknowledged feed must return an empty page");
    assert!(empty_page.changes().is_empty());
    assert_eq!(empty_page.from_sequence(), Sequence::new(3));
    assert_eq!(empty_page.through_sequence(), Sequence::new(3));
    assert_eq!(empty_page.high_watermark().sequence(), Sequence::new(3));
    assert!(!empty_page.has_more());
    let older_replay = sync
        .acknowledge(owner_id, device_one.id(), library_id, first_evidence)
        .await
        .expect("older acknowledgment replay must converge");
    assert_eq!(older_replay.acknowledged_sequence(), Sequence::new(3));
    assert_eq!(
        sync.acknowledge(
            owner_id,
            device_one.id(),
            library_id,
            SyncAckEvidence::new(
                owner_id,
                device_one.id(),
                library_id,
                Sequence::new(1),
                Sequence::new(3),
                Sequence::new(2),
                Sequence::new(3),
            ),
        )
        .await,
        Err(SyncError::InvalidAckToken)
    );

    let out_of_order = SyncAckEvidence::new(
        owner_id,
        device_two.id(),
        library_id,
        Sequence::new(1),
        Sequence::new(1),
        Sequence::new(2),
        Sequence::new(3),
    );
    assert_eq!(
        sync.acknowledge(owner_id, device_two.id(), library_id, out_of_order)
            .await,
        Err(SyncError::CheckpointConflict)
    );
    let beyond_head = SyncAckEvidence::new(
        owner_id,
        device_two.id(),
        library_id,
        Sequence::new(1),
        Sequence::new(0),
        Sequence::new(4),
        Sequence::new(4),
    );
    assert_eq!(
        sync.acknowledge(owner_id, device_two.id(), library_id, beyond_head)
            .await,
        Err(SyncError::InvalidAckToken)
    );
    let wrong_epoch = SyncAckEvidence::new(
        owner_id,
        device_two.id(),
        library_id,
        Sequence::new(2),
        Sequence::new(0),
        Sequence::new(1),
        Sequence::new(3),
    );
    assert_eq!(
        sync.acknowledge(owner_id, device_two.id(), library_id, wrong_epoch)
            .await,
        Err(SyncError::RebaselineRequired {
            reason: RebaselineReason::EpochMismatch,
            current_epoch: Sequence::new(1),
            minimum_retained_sequence: Sequence::new(0),
        })
    );

    let library_two_node = metadata
        .create_directory(
            owner_id,
            other_library_id,
            Some(other_root.id()),
            name("other-library-change"),
        )
        .await;
    assert_eq!(library_two_node, Err(FileMetadataError::NotFound));

    let mut owner_library_two_device = Device::new(
        DeviceId::new(),
        owner_id,
        name("Second library device"),
        observed_at,
    );
    owner_library_two_device
        .transition_status(DeviceStatus::Active, observed_at)
        .expect("second-library device must become active");
    repository
        .insert_device(&owner_library_two_device)
        .await
        .expect("second-library device must persist");
    let library_two_id = LibraryId::new();
    let library_two_root = Node::new_root(
        NodeId::new(),
        library_two_id,
        name("second-library-root"),
        observed_at,
    );
    let library_two = Library::new(
        library_two_id,
        owner_id,
        name("Second library"),
        &library_two_root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("second owner library must satisfy domain invariants");
    repository
        .insert_library_with_root(&library_two, &library_two_root)
        .await
        .expect("second owner library must persist");
    metadata
        .create_directory(
            owner_id,
            library_two_id,
            Some(library_two_root.id()),
            name("independent-change"),
        )
        .await
        .expect("second library mutation must commit");
    let library_two_page = sync
        .fetch_feed(owner_id, owner_library_two_device.id(), library_two_id, 100)
        .await
        .expect("second library feed must load independently");
    assert_eq!(library_two_page.changes().len(), 1);
    assert_eq!(library_two_page.from_sequence(), Sequence::new(0));
    assert_eq!(
        library_two_page.high_watermark().sequence(),
        Sequence::new(1)
    );

    let concurrent_ack_page = sync
        .fetch_feed(owner_id, device_three.id(), library_id, 2)
        .await
        .expect("third device feed must load");
    let concurrent_ack_evidence = SyncAckEvidence::new(
        owner_id,
        device_three.id(),
        library_id,
        concurrent_ack_page.checkpoint().journal_epoch(),
        concurrent_ack_page.from_sequence(),
        concurrent_ack_page.through_sequence(),
        concurrent_ack_page.high_watermark().sequence(),
    );
    let sync_for_ack_a = sync.clone();
    let sync_for_ack_b = sync.clone();
    let (ack_a, ack_b) = tokio::join!(
        sync_for_ack_a.acknowledge(
            owner_id,
            device_three.id(),
            library_id,
            concurrent_ack_evidence
        ),
        sync_for_ack_b.acknowledge(
            owner_id,
            device_three.id(),
            library_id,
            concurrent_ack_evidence
        ),
    );
    let ack_a = ack_a.expect("first concurrent acknowledgment must succeed");
    let ack_b = ack_b.expect("second concurrent acknowledgment must succeed");
    assert_eq!(ack_a.acknowledged_sequence(), Sequence::new(2));
    assert_eq!(ack_b.acknowledged_sequence(), Sequence::new(2));

    let sync_for_reader = sync.clone();
    let metadata_for_writer = metadata.clone();
    let (reader_result, writer_result) = tokio::join!(
        sync_for_reader.fetch_feed(owner_id, device_three.id(), library_id, 500),
        metadata_for_writer.create_directory(
            owner_id,
            library_id,
            Some(root.id()),
            name("written-during-feed"),
        ),
    );
    let reader_result = reader_result.expect("feed reader racing a writer must succeed");
    let writer_node = writer_result.expect("journal writer racing a feed must commit");
    assert!(
        reader_result
            .changes()
            .iter()
            .all(|event| event.sequence().get() <= reader_result.high_watermark().sequence().get())
    );
    let follow_up = sync
        .fetch_feed(owner_id, device_three.id(), library_id, 500)
        .await
        .expect("follow-up feed after racing writer must succeed");
    let observed_sequences = reader_result
        .changes()
        .iter()
        .chain(follow_up.changes().iter())
        .map(|event| event.sequence().get())
        .collect::<Vec<_>>();
    assert!(
        observed_sequences.contains(&4),
        "the committed racing writer event must remain feed-visible"
    );
    assert!(reader_result.high_watermark().sequence().get() >= 3);
    assert_eq!(
        writer_node.library_id(),
        library_id,
        "writer must remain in the requested logical library"
    );

    sqlx::query(
        "UPDATE libraries
         SET minimum_retained_sequence = 2
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(library_id.into_uuid())
    .bind(owner_id.into_uuid())
    .execute(&inspection_pool)
    .await
    .expect("retained boundary update must succeed");
    assert_eq!(
        sync.fetch_feed(owner_id, device_two.id(), library_id, 100)
            .await,
        Err(SyncError::RebaselineRequired {
            reason: RebaselineReason::HistoryUnavailable,
            current_epoch: Sequence::new(1),
            minimum_retained_sequence: Sequence::new(2),
        })
    );

    sqlx::query(
        "UPDATE libraries
         SET journal_epoch = 2
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(library_id.into_uuid())
    .bind(owner_id.into_uuid())
    .execute(&inspection_pool)
    .await
    .expect("journal epoch update must succeed");
    assert_eq!(
        sync.fetch_feed(owner_id, device_one.id(), library_id, 100)
            .await,
        Err(SyncError::RebaselineRequired {
            reason: RebaselineReason::EpochMismatch,
            current_epoch: Sequence::new(2),
            minimum_retained_sequence: Sequence::new(2),
        })
    );
    assert_eq!(
        sync.acknowledge(owner_id, device_one.id(), library_id, second_evidence)
            .await,
        Err(SyncError::RebaselineRequired {
            reason: RebaselineReason::EpochMismatch,
            current_epoch: Sequence::new(2),
            minimum_retained_sequence: Sequence::new(2),
        })
    );

    sqlx::query(
        "UPDATE devices
         SET status = 'REVOKED'
         WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(device_one.id().into_uuid())
    .bind(owner_id.into_uuid())
    .execute(&inspection_pool)
    .await
    .expect("device revocation update must succeed");
    assert_eq!(
        sync.fetch_feed(owner_id, device_one.id(), library_id, 100)
            .await,
        Err(SyncError::NotFound)
    );

    drop(metadata);
    drop(sync);
    inspection_pool.close().await;
    pool.close().await;

    // Keep the named nodes live through the assertions above so the compiler
    // does not permit an accidental fixture simplification that removes the
    // logical event subjects from the test's scope.
    let _ = (first_node, second_node);
}

/// Prompt 33's materialized bootstrap proof. The test is deliberately one
/// live PostgreSQL scenario so journal mutations, manifest capture, checkpoint
/// replacement, restart/retry, and real lock ordering are exercised together.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_sync_rebaseline_bootstrap_is_coherent_bounded_and_fenced() {
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
        .expect("Prompt 33 migration execution must succeed");
    let inspection_pool = PgPool::connect(&url)
        .await
        .expect("bootstrap inspection connection must succeed");
    for table in ["sync_bootstraps", "sync_bootstrap_nodes"] {
        assert!(
            pool.table_exists(table)
                .await
                .expect("bootstrap table probe must succeed"),
            "expected Prompt 33 table {table}"
        );
    }

    let observed_at = timestamp("2026-08-27T00:00:00.123456Z");
    let owner_id = UserId::new();
    let other_owner_id = UserId::new();
    let owner_login = format!("rebaseline-owner-{owner_id}");
    let other_login = format!("rebaseline-other-{other_owner_id}");
    let owner = User::new(
        owner_id,
        synveil_core::LoginIdentifier::new(&owner_login, owner_id.to_string())
            .expect("bootstrap owner login must be valid"),
        UserStatus::Active,
        observed_at,
    );
    let other_owner = User::new(
        other_owner_id,
        synveil_core::LoginIdentifier::new(&other_login, other_owner_id.to_string())
            .expect("other bootstrap owner login must be valid"),
        UserStatus::Active,
        observed_at,
    );
    let repository = DomainRepository::new(&pool);
    repository
        .insert_user(&owner)
        .await
        .expect("bootstrap owner must persist");
    repository
        .insert_user(&other_owner)
        .await
        .expect("other bootstrap owner must persist");

    let mut devices = Vec::new();
    for index in 0..14 {
        devices.push(
            insert_active_sync_device(
                &repository,
                owner_id,
                &format!("Rebaseline device {index}"),
                observed_at,
            )
            .await,
        );
    }
    let mut revoked_device = Device::new(
        DeviceId::new(),
        owner_id,
        name("Revoked rebaseline device"),
        observed_at,
    );
    revoked_device
        .transition_status(DeviceStatus::Active, observed_at)
        .expect("revoked fixture device must first become active");
    revoked_device
        .transition_status(DeviceStatus::Revoked, observed_at)
        .expect("fixture device must become revoked");
    repository
        .insert_device(&revoked_device)
        .await
        .expect("revoked fixture device must persist");

    let library_id = LibraryId::new();
    let root = Node::new_root(
        NodeId::new(),
        library_id,
        name("rebaseline-root"),
        observed_at,
    );
    let library = Library::new(
        library_id,
        owner_id,
        name("Rebaseline library"),
        &root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("bootstrap library must satisfy domain invariants");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("bootstrap library must persist");

    let empty_library_id = LibraryId::new();
    let empty_root = Node::new_root(
        NodeId::new(),
        empty_library_id,
        name("empty-rebaseline-root"),
        observed_at,
    );
    let empty_library = Library::new(
        empty_library_id,
        owner_id,
        name("Empty rebaseline library"),
        &empty_root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("empty bootstrap library must satisfy domain invariants");
    repository
        .insert_library_with_root(&empty_library, &empty_root)
        .await
        .expect("empty bootstrap library must persist");

    let metadata = FileMetadataService::new(pool.clone());
    let journal = ChangeJournalService::new(pool.clone());
    let sync = DeviceSyncService::new(pool.clone());
    let bootstraps = SyncBootstrapService::new(pool.clone());

    assert_eq!(
        bootstraps
            .start(other_owner_id, devices[0].id(), library_id)
            .await,
        Err(RebaselineError::NotFound)
    );
    assert_eq!(
        bootstraps
            .start(owner_id, revoked_device.id(), library_id)
            .await,
        Err(RebaselineError::NotFound)
    );
    assert_eq!(
        bootstraps
            .page_nodes(
                owner_id,
                devices[0].id(),
                library_id,
                synveil_core::SyncBootstrapId::new(),
                None,
                0,
            )
            .await,
        Err(RebaselineError::InvalidLimit)
    );

    // An empty user library still includes its one canonical active root and
    // can issue terminal proof and complete at journal sequence zero.
    let empty_bootstrap = bootstraps
        .start(owner_id, devices[0].id(), empty_library_id)
        .await
        .expect("root-only bootstrap must start");
    let (empty_nodes, empty_terminal) = drain_bootstrap(
        &bootstraps,
        owner_id,
        devices[0].id(),
        empty_library_id,
        empty_bootstrap,
        1,
    )
    .await;
    assert_eq!(empty_nodes.len(), 1);
    assert_eq!(empty_nodes[0].node_id(), empty_root.id());
    assert_eq!(empty_nodes[0].parent_node_id(), None);
    assert_eq!(empty_nodes[0].kind(), NodeKind::Directory);
    assert_eq!(empty_nodes[0].state(), NodeState::Active);
    let empty_completion = bootstraps
        .complete(
            owner_id,
            devices[0].id(),
            empty_library_id,
            empty_bootstrap.id(),
            synveil_metadata::BootstrapCompletionEvidence::new(empty_terminal),
        )
        .await
        .expect("root-only bootstrap must complete");
    assert_eq!(
        empty_completion.checkpoint().acknowledged_sequence(),
        Sequence::new(0)
    );
    assert!(
        sync.fetch_feed(owner_id, devices[0].id(), empty_library_id, 10)
            .await
            .expect("post-empty-bootstrap feed must load")
            .changes()
            .is_empty()
    );

    let rename_node = metadata
        .create_directory(owner_id, library_id, Some(root.id()), name("rename-before"))
        .await
        .expect("rename race node must persist");
    let move_node = metadata
        .create_directory(owner_id, library_id, Some(root.id()), name("move-before"))
        .await
        .expect("move race node must persist");
    let move_destination = metadata
        .create_directory(
            owner_id,
            library_id,
            Some(root.id()),
            name("move-destination"),
        )
        .await
        .expect("move destination must persist");
    let trash_node = metadata
        .create_directory(owner_id, library_id, Some(root.id()), name("trash-before"))
        .await
        .expect("Trash race node must persist");
    let purge_node = metadata
        .create_directory(owner_id, library_id, Some(root.id()), name("purge-before"))
        .await
        .expect("purge race node must persist");
    let purged_trash = metadata
        .delete_node(owner_id, purge_node.id(), purge_node.revision())
        .await
        .expect("purge fixture must enter Trash");
    sqlx::query(
        "UPDATE nodes
         SET trashed_at = CURRENT_TIMESTAMP - INTERVAL '31 days'
         WHERE id = $1 AND library_id = $2",
    )
    .bind(purge_node.id().into_uuid())
    .bind(library_id.into_uuid())
    .execute(&inspection_pool)
    .await
    .expect("purge fixture age must persist");
    let purge_service = TrashRetentionService::new(pool.clone(), TrashRetentionPolicy::default());
    let purging_node = purge_service
        .begin_node_purge(owner_id, purge_node.id(), purged_trash.revision())
        .await
        .expect("purge fixture must enter internal PURGING");
    assert_eq!(purging_node.state(), NodeState::Purging);

    let mut file_node = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::File,
        name("content-race.txt"),
        observed_at,
    )
    .expect("file fixture must satisfy domain invariants");
    repository
        .insert_node(&file_node)
        .await
        .expect("file fixture node must persist");
    let historical_object = insert_verified_object(
        &repository,
        &inspection_pool,
        library.dedup_domain_id(),
        0x31,
        31,
        observed_at,
    )
    .await;
    let historical_version = FileVersion::new(
        FileVersionId::new(),
        &library,
        &file_node,
        historical_object,
        None,
        observed_at,
    )
    .expect("historical fixture version must be valid");
    repository
        .insert_file_version(historical_version)
        .await
        .expect("historical fixture version must persist");
    let current_object = insert_verified_object(
        &repository,
        &inspection_pool,
        library.dedup_domain_id(),
        0x32,
        32,
        observed_at,
    )
    .await;
    let current_version = FileVersion::new(
        FileVersionId::new(),
        &library,
        &file_node,
        current_object,
        Some(historical_version.id()),
        observed_at,
    )
    .expect("current fixture version must be valid");
    repository
        .insert_file_version(current_version)
        .await
        .expect("current fixture version must persist");
    file_node = file_node
        .with_current_version(&current_version, observed_at)
        .expect("file fixture head must update");
    repository
        .update_node(&file_node)
        .await
        .expect("file fixture head must persist");

    // Directory create racing capture: either the captured manifest contains
    // the resulting row and its event is at/before N, or the event is > N.
    let (create_bootstrap, created_node) = tokio::join!(
        bootstraps.start(owner_id, devices[1].id(), library_id),
        metadata.create_directory(owner_id, library_id, Some(root.id()), name("racing-create")),
    );
    let create_bootstrap = create_bootstrap.expect("create-race bootstrap must start");
    let created_node = created_node.expect("racing directory create must commit");
    let (create_snapshot, _) = drain_bootstrap(
        &bootstraps,
        owner_id,
        devices[1].id(),
        library_id,
        create_bootstrap,
        1_000,
    )
    .await;
    let create_event = list_journal_events(&journal, owner_id, library_id)
        .await
        .into_iter()
        .find(|event| {
            event.resource_id() == created_node.id()
                && event.change_kind() == ChangeKind::NodeCreated
        })
        .expect("racing create event must exist");
    let create_in_snapshot = create_snapshot
        .iter()
        .any(|node| node.node_id() == created_node.id());
    assert_eq!(
        create_in_snapshot,
        create_event.sequence() <= create_bootstrap.snapshot_resume_sequence()
    );

    let (rename_bootstrap, renamed_node) = tokio::join!(
        bootstraps.start(owner_id, devices[2].id(), library_id),
        metadata.rename_node(
            owner_id,
            rename_node.id(),
            name("rename-after"),
            rename_node.revision(),
        ),
    );
    let rename_bootstrap = rename_bootstrap.expect("rename-race bootstrap must start");
    let renamed_node = renamed_node.expect("racing rename must commit");
    let (rename_snapshot, _) = drain_bootstrap(
        &bootstraps,
        owner_id,
        devices[2].id(),
        library_id,
        rename_bootstrap,
        1_000,
    )
    .await;
    let rename_projection = rename_snapshot
        .iter()
        .find(|node| node.node_id() == rename_node.id())
        .expect("renamed node must remain in the logical snapshot");
    let rename_event = list_journal_events(&journal, owner_id, library_id)
        .await
        .into_iter()
        .find(|event| {
            event.resource_id() == renamed_node.id()
                && event.change_kind() == ChangeKind::NodeRenamed
        })
        .expect("racing rename event must exist");
    assert_eq!(
        rename_projection.name().as_str() == "rename-after",
        rename_event.sequence() <= rename_bootstrap.snapshot_resume_sequence()
    );

    let (move_bootstrap, moved_node) = tokio::join!(
        bootstraps.start(owner_id, devices[3].id(), library_id),
        metadata.move_node(
            owner_id,
            move_node.id(),
            move_destination.id(),
            move_node.revision(),
        ),
    );
    let move_bootstrap = move_bootstrap.expect("move-race bootstrap must start");
    let moved_node = moved_node.expect("racing move must commit");
    let (move_snapshot, _) = drain_bootstrap(
        &bootstraps,
        owner_id,
        devices[3].id(),
        library_id,
        move_bootstrap,
        1_000,
    )
    .await;
    let move_projection = move_snapshot
        .iter()
        .find(|node| node.node_id() == move_node.id())
        .expect("moved node must remain in the logical snapshot");
    let move_event = list_journal_events(&journal, owner_id, library_id)
        .await
        .into_iter()
        .find(|event| {
            event.resource_id() == moved_node.id() && event.change_kind() == ChangeKind::NodeMoved
        })
        .expect("racing move event must exist");
    assert_eq!(
        move_projection.parent_node_id() == Some(move_destination.id()),
        move_event.sequence() <= move_bootstrap.snapshot_resume_sequence()
    );

    let (trash_bootstrap, trashed_node) = tokio::join!(
        bootstraps.start(owner_id, devices[4].id(), library_id),
        metadata.delete_node(owner_id, trash_node.id(), trash_node.revision()),
    );
    let trash_bootstrap = trash_bootstrap.expect("Trash-race bootstrap must start");
    let trashed_node = trashed_node.expect("racing Trash mutation must commit");
    let (trash_snapshot, _) = drain_bootstrap(
        &bootstraps,
        owner_id,
        devices[4].id(),
        library_id,
        trash_bootstrap,
        1_000,
    )
    .await;
    let trash_projection = trash_snapshot
        .iter()
        .find(|node| node.node_id() == trash_node.id())
        .expect("TRASHED nodes must be included in a fresh snapshot");
    let trash_event = list_journal_events(&journal, owner_id, library_id)
        .await
        .into_iter()
        .find(|event| {
            event.resource_id() == trashed_node.id()
                && event.change_kind() == ChangeKind::NodeTrashed
        })
        .expect("racing Trash event must exist");
    assert_eq!(
        trash_projection.state() == NodeState::Trashed,
        trash_event.sequence() <= trash_bootstrap.snapshot_resume_sequence()
    );

    // PURGING is internal-only and therefore absent whether permanent purge
    // commits before or after the cut. If it commits after, its tombstone is
    // strictly post-cut.
    let (purge_bootstrap, purge_result) = tokio::join!(
        bootstraps.start(owner_id, devices[5].id(), library_id),
        purge_service.execute_metadata_purge(owner_id, purging_node.id(), purging_node.revision(),),
    );
    let purge_bootstrap = purge_bootstrap.expect("purge-race bootstrap must start");
    assert_eq!(
        purge_result.expect("racing purge must finish"),
        PurgeExecutionResult::Completed
    );
    let (purge_snapshot, _) = drain_bootstrap(
        &bootstraps,
        owner_id,
        devices[5].id(),
        library_id,
        purge_bootstrap,
        1_000,
    )
    .await;
    let purged_outcome_is_captured = purge_snapshot
        .iter()
        .all(|node| node.node_id() != purge_node.id());
    assert!(
        purged_outcome_is_captured,
        "internal PURGING and permanently purged rows must not be exposed"
    );
    let purge_event = list_journal_events(&journal, owner_id, library_id)
        .await
        .into_iter()
        .find(|event| {
            event.resource_id() == purge_node.id() && event.change_kind() == ChangeKind::NodePurged
        })
        .expect("racing purge tombstone must exist");
    assert!(
        (purged_outcome_is_captured
            && purge_event.sequence() <= purge_bootstrap.snapshot_resume_sequence())
            || purge_event.sequence() > purge_bootstrap.snapshot_resume_sequence(),
        "the purge outcome must be captured at the cut or its tombstone must be post-cut"
    );

    // Prepare verified replacement bytes before racing only the short logical
    // finalization transaction with bootstrap capture.
    let upload_repository = PostgresUploadRepository::new(pool.clone());
    let replacement_session_id = UploadSessionId::new();
    let replacement_object_id = ObjectId::new();
    let replacement_replica_id = ObjectReplicaId::new();
    let replacement_hash = Sha256Digest::from_bytes([0x41; 32]);
    let replacement_observed = timestamp("2026-08-27T00:10:00.123456Z");
    upload_repository
        .create_upload_session(NewUploadSession {
            id: replacement_session_id,
            owner_user_id: owner_id,
            library_id,
            operation: UploadOperation::ReplaceContent,
            target_node_id: file_node.id(),
            target_parent_node_id: None,
            target_name: None,
            expected_node_revision: Some(file_node.revision()),
            expected_length: 41,
            expected_sha256: Some(replacement_hash),
            object_id: replacement_object_id,
            object_replica_id: replacement_replica_id,
            object_key: format!("objects/v1/{replacement_object_id}"),
            staging_handle: format!("prompt-33-{replacement_session_id}"),
            max_active_sessions: 8,
            created_at: replacement_observed,
            expires_at: timestamp("2026-08-27T01:10:00.123456Z"),
        })
        .await
        .expect("replacement upload session must persist");
    upload_repository
        .record_upload_progress(
            owner_id,
            replacement_session_id,
            0,
            41,
            replacement_observed,
        )
        .await
        .expect("replacement upload progress must persist");
    let replacement_generation = match upload_repository
        .claim_upload(
            owner_id,
            replacement_session_id,
            replacement_observed,
            timestamp("2026-08-27T00:20:00.123456Z"),
        )
        .await
        .expect("replacement upload claim must persist")
    {
        UploadClaim::Acquired(record) => record.lease_generation,
        other => panic!("replacement upload claim was not acquired: {other:?}"),
    };
    upload_repository
        .record_upload_durable(
            owner_id,
            replacement_session_id,
            replacement_generation,
            UploadDurabilityReceipt {
                backend_kind: "LOCAL_FILESYSTEM".to_owned(),
                storage_key: format!("objects/v1/{replacement_object_id}"),
                backend_version: Some("v1".to_owned()),
                length: 41,
                sha256: replacement_hash,
                verified_at: replacement_observed,
            },
            replacement_observed,
        )
        .await
        .expect("replacement durability receipt must persist");
    let (content_bootstrap, content_result) = tokio::join!(
        bootstraps.start(owner_id, devices[6].id(), library_id),
        upload_repository.finalize_upload(
            owner_id,
            replacement_session_id,
            replacement_generation,
            replacement_observed,
        ),
    );
    let content_bootstrap = content_bootstrap.expect("content-race bootstrap must start");
    let content_completion = match content_result.expect("content replacement must finish") {
        UploadFinalization::Completed(completion) => completion,
        other => panic!("content replacement was not completed: {other:?}"),
    };
    let (content_snapshot, _) = drain_bootstrap(
        &bootstraps,
        owner_id,
        devices[6].id(),
        library_id,
        content_bootstrap,
        1_000,
    )
    .await;
    let content_projection = content_snapshot
        .iter()
        .find(|node| node.node_id() == file_node.id())
        .expect("current file must be projected");
    let content_event = list_journal_events(&journal, owner_id, library_id)
        .await
        .into_iter()
        .find(|event| {
            event.resource_id() == file_node.id()
                && event.change_kind() == ChangeKind::FileContentCommitted
                && event.current_version_id() == Some(content_completion.file_version_id)
        })
        .expect("racing content event must exist");
    assert_eq!(
        content_projection.current_version_id() == Some(content_completion.file_version_id),
        content_event.sequence() <= content_bootstrap.snapshot_resume_sequence()
    );
    if content_projection.current_version_id() == Some(content_completion.file_version_id) {
        assert_eq!(content_projection.content_length(), Some(41));
        assert_eq!(content_projection.content_sha256(), Some(replacement_hash));
    } else {
        assert_eq!(content_projection.content_length(), Some(32));
        assert_eq!(
            content_projection.content_sha256(),
            Some(current_object.canonical_hash())
        );
    }

    let current_after_upload = metadata
        .get_node(owner_id, file_node.id())
        .await
        .expect("file after replacement must load");
    let restore_service = VersionRestoreService::new(pool.clone());
    let (restore_bootstrap, restore_result) = tokio::join!(
        bootstraps.start(owner_id, devices[7].id(), library_id),
        restore_service.restore_file_version(
            owner_id,
            file_node.id(),
            historical_version.id(),
            current_after_upload.revision(),
            format!("prompt-33-restore-{}", file_node.id()),
        ),
    );
    let restore_bootstrap = restore_bootstrap.expect("restore-race bootstrap must start");
    let restored = restore_result.expect("racing version restore must commit");
    let restored_version_id = restored.version().id();
    let (restore_snapshot, _) = drain_bootstrap(
        &bootstraps,
        owner_id,
        devices[7].id(),
        library_id,
        restore_bootstrap,
        1_000,
    )
    .await;
    let restore_projection = restore_snapshot
        .iter()
        .find(|node| node.node_id() == file_node.id())
        .expect("restored file must remain projected");
    let restore_event = list_journal_events(&journal, owner_id, library_id)
        .await
        .into_iter()
        .find(|event| {
            event.resource_id() == file_node.id()
                && event.change_kind() == ChangeKind::FileVersionRestored
                && event.current_version_id() == Some(restored_version_id)
        })
        .expect("racing version-restore event must exist");
    assert_eq!(
        restore_projection.current_version_id() == Some(restored_version_id),
        restore_event.sequence() <= restore_bootstrap.snapshot_resume_sequence()
    );

    // Build a deterministic multi-page cut, then perform every supported
    // mixed-time hazard after page 1. All remaining pages must retain the
    // pre-mutation projection, while the feed contains the post-cut results.
    let mp_rename = metadata
        .create_directory(
            owner_id,
            library_id,
            Some(root.id()),
            name("mp-rename-before"),
        )
        .await
        .expect("multi-page rename node must persist");
    let mp_move = metadata
        .create_directory(
            owner_id,
            library_id,
            Some(root.id()),
            name("mp-move-before"),
        )
        .await
        .expect("multi-page move node must persist");
    let mp_destination = metadata
        .create_directory(
            owner_id,
            library_id,
            Some(root.id()),
            name("mp-destination"),
        )
        .await
        .expect("multi-page destination must persist");
    let mp_trash = metadata
        .create_directory(
            owner_id,
            library_id,
            Some(root.id()),
            name("mp-trash-before"),
        )
        .await
        .expect("multi-page Trash node must persist");
    let file_before_multipage = metadata
        .get_node(owner_id, file_node.id())
        .await
        .expect("multi-page file state must load");
    let file_version_before_multipage = file_before_multipage
        .current_version_id()
        .expect("multi-page file must have a current version");

    let mp_upload_id = UploadSessionId::new();
    let mp_object_id = ObjectId::new();
    let mp_replica_id = ObjectReplicaId::new();
    let mp_hash = Sha256Digest::from_bytes([0x51; 32]);
    upload_repository
        .create_upload_session(NewUploadSession {
            id: mp_upload_id,
            owner_user_id: owner_id,
            library_id,
            operation: UploadOperation::ReplaceContent,
            target_node_id: file_node.id(),
            target_parent_node_id: None,
            target_name: None,
            expected_node_revision: Some(file_before_multipage.revision()),
            expected_length: 51,
            expected_sha256: Some(mp_hash),
            object_id: mp_object_id,
            object_replica_id: mp_replica_id,
            object_key: format!("objects/v1/{mp_object_id}"),
            staging_handle: format!("prompt-33-mp-{mp_upload_id}"),
            max_active_sessions: 8,
            created_at: timestamp("2026-08-27T00:30:00.123456Z"),
            expires_at: timestamp("2026-08-27T01:30:00.123456Z"),
        })
        .await
        .expect("multi-page upload session must persist");
    upload_repository
        .record_upload_progress(
            owner_id,
            mp_upload_id,
            0,
            51,
            timestamp("2026-08-27T00:30:01.123456Z"),
        )
        .await
        .expect("multi-page upload progress must persist");
    let mp_upload_generation = match upload_repository
        .claim_upload(
            owner_id,
            mp_upload_id,
            timestamp("2026-08-27T00:30:02.123456Z"),
            timestamp("2026-08-27T00:40:00.123456Z"),
        )
        .await
        .expect("multi-page upload claim must persist")
    {
        UploadClaim::Acquired(record) => record.lease_generation,
        other => panic!("multi-page upload claim was not acquired: {other:?}"),
    };
    upload_repository
        .record_upload_durable(
            owner_id,
            mp_upload_id,
            mp_upload_generation,
            UploadDurabilityReceipt {
                backend_kind: "LOCAL_FILESYSTEM".to_owned(),
                storage_key: format!("objects/v1/{mp_object_id}"),
                backend_version: Some("v1".to_owned()),
                length: 51,
                sha256: mp_hash,
                verified_at: timestamp("2026-08-27T00:30:03.123456Z"),
            },
            timestamp("2026-08-27T00:30:03.123456Z"),
        )
        .await
        .expect("multi-page upload durability must persist");

    let multipage_bootstrap = bootstraps
        .start(owner_id, devices[8].id(), library_id)
        .await
        .expect("multi-page bootstrap must start");
    let repeated_start = bootstraps
        .start(owner_id, devices[8].id(), library_id)
        .await
        .expect("start retry must return the active bootstrap");
    assert_eq!(repeated_start, multipage_bootstrap);
    let checkpoint_before_pages = sync
        .ensure_checkpoint(owner_id, devices[8].id(), library_id)
        .await
        .expect("multi-page checkpoint must load");
    let first_page = bootstraps
        .page_nodes(
            owner_id,
            devices[8].id(),
            library_id,
            multipage_bootstrap.id(),
            None,
            2,
        )
        .await
        .expect("first multi-page page must load");
    assert!(first_page.has_more());
    let first_page_before_mutation = first_page.clone();
    let first_position = first_page
        .next_position()
        .expect("first multi-page page must carry a cursor position");

    let (mp_rename_result, mp_move_result, mp_trash_result, mp_create_result, mp_upload_result) = tokio::join!(
        metadata.rename_node(
            owner_id,
            mp_rename.id(),
            name("mp-rename-after"),
            mp_rename.revision(),
        ),
        metadata.move_node(
            owner_id,
            mp_move.id(),
            mp_destination.id(),
            mp_move.revision(),
        ),
        metadata.delete_node(owner_id, mp_trash.id(), mp_trash.revision()),
        metadata.create_directory(
            owner_id,
            library_id,
            Some(root.id()),
            name("mp-created-after-cut"),
        ),
        upload_repository.finalize_upload(
            owner_id,
            mp_upload_id,
            mp_upload_generation,
            timestamp("2026-08-27T00:30:04.123456Z"),
        ),
    );
    mp_rename_result.expect("multi-page rename must commit");
    mp_move_result.expect("multi-page move must commit");
    mp_trash_result.expect("multi-page Trash must commit");
    let mp_created = mp_create_result.expect("multi-page create must commit");
    let mp_content_completion = match mp_upload_result.expect("multi-page upload must finish") {
        UploadFinalization::Completed(completion) => completion,
        other => panic!("multi-page upload was not completed: {other:?}"),
    };

    let replayed_first_page = bootstraps
        .page_nodes(
            owner_id,
            devices[8].id(),
            library_id,
            multipage_bootstrap.id(),
            None,
            2,
        )
        .await
        .expect("first page retry must load");
    assert_eq!(replayed_first_page, first_page_before_mutation);

    let restarted_bootstraps = SyncBootstrapService::new(pool.clone());
    let mut complete_manifest = first_page.nodes().to_vec();
    let mut position = Some(first_position);
    let (terminal_page, terminal_input_position, terminal_evidence) = loop {
        let input_position = position;
        let page = restarted_bootstraps
            .page_nodes(
                owner_id,
                devices[8].id(),
                library_id,
                multipage_bootstrap.id(),
                input_position,
                2,
            )
            .await
            .expect("continued page after service restart must load");
        complete_manifest.extend_from_slice(page.nodes());
        if let Some(evidence) = page.terminal_evidence() {
            break (page, input_position, evidence);
        }
        position = page.next_position();
    };
    let repeated_terminal = restarted_bootstraps
        .page_nodes(
            owner_id,
            devices[8].id(),
            library_id,
            multipage_bootstrap.id(),
            terminal_input_position,
            2,
        )
        .await
        .expect("terminal page retry must load");
    assert_eq!(repeated_terminal, terminal_page);

    let mp_rename_projection = complete_manifest
        .iter()
        .find(|node| node.node_id() == mp_rename.id())
        .expect("multi-page rename projection must exist");
    assert_eq!(mp_rename_projection.name().as_str(), "mp-rename-before");
    let mp_move_projection = complete_manifest
        .iter()
        .find(|node| node.node_id() == mp_move.id())
        .expect("multi-page move projection must exist");
    assert_eq!(mp_move_projection.parent_node_id(), Some(root.id()));
    let mp_trash_projection = complete_manifest
        .iter()
        .find(|node| node.node_id() == mp_trash.id())
        .expect("multi-page Trash projection must exist");
    assert_eq!(mp_trash_projection.state(), NodeState::Active);
    assert!(
        complete_manifest
            .iter()
            .all(|node| node.node_id() != mp_created.id()),
        "post-cut create must not appear in the immutable manifest"
    );
    let mp_file_projection = complete_manifest
        .iter()
        .find(|node| node.node_id() == file_node.id())
        .expect("multi-page file projection must exist");
    assert_eq!(
        mp_file_projection.current_version_id(),
        Some(file_version_before_multipage)
    );
    assert_ne!(
        mp_file_projection.current_version_id(),
        Some(mp_content_completion.file_version_id)
    );
    assert_eq!(
        complete_manifest.len() as u64,
        multipage_bootstrap.manifest_item_count()
    );
    let mut ordered_ids = complete_manifest
        .iter()
        .map(LogicalSnapshotNode::node_id)
        .collect::<Vec<_>>();
    let original_ids = ordered_ids.clone();
    ordered_ids.sort_unstable();
    assert_eq!(
        original_ids, ordered_ids,
        "snapshot order must be immutable Node ID"
    );

    let checkpoint_after_pages = sync
        .ensure_checkpoint(owner_id, devices[8].id(), library_id)
        .await
        .expect("checkpoint after page reads must load");
    assert_eq!(checkpoint_after_pages, checkpoint_before_pages);

    let invalid_terminal_evidence = BootstrapTerminalEvidence::new(
        terminal_evidence.owner_user_id(),
        terminal_evidence.device_id(),
        terminal_evidence.library_id(),
        terminal_evidence.bootstrap_id(),
        terminal_evidence.generation(),
        terminal_evidence.snapshot_epoch(),
        terminal_evidence.snapshot_resume_sequence(),
        terminal_evidence.manifest_item_count() + 1,
        terminal_evidence.terminal_node_id(),
    );
    assert_eq!(
        restarted_bootstraps
            .complete(
                owner_id,
                devices[8].id(),
                library_id,
                multipage_bootstrap.id(),
                synveil_metadata::BootstrapCompletionEvidence::new(invalid_terminal_evidence),
            )
            .await,
        Err(RebaselineError::InvalidBootstrapToken)
    );
    assert_eq!(
        sync.ensure_checkpoint(owner_id, devices[8].id(), library_id)
            .await
            .expect("checkpoint after rejected terminal proof must load"),
        checkpoint_before_pages
    );

    let completed = restarted_bootstraps
        .complete(
            owner_id,
            devices[8].id(),
            library_id,
            multipage_bootstrap.id(),
            synveil_metadata::BootstrapCompletionEvidence::new(terminal_evidence),
        )
        .await
        .expect("multi-page bootstrap completion must commit");
    assert!(!completed.replayed());
    assert_eq!(
        completed.checkpoint().journal_epoch(),
        multipage_bootstrap.snapshot_epoch()
    );
    assert_eq!(
        completed.checkpoint().acknowledged_sequence(),
        multipage_bootstrap.snapshot_resume_sequence()
    );
    let response_loss_replay = SyncBootstrapService::new(pool.clone())
        .complete(
            owner_id,
            devices[8].id(),
            library_id,
            multipage_bootstrap.id(),
            synveil_metadata::BootstrapCompletionEvidence::new(terminal_evidence),
        )
        .await
        .expect("completion after response loss must replay");
    assert!(response_loss_replay.replayed());
    assert_eq!(response_loss_replay.checkpoint(), completed.checkpoint());
    let post_bootstrap_feed = sync
        .fetch_feed(owner_id, devices[8].id(), library_id, 500)
        .await
        .expect("post-bootstrap incremental feed must load");
    assert!(!post_bootstrap_feed.changes().is_empty());
    assert!(
        post_bootstrap_feed
            .changes()
            .iter()
            .all(|event| { event.sequence() > multipage_bootstrap.snapshot_resume_sequence() })
    );
    assert!(
        post_bootstrap_feed
            .changes()
            .iter()
            .any(|event| event.resource_id() == mp_created.id())
    );

    // A feed acknowledgment that advances beyond an older bootstrap cut wins;
    // terminal proof may not rewind it.
    let stale_bootstrap = bootstraps
        .start(owner_id, devices[9].id(), library_id)
        .await
        .expect("stale-progress bootstrap must start");
    let (_, stale_terminal) = drain_bootstrap(
        &bootstraps,
        owner_id,
        devices[9].id(),
        library_id,
        stale_bootstrap,
        1_000,
    )
    .await;
    metadata
        .create_directory(
            owner_id,
            library_id,
            Some(root.id()),
            name("newer-than-stale-bootstrap"),
        )
        .await
        .expect("post-stale-cut mutation must commit");
    let newer_feed = sync
        .fetch_feed(owner_id, devices[9].id(), library_id, 500)
        .await
        .expect("newer progress feed must load");
    assert!(newer_feed.through_sequence() > stale_bootstrap.snapshot_resume_sequence());
    sync.acknowledge(
        owner_id,
        devices[9].id(),
        library_id,
        SyncAckEvidence::new(
            owner_id,
            devices[9].id(),
            library_id,
            newer_feed.checkpoint().journal_epoch(),
            newer_feed.from_sequence(),
            newer_feed.through_sequence(),
            newer_feed.high_watermark().sequence(),
        ),
    )
    .await
    .expect("newer checkpoint acknowledgment must commit");
    assert_eq!(
        bootstraps
            .complete(
                owner_id,
                devices[9].id(),
                library_id,
                stale_bootstrap.id(),
                synveil_metadata::BootstrapCompletionEvidence::new(stale_terminal),
            )
            .await,
        Err(RebaselineError::BootstrapConflict)
    );

    // PostgreSQL/server time expires a session. Starting again transitions the
    // stale OPEN row and increments the checkpoint generation; old proof can
    // never replace the newer generation.
    let expired_bootstrap = bootstraps
        .start(owner_id, devices[10].id(), library_id)
        .await
        .expect("expiry bootstrap must start");
    let (_, expired_terminal) = drain_bootstrap(
        &bootstraps,
        owner_id,
        devices[10].id(),
        library_id,
        expired_bootstrap,
        1_000,
    )
    .await;
    sqlx::query("UPDATE sync_bootstraps SET expires_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(expired_bootstrap.id().into_uuid())
        .execute(&inspection_pool)
        .await
        .expect("bootstrap expiry must persist");
    assert_eq!(
        bootstraps
            .complete(
                owner_id,
                devices[10].id(),
                library_id,
                expired_bootstrap.id(),
                synveil_metadata::BootstrapCompletionEvidence::new(expired_terminal),
            )
            .await,
        Err(RebaselineError::BootstrapExpired)
    );
    let replacement_bootstrap = bootstraps
        .start(owner_id, devices[10].id(), library_id)
        .await
        .expect("expired bootstrap replacement must start");
    assert!(replacement_bootstrap.generation() > expired_bootstrap.generation());
    assert_eq!(
        bootstraps
            .complete(
                owner_id,
                devices[10].id(),
                library_id,
                expired_bootstrap.id(),
                synveil_metadata::BootstrapCompletionEvidence::new(expired_terminal),
            )
            .await,
        Err(RebaselineError::BootstrapExpired)
    );

    let epoch_bootstrap = bootstraps
        .start(owner_id, devices[11].id(), empty_library_id)
        .await
        .expect("epoch invalidation bootstrap must start");
    let (_, epoch_terminal) = drain_bootstrap(
        &bootstraps,
        owner_id,
        devices[11].id(),
        empty_library_id,
        epoch_bootstrap,
        10,
    )
    .await;
    sqlx::query("UPDATE libraries SET journal_epoch = journal_epoch + 1 WHERE id = $1")
        .bind(empty_library_id.into_uuid())
        .execute(&inspection_pool)
        .await
        .expect("epoch rotation fixture must persist");
    assert_eq!(
        bootstraps
            .complete(
                owner_id,
                devices[11].id(),
                empty_library_id,
                epoch_bootstrap.id(),
                synveil_metadata::BootstrapCompletionEvidence::new(epoch_terminal),
            )
            .await,
        Err(RebaselineError::RebaselineRequired {
            reason: RebaselineReason::EpochMismatch,
            current_epoch: Sequence::new(2),
            minimum_retained_sequence: Sequence::new(0),
        })
    );

    let retention_bootstrap = bootstraps
        .start(owner_id, devices[12].id(), library_id)
        .await
        .expect("retention invalidation bootstrap must start");
    let (_, retention_terminal) = drain_bootstrap(
        &bootstraps,
        owner_id,
        devices[12].id(),
        library_id,
        retention_bootstrap,
        1_000,
    )
    .await;
    metadata
        .create_directory(
            owner_id,
            library_id,
            Some(root.id()),
            name("retention-post-cut"),
        )
        .await
        .expect("retention post-cut event must commit");
    let minimum_after_cut = retention_bootstrap
        .snapshot_resume_sequence()
        .get()
        .checked_add(1)
        .expect("retention boundary arithmetic must not overflow");
    sqlx::query("UPDATE libraries SET minimum_retained_sequence = $2 WHERE id = $1")
        .bind(library_id.into_uuid())
        .bind(i64::try_from(minimum_after_cut).expect("test sequence fits BIGINT"))
        .execute(&inspection_pool)
        .await
        .expect("retention boundary fixture must persist");
    assert_eq!(
        bootstraps
            .complete(
                owner_id,
                devices[12].id(),
                library_id,
                retention_bootstrap.id(),
                synveil_metadata::BootstrapCompletionEvidence::new(retention_terminal),
            )
            .await,
        Err(RebaselineError::RebaselineRequired {
            reason: RebaselineReason::HistoryUnavailable,
            current_epoch: retention_bootstrap.snapshot_epoch(),
            minimum_retained_sequence: Sequence::new(minimum_after_cut),
        })
    );

    let first_concurrent_service = bootstraps.clone();
    let second_concurrent_service = bootstraps.clone();
    let (concurrent_a, concurrent_b) = tokio::join!(
        first_concurrent_service.start(owner_id, devices[13].id(), library_id),
        second_concurrent_service.start(owner_id, devices[13].id(), library_id),
    );
    assert_eq!(
        concurrent_a.expect("first concurrent start must succeed"),
        concurrent_b.expect("second concurrent start must safely retry")
    );

    let manifest_columns = sqlx::query_scalar::<_, String>(
        "SELECT column_name
         FROM information_schema.columns
         WHERE table_schema = 'public' AND table_name = 'sync_bootstrap_nodes'
         ORDER BY ordinal_position",
    )
    .fetch_all(&inspection_pool)
    .await
    .expect("manifest column inspection must succeed");
    for forbidden in [
        "object_id",
        "object_replica_id",
        "storage_key",
        "staging_handle",
        "filesystem_path",
        "backend_version",
        "gc_state",
        "credentials",
    ] {
        assert!(
            manifest_columns.iter().all(|column| column != forbidden),
            "logical manifest must not contain physical column {forbidden}"
        );
    }

    let deleted = bootstraps
        .cleanup_retired(100)
        .await
        .expect("bounded retired-bootstrap cleanup must succeed");
    assert!(
        deleted >= 1,
        "expired manifest cleanup must remove bounded state"
    );
    assert_eq!(
        repository
            .find_node(root.id())
            .await
            .expect("canonical root lookup after cleanup must succeed")
            .map(|node| node.id()),
        Some(root.id()),
        "bootstrap cleanup must never delete canonical Node data"
    );
    let retained_journal_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM change_journal WHERE owner_user_id = $1 AND library_id = $2",
    )
    .bind(owner_id.into_uuid())
    .bind(library_id.into_uuid())
    .fetch_one(&inspection_pool)
    .await
    .expect("journal rows must remain after bootstrap cleanup");
    assert!(retained_journal_count > 0);

    inspection_pool.close().await;
    pool.close().await;
}

/// This test is intentionally ignored unless a caller supplies an explicitly
/// disposable PostgreSQL database. It covers the complete Prompt 34 logical
/// mutation contract, including the exact journal/checkpoint/rebaseline
/// handoffs and the permanent-purge tombstone boundary.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_client_mutations_are_atomic_idempotent_scoped_and_bootstrap_safe() {
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
        .expect("Prompt 34 migration execution must succeed");
    let inspection_pool = PgPool::connect(&url)
        .await
        .expect("mutation inspection connection must succeed");
    assert!(
        pool.table_exists("device_mutation_operations")
            .await
            .expect("mutation operation table probe must succeed")
    );

    let fixture = insert_client_mutation_fixture(&pool).await;
    let repository = DomainRepository::new(&pool);
    let service = ClientMutationService::new(pool.clone());
    let journal = ChangeJournalService::new(pool.clone());
    let sync = DeviceSyncService::new(pool.clone());
    let bootstraps = SyncBootstrapService::new(pool.clone());

    let initial_checkpoint = sync
        .ensure_checkpoint(fixture.owner_id, fixture.device_one_id, fixture.library_id)
        .await
        .expect("originating device checkpoint must be created");
    assert_eq!(initial_checkpoint.journal_epoch(), Sequence::new(1));
    assert_eq!(initial_checkpoint.acknowledged_sequence(), Sequence::new(0));

    let root = repository
        .find_node(fixture.root_id)
        .await
        .expect("mutation root lookup must succeed")
        .expect("mutation root must exist");
    let create_request = client_mutation_request(
        ClientMutationId::new(),
        0,
        ClientMutation::create_directory(root.id(), root.revision(), name("client-created")),
    );
    let created_result = service
        .submit(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            create_request.clone(),
        )
        .await
        .expect("client directory creation must apply");
    let (created, created_event_id, created_sequence) = applied_mutation_parts(&created_result);
    assert_eq!(created.parent_node_id(), Some(root.id()));
    assert_eq!(created.kind(), NodeKind::Directory);
    assert_eq!(created.state(), NodeState::Active);
    assert_eq!(created.revision(), Revision::new(0));
    assert_eq!(created_sequence, Sequence::new(1));
    assert!(!created_result.replayed());

    // Simulate a lost HTTP response: the second submission must reconstruct
    // the durable logical result and must not allocate another Node/event.
    let created_replay = service
        .submit(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            create_request,
        )
        .await
        .expect("same client mutation must replay");
    let (replayed_node, replayed_event_id, replayed_sequence) =
        applied_mutation_parts(&created_replay);
    assert_eq!(replayed_node, created);
    assert_eq!(replayed_event_id, created_event_id);
    assert_eq!(replayed_sequence, created_sequence);
    assert!(created_replay.replayed());

    let checkpoint_after_create = sync
        .ensure_checkpoint(fixture.owner_id, fixture.device_one_id, fixture.library_id)
        .await
        .expect("checkpoint must remain readable after mutation");
    assert_eq!(
        checkpoint_after_create.acknowledged_sequence(),
        Sequence::new(0),
        "client submission must not advance its device checkpoint"
    );

    let first_device_feed = sync
        .fetch_feed(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            100,
        )
        .await
        .expect("originating device must observe the mutation event");
    assert_eq!(first_device_feed.changes().len(), 1);
    assert_eq!(first_device_feed.changes()[0].id(), created_event_id);
    let second_device_feed = sync
        .fetch_feed(
            fixture.owner_id,
            fixture.device_two_id,
            fixture.library_id,
            100,
        )
        .await
        .expect("another device must independently observe the mutation event");
    assert_eq!(second_device_feed.changes().len(), 1);
    assert_eq!(second_device_feed.changes()[0].id(), created_event_id);

    let destination_result = service
        .submit(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            client_mutation_request(
                ClientMutationId::new(),
                1,
                ClientMutation::create_directory(root.id(), Revision::new(0), name("destination")),
            ),
        )
        .await
        .expect("destination directory mutation must apply");
    let (destination, _, destination_sequence) = applied_mutation_parts(&destination_result);
    let destination = destination.clone();
    assert_eq!(destination_sequence, Sequence::new(2));

    let rename_result = service
        .submit(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            client_mutation_request(
                ClientMutationId::new(),
                2,
                ClientMutation::rename_node(
                    created.id(),
                    created.revision(),
                    name("client-renamed"),
                ),
            ),
        )
        .await
        .expect("rename mutation must apply");
    let (renamed, _, renamed_sequence) = applied_mutation_parts(&rename_result);
    let renamed = renamed.clone();
    assert_eq!(renamed.name().as_str(), "client-renamed");
    assert_eq!(renamed.revision(), Revision::new(1));
    assert_eq!(renamed_sequence, Sequence::new(3));

    let move_result = service
        .submit(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            client_mutation_request(
                ClientMutationId::new(),
                3,
                ClientMutation::move_node(
                    renamed.id(),
                    renamed.revision(),
                    destination.id(),
                    destination.revision(),
                ),
            ),
        )
        .await
        .expect("move mutation must apply");
    let (moved, _, moved_sequence) = applied_mutation_parts(&move_result);
    let moved = moved.clone();
    assert_eq!(moved.parent_node_id(), Some(destination.id()));
    assert_eq!(moved.revision(), Revision::new(2));
    assert_eq!(moved_sequence, Sequence::new(4));

    let trash_result = service
        .submit(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            client_mutation_request(
                ClientMutationId::new(),
                4,
                ClientMutation::trash_node(moved.id(), moved.revision()),
            ),
        )
        .await
        .expect("Trash mutation must apply");
    let (trashed, _, trashed_sequence) = applied_mutation_parts(&trash_result);
    let trashed = trashed.clone();
    assert_eq!(trashed.state(), NodeState::Trashed);
    assert_eq!(trashed.revision(), Revision::new(3));
    assert_eq!(trashed_sequence, Sequence::new(5));

    let restore_result = service
        .submit(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            client_mutation_request(
                ClientMutationId::new(),
                5,
                ClientMutation::restore_node(
                    trashed.id(),
                    trashed.revision(),
                    destination.id(),
                    destination.revision(),
                ),
            ),
        )
        .await
        .expect("restore mutation must apply");
    let (restored, _, restored_sequence) = applied_mutation_parts(&restore_result);
    let restored = restored.clone();
    assert_eq!(restored.state(), NodeState::Active);
    assert_eq!(restored.parent_node_id(), Some(destination.id()));
    assert_eq!(restored.revision(), Revision::new(4));
    assert_eq!(restored_sequence, Sequence::new(6));

    let events = list_journal_events(&journal, fixture.owner_id, fixture.library_id).await;
    assert_eq!(events.len(), 6);
    assert_eq!(
        events
            .iter()
            .map(|event| event.change_kind())
            .collect::<Vec<_>>(),
        vec![
            ChangeKind::NodeCreated,
            ChangeKind::NodeCreated,
            ChangeKind::NodeRenamed,
            ChangeKind::NodeMoved,
            ChangeKind::NodeTrashed,
            ChangeKind::NodeRestored,
        ]
    );
    let operation_counts = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT count(*),
                count(*) FILTER (WHERE outcome = 'APPLIED'),
                count(*) FILTER (WHERE outcome = 'CONFLICT')
         FROM device_mutation_operations
         WHERE owner_user_id = $1 AND device_id = $2 AND library_id = $3",
    )
    .bind(fixture.owner_id.into_uuid())
    .bind(fixture.device_one_id.into_uuid())
    .bind(fixture.library_id.into_uuid())
    .fetch_one(&inspection_pool)
    .await
    .expect("mutation operation count query must succeed");
    assert_eq!(operation_counts, (6, 6, 0));

    // A stale revision is a durable conflict, not a second mutation. Its
    // replay remains byte-for-byte equivalent at the transport-neutral layer.
    let stale_request = client_mutation_request(
        ClientMutationId::new(),
        6,
        ClientMutation::rename_node(restored.id(), Revision::new(3), name("stale-name")),
    );
    let stale_result = service
        .submit(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            stale_request.clone(),
        )
        .await
        .expect("stale revision must become a persisted conflict result");
    assert_eq!(
        conflict_reason(&stale_result),
        MutationConflictReason::RevisionMismatch
    );
    assert!(!stale_result.replayed());
    let stale_replay = service
        .submit(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            stale_request.clone(),
        )
        .await
        .expect("stale conflict must replay");
    assert_eq!(
        conflict_reason(&stale_replay),
        MutationConflictReason::RevisionMismatch
    );
    assert!(stale_replay.replayed());
    match (&stale_result, &stale_replay) {
        (
            ClientMutationResult::Conflict {
                conflict: original, ..
            },
            ClientMutationResult::Conflict {
                conflict: replayed, ..
            },
        ) => assert_eq!(original, replayed),
        _ => panic!("stale mutation must return conflict on both attempts"),
    }
    let events_after_conflict =
        list_journal_events(&journal, fixture.owner_id, fixture.library_id).await;
    assert_eq!(events_after_conflict.len(), 6);
    let mut different_fingerprint = stale_request.clone();
    different_fingerprint = ClientMutationRequest::new(
        different_fingerprint.mutation_id(),
        different_fingerprint.base_epoch(),
        different_fingerprint.base_sequence(),
        ClientMutation::rename_node(restored.id(), Revision::new(3), name("other-name")),
    );
    assert_eq!(
        service
            .submit(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                different_fingerprint,
            )
            .await,
        Err(ClientMutationError::MutationIdConflict)
    );

    // A bootstrap cut may coexist with a later client mutation. Completion
    // hands the device to the cut, leaving the post-cut event in the feed.
    let bootstrap = bootstraps
        .start(fixture.owner_id, fixture.device_two_id, fixture.library_id)
        .await
        .expect("bootstrap must start at the current mutation head");
    assert_eq!(bootstrap.snapshot_epoch(), Sequence::new(1));
    assert_eq!(bootstrap.snapshot_resume_sequence(), Sequence::new(6));
    let post_cut_result = service
        .submit(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            client_mutation_request(
                ClientMutationId::new(),
                bootstrap.snapshot_resume_sequence().get(),
                ClientMutation::create_directory(
                    root.id(),
                    root.revision(),
                    name("post-cut-directory"),
                ),
            ),
        )
        .await
        .expect("mutation after bootstrap cut must apply");
    let (_, post_cut_event_id, post_cut_sequence) = applied_mutation_parts(&post_cut_result);
    assert_eq!(post_cut_sequence, Sequence::new(7));
    let (bootstrap_nodes, terminal) = drain_bootstrap(
        &bootstraps,
        fixture.owner_id,
        fixture.device_two_id,
        fixture.library_id,
        bootstrap,
        2,
    )
    .await;
    assert!(
        bootstrap_nodes
            .iter()
            .all(|node| node.name().as_str() != "post-cut-directory")
    );
    let completion = bootstraps
        .complete(
            fixture.owner_id,
            fixture.device_two_id,
            fixture.library_id,
            bootstrap.id(),
            synveil_metadata::BootstrapCompletionEvidence::new(terminal),
        )
        .await
        .expect("bootstrap completion must hand off at the captured cut");
    assert_eq!(
        completion.checkpoint().acknowledged_sequence(),
        Sequence::new(6)
    );
    let post_cut_feed = sync
        .fetch_feed(
            fixture.owner_id,
            fixture.device_two_id,
            fixture.library_id,
            100,
        )
        .await
        .expect("post-bootstrap incremental feed must load");
    assert_eq!(post_cut_feed.changes().len(), 1);
    assert_eq!(post_cut_feed.changes()[0].id(), post_cut_event_id);
    assert_eq!(post_cut_feed.changes()[0].sequence(), post_cut_sequence);

    // Permanently purged metadata is represented only by its retained journal
    // tombstone. A later client mutation cannot recreate the Node.
    let purge_file = Node::new_child(
        NodeId::new(),
        fixture.library_id,
        &root,
        NodeKind::File,
        name("purged-client-target"),
        timestamp("2026-08-27T00:00:00.123456Z"),
    )
    .expect("purge fixture file must satisfy domain invariants");
    repository
        .insert_node(&purge_file)
        .await
        .expect("purge fixture file must persist");
    let metadata = FileMetadataService::new(pool.clone());
    let purged_trash = metadata
        .delete_node(fixture.owner_id, purge_file.id(), purge_file.revision())
        .await
        .expect("purge fixture must enter Trash");
    sqlx::query(
        "UPDATE nodes
         SET trashed_at = CURRENT_TIMESTAMP - INTERVAL '31 days'
         WHERE id = $1 AND library_id = $2",
    )
    .bind(purge_file.id().into_uuid())
    .bind(fixture.library_id.into_uuid())
    .execute(&inspection_pool)
    .await
    .expect("purge fixture age update must succeed");
    let retention = TrashRetentionService::new(pool.clone(), TrashRetentionPolicy::default());
    let purging = retention
        .begin_node_purge(fixture.owner_id, purge_file.id(), purged_trash.revision())
        .await
        .expect("purge fixture must enter PURGING");
    retention
        .execute_metadata_purge(fixture.owner_id, purge_file.id(), purging.revision())
        .await
        .expect("purge fixture must commit permanent metadata deletion");
    assert!(
        repository
            .find_node(purge_file.id())
            .await
            .expect("purged node lookup must succeed")
            .is_none()
    );
    let purge_head = library_sync_head(&inspection_pool, fixture.library_id).await;
    let purged_request = client_mutation_request(
        ClientMutationId::new(),
        purge_head.get(),
        ClientMutation::trash_node(purge_file.id(), purging.revision()),
    );
    let purged_result = service
        .submit(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            purged_request.clone(),
        )
        .await
        .expect("purged target must become a durable conflict");
    assert_eq!(
        conflict_reason(&purged_result),
        MutationConflictReason::ResourcePurged
    );
    let purged_replay = service
        .submit(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            purged_request,
        )
        .await
        .expect("purged conflict must replay");
    assert!(purged_replay.replayed());
    assert_eq!(
        conflict_reason(&purged_replay),
        MutationConflictReason::ResourcePurged
    );
    assert!(
        repository
            .find_node(purge_file.id())
            .await
            .expect("purged node must remain absent after replay")
            .is_none()
    );

    let final_events = list_journal_events(&journal, fixture.owner_id, fixture.library_id).await;
    assert_eq!(
        final_events
            .iter()
            .filter(|event| event.resource_id() == purge_file.id())
            .map(|event| event.change_kind())
            .collect::<Vec<_>>(),
        vec![ChangeKind::NodeTrashed, ChangeKind::NodePurged]
    );
    let final_operation_counts = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT count(*),
                count(*) FILTER (WHERE outcome = 'APPLIED'),
                count(*) FILTER (WHERE outcome = 'CONFLICT')
         FROM device_mutation_operations
         WHERE owner_user_id = $1 AND device_id = $2 AND library_id = $3",
    )
    .bind(fixture.owner_id.into_uuid())
    .bind(fixture.device_one_id.into_uuid())
    .bind(fixture.library_id.into_uuid())
    .fetch_one(&inspection_pool)
    .await
    .expect("final mutation operation count query must succeed");
    assert_eq!(final_operation_counts, (9, 7, 2));

    // Base-context failures are distinct from resource conflicts and are not
    // recorded as terminal mutation outcomes.
    let current_head = library_sync_head(&inspection_pool, fixture.library_id).await;
    let wrong_epoch = ClientMutationRequest::new(
        ClientMutationId::new(),
        Sequence::new(2),
        current_head,
        ClientMutation::rename_node(restored.id(), restored.revision(), name("wrong-epoch")),
    );
    assert_eq!(
        service
            .submit(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                wrong_epoch,
            )
            .await,
        Err(ClientMutationError::RebaselineRequired {
            reason: RebaselineReason::EpochMismatch,
            current_epoch: Sequence::new(1),
            minimum_retained_sequence: Sequence::new(0),
        })
    );
    sqlx::query("UPDATE libraries SET minimum_retained_sequence = $2 WHERE id = $1")
        .bind(fixture.library_id.into_uuid())
        .bind(i64::try_from(current_head.get()).expect("test sequence fits BIGINT"))
        .execute(&inspection_pool)
        .await
        .expect("retention boundary fixture update must succeed");
    let below_retained = client_mutation_request(
        ClientMutationId::new(),
        current_head.get().saturating_sub(1),
        ClientMutation::rename_node(restored.id(), restored.revision(), name("old-base")),
    );
    assert_eq!(
        service
            .submit(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                below_retained,
            )
            .await,
        Err(ClientMutationError::RebaselineRequired {
            reason: RebaselineReason::HistoryUnavailable,
            current_epoch: Sequence::new(1),
            minimum_retained_sequence: current_head,
        })
    );
    let future_base = client_mutation_request(
        ClientMutationId::new(),
        current_head.get() + 1,
        ClientMutation::rename_node(restored.id(), restored.revision(), name("future-base")),
    );
    assert_eq!(
        service
            .submit(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                future_base,
            )
            .await,
        Err(ClientMutationError::InvalidMutation)
    );

    // Owner/device/library scope is checked before any operation row is
    // created. Reusing a valid ID never turns possession into authorization.
    for (owner_id, device_id, library_id) in [
        (
            fixture.other_owner_id,
            fixture.device_one_id,
            fixture.library_id,
        ),
        (
            fixture.owner_id,
            fixture.foreign_device_id,
            fixture.library_id,
        ),
        (
            fixture.owner_id,
            fixture.device_one_id,
            fixture.other_library_id,
        ),
        (
            fixture.owner_id,
            fixture.revoked_device_id,
            fixture.library_id,
        ),
    ] {
        let result = service
            .submit(
                owner_id,
                device_id,
                library_id,
                client_mutation_request(
                    ClientMutationId::new(),
                    current_head.get(),
                    ClientMutation::rename_node(
                        restored.id(),
                        restored.revision(),
                        name("scope-probe"),
                    ),
                ),
            )
            .await;
        assert_eq!(result, Err(ClientMutationError::NotFound));
    }

    inspection_pool.close().await;
    pool.close().await;
}

/// Required PostgreSQL races for Prompt 34. Each fixture starts with direct
/// canonical rows and no journal entries so every assertion measures exactly
/// the operation pair under test.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_client_mutations_are_race_safe_for_duplicate_and_stale_pairs() {
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
        .expect("Prompt 34 race migrations must be current");
    let inspection_pool = PgPool::connect(&url)
        .await
        .expect("race inspection connection must succeed");
    let fixture = insert_client_mutation_fixture(&pool).await;
    let repository = DomainRepository::new(&pool);
    let service = ClientMutationService::new(pool.clone());
    let journal = ChangeJournalService::new(pool.clone());
    let observed_at = timestamp("2026-08-27T00:00:00.123456Z");
    let root = repository
        .find_node(fixture.root_id)
        .await
        .expect("race root lookup must succeed")
        .expect("race root must exist");

    // Same ID and same fingerprint: one transaction is the executor and the
    // waiter replays its committed operation row.
    let duplicate_node = Node::new_child(
        NodeId::new(),
        fixture.library_id,
        &root,
        NodeKind::Directory,
        name("duplicate-target"),
        observed_at,
    )
    .expect("duplicate fixture node must be valid");
    repository
        .insert_node(&duplicate_node)
        .await
        .expect("duplicate fixture node must persist");
    let duplicate_request = client_mutation_request(
        ClientMutationId::new(),
        0,
        ClientMutation::rename_node(
            duplicate_node.id(),
            duplicate_node.revision(),
            name("duplicate-renamed"),
        ),
    );
    let duplicate_a = service.clone();
    let duplicate_b = service.clone();
    let duplicate_pair = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(
            duplicate_a.submit(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                duplicate_request.clone(),
            ),
            duplicate_b.submit(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                duplicate_request,
            ),
        )
    })
    .await
    .expect("same-ID race must not deadlock");
    let duplicate_first = duplicate_pair
        .0
        .expect("first same-ID request must succeed");
    let duplicate_second = duplicate_pair
        .1
        .expect("second same-ID request must replay");
    assert!(matches!(
        duplicate_first,
        ClientMutationResult::Applied { .. }
    ));
    assert!(matches!(
        duplicate_second,
        ClientMutationResult::Applied { .. }
    ));
    assert_ne!(duplicate_first.replayed(), duplicate_second.replayed());
    let (duplicate_first_node, duplicate_first_event, duplicate_first_sequence) =
        applied_mutation_parts(&duplicate_first);
    let (duplicate_second_node, duplicate_second_event, duplicate_second_sequence) =
        applied_mutation_parts(&duplicate_second);
    assert_eq!(duplicate_first_node, duplicate_second_node);
    assert_eq!(duplicate_first_event, duplicate_second_event);
    assert_eq!(duplicate_first_sequence, duplicate_second_sequence);

    // Two IDs at one stale revision: rename-versus-rename.
    let rename_node = Node::new_child(
        NodeId::new(),
        fixture.library_id,
        &root,
        NodeKind::Directory,
        name("rename-race-target"),
        observed_at,
    )
    .expect("rename race fixture must be valid");
    repository
        .insert_node(&rename_node)
        .await
        .expect("rename race fixture must persist");
    let rename_a = client_mutation_request(
        ClientMutationId::new(),
        0,
        ClientMutation::rename_node(rename_node.id(), Revision::new(0), name("rename-a")),
    );
    let rename_b = client_mutation_request(
        ClientMutationId::new(),
        0,
        ClientMutation::rename_node(rename_node.id(), Revision::new(0), name("rename-b")),
    );
    let (rename_a, rename_b) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(
            service.submit(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                rename_a,
            ),
            service.submit(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                rename_b,
            ),
        )
    })
    .await
    .expect("rename race must not deadlock");
    let rename_a = rename_a.expect("rename race first result must return");
    let rename_b = rename_b.expect("rename race second result must return");
    assert_one_applied_one_conflict(&rename_a, &rename_b);
    assert_eq!(
        conflict_reason(
            if matches!(rename_a, ClientMutationResult::Conflict { .. }) {
                &rename_a
            } else {
                &rename_b
            }
        ),
        MutationConflictReason::RevisionMismatch
    );

    // Two IDs at one stale revision: move-versus-move. Destination revisions
    // are explicit, so the pair cannot silently retarget a changed namespace.
    let move_node = Node::new_child(
        NodeId::new(),
        fixture.library_id,
        &root,
        NodeKind::Directory,
        name("move-race-target"),
        observed_at,
    )
    .expect("move race source must be valid");
    let move_destination_a = Node::new_child(
        NodeId::new(),
        fixture.library_id,
        &root,
        NodeKind::Directory,
        name("move-destination-a"),
        observed_at,
    )
    .expect("move race destination A must be valid");
    let move_destination_b = Node::new_child(
        NodeId::new(),
        fixture.library_id,
        &root,
        NodeKind::Directory,
        name("move-destination-b"),
        observed_at,
    )
    .expect("move race destination B must be valid");
    for node in [&move_node, &move_destination_a, &move_destination_b] {
        repository
            .insert_node(node)
            .await
            .expect("move race fixture must persist");
    }
    let move_a = client_mutation_request(
        ClientMutationId::new(),
        0,
        ClientMutation::move_node(
            move_node.id(),
            Revision::new(0),
            move_destination_a.id(),
            Revision::new(0),
        ),
    );
    let move_b = client_mutation_request(
        ClientMutationId::new(),
        0,
        ClientMutation::move_node(
            move_node.id(),
            Revision::new(0),
            move_destination_b.id(),
            Revision::new(0),
        ),
    );
    let (move_a, move_b) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(
            service.submit(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                move_a,
            ),
            service.submit(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                move_b,
            ),
        )
    })
    .await
    .expect("move race must not deadlock");
    let move_a = move_a.expect("move race first result must return");
    let move_b = move_b.expect("move race second result must return");
    assert_one_applied_one_conflict(&move_a, &move_b);
    assert_eq!(
        conflict_reason(if matches!(move_a, ClientMutationResult::Conflict { .. }) {
            &move_a
        } else {
            &move_b
        }),
        MutationConflictReason::RevisionMismatch
    );

    // Trash-versus-rename from one revision must also serialize to one winner
    // and one explicit state/revision conflict.
    let trash_node = Node::new_child(
        NodeId::new(),
        fixture.library_id,
        &root,
        NodeKind::File,
        name("trash-rename-target"),
        observed_at,
    )
    .expect("Trash/rename fixture must be valid");
    repository
        .insert_node(&trash_node)
        .await
        .expect("Trash/rename fixture must persist");
    let trash_request = client_mutation_request(
        ClientMutationId::new(),
        0,
        ClientMutation::trash_node(trash_node.id(), Revision::new(0)),
    );
    let rename_request = client_mutation_request(
        ClientMutationId::new(),
        0,
        ClientMutation::rename_node(trash_node.id(), Revision::new(0), name("after-trash")),
    );
    let (trash_result, rename_result) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(
            service.submit(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                trash_request,
            ),
            service.submit(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                rename_request,
            ),
        )
    })
    .await
    .expect("Trash/rename race must not deadlock");
    let trash_result = trash_result.expect("Trash race result must return");
    let rename_result = rename_result.expect("rename race result must return");
    assert_one_applied_one_conflict(&trash_result, &rename_result);
    let conflict = if matches!(trash_result, ClientMutationResult::Conflict { .. }) {
        &trash_result
    } else {
        &rename_result
    };
    assert!(matches!(
        conflict_reason(conflict),
        MutationConflictReason::RevisionMismatch | MutationConflictReason::NodeStateChanged
    ));

    // Same parent/name creation is serialized by the namespace guard. No
    // database uniqueness assumption is needed for the logical decision.
    let create_a = client_mutation_request(
        ClientMutationId::new(),
        0,
        ClientMutation::create_directory(root.id(), Revision::new(0), name("same-name")),
    );
    let create_b = client_mutation_request(
        ClientMutationId::new(),
        0,
        ClientMutation::create_directory(root.id(), Revision::new(0), name("same-name")),
    );
    let (create_a, create_b) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(
            service.submit(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                create_a,
            ),
            service.submit(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                create_b,
            ),
        )
    })
    .await
    .expect("same-name create race must not deadlock");
    let create_a = create_a.expect("same-name create A result must return");
    let create_b = create_b.expect("same-name create B result must return");
    assert_one_applied_one_conflict(&create_a, &create_b);
    let conflict = if matches!(create_a, ClientMutationResult::Conflict { .. }) {
        &create_a
    } else {
        &create_b
    };
    assert_eq!(
        conflict_reason(conflict),
        MutationConflictReason::NameOccupied
    );

    // The same stale revision from two different registered devices still
    // has one canonical winner; device identity does not weaken preconditions.
    let multi_device_node = Node::new_child(
        NodeId::new(),
        fixture.library_id,
        &root,
        NodeKind::Directory,
        name("multi-device-target"),
        observed_at,
    )
    .expect("multi-device fixture must be valid");
    repository
        .insert_node(&multi_device_node)
        .await
        .expect("multi-device fixture must persist");
    let device_request_a = client_mutation_request(
        ClientMutationId::new(),
        0,
        ClientMutation::rename_node(
            multi_device_node.id(),
            Revision::new(0),
            name("device-one-name"),
        ),
    );
    let device_request_b = client_mutation_request(
        ClientMutationId::new(),
        0,
        ClientMutation::rename_node(
            multi_device_node.id(),
            Revision::new(0),
            name("device-two-name"),
        ),
    );
    let (device_result_a, device_result_b) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(
            service.submit(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                device_request_a,
            ),
            service.submit(
                fixture.owner_id,
                fixture.device_two_id,
                fixture.library_id,
                device_request_b,
            ),
        )
    })
    .await
    .expect("multi-device stale-write race must not deadlock");
    let device_result_a = device_result_a.expect("device one stale result must return");
    let device_result_b = device_result_b.expect("device two stale result must return");
    assert_one_applied_one_conflict(&device_result_a, &device_result_b);
    assert_eq!(
        conflict_reason(
            if matches!(device_result_a, ClientMutationResult::Conflict { .. }) {
                &device_result_a
            } else {
                &device_result_b
            }
        ),
        MutationConflictReason::RevisionMismatch
    );

    let events = list_journal_events(&journal, fixture.owner_id, fixture.library_id).await;
    // Six race pairs above each admit exactly one journal event.
    assert_eq!(events.len(), 6);
    let rename_event_count = events
        .iter()
        .filter(|event| event.change_kind() == ChangeKind::NodeRenamed)
        .count();
    assert!(
        (3..=4).contains(&rename_event_count),
        "duplicate, rename, Trash/rename, and multi-device races emit three or four rename events"
    );
    let (operation_count, applied_count, conflict_count): (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*),
                count(*) FILTER (WHERE outcome = 'APPLIED'),
                count(*) FILTER (WHERE outcome = 'CONFLICT')
         FROM device_mutation_operations
         WHERE owner_user_id = $1 AND library_id = $2",
    )
    .bind(fixture.owner_id.into_uuid())
    .bind(fixture.library_id.into_uuid())
    .fetch_one(&inspection_pool)
    .await
    .expect("race operation count query must succeed");
    assert_eq!((operation_count, applied_count, conflict_count), (11, 6, 5));

    inspection_pool.close().await;
    pool.close().await;
}

/// Prompt 35's complete durable inspection and manual-resolution protocol on
/// live PostgreSQL. One scenario deliberately crosses journal, feed,
/// checkpoint, rebaseline, Trash, and purge boundaries so a control-plane
/// conflict cannot accidentally become a parallel metadata mutation path.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_sync_conflicts_are_durable_inspectable_and_manually_resolvable() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable test database");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    MigrationRunner::new()
        .run(&pool)
        .await
        .expect("Prompt 35 migrations must succeed");
    let inspection_pool = PgPool::connect(&url)
        .await
        .expect("conflict inspection connection must succeed");
    for table in ["sync_conflicts", "sync_conflict_resolutions"] {
        assert!(
            pool.table_exists(table)
                .await
                .expect("Prompt 35 table probe must succeed"),
            "expected Prompt 35 table {table}"
        );
    }

    let fixture = insert_client_mutation_fixture(&pool).await;
    let repository = DomainRepository::new(&pool);
    let mutations = ClientMutationService::new(pool.clone());
    let conflicts = ConflictManagementService::new(pool.clone());
    let journal = ChangeJournalService::new(pool.clone());
    let sync = DeviceSyncService::new(pool.clone());
    let bootstraps = SyncBootstrapService::new(pool.clone());
    let root = repository
        .find_node(fixture.root_id)
        .await
        .expect("conflict root lookup must succeed")
        .expect("conflict root must exist");
    let origin_checkpoint = sync
        .ensure_checkpoint(fixture.owner_id, fixture.device_one_id, fixture.library_id)
        .await
        .expect("origin conflict checkpoint must exist");
    assert_eq!(origin_checkpoint.acknowledged_sequence(), Sequence::new(0));

    let (accept_original, accept_conflict_id) = create_stale_rename_conflict(
        &mutations,
        &inspection_pool,
        fixture,
        &root,
        "accept-client-intent",
    )
    .await;
    let accept_replay = mutations
        .submit(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            accept_original.clone(),
        )
        .await
        .expect("original conflict mutation must replay");
    assert_eq!(conflict_id(&accept_replay), accept_conflict_id);
    assert!(accept_replay.replayed());
    let linked_count: i64 = sqlx::query_scalar(
        "SELECT count(*)
         FROM sync_conflicts AS c
         INNER JOIN device_mutation_operations AS o
            ON o.owner_user_id = c.owner_user_id
           AND o.device_id = c.device_id
           AND o.library_id = c.library_id
           AND o.client_mutation_id = c.original_client_mutation_id
           AND o.conflict_id = c.conflict_id
         WHERE c.conflict_id = $1 AND o.outcome = 'CONFLICT'",
    )
    .bind(accept_conflict_id.into_uuid())
    .fetch_one(&inspection_pool)
    .await
    .expect("conflict linkage count must succeed");
    assert_eq!(linked_count, 1);

    let (_, apply_conflict_id) = create_stale_rename_conflict(
        &mutations,
        &inspection_pool,
        fixture,
        &root,
        "apply-client-intent",
    )
    .await;
    let (_, rebaseline_conflict_id) = create_stale_rename_conflict(
        &mutations,
        &inspection_pool,
        fixture,
        &root,
        "post-rebaseline-intent",
    )
    .await;

    let first_page = conflicts
        .list_open(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            None,
            1,
        )
        .await
        .expect("first OPEN conflict page must load");
    assert_eq!(first_page.conflicts().len(), 1);
    assert!(first_page.has_more());
    let second_page = conflicts
        .list_open(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            first_page.next_position(),
            1,
        )
        .await
        .expect("second OPEN conflict page must load");
    assert_eq!(second_page.conflicts().len(), 1);
    assert_ne!(
        first_page.conflicts()[0].conflict_id(),
        second_page.conflicts()[0].conflict_id()
    );

    let accept_detail = conflicts
        .detail(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            accept_conflict_id,
        )
        .await
        .expect("conflict detail must load");
    assert_eq!(accept_detail.lifecycle(), ConflictLifecycle::Open);
    assert_eq!(
        accept_detail.original_client_mutation_id(),
        accept_original.mutation_id()
    );
    assert_eq!(accept_detail.resource_id(), root.id());
    assert_eq!(
        accept_detail.historical_observation().reason(),
        MutationConflictReason::RevisionMismatch
    );
    match accept_detail.original_intent() {
        ClientMutation::RenameNode {
            node_id,
            expected_revision,
            new_name,
        } => {
            assert_eq!(*node_id, root.id());
            assert_eq!(*expected_revision, Revision::new(1));
            assert_eq!(new_name.as_str(), "accept-client-intent");
        }
        _ => panic!("durable original intent must preserve the rename projection"),
    }
    for (owner, device, library) in [
        (
            fixture.other_owner_id,
            fixture.device_one_id,
            fixture.library_id,
        ),
        (fixture.owner_id, fixture.device_two_id, fixture.library_id),
        (
            fixture.owner_id,
            fixture.device_one_id,
            fixture.other_library_id,
        ),
        (
            fixture.owner_id,
            fixture.revoked_device_id,
            fixture.library_id,
        ),
    ] {
        assert_eq!(
            conflicts
                .detail(owner, device, library, accept_conflict_id)
                .await,
            Err(ConflictManagementError::NotFound)
        );
    }

    let immutable_update = sqlx::query(
        "UPDATE sync_conflicts SET intent_requested_name = 'tampered' WHERE conflict_id = $1",
    )
    .bind(accept_conflict_id.into_uuid())
    .execute(&inspection_pool)
    .await;
    assert!(immutable_update.is_err());
    assert_eq!(
        conflicts
            .detail(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                accept_conflict_id,
            )
            .await
            .expect("immutable conflict detail must still load")
            .original_intent(),
        accept_detail.original_intent()
    );

    let node_before_accept = repository
        .find_node(root.id())
        .await
        .expect("pre-accept Node lookup must succeed")
        .expect("pre-accept Node must exist");
    let journal_before_accept =
        list_journal_events(&journal, fixture.owner_id, fixture.library_id).await;
    let accept_resolution_id = ConflictResolutionId::new();
    let accept_request = ConflictResolutionRequest::new(
        accept_resolution_id,
        ConflictResolutionAction::AcceptServer,
        None,
        None,
    );
    let accepted = conflicts
        .resolve(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            accept_conflict_id,
            accept_request,
        )
        .await
        .expect("ACCEPT_SERVER must dismiss an OPEN conflict");
    assert!(matches!(
        accepted,
        ConflictResolutionResult::AcceptedServer {
            replayed: false,
            ..
        }
    ));
    let accepted_replay = conflicts
        .resolve(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            accept_conflict_id,
            accept_request,
        )
        .await
        .expect("lost ACCEPT_SERVER response must replay");
    assert!(matches!(
        accepted_replay,
        ConflictResolutionResult::AcceptedServer { replayed: true, .. }
    ));
    assert_eq!(
        conflicts
            .resolve(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                accept_conflict_id,
                ConflictResolutionRequest::new(
                    accept_resolution_id,
                    ConflictResolutionAction::ApplyClientIntent,
                    Some(root.revision()),
                    None,
                ),
            )
            .await,
        Err(ConflictManagementError::ResolutionIdConflict)
    );
    assert_eq!(
        conflicts
            .resolve(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                accept_conflict_id,
                ConflictResolutionRequest::new(
                    ConflictResolutionId::new(),
                    ConflictResolutionAction::AcceptServer,
                    None,
                    None,
                ),
            )
            .await,
        Err(ConflictManagementError::ConflictNotOpen {
            lifecycle: ConflictLifecycle::Dismissed,
        })
    );
    assert_eq!(
        repository
            .find_node(root.id())
            .await
            .expect("post-accept Node lookup must succeed")
            .expect("post-accept Node must exist"),
        node_before_accept
    );
    assert_eq!(
        list_journal_events(&journal, fixture.owner_id, fixture.library_id).await,
        journal_before_accept
    );
    assert_eq!(
        conflicts
            .detail(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                accept_conflict_id,
            )
            .await
            .expect("dismissed conflict detail must load")
            .lifecycle(),
        ConflictLifecycle::Dismissed
    );

    let apply_resolution_id = ConflictResolutionId::new();
    let apply_request = ConflictResolutionRequest::new(
        apply_resolution_id,
        ConflictResolutionAction::ApplyClientIntent,
        Some(root.revision()),
        None,
    );
    let applied = conflicts
        .resolve(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            apply_conflict_id,
            apply_request,
        )
        .await
        .expect("fresh manual apply must succeed");
    let (applied_event_id, applied_sequence) = match applied {
        ConflictResolutionResult::AppliedClientIntent {
            journal_event_id,
            journal_sequence,
            replayed,
            ..
        } => {
            assert!(!replayed);
            (journal_event_id, journal_sequence)
        }
        ConflictResolutionResult::AcceptedServer { .. } => {
            panic!("manual apply must not become accept-server")
        }
    };
    assert_eq!(applied_sequence, Sequence::new(1));
    let renamed = repository
        .find_node(root.id())
        .await
        .expect("manual-apply Node lookup must succeed")
        .expect("manual-apply Node must exist");
    assert_eq!(renamed.name().as_str(), "apply-client-intent");
    assert_eq!(renamed.revision(), Revision::new(1));
    let apply_replay = conflicts
        .resolve(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            apply_conflict_id,
            apply_request,
        )
        .await
        .expect("lost manual-apply response must replay");
    assert!(matches!(
        apply_replay,
        ConflictResolutionResult::AppliedClientIntent {
            journal_event_id,
            journal_sequence,
            replayed: true,
            ..
        } if journal_event_id == applied_event_id && journal_sequence == applied_sequence
    ));
    let original_outcome: String = sqlx::query_scalar(
        "SELECT outcome FROM device_mutation_operations
         WHERE conflict_id = $1",
    )
    .bind(apply_conflict_id.into_uuid())
    .fetch_one(&inspection_pool)
    .await
    .expect("original operation outcome query must succeed");
    assert_eq!(original_outcome, "CONFLICT");
    let other_feed = sync
        .fetch_feed(
            fixture.owner_id,
            fixture.device_two_id,
            fixture.library_id,
            100,
        )
        .await
        .expect("other Device feed must observe successful manual apply");
    assert_eq!(other_feed.changes().len(), 1);
    assert_eq!(other_feed.changes()[0].id(), applied_event_id);
    assert_eq!(
        sync.ensure_checkpoint(fixture.owner_id, fixture.device_one_id, fixture.library_id)
            .await
            .expect("origin checkpoint must remain readable")
            .acknowledged_sequence(),
        Sequence::new(0)
    );

    let bootstrap = bootstraps
        .start(fixture.owner_id, fixture.device_two_id, fixture.library_id)
        .await
        .expect("rebaseline with an OPEN conflict must start");
    let (_, terminal) = drain_bootstrap(
        &bootstraps,
        fixture.owner_id,
        fixture.device_two_id,
        fixture.library_id,
        bootstrap,
        2,
    )
    .await;
    bootstraps
        .complete(
            fixture.owner_id,
            fixture.device_two_id,
            fixture.library_id,
            bootstrap.id(),
            synveil_metadata::BootstrapCompletionEvidence::new(terminal),
        )
        .await
        .expect("rebaseline with an OPEN conflict must complete");
    assert_eq!(
        conflicts
            .detail(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                rebaseline_conflict_id,
            )
            .await
            .expect("OPEN conflict must survive rebaseline")
            .lifecycle(),
        ConflictLifecycle::Open
    );
    conflicts
        .resolve(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            rebaseline_conflict_id,
            ConflictResolutionRequest::new(
                ConflictResolutionId::new(),
                ConflictResolutionAction::ApplyClientIntent,
                Some(renamed.revision()),
                None,
            ),
        )
        .await
        .expect("post-rebaseline fresh manual apply must succeed");
    let post_rebaseline = repository
        .find_node(root.id())
        .await
        .expect("post-rebaseline Node lookup must succeed")
        .expect("post-rebaseline Node must exist");
    assert_eq!(post_rebaseline.name().as_str(), "post-rebaseline-intent");

    let (_, stale_conflict_id) = create_stale_rename_conflict(
        &mutations,
        &inspection_pool,
        fixture,
        &post_rebaseline,
        "stale-resolution-intent",
    )
    .await;
    let server_head = library_sync_head(&inspection_pool, fixture.library_id).await;
    let server_result = mutations
        .submit(
            fixture.owner_id,
            fixture.device_two_id,
            fixture.library_id,
            client_mutation_request(
                ClientMutationId::new(),
                server_head.get(),
                ClientMutation::rename_node(
                    root.id(),
                    post_rebaseline.revision(),
                    name("concurrent-server-winner"),
                ),
            ),
        )
        .await
        .expect("concurrent server rename fixture must return");
    assert!(matches!(
        server_result,
        ClientMutationResult::Applied { .. }
    ));
    let events_before_stale_resolution =
        list_journal_events(&journal, fixture.owner_id, fixture.library_id).await;
    let stale_resolution_id = ConflictResolutionId::new();
    let stale_request = ConflictResolutionRequest::new(
        stale_resolution_id,
        ConflictResolutionAction::ApplyClientIntent,
        Some(post_rebaseline.revision()),
        None,
    );
    let stale_error = conflicts
        .resolve(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            stale_conflict_id,
            stale_request,
        )
        .await
        .expect_err("stale manual apply must fail explicitly");
    assert!(matches!(
        stale_error,
        ConflictManagementError::ResolutionConflict {
            replayed: false,
            ..
        }
    ));
    assert_eq!(
        conflicts
            .detail(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                stale_conflict_id,
            )
            .await
            .expect("stale conflict must remain inspectable")
            .lifecycle(),
        ConflictLifecycle::Open
    );
    assert_eq!(
        list_journal_events(&journal, fixture.owner_id, fixture.library_id).await,
        events_before_stale_resolution
    );
    assert!(matches!(
        conflicts
            .resolve(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                stale_conflict_id,
                stale_request,
            )
            .await,
        Err(ConflictManagementError::ResolutionConflict { replayed: true, .. })
    ));

    let purge_file = Node::new_child(
        NodeId::new(),
        fixture.library_id,
        &post_rebaseline,
        NodeKind::File,
        name("prompt-35-purged-target"),
        timestamp("2026-08-27T00:00:00.123456Z"),
    )
    .expect("purged conflict fixture must be valid");
    repository
        .insert_node(&purge_file)
        .await
        .expect("purged conflict fixture must persist");
    let metadata = FileMetadataService::new(pool.clone());
    let trashed = metadata
        .delete_node(fixture.owner_id, purge_file.id(), purge_file.revision())
        .await
        .expect("purged conflict fixture must enter Trash");
    sqlx::query(
        "UPDATE nodes SET trashed_at = CURRENT_TIMESTAMP - INTERVAL '31 days'
         WHERE id = $1 AND library_id = $2",
    )
    .bind(purge_file.id().into_uuid())
    .bind(fixture.library_id.into_uuid())
    .execute(&inspection_pool)
    .await
    .expect("purged conflict fixture age update must succeed");
    let retention = TrashRetentionService::new(pool.clone(), TrashRetentionPolicy::default());
    let purging = retention
        .begin_node_purge(fixture.owner_id, purge_file.id(), trashed.revision())
        .await
        .expect("purged conflict fixture must enter PURGING");
    retention
        .execute_metadata_purge(fixture.owner_id, purge_file.id(), purging.revision())
        .await
        .expect("purged conflict fixture must be deleted");
    let purge_head = library_sync_head(&inspection_pool, fixture.library_id).await;
    let purged_result = mutations
        .submit(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            client_mutation_request(
                ClientMutationId::new(),
                purge_head.get(),
                ClientMutation::trash_node(purge_file.id(), purging.revision()),
            ),
        )
        .await
        .expect("purged intent must become a durable conflict");
    let purged_conflict_id = conflict_id(&purged_result);
    assert_eq!(
        conflict_reason(&purged_result),
        MutationConflictReason::ResourcePurged
    );
    assert!(
        repository
            .find_node(purge_file.id())
            .await
            .expect("purged Node lookup must succeed")
            .is_none()
    );
    let purged_apply = conflicts
        .resolve(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            purged_conflict_id,
            ConflictResolutionRequest::new(
                ConflictResolutionId::new(),
                ConflictResolutionAction::ApplyClientIntent,
                Some(purging.revision()),
                None,
            ),
        )
        .await;
    assert!(matches!(
        purged_apply,
        Err(ConflictManagementError::ResolutionConflict {
            conflict,
            ..
        }) if conflict.reason() == MutationConflictReason::ResourcePurged
    ));
    assert!(
        repository
            .find_node(purge_file.id())
            .await
            .expect("purged Node recheck must succeed")
            .is_none()
    );
    conflicts
        .resolve(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            purged_conflict_id,
            ConflictResolutionRequest::new(
                ConflictResolutionId::new(),
                ConflictResolutionAction::AcceptServer,
                None,
                None,
            ),
        )
        .await
        .expect("purged-resource conflict may be explicitly accepted");

    let before_invalid = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM sync_conflicts")
        .fetch_one(&inspection_pool)
        .await
        .expect("pre-invalid conflict count must succeed");
    let current = repository
        .find_node(root.id())
        .await
        .expect("current root lookup must succeed")
        .expect("current root must exist");
    assert!(matches!(
        mutations
            .submit(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                ClientMutationRequest::new(
                    ClientMutationId::new(),
                    Sequence::new(2),
                    library_sync_head(&inspection_pool, fixture.library_id).await,
                    ClientMutation::rename_node(
                        current.id(),
                        current.revision(),
                        name("wrong-epoch-no-conflict"),
                    ),
                ),
            )
            .await,
        Err(ClientMutationError::RebaselineRequired { .. })
    ));
    let after_invalid = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM sync_conflicts")
        .fetch_one(&inspection_pool)
        .await
        .expect("post-invalid conflict count must succeed");
    assert_eq!(before_invalid, after_invalid);

    let column_names = sqlx::query_scalar::<_, String>(
        "SELECT column_name
         FROM information_schema.columns
         WHERE table_name IN ('sync_conflicts', 'sync_conflict_resolutions')",
    )
    .fetch_all(&inspection_pool)
    .await
    .expect("conflict column audit must succeed")
    .join(" ")
    .to_ascii_lowercase();
    for forbidden in [
        "storage_key",
        "replica_locator",
        "filesystem_path",
        "staging_handle",
        "backend_credential",
        "session_secret",
        "csrf_secret",
        "hmac_key",
        "raw_request_json",
    ] {
        assert!(!column_names.contains(forbidden));
    }

    inspection_pool.close().await;
    pool.close().await;
}

/// Real PostgreSQL fencing for Prompt 35. Every pair is bounded by a timeout;
/// the namespace advisory guard supplies one lock order across Prompt 34,
/// manual apply, move, and purge paths.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_sync_conflict_resolution_is_fenced_race_safe_and_replayable() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable test database");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    MigrationRunner::new()
        .run(&pool)
        .await
        .expect("Prompt 35 race migrations must succeed");
    let inspection_pool = PgPool::connect(&url)
        .await
        .expect("conflict race inspection connection must succeed");
    let fixture = insert_client_mutation_fixture(&pool).await;
    let repository = DomainRepository::new(&pool);
    let mutations = ClientMutationService::new(pool.clone());
    let conflicts = ConflictManagementService::new(pool.clone());
    let journal = ChangeJournalService::new(pool.clone());
    let root = repository
        .find_node(fixture.root_id)
        .await
        .expect("race root lookup must succeed")
        .expect("race root must exist");

    // APPLY vs APPLY: exactly one decision reaches canonical metadata.
    let (_, apply_race_conflict_id) = create_stale_rename_conflict(
        &mutations,
        &inspection_pool,
        fixture,
        &root,
        "apply-race-winner",
    )
    .await;
    let apply_a = ConflictResolutionRequest::new(
        ConflictResolutionId::new(),
        ConflictResolutionAction::ApplyClientIntent,
        Some(root.revision()),
        None,
    );
    let apply_b = ConflictResolutionRequest::new(
        ConflictResolutionId::new(),
        ConflictResolutionAction::ApplyClientIntent,
        Some(root.revision()),
        None,
    );
    let (apply_result_a, apply_result_b) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(
            conflicts.resolve(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                apply_race_conflict_id,
                apply_a,
            ),
            conflicts.resolve(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                apply_race_conflict_id,
                apply_b,
            )
        )
    })
    .await
    .expect("two-APPLY race must not deadlock");
    assert_eq!(
        usize::from(apply_result_a.is_ok()) + usize::from(apply_result_b.is_ok()),
        1
    );
    let apply_loser = if apply_result_a.is_err() {
        apply_result_a
    } else {
        apply_result_b
    };
    assert_eq!(
        apply_loser,
        Err(ConflictManagementError::ConflictNotOpen {
            lifecycle: ConflictLifecycle::Resolved,
        })
    );
    assert_eq!(
        list_journal_events(&journal, fixture.owner_id, fixture.library_id)
            .await
            .len(),
        1
    );

    // Original mutation replay vs resolution: replay retains the exact
    // original conflict ID while the control-plane lifecycle terminalizes.
    let current = repository
        .find_node(root.id())
        .await
        .expect("post-apply root lookup must succeed")
        .expect("post-apply root must exist");
    let (replay_request, replay_conflict_id) = create_stale_rename_conflict(
        &mutations,
        &inspection_pool,
        fixture,
        &current,
        "replay-race-intent",
    )
    .await;
    let (original_replay, accepted) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(
            mutations.submit(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                replay_request,
            ),
            conflicts.resolve(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                replay_conflict_id,
                ConflictResolutionRequest::new(
                    ConflictResolutionId::new(),
                    ConflictResolutionAction::AcceptServer,
                    None,
                    None,
                ),
            )
        )
    })
    .await
    .expect("original replay vs resolution must not deadlock");
    let original_replay = original_replay.expect("original mutation replay must return");
    assert_eq!(conflict_id(&original_replay), replay_conflict_id);
    assert!(original_replay.replayed());
    assert!(matches!(
        accepted,
        Ok(ConflictResolutionResult::AcceptedServer { .. })
    ));
    let replay_conflict_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sync_conflicts
         WHERE original_client_mutation_id = $1",
    )
    .bind(original_replay.mutation_id().into_uuid())
    .fetch_one(&inspection_pool)
    .await
    .expect("original replay conflict count must succeed");
    assert_eq!(replay_conflict_rows, 1);

    // ACCEPT_SERVER vs APPLY_CLIENT_INTENT: only one terminal decision wins;
    // accept contributes no event and apply contributes exactly one.
    let current = repository
        .find_node(root.id())
        .await
        .expect("accept/apply root lookup must succeed")
        .expect("accept/apply root must exist");
    let (_, decision_conflict_id) = create_stale_rename_conflict(
        &mutations,
        &inspection_pool,
        fixture,
        &current,
        "accept-apply-race-intent",
    )
    .await;
    let event_count_before = list_journal_events(&journal, fixture.owner_id, fixture.library_id)
        .await
        .len();
    let (accept_result, apply_result) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(
            conflicts.resolve(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                decision_conflict_id,
                ConflictResolutionRequest::new(
                    ConflictResolutionId::new(),
                    ConflictResolutionAction::AcceptServer,
                    None,
                    None,
                ),
            ),
            conflicts.resolve(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                decision_conflict_id,
                ConflictResolutionRequest::new(
                    ConflictResolutionId::new(),
                    ConflictResolutionAction::ApplyClientIntent,
                    Some(current.revision()),
                    None,
                ),
            )
        )
    })
    .await
    .expect("accept/apply race must not deadlock");
    assert_eq!(
        usize::from(accept_result.is_ok()) + usize::from(apply_result.is_ok()),
        1
    );
    let decision_detail = conflicts
        .detail(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            decision_conflict_id,
        )
        .await
        .expect("terminal decision detail must load");
    let event_count_after = list_journal_events(&journal, fixture.owner_id, fixture.library_id)
        .await
        .len();
    match decision_detail.lifecycle() {
        ConflictLifecycle::Dismissed => assert_eq!(event_count_after, event_count_before),
        ConflictLifecycle::Resolved => assert_eq!(event_count_after, event_count_before + 1),
        ConflictLifecycle::Open => panic!("accept/apply race must terminalize the conflict"),
    }

    // Force a normal server rename ahead of manual apply in PostgreSQL's
    // advisory-lock queue. The resolution must persist STALE and leave OPEN.
    let current = repository
        .find_node(root.id())
        .await
        .expect("rename race root lookup must succeed")
        .expect("rename race root must exist");
    let (_, rename_conflict_id) = create_stale_rename_conflict(
        &mutations,
        &inspection_pool,
        fixture,
        &current,
        "resolution-after-server-rename",
    )
    .await;
    let mut blocker = inspection_pool
        .begin()
        .await
        .expect("rename race blocker transaction must begin");
    sqlx::query(
        "SELECT pg_advisory_xact_lock(
            hashtextextended($1::TEXT || ':' || $2::UUID::TEXT, 0)
         )",
    )
    .bind("synveil:library-namespace:v1")
    .bind(fixture.library_id.into_uuid())
    .execute(&mut *blocker)
    .await
    .expect("rename race blocker must acquire namespace guard");
    let server_mutations = mutations.clone();
    let server_head = library_sync_head(&inspection_pool, fixture.library_id).await;
    let server_request = client_mutation_request(
        ClientMutationId::new(),
        server_head.get(),
        ClientMutation::rename_node(
            current.id(),
            current.revision(),
            name("queued-server-rename"),
        ),
    );
    let server_task = tokio::spawn(async move {
        server_mutations
            .submit(
                fixture.owner_id,
                fixture.device_two_id,
                fixture.library_id,
                server_request,
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(25)).await;
    let resolution_service = conflicts.clone();
    let resolution_task = tokio::spawn(async move {
        resolution_service
            .resolve(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                rename_conflict_id,
                ConflictResolutionRequest::new(
                    ConflictResolutionId::new(),
                    ConflictResolutionAction::ApplyClientIntent,
                    Some(current.revision()),
                    None,
                ),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(25)).await;
    blocker
        .commit()
        .await
        .expect("rename race blocker must release namespace guard");
    let (server_result, resolution_result) = tokio::time::timeout(Duration::from_secs(10), async {
        (
            server_task.await.expect("server rename task must join"),
            resolution_task
                .await
                .expect("rename resolution task must join"),
        )
    })
    .await
    .expect("resolution vs rename race must not deadlock");
    assert!(matches!(
        server_result,
        Ok(ClientMutationResult::Applied { .. })
    ));
    assert!(matches!(
        resolution_result,
        Err(ConflictManagementError::ResolutionConflict { .. })
    ));
    assert_eq!(
        conflicts
            .detail(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                rename_conflict_id,
            )
            .await
            .expect("rename race conflict detail must load")
            .lifecycle(),
        ConflictLifecycle::Open
    );

    // Force a server move ahead of the original move intent's manual apply.
    let root = repository
        .find_node(root.id())
        .await
        .expect("move race root lookup must succeed")
        .expect("move race root must exist");
    let observed_at = timestamp("2026-08-27T00:00:00.123456Z");
    let destination_a = Node::new_child(
        NodeId::new(),
        fixture.library_id,
        &root,
        NodeKind::Directory,
        name("move-resolution-destination"),
        observed_at,
    )
    .expect("move destination A must be valid");
    let destination_b = Node::new_child(
        NodeId::new(),
        fixture.library_id,
        &root,
        NodeKind::Directory,
        name("move-server-destination"),
        observed_at,
    )
    .expect("move destination B must be valid");
    let movable = Node::new_child(
        NodeId::new(),
        fixture.library_id,
        &root,
        NodeKind::File,
        name("move-race-node"),
        observed_at,
    )
    .expect("move race node must be valid");
    for node in [&destination_a, &destination_b, &movable] {
        repository
            .insert_node(node)
            .await
            .expect("move race Node must persist");
    }
    let move_head = library_sync_head(&inspection_pool, fixture.library_id).await;
    let move_conflict_result = mutations
        .submit(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            client_mutation_request(
                ClientMutationId::new(),
                move_head.get(),
                ClientMutation::move_node(
                    movable.id(),
                    Revision::new(1),
                    destination_a.id(),
                    destination_a.revision(),
                ),
            ),
        )
        .await
        .expect("stale move must create a conflict");
    let move_conflict_id = conflict_id(&move_conflict_result);
    let mut blocker = inspection_pool
        .begin()
        .await
        .expect("move race blocker transaction must begin");
    sqlx::query(
        "SELECT pg_advisory_xact_lock(
            hashtextextended($1::TEXT || ':' || $2::UUID::TEXT, 0)
         )",
    )
    .bind("synveil:library-namespace:v1")
    .bind(fixture.library_id.into_uuid())
    .execute(&mut *blocker)
    .await
    .expect("move race blocker must acquire namespace guard");
    let server_mutations = mutations.clone();
    let move_head = library_sync_head(&inspection_pool, fixture.library_id).await;
    let movable_id = movable.id();
    let movable_revision = movable.revision();
    let destination_a_revision = destination_a.revision();
    let destination_b_id = destination_b.id();
    let destination_b_revision = destination_b.revision();
    let server_move = tokio::spawn(async move {
        server_mutations
            .submit(
                fixture.owner_id,
                fixture.device_two_id,
                fixture.library_id,
                client_mutation_request(
                    ClientMutationId::new(),
                    move_head.get(),
                    ClientMutation::move_node(
                        movable_id,
                        movable_revision,
                        destination_b_id,
                        destination_b_revision,
                    ),
                ),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(25)).await;
    let resolution_service = conflicts.clone();
    let move_resolution = tokio::spawn(async move {
        resolution_service
            .resolve(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                move_conflict_id,
                ConflictResolutionRequest::new(
                    ConflictResolutionId::new(),
                    ConflictResolutionAction::ApplyClientIntent,
                    Some(movable_revision),
                    Some(destination_a_revision),
                ),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(25)).await;
    blocker
        .commit()
        .await
        .expect("move race blocker must release namespace guard");
    let (server_move_result, move_resolution_result) =
        tokio::time::timeout(Duration::from_secs(10), async {
            (
                server_move.await.expect("server move task must join"),
                move_resolution
                    .await
                    .expect("move resolution task must join"),
            )
        })
        .await
        .expect("resolution vs move race must not deadlock");
    assert!(matches!(
        server_move_result,
        Ok(ClientMutationResult::Applied { .. })
    ));
    assert!(matches!(
        move_resolution_result,
        Err(ConflictManagementError::ResolutionConflict { .. })
    ));

    // Force purge-state transition ahead of restore apply. The fresh
    // precondition becomes stale and the conflict remains OPEN; completing
    // purge afterward cannot erase its evidence.
    let purge_target = Node::new_child(
        NodeId::new(),
        fixture.library_id,
        &root,
        NodeKind::File,
        name("purge-race-node"),
        observed_at,
    )
    .expect("purge race Node must be valid");
    repository
        .insert_node(&purge_target)
        .await
        .expect("purge race Node must persist");
    let metadata = FileMetadataService::new(pool.clone());
    let trashed = metadata
        .delete_node(fixture.owner_id, purge_target.id(), purge_target.revision())
        .await
        .expect("purge race Node must enter Trash");
    sqlx::query(
        "UPDATE nodes SET trashed_at = CURRENT_TIMESTAMP - INTERVAL '31 days'
         WHERE id = $1 AND library_id = $2",
    )
    .bind(purge_target.id().into_uuid())
    .bind(fixture.library_id.into_uuid())
    .execute(&inspection_pool)
    .await
    .expect("purge race age update must succeed");
    let purge_head = library_sync_head(&inspection_pool, fixture.library_id).await;
    let purge_conflict_result = mutations
        .submit(
            fixture.owner_id,
            fixture.device_one_id,
            fixture.library_id,
            client_mutation_request(
                ClientMutationId::new(),
                purge_head.get(),
                ClientMutation::restore_node(
                    purge_target.id(),
                    Revision::new(trashed.revision().get() + 1),
                    root.id(),
                    root.revision(),
                ),
            ),
        )
        .await
        .expect("stale restore must create purge race conflict");
    let purge_conflict_id = conflict_id(&purge_conflict_result);
    let mut blocker = inspection_pool
        .begin()
        .await
        .expect("purge race blocker transaction must begin");
    sqlx::query(
        "SELECT pg_advisory_xact_lock(
            hashtextextended($1::TEXT || ':' || $2::UUID::TEXT, 0)
         )",
    )
    .bind("synveil:library-namespace:v1")
    .bind(fixture.library_id.into_uuid())
    .execute(&mut *blocker)
    .await
    .expect("purge race blocker must acquire namespace guard");
    let retention = TrashRetentionService::new(pool.clone(), TrashRetentionPolicy::default());
    let purge_worker = retention.clone();
    let purge_target_id = purge_target.id();
    let trashed_revision = trashed.revision();
    let purge_parent_revision = root.revision();
    let begin_purge = tokio::spawn(async move {
        purge_worker
            .begin_node_purge(fixture.owner_id, purge_target_id, trashed_revision)
            .await
    });
    tokio::time::sleep(Duration::from_millis(25)).await;
    let resolution_service = conflicts.clone();
    let purge_resolution = tokio::spawn(async move {
        resolution_service
            .resolve(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                purge_conflict_id,
                ConflictResolutionRequest::new(
                    ConflictResolutionId::new(),
                    ConflictResolutionAction::ApplyClientIntent,
                    Some(trashed_revision),
                    Some(purge_parent_revision),
                ),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(25)).await;
    blocker
        .commit()
        .await
        .expect("purge race blocker must release namespace guard");
    let (purging, purge_resolution_result) = tokio::time::timeout(Duration::from_secs(10), async {
        (
            begin_purge
                .await
                .expect("begin-purge task must join")
                .expect("begin-purge server mutation must win"),
            purge_resolution
                .await
                .expect("purge resolution task must join"),
        )
    })
    .await
    .expect("resolution vs purge race must not deadlock");
    assert!(matches!(
        purge_resolution_result,
        Err(ConflictManagementError::ResolutionConflict { .. })
    ));
    assert_eq!(
        conflicts
            .detail(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                purge_conflict_id,
            )
            .await
            .expect("purge race conflict must remain inspectable")
            .lifecycle(),
        ConflictLifecycle::Open
    );
    retention
        .execute_metadata_purge(fixture.owner_id, purge_target_id, purging.revision())
        .await
        .expect("purge race metadata deletion must finish");
    assert!(
        conflicts
            .detail(
                fixture.owner_id,
                fixture.device_one_id,
                fixture.library_id,
                purge_conflict_id,
            )
            .await
            .is_ok()
    );

    let (operation_conflicts, durable_conflicts): (i64, i64) = sqlx::query_as(
        "SELECT
            (SELECT count(*) FROM device_mutation_operations WHERE outcome = 'CONFLICT'),
            (SELECT count(*) FROM sync_conflicts)",
    )
    .fetch_one(&inspection_pool)
    .await
    .expect("terminal conflict equivalence count must succeed");
    assert_eq!(operation_conflicts, durable_conflicts);

    inspection_pool.close().await;
    pool.close().await;
}
