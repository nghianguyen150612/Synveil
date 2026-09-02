use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use bytes::Bytes;
use sha2::{Digest, Sha256};
use synveil_core::{
    FileVersionId, LibraryId, LogicalName, NodeId, Revision, Sha256Digest, Timestamp,
    UploadSessionId, UploadSessionState, UserId,
};
use synveil_metadata::{
    MappingError, MetadataError, NewUploadSession, UploadClaim, UploadCleanupCandidate,
    UploadCompletion, UploadDurabilityReceipt, UploadFinalization, UploadMetadataBackend,
    UploadSessionRecord,
};
use synveil_object_store::{IntegrityExpectation, ObjectKey, ObjectStore, StagingHandle};
use synveil_storage::{
    CreateUploadSessionRequest, LocalFilesystemObjectStore, UploadApplicationService, UploadError,
    UploadLimits, UploadTargetRequest, boxed_upload_stream,
};
use uuid::Uuid;

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("synveil-upload-session-test-{}", Uuid::now_v7()));
        fs::create_dir_all(&path).expect("create test root");
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

#[derive(Clone, Default)]
struct TestMetadata {
    state: Arc<Mutex<TestMetadataState>>,
}

#[derive(Default)]
struct TestMetadataState {
    records: BTreeMap<UploadSessionId, UploadSessionRecord>,
    finalize_calls: u32,
    force_version_conflict: bool,
}

impl TestMetadata {
    fn finalize_calls(&self) -> u32 {
        self.state.lock().expect("metadata lock").finalize_calls
    }

    fn force_version_conflict(&self) {
        self.state
            .lock()
            .expect("metadata lock")
            .force_version_conflict = true;
    }

    fn record(&self, session_id: UploadSessionId) -> UploadSessionRecord {
        self.state
            .lock()
            .expect("metadata lock")
            .records
            .get(&session_id)
            .cloned()
            .expect("test session record")
    }
}

fn not_found() -> MetadataError {
    MetadataError::Mapping(MappingError::RelationMismatch {
        relation: "upload_sessions.id",
    })
}

fn record_from_input(input: NewUploadSession) -> UploadSessionRecord {
    UploadSessionRecord {
        id: input.id,
        owner_user_id: input.owner_user_id,
        library_id: input.library_id,
        operation: input.operation,
        target_node_id: input.target_node_id,
        target_parent_node_id: input.target_parent_node_id,
        target_name: input.target_name,
        expected_node_revision: input.expected_node_revision,
        expected_length: input.expected_length,
        expected_sha256: input.expected_sha256,
        object_id: input.object_id,
        object_replica_id: input.object_replica_id,
        object_key: input.object_key,
        staging_handle: input.staging_handle,
        bytes_received: 0,
        state: UploadSessionState::Open,
        lease_generation: 0,
        lease_expires_at: None,
        cancel_requested_at: None,
        created_at: input.created_at,
        updated_at: input.created_at,
        expires_at: input.expires_at,
        last_error_code: None,
        terminal_failure_code: None,
        durability: None,
        completion: None,
    }
}

fn owned_record(
    records: &mut BTreeMap<UploadSessionId, UploadSessionRecord>,
    owner_user_id: UserId,
    session_id: UploadSessionId,
) -> Result<&mut UploadSessionRecord, MetadataError> {
    let record = records.get_mut(&session_id).ok_or_else(not_found)?;
    if record.owner_user_id != owner_user_id {
        return Err(not_found());
    }
    Ok(record)
}

#[async_trait]
impl UploadMetadataBackend for TestMetadata {
    async fn create_upload_session(
        &self,
        input: NewUploadSession,
    ) -> Result<UploadSessionRecord, MetadataError> {
        let mut state = self.state.lock().expect("metadata lock");
        let record = record_from_input(input);
        state.records.insert(record.id, record.clone());
        Ok(record)
    }

    async fn find_upload_session(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
    ) -> Result<Option<UploadSessionRecord>, MetadataError> {
        let state = self.state.lock().expect("metadata lock");
        Ok(state
            .records
            .get(&session_id)
            .filter(|record| record.owner_user_id == owner_user_id)
            .cloned())
    }

