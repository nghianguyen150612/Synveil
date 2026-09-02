use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::stream;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use synveil_core::{
    DedupDomainId, FileVersion, FileVersionId, GcWorkerConfig, GcWorkerRetryPolicy, Library,
    LibraryId, LogicalName, Node, NodeId, NodeKind, ObjectGcOperationId, ObjectGcPolicy, ObjectId,
    ObjectReference, ObjectReplicaId, Sha256Digest, Timestamp, UploadOperation, UploadSessionId,
    User, UserId, UserStatus,
};
use synveil_metadata::{
    DatabaseConfig, DatabasePool, DomainRepository, MigrationRunner, NewUploadSession,
    ObjectGcExecutionMetadataBackend, ObjectGcExecutionMetadataError, ObjectGcPlanResult,
    ObjectGcPlanningService, ObjectGcReplicaDirective, ObjectGcReplicaObservation,
    PostgresObjectGcExecutionRepository, PostgresObjectGcWorkerRepository,
    PostgresUploadRepository, UploadClaim, UploadDurabilityReceipt, UploadMetadataBackend,
    VersionRestoreService,
};
use synveil_object_store::{
    ByteRange, ByteStream, DeleteOutcome, DeleteReconciliation, IntegrityExpectation, ObjectKey,
    ObjectMetadata, ObjectRead, ObjectStore, ObjectStoreError, ObjectVersion, PromotionReceipt,
    PutRequest, StagedMetadata, StagingHandle, StagingProgress, StorageCapabilities, boxed_stream,
};
use synveil_storage::{
    GcWorker, GcWorkerCycleStatus, LocalFilesystemObjectStore, ObjectGcExecutionError,
    ObjectGcExecutionService, ObjectGcStepOutcome,
};
use uuid::Uuid;

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("synveil-physical-gc-{}", Uuid::now_v7()));
        fs::create_dir_all(&path).expect("create physical-GC test root");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Copy)]
enum AmbiguousDeleteMode {
    DeleteThenUnavailable,
    UnavailableWithoutDelete,
    Delegate,
}

/// Fault-injection adapter that retains the production local store for every
/// physical operation. It can lose exactly one delete response either after
/// the local delete committed or before the local delete began. `get` calls
/// are counted to prove GC never buffers object bytes to decide an outcome.
struct AmbiguousDeleteStore {
    inner: Arc<LocalFilesystemObjectStore>,
    next_delete: Mutex<AmbiguousDeleteMode>,
    get_calls: AtomicUsize,
    conditional_delete_calls: AtomicUsize,
}

impl AmbiguousDeleteStore {
    fn new(inner: Arc<LocalFilesystemObjectStore>, mode: AmbiguousDeleteMode) -> Self {
        Self {
            inner,
            next_delete: Mutex::new(mode),
            get_calls: AtomicUsize::new(0),
            conditional_delete_calls: AtomicUsize::new(0),
        }
    }

    fn take_delete_mode(&self) -> AmbiguousDeleteMode {
        let mut mode = self.next_delete.lock().expect("fault mode lock is healthy");
        let selected = *mode;
        *mode = AmbiguousDeleteMode::Delegate;
        selected
    }

    fn get_calls(&self) -> usize {
        self.get_calls.load(Ordering::SeqCst)
    }

