use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::stream;
use sha2::{Digest, Sha256};
use synveil_client_sync::{
    BootstrapCompletion, BootstrapPage, ClientSyncError, EngineConfig, EngineStatus,
    FailureInjector, FailurePoint, FilesystemLocalReplica, InboundChange, InboundSyncEngine,
    LocalIssueKind, LocalStateConfig, LocalStateStore, OpaqueEvidence, RemoteCheckpoint,
    RemoteContent, RemoteError, RemoteErrorKind, RemoteFeedPage, ReplicaScope, SyncOutcome,
    SyncRemote, boxed_content_stream,
};
use synveil_core::{
    ChangeEvent, ChangeEventId, ChangeKind, ChangeResourceKind, DeviceId, FileVersionId, LibraryId,
    LogicalName, LogicalSnapshotNode, NodeId, NodeKind, NodeState, Revision, Sequence,
    Sha256Digest, SyncBootstrap, SyncBootstrapId, SyncBootstrapState, Timestamp, UserId,
};

fn timestamp(value: &str) -> Timestamp {
    Timestamp::from_str(value).unwrap()
}

fn digest(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::from_bytes(Sha256::digest(bytes).into())
}

fn name(value: &str) -> LogicalName {
    LogicalName::new(value).unwrap()
}

fn snapshot_directory(
    node_id: NodeId,
    parent_node_id: Option<NodeId>,
    logical_name: &str,
    state: NodeState,
    revision: u64,
) -> LogicalSnapshotNode {
    LogicalSnapshotNode::new(
        node_id,
        parent_node_id,
        name(logical_name),
        NodeKind::Directory,
        state,
        Revision::new(revision),
        None,
        None,
        None,
    )
    .unwrap()
}

fn snapshot_file(
    node_id: NodeId,
    parent_node_id: NodeId,
    logical_name: &str,
    revision: u64,
    version_id: FileVersionId,
    bytes: &[u8],
) -> LogicalSnapshotNode {
    LogicalSnapshotNode::new(
        node_id,
        Some(parent_node_id),
        name(logical_name),
        NodeKind::File,
        NodeState::Active,
        Revision::new(revision),
        Some(version_id),
        Some(bytes.len() as u64),
        Some(digest(bytes)),
    )
    .unwrap()
}

#[allow(clippy::too_many_arguments)]
fn change(
    scope: ReplicaScope,
    epoch: Sequence,
    sequence: u64,
    resource_id: NodeId,
    kind: ChangeKind,
    revision: u64,
    desired: Option<LogicalSnapshotNode>,
) -> InboundChange {
    let event = ChangeEvent::new(
        ChangeEventId::new(),
        scope.owner_user_id(),
        scope.library_id(),
        epoch,
        Sequence::new(sequence),
        1,
        ChangeResourceKind::Node,
        resource_id,
        kind,
        timestamp("2026-08-27T02:00:00.123456Z"),
        Revision::new(revision),
        desired
            .as_ref()
            .and_then(LogicalSnapshotNode::parent_node_id),
        desired.as_ref().map(LogicalSnapshotNode::kind),
        desired.as_ref().map(LogicalSnapshotNode::state),
        desired
            .as_ref()
            .and_then(LogicalSnapshotNode::current_version_id),
    );
    InboundChange::new(event, desired)
}

#[derive(Clone)]
struct ScenarioIds {
    scope: ReplicaScope,
    other_device: DeviceId,
    root: NodeId,
    folder: NodeId,
    file: NodeId,
    transient: NodeId,
    purged: NodeId,
    version_one: FileVersionId,
    version_two: FileVersionId,
}

struct RemoteState {
    bootstrap: SyncBootstrap,
    snapshot_pages: Vec<Vec<LogicalSnapshotNode>>,
    changes: Vec<InboundChange>,
    contents: HashMap<FileVersionId, (NodeId, Vec<u8>)>,
    corrupt_versions: HashSet<FileVersionId>,
    wrong_hash_versions: HashSet<FileVersionId>,
    feed_batch_size: usize,
    feed_epoch_override: Option<Sequence>,
    offline: bool,
    acknowledged: Sequence,
    other_acknowledged: Sequence,
    ack_attempts: usize,
    completion_attempts: usize,
}

struct DeterministicRemote {
    scope: ReplicaScope,
    other_device: DeviceId,
    state: Mutex<RemoteState>,
}

