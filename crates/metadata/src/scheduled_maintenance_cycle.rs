//! Manually invoked bounded scheduled-maintenance cycle orchestration.
//!
//! One explicit cycle invocation composes exactly one scheduler tick and
//! exactly one scheduled-maintenance worker step. There is no loop, daemon,
//! polling, heartbeat, lease renewal, or background execution. The caller
//! controls invocation frequency; each call has a hard bound of one tick
//! and one worker transition.

use synveil_core::{
    BackupScheduledMaintenanceWorkerId, BackupSchedulerSkipOutcome, BackupSchedulerTickOutcome,
    BackupSchedulerTickResult, Timestamp,
};

use crate::{
    BackupSchedulerError, BackupSchedulerService, BackupService, DatabasePool,
    ScheduledMaintenanceWorkerError, ScheduledMaintenanceWorkerService,
    ScheduledMaintenanceWorkerStepOutcome,
};

/// Typed failures for the scheduled-maintenance cycle boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduledMaintenanceCycleError {
    Scheduler(BackupSchedulerError),
    Worker(ScheduledMaintenanceWorkerError),
    /// The provided lease duration is outside the canonical bounds (10–900
    /// seconds). The value is not silently clamped.
    InvalidLeaseDuration,
}

impl std::fmt::Display for ScheduledMaintenanceCycleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Scheduler(error) => error.fmt(formatter),
            Self::Worker(error) => error.fmt(formatter),
            Self::InvalidLeaseDuration => {
                formatter.write_str("lease duration is outside the valid range (10–900 seconds)")
            }
        }
    }
}

impl std::error::Error for ScheduledMaintenanceCycleError {}

impl From<BackupSchedulerError> for ScheduledMaintenanceCycleError {
    fn from(error: BackupSchedulerError) -> Self {
        Self::Scheduler(error)
    }
}

impl From<ScheduledMaintenanceWorkerError> for ScheduledMaintenanceCycleError {
    fn from(error: ScheduledMaintenanceWorkerError) -> Self {
        Self::Worker(error)
    }
}

/// Outcome of one scheduler tick within the cycle.
#[derive(Clone, Debug, PartialEq)]
pub enum ScheduledMaintenanceCycleTickOutcome {
    /// No due work was found; the scheduler was idle.
    Idle,
    /// An expired prefix was skipped per misfire policy.
    SkippedExpired(BackupSchedulerSkipOutcome),
    /// An already-materialized occurrence was handed off.
    HandedOffExisting(BackupSchedulerTickOutcome),
    /// A new occurrence was materialized and handed off.
    MaterializedAndHandedOff(BackupSchedulerTickOutcome),
}

impl From<BackupSchedulerTickResult> for ScheduledMaintenanceCycleTickOutcome {
    fn from(result: BackupSchedulerTickResult) -> Self {
        match result {
            BackupSchedulerTickResult::Idle => Self::Idle,
            BackupSchedulerTickResult::SkippedExpired(skip) => Self::SkippedExpired(skip),
            BackupSchedulerTickResult::HandedOffExisting(outcome) => {
                Self::HandedOffExisting(outcome)
            }
            BackupSchedulerTickResult::MaterializedAndHandedOff(outcome) => {
                Self::MaterializedAndHandedOff(outcome)
            }
        }
    }
}

impl ScheduledMaintenanceCycleTickOutcome {
    #[must_use]
    pub const fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    #[must_use]
    pub fn outcome(&self) -> Option<BackupSchedulerTickOutcome> {
        match self {
            Self::Idle | Self::SkippedExpired(_) => None,
            Self::HandedOffExisting(outcome) | Self::MaterializedAndHandedOff(outcome) => {
                Some(*outcome)
            }
        }
    }

    #[must_use]
    pub fn skip_outcome(&self) -> Option<BackupSchedulerSkipOutcome> {
        match self {
            Self::SkippedExpired(skip) => Some(*skip),
            Self::Idle | Self::HandedOffExisting(_) | Self::MaterializedAndHandedOff(_) => None,
        }
    }
}

/// Outcome of one worker step within the cycle. The stepped payload is boxed
/// to keep the manually returned orchestration result small; one cycle
/// invocation returns exactly one such value by value.
#[derive(Clone, Debug, PartialEq)]
pub enum ScheduledMaintenanceCycleWorkerOutcome {
    /// No eligible scheduled-maintenance work was found.
    Idle,
    /// A claim was made or reconciled, and a transition executed or reconciled.
    Stepped(Box<ScheduledMaintenanceWorkerStepOutcome>),
}

impl ScheduledMaintenanceCycleWorkerOutcome {
    #[must_use]
    pub const fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    #[must_use]
    pub fn stepped(&self) -> Option<&ScheduledMaintenanceWorkerStepOutcome> {
        match self {
            Self::Idle => None,
            Self::Stepped(outcome) => Some(outcome),
        }
    }
}

/// Combined result of one bounded cycle invocation.
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduledMaintenanceCycleResult {
    tick: ScheduledMaintenanceCycleTickOutcome,
    worker: ScheduledMaintenanceCycleWorkerOutcome,
}

impl ScheduledMaintenanceCycleResult {
    #[must_use]
    pub const fn tick(&self) -> &ScheduledMaintenanceCycleTickOutcome {
        &self.tick
    }

    #[must_use]
    pub const fn worker(&self) -> &ScheduledMaintenanceCycleWorkerOutcome {
        &self.worker
    }