    fn conditional_delete_calls(&self) -> usize {
        self.conditional_delete_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ObjectStore for AmbiguousDeleteStore {
    fn capabilities(&self) -> StorageCapabilities {
        self.inner.capabilities()
    }

    async fn put(&self, request: PutRequest) -> Result<ObjectMetadata, ObjectStoreError> {
        self.inner.put(request).await
    }

    async fn begin_staged_write(&self) -> Result<StagingHandle, ObjectStoreError> {
        self.inner.begin_staged_write().await
    }

    async fn write_staged(
        &self,
        handle: &StagingHandle,
        body: ByteStream,
        integrity: IntegrityExpectation,
    ) -> Result<StagedMetadata, ObjectStoreError> {
        self.inner.write_staged(handle, body, integrity).await
    }

    async fn append_staged(
        &self,
        handle: &StagingHandle,
        expected_offset: u64,
        chunk: Bytes,
        maximum_length: u64,
    ) -> Result<StagingProgress, ObjectStoreError> {
        self.inner
            .append_staged(handle, expected_offset, chunk, maximum_length)
            .await
    }

    async fn staging_progress(
        &self,
        handle: &StagingHandle,
    ) -> Result<StagingProgress, ObjectStoreError> {
        self.inner.staging_progress(handle).await
    }

    async fn finalize_staged(
        &self,
        handle: &StagingHandle,
        integrity: IntegrityExpectation,
    ) -> Result<StagedMetadata, ObjectStoreError> {
        self.inner.finalize_staged(handle, integrity).await
    }

    async fn promote_temp(
        &self,
        handle: &StagingHandle,
        destination: &ObjectKey,
    ) -> Result<PromotionReceipt, ObjectStoreError> {
        self.inner.promote_temp(handle, destination).await
    }

    async fn abort_staged(&self, handle: &StagingHandle) -> Result<(), ObjectStoreError> {
        self.inner.abort_staged(handle).await
    }

    async fn get(&self, key: &ObjectKey) -> Result<ObjectRead, ObjectStoreError> {
        self.get_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.get(key).await
    }

    async fn range_read(
        &self,
        key: &ObjectKey,
        range: ByteRange,
    ) -> Result<ObjectRead, ObjectStoreError> {
        self.inner.range_read(key, range).await
    }

    async fn exists(&self, key: &ObjectKey) -> Result<bool, ObjectStoreError> {
        self.inner.exists(key).await
    }

    async fn metadata(&self, key: &ObjectKey) -> Result<ObjectMetadata, ObjectStoreError> {
        self.inner.metadata(key).await
    }

    async fn delete(&self, key: &ObjectKey) -> Result<DeleteOutcome, ObjectStoreError> {
        self.inner.delete(key).await
    }

    async fn conditional_delete(
        &self,
        key: &ObjectKey,
        expected_version: &ObjectVersion,
    ) -> Result<DeleteOutcome, ObjectStoreError> {
        self.conditional_delete_calls.fetch_add(1, Ordering::SeqCst);
        match self.take_delete_mode() {
            AmbiguousDeleteMode::DeleteThenUnavailable => {
                self.inner.conditional_delete(key, expected_version).await?;
                Err(ObjectStoreError::StorageUnavailable)
            }
            AmbiguousDeleteMode::UnavailableWithoutDelete => {
                Err(ObjectStoreError::StorageUnavailable)
            }
            AmbiguousDeleteMode::Delegate => {
                self.inner.conditional_delete(key, expected_version).await
            }
        }
    }

    async fn reconcile_delete(
        &self,
        key: &ObjectKey,
    ) -> Result<DeleteReconciliation, ObjectStoreError> {
        self.inner.reconcile_delete(key).await
    }
}

fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse(value).expect("test timestamp is valid")
}

fn name(value: &str) -> LogicalName {
    LogicalName::new(value).expect("test logical name is valid")
}

fn digest(bytes: &[u8]) -> Sha256Digest {
    let hash = Sha256::digest(bytes);
    let mut value = [0_u8; 32];
    value.copy_from_slice(&hash);
    Sha256Digest::from_bytes(value)
}

fn body(bytes: &'static [u8]) -> synveil_object_store::ByteStream {
    boxed_stream(stream::iter(vec![Ok(Bytes::from_static(bytes))]))
}

async fn insert_replica(
    pool: &PgPool,
    store: &LocalFilesystemObjectStore,
    object: ObjectReference,
    key_value: &str,
    bytes: &'static [u8],
) -> (ObjectReplicaId, ObjectKey, ObjectVersion) {
    let key = ObjectKey::new(key_value).expect("test key is valid");
    let metadata = store
        .put(PutRequest::new(key.clone(), body(bytes)))
        .await
        .expect("physical replica must persist");
    assert_eq!(metadata.length(), object.plaintext_length());
    assert_eq!(metadata.sha256(), Some(&object.canonical_hash()));
    let version = metadata
        .version()
        .expect("local object has conditional-delete evidence")
        .clone();
    let replica_id = ObjectReplicaId::new();
    let observed_at = timestamp("2026-08-26T00:00:00.123456Z");
    sqlx::query(
        "INSERT INTO object_replicas
            (id, object_id, object_dedup_domain_id, backend_kind, storage_key,
             stored_length, stored_sha256, backend_version, state, created_at, verified_at)
         VALUES ($1, $2, $3, 'LOCAL_FILESYSTEM', $4, $5::NUMERIC, $6, $7,
                 'VERIFIED', $8, $8)",
    )
    .bind(replica_id.into_uuid())
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .bind(key.as_str())
    .bind(object.plaintext_length().to_string())
    .bind(object.canonical_hash().as_bytes())
    .bind(version.as_str())
    .bind(observed_at.as_offset_datetime())
    .execute(pool)
    .await
    .expect("replica metadata must persist");
    (replica_id, key, version)
}

async fn insert_candidate(pool: &PgPool, object: ObjectReference) {
    sqlx::query(
        "INSERT INTO object_gc_candidates
            (object_id, object_dedup_domain_id, unreferenced_at, source)
         VALUES ($1, $2, clock_timestamp() - INTERVAL '60 seconds',
                 'METADATA_PURGE')",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .execute(pool)
    .await
    .expect("physical-GC candidate must persist");
}

async fn ready_lease(
    planner: &ObjectGcPlanningService,
    pool: &PgPool,
    object: ObjectReference,
) -> synveil_metadata::ObjectGcLease {
    insert_candidate(pool, object).await;
    let lease = planner
        .claim_candidates(1)
        .await
        .expect("candidate claim must succeed")
        .pop()
        .expect("candidate must be claimed");
    assert_eq!(lease.object_id(), object.object_id());
    match planner
        .mark_ready_for_deletion(lease)
        .await
        .expect("candidate ready transition must succeed")
    {
        ObjectGcPlanResult::Valid(candidate) => candidate.lease().expect("READY retains lease"),
        ObjectGcPlanResult::Invalidated => panic!("unreferenced candidate was invalidated"),
    }
}

/// Prompt 28's physical integration gate deliberately requires an explicitly
/// disposable PostgreSQL database. One test owns the candidate queue so its
/// deterministic claim assertions cannot race another test process.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_and_local_physical_gc_is_fenced_restartable_and_reconciled() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable test database");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    MigrationRunner::new()
        .run(&pool)
        .await
        .expect("SQLx migrations must succeed");
    let inspection = PgPool::connect(&url)
        .await
        .expect("inspection connection must succeed");
    let repository = DomainRepository::new(&pool);
    let policy = ObjectGcPolicy::new(Duration::from_secs(1), Duration::from_secs(30), 1)
        .expect("focused physical-GC policy is valid");
    let planner = ObjectGcPlanningService::new(pool.clone(), policy);
    let root = TempRoot::new();
    let store = Arc::new(
        LocalFilesystemObjectStore::open(root.path()).expect("open production local adapter"),
    );
    let service = ObjectGcExecutionService::new(pool.clone(), policy, store.clone())
        .expect("physical-GC service configuration is valid");

    // A live LEASED capability is deliberately insufficient. Only the
    // metadata planner's READY transition can enter physical execution.
    let leased_only_object = ObjectReference::new(
        ObjectId::new(),
        DedupDomainId::new(),
        Sha256Digest::from_bytes([0x11; 32]),
        0,
    );
    repository
        .insert_object(leased_only_object, timestamp("2026-08-26T00:00:00.023456Z"))
        .await
        .expect("LEASED-only object persists");
    insert_candidate(&inspection, leased_only_object).await;
    let leased_only = planner
        .claim_candidates(1)
        .await
        .expect("LEASED-only candidate claim succeeds")
        .pop()
        .expect("LEASED-only candidate is claimed");
    assert_eq!(
        service.start_gc_execution(leased_only).await,
        Err(ObjectGcExecutionError::ReadyRequired)
    );
    let leased_only_ready = match planner
        .mark_ready_for_deletion(leased_only)
        .await
        .expect("LEASED-only candidate can be validated")
    {
        ObjectGcPlanResult::Valid(candidate) => candidate.lease().expect("READY lease exists"),
        ObjectGcPlanResult::Invalidated => panic!("LEASED-only candidate was invalidated"),
    };
    let leased_only_operation = service
        .start_gc_execution(leased_only_ready)
        .await
        .expect("READY candidate starts");
    service
        .complete_gc_execution(leased_only_operation.operation_id(), leased_only_ready)
        .await
        .expect("READY zero-replica operation completes");

    // Durable operation before the first side effect, deterministic replica
    // order, partial progress, lease expiry/reclaim, reconnect, and completion.
    const MULTI_BYTES: &[u8] = b"two deterministic physical replicas";
    let multi = ObjectReference::new(
        ObjectId::new(),
        DedupDomainId::new(),
        digest(MULTI_BYTES),
        MULTI_BYTES.len() as u64,
    );
    repository
        .insert_object(multi, timestamp("2026-08-26T00:00:00.123456Z"))
        .await
        .expect("multi-replica object must persist");
    let (_replica_a, key_a, _version_a) = insert_replica(
        &inspection,
        &store,
        multi,
        "objects/v1/gc_multi_a",
        MULTI_BYTES,
    )
    .await;
    let (_replica_b, key_b, _version_b) = insert_replica(
        &inspection,
        &store,
        multi,
        "objects/v1/gc_multi_b",
        MULTI_BYTES,
    )
    .await;
    let ready = ready_lease(&planner, &inspection, multi).await;
    let operation = service
        .start_gc_execution(ready)
        .await
        .expect("physical-GC operation must start");
    assert_eq!(operation.replica_count(), 2);
    assert_eq!(operation.deleted_replica_count(), 0);
    assert!(store.exists(&key_a).await.expect("first replica exists"));
    assert!(store.exists(&key_b).await.expect("second replica exists"));
    let durable_counts = sqlx::query_as::<_, (i64, i64, String)>(
        "SELECT
            (SELECT count(*) FROM object_gc_operations WHERE operation_id = $1),
            (SELECT count(*) FROM object_gc_replica_actions WHERE operation_id = $1),
            (SELECT lifecycle_state FROM objects WHERE id = $2)",
    )
    .bind(operation.operation_id().into_uuid())
    .bind(multi.object_id().into_uuid())
    .fetch_one(&inspection)
    .await
    .expect("durable operation evidence query succeeds");
    assert_eq!(durable_counts, (1, 2, "GC_DELETING".to_owned()));

    let first = service
        .delete_next_replica(operation.operation_id(), ready)
        .await
        .expect("first replica deletion must succeed");
    assert_eq!(first.outcome(), ObjectGcStepOutcome::ReplicaDeleted);
    assert!(!store.exists(&key_a).await.expect("first key is absent"));
    assert!(store.exists(&key_b).await.expect("second key remains"));
    assert_eq!(first.operation().deleted_replica_count(), 1);
    let partial = sqlx::query_as::<_, (i64, i64)>(
        "SELECT
            (SELECT count(*) FROM object_replicas WHERE object_id = $1),
            (SELECT count(*) FROM object_gc_replica_actions
             WHERE operation_id = $2 AND state = 'DELETED')",
    )
    .bind(multi.object_id().into_uuid())
    .bind(operation.operation_id().into_uuid())
    .fetch_one(&inspection)
    .await
    .expect("partial progress query succeeds");
    assert_eq!(partial, (1, 1));

    sqlx::query(
        "UPDATE object_gc_candidates
         SET lease_acquired_at = clock_timestamp() - INTERVAL '2 seconds',
             lease_expires_at = clock_timestamp() - INTERVAL '1 second'
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(multi.object_id().into_uuid())
    .bind(multi.dedup_domain_id().into_uuid())
    .execute(&inspection)
    .await
    .expect("test lease expiry must persist");
    assert_eq!(
        service
            .delete_next_replica(operation.operation_id(), first.lease())
            .await,
        Err(ObjectGcExecutionError::LeaseExpired)
    );
    let recovery_repository = PostgresObjectGcWorkerRepository::new(pool.clone(), policy);
    let reclaimed = recovery_repository
        .claim_recoverable_operations(1)
        .await
        .expect("expired physical-GC operation is reclaimable")
        .pop()
        .expect("incomplete operation is reclaimed")
        .lease();
    assert!(reclaimed.lease_generation() > first.lease().lease_generation());
    let reclaimed_ready = match planner
        .mark_ready_for_deletion(reclaimed)
        .await
        .expect("reclaimed candidate becomes READY")
    {
        ObjectGcPlanResult::Valid(candidate) => candidate.lease().expect("READY lease exists"),
        ObjectGcPlanResult::Invalidated => panic!("reclaimed operation was invalidated"),
    };
    assert_eq!(
        service.resume_gc_execution(first.lease()).await,
        Err(ObjectGcExecutionError::StaleLease)
    );
    service
        .resume_gc_execution(reclaimed_ready)
        .await
        .expect("new lease rebinds the incomplete operation");

    drop(service);
    drop(store);
    let reconnected_pool = DatabasePool::connect(&config)
        .await
        .expect("reconnected metadata pool succeeds");
    let reopened_store = Arc::new(
        LocalFilesystemObjectStore::open(root.path()).expect("reopen production local adapter"),
    );
    let reconnected =
        ObjectGcExecutionService::new(reconnected_pool.clone(), policy, reopened_store.clone())
            .expect("reconnected service is valid");
    reconnected
        .resume_gc_execution(reclaimed_ready)
        .await
        .expect("reconnect resumes durable operation");
    let second = reconnected
        .delete_next_replica(operation.operation_id(), reclaimed_ready)
        .await
        .expect("remaining replica deletion succeeds");
    assert_eq!(second.outcome(), ObjectGcStepOutcome::ReplicaDeleted);
    assert!(
        !reopened_store
            .exists(&key_b)
            .await
            .expect("second key is absent")
    );
    // Simulate the final crash boundary: every action is durably DELETED, but
    // candidate/Object metadata has not yet been removed. A fresh pool and
    // adapter instance must finish that exact operation deterministically.
    let completion_pool = DatabasePool::connect(&config)
        .await
        .expect("post-absence reconnect succeeds");
    let completion_store = Arc::new(
        LocalFilesystemObjectStore::open(root.path())
            .expect("post-absence local adapter reconnect succeeds"),
    );
    let completion_service =
        ObjectGcExecutionService::new(completion_pool.clone(), policy, completion_store)
            .expect("post-absence completion service is valid");
    let completed = completion_service
        .complete_gc_execution(operation.operation_id(), second.lease())
        .await
        .expect("fully absent object metadata completes");
    assert_eq!(completed.outcome(), ObjectGcStepOutcome::Completed);
    assert_eq!(completed.operation().deleted_replica_count(), 2);
    let terminal_counts = sqlx::query_as::<_, (i64, i64, i64, String)>(
        "SELECT
            (SELECT count(*) FROM objects WHERE id = $1),
            (SELECT count(*) FROM object_replicas WHERE object_id = $1),
            (SELECT count(*) FROM object_gc_candidates WHERE object_id = $1),
            (SELECT state FROM object_gc_operations WHERE operation_id = $2)",
    )
    .bind(multi.object_id().into_uuid())
    .bind(operation.operation_id().into_uuid())
    .fetch_one(&inspection)
    .await
    .expect("terminal metadata query succeeds");
    assert_eq!(terminal_counts, (0, 0, 0, "COMPLETED".to_owned()));
    assert_eq!(
        completion_service
            .complete_gc_execution(operation.operation_id(), second.lease())
            .await
            .expect("completed replay is deterministic")
            .operation(),
        completed.operation()
    );
    drop(completion_service);
    completion_pool.close().await;

    // Crash after physical success but before DB outcome persistence. The next
    // worker inspects exact-key absence and converges without a second guess.
    const CRASH_BYTES: &[u8] = b"crash after delete before metadata outcome";
    let crash_object = ObjectReference::new(
        ObjectId::new(),
        DedupDomainId::new(),
        digest(CRASH_BYTES),
        CRASH_BYTES.len() as u64,
    );
    repository
        .insert_object(crash_object, timestamp("2026-08-26T00:00:01.123456Z"))
        .await
        .expect("crash object must persist");
    let (_crash_replica, crash_key, crash_version) = insert_replica(
        &inspection,
        &reopened_store,
        crash_object,
        "objects/v1/gc_crash_boundary",
        CRASH_BYTES,
    )
    .await;
    let crash_ready = ready_lease(&planner, &inspection, crash_object).await;
    let crash_operation = reconnected
        .start_gc_execution(crash_ready)
        .await
        .expect("crash operation starts");
    let metadata_backend =
        PostgresObjectGcExecutionRepository::new(reconnected_pool.clone(), policy);
    let crash_renewed = metadata_backend
        .renew_execution_lease(crash_ready)
        .await
        .expect("crash action lease renews");
    let crash_action = match metadata_backend
        .fence_next_replica(crash_operation.operation_id(), crash_renewed)
        .await
        .expect("crash action fence persists")
    {
        ObjectGcReplicaDirective::Delete(action) => action,
        directive => panic!("unexpected crash directive: {directive:?}"),
    };
    assert_eq!(
        reopened_store
            .conditional_delete(&crash_key, &crash_version)
            .await,
        Ok(DeleteOutcome::Deleted)
    );
    // Deliberately omit record_replica_observation: this is the crash.
    let reconciled = reconnected
        .reconcile_replica(crash_operation.operation_id(), crash_renewed)
        .await
        .expect("reconnected worker reconciles absent key");
    assert_eq!(
        reconciled.outcome(),
        ObjectGcStepOutcome::ReplicaReconciledAbsent
    );
    assert_eq!(reconciled.replica_id(), Some(crash_action.replica_id()));
    reconnected
        .complete_gc_execution(crash_operation.operation_id(), reconciled.lease())
        .await
        .expect("reconciled crash operation completes");

    // A fenced action that never reached storage reconciles as present and is
    // retried without advancing to another replica.
    const PRESENT_BYTES: &[u8] = b"crash before external delete";
    let present_object = ObjectReference::new(
        ObjectId::new(),
        DedupDomainId::new(),
        digest(PRESENT_BYTES),
        PRESENT_BYTES.len() as u64,
    );
    repository
        .insert_object(present_object, timestamp("2026-08-26T00:00:02.123456Z"))
        .await
        .expect("present object persists");
    let (_present_replica, present_key, _present_version) = insert_replica(
        &inspection,
        &reopened_store,
        present_object,
        "objects/v1/gc_present_boundary",
        PRESENT_BYTES,
    )
    .await;
    let present_ready = ready_lease(&planner, &inspection, present_object).await;
    let present_operation = reconnected
        .start_gc_execution(present_ready)
        .await
        .expect("present operation starts");
    let present_renewed = metadata_backend
        .renew_execution_lease(present_ready)
        .await
        .expect("present action lease renews");
    match metadata_backend
        .fence_next_replica(present_operation.operation_id(), present_renewed)
        .await
        .expect("present action fence persists")
    {
        ObjectGcReplicaDirective::Delete(_) => {}
        directive => panic!("unexpected present directive: {directive:?}"),
    }
    let still_present = reconnected
        .reconcile_replica(present_operation.operation_id(), present_renewed)
        .await
        .expect("present replica reconciles retryably");
    assert_eq!(
        still_present.outcome(),
        ObjectGcStepOutcome::ReplicaStillPresent
    );
    assert!(
        reopened_store
            .exists(&present_key)
            .await
            .expect("present key remains")
    );
    let present_deleted = reconnected
        .delete_next_replica(present_operation.operation_id(), still_present.lease())
        .await
        .expect("retry deletes only the remaining replica");
    assert_eq!(
        present_deleted.outcome(),
        ObjectGcStepOutcome::ReplicaDeleted
    );
    reconnected
        .complete_gc_execution(present_operation.operation_id(), present_deleted.lease())
        .await
        .expect("present operation completes");

    // A worker that fenced an action must lose both authorization and result
    // persistence when its candidate lease is reclaimed. This proves that a
    // stale process cannot report a later generation's replica as deleted.
    const STALE_FENCE_BYTES: &[u8] = b"stale worker must not persist an action result";
    let stale_fence_object = ObjectReference::new(
        ObjectId::new(),
        DedupDomainId::new(),
        digest(STALE_FENCE_BYTES),
        STALE_FENCE_BYTES.len() as u64,
    );
    repository
        .insert_object(stale_fence_object, timestamp("2026-08-26T00:00:02.173456Z"))
        .await
        .expect("stale-fence object persists");
    let (stale_fence_replica, stale_fence_key, _) = insert_replica(
        &inspection,
        &reopened_store,
        stale_fence_object,
        "objects/v1/gc_stale_fence",
        STALE_FENCE_BYTES,
    )
    .await;
    let stale_fence_ready = ready_lease(&planner, &inspection, stale_fence_object).await;
    let stale_fence_operation = reconnected
        .start_gc_execution(stale_fence_ready)
        .await
        .expect("stale-fence operation starts");
    let stale_fence_lease = metadata_backend
        .renew_execution_lease(stale_fence_ready)
        .await
        .expect("stale-fence lease renews");
    let stale_fence_action = match metadata_backend
        .fence_next_replica(stale_fence_operation.operation_id(), stale_fence_lease)
        .await
        .expect("stale-fence action persists")
    {
        ObjectGcReplicaDirective::Delete(action) => action,
        directive => panic!("unexpected stale-fence directive: {directive:?}"),
    };
    sqlx::query(
        "UPDATE object_gc_candidates
         SET lease_acquired_at = clock_timestamp() - INTERVAL '2 seconds',
             lease_expires_at = clock_timestamp() - INTERVAL '1 second'
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(stale_fence_object.object_id().into_uuid())
    .bind(stale_fence_object.dedup_domain_id().into_uuid())
    .execute(&inspection)
    .await
    .expect("stale-fence lease expiry persists");
    let reclaimed_stale_fence = recovery_repository
        .claim_recoverable_operations(1)
        .await
        .expect("stale-fence operation is reclaimable")
        .pop()
        .expect("stale-fence operation is reclaimed")
        .lease();
    let reclaimed_stale_fence_ready = match planner
        .mark_ready_for_deletion(reclaimed_stale_fence)
        .await
        .expect("reclaimed stale-fence candidate becomes READY")
    {
        ObjectGcPlanResult::Valid(candidate) => candidate.lease().expect("READY lease exists"),
        ObjectGcPlanResult::Invalidated => panic!("stale-fence candidate was invalidated"),
    };
    reconnected
        .resume_gc_execution(reclaimed_stale_fence_ready)
        .await
        .expect("new generation rebinds the stale-fence operation");
    assert_eq!(
        metadata_backend
            .authorize_replica_delete(&stale_fence_action, stale_fence_lease)
            .await,
        Err(ObjectGcExecutionMetadataError::StaleLease)
    );
    assert_eq!(
        metadata_backend
            .record_replica_observation(
                &stale_fence_action,
                stale_fence_lease,
                ObjectGcReplicaObservation::Deleted,
                None,
            )
            .await,
        Err(ObjectGcExecutionMetadataError::StaleLease)
    );
    assert!(
        reopened_store
            .exists(&stale_fence_key)
            .await
            .expect("stale worker did not delete bytes")
    );
    let stale_fence_metadata = sqlx::query_as::<_, (i64, String)>(
        "SELECT
            (SELECT count(*) FROM object_replicas WHERE id = $1),
            (SELECT state FROM object_gc_replica_actions
             WHERE operation_id = $2 AND replica_id = $1)",
    )
    .bind(stale_fence_replica.into_uuid())
    .bind(stale_fence_operation.operation_id().into_uuid())
    .fetch_one(&inspection)
    .await
    .expect("stale-fence metadata remains inspectable");
    assert_eq!(stale_fence_metadata, (1, "DELETE_FENCED".to_owned()));
    let stale_fence_reconciled = reconnected
        .reconcile_replica(
            stale_fence_operation.operation_id(),
            reclaimed_stale_fence_ready,
        )
        .await
        .expect("new worker reconciles the prior action before retrying");
    assert_eq!(
        stale_fence_reconciled.outcome(),
        ObjectGcStepOutcome::ReplicaStillPresent
    );
    let stale_fence_deleted = reconnected
        .delete_next_replica(
            stale_fence_operation.operation_id(),
            stale_fence_reconciled.lease(),
        )
        .await
        .expect("new worker owns the retry after reconciliation");
    assert_eq!(
        stale_fence_deleted.outcome(),
        ObjectGcStepOutcome::ReplicaDeleted
    );
    reconnected
        .complete_gc_execution(
            stale_fence_operation.operation_id(),
            stale_fence_deleted.lease(),
        )
        .await
        .expect("stale-fence operation completes after the current worker retry");

    // An unavailable response after the provider committed the delete is not
    // guessed from the response: exact-key reconciliation proves absence. GC
    // uses head evidence only and never calls the byte-streaming `get` method.
    const AMBIGUOUS_ABSENT_BYTES: &[u8] = b"delete committed but response was lost";
    let ambiguous_absent_object = ObjectReference::new(
        ObjectId::new(),
        DedupDomainId::new(),
        digest(AMBIGUOUS_ABSENT_BYTES),
        AMBIGUOUS_ABSENT_BYTES.len() as u64,
    );
    repository
        .insert_object(
            ambiguous_absent_object,
            timestamp("2026-08-26T00:00:02.223456Z"),
        )
        .await
        .expect("ambiguous-absent object persists");
    let (_ambiguous_absent_replica, ambiguous_absent_key, _) = insert_replica(
        &inspection,
        &reopened_store,
        ambiguous_absent_object,
        "objects/v1/gc_ambiguous_absent",
        AMBIGUOUS_ABSENT_BYTES,
    )
    .await;
    let ambiguous_absent_ready = ready_lease(&planner, &inspection, ambiguous_absent_object).await;
    let ambiguous_absent_store = Arc::new(AmbiguousDeleteStore::new(
        reopened_store.clone(),
        AmbiguousDeleteMode::DeleteThenUnavailable,
    ));
    let ambiguous_absent_service = ObjectGcExecutionService::new(
        reconnected_pool.clone(),
        policy,
        ambiguous_absent_store.clone(),
    )
    .expect("ambiguous-absent service is valid");
    let ambiguous_absent_operation = ambiguous_absent_service
        .start_gc_execution(ambiguous_absent_ready)
        .await
        .expect("ambiguous-absent operation starts");
    let ambiguous_absent = ambiguous_absent_service
        .delete_next_replica(
            ambiguous_absent_operation.operation_id(),
            ambiguous_absent_ready,
        )
        .await
        .expect("confirmed absence converges after a lost delete response");
    assert_eq!(
        ambiguous_absent.outcome(),
        ObjectGcStepOutcome::ReplicaReconciledAbsent
    );
    assert!(
        !reopened_store
            .exists(&ambiguous_absent_key)
            .await
            .expect("ambiguous-absent key is gone")
    );
    assert_eq!(ambiguous_absent_store.get_calls(), 0);
    ambiguous_absent_service
        .complete_gc_execution(
            ambiguous_absent_operation.operation_id(),
            ambiguous_absent.lease(),
        )
        .await
        .expect("ambiguous-absent operation completes");

    // If the same unavailable response occurs before the provider mutation,
    // exact metadata proves the replica is still present and makes it
    // retryable. A later bounded call deletes that same replica.
    const AMBIGUOUS_PRESENT_BYTES: &[u8] = b"delete never started and response was lost";
    let ambiguous_present_object = ObjectReference::new(
        ObjectId::new(),
        DedupDomainId::new(),
        digest(AMBIGUOUS_PRESENT_BYTES),
        AMBIGUOUS_PRESENT_BYTES.len() as u64,
    );
    repository
        .insert_object(
            ambiguous_present_object,
            timestamp("2026-08-26T00:00:02.323456Z"),
        )
        .await
        .expect("ambiguous-present object persists");
    let (_ambiguous_present_replica, ambiguous_present_key, _) = insert_replica(
        &inspection,
        &reopened_store,
        ambiguous_present_object,
        "objects/v1/gc_ambiguous_present",
        AMBIGUOUS_PRESENT_BYTES,
    )
    .await;
    let ambiguous_present_ready =
        ready_lease(&planner, &inspection, ambiguous_present_object).await;
    let ambiguous_present_store = Arc::new(AmbiguousDeleteStore::new(
        reopened_store.clone(),
        AmbiguousDeleteMode::UnavailableWithoutDelete,
    ));
    let ambiguous_present_service = ObjectGcExecutionService::new(
        reconnected_pool.clone(),
        policy,
        ambiguous_present_store.clone(),
    )
    .expect("ambiguous-present service is valid");
    let ambiguous_present_operation = ambiguous_present_service
        .start_gc_execution(ambiguous_present_ready)
        .await
        .expect("ambiguous-present operation starts");
    let ambiguous_present = ambiguous_present_service
        .delete_next_replica(
            ambiguous_present_operation.operation_id(),
            ambiguous_present_ready,
        )
        .await
        .expect("confirmed presence is retryable after a lost response");
    assert_eq!(
        ambiguous_present.outcome(),
        ObjectGcStepOutcome::ReplicaStillPresent
    );
    assert!(
        reopened_store
            .exists(&ambiguous_present_key)
            .await
            .expect("ambiguous-present key remains")
    );
    assert_eq!(ambiguous_present_store.get_calls(), 0);
    let ambiguous_present_deleted = ambiguous_present_service
        .delete_next_replica(
            ambiguous_present_operation.operation_id(),
            ambiguous_present.lease(),
        )
        .await
        .expect("retry deletes the still-present replica");
    assert_eq!(
        ambiguous_present_deleted.outcome(),
        ObjectGcStepOutcome::ReplicaDeleted
    );
    ambiguous_present_service
        .complete_gc_execution(
            ambiguous_present_operation.operation_id(),
            ambiguous_present_deleted.lease(),
        )
        .await
        .expect("ambiguous-present operation completes after retry");

    // An ObjectReplica row may outlive externally missing bytes. Exact-key
    // absence is idempotent success, but metadata is retained until that
    // absence has been observed through ObjectStore reconciliation.
    let missing_object =
        ObjectReference::new(ObjectId::new(), DedupDomainId::new(), digest(b""), 0);
    repository
        .insert_object(missing_object, timestamp("2026-08-26T00:00:02.423456Z"))
        .await
        .expect("missing-replica object persists");
    let missing_replica = ObjectReplicaId::new();
    sqlx::query(
        "INSERT INTO object_replicas
            (id, object_id, object_dedup_domain_id, backend_kind, storage_key,
             stored_length, stored_sha256, backend_version, state, created_at, verified_at)
         VALUES ($1, $2, $3, 'LOCAL_FILESYSTEM', 'objects/v1/gc_already_absent',
                 0, $4, 'v1-missing-fixture', 'VERIFIED',
                 clock_timestamp(), clock_timestamp())",
    )
    .bind(missing_replica.into_uuid())
    .bind(missing_object.object_id().into_uuid())
    .bind(missing_object.dedup_domain_id().into_uuid())
    .bind(missing_object.canonical_hash().as_bytes())
    .execute(&inspection)
    .await
    .expect("missing replica metadata persists before reconciliation");
    let missing_ready = ready_lease(&planner, &inspection, missing_object).await;
    let missing_operation = reconnected
        .start_gc_execution(missing_ready)
        .await
        .expect("missing-replica operation starts");
    let missing_reconciled = reconnected
        .delete_next_replica(missing_operation.operation_id(), missing_ready)
        .await
        .expect("already-absent exact key reconciles");
    assert_eq!(
        missing_reconciled.outcome(),
        ObjectGcStepOutcome::ReplicaAlreadyAbsent
    );
    reconnected
        .complete_gc_execution(missing_operation.operation_id(), missing_reconciled.lease())
        .await
        .expect("already-absent operation completes");

    // Persisted evidence that does not match immutable provider head metadata
    // is a terminal fail-closed action; physical bytes and replica metadata
    // both remain for explicit recovery.
    const MISMATCH_BYTES: &[u8] = b"evidence mismatch must not delete";
    let mismatch_object = ObjectReference::new(
        ObjectId::new(),
        DedupDomainId::new(),
        digest(MISMATCH_BYTES),
        MISMATCH_BYTES.len() as u64,
    );
    repository
        .insert_object(mismatch_object, timestamp("2026-08-26T00:00:02.523456Z"))
        .await
        .expect("mismatch object persists");
    let (mismatch_replica, mismatch_key, _) = insert_replica(
        &inspection,
        &reopened_store,
        mismatch_object,
        "objects/v1/gc_mismatch",
        MISMATCH_BYTES,
    )
    .await;
    sqlx::query("UPDATE object_replicas SET backend_version = 'v1-mismatch' WHERE id = $1")
        .bind(mismatch_replica.into_uuid())
        .execute(&inspection)
        .await
        .expect("test mismatch evidence persists");
    let mismatch_ready = ready_lease(&planner, &inspection, mismatch_object).await;
    let mismatch_operation = reconnected
        .start_gc_execution(mismatch_ready)
        .await
        .expect("mismatch operation persists before storage inspection");
    let mismatch = reconnected
        .delete_next_replica(mismatch_operation.operation_id(), mismatch_ready)
        .await
        .expect("mismatch is a persisted closed outcome");
    assert_eq!(mismatch.outcome(), ObjectGcStepOutcome::NeedsAttention);
    assert!(
        reopened_store
            .exists(&mismatch_key)
            .await
            .expect("mismatched bytes remain")
    );
    let mismatch_counts = sqlx::query_as::<_, (i64, String, String)>(
        "SELECT
            (SELECT count(*) FROM object_replicas WHERE id = $1),
            (SELECT state FROM object_gc_replica_actions
             WHERE operation_id = $2 AND replica_id = $1),
            (SELECT state FROM object_gc_operations WHERE operation_id = $2)",
    )
    .bind(mismatch_replica.into_uuid())
    .bind(mismatch_operation.operation_id().into_uuid())
    .fetch_one(&inspection)
    .await
    .expect("mismatch recovery state query succeeds");
    assert_eq!(
        mismatch_counts,
        (1, "FAILED".to_owned(), "NEEDS_ATTENTION".to_owned())
    );

    // Active generic holds are authoritative even though backup/share/sync do
    // not yet have producers. Releasing the hold permits a fresh revalidation.
    let held_object = ObjectReference::new(
        ObjectId::new(),
        DedupDomainId::new(),
        Sha256Digest::from_bytes([0x42; 32]),
        0,
    );
    repository
        .insert_object(held_object, timestamp("2026-08-26T00:00:03.123456Z"))
        .await
        .expect("held object persists");
    let held_ready = ready_lease(&planner, &inspection, held_object).await;
    let hold_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO object_gc_holds
            (hold_id, object_id, object_dedup_domain_id, hold_class, created_at)
         VALUES ($1, $2, $3, 'ADMINISTRATIVE', clock_timestamp())",
    )
    .bind(hold_id)
    .bind(held_object.object_id().into_uuid())
    .bind(held_object.dedup_domain_id().into_uuid())
    .execute(&inspection)
    .await
    .expect("active hold persists");
    assert_eq!(
        reconnected.start_gc_execution(held_ready).await,
        Err(ObjectGcExecutionError::HoldExists)
    );
    sqlx::query("UPDATE object_gc_holds SET released_at = clock_timestamp() WHERE hold_id = $1")
        .bind(hold_id)
        .execute(&inspection)
        .await
        .expect("hold release persists");
    let held_operation = reconnected
        .start_gc_execution(held_ready)
        .await
        .expect("released hold permits zero-replica operation");
    let reactivate_hold =
        sqlx::query("UPDATE object_gc_holds SET released_at = NULL WHERE hold_id = $1")
            .bind(hold_id)
            .execute(&inspection)
            .await;
    assert!(reactivate_hold.is_err());
    reconnected
        .complete_gc_execution(held_operation.operation_id(), held_ready)
        .await
        .expect("zero-replica object completes only after revalidation");

    let expired_hold_object = ObjectReference::new(
        ObjectId::new(),
        DedupDomainId::new(),
        Sha256Digest::from_bytes([0x44; 32]),
        0,
    );
    repository
        .insert_object(
            expired_hold_object,
            timestamp("2026-08-26T00:00:03.173456Z"),
        )
        .await
        .expect("expired-hold object persists");
    let expired_hold_ready = ready_lease(&planner, &inspection, expired_hold_object).await;
    sqlx::query(
        "INSERT INTO object_gc_holds
            (hold_id, object_id, object_dedup_domain_id, hold_class, created_at, expires_at)
         VALUES ($1, $2, $3, 'ADMINISTRATIVE',
                 clock_timestamp() - INTERVAL '2 seconds',
                 clock_timestamp() - INTERVAL '1 second')",
    )
    .bind(Uuid::now_v7())
    .bind(expired_hold_object.object_id().into_uuid())
    .bind(expired_hold_object.dedup_domain_id().into_uuid())
    .execute(&inspection)
    .await
    .expect("expired hold persists as inactive evidence");
    let expired_hold_operation = reconnected
        .start_gc_execution(expired_hold_ready)
        .await
        .expect("expired hold no longer blocks GC");
    reconnected
        .complete_gc_execution(expired_hold_operation.operation_id(), expired_hold_ready)
        .await
        .expect("expired-hold object completes after final revalidation");

    // Concurrent workers converge on one durable operation identity. The
    // uniqueness fence prevents duplicate action plans for the same Object.
    let concurrent_object = ObjectReference::new(
        ObjectId::new(),
        DedupDomainId::new(),
        Sha256Digest::from_bytes([0x43; 32]),
        0,
    );
    repository
        .insert_object(concurrent_object, timestamp("2026-08-26T00:00:03.223456Z"))
        .await
        .expect("concurrent-worker object persists");
    let concurrent_ready = ready_lease(&planner, &inspection, concurrent_object).await;
    let (worker_a, worker_b) = tokio::join!(
        reconnected.start_gc_execution(concurrent_ready),
        reconnected.start_gc_execution(concurrent_ready),
    );
    let worker_a = worker_a.expect("first GC worker converges");
    let worker_b = worker_b.expect("second GC worker converges");
    assert_eq!(worker_a.operation_id(), worker_b.operation_id());
    let concurrent_operation_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_gc_operations
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(concurrent_object.object_id().into_uuid())
    .bind(concurrent_object.dedup_domain_id().into_uuid())
    .fetch_one(&inspection)
    .await
    .expect("concurrent operation count succeeds");
    assert_eq!(concurrent_operation_count, 1);
    reconnected
        .complete_gc_execution(worker_a.operation_id(), concurrent_ready)
        .await
        .expect("concurrent-worker operation completes once");

    // Two workers holding the same lease race through a real production
    // LocalFilesystemObjectStore replica. The action fence lets at most one
    // call reach conditional physical deletion; the other may only observe or
    // be fenced, and a bounded retry converges without a duplicate side effect.
    const CONCURRENT_DELETE_BYTES: &[u8] = b"one fenced worker deletes this replica";
    let concurrent_delete_object = ObjectReference::new(
        ObjectId::new(),
        DedupDomainId::new(),
        digest(CONCURRENT_DELETE_BYTES),
        CONCURRENT_DELETE_BYTES.len() as u64,
    );
    repository
        .insert_object(
            concurrent_delete_object,
            timestamp("2026-08-26T00:00:03.323456Z"),
        )
        .await
        .expect("concurrent-delete object persists");
    let (_concurrent_delete_replica, concurrent_delete_key, _) = insert_replica(
        &inspection,
        &reopened_store,
        concurrent_delete_object,
        "objects/v1/gc_concurrent_delete",
        CONCURRENT_DELETE_BYTES,
    )
    .await;
    let concurrent_delete_ready =
        ready_lease(&planner, &inspection, concurrent_delete_object).await;
    let concurrent_delete_store = Arc::new(AmbiguousDeleteStore::new(
        reopened_store.clone(),
        AmbiguousDeleteMode::Delegate,
    ));
    let concurrent_delete_service = ObjectGcExecutionService::new(
        reconnected_pool.clone(),
        policy,
        concurrent_delete_store.clone(),
    )
    .expect("concurrent-delete service is valid");
    let concurrent_delete_operation = concurrent_delete_service
        .start_gc_execution(concurrent_delete_ready)
        .await
        .expect("concurrent-delete operation starts");
    let (delete_worker_a, delete_worker_b) = tokio::join!(
        concurrent_delete_service.delete_next_replica(
            concurrent_delete_operation.operation_id(),
            concurrent_delete_ready,
        ),
        concurrent_delete_service.delete_next_replica(
            concurrent_delete_operation.operation_id(),
            concurrent_delete_ready,
        ),
    );
    for result in [delete_worker_a, delete_worker_b] {
        match result {
            Ok(step) => assert!(matches!(
                step.outcome(),
                ObjectGcStepOutcome::ReplicaDeleted
                    | ObjectGcStepOutcome::ReplicaAlreadyAbsent
                    | ObjectGcStepOutcome::ReplicaReconciledAbsent
                    | ObjectGcStepOutcome::ReplicaStillPresent
                    | ObjectGcStepOutcome::ReplicaDeleteInProgress
            )),
            Err(error) => assert_eq!(error, ObjectGcExecutionError::InvalidState),
        }
    }
    assert!(
        concurrent_delete_store.conditional_delete_calls() <= 1,
        "only the DELETE_FENCED worker may invoke physical conditional deletion"
    );
    let concurrent_progress = concurrent_delete_service
        .load_gc_execution(concurrent_delete_operation.operation_id())
        .await
        .expect("concurrent-delete durable progress loads");
    if concurrent_progress.deleted_replica_count() == 0 {
        // A competing worker may have observed the key as present immediately
        // before the deleting worker removed it. Reconcile first: that marks
        // the durable action deleted from exact-key absence without issuing a
        // second conditional deletion. If it is genuinely still present, only
        // then does a bounded retry delete it.
        let reconciled = concurrent_delete_service
            .reconcile_replica(
                concurrent_delete_operation.operation_id(),
                concurrent_delete_ready,
            )
            .await
            .expect("bounded reconciliation converges the fenced replica");
        if matches!(
            reconciled.outcome(),
            ObjectGcStepOutcome::ReplicaStillPresent | ObjectGcStepOutcome::ReplicaDeleteInProgress
        ) {
            let retried = concurrent_delete_service
                .delete_next_replica(
                    concurrent_delete_operation.operation_id(),
                    reconciled.lease(),
                )
                .await
                .expect("bounded retry deletes a confirmed-present replica");
            assert!(matches!(
                retried.outcome(),
                ObjectGcStepOutcome::ReplicaDeleted
                    | ObjectGcStepOutcome::ReplicaAlreadyAbsent
                    | ObjectGcStepOutcome::ReplicaReconciledAbsent
            ));
        } else {
            assert!(matches!(
                reconciled.outcome(),
                ObjectGcStepOutcome::ReplicaDeleted
                    | ObjectGcStepOutcome::ReplicaAlreadyAbsent
                    | ObjectGcStepOutcome::ReplicaReconciledAbsent
            ));
        }
    }
    assert!(
        !reopened_store
            .exists(&concurrent_delete_key)
            .await
            .expect("concurrent-delete key is absent after convergence")
    );
    assert_eq!(concurrent_delete_store.conditional_delete_calls(), 1);
    concurrent_delete_service
        .complete_gc_execution(
            concurrent_delete_operation.operation_id(),
            concurrent_delete_ready,
        )
        .await
        .expect("concurrent-delete operation completes once");

    // Object lifecycle fencing serializes physical GC against every new
    // FileVersion writer. Exactly one side of the real PostgreSQL race wins.
    let owner_id = UserId::new();
    let observed_at = timestamp("2026-08-26T00:00:04.123456Z");
    repository
        .insert_user(&User::new(
            owner_id,
            synveil_core::LoginIdentifier::new("physical-gc-race", owner_id.to_string())
                .expect("race login is valid"),
            UserStatus::Active,
            observed_at,
        ))
        .await
        .expect("race owner persists");
    let library_id = LibraryId::new();
    let race_domain = DedupDomainId::new();
    let library_root = Node::new_root(NodeId::new(), library_id, name("race-root"), observed_at);
    let library = Library::new(
        library_id,
        owner_id,
        name("Physical GC Race"),
        &library_root,
        race_domain,
        observed_at,
    )
    .expect("race library is valid");
    repository
        .insert_library_with_root(&library, &library_root)
        .await
        .expect("race library persists");
    let race_file = Node::new_child(
        NodeId::new(),
        library_id,
        &library_root,
        NodeKind::File,
        name("race-file"),
        observed_at,
    )
    .expect("race file is valid");
    repository
        .insert_node(&race_file)
        .await
        .expect("race file persists");

    // READY planning is revocable. A committed FileVersion written after the
    // planner's READY transition but before physical execution must be found
    // by the final start/action proof, leaving its real local bytes untouched.
    let final_reference_file = Node::new_child(
        NodeId::new(),
        library_id,
        &library_root,
        NodeKind::File,
        name("final-reference-file"),
        observed_at,
    )
    .expect("final-reference file is valid");
    repository
        .insert_node(&final_reference_file)
        .await
        .expect("final-reference file persists");
    const FINAL_REFERENCE_BYTES: &[u8] = b"a reference wins before physical deletion";
    let final_reference_object = ObjectReference::new(
        ObjectId::new(),
        race_domain,
        digest(FINAL_REFERENCE_BYTES),
        FINAL_REFERENCE_BYTES.len() as u64,
    );
    repository
        .insert_object(final_reference_object, observed_at)
        .await
        .expect("final-reference object persists");
    let (_final_reference_replica, final_reference_key, _) = insert_replica(
        &inspection,
        &reopened_store,
        final_reference_object,
        "objects/v1/gc_final_reference",
        FINAL_REFERENCE_BYTES,
    )
    .await;
    let final_reference_ready = ready_lease(&planner, &inspection, final_reference_object).await;
    let final_reference_version = FileVersion::new(
        FileVersionId::new(),
        &library,
        &final_reference_file,
        final_reference_object,
        None,
        observed_at,
    )
    .expect("final-reference version is valid");
    // Use a direct committed writer to model a concurrent/future writer that
    // does not first clear candidate metadata. The database lifecycle trigger
    // still runs, and physical GC must independently revalidate the reference.
    sqlx::query(
        "INSERT INTO file_versions
            (id, library_id, node_id, object_id, object_dedup_domain_id,
             parent_version_id, committed_at, revision)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8::NUMERIC)",
    )
    .bind(final_reference_version.id().into_uuid())
    .bind(final_reference_version.library_id().into_uuid())
    .bind(final_reference_version.node_id().into_uuid())
    .bind(final_reference_object.object_id().into_uuid())
    .bind(final_reference_object.dedup_domain_id().into_uuid())
    .bind(
        final_reference_version
            .parent_version_id()
            .map(FileVersionId::into_uuid),
    )
    .bind(final_reference_version.committed_at().as_offset_datetime())
    .bind(final_reference_version.revision().get().to_string())
    .execute(&inspection)
    .await
    .expect("committed final-reference writer succeeds before GC starts");
    assert_eq!(
        reconnected.start_gc_execution(final_reference_ready).await,
        Err(ObjectGcExecutionError::ReferenceExists)
    );
    assert!(
        reopened_store
            .exists(&final_reference_key)
            .await
            .expect("final-reference bytes remain readable")
    );
    let final_reference_counts = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT
            (SELECT count(*) FROM objects WHERE id = $1),
            (SELECT count(*) FROM object_replicas WHERE object_id = $1),
            (SELECT count(*) FROM object_gc_operations
             WHERE object_id = $1 AND object_dedup_domain_id = $2)",
    )
    .bind(final_reference_object.object_id().into_uuid())
    .bind(final_reference_object.dedup_domain_id().into_uuid())
    .fetch_one(&inspection)
    .await
    .expect("final-reference metadata remains inspectable");
    assert_eq!(final_reference_counts, (1, 1, 0));

