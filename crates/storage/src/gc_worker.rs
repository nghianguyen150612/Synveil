//! Internal, bounded orchestration for object-GC planning and physical work.
//!
//! `GcWorker::run_once` is deliberately a single deterministic cycle rather
//! than a hidden infinite loop. Runtime composition owns recurring scheduling;
//! this module coordinates only accepted Prompt 27/28 service boundaries and
//! never opens a filesystem path or calls an ObjectStore directly.

use std::{
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use futures_util::{StreamExt, stream};
use synveil_core::{GcWorkerConfig, ObjectGcPolicy};
use synveil_metadata::{
    DatabasePool, ObjectGcError, ObjectGcExecutionState, ObjectGcPlanResult,
    ObjectGcPlanningService, ObjectGcReconciliationReport, ObjectGcRecoveryClaim,
    ObjectGcWorkerMetadataError, PostgresObjectGcWorkerRepository,
};
use synveil_object_store::ObjectStore;
use tokio::{
    sync::{Semaphore, watch},
    time::timeout,
};

use crate::{
    ObjectGcExecutionConfigurationError, ObjectGcExecutionError, ObjectGcExecutionService,
    ObjectGcStepOutcome,
};

/// The bounded limits handed to one worker cycle. They contain no worker
/// identity, storage path, backend URL, or lease token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GcWorkerLimits {
    max_candidate_claims_per_cycle: u32,
    max_active_operations: u32,
    max_replica_actions_per_cycle: u32,
    max_concurrent_executions: u32,
    max_concurrent_replica_deletes: u32,
    retry_policy: synveil_core::GcWorkerRetryPolicy,
    shutdown_timeout: Duration,
}

impl From<GcWorkerConfig> for GcWorkerLimits {
    fn from(config: GcWorkerConfig) -> Self {
        Self {
            max_candidate_claims_per_cycle: config.max_candidate_claims_per_cycle(),
            max_active_operations: config.max_active_operations(),
            max_replica_actions_per_cycle: config.max_replica_actions_per_cycle(),
            max_concurrent_executions: config.max_concurrent_executions(),
            max_concurrent_replica_deletes: config.max_concurrent_replica_deletes(),
            retry_policy: config.retry_policy(),
            shutdown_timeout: config.shutdown_timeout(),
        }
    }
}

impl GcWorkerLimits {
    #[must_use]
    pub const fn max_candidate_claims_per_cycle(self) -> u32 {
        self.max_candidate_claims_per_cycle
    }

    #[must_use]
    pub const fn max_active_operations(self) -> u32 {
        self.max_active_operations
    }

    #[must_use]
    pub const fn max_replica_actions_per_cycle(self) -> u32 {
        self.max_replica_actions_per_cycle
    }

    #[must_use]
    pub const fn max_concurrent_executions(self) -> u32 {
        self.max_concurrent_executions
    }

    #[must_use]
    pub const fn max_concurrent_replica_deletes(self) -> u32 {
        self.max_concurrent_replica_deletes
    }

    #[must_use]
    pub const fn retry_policy(self) -> synveil_core::GcWorkerRetryPolicy {
        self.retry_policy
    }

    #[must_use]
    pub const fn shutdown_timeout(self) -> Duration {
        self.shutdown_timeout
    }

    #[must_use]
    fn operation_slice_limit(self) -> u32 {
        self.max_active_operations
            .min(self.max_replica_actions_per_cycle)
    }
}

/// Aggregate outcome of a bounded recovery or new-work slice. Counters are
/// safe operational telemetry: no storage keys or raw backend errors escape.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GcWorkerWorkReport {
    recovery_claimed: u32,
    candidates_claimed: u32,
    operations_resumed: u32,
    operations_started: u32,
    replica_actions_attempted: u32,
    replicas_deleted: u32,
    replicas_reconciled_absent: u32,
    operations_completed: u32,
    retry_scheduled: u32,
    deferred: u32,
    cancelled_or_invalidated: u32,
    needs_attention: u32,
    transient_failures: u32,
}