    async fn record_upload_progress(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        expected_offset: u64,
        new_offset: u64,
        observed_at: Timestamp,
    ) -> Result<UploadSessionRecord, MetadataError> {
        let mut state = self.state.lock().expect("metadata lock");
        let record = owned_record(&mut state.records, owner_user_id, session_id)?;
        if record.state == UploadSessionState::Open && record.bytes_received == expected_offset {
            record.bytes_received = new_offset;
            record.updated_at = observed_at;
            record.last_error_code = None;
        }
        Ok(record.clone())
    }

    async fn claim_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        observed_at: Timestamp,
        lease_until: Timestamp,
    ) -> Result<UploadClaim, MetadataError> {
        let mut state = self.state.lock().expect("metadata lock");
        let record = owned_record(&mut state.records, owner_user_id, session_id)?;
        if record.state == UploadSessionState::Open
            && record.expires_at.as_offset_datetime() <= observed_at.as_offset_datetime()
        {
            record.state = UploadSessionState::Expired;
            record.last_error_code = Some("upload_expired".to_owned());
            record.updated_at = observed_at;
            return Ok(UploadClaim::Terminal(record.clone()));
        }
        if record.state.is_terminal() {
            return Ok(UploadClaim::Terminal(record.clone()));
        }
        if record
            .lease_expires_at
            .is_some_and(|until| until.as_offset_datetime() > observed_at.as_offset_datetime())
        {
            return Ok(UploadClaim::Busy(record.clone()));
        }
        let next_state = match record.state {
            UploadSessionState::Open | UploadSessionState::Verifying => {
                UploadSessionState::Verifying
            }
            UploadSessionState::Committing => UploadSessionState::Committing,
            state => state,
        };
        record.state = next_state;
        record.lease_generation =
            record
                .lease_generation
                .checked_add(1)
                .ok_or(MetadataError::Mapping(MappingError::InvalidDecimal {
                    field: "upload_sessions.lease_generation",
                }))?;
        record.lease_expires_at = Some(lease_until);
        record.updated_at = observed_at;
        Ok(UploadClaim::Acquired(record.clone()))
    }

    async fn record_upload_durable(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        lease_generation: u64,
        receipt: UploadDurabilityReceipt,
        observed_at: Timestamp,
    ) -> Result<UploadSessionRecord, MetadataError> {
        let mut state = self.state.lock().expect("metadata lock");
        let record = owned_record(&mut state.records, owner_user_id, session_id)?;
        if record.state == UploadSessionState::Verifying
            && record.lease_generation == lease_generation
            && receipt.storage_key == record.object_key
            && receipt.length == record.expected_length
            && record
                .expected_sha256
                .is_none_or(|expected| expected == receipt.sha256)
        {
            record.state = UploadSessionState::Committing;
            record.lease_expires_at = Some(observed_at);
            record.updated_at = observed_at;
            record.last_error_code = None;
            record.durability = Some(receipt);
        }
        Ok(record.clone())
    }

    async fn release_upload_lease(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        lease_generation: u64,
        safe_error_code: &'static str,
        observed_at: Timestamp,
    ) -> Result<(), MetadataError> {
        let mut state = self.state.lock().expect("metadata lock");
        let record = owned_record(&mut state.records, owner_user_id, session_id)?;
        if record.lease_generation == lease_generation
            && matches!(
                record.state,
                UploadSessionState::Verifying | UploadSessionState::Committing
            )
        {
            record.lease_expires_at = Some(observed_at);
            record.last_error_code = Some(safe_error_code.to_owned());
            record.updated_at = observed_at;
        }
        Ok(())
    }

    async fn fail_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        lease_generation: u64,
        safe_error_code: &'static str,
        observed_at: Timestamp,
    ) -> Result<UploadSessionRecord, MetadataError> {
        let mut state = self.state.lock().expect("metadata lock");
        let record = owned_record(&mut state.records, owner_user_id, session_id)?;
        if !record.state.is_terminal() && record.lease_generation == lease_generation {
            record.state = UploadSessionState::Failed;
            record.lease_expires_at = None;
            record.last_error_code = Some(safe_error_code.to_owned());
            record.terminal_failure_code = Some(safe_error_code.to_owned());
            record.updated_at = observed_at;
        }
        Ok(record.clone())
    }

    async fn finalize_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        lease_generation: u64,
        observed_at: Timestamp,
    ) -> Result<UploadFinalization, MetadataError> {
        let mut state = self.state.lock().expect("metadata lock");
        let force_version_conflict = state.force_version_conflict;
        let record = owned_record(&mut state.records, owner_user_id, session_id)?;
        if record.state == UploadSessionState::Committed {
            return record
                .completion
                .clone()
                .map(UploadFinalization::Completed)
                .ok_or(MetadataError::Mapping(MappingError::RelationMismatch {
                    relation: "upload_sessions.completion",
                }));
        }
        if record.state.is_terminal() {
            return Ok(UploadFinalization::Terminal(record.clone()));
        }
        if record.state != UploadSessionState::Committing
            || record.lease_generation != lease_generation
        {
            return Ok(UploadFinalization::NotReady(record.clone()));
        }
        if force_version_conflict {
            let current_revision = record
                .expected_node_revision
                .map_or(Revision::new(1), |revision| {
                    Revision::new(revision.get().saturating_add(1))
                });
            record.state = UploadSessionState::Failed;
            record.lease_expires_at = None;
            record.last_error_code = Some("version_conflict".to_owned());
            record.terminal_failure_code = Some("version_conflict".to_owned());
            record.updated_at = observed_at;
            return Ok(UploadFinalization::VersionConflict {
                current_revision,
                session: record.clone(),
            });
        }
        let durability = record.durability.clone().ok_or(MetadataError::Mapping(
            MappingError::RelationMismatch {
                relation: "upload_sessions.durability",
            },
        ))?;
        let completion = UploadCompletion {
            session_id,
            node_id: record.target_node_id,
            file_version_id: FileVersionId::new(),
            object_id: record.object_id,
            object_replica_id: record.object_replica_id,
            node_revision: Revision::new(1),
            length: durability.length,
            sha256: durability.sha256,
            committed_at: observed_at,
        };
        record.state = UploadSessionState::Committed;
        record.lease_expires_at = None;
        record.last_error_code = None;
        record.updated_at = observed_at;
        record.completion = Some(completion.clone());
        state.finalize_calls += 1;
        Ok(UploadFinalization::Completed(completion))
    }

    async fn abort_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        observed_at: Timestamp,
    ) -> Result<UploadSessionRecord, MetadataError> {
        let mut state = self.state.lock().expect("metadata lock");
        let record = owned_record(&mut state.records, owner_user_id, session_id)?;
        if !record.state.is_terminal() {
            record.state = UploadSessionState::Aborted;
            record.cancel_requested_at = Some(observed_at);
            record.lease_expires_at = None;
            record.updated_at = observed_at;
        }
        Ok(record.clone())
    }

    async fn expire_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        observed_at: Timestamp,
    ) -> Result<UploadSessionRecord, MetadataError> {
        let mut state = self.state.lock().expect("metadata lock");
        let record = owned_record(&mut state.records, owner_user_id, session_id)?;
        if record.state == UploadSessionState::Open
            && record.expires_at.as_offset_datetime() <= observed_at.as_offset_datetime()
        {
            record.state = UploadSessionState::Expired;
            record.last_error_code = Some("upload_expired".to_owned());
            record.updated_at = observed_at;
        }
        Ok(record.clone())
    }

    async fn list_upload_cleanup_candidates(
        &self,
        limit: u32,
    ) -> Result<Vec<UploadCleanupCandidate>, MetadataError> {
        let state = self.state.lock().expect("metadata lock");
        Ok(state
            .records
            .values()
            .filter(|record| record.state.is_terminal())
            .take(limit as usize)
            .map(|record| UploadCleanupCandidate {
                session_id: record.id,
                state: record.state,
                staging_handle: record.staging_handle.clone(),
                object_key: record.object_key.clone(),
                lease_generation: record.lease_generation,
            })
            .collect())
    }
}

