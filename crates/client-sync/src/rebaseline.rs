//! Durable, transport-neutral local application of a server-created snapshot.
//!
//! Pages are committed to a candidate namespace one at a time.  Only after the
//! complete candidate has passed aggregate validation does SQLite replace the
//! authoritative local mirror in one transaction.  Outbound intent and upload
//! rows are intentionally outside that transaction.

use std::sync::Arc;

use async_trait::async_trait;
use synveil_core::{LibraryId, LogicalSnapshotNode, RebaselineSnapshotId, Sequence};

use crate::{
    ClientSyncError, LocalStateStore, MAX_PAGE_ITEMS, MAX_REBASELINE_ITEMS, OpaqueEvidence,
    RemoteError, ReplicaScope,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RebaselineBoundary {
    journal_epoch: Sequence,
    resume_sequence: Sequence,
}

impl RebaselineBoundary {
    #[must_use]
    pub const fn new(journal_epoch: Sequence, resume_sequence: Sequence) -> Self {
        Self {
            journal_epoch,
            resume_sequence,
        }
    }
    #[must_use]
    pub const fn journal_epoch(self) -> Sequence {
        self.journal_epoch
    }
    #[must_use]
    pub const fn resume_sequence(self) -> Sequence {
        self.resume_sequence
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RebaselineSnapshotDescriptor {
    snapshot_id: RebaselineSnapshotId,
    library_id: LibraryId,
    boundary: RebaselineBoundary,
    entry_count: u64,
}

impl RebaselineSnapshotDescriptor {
    #[must_use]
    pub const fn new(
        snapshot_id: RebaselineSnapshotId,
        library_id: LibraryId,
        boundary: RebaselineBoundary,
        entry_count: u64,
    ) -> Self {
        Self {
            snapshot_id,
            library_id,
            boundary,
            entry_count,
        }
    }
    #[must_use]
    pub const fn snapshot_id(self) -> RebaselineSnapshotId {
        self.snapshot_id
    }
    #[must_use]
    pub const fn library_id(self) -> LibraryId {
        self.library_id
    }
    #[must_use]
    pub const fn boundary(self) -> RebaselineBoundary {
        self.boundary
    }
    #[must_use]
    pub const fn entry_count(self) -> u64 {
        self.entry_count
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RebaselineSnapshotPage {
    descriptor: RebaselineSnapshotDescriptor,
    entries: Vec<LogicalSnapshotNode>,
    next_cursor: Option<OpaqueEvidence>,
}

impl RebaselineSnapshotPage {
    pub fn new(
        descriptor: RebaselineSnapshotDescriptor,
        entries: Vec<LogicalSnapshotNode>,
        next_cursor: Option<OpaqueEvidence>,
    ) -> Result<Self, ClientSyncError> {
        if entries.len() > MAX_PAGE_ITEMS || descriptor.entry_count() > MAX_REBASELINE_ITEMS {
            return Err(ClientSyncError::ResourceLimit);
        }
        Ok(Self {
            descriptor,
            entries,
            next_cursor,
        })
    }
    #[must_use]
    pub const fn descriptor(&self) -> RebaselineSnapshotDescriptor {
        self.descriptor
    }
    #[must_use]
    pub fn entries(&self) -> &[LogicalSnapshotNode] {
        &self.entries
    }
    #[must_use]
    pub const fn next_cursor(&self) -> Option<&OpaqueEvidence> {
        self.next_cursor.as_ref()
    }
}

/// Reads already-created snapshot pages. It has no create, ACK, or checkpoint
/// operation, which prevents this primitive from advancing server state.
#[async_trait]
pub trait RebaselineSnapshotSource: Send + Sync {
    async fn read_page(
        &self,
        scope: ReplicaScope,
        snapshot_id: RebaselineSnapshotId,
        cursor: Option<&OpaqueEvidence>,
        limit: u32,
    ) -> Result<RebaselineSnapshotPage, RemoteError>;
}

/// Authenticated transport for the four existing durable rebaseline
/// operations. Snapshot creation returns only the server-authored descriptor;
/// the caller never supplies a checkpoint, epoch, or sequence.
#[async_trait]
pub trait RebaselineSnapshotRemote: RebaselineSnapshotSource {
    async fn create_snapshot(
        &self,
        scope: ReplicaScope,
    ) -> Result<RebaselineSnapshotDescriptor, RemoteError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RebaselineApplyOutcome {
    Applied,
    AlreadyApplied,
}

/// Result of finishing the local/server checkpoint handoff. `AlreadyComplete`
/// is safe after a response-loss retry or a second caller observes that the
/// same local finalization transaction already committed; it never recreates a
/// candidate or rewrites the snapshot mirror.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RebaselineHandoffOutcome {
    Completed,
    AlreadyComplete,
}

pub struct RebaselineApplier {
    scope: ReplicaScope,
    state: Arc<LocalStateStore>,
    page_limit: u32,
}

impl RebaselineApplier {
    pub fn new(
        scope: ReplicaScope,
        state: Arc<LocalStateStore>,
        page_limit: u32,
    ) -> Result<Self, ClientSyncError> {
        if !(1..=MAX_PAGE_ITEMS as u32).contains(&page_limit) {
            return Err(ClientSyncError::ResourceLimit);
        }
        Ok(Self {
            scope,
            state,
            page_limit,
        })
    }

    pub async fn apply(
        &self,
        descriptor: RebaselineSnapshotDescriptor,
        source: &dyn RebaselineSnapshotSource,
    ) -> Result<RebaselineApplyOutcome, ClientSyncError> {
        if descriptor.library_id() != self.scope.library_id() {
            return Err(ClientSyncError::WrongScope);
        }
        if descriptor.entry_count() == 0 || descriptor.entry_count() > MAX_REBASELINE_ITEMS {
            return Err(ClientSyncError::ResourceLimit);
        }
        match self
            .state
            .rebaseline_is_applied(self.scope.library_id(), descriptor)
            .await
        {
            Ok(true) => return Ok(RebaselineApplyOutcome::AlreadyApplied),
            Ok(false) | Err(ClientSyncError::RebaselinePendingHandoff) => {
                // A different pending marker is the deliberate Prompt 87
                // recovery shape: H1 remains the inbound fence while this
                // separately staged candidate becomes H2 atomically.
            }
            Err(error) => return Err(error),
        }
        self.state
            .begin_rebaseline_candidate(self.scope, descriptor)
            .await?;
        loop {
            let candidate = self
                .state
                .rebaseline_candidate(self.scope.library_id())
                .await?
                .ok_or(ClientSyncError::CandidateCorrupt)?;
            if candidate.terminal_fetched {
                break;
            }
            let page = source
                .read_page(
                    self.scope,
                    descriptor.snapshot_id(),
                    candidate.next_cursor.as_ref(),
                    self.page_limit,
                )
                .await?;
            self.state
                .persist_rebaseline_page(self.scope, descriptor, &page)
                .await?;
        }
        self.state
            .activate_rebaseline_candidate(self.scope, descriptor)
            .await?;
        Ok(RebaselineApplyOutcome::Applied)
    }

    /// Removes only an incomplete local candidate; it cannot alter active
    /// metadata, local bytes, outbound work, or an applied handoff.
    pub async fn abort_candidate(&self) -> Result<(), ClientSyncError> {
        self.state
            .abort_rebaseline_candidate(self.scope.library_id())
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        fs,
        sync::{Arc, Mutex},
    };

    use super::*;
    use crate::{
        LocalFingerprint, LocalNode, LocalStateConfig, ManagedRelativePath, OutboundIntent,
        OutboundIntentKind, RootBindingId, SyncConflictKind,
    };
    use synveil_core::{
        DeviceId, LibraryId, LogicalName, NodeId, NodeKind, NodeState, Revision, Sequence, UserId,
    };

    struct OnePageSource(Mutex<Option<RebaselineSnapshotPage>>);
    #[async_trait]
    impl RebaselineSnapshotSource for OnePageSource {
        async fn read_page(
            &self,
            _scope: ReplicaScope,
            _snapshot_id: RebaselineSnapshotId,
            _cursor: Option<&OpaqueEvidence>,
            _limit: u32,
        ) -> Result<RebaselineSnapshotPage, RemoteError> {
            self.0
                .lock()
                .unwrap()
                .take()
                .ok_or(RemoteError::new(crate::RemoteErrorKind::Protocol))
        }
    }

    struct ScriptedSource(Mutex<VecDeque<Result<RebaselineSnapshotPage, RemoteError>>>);
    #[async_trait]
    impl RebaselineSnapshotSource for ScriptedSource {
        async fn read_page(
            &self,
            _scope: ReplicaScope,
            _snapshot_id: RebaselineSnapshotId,
            _cursor: Option<&OpaqueEvidence>,
            _limit: u32,
        ) -> Result<RebaselineSnapshotPage, RemoteError> {
            self.0
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(RemoteError::new(crate::RemoteErrorKind::Protocol)))
        }
    }

    fn directory(id: NodeId, parent: Option<NodeId>, name: &str) -> LogicalSnapshotNode {
        LogicalSnapshotNode::new(
            id,
            parent,
            LogicalName::new(name).unwrap(),
            NodeKind::Directory,
            NodeState::Active,
            Revision::new(1),
            None,
            None,
            None,
        )
        .unwrap()
    }

    fn cursor(value: u8) -> OpaqueEvidence {
        OpaqueEvidence::new(vec![b'a' + value]).unwrap()
    }

    fn page_cursor(value: usize) -> OpaqueEvidence {
        OpaqueEvidence::new(format!("cursor-{value}").into_bytes()).unwrap()
    }

    async fn setup(
        label: &str,
    ) -> (
        Arc<LocalStateStore>,
        std::path::PathBuf,
        ReplicaScope,
        NodeId,
        NodeId,
        OutboundIntent,
    ) {
        let directory_path = std::env::temp_dir().join(format!(
            "synveil-rebaseline-{label}-{}",
            uuid::Uuid::now_v7()
        ));
        fs::create_dir(&directory_path).unwrap();
        let store = Arc::new(
            LocalStateStore::open(&LocalStateConfig::new(directory_path.join("state.sqlite3")))
                .await
                .unwrap(),
        );
        let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
        store
            .bind_replica(scope, RootBindingId::new())
            .await
            .unwrap();
        let root = NodeId::new();
        let obsolete = NodeId::new();
        for node in [
            LocalNode::new(
                scope.library_id(),
                root,
                None,
                ManagedRelativePath::root(),
                LogicalName::new("root").unwrap(),
                NodeKind::Directory,
                NodeState::Active,
                Revision::new(1),
                None,
                None,
                None,
                None,
                None,
                Sequence::new(0),
                false,
                None,
            ),
            LocalNode::new(
                scope.library_id(),
                obsolete,
                Some(root),
                ManagedRelativePath::new("obsolete").unwrap(),
                LogicalName::new("obsolete").unwrap(),
                NodeKind::Directory,
                NodeState::Active,
                Revision::new(1),
                None,
                None,
                None,
                None,
                None,
                Sequence::new(0),
                false,
                None,
            ),
        ] {
            store.upsert_local_node(&node).await.unwrap();
        }
        let intent = OutboundIntent::new(
            scope.library_id(),
            Some(obsolete),
            None,
            OutboundIntentKind::RenameNode,
            ManagedRelativePath::new("local-name").unwrap(),
            Some(ManagedRelativePath::new("obsolete").unwrap()),
            Some(LocalFingerprint::directory()),
            Sequence::new(1),
            Sequence::new(1),
            Some(Revision::new(1)),
            None,
            None,
        )
        .unwrap();
        store.upsert_outbound_intent(&intent).await.unwrap();
        (store, directory_path, scope, root, obsolete, intent)
    }

    async fn assert_old_and_intent(
        store: &LocalStateStore,
        scope: ReplicaScope,
        obsolete: NodeId,
        intent: &OutboundIntent,
    ) {
        assert!(
            store
                .local_node(scope.library_id(), obsolete)
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(
            store.outbound_intent(intent.intent_id()).await.unwrap(),
            Some(intent.clone())
        );
        assert!(
            !store
                .rebaseline_handoff_pending(scope.library_id())
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn activation_replaces_only_the_remote_mirror_and_fences_inbound() {
        let directory_path =
            std::env::temp_dir().join(format!("synveil-rebaseline-{}", uuid::Uuid::now_v7()));
        fs::create_dir(&directory_path).unwrap();
        let database = directory_path.join("state.sqlite3");
        let store = Arc::new(
            LocalStateStore::open(&LocalStateConfig::new(&database))
                .await
                .unwrap(),
        );
        let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
        store
            .bind_replica(scope, RootBindingId::new())
            .await
            .unwrap();
        let root = NodeId::new();
        let obsolete = NodeId::new();
        store
            .upsert_local_node(&LocalNode::new(
                scope.library_id(),
                root,
                None,
                ManagedRelativePath::root(),
                LogicalName::new("root").unwrap(),
                NodeKind::Directory,
                NodeState::Active,
                Revision::new(1),
                None,
                None,
                None,
                None,
                None,
                Sequence::new(0),
                false,
                None,
            ))
            .await
            .unwrap();
        store
            .upsert_local_node(&LocalNode::new(
                scope.library_id(),
                obsolete,
                Some(root),
                ManagedRelativePath::new("obsolete").unwrap(),
                LogicalName::new("obsolete").unwrap(),
                NodeKind::Directory,
                NodeState::Active,
                Revision::new(1),
                None,
                None,
                None,
                None,
                None,
                Sequence::new(0),
                false,
                None,
            ))
            .await
            .unwrap();
        let intent = OutboundIntent::new(
            scope.library_id(),
            Some(obsolete),
            None,
            OutboundIntentKind::RenameNode,
            ManagedRelativePath::new("local-name").unwrap(),
            Some(ManagedRelativePath::new("obsolete").unwrap()),
            Some(LocalFingerprint::directory()),
            Sequence::new(1),
            Sequence::new(1),
            Some(Revision::new(1)),
            None,
            None,
        )
        .unwrap();
        store.upsert_outbound_intent(&intent).await.unwrap();
        let new_child = NodeId::new();
        let boundary = RebaselineBoundary::new(Sequence::new(9), Sequence::new(17));
        let descriptor = RebaselineSnapshotDescriptor::new(
            RebaselineSnapshotId::new(),
            scope.library_id(),
            boundary,
            2,
        );
        let mut entries = vec![
            directory(root, None, "root"),
            directory(new_child, Some(root), "server"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        let source = OnePageSource(Mutex::new(Some(
            RebaselineSnapshotPage::new(descriptor, entries, None).unwrap(),
        )));
        let applier = RebaselineApplier::new(scope, Arc::clone(&store), 256).unwrap();
        assert_eq!(
            applier.apply(descriptor, &source).await.unwrap(),
            RebaselineApplyOutcome::Applied
        );
        assert!(
            store
                .local_node(scope.library_id(), obsolete)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .local_node(scope.library_id(), new_child)
                .await
                .unwrap()
                .is_some()
        );
        let preserved = store
            .outbound_intent(intent.intent_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(preserved, intent);
        let conflicts = store
            .list_unresolved_conflicts(scope.library_id(), None, None)
            .await
            .unwrap();
        assert_eq!(conflicts.items().len(), 1);
        assert_eq!(conflicts.items()[0].intent_id(), intent.intent_id());
        assert_eq!(conflicts.items()[0].kind(), SyncConflictKind::RemoteMissing);
        assert!(
            store
                .rebaseline_handoff_pending(scope.library_id())
                .await
                .unwrap()
        );
        assert_eq!(
            applier.apply(descriptor, &source).await.unwrap(),
            RebaselineApplyOutcome::AlreadyApplied
        );
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(directory_path).unwrap();
    }

    #[tokio::test]
    async fn proof_loss_s1_to_s2_creates_one_conflict_and_preserves_the_original_intent() {
        let (store, directory_path, scope, root, obsolete, intent) = setup("s1-s2-conflict").await;
        let applier = RebaselineApplier::new(scope, Arc::clone(&store), 256).unwrap();

        let s1 = RebaselineSnapshotDescriptor::new(
            RebaselineSnapshotId::new(),
            scope.library_id(),
            RebaselineBoundary::new(Sequence::new(5), Sequence::new(10)),
            2,
        );
        let mut s1_entries = vec![
            directory(root, None, "root"),
            directory(obsolete, Some(root), "obsolete"),
        ];
        s1_entries.sort_by_key(LogicalSnapshotNode::node_id);
        let s1_source = OnePageSource(Mutex::new(Some(
            RebaselineSnapshotPage::new(s1, s1_entries, None).unwrap(),
        )));
        assert_eq!(
            applier.apply(s1, &s1_source).await.unwrap(),
            RebaselineApplyOutcome::Applied
        );
        assert!(
            store
                .list_unresolved_conflicts(scope.library_id(), None, None)
                .await
                .unwrap()
                .items()
                .is_empty()
        );

        let s2 = RebaselineSnapshotDescriptor::new(
            RebaselineSnapshotId::new(),
            scope.library_id(),
            RebaselineBoundary::new(Sequence::new(6), Sequence::new(20)),
            2,
        );
        let changed = LogicalSnapshotNode::new(
            obsolete,
            Some(root),
            LogicalName::new("server-renamed").unwrap(),
            NodeKind::Directory,
            NodeState::Active,
            Revision::new(2),
            None,
            None,
            None,
        )
        .unwrap();
        let mut s2_entries = vec![directory(root, None, "root"), changed];
        s2_entries.sort_by_key(LogicalSnapshotNode::node_id);
        let s2_source = OnePageSource(Mutex::new(Some(
            RebaselineSnapshotPage::new(s2, s2_entries, None).unwrap(),
        )));
        assert_eq!(
            applier.apply(s2, &s2_source).await.unwrap(),
            RebaselineApplyOutcome::Applied
        );
        let conflicts = store
            .list_unresolved_conflicts(scope.library_id(), None, None)
            .await
            .unwrap();
        assert_eq!(conflicts.items().len(), 1);
        assert_eq!(conflicts.items()[0].intent_id(), intent.intent_id());
        assert_eq!(
            conflicts.items()[0].kind(),
            SyncConflictKind::RemoteRevisionChanged
        );
        assert_eq!(conflicts.items()[0].remote_epoch(), Some(Sequence::new(6)));
        assert_eq!(
            conflicts.items()[0].remote_sequence(),
            Some(Sequence::new(20))
        );
        assert_eq!(
            store.outbound_intent(intent.intent_id()).await.unwrap(),
            Some(intent)
        );
        assert_eq!(
            applier.apply(s2, &s2_source).await.unwrap(),
            RebaselineApplyOutcome::AlreadyApplied
        );
        assert_eq!(
            store
                .list_unresolved_conflicts(scope.library_id(), None, None)
                .await
                .unwrap()
                .items()
                .len(),
            1
        );

        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(directory_path).unwrap();
    }

    #[tokio::test]
    async fn malformed_pages_fail_closed_without_touching_active_or_outbound_state() {
        for case in 0..8_u8 {
            let (store, directory_path, scope, root, obsolete, intent) = setup("malformed").await;
            let descriptor = RebaselineSnapshotDescriptor::new(
                RebaselineSnapshotId::new(),
                scope.library_id(),
                RebaselineBoundary::new(Sequence::new(2), Sequence::new(3)),
                2,
            );
            let other_descriptor = RebaselineSnapshotDescriptor::new(
                RebaselineSnapshotId::new(),
                scope.library_id(),
                descriptor.boundary(),
                2,
            );
            let child = NodeId::new();
            let mut valid = vec![
                directory(root, None, "root"),
                directory(child, Some(root), "child"),
            ];
            valid.sort_by_key(LogicalSnapshotNode::node_id);
            let page = match case {
                0 => RebaselineSnapshotPage::new(other_descriptor, valid, None).unwrap(),
                1 => RebaselineSnapshotPage::new(
                    RebaselineSnapshotDescriptor::new(
                        descriptor.snapshot_id(),
                        LibraryId::new(),
                        descriptor.boundary(),
                        2,
                    ),
                    valid,
                    None,
                )
                .unwrap(),
                2 => RebaselineSnapshotPage::new(
                    RebaselineSnapshotDescriptor::new(
                        descriptor.snapshot_id(),
                        scope.library_id(),
                        RebaselineBoundary::new(Sequence::new(9), Sequence::new(3)),
                        2,
                    ),
                    valid,
                    None,
                )
                .unwrap(),
                3 => RebaselineSnapshotPage::new(
                    RebaselineSnapshotDescriptor::new(
                        descriptor.snapshot_id(),
                        scope.library_id(),
                        descriptor.boundary(),
                        3,
                    ),
                    valid,
                    None,
                )
                .unwrap(),
                4 => RebaselineSnapshotPage::new(
                    descriptor,
                    vec![directory(root, None, "root"), directory(root, None, "root")],
                    None,
                )
                .unwrap(),
                5 => {
                    valid.reverse();
                    RebaselineSnapshotPage::new(descriptor, valid, None).unwrap()
                }
                6 => RebaselineSnapshotPage::new(descriptor, vec![], Some(cursor(1))).unwrap(),
                _ => RebaselineSnapshotPage::new(
                    descriptor,
                    vec![directory(root, None, "root")],
                    None,
                )
                .unwrap(),
            };
            let source = ScriptedSource(Mutex::new(VecDeque::from([Ok(page)])));
            let applier = RebaselineApplier::new(scope, Arc::clone(&store), 256).unwrap();
            assert!(
                applier.apply(descriptor, &source).await.is_err(),
                "case {case}"
            );
            assert_old_and_intent(&store, scope, obsolete, &intent).await;
            store.close_pool().await;
            drop(store);
            fs::remove_dir_all(directory_path).unwrap();
        }
    }

    #[tokio::test]
    async fn durable_resume_cursor_cycle_and_transport_failure_do_not_activate_partial_state() {
        let (store, directory_path, scope, root, obsolete, intent) = setup("resume").await;
        let descriptor = RebaselineSnapshotDescriptor::new(
            RebaselineSnapshotId::new(),
            scope.library_id(),
            RebaselineBoundary::new(Sequence::new(2), Sequence::new(3)),
            2,
        );
        let first = RebaselineSnapshotPage::new(
            descriptor,
            vec![directory(root, None, "root")],
            Some(cursor(1)),
        )
        .unwrap();
        let failing = ScriptedSource(Mutex::new(VecDeque::from([
            Ok(first),
            Err(RemoteError::new(crate::RemoteErrorKind::Offline)),
        ])));
        let applier = RebaselineApplier::new(scope, Arc::clone(&store), 1).unwrap();
        assert!(applier.apply(descriptor, &failing).await.is_err());
        assert_old_and_intent(&store, scope, obsolete, &intent).await;
        assert_eq!(
            store
                .rebaseline_candidate(scope.library_id())
                .await
                .unwrap()
                .unwrap()
                .received_count,
            1
        );
        let child = NodeId::new();
        let resumed = ScriptedSource(Mutex::new(VecDeque::from([Ok(
            RebaselineSnapshotPage::new(
                descriptor,
                vec![directory(child, Some(root), "child")],
                None,
            )
            .unwrap(),
        )])));
        assert_eq!(
            applier.apply(descriptor, &resumed).await.unwrap(),
            RebaselineApplyOutcome::Applied
        );
        assert!(
            store
                .local_node(scope.library_id(), child)
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(
            store.outbound_intent(intent.intent_id()).await.unwrap(),
            Some(intent)
        );
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(directory_path).unwrap();
    }

    #[tokio::test]
    async fn large_snapshots_apply_equivalently_at_one_256_and_1000_item_pages() {
        for page_size in [1_usize, 256, 1000] {
            let (store, directory_path, scope, root, _obsolete, intent) = setup("large").await;
            let mut entries = Vec::with_capacity(5_000);
            entries.push(directory(root, None, "root"));
            for index in 0..4_999 {
                entries.push(directory(
                    NodeId::new(),
                    Some(root),
                    &format!("node-{index}"),
                ));
            }
            entries.sort_by_key(LogicalSnapshotNode::node_id);
            let descriptor = RebaselineSnapshotDescriptor::new(
                RebaselineSnapshotId::new(),
                scope.library_id(),
                RebaselineBoundary::new(Sequence::new(4), Sequence::new(8)),
                entries.len() as u64,
            );
            let pages = entries
                .chunks(page_size)
                .enumerate()
                .map(|(index, chunk)| {
                    let next =
                        (index + 1 < entries.len().div_ceil(page_size)).then(|| page_cursor(index));
                    Ok(RebaselineSnapshotPage::new(descriptor, chunk.to_vec(), next).unwrap())
                })
                .collect();
            let source = ScriptedSource(Mutex::new(pages));
            let applier =
                RebaselineApplier::new(scope, Arc::clone(&store), page_size as u32).unwrap();
            assert_eq!(
                applier.apply(descriptor, &source).await.unwrap(),
                RebaselineApplyOutcome::Applied,
                "page size {page_size}"
            );
            assert_eq!(
                store.local_nodes(scope.library_id()).await.unwrap().len(),
                5_000
            );
            assert_eq!(
                store.outbound_intent(intent.intent_id()).await.unwrap(),
                Some(intent)
            );
            store.close_pool().await;
            drop(store);
            fs::remove_dir_all(directory_path).unwrap();
        }
    }

    #[tokio::test]
    async fn activation_transaction_rollback_keeps_old_base_candidate_and_outbound_intent() {
        let (store, directory_path, scope, root, obsolete, intent) = setup("rollback").await;
        let descriptor = RebaselineSnapshotDescriptor::new(
            RebaselineSnapshotId::new(),
            scope.library_id(),
            RebaselineBoundary::new(Sequence::new(2), Sequence::new(3)),
            2,
        );
        store
            .begin_rebaseline_candidate(scope, descriptor)
            .await
            .unwrap();
        let child = NodeId::new();
        store
            .persist_rebaseline_page(
                scope,
                descriptor,
                &RebaselineSnapshotPage::new(
                    descriptor,
                    vec![
                        directory(root, None, "root"),
                        directory(child, Some(root), "child"),
                    ],
                    None,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(matches!(
            store
                .activate_rebaseline_candidate_fail_before_commit(scope, descriptor)
                .await,
            Err(ClientSyncError::InjectedFailure)
        ));
        assert_old_and_intent(&store, scope, obsolete, &intent).await;
        assert!(
            store
                .rebaseline_candidate(scope.library_id())
                .await
                .unwrap()
                .is_some()
        );
        store
            .activate_rebaseline_candidate(scope, descriptor)
            .await
            .unwrap();
        assert!(
            store
                .local_node(scope.library_id(), child)
                .await
                .unwrap()
                .is_some()
        );
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(directory_path).unwrap();
    }

    #[tokio::test]
    async fn activation_and_new_outbound_intent_serialize_without_loss() {
        for round in 0..25_u32 {
            let (store, directory_path, scope, root, _obsolete, _intent) =
                setup("concurrent").await;
            let descriptor = RebaselineSnapshotDescriptor::new(
                RebaselineSnapshotId::new(),
                scope.library_id(),
                RebaselineBoundary::new(Sequence::new(2), Sequence::new(3)),
                2,
            );
            store
                .begin_rebaseline_candidate(scope, descriptor)
                .await
                .unwrap();
            let child = NodeId::new();
            store
                .persist_rebaseline_page(
                    scope,
                    descriptor,
                    &RebaselineSnapshotPage::new(
                        descriptor,
                        vec![
                            directory(root, None, "root"),
                            directory(child, Some(root), "child"),
                        ],
                        None,
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            let applier = RebaselineApplier::new(scope, Arc::clone(&store), 256).unwrap();
            let inserted = OutboundIntent::new(
                scope.library_id(),
                None,
                None,
                OutboundIntentKind::CreateDirectory,
                ManagedRelativePath::new(format!("created-{round}")).unwrap(),
                None,
                Some(LocalFingerprint::directory()),
                Sequence::new(1),
                Sequence::new(1),
                None,
                None,
                None,
            )
            .unwrap();
            let source = OnePageSource(Mutex::new(None));
            let activate = applier.apply(descriptor, &source);
            let persist = store.upsert_outbound_intent(&inserted);
            let (activation, persistence) = tokio::join!(activate, persist);
            assert_eq!(activation.unwrap(), RebaselineApplyOutcome::Applied);
            assert_eq!(persistence.unwrap(), inserted);
            assert_eq!(
                store.outbound_intent(inserted.intent_id()).await.unwrap(),
                Some(inserted)
            );
            store.close_pool().await;
            drop(store);
            fs::remove_dir_all(directory_path).unwrap();
        }
    }
}