impl GcWorkerWorkReport {
    #[must_use]
    pub const fn recovery_claimed(self) -> u32 {
        self.recovery_claimed
    }

    #[must_use]
    pub const fn candidates_claimed(self) -> u32 {
        self.candidates_claimed
    }

    #[must_use]
    pub const fn operations_resumed(self) -> u32 {
        self.operations_resumed
    }

    #[must_use]
    pub const fn operations_started(self) -> u32 {
        self.operations_started
    }

    #[must_use]
    pub const fn replica_actions_attempted(self) -> u32 {
        self.replica_actions_attempted
    }

    #[must_use]
    pub const fn replicas_deleted(self) -> u32 {
        self.replicas_deleted
    }

    #[must_use]
    pub const fn replicas_reconciled_absent(self) -> u32 {
        self.replicas_reconciled_absent
    }

    #[must_use]
    pub const fn operations_completed(self) -> u32 {
        self.operations_completed
    }

    #[must_use]
    pub const fn retry_scheduled(self) -> u32 {
        self.retry_scheduled
    }

    #[must_use]
    pub const fn deferred(self) -> u32 {
        self.deferred
    }

    #[must_use]
    pub const fn cancelled_or_invalidated(self) -> u32 {
        self.cancelled_or_invalidated
    }

    #[must_use]
    pub const fn needs_attention(self) -> u32 {
        self.needs_attention
    }

    #[must_use]
    pub const fn transient_failures(self) -> u32 {
        self.transient_failures
    }

    #[must_use]
    pub const fn has_work(self) -> bool {
        self.recovery_claimed != 0
            || self.candidates_claimed != 0
            || self.operations_resumed != 0
            || self.operations_started != 0
            || self.replica_actions_attempted != 0
            || self.operations_completed != 0
    }

    fn merge(&mut self, other: Self) {
        self.recovery_claimed += other.recovery_claimed;
        self.candidates_claimed += other.candidates_claimed;
        self.operations_resumed += other.operations_resumed;
        self.operations_started += other.operations_started;
        self.replica_actions_attempted += other.replica_actions_attempted;
        self.replicas_deleted += other.replicas_deleted;
        self.replicas_reconciled_absent += other.replicas_reconciled_absent;
        self.operations_completed += other.operations_completed;
        self.retry_scheduled += other.retry_scheduled;
        self.deferred += other.deferred;
        self.cancelled_or_invalidated += other.cancelled_or_invalidated;
        self.needs_attention += other.needs_attention;
        self.transient_failures += other.transient_failures;
    }
}

/// Status of one `run_once` invocation. A degraded report is still an honest
/// bounded cycle; it is never a success claim for unfinished storage work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GcWorkerCycleStatus {
    Disabled,
    Idle,
    Worked,
    Degraded,
}

/// Safe, internal operator status for one bounded worker cycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GcWorkerCycleReport {
    status: GcWorkerCycleStatus,
    reconciliation: ObjectGcReconciliationReport,
    recovery: GcWorkerWorkReport,
    new_work: GcWorkerWorkReport,
    duration: Duration,
}

impl GcWorkerCycleReport {
    #[must_use]
    pub const fn status(self) -> GcWorkerCycleStatus {
        self.status
    }

    #[must_use]
    pub const fn reconciliation(self) -> ObjectGcReconciliationReport {
        self.reconciliation
    }

    #[must_use]
    pub const fn recovery(self) -> GcWorkerWorkReport {
        self.recovery
    }

    #[must_use]
    pub const fn new_work(self) -> GcWorkerWorkReport {
        self.new_work
    }

    #[must_use]
    pub const fn duration(self) -> Duration {
        self.duration
    }
}

/// A scheduling wait completed normally or stopped before a new claim cycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GcWorkerWaitOutcome {
    NextCycle,
    Shutdown,
}

/// Stable error boundary between runtime composition and the internal worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GcWorkerError {
    Metadata(ObjectGcWorkerMetadataError),
    Planning(ObjectGcError),
    Execution(ObjectGcExecutionError),
}