fn digest(bytes: &[u8]) -> Sha256Digest {
    let mut output = [0_u8; 32];
    output.copy_from_slice(&Sha256::digest(bytes));
    Sha256Digest::from_bytes(output)
}

fn service_with_store(
    metadata: Arc<TestMetadata>,
    root: &Path,
) -> (UploadApplicationService, Arc<dyn ObjectStore>) {
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFilesystemObjectStore::open(root).expect("open local object store"));
    let service = UploadApplicationService::new(
        metadata,
        store.clone(),
        UploadLimits {
            max_object_size: 64,
            max_chunk_size: 8,
            max_active_sessions: 4,
            session_ttl: Duration::from_secs(3_600),
            lease_ttl: Duration::from_secs(30),
        },
    )
    .expect("valid upload limits");
    (service, store)
}

fn service(metadata: Arc<TestMetadata>, root: &Path) -> UploadApplicationService {
    service_with_store(metadata, root).0
}

fn create_request(
    owner_user_id: UserId,
    expected_length: u64,
    expected_sha256: Option<Sha256Digest>,
) -> CreateUploadSessionRequest {
    CreateUploadSessionRequest {
        owner_user_id,
        target: UploadTargetRequest::CreateFile {
            library_id: LibraryId::new(),
            parent_node_id: NodeId::new(),
            name: LogicalName::new("resumable.txt").expect("valid logical name"),
        },
        expected_length,
        expected_sha256,
    }
}