impl DeterministicRemote {
    fn full() -> (Arc<Self>, ScenarioIds) {
        let owner = UserId::new();
        let device = DeviceId::new();
        let other_device = DeviceId::new();
        let library = LibraryId::new();
        let scope = ReplicaScope::new(owner, device, library);
        let epoch = Sequence::new(7);
        let root = NodeId::new();
        let folder = NodeId::new();
        let file = NodeId::new();
        let transient = NodeId::new();
        let purged = NodeId::new();
        let version_one = FileVersionId::new();
        let version_two = FileVersionId::new();
        let bytes_one = b"version one".to_vec();
        let bytes_two = b"version two is streamed".to_vec();

        let root_snapshot = snapshot_directory(root, None, "root", NodeState::Active, 1);
        let folder_snapshot =
            snapshot_directory(folder, Some(root), "folder", NodeState::Active, 1);
        let file_snapshot = snapshot_file(file, root, "file.txt", 1, version_one, &bytes_one);
        let purged_snapshot = snapshot_directory(purged, Some(root), "old", NodeState::Trashed, 1);
        let bootstrap_id = SyncBootstrapId::new();
        let bootstrap = SyncBootstrap::new(
            bootstrap_id,
            owner,
            device,
            library,
            Sequence::new(1),
            epoch,
            Sequence::new(0),
            4,
            Some(purged),
            SyncBootstrapState::Open,
            timestamp("2026-08-27T01:00:00.123456Z"),
            timestamp("2026-08-28T01:00:00.123456Z"),
            None,
        );

        let created = snapshot_directory(transient, Some(root), "newdir", NodeState::Active, 1);
        let renamed = snapshot_directory(transient, Some(root), "renamed", NodeState::Active, 2);
        let moved = snapshot_directory(transient, Some(folder), "renamed", NodeState::Active, 3);
        let trashed = snapshot_directory(transient, Some(folder), "renamed", NodeState::Trashed, 4);
        let restored = snapshot_directory(transient, Some(folder), "renamed", NodeState::Active, 5);
        let content_two = snapshot_file(file, root, "file.txt", 2, version_two, &bytes_two);
        let content_one = snapshot_file(file, root, "file.txt", 3, version_one, &bytes_one);
        let changes = vec![
            change(
                scope,
                epoch,
                1,
                transient,
                ChangeKind::NodeCreated,
                1,
                Some(created),
            ),
            change(
                scope,
                epoch,
                2,
                transient,
                ChangeKind::NodeRenamed,
                2,
                Some(renamed),
            ),
            change(
                scope,
                epoch,
                3,
                transient,
                ChangeKind::NodeMoved,
                3,
                Some(moved),
            ),
            change(
                scope,
                epoch,
                4,
                transient,
                ChangeKind::NodeTrashed,
                4,
                Some(trashed),
            ),
            change(
                scope,
                epoch,
                5,
                transient,
                ChangeKind::NodeRestored,
                5,
                Some(restored),
            ),
            change(
                scope,
                epoch,
                6,
                file,
                ChangeKind::FileContentCommitted,
                2,
                Some(content_two),
            ),
            change(
                scope,
                epoch,
                7,
                file,
                ChangeKind::FileVersionRestored,
                3,
                Some(content_one),
            ),
            change(scope, epoch, 8, purged, ChangeKind::NodePurged, 2, None),
        ];
        let contents = HashMap::from([
            (version_one, (file, bytes_one)),
            (version_two, (file, bytes_two)),
        ]);
        let remote = Arc::new(Self {
            scope,
            other_device,
            state: Mutex::new(RemoteState {
                bootstrap,
                snapshot_pages: vec![
                    vec![root_snapshot, folder_snapshot],
                    vec![file_snapshot, purged_snapshot],
                ],
                changes,
                contents,
                corrupt_versions: HashSet::new(),
                wrong_hash_versions: HashSet::new(),
                feed_batch_size: 1,
                feed_epoch_override: None,
                offline: false,
                acknowledged: Sequence::new(0),
                other_acknowledged: Sequence::new(0),
                ack_attempts: 0,
                completion_attempts: 0,
            }),
        });
        (
            remote,
            ScenarioIds {
                scope,
                other_device,
                root,
                folder,
                file,
                transient,
                purged,
                version_one,
                version_two,
            },
        )
    }

    fn root_with_changes(
        changes: Vec<InboundChange>,
        scope: ReplicaScope,
        root: NodeId,
    ) -> Arc<Self> {
        let epoch = Sequence::new(7);
        let bootstrap = SyncBootstrap::new(
            SyncBootstrapId::new(),
            scope.owner_user_id(),
            scope.device_id(),
            scope.library_id(),
            Sequence::new(1),
            epoch,
            Sequence::new(0),
            1,
            Some(root),
            SyncBootstrapState::Open,
            timestamp("2026-08-27T01:00:00.123456Z"),
            timestamp("2026-08-28T01:00:00.123456Z"),
            None,
        );
        Arc::new(Self {
            scope,
            other_device: DeviceId::new(),
            state: Mutex::new(RemoteState {
                bootstrap,
                snapshot_pages: vec![vec![snapshot_directory(
                    root,
                    None,
                    "root",
                    NodeState::Active,
                    1,
                )]],
                changes,
                contents: HashMap::new(),
                corrupt_versions: HashSet::new(),
                wrong_hash_versions: HashSet::new(),
                feed_batch_size: 1,
                feed_epoch_override: None,
                offline: false,
                acknowledged: Sequence::new(0),
                other_acknowledged: Sequence::new(0),
                ack_attempts: 0,
                completion_attempts: 0,
            }),
        })
    }

    fn checkpoint_for(&self, device_id: DeviceId) -> Sequence {
        let state = self.state.lock().unwrap();
        if device_id == self.scope.device_id() {
            state.acknowledged
        } else if device_id == self.other_device {
            state.other_acknowledged
        } else {
            Sequence::new(0)
        }
    }

    fn ack_attempts(&self) -> usize {
        self.state.lock().unwrap().ack_attempts
    }

    fn completion_attempts(&self) -> usize {
        self.state.lock().unwrap().completion_attempts
    }
}

#[async_trait]
impl SyncRemote for DeterministicRemote {
    async fn get_checkpoint(&self, scope: ReplicaScope) -> Result<RemoteCheckpoint, RemoteError> {
        if scope != self.scope {
            return Err(RemoteError::new(RemoteErrorKind::NotFound));
        }
        let state = self.state.lock().unwrap();
        if state.offline {
            return Err(RemoteError::new(RemoteErrorKind::Offline));
        }
        Ok(RemoteCheckpoint::new(
            scope,
            state.bootstrap.snapshot_epoch(),
            state.acknowledged,
        ))
    }

    async fn fetch_changes(
        &self,
        scope: ReplicaScope,
        limit: u32,
    ) -> Result<RemoteFeedPage, RemoteError> {
        if scope != self.scope {
            return Err(RemoteError::new(RemoteErrorKind::NotFound));
        }
        let state = self.state.lock().unwrap();
        if state.offline {
            return Err(RemoteError::new(RemoteErrorKind::Offline));
        }
        let from = state.acknowledged;
        let index = usize::try_from(from.get()).unwrap();
        let high = Sequence::new(state.changes.len() as u64);
        if index >= state.changes.len() {
            return RemoteFeedPage::new(
                scope,
                state.bootstrap.snapshot_epoch(),
                from,
                from,
                high,
                false,
                vec![],
                None,
            )
            .map_err(|_| RemoteError::new(RemoteErrorKind::Rejected));
        }
        let page_size = state
            .feed_batch_size
            .min(limit as usize)
            .min(state.changes.len() - index);
        let through = Sequence::new(from.get() + page_size as u64);
        RemoteFeedPage::new(
            scope,
            state
                .feed_epoch_override
                .unwrap_or_else(|| state.bootstrap.snapshot_epoch()),
            from,
            through,
            high,
            through < high,
            state.changes[index..index + page_size].to_vec(),
            Some(OpaqueEvidence::new(format!("ack-{}", through.get())).unwrap()),
        )
        .map_err(|_| RemoteError::new(RemoteErrorKind::Rejected))
    }

