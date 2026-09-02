use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::{StreamExt, stream};
use sha2::{Digest, Sha256};
use synveil_core::{FileVersionId, NodeId, ObjectId, Revision, Sha256Digest, Timestamp, UserId};
use synveil_metadata::{
    AuthorizedContent, ContentReadMetadataBackend, ContentReadResolution, MetadataError,
};
use synveil_object_store::{
    ByteRange, ByteStream, CapabilitySupport, DeleteOutcome, IntegrityExpectation, ObjectKey,
    ObjectMetadata, ObjectRead, ObjectStore, ObjectStoreError, ObjectVersion, PromotionReceipt,
    PutRequest, StagedMetadata, StagingHandle, StagingProgress, StorageAvailability,
    StorageBackendKind, StorageCapabilities, StorageCapability, boxed_stream,
};
use synveil_storage::{
    ContentDescriptor, ContentReadApplicationService, ContentReadError, LocalFilesystemObjectStore,
};
use uuid::Uuid;

#[derive(Clone, Default)]
struct TestMetadata {
    state: Arc<Mutex<TestMetadataState>>,
}

#[derive(Default)]
struct TestMetadataState {
    current: BTreeMap<(UserId, NodeId), ContentReadResolution>,
    historical: BTreeMap<(UserId, FileVersionId), ContentReadResolution>,
    current_calls: u32,
    version_calls: u32,
    backend_kinds: Vec<String>,
}

impl TestMetadata {
    fn set_current(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
        resolution: ContentReadResolution,
    ) {
        self.state
            .lock()
            .expect("metadata lock")
            .current
            .insert((owner_user_id, node_id), resolution);
    }

    fn set_historical(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
        resolution: ContentReadResolution,
    ) {
        self.state
            .lock()
            .expect("metadata lock")
            .historical
            .insert((owner_user_id, file_version_id), resolution);
    }

    fn current_calls(&self) -> u32 {
        self.state.lock().expect("metadata lock").current_calls
    }

    fn backend_kinds(&self) -> Vec<String> {
        self.state
            .lock()
            .expect("metadata lock")
            .backend_kinds
            .clone()
    }
}

#[async_trait]
impl ContentReadMetadataBackend for TestMetadata {
    async fn resolve_current_content(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
        backend_kind: &str,
    ) -> Result<ContentReadResolution, MetadataError> {
        let mut state = self.state.lock().expect("metadata lock");
        state.current_calls += 1;
        state.backend_kinds.push(backend_kind.to_owned());
        Ok(state
            .current
            .get(&(owner_user_id, node_id))
            .cloned()
            .unwrap_or(ContentReadResolution::NotFound))
    }

    async fn resolve_file_version_content(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
        backend_kind: &str,
    ) -> Result<ContentReadResolution, MetadataError> {
        let mut state = self.state.lock().expect("metadata lock");
        state.version_calls += 1;
        state.backend_kinds.push(backend_kind.to_owned());
        Ok(state
            .historical
            .get(&(owner_user_id, file_version_id))
            .cloned()
            .unwrap_or(ContentReadResolution::NotFound))
    }
}

#[derive(Clone)]
struct MemoryObjectStore {
    state: Arc<Mutex<MemoryObjectStoreState>>,
}

struct MemoryObjectStoreState {
    objects: BTreeMap<String, StoredObject>,
    read_calls: u32,
    range_calls: u32,
    capabilities: StorageCapabilities,
}

impl Default for MemoryObjectStore {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(MemoryObjectStoreState {
                objects: BTreeMap::new(),
                read_calls: 0,
                range_calls: 0,
                capabilities: memory_capabilities(),
            })),
        }
    }
}

#[derive(Clone)]
struct StoredObject {
    bytes: Vec<u8>,
    metadata_length: u64,
    metadata_sha256: Option<Sha256Digest>,
}