impl fmt::Display for GcWorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(error) => error.fmt(formatter),
            Self::Planning(error) => error.fmt(formatter),
            Self::Execution(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for GcWorkerError {}

/// Specific orchestration port. Production implementation delegates to the
/// existing planner, physical execution service, and PostgreSQL recovery
/// repository; this is not a generic job framework.
#[async_trait]
pub trait GcWorkerBackend: Send + Sync {
    async fn inspect_reconciliation(
        &self,
        limit: u32,
    ) -> Result<ObjectGcReconciliationReport, GcWorkerError>;

    async fn advance_recovery_slice(
        &self,
        limits: GcWorkerLimits,
    ) -> Result<GcWorkerWorkReport, GcWorkerError>;

    async fn advance_new_work_slice(
        &self,
        limits: GcWorkerLimits,
    ) -> Result<GcWorkerWorkReport, GcWorkerError>;
}

/// Transport-neutral internal GC coordinator. It has no HTTP router, no
/// normal-user action, no worker hostname/PID identity, and no direct storage
/// deletion capability.
pub struct GcWorker {
    config: GcWorkerConfig,
    backend: Arc<dyn GcWorkerBackend>,
}

impl GcWorker {
    /// Compose the production worker from the accepted Prompt 27/28 services.
    pub fn new(
        pool: DatabasePool,
        policy: ObjectGcPolicy,
        object_stores: Vec<Arc<dyn ObjectStore>>,
        config: GcWorkerConfig,
    ) -> Result<Self, ObjectGcExecutionConfigurationError> {
        let execution = Arc::new(ObjectGcExecutionService::with_object_stores(
            pool.clone(),
            policy,
            object_stores,
        )?);
        let backend = Arc::new(PostgresGcWorkerBackend {
            planning: ObjectGcPlanningService::new(pool.clone(), policy),
            recovery: PostgresObjectGcWorkerRepository::new(pool, policy),
            execution,
            replica_delete_semaphore: Arc::new(Semaphore::new(
                config.max_concurrent_replica_deletes() as usize,
            )),
        });
        Ok(Self::with_backend(config, backend))
    }

    /// Construct with a reviewed test/composition backend. The worker retains
    /// all cycle ordering and bounds; the backend remains responsible for the
    /// exact accepted service calls.
    #[must_use]
    pub fn with_backend(config: GcWorkerConfig, backend: Arc<dyn GcWorkerBackend>) -> Self {
        Self { config, backend }
    }

    #[must_use]
    pub const fn config(&self) -> GcWorkerConfig {
        self.config
    }

    /// Run one deterministic, bounded worker cycle. Existing recoverable
    /// physical operations are always advanced before any new candidate is
    /// claimed. If any recovery lease was claimed, new destructive work waits
    /// for a later cycle rather than competing with recovery pressure.
    pub async fn run_once(&self) -> Result<GcWorkerCycleReport, GcWorkerError> {
        let started = Instant::now();
        if !self.config.enabled() {
            return Ok(GcWorkerCycleReport {
                status: GcWorkerCycleStatus::Disabled,
                reconciliation: ObjectGcReconciliationReport::default(),
                recovery: GcWorkerWorkReport::default(),
                new_work: GcWorkerWorkReport::default(),
                duration: started.elapsed(),
            });
        }

        let limits = GcWorkerLimits::from(self.config);
        let reconciliation = self
            .backend
            .inspect_reconciliation(limits.max_replica_actions_per_cycle())
            .await?;
        let recovery = self.backend.advance_recovery_slice(limits).await?;
        let new_work = if recovery.recovery_claimed() == 0 {
            self.backend.advance_new_work_slice(limits).await?
        } else {
            GcWorkerWorkReport::default()
        };
        let status = if recovery.transient_failures() != 0
            || new_work.transient_failures() != 0
            || recovery.needs_attention() != 0
            || new_work.needs_attention() != 0
            || reconciliation.total_findings() != 0
        {
            GcWorkerCycleStatus::Degraded
        } else if recovery.has_work() || new_work.has_work() {
            GcWorkerCycleStatus::Worked
        } else {
            GcWorkerCycleStatus::Idle
        };
        Ok(GcWorkerCycleReport {
            status,
            reconciliation,
            recovery,
            new_work,
            duration: started.elapsed(),
        })
    }

    /// Wait once between cycles while respecting a graceful runtime shutdown.
    /// The recurring loop belongs in the binary/runtime composition, not in
    /// this application boundary.
    pub async fn wait_for_next_cycle(
        &self,
        shutdown: &mut watch::Receiver<bool>,
    ) -> GcWorkerWaitOutcome {
        if *shutdown.borrow_and_update() {
            return GcWorkerWaitOutcome::Shutdown;
        }
        tokio::select! {
            () = tokio::time::sleep(self.config.cycle_interval()) => GcWorkerWaitOutcome::NextCycle,
            changed = shutdown.changed() => {
                let _ = changed;
                GcWorkerWaitOutcome::Shutdown
            }
        }
    }
}

struct PostgresGcWorkerBackend {
    planning: ObjectGcPlanningService,
    recovery: PostgresObjectGcWorkerRepository,
    execution: Arc<ObjectGcExecutionService>,
    replica_delete_semaphore: Arc<Semaphore>,
}

#[async_trait]
impl GcWorkerBackend for PostgresGcWorkerBackend {
    async fn inspect_reconciliation(
        &self,
        limit: u32,
    ) -> Result<ObjectGcReconciliationReport, GcWorkerError> {
        self.recovery
            .inspect_reconciliation(limit)
            .await
            .map_err(GcWorkerError::Metadata)
    }

    async fn advance_recovery_slice(
        &self,
        limits: GcWorkerLimits,
    ) -> Result<GcWorkerWorkReport, GcWorkerError> {
        let limit = limits
            .operation_slice_limit()
            .min(self.planning.policy().max_batch_size());
        let claims = self
            .recovery
            .claim_recoverable_operations(limit)
            .await
            .map_err(GcWorkerError::Metadata)?;
        let claimed = u32::try_from(claims.len()).expect("worker claim batch is bounded");
        let reports = stream::iter(
            claims
                .into_iter()
                .map(|claim| self.advance_recovery_claim(claim, limits)),
        )
        .buffer_unordered(limits.max_concurrent_executions() as usize)
        .collect::<Vec<_>>()
        .await;
        let mut report = GcWorkerWorkReport {
            recovery_claimed: claimed,
            ..GcWorkerWorkReport::default()
        };
        for task_report in reports {
            report.merge(task_report);
        }
        Ok(report)
    }

    async fn advance_new_work_slice(
        &self,
        limits: GcWorkerLimits,
    ) -> Result<GcWorkerWorkReport, GcWorkerError> {
        let limit = limits
            .max_candidate_claims_per_cycle()
            .min(limits.operation_slice_limit())
            .min(self.planning.policy().max_batch_size());
        let leases = self
            .planning
            .claim_candidates(limit)
            .await
            .map_err(GcWorkerError::Planning)?;
        let claimed = u32::try_from(leases.len()).expect("worker claim batch is bounded");
        let reports = stream::iter(
            leases
                .into_iter()
                .map(|lease| self.advance_new_claim(lease, limits)),
        )
        .buffer_unordered(limits.max_concurrent_executions() as usize)
        .collect::<Vec<_>>()
        .await;
        let mut report = GcWorkerWorkReport {
            candidates_claimed: claimed,
            ..GcWorkerWorkReport::default()
        };
        for task_report in reports {
            report.merge(task_report);
        }
        Ok(report)
    }
}

impl PostgresGcWorkerBackend {
    async fn advance_recovery_claim(
        &self,
        claim: ObjectGcRecoveryClaim,
        limits: GcWorkerLimits,
    ) -> GcWorkerWorkReport {
        let mut report = GcWorkerWorkReport::default();
        let Some(lease) = self.ready_lease(claim.lease(), &mut report).await else {
            return report;
        };
        let operation = match self.execution.resume_gc_execution(lease).await {
            Ok(operation) => operation,
            Err(error) => {
                // A recovery claim already identifies the durable operation
                // that was selected under the candidate lock. Preserve that
                // identity so a terminal resume failure can be recorded as
                // `NEEDS_ATTENTION` instead of being reclaimed forever.
                self.handle_execution_error(Some(claim.operation_id()), lease, error, &mut report)
                    .await;
                return report;
            }
        };
        if operation.operation_id() != claim.operation_id() {
            self.mark_needs_attention(
                operation.operation_id(),
                lease,
                "gc_operation_identity_mismatch",
                &mut report,
            )
            .await;
            return report;
        }
        report.operations_resumed += 1;
        self.advance_operation(operation, lease, limits, &mut report)
            .await;
        report
    }

    async fn advance_new_claim(
        &self,
        lease: synveil_metadata::ObjectGcLease,
        limits: GcWorkerLimits,
    ) -> GcWorkerWorkReport {
        let mut report = GcWorkerWorkReport::default();
        let Some(lease) = self.ready_lease(lease, &mut report).await else {
            return report;
        };
        let operation = match self.execution.start_gc_execution(lease).await {
            Ok(operation) => operation,
            Err(error) => {
                self.handle_execution_error(None, lease, error, &mut report)
                    .await;
                return report;
            }
        };
        report.operations_started += 1;
        self.advance_operation(operation, lease, limits, &mut report)
            .await;
        report
    }

    async fn ready_lease(
        &self,
        lease: synveil_metadata::ObjectGcLease,
        report: &mut GcWorkerWorkReport,
    ) -> Option<synveil_metadata::ObjectGcLease> {
        match self.planning.mark_ready_for_deletion(lease).await {
            Ok(ObjectGcPlanResult::Valid(candidate)) => candidate.lease(),
            Ok(ObjectGcPlanResult::Invalidated) => {
                report.cancelled_or_invalidated += 1;
                None
            }
            Err(error) => {
                self.handle_planning_error(error, Some(lease), report).await;
                None
            }
        }
    }

    async fn advance_operation(
        &self,
        operation: synveil_metadata::ObjectGcOperation,
        lease: synveil_metadata::ObjectGcLease,
        limits: GcWorkerLimits,
        report: &mut GcWorkerWorkReport,
    ) {
        if operation.state() == ObjectGcExecutionState::NeedsAttention {
            report.needs_attention += 1;
            return;
        }
        let permit = match Arc::clone(&self.replica_delete_semaphore)
            .acquire_owned()
            .await
        {
            Ok(permit) => permit,
            Err(_) => {
                report.transient_failures += 1;
                self.release_after_slice(lease, report).await;
                return;
            }
        };
        let step = timeout(
            limits.shutdown_timeout(),
            self.execution.delete_next_replica_with_retry_policy(
                operation.operation_id(),
                lease,
                limits.retry_policy(),
            ),
        )
        .await;
        drop(permit);
        let step = match step {
            Ok(Ok(step)) => step,
            Ok(Err(error)) => {
                self.handle_execution_error(Some(operation.operation_id()), lease, error, report)
                    .await;
                return;
            }
            Err(_) => {
                // Dropping a timed-out future never records success. A fenced
                // action is intentionally left for Prompt 28 reconciliation.
                report.transient_failures += 1;
                self.release_after_slice(lease, report).await;
                return;
            }
        };

        match step.outcome() {
            ObjectGcStepOutcome::Completed => {
                report.operations_completed += 1;
            }
            ObjectGcStepOutcome::ReadyToComplete => {
                match timeout(
                    limits.shutdown_timeout(),
                    self.execution
                        .complete_gc_execution(step.operation().operation_id(), step.lease()),
                )
                .await
                {
                    Ok(Ok(_)) => report.operations_completed += 1,
                    Ok(Err(error)) => {
                        self.handle_execution_error(
                            Some(step.operation().operation_id()),
                            step.lease(),
                            error,
                            report,
                        )
                        .await;
                    }
                    Err(_) => {
                        report.transient_failures += 1;
                        self.release_after_slice(step.lease(), report).await;
                    }
                }
            }
            ObjectGcStepOutcome::Deferred => {
                report.deferred += 1;
                self.release_after_slice(step.lease(), report).await;
            }
            ObjectGcStepOutcome::NeedsAttention | ObjectGcStepOutcome::EvidenceMismatch => {
                report.needs_attention += 1;
            }
            ObjectGcStepOutcome::ReplicaDeleted
            | ObjectGcStepOutcome::ReplicaAlreadyAbsent
            | ObjectGcStepOutcome::ReplicaReconciledAbsent
            | ObjectGcStepOutcome::ReplicaStillPresent
            | ObjectGcStepOutcome::ReplicaDeleteInProgress
            | ObjectGcStepOutcome::ReconciliationRequired => {
                report.replica_actions_attempted += 1;
                match step.outcome() {
                    ObjectGcStepOutcome::ReplicaDeleted
                    | ObjectGcStepOutcome::ReplicaAlreadyAbsent => report.replicas_deleted += 1,
                    ObjectGcStepOutcome::ReplicaReconciledAbsent => {
                        report.replicas_reconciled_absent += 1;
                    }
                    ObjectGcStepOutcome::ReplicaStillPresent
                    | ObjectGcStepOutcome::ReplicaDeleteInProgress
                    | ObjectGcStepOutcome::ReconciliationRequired => report.retry_scheduled += 1,
                    ObjectGcStepOutcome::Completed
                    | ObjectGcStepOutcome::ReadyToComplete
                    | ObjectGcStepOutcome::EvidenceMismatch
                    | ObjectGcStepOutcome::Deferred
                    | ObjectGcStepOutcome::NeedsAttention => unreachable!("matched above"),
                }
                self.release_after_slice(step.lease(), report).await;
            }
        }
    }

    async fn handle_planning_error(
        &self,
        error: ObjectGcError,
        lease: Option<synveil_metadata::ObjectGcLease>,
        report: &mut GcWorkerWorkReport,
    ) {
        match error {
            ObjectGcError::CandidateInvalidated | ObjectGcError::NotFound => {
                report.cancelled_or_invalidated += 1;
            }
            ObjectGcError::StaleLease
            | ObjectGcError::LeaseExpired
            | ObjectGcError::InvalidRequest => {
                report.transient_failures += 1;
            }
            ObjectGcError::Database(_) => report.transient_failures += 1,
            ObjectGcError::InvalidPolicy | ObjectGcError::InvalidPersistedData => {
                report.needs_attention += 1;
            }
        }
        if let Some(lease) = lease {
            self.release_after_slice(lease, report).await;
        }
    }

    async fn handle_execution_error(
        &self,
        operation_id: Option<synveil_core::ObjectGcOperationId>,
        lease: synveil_metadata::ObjectGcLease,
        error: ObjectGcExecutionError,
        report: &mut GcWorkerWorkReport,
    ) {
        let terminal_code = match error {
            ObjectGcExecutionError::BackendUnavailable => Some("gc_backend_unavailable"),
            ObjectGcExecutionError::EvidenceMismatch => Some("gc_replica_evidence_mismatch"),
            ObjectGcExecutionError::InvalidPersistedData => Some("gc_invalid_persisted_state"),
            ObjectGcExecutionError::NeedsAttention => Some("gc_needs_attention"),
            ObjectGcExecutionError::InvalidState => Some("gc_invalid_state"),
            ObjectGcExecutionError::CandidateNotFound
            | ObjectGcExecutionError::ReadyRequired
            | ObjectGcExecutionError::StaleLease
            | ObjectGcExecutionError::LeaseExpired
            | ObjectGcExecutionError::ReferenceExists
            | ObjectGcExecutionError::HoldExists
            | ObjectGcExecutionError::OperationNotFound
            | ObjectGcExecutionError::ReconciliationRequired
            | ObjectGcExecutionError::DatabaseUnavailable => None,
        };
        if let Some(error_code) = terminal_code {
            if let Some(operation_id) = operation_id {
                self.mark_needs_attention(operation_id, lease, error_code, report)
                    .await;
            } else {
                report.needs_attention += 1;
            }
            return;
        }
        match error {
            ObjectGcExecutionError::CandidateNotFound
            | ObjectGcExecutionError::ReferenceExists
            | ObjectGcExecutionError::HoldExists
            | ObjectGcExecutionError::OperationNotFound => report.cancelled_or_invalidated += 1,
            ObjectGcExecutionError::ReadyRequired
            | ObjectGcExecutionError::StaleLease
            | ObjectGcExecutionError::LeaseExpired
            | ObjectGcExecutionError::ReconciliationRequired
            | ObjectGcExecutionError::DatabaseUnavailable => report.transient_failures += 1,
            ObjectGcExecutionError::BackendUnavailable
            | ObjectGcExecutionError::EvidenceMismatch
            | ObjectGcExecutionError::NeedsAttention
            | ObjectGcExecutionError::InvalidState
            | ObjectGcExecutionError::InvalidPersistedData => unreachable!("handled above"),
        }
        self.release_after_slice(lease, report).await;
    }

    async fn mark_needs_attention(
        &self,
        operation_id: synveil_core::ObjectGcOperationId,
        lease: synveil_metadata::ObjectGcLease,
        error_code: &'static str,
        report: &mut GcWorkerWorkReport,
    ) {
        report.needs_attention += 1;
        if self
            .execution
            .mark_gc_execution_needs_attention(operation_id, lease, error_code)
            .await
            .is_err()
        {
            // Keep the candidate lease intact if this terminal marker cannot
            // be persisted; expiry plus reconciliation remains the recovery
            // path and no false completion is emitted.
            report.transient_failures += 1;
        }
    }

    async fn release_after_slice(
        &self,
        lease: synveil_metadata::ObjectGcLease,
        report: &mut GcWorkerWorkReport,
    ) {
        match self.planning.release_lease(lease).await {
            Ok(_) => {}
            Err(ObjectGcError::NotFound | ObjectGcError::CandidateInvalidated) => {
                report.cancelled_or_invalidated += 1;
            }
            Err(_) => report.transient_failures += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use synveil_core::{GcWorkerConfig, GcWorkerRetryPolicy};
    use synveil_metadata::{
        DatabaseError, DatabaseErrorKind, ObjectGcReconciliationReport, ObjectGcWorkerMetadataError,
    };
    use tokio::sync::watch;

    use super::{
        GcWorker, GcWorkerBackend, GcWorkerCycleStatus, GcWorkerError, GcWorkerLimits,
        GcWorkerWaitOutcome, GcWorkerWorkReport,
    };

    #[derive(Default)]
    struct MockState {
        inspect_calls: u32,
        recovery_calls: u32,
        new_calls: u32,
        last_limits: Option<GcWorkerLimits>,
        recovery_report: GcWorkerWorkReport,
        new_report: GcWorkerWorkReport,
        inspection_error: Option<GcWorkerError>,
    }

    struct MockBackend(Mutex<MockState>);

    #[async_trait]
    impl GcWorkerBackend for MockBackend {
        async fn inspect_reconciliation(
            &self,
            _limit: u32,
        ) -> Result<ObjectGcReconciliationReport, GcWorkerError> {
            let mut state = self.0.lock().expect("mock state lock");
            state.inspect_calls += 1;
            if let Some(error) = state.inspection_error {
                return Err(error);
            }
            Ok(ObjectGcReconciliationReport::default())
        }

        async fn advance_recovery_slice(
            &self,
            limits: GcWorkerLimits,
        ) -> Result<GcWorkerWorkReport, GcWorkerError> {
            let mut state = self.0.lock().expect("mock state lock");
            state.recovery_calls += 1;
            state.last_limits = Some(limits);
            Ok(state.recovery_report)
        }

        async fn advance_new_work_slice(
            &self,
            limits: GcWorkerLimits,
        ) -> Result<GcWorkerWorkReport, GcWorkerError> {
            let mut state = self.0.lock().expect("mock state lock");
            state.new_calls += 1;
            state.last_limits = Some(limits);
            Ok(state.new_report)
        }
    }

    fn enabled_config() -> GcWorkerConfig {
        GcWorkerConfig::new(
            true,
            std::time::Duration::from_secs(1),
            3,
            2,
            2,
            2,
            1,
            GcWorkerRetryPolicy::default(),
            std::time::Duration::from_secs(1),
        )
        .expect("test worker configuration is valid")
    }

    #[tokio::test]
    async fn no_work_cycle_is_bounded_and_idempotent() {
        let backend = Arc::new(MockBackend(Mutex::new(MockState::default())));
        let worker = GcWorker::with_backend(enabled_config(), backend.clone());

        let first = worker.run_once().await.expect("first no-work cycle");
        let second = worker.run_once().await.expect("second no-work cycle");

        assert_eq!(first.status(), GcWorkerCycleStatus::Idle);
        assert_eq!(second.status(), GcWorkerCycleStatus::Idle);
        let state = backend.0.lock().expect("mock state lock");
        assert_eq!(state.inspect_calls, 2);
        assert_eq!(state.recovery_calls, 2);
        assert_eq!(state.new_calls, 2);
    }

    #[tokio::test]
    async fn recovery_claims_are_advanced_before_new_candidates() {
        let backend = Arc::new(MockBackend(Mutex::new(MockState {
            recovery_report: GcWorkerWorkReport {
                recovery_claimed: 1,
                operations_resumed: 1,
                ..GcWorkerWorkReport::default()
            },
            ..MockState::default()
        })));
        let worker = GcWorker::with_backend(enabled_config(), backend.clone());

        let report = worker.run_once().await.expect("recovery cycle");

        assert_eq!(report.status(), GcWorkerCycleStatus::Worked);
        assert_eq!(report.recovery().recovery_claimed(), 1);
        assert_eq!(report.new_work().candidates_claimed(), 0);
        assert_eq!(backend.0.lock().expect("mock state lock").new_calls, 0);
    }

    #[tokio::test]
    async fn cycle_forwards_configured_bounded_concurrency_limits() {
        let backend = Arc::new(MockBackend(Mutex::new(MockState::default())));
        let worker = GcWorker::with_backend(enabled_config(), backend.clone());

        let _ = worker.run_once().await.expect("bounded cycle");

        let limits = backend
            .0
            .lock()
            .expect("mock state lock")
            .last_limits
            .expect("worker supplied limits");
        assert_eq!(limits.max_candidate_claims_per_cycle(), 3);
        assert_eq!(limits.max_active_operations(), 2);
        assert_eq!(limits.max_replica_actions_per_cycle(), 2);
        assert_eq!(limits.max_concurrent_executions(), 2);
        assert_eq!(limits.max_concurrent_replica_deletes(), 1);
    }

    #[tokio::test]
    async fn shutdown_wait_returns_without_claiming_more_work() {
        let backend = Arc::new(MockBackend(Mutex::new(MockState::default())));
        let worker = GcWorker::with_backend(enabled_config(), backend);
        let (shutdown_sender, mut shutdown) = watch::channel(false);
        shutdown_sender
            .send(true)
            .expect("shutdown receiver exists");

        assert_eq!(
            worker.wait_for_next_cycle(&mut shutdown).await,
            GcWorkerWaitOutcome::Shutdown
        );
    }

    #[tokio::test]
    async fn database_outage_never_advances_to_new_work() {
        let backend = Arc::new(MockBackend(Mutex::new(MockState {
            inspection_error: Some(GcWorkerError::Metadata(
                ObjectGcWorkerMetadataError::Database(DatabaseError::Failure(
                    DatabaseErrorKind::ConnectionUnavailable,
                )),
            )),
            ..MockState::default()
        })));
        let worker = GcWorker::with_backend(enabled_config(), backend.clone());

        assert!(worker.run_once().await.is_err());
        let state = backend.0.lock().expect("mock state lock");
        assert_eq!(state.recovery_calls, 0);
        assert_eq!(state.new_calls, 0);
    }
}