#[tokio::test]
async fn offsets_and_status_survive_store_reopen_and_completion_is_idempotent() {
    let root = TempRoot::new();
    let metadata = Arc::new(TestMetadata::default());
    let owner = UserId::new();
    let content = b"hello world";
    let session_id;

    {
        let service = service(metadata.clone(), root.path());
        let view = service
            .create_upload_session(create_request(
                owner,
                content.len() as u64,
                Some(digest(content)),
            ))
            .await
            .expect("create upload session");
        session_id = view.id;
        let view_debug = format!("{view:?}");
        assert!(!view_debug.contains("objects/v1/"));
        assert!(!view_debug.contains("staging"));

        assert_eq!(
            service
                .append_upload_chunk(owner, session_id, 0, Bytes::from_static(b"hello"))
                .await
                .expect("append first chunk")
                .received_bytes,
            5
        );
        assert_eq!(
            service
                .append_upload_chunk(owner, session_id, 0, Bytes::from_static(b"hello"))
                .await,
            Err(UploadError::InvalidOffset { current_offset: 5 })
        );
        assert_eq!(
            service
                .get_upload_session(owner, session_id)
                .await
                .expect("status after first chunk")
                .received_bytes,
            5
        );
    }

    {
        let service = service(metadata.clone(), root.path());
        assert_eq!(
            service
                .append_upload_chunk(owner, session_id, 5, Bytes::from_static(b" world"))
                .await
                .expect("append after store reopen")
                .received_bytes,
            content.len() as u64
        );
        let completion = service
            .complete_upload(owner, session_id)
            .await
            .expect("complete upload");
        assert_eq!(completion.session_id, session_id);
        assert_eq!(completion.length, content.len() as u64);
        assert_eq!(completion.sha256, digest(content));
        assert_eq!(
            service
                .complete_upload(owner, session_id)
                .await
                .expect("idempotent completion"),
            completion
        );
        assert_eq!(metadata.finalize_calls(), 1);
        assert_eq!(
            metadata.record(session_id).state,
            UploadSessionState::Committed
        );
    }
}

#[tokio::test]
async fn concurrent_completion_has_one_metadata_winner() {
    let root = TempRoot::new();
    let metadata = Arc::new(TestMetadata::default());
    let owner = UserId::new();
    let service = Arc::new(service(metadata.clone(), root.path()));
    let content = b"race";
    let session_id = service
        .create_upload_session(create_request(
            owner,
            content.len() as u64,
            Some(digest(content)),
        ))
        .await
        .expect("create race session")
        .id;
    service
        .append_upload_chunk(owner, session_id, 0, Bytes::from_static(content))
        .await
        .expect("append race content");

    let first = service.clone();
    let second = service.clone();
    let (left, right) = tokio::join!(
        first.complete_upload(owner, session_id),
        second.complete_upload(owner, session_id)
    );
    assert_eq!(metadata.finalize_calls(), 1);
    match (left, right) {
        (Ok(first), Ok(second)) => assert_eq!(first, second),
        (Ok(_), Err(UploadError::InProgress)) | (Err(UploadError::InProgress), Ok(_)) => {}
        (left, right) => panic!("unexpected completion race outcomes: {left:?}, {right:?}"),
    }
}

