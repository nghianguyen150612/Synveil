use std::{
    env, fmt,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{NodeState, Timestamp};

/// The single supported environment override for the logical Trash retention
/// policy. The value is an unsigned number of seconds so configuration is
/// explicit and does not depend on a locale or an ambiguous calendar unit.
pub const TRASH_RETENTION_SECONDS_ENV: &str = "SYNVEIL_TRASH_RETENTION_SECONDS";

/// The authoritative object-GC grace-period override. The value is an
/// unsigned number of seconds and is evaluated against PostgreSQL/server time
/// by the metadata planning service.
pub const OBJECT_GC_GRACE_SECONDS_ENV: &str = "SYNVEIL_OBJECT_GC_GRACE_SECONDS";

/// The authoritative object-GC worker-lease override. The value is an
/// unsigned number of seconds and must be greater than zero.
pub const OBJECT_GC_LEASE_SECONDS_ENV: &str = "SYNVEIL_OBJECT_GC_LEASE_SECONDS";

/// The bounded object-GC claim batch-size override.
pub const OBJECT_GC_MAX_BATCH_SIZE_ENV: &str = "SYNVEIL_OBJECT_GC_MAX_BATCH_SIZE";

/// Enables the internal object-GC worker. Disabled is the conservative default
/// until an operator has configured both PostgreSQL and an ObjectStore root.
pub const GC_WORKER_ENABLED_ENV: &str = "SYNVEIL_GC_WORKER_ENABLED";

/// Bounded interval between internal object-GC worker cycles, in seconds.
pub const GC_WORKER_CYCLE_INTERVAL_SECONDS_ENV: &str = "SYNVEIL_GC_WORKER_CYCLE_INTERVAL_SECONDS";

/// Maximum number of new object-GC candidates the worker may claim per cycle.
pub const GC_WORKER_MAX_CANDIDATE_CLAIMS_ENV: &str = "SYNVEIL_GC_WORKER_MAX_CANDIDATE_CLAIMS";

/// Maximum number of active physical operations the worker may advance per
/// cycle. Recovery work consumes this same bounded budget before new work.
pub const GC_WORKER_MAX_ACTIVE_OPERATIONS_ENV: &str = "SYNVEIL_GC_WORKER_MAX_ACTIVE_OPERATIONS";

/// Maximum number of replica actions the worker may initiate or reconcile in
/// one cycle.
pub const GC_WORKER_MAX_REPLICA_ACTIONS_ENV: &str = "SYNVEIL_GC_WORKER_MAX_REPLICA_ACTIONS";

/// Maximum number of operation futures allowed to execute concurrently.
pub const GC_WORKER_MAX_CONCURRENT_EXECUTIONS_ENV: &str =
    "SYNVEIL_GC_WORKER_MAX_CONCURRENT_EXECUTIONS";

/// Maximum number of concurrent worker-owned physical replica deletions.
pub const GC_WORKER_MAX_CONCURRENT_REPLICA_DELETES_ENV: &str =
    "SYNVEIL_GC_WORKER_MAX_CONCURRENT_REPLICA_DELETES";

/// Initial bounded retry delay for a retryable physical-GC action, in seconds.
pub const GC_WORKER_RETRY_BASE_SECONDS_ENV: &str = "SYNVEIL_GC_WORKER_RETRY_BASE_SECONDS";

/// Maximum bounded retry delay for a retryable physical-GC action, in seconds.
pub const GC_WORKER_RETRY_MAX_SECONDS_ENV: &str = "SYNVEIL_GC_WORKER_RETRY_MAX_SECONDS";

/// Maximum durable attempts before a retryable action becomes intervention
/// required rather than spinning indefinitely.
pub const GC_WORKER_MAX_ATTEMPTS_ENV: &str = "SYNVEIL_GC_WORKER_MAX_ATTEMPTS";

/// Maximum amount of time the runtime gives one bounded cycle to settle during
/// graceful shutdown, in seconds.
pub const GC_WORKER_SHUTDOWN_TIMEOUT_SECONDS_ENV: &str =
    "SYNVEIL_GC_WORKER_SHUTDOWN_TIMEOUT_SECONDS";

/// Conservative personal-cloud default. This is a configurable deployment
/// default, not an immutable protocol guarantee.
pub const DEFAULT_TRASH_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// A conservative delay between an object becoming unreferenced and its first
/// metadata-only GC claim. This is not permission to delete physical bytes.
pub const DEFAULT_OBJECT_GC_GRACE_PERIOD: Duration = Duration::from_secs(24 * 60 * 60);

/// A bounded lease long enough for one short planning/revalidation cycle but
/// short enough that a crashed worker does not block progress indefinitely.
pub const DEFAULT_OBJECT_GC_LEASE_DURATION: Duration = Duration::from_secs(15 * 60);

/// Default maximum number of candidates one claim transaction may lease.
pub const DEFAULT_OBJECT_GC_MAX_BATCH_SIZE: u32 = 100;

/// Hard upper bound for one object-GC claim batch.
pub const MAX_OBJECT_GC_BATCH_SIZE: u32 = 500;

/// The worker remains opt-in until the deployment has explicitly configured
/// its internal PostgreSQL and ObjectStore dependencies.
pub const DEFAULT_GC_WORKER_ENABLED: bool = false;

/// One minute avoids high-frequency polling while keeping recovery latency
/// reasonable for a small personal deployment.
pub const DEFAULT_GC_WORKER_CYCLE_INTERVAL: Duration = Duration::from_secs(60);

/// Conservative defaults intentionally favor bounded recovery over throughput.
pub const DEFAULT_GC_WORKER_MAX_CANDIDATE_CLAIMS: u32 = 8;
pub const DEFAULT_GC_WORKER_MAX_ACTIVE_OPERATIONS: u32 = 2;
pub const DEFAULT_GC_WORKER_MAX_REPLICA_ACTIONS: u32 = 4;
pub const DEFAULT_GC_WORKER_MAX_CONCURRENT_EXECUTIONS: u32 = 2;
pub const DEFAULT_GC_WORKER_MAX_CONCURRENT_REPLICA_DELETES: u32 = 1;
pub const DEFAULT_GC_WORKER_RETRY_BASE: Duration = Duration::from_secs(30);
pub const DEFAULT_GC_WORKER_RETRY_MAX: Duration = Duration::from_secs(15 * 60);
pub const DEFAULT_GC_WORKER_MAX_ATTEMPTS: u32 = 12;
pub const DEFAULT_GC_WORKER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

/// Hard bounds prevent an environment typo from turning a maintenance worker
/// into an unbounded deletion scheduler.
pub const MAX_GC_WORKER_CYCLE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
pub const MAX_GC_WORKER_CANDIDATE_CLAIMS: u32 = 100;
pub const MAX_GC_WORKER_ACTIVE_OPERATIONS: u32 = 32;
pub const MAX_GC_WORKER_REPLICA_ACTIONS: u32 = 100;
pub const MAX_GC_WORKER_CONCURRENT_EXECUTIONS: u32 = 16;
pub const MAX_GC_WORKER_CONCURRENT_REPLICA_DELETES: u32 = 16;
pub const MAX_GC_WORKER_RETRY_DELAY: Duration = Duration::from_secs(24 * 60 * 60);
pub const MAX_GC_WORKER_ATTEMPTS: u32 = 100;
pub const MAX_GC_WORKER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrashRetentionPolicyError {
    ZeroDuration,
    DurationOutOfRange,
    InvalidEnvironmentValue,
}

impl fmt::Display for TrashRetentionPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::ZeroDuration => "Trash retention duration must be greater than zero",
            Self::DurationOutOfRange => "Trash retention duration is out of range",
            Self::InvalidEnvironmentValue => {
                "Trash retention configuration must be an unsigned number of seconds"
            }
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for TrashRetentionPolicyError {}

/// Authoritative logical Trash retention policy shared by metadata and API
/// composition roots. It contains no storage, HTTP, or operating-system
/// assumptions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrashRetentionPolicy {
    retention_duration: Duration,
}