    // A real historical restore reuses the exact canonical object. Its source
    // FileVersion is already authoritative, so physical GC must lose the
    // concurrent race before touching the replica; the restore then commits a
    // second FileVersion that remains backed by the same real local bytes.
    let restore_service = VersionRestoreService::new(reconnected_pool.clone());
    let (gc_restore_result, restore_result) = tokio::join!(
        reconnected.start_gc_execution(final_reference_ready),
        restore_service.restore_file_version(
            owner_id,
            final_reference_file.id(),
            final_reference_version.id(),
            final_reference_file.revision(),
            "gc-restore-race-001".to_owned(),
        ),
    );
    assert!(matches!(
        gc_restore_result,
        Err(ObjectGcExecutionError::ReferenceExists)
            | Err(ObjectGcExecutionError::CandidateNotFound)
    ));
    restore_result.expect("restore wins without exposing deleted content");
    assert!(
        reopened_store
            .exists(&final_reference_key)
            .await
            .expect("restored object bytes remain readable")
    );
    let restored_reference_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM file_versions
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(final_reference_object.object_id().into_uuid())
    .bind(final_reference_object.dedup_domain_id().into_uuid())
    .fetch_one(&inspection)
    .await
    .expect("restored references are durable");
    assert_eq!(restored_reference_count, 2);