#[tokio::test]
async fn completion_rejects_a_hash_mismatch_and_persists_a_safe_terminal_error() {
    let root = TempRoot::new();
    let metadata = Arc::new(TestMetadata::default());
    let owner = UserId::new();
    let service = service(metadata.clone(), root.path());
    let session_id = service
        .create_upload_session(create_request(owner, 7, Some(digest(b"correct"))))
        .await
        .expect("create upload session")
        .id;
    service
        .append_upload_chunk(owner, session_id, 0, Bytes::from_static(b"wrong!!"))
        .await
        .expect("append wrong bytes");

    assert_eq!(
        service.complete_upload(owner, session_id).await,
        Err(UploadError::HashMismatch)
    );
    let view = service
        .get_upload_session(owner, session_id)
        .await
        .expect("failed session status");
    assert_eq!(view.state, UploadSessionState::Failed);
    assert_eq!(view.terminal_failure_code.as_deref(), Some("hash_mismatch"));
    assert_eq!(
        service
            .append_upload_chunk(owner, session_id, 7, Bytes::from_static(b"later"))
            .await,
        Err(UploadError::Failed {
            code: "hash_mismatch".to_owned(),
        })
    );
    assert_eq!(metadata.finalize_calls(), 0);
}

#[tokio::test]
async fn replacement_completion_rechecks_the_expected_revision() {
    let root = TempRoot::new();
    let metadata = Arc::new(TestMetadata::default());
    metadata.force_version_conflict();
    let owner = UserId::new();
    let node_id = NodeId::new();
    let service = service(metadata.clone(), root.path());
    let session_id = service
        .create_upload_session(CreateUploadSessionRequest {
            owner_user_id: owner,
            target: UploadTargetRequest::ReplaceContent {
                library_id: LibraryId::new(),
                node_id,
                expected_revision: Revision::new(0),
            },
            expected_length: 4,
            expected_sha256: Some(digest(b"data")),
        })
        .await
        .expect("create replacement session")
        .id;
    service
        .append_upload_chunk(owner, session_id, 0, Bytes::from_static(b"data"))
        .await
        .expect("append replacement bytes");

    assert_eq!(
        service.complete_upload(owner, session_id).await,
        Err(UploadError::VersionConflict {
            current_revision: Some(Revision::new(1)),
        })
    );
    assert_eq!(
        metadata.record(session_id).terminal_failure_code.as_deref(),
        Some("version_conflict")
    );
}

#[tokio::test]
async fn chunk_limits_are_enforced_before_storage_mutation() {
    let root = TempRoot::new();
    let metadata = Arc::new(TestMetadata::default());
    let owner = UserId::new();
    let service = service(metadata, root.path());
    let session_id = service
        .create_upload_session(create_request(owner, 8, None))
        .await
        .expect("create upload session")
        .id;

    assert_eq!(
        service
            .append_upload_chunk(owner, session_id, 0, Bytes::from_static(b"123456789"))
            .await,
        Err(UploadError::ChunkTooLarge)
    );
}

#[tokio::test]
async fn streaming_append_persists_each_frame_and_bounds_the_aggregate_request() {
    let root = TempRoot::new();
    let metadata = Arc::new(TestMetadata::default());
    let owner = UserId::new();
    let service = service(metadata.clone(), root.path());
    let session_id = service
        .create_upload_session(create_request(owner, 8, Some(digest(b"abcdefgh"))))
        .await
        .expect("create streaming upload session")
        .id;

    let progress = service
        .append_upload_stream(
            owner,
            session_id,
            0,
            boxed_upload_stream(futures_util::stream::iter([
                Ok(Bytes::from_static(b"abc")),
                Ok(Bytes::from_static(b"def")),
                Ok(Bytes::from_static(b"gh")),
            ])),
        )
        .await
        .expect("append framed upload stream");
    assert_eq!(progress.received_bytes, 8);
    assert_eq!(metadata.record(session_id).bytes_received, 8);
    assert_eq!(
        service
            .get_upload_session(owner, session_id)
            .await
            .expect("status after framed stream")
            .received_bytes,
        8
    );
    assert_eq!(
        service
            .append_upload_stream(
                owner,
                session_id,
                0,
                boxed_upload_stream(futures_util::stream::iter([Ok(
                    Bytes::from_static(b"abc",)
                )])),
            )
            .await,
        Err(UploadError::InvalidOffset { current_offset: 8 })
    );

    let oversized_session = service
        .create_upload_session(create_request(owner, 9, None))
        .await
        .expect("create aggregate-limit session")
        .id;
    assert_eq!(
        service
            .append_upload_stream(
                owner,
                oversized_session,
                0,
                boxed_upload_stream(futures_util::stream::iter([
                    Ok(Bytes::from_static(b"1234")),
                    Ok(Bytes::from_static(b"5678")),
                    Ok(Bytes::from_static(b"9")),
                ])),
            )
            .await,
        Err(UploadError::ChunkTooLarge)
    );
    assert_eq!(
        service
            .get_upload_session(owner, oversized_session)
            .await
            .expect("status after aggregate-limit rejection")
            .received_bytes,
        8,
        "already durable frames remain authoritative after a streaming overflow"
    );
}