impl TrashRetentionPolicy {
    pub fn new(retention_duration: Duration) -> Result<Self, TrashRetentionPolicyError> {
        if retention_duration.is_zero() {
            return Err(TrashRetentionPolicyError::ZeroDuration);
        }
        if retention_duration.as_secs() > i64::MAX as u64
            || (retention_duration.as_secs() == i64::MAX as u64
                && retention_duration.subsec_nanos() != 0)
        {
            return Err(TrashRetentionPolicyError::DurationOutOfRange);
        }
        Ok(Self { retention_duration })
    }

    pub fn from_env() -> Result<Self, TrashRetentionPolicyError> {
        let Ok(value) = env::var(TRASH_RETENTION_SECONDS_ENV) else {
            return Ok(Self::default());
        };
        let seconds = value
            .parse::<u64>()
            .map_err(|_| TrashRetentionPolicyError::InvalidEnvironmentValue)?;
        Self::new(Duration::from_secs(seconds))
    }

    #[must_use]
    pub const fn retention_duration(self) -> Duration {
        self.retention_duration
    }

    /// Calculate the user-visible restore deadline without persisting a
    /// redundant derived timestamp.
    #[must_use]
    pub fn restore_deadline(self, trashed_at: Timestamp) -> Option<Timestamp> {
        trashed_at.checked_add_std(self.retention_duration)
    }

    /// Calculate the inclusive timestamp cutoff used by bounded candidate
    /// scans. The comparison is equivalent to `now >= trashed_at + retention`.
    #[must_use]
    pub fn eligibility_cutoff(self, now: Timestamp) -> Option<Timestamp> {
        now.checked_sub_std(self.retention_duration)
    }

    /// Evaluate only canonical state and server-observed timestamps. Client
    /// timestamps are not inputs to this rule.
    #[must_use]
    pub fn is_purge_eligible(
        self,
        state: NodeState,
        is_root: bool,
        trashed_at: Option<Timestamp>,
        now: Timestamp,
    ) -> bool {
        if is_root || state != NodeState::Trashed {
            return false;
        }
        let Some(trashed_at) = trashed_at else {
            return false;
        };
        let Some(deadline) = self.restore_deadline(trashed_at) else {
            return false;
        };
        now >= deadline
    }
}

