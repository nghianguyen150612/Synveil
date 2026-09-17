//! One-shot retained-cursor and handoff-proof recovery orchestration.
//!
//! This module deliberately contains no retry, recursion, polling, or
//! background work. One call walks a finite durable-state machine:
//!
//! `InspectLocalState -> ResumeCandidate | CompletePendingHandoff |
//! NormalIncremental -> CreateSnapshot -> Apply -> Handoff -> Finalize`.
//!
//! At most one new snapshot POST is possible on any path through that graph.
//! The page loop lives solely in `RebaselineApplier`, whose persisted opaque
//! cursor/terminal marker gives it a finite server-provided sequence.

use std::sync::Arc;

use synveil_core::RebaselineSnapshotId;

use crate::state::{RebaselineCandidateRecord, RebaselineCandidateState};
use crate::{
    ClientSyncError, InboundSyncEngine, LocalStateStore, RebaselineApplier, RebaselineBoundary,
    RebaselineSnapshotDescriptor, RebaselineSnapshotRemote, RemoteErrorKind, ReplicaScope,
    SyncOutcome,
};

/// The finite, transport-neutral outcome of one convergence invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RebaselineConvergenceOutcome {
    /// Ordinary incremental processing was already usable and had no page to
    /// apply. No snapshot was created.
    IncrementalReady,
    /// Ordinary incremental processing made bounded progress, found an
    /// unrelated local blocker, or remains in the legacy bootstrap state.
    /// Callers retain the precise [`SyncOutcome`] without a recovery loop.
    IncrementalProgress(SyncOutcome),
    /// A server-authored snapshot boundary was atomically applied, handed off,
    /// and finalized locally. Inbound processing is now unfenced at exactly
    /// this boundary.
    RebaselineConverged {
        snapshot_id: RebaselineSnapshotId,
        boundary: RebaselineBoundary,
    },
    /// Recovery deliberately stopped without issuing another snapshot POST.
    /// All durable candidate/handoff state remains available to a later call.
    RecoveryBlocked {
        reason: RebaselineRecoveryBlockedReason,
    },
}

/// Explicit non-error stop reasons that must not become automatic retries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RebaselineRecoveryBlockedReason {
    /// The server's active-artifact budget rejected the one allowed create.
    RateLimited,
    /// The newly applied recovery artifact still could not complete the
    /// handoff because its proof disappeared or the checkpoint conflicted.
    /// This invocation will never create S3.
    DidNotConverge,
}

/// Bounded coordinator for retained-history invalidation and unavailable
/// pending-handoff proof recovery.
///
/// `snapshot_remote` must be the same authenticated server/profile transport
/// used to construct `inbound`. The type split keeps page/create transport
/// neutral while `InboundSyncEngine` remains the canonical Prompt 85 handoff
/// and finalization primitive.
pub struct RebaselineConvergenceCoordinator {
    scope: ReplicaScope,
    inbound: Arc<InboundSyncEngine>,
    snapshot_remote: Arc<dyn RebaselineSnapshotRemote>,
    state: Arc<LocalStateStore>,
    page_limit: u32,
}

impl RebaselineConvergenceCoordinator {
    pub fn new(
        inbound: Arc<InboundSyncEngine>,
        snapshot_remote: Arc<dyn RebaselineSnapshotRemote>,
        state: Arc<LocalStateStore>,
        page_limit: u32,
    ) -> Result<Self, ClientSyncError> {
        if !(1..=crate::MAX_PAGE_ITEMS as u32).contains(&page_limit) {
            return Err(ClientSyncError::ResourceLimit);
        }
        if !Arc::ptr_eq(inbound.state(), &state) {
            return Err(ClientSyncError::InvalidState);
        }
        Ok(Self {
            scope: inbound.scope(),
            inbound,
            snapshot_remote,
            state,
            page_limit,
        })
    }

    #[must_use]
    pub const fn scope(&self) -> ReplicaScope {
        self.scope
    }

    pub(crate) fn state(&self) -> &Arc<LocalStateStore> {
        &self.state
    }

    /// Execute one finite recovery state-machine traversal.
    ///
    /// The library-scoped guard is intentionally not a database transaction:
    /// the potentially slow HTTP calls stay outside SQLite write transactions,
    /// while the durable candidate claim prevents two same-library callers
    /// from both issuing a snapshot POST. Different libraries use different
    /// guards and can proceed independently.
    pub async fn run_convergence_once(
        &self,
    ) -> Result<RebaselineConvergenceOutcome, ClientSyncError> {
        let _library_guard = self
            .state
            .lock_rebaseline_convergence(self.scope.library_id())
            .await;

        // Phase: InspectLocalState / ResumeCandidate. A real candidate always
        // wins over an old handoff, which is essential after PR1–PR3 restarts.
        if let Some(candidate) = self
            .state
            .rebaseline_candidate(self.scope.library_id())
            .await?
        {
            return match candidate.state {
                RebaselineCandidateState::Fetching | RebaselineCandidateState::Complete => {
                    self.resume_candidate(candidate).await
                }
                // This is only the descriptor-less POST claim. It has no
                // candidate pages or authority; a previous process may have
                // lost the create response, so a later invocation is allowed
                // one replacement POST after atomically reclaiming it.
                RebaselineCandidateState::CreateClaim => self.create_apply_and_handoff().await,
            };
        }

        // Phase: CompletePendingHandoff. A valid H1 is always tried before a
        // replacement artifact can be created.
        if let Some(pending) = self
            .state
            .pending_rebaseline_handoff(self.scope.library_id())
            .await?
        {
            return match self
                .complete_handoff(
                    pending.snapshot_id(),
                    pending.journal_epoch(),
                    pending.resume_sequence(),
                )
                .await
            {
                Ok(outcome) => Ok(outcome),
                Err(error) if is_recovery_handoff_trigger(&error) => {
                    // H1 deliberately remains in SQLite while S2 downloads.
                    self.create_apply_and_handoff().await
                }
                Err(error) => Err(error),
            };
        }

        // Phase: NormalIncremental. The special engine entry point performs
        // no legacy bootstrap mutation when the server says retained history
        // or epoch authority is unavailable.
        match self.inbound.synchronize_incremental_once().await? {
            SyncOutcome::RebaselineRequired => self.create_apply_and_handoff().await,
            SyncOutcome::Idle => Ok(RebaselineConvergenceOutcome::IncrementalReady),
            outcome => Ok(RebaselineConvergenceOutcome::IncrementalProgress(outcome)),
        }
    }