#[tokio::test]
async fn status_reconciles_durable_staging_progress_before_reporting_resume_offset() {
    let root = TempRoot::new();
    let metadata = Arc::new(TestMetadata::default());
    let owner = UserId::new();
    let (service, store) = service_with_store(metadata.clone(), root.path());
    let session_id = service
        .create_upload_session(create_request(owner, 4, Some(digest(b"data"))))
        .await
        .expect("create status-reconciliation session")
        .id;
    let record = metadata.record(session_id);
    let handle = StagingHandle::new(record.staging_handle).expect("staging handle");

    // Simulate the crash boundary after durable storage append but before the
    // corresponding PostgreSQL progress write became observable.
    store
        .append_staged(&handle, 0, Bytes::from_static(b"data"), 4)
        .await
        .expect("write durable unacknowledged bytes");
    assert_eq!(metadata.record(session_id).bytes_received, 0);

    let status = service
        .get_upload_session(owner, session_id)
        .await
        .expect("status reconciles durable staging progress");
    assert_eq!(status.received_bytes, 4);
    assert_eq!(metadata.record(session_id).bytes_received, 4);
    assert_eq!(
        service
            .append_upload_chunk(owner, session_id, 0, Bytes::from_static(b"data"))
            .await,
        Err(UploadError::InvalidOffset { current_offset: 4 })
    );
}

#[tokio::test]
async fn an_already_promoted_object_is_reconciled_when_staging_is_missing() {
    let root = TempRoot::new();
    let metadata = Arc::new(TestMetadata::default());
    let owner = UserId::new();
    let content = b"resume";
    let (service, store) = service_with_store(metadata.clone(), root.path());
    let session_id = service
        .create_upload_session(create_request(
            owner,
            content.len() as u64,
            Some(digest(content)),
        ))
        .await
        .expect("create upload session")
        .id;
    service
        .append_upload_chunk(owner, session_id, 0, Bytes::from_static(content))
        .await
        .expect("append content");

    let record = metadata.record(session_id);
    let handle = StagingHandle::new(record.staging_handle).expect("test staging handle");
    let key = ObjectKey::new(record.object_key).expect("test object key");
    store
        .finalize_staged(
            &handle,
            IntegrityExpectation::none()
                .with_length(content.len() as u64)
                .with_sha256(digest(content)),
        )
        .await
        .expect("finalize before simulated process crash");
    store
        .promote_temp(&handle, &key)
        .await
        .expect("promote before simulated process crash");

    let completion = service
        .complete_upload(owner, session_id)
        .await
        .expect("reconcile already-promoted object");
    assert_eq!(completion.sha256, digest(content));
    assert_eq!(
        metadata.record(session_id).state,
        UploadSessionState::Committed
    );
}