impl MemoryObjectStore {
    fn insert(&self, key: &ObjectKey, bytes: &[u8]) {
        let digest = digest(bytes);
        self.state
            .lock()
            .expect("object-store lock")
            .objects
            .insert(
                key.as_str().to_owned(),
                StoredObject {
                    bytes: bytes.to_vec(),
                    metadata_length: bytes.len() as u64,
                    metadata_sha256: Some(digest),
                },
            );
    }

    fn set_metadata_length(&self, key: &ObjectKey, length: u64) {
        self.state
            .lock()
            .expect("object-store lock")
            .objects
            .get_mut(key.as_str())
            .expect("object exists")
            .metadata_length = length;
    }

    fn set_metadata_sha256(&self, key: &ObjectKey, value: Option<Sha256Digest>) {
        self.state
            .lock()
            .expect("object-store lock")
            .objects
            .get_mut(key.as_str())
            .expect("object exists")
            .metadata_sha256 = value;
    }

    fn read_calls(&self) -> u32 {
        self.state.lock().expect("object-store lock").read_calls
    }

    fn range_calls(&self) -> u32 {
        self.state.lock().expect("object-store lock").range_calls
    }

    fn set_capabilities(&self, capabilities: StorageCapabilities) {
        self.state.lock().expect("object-store lock").capabilities = capabilities;
    }
}

fn memory_capabilities() -> StorageCapabilities {
    StorageCapabilities::for_location(
        StorageBackendKind::ObjectStore,
        StorageAvailability::Available,
        synveil_object_store::CapabilityEvidence::AdapterProbe { version: 1 },
    )
    .with_support(
        StorageCapability::Checksumming,
        synveil_object_store::CapabilitySupport::Supported,
    )
    .with_support(
        StorageCapability::RangeReads,
        synveil_object_store::CapabilitySupport::Supported,
    )
}

#[async_trait]
impl ObjectStore for MemoryObjectStore {
    fn capabilities(&self) -> StorageCapabilities {
        self.state
            .lock()
            .expect("object-store lock")
            .capabilities
            .clone()
    }

    async fn put(&self, _request: PutRequest) -> Result<ObjectMetadata, ObjectStoreError> {
        Err(ObjectStoreError::InvalidRequest)
    }

    async fn begin_staged_write(&self) -> Result<StagingHandle, ObjectStoreError> {
        Err(ObjectStoreError::InvalidRequest)
    }

    async fn write_staged(
        &self,
        _handle: &StagingHandle,
        _body: ByteStream,
        _integrity: IntegrityExpectation,
    ) -> Result<StagedMetadata, ObjectStoreError> {
        Err(ObjectStoreError::InvalidRequest)
    }

    async fn append_staged(
        &self,
        _handle: &StagingHandle,
        _expected_offset: u64,
        _chunk: Bytes,
        _maximum_length: u64,
    ) -> Result<StagingProgress, ObjectStoreError> {
        Err(ObjectStoreError::InvalidRequest)
    }

    async fn staging_progress(
        &self,
        _handle: &StagingHandle,
    ) -> Result<StagingProgress, ObjectStoreError> {
        Err(ObjectStoreError::InvalidRequest)
    }

    async fn finalize_staged(
        &self,
        _handle: &StagingHandle,
        _integrity: IntegrityExpectation,
    ) -> Result<StagedMetadata, ObjectStoreError> {
        Err(ObjectStoreError::InvalidRequest)
    }

    async fn promote_temp(
        &self,
        _handle: &StagingHandle,
        _destination: &ObjectKey,
    ) -> Result<PromotionReceipt, ObjectStoreError> {
        Err(ObjectStoreError::InvalidRequest)
    }

    async fn abort_staged(&self, _handle: &StagingHandle) -> Result<(), ObjectStoreError> {
        Err(ObjectStoreError::InvalidRequest)
    }