    async fn acknowledge_changes(
        &self,
        scope: ReplicaScope,
        evidence: &OpaqueEvidence,
    ) -> Result<RemoteCheckpoint, RemoteError> {
        if scope != self.scope {
            return Err(RemoteError::new(RemoteErrorKind::NotFound));
        }
        let token = std::str::from_utf8(evidence.as_bytes())
            .ok()
            .and_then(|value| value.strip_prefix("ack-"))
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| RemoteError::new(RemoteErrorKind::Rejected))?;
        let mut state = self.state.lock().unwrap();
        if state.offline {
            return Err(RemoteError::new(RemoteErrorKind::Offline));
        }
        state.ack_attempts += 1;
        if token > state.changes.len() as u64 {
            return Err(RemoteError::new(RemoteErrorKind::Rejected));
        }
        if token > state.acknowledged.get() {
            state.acknowledged = Sequence::new(token);
        }
        Ok(RemoteCheckpoint::new(
            scope,
            state.bootstrap.snapshot_epoch(),
            state.acknowledged,
        ))
    }

    async fn start_rebaseline(&self, scope: ReplicaScope) -> Result<SyncBootstrap, RemoteError> {
        if scope != self.scope {
            return Err(RemoteError::new(RemoteErrorKind::NotFound));
        }
        let state = self.state.lock().unwrap();
        if state.offline {
            return Err(RemoteError::new(RemoteErrorKind::Offline));
        }
        Ok(state.bootstrap)
    }

    async fn fetch_rebaseline_page(
        &self,
        scope: ReplicaScope,
        bootstrap_id: SyncBootstrapId,
        cursor: Option<&OpaqueEvidence>,
        _limit: u32,
    ) -> Result<BootstrapPage, RemoteError> {
        if scope != self.scope {
            return Err(RemoteError::new(RemoteErrorKind::NotFound));
        }
        let state = self.state.lock().unwrap();
        if state.offline {
            return Err(RemoteError::new(RemoteErrorKind::Offline));
        }
        if bootstrap_id != state.bootstrap.id() {
            return Err(RemoteError::new(RemoteErrorKind::NotFound));
        }
        let index = match cursor {
            None => 0,
            Some(cursor) => std::str::from_utf8(cursor.as_bytes())
                .ok()
                .and_then(|value| value.strip_prefix("cursor-"))
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| RemoteError::new(RemoteErrorKind::Rejected))?,
        };
        let nodes = state
            .snapshot_pages
            .get(index)
            .cloned()
            .ok_or_else(|| RemoteError::new(RemoteErrorKind::Rejected))?;
        let has_more = index + 1 < state.snapshot_pages.len();
        BootstrapPage::new(
            state.bootstrap,
            nodes,
            has_more,
            has_more.then(|| OpaqueEvidence::new(format!("cursor-{}", index + 1)).unwrap()),
            (!has_more).then(|| OpaqueEvidence::new(b"complete-v1".to_vec()).unwrap()),
        )
        .map_err(|_| RemoteError::new(RemoteErrorKind::Rejected))
    }

    async fn complete_rebaseline(
        &self,
        scope: ReplicaScope,
        bootstrap_id: SyncBootstrapId,
        evidence: &OpaqueEvidence,
    ) -> Result<BootstrapCompletion, RemoteError> {
        if scope != self.scope || evidence.as_bytes() != b"complete-v1" {
            return Err(RemoteError::new(RemoteErrorKind::Rejected));
        }
        let mut state = self.state.lock().unwrap();
        if state.offline {
            return Err(RemoteError::new(RemoteErrorKind::Offline));
        }
        if bootstrap_id != state.bootstrap.id() {
            return Err(RemoteError::new(RemoteErrorKind::NotFound));
        }
        state.completion_attempts += 1;
        state.acknowledged = state.bootstrap.snapshot_resume_sequence();
        Ok(BootstrapCompletion::new(
            bootstrap_id,
            RemoteCheckpoint::new(scope, state.bootstrap.snapshot_epoch(), state.acknowledged),
        ))
    }

    async fn download_current_content(
        &self,
        scope: ReplicaScope,
        node_id: NodeId,
        version_id: FileVersionId,
    ) -> Result<RemoteContent, RemoteError> {
        if scope != self.scope {
            return Err(RemoteError::new(RemoteErrorKind::NotFound));
        }
        let state = self.state.lock().unwrap();
        if state.offline {
            return Err(RemoteError::new(RemoteErrorKind::Offline));
        }
        let (expected_node, canonical) = state
            .contents
            .get(&version_id)
            .cloned()
            .ok_or_else(|| RemoteError::new(RemoteErrorKind::NotFound))?;
        if expected_node != node_id {
            return Err(RemoteError::new(RemoteErrorKind::NotFound));
        }
        let body = if state.corrupt_versions.contains(&version_id) {
            b"corrupt".to_vec()
        } else if state.wrong_hash_versions.contains(&version_id) {
            canonical.iter().map(|byte| byte ^ 1).collect()
        } else {
            canonical.clone()
        };
        let chunks: Vec<_> = body.chunks(3).map(Bytes::copy_from_slice).map(Ok).collect();
        Ok(RemoteContent::new(
            node_id,
            version_id,
            canonical.len() as u64,
            digest(&canonical),
            boxed_content_stream(stream::iter(chunks)),
        ))
    }
}

struct Harness {
    base: PathBuf,
    root: PathBuf,
    database: PathBuf,
    scope: ReplicaScope,
    remote: Arc<DeterministicRemote>,
}

impl Harness {
    fn new(remote: Arc<DeterministicRemote>, scope: ReplicaScope, label: &str) -> Self {
        let base =
            std::env::temp_dir().join(format!("synveil-inbound-{label}-{}", uuid::Uuid::now_v7()));
        let root = base.join("managed-root");
        fs::create_dir_all(&root).unwrap();
        FilesystemLocalReplica::initialize(&root, scope).unwrap();
        let database = base.join("state/client.sqlite3");
        Self {
            base,
            root,
            database,
            scope,
            remote,
        }
    }