#[tokio::test]
async fn a_persisted_committing_session_can_finalize_after_restart() {
    let root = TempRoot::new();
    let metadata = Arc::new(TestMetadata::default());
    let owner = UserId::new();
    let content = b"durable";
    let (service, store) = service_with_store(metadata.clone(), root.path());
    let session_id = service
        .create_upload_session(create_request(
            owner,
            content.len() as u64,
            Some(digest(content)),
        ))
        .await
        .expect("create upload session")
        .id;
    service
        .append_upload_chunk(owner, session_id, 0, Bytes::from_static(content))
        .await
        .expect("append content");

    let record = metadata.record(session_id);
    let handle = StagingHandle::new(record.staging_handle.clone()).expect("staging handle");
    let key = ObjectKey::new(record.object_key.clone()).expect("object key");
    store
        .finalize_staged(
            &handle,
            IntegrityExpectation::none()
                .with_length(content.len() as u64)
                .with_sha256(digest(content)),
        )
        .await
        .expect("finalize staged object");
    store
        .promote_temp(&handle, &key)
        .await
        .expect("promote staged object");
    {
        let mut state = metadata.state.lock().expect("metadata lock");
        let record = state.records.get_mut(&session_id).expect("session record");
        record.state = UploadSessionState::Committing;
        record.lease_expires_at = Some(Timestamp::from_offset_datetime(
            Timestamp::now().as_offset_datetime() - time::Duration::seconds(1),
        ));
        record.durability = Some(UploadDurabilityReceipt {
            backend_kind: "LOCAL_FILESYSTEM".to_owned(),
            storage_key: record.object_key.clone(),
            backend_version: None,
            length: content.len() as u64,
            sha256: digest(content),
            verified_at: Timestamp::now(),
        });
    }

    let completion = service
        .complete_upload(owner, session_id)
        .await
        .expect("finalize persisted committing session");
    assert_eq!(completion.sha256, digest(content));
    assert_eq!(metadata.finalize_calls(), 1);
    assert_eq!(
        metadata.record(session_id).state,
        UploadSessionState::Committed
    );
}

#[tokio::test]
async fn completion_rejects_unacknowledged_bytes_written_after_verification_claim() {
    let root = TempRoot::new();
    let metadata = Arc::new(TestMetadata::default());
    let owner = UserId::new();
    let (service, store) = service_with_store(metadata.clone(), root.path());
    let session_id = service
        .create_upload_session(create_request(owner, 4, Some(digest(b"data"))))
        .await
        .expect("create upload session")
        .id;
    let record = metadata.record(session_id);
    let handle = StagingHandle::new(record.staging_handle).expect("staging handle");
    store
        .append_staged(&handle, 0, Bytes::from_static(b"data"), 4)
        .await
        .expect("simulate concurrent append");
    {
        let mut state = metadata.state.lock().expect("metadata lock");
        let record = state.records.get_mut(&session_id).expect("session record");
        record.state = UploadSessionState::Verifying;
        record.lease_generation = 1;
        record.lease_expires_at = Some(Timestamp::from_offset_datetime(
            Timestamp::now().as_offset_datetime() - time::Duration::seconds(1),
        ));
    }

    assert_eq!(
        service.complete_upload(owner, session_id).await,
        Err(UploadError::Failed {
            code: "concurrent_append".to_owned(),
        })
    );
    assert_eq!(
        metadata.record(session_id).state,
        UploadSessionState::Failed
    );
    assert_eq!(metadata.finalize_calls(), 0);
}