    async fn get(&self, key: &ObjectKey) -> Result<ObjectRead, ObjectStoreError> {
        let object = {
            let mut state = self.state.lock().expect("object-store lock");
            state.read_calls += 1;
            state
                .objects
                .get(key.as_str())
                .cloned()
                .ok_or(ObjectStoreError::NotFound)?
        };
        Ok(ObjectRead::new(
            ObjectMetadata::new(
                key.clone(),
                object.metadata_length,
                object.metadata_sha256,
                None,
            ),
            None,
            byte_stream(object.bytes),
        ))
    }

    async fn range_read(
        &self,
        key: &ObjectKey,
        range: ByteRange,
    ) -> Result<ObjectRead, ObjectStoreError> {
        let object = {
            let mut state = self.state.lock().expect("object-store lock");
            state.range_calls += 1;
            state
                .objects
                .get(key.as_str())
                .cloned()
                .ok_or(ObjectStoreError::NotFound)?
        };
        if range.end_exclusive() > object.bytes.len() as u64 {
            return Err(ObjectStoreError::InvalidRange);
        }
        let start = usize::try_from(range.start()).map_err(|_| ObjectStoreError::InvalidRange)?;
        let end =
            usize::try_from(range.end_exclusive()).map_err(|_| ObjectStoreError::InvalidRange)?;
        Ok(ObjectRead::new(
            ObjectMetadata::new(
                key.clone(),
                object.metadata_length,
                object.metadata_sha256,
                None,
            ),
            Some(range),
            byte_stream(object.bytes[start..end].to_vec()),
        ))
    }

    async fn exists(&self, key: &ObjectKey) -> Result<bool, ObjectStoreError> {
        Ok(self
            .state
            .lock()
            .expect("object-store lock")
            .objects
            .contains_key(key.as_str()))
    }

    async fn metadata(&self, key: &ObjectKey) -> Result<ObjectMetadata, ObjectStoreError> {
        let object = self
            .state
            .lock()
            .expect("object-store lock")
            .objects
            .get(key.as_str())
            .cloned()
            .ok_or(ObjectStoreError::NotFound)?;
        Ok(ObjectMetadata::new(
            key.clone(),
            object.metadata_length,
            object.metadata_sha256,
            None,
        ))
    }

    async fn delete(&self, _key: &ObjectKey) -> Result<DeleteOutcome, ObjectStoreError> {
        Err(ObjectStoreError::InvalidRequest)
    }

    async fn conditional_delete(
        &self,
        _key: &ObjectKey,
        _expected_version: &ObjectVersion,
    ) -> Result<DeleteOutcome, ObjectStoreError> {
        Err(ObjectStoreError::InvalidRequest)
    }
}

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("synveil-download-test-{}", Uuid::now_v7()));
        fs::create_dir_all(&path).expect("create isolated test root");
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

struct Fixture {
    owner_user_id: UserId,
    node_id: NodeId,
    file_version_id: FileVersionId,
    object_id: ObjectId,
    key: ObjectKey,
    metadata: TestMetadata,
    store: MemoryObjectStore,
    service: ContentReadApplicationService,
}

impl Fixture {
    fn new(bytes: &[u8]) -> Self {
        let owner_user_id = UserId::new();
        let node_id = NodeId::new();
        let file_version_id = FileVersionId::new();
        let object_id = ObjectId::new();
        let key = ObjectKey::new(format!("objects/v1/{object_id}")).expect("valid object key");
        let metadata = TestMetadata::default();
        let store = MemoryObjectStore::default();
        let content = authorized_content(node_id, file_version_id, object_id, &key, bytes);
        metadata.set_current(
            owner_user_id,
            node_id,
            ContentReadResolution::Found(content),
        );
        store.insert(&key, bytes);
        let service =
            ContentReadApplicationService::new(Arc::new(metadata.clone()), Arc::new(store.clone()));
        Self {
            owner_user_id,
            node_id,
            file_version_id,
            object_id,
            key,
            metadata,
            store,
            service,
        }
    }

    fn current_content(&self, bytes: &[u8]) -> AuthorizedContent {
        authorized_content(
            self.node_id,
            self.file_version_id,
            self.object_id,
            &self.key,
            bytes,
        )
    }
}

