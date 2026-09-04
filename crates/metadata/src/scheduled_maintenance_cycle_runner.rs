//! Application-level service integration boundary for manually invoking one
//! bounded scheduled-maintenance cycle.
//!
//! `ScheduledMaintenanceCycleRunner` provides a narrow, production-oriented
//! caller that composes exactly one scheduler tick and exactly one scheduled-
//! maintenance worker step through the canonical Prompt 67 cycle. It
//! establishes the integration point for future explicitly authorized
//! lifecycle callers without introducing autonomous scheduling, polling,
//! daemon loops, heartbeat renewal, retry/backoff, or background execution.
//!
//! The runner holds a process-scoped worker identity that is reused across
//! all invocations from the same process, matching the Prompt 66 identity
//! model. A fresh identity is created once at construction time and never
//! persisted to a dedicated identity table.
//!
//! # Invariants
//!
//! - exactly one scheduler tick per invocation
//! - exactly one worker step per invocation (at most one maintenance transition)
//! - explicit time injection; no hidden `Utc::now()` in the orchestration layer
//! - canonical lease bounds validated without silent clamping
//! - scheduler failure prevents worker execution
//! - worker failure preserves scheduler durable commits
//! - no loop, spawn, sleep, interval, heartbeat, retry, cron, timer, cursor,
//!   cache, daemon, API, or UI exposure
//!
//! # Known concurrency limitation
//!
//! Concurrent cycle invocations may encounter PostgreSQL lock-order contention
//! under heavy pile-on. The runner does not hide this with automatic retry,
//! process-local serialization, or application-global mutex. Deadlock aborts
//! remain bounded invocation failures.

use synveil_core::{
    BackupScheduledMaintenanceWorkerId, Timestamp, validate_scheduled_maintenance_lease_duration,
};

use crate::{
    DatabasePool, ScheduledMaintenanceCycleError, ScheduledMaintenanceCycleResult,
    ScheduledMaintenanceCycleService,
};

/// Application-level runner for manually invoking one bounded scheduled-
/// maintenance cycle.
///
/// The runner composes exactly one scheduler tick followed by exactly one
/// worker step, delegating to the canonical Prompt 67
/// `ScheduledMaintenanceCycleService`. A process-scoped worker identity
/// is created once at construction and reused for all invocations.
///
/// # Process identity lifecycle
///
/// For one manually invoked service process, the worker identity is created
/// at the appropriate service-lifecycle boundary and reused for subsequent
/// manual invocations by that process. The identity is not persisted to a
/// dedicated table and is not exposed as business/user-visible identity.
#[derive(Clone)]
pub struct ScheduledMaintenanceCycleRunner {
    pool: DatabasePool,
    worker_id: BackupScheduledMaintenanceWorkerId,
}

impl ScheduledMaintenanceCycleRunner {
    /// Construct a new runner with a fresh process-scoped worker identity.
    ///
    /// The worker identity is created once and reused for all subsequent
    /// invocations from this runner instance.
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self {
            pool,
            worker_id: BackupScheduledMaintenanceWorkerId::new(),
        }
    }

    /// Construct a runner with an explicit worker identity. Use this when
    /// the caller needs to supply a specific identity (e.g., for testing
    /// or when the identity is derived from external process metadata).
    #[must_use]
    pub fn with_worker_id(
        pool: DatabasePool,
        worker_id: BackupScheduledMaintenanceWorkerId,
    ) -> Self {
        Self { pool, worker_id }
    }

    /// Access the underlying database pool.
    #[must_use]
    pub const fn pool(&self) -> &DatabasePool {
        &self.pool
    }

    /// Access the runner's worker identity.
    #[must_use]
    pub const fn worker_id(&self) -> &BackupScheduledMaintenanceWorkerId {
        &self.worker_id
    }

    /// Execute one bounded scheduled-maintenance cycle.
    ///
    /// This composes exactly one scheduler tick (Prompt 64–65) followed by
    /// exactly one scheduled-maintenance worker step (Prompt 66). The caller
    /// supplies the observation timestamp; no hidden wall-clock reads appear
    /// in the orchestration layer.
    ///
    /// The lease duration is validated against canonical bounds
    /// (10–900 seconds) without silent clamping. Out-of-range values
    /// are rejected.
    ///
    /// # Arguments
    ///
    /// * `observed_at_utc` - The explicit observation timestamp used for
    ///   scheduler due evaluation and claim lease timing.
    /// * `lease_duration_seconds` - The bounded lease duration for the
    ///   worker claim. Must be within 10–900 seconds.
    ///
    /// # Returns
    ///
    /// The typed `ScheduledMaintenanceCycleResult` containing distinct
    /// tick and worker outcomes.
    ///
    /// # Errors
    ///
    /// Returns `ScheduledMaintenanceCycleError::Scheduler` if the scheduler
    /// tick fails; the worker step is not executed in that case.
    ///
    /// Returns `ScheduledMaintenanceCycleError::Worker` if the scheduler
    /// tick succeeds but the worker step fails; the scheduler's durable
    /// changes are preserved.
    ///
    /// Returns a lease-duration validation error if the provided duration
    /// is outside the canonical bounds.
    pub async fn run_one_scheduled_backup_maintenance_cycle(
        &self,
        observed_at_utc: Timestamp,
        lease_duration_seconds: u64,
    ) -> Result<ScheduledMaintenanceCycleResult, ScheduledMaintenanceCycleError> {
        // Validate lease duration against canonical bounds (no clamping).
        validate_scheduled_maintenance_lease_duration(lease_duration_seconds)
            .map_err(|_| ScheduledMaintenanceCycleError::InvalidLeaseDuration)?;

        // Delegate to the canonical Prompt 67 cycle service.
        let cycle_service = ScheduledMaintenanceCycleService::new(self.pool.clone());
        cycle_service
            .run_scheduled_maintenance_cycle(
                self.worker_id,
                observed_at_utc,
                lease_duration_seconds,
            )
            .await
    }
}

/// Typed error for lease-duration validation failures at the runner boundary.
///
/// This wraps the core lease validation error to preserve the runner's
/// independent error boundary without conflating it with scheduler or
/// worker failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduledMaintenanceCycleRunnerError {
    /// The provided lease duration is outside the canonical bounds.
    InvalidLeaseDuration,
    /// The underlying cycle failed with a scheduler error.
    Cycle(ScheduledMaintenanceCycleError),
}

impl std::fmt::Display for ScheduledMaintenanceCycleRunnerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLeaseDuration => {
                formatter.write_str("lease duration is outside the valid range (10–900 seconds)")
            }
            Self::Cycle(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ScheduledMaintenanceCycleRunnerError {}

impl From<ScheduledMaintenanceCycleError> for ScheduledMaintenanceCycleRunnerError {
    fn from(error: ScheduledMaintenanceCycleError) -> Self {
        Self::Cycle(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_error_display() {
        let error = ScheduledMaintenanceCycleRunnerError::InvalidLeaseDuration;
        assert!(error.to_string().contains("10"));
        assert!(error.to_string().contains("900"));
    }

    #[test]
    fn runner_error_from_cycle_error() {
        let cycle_error = ScheduledMaintenanceCycleError::InvalidLeaseDuration;
        let runner_error = ScheduledMaintenanceCycleRunnerError::from(cycle_error);
        assert!(matches!(
            runner_error,
            ScheduledMaintenanceCycleRunnerError::Cycle(
                ScheduledMaintenanceCycleError::InvalidLeaseDuration
            )
        ));
    }
}
