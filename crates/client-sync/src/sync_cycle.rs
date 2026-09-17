//! One-shot bidirectional synchronization composition.
//!
//! This module is deliberately a composition boundary, not a third
//! synchronization state machine. [`RebaselineConvergenceCoordinator`] owns
//! inbound progress and bounded recovery; [`OutboundSubmissionEngine`] owns
//! durable intent submission, idempotency, uploads, and the Prompt 88
//! conflict fence. The runner calls each existing boundary once, in that
//! order, and owns no durable cycle state of its own.

use std::sync::Arc;

use synveil_core::Timestamp;

use crate::{
    ClientSyncError, OutboundSubmissionEngine, OutboundSubmissionOutcome,
    RebaselineConvergenceCoordinator, RebaselineConvergenceOutcome,
    RebaselineRecoveryBlockedReason, RemoteErrorKind, ReplicaScope, SyncOutcome,
};

/// The inbound/convergence half of one bounded synchronization cycle.
///
/// The successful variant retains the complete Prompt 87 result rather than
/// flattening it. Expected remote operational failures are represented by
/// safe, typed classifications; they do not become generic internal errors
/// and they always make the outbound half ineligible for this invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InboundCycleOutcome {
    /// Prompt 87 completed its one bounded convergence traversal.
    Converged(RebaselineConvergenceOutcome),
    /// The current device credential is absent, unauthorized, revoked, or
    /// otherwise forbidden. No outbound request is permitted afterward.
    AuthRequired,
    /// The transport could not establish a safe current inbound state.
    /// `RemoteErrorKind` contains only a closed, non-sensitive classification.
    Offline,
    /// The inbound request was rate limited. The cycle never retries it and
    /// does not use an unconfirmed state as an outbound base.
    RateLimited,
    /// Another expected remote failure was returned without a safe base.
    RemoteFailure(RemoteErrorKind),
}

impl InboundCycleOutcome {
    #[must_use]
    pub const fn is_safe_for_outbound(self) -> bool {
        match self {
            Self::Converged(outcome) => outcome_is_safe_for_outbound(outcome),
            Self::AuthRequired | Self::Offline | Self::RateLimited | Self::RemoteFailure(_) => {
                false
            }
        }
    }

    #[must_use]
    pub const fn is_auth_required(self) -> bool {
        matches!(self, Self::AuthRequired)
    }

    #[must_use]
    pub const fn is_transport_failure(self) -> bool {
        matches!(self, Self::Offline)
    }

    #[must_use]
    pub const fn made_durable_progress(self) -> bool {
        match self {
            Self::Converged(outcome) => convergence_made_progress(outcome),
            Self::AuthRequired | Self::Offline | Self::RateLimited | Self::RemoteFailure(_) => {
                false
            }
        }
    }

    #[must_use]
    pub const fn more_work_likely(self) -> bool {
        match self {
            Self::Converged(outcome) => convergence_more_work_likely(outcome),
            Self::AuthRequired | Self::Offline | Self::RateLimited | Self::RemoteFailure(_) => true,
        }
    }
}

/// Why the bounded outbound step was deliberately not called.
///
/// These are policy outcomes, not failures. The underlying durable state is
/// left for a later caller or for explicit user intervention.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboundSkipReason {
    InboundAuthenticationRequired,
    InboundOffline,
    InboundRateLimited,
    InboundRemoteFailure(RemoteErrorKind),
    RecoveryBlocked(RebaselineRecoveryBlockedReason),
    IncrementalNotReady(SyncOutcome),
    CandidatePending,
    HandoffPending,
    BootstrapPending,
    InboundWorkPending,
    LocalIssue,
    ReplicaNotReady,
}

/// The outbound half of one bounded synchronization cycle.
///
/// `Attempted` contains the existing Prompt 88 outcome. In particular,
/// `Attempted(Conflict { .. })` means the conflict was durably persisted and
/// `Attempted(BlockedByConflict(..))` means the existing library-wide fence
/// was honored without another mutation request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboundCycleOutcome {
    NotAttempted(OutboundSkipReason),
    Attempted(OutboundSubmissionOutcome),
}

impl OutboundCycleOutcome {
    #[must_use]
    pub const fn attempted(self) -> bool {
        matches!(self, Self::Attempted(_))
    }

    #[must_use]
    pub const fn is_idle(self) -> bool {
        matches!(
            self,
            Self::Attempted(OutboundSubmissionOutcome::NoReadyIntent)
        )
    }

    #[must_use]
    pub const fn made_durable_progress(self) -> bool {
        match self {
            Self::NotAttempted(_) => false,
            Self::Attempted(outcome) => matches!(
                outcome,
                OutboundSubmissionOutcome::Submitted(_)
                    | OutboundSubmissionOutcome::Conflict { .. }
                    | OutboundSubmissionOutcome::Blocked(_)
            ),
        }
    }

    #[must_use]
    pub const fn more_work_likely(self) -> bool {
        match self {
            Self::NotAttempted(_) => true,
            Self::Attempted(outcome) => {
                !matches!(outcome, OutboundSubmissionOutcome::NoReadyIntent)
            }
        }
    }

    #[must_use]
    pub const fn requires_conflict_resolution(self) -> bool {
        matches!(
            self,
            Self::Attempted(
                OutboundSubmissionOutcome::Conflict { .. }
                    | OutboundSubmissionOutcome::BlockedByConflict(_)
            )
        )
    }

    #[must_use]
    pub const fn requires_authentication(self) -> bool {
        matches!(
            self,
            Self::Attempted(OutboundSubmissionOutcome::AuthRequired)
                | Self::NotAttempted(OutboundSkipReason::InboundAuthenticationRequired)
        )
    }
}

/// The complete safe report for one finite bidirectional invocation.
///
/// The timestamp is caller-supplied metadata for deterministic observation and
/// logging. It is not persisted by the orchestrator and it does not replace
/// the lower-level engines' existing durable timestamps or server evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncCycleResult {
    observed_at: Timestamp,
    inbound: InboundCycleOutcome,
    outbound: OutboundCycleOutcome,
}

impl SyncCycleResult {
    #[must_use]
    pub const fn new(
        observed_at: Timestamp,
        inbound: InboundCycleOutcome,
        outbound: OutboundCycleOutcome,
    ) -> Self {
        Self {
            observed_at,
            inbound,
            outbound,
        }
    }

    #[must_use]
    pub const fn observed_at(self) -> Timestamp {
        self.observed_at
    }

    #[must_use]
    pub const fn inbound(self) -> InboundCycleOutcome {
        self.inbound
    }

    #[must_use]
    pub const fn outbound(self) -> OutboundCycleOutcome {
        self.outbound
    }

    #[must_use]
    pub const fn is_idle(self) -> bool {
        matches!(
            self.inbound,
            InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::IncrementalReady)
        ) && self.outbound.is_idle()
    }

    #[must_use]
    pub const fn made_durable_progress(self) -> bool {
        self.inbound.made_durable_progress() || self.outbound.made_durable_progress()
    }

    #[must_use]
    pub const fn more_work_likely(self) -> bool {
        self.inbound.more_work_likely() || self.outbound.more_work_likely()
    }

    #[must_use]
    pub const fn requires_conflict_resolution(self) -> bool {
        self.outbound.requires_conflict_resolution()
    }

    #[must_use]
    pub const fn requires_authentication(self) -> bool {
        self.inbound.is_auth_required() || self.outbound.requires_authentication()
    }
}