fn authorized_content(
    node_id: NodeId,
    file_version_id: FileVersionId,
    object_id: ObjectId,
    key: &ObjectKey,
    bytes: &[u8],
) -> AuthorizedContent {
    AuthorizedContent::new(
        node_id,
        file_version_id,
        object_id,
        bytes.len() as u64,
        digest(bytes),
        Revision::new(7),
        Timestamp::parse("2026-08-24T00:00:00Z").expect("fixed timestamp"),
        key.as_str(),
    )
}

fn digest(bytes: &[u8]) -> Sha256Digest {
    let value = Sha256::digest(bytes);
    Sha256Digest::try_from(value.as_slice()).expect("SHA-256 output is valid")
}

fn byte_stream(bytes: Vec<u8>) -> ByteStream {
    boxed_stream(stream::iter([Ok(Bytes::from(bytes))]))
}

async fn collect(descriptor: ContentDescriptor) -> Result<Vec<u8>, ContentReadError> {
    let mut body = descriptor.into_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = body.next().await {
        bytes.extend_from_slice(&chunk?);
    }
    Ok(bytes)
}

#[tokio::test]
async fn current_file_content_is_owner_authorized_and_streamed() {
    let fixture = Fixture::new(b"synveil");

    let descriptor = fixture
        .service
        .open_current_file_content(fixture.owner_user_id, fixture.node_id)
        .await
        .expect("owner can read current file");

    assert_eq!(descriptor.node_id(), fixture.node_id);
    assert_eq!(descriptor.file_version_id(), fixture.file_version_id);
    assert_eq!(descriptor.object_id(), fixture.object_id);
    assert_eq!(descriptor.length(), 7);
    assert_eq!(descriptor.stream_length(), 7);
    assert_eq!(collect(descriptor).await, Ok(b"synveil".to_vec()));
    assert_eq!(fixture.metadata.backend_kinds(), vec!["OBJECT_STORE"]);
}

#[tokio::test]
async fn directory_content_read_is_rejected_without_opening_storage() {
    let fixture = Fixture::new(b"unused");
    fixture.metadata.set_current(
        fixture.owner_user_id,
        fixture.node_id,
        ContentReadResolution::NotAFile,
    );

    let result = fixture
        .service
        .open_current_file_content(fixture.owner_user_id, fixture.node_id)
        .await;

    assert!(matches!(result, Err(ContentReadError::NotAFile)));
    assert_eq!(fixture.store.read_calls(), 0);
}

#[tokio::test]
async fn missing_and_cross_owner_nodes_are_concealed() {
    let fixture = Fixture::new(b"private");

    let missing = fixture
        .service
        .open_current_file_content(fixture.owner_user_id, NodeId::new())
        .await;
    let cross_owner = fixture
        .service
        .open_current_file_content(UserId::new(), fixture.node_id)
        .await;

    assert!(matches!(missing, Err(ContentReadError::ContentNotFound)));
    assert!(matches!(
        cross_owner,
        Err(ContentReadError::ContentNotFound)
    ));
    assert_eq!(fixture.store.read_calls(), 0);
}

#[tokio::test]
async fn trashed_nodes_follow_the_normal_content_concealment_contract() {
    let fixture = Fixture::new(b"trashed");
    fixture.metadata.set_current(
        fixture.owner_user_id,
        fixture.node_id,
        ContentReadResolution::NotFound,
    );

    let result = fixture
        .service
        .open_current_file_content(fixture.owner_user_id, fixture.node_id)
        .await;

    assert!(matches!(result, Err(ContentReadError::ContentNotFound)));
    assert_eq!(fixture.store.read_calls(), 0);
}

