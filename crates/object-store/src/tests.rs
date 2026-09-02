use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use bytes::Bytes;
use futures_util::{StreamExt, stream};
use sha2::{Digest, Sha256};
use synveil_core::Sha256Digest;

use super::*;

#[derive(Clone)]
struct MemoryStore {
    state: Arc<Mutex<MemoryState>>,
    capabilities: StorageCapabilities,
}

struct MemoryState {
    next_staging: u64,
    next_version: u64,
    staging: BTreeMap<StagingHandle, Option<StoredStaging>>,
    partial_staging: BTreeMap<StagingHandle, Vec<u8>>,
    objects: BTreeMap<ObjectKey, StoredObject>,
}

#[derive(Clone)]
struct StoredStaging {
    bytes: Vec<u8>,
    metadata: StagedMetadata,
}

#[derive(Clone)]
struct StoredObject {
    bytes: Vec<u8>,
    metadata: ObjectMetadata,
}

impl MemoryStore {
    fn new() -> Self {
        let capabilities = StorageCapabilities::for_location(
            StorageBackendKind::ObjectStore,
            StorageAvailability::Available,
            CapabilityEvidence::ConformanceTested { version: 1 },
        )
        .with_support(
            StorageCapability::AtomicPromotion,
            CapabilitySupport::Supported,
        )
        .with_support(
            StorageCapability::ExclusiveCreate,
            CapabilitySupport::Supported,
        )
        .with_support(StorageCapability::RangeReads, CapabilitySupport::Supported)
        .with_support(
            StorageCapability::ReadAfterWrite,
            CapabilitySupport::Supported,
        )
        .with_support(
            StorageCapability::ConditionalDelete,
            CapabilitySupport::Supported,
        )
        .with_support(
            StorageCapability::Checksumming,
            CapabilitySupport::Supported,
        )
        .with_support(
            StorageCapability::DurableFlush,
            CapabilitySupport::Unsupported,
        )
        .with_support(
            StorageCapability::Compression,
            CapabilitySupport::Unsupported,
        )
        .with_support(
            StorageCapability::NativeSnapshot,
            CapabilitySupport::Unsupported,
        );

        Self {
            state: Arc::new(Mutex::new(MemoryState {
                next_staging: 0,
                next_version: 0,
                staging: BTreeMap::new(),
                partial_staging: BTreeMap::new(),
                objects: BTreeMap::new(),
            })),
            capabilities,
        }
    }

    fn next_staging(state: &mut MemoryState) -> StagingHandle {
        state.next_staging += 1;
        StagingHandle::new(format!("staging-{}", state.next_staging)).expect("test handle")
    }

    fn next_version(state: &mut MemoryState) -> ObjectVersion {
        state.next_version += 1;
        ObjectVersion::new(format!("version-{}", state.next_version)).expect("test version")
    }
}

fn digest(bytes: &[u8]) -> Sha256Digest {
    let computed = Sha256::digest(bytes);
    let mut output = [0_u8; 32];
    output.copy_from_slice(&computed);
    Sha256Digest::from_bytes(output)
}

async fn collect_body(mut body: ByteStream) -> Result<Vec<u8>, ObjectStoreError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = body.next().await {
        bytes.extend_from_slice(&chunk?);
    }
    Ok(bytes)
}

fn verify_integrity(
    bytes: &[u8],
    integrity: IntegrityExpectation,
) -> Result<Sha256Digest, ObjectStoreError> {
    if let Some(expected_length) = integrity.expected_length()
        && expected_length != bytes.len() as u64
    {
        return Err(ObjectStoreError::IntegrityMismatch);
    }

    let checksum = digest(bytes);
    if integrity
        .expected_sha256()
        .is_some_and(|expected| expected != checksum)
    {
        return Err(ObjectStoreError::IntegrityMismatch);
    }
    Ok(checksum)
}

fn streamed_read(
    metadata: ObjectMetadata,
    bytes: Vec<u8>,
    range: Option<ByteRange>,
) -> Result<ObjectRead, ObjectStoreError> {
    let bytes = if let Some(range) = range {
        let start = usize::try_from(range.start()).map_err(|_| ObjectStoreError::InvalidRange)?;
        let end =
            usize::try_from(range.end_exclusive()).map_err(|_| ObjectStoreError::InvalidRange)?;
        if end > bytes.len() || start >= end {
            return Err(ObjectStoreError::InvalidRange);
        }
        bytes[start..end].to_vec()
    } else {
        bytes
    };

    Ok(ObjectRead::new(
        metadata,
        range,
        boxed_stream(stream::iter(vec![Ok(Bytes::from(bytes))])),
    ))
}