impl Default for TrashRetentionPolicy {
    fn default() -> Self {
        Self {
            retention_duration: DEFAULT_TRASH_RETENTION,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectGcPolicyError {
    ZeroGracePeriod,
    ZeroLeaseDuration,
    GracePeriodOutOfRange,
    LeaseDurationOutOfRange,
    BatchSizeOutOfRange,
    InvalidEnvironmentValue,
}

impl fmt::Display for ObjectGcPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::ZeroGracePeriod => "object GC grace period must be greater than zero",
            Self::ZeroLeaseDuration => "object GC lease duration must be greater than zero",
            Self::GracePeriodOutOfRange => "object GC grace period is out of range",
            Self::LeaseDurationOutOfRange => "object GC lease duration is out of range",
            Self::BatchSizeOutOfRange => "object GC batch size is out of range",
            Self::InvalidEnvironmentValue => {
                "object GC configuration must use unsigned decimal values"
            }
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ObjectGcPolicyError {}

/// Authoritative, transport-neutral policy for metadata-only object-GC
/// planning. The policy deliberately contains no ObjectStore or filesystem
/// behavior; physical deletion is a later release gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectGcPolicy {
    grace_period: Duration,
    lease_duration: Duration,
    max_batch_size: u32,
}

impl ObjectGcPolicy {
    pub fn new(
        grace_period: Duration,
        lease_duration: Duration,
        max_batch_size: u32,
    ) -> Result<Self, ObjectGcPolicyError> {
        validate_gc_duration(
            grace_period,
            ObjectGcPolicyError::ZeroGracePeriod,
            ObjectGcPolicyError::GracePeriodOutOfRange,
        )?;
        validate_gc_duration(
            lease_duration,
            ObjectGcPolicyError::ZeroLeaseDuration,
            ObjectGcPolicyError::LeaseDurationOutOfRange,
        )?;
        if !(1..=MAX_OBJECT_GC_BATCH_SIZE).contains(&max_batch_size) {
            return Err(ObjectGcPolicyError::BatchSizeOutOfRange);
        }
        Ok(Self {
            grace_period,
            lease_duration,
            max_batch_size,
        })
    }

    pub fn from_env() -> Result<Self, ObjectGcPolicyError> {
        let grace_period =
            env_duration(OBJECT_GC_GRACE_SECONDS_ENV, DEFAULT_OBJECT_GC_GRACE_PERIOD)?;
        let lease_duration = env_duration(
            OBJECT_GC_LEASE_SECONDS_ENV,
            DEFAULT_OBJECT_GC_LEASE_DURATION,
        )?;
        let max_batch_size = match env::var(OBJECT_GC_MAX_BATCH_SIZE_ENV) {
            Ok(value) => value
                .parse::<u32>()
                .map_err(|_| ObjectGcPolicyError::InvalidEnvironmentValue)?,
            Err(env::VarError::NotPresent) => DEFAULT_OBJECT_GC_MAX_BATCH_SIZE,
            Err(env::VarError::NotUnicode(_)) => {
                return Err(ObjectGcPolicyError::InvalidEnvironmentValue);
            }
        };
        Self::new(grace_period, lease_duration, max_batch_size)
    }

    #[must_use]
    pub const fn grace_period(self) -> Duration {
        self.grace_period
    }

    #[must_use]
    pub const fn lease_duration(self) -> Duration {
        self.lease_duration
    }

    #[must_use]
    pub const fn max_batch_size(self) -> u32 {
        self.max_batch_size
    }

    /// The grace boundary is inclusive: `now >= unreferenced_at + grace`.
    #[must_use]
    pub fn grace_deadline(self, unreferenced_at: Timestamp) -> Option<Timestamp> {
        unreferenced_at.checked_add_std(self.grace_period)
    }

    #[must_use]
    pub fn lease_expiry(self, acquired_at: Timestamp) -> Option<Timestamp> {
        acquired_at.checked_add_std(self.lease_duration)
    }

    #[must_use]
    pub fn is_grace_matured(self, unreferenced_at: Timestamp, now: Timestamp) -> bool {
        self.grace_deadline(unreferenced_at)
            .is_some_and(|deadline| now >= deadline)
    }
}

impl Default for ObjectGcPolicy {
    fn default() -> Self {
        Self {
            grace_period: DEFAULT_OBJECT_GC_GRACE_PERIOD,
            lease_duration: DEFAULT_OBJECT_GC_LEASE_DURATION,
            max_batch_size: DEFAULT_OBJECT_GC_MAX_BATCH_SIZE,
        }
    }
}

/// Closed validation errors for the internal GC-worker retry policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GcWorkerRetryPolicyError {
    ZeroBaseDelay,
    BaseDelayOutOfRange,
    MaxDelayOutOfRange,
    MaxDelayBeforeBaseDelay,
    AttemptLimitOutOfRange,
}

impl fmt::Display for GcWorkerRetryPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::ZeroBaseDelay => "GC worker retry base delay must be greater than zero",
            Self::BaseDelayOutOfRange => "GC worker retry base delay is out of range",
            Self::MaxDelayOutOfRange => "GC worker retry maximum delay is out of range",
            Self::MaxDelayBeforeBaseDelay => {
                "GC worker retry maximum delay must not be less than the base delay"
            }
            Self::AttemptLimitOutOfRange => "GC worker retry attempt limit is out of range",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for GcWorkerRetryPolicyError {}

/// Bounded exponential retry policy shared by the orchestration worker and
/// durable physical-GC action metadata. The salt makes the small jitter stable
/// per action: restarts preserve the same schedule without requiring an
/// in-memory random source or exposing host identity as a correctness input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GcWorkerRetryPolicy {
    base_delay: Duration,
    max_delay: Duration,
    max_attempts: u32,
}