/// One transport-neutral bidirectional synchronization step.
///
/// The runner is restart-stateless: every correctness boundary belongs to the
/// existing SQLite-backed inbound, recovery, outbound, and conflict engines.
/// One invocation performs one convergence call and, only when the resulting
/// durable state is safe, one outbound intent/submission call. It never loops,
/// recurses, retries, sleeps, polls, drains the feed, or drains the queue.
pub struct BidirectionalSyncCycleRunner {
    scope: ReplicaScope,
    convergence: Arc<RebaselineConvergenceCoordinator>,
    outbound: Arc<OutboundSubmissionEngine>,
    state: Arc<crate::LocalStateStore>,
    #[cfg(test)]
    hook: Option<Arc<dyn SyncCycleFailureInjector>>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SyncCycleFailurePoint {
    BeforeInbound,
}

#[cfg(test)]
pub(crate) trait SyncCycleFailureInjector: Send + Sync {
    fn check(&self, point: SyncCycleFailurePoint) -> Result<(), ClientSyncError>;
}

/// Repository-compatible name for callers that prefer the coordinator
/// vocabulary used by the preceding sync prompts.
pub type SyncCycleCoordinator = BidirectionalSyncCycleRunner;

impl BidirectionalSyncCycleRunner {
    /// Compose the already-constructed Prompt 87 and Prompt 88 engines.
    ///
    /// Both engines must refer to the same scope and the same local state
    /// store. Requiring the shared `Arc` prevents a caller from accidentally
    /// inspecting one SQLite database while mutating another. No network or
    /// database work is performed by construction.
    pub fn new(
        convergence: Arc<RebaselineConvergenceCoordinator>,
        outbound: Arc<OutboundSubmissionEngine>,
    ) -> Result<Self, ClientSyncError> {
        if convergence.scope() != outbound.scope() {
            return Err(ClientSyncError::WrongScope);
        }
        if !Arc::ptr_eq(convergence.state(), outbound.state()) {
            return Err(ClientSyncError::InvalidState);
        }
        Ok(Self {
            scope: convergence.scope(),
            state: Arc::clone(convergence.state()),
            convergence,
            outbound,
            #[cfg(test)]
            hook: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn with_failure_injector(
        convergence: Arc<RebaselineConvergenceCoordinator>,
        outbound: Arc<OutboundSubmissionEngine>,
        hook: Arc<dyn SyncCycleFailureInjector>,
    ) -> Result<Self, ClientSyncError> {
        let mut runner = Self::new(convergence, outbound)?;
        runner.hook = Some(hook);
        Ok(runner)
    }

    #[cfg(test)]
    fn check(&self, point: SyncCycleFailurePoint) -> Result<(), ClientSyncError> {
        if let Some(hook) = &self.hook {
            hook.check(point)?;
        }
        Ok(())
    }

    #[must_use]
    pub const fn scope(&self) -> ReplicaScope {
        self.scope
    }

    /// Run exactly one bounded bidirectional synchronization cycle.
    ///
    /// The initial read is deliberately local and side-effect free. The
    /// coordinator then rechecks its own more specific candidate/handoff
    /// precedence under its established per-library guard. After it returns,
    /// the runner performs a second local read before deciding whether Prompt
    /// 88 may be called. That final check is what prevents an incomplete
    /// candidate, handoff, bootstrap, pending inbound acknowledgement, or
    /// local blocker from being used as an outbound base.
    pub async fn run_once(
        &self,
        observed_at: Timestamp,
    ) -> Result<SyncCycleResult, ClientSyncError> {
        let _ = self.inspect_local_state().await?;
        #[cfg(test)]
        self.check(SyncCycleFailurePoint::BeforeInbound)?;

        let inbound = match self.convergence.run_convergence_once().await {
            Ok(outcome) => normalize_inbound_outcome(outcome),
            Err(error) => map_inbound_error(error)?,
        };

        let outbound = if let Some(reason) = skip_for_inbound(inbound) {
            OutboundCycleOutcome::NotAttempted(reason)
        } else if let Some(reason) = self.skip_for_local_state().await? {
            OutboundCycleOutcome::NotAttempted(reason)
        } else {
            OutboundCycleOutcome::Attempted(self.outbound.process_next_ready_intent().await?)
        };

        Ok(SyncCycleResult::new(observed_at, inbound, outbound))
    }

    async fn inspect_local_state(&self) -> Result<LocalStateInspection, ClientSyncError> {
        let replica = self
            .state
            .replica(self.scope.library_id())
            .await?
            .ok_or(ClientSyncError::InvalidState)?;
        if replica.scope() != self.scope {
            return Err(ClientSyncError::WrongScope);
        }

        let inspection = LocalStateInspection {
            has_candidate: self
                .state
                .rebaseline_candidate(self.scope.library_id())
                .await?
                .is_some(),
            has_pending_handoff: self
                .state
                .pending_rebaseline_handoff(self.scope.library_id())
                .await?
                .is_some(),
            has_bootstrap: self
                .state
                .bootstrap(self.scope.library_id())
                .await?
                .is_some(),
            has_pending_page: self
                .state
                .pending_page(self.scope.library_id())
                .await?
                .is_some(),
            has_pending_ack: self
                .state
                .pending_ack(self.scope.library_id())
                .await?
                .is_some(),
            has_local_issue: !self
                .state
                .unresolved_issues(self.scope.library_id())
                .await?
                .is_empty()
                || !self
                    .state
                    .observation_issues(self.scope.library_id())
                    .await?
                    .is_empty(),
            root_present: replica.root_node_id().is_some(),
        };

        // Read the existing conflict fence as part of local inspection. It is
        // intentionally not used to skip inbound: Prompt 88 requires remote
        // authoritative state to continue advancing while outbound is fenced.
        let _ = self
            .state
            .first_unresolved_conflict(self.scope.library_id())
            .await?;
        Ok(inspection)
    }

    async fn skip_for_local_state(&self) -> Result<Option<OutboundSkipReason>, ClientSyncError> {
        let state = self.inspect_local_state().await?;
        Ok(if state.has_candidate {
            Some(OutboundSkipReason::CandidatePending)
        } else if state.has_pending_handoff {
            Some(OutboundSkipReason::HandoffPending)
        } else if state.has_bootstrap {
            Some(OutboundSkipReason::BootstrapPending)
        } else if state.has_pending_page || state.has_pending_ack {
            Some(OutboundSkipReason::InboundWorkPending)
        } else if state.has_local_issue {
            Some(OutboundSkipReason::LocalIssue)
        } else if !state.root_present {
            Some(OutboundSkipReason::ReplicaNotReady)
        } else {
            None
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LocalStateInspection {
    has_candidate: bool,
    has_pending_handoff: bool,
    has_bootstrap: bool,
    has_pending_page: bool,
    has_pending_ack: bool,
    has_local_issue: bool,
    root_present: bool,
}

fn normalize_inbound_outcome(outcome: RebaselineConvergenceOutcome) -> InboundCycleOutcome {
    match outcome {
        RebaselineConvergenceOutcome::IncrementalProgress(SyncOutcome::Offline) => {
            InboundCycleOutcome::Offline
        }
        other => InboundCycleOutcome::Converged(other),
    }
}

fn map_inbound_error(error: ClientSyncError) -> Result<InboundCycleOutcome, ClientSyncError> {
    match error {
        ClientSyncError::AuthenticationRequired => Ok(InboundCycleOutcome::AuthRequired),
        // Prompt 85 normalizes a transport failure during handoff to this
        // stable client error. The caller still receives a typed cycle result
        // and, conservatively, cannot submit outbound work in the same call.
        ClientSyncError::HandoffTransport => Ok(InboundCycleOutcome::Offline),
        ClientSyncError::Remote(remote) => Ok(match remote.kind() {
            RemoteErrorKind::AuthRequired
            | RemoteErrorKind::DeviceRevoked
            | RemoteErrorKind::Forbidden => InboundCycleOutcome::AuthRequired,
            RemoteErrorKind::RateLimited => InboundCycleOutcome::RateLimited,
            RemoteErrorKind::Offline
            | RemoteErrorKind::Unavailable
            | RemoteErrorKind::Timeout
            | RemoteErrorKind::Tls => InboundCycleOutcome::Offline,
            kind => InboundCycleOutcome::RemoteFailure(kind),
        }),
        other => Err(other),
    }
}

fn skip_for_inbound(inbound: InboundCycleOutcome) -> Option<OutboundSkipReason> {
    match inbound {
        InboundCycleOutcome::AuthRequired => {
            Some(OutboundSkipReason::InboundAuthenticationRequired)
        }
        InboundCycleOutcome::Offline => Some(OutboundSkipReason::InboundOffline),
        InboundCycleOutcome::RateLimited => Some(OutboundSkipReason::InboundRateLimited),
        InboundCycleOutcome::RemoteFailure(kind) => {
            Some(OutboundSkipReason::InboundRemoteFailure(kind))
        }
        InboundCycleOutcome::Converged(outcome) => match outcome {
            RebaselineConvergenceOutcome::RecoveryBlocked { reason } => {
                Some(OutboundSkipReason::RecoveryBlocked(reason))
            }
            RebaselineConvergenceOutcome::IncrementalProgress(outcome)
                if !sync_outcome_is_safe_for_outbound(outcome) =>
            {
                Some(OutboundSkipReason::IncrementalNotReady(outcome))
            }
            RebaselineConvergenceOutcome::IncrementalReady
            | RebaselineConvergenceOutcome::RebaselineConverged { .. }
            | RebaselineConvergenceOutcome::IncrementalProgress(_) => None,
        },
    }
}

const fn sync_outcome_is_safe_for_outbound(outcome: SyncOutcome) -> bool {
    matches!(
        outcome,
        SyncOutcome::Idle | SyncOutcome::Progressed | SyncOutcome::MoreAvailable
    )
}

const fn outcome_is_safe_for_outbound(outcome: RebaselineConvergenceOutcome) -> bool {
    match outcome {
        RebaselineConvergenceOutcome::IncrementalReady
        | RebaselineConvergenceOutcome::RebaselineConverged { .. } => true,
        RebaselineConvergenceOutcome::IncrementalProgress(outcome) => {
            matches!(
                outcome,
                SyncOutcome::Idle | SyncOutcome::Progressed | SyncOutcome::MoreAvailable
            )
        }
        RebaselineConvergenceOutcome::RecoveryBlocked { .. } => false,
    }
}

const fn convergence_made_progress(outcome: RebaselineConvergenceOutcome) -> bool {
    match outcome {
        RebaselineConvergenceOutcome::IncrementalReady => false,
        RebaselineConvergenceOutcome::IncrementalProgress(outcome) => matches!(
            outcome,
            SyncOutcome::Progressed | SyncOutcome::MoreAvailable | SyncOutcome::BootstrapRequired
        ),
        RebaselineConvergenceOutcome::RebaselineConverged { .. } => true,
        RebaselineConvergenceOutcome::RecoveryBlocked { reason } => {
            matches!(reason, RebaselineRecoveryBlockedReason::DidNotConverge)
        }
    }
}

const fn convergence_more_work_likely(outcome: RebaselineConvergenceOutcome) -> bool {
    match outcome {
        RebaselineConvergenceOutcome::IncrementalReady => false,
        RebaselineConvergenceOutcome::IncrementalProgress(outcome) => matches!(
            outcome,
            SyncOutcome::MoreAvailable
                | SyncOutcome::BootstrapRequired
                | SyncOutcome::Blocked
                | SyncOutcome::Offline
                | SyncOutcome::RebaselineRequired
        ),
        RebaselineConvergenceOutcome::RebaselineConverged { .. } => false,
        RebaselineConvergenceOutcome::RecoveryBlocked { .. } => true,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, VecDeque},
        fs,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use async_trait::async_trait;
    use synveil_core::{
        ChangeEvent, ChangeEventId, ChangeKind, ChangeResourceKind, ClientMutation, DeviceId,
        FileVersionId, LibraryId, LogicalName, LogicalSnapshotNode, NodeId, NodeKind, NodeState,
        RebaselineSnapshotId, Revision, Sequence, Timestamp, UserId,
    };

    use super::*;
    use crate::outbound::{OutboundFailureInjector, OutboundFailurePoint};
    use crate::{
        BootstrapCompletion, BootstrapPage, FilesystemLocalReplica, InboundChange, LocalNode,
        LocalReplica, LocalStateConfig, LocalStateStore, ManagedRelativePath, OpaqueEvidence,
        RebaselineHandoffConfirmation, RebaselineSnapshotDescriptor, RebaselineSnapshotPage,
        RebaselineSnapshotRemote, RebaselineSnapshotSource, RemoteCheckpoint, RemoteContent,
        RemoteError, RemoteFeedPage, RemoteMutationApplied, RemoteMutationConflict,
        RemoteMutationOutcome, SyncRemote,
    };

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "synveil-sync-cycle-{label}-{}",
                uuid::Uuid::now_v7()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct ScriptedRemote {
        inner: Mutex<RemoteState>,
    }

    struct RemoteState {
        epoch: Sequence,
        acknowledged: Sequence,
        checkpoints: VecDeque<Result<RemoteCheckpoint, RemoteError>>,
        feeds: VecDeque<Result<RemoteFeedPage, RemoteError>>,
        acknowledgements: VecDeque<Result<RemoteCheckpoint, RemoteError>>,
        handoffs: VecDeque<Result<RebaselineHandoffConfirmation, RemoteError>>,
        creates: VecDeque<Result<RebaselineSnapshotDescriptor, RemoteError>>,
        pages: VecDeque<Result<RebaselineSnapshotPage, RemoteError>>,
        mutations: VecDeque<Result<RemoteMutationOutcome, RemoteError>>,
        committed_mutations: BTreeMap<synveil_core::ClientMutationId, RemoteMutationApplied>,
        mutation_attempts: usize,
        mutation_commits: usize,
        create_calls: usize,
        handoff_calls: usize,
        feed_calls: usize,
        fail_mutation_after_commit: bool,
    }

    impl ScriptedRemote {
        fn new() -> Self {
            Self {
                inner: Mutex::new(RemoteState {
                    epoch: Sequence::new(1),
                    acknowledged: Sequence::new(0),
                    checkpoints: VecDeque::new(),
                    feeds: VecDeque::new(),
                    acknowledgements: VecDeque::new(),
                    handoffs: VecDeque::new(),
                    creates: VecDeque::new(),
                    pages: VecDeque::new(),
                    mutations: VecDeque::new(),
                    committed_mutations: BTreeMap::new(),
                    mutation_attempts: 0,
                    mutation_commits: 0,
                    create_calls: 0,
                    handoff_calls: 0,
                    feed_calls: 0,
                    fail_mutation_after_commit: false,
                }),
            }
        }

        fn with_state<R>(&self, action: impl FnOnce(&mut RemoteState) -> R) -> R {
            action(&mut self.inner.lock().unwrap())
        }

        fn mutation_attempts(&self) -> usize {
            self.with_state(|state| state.mutation_attempts)
        }

        fn mutation_commits(&self) -> usize {
            self.with_state(|state| state.mutation_commits)
        }

        fn create_calls(&self) -> usize {
            self.with_state(|state| state.create_calls)
        }

        fn handoff_calls(&self) -> usize {
            self.with_state(|state| state.handoff_calls)
        }

        fn feed_calls(&self) -> usize {
            self.with_state(|state| state.feed_calls)
        }

        fn pending_feed_scripts(&self) -> usize {
            self.with_state(|state| state.feeds.len())
        }

        fn set_failure_after_commit(&self) {
            self.with_state(|state| state.fail_mutation_after_commit = true);
        }
    }

    #[async_trait]
    impl SyncRemote for ScriptedRemote {
        async fn get_checkpoint(
            &self,
            scope: ReplicaScope,
        ) -> Result<RemoteCheckpoint, RemoteError> {
            self.with_state(|state| {
                state.checkpoints.pop_front().unwrap_or_else(|| {
                    Ok(RemoteCheckpoint::new(
                        scope,
                        state.epoch,
                        state.acknowledged,
                    ))
                })
            })
        }

        async fn fetch_changes(
            &self,
            scope: ReplicaScope,
            _limit: u32,
        ) -> Result<RemoteFeedPage, RemoteError> {
            self.with_state(|state| {
                state.feed_calls += 1;
                state.feeds.pop_front().unwrap_or_else(|| {
                    Ok(RemoteFeedPage::new(
                        scope,
                        state.epoch,
                        state.acknowledged,
                        state.acknowledged,
                        state.acknowledged,
                        false,
                        Vec::new(),
                        None,
                    )
                    .expect("valid empty feed"))
                })
            })
        }

        async fn acknowledge_changes(
            &self,
            scope: ReplicaScope,
            _evidence: &OpaqueEvidence,
        ) -> Result<RemoteCheckpoint, RemoteError> {
            self.with_state(|state| {
                let result = state.acknowledgements.pop_front().unwrap_or_else(|| {
                    Ok(RemoteCheckpoint::new(
                        scope,
                        state.epoch,
                        state.acknowledged,
                    ))
                });
                if let Ok(checkpoint) = result {
                    state.epoch = checkpoint.epoch();
                    state.acknowledged = checkpoint.acknowledged_sequence();
                    Ok(checkpoint)
                } else {
                    result
                }
            })
        }

        async fn complete_rebaseline_handoff(
            &self,
            scope: ReplicaScope,
            _snapshot_id: RebaselineSnapshotId,
        ) -> Result<RebaselineHandoffConfirmation, RemoteError> {
            self.with_state(|state| {
                state.handoff_calls += 1;
                let result = state
                    .handoffs
                    .pop_front()
                    .unwrap_or_else(|| Err(RemoteError::new(RemoteErrorKind::Protocol)));
                if let Ok(confirmation) = result {
                    let checkpoint = confirmation.checkpoint();
                    state.epoch = checkpoint.epoch();
                    state.acknowledged = checkpoint.acknowledged_sequence();
                    if confirmation.library_id() == scope.library_id()
                        && checkpoint.scope() == scope
                    {
                        Ok(confirmation)
                    } else {
                        Err(RemoteError::new(RemoteErrorKind::Protocol))
                    }
                } else {
                    result
                }
            })
        }

        async fn start_rebaseline(
            &self,
            _scope: ReplicaScope,
        ) -> Result<synveil_core::SyncBootstrap, RemoteError> {
            Err(RemoteError::new(RemoteErrorKind::Forbidden))
        }

        async fn fetch_rebaseline_page(
            &self,
            _scope: ReplicaScope,
            _bootstrap_id: synveil_core::SyncBootstrapId,
            _cursor: Option<&OpaqueEvidence>,
            _limit: u32,
        ) -> Result<BootstrapPage, RemoteError> {
            Err(RemoteError::new(RemoteErrorKind::Forbidden))
        }

        async fn complete_rebaseline(
            &self,
            _scope: ReplicaScope,
            _bootstrap_id: synveil_core::SyncBootstrapId,
            _evidence: &OpaqueEvidence,
        ) -> Result<BootstrapCompletion, RemoteError> {
            Err(RemoteError::new(RemoteErrorKind::Forbidden))
        }

        async fn download_current_content(
            &self,
            _scope: ReplicaScope,
            _node_id: NodeId,
            _version_id: FileVersionId,
        ) -> Result<RemoteContent, RemoteError> {
            Err(RemoteError::new(RemoteErrorKind::Forbidden))
        }

        async fn submit_client_mutation(
            &self,
            _scope: ReplicaScope,
            request: &synveil_core::ClientMutationRequest,
        ) -> Result<RemoteMutationOutcome, RemoteError> {
            self.with_state(|state| {
                state.mutation_attempts += 1;
                if let Some(outcome) = state.mutations.pop_front() {
                    return outcome;
                }
                if let Some(applied) = state.committed_mutations.get(&request.mutation_id()) {
                    return Ok(RemoteMutationOutcome::Applied(RemoteMutationApplied::new(
                        applied.mutation_id(),
                        applied.node_id(),
                        applied.revision(),
                        applied.journal_event_id(),
                        applied.journal_sequence(),
                        true,
                    )));
                }
                let node_id = match request.mutation() {
                    ClientMutation::CreateDirectory { parent_node_id, .. } => *parent_node_id,
                    ClientMutation::RenameNode { node_id, .. }
                    | ClientMutation::MoveNode { node_id, .. }
                    | ClientMutation::TrashNode { node_id, .. }
                    | ClientMutation::RestoreNode { node_id, .. } => *node_id,
                };
                let applied = RemoteMutationApplied::new(
                    request.mutation_id(),
                    node_id,
                    Revision::new(2),
                    ChangeEventId::new(),
                    Sequence::new(state.acknowledged.get().saturating_add(1)),
                    false,
                );
                state.mutation_commits += 1;
                state
                    .committed_mutations
                    .insert(request.mutation_id(), applied);
                if state.fail_mutation_after_commit {
                    state.fail_mutation_after_commit = false;
                    Err(RemoteError::new(RemoteErrorKind::Timeout))
                } else {
                    Ok(RemoteMutationOutcome::Applied(applied))
                }
            })
        }
    }

    #[async_trait]
    impl RebaselineSnapshotSource for ScriptedRemote {
        async fn read_page(
            &self,
            _scope: ReplicaScope,
            _snapshot_id: RebaselineSnapshotId,
            _cursor: Option<&OpaqueEvidence>,
            _limit: u32,
        ) -> Result<RebaselineSnapshotPage, RemoteError> {
            self.with_state(|state| {
                state
                    .pages
                    .pop_front()
                    .unwrap_or_else(|| Err(RemoteError::new(RemoteErrorKind::Protocol)))
            })
        }
    }

    #[async_trait]
    impl RebaselineSnapshotRemote for ScriptedRemote {
        async fn create_snapshot(
            &self,
            _scope: ReplicaScope,
        ) -> Result<RebaselineSnapshotDescriptor, RemoteError> {
            self.with_state(|state| {
                state.create_calls += 1;
                state
                    .creates
                    .pop_front()
                    .unwrap_or_else(|| Err(RemoteError::new(RemoteErrorKind::Protocol)))
            })
        }
    }

    struct Harness {
        _temp: TempDir,
        scope: ReplicaScope,
        state: Arc<LocalStateStore>,
        replica: Arc<FilesystemLocalReplica>,
        remote: Arc<ScriptedRemote>,
        convergence: Arc<RebaselineConvergenceCoordinator>,
        outbound: Arc<OutboundSubmissionEngine>,
        cycle: Arc<BidirectionalSyncCycleRunner>,
    }

    impl Harness {
        async fn new(label: &str) -> Self {
            let temp = TempDir::new(label);
            let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
            let state = Arc::new(
                LocalStateStore::open(&LocalStateConfig::new(temp.0.join("state.sqlite3")))
                    .await
                    .unwrap(),
            );
            let root = temp.0.join("managed");
            fs::create_dir_all(&root).unwrap();
            let replica = Arc::new(FilesystemLocalReplica::initialize(&root, scope).unwrap());
            let remote = Arc::new(ScriptedRemote::new());
            let inbound = Arc::new(
                crate::InboundSyncEngine::new(
                    scope,
                    remote.clone(),
                    replica.clone(),
                    state.clone(),
                    crate::EngineConfig::new(2, 2).unwrap(),
                )
                .await
                .unwrap(),
            );
            let convergence = Arc::new(
                RebaselineConvergenceCoordinator::new(inbound, remote.clone(), state.clone(), 2)
                    .unwrap(),
            );
            let outbound = Arc::new(
                crate::OutboundSubmissionEngine::new(
                    scope,
                    remote.clone(),
                    replica.clone(),
                    state.clone(),
                )
                .await
                .unwrap(),
            );
            let cycle = Arc::new(
                BidirectionalSyncCycleRunner::new(convergence.clone(), outbound.clone()).unwrap(),
            );
            Self {
                _temp: temp,
                scope,
                state,
                replica,
                remote,
                convergence,
                outbound,
                cycle,
            }
        }

        async fn close(self) {
            self.state.close_pool().await;
            drop(self);
        }

        async fn cycle_with_outbound_hook(
            &self,
            hook: Arc<dyn OutboundFailureInjector>,
        ) -> Arc<BidirectionalSyncCycleRunner> {
            let outbound = Arc::new(
                OutboundSubmissionEngine::with_failure_injector(
                    self.scope,
                    self.remote.clone(),
                    self.replica.clone(),
                    self.state.clone(),
                    hook,
                )
                .await
                .unwrap(),
            );
            Arc::new(BidirectionalSyncCycleRunner::new(self.convergence.clone(), outbound).unwrap())
        }

        fn cycle_with_cycle_hook(
            &self,
            hook: Arc<dyn SyncCycleFailureInjector>,
        ) -> Arc<BidirectionalSyncCycleRunner> {
            Arc::new(
                BidirectionalSyncCycleRunner::with_failure_injector(
                    self.convergence.clone(),
                    self.outbound.clone(),
                    hook,
                )
                .unwrap(),
            )
        }
    }

    struct OnceHook {
        point: OutboundFailurePoint,
        fired: Mutex<bool>,
    }

    impl OnceHook {
        fn new(point: OutboundFailurePoint) -> Self {
            Self {
                point,
                fired: Mutex::new(false),
            }
        }
    }

    impl OutboundFailureInjector for OnceHook {
        fn check(&self, point: OutboundFailurePoint) -> Result<(), crate::ClientSyncError> {
            let mut fired = self.fired.lock().unwrap();
            if point == self.point && !*fired {
                *fired = true;
                return Err(crate::ClientSyncError::InjectedFailure);
            }
            Ok(())
        }
    }

    struct CycleOnceHook {
        fired: Mutex<bool>,
    }

    impl CycleOnceHook {
        fn new() -> Self {
            Self {
                fired: Mutex::new(false),
            }
        }
    }

    impl SyncCycleFailureInjector for CycleOnceHook {
        fn check(&self, point: SyncCycleFailurePoint) -> Result<(), ClientSyncError> {
            let mut fired = self.fired.lock().unwrap();
            if point == SyncCycleFailurePoint::BeforeInbound && !*fired {
                *fired = true;
                return Err(ClientSyncError::InjectedFailure);
            }
            Ok(())
        }
    }

    fn fixed_time() -> Timestamp {
        Timestamp::parse("2026-09-12T00:00:00.123456Z").unwrap()
    }

    fn root_node(root: NodeId) -> LogicalSnapshotNode {
        LogicalSnapshotNode::new(
            root,
            None,
            LogicalName::new("root").unwrap(),
            NodeKind::Directory,
            NodeState::Active,
            Revision::new(1),
            None,
            None,
            None,
        )
        .unwrap()
    }

    async fn install_root(harness: &Harness) -> NodeId {
        let root = NodeId::new();
        harness
            .state
            .upsert_local_node(&LocalNode::new(
                harness.scope.library_id(),
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
        .bind(harness.scope.library_id().to_string())
        .execute(&harness.state.pool)
        .await
        .unwrap();
        root
    }

    async fn create_directory_intent(
        harness: &Harness,
        root: NodeId,
        name: &str,
        base_epoch: u64,
        base_sequence: u64,
    ) -> crate::OutboundIntent {
        let relative = ManagedRelativePath::new(name).unwrap();
        fs::create_dir(harness.replica.root_path().join(relative.as_path())).unwrap();
        let intent = crate::OutboundIntent::new(
            harness.scope.library_id(),
            None,
            Some(root),
            crate::OutboundIntentKind::CreateDirectory,
            relative,
            None,
            Some(crate::LocalFingerprint::directory()),
            Sequence::new(base_epoch),
            Sequence::new(base_sequence),
            None,
            None,
            Some(Revision::new(1)),
        )
        .unwrap();
        harness.state.upsert_outbound_intent(&intent).await.unwrap()
    }

    async fn install_local_target(harness: &Harness, root: NodeId, name: &str) -> NodeId {
        let node = NodeId::new();
        let relative = ManagedRelativePath::new(name).unwrap();
        fs::create_dir(harness.replica.root_path().join(relative.as_path())).unwrap();
        harness
            .state
            .upsert_local_node(&LocalNode::new(
                harness.scope.library_id(),
                node,
                Some(root),
                relative,
                LogicalName::new(name).unwrap(),
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
        node
    }

    async fn create_rename_intent(
        harness: &Harness,
        root: NodeId,
        node: NodeId,
        old_name: &str,
        new_name: &str,
    ) -> crate::OutboundIntent {
        fs::rename(
            harness.replica.root_path().join(old_name),
            harness.replica.root_path().join(new_name),
        )
        .unwrap();
        let intent = crate::OutboundIntent::new(
            harness.scope.library_id(),
            Some(node),
            Some(root),
            crate::OutboundIntentKind::RenameNode,
            ManagedRelativePath::new(new_name).unwrap(),
            Some(ManagedRelativePath::new(old_name).unwrap()),
            Some(crate::LocalFingerprint::directory()),
            Sequence::new(1),
            Sequence::new(0),
            Some(Revision::new(1)),
            None,
            Some(Revision::new(1)),
        )
        .unwrap();
        harness.state.upsert_outbound_intent(&intent).await.unwrap()
    }

    fn remote_child(root: NodeId, name: &str) -> LogicalSnapshotNode {
        LogicalSnapshotNode::new(
            NodeId::new(),
            Some(root),
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

    fn feed_page(
        scope: ReplicaScope,
        root: NodeId,
        name: &str,
        sequence: u64,
        has_more: bool,
    ) -> RemoteFeedPage {
        let desired = remote_child(root, name);
        let event = ChangeEvent::new(
            ChangeEventId::new(),
            scope.owner_user_id(),
            scope.library_id(),
            Sequence::new(1),
            Sequence::new(sequence),
            1,
            ChangeResourceKind::Node,
            desired.node_id(),
            ChangeKind::NodeCreated,
            fixed_time(),
            desired.revision(),
            desired.parent_node_id(),
            Some(desired.kind()),
            Some(desired.state()),
            desired.current_version_id(),
        );
        RemoteFeedPage::new(
            scope,
            Sequence::new(1),
            Sequence::new(sequence.saturating_sub(1)),
            Sequence::new(sequence),
            Sequence::new(if has_more {
                sequence.saturating_add(1)
            } else {
                sequence
            }),
            has_more,
            vec![InboundChange::new(event, Some(desired))],
            Some(OpaqueEvidence::new(format!("ack-{sequence}")).unwrap()),
        )
        .unwrap()
    }

    fn renamed_feed_page(
        scope: ReplicaScope,
        root: NodeId,
        node: NodeId,
        name: &str,
        revision: u64,
    ) -> RemoteFeedPage {
        let desired = LogicalSnapshotNode::new(
            node,
            Some(root),
            LogicalName::new(name).unwrap(),
            NodeKind::Directory,
            NodeState::Active,
            Revision::new(revision),
            None,
            None,
            None,
        )
        .unwrap();
        let event = ChangeEvent::new(
            ChangeEventId::new(),
            scope.owner_user_id(),
            scope.library_id(),
            Sequence::new(1),
            Sequence::new(1),
            1,
            ChangeResourceKind::Node,
            node,
            ChangeKind::NodeRenamed,
            fixed_time(),
            desired.revision(),
            desired.parent_node_id(),
            Some(desired.kind()),
            Some(desired.state()),
            desired.current_version_id(),
        );
        RemoteFeedPage::new(
            scope,
            Sequence::new(1),
            Sequence::new(0),
            Sequence::new(1),
            Sequence::new(1),
            false,
            vec![InboundChange::new(event, Some(desired))],
            Some(OpaqueEvidence::new("ack-rename").unwrap()),
        )
        .unwrap()
    }

    fn descriptor(
        library_id: LibraryId,
        epoch: u64,
        sequence: u64,
    ) -> RebaselineSnapshotDescriptor {
        RebaselineSnapshotDescriptor::new(
            RebaselineSnapshotId::new(),
            library_id,
            crate::RebaselineBoundary::new(Sequence::new(epoch), Sequence::new(sequence)),
            1,
        )
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

    fn recovery_page(
        descriptor: RebaselineSnapshotDescriptor,
        root: NodeId,
    ) -> RebaselineSnapshotPage {
        RebaselineSnapshotPage::new(descriptor, vec![root_node(root)], None).unwrap()
    }

    fn recovery_page_with_root_revision(
        descriptor: RebaselineSnapshotDescriptor,
        root: NodeId,
        revision: u64,
    ) -> RebaselineSnapshotPage {
        let root = LogicalSnapshotNode::new(
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
        .unwrap();
        RebaselineSnapshotPage::new(descriptor, vec![root], None).unwrap()
    }

    async fn configure_recovery(
        harness: &Harness,
        root: NodeId,
        descriptor: RebaselineSnapshotDescriptor,
    ) {
        harness.remote.with_state(|state| {
            state
                .checkpoints
                .push_back(Err(RemoteError::new(RemoteErrorKind::RebaselineRequired)));
            state.creates.push_back(Ok(descriptor));
            state.pages.push_back(Ok(recovery_page(descriptor, root)));
            state
                .handoffs
                .push_back(Ok(handoff(harness.scope, descriptor)));
        });
    }

    #[test]
    fn result_interpretation_keeps_expected_states_typed() {
        let inbound =
            InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::IncrementalReady);
        let outbound = OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::NoReadyIntent);
        let result = SyncCycleResult::new(fixed_time(), inbound, outbound);
        assert!(result.is_idle());
        assert!(!result.made_durable_progress());
        assert!(!result.more_work_likely());
        assert_eq!(result.observed_at(), fixed_time());
        assert!(InboundCycleOutcome::Offline.is_transport_failure());
        assert!(!InboundCycleOutcome::RateLimited.is_transport_failure());
    }

    #[tokio::test]
    async fn cy_c1_crash_before_inbound_leaves_durable_state_unchanged() {
        let harness = Harness::new("before-inbound-crash").await;
        let root = install_root(&harness).await;
        let intent = create_directory_intent(&harness, root, "local", 1, 0).await;
        let before_replica = harness
            .state
            .replica(harness.scope.library_id())
            .await
            .unwrap()
            .unwrap();
        let before_intent_state = harness
            .state
            .outbound_intent(intent.intent_id())
            .await
            .unwrap()
            .unwrap()
            .state();

        let hooked = harness.cycle_with_cycle_hook(Arc::new(CycleOnceHook::new()));
        assert!(matches!(
            hooked.run_once(fixed_time()).await,
            Err(ClientSyncError::InjectedFailure)
        ));
        assert_eq!(harness.remote.feed_calls(), 0);
        assert_eq!(harness.remote.create_calls(), 0);
        assert_eq!(harness.remote.handoff_calls(), 0);
        assert_eq!(harness.remote.mutation_attempts(), 0);
        assert_eq!(
            harness
                .state
                .replica(harness.scope.library_id())
                .await
                .unwrap()
                .unwrap(),
            before_replica
        );
        assert_eq!(
            harness
                .state
                .outbound_intent(intent.intent_id())
                .await
                .unwrap()
                .unwrap()
                .state(),
            before_intent_state
        );
        harness.close().await;
    }

    #[tokio::test]
    async fn sc1_idle_cycle_is_one_bounded_inbound_step() {
        let harness = Harness::new("idle").await;
        install_root(&harness).await;
        let result = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert_eq!(
            result.inbound(),
            InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::IncrementalReady)
        );
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::NoReadyIntent)
        );
        assert!(result.is_idle());
        assert_eq!(harness.remote.feed_calls(), 1);
        assert_eq!(harness.remote.create_calls(), 0);
        assert_eq!(harness.remote.mutation_attempts(), 0);
        harness.close().await;
    }

    #[tokio::test]
    async fn sc2_inbound_only_advances_one_page_and_ack() {
        let harness = Harness::new("inbound-only").await;
        let root = install_root(&harness).await;
        harness.remote.with_state(|state| {
            state
                .feeds
                .push_back(Ok(feed_page(harness.scope, root, "remote", 1, false)));
            state.acknowledgements.push_back(Ok(RemoteCheckpoint::new(
                harness.scope,
                Sequence::new(1),
                Sequence::new(1),
            )));
        });
        let result = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert!(matches!(
            result.inbound(),
            InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::IncrementalProgress(
                SyncOutcome::Progressed
            ))
        ));
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::NoReadyIntent)
        );
        assert_eq!(harness.remote.feed_calls(), 1);
        assert_eq!(harness.remote.pending_feed_scripts(), 0);
        let replica = harness
            .state
            .replica(harness.scope.library_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(replica.acknowledged_sequence(), Sequence::new(1));
        harness.close().await;
    }

    #[tokio::test]
    async fn sc3_outbound_only_submits_one_intent_after_idle_inbound() {
        let harness = Harness::new("outbound-only").await;
        let root = install_root(&harness).await;
        let intent = create_directory_intent(&harness, root, "local", 1, 0).await;
        let result = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Submitted(
                intent.intent_id()
            ))
        );
        assert_eq!(harness.remote.mutation_attempts(), 1);
        assert_eq!(harness.remote.mutation_commits(), 1);
        harness.close().await;
    }

    #[tokio::test]
    async fn sc4_inbound_and_outbound_both_progress_without_an_outer_loop() {
        let harness = Harness::new("combined").await;
        let root = install_root(&harness).await;
        let intent = create_directory_intent(&harness, root, "local", 1, 1).await;
        harness.remote.with_state(|state| {
            state
                .feeds
                .push_back(Ok(feed_page(harness.scope, root, "remote", 1, false)));
            state.acknowledgements.push_back(Ok(RemoteCheckpoint::new(
                harness.scope,
                Sequence::new(1),
                Sequence::new(1),
            )));
        });
        let result = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert!(result.inbound().made_durable_progress());
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Submitted(
                intent.intent_id()
            ))
        );
        assert_eq!(harness.remote.feed_calls(), 1);
        assert_eq!(harness.remote.mutation_attempts(), 1);
        harness.close().await;
    }

    #[tokio::test]
    async fn sc5_inbound_same_node_conflicts_before_outbound() {
        let harness = Harness::new("same-node-conflict").await;
        let root = install_root(&harness).await;
        let node = install_local_target(&harness, root, "target").await;
        let intent = create_rename_intent(&harness, root, node, "target", "local").await;
        harness.remote.with_state(|state| {
            state.feeds.push_back(Ok(renamed_feed_page(
                harness.scope,
                root,
                node,
                "remote",
                2,
            )));
            state.acknowledgements.push_back(Ok(RemoteCheckpoint::new(
                harness.scope,
                Sequence::new(1),
                Sequence::new(1),
            )));
        });
        let result = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert_eq!(
            result.inbound(),
            InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::IncrementalProgress(
                SyncOutcome::Blocked
            ))
        );
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::NotAttempted(OutboundSkipReason::IncrementalNotReady(
                SyncOutcome::Blocked
            ))
        );
        assert_eq!(harness.remote.mutation_attempts(), 0);
        assert_eq!(
            harness
                .state
                .outbound_intent(intent.intent_id())
                .await
                .unwrap()
                .unwrap()
                .state(),
            crate::OutboundIntentState::NeedsRebaseValidation
        );
        assert_eq!(
            harness
                .state
                .observation_issues(harness.scope.library_id())
                .await
                .unwrap()
                .len(),
            1
        );
        harness.close().await;
    }

    #[tokio::test]
    async fn sc6_existing_conflict_does_not_stop_inbound_and_fences_outbound() {
        let harness = Harness::new("existing-conflict").await;
        let root = install_root(&harness).await;
        let intent = create_directory_intent(&harness, root, "local", 1, 0).await;
        let conflict = RemoteMutationConflict::with_evidence(
            synveil_core::SyncConflictId::new(),
            "NAME_COLLISION",
            false,
            root,
            Some(Revision::new(1)),
            Some(Revision::new(2)),
            Some(NodeState::Active),
            None,
            Sequence::new(1),
            Sequence::new(0),
        )
        .unwrap();
        harness
            .state
            .record_mutation_conflict(
                intent.intent_id(),
                synveil_core::ClientMutationId::new(),
                &conflict,
            )
            .await
            .unwrap();
        harness.remote.with_state(|state| {
            state
                .feeds
                .push_back(Ok(feed_page(harness.scope, root, "remote", 1, false)));
            state.acknowledgements.push_back(Ok(RemoteCheckpoint::new(
                harness.scope,
                Sequence::new(1),
                Sequence::new(1),
            )));
        });
        let result = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert!(result.inbound().made_durable_progress());
        assert!(matches!(
            result.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::BlockedByConflict(_))
        ));
        assert_eq!(harness.remote.mutation_attempts(), 0);
        assert_eq!(
            harness
                .state
                .list_unresolved_conflicts(harness.scope.library_id(), None, None)
                .await
                .unwrap()
                .items()
                .len(),
            1
        );
        harness.close().await;
    }

    #[tokio::test]
    async fn sc7_recovery_then_one_valid_outbound_intent() {
        let harness = Harness::new("recovery-outbound").await;
        let root = install_root(&harness).await;
        let descriptor = descriptor(harness.scope.library_id(), 2, 7);
        let intent = create_directory_intent(&harness, root, "local", 2, 7).await;
        configure_recovery(&harness, root, descriptor).await;
        let result = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert_eq!(
            result.inbound(),
            InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::RebaselineConverged {
                snapshot_id: descriptor.snapshot_id(),
                boundary: descriptor.boundary(),
            })
        );
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Submitted(
                intent.intent_id()
            ))
        );
        assert_eq!(harness.remote.create_calls(), 1);
        assert_eq!(harness.remote.mutation_attempts(), 1);
        harness.close().await;
    }

    #[tokio::test]
    async fn sc8_recovery_marks_newly_stale_outbound_intent_as_conflict() {
        let harness = Harness::new("recovery-conflict").await;
        let root = install_root(&harness).await;
        let descriptor = descriptor(harness.scope.library_id(), 2, 7);
        let _intent = create_directory_intent(&harness, root, "local", 2, 7).await;
        harness.remote.with_state(|state| {
            state
                .checkpoints
                .push_back(Err(RemoteError::new(RemoteErrorKind::RebaselineRequired)));
            state.creates.push_back(Ok(descriptor));
            state
                .pages
                .push_back(Ok(recovery_page_with_root_revision(descriptor, root, 2)));
            state
                .handoffs
                .push_back(Ok(handoff(harness.scope, descriptor)));
        });
        let result = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert!(matches!(
            result.inbound(),
            InboundCycleOutcome::Converged(
                RebaselineConvergenceOutcome::RebaselineConverged { .. }
            )
        ));
        assert!(matches!(
            result.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::BlockedByConflict(_))
        ));
        assert_eq!(harness.remote.create_calls(), 1);
        assert_eq!(harness.remote.mutation_attempts(), 0);
        assert_eq!(
            harness
                .state
                .list_unresolved_conflicts(harness.scope.library_id(), None, None)
                .await
                .unwrap()
                .items()
                .len(),
            1
        );
        harness.close().await;
    }

    #[tokio::test]
    async fn sc9_proof_loss_recovery_preserves_outbound_work() {
        let harness = Harness::new("proof-loss-outbound").await;
        let root = install_root(&harness).await;
        let first = descriptor(harness.scope.library_id(), 2, 5);
        let second = descriptor(harness.scope.library_id(), 2, 7);
        let intent = create_directory_intent(&harness, root, "local", 2, 7).await;

        harness
            .state
            .begin_rebaseline_candidate(harness.scope, first)
            .await
            .unwrap();
        harness
            .state
            .persist_rebaseline_page(harness.scope, first, &recovery_page(first, root))
            .await
            .unwrap();
        harness
            .state
            .activate_rebaseline_candidate(harness.scope, first)
            .await
            .unwrap();
        harness.remote.with_state(|state| {
            state
                .handoffs
                .push_back(Err(RemoteError::new(RemoteErrorKind::NotFound)));
            state.creates.push_back(Ok(second));
            state.pages.push_back(Ok(recovery_page(second, root)));
            state.handoffs.push_back(Ok(handoff(harness.scope, second)));
        });

        let result = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert!(matches!(
            result.inbound(),
            InboundCycleOutcome::Converged(
                RebaselineConvergenceOutcome::RebaselineConverged {
                    snapshot_id,
                    boundary,
                }
            ) if snapshot_id == second.snapshot_id() && boundary == second.boundary()
        ));
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Submitted(
                intent.intent_id()
            ))
        );
        assert_eq!(harness.remote.create_calls(), 1);
        assert_eq!(harness.remote.handoff_calls(), 2);
        assert_eq!(harness.remote.mutation_attempts(), 1);
        assert!(
            harness
                .state
                .pending_rebaseline_handoff(harness.scope.library_id())
                .await
                .unwrap()
                .is_none()
        );
        harness.close().await;
    }

    #[tokio::test]
    async fn cy_c3_recovery_completion_before_outbound_is_restart_safe() {
        let harness = Harness::new("recovery-crash-restart").await;
        let root = install_root(&harness).await;
        let descriptor = descriptor(harness.scope.library_id(), 2, 7);
        let intent = create_directory_intent(&harness, root, "local", 2, 7).await;
        configure_recovery(&harness, root, descriptor).await;

        // Recovery has completed and installed its server-proved handoff, but
        // the caller ends before it reaches the outbound phase.
        assert_eq!(
            harness.convergence.run_convergence_once().await.unwrap(),
            RebaselineConvergenceOutcome::RebaselineConverged {
                snapshot_id: descriptor.snapshot_id(),
                boundary: descriptor.boundary(),
            }
        );
        assert_eq!(harness.remote.create_calls(), 1);
        assert_eq!(harness.remote.handoff_calls(), 1);
        assert_eq!(harness.remote.mutation_attempts(), 0);

        let restarted = Arc::new(
            BidirectionalSyncCycleRunner::new(
                harness.convergence.clone(),
                harness.outbound.clone(),
            )
            .unwrap(),
        );
        let result = restarted.run_once(fixed_time()).await.unwrap();
        assert_eq!(
            result.inbound(),
            InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::IncrementalReady)
        );
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Submitted(
                intent.intent_id(),
            ))
        );
        assert_eq!(harness.remote.create_calls(), 1);
        assert_eq!(harness.remote.handoff_calls(), 1);
        assert_eq!(harness.remote.mutation_attempts(), 1);
        harness.close().await;
    }

    #[tokio::test]
    async fn sc10_candidate_and_sc11_handoff_precede_outbound() {
        let harness = Harness::new("candidate-handoff").await;
        let root = install_root(&harness).await;
        let descriptor = descriptor(harness.scope.library_id(), 2, 7);
        let intent = create_directory_intent(&harness, root, "local", 2, 7).await;
        harness
            .state
            .begin_rebaseline_candidate(harness.scope, descriptor)
            .await
            .unwrap();
        harness
            .state
            .persist_rebaseline_page(harness.scope, descriptor, &recovery_page(descriptor, root))
            .await
            .unwrap();
        harness.remote.with_state(|state| {
            state
                .handoffs
                .push_back(Ok(handoff(harness.scope, descriptor)))
        });
        let first = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert!(matches!(
            first.inbound(),
            InboundCycleOutcome::Converged(
                RebaselineConvergenceOutcome::RebaselineConverged { .. }
            )
        ));
        assert_eq!(
            first.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Submitted(
                intent.intent_id()
            ))
        );
        assert_eq!(harness.remote.create_calls(), 0);
        assert_eq!(harness.remote.mutation_attempts(), 1);
        assert!(
            harness
                .state
                .rebaseline_candidate(harness.scope.library_id())
                .await
                .unwrap()
                .is_none()
        );
        harness.close().await;
    }

    #[tokio::test]
    async fn sc11_pending_handoff_finalizes_before_outbound() {
        let harness = Harness::new("handoff-outbound").await;
        let root = install_root(&harness).await;
        let descriptor = descriptor(harness.scope.library_id(), 2, 7);
        let intent = create_directory_intent(&harness, root, "local", 2, 7).await;
        harness
            .state
            .begin_rebaseline_candidate(harness.scope, descriptor)
            .await
            .unwrap();
        harness
            .state
            .persist_rebaseline_page(harness.scope, descriptor, &recovery_page(descriptor, root))
            .await
            .unwrap();
        harness
            .state
            .activate_rebaseline_candidate(harness.scope, descriptor)
            .await
            .unwrap();
        harness.remote.with_state(|state| {
            state
                .handoffs
                .push_back(Ok(handoff(harness.scope, descriptor)))
        });

        let result = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert!(matches!(
            result.inbound(),
            InboundCycleOutcome::Converged(
                RebaselineConvergenceOutcome::RebaselineConverged { .. }
            )
        ));
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Submitted(
                intent.intent_id()
            ))
        );
        assert_eq!(harness.remote.create_calls(), 0);
        assert_eq!(harness.remote.handoff_calls(), 1);
        assert_eq!(harness.remote.mutation_attempts(), 1);
        harness.close().await;
    }

    #[tokio::test]
    async fn sc12_rate_limited_recovery_skips_outbound_without_retry() {
        let harness = Harness::new("rate-limited").await;
        let root = install_root(&harness).await;
        let _intent = create_directory_intent(&harness, root, "local", 1, 0).await;
        harness.remote.with_state(|state| {
            state
                .checkpoints
                .push_back(Err(RemoteError::new(RemoteErrorKind::RebaselineRequired)));
            state
                .creates
                .push_back(Err(RemoteError::new(RemoteErrorKind::RateLimited)));
        });
        let result = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::NotAttempted(OutboundSkipReason::RecoveryBlocked(
                RebaselineRecoveryBlockedReason::RateLimited
            ))
        );
        assert_eq!(harness.remote.create_calls(), 1);
        assert_eq!(harness.remote.mutation_attempts(), 0);
        assert!(
            harness
                .state
                .rebaseline_candidate(harness.scope.library_id())
                .await
                .unwrap()
                .is_none()
        );
        harness.close().await;
    }

    #[tokio::test]
    async fn sc13_auth_failure_makes_zero_outbound_requests() {
        let harness = Harness::new("auth").await;
        let root = install_root(&harness).await;
        let _intent = create_directory_intent(&harness, root, "local", 1, 0).await;
        harness.remote.with_state(|state| {
            state
                .checkpoints
                .push_back(Err(RemoteError::new(RemoteErrorKind::AuthRequired)))
        });
        let result = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert_eq!(result.inbound(), InboundCycleOutcome::AuthRequired);
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::NotAttempted(OutboundSkipReason::InboundAuthenticationRequired)
        );
        assert_eq!(harness.remote.mutation_attempts(), 0);
        harness.close().await;
    }

    #[tokio::test]
    async fn sc14_transport_failure_skips_outbound_conservatively() {
        let harness = Harness::new("offline").await;
        let root = install_root(&harness).await;
        let _intent = create_directory_intent(&harness, root, "local", 1, 0).await;
        harness.remote.with_state(|state| {
            state
                .checkpoints
                .push_back(Err(RemoteError::new(RemoteErrorKind::Timeout)))
        });
        let result = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert_eq!(result.inbound(), InboundCycleOutcome::Offline);
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::NotAttempted(OutboundSkipReason::InboundOffline)
        );
        assert_eq!(harness.remote.mutation_attempts(), 0);
        harness.close().await;
    }

    #[tokio::test]
    async fn sc15_snapshot_create_429_is_typed_and_skips_outbound() {
        let harness = Harness::new("snapshot-rate-limited").await;
        let root = install_root(&harness).await;
        let _intent = create_directory_intent(&harness, root, "local", 1, 0).await;
        harness.remote.with_state(|state| {
            state
                .checkpoints
                .push_back(Err(RemoteError::new(RemoteErrorKind::RebaselineRequired)));
            state
                .creates
                .push_back(Err(RemoteError::new(RemoteErrorKind::RateLimited)));
        });

        let result = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert_eq!(
            result.inbound(),
            InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::RecoveryBlocked {
                reason: RebaselineRecoveryBlockedReason::RateLimited,
            })
        );
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::NotAttempted(OutboundSkipReason::RecoveryBlocked(
                RebaselineRecoveryBlockedReason::RateLimited,
            ))
        );
        assert_eq!(harness.remote.create_calls(), 1);
        assert_eq!(harness.remote.mutation_attempts(), 0);
        harness.close().await;
    }

    #[tokio::test]
    async fn sc16_conflict_is_durable_and_not_retried_on_the_next_cycle() {
        let harness = Harness::new("conflict-restart").await;
        let root = install_root(&harness).await;
        let intent = create_directory_intent(&harness, root, "local", 1, 0).await;
        let conflict = RemoteMutationConflict::with_evidence(
            synveil_core::SyncConflictId::new(),
            "NAME_COLLISION",
            false,
            root,
            Some(Revision::new(1)),
            Some(Revision::new(2)),
            Some(NodeState::Active),
            None,
            Sequence::new(1),
            Sequence::new(0),
        )
        .unwrap();
        harness.remote.with_state(|state| {
            state
                .mutations
                .push_back(Ok(RemoteMutationOutcome::Conflict(conflict)))
        });
        let first = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert!(matches!(
            first.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Conflict { .. })
        ));
        let second = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert!(matches!(
            second.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::BlockedByConflict(_))
        ));
        assert_eq!(harness.remote.mutation_attempts(), 1);
        assert_eq!(
            harness
                .state
                .outbound_intent(intent.intent_id())
                .await
                .unwrap()
                .unwrap()
                .state(),
            crate::OutboundIntentState::Conflict
        );
        harness.close().await;
    }

    #[tokio::test]
    async fn sc17_same_library_callers_share_existing_guards_without_duplicate_submit() {
        let harness = Harness::new("same-library").await;
        let root = install_root(&harness).await;
        let intent = create_directory_intent(&harness, root, "local", 1, 0).await;
        let (first, second) = tokio::join!(
            harness.cycle.run_once(fixed_time()),
            harness.cycle.run_once(fixed_time())
        );
        let first = first.unwrap();
        let second = second.unwrap();
        assert!(
            [first.outbound(), second.outbound()]
                .into_iter()
                .filter(|outcome| matches!(
                    outcome,
                    OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Submitted(id))
                        if *id == intent.intent_id()
                ))
                .count()
                <= 1
        );
        assert_eq!(harness.remote.mutation_commits(), 1);
        assert_eq!(harness.remote.mutation_attempts(), 1);
        harness.close().await;
    }

    #[tokio::test]
    async fn sc18_different_libraries_use_independent_cycle_state() {
        let first = Harness::new("library-a").await;
        let second = Harness::new("library-b").await;
        let root_a = install_root(&first).await;
        let root_b = install_root(&second).await;
        let intent_a = create_directory_intent(&first, root_a, "local-a", 1, 0).await;
        let intent_b = create_directory_intent(&second, root_b, "local-b", 1, 0).await;
        let (result_a, result_b) = tokio::join!(
            first.cycle.run_once(fixed_time()),
            second.cycle.run_once(fixed_time())
        );
        assert_eq!(
            result_a.unwrap().outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Submitted(
                intent_a.intent_id()
            ))
        );
        assert_eq!(
            result_b.unwrap().outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Submitted(
                intent_b.intent_id()
            ))
        );
        assert_eq!(first.remote.mutation_commits(), 1);
        assert_eq!(second.remote.mutation_commits(), 1);
        first.close().await;
        second.close().await;
    }

    #[tokio::test]
    async fn sc19_crash_after_inbound_before_outbound_restarts_from_durable_state() {
        let harness = Harness::new("inbound-crash-restart").await;
        let root = install_root(&harness).await;
        let intent = create_directory_intent(&harness, root, "local", 1, 1).await;
        harness.remote.with_state(|state| {
            state
                .feeds
                .push_back(Ok(feed_page(harness.scope, root, "remote", 1, false)));
            state.acknowledgements.push_back(Ok(RemoteCheckpoint::new(
                harness.scope,
                Sequence::new(1),
                Sequence::new(1),
            )));
        });

        // The caller ends after the inbound phase. All state needed by the
        // next invocation is already durable in the local store.
        assert_eq!(
            harness.convergence.run_convergence_once().await.unwrap(),
            RebaselineConvergenceOutcome::IncrementalProgress(SyncOutcome::Progressed)
        );
        assert_eq!(harness.remote.mutation_attempts(), 0);

        let restarted = Arc::new(
            BidirectionalSyncCycleRunner::new(
                harness.convergence.clone(),
                harness.outbound.clone(),
            )
            .unwrap(),
        );
        let result = restarted.run_once(fixed_time()).await.unwrap();
        assert_eq!(
            result.inbound(),
            InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::IncrementalReady)
        );
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Submitted(
                intent.intent_id(),
            ))
        );
        assert_eq!(harness.remote.feed_calls(), 2);
        assert_eq!(harness.remote.mutation_attempts(), 1);
        assert_eq!(harness.remote.mutation_commits(), 1);
        harness.close().await;
    }

    #[tokio::test]
    async fn sc20_response_loss_replays_durable_mutation_without_duplicate_commit() {
        let harness = Harness::new("response-loss").await;
        let root = install_root(&harness).await;
        let intent = create_directory_intent(&harness, root, "local", 1, 0).await;
        harness.remote.set_failure_after_commit();
        let first = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert_eq!(
            first.inbound(),
            InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::IncrementalReady)
        );
        assert_eq!(
            first.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Offline)
        );
        let second = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert_eq!(
            second.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Submitted(
                intent.intent_id()
            ))
        );
        assert_eq!(harness.remote.mutation_commits(), 1);
        assert_eq!(harness.remote.mutation_attempts(), 2);
        harness.close().await;
    }

    #[tokio::test]
    async fn sc21_crash_after_conflict_persistence_keeps_the_outbound_fence() {
        let harness = Harness::new("conflict-crash-restart").await;
        let root = install_root(&harness).await;
        let intent = create_directory_intent(&harness, root, "local", 1, 0).await;
        let conflict = RemoteMutationConflict::with_evidence(
            synveil_core::SyncConflictId::new(),
            "NAME_COLLISION",
            false,
            root,
            Some(Revision::new(1)),
            Some(Revision::new(2)),
            Some(NodeState::Active),
            None,
            Sequence::new(1),
            Sequence::new(0),
        )
        .unwrap();
        harness.remote.with_state(|state| {
            state
                .mutations
                .push_back(Ok(RemoteMutationOutcome::Conflict(conflict)))
        });

        let first = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert!(matches!(
            first.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Conflict { .. })
        ));

        // The process dies after the conflict transaction commits. A fresh
        // runner must observe the durable fence and make no second POST.
        let restarted = Arc::new(
            BidirectionalSyncCycleRunner::new(
                harness.convergence.clone(),
                harness.outbound.clone(),
            )
            .unwrap(),
        );
        let second = restarted.run_once(fixed_time()).await.unwrap();
        assert!(matches!(
            second.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::BlockedByConflict(_))
        ));
        assert_eq!(harness.remote.mutation_attempts(), 1);
        assert_eq!(
            harness
                .state
                .outbound_intent(intent.intent_id())
                .await
                .unwrap()
                .unwrap()
                .state(),
            crate::OutboundIntentState::Conflict
        );
        assert_eq!(
            harness
                .state
                .list_unresolved_conflicts(harness.scope.library_id(), None, None)
                .await
                .unwrap()
                .items()
                .len(),
            1
        );
        harness.close().await;
    }

    #[tokio::test]
    async fn cy_c6_server_applied_transition_is_not_resubmitted_after_local_crash() {
        let harness = Harness::new("server-applied-crash").await;
        let root = install_root(&harness).await;
        let intent = create_directory_intent(&harness, root, "local", 1, 0).await;
        let hooked = harness
            .cycle_with_outbound_hook(Arc::new(OnceHook::new(
                OutboundFailurePoint::AfterMutationServerAppliedPersistedBeforeFeed,
            )))
            .await;

        assert!(matches!(
            hooked.run_once(fixed_time()).await,
            Err(ClientSyncError::InjectedFailure)
        ));
        assert_eq!(harness.remote.mutation_attempts(), 1);
        assert_eq!(harness.remote.mutation_commits(), 1);
        assert_eq!(
            harness
                .state
                .outbound_intent(intent.intent_id())
                .await
                .unwrap()
                .unwrap()
                .state(),
            crate::OutboundIntentState::ServerApplied
        );

        let result = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::NoReadyIntent)
        );
        assert_eq!(harness.remote.mutation_attempts(), 1);
        assert_eq!(harness.remote.mutation_commits(), 1);
        harness.close().await;
    }

    #[tokio::test]
    async fn sc22_one_cycle_does_not_drain_100_intents_or_two_feed_pages() {
        let harness = Harness::new("bounds").await;
        let root = install_root(&harness).await;
        for index in 0..100 {
            let _ = create_directory_intent(&harness, root, &format!("local-{index}"), 1, 0).await;
        }
        let result = harness.cycle.run_once(fixed_time()).await.unwrap();
        assert!(matches!(
            result.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Submitted(_))
        ));
        assert_eq!(harness.remote.mutation_attempts(), 1);
        assert_eq!(
            harness
                .state
                .list_pending_intents(harness.scope.library_id())
                .await
                .unwrap()
                .len(),
            99
        );

        let page_one = feed_page(harness.scope, root, "page-one", 1, true);
        let page_two = feed_page(harness.scope, root, "page-two", 2, false);
        // The first cycle above already advanced local sequence 0 only through
        // outbound, so install a separate fixture for the ordinary feed bound.
        let feed_harness = Harness::new("feed-bound").await;
        let feed_root = install_root(&feed_harness).await;
        feed_harness.remote.with_state(|state| {
            state.feeds.push_back(Ok(feed_page(
                feed_harness.scope,
                feed_root,
                "page-one",
                1,
                true,
            )));
            state.feeds.push_back(Ok(feed_page(
                feed_harness.scope,
                feed_root,
                "page-two",
                2,
                false,
            )));
            state.acknowledgements.push_back(Ok(RemoteCheckpoint::new(
                feed_harness.scope,
                Sequence::new(1),
                Sequence::new(1),
            )));
        });
        let _ = feed_harness.cycle.run_once(fixed_time()).await.unwrap();
        assert_eq!(feed_harness.remote.feed_calls(), 1);
        assert_eq!(feed_harness.remote.pending_feed_scripts(), 1);
        let _ = (page_one, page_two);
        harness.close().await;
        feed_harness.close().await;
    }
}