    #[must_use]
    pub const fn is_tick_idle(&self) -> bool {
        matches!(self.tick, ScheduledMaintenanceCycleTickOutcome::Idle)
    }

    #[must_use]
    pub const fn is_worker_idle(&self) -> bool {
        self.worker.is_idle()
    }

    #[must_use]
    pub const fn is_idle(&self) -> bool {
        self.is_tick_idle() && self.is_worker_idle()
    }
}

/// Service for manually invoking a bounded scheduled-maintenance cycle.
#[derive(Clone)]
pub struct ScheduledMaintenanceCycleService {
    pool: DatabasePool,
}

impl ScheduledMaintenanceCycleService {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    #[must_use]
    pub const fn pool(&self) -> &DatabasePool {
        &self.pool
    }

    /// Execute one bounded cycle: one scheduler tick followed by one worker step.
    ///
    /// The caller supplies the observed time and worker identity. The same
    /// observed time is used for both phases where semantics permit.
    ///
    /// # Errors
    ///
    /// Returns `ScheduledMaintenanceCycleError::Scheduler` if the scheduler tick
    /// fails; the worker step is not executed in that case.
    ///
    /// Returns `ScheduledMaintenanceCycleError::Worker` if the scheduler tick
    /// succeeds but the worker step fails; the scheduler's durable changes are
    /// preserved.
    pub async fn run_scheduled_maintenance_cycle(
        &self,
        worker_id: BackupScheduledMaintenanceWorkerId,
        observed_at_utc: Timestamp,
        lease_duration_seconds: u64,
    ) -> Result<ScheduledMaintenanceCycleResult, ScheduledMaintenanceCycleError> {
        // Phase 1: Scheduler tick
        let scheduler_service = BackupSchedulerService::new(self.pool.clone());
        let tick_result = scheduler_service
            .run_scheduler_tick(observed_at_utc)
            .await?;
        let tick_outcome = tick_result.into();

        // Phase 2: Worker step (only if tick succeeded)
        let worker_service = ScheduledMaintenanceWorkerService::new(self.pool.clone());
        let worker_outcome = worker_service
            .run_scheduled_maintenance_worker_step(
                worker_id,
                observed_at_utc,
                lease_duration_seconds,
            )
            .await?;
        let worker_result = if worker_outcome.is_idle() {
            ScheduledMaintenanceCycleWorkerOutcome::Idle
        } else {
            ScheduledMaintenanceCycleWorkerOutcome::Stepped(Box::new(worker_outcome))
        };

        Ok(ScheduledMaintenanceCycleResult {
            tick: tick_outcome,
            worker: worker_result,
        })
    }
}

impl BackupSchedulerService {
    /// Convenience boundary for callers that hold a `BackupSchedulerService`.
    /// Delegates to `ScheduledMaintenanceCycleService`.
    pub async fn run_scheduled_maintenance_cycle(
        &self,
        worker_id: BackupScheduledMaintenanceWorkerId,
        observed_at_utc: Timestamp,
        lease_duration_seconds: u64,
    ) -> Result<ScheduledMaintenanceCycleResult, ScheduledMaintenanceCycleError> {
        ScheduledMaintenanceCycleService::new(self.pool().clone())
            .run_scheduled_maintenance_cycle(worker_id, observed_at_utc, lease_duration_seconds)
            .await
    }
}

impl ScheduledMaintenanceWorkerService {
    /// Convenience boundary for callers that hold a `ScheduledMaintenanceWorkerService`.
    /// Delegates to `ScheduledMaintenanceCycleService`.
    pub async fn run_scheduled_maintenance_cycle(
        &self,
        worker_id: BackupScheduledMaintenanceWorkerId,
        observed_at_utc: Timestamp,
        lease_duration_seconds: u64,
    ) -> Result<ScheduledMaintenanceCycleResult, ScheduledMaintenanceCycleError> {
        ScheduledMaintenanceCycleService::new(self.pool().clone())
            .run_scheduled_maintenance_cycle(worker_id, observed_at_utc, lease_duration_seconds)
            .await
    }
}

impl BackupService {
    /// Convenience boundary for callers that hold a `BackupService`.
    /// Delegates to `ScheduledMaintenanceCycleService`.
    pub async fn run_scheduled_maintenance_cycle(
        &self,
        worker_id: BackupScheduledMaintenanceWorkerId,
        observed_at_utc: Timestamp,
        lease_duration_seconds: u64,
    ) -> Result<ScheduledMaintenanceCycleResult, ScheduledMaintenanceCycleError> {
        ScheduledMaintenanceCycleService::new(self.pool().clone())
            .run_scheduled_maintenance_cycle(worker_id, observed_at_utc, lease_duration_seconds)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_result_distinguishes_tick_and_worker() {
        let tick = ScheduledMaintenanceCycleTickOutcome::Idle;
        let worker = ScheduledMaintenanceCycleWorkerOutcome::Idle;
        let result = ScheduledMaintenanceCycleResult { tick, worker };
        assert!(result.is_idle());
    }

    #[test]
    fn tick_outcome_preserves_scheduler_result() {
        assert_eq!(
            ScheduledMaintenanceCycleTickOutcome::from(BackupSchedulerTickResult::Idle),
            ScheduledMaintenanceCycleTickOutcome::Idle
        );
    }
}