fn chunks(parts: &[&[u8]]) -> ByteStream {
    let chunks = parts
        .iter()
        .map(|part| Ok(Bytes::copy_from_slice(part)))
        .collect::<Vec<_>>();
    boxed_stream(stream::iter(chunks))
}

#[async_trait::async_trait]
impl ObjectStore for MemoryStore {
    fn capabilities(&self) -> StorageCapabilities {
        self.capabilities.clone()
    }

    async fn put(&self, request: PutRequest) -> Result<ObjectMetadata, ObjectStoreError> {
        let (key, body, integrity) = request.into_parts();
        let bytes = collect_body(body).await?;
        let checksum = verify_integrity(&bytes, integrity)?;
        let mut state = self.state.lock().expect("memory store lock");
        if state.objects.contains_key(&key) {
            return Err(ObjectStoreError::AlreadyExists);
        }

        let metadata = ObjectMetadata::new(
            key.clone(),
            bytes.len() as u64,
            Some(checksum),
            Some(Self::next_version(&mut state)),
        );
        state.objects.insert(
            key,
            StoredObject {
                bytes,
                metadata: metadata.clone(),
            },
        );
        Ok(metadata)
    }

    async fn begin_staged_write(&self) -> Result<StagingHandle, ObjectStoreError> {
        let mut state = self.state.lock().expect("memory store lock");
        let handle = Self::next_staging(&mut state);
        state.staging.insert(handle.clone(), None);
        state.partial_staging.insert(handle.clone(), Vec::new());
        Ok(handle)
    }

    async fn write_staged(
        &self,
        handle: &StagingHandle,
        body: ByteStream,
        integrity: IntegrityExpectation,
    ) -> Result<StagedMetadata, ObjectStoreError> {
        let bytes = collect_body(body).await?;
        let checksum = verify_integrity(&bytes, integrity)?;
        let metadata = StagedMetadata::new(handle.clone(), bytes.len() as u64, Some(checksum));
        let mut state = self.state.lock().expect("memory store lock");
        let Some(staging) = state.staging.get_mut(handle) else {
            return Err(ObjectStoreError::StagingNotFound);
        };
        *staging = Some(StoredStaging {
            bytes,
            metadata: metadata.clone(),
        });
        state.partial_staging.remove(handle);
        Ok(metadata)
    }

    async fn append_staged(
        &self,
        handle: &StagingHandle,
        expected_offset: u64,
        chunk: Bytes,
        maximum_length: u64,
    ) -> Result<StagingProgress, ObjectStoreError> {
        let mut state = self.state.lock().expect("memory store lock");
        let Some(staging) = state.staging.get(handle) else {
            return Err(ObjectStoreError::StagingNotFound);
        };
        if staging.is_some() {
            return Err(ObjectStoreError::StagingConflict);
        }
        let bytes = state
            .partial_staging
            .get_mut(handle)
            .ok_or(ObjectStoreError::StagingNotFound)?;
        if bytes.len() as u64 != expected_offset {
            return Err(ObjectStoreError::PreconditionFailed);
        }
        let next_length = expected_offset
            .checked_add(chunk.len() as u64)
            .ok_or(ObjectStoreError::IntegrityMismatch)?;
        if next_length > maximum_length {
            return Err(ObjectStoreError::IntegrityMismatch);
        }
        bytes.extend_from_slice(&chunk);
        Ok(StagingProgress::partial(handle.clone(), next_length))
    }

    async fn staging_progress(
        &self,
        handle: &StagingHandle,
    ) -> Result<StagingProgress, ObjectStoreError> {
        let state = self.state.lock().expect("memory store lock");
        let Some(staging) = state.staging.get(handle) else {
            return Err(ObjectStoreError::StagingNotFound);
        };
        if let Some(staging) = staging {
            return Ok(StagingProgress::verified(
                handle.clone(),
                staging.metadata.length(),
                *staging
                    .metadata
                    .sha256()
                    .ok_or(ObjectStoreError::IntegrityMismatch)?,
            ));
        }
        let length = state
            .partial_staging
            .get(handle)
            .ok_or(ObjectStoreError::StagingNotFound)?
            .len() as u64;
        Ok(StagingProgress::partial(handle.clone(), length))
    }