impl GcWorkerRetryPolicy {
    pub fn new(
        base_delay: Duration,
        max_delay: Duration,
        max_attempts: u32,
    ) -> Result<Self, GcWorkerRetryPolicyError> {
        if base_delay.is_zero() {
            return Err(GcWorkerRetryPolicyError::ZeroBaseDelay);
        }
        if !valid_worker_duration(base_delay, MAX_GC_WORKER_RETRY_DELAY) {
            return Err(GcWorkerRetryPolicyError::BaseDelayOutOfRange);
        }
        if !valid_worker_duration(max_delay, MAX_GC_WORKER_RETRY_DELAY) {
            return Err(GcWorkerRetryPolicyError::MaxDelayOutOfRange);
        }
        if max_delay < base_delay {
            return Err(GcWorkerRetryPolicyError::MaxDelayBeforeBaseDelay);
        }
        if !(1..=MAX_GC_WORKER_ATTEMPTS).contains(&max_attempts) {
            return Err(GcWorkerRetryPolicyError::AttemptLimitOutOfRange);
        }
        Ok(Self {
            base_delay,
            max_delay,
            max_attempts,
        })
    }

    #[must_use]
    pub const fn base_delay(self) -> Duration {
        self.base_delay
    }

    #[must_use]
    pub const fn max_delay(self) -> Duration {
        self.max_delay
    }

    #[must_use]
    pub const fn max_attempts(self) -> u32 {
        self.max_attempts
    }

    /// Return a deterministic, bounded exponential delay for an already
    /// recorded action attempt. `attempt_count` is one-based; zero is treated
    /// as the first attempt defensively. Jitter is at most 10% and never moves
    /// the durable schedule past `max_delay`.
    #[must_use]
    pub fn delay_for_attempt(self, attempt_count: u32, salt: u64) -> Duration {
        let exponent = attempt_count.saturating_sub(1).min(63);
        let multiplier = 1_u128 << exponent;
        let maximum_nanos = self.max_delay.as_nanos();
        let exponential_nanos = self
            .base_delay
            .as_nanos()
            .saturating_mul(multiplier)
            .min(maximum_nanos);
        let jitter_ceiling = exponential_nanos / 10;
        let jitter = if jitter_ceiling == 0 {
            0
        } else {
            u128::from(salt) % (jitter_ceiling + 1)
        };
        let bounded_nanos = exponential_nanos.saturating_add(jitter).min(maximum_nanos);
        // All accepted worker delays are at most one day, so this conversion
        // remains representable by `Duration::from_nanos`.
        Duration::from_nanos(u64::try_from(bounded_nanos).expect("bounded worker duration"))
    }
}

impl Default for GcWorkerRetryPolicy {
    fn default() -> Self {
        Self {
            base_delay: DEFAULT_GC_WORKER_RETRY_BASE,
            max_delay: DEFAULT_GC_WORKER_RETRY_MAX,
            max_attempts: DEFAULT_GC_WORKER_MAX_ATTEMPTS,
        }
    }
}

/// Closed validation errors for the internal, transport-neutral object-GC
/// worker configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GcWorkerConfigError {
    InvalidEnvironmentValue,
    CycleIntervalOutOfRange,
    CandidateClaimsOutOfRange,
    ActiveOperationsOutOfRange,
    ReplicaActionsOutOfRange,
    ConcurrentExecutionsOutOfRange,
    ConcurrentReplicaDeletesOutOfRange,
    ConcurrentExecutionsExceedActiveOperations,
    ConcurrentReplicaDeletesExceedExecutions,
    ShutdownTimeoutOutOfRange,
    RetryPolicy(GcWorkerRetryPolicyError),
}

impl fmt::Display for GcWorkerConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidEnvironmentValue => {
                "GC worker configuration must use documented boolean or unsigned decimal values"
            }
            Self::CycleIntervalOutOfRange => "GC worker cycle interval is out of range",
            Self::CandidateClaimsOutOfRange => "GC worker candidate-claim limit is out of range",
            Self::ActiveOperationsOutOfRange => "GC worker active-operation limit is out of range",
            Self::ReplicaActionsOutOfRange => "GC worker replica-action limit is out of range",
            Self::ConcurrentExecutionsOutOfRange => {
                "GC worker concurrent-execution limit is out of range"
            }
            Self::ConcurrentReplicaDeletesOutOfRange => {
                "GC worker concurrent replica-delete limit is out of range"
            }
            Self::ConcurrentExecutionsExceedActiveOperations => {
                "GC worker concurrent executions cannot exceed active operations"
            }
            Self::ConcurrentReplicaDeletesExceedExecutions => {
                "GC worker concurrent replica deletes cannot exceed concurrent executions"
            }
            Self::ShutdownTimeoutOutOfRange => "GC worker shutdown timeout is out of range",
            Self::RetryPolicy(error) => return error.fmt(formatter),
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for GcWorkerConfigError {}