    async fn resume_candidate(
        &self,
        candidate: RebaselineCandidateRecord,
    ) -> Result<RebaselineConvergenceOutcome, ClientSyncError> {
        let descriptor = descriptor_from_candidate(self.scope, candidate);
        self.apply_and_handoff(descriptor).await
    }

    /// Phase: CreateSnapshot. This is the only function that can issue the
    /// non-idempotent POST, and it invokes it exactly once.
    async fn create_apply_and_handoff(
        &self,
    ) -> Result<RebaselineConvergenceOutcome, ClientSyncError> {
        let claim_id = self
            .state
            .claim_rebaseline_snapshot_creation(self.scope)
            .await?;
        let descriptor = match self.snapshot_remote.create_snapshot(self.scope).await {
            Ok(descriptor) => descriptor,
            Err(error) => {
                self.state
                    .release_rebaseline_snapshot_creation_claim(self.scope.library_id(), claim_id)
                    .await?;
                if error.kind() == RemoteErrorKind::RateLimited {
                    return Ok(RebaselineConvergenceOutcome::RecoveryBlocked {
                        reason: RebaselineRecoveryBlockedReason::RateLimited,
                    });
                }
                return Err(error.into());
            }
        };
        if descriptor.library_id() != self.scope.library_id()
            || descriptor.boundary().journal_epoch().get() == 0
        {
            self.state
                .release_rebaseline_snapshot_creation_claim(self.scope.library_id(), claim_id)
                .await?;
            return Err(ClientSyncError::InvalidRemoteResponse);
        }
        self.state
            .promote_rebaseline_snapshot_creation_claim(self.scope, claim_id, descriptor)
            .await?;
        self.apply_and_handoff(descriptor).await
    }

    /// Phases: ApplyRecoverySnapshot -> CompleteRecoveryHandoff -> Finalize.
    /// The applier delegates page persistence and the single atomic base/H1 to
    /// H2 replacement transaction to Prompt 84's canonical local primitive.
    async fn apply_and_handoff(
        &self,
        descriptor: RebaselineSnapshotDescriptor,
    ) -> Result<RebaselineConvergenceOutcome, ClientSyncError> {
        let applier = RebaselineApplier::new(self.scope, Arc::clone(&self.state), self.page_limit)?;
        let _ = applier
            .apply(descriptor, self.snapshot_remote.as_ref())
            .await?;
        match self
            .complete_handoff(
                descriptor.snapshot_id(),
                descriptor.boundary().journal_epoch(),
                descriptor.boundary().resume_sequence(),
            )
            .await
        {
            Ok(outcome) => Ok(outcome),
            Err(error) if is_recovery_handoff_trigger(&error) => {
                // The one allowed new snapshot was already created or resumed;
                // ending here is the explicit no-S3/no-loop guarantee.
                Ok(RebaselineConvergenceOutcome::RecoveryBlocked {
                    reason: RebaselineRecoveryBlockedReason::DidNotConverge,
                })
            }
            Err(error) => Err(error),
        }
    }

    async fn complete_handoff(
        &self,
        snapshot_id: RebaselineSnapshotId,
        journal_epoch: synveil_core::Sequence,
        resume_sequence: synveil_core::Sequence,
    ) -> Result<RebaselineConvergenceOutcome, ClientSyncError> {
        self.inbound
            .complete_rebaseline_handoff(snapshot_id)
            .await?;
        Ok(RebaselineConvergenceOutcome::RebaselineConverged {
            snapshot_id,
            boundary: RebaselineBoundary::new(journal_epoch, resume_sequence),
        })
    }
}

fn descriptor_from_candidate(
    scope: ReplicaScope,
    candidate: RebaselineCandidateRecord,
) -> RebaselineSnapshotDescriptor {
    RebaselineSnapshotDescriptor::new(
        candidate.snapshot_id,
        scope.library_id(),
        RebaselineBoundary::new(candidate.journal_epoch, candidate.resume_sequence),
        candidate.expected_count,
    )
}