    // Upload finalization is a real writer of ObjectReplica + FileVersion
    // metadata. Exercise the production PostgreSQL finalizer against the
    // exact canonical identity that GC is fencing: its duplicate canonical
    // Object insert must fail closed, and no new FileVersion can commit while
    // GC proceeds to remove the real disposable bytes.
    let upload_race_file = Node::new_child(
        NodeId::new(),
        library_id,
        &library_root,
        NodeKind::File,
        name("upload-finalization-race-file"),
        observed_at,
    )
    .expect("upload-race file is valid");
    repository
        .insert_node(&upload_race_file)
        .await
        .expect("upload-race file persists");
    const UPLOAD_RACE_BYTES: &[u8] = b"upload finalization must not beat GC fencing";
    let upload_race_object = ObjectReference::new(
        ObjectId::new(),
        race_domain,
        digest(UPLOAD_RACE_BYTES),
        UPLOAD_RACE_BYTES.len() as u64,
    );
    repository
        .insert_object(upload_race_object, observed_at)
        .await
        .expect("upload-race object persists");
    let (_upload_race_replica, upload_race_key, upload_race_version) = insert_replica(
        &inspection,
        &reopened_store,
        upload_race_object,
        "objects/v1/gc_upload_finalization_race",
        UPLOAD_RACE_BYTES,
    )
    .await;
    let upload_race_ready = ready_lease(&planner, &inspection, upload_race_object).await;
    let upload_repository = PostgresUploadRepository::new(reconnected_pool.clone());
    let upload_session_id = UploadSessionId::new();
    upload_repository
        .create_upload_session(NewUploadSession {
            id: upload_session_id,
            owner_user_id: owner_id,
            library_id,
            operation: UploadOperation::ReplaceContent,
            target_node_id: upload_race_file.id(),
            target_parent_node_id: None,
            target_name: None,
            expected_node_revision: Some(upload_race_file.revision()),
            expected_length: upload_race_object.plaintext_length(),
            expected_sha256: Some(upload_race_object.canonical_hash()),
            object_id: upload_race_object.object_id(),
            object_replica_id: ObjectReplicaId::new(),
            object_key: upload_race_key.as_str().to_owned(),
            staging_handle: format!("gc-upload-race-{upload_session_id}"),
            max_active_sessions: 2,
            created_at: observed_at,
            expires_at: timestamp("2026-08-26T01:00:04.123456Z"),
        })
        .await
        .expect("upload-race session persists");
    upload_repository
        .record_upload_progress(
            owner_id,
            upload_session_id,
            0,
            upload_race_object.plaintext_length(),
            observed_at,
        )
        .await
        .expect("upload-race progress persists");
    let upload_generation = match upload_repository
        .claim_upload(
            owner_id,
            upload_session_id,
            observed_at,
            timestamp("2026-08-26T00:10:04.123456Z"),
        )
        .await
        .expect("upload-race claim persists")
    {
        UploadClaim::Acquired(record) => record.lease_generation,
        other => panic!("upload-race claim was not acquired: {other:?}"),
    };
    upload_repository
        .record_upload_durable(
            owner_id,
            upload_session_id,
            upload_generation,
            UploadDurabilityReceipt {
                backend_kind: "LOCAL_FILESYSTEM".to_owned(),
                storage_key: upload_race_key.as_str().to_owned(),
                backend_version: Some(upload_race_version.as_str().to_owned()),
                length: upload_race_object.plaintext_length(),
                sha256: upload_race_object.canonical_hash(),
                verified_at: observed_at,
            },
            observed_at,
        )
        .await
        .expect("upload-race durability receipt persists");
    let (gc_upload_result, upload_result) = tokio::join!(
        reconnected.start_gc_execution(upload_race_ready),
        upload_repository.finalize_upload(
            owner_id,
            upload_session_id,
            upload_generation,
            observed_at,
        ),
    );
    let upload_race_operation = gc_upload_result.expect("GC wins the upload-finalization fence");
    assert!(
        upload_result.is_err(),
        "a finalizer cannot commit a new reference to the fenced canonical Object"
    );
    let upload_race_reference_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM file_versions
         WHERE node_id = $1 AND library_id = $2",
    )
    .bind(upload_race_file.id().into_uuid())
    .bind(library_id.into_uuid())
    .fetch_one(&inspection)
    .await
    .expect("upload-race reference count succeeds");
    assert_eq!(upload_race_reference_count, 0);
    let upload_race_deleted = reconnected
        .delete_next_replica(upload_race_operation.operation_id(), upload_race_ready)
        .await
        .expect("GC removes the fenced upload-race bytes");
    assert_eq!(
        upload_race_deleted.outcome(),
        ObjectGcStepOutcome::ReplicaDeleted
    );
    assert!(
        !reopened_store
            .exists(&upload_race_key)
            .await
            .expect("upload-race physical bytes are absent only after GC")
    );
    reconnected
        .complete_gc_execution(
            upload_race_operation.operation_id(),
            upload_race_deleted.lease(),
        )
        .await
        .expect("upload-race GC metadata completes after the failed finalizer");

    let race_object = ObjectReference::new(
        ObjectId::new(),
        race_domain,
        Sha256Digest::from_bytes([0x55; 32]),
        0,
    );
    repository
        .insert_object(race_object, observed_at)
        .await
        .expect("race object persists");
    let race_ready = ready_lease(&planner, &inspection, race_object).await;
    let race_version = FileVersion::new(
        FileVersionId::new(),
        &library,
        &race_file,
        race_object,
        None,
        observed_at,
    )
    .expect("race version is valid");
    let (gc_result, reference_result) = tokio::join!(
        reconnected.start_gc_execution(race_ready),
        repository.insert_file_version(race_version),
    );
    assert_ne!(gc_result.is_ok(), reference_result.is_ok());
    let reference_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM file_versions
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(race_object.object_id().into_uuid())
    .bind(race_domain.into_uuid())
    .fetch_one(&inspection)
    .await
    .expect("race reference count succeeds");
    if let Ok(operation) = gc_result {
        assert_eq!(reference_count, 0);
        reconnected
            .complete_gc_execution(operation.operation_id(), race_ready)
            .await
            .expect("winning zero-replica GC completes");
    } else {
        assert_eq!(reference_count, 1);
    }

    // A replica writer (upload finalization/repair class) is likewise rejected
    // after GC_DELETING wins, before it can create usable metadata.
    let replica_race_object = ObjectReference::new(
        ObjectId::new(),
        DedupDomainId::new(),
        Sha256Digest::from_bytes([0x66; 32]),
        0,
    );
    repository
        .insert_object(replica_race_object, observed_at)
        .await
        .expect("replica-race object persists");
    let replica_race_ready = ready_lease(&planner, &inspection, replica_race_object).await;
    let replica_race_operation = reconnected
        .start_gc_execution(replica_race_ready)
        .await
        .expect("replica-race GC starts");
    let replica_insert = sqlx::query(
        "INSERT INTO object_replicas
            (id, object_id, object_dedup_domain_id, backend_kind, storage_key,
             stored_length, stored_sha256, backend_version, state, created_at, verified_at)
         VALUES ($1, $2, $3, 'LOCAL_FILESYSTEM', 'objects/v1/too_late',
                 0, $4, 'v1', 'VERIFIED', clock_timestamp(), clock_timestamp())",
    )
    .bind(ObjectReplicaId::new().into_uuid())
    .bind(replica_race_object.object_id().into_uuid())
    .bind(replica_race_object.dedup_domain_id().into_uuid())
    .bind(replica_race_object.canonical_hash().as_bytes())
    .execute(&inspection)
    .await;
    assert!(replica_insert.is_err());
    reconnected
        .complete_gc_execution(replica_race_operation.operation_id(), replica_race_ready)
        .await
        .expect("replica-race zero-replica GC completes");

    // Closed observation API cannot be tricked into accepting an arbitrary
    // safe-error string or a different action generation.
    assert_eq!(
        metadata_backend
            .record_replica_observation(
                &crash_action,
                crash_renewed,
                ObjectGcReplicaObservation::Deleted,
                None,
            )
            .await
            .expect("completed observation replay is deterministic")
            .state(),
        synveil_metadata::ObjectGcExecutionState::Completed
    );
    assert_eq!(
        metadata_backend
            .load_gc_execution(ObjectGcOperationId::new())
            .await,
        Err(ObjectGcExecutionMetadataError::OperationNotFound)
    );

    inspection.close().await;
    reconnected_pool.close().await;
    pool.close().await;
}