#[tokio::test]
async fn historical_version_is_bound_to_immutable_bytes_after_a_newer_current_version() {
    let fixture = Fixture::new(b"new content");
    let historical_version_id = FileVersionId::new();
    let historical_object_id = ObjectId::new();
    let historical_key =
        ObjectKey::new(format!("objects/v1/{historical_object_id}")).expect("valid historical key");
    fixture.store.insert(&historical_key, b"old content");
    fixture.metadata.set_historical(
        fixture.owner_user_id,
        historical_version_id,
        ContentReadResolution::Found(authorized_content(
            fixture.node_id,
            historical_version_id,
            historical_object_id,
            &historical_key,
            b"old content",
        )),
    );

    let descriptor = fixture
        .service
        .open_file_version_content(fixture.owner_user_id, historical_version_id)
        .await
        .expect("old immutable version remains readable");

    assert_eq!(descriptor.file_version_id(), historical_version_id);
    assert_eq!(collect(descriptor).await, Ok(b"old content".to_vec()));
}

#[tokio::test]
async fn metadata_preflight_does_not_open_object_bytes() {
    let fixture = Fixture::new(b"metadata only");

    let metadata = fixture
        .service
        .current_content_metadata(fixture.owner_user_id, fixture.node_id)
        .await
        .expect("content metadata resolves");

    assert_eq!(metadata.node_id(), fixture.node_id);
    assert_eq!(metadata.file_version_id(), fixture.file_version_id);
    assert_eq!(metadata.length(), 13);
    assert_eq!(metadata.sha256(), digest(b"metadata only"));
    assert_eq!(fixture.metadata.current_calls(), 1);
    assert_eq!(fixture.store.read_calls(), 0);
    assert_eq!(fixture.store.range_calls(), 0);
}

#[tokio::test]
async fn historical_content_range_reads_preserve_immutable_version_identity() {
    let fixture = Fixture::new(b"current bytes");
    let historical_version_id = FileVersionId::new();
    let historical_object_id = ObjectId::new();
    let historical_key =
        ObjectKey::new(format!("objects/v1/{historical_object_id}")).expect("valid history key");
    fixture.store.insert(&historical_key, b"old bytes");
    fixture.metadata.set_historical(
        fixture.owner_user_id,
        historical_version_id,
        ContentReadResolution::Found(authorized_content(
            fixture.node_id,
            historical_version_id,
            historical_object_id,
            &historical_key,
            b"old bytes",
        )),
    );

    let range = ByteRange::new(4, 8).expect("valid history range");
    let descriptor = fixture
        .service
        .open_file_version_content_range(fixture.owner_user_id, historical_version_id, range)
        .await
        .expect("historical range read");

    assert_eq!(descriptor.file_version_id(), historical_version_id);
    assert_eq!(descriptor.range(), Some(range));
    assert_eq!(descriptor.stream_length(), 4);
    assert_eq!(collect(descriptor).await, Ok(b"byte".to_vec()));
    assert_eq!(fixture.store.range_calls(), 1);
}

#[tokio::test]
async fn historical_version_cross_owner_access_is_concealed() {
    let fixture = Fixture::new(b"private history");
    fixture.metadata.set_historical(
        fixture.owner_user_id,
        fixture.file_version_id,
        ContentReadResolution::Found(fixture.current_content(b"private history")),
    );

    let result = fixture
        .service
        .open_file_version_content(UserId::new(), fixture.file_version_id)
        .await;

    assert!(matches!(result, Err(ContentReadError::VersionNotFound)));
    assert_eq!(fixture.store.read_calls(), 0);
}

#[tokio::test]
async fn zero_byte_content_streams_without_special_case_mutation() {
    let fixture = Fixture::new(b"");

    let descriptor = fixture
        .service
        .open_current_file_content(fixture.owner_user_id, fixture.node_id)
        .await
        .expect("zero-byte object is valid immutable content");

    assert_eq!(descriptor.length(), 0);
    assert_eq!(descriptor.stream_length(), 0);
    assert_eq!(collect(descriptor).await, Ok(Vec::new()));
}

