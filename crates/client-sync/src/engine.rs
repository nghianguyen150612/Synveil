use std::{collections::BTreeSet, sync::Arc};

use synveil_core::{
    ChangeKind, LogicalSnapshotNode, NodeId, NodeKind, NodeState, RebaselineSnapshotId, Sequence,
    SyncBootstrapState,
};
use uuid::Uuid;

use crate::state::{RebaselineHandoffFinalizeOutcome, RebaselineHandoffRecord, StoredChange};
use crate::{
    BootstrapRecord, ClientSyncError, EngineStatus, LocalFingerprint, LocalIssueKind, LocalNode,
    LocalObjectKind, LocalOperation, LocalOperationKind, LocalOperationState, LocalReplica,
    LocalStateStore, ManagedRelativePath, RebaselineHandoffOutcome, RecoveryClassification,
    RemoteCheckpoint, RemoteError, RemoteErrorKind, ReplicaScope, ServerProfileId, SyncRemote,
    local_collision_key, validate_logical_name,
};

const MAX_BOOTSTRAP_ITEMS: u64 = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EngineConfig {
    feed_page_limit: u32,
    bootstrap_page_limit: u32,
}

impl EngineConfig {
    pub fn new(feed_page_limit: u32, bootstrap_page_limit: u32) -> Result<Self, ClientSyncError> {
        if !(1..=1_000).contains(&feed_page_limit) || !(1..=1_000).contains(&bootstrap_page_limit) {
            return Err(ClientSyncError::ResourceLimit);
        }
        Ok(Self {
            feed_page_limit,
            bootstrap_page_limit,
        })
    }

    #[must_use]
    pub const fn feed_page_limit(self) -> u32 {
        self.feed_page_limit
    }

    #[must_use]
    pub const fn bootstrap_page_limit(self) -> u32 {
        self.bootstrap_page_limit
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            feed_page_limit: 256,
            bootstrap_page_limit: 256,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncOutcome {
    Idle,
    Progressed,
    MoreAvailable,
    BootstrapRequired,
    /// The server has authoritatively rejected the retained cursor/epoch for
    /// ordinary incremental processing. This outcome is surfaced only by the
    /// convergence-facing operation; legacy `synchronize_once` retains its
    /// established bootstrap behavior.
    RebaselineRequired,
    Blocked,
    Offline,
}

/// Deterministic crash boundaries used by recovery tests and embedders that
/// want to verify their process supervisor behavior without sleeps.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FailurePoint {
    AfterPageIntentPersisted,
    BeforeFilesystemAction,
    AfterContentStagedBeforeExpose,
    AfterFilesystemActionBeforeState,
    AfterLocalCommitBeforeAck,
    AfterServerAckBeforeLocalState,
    AfterBootstrapPagePersisted,
    AfterBootstrapLocalComplete,
    AfterServerBootstrapCompleteBeforeLocalState,
    AfterServerRebaselineHandoffBeforeLocalState,
}

pub trait FailureInjector: Send + Sync {
    fn check(&self, point: FailurePoint) -> Result<(), ClientSyncError>;
}

#[derive(Debug, Default)]
pub struct NoopFailureInjector;

impl FailureInjector for NoopFailureInjector {
    fn check(&self, _point: FailurePoint) -> Result<(), ClientSyncError> {
        Ok(())
    }
}

pub struct InboundSyncEngine {
    scope: ReplicaScope,
    server_profile_id: Option<ServerProfileId>,
    remote: Arc<dyn SyncRemote>,
    replica: Arc<dyn LocalReplica>,
    state: Arc<LocalStateStore>,
    config: EngineConfig,
    failures: Arc<dyn FailureInjector>,
}

impl InboundSyncEngine {
    pub async fn new(
        scope: ReplicaScope,
        remote: Arc<dyn SyncRemote>,
        replica: Arc<dyn LocalReplica>,
        state: Arc<LocalStateStore>,
        config: EngineConfig,
    ) -> Result<Self, ClientSyncError> {
        Self::with_failure_injector(
            scope,
            remote,
            replica,
            state,
            config,
            Arc::new(NoopFailureInjector),
        )
        .await
    }

    pub async fn with_failure_injector(
        scope: ReplicaScope,
        remote: Arc<dyn SyncRemote>,
        replica: Arc<dyn LocalReplica>,
        state: Arc<LocalStateStore>,
        config: EngineConfig,
        failures: Arc<dyn FailureInjector>,
    ) -> Result<Self, ClientSyncError> {
        replica.validate_root()?;
        if replica.scope() != scope {
            return Err(ClientSyncError::WrongScope);
        }
        let server_profile_id = remote.server_profile_id();
        if replica.server_profile_id() != server_profile_id {
            return Err(ClientSyncError::WrongServerProfile);
        }
        match server_profile_id {
            Some(profile_id) => {
                let enrollment = state
                    .profile_enrollment(profile_id)
                    .await?
                    .ok_or(ClientSyncError::AuthenticationRequired)?;
                if remote.device_credential_id() != Some(enrollment.credential_id()) {
                    return Err(ClientSyncError::AuthenticationRequired);
                }
                state
                    .bind_replica_to_profile(scope, replica.binding_id(), profile_id)
                    .await?;
            }
            None => {
                state.bind_replica(scope, replica.binding_id()).await?;
            }
        }
        Ok(Self {
            scope,
            server_profile_id,
            remote,
            replica,
            state,
            config,
            failures,
        })
    }

    #[must_use]
    pub const fn scope(&self) -> ReplicaScope {
        self.scope
    }

    pub(crate) fn state(&self) -> &Arc<LocalStateStore> {
        &self.state
    }

    #[must_use]
    pub const fn config(&self) -> EngineConfig {
        self.config
    }

    /// Advance at most one bounded remote page or one bounded local bootstrap
    /// batch. Calls are safe to repeat after process restart.
    pub async fn synchronize_once(&self) -> Result<SyncOutcome, ClientSyncError> {
        self.synchronize_once_with_rebaseline_policy(false).await
    }

    /// Run one bounded ordinary incremental operation while surfacing the
    /// server's canonical retained-history/epoch invalidation instead of
    /// beginning the legacy bootstrap flow. This operation makes no snapshot
    /// creation request and never clears a local cursor; Prompt 87 owns the
    /// subsequent durable-snapshot convergence decision.
    pub async fn synchronize_incremental_once(&self) -> Result<SyncOutcome, ClientSyncError> {
        self.synchronize_once_with_rebaseline_policy(true).await
    }

    async fn synchronize_once_with_rebaseline_policy(
        &self,
        surface_rebaseline_required: bool,
    ) -> Result<SyncOutcome, ClientSyncError> {
        let _guard = self
            .state
            .lock_replica_writer(self.scope.library_id())
            .await;
        match self
            .synchronize_once_inner(surface_rebaseline_required)
            .await
        {
            Ok(outcome) => Ok(outcome),
            Err(ClientSyncError::Remote(error))
                if matches!(
                    error.kind(),
                    RemoteErrorKind::Offline | RemoteErrorKind::Unavailable
                ) =>
            {
                self.state
                    .set_status(self.scope.library_id(), EngineStatus::Offline)
                    .await?;
                Ok(SyncOutcome::Offline)
            }
            Err(ClientSyncError::LocalIssue(_)) => Ok(SyncOutcome::Blocked),
            Err(ClientSyncError::ObservationIssue(_)) => Ok(SyncOutcome::Blocked),
            Err(ClientSyncError::InjectedFailure) => Err(ClientSyncError::InjectedFailure),
            Err(error) => {
                let _ = self
                    .state
                    .set_status(self.scope.library_id(), EngineStatus::Error)
                    .await;
                Err(error)
            }
        }
    }

    /// Complete the server checkpoint and local cursor handoff for the durable
    /// snapshot activated by Prompt 84. The network request is deliberately
    /// outside the local writer critical section so outbound intent creation
    /// remains available while the server transaction is in flight. Only the
    /// final local cursor/marker transition is serialized and transactional.
    pub async fn complete_rebaseline_handoff(
        &self,
        snapshot_id: RebaselineSnapshotId,
    ) -> Result<RebaselineHandoffOutcome, ClientSyncError> {
        self.validate_binding().await?;
        let Some(pending) = self
            .state
            .pending_rebaseline_handoff(self.scope.library_id())
            .await?
        else {
            return Ok(RebaselineHandoffOutcome::AlreadyComplete);
        };
        if pending.snapshot_id() != snapshot_id || pending.library_id() != self.scope.library_id() {
            return Err(ClientSyncError::HandoffResponseMismatch);
        }

        let confirmation = self
            .remote
            .complete_rebaseline_handoff(self.scope, snapshot_id)
            .await
            .map_err(map_handoff_remote_error)?;
        if confirmation.snapshot_id() != pending.snapshot_id()
            || confirmation.library_id() != self.scope.library_id()
        {
            return Err(ClientSyncError::HandoffResponseMismatch);
        }
        let checkpoint = confirmation.checkpoint();
        if checkpoint.scope() != self.scope
            || checkpoint.epoch().get() > i64::MAX as u64
            || checkpoint.acknowledged_sequence().get() > i64::MAX as u64
        {
            return Err(ClientSyncError::HandoffResponseMismatch);
        }
        if checkpoint.epoch() != pending.journal_epoch()
            || checkpoint.acknowledged_sequence() != pending.resume_sequence()
        {
            return Err(ClientSyncError::HandoffResponseMismatch);
        }
        self.failures
            .check(FailurePoint::AfterServerRebaselineHandoffBeforeLocalState)?;

        let _guard = self
            .state
            .lock_replica_writer(self.scope.library_id())
            .await;
        let expected = RebaselineHandoffRecord::new(
            pending.snapshot_id(),
            pending.library_id(),
            pending.journal_epoch(),
            pending.resume_sequence(),
        );
        match self
            .state
            .finalize_rebaseline_handoff(self.scope, expected)
            .await?
        {
            RebaselineHandoffFinalizeOutcome::Completed => Ok(RebaselineHandoffOutcome::Completed),
            RebaselineHandoffFinalizeOutcome::AlreadyComplete => {
                Ok(RebaselineHandoffOutcome::AlreadyComplete)
            }
        }
    }