/// Only Prompt 85's authenticated proof-unavailable and checkpoint-conflict
/// classifications authorize replacement recovery. Authentication, revocation,
/// rate, protocol, local-database, and transport failures remain failures.
fn is_recovery_handoff_trigger(error: &ClientSyncError) -> bool {
    matches!(
        error,
        ClientSyncError::HandoffSnapshotUnavailable | ClientSyncError::HandoffCheckpointConflict
    )
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        fs,
        path::PathBuf,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use async_trait::async_trait;
    use synveil_core::{
        DeviceId, LibraryId, LogicalName, LogicalSnapshotNode, NodeId, NodeKind, NodeState,
        RebaselineSnapshotId, Revision, Sequence, UserId,
    };

    use super::*;
    use crate::{
        EngineConfig, FilesystemLocalReplica, InboundSyncEngine, LocalFingerprint, LocalNode,
        LocalStateConfig, ManagedRelativePath, OutboundIntent, OutboundIntentKind,
        RebaselineHandoffConfirmation, RebaselineSnapshotPage, RebaselineSnapshotSource,
        RemoteCheckpoint, RemoteError, RemoteFeedPage, SyncRemote,
    };

    struct ScriptedRemote {
        checkpoints: Mutex<VecDeque<Result<RemoteCheckpoint, RemoteError>>>,
        feeds: Mutex<VecDeque<Result<RemoteFeedPage, RemoteError>>>,
        handoffs: Mutex<VecDeque<Result<RebaselineHandoffConfirmation, RemoteError>>>,
        creates: Mutex<VecDeque<Result<RebaselineSnapshotDescriptor, RemoteError>>>,
        pages: Mutex<VecDeque<Result<RebaselineSnapshotPage, RemoteError>>>,
        create_calls: AtomicUsize,
    }

    impl ScriptedRemote {
        fn new(
            checkpoints: impl IntoIterator<Item = Result<RemoteCheckpoint, RemoteError>>,
            feeds: impl IntoIterator<Item = Result<RemoteFeedPage, RemoteError>>,
            handoffs: impl IntoIterator<Item = Result<RebaselineHandoffConfirmation, RemoteError>>,
            creates: impl IntoIterator<Item = Result<RebaselineSnapshotDescriptor, RemoteError>>,
            pages: impl IntoIterator<Item = Result<RebaselineSnapshotPage, RemoteError>>,
        ) -> Self {
            Self {
                checkpoints: Mutex::new(checkpoints.into_iter().collect()),
                feeds: Mutex::new(feeds.into_iter().collect()),
                handoffs: Mutex::new(handoffs.into_iter().collect()),
                creates: Mutex::new(creates.into_iter().collect()),
                pages: Mutex::new(pages.into_iter().collect()),
                create_calls: AtomicUsize::new(0),
            }
        }

        fn create_calls(&self) -> usize {
            self.create_calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl SyncRemote for ScriptedRemote {
        async fn get_checkpoint(
            &self,
            _scope: ReplicaScope,
        ) -> Result<RemoteCheckpoint, RemoteError> {
            self.checkpoints
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(RemoteError::new(RemoteErrorKind::Protocol)))
        }

        async fn fetch_changes(
            &self,
            _scope: ReplicaScope,
            _limit: u32,
        ) -> Result<RemoteFeedPage, RemoteError> {
            self.feeds
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(RemoteError::new(RemoteErrorKind::Protocol)))
        }

        async fn acknowledge_changes(
            &self,
            _scope: ReplicaScope,
            _evidence: &crate::OpaqueEvidence,
        ) -> Result<RemoteCheckpoint, RemoteError> {
            Err(RemoteError::new(RemoteErrorKind::Protocol))
        }

        async fn complete_rebaseline_handoff(
            &self,
            _scope: ReplicaScope,
            _snapshot_id: RebaselineSnapshotId,
        ) -> Result<RebaselineHandoffConfirmation, RemoteError> {
            self.handoffs
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(RemoteError::new(RemoteErrorKind::Protocol)))
        }

        async fn start_rebaseline(
            &self,
            _scope: ReplicaScope,
        ) -> Result<synveil_core::SyncBootstrap, RemoteError> {
            Err(RemoteError::new(RemoteErrorKind::Protocol))
        }

        async fn fetch_rebaseline_page(
            &self,
            _scope: ReplicaScope,
            _bootstrap_id: synveil_core::SyncBootstrapId,
            _cursor: Option<&crate::OpaqueEvidence>,
            _limit: u32,
        ) -> Result<crate::BootstrapPage, RemoteError> {
            Err(RemoteError::new(RemoteErrorKind::Protocol))
        }

        async fn complete_rebaseline(
            &self,
            _scope: ReplicaScope,
            _bootstrap_id: synveil_core::SyncBootstrapId,
            _evidence: &crate::OpaqueEvidence,
        ) -> Result<crate::BootstrapCompletion, RemoteError> {
            Err(RemoteError::new(RemoteErrorKind::Protocol))
        }

        async fn download_current_content(
            &self,
            _scope: ReplicaScope,
            _node_id: NodeId,
            _version_id: synveil_core::FileVersionId,
        ) -> Result<crate::RemoteContent, RemoteError> {
            Err(RemoteError::new(RemoteErrorKind::Protocol))
        }
    }

    #[async_trait]
    impl RebaselineSnapshotSource for ScriptedRemote {
        async fn read_page(
            &self,
            _scope: ReplicaScope,
            _snapshot_id: RebaselineSnapshotId,
            _cursor: Option<&crate::OpaqueEvidence>,
            _limit: u32,
        ) -> Result<RebaselineSnapshotPage, RemoteError> {
            self.pages
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(RemoteError::new(RemoteErrorKind::Protocol)))
        }
    }

    #[async_trait]
    impl RebaselineSnapshotRemote for ScriptedRemote {
        async fn create_snapshot(
            &self,
            _scope: ReplicaScope,
        ) -> Result<RebaselineSnapshotDescriptor, RemoteError> {
            self.create_calls.fetch_add(1, Ordering::SeqCst);
            self.creates
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(RemoteError::new(RemoteErrorKind::Protocol)))
        }
    }

    struct Harness {
        state: Arc<LocalStateStore>,
        scope: ReplicaScope,
        coordinator: Arc<RebaselineConvergenceCoordinator>,
        remote: Arc<ScriptedRemote>,
        directory: PathBuf,
    }

    fn new_scope() -> ReplicaScope {
        ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new())
    }

    async fn harness(scope: ReplicaScope, remote: Arc<ScriptedRemote>) -> Harness {
        let directory = std::env::temp_dir().join(format!(
            "synveil-rebaseline-convergence-{}",
            uuid::Uuid::now_v7()
        ));
        fs::create_dir(&directory).unwrap();
        let root_directory = directory.join("root");
        fs::create_dir(&root_directory).unwrap();
        let state = Arc::new(
            LocalStateStore::open(&LocalStateConfig::new(directory.join("state.sqlite3")))
                .await
                .unwrap(),
        );
        let replica = Arc::new(FilesystemLocalReplica::initialize(&root_directory, scope).unwrap());
        let inbound = Arc::new(
            InboundSyncEngine::new(
                scope,
                remote.clone(),
                replica,
                state.clone(),
                EngineConfig::default(),
            )
            .await
            .unwrap(),
        );
        let coordinator = Arc::new(
            RebaselineConvergenceCoordinator::new(inbound, remote.clone(), state.clone(), 32)
                .unwrap(),
        );
        Harness {
            state,
            scope,
            coordinator,
            remote,
            directory,
        }
    }

    async fn close(harness: Harness) {
        let directory = harness.directory.clone();
        harness.state.close_pool().await;
        drop(harness);
        fs::remove_dir_all(directory).unwrap();
    }

    fn root_node(root: NodeId, revision: u64) -> LogicalSnapshotNode {
        LogicalSnapshotNode::new(
            root,
            None,
            LogicalName::new("root").unwrap(),
            NodeKind::Directory,
            NodeState::Active,
            Revision::new(revision),
            None,
            None,
            None,
        )
        .unwrap()
    }

    fn descriptor(
        scope: ReplicaScope,
        snapshot_id: RebaselineSnapshotId,
        epoch: u64,
        sequence: u64,
    ) -> RebaselineSnapshotDescriptor {
        RebaselineSnapshotDescriptor::new(
            snapshot_id,
            scope.library_id(),
            RebaselineBoundary::new(Sequence::new(epoch), Sequence::new(sequence)),
            1,
        )
    }

    fn page(descriptor: RebaselineSnapshotDescriptor, root: NodeId) -> RebaselineSnapshotPage {
        RebaselineSnapshotPage::new(descriptor, vec![root_node(root, 1)], None).unwrap()
    }

    fn handoff(
        scope: ReplicaScope,
        descriptor: RebaselineSnapshotDescriptor,
    ) -> RebaselineHandoffConfirmation {
        RebaselineHandoffConfirmation::new(
            descriptor.snapshot_id(),
            descriptor.library_id(),
            RemoteCheckpoint::new(
                scope,
                descriptor.boundary().journal_epoch(),
                descriptor.boundary().resume_sequence(),
            ),
        )
    }

    fn empty_feed(scope: ReplicaScope) -> RemoteFeedPage {
        RemoteFeedPage::new(
            scope,
            Sequence::new(1),
            Sequence::new(0),
            Sequence::new(0),
            Sequence::new(0),
            false,
            vec![],
            None,
        )
        .unwrap()
    }

    async fn install_incremental_root_for(
        state: &LocalStateStore,
        scope: ReplicaScope,
        root: NodeId,
    ) {
        state
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
        sqlx::query(
            "UPDATE replicas
             SET root_node_id = ?, journal_epoch = 1, applied_sequence = 0,
                 acknowledged_sequence = 0
             WHERE library_id = ?",
        )
        .bind(root.to_string())
        .bind(scope.library_id().to_string())
        .execute(&state.pool)
        .await
        .unwrap();
    }

    async fn install_incremental_root(harness: &Harness, root: NodeId) {
        install_incremental_root_for(harness.state.as_ref(), harness.scope, root).await;
    }

    async fn coordinator_sharing_state(
        harness: &Harness,
        scope: ReplicaScope,
        remote: Arc<ScriptedRemote>,
        label: &str,
    ) -> Arc<RebaselineConvergenceCoordinator> {
        let root_directory = harness.directory.join(label);
        fs::create_dir(&root_directory).unwrap();
        let replica = Arc::new(FilesystemLocalReplica::initialize(&root_directory, scope).unwrap());
        let inbound = Arc::new(
            InboundSyncEngine::new(
                scope,
                remote.clone(),
                replica,
                harness.state.clone(),
                EngineConfig::default(),
            )
            .await
            .unwrap(),
        );
        Arc::new(
            RebaselineConvergenceCoordinator::new(inbound, remote, harness.state.clone(), 32)
                .unwrap(),
        )
    }

    async fn stage_pending(
        harness: &Harness,
        descriptor: RebaselineSnapshotDescriptor,
        root: NodeId,
    ) {
        harness
            .state
            .begin_rebaseline_candidate(harness.scope, descriptor)
            .await
            .unwrap();
        harness
            .state
            .persist_rebaseline_page(harness.scope, descriptor, &page(descriptor, root))
            .await
            .unwrap();
        harness
            .state
            .activate_rebaseline_candidate(harness.scope, descriptor)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn healthy_incremental_sync_creates_no_snapshot() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new(
            [Ok(RemoteCheckpoint::new(
                scope,
                Sequence::new(1),
                Sequence::new(0),
            ))],
            [Ok(empty_feed(scope))],
            [],
            [],
            [],
        ));
        let harness = harness(scope, remote).await;
        install_incremental_root(&harness, NodeId::new()).await;
        assert_eq!(
            harness.coordinator.run_convergence_once().await.unwrap(),
            RebaselineConvergenceOutcome::IncrementalReady
        );
        assert_eq!(harness.remote.create_calls(), 0);
        close(harness).await;
    }

    #[tokio::test]
    async fn retained_history_trigger_creates_one_snapshot_and_converges() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope, remote).await;
        install_incremental_root(&harness, NodeId::new()).await;
        let snapshot = RebaselineSnapshotId::new();
        let created = descriptor(harness.scope, snapshot, 2, 9);
        *harness.remote.checkpoints.lock().unwrap() =
            VecDeque::from([Err(RemoteError::new(RemoteErrorKind::RebaselineRequired))]);
        *harness.remote.creates.lock().unwrap() = VecDeque::from([Ok(created)]);
        *harness.remote.pages.lock().unwrap() = VecDeque::from([Ok(page(created, NodeId::new()))]);
        *harness.remote.handoffs.lock().unwrap() =
            VecDeque::from([Ok(handoff(harness.scope, created))]);

        assert_eq!(
            harness.coordinator.run_convergence_once().await.unwrap(),
            RebaselineConvergenceOutcome::RebaselineConverged {
                snapshot_id: snapshot,
                boundary: created.boundary(),
            }
        );
        assert_eq!(harness.remote.create_calls(), 1);
        assert!(
            !harness
                .state
                .rebaseline_handoff_pending(harness.scope.library_id())
                .await
                .unwrap()
        );
        let replica = harness
            .state
            .replica(harness.scope.library_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(replica.journal_epoch(), Sequence::new(2));
        assert_eq!(replica.acknowledged_sequence(), Sequence::new(9));
        close(harness).await;
    }

    #[tokio::test]
    async fn valid_pending_handoff_finalizes_without_creating_snapshot() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope, remote).await;
        let snapshot = RebaselineSnapshotId::new();
        let pending = descriptor(harness.scope, snapshot, 2, 7);
        stage_pending(&harness, pending, NodeId::new()).await;
        *harness.remote.handoffs.lock().unwrap() =
            VecDeque::from([Ok(handoff(harness.scope, pending))]);

        assert_eq!(
            harness.coordinator.run_convergence_once().await.unwrap(),
            RebaselineConvergenceOutcome::RebaselineConverged {
                snapshot_id: snapshot,
                boundary: pending.boundary(),
            }
        );
        assert_eq!(harness.remote.create_calls(), 0);
        close(harness).await;
    }

    #[tokio::test]
    async fn existing_completed_candidate_resumes_before_new_snapshot_creation() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope, remote).await;
        let snapshot = RebaselineSnapshotId::new();
        let candidate = descriptor(harness.scope, snapshot, 2, 7);
        harness
            .state
            .begin_rebaseline_candidate(harness.scope, candidate)
            .await
            .unwrap();
        harness
            .state
            .persist_rebaseline_page(harness.scope, candidate, &page(candidate, NodeId::new()))
            .await
            .unwrap();
        *harness.remote.handoffs.lock().unwrap() =
            VecDeque::from([Ok(handoff(harness.scope, candidate))]);

        assert_eq!(
            harness.coordinator.run_convergence_once().await.unwrap(),
            RebaselineConvergenceOutcome::RebaselineConverged {
                snapshot_id: snapshot,
                boundary: candidate.boundary(),
            }
        );
        assert_eq!(harness.remote.create_calls(), 0);
        close(harness).await;
    }

    #[tokio::test]
    async fn missing_handoff_proof_keeps_h1_until_atomic_h2_replacement_then_converges() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope, remote).await;
        let old = descriptor(harness.scope, RebaselineSnapshotId::new(), 2, 7);
        stage_pending(&harness, old, NodeId::new()).await;
        let replacement = descriptor(harness.scope, RebaselineSnapshotId::new(), 3, 12);
        *harness.remote.handoffs.lock().unwrap() = VecDeque::from([
            Err(RemoteError::new(RemoteErrorKind::NotFound)),
            Ok(handoff(harness.scope, replacement)),
        ]);
        *harness.remote.creates.lock().unwrap() = VecDeque::from([Ok(replacement)]);
        *harness.remote.pages.lock().unwrap() =
            VecDeque::from([Ok(page(replacement, NodeId::new()))]);

        assert_eq!(
            harness.coordinator.run_convergence_once().await.unwrap(),
            RebaselineConvergenceOutcome::RebaselineConverged {
                snapshot_id: replacement.snapshot_id(),
                boundary: replacement.boundary(),
            }
        );
        assert_eq!(harness.remote.create_calls(), 1);
        assert!(
            !harness
                .state
                .rebaseline_handoff_pending(harness.scope.library_id())
                .await
                .unwrap()
        );
        close(harness).await;
    }

    #[tokio::test]
    async fn checkpoint_ahead_handoff_conflict_creates_one_current_replacement_and_converges() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope, remote).await;
        let old = descriptor(harness.scope, RebaselineSnapshotId::new(), 2, 7);
        stage_pending(&harness, old, NodeId::new()).await;
        let replacement = descriptor(harness.scope, RebaselineSnapshotId::new(), 3, 12);
        *harness.remote.handoffs.lock().unwrap() = VecDeque::from([
            Err(RemoteError::new(RemoteErrorKind::CheckpointConflict)),
            Ok(handoff(harness.scope, replacement)),
        ]);
        *harness.remote.creates.lock().unwrap() = VecDeque::from([Ok(replacement)]);
        *harness.remote.pages.lock().unwrap() =
            VecDeque::from([Ok(page(replacement, NodeId::new()))]);

        assert_eq!(
            harness.coordinator.run_convergence_once().await.unwrap(),
            RebaselineConvergenceOutcome::RebaselineConverged {
                snapshot_id: replacement.snapshot_id(),
                boundary: replacement.boundary(),
            }
        );
        assert_eq!(harness.remote.create_calls(), 1);
        close(harness).await;
    }

    #[tokio::test]
    async fn a_second_handoff_conflict_stops_without_s3() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope, remote).await;
        let old = descriptor(harness.scope, RebaselineSnapshotId::new(), 2, 7);
        stage_pending(&harness, old, NodeId::new()).await;
        let replacement = descriptor(harness.scope, RebaselineSnapshotId::new(), 3, 12);
        *harness.remote.handoffs.lock().unwrap() = VecDeque::from([
            Err(RemoteError::new(RemoteErrorKind::CheckpointConflict)),
            Err(RemoteError::new(RemoteErrorKind::CheckpointConflict)),
        ]);
        *harness.remote.creates.lock().unwrap() = VecDeque::from([Ok(replacement)]);
        *harness.remote.pages.lock().unwrap() =
            VecDeque::from([Ok(page(replacement, NodeId::new()))]);

        assert_eq!(
            harness.coordinator.run_convergence_once().await.unwrap(),
            RebaselineConvergenceOutcome::RecoveryBlocked {
                reason: RebaselineRecoveryBlockedReason::DidNotConverge,
            }
        );
        assert_eq!(harness.remote.create_calls(), 1);
        let pending = harness
            .state
            .pending_rebaseline_handoff(harness.scope.library_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(pending.snapshot_id(), replacement.snapshot_id());
        close(harness).await;
    }

    #[tokio::test]
    async fn authentication_failure_during_pending_handoff_never_creates_replacement() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope, remote).await;
        let pending = descriptor(harness.scope, RebaselineSnapshotId::new(), 2, 7);
        stage_pending(&harness, pending, NodeId::new()).await;
        *harness.remote.handoffs.lock().unwrap() =
            VecDeque::from([Err(RemoteError::new(RemoteErrorKind::AuthRequired))]);

        assert!(matches!(
            harness.coordinator.run_convergence_once().await,
            Err(ClientSyncError::Remote(error)) if error.kind() == RemoteErrorKind::AuthRequired
        ));
        assert_eq!(harness.remote.create_calls(), 0);
        assert!(
            harness
                .state
                .rebaseline_handoff_pending(harness.scope.library_id())
                .await
                .unwrap()
        );
        close(harness).await;
    }

    #[tokio::test]
    async fn revoked_device_during_pending_handoff_never_creates_replacement() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope, remote).await;
        let pending = descriptor(harness.scope, RebaselineSnapshotId::new(), 2, 7);
        stage_pending(&harness, pending, NodeId::new()).await;
        *harness.remote.handoffs.lock().unwrap() =
            VecDeque::from([Err(RemoteError::new(RemoteErrorKind::DeviceRevoked))]);

        assert!(matches!(
            harness.coordinator.run_convergence_once().await,
            Err(ClientSyncError::Remote(error)) if error.kind() == RemoteErrorKind::DeviceRevoked
        ));
        assert_eq!(harness.remote.create_calls(), 0);
        assert!(
            harness
                .state
                .rebaseline_handoff_pending(harness.scope.library_id())
                .await
                .unwrap()
        );
        close(harness).await;
    }

    #[tokio::test]
    async fn create_rate_limit_preserves_state_and_does_not_retry() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope, remote).await;
        install_incremental_root(&harness, NodeId::new()).await;
        *harness.remote.checkpoints.lock().unwrap() =
            VecDeque::from([Err(RemoteError::new(RemoteErrorKind::RebaselineRequired))]);
        *harness.remote.creates.lock().unwrap() =
            VecDeque::from([Err(RemoteError::new(RemoteErrorKind::RateLimited))]);

        assert_eq!(
            harness.coordinator.run_convergence_once().await.unwrap(),
            RebaselineConvergenceOutcome::RecoveryBlocked {
                reason: RebaselineRecoveryBlockedReason::RateLimited,
            }
        );
        assert_eq!(harness.remote.create_calls(), 1);
        assert!(
            harness
                .state
                .rebaseline_candidate(harness.scope.library_id())
                .await
                .unwrap()
                .is_none()
        );
        close(harness).await;
    }

    #[tokio::test]
    async fn snapshot_page_transport_failure_leaves_resumable_candidate_without_second_post() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope, remote).await;
        install_incremental_root(&harness, NodeId::new()).await;
        let snapshot = RebaselineSnapshotId::new();
        let created = descriptor(harness.scope, snapshot, 2, 9);
        *harness.remote.checkpoints.lock().unwrap() =
            VecDeque::from([Err(RemoteError::new(RemoteErrorKind::RebaselineRequired))]);
        *harness.remote.creates.lock().unwrap() = VecDeque::from([Ok(created)]);
        *harness.remote.pages.lock().unwrap() =
            VecDeque::from([Err(RemoteError::new(RemoteErrorKind::Offline))]);

        assert!(matches!(
            harness.coordinator.run_convergence_once().await,
            Err(ClientSyncError::Remote(error)) if error.kind() == RemoteErrorKind::Offline
        ));
        assert_eq!(harness.remote.create_calls(), 1);
        let candidate = harness
            .state
            .rebaseline_candidate(harness.scope.library_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(candidate.snapshot_id, snapshot);
        assert_eq!(candidate.state, RebaselineCandidateState::Fetching);

        *harness.remote.pages.lock().unwrap() = VecDeque::from([Ok(page(created, NodeId::new()))]);
        *harness.remote.handoffs.lock().unwrap() =
            VecDeque::from([Ok(handoff(harness.scope, created))]);
        assert_eq!(
            harness.coordinator.run_convergence_once().await.unwrap(),
            RebaselineConvergenceOutcome::RebaselineConverged {
                snapshot_id: snapshot,
                boundary: created.boundary(),
            }
        );
        assert_eq!(harness.remote.create_calls(), 1);
        close(harness).await;
    }

    #[tokio::test]
    async fn epoch_mismatch_creates_one_current_epoch_snapshot_and_converges() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope, remote).await;
        install_incremental_root(&harness, NodeId::new()).await;
        let snapshot = RebaselineSnapshotId::new();
        let created = descriptor(harness.scope, snapshot, 2, 11);
        *harness.remote.checkpoints.lock().unwrap() = VecDeque::from([Ok(RemoteCheckpoint::new(
            harness.scope,
            Sequence::new(2),
            Sequence::new(0),
        ))]);
        *harness.remote.creates.lock().unwrap() = VecDeque::from([Ok(created)]);
        *harness.remote.pages.lock().unwrap() = VecDeque::from([Ok(page(created, NodeId::new()))]);
        *harness.remote.handoffs.lock().unwrap() =
            VecDeque::from([Ok(handoff(harness.scope, created))]);

        assert_eq!(
            harness.coordinator.run_convergence_once().await.unwrap(),
            RebaselineConvergenceOutcome::RebaselineConverged {
                snapshot_id: snapshot,
                boundary: created.boundary(),
            }
        );
        assert_eq!(harness.remote.create_calls(), 1);
        close(harness).await;
    }

    #[tokio::test]
    async fn no_cursor_rebaseline_signal_creates_a_snapshot_without_starting_bootstrap() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope, remote).await;
        let snapshot = RebaselineSnapshotId::new();
        let created = descriptor(harness.scope, snapshot, 2, 11);
        *harness.remote.checkpoints.lock().unwrap() =
            VecDeque::from([Err(RemoteError::new(RemoteErrorKind::RebaselineRequired))]);
        *harness.remote.creates.lock().unwrap() = VecDeque::from([Ok(created)]);
        *harness.remote.pages.lock().unwrap() = VecDeque::from([Ok(page(created, NodeId::new()))]);
        *harness.remote.handoffs.lock().unwrap() =
            VecDeque::from([Ok(handoff(harness.scope, created))]);

        assert!(matches!(
            harness.coordinator.run_convergence_once().await.unwrap(),
            RebaselineConvergenceOutcome::RebaselineConverged { snapshot_id, .. }
                if snapshot_id == snapshot
        ));
        assert!(
            harness
                .state
                .bootstrap(harness.scope.library_id())
                .await
                .unwrap()
                .is_none(),
            "the authoritative rebaseline signal must not create legacy bootstrap state"
        );
        assert_eq!(harness.remote.create_calls(), 1);
        close(harness).await;
    }

    #[tokio::test]
    async fn lost_snapshot_create_response_releases_only_the_inert_claim_and_never_retries() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope, remote).await;
        install_incremental_root(&harness, NodeId::new()).await;
        *harness.remote.checkpoints.lock().unwrap() =
            VecDeque::from([Err(RemoteError::new(RemoteErrorKind::RebaselineRequired))]);
        *harness.remote.creates.lock().unwrap() =
            VecDeque::from([Err(RemoteError::new(RemoteErrorKind::Offline))]);

        assert!(matches!(
            harness.coordinator.run_convergence_once().await,
            Err(ClientSyncError::Remote(error)) if error.kind() == RemoteErrorKind::Offline
        ));
        assert_eq!(harness.remote.create_calls(), 1);
        assert!(
            harness
                .state
                .rebaseline_candidate(harness.scope.library_id())
                .await
                .unwrap()
                .is_none(),
            "an unknown server snapshot id must never become a local candidate"
        );
        close(harness).await;
    }

    #[tokio::test]
    async fn transient_handoff_failure_keeps_h1_and_does_not_create_a_replacement() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope, remote).await;
        let pending = descriptor(harness.scope, RebaselineSnapshotId::new(), 2, 7);
        stage_pending(&harness, pending, NodeId::new()).await;
        *harness.remote.handoffs.lock().unwrap() =
            VecDeque::from([Err(RemoteError::new(RemoteErrorKind::Internal))]);

        assert!(matches!(
            harness.coordinator.run_convergence_once().await,
            Err(ClientSyncError::HandoffTransport)
        ));
        assert_eq!(harness.remote.create_calls(), 0);
        let still_pending = harness
            .state
            .pending_rebaseline_handoff(harness.scope.library_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(still_pending.snapshot_id(), pending.snapshot_id());
        close(harness).await;
    }

    #[tokio::test]
    async fn malformed_creation_descriptor_is_rejected_without_a_candidate_or_second_post() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope, remote).await;
        install_incremental_root(&harness, NodeId::new()).await;
        let foreign_scope = ReplicaScope::new(
            harness.scope.owner_user_id(),
            harness.scope.device_id(),
            LibraryId::new(),
        );
        *harness.remote.checkpoints.lock().unwrap() =
            VecDeque::from([Err(RemoteError::new(RemoteErrorKind::RebaselineRequired))]);
        *harness.remote.creates.lock().unwrap() = VecDeque::from([Ok(descriptor(
            foreign_scope,
            RebaselineSnapshotId::new(),
            2,
            9,
        ))]);

        assert!(matches!(
            harness.coordinator.run_convergence_once().await,
            Err(ClientSyncError::InvalidRemoteResponse)
        ));
        assert_eq!(harness.remote.create_calls(), 1);
        assert!(
            harness
                .state
                .rebaseline_candidate(harness.scope.library_id())
                .await
                .unwrap()
                .is_none()
        );
        close(harness).await;
    }

    #[tokio::test]
    async fn corrupt_candidate_fails_closed_without_creating_a_replacement() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope, remote).await;
        let candidate = descriptor(harness.scope, RebaselineSnapshotId::new(), 2, 7);
        harness
            .state
            .begin_rebaseline_candidate(harness.scope, candidate)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE rebaseline_candidates SET state = 'FAILED', expected_count = 1
             WHERE library_id = ?",
        )
        .bind(harness.scope.library_id().to_string())
        .execute(&harness.state.pool)
        .await
        .unwrap();

        assert!(matches!(
            harness.coordinator.run_convergence_once().await,
            Err(ClientSyncError::CandidateCorrupt)
        ));
        assert_eq!(harness.remote.create_calls(), 0);
        close(harness).await;
    }

    #[tokio::test]
    async fn replacement_rebaseline_preserves_pending_outbound_intent_exactly() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope, remote).await;
        let old = descriptor(harness.scope, RebaselineSnapshotId::new(), 2, 7);
        stage_pending(&harness, old, NodeId::new()).await;
        let intent = OutboundIntent::new(
            harness.scope.library_id(),
            None,
            None,
            OutboundIntentKind::CreateDirectory,
            ManagedRelativePath::new("still-local").unwrap(),
            None,
            Some(LocalFingerprint::directory()),
            Sequence::new(1),
            Sequence::new(1),
            None,
            None,
            None,
        )
        .unwrap();
        harness.state.upsert_outbound_intent(&intent).await.unwrap();
        let replacement = descriptor(harness.scope, RebaselineSnapshotId::new(), 3, 12);
        *harness.remote.handoffs.lock().unwrap() = VecDeque::from([
            Err(RemoteError::new(RemoteErrorKind::NotFound)),
            Ok(handoff(harness.scope, replacement)),
        ]);
        *harness.remote.creates.lock().unwrap() = VecDeque::from([Ok(replacement)]);
        *harness.remote.pages.lock().unwrap() =
            VecDeque::from([Ok(page(replacement, NodeId::new()))]);

        assert!(matches!(
            harness.coordinator.run_convergence_once().await.unwrap(),
            RebaselineConvergenceOutcome::RebaselineConverged { snapshot_id, .. }
                if snapshot_id == replacement.snapshot_id()
        ));
        assert_eq!(
            harness
                .state
                .outbound_intent(intent.intent_id())
                .await
                .unwrap(),
            Some(intent)
        );
        close(harness).await;
    }

    #[tokio::test]
    async fn same_library_concurrent_callers_create_only_one_snapshot() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope, remote).await;
        install_incremental_root(&harness, NodeId::new()).await;
        let created = descriptor(harness.scope, RebaselineSnapshotId::new(), 2, 9);
        *harness.remote.checkpoints.lock().unwrap() = VecDeque::from([
            Err(RemoteError::new(RemoteErrorKind::RebaselineRequired)),
            Ok(RemoteCheckpoint::new(
                harness.scope,
                created.boundary().journal_epoch(),
                created.boundary().resume_sequence(),
            )),
        ]);
        *harness.remote.feeds.lock().unwrap() = VecDeque::from([Ok(RemoteFeedPage::new(
            harness.scope,
            created.boundary().journal_epoch(),
            created.boundary().resume_sequence(),
            created.boundary().resume_sequence(),
            created.boundary().resume_sequence(),
            false,
            vec![],
            None,
        )
        .unwrap())]);
        *harness.remote.creates.lock().unwrap() = VecDeque::from([Ok(created)]);
        *harness.remote.pages.lock().unwrap() = VecDeque::from([Ok(page(created, NodeId::new()))]);
        *harness.remote.handoffs.lock().unwrap() =
            VecDeque::from([Ok(handoff(harness.scope, created))]);

        let first = harness.coordinator.run_convergence_once();
        let second = harness.coordinator.run_convergence_once();
        let (first, second) = tokio::join!(first, second);
        assert!(matches!(
            first.unwrap(),
            RebaselineConvergenceOutcome::RebaselineConverged { .. }
        ));
        assert_eq!(
            second.unwrap(),
            RebaselineConvergenceOutcome::IncrementalReady
        );
        assert_eq!(harness.remote.create_calls(), 1);
        close(harness).await;
    }

    #[tokio::test]
    async fn handoff_response_loss_keeps_handoff_for_idempotent_later_completion() {
        let scope = new_scope();
        let remote = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope, remote).await;
        let pending = descriptor(harness.scope, RebaselineSnapshotId::new(), 2, 7);
        stage_pending(&harness, pending, NodeId::new()).await;
        *harness.remote.handoffs.lock().unwrap() = VecDeque::from([
            Err(RemoteError::new(RemoteErrorKind::Offline)),
            Ok(handoff(harness.scope, pending)),
        ]);

        assert!(matches!(
            harness.coordinator.run_convergence_once().await,
            Err(ClientSyncError::HandoffTransport)
        ));
        assert!(
            harness
                .state
                .rebaseline_handoff_pending(harness.scope.library_id())
                .await
                .unwrap()
        );
        assert_eq!(harness.remote.create_calls(), 0);
        assert!(matches!(
            harness.coordinator.run_convergence_once().await.unwrap(),
            RebaselineConvergenceOutcome::RebaselineConverged { snapshot_id, .. }
                if snapshot_id == pending.snapshot_id()
        ));
        assert_eq!(harness.remote.create_calls(), 0);
        close(harness).await;
    }

    #[tokio::test]
    async fn independent_libraries_converge_without_cross_library_state_mutation() {
        let scope_a = new_scope();
        let remote_a = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let harness = harness(scope_a, remote_a).await;
        install_incremental_root(&harness, NodeId::new()).await;
        let scope_b = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
        let remote_b = Arc::new(ScriptedRemote::new([], [], [], [], []));
        let coordinator_b =
            coordinator_sharing_state(&harness, scope_b, remote_b.clone(), "root-b").await;
        install_incremental_root_for(harness.state.as_ref(), scope_b, NodeId::new()).await;

        let snapshot_a = descriptor(scope_a, RebaselineSnapshotId::new(), 2, 9);
        *harness.remote.checkpoints.lock().unwrap() =
            VecDeque::from([Err(RemoteError::new(RemoteErrorKind::RebaselineRequired))]);
        *harness.remote.creates.lock().unwrap() = VecDeque::from([Ok(snapshot_a)]);
        *harness.remote.pages.lock().unwrap() =
            VecDeque::from([Ok(page(snapshot_a, NodeId::new()))]);
        *harness.remote.handoffs.lock().unwrap() =
            VecDeque::from([Ok(handoff(scope_a, snapshot_a))]);

        let snapshot_b = descriptor(scope_b, RebaselineSnapshotId::new(), 3, 12);
        *remote_b.checkpoints.lock().unwrap() =
            VecDeque::from([Err(RemoteError::new(RemoteErrorKind::RebaselineRequired))]);
        *remote_b.creates.lock().unwrap() = VecDeque::from([Ok(snapshot_b)]);
        *remote_b.pages.lock().unwrap() = VecDeque::from([Ok(page(snapshot_b, NodeId::new()))]);
        *remote_b.handoffs.lock().unwrap() = VecDeque::from([Ok(handoff(scope_b, snapshot_b))]);

        let (outcome_a, outcome_b) = tokio::join!(
            harness.coordinator.run_convergence_once(),
            coordinator_b.run_convergence_once(),
        );
        assert!(matches!(
            outcome_a.unwrap(),
            RebaselineConvergenceOutcome::RebaselineConverged { snapshot_id, .. }
                if snapshot_id == snapshot_a.snapshot_id()
        ));
        assert!(matches!(
            outcome_b.unwrap(),
            RebaselineConvergenceOutcome::RebaselineConverged { snapshot_id, .. }
                if snapshot_id == snapshot_b.snapshot_id()
        ));
        assert_eq!(harness.remote.create_calls(), 1);
        assert_eq!(remote_b.create_calls(), 1);
        assert!(
            harness
                .state
                .rebaseline_candidate(scope_a.library_id())
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            harness
                .state
                .rebaseline_candidate(scope_b.library_id())
                .await
                .unwrap()
                .is_none()
        );
        drop(coordinator_b);
        close(harness).await;
    }
}