#[tokio::test]
async fn current_content_range_reads_support_interior_and_final_byte() {
    let fixture = Fixture::new(b"synveil");
    let interior = ByteRange::new(1, 5).expect("valid interior range");
    let final_byte = ByteRange::new(6, 7).expect("valid final-byte range");

    let interior_descriptor = fixture
        .service
        .open_file_content_range(fixture.owner_user_id, fixture.node_id, interior)
        .await
        .expect("interior range reads");
    let final_descriptor = fixture
        .service
        .open_file_content_range(fixture.owner_user_id, fixture.node_id, final_byte)
        .await
        .expect("final byte reads");

    assert_eq!(interior_descriptor.range(), Some(interior));
    assert_eq!(interior_descriptor.stream_length(), 4);
    assert_eq!(collect(interior_descriptor).await, Ok(b"ynve".to_vec()));
    assert_eq!(collect(final_descriptor).await, Ok(b"l".to_vec()));
    assert_eq!(fixture.store.range_calls(), 2);
}

#[tokio::test]
async fn ranges_past_the_canonical_length_fail_before_storage_is_opened() {
    let fixture = Fixture::new(b"short");
    let range = ByteRange::new(4, 6).expect("shape itself is valid");

    let result = fixture
        .service
        .open_file_content_range(fixture.owner_user_id, fixture.node_id, range)
        .await;

    assert!(matches!(result, Err(ContentReadError::InvalidRange)));
    assert_eq!(fixture.store.range_calls(), 0);
}

#[tokio::test]
async fn required_storage_capabilities_are_checked_before_metadata_or_bytes() {
    let fixture = Fixture::new(b"capability gate");
    fixture.store.set_capabilities(
        StorageCapabilities::for_location(
            StorageBackendKind::ObjectStore,
            StorageAvailability::Available,
            synveil_object_store::CapabilityEvidence::AdapterProbe { version: 1 },
        )
        .with_support(StorageCapability::RangeReads, CapabilitySupport::Supported),
    );

    let full = fixture
        .service
        .open_current_file_content(fixture.owner_user_id, fixture.node_id)
        .await;

    assert!(matches!(full, Err(ContentReadError::StorageUnavailable)));
    assert_eq!(fixture.metadata.current_calls(), 0);
    assert_eq!(fixture.store.read_calls(), 0);

    fixture.store.set_capabilities(
        StorageCapabilities::for_location(
            StorageBackendKind::ObjectStore,
            StorageAvailability::Available,
            synveil_object_store::CapabilityEvidence::AdapterProbe { version: 1 },
        )
        .with_support(
            StorageCapability::Checksumming,
            CapabilitySupport::Supported,
        ),
    );
    let range = fixture
        .service
        .open_file_content_range(
            fixture.owner_user_id,
            fixture.node_id,
            ByteRange::new(0, 1).expect("valid range"),
        )
        .await;

    assert!(matches!(range, Err(ContentReadError::StorageUnavailable)));
    assert_eq!(fixture.metadata.current_calls(), 0);
    assert_eq!(fixture.store.range_calls(), 0);
}

#[test]
fn empty_and_overflowing_ranges_are_rejected_by_the_shared_object_store_contract() {
    assert!(matches!(
        ByteRange::new(2, 2),
        Err(ObjectStoreError::InvalidRange)
    ));
    assert!(matches!(
        ByteRange::from_start_length(u64::MAX, 1),
        Err(ObjectStoreError::InvalidRange)
    ));
}

#[tokio::test]
async fn object_metadata_length_mismatch_fails_closed() {
    let fixture = Fixture::new(b"length");
    fixture.store.set_metadata_length(&fixture.key, 7);

    let result = fixture
        .service
        .open_current_file_content(fixture.owner_user_id, fixture.node_id)
        .await;

    assert!(matches!(result, Err(ContentReadError::IntegrityMismatch)));
}