    async fn open(
        &self,
        failure: Arc<dyn FailureInjector>,
    ) -> (Arc<LocalStateStore>, InboundSyncEngine) {
        let state = Arc::new(
            LocalStateStore::open(&LocalStateConfig::new(&self.database))
                .await
                .unwrap(),
        );
        let replica = Arc::new(FilesystemLocalReplica::open(&self.root, self.scope).unwrap());
        let engine = InboundSyncEngine::with_failure_injector(
            self.scope,
            self.remote.clone(),
            replica,
            state.clone(),
            EngineConfig::new(256, 2).unwrap(),
            failure,
        )
        .await
        .unwrap();
        (state, engine)
    }

    async fn baseline(&self) {
        let (state, engine) = self.open(Arc::new(NoFailure)).await;
        for _ in 0..16 {
            engine.synchronize_once().await.unwrap();
            let record = state
                .replica(self.scope.library_id())
                .await
                .unwrap()
                .unwrap();
            if record.root_node_id().is_some()
                && state
                    .bootstrap(self.scope.library_id())
                    .await
                    .unwrap()
                    .is_none()
            {
                drop(engine);
                drop(state);
                return;
            }
        }
        panic!("bootstrap did not converge");
    }

    fn cleanup(self) {
        fs::remove_dir_all(self.base).unwrap();
    }
}

#[derive(Debug)]
struct NoFailure;

impl FailureInjector for NoFailure {
    fn check(&self, _point: FailurePoint) -> Result<(), ClientSyncError> {
        Ok(())
    }
}

struct FailOnce {
    point: FailurePoint,
    fired: AtomicBool,
}

impl FailOnce {
    fn new(point: FailurePoint) -> Self {
        Self {
            point,
            fired: AtomicBool::new(false),
        }
    }
}

impl FailureInjector for FailOnce {
    fn check(&self, point: FailurePoint) -> Result<(), ClientSyncError> {
        if point == self.point && !self.fired.swap(true, Ordering::SeqCst) {
            return Err(ClientSyncError::InjectedFailure);
        }
        Ok(())
    }
}