    async fn finalize_staged(
        &self,
        handle: &StagingHandle,
        integrity: IntegrityExpectation,
    ) -> Result<StagedMetadata, ObjectStoreError> {
        let mut state = self.state.lock().expect("memory store lock");
        let Some(staging) = state.staging.get(handle) else {
            return Err(ObjectStoreError::StagingNotFound);
        };
        if let Some(staging) = staging {
            let checksum = staging
                .metadata
                .sha256()
                .copied()
                .ok_or(ObjectStoreError::IntegrityMismatch)?;
            if integrity
                .expected_length()
                .is_some_and(|expected| expected != staging.metadata.length())
                || integrity
                    .expected_sha256()
                    .is_some_and(|expected| expected != checksum)
            {
                return Err(ObjectStoreError::IntegrityMismatch);
            }
            return Ok(staging.metadata.clone());
        }
        let bytes = state
            .partial_staging
            .remove(handle)
            .ok_or(ObjectStoreError::StagingNotFound)?;
        let checksum = verify_integrity(&bytes, integrity)?;
        let metadata = StagedMetadata::new(handle.clone(), bytes.len() as u64, Some(checksum));
        let staging = state
            .staging
            .get_mut(handle)
            .ok_or(ObjectStoreError::StagingNotFound)?;
        *staging = Some(StoredStaging {
            bytes,
            metadata: metadata.clone(),
        });
        Ok(metadata)
    }

    async fn promote_temp(
        &self,
        handle: &StagingHandle,
        destination: &ObjectKey,
    ) -> Result<PromotionReceipt, ObjectStoreError> {
        let mut state = self.state.lock().expect("memory store lock");
        if state.objects.contains_key(destination) {
            return Err(ObjectStoreError::AlreadyExists);
        }
        let Some(Some(staging)) = state.staging.get(handle) else {
            return Err(ObjectStoreError::StagingConflict);
        };
        let staging = staging.clone();
        let metadata = ObjectMetadata::new(
            destination.clone(),
            staging.metadata.length(),
            staging.metadata.sha256().copied(),
            Some(Self::next_version(&mut state)),
        );
        state.objects.insert(
            destination.clone(),
            StoredObject {
                bytes: staging.bytes,
                metadata: metadata.clone(),
            },
        );
        state.staging.remove(handle);
        Ok(PromotionReceipt::new(metadata))
    }

    async fn abort_staged(&self, handle: &StagingHandle) -> Result<(), ObjectStoreError> {
        let mut state = self.state.lock().expect("memory store lock");
        state.staging.remove(handle);
        state.partial_staging.remove(handle);
        Ok(())
    }

    async fn get(&self, key: &ObjectKey) -> Result<ObjectRead, ObjectStoreError> {
        let state = self.state.lock().expect("memory store lock");
        let Some(object) = state.objects.get(key) else {
            return Err(ObjectStoreError::NotFound);
        };
        streamed_read(object.metadata.clone(), object.bytes.clone(), None)
    }

    async fn range_read(
        &self,
        key: &ObjectKey,
        range: ByteRange,
    ) -> Result<ObjectRead, ObjectStoreError> {
        let state = self.state.lock().expect("memory store lock");
        let Some(object) = state.objects.get(key) else {
            return Err(ObjectStoreError::NotFound);
        };
        streamed_read(object.metadata.clone(), object.bytes.clone(), Some(range))
    }

    async fn exists(&self, key: &ObjectKey) -> Result<bool, ObjectStoreError> {
        let state = self.state.lock().expect("memory store lock");
        Ok(state.objects.contains_key(key))
    }

    async fn metadata(&self, key: &ObjectKey) -> Result<ObjectMetadata, ObjectStoreError> {
        let state = self.state.lock().expect("memory store lock");
        state
            .objects
            .get(key)
            .map(|object| object.metadata.clone())
            .ok_or(ObjectStoreError::NotFound)
    }

    async fn delete(&self, key: &ObjectKey) -> Result<DeleteOutcome, ObjectStoreError> {
        let mut state = self.state.lock().expect("memory store lock");
        Ok(if state.objects.remove(key).is_some() {
            DeleteOutcome::Deleted
        } else {
            DeleteOutcome::AlreadyAbsent
        })
    }

    async fn conditional_delete(
        &self,
        key: &ObjectKey,
        expected_version: &ObjectVersion,
    ) -> Result<DeleteOutcome, ObjectStoreError> {
        let mut state = self.state.lock().expect("memory store lock");
        let Some(object) = state.objects.get(key) else {
            return Ok(DeleteOutcome::AlreadyAbsent);
        };
        if object.metadata.version() != Some(expected_version) {
            return Err(ObjectStoreError::PreconditionFailed);
        }
        state.objects.remove(key);
        Ok(DeleteOutcome::Deleted)
    }
}