/// Runtime configuration for the internal object-GC orchestration worker.
/// It deliberately has no process, host, database URL, storage path, or
/// credentials: those concerns belong to the composition root.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GcWorkerConfig {
    enabled: bool,
    cycle_interval: Duration,
    max_candidate_claims_per_cycle: u32,
    max_active_operations: u32,
    max_replica_actions_per_cycle: u32,
    max_concurrent_executions: u32,
    max_concurrent_replica_deletes: u32,
    retry_policy: GcWorkerRetryPolicy,
    shutdown_timeout: Duration,
}

impl GcWorkerConfig {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        enabled: bool,
        cycle_interval: Duration,
        max_candidate_claims_per_cycle: u32,
        max_active_operations: u32,
        max_replica_actions_per_cycle: u32,
        max_concurrent_executions: u32,
        max_concurrent_replica_deletes: u32,
        retry_policy: GcWorkerRetryPolicy,
        shutdown_timeout: Duration,
    ) -> Result<Self, GcWorkerConfigError> {
        if cycle_interval < Duration::from_secs(1)
            || !valid_worker_duration(cycle_interval, MAX_GC_WORKER_CYCLE_INTERVAL)
        {
            return Err(GcWorkerConfigError::CycleIntervalOutOfRange);
        }
        if !(1..=MAX_GC_WORKER_CANDIDATE_CLAIMS).contains(&max_candidate_claims_per_cycle) {
            return Err(GcWorkerConfigError::CandidateClaimsOutOfRange);
        }
        if !(1..=MAX_GC_WORKER_ACTIVE_OPERATIONS).contains(&max_active_operations) {
            return Err(GcWorkerConfigError::ActiveOperationsOutOfRange);
        }
        if !(1..=MAX_GC_WORKER_REPLICA_ACTIONS).contains(&max_replica_actions_per_cycle) {
            return Err(GcWorkerConfigError::ReplicaActionsOutOfRange);
        }
        if !(1..=MAX_GC_WORKER_CONCURRENT_EXECUTIONS).contains(&max_concurrent_executions) {
            return Err(GcWorkerConfigError::ConcurrentExecutionsOutOfRange);
        }
        if !(1..=MAX_GC_WORKER_CONCURRENT_REPLICA_DELETES).contains(&max_concurrent_replica_deletes)
        {
            return Err(GcWorkerConfigError::ConcurrentReplicaDeletesOutOfRange);
        }
        if max_concurrent_executions > max_active_operations {
            return Err(GcWorkerConfigError::ConcurrentExecutionsExceedActiveOperations);
        }
        if max_concurrent_replica_deletes > max_concurrent_executions {
            return Err(GcWorkerConfigError::ConcurrentReplicaDeletesExceedExecutions);
        }
        if shutdown_timeout.is_zero()
            || !valid_worker_duration(shutdown_timeout, MAX_GC_WORKER_SHUTDOWN_TIMEOUT)
        {
            return Err(GcWorkerConfigError::ShutdownTimeoutOutOfRange);
        }
        Ok(Self {
            enabled,
            cycle_interval,
            max_candidate_claims_per_cycle,
            max_active_operations,
            max_replica_actions_per_cycle,
            max_concurrent_executions,
            max_concurrent_replica_deletes,
            retry_policy,
            shutdown_timeout,
        })
    }

    pub fn from_env() -> Result<Self, GcWorkerConfigError> {
        let enabled = worker_env_bool(GC_WORKER_ENABLED_ENV, DEFAULT_GC_WORKER_ENABLED)?;
        let cycle_interval = worker_env_duration(
            GC_WORKER_CYCLE_INTERVAL_SECONDS_ENV,
            DEFAULT_GC_WORKER_CYCLE_INTERVAL,
        )?;
        let max_candidate_claims_per_cycle = worker_env_u32(
            GC_WORKER_MAX_CANDIDATE_CLAIMS_ENV,
            DEFAULT_GC_WORKER_MAX_CANDIDATE_CLAIMS,
        )?;
        let max_active_operations = worker_env_u32(
            GC_WORKER_MAX_ACTIVE_OPERATIONS_ENV,
            DEFAULT_GC_WORKER_MAX_ACTIVE_OPERATIONS,
        )?;
        let max_replica_actions_per_cycle = worker_env_u32(
            GC_WORKER_MAX_REPLICA_ACTIONS_ENV,
            DEFAULT_GC_WORKER_MAX_REPLICA_ACTIONS,
        )?;
        let max_concurrent_executions = worker_env_u32(
            GC_WORKER_MAX_CONCURRENT_EXECUTIONS_ENV,
            DEFAULT_GC_WORKER_MAX_CONCURRENT_EXECUTIONS,
        )?;
        let max_concurrent_replica_deletes = worker_env_u32(
            GC_WORKER_MAX_CONCURRENT_REPLICA_DELETES_ENV,
            DEFAULT_GC_WORKER_MAX_CONCURRENT_REPLICA_DELETES,
        )?;
        let retry_policy = GcWorkerRetryPolicy::new(
            worker_env_duration(
                GC_WORKER_RETRY_BASE_SECONDS_ENV,
                DEFAULT_GC_WORKER_RETRY_BASE,
            )?,
            worker_env_duration(GC_WORKER_RETRY_MAX_SECONDS_ENV, DEFAULT_GC_WORKER_RETRY_MAX)?,
            worker_env_u32(GC_WORKER_MAX_ATTEMPTS_ENV, DEFAULT_GC_WORKER_MAX_ATTEMPTS)?,
        )
        .map_err(GcWorkerConfigError::RetryPolicy)?;
        let shutdown_timeout = worker_env_duration(
            GC_WORKER_SHUTDOWN_TIMEOUT_SECONDS_ENV,
            DEFAULT_GC_WORKER_SHUTDOWN_TIMEOUT,
        )?;
        Self::new(
            enabled,
            cycle_interval,
            max_candidate_claims_per_cycle,
            max_active_operations,
            max_replica_actions_per_cycle,
            max_concurrent_executions,
            max_concurrent_replica_deletes,
            retry_policy,
            shutdown_timeout,
        )
    }

    #[must_use]
    pub const fn enabled(self) -> bool {
        self.enabled
    }

    #[must_use]
    pub const fn cycle_interval(self) -> Duration {
        self.cycle_interval
    }

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
    pub const fn retry_policy(self) -> GcWorkerRetryPolicy {
        self.retry_policy
    }

    #[must_use]
    pub const fn shutdown_timeout(self) -> Duration {
        self.shutdown_timeout
    }
}