#[tokio::test]
async fn object_metadata_hash_mismatch_fails_closed() {
    let fixture = Fixture::new(b"digest");
    fixture
        .store
        .set_metadata_sha256(&fixture.key, Some(digest(b"different")));

    let result = fixture
        .service
        .open_current_file_content(fixture.owner_user_id, fixture.node_id)
        .await;

    assert!(matches!(result, Err(ContentReadError::IntegrityMismatch)));
}

#[tokio::test]
async fn missing_and_unverified_replicas_are_not_readable() {
    let fixture = Fixture::new(b"available only in a bad replica");
    fixture.metadata.set_current(
        fixture.owner_user_id,
        fixture.node_id,
        ContentReadResolution::ContentUnavailable,
    );

    let missing = fixture
        .service
        .open_current_file_content(fixture.owner_user_id, fixture.node_id)
        .await;
    let unverified = fixture
        .service
        .open_file_content_range(
            fixture.owner_user_id,
            fixture.node_id,
            ByteRange::new(0, 1).expect("valid range"),
        )
        .await;

    assert!(matches!(missing, Err(ContentReadError::ContentUnavailable)));
    assert!(matches!(
        unverified,
        Err(ContentReadError::ContentUnavailable)
    ));
    assert_eq!(fixture.store.read_calls(), 0);
    assert_eq!(fixture.store.range_calls(), 0);
}

#[tokio::test]
async fn missing_backend_bytes_are_content_unavailable_not_not_found() {
    let fixture = Fixture::new(b"missing backend bytes");
    fixture
        .store
        .state
        .lock()
        .expect("object-store lock")
        .objects
        .remove(fixture.key.as_str());

    let result = fixture
        .service
        .open_current_file_content(fixture.owner_user_id, fixture.node_id)
        .await;

    assert!(matches!(result, Err(ContentReadError::ContentUnavailable)));
}

#[tokio::test]
async fn dropping_a_stream_has_no_metadata_or_object_mutation_side_effect() {
    let fixture = Fixture::new(b"do not consume this stream");
    let descriptor = fixture
        .service
        .open_current_file_content(fixture.owner_user_id, fixture.node_id)
        .await
        .expect("descriptor opens without consuming bytes");
    drop(descriptor);

    assert_eq!(fixture.metadata.current_calls(), 1);
    assert_eq!(fixture.store.read_calls(), 1);
    assert!(
        fixture
            .store
            .state
            .lock()
            .expect("object-store lock")
            .objects
            .contains_key(fixture.key.as_str())
    );
}

#[tokio::test]
async fn production_local_adapter_supports_one_full_and_one_range_read() {
    let root = TempRoot::new();
    let store = LocalFilesystemObjectStore::open(root.path()).expect("open local store");
    let owner_user_id = UserId::new();
    let node_id = NodeId::new();
    let file_version_id = FileVersionId::new();
    let object_id = ObjectId::new();
    let key = ObjectKey::new(format!("objects/v1/{object_id}")).expect("valid object key");
    let bytes = b"local adapter bytes";
    store
        .put(
            PutRequest::new(key.clone(), byte_stream(bytes.to_vec())).with_integrity(
                IntegrityExpectation::none()
                    .with_length(bytes.len() as u64)
                    .with_sha256(digest(bytes)),
            ),
        )
        .await
        .expect("seed committed local object");

    let metadata = TestMetadata::default();
    metadata.set_current(
        owner_user_id,
        node_id,
        ContentReadResolution::Found(authorized_content(
            node_id,
            file_version_id,
            object_id,
            &key,
            bytes,
        )),
    );
    let service = ContentReadApplicationService::new(Arc::new(metadata), Arc::new(store));

    let full = service
        .open_current_file_content(owner_user_id, node_id)
        .await
        .expect("local full read");
    let range = service
        .open_file_content_range(
            owner_user_id,
            node_id,
            ByteRange::new(6, 13).expect("local range"),
        )
        .await
        .expect("local range read");

    assert_eq!(collect(full).await, Ok(bytes.to_vec()));
    assert_eq!(collect(range).await, Ok(b"adapter".to_vec()));
}