#[test]
fn object_keys_reject_traversal_and_redact_physical_values() {
    assert!(ObjectKey::new("objects/v1/valid-key").is_ok());
    assert!(ObjectKey::new("../outside").is_err());
    assert!(ObjectKey::new("objects\\outside").is_err());
    assert!(ObjectKey::new("objects/user filename").is_err());

    let key = ObjectKey::new("objects/v1/secret-key").expect("valid test key");
    assert!(!format!("{key:?}").contains("secret-key"));
}

#[tokio::test]
async fn in_memory_store_proves_streaming_staging_promotion_and_conditional_delete() {
    let store = MemoryStore::new();
    let key = ObjectKey::new("objects/v1/one").expect("valid object key");
    let content = b"hello object store";
    let expected = digest(content);
    let metadata = store
        .put(
            PutRequest::new(key.clone(), chunks(&[b"hello ", b"object ", b"store"]))
                .with_integrity(
                    IntegrityExpectation::none()
                        .with_length(content.len() as u64)
                        .with_sha256(expected),
                ),
        )
        .await
        .expect("streamed put");

    assert_eq!(metadata.length(), content.len() as u64);
    assert_eq!(metadata.sha256(), Some(&expected));
    let metadata_debug = format!("{metadata:?}");
    assert!(metadata_debug.contains("<redacted>"));
    assert!(!metadata_debug.contains(&expected.to_string()));
    assert_eq!(
        store
            .put(PutRequest::new(key.clone(), chunks(&[b"replacement"])))
            .await,
        Err(ObjectStoreError::AlreadyExists)
    );

    let read = store.get(&key).await.expect("full read");
    assert_eq!(read.range(), None);
    assert_eq!(collect_body(read.into_stream()).await.unwrap(), content);

    let range = ByteRange::new(6, 12).expect("valid range");
    let read = store.range_read(&key, range).await.expect("range read");
    assert_eq!(read.range(), Some(range));
    assert_eq!(collect_body(read.into_stream()).await.unwrap(), b"object");
    assert_eq!(ByteRange::new(4, 4), Err(ObjectStoreError::InvalidRange));
    assert!(matches!(
        store
            .range_read(&key, ByteRange::new(0, 128).expect("valid shape"))
            .await,
        Err(ObjectStoreError::InvalidRange)
    ));

    let handle = store.begin_staged_write().await.expect("staging handle");
    let staged = store
        .write_staged(
            &handle,
            chunks(&[b"staged ", b"bytes"]),
            IntegrityExpectation::none(),
        )
        .await
        .expect("staged write");
    assert_eq!(staged.length(), 12);

    let promoted_key = ObjectKey::new("objects/v1/promoted").expect("valid object key");
    let receipt = store
        .promote_temp(&handle, &promoted_key)
        .await
        .expect("promotion");
    let version = receipt.metadata().version().expect("test version").clone();
    let wrong_version = ObjectVersion::new("wrong-version").expect("test version");
    assert_eq!(
        store
            .conditional_delete(&promoted_key, &wrong_version)
            .await,
        Err(ObjectStoreError::PreconditionFailed)
    );
    assert_eq!(
        store
            .conditional_delete(&promoted_key, &version)
            .await
            .unwrap(),
        DeleteOutcome::Deleted
    );
    assert!(!store.exists(&promoted_key).await.unwrap());
}

#[tokio::test]
async fn in_memory_store_enforces_resumable_offsets_and_idempotent_finalize() {
    let store = MemoryStore::new();
    let handle = store.begin_staged_write().await.expect("begin staging");
    assert_eq!(
        store
            .append_staged(&handle, 0, Bytes::from_static(b"abc"), 6)
            .await
            .expect("append first chunk")
            .length(),
        3
    );
    assert_eq!(
        store
            .append_staged(&handle, 0, Bytes::from_static(b"abc"), 6)
            .await,
        Err(ObjectStoreError::PreconditionFailed)
    );
    assert_eq!(
        store
            .staging_progress(&handle)
            .await
            .expect("inspect partial progress")
            .length(),
        3
    );
    store
        .append_staged(&handle, 3, Bytes::from_static(b"def"), 6)
        .await
        .expect("append second chunk");
    let expected = digest(b"abcdef");
    let expectation = IntegrityExpectation::none()
        .with_length(6)
        .with_sha256(expected);
    let first = store
        .finalize_staged(&handle, expectation)
        .await
        .expect("finalize append-built staging");
    let second = store
        .finalize_staged(&handle, expectation)
        .await
        .expect("repeat finalize");
    assert_eq!(first, second);
}

#[tokio::test]
async fn in_memory_store_passes_the_shared_conformance_suite() {
    let store = MemoryStore::new();
    crate::conformance::run_basic_conformance(&store).await;
}