impl Default for GcWorkerConfig {
    fn default() -> Self {
        Self {
            enabled: DEFAULT_GC_WORKER_ENABLED,
            cycle_interval: DEFAULT_GC_WORKER_CYCLE_INTERVAL,
            max_candidate_claims_per_cycle: DEFAULT_GC_WORKER_MAX_CANDIDATE_CLAIMS,
            max_active_operations: DEFAULT_GC_WORKER_MAX_ACTIVE_OPERATIONS,
            max_replica_actions_per_cycle: DEFAULT_GC_WORKER_MAX_REPLICA_ACTIONS,
            max_concurrent_executions: DEFAULT_GC_WORKER_MAX_CONCURRENT_EXECUTIONS,
            max_concurrent_replica_deletes: DEFAULT_GC_WORKER_MAX_CONCURRENT_REPLICA_DELETES,
            retry_policy: GcWorkerRetryPolicy::default(),
            shutdown_timeout: DEFAULT_GC_WORKER_SHUTDOWN_TIMEOUT,
        }
    }
}

fn valid_worker_duration(value: Duration, maximum: Duration) -> bool {
    !value.is_zero()
        && value <= maximum
        && value.as_secs() <= i64::MAX as u64
        && value.subsec_nanos().is_multiple_of(1_000)
}

fn worker_env_bool(variable: &'static str, default: bool) -> Result<bool, GcWorkerConfigError> {
    let value = match env::var(variable) {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => return Ok(default),
        Err(env::VarError::NotUnicode(_)) => {
            return Err(GcWorkerConfigError::InvalidEnvironmentValue);
        }
    };
    match value.as_str() {
        "true" | "TRUE" | "True" => Ok(true),
        "false" | "FALSE" | "False" => Ok(false),
        _ => Err(GcWorkerConfigError::InvalidEnvironmentValue),
    }
}

fn worker_env_duration(
    variable: &'static str,
    default: Duration,
) -> Result<Duration, GcWorkerConfigError> {
    let seconds = worker_env_u64(variable, default.as_secs())?;
    Ok(Duration::from_secs(seconds))
}

fn worker_env_u32(variable: &'static str, default: u32) -> Result<u32, GcWorkerConfigError> {
    worker_env_u64(variable, u64::from(default))?
        .try_into()
        .map_err(|_| GcWorkerConfigError::InvalidEnvironmentValue)
}

fn worker_env_u64(variable: &'static str, default: u64) -> Result<u64, GcWorkerConfigError> {
    match env::var(variable) {
        Ok(value) => value
            .parse::<u64>()
            .map_err(|_| GcWorkerConfigError::InvalidEnvironmentValue),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(env::VarError::NotUnicode(_)) => Err(GcWorkerConfigError::InvalidEnvironmentValue),
    }
}

fn validate_gc_duration(
    value: Duration,
    zero_error: ObjectGcPolicyError,
    range_error: ObjectGcPolicyError,
) -> Result<(), ObjectGcPolicyError> {
    if value.is_zero() {
        return Err(zero_error);
    }
    if value.as_secs() > i64::MAX as u64
        || (value.as_secs() == i64::MAX as u64 && value.subsec_nanos() != 0)
        || !value.subsec_nanos().is_multiple_of(1_000)
    {
        return Err(range_error);
    }
    Ok(())
}

fn env_duration(
    variable: &'static str,
    default: Duration,
) -> Result<Duration, ObjectGcPolicyError> {
    let value = match env::var(variable) {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => return Ok(default),
        Err(env::VarError::NotUnicode(_)) => {
            return Err(ObjectGcPolicyError::InvalidEnvironmentValue);
        }
    };
    let seconds = value
        .parse::<u64>()
        .map_err(|_| ObjectGcPolicyError::InvalidEnvironmentValue)?;
    Ok(Duration::from_secs(seconds))
}

macro_rules! logical_path {
    ($name:ident) => {
        /// A logical path supplied by configuration; platform resolution is external to core.
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(PathBuf);

        impl $name {
            #[must_use]
            pub fn new(path: impl Into<PathBuf>) -> Self {
                Self(path.into())
            }

            #[must_use]
            pub fn as_path(&self) -> &Path {
                &self.0
            }

            #[must_use]
            pub fn into_path_buf(self) -> PathBuf {
                self.0
            }
        }

        impl From<PathBuf> for $name {
            fn from(path: PathBuf) -> Self {
                Self::new(path)
            }
        }

        impl AsRef<Path> for $name {
            fn as_ref(&self) -> &Path {
                self.as_path()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.display().fmt(formatter)
            }
        }
    };
}

