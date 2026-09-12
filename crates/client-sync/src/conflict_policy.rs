//! Durable, client-local synchronization conflict policy.
//!
//! A conflict never changes the authoritative remote base and never rewrites
//! the original outbound precondition. Resolution is an explicit local
//! transaction; later network submission remains the outbound engine's job.

use std::str::FromStr;

use synveil_core::{
    ConflictResolutionId, LibraryId, NodeId, NodeState, OutboundIntentId, Revision, Sequence,
    SyncConflictId,
};

use crate::ClientSyncError;

pub const DEFAULT_CONFLICT_PAGE_LIMIT: u32 = 100;
pub const MAX_CONFLICT_PAGE_LIMIT: u32 = 1_000;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SyncConflictKind {
    RemoteRevisionChanged,
    RemoteContentChanged,
    RemoteStateChanged,
    RemoteMissing,
    NameCollision,
    ParentChangedOrUnavailable,
}

impl SyncConflictKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RemoteRevisionChanged => "REMOTE_REVISION_CHANGED",
            Self::RemoteContentChanged => "REMOTE_CONTENT_CHANGED",
            Self::RemoteStateChanged => "REMOTE_STATE_CHANGED",
            Self::RemoteMissing => "REMOTE_MISSING",
            Self::NameCollision => "NAME_COLLISION",
            Self::ParentChangedOrUnavailable => "PARENT_CHANGED_OR_UNAVAILABLE",
        }
    }

    pub(crate) fn from_server_reason(reason: &str) -> Result<Self, ClientSyncError> {
        match reason {
            "REVISION_MISMATCH" => Ok(Self::RemoteRevisionChanged),
            "NODE_STATE_CHANGED" => Ok(Self::RemoteStateChanged),
            "PARENT_CHANGED" | "DESTINATION_CHANGED" => Ok(Self::ParentChangedOrUnavailable),
            "NAME_OCCUPIED" => Ok(Self::NameCollision),
            "RESOURCE_PURGED" => Ok(Self::RemoteMissing),
            _ => Ok(Self::RemoteStateChanged),
        }
    }
}

impl FromStr for SyncConflictKind {
    type Err = ClientSyncError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "REMOTE_REVISION_CHANGED" => Ok(Self::RemoteRevisionChanged),
            "REMOTE_CONTENT_CHANGED" => Ok(Self::RemoteContentChanged),
            "REMOTE_STATE_CHANGED" => Ok(Self::RemoteStateChanged),
            "REMOTE_MISSING" => Ok(Self::RemoteMissing),
            "NAME_COLLISION" => Ok(Self::NameCollision),
            "PARENT_CHANGED_OR_UNAVAILABLE" => Ok(Self::ParentChangedOrUnavailable),
            _ => Err(ClientSyncError::InvalidState),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncConflictStatus {
    Unresolved,
    Resolved,
}

impl SyncConflictStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unresolved => "UNRESOLVED",
            Self::Resolved => "RESOLVED",
        }
    }
}

impl FromStr for SyncConflictStatus {
    type Err = ClientSyncError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "UNRESOLVED" => Ok(Self::Unresolved),
            "RESOLVED" => Ok(Self::Resolved),
            _ => Err(ClientSyncError::InvalidState),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncConflictResolution {
    AcceptRemote,
    RetryLocalAgainstCurrentBase,
}

impl SyncConflictResolution {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AcceptRemote => "ACCEPT_REMOTE",
            Self::RetryLocalAgainstCurrentBase => "RETRY_LOCAL_AGAINST_CURRENT_BASE",
        }
    }
}

impl FromStr for SyncConflictResolution {
    type Err = ClientSyncError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "ACCEPT_REMOTE" => Ok(Self::AcceptRemote),
            "RETRY_LOCAL_AGAINST_CURRENT_BASE" => Ok(Self::RetryLocalAgainstCurrentBase),
            _ => Err(ClientSyncError::InvalidState),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConflictCursor {
    detected_at_ms: u64,
    conflict_id: SyncConflictId,
}

impl ConflictCursor {
    #[must_use]
    pub const fn new(detected_at_ms: u64, conflict_id: SyncConflictId) -> Self {
        Self {
            detected_at_ms,
            conflict_id,
        }
    }

    #[must_use]
    pub const fn detected_at_ms(self) -> u64 {
        self.detected_at_ms
    }