#[tokio::test]
async fn missing_promoted_object_is_retryable_and_corrupt_one_is_terminal() {
    let root = TempRoot::new();
    let metadata = Arc::new(TestMetadata::default());
    let owner = UserId::new();
    let (service, store) = service_with_store(metadata.clone(), root.path());
    let missing_session = service
        .create_upload_session(create_request(owner, 4, Some(digest(b"data"))))
        .await
        .expect("create missing-object session")
        .id;
    service
        .append_upload_chunk(owner, missing_session, 0, Bytes::from_static(b"data"))
        .await
        .expect("append missing-object content");
    let missing_record = metadata.record(missing_session);
    store
        .abort_staged(
            &StagingHandle::new(missing_record.staging_handle).expect("missing staging handle"),
        )
        .await
        .expect("remove staging to simulate absent promotion");
    assert_eq!(
        service.complete_upload(owner, missing_session).await,
        Err(UploadError::StorageUnavailable)
    );
    assert_eq!(
        metadata.record(missing_session).last_error_code.as_deref(),
        Some("storage_unavailable")
    );

    let corrupt_session = service
        .create_upload_session(create_request(owner, 4, Some(digest(b"data"))))
        .await
        .expect("create corrupt-object session")
        .id;
    service
        .append_upload_chunk(owner, corrupt_session, 0, Bytes::from_static(b"data"))
        .await
        .expect("append corrupt-object content");
    let corrupt_record = metadata.record(corrupt_session);
    let corrupt_handle =
        StagingHandle::new(corrupt_record.staging_handle.clone()).expect("corrupt staging handle");
    let corrupt_key = ObjectKey::new(corrupt_record.object_key.clone()).expect("corrupt key");
    store
        .abort_staged(&corrupt_handle)
        .await
        .expect("remove original staging");
    let replacement_handle = store
        .begin_staged_write()
        .await
        .expect("begin corrupt staging");
    store
        .write_staged(
            &replacement_handle,
            synveil_object_store::boxed_stream(futures_util::stream::iter(vec![Ok(
                Bytes::from_static(b"nope"),
            )])),
            IntegrityExpectation::none(),
        )
        .await
        .expect("stage corrupt replacement");
    store
        .promote_temp(&replacement_handle, &corrupt_key)
        .await
        .expect("promote corrupt replacement");

    assert_eq!(
        service.complete_upload(owner, corrupt_session).await,
        Err(UploadError::HashMismatch)
    );
    assert_eq!(
        metadata
            .record(corrupt_session)
            .terminal_failure_code
            .as_deref(),
        Some("object_corrupt")
    );
}

#[tokio::test]
async fn ownership_abort_expiry_and_size_boundaries_are_enforced() {
    let root = TempRoot::new();
    let metadata = Arc::new(TestMetadata::default());
    let owner = UserId::new();
    let other_owner = UserId::new();
    let service = service(metadata.clone(), root.path());
    let session_id = service
        .create_upload_session(create_request(owner, 4, None))
        .await
        .expect("create upload session")
        .id;
    assert_eq!(
        service.get_upload_session(other_owner, session_id).await,
        Err(UploadError::NotFound)
    );
    assert_eq!(
        service
            .append_upload_chunk(other_owner, session_id, 0, Bytes::from_static(b"data"))
            .await,
        Err(UploadError::NotFound)
    );
    service
        .abort_upload(owner, session_id)
        .await
        .expect("abort upload");
    assert_eq!(
        service
            .append_upload_chunk(owner, session_id, 0, Bytes::from_static(b"data"))
            .await,
        Err(UploadError::InvalidState {
            state: UploadSessionState::Aborted,
        })
    );

    let expired_session = service
        .create_upload_session(create_request(owner, 1, None))
        .await
        .expect("create expiring session")
        .id;
    metadata
        .state
        .lock()
        .expect("metadata lock")
        .records
        .get_mut(&expired_session)
        .expect("expired record")
        .expires_at = Timestamp::from_offset_datetime(
        Timestamp::now().as_offset_datetime() - time::Duration::seconds(1),
    );
    assert_eq!(
        service.get_upload_session(owner, expired_session).await,
        Ok(synveil_storage::UploadSessionView {
            id: expired_session,
            owner_user_id: owner,
            target: synveil_storage::UploadTargetView::CreateFile {
                library_id: metadata.record(expired_session).library_id,
                parent_node_id: metadata
                    .record(expired_session)
                    .target_parent_node_id
                    .expect("expired parent"),
                node_id: metadata.record(expired_session).target_node_id,
                name: metadata
                    .record(expired_session)
                    .target_name
                    .expect("expired name"),
            },
            expected_length: 1,
            expected_sha256: None,
            received_bytes: 0,
            state: UploadSessionState::Expired,
            created_at: metadata.record(expired_session).created_at,
            updated_at: metadata.record(expired_session).updated_at,
            expires_at: metadata.record(expired_session).expires_at,
            last_error_code: Some("upload_expired".to_owned()),
            terminal_failure_code: None,
            completion: None,
        })
    );
    assert_eq!(
        service
            .append_upload_chunk(owner, expired_session, 0, Bytes::from_static(b"x"))
            .await,
        Err(UploadError::UploadExpired)
    );
    assert_eq!(
        service
            .create_upload_session(create_request(owner, 65, None))
            .await,
        Err(UploadError::SizeMismatch)
    );
}