#[tokio::test]
async fn bootstrap_then_all_eight_feed_kinds_converge_without_cross_device_leakage() {
    let (remote, ids) = DeterministicRemote::full();
    let harness = Harness::new(remote.clone(), ids.scope, "full");
    harness.baseline().await;

    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    for _ in 0..24 {
        engine.synchronize_once().await.unwrap();
        if state
            .replica(ids.scope.library_id())
            .await
            .unwrap()
            .unwrap()
            .acknowledged_sequence()
            == Sequence::new(8)
        {
            break;
        }
    }
    let replica = state
        .replica(ids.scope.library_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(replica.applied_sequence(), Sequence::new(8));
    assert_eq!(replica.acknowledged_sequence(), Sequence::new(8));
    assert!(harness.root.join("folder/renamed").is_dir());
    assert_eq!(
        fs::read(harness.root.join("file.txt")).unwrap(),
        b"version one"
    );
    assert!(
        state
            .local_node(ids.scope.library_id(), ids.purged)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(remote.checkpoint_for(ids.other_device), Sequence::new(0));
    assert_eq!(remote.ack_attempts(), 8);
    assert!(remote.completion_attempts() >= 1);
    assert!(
        state
            .unresolved_issues(ids.scope.library_id())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        state
            .local_node(ids.scope.library_id(), ids.transient)
            .await
            .unwrap()
            .unwrap()
            .relative_path()
            .as_str(),
        "folder/renamed"
    );
    assert!(
        state
            .local_node(ids.scope.library_id(), ids.root)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        state
            .local_node(ids.scope.library_id(), ids.folder)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        state
            .local_node(ids.scope.library_id(), ids.file)
            .await
            .unwrap()
            .is_some()
    );
    assert_ne!(ids.version_one, ids.version_two);
    assert_eq!(engine.synchronize_once().await.unwrap(), SyncOutcome::Idle);
    let after_empty_page = state
        .replica(ids.scope.library_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after_empty_page.applied_sequence(), Sequence::new(8));
    assert_eq!(after_empty_page.acknowledged_sequence(), Sequence::new(8));
    assert!(
        fs::read_dir(harness.root.join(".synveil/staging"))
            .unwrap()
            .next()
            .is_none()
    );
    drop(engine);
    drop(state);
    harness.cleanup();

    let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
    let root = NodeId::new();
    let created = NodeId::new();
    let remote = DeterministicRemote::root_with_changes(
        vec![change(
            scope,
            Sequence::new(7),
            1,
            created,
            ChangeKind::NodeCreated,
            1,
            Some(snapshot_directory(
                created,
                Some(root),
                "file",
                NodeState::Active,
                1,
            )),
        )],
        scope,
        root,
    );
    let harness = Harness::new(remote, scope, "unknown-case-collision");
    harness.baseline().await;
    fs::write(harness.root.join("File"), b"unknown local bytes").unwrap();
    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    assert_eq!(
        engine.synchronize_once().await.unwrap(),
        SyncOutcome::Blocked
    );
    assert_eq!(
        state.unresolved_issues(scope.library_id()).await.unwrap()[0].kind(),
        LocalIssueKind::LocalNameCollision
    );
    assert_eq!(
        fs::read(harness.root.join("File")).unwrap(),
        b"unknown local bytes"
    );
    drop(engine);
    drop(state);
    harness.cleanup();

    let (remote, ids) = DeterministicRemote::full();
    let harness = Harness::new(remote.clone(), ids.scope, "wrong-hash-content");
    harness.baseline().await;
    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    for _ in 0..5 {
        engine.synchronize_once().await.unwrap();
    }
    remote
        .state
        .lock()
        .unwrap()
        .wrong_hash_versions
        .insert(ids.version_two);
    assert_eq!(
        engine.synchronize_once().await.unwrap(),
        SyncOutcome::Blocked
    );
    assert_eq!(
        fs::read(harness.root.join("file.txt")).unwrap(),
        b"version one"
    );
    assert!(
        state
            .unresolved_issues(ids.scope.library_id())
            .await
            .unwrap()
            .iter()
            .any(|issue| issue.kind() == LocalIssueKind::ContentIntegrityMismatch)
    );
    drop(engine);
    drop(state);
    harness.cleanup();
}

#[tokio::test]
async fn unknown_destination_and_local_file_divergence_are_durable_blockers() {
    let owner = UserId::new();
    let device = DeviceId::new();
    let library = LibraryId::new();
    let scope = ReplicaScope::new(owner, device, library);
    let root = NodeId::new();
    let created_id = NodeId::new();
    let desired = snapshot_directory(created_id, Some(root), "occupied", NodeState::Active, 1);
    let remote = DeterministicRemote::root_with_changes(
        vec![change(
            scope,
            Sequence::new(7),
            1,
            created_id,
            ChangeKind::NodeCreated,
            1,
            Some(desired),
        )],
        scope,
        root,
    );
    let harness = Harness::new(remote, scope, "occupied");
    harness.baseline().await;
    fs::write(harness.root.join("occupied"), b"unknown user bytes").unwrap();
    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    assert_eq!(
        engine.synchronize_once().await.unwrap(),
        SyncOutcome::Blocked
    );
    assert_eq!(
        fs::read(harness.root.join("occupied")).unwrap(),
        b"unknown user bytes"
    );
    assert_eq!(
        state.unresolved_issues(library).await.unwrap()[0].kind(),
        LocalIssueKind::LocalPathOccupied
    );
    drop(engine);
    drop(state);
    harness.cleanup();

    let (remote, ids) = DeterministicRemote::full();
    let harness = Harness::new(remote, ids.scope, "diverged");
    harness.baseline().await;
    fs::write(harness.root.join("file.txt"), b"user edit").unwrap();
    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    for _ in 0..8 {
        if engine.synchronize_once().await.unwrap() == SyncOutcome::Blocked {
            break;
        }
    }
    assert_eq!(
        fs::read(harness.root.join("file.txt")).unwrap(),
        b"user edit"
    );
    assert!(
        state
            .unresolved_issues(ids.scope.library_id())
            .await
            .unwrap()
            .iter()
            .any(|issue| issue.kind() == LocalIssueKind::LocalDivergence)
    );
    drop(engine);
    drop(state);
    harness.cleanup();

    let (remote, ids) = DeterministicRemote::full();
    let harness = Harness::new(remote, ids.scope, "replace-before-db");
    harness.baseline().await;
    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    for _ in 0..5 {
        engine.synchronize_once().await.unwrap();
    }
    drop(engine);
    drop(state);

    let (state, engine) = harness
        .open(Arc::new(FailOnce::new(
            FailurePoint::AfterFilesystemActionBeforeState,
        )))
        .await;
    assert!(matches!(
        engine.synchronize_once().await,
        Err(ClientSyncError::InjectedFailure)
    ));
    assert_eq!(
        fs::read(harness.root.join("file.txt")).unwrap(),
        b"version two is streamed"
    );
    let interrupted = state
        .replica(ids.scope.library_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(interrupted.applied_sequence(), Sequence::new(5));
    assert_eq!(interrupted.acknowledged_sequence(), Sequence::new(5));
    drop(engine);
    drop(state);

    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    engine.synchronize_once().await.unwrap();
    let recovered = state
        .replica(ids.scope.library_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recovered.applied_sequence(), Sequence::new(6));
    assert_eq!(recovered.acknowledged_sequence(), Sequence::new(6));
    assert_eq!(
        fs::read(harness.root.join("file.txt")).unwrap(),
        b"version two is streamed"
    );
    drop(engine);
    drop(state);
    harness.cleanup();
}

#[tokio::test]
async fn feed_crash_matrix_reopens_and_converges_without_early_ack() {
    for point in [
        FailurePoint::AfterPageIntentPersisted,
        FailurePoint::BeforeFilesystemAction,
        FailurePoint::AfterFilesystemActionBeforeState,
        FailurePoint::AfterLocalCommitBeforeAck,
        FailurePoint::AfterServerAckBeforeLocalState,
    ] {
        let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
        let root = NodeId::new();
        let created = NodeId::new();
        let desired = snapshot_directory(created, Some(root), "created", NodeState::Active, 1);
        let remote = DeterministicRemote::root_with_changes(
            vec![change(
                scope,
                Sequence::new(7),
                1,
                created,
                ChangeKind::NodeCreated,
                1,
                Some(desired),
            )],
            scope,
            root,
        );
        let harness = Harness::new(remote.clone(), scope, "feed-crash");
        harness.baseline().await;
        let (state, engine) = harness.open(Arc::new(FailOnce::new(point))).await;
        assert!(matches!(
            engine.synchronize_once().await,
            Err(ClientSyncError::InjectedFailure)
        ));
        let interrupted = state.replica(scope.library_id()).await.unwrap().unwrap();
        match point {
            FailurePoint::AfterPageIntentPersisted => {
                assert_eq!(interrupted.applied_sequence(), Sequence::new(0));
                assert_eq!(interrupted.acknowledged_sequence(), Sequence::new(0));
                assert!(!harness.root.join("created").exists());
            }
            FailurePoint::AfterLocalCommitBeforeAck => {
                assert_eq!(interrupted.applied_sequence(), Sequence::new(1));
                assert_eq!(interrupted.acknowledged_sequence(), Sequence::new(0));
                assert!(harness.root.join("created").is_dir());
            }
            _ => {}
        }
        drop(engine);
        drop(state);

        let (state, engine) = harness.open(Arc::new(NoFailure)).await;
        for _ in 0..8 {
            engine.synchronize_once().await.unwrap();
            if state
                .replica(scope.library_id())
                .await
                .unwrap()
                .unwrap()
                .acknowledged_sequence()
                == Sequence::new(1)
            {
                break;
            }
        }
        let record = state.replica(scope.library_id()).await.unwrap().unwrap();
        assert_eq!(record.applied_sequence(), Sequence::new(1), "{point:?}");
        assert_eq!(
            record.acknowledged_sequence(),
            Sequence::new(1),
            "{point:?}"
        );
        assert!(harness.root.join("created").is_dir(), "{point:?}");
        assert!(
            state
                .unresolved_issues(scope.library_id())
                .await
                .unwrap()
                .is_empty()
        );
        if point == FailurePoint::AfterServerAckBeforeLocalState {
            assert!(remote.ack_attempts() >= 2);
        }
        drop(engine);
        drop(state);
        harness.cleanup();
    }
}

#[tokio::test]
async fn bootstrap_completion_crashes_retry_without_rebuilding_the_manifest() {
    for point in [
        FailurePoint::AfterBootstrapPagePersisted,
        FailurePoint::BeforeFilesystemAction,
        FailurePoint::AfterContentStagedBeforeExpose,
        FailurePoint::AfterFilesystemActionBeforeState,
        FailurePoint::AfterBootstrapLocalComplete,
        FailurePoint::AfterServerBootstrapCompleteBeforeLocalState,
    ] {
        let (remote, ids) = DeterministicRemote::full();
        let harness = Harness::new(remote.clone(), ids.scope, "bootstrap-crash");
        let (state, engine) = harness.open(Arc::new(FailOnce::new(point))).await;
        let mut failed = false;
        for _ in 0..16 {
            match engine.synchronize_once().await {
                Err(ClientSyncError::InjectedFailure) => {
                    failed = true;
                    break;
                }
                Ok(_) => {}
                Err(error) => panic!("unexpected error at {point:?}: {error}"),
            }
        }
        assert!(failed, "failure point was not reached: {point:?}");
        drop(engine);
        drop(state);

        let (state, engine) = harness.open(Arc::new(NoFailure)).await;
        for _ in 0..20 {
            engine.synchronize_once().await.unwrap();
            if state
                .replica(ids.scope.library_id())
                .await
                .unwrap()
                .unwrap()
                .root_node_id()
                .is_some()
                && state
                    .bootstrap(ids.scope.library_id())
                    .await
                    .unwrap()
                    .is_none()
            {
                break;
            }
        }
        let record = state
            .replica(ids.scope.library_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.applied_sequence(), Sequence::new(0), "{point:?}");
        assert_eq!(
            record.acknowledged_sequence(),
            Sequence::new(0),
            "{point:?}"
        );
        if point == FailurePoint::AfterServerBootstrapCompleteBeforeLocalState {
            assert!(remote.completion_attempts() >= 2);
        }
        drop(engine);
        drop(state);
        harness.cleanup();
    }
}

#[tokio::test]
async fn corrupt_download_is_never_visible_and_staged_download_resumes_after_restart() {
    let (remote, ids) = DeterministicRemote::full();
    let harness = Harness::new(remote.clone(), ids.scope, "corrupt-content");
    harness.baseline().await;
    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    for _ in 0..5 {
        engine.synchronize_once().await.unwrap();
    }
    assert_eq!(
        state
            .replica(ids.scope.library_id())
            .await
            .unwrap()
            .unwrap()
            .acknowledged_sequence(),
        Sequence::new(5)
    );
    remote
        .state
        .lock()
        .unwrap()
        .corrupt_versions
        .insert(ids.version_two);
    assert_eq!(
        engine.synchronize_once().await.unwrap(),
        SyncOutcome::Blocked
    );
    assert_eq!(
        fs::read(harness.root.join("file.txt")).unwrap(),
        b"version one"
    );
    assert!(
        state
            .unresolved_issues(ids.scope.library_id())
            .await
            .unwrap()
            .iter()
            .any(|issue| issue.kind() == LocalIssueKind::ContentIntegrityMismatch)
    );
    drop(engine);
    drop(state);
    harness.cleanup();

    let (remote, ids) = DeterministicRemote::full();
    let harness = Harness::new(remote, ids.scope, "staged-restart");
    harness.baseline().await;
    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    for _ in 0..5 {
        engine.synchronize_once().await.unwrap();
    }
    drop(engine);
    drop(state);

    let (state, engine) = harness
        .open(Arc::new(FailOnce::new(
            FailurePoint::AfterContentStagedBeforeExpose,
        )))
        .await;
    assert!(matches!(
        engine.synchronize_once().await,
        Err(ClientSyncError::InjectedFailure)
    ));
    assert_eq!(
        fs::read(harness.root.join("file.txt")).unwrap(),
        b"version one"
    );
    drop(engine);
    drop(state);

    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    engine.synchronize_once().await.unwrap();
    assert_eq!(
        fs::read(harness.root.join("file.txt")).unwrap(),
        b"version two is streamed"
    );
    let record = state
        .replica(ids.scope.library_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.applied_sequence(), Sequence::new(6));
    assert_eq!(record.acknowledged_sequence(), Sequence::new(6));
    drop(engine);
    drop(state);
    harness.cleanup();
}

#[tokio::test]
async fn completed_manifest_sweeps_only_tracked_clean_nodes_and_preserves_unknown_files() {
    let (remote, ids) = DeterministicRemote::full();
    let harness = Harness::new(remote.clone(), ids.scope, "bootstrap-sweep");
    harness.baseline().await;
    fs::write(harness.root.join("unknown-local.txt"), b"keep me").unwrap();
    {
        let mut state = remote.state.lock().unwrap();
        let next_epoch = Sequence::new(8);
        state.bootstrap = SyncBootstrap::new(
            SyncBootstrapId::new(),
            ids.scope.owner_user_id(),
            ids.scope.device_id(),
            ids.scope.library_id(),
            Sequence::new(2),
            next_epoch,
            Sequence::new(0),
            1,
            Some(ids.root),
            SyncBootstrapState::Open,
            timestamp("2026-08-27T03:00:00.123456Z"),
            timestamp("2026-08-28T03:00:00.123456Z"),
            None,
        );
        state.snapshot_pages = vec![vec![snapshot_directory(
            ids.root,
            None,
            "root",
            NodeState::Active,
            1,
        )]];
        state.changes.clear();
        state.acknowledged = Sequence::new(0);
    }
    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    for _ in 0..20 {
        engine.synchronize_once().await.unwrap();
        let record = state
            .replica(ids.scope.library_id())
            .await
            .unwrap()
            .unwrap();
        if record.journal_epoch() == Sequence::new(8)
            && state
                .bootstrap(ids.scope.library_id())
                .await
                .unwrap()
                .is_none()
        {
            break;
        }
    }
    assert_eq!(
        fs::read(harness.root.join("unknown-local.txt")).unwrap(),
        b"keep me"
    );
    assert!(!harness.root.join("file.txt").exists());
    let nodes = state.local_nodes(ids.scope.library_id()).await.unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].node_id(), ids.root);
    drop(engine);
    drop(state);
    harness.cleanup();
}

#[tokio::test]
async fn collision_missing_source_and_stale_quarantine_fail_closed() {
    let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
    let root = NodeId::new();
    let first = NodeId::new();
    let second = NodeId::new();
    let remote = DeterministicRemote::root_with_changes(
        vec![
            change(
                scope,
                Sequence::new(7),
                1,
                first,
                ChangeKind::NodeCreated,
                1,
                Some(snapshot_directory(
                    first,
                    Some(root),
                    "File",
                    NodeState::Active,
                    1,
                )),
            ),
            change(
                scope,
                Sequence::new(7),
                2,
                second,
                ChangeKind::NodeCreated,
                1,
                Some(snapshot_directory(
                    second,
                    Some(root),
                    "file",
                    NodeState::Active,
                    1,
                )),
            ),
        ],
        scope,
        root,
    );
    let harness = Harness::new(remote, scope, "collision");
    harness.baseline().await;
    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    engine.synchronize_once().await.unwrap();
    assert_eq!(
        engine.synchronize_once().await.unwrap(),
        SyncOutcome::Blocked
    );
    assert_eq!(
        state.unresolved_issues(scope.library_id()).await.unwrap()[0].kind(),
        LocalIssueKind::LocalNameCollision
    );
    assert!(harness.root.join("File").is_dir());
    assert!(!harness.root.join("file").exists());
    drop(engine);
    drop(state);
    harness.cleanup();

    let (remote, ids) = DeterministicRemote::full();
    let harness = Harness::new(remote, ids.scope, "missing-source");
    harness.baseline().await;
    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    engine.synchronize_once().await.unwrap();
    fs::remove_dir(harness.root.join("newdir")).unwrap();
    assert_eq!(
        engine.synchronize_once().await.unwrap(),
        SyncOutcome::Blocked
    );
    assert!(
        state
            .unresolved_issues(ids.scope.library_id())
            .await
            .unwrap()
            .iter()
            .any(|issue| issue.kind() == LocalIssueKind::LocalDivergence)
    );
    drop(engine);
    drop(state);
    harness.cleanup();

    let (remote, ids) = DeterministicRemote::full();
    let harness = Harness::new(remote, ids.scope, "stale-quarantine");
    harness.baseline().await;
    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    for _ in 0..4 {
        engine.synchronize_once().await.unwrap();
    }
    let tracked = state
        .local_node(ids.scope.library_id(), ids.transient)
        .await
        .unwrap()
        .unwrap();
    let quarantine = tracked.quarantine_relative_path().unwrap();
    fs::write(
        harness.root.join(quarantine.as_path()).join("unknown"),
        b"drift",
    )
    .unwrap();
    assert_eq!(
        engine.synchronize_once().await.unwrap(),
        SyncOutcome::Blocked
    );
    assert!(!harness.root.join("folder/renamed").exists());
    assert!(
        state
            .unresolved_issues(ids.scope.library_id())
            .await
            .unwrap()
            .iter()
            .any(|issue| issue.kind() == LocalIssueKind::LocalDivergence)
    );
    drop(engine);
    drop(state);
    harness.cleanup();
}

#[tokio::test]
async fn divergent_file_is_not_purged_or_quarantined() {
    let (remote, ids) = DeterministicRemote::full();
    {
        let mut state = remote.state.lock().unwrap();
        state.bootstrap = SyncBootstrap::new(
            SyncBootstrapId::new(),
            ids.scope.owner_user_id(),
            ids.scope.device_id(),
            ids.scope.library_id(),
            Sequence::new(1),
            Sequence::new(7),
            Sequence::new(0),
            2,
            Some(ids.file),
            SyncBootstrapState::Open,
            timestamp("2026-08-27T01:00:00.123456Z"),
            timestamp("2026-08-28T01:00:00.123456Z"),
            None,
        );
        state.snapshot_pages = vec![vec![
            snapshot_directory(ids.root, None, "root", NodeState::Active, 1),
            snapshot_file(
                ids.file,
                ids.root,
                "file.txt",
                1,
                ids.version_one,
                b"version one",
            ),
        ]];
        state.changes = vec![change(
            ids.scope,
            Sequence::new(7),
            1,
            ids.file,
            ChangeKind::NodePurged,
            2,
            None,
        )];
    }
    let harness = Harness::new(remote, ids.scope, "purge-diverged");
    harness.baseline().await;
    fs::write(harness.root.join("file.txt"), b"local edit").unwrap();
    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    assert_eq!(
        engine.synchronize_once().await.unwrap(),
        SyncOutcome::Blocked
    );
    assert_eq!(
        fs::read(harness.root.join("file.txt")).unwrap(),
        b"local edit"
    );
    assert_eq!(
        state
            .replica(ids.scope.library_id())
            .await
            .unwrap()
            .unwrap()
            .acknowledged_sequence(),
        Sequence::new(0)
    );
    drop(engine);
    drop(state);
    harness.cleanup();
}

#[tokio::test]
async fn multi_event_page_applies_atomically_before_one_server_ack() {
    let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
    let root = NodeId::new();
    let first = NodeId::new();
    let second = NodeId::new();
    let remote = DeterministicRemote::root_with_changes(
        vec![
            change(
                scope,
                Sequence::new(7),
                1,
                first,
                ChangeKind::NodeCreated,
                1,
                Some(snapshot_directory(
                    first,
                    Some(root),
                    "first",
                    NodeState::Active,
                    1,
                )),
            ),
            change(
                scope,
                Sequence::new(7),
                2,
                second,
                ChangeKind::NodeCreated,
                1,
                Some(snapshot_directory(
                    second,
                    Some(root),
                    "second",
                    NodeState::Active,
                    1,
                )),
            ),
        ],
        scope,
        root,
    );
    remote.state.lock().unwrap().feed_batch_size = 2;
    let harness = Harness::new(remote.clone(), scope, "multi-event-page");
    harness.baseline().await;
    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    assert_eq!(
        engine.synchronize_once().await.unwrap(),
        SyncOutcome::Progressed
    );
    let record = state.replica(scope.library_id()).await.unwrap().unwrap();
    assert_eq!(record.applied_sequence(), Sequence::new(2));
    assert_eq!(record.acknowledged_sequence(), Sequence::new(2));
    assert_eq!(remote.ack_attempts(), 1);
    assert!(harness.root.join("first").is_dir());
    assert!(harness.root.join("second").is_dir());
    drop(engine);
    drop(state);
    harness.cleanup();
}

#[tokio::test]
async fn wrong_epoch_sequence_gap_and_unknown_kind_fail_closed() {
    assert!(ChangeKind::from_str("FUTURE_EVENT_KIND").is_err());

    let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
    let root = NodeId::new();
    let created = NodeId::new();
    let remote = DeterministicRemote::root_with_changes(
        vec![change(
            scope,
            Sequence::new(7),
            1,
            created,
            ChangeKind::NodeCreated,
            1,
            Some(snapshot_directory(
                created,
                Some(root),
                "wrong-epoch",
                NodeState::Active,
                1,
            )),
        )],
        scope,
        root,
    );
    let harness = Harness::new(remote.clone(), scope, "wrong-epoch");
    harness.baseline().await;
    remote.state.lock().unwrap().feed_epoch_override = Some(Sequence::new(8));
    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    assert!(matches!(
        engine.synchronize_once().await,
        Err(ClientSyncError::WrongEpoch)
    ));
    let record = state.replica(scope.library_id()).await.unwrap().unwrap();
    assert_eq!(record.applied_sequence(), Sequence::new(0));
    assert_eq!(record.acknowledged_sequence(), Sequence::new(0));
    assert!(!harness.root.join("wrong-epoch").exists());
    drop(engine);
    drop(state);
    harness.cleanup();

    let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
    let root = NodeId::new();
    let created = NodeId::new();
    let remote = DeterministicRemote::root_with_changes(
        vec![change(
            scope,
            Sequence::new(7),
            2,
            created,
            ChangeKind::NodeCreated,
            1,
            Some(snapshot_directory(
                created,
                Some(root),
                "gap",
                NodeState::Active,
                1,
            )),
        )],
        scope,
        root,
    );
    let harness = Harness::new(remote, scope, "sequence-gap");
    harness.baseline().await;
    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    assert!(matches!(
        engine.synchronize_once().await,
        Err(ClientSyncError::SequenceGap)
    ));
    let record = state.replica(scope.library_id()).await.unwrap().unwrap();
    assert_eq!(record.applied_sequence(), Sequence::new(0));
    assert_eq!(record.acknowledged_sequence(), Sequence::new(0));
    assert!(!harness.root.join("gap").exists());
    drop(engine);
    drop(state);
    harness.cleanup();
}

#[tokio::test]
async fn prepared_directory_operation_never_adopts_a_racing_unknown_directory() {
    let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
    let root = NodeId::new();
    let created = NodeId::new();
    let remote = DeterministicRemote::root_with_changes(
        vec![change(
            scope,
            Sequence::new(7),
            1,
            created,
            ChangeKind::NodeCreated,
            1,
            Some(snapshot_directory(
                created,
                Some(root),
                "raced",
                NodeState::Active,
                1,
            )),
        )],
        scope,
        root,
    );
    let harness = Harness::new(remote, scope, "racing-unknown-directory");
    harness.baseline().await;
    let (state, engine) = harness
        .open(Arc::new(FailOnce::new(
            FailurePoint::BeforeFilesystemAction,
        )))
        .await;
    assert!(matches!(
        engine.synchronize_once().await,
        Err(ClientSyncError::InjectedFailure)
    ));
    drop(engine);
    drop(state);

    fs::create_dir(harness.root.join("raced")).unwrap();
    fs::write(harness.root.join("raced/user.txt"), b"user-owned").unwrap();
    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    assert_eq!(
        engine.synchronize_once().await.unwrap(),
        SyncOutcome::Blocked
    );
    assert!(
        state
            .unresolved_issues(scope.library_id())
            .await
            .unwrap()
            .iter()
            .any(|issue| issue.kind() == LocalIssueKind::LocalRecoveryAmbiguous)
    );
    assert_eq!(
        fs::read(harness.root.join("raced/user.txt")).unwrap(),
        b"user-owned"
    );
    let record = state.replica(scope.library_id()).await.unwrap().unwrap();
    assert_eq!(record.applied_sequence(), Sequence::new(0));
    assert_eq!(record.acknowledged_sequence(), Sequence::new(0));
    drop(engine);
    drop(state);
    harness.cleanup();
}

#[tokio::test]
async fn unrepresentable_name_offline_retry_and_wrong_root_binding_are_safe() {
    let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
    let root = NodeId::new();
    let created = NodeId::new();
    let remote = DeterministicRemote::root_with_changes(
        vec![change(
            scope,
            Sequence::new(7),
            1,
            created,
            ChangeKind::NodeCreated,
            1,
            Some(snapshot_directory(
                created,
                Some(root),
                "CON",
                NodeState::Active,
                1,
            )),
        )],
        scope,
        root,
    );
    let harness = Harness::new(remote, scope, "unrepresentable-name");
    harness.baseline().await;
    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    assert_eq!(
        engine.synchronize_once().await.unwrap(),
        SyncOutcome::Blocked
    );
    assert_eq!(
        state.unresolved_issues(scope.library_id()).await.unwrap()[0].kind(),
        LocalIssueKind::LocalNameUnrepresentable
    );
    assert!(!harness.root.join("CON").exists());
    drop(engine);
    drop(state);
    harness.cleanup();

    let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
    let root = NodeId::new();
    let remote = DeterministicRemote::root_with_changes(vec![], scope, root);
    let harness = Harness::new(remote.clone(), scope, "offline-and-binding");
    harness.baseline().await;
    remote.state.lock().unwrap().offline = true;
    let (state, engine) = harness.open(Arc::new(NoFailure)).await;
    assert_eq!(
        engine.synchronize_once().await.unwrap(),
        SyncOutcome::Offline
    );
    let record = state.replica(scope.library_id()).await.unwrap().unwrap();
    assert_eq!(record.applied_sequence(), Sequence::new(0));
    assert_eq!(record.acknowledged_sequence(), Sequence::new(0));
    assert_eq!(record.status(), EngineStatus::Offline);
    drop(engine);
    drop(state);

    let second_root = harness.base.join("second-managed-root");
    fs::create_dir(&second_root).unwrap();
    let second_replica = Arc::new(FilesystemLocalReplica::initialize(&second_root, scope).unwrap());
    let state = Arc::new(
        LocalStateStore::open(&LocalStateConfig::new(&harness.database))
            .await
            .unwrap(),
    );
    assert!(matches!(
        InboundSyncEngine::new(
            scope,
            remote,
            second_replica,
            state.clone(),
            EngineConfig::default(),
        )
        .await,
        Err(ClientSyncError::WrongRootBinding)
    ));
    drop(state);
    harness.cleanup();
}