/// Prompt 29's end-to-end gate proves that the worker owns scheduling only:
/// the actual byte deletion still flows through Prompt 28's execution service
/// and the durable operation is resumed in a later bounded cycle before final
/// metadata cleanup.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_worker_drives_bounded_local_physical_gc() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable test database");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    MigrationRunner::new()
        .run(&pool)
        .await
        .expect("SQLx migrations must succeed");
    let inspection = PgPool::connect(&url)
        .await
        .expect("inspection connection must succeed");
    let policy = ObjectGcPolicy::new(Duration::from_secs(1), Duration::from_secs(30), 1)
        .expect("focused worker policy is valid");
    let worker_config = GcWorkerConfig::new(
        true,
        Duration::from_secs(1),
        1,
        1,
        1,
        1,
        1,
        GcWorkerRetryPolicy::new(Duration::from_secs(1), Duration::from_secs(4), 3)
            .expect("focused retry policy is valid"),
        Duration::from_secs(5),
    )
    .expect("focused worker configuration is valid");
    let root = TempRoot::new();
    let store = Arc::new(
        LocalFilesystemObjectStore::open(root.path()).expect("open production local adapter"),
    );
    let repository = DomainRepository::new(&pool);
    const BYTES: &[u8] = b"worker-driven physical object GC";
    let object = ObjectReference::new(
        ObjectId::new(),
        DedupDomainId::new(),
        digest(BYTES),
        BYTES.len() as u64,
    );
    repository
        .insert_object(object, timestamp("2026-08-26T00:00:10.123456Z"))
        .await
        .expect("worker object persists");
    let (_replica, key, _version) = insert_replica(
        &inspection,
        &store,
        object,
        "objects/v1/gc_worker_e2e",
        BYTES,
    )
    .await;
    insert_candidate(&inspection, object).await;

    let worker = GcWorker::new(pool.clone(), policy, vec![store.clone()], worker_config)
        .expect("worker composition is valid");
    let first = worker
        .run_once()
        .await
        .expect("first worker cycle succeeds");
    assert_ne!(first.status(), GcWorkerCycleStatus::Disabled);
    assert_eq!(first.recovery().recovery_claimed(), 0);
    assert_eq!(first.new_work().candidates_claimed(), 1);
    assert_eq!(first.new_work().operations_started(), 1);
    assert_eq!(first.new_work().replicas_deleted(), 1);
    assert!(
        !store
            .exists(&key)
            .await
            .expect("worker deleted only the exact managed key")
    );
    let intermediate = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT operation.state, candidate.state,
                (SELECT count(*) FROM object_gc_replica_actions AS action
                 WHERE action.operation_id = operation.operation_id
                   AND action.state = 'DELETED')
         FROM object_gc_operations AS operation
         JOIN object_gc_candidates AS candidate
           ON candidate.object_id = operation.object_id
          AND candidate.object_dedup_domain_id = operation.object_dedup_domain_id
         WHERE operation.object_id = $1 AND operation.object_dedup_domain_id = $2",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .fetch_one(&inspection)
    .await
    .expect("worker operation remains durable after its first bounded action");
    assert_eq!(
        intermediate,
        ("ACTIVE".to_owned(), "ELIGIBLE".to_owned(), 1)
    );

    let second = worker
        .run_once()
        .await
        .expect("recovery worker cycle succeeds");
    assert_ne!(second.status(), GcWorkerCycleStatus::Disabled);
    assert_eq!(second.recovery().recovery_claimed(), 1);
    assert_eq!(second.recovery().operations_resumed(), 1);
    assert_eq!(second.recovery().operations_completed(), 1);
    assert_eq!(second.new_work().candidates_claimed(), 0);
    let terminal = sqlx::query_as::<_, (i64, i64, i64, String)>(
        "SELECT
            (SELECT count(*) FROM objects WHERE id = $1),
            (SELECT count(*) FROM object_replicas WHERE object_id = $1),
            (SELECT count(*) FROM object_gc_candidates WHERE object_id = $1),
            (SELECT state FROM object_gc_operations
             WHERE object_id = $1 AND object_dedup_domain_id = $2)",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .fetch_one(&inspection)
    .await
    .expect("worker terminal metadata query succeeds");
    assert_eq!(terminal, (0, 0, 0, "COMPLETED".to_owned()));

    inspection.close().await;
    pool.close().await;
}