    #[must_use]
    pub const fn conflict_id(self) -> SyncConflictId {
        self.conflict_id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncConflictRecord {
    pub(crate) conflict_id: SyncConflictId,
    pub(crate) library_id: LibraryId,
    pub(crate) intent_id: OutboundIntentId,
    pub(crate) node_id: Option<NodeId>,
    pub(crate) kind: SyncConflictKind,
    pub(crate) local_base_revision: Option<Revision>,
    pub(crate) remote_observed_revision: Option<Revision>,
    pub(crate) remote_observed_state: Option<NodeState>,
    pub(crate) remote_parent_node_id: Option<NodeId>,
    pub(crate) remote_epoch: Option<Sequence>,
    pub(crate) remote_sequence: Option<Sequence>,
    pub(crate) detected_at_ms: u64,
    pub(crate) status: SyncConflictStatus,
    pub(crate) resolution_id: Option<ConflictResolutionId>,
    pub(crate) resolution: Option<SyncConflictResolution>,
    pub(crate) resolved_at_ms: Option<u64>,
    pub(crate) replacement_intent_id: Option<OutboundIntentId>,
}

impl SyncConflictRecord {
    #[must_use]
    pub const fn conflict_id(&self) -> SyncConflictId {
        self.conflict_id
    }
    #[must_use]
    pub const fn library_id(&self) -> LibraryId {
        self.library_id
    }
    #[must_use]
    pub const fn intent_id(&self) -> OutboundIntentId {
        self.intent_id
    }
    #[must_use]
    pub const fn node_id(&self) -> Option<NodeId> {
        self.node_id
    }
    #[must_use]
    pub const fn kind(&self) -> SyncConflictKind {
        self.kind
    }
    #[must_use]
    pub const fn local_base_revision(&self) -> Option<Revision> {
        self.local_base_revision
    }
    #[must_use]
    pub const fn remote_observed_revision(&self) -> Option<Revision> {
        self.remote_observed_revision
    }
    #[must_use]
    pub const fn remote_observed_state(&self) -> Option<NodeState> {
        self.remote_observed_state
    }
    #[must_use]
    pub const fn remote_parent_node_id(&self) -> Option<NodeId> {
        self.remote_parent_node_id
    }
    #[must_use]
    pub const fn remote_epoch(&self) -> Option<Sequence> {
        self.remote_epoch
    }
    #[must_use]
    pub const fn remote_sequence(&self) -> Option<Sequence> {
        self.remote_sequence
    }
    #[must_use]
    pub const fn detected_at_ms(&self) -> u64 {
        self.detected_at_ms
    }
    #[must_use]
    pub const fn status(&self) -> SyncConflictStatus {
        self.status
    }
    #[must_use]
    pub const fn resolution_id(&self) -> Option<ConflictResolutionId> {
        self.resolution_id
    }
    #[must_use]
    pub const fn resolution(&self) -> Option<SyncConflictResolution> {
        self.resolution
    }
    #[must_use]
    pub const fn resolved_at_ms(&self) -> Option<u64> {
        self.resolved_at_ms
    }
    #[must_use]
    pub const fn replacement_intent_id(&self) -> Option<OutboundIntentId> {
        self.replacement_intent_id
    }

    #[must_use]
    pub const fn cursor(&self) -> ConflictCursor {
        ConflictCursor::new(self.detected_at_ms, self.conflict_id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictPage {
    items: Vec<SyncConflictRecord>,
    next_cursor: Option<ConflictCursor>,
}

impl ConflictPage {
    pub(crate) fn new(items: Vec<SyncConflictRecord>, next_cursor: Option<ConflictCursor>) -> Self {
        Self { items, next_cursor }
    }

    #[must_use]
    pub fn items(&self) -> &[SyncConflictRecord] {
        &self.items
    }
    #[must_use]
    pub const fn next_cursor(&self) -> Option<ConflictCursor> {
        self.next_cursor
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ConflictEvidence {
    pub conflict_id: SyncConflictId,
    pub kind: SyncConflictKind,
    pub node_id: Option<NodeId>,
    pub local_base_revision: Option<Revision>,
    pub remote_observed_revision: Option<Revision>,
    pub remote_observed_state: Option<NodeState>,
    pub remote_parent_node_id: Option<NodeId>,
    pub remote_epoch: Option<Sequence>,
    pub remote_sequence: Option<Sequence>,
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, sync::Arc};

    use synveil_core::{
        ClientMutationId, DeviceId, LibraryId, LogicalName, NodeId, NodeKind, NodeState, Revision,
        Sequence, SyncConflictId, UserId,
    };

    use super::{SyncConflictKind, SyncConflictResolution, SyncConflictStatus};
    use crate::{
        LocalFingerprint, LocalNode, LocalStateConfig, LocalStateStore, ManagedRelativePath,
        OutboundIntent, OutboundIntentKind, OutboundIntentState, RemoteMutationConflict,
        ReplicaScope, RootBindingId,
    };

    struct Harness {
        directory: PathBuf,
        store: Arc<LocalStateStore>,
        scope: ReplicaScope,
        root_id: NodeId,
        node_id: NodeId,
    }

    impl Harness {
        async fn new(label: &str) -> Self {
            let directory = std::env::temp_dir()
                .join(format!("synveil-conflict-{label}-{}", uuid::Uuid::now_v7()));
            fs::create_dir(&directory).unwrap();
            let store = Arc::new(
                LocalStateStore::open(&LocalStateConfig::new(directory.join("state.sqlite3")))
                    .await
                    .unwrap(),
            );
            let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
            store
                .bind_replica(scope, RootBindingId::new())
                .await
                .unwrap();
            let root_id = NodeId::new();
            let node_id = NodeId::new();
            store
                .upsert_local_node(&LocalNode::new(
                    scope.library_id(),
                    root_id,
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
                    true,
                    None,
                ))
                .await
                .unwrap();
            store
                .upsert_local_node(&LocalNode::new(
                    scope.library_id(),
                    node_id,
                    Some(root_id),
                    ManagedRelativePath::new("remote.txt").unwrap(),
                    LogicalName::new("remote.txt").unwrap(),
                    NodeKind::File,
                    NodeState::Active,
                    Revision::new(2),
                    None,
                    None,
                    None,
                    None,
                    None,
                    Sequence::new(0),
                    true,
                    None,
                ))
                .await
                .unwrap();
            Self {
                directory,
                store,
                scope,
                root_id,
                node_id,
            }
        }

        fn rename_intent(&self) -> OutboundIntent {
            OutboundIntent::new(
                self.scope.library_id(),
                Some(self.node_id),
                Some(self.root_id),
                OutboundIntentKind::RenameNode,
                ManagedRelativePath::new("local.txt").unwrap(),
                Some(ManagedRelativePath::new("remote.txt").unwrap()),
                Some(LocalFingerprint::file(
                    3,
                    synveil_core::Sha256Digest::from_bytes([7; 32]),
                )),
                Sequence::new(1),
                Sequence::new(1),
                Some(Revision::new(1)),
                None,
                Some(Revision::new(1)),
            )
            .unwrap()
        }

        fn remote_conflict(&self, conflict_id: SyncConflictId) -> RemoteMutationConflict {
            RemoteMutationConflict::with_evidence(
                conflict_id,
                "REVISION_MISMATCH",
                false,
                self.node_id,
                Some(Revision::new(1)),
                Some(Revision::new(2)),
                Some(NodeState::Active),
                Some(self.root_id),
                Sequence::new(1),
                Sequence::new(2),
            )
            .unwrap()
        }

        async fn close(self) {
            self.store.close_pool().await;
            drop(self.store);
            fs::remove_dir_all(self.directory).unwrap();
        }
    }

    #[tokio::test]
    async fn canonical_detection_is_idempotent_bounded_and_restart_durable() {
        let harness = Harness::new("durable").await;
        let intent = harness.rename_intent();
        harness.store.upsert_outbound_intent(&intent).await.unwrap();
        let conflict_id = SyncConflictId::new();
        let mutation_id = ClientMutationId::new();
        let first = harness
            .store
            .record_mutation_conflict(
                intent.intent_id(),
                mutation_id,
                &harness.remote_conflict(conflict_id),
            )
            .await
            .unwrap();
        let repeated = harness
            .store
            .record_mutation_conflict(
                intent.intent_id(),
                mutation_id,
                &harness.remote_conflict(conflict_id),
            )
            .await
            .unwrap();
        assert_eq!(first, repeated);
        assert_eq!(first.kind(), SyncConflictKind::RemoteRevisionChanged);
        assert_eq!(first.local_base_revision(), Some(Revision::new(1)));
        assert_eq!(first.remote_observed_revision(), Some(Revision::new(2)));
        let page = harness
            .store
            .list_unresolved_conflicts(harness.scope.library_id(), None, Some(1))
            .await
            .unwrap();
        assert_eq!(page.items(), std::slice::from_ref(&first));
        assert_eq!(
            harness
                .store
                .outbound_intent(intent.intent_id())
                .await
                .unwrap()
                .unwrap()
                .state(),
            OutboundIntentState::Conflict
        );
        let config = LocalStateConfig::new(harness.directory.join("state.sqlite3"));
        harness.store.close_pool().await;
        drop(harness.store);
        let reopened = Arc::new(LocalStateStore::open(&config).await.unwrap());
        assert_eq!(
            reopened.get_conflict(conflict_id).await.unwrap(),
            Some(first)
        );
        reopened.close_pool().await;
        drop(reopened);
        fs::remove_dir_all(harness.directory).unwrap();
    }

    #[tokio::test]
    async fn accept_remote_is_explicit_atomic_and_idempotent() {
        let harness = Harness::new("accept").await;
        let intent = harness.rename_intent();
        harness.store.upsert_outbound_intent(&intent).await.unwrap();
        let conflict = harness
            .store
            .record_mutation_conflict(
                intent.intent_id(),
                ClientMutationId::new(),
                &harness.remote_conflict(SyncConflictId::new()),
            )
            .await
            .unwrap();
        let resolved = harness
            .store
            .resolve_conflict(conflict.conflict_id(), SyncConflictResolution::AcceptRemote)
            .await
            .unwrap();
        assert_eq!(resolved.status(), SyncConflictStatus::Resolved);
        assert_eq!(
            resolved.resolution(),
            Some(SyncConflictResolution::AcceptRemote)
        );
        assert_eq!(resolved.replacement_intent_id(), None);
        assert_eq!(
            harness
                .store
                .outbound_intent(intent.intent_id())
                .await
                .unwrap()
                .unwrap()
                .state(),
            OutboundIntentState::Cancelled
        );
        assert_eq!(
            harness
                .store
                .resolve_conflict(conflict.conflict_id(), SyncConflictResolution::AcceptRemote)
                .await
                .unwrap(),
            resolved
        );
        harness.close().await;
    }

    #[tokio::test]
    async fn retry_local_creates_exactly_one_new_intent_with_current_base() {
        let harness = Harness::new("retry").await;
        let intent = harness.rename_intent();
        harness.store.upsert_outbound_intent(&intent).await.unwrap();
        let conflict = harness
            .store
            .record_mutation_conflict(
                intent.intent_id(),
                ClientMutationId::new(),
                &harness.remote_conflict(SyncConflictId::new()),
            )
            .await
            .unwrap();
        let resolved = harness
            .store
            .resolve_conflict(
                conflict.conflict_id(),
                SyncConflictResolution::RetryLocalAgainstCurrentBase,
            )
            .await
            .unwrap();
        let replacement_id = resolved.replacement_intent_id().unwrap();
        let replacement = harness
            .store
            .outbound_intent(replacement_id)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(replacement.intent_id(), intent.intent_id());
        assert_eq!(replacement.base_revision(), Some(Revision::new(2)));
        assert_eq!(replacement.state(), OutboundIntentState::Pending);
        assert_eq!(
            harness
                .store
                .outbound_intent(intent.intent_id())
                .await
                .unwrap()
                .unwrap()
                .state(),
            OutboundIntentState::Superseded
        );
        let replayed = harness
            .store
            .resolve_conflict(
                conflict.conflict_id(),
                SyncConflictResolution::RetryLocalAgainstCurrentBase,
            )
            .await
            .unwrap();
        assert_eq!(replayed.replacement_intent_id(), Some(replacement_id));
        harness.close().await;
    }

    #[tokio::test]
    async fn replacement_intent_can_conflict_again_as_an_independent_record() {
        let harness = Harness::new("retry-chain").await;
        let intent = harness.rename_intent();
        harness.store.upsert_outbound_intent(&intent).await.unwrap();
        let first = harness
            .store
            .record_mutation_conflict(
                intent.intent_id(),
                ClientMutationId::new(),
                &harness.remote_conflict(SyncConflictId::new()),
            )
            .await
            .unwrap();
        let resolved = harness
            .store
            .resolve_conflict(
                first.conflict_id(),
                SyncConflictResolution::RetryLocalAgainstCurrentBase,
            )
            .await
            .unwrap();
        let replacement_id = resolved.replacement_intent_id().unwrap();
        let second = harness
            .store
            .record_mutation_conflict(
                replacement_id,
                ClientMutationId::new(),
                &harness.remote_conflict(SyncConflictId::new()),
            )
            .await
            .unwrap();
        assert_ne!(second.conflict_id(), first.conflict_id());
        assert_ne!(second.intent_id(), first.intent_id());
        assert_eq!(second.intent_id(), replacement_id);
        assert_eq!(second.status(), SyncConflictStatus::Unresolved);
        assert_eq!(
            harness
                .store
                .get_conflict(first.conflict_id())
                .await
                .unwrap()
                .unwrap()
                .status(),
            SyncConflictStatus::Resolved
        );
        assert_eq!(
            harness
                .store
                .outbound_intent(replacement_id)
                .await
                .unwrap()
                .unwrap()
                .state(),
            OutboundIntentState::Conflict
        );
        harness.close().await;
    }

    #[tokio::test]
    async fn both_resolutions_survive_close_and_reopen() {
        let harness = Harness::new("resolution-restart").await;
        let accepted_intent = harness.rename_intent();
        harness
            .store
            .upsert_outbound_intent(&accepted_intent)
            .await
            .unwrap();
        let accepted = harness
            .store
            .record_mutation_conflict(
                accepted_intent.intent_id(),
                ClientMutationId::new(),
                &harness.remote_conflict(SyncConflictId::new()),
            )
            .await
            .unwrap();
        let retried_intent = OutboundIntent::new(
            harness.scope.library_id(),
            Some(harness.node_id),
            Some(harness.root_id),
            OutboundIntentKind::RenameNode,
            ManagedRelativePath::new("local-retry.txt").unwrap(),
            Some(ManagedRelativePath::new("remote.txt").unwrap()),
            Some(LocalFingerprint::file(
                3,
                synveil_core::Sha256Digest::from_bytes([7; 32]),
            )),
            Sequence::new(1),
            Sequence::new(1),
            Some(Revision::new(1)),
            None,
            Some(Revision::new(1)),
        )
        .unwrap();
        harness
            .store
            .upsert_outbound_intent(&retried_intent)
            .await
            .unwrap();
        let retried = harness
            .store
            .record_mutation_conflict(
                retried_intent.intent_id(),
                ClientMutationId::new(),
                &harness.remote_conflict(SyncConflictId::new()),
            )
            .await
            .unwrap();
        let accepted_resolved = harness
            .store
            .resolve_conflict(accepted.conflict_id(), SyncConflictResolution::AcceptRemote)
            .await
            .unwrap();
        let retried_resolved = harness
            .store
            .resolve_conflict(
                retried.conflict_id(),
                SyncConflictResolution::RetryLocalAgainstCurrentBase,
            )
            .await
            .unwrap();
        let replacement_id = retried_resolved.replacement_intent_id().unwrap();
        let config = LocalStateConfig::new(harness.directory.join("state.sqlite3"));
        harness.store.close_pool().await;
        drop(harness.store);
        let reopened = Arc::new(LocalStateStore::open(&config).await.unwrap());
        assert_eq!(
            reopened
                .get_conflict(accepted.conflict_id())
                .await
                .unwrap()
                .unwrap(),
            accepted_resolved
        );
        assert_eq!(
            reopened
                .get_conflict(retried.conflict_id())
                .await
                .unwrap()
                .unwrap(),
            retried_resolved
        );
        assert_eq!(
            reopened
                .outbound_intent(accepted_intent.intent_id())
                .await
                .unwrap()
                .unwrap()
                .state(),
            OutboundIntentState::Cancelled
        );
        assert_eq!(
            reopened
                .outbound_intent(retried_intent.intent_id())
                .await
                .unwrap()
                .unwrap()
                .state(),
            OutboundIntentState::Superseded
        );
        assert_eq!(
            reopened
                .outbound_intent(replacement_id)
                .await
                .unwrap()
                .unwrap()
                .state(),
            OutboundIntentState::Pending
        );
        assert!(
            reopened
                .list_unresolved_conflicts(harness.scope.library_id(), None, None)
                .await
                .unwrap()
                .items()
                .is_empty()
        );
        reopened.close_pool().await;
        drop(reopened);
        fs::remove_dir_all(harness.directory).unwrap();
    }

    #[tokio::test]
    async fn content_conflict_and_retry_preserve_the_original_staged_source_reference() {
        let harness = Harness::new("content-source").await;
        let digest = synveil_core::Sha256Digest::from_bytes([9; 32]);
        let intent = OutboundIntent::new(
            harness.scope.library_id(),
            Some(harness.node_id),
            Some(harness.root_id),
            OutboundIntentKind::ModifyFileContent,
            ManagedRelativePath::new("remote.txt").unwrap(),
            None,
            Some(LocalFingerprint::file(4, digest)),
            Sequence::new(1),
            Sequence::new(1),
            Some(Revision::new(1)),
            None,
            Some(Revision::new(1)),
        )
        .unwrap();
        harness.store.upsert_outbound_intent(&intent).await.unwrap();
        let source =
            ManagedRelativePath::new(format!(".synveil/staging/{}.part", intent.intent_id()))
                .unwrap();
        harness
            .store
            .persist_staged_upload(intent.intent_id(), "REPLACE_CONTENT", &source, 4, digest)
            .await
            .unwrap();
        let conflict = harness
            .store
            .record_upload_conflict(intent.intent_id(), Some(Revision::new(2)))
            .await
            .unwrap();
        assert_eq!(conflict.kind(), SyncConflictKind::RemoteContentChanged);
        let resolved = harness
            .store
            .resolve_conflict(
                conflict.conflict_id(),
                SyncConflictResolution::RetryLocalAgainstCurrentBase,
            )
            .await
            .unwrap();
        let original_source = harness
            .store
            .durable_upload_session(intent.intent_id())
            .await
            .unwrap()
            .unwrap();
        let replacement_source = harness
            .store
            .durable_upload_session(resolved.replacement_intent_id().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(original_source.staging_relative_path(), &source);
        assert_eq!(replacement_source.staging_relative_path(), &source);
        assert_eq!(replacement_source.upload_session_id(), None);
        harness.close().await;
    }

    #[tokio::test]
    async fn content_retry_without_a_durable_source_fails_closed() {
        let harness = Harness::new("content-no-source").await;
        let intent = OutboundIntent::new(
            harness.scope.library_id(),
            Some(harness.node_id),
            Some(harness.root_id),
            OutboundIntentKind::ModifyFileContent,
            ManagedRelativePath::new("remote.txt").unwrap(),
            None,
            Some(LocalFingerprint::file(
                4,
                synveil_core::Sha256Digest::from_bytes([5; 32]),
            )),
            Sequence::new(1),
            Sequence::new(1),
            Some(Revision::new(1)),
            None,
            Some(Revision::new(1)),
        )
        .unwrap();
        harness.store.upsert_outbound_intent(&intent).await.unwrap();
        let conflict = harness
            .store
            .record_upload_conflict(intent.intent_id(), Some(Revision::new(2)))
            .await
            .unwrap();
        assert!(matches!(
            harness
                .store
                .resolve_conflict(
                    conflict.conflict_id(),
                    SyncConflictResolution::RetryLocalAgainstCurrentBase,
                )
                .await,
            Err(crate::ClientSyncError::ResolutionNotApplicable)
        ));
        assert_eq!(
            harness
                .store
                .get_conflict(conflict.conflict_id())
                .await
                .unwrap()
                .unwrap()
                .status(),
            SyncConflictStatus::Unresolved
        );
        harness.close().await;
    }

    #[tokio::test]
    async fn concurrent_observers_create_one_active_conflict() {
        let harness = Harness::new("concurrent-detect").await;
        let intent = harness.rename_intent();
        harness.store.upsert_outbound_intent(&intent).await.unwrap();
        let conflict = harness.remote_conflict(SyncConflictId::new());
        let left_store = Arc::clone(&harness.store);
        let right_store = Arc::clone(&harness.store);
        let left_conflict = conflict.clone();
        let right_conflict = conflict.clone();
        let mutation_id = ClientMutationId::new();
        let (left, right) = tokio::join!(
            left_store.record_mutation_conflict(intent.intent_id(), mutation_id, &left_conflict),
            right_store.record_mutation_conflict(intent.intent_id(), mutation_id, &right_conflict)
        );
        assert_eq!(left.unwrap(), right.unwrap());
        let page = harness
            .store
            .list_unresolved_conflicts(harness.scope.library_id(), None, None)
            .await
            .unwrap();
        assert_eq!(page.items().len(), 1);
        harness.close().await;
    }

    #[tokio::test]
    async fn conflicts_are_library_isolated_and_distinct_intents_get_distinct_records() {
        let harness = Harness::new("library-isolation").await;
        let first = harness.rename_intent();
        harness.store.upsert_outbound_intent(&first).await.unwrap();
        let first_conflict = harness
            .store
            .record_mutation_conflict(
                first.intent_id(),
                ClientMutationId::new(),
                &harness.remote_conflict(SyncConflictId::new()),
            )
            .await
            .unwrap();

        let other_scope = ReplicaScope::new(
            harness.scope.owner_user_id(),
            harness.scope.device_id(),
            LibraryId::new(),
        );
        harness
            .store
            .bind_replica(other_scope, RootBindingId::new())
            .await
            .unwrap();
        let other_root = NodeId::new();
        let other_node = NodeId::new();
        for node in [
            LocalNode::new(
                other_scope.library_id(),
                other_root,
                None,
                ManagedRelativePath::root(),
                LogicalName::new("other-root").unwrap(),
                NodeKind::Directory,
                NodeState::Active,
                Revision::new(1),
                None,
                None,
                None,
                None,
                None,
                Sequence::new(0),
                true,
                None,
            ),
            LocalNode::new(
                other_scope.library_id(),
                other_node,
                Some(other_root),
                ManagedRelativePath::new("other.txt").unwrap(),
                LogicalName::new("other.txt").unwrap(),
                NodeKind::File,
                NodeState::Active,
                Revision::new(1),
                None,
                None,
                None,
                None,
                None,
                Sequence::new(0),
                true,
                None,
            ),
        ] {
            harness.store.upsert_local_node(&node).await.unwrap();
        }
        let other_intent = OutboundIntent::new(
            other_scope.library_id(),
            Some(other_node),
            Some(other_root),
            OutboundIntentKind::RenameNode,
            ManagedRelativePath::new("other-local.txt").unwrap(),
            Some(ManagedRelativePath::new("other.txt").unwrap()),
            Some(LocalFingerprint::file(
                2,
                synveil_core::Sha256Digest::from_bytes([3; 32]),
            )),
            Sequence::new(1),
            Sequence::new(1),
            Some(Revision::new(1)),
            None,
            Some(Revision::new(1)),
        )
        .unwrap();
        harness
            .store
            .upsert_outbound_intent(&other_intent)
            .await
            .unwrap();
        assert!(
            harness
                .store
                .list_unresolved_conflicts(other_scope.library_id(), None, None)
                .await
                .unwrap()
                .items()
                .is_empty()
        );
        assert_eq!(
            harness
                .store
                .outbound_intent(other_intent.intent_id())
                .await
                .unwrap(),
            Some(other_intent.clone())
        );

        let second = OutboundIntent::new(
            harness.scope.library_id(),
            Some(harness.node_id),
            Some(harness.root_id),
            OutboundIntentKind::ModifyFileContent,
            ManagedRelativePath::new("remote.txt").unwrap(),
            None,
            Some(LocalFingerprint::file(
                4,
                synveil_core::Sha256Digest::from_bytes([4; 32]),
            )),
            Sequence::new(1),
            Sequence::new(1),
            Some(Revision::new(2)),
            None,
            Some(Revision::new(1)),
        )
        .unwrap();
        harness.store.upsert_outbound_intent(&second).await.unwrap();
        let second_conflict = harness
            .store
            .record_upload_conflict(second.intent_id(), Some(Revision::new(3)))
            .await
            .unwrap();
        assert_ne!(first_conflict.conflict_id(), second_conflict.conflict_id());
        assert_eq!(
            harness
                .store
                .list_unresolved_conflicts(harness.scope.library_id(), None, None)
                .await
                .unwrap()
                .items()
                .len(),
            2
        );
        harness.close().await;
    }

    #[tokio::test]
    async fn resolution_vs_redetection_is_linearizable_for_one_hundred_rounds() {
        let harness = Harness::new("resolution-race").await;
        for round in 0..100_u64 {
            let intent = OutboundIntent::new(
                harness.scope.library_id(),
                Some(harness.node_id),
                Some(harness.root_id),
                OutboundIntentKind::RenameNode,
                ManagedRelativePath::new(format!("local-{round}.txt")).unwrap(),
                Some(ManagedRelativePath::new("remote.txt").unwrap()),
                Some(LocalFingerprint::file(
                    3,
                    synveil_core::Sha256Digest::from_bytes([7; 32]),
                )),
                Sequence::new(1),
                Sequence::new(1),
                Some(Revision::new(1)),
                None,
                Some(Revision::new(1)),
            )
            .unwrap();
            harness.store.upsert_outbound_intent(&intent).await.unwrap();
            let conflict = harness
                .store
                .record_mutation_conflict(
                    intent.intent_id(),
                    ClientMutationId::new(),
                    &harness.remote_conflict(SyncConflictId::new()),
                )
                .await
                .unwrap();
            let resolver = Arc::clone(&harness.store);
            let detector = Arc::clone(&harness.store);
            let repeated = harness.remote_conflict(SyncConflictId::new());
            let (resolution, redetection) = tokio::join!(
                resolver.resolve_conflict(
                    conflict.conflict_id(),
                    SyncConflictResolution::AcceptRemote,
                ),
                detector.record_mutation_conflict(
                    intent.intent_id(),
                    ClientMutationId::new(),
                    &repeated,
                )
            );
            assert_eq!(resolution.unwrap().status(), SyncConflictStatus::Resolved);
            assert!(
                redetection.is_ok()
                    || matches!(
                        redetection,
                        Err(crate::ClientSyncError::ConflictAlreadyResolved)
                    )
            );
            assert_eq!(
                harness
                    .store
                    .outbound_intent(intent.intent_id())
                    .await
                    .unwrap()
                    .unwrap()
                    .state(),
                OutboundIntentState::Cancelled
            );
        }
        assert_eq!(
            harness
                .store
                .list_resolved_conflicts(harness.scope.library_id(), None, Some(1_000))
                .await
                .unwrap()
                .items()
                .len(),
            100
        );
        assert!(
            harness
                .store
                .list_unresolved_conflicts(harness.scope.library_id(), None, Some(1_000))
                .await
                .unwrap()
                .items()
                .is_empty()
        );
        harness.close().await;
    }

    #[tokio::test]
    async fn real_v5_to_v6_upgrade_preserves_handoff_candidate_and_outbound_source() {
        let directory =
            std::env::temp_dir().join(format!("synveil-conflict-v5v6-{}", uuid::Uuid::now_v7()));
        fs::create_dir(&directory).unwrap();
        let database = directory.join("state.sqlite3");
        let migrations = directory.join("migrations-v5");
        fs::create_dir(&migrations).unwrap();
        for name in [
            "0001_initial.sql",
            "0002_server_profiles.sql",
            "0003_outbound_observation.sql",
            "0004_outbound_submission.sql",
            "0005_rebaseline_candidates.sql",
        ] {
            fs::copy(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("migrations")
                    .join(name),
                migrations.join(name),
            )
            .unwrap();
        }
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!("sqlite:{}?mode=rwc", database.display()))
            .await
            .unwrap();
        sqlx::migrate::Migrator::new(migrations)
            .await
            .unwrap()
            .run(&pool)
            .await
            .unwrap();
        let owner = UserId::new();
        let device = DeviceId::new();
        let library = LibraryId::new();
        let intent = uuid::Uuid::now_v7();
        let snapshot = uuid::Uuid::now_v7();
        let now = 1_700_000_000_000_i64;
        sqlx::query(
            "INSERT INTO replicas (
                 library_id, owner_user_id, device_id, root_binding_id,
                 journal_epoch, applied_sequence, acknowledged_sequence, status,
                 created_at_ms, updated_at_ms
             ) VALUES (?, ?, ?, ?, 1, 4, 4, 'IDLE', ?, ?)",
        )
        .bind(library.to_string())
        .bind(owner.to_string())
        .bind(device.to_string())
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO outbound_intents (
                 intent_id, library_id, intent_kind, state, observed_relative_path,
                 observed_kind, observed_length, observed_sha256,
                 base_epoch, base_applied_sequence, dedupe_version, dedupe_sha256,
                 created_at_ms, updated_at_ms
             ) VALUES (?, ?, 'CREATE_FILE', 'PENDING', 'pending.bin',
                       'FILE', 4, ?, 1, 4, 1, ?, ?, ?)",
        )
        .bind(intent.to_string())
        .bind(library.to_string())
        .bind(vec![3_u8; 32])
        .bind(vec![4_u8; 32])
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO outbound_upload_sessions (
                 intent_id, operation, staging_relative_path, expected_length,
                 expected_sha256, state, created_at_ms, updated_at_ms
             ) VALUES (?, 'CREATE_FILE', '.synveil/staging/source.part', 4, ?, 'STAGED', ?, ?)",
        )
        .bind(intent.to_string())
        .bind(vec![3_u8; 32])
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO rebaseline_candidates (
                 library_id, snapshot_id, journal_epoch, resume_sequence,
                 expected_count, received_count, terminal_fetched, state,
                 created_at_ms, updated_at_ms
             ) VALUES (?, ?, 1, 4, 0, 0, 1, 'COMPLETE', ?, ?)",
        )
        .bind(library.to_string())
        .bind(snapshot.to_string())
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO rebaseline_applied_handoffs (
                 library_id, snapshot_id, journal_epoch, resume_sequence, applied_at_ms
             ) VALUES (?, ?, 1, 4, ?)",
        )
        .bind(library.to_string())
        .bind(snapshot.to_string())
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;

        let store = LocalStateStore::open(&LocalStateConfig::new(&database))
            .await
            .unwrap();
        assert_eq!(store.schema_version().await.unwrap(), 6);
        let verify = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!("sqlite:{}?mode=rw", database.display()))
            .await
            .unwrap();
        for table in [
            "outbound_intents",
            "outbound_upload_sessions",
            "rebaseline_candidates",
            "rebaseline_applied_handoffs",
        ] {
            let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&verify)
                .await
                .unwrap();
            assert_eq!(count, 1, "{table} must survive v5 -> v6");
        }
        let conflicts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_conflicts")
            .fetch_one(&verify)
            .await
            .unwrap();
        assert_eq!(conflicts, 0);
        verify.close().await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }
}