    async fn synchronize_once_inner(
        &self,
        surface_rebaseline_required: bool,
    ) -> Result<SyncOutcome, ClientSyncError> {
        self.validate_binding().await?;
        // Prompt 84 deliberately leaves checkpoint handoff to Prompt 85.  An
        // old incremental cursor therefore must never be used over the newly
        // activated remote base.
        if self
            .state
            .rebaseline_handoff_pending(self.scope.library_id())
            .await?
        {
            return Err(ClientSyncError::RebaselinePendingHandoff);
        }
        if !self
            .state
            .unresolved_issues(self.scope.library_id())
            .await?
            .is_empty()
        {
            return Ok(SyncOutcome::Blocked);
        }
        self.recover_startup_inner().await?;
        self.cleanup_completed_operations().await?;

        if let Some(bootstrap) = self.state.bootstrap(self.scope.library_id()).await? {
            return self.advance_bootstrap(bootstrap).await;
        }

        let replica = self
            .state
            .replica(self.scope.library_id())
            .await?
            .ok_or(ClientSyncError::InvalidState)?;
        if replica.root_node_id().is_none() {
            // A fresh replica ordinarily starts the legacy bootstrap flow.
            // The convergence-facing entry point makes one narrow exception:
            // ask the authoritative server whether that no-cursor state is
            // still usable before creating any bootstrap state locally.  A
            // retained-floor/epoch rejection is owned by Prompt 87, whereas a
            // valid checkpoint deliberately retains the established bootstrap
            // behavior.  No local cursor is reset or inferred here.
            if surface_rebaseline_required {
                match self.remote.get_checkpoint(self.scope).await {
                    Ok(checkpoint) => validate_checkpoint(self.scope, checkpoint)?,
                    Err(error) if error.kind() == RemoteErrorKind::RebaselineRequired => {
                        return Ok(SyncOutcome::RebaselineRequired);
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            self.begin_bootstrap().await?;
            return Ok(SyncOutcome::BootstrapRequired);
        }

        if self
            .state
            .pending_ack(self.scope.library_id())
            .await?
            .is_some()
        {
            return self.retry_pending_ack().await;
        }
        if self
            .state
            .pending_page(self.scope.library_id())
            .await?
            .is_some()
        {
            return self.apply_pending_page().await;
        }

        let checkpoint = match self.remote.get_checkpoint(self.scope).await {
            Ok(checkpoint) => checkpoint,
            Err(error) if error.kind() == RemoteErrorKind::RebaselineRequired => {
                if surface_rebaseline_required {
                    return Ok(SyncOutcome::RebaselineRequired);
                }
                self.begin_bootstrap().await?;
                return Ok(SyncOutcome::BootstrapRequired);
            }
            Err(error) => return Err(error.into()),
        };
        validate_checkpoint(self.scope, checkpoint)?;
        if checkpoint.epoch() != replica.journal_epoch() {
            if surface_rebaseline_required {
                return Ok(SyncOutcome::RebaselineRequired);
            }
            self.begin_bootstrap().await?;
            return Ok(SyncOutcome::BootstrapRequired);
        }
        if checkpoint.acknowledged_sequence() != replica.acknowledged_sequence()
            || replica.applied_sequence() != replica.acknowledged_sequence()
        {
            return Err(ClientSyncError::InvalidRemoteResponse);
        }

        let page = match self
            .remote
            .fetch_changes(self.scope, self.config.feed_page_limit())
            .await
        {
            Ok(page) => page,
            Err(error) if error.kind() == RemoteErrorKind::RebaselineRequired => {
                if surface_rebaseline_required {
                    return Ok(SyncOutcome::RebaselineRequired);
                }
                self.begin_bootstrap().await?;
                return Ok(SyncOutcome::BootstrapRequired);
            }
            Err(error) => return Err(error.into()),
        };
        if page.scope() != self.scope {
            return Err(ClientSyncError::WrongScope);
        }
        if page.epoch() != replica.journal_epoch() {
            return Err(ClientSyncError::WrongEpoch);
        }
        if page.from_sequence() < replica.acknowledged_sequence() {
            return Err(ClientSyncError::SequenceRegression);
        }
        if page.from_sequence() > replica.acknowledged_sequence() {
            return Err(ClientSyncError::SequenceGap);
        }
        self.state.persist_page(&page).await?;
        self.failures
            .check(FailurePoint::AfterPageIntentPersisted)?;
        self.apply_pending_page().await
    }

    async fn validate_binding(&self) -> Result<(), ClientSyncError> {
        self.replica.validate_root()?;
        let record = self
            .state
            .replica(self.scope.library_id())
            .await?
            .ok_or(ClientSyncError::InvalidState)?;
        if record.scope() != self.scope || record.root_binding_id() != self.replica.binding_id() {
            return Err(ClientSyncError::WrongRootBinding);
        }
        if record.server_profile_id() != self.server_profile_id
            || self.replica.server_profile_id() != self.server_profile_id
            || self.remote.server_profile_id() != self.server_profile_id
        {
            return Err(ClientSyncError::WrongServerProfile);
        }
        if let Some(profile_id) = self.server_profile_id {
            let enrollment = self
                .state
                .profile_enrollment(profile_id)
                .await?
                .ok_or(ClientSyncError::AuthenticationRequired)?;
            if enrollment.forgotten_at_ms().is_some()
                || self.remote.device_credential_id() != Some(enrollment.credential_id())
            {
                return Err(ClientSyncError::AuthenticationRequired);
            }
            if enrollment.owner_user_id() != self.scope.owner_user_id()
                || enrollment.device_id() != self.scope.device_id()
            {
                return Err(ClientSyncError::WrongScope);
            }
        }
        Ok(())
    }

    async fn begin_bootstrap(&self) -> Result<(), ClientSyncError> {
        let bootstrap = self.remote.start_rebaseline(self.scope).await?;
        if bootstrap.owner_user_id() != self.scope.owner_user_id()
            || bootstrap.device_id() != self.scope.device_id()
            || bootstrap.library_id() != self.scope.library_id()
            || bootstrap.state() != SyncBootstrapState::Open
            || bootstrap.manifest_item_count() == 0
            || bootstrap.manifest_item_count() > MAX_BOOTSTRAP_ITEMS
        {
            return Err(ClientSyncError::InvalidRemoteResponse);
        }
        self.state.begin_bootstrap(self.scope, bootstrap).await
    }

    async fn advance_bootstrap(
        &self,
        record: BootstrapRecord,
    ) -> Result<SyncOutcome, ClientSyncError> {
        match record.state() {
            "FETCHING" => {
                let page = self
                    .remote
                    .fetch_rebaseline_page(
                        self.scope,
                        record.bootstrap_id(),
                        record.next_cursor(),
                        self.config.bootstrap_page_limit(),
                    )
                    .await?;
                let returned = page.bootstrap();
                if returned.id() != record.bootstrap_id()
                    || returned.owner_user_id() != self.scope.owner_user_id()
                    || returned.device_id() != self.scope.device_id()
                    || returned.library_id() != self.scope.library_id()
                    || returned.generation() != record.generation()
                    || returned.snapshot_epoch() != record.snapshot_epoch()
                    || returned.snapshot_resume_sequence() != record.resume_sequence()
                    || returned.manifest_item_count() != record.manifest_item_count()
                    || returned.state() != SyncBootstrapState::Open
                {
                    return Err(ClientSyncError::InvalidRemoteResponse);
                }
                self.state.persist_bootstrap_page(self.scope, &page).await?;
                self.failures
                    .check(FailurePoint::AfterBootstrapPagePersisted)?;
                Ok(SyncOutcome::MoreAvailable)
            }
            "MANIFEST_DURABLE" | "APPLYING" => {
                let desired = self
                    .state
                    .unapplied_bootstrap_nodes(self.scope.library_id(), record.bootstrap_id())
                    .await?;
                if !desired.is_empty() {
                    for node in desired {
                        let fact = ApplyFact::Bootstrap(record.generation());
                        let applied = self.apply_desired_node(&node, fact, None).await?;
                        self.state
                            .record_bootstrap_node_applied(
                                &applied.node,
                                record.bootstrap_id(),
                                applied.operation_id,
                            )
                            .await?;
                    }
                    return Ok(SyncOutcome::MoreAvailable);
                }

                let sweep = self
                    .state
                    .bootstrap_sweep_candidates(self.scope.library_id(), record.generation())
                    .await?;
                if !sweep.is_empty() {
                    for node in sweep {
                        self.sweep_absent_bootstrap_node(&node, record.generation())
                            .await?;
                    }
                    return Ok(SyncOutcome::MoreAvailable);
                }

                self.state
                    .mark_bootstrap_local_complete(self.scope.library_id(), record.bootstrap_id())
                    .await?;
                self.failures
                    .check(FailurePoint::AfterBootstrapLocalComplete)?;
                let current = self
                    .state
                    .bootstrap(self.scope.library_id())
                    .await?
                    .ok_or(ClientSyncError::InvalidState)?;
                self.complete_bootstrap(&current).await
            }
            "COMPLETION_PENDING" | "LOCAL_COMPLETE" => self.complete_bootstrap(&record).await,
            _ => Err(ClientSyncError::InvalidState),
        }
    }

    async fn complete_bootstrap(
        &self,
        record: &BootstrapRecord,
    ) -> Result<SyncOutcome, ClientSyncError> {
        let evidence = record
            .completion_evidence()
            .ok_or(ClientSyncError::InvalidState)?;
        let completion = self
            .remote
            .complete_rebaseline(self.scope, record.bootstrap_id(), evidence)
            .await?;
        if completion.bootstrap_id() != record.bootstrap_id() {
            return Err(ClientSyncError::InvalidRemoteResponse);
        }
        let checkpoint = completion.checkpoint();
        validate_checkpoint(self.scope, checkpoint)?;
        if checkpoint.epoch() != record.snapshot_epoch()
            || checkpoint.acknowledged_sequence() != record.resume_sequence()
        {
            return Err(ClientSyncError::InvalidRemoteResponse);
        }
        self.failures
            .check(FailurePoint::AfterServerBootstrapCompleteBeforeLocalState)?;
        self.state
            .complete_bootstrap(
                self.scope,
                record.bootstrap_id(),
                checkpoint.epoch(),
                checkpoint.acknowledged_sequence(),
            )
            .await?;
        Ok(SyncOutcome::Progressed)
    }

    async fn apply_pending_page(&self) -> Result<SyncOutcome, ClientSyncError> {
        let page = self
            .state
            .pending_page(self.scope.library_id())
            .await?
            .ok_or(ClientSyncError::InvalidState)?;
        let replica = self
            .state
            .replica(self.scope.library_id())
            .await?
            .ok_or(ClientSyncError::InvalidState)?;
        if page.epoch != replica.journal_epoch() {
            return Err(ClientSyncError::WrongEpoch);
        }
        if page.from_sequence != replica.acknowledged_sequence()
            || page.high_watermark < page.through_sequence
            || (page.changes.is_empty() != page.ack_evidence.is_none())
        {
            return Err(ClientSyncError::InvalidRemoteResponse);
        }
        self.state
            .mark_page_applying(self.scope.library_id())
            .await?;
        for change in &page.changes {
            if change.sequence <= replica.applied_sequence() {
                self.state
                    .record_event_applied(self.scope, page.epoch, change, None, None)
                    .await?;
                continue;
            }
            match change.change_kind {
                ChangeKind::NodePurged => {
                    self.apply_purge(change, page.epoch).await?;
                }
                ChangeKind::NodeCreated
                | ChangeKind::NodeRenamed
                | ChangeKind::NodeMoved
                | ChangeKind::NodeTrashed
                | ChangeKind::NodeRestored
                | ChangeKind::FileContentCommitted
                | ChangeKind::FileVersionRestored => {
                    let desired = change
                        .desired_node
                        .as_ref()
                        .ok_or(ClientSyncError::InvalidRemoteResponse)?;
                    let applied = self
                        .apply_desired_node(
                            desired,
                            ApplyFact::Feed(change.sequence),
                            Some(change.change_kind),
                        )
                        .await?;
                    self.state
                        .record_event_applied(
                            self.scope,
                            page.epoch,
                            change,
                            Some(&applied.node),
                            applied.operation_id,
                        )
                        .await?;
                }
            }
        }
        self.failures
            .check(FailurePoint::AfterLocalCommitBeforeAck)?;
        self.state
            .reconcile_server_applied_intents(self.scope.library_id())
            .await?;
        let pending = self
            .state
            .mark_page_locally_committed(self.scope.library_id())
            .await?;
        if pending.is_none() {
            self.state
                .set_status(self.scope.library_id(), EngineStatus::Idle)
                .await?;
            return Ok(if page.has_more {
                SyncOutcome::MoreAvailable
            } else {
                SyncOutcome::Idle
            });
        }
        self.retry_pending_ack().await.map(|_| {
            if page.has_more {
                SyncOutcome::MoreAvailable
            } else {
                SyncOutcome::Progressed
            }
        })
    }

    async fn retry_pending_ack(&self) -> Result<SyncOutcome, ClientSyncError> {
        let pending = self
            .state
            .pending_ack(self.scope.library_id())
            .await?
            .ok_or(ClientSyncError::InvalidState)?;
        if pending.library_id() != self.scope.library_id() {
            return Err(ClientSyncError::WrongScope);
        }
        self.state
            .set_status(self.scope.library_id(), EngineStatus::AckPending)
            .await?;
        let checkpoint = self
            .remote
            .acknowledge_changes(self.scope, pending.evidence())
            .await?;
        validate_checkpoint(self.scope, checkpoint)?;
        if checkpoint.epoch() != pending.epoch() {
            return Err(ClientSyncError::WrongEpoch);
        }
        self.failures
            .check(FailurePoint::AfterServerAckBeforeLocalState)?;
        self.state
            .confirm_ack(
                self.scope,
                checkpoint.epoch(),
                checkpoint.acknowledged_sequence(),
            )
            .await?;
        self.cleanup_completed_operations().await?;
        Ok(SyncOutcome::Progressed)
    }

    async fn cleanup_completed_operations(&self) -> Result<(), ClientSyncError> {
        for operation in self
            .state
            .completed_operations(self.scope.library_id())
            .await?
        {
            self.replica
                .remove_operation_receipt(operation.operation_id())?;
            self.state
                .delete_completed_operation(operation.operation_id())
                .await?;
        }
        Ok(())
    }

    async fn apply_desired_node(
        &self,
        desired: &LogicalSnapshotNode,
        fact: ApplyFact,
        event_kind: Option<ChangeKind>,
    ) -> Result<AppliedNode, ClientSyncError> {
        if self
            .state
            .mark_outbound_intents_needing_rebase(self.scope.library_id(), Some(desired.node_id()))
            .await?
        {
            return Err(ClientSyncError::ObservationIssue(
                crate::ObservationIssueKind::BaseStateChanged,
            ));
        }
        match self
            .apply_desired_node_inner(desired, fact, event_kind)
            .await
        {
            Err(ClientSyncError::LocalIo) => Err(self
                .block(
                    LocalIssueKind::LocalIoUnavailable,
                    Some(desired.node_id()),
                    fact,
                    "local filesystem operation must become available before retry",
                )
                .await),
            result => result,
        }
    }

    async fn apply_desired_node_inner(
        &self,
        desired: &LogicalSnapshotNode,
        fact: ApplyFact,
        event_kind: Option<ChangeKind>,
    ) -> Result<AppliedNode, ClientSyncError> {
        let target = self.desired_path(desired, fact).await?;
        let existing = self
            .state
            .local_node(self.scope.library_id(), desired.node_id())
            .await?;
        self.check_collision(desired.node_id(), &target, fact)
            .await?;
        let self_originated = if let Some(sequence) = fact.server_sequence()
            && let Some(result) = self
                .state
                .outbound_result_for_sequence(self.scope.library_id(), sequence)
                .await?
        {
            result.matches_desired(desired)
                && match existing.as_ref() {
                    Some(existing) if existing.present() && existing.kind() == desired.kind() => {
                        self.self_originated_filesystem_matches(existing, desired, &target)?
                    }
                    // A locally observed create has no local-node mapping until
                    // this feed event assigns the server's Node ID. The durable
                    // mutation result and the exact filesystem fingerprint are
                    // the attribution proof for that first mapping.
                    None => self.self_originated_create_matches(desired, &target)?,
                    _ => false,
                }
        } else {
            false
        };
        if self_originated {
            return Ok(AppliedNode {
                node: self.local_node_from_desired(
                    desired,
                    target,
                    fact,
                    true,
                    None,
                    existing.as_ref(),
                ),
                operation_id: None,
            });
        }

        if desired.parent_node_id().is_none() {
            if desired.kind() != NodeKind::Directory || desired.state() != NodeState::Active {
                return Err(ClientSyncError::InvalidRemoteResponse);
            }
            self.replica.ensure_directory(&target)?;
            return Ok(AppliedNode {
                node: self.local_node_from_desired(desired, target, fact, true, None, None),
                operation_id: None,
            });
        }

        if desired.state() == NodeState::Trashed {
            return self
                .apply_trashed(desired, target, existing.as_ref(), fact)
                .await;
        }
        if desired.state() != NodeState::Active {
            return Err(ClientSyncError::InvalidRemoteResponse);
        }

        if let Some(existing) = existing.as_ref()
            && existing.kind() != desired.kind()
        {
            return Err(self
                .block(
                    LocalIssueKind::LocalTypeMismatch,
                    Some(desired.node_id()),
                    fact,
                    "tracked object type must match the server Node type",
                )
                .await);
        }

        let operation_kind = self
            .active_operation_kind(desired, existing.as_ref(), &target, event_kind)
            .await?;
        let Some(operation_kind) = operation_kind else {
            let existing = existing.as_ref().ok_or(ClientSyncError::InvalidState)?;
            self.verify_clean(existing, false, fact).await?;
            return Ok(AppliedNode {
                node: self.local_node_from_desired(
                    desired,
                    target,
                    fact,
                    true,
                    None,
                    Some(existing),
                ),
                operation_id: None,
            });
        };

        let recovered_operation = self
            .state
            .operation_for_fact(
                self.scope.library_id(),
                desired.node_id(),
                fact.server_sequence(),
                fact.bootstrap_generation(),
            )
            .await?;
        let filesystem_already_applied = recovered_operation
            .as_ref()
            .is_some_and(|operation| operation.state() == LocalOperationState::FilesystemApplied);
        if let Some(operation) = recovered_operation
            .as_ref()
            .filter(|operation| operation.state() == LocalOperationState::FilesystemApplied)
        {
            self.verify_recovered_operation(operation, fact).await?;
        }
        if !filesystem_already_applied {
            if operation_kind == LocalOperationKind::Restore
                && let Some(existing) = existing.as_ref()
            {
                self.verify_quarantine_clean(existing, fact).await?;
            }
            if let Some(existing) = existing.as_ref().filter(|node| node.present()) {
                let is_self_originated_content = operation_kind == LocalOperationKind::ReplaceFile
                    && self
                        .state
                        .has_pending_file_content_submission(
                            self.scope.library_id(),
                            existing.node_id(),
                        )
                        .await
                        .unwrap_or(false);
                if !is_self_originated_content {
                    self.verify_clean(
                        existing,
                        operation_kind != LocalOperationKind::ReplaceFile,
                        fact,
                    )
                    .await?;
                }
            }
            self.ensure_destination_available(existing.as_ref(), &target, fact)
                .await?;
        }

        let mut operation = self
            .operation_for_desired(desired, existing.as_ref(), &target, fact, operation_kind)
            .await?;
        if let Err(error) = self.execute_operation(&mut operation, Some(desired)).await {
            if matches!(
                error,
                ClientSyncError::LocalIssue(LocalIssueKind::ContentIntegrityMismatch)
            ) {
                return Err(self
                    .block(
                        LocalIssueKind::ContentIntegrityMismatch,
                        Some(desired.node_id()),
                        fact,
                        "downloaded content must match the declared length and SHA-256",
                    )
                    .await);
            }
            return Err(error);
        }
        Ok(AppliedNode {
            node: self.local_node_from_desired(
                desired,
                target,
                fact,
                true,
                None,
                existing.as_ref(),
            ),
            operation_id: Some(operation.operation_id()),
        })
    }

    async fn apply_trashed(
        &self,
        desired: &LogicalSnapshotNode,
        target: ManagedRelativePath,
        existing: Option<&LocalNode>,
        fact: ApplyFact,
    ) -> Result<AppliedNode, ClientSyncError> {
        let Some(existing) = existing else {
            return Ok(AppliedNode {
                node: self.local_node_from_desired(desired, target, fact, false, None, None),
                operation_id: None,
            });
        };
        if !existing.present() {
            return Ok(AppliedNode {
                node: self.local_node_from_desired(
                    desired,
                    target,
                    fact,
                    false,
                    existing.quarantine_relative_path().cloned(),
                    Some(existing),
                ),
                operation_id: None,
            });
        }
        let recovered = self
            .state
            .operation_for_fact(
                self.scope.library_id(),
                desired.node_id(),
                fact.server_sequence(),
                fact.bootstrap_generation(),
            )
            .await?;
        if !recovered
            .as_ref()
            .is_some_and(|operation| operation.state() == LocalOperationState::FilesystemApplied)
        {
            self.verify_clean(existing, true, fact).await?;
        }
        if let Some(operation) = recovered
            .as_ref()
            .filter(|operation| operation.state() == LocalOperationState::FilesystemApplied)
        {
            self.verify_recovered_operation(operation, fact).await?;
        }
        let mut operation = self
            .operation_for_desired(
                desired,
                Some(existing),
                &target,
                fact,
                LocalOperationKind::Trash,
            )
            .await?;
        self.execute_operation(&mut operation, Some(desired))
            .await?;
        Ok(AppliedNode {
            node: self.local_node_from_desired(
                desired,
                target,
                fact,
                false,
                operation.staging().cloned(),
                Some(existing),
            ),
            operation_id: Some(operation.operation_id()),
        })
    }

    async fn active_operation_kind(
        &self,
        desired: &LogicalSnapshotNode,
        existing: Option<&LocalNode>,
        target: &ManagedRelativePath,
        event_kind: Option<ChangeKind>,
    ) -> Result<Option<LocalOperationKind>, ClientSyncError> {
        let Some(existing) = existing else {
            return Ok(Some(match desired.kind() {
                NodeKind::Directory => LocalOperationKind::CreateDirectory,
                NodeKind::File => LocalOperationKind::ReplaceFile,
            }));
        };
        if !existing.present() {
            if let Some(quarantine) = existing.quarantine_relative_path() {
                match self.replica.inspect(quarantine) {
                    Ok(Some(_)) | Err(ClientSyncError::InvalidState) => {
                        if quarantine_matches_desired(existing, desired) {
                            return Ok(Some(LocalOperationKind::Restore));
                        }
                    }
                    Ok(None) => {}
                    Err(error) => return Err(error),
                }
            }
            return Ok(Some(match desired.kind() {
                NodeKind::Directory => LocalOperationKind::CreateDirectory,
                NodeKind::File => LocalOperationKind::ReplaceFile,
            }));
        }
        let path_changed = existing.relative_path() != target;
        let content_changed = desired.kind() == NodeKind::File
            && (existing.current_version_id() != desired.current_version_id()
                || existing.content_length() != desired.content_length()
                || existing.content_sha256() != desired.content_sha256());
        if content_changed {
            return Ok(Some(LocalOperationKind::ReplaceFile));
        }
        if path_changed {
            return Ok(Some(match event_kind {
                Some(ChangeKind::NodeMoved) => LocalOperationKind::Move,
                _ => LocalOperationKind::Rename,
            }));
        }
        Ok(None)
    }

    async fn operation_for_desired(
        &self,
        desired: &LogicalSnapshotNode,
        existing: Option<&LocalNode>,
        target: &ManagedRelativePath,
        fact: ApplyFact,
        kind: LocalOperationKind,
    ) -> Result<LocalOperation, ClientSyncError> {
        if let Some(operation) = self
            .state
            .operation_for_fact(
                self.scope.library_id(),
                desired.node_id(),
                fact.server_sequence(),
                fact.bootstrap_generation(),
            )
            .await?
        {
            if operation.kind() != kind {
                return Err(ClientSyncError::InvalidState);
            }
            return Ok(operation);
        }
        let expected = existing.and_then(expected_fingerprint);
        let source = existing
            .filter(|node| node.present())
            .map(|node| node.relative_path().clone());
        let mut operation = LocalOperation::new(
            self.scope.library_id(),
            desired.node_id(),
            fact.server_sequence(),
            fact.bootstrap_generation(),
            kind,
            source,
            Some(target.clone()),
            expected.map(|value| match value.kind() {
                LocalObjectKind::File => NodeKind::File,
                LocalObjectKind::Directory => NodeKind::Directory,
            }),
            expected.and_then(LocalFingerprint::length),
            expected.and_then(LocalFingerprint::sha256),
            desired.revision(),
            desired.current_version_id(),
            desired.content_length(),
            desired.content_sha256(),
        );
        match kind {
            LocalOperationKind::CreateDirectory => {
                operation.set_staging(
                    self.replica
                        .directory_staging_location(operation.operation_id())?,
                );
            }
            LocalOperationKind::ReplaceFile => {
                operation.set_staging(self.replica.staging_location(operation.operation_id())?);
            }
            LocalOperationKind::Trash | LocalOperationKind::Purge => {
                operation.set_staging(
                    self.replica
                        .quarantine_location(desired.node_id(), operation.operation_id())?,
                );
            }
            LocalOperationKind::Restore => {
                operation.set_staging(
                    existing
                        .and_then(LocalNode::quarantine_relative_path)
                        .cloned()
                        .ok_or(ClientSyncError::InvalidState)?,
                );
            }
            LocalOperationKind::Rename | LocalOperationKind::Move => {}
        }
        self.state.prepare_operation(&operation).await?;
        Ok(operation)
    }

    async fn execute_operation(
        &self,
        operation: &mut LocalOperation,
        desired: Option<&LogicalSnapshotNode>,
    ) -> Result<(), ClientSyncError> {
        if operation.state() == LocalOperationState::FilesystemApplied {
            return Ok(());
        }
        if operation.state() == LocalOperationState::NeedsAttention {
            return Err(ClientSyncError::LocalIssue(
                LocalIssueKind::LocalRecoveryAmbiguous,
            ));
        }
        if operation.state() != LocalOperationState::Prepared {
            return Err(ClientSyncError::InvalidState);
        }
        self.failures.check(FailurePoint::BeforeFilesystemAction)?;
        match operation.kind() {
            LocalOperationKind::CreateDirectory => {
                let staging = operation.staging().ok_or(ClientSyncError::InvalidState)?;
                let destination = operation
                    .destination()
                    .ok_or(ClientSyncError::InvalidState)?;
                if self.replica.inspect(staging)?.is_none() {
                    let actual = self.replica.stage_directory(operation.operation_id())?;
                    if &actual != staging {
                        return Err(ClientSyncError::InvalidState);
                    }
                }
                if self.replica.inspect(staging)? == Some(LocalFingerprint::directory())
                    && self.replica.inspect(destination)?.is_none()
                {
                    self.replica.rename_path(staging, destination)?;
                } else {
                    return Err(ClientSyncError::InvalidState);
                }
            }
            LocalOperationKind::ReplaceFile => {
                let desired = desired.ok_or(ClientSyncError::InvalidState)?;
                let version_id = desired
                    .current_version_id()
                    .ok_or(ClientSyncError::InvalidRemoteResponse)?;
                let length = desired
                    .content_length()
                    .ok_or(ClientSyncError::InvalidRemoteResponse)?;
                let sha256 = desired
                    .content_sha256()
                    .ok_or(ClientSyncError::InvalidRemoteResponse)?;
                let staging = operation
                    .staging()
                    .ok_or(ClientSyncError::InvalidState)?
                    .clone();
                if self.replica.inspect(&staging)? != Some(LocalFingerprint::file(length, sha256)) {
                    let content = self
                        .remote
                        .download_current_content(self.scope, operation.node_id(), version_id)
                        .await?;
                    if content.node_id() != operation.node_id()
                        || content.version_id() != version_id
                        || content.length() != length
                        || content.sha256() != sha256
                    {
                        return Err(ClientSyncError::InvalidRemoteResponse);
                    }
                    let staged = self
                        .replica
                        .stage_content(
                            operation.operation_id(),
                            length,
                            sha256,
                            content.into_stream(),
                        )
                        .await
                        .map_err(|error| match error {
                            ClientSyncError::ContentIntegrityMismatch => {
                                ClientSyncError::LocalIssue(
                                    LocalIssueKind::ContentIntegrityMismatch,
                                )
                            }
                            other => other,
                        })?;
                    if staged != staging {
                        return Err(ClientSyncError::InvalidState);
                    }
                }
                self.failures
                    .check(FailurePoint::AfterContentStagedBeforeExpose)?;
                let destination = operation
                    .destination()
                    .ok_or(ClientSyncError::InvalidState)?;
                if let Some(source) = operation.source().filter(|source| *source != destination) {
                    let source_state = self.replica.inspect(source)?;
                    let destination_state = self.replica.inspect(destination)?;
                    if source_state.is_some() && destination_state.is_none() {
                        self.replica.rename_path(source, destination)?;
                    } else if source_state.is_some() || destination_state.is_none() {
                        return Err(ClientSyncError::InvalidState);
                    }
                }
                if self.replica.inspect(destination)?
                    == Some(LocalFingerprint::file(length, sha256))
                {
                    self.replica.remove_owned_staging(&staging)?;
                } else {
                    self.replica.expose_staged_file(
                        &staging,
                        destination,
                        operation.operation_id(),
                    )?;
                }
            }
            LocalOperationKind::Rename | LocalOperationKind::Move => {
                let source = operation.source().ok_or(ClientSyncError::InvalidState)?;
                let destination = operation
                    .destination()
                    .ok_or(ClientSyncError::InvalidState)?;
                if self.replica.inspect(source)?.is_some()
                    && self.replica.inspect(destination)?.is_none()
                {
                    self.replica.rename_path(source, destination)?;
                } else if self.replica.inspect(source)?.is_some()
                    || !fingerprint_matches_operation(operation, self.replica.inspect(destination)?)
                {
                    return Err(ClientSyncError::InvalidState);
                }
            }
            LocalOperationKind::Trash | LocalOperationKind::Purge => {
                let source = operation.source().ok_or(ClientSyncError::InvalidState)?;
                let quarantine = operation.staging().ok_or(ClientSyncError::InvalidState)?;
                if self.replica.inspect(source)?.is_some()
                    && self.replica.inspect(quarantine)?.is_none()
                {
                    let actual = self.replica.quarantine_path(
                        source,
                        operation.node_id(),
                        operation.operation_id(),
                    )?;
                    if &actual != quarantine {
                        return Err(ClientSyncError::InvalidState);
                    }
                } else if self.replica.inspect(source)?.is_some()
                    || self.replica.inspect(quarantine)?.is_none()
                {
                    return Err(ClientSyncError::InvalidState);
                }
            }
            LocalOperationKind::Restore => {
                let quarantine = operation.staging().ok_or(ClientSyncError::InvalidState)?;
                let destination = operation
                    .destination()
                    .ok_or(ClientSyncError::InvalidState)?;
                let quarantine_state = self.replica.inspect(quarantine)?;
                if !fingerprint_matches_operation(operation, quarantine_state) {
                    return Err(ClientSyncError::InvalidState);
                }
                if quarantine_state.is_some() && self.replica.inspect(destination)?.is_none() {
                    self.replica.restore_quarantined(quarantine, destination)?;
                } else if self.replica.inspect(quarantine)?.is_some()
                    || self.replica.inspect(destination)?.is_none()
                {
                    return Err(ClientSyncError::InvalidState);
                }
            }
        }
        self.replica
            .write_operation_receipt(operation.operation_id())?;
        self.failures
            .check(FailurePoint::AfterFilesystemActionBeforeState)?;
        self.state
            .set_operation_state(
                operation.operation_id(),
                LocalOperationState::FilesystemApplied,
            )
            .await?;
        operation.set_state(LocalOperationState::FilesystemApplied);
        Ok(())
    }

    async fn apply_purge(
        &self,
        change: &StoredChange,
        epoch: Sequence,
    ) -> Result<(), ClientSyncError> {
        match self.apply_purge_inner(change, epoch).await {
            Err(ClientSyncError::LocalIo) => Err(self
                .block(
                    LocalIssueKind::LocalIoUnavailable,
                    Some(change.resource_id),
                    ApplyFact::Feed(change.sequence),
                    "local filesystem operation must become available before retry",
                )
                .await),
            result => result,
        }
    }

    async fn apply_purge_inner(
        &self,
        change: &StoredChange,
        epoch: Sequence,
    ) -> Result<(), ClientSyncError> {
        if self
            .state
            .mark_outbound_intents_needing_rebase(self.scope.library_id(), Some(change.resource_id))
            .await?
        {
            return Err(ClientSyncError::ObservationIssue(
                crate::ObservationIssueKind::BaseStateChanged,
            ));
        }
        if change.desired_node.is_some() {
            return Err(ClientSyncError::InvalidRemoteResponse);
        }
        let existing = self
            .state
            .local_node(self.scope.library_id(), change.resource_id)
            .await?;
        let mut operation_id = None;
        if let Some(existing) = existing.as_ref().filter(|node| node.present()) {
            let recovered = self
                .state
                .operation_for_fact(
                    self.scope.library_id(),
                    existing.node_id(),
                    Some(change.sequence),
                    None,
                )
                .await?;
            if !recovered.as_ref().is_some_and(|operation| {
                operation.state() == LocalOperationState::FilesystemApplied
            }) {
                self.verify_clean(existing, true, ApplyFact::Feed(change.sequence))
                    .await?;
            }
            if let Some(operation) = recovered
                .as_ref()
                .filter(|operation| operation.state() == LocalOperationState::FilesystemApplied)
            {
                self.verify_recovered_operation(operation, ApplyFact::Feed(change.sequence))
                    .await?;
            }
            let descendants = self
                .state
                .local_nodes(self.scope.library_id())
                .await?
                .into_iter()
                .any(|node| node.parent_node_id() == Some(existing.node_id()));
            if descendants {
                return Err(self
                    .block(
                        LocalIssueKind::LocalDivergence,
                        Some(existing.node_id()),
                        ApplyFact::Feed(change.sequence),
                        "purge requires tracked descendants to be removed first",
                    )
                    .await);
            }
            let mut operation = if let Some(operation) = self
                .state
                .operation_for_fact(
                    self.scope.library_id(),
                    existing.node_id(),
                    Some(change.sequence),
                    None,
                )
                .await?
            {
                operation
            } else {
                let expected = expected_fingerprint(existing);
                let mut operation = LocalOperation::new(
                    self.scope.library_id(),
                    existing.node_id(),
                    Some(change.sequence),
                    None,
                    LocalOperationKind::Purge,
                    Some(existing.relative_path().clone()),
                    None,
                    expected.map(|value| match value.kind() {
                        LocalObjectKind::File => NodeKind::File,
                        LocalObjectKind::Directory => NodeKind::Directory,
                    }),
                    expected.and_then(LocalFingerprint::length),
                    expected.and_then(LocalFingerprint::sha256),
                    change.resource_revision,
                    None,
                    None,
                    None,
                );
                operation.set_staging(
                    self.replica
                        .quarantine_location(existing.node_id(), operation.operation_id())?,
                );
                self.state.prepare_operation(&operation).await?;
                operation
            };
            self.execute_operation(&mut operation, None).await?;
            operation_id = Some(operation.operation_id());
        }
        self.state
            .record_event_applied(self.scope, epoch, change, None, operation_id)
            .await
    }

    async fn sweep_absent_bootstrap_node(
        &self,
        node: &LocalNode,
        generation: Sequence,
    ) -> Result<(), ClientSyncError> {
        match self
            .sweep_absent_bootstrap_node_inner(node, generation)
            .await
        {
            Err(ClientSyncError::LocalIo) => Err(self
                .block(
                    LocalIssueKind::LocalIoUnavailable,
                    Some(node.node_id()),
                    ApplyFact::Bootstrap(generation),
                    "local filesystem operation must become available before retry",
                )
                .await),
            result => result,
        }
    }

    async fn sweep_absent_bootstrap_node_inner(
        &self,
        node: &LocalNode,
        generation: Sequence,
    ) -> Result<(), ClientSyncError> {
        if node.relative_path().is_root() {
            return Err(self
                .block(
                    LocalIssueKind::LocalDivergence,
                    Some(node.node_id()),
                    ApplyFact::Bootstrap(generation),
                    "bootstrap root identity must remain stable",
                )
                .await);
        }
        if !node.present() {
            self.state
                .remove_local_node(node.library_id(), node.node_id())
                .await?;
            return Ok(());
        }
        let recovered = self
            .state
            .operation_for_fact(node.library_id(), node.node_id(), None, Some(generation))
            .await?;
        if !recovered
            .as_ref()
            .is_some_and(|operation| operation.state() == LocalOperationState::FilesystemApplied)
        {
            self.verify_clean(node, true, ApplyFact::Bootstrap(generation))
                .await?;
        }
        if let Some(operation) = recovered
            .as_ref()
            .filter(|operation| operation.state() == LocalOperationState::FilesystemApplied)
        {
            self.verify_recovered_operation(operation, ApplyFact::Bootstrap(generation))
                .await?;
        }
        let mut operation = if let Some(operation) = self
            .state
            .operation_for_fact(node.library_id(), node.node_id(), None, Some(generation))
            .await?
        {
            operation
        } else {
            let expected = expected_fingerprint(node);
            let mut operation = LocalOperation::new(
                node.library_id(),
                node.node_id(),
                None,
                Some(generation),
                LocalOperationKind::Purge,
                Some(node.relative_path().clone()),
                None,
                expected.map(|value| match value.kind() {
                    LocalObjectKind::File => NodeKind::File,
                    LocalObjectKind::Directory => NodeKind::Directory,
                }),
                expected.and_then(LocalFingerprint::length),
                expected.and_then(LocalFingerprint::sha256),
                node.revision(),
                None,
                None,
                None,
            );
            operation.set_staging(
                self.replica
                    .quarantine_location(node.node_id(), operation.operation_id())?,
            );
            self.state.prepare_operation(&operation).await?;
            operation
        };
        self.execute_operation(&mut operation, None).await?;
        self.state
            .commit_removed_node_operation(
                node.library_id(),
                node.node_id(),
                operation.operation_id(),
            )
            .await
    }

    async fn desired_path(
        &self,
        desired: &LogicalSnapshotNode,
        fact: ApplyFact,
    ) -> Result<ManagedRelativePath, ClientSyncError> {
        let Some(parent_id) = desired.parent_node_id() else {
            return Ok(ManagedRelativePath::root());
        };
        if !validate_logical_name(desired.name().as_str()).is_portable() {
            return Err(self
                .block(
                    LocalIssueKind::LocalNameUnrepresentable,
                    Some(desired.node_id()),
                    fact,
                    "logical name is not representable as one exact Windows and Linux segment",
                )
                .await);
        }
        let parent = self
            .state
            .local_node(self.scope.library_id(), parent_id)
            .await?;
        let parent = match parent {
            Some(parent) if parent.kind() == NodeKind::Directory => parent,
            Some(_) => {
                return Err(self
                    .block(
                        LocalIssueKind::LocalTypeMismatch,
                        Some(desired.node_id()),
                        fact,
                        "desired parent must be a tracked directory",
                    )
                    .await);
            }
            None => {
                return Err(self
                    .block(
                        LocalIssueKind::LocalParentMissing,
                        Some(desired.node_id()),
                        fact,
                        "desired parent mapping must be durably applied first",
                    )
                    .await);
            }
        };
        parent.relative_path().child(desired.name().as_str())
    }

    async fn check_collision(
        &self,
        node_id: NodeId,
        target: &ManagedRelativePath,
        fact: ApplyFact,
    ) -> Result<(), ClientSyncError> {
        let key = target
            .as_str()
            .split('/')
            .map(local_collision_key)
            .collect::<Vec<_>>()
            .join("/");
        if self
            .state
            .local_node_at_collision_key(self.scope.library_id(), &key)
            .await?
            .is_some_and(|other| other.node_id() != node_id)
        {
            return Err(self
                .block(
                    LocalIssueKind::LocalNameCollision,
                    Some(node_id),
                    fact,
                    "portable case-folded path collides with another managed Node",
                )
                .await);
        }
        Ok(())
    }

    async fn ensure_destination_available(
        &self,
        existing: Option<&LocalNode>,
        target: &ManagedRelativePath,
        fact: ApplyFact,
    ) -> Result<(), ClientSyncError> {
        let same_managed_path =
            existing.is_some_and(|node| node.present() && node.relative_path() == target);
        if self.replica.has_portable_name_collision(target)? {
            return Err(self
                .block(
                    LocalIssueKind::LocalNameCollision,
                    existing.map(LocalNode::node_id),
                    fact,
                    "destination collides with another local name under the portable comparison policy",
                )
                .await);
        }
        let occupied = match self.replica.inspect(target) {
            Ok(value) => value.is_some(),
            Err(ClientSyncError::InvalidState | ClientSyncError::InvalidRelativePath) => true,
            Err(error) => return Err(error),
        };
        if !same_managed_path && occupied {
            return Err(self
                .block(
                    LocalIssueKind::LocalPathOccupied,
                    existing.map(LocalNode::node_id),
                    fact,
                    "destination is occupied by an object not attributable to this managed Node",
                )
                .await);
        }
        Ok(())
    }

    async fn verify_clean(
        &self,
        node: &LocalNode,
        include_tree: bool,
        fact: ApplyFact,
    ) -> Result<(), ClientSyncError> {
        if !node.present() {
            return Ok(());
        }
        let actual = match self.replica.inspect(node.relative_path()) {
            Ok(actual) => actual,
            Err(ClientSyncError::InvalidState | ClientSyncError::InvalidRelativePath) => {
                return Err(self
                    .block(
                        LocalIssueKind::LocalTypeMismatch,
                        Some(node.node_id()),
                        fact,
                        "tracked local object is not a supported regular file or directory",
                    )
                    .await);
            }
            Err(error) => return Err(error),
        };
        let expected = expected_fingerprint(node).ok_or(ClientSyncError::InvalidState)?;
        if actual.is_none() {
            return Err(self
                .block(
                    LocalIssueKind::LocalDivergence,
                    Some(node.node_id()),
                    fact,
                    "tracked local object is missing from its last durable path",
                )
                .await);
        }
        if actual.map(LocalFingerprint::kind) != Some(expected.kind()) {
            return Err(self
                .block(
                    LocalIssueKind::LocalTypeMismatch,
                    Some(node.node_id()),
                    fact,
                    "tracked local object type differs from the last durable apply",
                )
                .await);
        }
        if actual != Some(expected) {
            if node.kind() == NodeKind::File
                && let Some(staged) = self
                    .state
                    .durable_unresolved_content_conflict_source(node.library_id(), node.node_id())
                    .await?
            {
                let durable =
                    LocalFingerprint::file(staged.expected_length(), staged.expected_sha256());
                if self.replica.inspect(staged.staging_relative_path())? == Some(durable) {
                    return Ok(());
                }
            }
            return Err(self
                .block(
                    LocalIssueKind::LocalDivergence,
                    Some(node.node_id()),
                    fact,
                    "tracked local content differs from the last durable apply",
                )
                .await);
        }
        if include_tree && node.kind() == NodeKind::Directory {
            let actual_paths: BTreeSet<_> =
                match self.replica.list_descendants(node.relative_path()) {
                    Ok(paths) => paths.into_iter().collect(),
                    Err(ClientSyncError::InvalidState | ClientSyncError::InvalidRelativePath) => {
                        return Err(self
                            .block(
                                LocalIssueKind::LocalDivergence,
                                Some(node.node_id()),
                                fact,
                                "managed directory contains an unrepresentable local entry",
                            )
                            .await);
                    }
                    Err(error) => return Err(error),
                };
            let prefix = if node.relative_path().is_root() {
                String::new()
            } else {
                format!("{}/", node.relative_path().as_str())
            };
            let tracked: Vec<_> = self
                .state
                .local_nodes(node.library_id())
                .await?
                .into_iter()
                .filter(|candidate| {
                    candidate.present()
                        && candidate.node_id() != node.node_id()
                        && candidate.relative_path().as_str().starts_with(&prefix)
                })
                .collect();
            let tracked_paths: BTreeSet<_> = tracked
                .iter()
                .map(|candidate| candidate.relative_path().clone())
                .collect();
            if actual_paths != tracked_paths {
                return Err(self
                    .block(
                        LocalIssueKind::LocalDivergence,
                        Some(node.node_id()),
                        fact,
                        "managed directory tree contains unknown, missing, or redirected entries",
                    )
                    .await);
            }
            for child in tracked
                .iter()
                .filter(|child| child.kind() == NodeKind::File)
            {
                let actual = self.replica.inspect(child.relative_path())?;
                if actual != expected_fingerprint(child) {
                    return Err(self
                        .block(
                            LocalIssueKind::LocalDivergence,
                            Some(child.node_id()),
                            fact,
                            "managed descendant content differs from the last durable apply",
                        )
                        .await);
                }
            }
        }
        Ok(())
    }

    async fn verify_quarantine_clean(
        &self,
        node: &LocalNode,
        fact: ApplyFact,
    ) -> Result<(), ClientSyncError> {
        let quarantine = node
            .quarantine_relative_path()
            .ok_or(ClientSyncError::InvalidState)?;
        let actual = match self.replica.inspect(quarantine) {
            Ok(actual) => actual,
            Err(ClientSyncError::InvalidState | ClientSyncError::InvalidRelativePath) => {
                return Err(self
                    .block(
                        LocalIssueKind::LocalDivergence,
                        Some(node.node_id()),
                        fact,
                        "quarantine object is not a supported regular file or directory",
                    )
                    .await);
            }
            Err(error) => return Err(error),
        };
        let expected = expected_fingerprint(node).ok_or(ClientSyncError::InvalidState)?;
        let quarantine_has_descendants = if node.kind() == NodeKind::Directory {
            match self.replica.list_descendants(quarantine) {
                Ok(paths) => !paths.is_empty(),
                Err(ClientSyncError::InvalidState | ClientSyncError::InvalidRelativePath) => true,
                Err(error) => return Err(error),
            }
        } else {
            false
        };
        if actual != Some(expected) || quarantine_has_descendants {
            return Err(self
                .block(
                    LocalIssueKind::LocalDivergence,
                    Some(node.node_id()),
                    fact,
                    "quarantine contents must match the last durable managed state",
                )
                .await);
        }
        Ok(())
    }

    async fn verify_recovered_operation(
        &self,
        operation: &LocalOperation,
        fact: ApplyFact,
    ) -> Result<(), ClientSyncError> {
        let filesystem_root = match operation.kind() {
            LocalOperationKind::Trash | LocalOperationKind::Purge => operation.staging(),
            LocalOperationKind::CreateDirectory
            | LocalOperationKind::ReplaceFile
            | LocalOperationKind::Rename
            | LocalOperationKind::Move
            | LocalOperationKind::Restore => operation.destination(),
        }
        .ok_or(ClientSyncError::InvalidState)?;
        let expected_root = match operation.kind() {
            LocalOperationKind::CreateDirectory | LocalOperationKind::ReplaceFile => {
                desired_fingerprint(operation)
            }
            LocalOperationKind::Rename
            | LocalOperationKind::Move
            | LocalOperationKind::Trash
            | LocalOperationKind::Restore
            | LocalOperationKind::Purge => expected_operation_fingerprint(operation),
        }
        .ok_or(ClientSyncError::InvalidState)?;
        if self.replica.inspect(filesystem_root)? != Some(expected_root) {
            return Err(self
                .block(
                    LocalIssueKind::LocalDivergence,
                    Some(operation.node_id()),
                    fact,
                    "recovered filesystem result differs from the durable operation intent",
                )
                .await);
        }
        if expected_root.kind() != LocalObjectKind::Directory {
            return Ok(());
        }

        let logical_source = match operation.kind() {
            LocalOperationKind::Rename
            | LocalOperationKind::Move
            | LocalOperationKind::Trash
            | LocalOperationKind::Purge => operation.source(),
            LocalOperationKind::Restore => operation.destination(),
            LocalOperationKind::CreateDirectory | LocalOperationKind::ReplaceFile => None,
        };
        let mut expected_paths = BTreeSet::new();
        let mut expected_files = Vec::new();
        if let Some(logical_source) = logical_source {
            let prefix = format!("{}/", logical_source.as_str());
            for child in self
                .state
                .local_nodes(operation.library_id())
                .await?
                .into_iter()
                .filter(|child| {
                    child.present()
                        && child.node_id() != operation.node_id()
                        && child.relative_path().as_str().starts_with(&prefix)
                })
            {
                let suffix = child
                    .relative_path()
                    .as_str()
                    .strip_prefix(&prefix)
                    .ok_or(ClientSyncError::InvalidState)?;
                let mapped = append_relative(filesystem_root, suffix)?;
                expected_paths.insert(mapped.clone());
                if child.kind() == NodeKind::File {
                    expected_files.push((mapped, child));
                }
            }
        }
        let actual_paths: BTreeSet<_> = match self.replica.list_descendants(filesystem_root) {
            Ok(paths) => paths.into_iter().collect(),
            Err(ClientSyncError::InvalidState | ClientSyncError::InvalidRelativePath) => {
                return Err(self
                    .block(
                        LocalIssueKind::LocalDivergence,
                        Some(operation.node_id()),
                        fact,
                        "recovered directory contains an unrepresentable local entry",
                    )
                    .await);
            }
            Err(error) => return Err(error),
        };
        if actual_paths != expected_paths {
            return Err(self
                .block(
                    LocalIssueKind::LocalDivergence,
                    Some(operation.node_id()),
                    fact,
                    "recovered directory tree differs from the durable managed mapping",
                )
                .await);
        }
        for (path, child) in expected_files {
            if self.replica.inspect(&path)? != expected_fingerprint(&child) {
                return Err(self
                    .block(
                        LocalIssueKind::LocalDivergence,
                        Some(child.node_id()),
                        fact,
                        "recovered descendant content differs from the last durable apply",
                    )
                    .await);
            }
        }
        Ok(())
    }

    fn self_originated_filesystem_matches(
        &self,
        existing: &LocalNode,
        desired: &LogicalSnapshotNode,
        target: &ManagedRelativePath,
    ) -> Result<bool, ClientSyncError> {
        let target_fingerprint = match self.replica.inspect(target) {
            Ok(value) => value,
            Err(ClientSyncError::InvalidState | ClientSyncError::InvalidRelativePath) => {
                return Ok(false);
            }
            Err(error) => return Err(error),
        };
        if target_fingerprint != desired_node_fingerprint(desired)? {
            return Ok(false);
        }
        if existing.relative_path() == target {
            return Ok(true);
        }
        match self.replica.inspect(existing.relative_path()) {
            Ok(None) => Ok(true),
            Ok(Some(_)) => Ok(false),
            Err(ClientSyncError::InvalidState | ClientSyncError::InvalidRelativePath) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn self_originated_create_matches(
        &self,
        desired: &LogicalSnapshotNode,
        target: &ManagedRelativePath,
    ) -> Result<bool, ClientSyncError> {
        let target_fingerprint = match self.replica.inspect(target) {
            Ok(value) => value,
            Err(ClientSyncError::InvalidState | ClientSyncError::InvalidRelativePath) => {
                return Ok(false);
            }
            Err(error) => return Err(error),
        };
        Ok(target_fingerprint == desired_node_fingerprint(desired)?)
    }

    fn local_node_from_desired(
        &self,
        desired: &LogicalSnapshotNode,
        target: ManagedRelativePath,
        fact: ApplyFact,
        present: bool,
        quarantine: Option<ManagedRelativePath>,
        previous: Option<&LocalNode>,
    ) -> LocalNode {
        let active_file = present && desired.kind() == NodeKind::File;
        let local_length = if active_file {
            desired.content_length()
        } else {
            previous.and_then(LocalNode::local_length)
        };
        let local_sha256 = if active_file {
            desired.content_sha256()
        } else {
            previous.and_then(LocalNode::local_sha256)
        };
        LocalNode::new(
            self.scope.library_id(),
            desired.node_id(),
            desired.parent_node_id(),
            target,
            desired.name().clone(),
            desired.kind(),
            desired.state(),
            desired.revision(),
            desired.current_version_id(),
            desired.content_length(),
            desired.content_sha256(),
            local_length,
            local_sha256,
            fact.bootstrap_generation()
                .or_else(|| previous.map(LocalNode::bootstrap_generation))
                .unwrap_or_default(),
            present,
            quarantine,
        )
    }

    async fn block(
        &self,
        kind: LocalIssueKind,
        node_id: Option<NodeId>,
        fact: ApplyFact,
        expected_state: &str,
    ) -> ClientSyncError {
        match self
            .state
            .persist_issue(
                self.scope.library_id(),
                node_id,
                fact.server_sequence(),
                fact.bootstrap_generation(),
                kind,
                expected_state,
            )
            .await
        {
            Ok(_) => ClientSyncError::LocalIssue(kind),
            Err(error) => error,
        }
    }

    /// Inspect and classify every unfinished filesystem operation before new
    /// remote work. Deterministic completions advance to `FILESYSTEM_APPLIED`;
    /// ambiguity becomes a durable blocker.
    pub async fn recover_startup(&self) -> Result<(), ClientSyncError> {
        let _guard = self
            .state
            .lock_replica_writer(self.scope.library_id())
            .await;
        self.recover_startup_inner().await
    }

    async fn recover_startup_inner(&self) -> Result<(), ClientSyncError> {
        for operation in self
            .state
            .unfinished_operations(self.scope.library_id())
            .await?
        {
            let classification = match self.classify_operation(&operation) {
                Err(ClientSyncError::LocalIo) => {
                    return Err(self
                        .block(
                            LocalIssueKind::LocalIoUnavailable,
                            Some(operation.node_id()),
                            ApplyFact::from_operation(&operation)?,
                            "local filesystem recovery inspection must become available",
                        )
                        .await);
                }
                result => result?,
            };
            match classification {
                RecoveryClassification::FilesystemCompletedDatabasePending => {
                    self.state
                        .set_operation_state(
                            operation.operation_id(),
                            LocalOperationState::FilesystemApplied,
                        )
                        .await?;
                }
                RecoveryClassification::Ambiguous => {
                    self.state
                        .set_operation_state(
                            operation.operation_id(),
                            LocalOperationState::NeedsAttention,
                        )
                        .await?;
                    return Err(self
                        .block(
                            LocalIssueKind::LocalRecoveryAmbiguous,
                            Some(operation.node_id()),
                            ApplyFact::from_operation(&operation)?,
                            "interrupted filesystem operation cannot be attributed safely",
                        )
                        .await);
                }
                RecoveryClassification::NotStarted
                | RecoveryClassification::SafeToRetry
                | RecoveryClassification::DatabaseCompletedAckPending => {}
            }
        }
        Ok(())
    }

    pub fn classify_operation(
        &self,
        operation: &LocalOperation,
    ) -> Result<RecoveryClassification, ClientSyncError> {
        match operation.state() {
            LocalOperationState::DatabaseCommitted => {
                return Ok(RecoveryClassification::DatabaseCompletedAckPending);
            }
            LocalOperationState::FilesystemApplied => {
                return Ok(RecoveryClassification::FilesystemCompletedDatabasePending);
            }
            LocalOperationState::NeedsAttention => {
                return Ok(RecoveryClassification::Ambiguous);
            }
            LocalOperationState::Prepared => {}
        }
        let source = operation
            .source()
            .map(|path| self.replica.inspect(path))
            .transpose()?;
        let destination = operation
            .destination()
            .map(|path| self.replica.inspect(path))
            .transpose()?;
        let staging = operation
            .staging()
            .map(|path| self.replica.inspect(path))
            .transpose()?;
        let expected_source = fingerprint_matches_operation(operation, source.flatten());
        let desired_destination = desired_fingerprint(operation)
            .is_some_and(|expected| destination.flatten() == Some(expected));
        let has_receipt = match self.replica.has_operation_receipt(operation.operation_id()) {
            Ok(value) => value,
            Err(ClientSyncError::InvalidState) => return Ok(RecoveryClassification::Ambiguous),
            Err(error) => return Err(error),
        };

        let classification = if has_receipt {
            let completed = match operation.kind() {
                LocalOperationKind::CreateDirectory => {
                    destination.flatten() == Some(LocalFingerprint::directory())
                        && staging.flatten().is_none()
                }
                LocalOperationKind::ReplaceFile => desired_destination,
                LocalOperationKind::Rename | LocalOperationKind::Move => {
                    source.flatten().is_none()
                        && fingerprint_matches_operation(operation, destination.flatten())
                }
                LocalOperationKind::Trash | LocalOperationKind::Purge => {
                    source.flatten().is_none()
                        && fingerprint_matches_operation(operation, staging.flatten())
                }
                LocalOperationKind::Restore => {
                    staging.flatten().is_none()
                        && fingerprint_matches_operation(operation, destination.flatten())
                }
            };
            if completed {
                RecoveryClassification::FilesystemCompletedDatabasePending
            } else {
                RecoveryClassification::Ambiguous
            }
        } else {
            match operation.kind() {
                LocalOperationKind::CreateDirectory => {
                    match (staging.flatten(), destination.flatten()) {
                        (None, None) => RecoveryClassification::NotStarted,
                        (Some(value), None) if value == LocalFingerprint::directory() => {
                            RecoveryClassification::SafeToRetry
                        }
                        _ => RecoveryClassification::Ambiguous,
                    }
                }
                LocalOperationKind::ReplaceFile => {
                    let staging_retryable = staging.flatten().is_none()
                        || staging.flatten() == desired_fingerprint(operation);
                    let destination_retryable = if operation.source() == operation.destination() {
                        expected_source
                    } else {
                        destination.flatten().is_none()
                    };
                    if expected_source && destination_retryable && staging_retryable {
                        RecoveryClassification::SafeToRetry
                    } else {
                        RecoveryClassification::Ambiguous
                    }
                }
                LocalOperationKind::Rename | LocalOperationKind::Move => {
                    if expected_source && destination.flatten().is_none() {
                        RecoveryClassification::NotStarted
                    } else {
                        RecoveryClassification::Ambiguous
                    }
                }
                LocalOperationKind::Trash | LocalOperationKind::Purge => {
                    if expected_source && staging.flatten().is_none() {
                        RecoveryClassification::NotStarted
                    } else {
                        RecoveryClassification::Ambiguous
                    }
                }
                LocalOperationKind::Restore => {
                    if fingerprint_matches_operation(operation, staging.flatten())
                        && destination.flatten().is_none()
                    {
                        RecoveryClassification::SafeToRetry
                    } else {
                        RecoveryClassification::Ambiguous
                    }
                }
            }
        };
        Ok(classification)
    }
}

#[derive(Clone, Copy)]
enum ApplyFact {
    Feed(Sequence),
    Bootstrap(Sequence),
}

impl ApplyFact {
    const fn server_sequence(self) -> Option<Sequence> {
        match self {
            Self::Feed(sequence) => Some(sequence),
            Self::Bootstrap(_) => None,
        }
    }

    const fn bootstrap_generation(self) -> Option<Sequence> {
        match self {
            Self::Feed(_) => None,
            Self::Bootstrap(generation) => Some(generation),
        }
    }

    fn from_operation(operation: &LocalOperation) -> Result<Self, ClientSyncError> {
        match (
            operation.server_sequence(),
            operation.bootstrap_generation(),
        ) {
            (Some(sequence), None) => Ok(Self::Feed(sequence)),
            (None, Some(generation)) => Ok(Self::Bootstrap(generation)),
            _ => Err(ClientSyncError::InvalidState),
        }
    }
}

struct AppliedNode {
    node: LocalNode,
    operation_id: Option<Uuid>,
}

fn validate_checkpoint(
    scope: ReplicaScope,
    checkpoint: RemoteCheckpoint,
) -> Result<(), ClientSyncError> {
    if checkpoint.scope() != scope {
        return Err(ClientSyncError::WrongScope);
    }
    if checkpoint.epoch().get() > i64::MAX as u64
        || checkpoint.acknowledged_sequence().get() > i64::MAX as u64
    {
        return Err(ClientSyncError::InvalidRemoteResponse);
    }
    Ok(())
}

fn map_handoff_remote_error(error: RemoteError) -> ClientSyncError {
    match error.kind() {
        RemoteErrorKind::NotFound => ClientSyncError::HandoffSnapshotUnavailable,
        RemoteErrorKind::CheckpointConflict => ClientSyncError::HandoffCheckpointConflict,
        RemoteErrorKind::Offline
        | RemoteErrorKind::Unavailable
        | RemoteErrorKind::Rejected
        | RemoteErrorKind::InvalidEvidence
        | RemoteErrorKind::RateLimited
        | RemoteErrorKind::Internal
        | RemoteErrorKind::Protocol
        | RemoteErrorKind::Tls
        | RemoteErrorKind::Timeout
        | RemoteErrorKind::BodyLimit
        | RemoteErrorKind::Redirect
        | RemoteErrorKind::RebaselineRequired
        | RemoteErrorKind::Conflict
        | RemoteErrorKind::Integrity => ClientSyncError::HandoffTransport,
        RemoteErrorKind::AuthRequired
        | RemoteErrorKind::DeviceRevoked
        | RemoteErrorKind::Forbidden => ClientSyncError::Remote(error),
    }
}

fn expected_fingerprint(node: &LocalNode) -> Option<LocalFingerprint> {
    match node.kind() {
        NodeKind::Directory => Some(LocalFingerprint::directory()),
        NodeKind::File => Some(LocalFingerprint::file(
            node.local_length()?,
            node.local_sha256()?,
        )),
    }
}

fn desired_node_fingerprint(
    desired: &LogicalSnapshotNode,
) -> Result<Option<LocalFingerprint>, ClientSyncError> {
    Ok(Some(match desired.kind() {
        NodeKind::Directory => LocalFingerprint::directory(),
        NodeKind::File => LocalFingerprint::file(
            desired
                .content_length()
                .ok_or(ClientSyncError::InvalidRemoteResponse)?,
            desired
                .content_sha256()
                .ok_or(ClientSyncError::InvalidRemoteResponse)?,
        ),
    }))
}

fn desired_fingerprint(operation: &LocalOperation) -> Option<LocalFingerprint> {
    match operation.kind() {
        LocalOperationKind::CreateDirectory => Some(LocalFingerprint::directory()),
        LocalOperationKind::ReplaceFile => Some(LocalFingerprint::file(
            operation.desired_length()?,
            operation.desired_sha256()?,
        )),
        _ => None,
    }
}

fn expected_operation_fingerprint(operation: &LocalOperation) -> Option<LocalFingerprint> {
    match operation.expected_kind()? {
        NodeKind::Directory => Some(LocalFingerprint::directory()),
        NodeKind::File => Some(LocalFingerprint::file(
            operation.expected_length()?,
            operation.expected_sha256()?,
        )),
    }
}

fn append_relative(
    base: &ManagedRelativePath,
    suffix: &str,
) -> Result<ManagedRelativePath, ClientSyncError> {
    suffix
        .split('/')
        .try_fold(base.clone(), |path, segment| path.child(segment))
}

fn quarantine_matches_desired(existing: &LocalNode, desired: &LogicalSnapshotNode) -> bool {
    match desired.kind() {
        NodeKind::Directory => existing.kind() == NodeKind::Directory,
        NodeKind::File => {
            existing.kind() == NodeKind::File
                && existing.local_length() == desired.content_length()
                && existing.local_sha256() == desired.content_sha256()
        }
    }
}

fn fingerprint_matches_operation(
    operation: &LocalOperation,
    actual: Option<LocalFingerprint>,
) -> bool {
    let expected = match operation.expected_kind() {
        Some(NodeKind::Directory) => Some(LocalFingerprint::directory()),
        Some(NodeKind::File) => operation
            .expected_length()
            .zip(operation.expected_sha256())
            .map(|(length, hash)| LocalFingerprint::file(length, hash)),
        None => None,
    };
    actual == expected
}