logical_path!(DataDir);
logical_path!(ConfigDir);
logical_path!(CacheDir);
logical_path!(RuntimeDir);

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use crate::{NodeState, Timestamp};

    use super::{
        CacheDir, ConfigDir, DEFAULT_GC_WORKER_CYCLE_INTERVAL,
        DEFAULT_GC_WORKER_MAX_CONCURRENT_REPLICA_DELETES, DEFAULT_GC_WORKER_RETRY_BASE,
        DEFAULT_GC_WORKER_RETRY_MAX, DEFAULT_OBJECT_GC_GRACE_PERIOD,
        DEFAULT_OBJECT_GC_LEASE_DURATION, DEFAULT_OBJECT_GC_MAX_BATCH_SIZE,
        DEFAULT_TRASH_RETENTION, DataDir, GcWorkerConfig, GcWorkerConfigError, GcWorkerRetryPolicy,
        GcWorkerRetryPolicyError, MAX_OBJECT_GC_BATCH_SIZE, ObjectGcPolicy, ObjectGcPolicyError,
        RuntimeDir, TrashRetentionPolicy, TrashRetentionPolicyError,
    };

    #[test]
    fn logical_paths_preserve_portable_path_values() {
        let path = PathBuf::from("synveil").join("data");

        assert_eq!(DataDir::new(path.clone()).as_path(), path.as_path());
        assert_eq!(ConfigDir::from(path.clone()).into_path_buf(), path);
        assert_eq!(CacheDir::new("cache").to_string(), "cache");
        assert_eq!(
            RuntimeDir::new("runtime").as_ref(),
            std::path::Path::new("runtime")
        );
    }

    #[test]
    fn trash_retention_defaults_to_thirty_days_and_rejects_unsafe_durations() {
        assert_eq!(
            TrashRetentionPolicy::default().retention_duration(),
            DEFAULT_TRASH_RETENTION
        );
        assert_eq!(
            TrashRetentionPolicy::new(Duration::ZERO),
            Err(TrashRetentionPolicyError::ZeroDuration)
        );
        assert_eq!(
            TrashRetentionPolicy::new(Duration::new(i64::MAX as u64, 1)),
            Err(TrashRetentionPolicyError::DurationOutOfRange)
        );
    }

    #[test]
    fn trash_purge_eligibility_is_inclusive_and_state_scoped() {
        let policy = TrashRetentionPolicy::new(Duration::from_secs(10)).unwrap();
        let trashed_at = Timestamp::parse("2026-08-24T00:00:00Z").unwrap();
        let deadline = Timestamp::parse("2026-08-24T00:00:10Z").unwrap();
        let after_deadline = Timestamp::parse("2026-08-24T00:00:11Z").unwrap();

        assert!(!policy.is_purge_eligible(
            NodeState::Trashed,
            false,
            Some(trashed_at),
            Timestamp::parse("2026-08-24T00:00:09Z").unwrap()
        ));
        assert!(policy.is_purge_eligible(NodeState::Trashed, false, Some(trashed_at), deadline));
        assert!(policy.is_purge_eligible(
            NodeState::Trashed,
            false,
            Some(trashed_at),
            after_deadline
        ));
        assert!(!policy.is_purge_eligible(
            NodeState::Active,
            false,
            Some(trashed_at),
            after_deadline
        ));
        assert!(!policy.is_purge_eligible(
            NodeState::Purging,
            false,
            Some(trashed_at),
            after_deadline
        ));
        assert!(!policy.is_purge_eligible(
            NodeState::Trashed,
            true,
            Some(trashed_at),
            after_deadline
        ));
        assert!(!policy.is_purge_eligible(NodeState::Trashed, false, None, after_deadline));
        assert_eq!(policy.restore_deadline(trashed_at), Some(deadline));
    }

    #[test]
    fn object_gc_policy_defaults_and_rejects_unsafe_values() {
        assert_eq!(
            ObjectGcPolicy::default().grace_period(),
            DEFAULT_OBJECT_GC_GRACE_PERIOD
        );
        assert_eq!(
            ObjectGcPolicy::default().lease_duration(),
            DEFAULT_OBJECT_GC_LEASE_DURATION
        );
        assert_eq!(
            ObjectGcPolicy::default().max_batch_size(),
            DEFAULT_OBJECT_GC_MAX_BATCH_SIZE
        );
        assert_eq!(
            ObjectGcPolicy::new(
                Duration::ZERO,
                DEFAULT_OBJECT_GC_LEASE_DURATION,
                DEFAULT_OBJECT_GC_MAX_BATCH_SIZE,
            ),
            Err(ObjectGcPolicyError::ZeroGracePeriod)
        );
        assert_eq!(
            ObjectGcPolicy::new(
                DEFAULT_OBJECT_GC_GRACE_PERIOD,
                Duration::ZERO,
                DEFAULT_OBJECT_GC_MAX_BATCH_SIZE,
            ),
            Err(ObjectGcPolicyError::ZeroLeaseDuration)
        );
        assert_eq!(
            ObjectGcPolicy::new(
                DEFAULT_OBJECT_GC_GRACE_PERIOD,
                DEFAULT_OBJECT_GC_LEASE_DURATION,
                0,
            ),
            Err(ObjectGcPolicyError::BatchSizeOutOfRange)
        );
        assert_eq!(
            ObjectGcPolicy::new(
                DEFAULT_OBJECT_GC_GRACE_PERIOD,
                DEFAULT_OBJECT_GC_LEASE_DURATION,
                MAX_OBJECT_GC_BATCH_SIZE + 1,
            ),
            Err(ObjectGcPolicyError::BatchSizeOutOfRange)
        );
        assert_eq!(
            ObjectGcPolicy::new(
                Duration::from_nanos(1_001),
                DEFAULT_OBJECT_GC_LEASE_DURATION,
                DEFAULT_OBJECT_GC_MAX_BATCH_SIZE,
            ),
            Err(ObjectGcPolicyError::GracePeriodOutOfRange)
        );
    }

    #[test]
    fn object_gc_grace_boundary_is_inclusive_and_lease_math_is_checked() {
        let policy = ObjectGcPolicy::new(
            Duration::from_secs(10),
            Duration::from_secs(30),
            DEFAULT_OBJECT_GC_MAX_BATCH_SIZE,
        )
        .unwrap();
        let unreferenced_at = Timestamp::parse("2026-08-24T00:00:00Z").unwrap();
        let grace_deadline = Timestamp::parse("2026-08-24T00:00:10Z").unwrap();
        let lease_deadline = Timestamp::parse("2026-08-24T00:00:30Z").unwrap();

        assert!(!policy.is_grace_matured(
            unreferenced_at,
            Timestamp::parse("2026-08-24T00:00:09Z").unwrap()
        ));
        assert!(policy.is_grace_matured(unreferenced_at, grace_deadline));
        assert_eq!(policy.grace_deadline(unreferenced_at), Some(grace_deadline));
        assert_eq!(policy.lease_expiry(unreferenced_at), Some(lease_deadline));
    }

    #[test]
    fn gc_worker_defaults_are_conservative_and_disabled() {
        let config = GcWorkerConfig::default();

        assert!(!config.enabled());
        assert_eq!(config.cycle_interval(), DEFAULT_GC_WORKER_CYCLE_INTERVAL);
        assert_eq!(config.max_candidate_claims_per_cycle(), 8);
        assert_eq!(config.max_active_operations(), 2);
        assert_eq!(config.max_replica_actions_per_cycle(), 4);
        assert_eq!(
            config.max_concurrent_replica_deletes(),
            DEFAULT_GC_WORKER_MAX_CONCURRENT_REPLICA_DELETES
        );
        assert_eq!(
            config.retry_policy().base_delay(),
            DEFAULT_GC_WORKER_RETRY_BASE
        );
        assert_eq!(
            config.retry_policy().max_delay(),
            DEFAULT_GC_WORKER_RETRY_MAX
        );
    }

    #[test]
    fn gc_worker_rejects_zero_intervals_and_invalid_concurrency() {
        let retry = GcWorkerRetryPolicy::default();
        assert_eq!(
            GcWorkerConfig::new(
                true,
                Duration::ZERO,
                1,
                1,
                1,
                1,
                1,
                retry,
                Duration::from_secs(1)
            ),
            Err(GcWorkerConfigError::CycleIntervalOutOfRange)
        );
        assert_eq!(
            GcWorkerConfig::new(
                true,
                Duration::from_secs(1),
                1,
                1,
                1,
                2,
                1,
                retry,
                Duration::from_secs(1),
            ),
            Err(GcWorkerConfigError::ConcurrentExecutionsExceedActiveOperations)
        );
        assert_eq!(
            GcWorkerConfig::new(
                true,
                Duration::from_secs(1),
                1,
                1,
                1,
                1,
                2,
                retry,
                Duration::from_secs(1),
            ),
            Err(GcWorkerConfigError::ConcurrentReplicaDeletesExceedExecutions)
        );
    }

    #[test]
    fn gc_worker_retry_backoff_is_bounded_and_attempt_limited() {
        let retry =
            GcWorkerRetryPolicy::new(Duration::from_secs(2), Duration::from_secs(10), 3).unwrap();
        let first = retry.delay_for_attempt(1, 0xdead_beef);
        let second = retry.delay_for_attempt(2, 0xdead_beef);
        let capped = retry.delay_for_attempt(99, 0xdead_beef);

        assert!(first >= Duration::from_secs(2));
        assert!(first <= Duration::from_millis(2_200));
        assert!(second >= Duration::from_secs(4));
        assert!(second <= Duration::from_millis(4_400));
        assert!(capped <= Duration::from_secs(10));
        assert_eq!(retry.max_attempts(), 3);
        assert_eq!(
            GcWorkerRetryPolicy::new(Duration::ZERO, Duration::from_secs(1), 1),
            Err(GcWorkerRetryPolicyError::ZeroBaseDelay)
        );
        assert_eq!(
            GcWorkerRetryPolicy::new(Duration::from_secs(2), Duration::from_secs(1), 1),
            Err(GcWorkerRetryPolicyError::MaxDelayBeforeBaseDelay)
        );
    }
}