/// A durable operation can fail terminally while a recovery worker is
/// revalidating it, before `resume_gc_execution` has committed the replacement
/// lease binding. That failure must be persisted as intervention-required,
/// rather than leaving the operation eligible for an unbounded recovery loop.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_worker_terminal_resume_error_becomes_needs_attention() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable test database");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    MigrationRunner::new()
        .run(&pool)
        .await
        .expect("SQLx migrations must succeed");
    let inspection = PgPool::connect(&url)
        .await
        .expect("inspection connection must succeed");
    let policy = ObjectGcPolicy::new(Duration::from_secs(1), Duration::from_secs(30), 1)
        .expect("focused worker policy is valid");
    let worker_config = GcWorkerConfig::new(
        true,
        Duration::from_secs(1),
        1,
        1,
        1,
        1,
        1,
        GcWorkerRetryPolicy::new(Duration::from_secs(1), Duration::from_secs(4), 3)
            .expect("focused retry policy is valid"),
        Duration::from_secs(5),
    )
    .expect("focused worker configuration is valid");
    let root = TempRoot::new();
    let store = Arc::new(
        LocalFilesystemObjectStore::open(root.path()).expect("open production local adapter"),
    );
    let repository = DomainRepository::new(&pool);
    const BYTES: &[u8] = b"terminal recovery evidence mismatch";
    let object = ObjectReference::new(
        ObjectId::new(),
        DedupDomainId::new(),
        digest(BYTES),
        BYTES.len() as u64,
    );
    repository
        .insert_object(object, timestamp("2026-08-26T00:00:15.123456Z"))
        .await
        .expect("terminal-recovery fixture object persists");
    let (_replica, key, _version) = insert_replica(
        &inspection,
        &store,
        object,
        "objects/v1/gc_worker_terminal_resume",
        BYTES,
    )
    .await;
    insert_candidate(&inspection, object).await;
    let worker = GcWorker::new(pool.clone(), policy, vec![store.clone()], worker_config)
        .expect("worker composition is valid");

    let first = worker
        .run_once()
        .await
        .expect("first worker cycle creates durable operation and deletes its replica");
    assert_eq!(first.new_work().operations_started(), 1);
    assert_eq!(first.new_work().replicas_deleted(), 1);
    assert!(
        !store
            .exists(&key)
            .await
            .expect("first bounded step deletes only the managed fixture key")
    );

    // Simulate a persisted action that no longer agrees with its now-absent
    // replica. This is a terminal evidence failure during resume, not a
    // retryable ObjectStore outcome.
    sqlx::query(
        "UPDATE object_gc_replica_actions
         SET state = 'PENDING', deleted_at = NULL, last_outcome = NULL
         WHERE operation_id = (
             SELECT operation_id FROM object_gc_operations
             WHERE object_id = $1 AND object_dedup_domain_id = $2
         )",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .execute(&inspection)
    .await
    .expect("test fixture creates a terminal resume evidence mismatch");

    let recovered = worker
        .run_once()
        .await
        .expect("terminal recovery failure is contained in a bounded cycle");
    assert_eq!(recovered.recovery().recovery_claimed(), 1);
    assert_eq!(recovered.recovery().operations_resumed(), 0);
    assert_eq!(recovered.recovery().needs_attention(), 1);
    assert_eq!(recovered.new_work().candidates_claimed(), 0);
    assert_eq!(recovered.status(), GcWorkerCycleStatus::Degraded);

    let terminal = sqlx::query_as::<_, (String, String)>(
        "SELECT state, last_error_code
         FROM object_gc_operations
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .fetch_one(&inspection)
    .await
    .expect("terminal recovery state query succeeds");
    assert_eq!(
        terminal,
        (
            "NEEDS_ATTENTION".to_owned(),
            "gc_replica_evidence_mismatch".to_owned()
        )
    );

    inspection.close().await;
    pool.close().await;
}

/// Durable action scheduling is driven by PostgreSQL server time. A transient
/// ObjectStore result releases the planning lease, records a future retry, and
/// prevents the next cycle from spinning or treating unknown bytes as absent.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_worker_persists_retry_backoff_and_reports_metadata_orphans() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable test database");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = DatabasePool::connect(&config)
        .await
        .expect("test PostgreSQL must accept a connection");
    MigrationRunner::new()
        .run(&pool)
        .await
        .expect("SQLx migrations must succeed");
    let inspection = PgPool::connect(&url)
        .await
        .expect("inspection connection must succeed");
    let policy = ObjectGcPolicy::new(Duration::from_secs(1), Duration::from_secs(30), 1)
        .expect("focused worker policy is valid");
    let worker_config = GcWorkerConfig::new(
        true,
        Duration::from_secs(1),
        1,
        1,
        1,
        1,
        1,
        GcWorkerRetryPolicy::new(Duration::from_secs(60), Duration::from_secs(60), 3)
            .expect("focused retry policy is valid"),
        Duration::from_secs(5),
    )
    .expect("focused worker configuration is valid");
    let root = TempRoot::new();
    let local_store = Arc::new(
        LocalFilesystemObjectStore::open(root.path()).expect("open production local adapter"),
    );
    let repository = DomainRepository::new(&pool);
    const BYTES: &[u8] = b"worker retry scheduling must be durable";
    let object = ObjectReference::new(
        ObjectId::new(),
        DedupDomainId::new(),
        digest(BYTES),
        BYTES.len() as u64,
    );
    repository
        .insert_object(object, timestamp("2026-08-26T00:00:20.123456Z"))
        .await
        .expect("retry object persists");
    let (_replica, key, _version) = insert_replica(
        &inspection,
        &local_store,
        object,
        "objects/v1/gc_worker_retry",
        BYTES,
    )
    .await;
    insert_candidate(&inspection, object).await;
    let unavailable_store = Arc::new(AmbiguousDeleteStore::new(
        local_store.clone(),
        AmbiguousDeleteMode::UnavailableWithoutDelete,
    ));
    let retry_worker = GcWorker::new(pool.clone(), policy, vec![unavailable_store], worker_config)
        .expect("retry worker composition is valid");

    let first = retry_worker
        .run_once()
        .await
        .expect("transient storage outcome is persisted safely");
    assert_eq!(first.new_work().retry_scheduled(), 1);
    assert!(
        local_store
            .exists(&key)
            .await
            .expect("retryable outcome leaves bytes intact")
    );
    let scheduled = sqlx::query_as::<_, (String, i32, bool, String)>(
        "SELECT action.state, action.attempt_count,
                action.next_attempt_at > clock_timestamp(), candidate.state
         FROM object_gc_replica_actions AS action
         JOIN object_gc_operations AS operation
           ON operation.operation_id = action.operation_id
         JOIN object_gc_candidates AS candidate
           ON candidate.object_id = operation.object_id
          AND candidate.object_dedup_domain_id = operation.object_dedup_domain_id
         WHERE operation.object_id = $1 AND operation.object_dedup_domain_id = $2",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .fetch_one(&inspection)
    .await
    .expect("retry schedule metadata persists");
    assert_eq!(
        scheduled,
        ("RETRYABLE".to_owned(), 1, true, "ELIGIBLE".to_owned())
    );

    let immediate = retry_worker
        .run_once()
        .await
        .expect("not-yet-due retry cycle is cheap");
    assert_eq!(immediate.recovery().recovery_claimed(), 0);
    assert_eq!(immediate.new_work().candidates_claimed(), 0);
    assert_eq!(immediate.recovery().replica_actions_attempted(), 0);

    sqlx::query(
        "UPDATE object_gc_replica_actions
         SET next_attempt_at = last_attempt_at
         WHERE operation_id IN (
            SELECT operation_id FROM object_gc_operations
            WHERE object_id = $1 AND object_dedup_domain_id = $2
         )",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .execute(&inspection)
    .await
    .expect("test retry deadline advances using PostgreSQL time");
    let recovered_worker = GcWorker::new(
        pool.clone(),
        policy,
        vec![local_store.clone()],
        worker_config,
    )
    .expect("recovery worker composition is valid");
    let recovered = recovered_worker
        .run_once()
        .await
        .expect("due retry resumes through Prompt 28");
    assert_eq!(recovered.recovery().recovery_claimed(), 1);
    assert_eq!(recovered.recovery().replicas_deleted(), 1);
    assert!(
        !local_store
            .exists(&key)
            .await
            .expect("only the recovered exact managed key is deleted")
    );
    let completed = recovered_worker
        .run_once()
        .await
        .expect("terminal cleanup follows bounded recovery");
    assert_eq!(completed.recovery().operations_completed(), 1);

    // A GC_DELETING Object without any durable operation is discovered as a
    // bounded metadata anomaly only. The worker reports it but cannot and does
    // not invent a storage deletion or alter the object.
    let orphan = ObjectReference::new(
        ObjectId::new(),
        DedupDomainId::new(),
        Sha256Digest::from_bytes([0x73; 32]),
        0,
    );
    repository
        .insert_object(orphan, timestamp("2026-08-26T00:00:21.123456Z"))
        .await
        .expect("metadata orphan fixture object persists");
    sqlx::query("UPDATE objects SET lifecycle_state = 'GC_DELETING' WHERE id = $1")
        .bind(orphan.object_id().into_uuid())
        .execute(&inspection)
        .await
        .expect("metadata orphan fixture transition persists");
    let orphan_report = recovered_worker
        .run_once()
        .await
        .expect("metadata-only orphan inspection completes");
    assert!(
        orphan_report
            .reconciliation()
            .gc_deleting_without_operation()
            >= 1
    );
    let orphan_count: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = $1")
        .bind(orphan.object_id().into_uuid())
        .fetch_one(&inspection)
        .await
        .expect("orphan fixture object remains metadata-only");
    assert_eq!(orphan_count, 1);

    inspection.close().await;
    pool.close().await;
}

/// PostgreSQL skip-locked claims and Prompt 28 generation fencing make two
/// process-equivalent worker cycles converge on one durable operation even
/// when they begin at the same time.
#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_concurrent_workers_do_not_duplicate_a_physical_operation() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable test database");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool_a = DatabasePool::connect(&config)
        .await
        .expect("first worker PostgreSQL connection succeeds");
    MigrationRunner::new()
        .run(&pool_a)
        .await
        .expect("SQLx migrations must succeed");
    let pool_b = DatabasePool::connect(&config)
        .await
        .expect("second worker PostgreSQL connection succeeds");
    let inspection = PgPool::connect(&url)
        .await
        .expect("inspection connection must succeed");
    let policy = ObjectGcPolicy::new(Duration::from_secs(1), Duration::from_secs(30), 1)
        .expect("focused worker policy is valid");
    let worker_config = GcWorkerConfig::new(
        true,
        Duration::from_secs(1),
        1,
        1,
        1,
        1,
        1,
        GcWorkerRetryPolicy::default(),
        Duration::from_secs(5),
    )
    .expect("focused worker configuration is valid");
    let root = TempRoot::new();
    let store = Arc::new(
        LocalFilesystemObjectStore::open(root.path()).expect("open production local adapter"),
    );
    let repository = DomainRepository::new(&pool_a);
    const BYTES: &[u8] = b"two concurrent workers one physical operation";
    let object = ObjectReference::new(
        ObjectId::new(),
        DedupDomainId::new(),
        digest(BYTES),
        BYTES.len() as u64,
    );
    repository
        .insert_object(object, timestamp("2026-08-26T00:00:30.123456Z"))
        .await
        .expect("concurrency fixture object persists");
    let (_replica, key, _version) = insert_replica(
        &inspection,
        &store,
        object,
        "objects/v1/gc_worker_concurrent",
        BYTES,
    )
    .await;
    insert_candidate(&inspection, object).await;
    let worker_a = GcWorker::new(pool_a.clone(), policy, vec![store.clone()], worker_config)
        .expect("first worker composition is valid");
    let worker_b = GcWorker::new(pool_b.clone(), policy, vec![store.clone()], worker_config)
        .expect("second worker composition is valid");

    let (result_a, result_b) = tokio::join!(worker_a.run_once(), worker_b.run_once());
    let report_a = result_a.expect("first concurrent worker cycle is safe");
    let report_b = result_b.expect("second concurrent worker cycle is safe");
    assert_eq!(
        report_a.new_work().candidates_claimed() + report_b.new_work().candidates_claimed(),
        1
    );
    let operation_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_gc_operations
         WHERE object_id = $1 AND object_dedup_domain_id = $2",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .fetch_one(&inspection)
    .await
    .expect("one durable operation query succeeds");
    assert_eq!(operation_count, 1);
    assert!(
        !store
            .exists(&key)
            .await
            .expect("only one worker-driven physical delete occurred")
    );

    let completion = worker_a
        .run_once()
        .await
        .expect("a later recovery cycle completes the shared operation");
    assert_eq!(completion.recovery().operations_completed(), 1);
    let completed_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_gc_operations
         WHERE object_id = $1 AND object_dedup_domain_id = $2 AND state = 'COMPLETED'",
    )
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .fetch_one(&inspection)
    .await
    .expect("completed operation query succeeds");
    assert_eq!(completed_count, 1);

    inspection.close().await;
    pool_b.close().await;
    pool_a.close().await;
}
