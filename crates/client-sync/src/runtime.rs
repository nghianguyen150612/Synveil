//! Long-running synchronization runtime and lifecycle supervision.
//!
//! This module is intentionally a scheduler boundary. It owns no
//! synchronization correctness state and never calls an inbound or outbound
//! engine directly. Every cycle is delegated to the Prompt 91
//! [`BidirectionalSyncCycleRunner`], which remains the only synchronization
//! execution primitive.

use std::{
    collections::BTreeMap,
    fmt,
    panic::AssertUnwindSafe,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use futures_util::FutureExt;
use synveil_core::{LibraryId, Timestamp};
use tokio::{
    sync::{Notify, broadcast, mpsc},
    task::JoinHandle,
    time::{self, Instant},
};

use crate::{
    BidirectionalSyncCycleRunner, ClientSyncError, InboundCycleOutcome, OutboundCycleOutcome,
    OutboundSkipReason, OutboundSubmissionOutcome, RebaselineConvergenceOutcome,
    RebaselineRecoveryBlockedReason, RemoteErrorKind, ReplicaScope, SyncCycleResult, SyncOutcome,
};

/// Conservative default safety-poll interval for libraries that are idle.
pub const DEFAULT_SYNC_RUNTIME_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Initial retry delay for transport/server failures.
pub const DEFAULT_SYNC_RUNTIME_TRANSIENT_BACKOFF_INITIAL: Duration = Duration::from_secs(1);

/// Maximum retry delay for transport/server failures.
pub const DEFAULT_SYNC_RUNTIME_TRANSIENT_BACKOFF_MAX: Duration = Duration::from_secs(60);

/// Fallback delay when Prompt 91 has no server-provided retry-after duration.
pub const DEFAULT_SYNC_RUNTIME_RATE_LIMIT_FALLBACK: Duration = Duration::from_secs(30);

/// Conservative default number of libraries that may execute concurrently.
pub const DEFAULT_SYNC_RUNTIME_MAX_CONCURRENT_LIBRARIES: usize = 4;

/// Minimum supported safety-poll interval.
pub const MIN_SYNC_RUNTIME_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Maximum supported safety-poll interval and runtime delay setting.
pub const MAX_SYNC_RUNTIME_DELAY: Duration = Duration::from_secs(60 * 60);

/// Maximum number of concurrently runnable libraries accepted by configuration.
pub const MAX_SYNC_RUNTIME_CONCURRENT_LIBRARIES: usize = 1_024;

/// Maximum number of registered libraries held by one runtime instance.
pub const MAX_SYNC_RUNTIME_LIBRARIES: usize = 4_096;

/// Event capacity. Event delivery is observability only and never gates work.
pub const SYNC_RUNTIME_EVENT_CAPACITY: usize = 256;

static NEXT_SYNC_RUNTIME_IDENTITY: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

// A non-zero cooperative delay prevents a permanently productive or broken
// result from becoming a same-tick busy loop. Tokio's paused clock makes this
// deterministic in tests.
const FAIR_FOLLOW_UP_DELAY: Duration = Duration::from_millis(1);
const NO_DEADLINE_WAIT: Duration = Duration::from_secs(24 * 60 * 60);

/// Validation failures for [`SyncRuntimeConfig`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncRuntimeConfigError {
    PollIntervalTooShort,
    PollIntervalTooLong,
    TransientBackoffInitialZero,
    TransientBackoffInitialTooLong,
    TransientBackoffMaximumZero,
    TransientBackoffMaximumTooLong,
    TransientBackoffOrder,
    RateLimitFallbackZero,
    RateLimitFallbackTooLong,
    ConcurrencyZero,
    ConcurrencyTooHigh,
}

impl fmt::Display for SyncRuntimeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::PollIntervalTooShort => "sync runtime poll interval is below the minimum",
            Self::PollIntervalTooLong => "sync runtime poll interval exceeds the maximum",
            Self::TransientBackoffInitialZero => {
                "sync runtime transient initial backoff must be non-zero"
            }
            Self::TransientBackoffInitialTooLong => {
                "sync runtime transient initial backoff exceeds the maximum"
            }
            Self::TransientBackoffMaximumZero => {
                "sync runtime transient maximum backoff must be non-zero"
            }
            Self::TransientBackoffMaximumTooLong => {
                "sync runtime transient maximum backoff exceeds the maximum"
            }
            Self::TransientBackoffOrder => {
                "sync runtime transient initial backoff exceeds its maximum"
            }
            Self::RateLimitFallbackZero => "sync runtime rate-limit fallback must be non-zero",
            Self::RateLimitFallbackTooLong => {
                "sync runtime rate-limit fallback exceeds the maximum"
            }
            Self::ConcurrencyZero => "sync runtime concurrency must be non-zero",
            Self::ConcurrencyTooHigh => "sync runtime concurrency exceeds the maximum",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SyncRuntimeConfigError {}

/// Validated process-local scheduling policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncRuntimeConfig {
    poll_interval: Duration,
    transient_backoff_initial: Duration,
    transient_backoff_max: Duration,
    rate_limit_fallback: Duration,
    max_concurrent_libraries: usize,
}

impl SyncRuntimeConfig {
    /// Build a validated runtime policy.
    pub fn new(
        poll_interval: Duration,
        transient_backoff_initial: Duration,
        transient_backoff_max: Duration,
        rate_limit_fallback: Duration,
        max_concurrent_libraries: usize,
    ) -> Result<Self, SyncRuntimeConfigError> {
        if poll_interval < MIN_SYNC_RUNTIME_POLL_INTERVAL {
            return Err(SyncRuntimeConfigError::PollIntervalTooShort);
        }
        if poll_interval > MAX_SYNC_RUNTIME_DELAY {
            return Err(SyncRuntimeConfigError::PollIntervalTooLong);
        }
        if transient_backoff_initial.is_zero() {
            return Err(SyncRuntimeConfigError::TransientBackoffInitialZero);
        }
        if transient_backoff_initial > MAX_SYNC_RUNTIME_DELAY {
            return Err(SyncRuntimeConfigError::TransientBackoffInitialTooLong);
        }
        if transient_backoff_max.is_zero() {
            return Err(SyncRuntimeConfigError::TransientBackoffMaximumZero);
        }
        if transient_backoff_max > MAX_SYNC_RUNTIME_DELAY {
            return Err(SyncRuntimeConfigError::TransientBackoffMaximumTooLong);
        }
        if transient_backoff_initial > transient_backoff_max {
            return Err(SyncRuntimeConfigError::TransientBackoffOrder);
        }
        if rate_limit_fallback.is_zero() {
            return Err(SyncRuntimeConfigError::RateLimitFallbackZero);
        }
        if rate_limit_fallback > MAX_SYNC_RUNTIME_DELAY {
            return Err(SyncRuntimeConfigError::RateLimitFallbackTooLong);
        }
        if max_concurrent_libraries == 0 {
            return Err(SyncRuntimeConfigError::ConcurrencyZero);
        }
        if max_concurrent_libraries > MAX_SYNC_RUNTIME_CONCURRENT_LIBRARIES {
            return Err(SyncRuntimeConfigError::ConcurrencyTooHigh);
        }
        Ok(Self {
            poll_interval,
            transient_backoff_initial,
            transient_backoff_max,
            rate_limit_fallback,
            max_concurrent_libraries,
        })
    }

    #[must_use]
    pub const fn poll_interval(self) -> Duration {
        self.poll_interval
    }

    #[must_use]
    pub const fn transient_backoff_initial(self) -> Duration {
        self.transient_backoff_initial
    }

    #[must_use]
    pub const fn transient_backoff_max(self) -> Duration {
        self.transient_backoff_max
    }

    #[must_use]
    pub const fn rate_limit_fallback(self) -> Duration {
        self.rate_limit_fallback
    }

    #[must_use]
    pub const fn max_concurrent_libraries(self) -> usize {
        self.max_concurrent_libraries
    }
}

impl Default for SyncRuntimeConfig {
    fn default() -> Self {
        Self {
            poll_interval: DEFAULT_SYNC_RUNTIME_POLL_INTERVAL,
            transient_backoff_initial: DEFAULT_SYNC_RUNTIME_TRANSIENT_BACKOFF_INITIAL,
            transient_backoff_max: DEFAULT_SYNC_RUNTIME_TRANSIENT_BACKOFF_MAX,
            rate_limit_fallback: DEFAULT_SYNC_RUNTIME_RATE_LIMIT_FALLBACK,
            max_concurrent_libraries: DEFAULT_SYNC_RUNTIME_MAX_CONCURRENT_LIBRARIES,
        }
    }
}

/// Typed reason for an explicit or scheduler-originated wake.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SyncRuntimeWakeReason {
    Startup,
    LocalChange,
    Manual,
    Periodic,
    NetworkAvailable,
    RootAvailable,
    CredentialChanged,
    PreviousProgress,
}

impl SyncRuntimeWakeReason {
    const fn priority(self) -> u8 {
        match self {
            Self::Startup => 1,
            Self::PreviousProgress => 2,
            Self::Periodic => 3,
            Self::LocalChange => 4,
            Self::NetworkAvailable => 5,
            Self::RootAvailable => 6,
            Self::Manual => 7,
            Self::CredentialChanged => 8,
        }
    }
}

/// Result of handing a scheduling hint to the runtime.
///
/// The first three variants describe accepted, bounded in-memory scheduling
/// work. `RuntimeStopped` and `UnknownLibrary` are deliberately typed rather
/// than being panics or implicit registration: durable producers must retain
/// their committed work and rely on a later startup poll when no supervisor is
/// available.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SyncRuntimeWakeResult {
    Queued,
    Coalesced,
    AlreadyRunningFollowupRecorded,
    RuntimeStopped,
    UnknownLibrary,
}

impl SyncRuntimeWakeResult {
    #[must_use]
    pub const fn was_accepted(self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Coalesced | Self::AlreadyRunningFollowupRecorded
        )
    }

    #[must_use]
    pub const fn is_runtime_unavailable(self) -> bool {
        matches!(self, Self::RuntimeStopped)
    }

    #[must_use]
    pub const fn is_unknown_library(self) -> bool {
        matches!(self, Self::UnknownLibrary)
    }

    fn into_control_result(self) -> Result<Self, SyncRuntimeError> {
        match self {
            Self::RuntimeStopped => Err(SyncRuntimeError::NotRunning),
            Self::UnknownLibrary => Err(SyncRuntimeError::NotRegistered),
            accepted => Ok(accepted),
        }
    }
}

/// Minimal dependency-inversion seam for local event producers.
///
/// Implementations only receive a library identity and a closed scheduling
/// reason. No credential, token, path, content, checkpoint, or synchronization
/// state crosses this boundary.
pub trait SyncWakeNotifier: Send + Sync {
    fn wake_library(
        &self,
        library_id: LibraryId,
        reason: SyncRuntimeWakeReason,
    ) -> SyncRuntimeWakeResult;

    /// Return the process-local runtime identity when this notifier is backed
    /// by [`SyncRuntime`] or [`SyncRuntimeHandle`]. Third-party notifiers may
    /// leave this as `None`; the identity is an observability/testing aid and
    /// is not part of scheduling correctness.
    fn runtime_identity(&self) -> Option<SyncRuntimeIdentity> {
        None
    }
}

/// Opaque process-local identity for one [`SyncRuntime`] supervisor.
///
/// This value is intentionally not a pointer or a durable identifier. It is
/// useful only for proving that application-owned producers were wired to one
/// shared runtime during the lifetime of a host.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SyncRuntimeIdentity(u64);

impl SyncRuntimeIdentity {
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

/// Safe result category used by scheduling and status reporting.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SyncRuntimeOutcome {
    Idle,
    Progress,
    ConflictBlocked,
    Offline,
    ServerTransient,
    RateLimited,
    AuthBlocked,
    RootUnavailable,
    RecoveryBlocked,
    FatalLocal,
    Panicked,
}

/// In-memory lifecycle state for one registered library.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SyncRuntimeLibraryPhase {
    Idle,
    Scheduled,
    Running,
    BackingOff,
    AuthBlocked,
    RootBlocked,
    Faulted,
    Stopped,
}

/// Bounded status snapshot safe for a future desktop/controller consumer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncRuntimeLibraryStatus {
    library_id: LibraryId,
    phase: SyncRuntimeLibraryPhase,
    next_due_in: Option<Duration>,
    last_outcome: Option<SyncRuntimeOutcome>,
    wake_pending: bool,
    transient_failures: u32,
}

impl SyncRuntimeLibraryStatus {
    #[must_use]
    pub const fn library_id(self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn phase(self) -> SyncRuntimeLibraryPhase {
        self.phase
    }

    /// Remaining delay relative to the instant at which this snapshot was
    /// read. `Some(Duration::ZERO)` means runnable now.
    #[must_use]
    pub const fn next_due_in(self) -> Option<Duration> {
        self.next_due_in
    }

    #[must_use]
    pub const fn last_outcome(self) -> Option<SyncRuntimeOutcome> {
        self.last_outcome
    }

    #[must_use]
    pub const fn wake_pending(self) -> bool {
        self.wake_pending
    }

    #[must_use]
    pub const fn transient_failures(self) -> u32 {
        self.transient_failures
    }
}

/// Non-blocking observability events emitted by the runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncRuntimeEvent {
    RuntimeStarted,
    RuntimeStopping,
    RuntimeStopped,
    RuntimeTaskFaulted,
    CycleStarted {
        library_id: LibraryId,
    },
    CycleFinished {
        library_id: LibraryId,
        outcome: SyncRuntimeOutcome,
    },
    BackoffScheduled {
        library_id: LibraryId,
        outcome: SyncRuntimeOutcome,
        delay: Duration,
    },
    AuthBlocked {
        library_id: LibraryId,
    },
    LibraryFaulted {
        library_id: LibraryId,
    },
}

/// Idempotent registration result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncRuntimeRegistration {
    Registered,
    AlreadyRegistered,
}

/// Idempotent unregistration result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncRuntimeUnregistration {
    Unregistered,
    AlreadyUnregistered,
}

/// Errors from the runtime control plane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncRuntimeError {
    AlreadyRunning,
    Stopping,
    NotRunning,
    NotRegistered,
    TooManyLibraries,
    TokioRuntimeUnavailable,
    TaskPanicked,
}

impl fmt::Display for SyncRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::AlreadyRunning => "sync runtime is already running",
            Self::Stopping => "sync runtime is stopping",
            Self::NotRunning => "sync runtime is not running",
            Self::NotRegistered => "sync library is not registered",
            Self::TooManyLibraries => "sync runtime library limit reached",
            Self::TokioRuntimeUnavailable => "a Tokio runtime is required to start sync runtime",
            Self::TaskPanicked => "sync runtime task panicked",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SyncRuntimeError {}

/// The narrow execution port used by the scheduler.
///
/// The production implementation is [`BidirectionalSyncCycleRunner`]. A
/// controlled implementation is also useful for deterministic scheduler and
/// fairness tests; it must retain the same finite-call contract.
#[async_trait]
pub trait SyncCycleExecutor: Send + Sync {
    fn scope(&self) -> ReplicaScope;

    async fn run_once(&self, observed_at: Timestamp) -> Result<SyncCycleResult, ClientSyncError>;
}

#[async_trait]
impl SyncCycleExecutor for BidirectionalSyncCycleRunner {
    fn scope(&self) -> ReplicaScope {
        BidirectionalSyncCycleRunner::scope(self)
    }

    async fn run_once(&self, observed_at: Timestamp) -> Result<SyncCycleResult, ClientSyncError> {
        BidirectionalSyncCycleRunner::run_once(self, observed_at).await
    }
}

/// One long-running, restart-stateless synchronization supervisor.
#[derive(Clone)]
pub struct SyncRuntime {
    shared: Arc<RuntimeShared>,
}

/// Control and join handle returned by [`SyncRuntime::start`].
#[derive(Clone)]
pub struct SyncRuntimeHandle {
    shared: Arc<RuntimeShared>,
}

impl fmt::Debug for SyncRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyncRuntime")
            .field("config", &self.shared.config)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for SyncRuntimeHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyncRuntimeHandle")
            .finish_non_exhaustive()
    }
}

impl SyncRuntime {
    #[must_use]
    pub fn new(config: SyncRuntimeConfig) -> Self {
        Self {
            shared: Arc::new(RuntimeShared::new(config)),
        }
    }

    #[must_use]
    pub fn config(&self) -> SyncRuntimeConfig {
        self.shared.config
    }

    #[must_use]
    pub fn identity(&self) -> SyncRuntimeIdentity {
        self.shared.identity
    }

    /// Register a Prompt 91 cycle executor. Registration is idempotent by
    /// library ID; a duplicate call leaves the existing executor untouched.
    pub fn register_library<E>(
        &self,
        executor: Arc<E>,
    ) -> Result<SyncRuntimeRegistration, SyncRuntimeError>
    where
        E: SyncCycleExecutor + 'static,
    {
        let executor: Arc<dyn SyncCycleExecutor> = executor;
        self.register_executor(executor)
    }

    /// Register an already type-erased Prompt 91 cycle executor.
    ///
    /// This companion to [`Self::register_library`] lets an embedding
    /// controller keep a heterogeneous collection of authenticated runners
    /// without weakening the runtime's one-executor-per-library invariant.
    pub fn register_executor(
        &self,
        executor: Arc<dyn SyncCycleExecutor>,
    ) -> Result<SyncRuntimeRegistration, SyncRuntimeError> {
        let lifecycle = lock_unpoisoned(&self.shared.lifecycle);
        if lifecycle.phase == LifecyclePhase::Stopping {
            return Err(SyncRuntimeError::Stopping);
        }
        let started = lifecycle.phase == LifecyclePhase::Running;

        let mut libraries = lock_unpoisoned(&self.shared.libraries);
        if libraries.contains_key(&executor.scope().library_id()) {
            return Ok(SyncRuntimeRegistration::AlreadyRegistered);
        }
        if libraries.len() >= MAX_SYNC_RUNTIME_LIBRARIES {
            return Err(SyncRuntimeError::TooManyLibraries);
        }
        libraries.insert(
            executor.scope().library_id(),
            RuntimeLibraryEntry::new(executor, started.then(Instant::now)),
        );
        drop(libraries);
        drop(lifecycle);
        if started {
            self.shared.wake_notify.notify_one();
        }
        Ok(SyncRuntimeRegistration::Registered)
    }

    /// Mark a library inactive without deleting any local synchronization
    /// data. An active bounded cycle is allowed to finish before its ephemeral
    /// scheduler entry is removed.
    pub fn unregister_library(
        &self,
        library_id: LibraryId,
    ) -> Result<SyncRuntimeUnregistration, SyncRuntimeError> {
        let lifecycle = lock_unpoisoned(&self.shared.lifecycle);
        if lifecycle.phase == LifecyclePhase::Stopping {
            return Err(SyncRuntimeError::Stopping);
        }
        let mut libraries = lock_unpoisoned(&self.shared.libraries);
        let Some(entry) = libraries.get_mut(&library_id) else {
            return Ok(SyncRuntimeUnregistration::AlreadyUnregistered);
        };
        if !entry.registered {
            return Ok(SyncRuntimeUnregistration::AlreadyUnregistered);
        }
        entry.registered = false;
        if !entry.state.running {
            libraries.remove(&library_id);
        }
        drop(libraries);
        drop(lifecycle);
        self.shared.wake_notify.notify_one();
        Ok(SyncRuntimeUnregistration::Unregistered)
    }

    #[must_use]
    pub fn registered_libraries(&self) -> Vec<LibraryId> {
        let libraries = lock_unpoisoned(&self.shared.libraries);
        libraries
            .iter()
            .filter_map(|(library_id, entry)| entry.registered.then_some(*library_id))
            .collect()
    }

    /// Start one supervisor. A second start on the same instance is rejected
    /// and cannot create duplicate workers.
    pub fn start(&self) -> Result<SyncRuntimeHandle, SyncRuntimeError> {
        let mut lifecycle = lock_unpoisoned(&self.shared.lifecycle);
        match lifecycle.phase {
            LifecyclePhase::Running => return Err(SyncRuntimeError::AlreadyRunning),
            LifecyclePhase::Stopping => return Err(SyncRuntimeError::Stopping),
            LifecyclePhase::Stopped => {}
        }
        let tokio_handle = tokio::runtime::Handle::try_current()
            .map_err(|_| SyncRuntimeError::TokioRuntimeUnavailable)?;

        self.shared
            .shutdown_requested
            .store(false, Ordering::Release);
        let now = Instant::now();
        let mut libraries = lock_unpoisoned(&self.shared.libraries);
        libraries.retain(|_, entry| entry.registered);
        for entry in libraries.values_mut() {
            entry.state.reset_for_start(now);
        }
        drop(libraries);

        lifecycle.phase = LifecyclePhase::Running;
        lifecycle.terminal_failure = None;
        let shared = Arc::clone(&self.shared);
        lifecycle.join = Some(tokio_handle.spawn(async move {
            let supervisor = AssertUnwindSafe(supervisor_loop(Arc::clone(&shared)))
                .catch_unwind()
                .await;
            if supervisor.is_err() {
                shared.emit(SyncRuntimeEvent::RuntimeTaskFaulted);
                let mut lifecycle = lock_unpoisoned(&shared.lifecycle);
                lifecycle.phase = LifecyclePhase::Stopped;
                lifecycle.terminal_failure = Some(SyncRuntimeError::TaskPanicked);
                drop(lifecycle);
                shared.completion_notify.notify_waiters();
            }
        }));
        drop(lifecycle);

        self.shared.emit(SyncRuntimeEvent::RuntimeStarted);
        self.shared.wake_notify.notify_one();
        Ok(SyncRuntimeHandle {
            shared: Arc::clone(&self.shared),
        })
    }

    pub fn wake_library(
        &self,
        library_id: LibraryId,
        reason: SyncRuntimeWakeReason,
    ) -> Result<SyncRuntimeWakeResult, SyncRuntimeError> {
        self.shared
            .wake_library(library_id, reason)
            .into_control_result()
    }

    /// Return the full typed wake status, including a stopped runtime or an
    /// unregistered library. This is the non-throwing form used by durable
    /// producers because a wake failure must never undo a committed change.
    #[must_use]
    pub fn wake_library_status(
        &self,
        library_id: LibraryId,
        reason: SyncRuntimeWakeReason,
    ) -> SyncRuntimeWakeResult {
        self.shared.wake_library(library_id, reason)
    }

    /// Request one promptly scheduled bounded cycle through Prompt 92.
    ///
    /// This reports scheduling only; it does not wait for synchronization to
    /// complete and does not invoke Prompt 91 directly.
    pub fn sync_now(&self, library_id: LibraryId) -> SyncRuntimeWakeResult {
        self.sync_now_status(library_id)
    }

    #[must_use]
    pub fn sync_now_status(&self, library_id: LibraryId) -> SyncRuntimeWakeResult {
        self.wake_library_status(library_id, SyncRuntimeWakeReason::Manual)
    }

    /// Notify every currently registered library that connectivity may have
    /// returned. This only changes scheduling; the bounded active set still
    /// enforces the global concurrency limit.
    #[must_use]
    pub fn network_available(&self) -> Vec<(LibraryId, SyncRuntimeWakeResult)> {
        self.wake_all(SyncRuntimeWakeReason::NetworkAvailable)
    }

    #[must_use]
    pub fn credential_changed(&self, library_id: LibraryId) -> SyncRuntimeWakeResult {
        self.wake_library_status(library_id, SyncRuntimeWakeReason::CredentialChanged)
    }

    #[must_use]
    pub fn credentials_changed(&self) -> Vec<(LibraryId, SyncRuntimeWakeResult)> {
        self.wake_all(SyncRuntimeWakeReason::CredentialChanged)
    }

    #[must_use]
    fn wake_all(&self, reason: SyncRuntimeWakeReason) -> Vec<(LibraryId, SyncRuntimeWakeResult)> {
        self.registered_libraries()
            .into_iter()
            .map(|library_id| (library_id, self.wake_library_status(library_id, reason)))
            .collect()
    }

    #[must_use]
    pub fn status(&self, library_id: LibraryId) -> Option<SyncRuntimeLibraryStatus> {
        self.shared.status(library_id)
    }

    #[must_use]
    pub fn statuses(&self) -> Vec<SyncRuntimeLibraryStatus> {
        self.shared.statuses()
    }

    #[must_use]
    pub fn events(&self) -> broadcast::Receiver<SyncRuntimeEvent> {
        self.shared.events.subscribe()
    }

    pub fn request_shutdown(&self) -> Result<(), SyncRuntimeError> {
        self.shared.request_shutdown()
    }

    pub async fn shutdown(&self) -> Result<(), SyncRuntimeError> {
        self.request_shutdown()?;
        self.join().await
    }

    pub async fn stop(&self) -> Result<(), SyncRuntimeError> {
        self.shutdown().await
    }

    pub async fn join(&self) -> Result<(), SyncRuntimeError> {
        join_shared(Arc::clone(&self.shared)).await
    }
}

impl SyncRuntimeHandle {
    #[must_use]
    pub fn identity(&self) -> SyncRuntimeIdentity {
        self.shared.identity
    }

    #[must_use]
    pub fn registered_libraries(&self) -> Vec<LibraryId> {
        let libraries = lock_unpoisoned(&self.shared.libraries);
        libraries
            .iter()
            .filter_map(|(library_id, entry)| entry.registered.then_some(*library_id))
            .collect()
    }

    pub fn wake_library(
        &self,
        library_id: LibraryId,
        reason: SyncRuntimeWakeReason,
    ) -> Result<SyncRuntimeWakeResult, SyncRuntimeError> {
        self.shared
            .wake_library(library_id, reason)
            .into_control_result()
    }

    #[must_use]
    pub fn wake_library_status(
        &self,
        library_id: LibraryId,
        reason: SyncRuntimeWakeReason,
    ) -> SyncRuntimeWakeResult {
        self.shared.wake_library(library_id, reason)
    }

    /// Schedule one bounded Prompt 91 cycle through the running Prompt 92
    /// supervisor. The result is a scheduling status, not a completion result.
    pub fn sync_now(&self, library_id: LibraryId) -> SyncRuntimeWakeResult {
        self.sync_now_status(library_id)
    }

    #[must_use]
    pub fn sync_now_status(&self, library_id: LibraryId) -> SyncRuntimeWakeResult {
        self.wake_library_status(library_id, SyncRuntimeWakeReason::Manual)
    }

    #[must_use]
    pub fn network_available(&self) -> Vec<(LibraryId, SyncRuntimeWakeResult)> {
        self.registered_libraries()
            .into_iter()
            .map(|library_id| {
                (
                    library_id,
                    self.wake_library_status(library_id, SyncRuntimeWakeReason::NetworkAvailable),
                )
            })
            .collect()
    }

    #[must_use]
    pub fn credential_changed(&self, library_id: LibraryId) -> SyncRuntimeWakeResult {
        self.wake_library_status(library_id, SyncRuntimeWakeReason::CredentialChanged)
    }

    #[must_use]
    pub fn credentials_changed(&self) -> Vec<(LibraryId, SyncRuntimeWakeResult)> {
        self.registered_libraries()
            .into_iter()
            .map(|library_id| {
                (
                    library_id,
                    self.wake_library_status(library_id, SyncRuntimeWakeReason::CredentialChanged),
                )
            })
            .collect()
    }

    #[must_use]
    pub fn status(&self, library_id: LibraryId) -> Option<SyncRuntimeLibraryStatus> {
        self.shared.status(library_id)
    }

    #[must_use]
    pub fn statuses(&self) -> Vec<SyncRuntimeLibraryStatus> {
        self.shared.statuses()
    }

    #[must_use]
    pub fn events(&self) -> broadcast::Receiver<SyncRuntimeEvent> {
        self.shared.events.subscribe()
    }

    pub fn request_shutdown(&self) -> Result<(), SyncRuntimeError> {
        self.shared.request_shutdown()
    }

    pub async fn shutdown(&self) -> Result<(), SyncRuntimeError> {
        self.request_shutdown()?;
        self.join().await
    }

    pub async fn stop(&self) -> Result<(), SyncRuntimeError> {
        self.shutdown().await
    }

    pub async fn join(&self) -> Result<(), SyncRuntimeError> {
        join_shared(Arc::clone(&self.shared)).await
    }
}

impl SyncWakeNotifier for SyncRuntime {
    fn wake_library(
        &self,
        library_id: LibraryId,
        reason: SyncRuntimeWakeReason,
    ) -> SyncRuntimeWakeResult {
        self.wake_library_status(library_id, reason)
    }

    fn runtime_identity(&self) -> Option<SyncRuntimeIdentity> {
        Some(self.identity())
    }
}

impl SyncWakeNotifier for SyncRuntimeHandle {
    fn wake_library(
        &self,
        library_id: LibraryId,
        reason: SyncRuntimeWakeReason,
    ) -> SyncRuntimeWakeResult {
        self.wake_library_status(library_id, reason)
    }

    fn runtime_identity(&self) -> Option<SyncRuntimeIdentity> {
        Some(self.identity())
    }
}

impl Drop for SyncRuntime {
    fn drop(&mut self) {
        request_shutdown_on_last_controller(&self.shared);
    }
}

impl Drop for SyncRuntimeHandle {
    fn drop(&mut self) {
        request_shutdown_on_last_controller(&self.shared);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LifecyclePhase {
    Stopped,
    Running,
    Stopping,
}

struct RuntimeLifecycle {
    phase: LifecyclePhase,
    join: Option<JoinHandle<()>>,
    terminal_failure: Option<SyncRuntimeError>,
}

struct RuntimeShared {
    config: SyncRuntimeConfig,
    identity: SyncRuntimeIdentity,
    libraries: StdMutex<BTreeMap<LibraryId, RuntimeLibraryEntry>>,
    lifecycle: StdMutex<RuntimeLifecycle>,
    scheduling_gate: StdMutex<()>,
    shutdown_requested: AtomicBool,
    wake_notify: Notify,
    shutdown_notify: Notify,
    completion_notify: Notify,
    events: broadcast::Sender<SyncRuntimeEvent>,
}

impl RuntimeShared {
    fn new(config: SyncRuntimeConfig) -> Self {
        let (events, _) = broadcast::channel(SYNC_RUNTIME_EVENT_CAPACITY);
        Self {
            config,
            identity: SyncRuntimeIdentity(
                NEXT_SYNC_RUNTIME_IDENTITY.fetch_add(1, Ordering::Relaxed),
            ),
            libraries: StdMutex::new(BTreeMap::new()),
            lifecycle: StdMutex::new(RuntimeLifecycle {
                phase: LifecyclePhase::Stopped,
                join: None,
                terminal_failure: None,
            }),
            scheduling_gate: StdMutex::new(()),
            shutdown_requested: AtomicBool::new(false),
            wake_notify: Notify::new(),
            shutdown_notify: Notify::new(),
            completion_notify: Notify::new(),
            events,
        }
    }

    fn emit(&self, event: SyncRuntimeEvent) {
        let _ = self.events.send(event);
    }

    fn request_shutdown(&self) -> Result<(), SyncRuntimeError> {
        let mut lifecycle = lock_unpoisoned(&self.lifecycle);
        match lifecycle.phase {
            LifecyclePhase::Stopped => return Ok(()),
            LifecyclePhase::Running => lifecycle.phase = LifecyclePhase::Stopping,
            LifecyclePhase::Stopping => {}
        }
        let _scheduling_gate = lock_unpoisoned(&self.scheduling_gate);
        self.shutdown_requested.store(true, Ordering::Release);
        drop(_scheduling_gate);
        drop(lifecycle);
        self.shutdown_notify.notify_one();
        self.wake_notify.notify_one();
        Ok(())
    }

    fn wake_library(
        &self,
        library_id: LibraryId,
        reason: SyncRuntimeWakeReason,
    ) -> SyncRuntimeWakeResult {
        let lifecycle = lock_unpoisoned(&self.lifecycle);
        match lifecycle.phase {
            LifecyclePhase::Stopped | LifecyclePhase::Stopping => {
                return SyncRuntimeWakeResult::RuntimeStopped;
            }
            LifecyclePhase::Running => {}
        }
        let mut libraries = lock_unpoisoned(&self.libraries);
        let Some(entry) = libraries.get_mut(&library_id) else {
            return SyncRuntimeWakeResult::UnknownLibrary;
        };
        if !entry.registered {
            return SyncRuntimeWakeResult::UnknownLibrary;
        }
        let result = apply_wake(&mut entry.state, reason, Instant::now());
        drop(libraries);
        drop(lifecycle);
        self.wake_notify.notify_one();
        result
    }

    fn status(&self, library_id: LibraryId) -> Option<SyncRuntimeLibraryStatus> {
        let libraries = lock_unpoisoned(&self.libraries);
        libraries
            .get(&library_id)
            .map(|entry| entry.state.status(library_id, Instant::now()))
    }

    fn statuses(&self) -> Vec<SyncRuntimeLibraryStatus> {
        let libraries = lock_unpoisoned(&self.libraries);
        let now = Instant::now();
        libraries
            .iter()
            .filter(|(_, entry)| entry.registered)
            .map(|(library_id, entry)| entry.state.status(*library_id, now))
            .collect()
    }
}

struct RuntimeLibraryEntry {
    runner: Arc<dyn SyncCycleExecutor>,
    registered: bool,
    state: RuntimeLibraryState,
}

impl RuntimeLibraryEntry {
    fn new(runner: Arc<dyn SyncCycleExecutor>, start_at: Option<Instant>) -> Self {
        let mut state = RuntimeLibraryState::new();
        if let Some(start_at) = start_at {
            state.reset_for_start(start_at);
        }
        Self {
            runner,
            registered: true,
            state,
        }
    }
}

struct RuntimeLibraryState {
    phase: SyncRuntimeLibraryPhase,
    running: bool,
    next_due: Option<Instant>,
    pending_wake: Option<SyncRuntimeWakeReason>,
    transient_failures: u32,
    last_outcome: Option<SyncRuntimeOutcome>,
}

impl RuntimeLibraryState {
    fn new() -> Self {
        Self {
            phase: SyncRuntimeLibraryPhase::Stopped,
            running: false,
            next_due: None,
            pending_wake: None,
            transient_failures: 0,
            last_outcome: None,
        }
    }

    fn reset_for_start(&mut self, now: Instant) {
        self.phase = SyncRuntimeLibraryPhase::Scheduled;
        self.running = false;
        self.next_due = Some(now);
        self.pending_wake = None;
        self.transient_failures = 0;
        self.last_outcome = None;
    }

    fn status(&self, library_id: LibraryId, now: Instant) -> SyncRuntimeLibraryStatus {
        let next_due_in = self
            .next_due
            .map(|due| if due > now { due - now } else { Duration::ZERO });
        SyncRuntimeLibraryStatus {
            library_id,
            phase: self.phase,
            next_due_in,
            last_outcome: self.last_outcome,
            wake_pending: self.pending_wake.is_some(),
            transient_failures: self.transient_failures,
        }
    }
}

enum CycleExecution {
    Returned(Result<SyncCycleResult, ClientSyncError>),
    Panicked,
}

struct CycleCompletion {
    library_id: LibraryId,
    execution: CycleExecution,
}

async fn supervisor_loop(shared: Arc<RuntimeShared>) {
    let capacity = shared.config.max_concurrent_libraries();
    let (completion_sender, mut completion_receiver) = mpsc::channel(capacity.max(1));
    let mut active = 0_usize;
    let mut round_robin_cursor = None;

    loop {
        if shared.shutdown_requested.load(Ordering::Acquire) {
            shared.emit(SyncRuntimeEvent::RuntimeStopping);
            while active > 0 {
                let Some(completion) = completion_receiver.recv().await else {
                    break;
                };
                active -= 1;
                finish_cycle(&shared, completion, true);
            }
            break;
        }

        let started =
            spawn_due_cycles(&shared, &completion_sender, active, &mut round_robin_cursor);
        active += started;
        if started > 0 {
            continue;
        }

        let now = Instant::now();
        let deadline = (active < capacity).then(|| earliest_due(&shared));
        let timer = match deadline.flatten() {
            Some(deadline) if deadline > now => time::sleep_until(deadline),
            Some(_) => time::sleep(Duration::ZERO),
            None => time::sleep(NO_DEADLINE_WAIT),
        };
        tokio::pin!(timer);
        tokio::select! {
            biased;
            _ = shared.shutdown_notify.notified() => {}
            completion = completion_receiver.recv(), if active > 0 => {
                if let Some(completion) = completion {
                    active -= 1;
                    finish_cycle(&shared, completion, false);
                }
            }
            _ = shared.wake_notify.notified() => {}
            _ = &mut timer => {}
        }
    }

    let mut libraries = lock_unpoisoned(&shared.libraries);
    libraries.retain(|_, entry| {
        if entry.registered {
            entry.state.phase = SyncRuntimeLibraryPhase::Stopped;
            entry.state.running = false;
            entry.state.next_due = None;
            entry.state.pending_wake = None;
            true
        } else {
            false
        }
    });
    drop(libraries);
    shared.emit(SyncRuntimeEvent::RuntimeStopped);

    let mut lifecycle = lock_unpoisoned(&shared.lifecycle);
    lifecycle.phase = LifecyclePhase::Stopped;
    drop(lifecycle);
    shared.completion_notify.notify_waiters();
}

fn spawn_due_cycles(
    shared: &Arc<RuntimeShared>,
    completion_sender: &mpsc::Sender<CycleCompletion>,
    active: usize,
    round_robin_cursor: &mut Option<LibraryId>,
) -> usize {
    let _scheduling_gate = lock_unpoisoned(&shared.scheduling_gate);
    if shared.shutdown_requested.load(Ordering::Acquire) {
        return 0;
    }
    let available = shared
        .config
        .max_concurrent_libraries()
        .saturating_sub(active);
    if available == 0 {
        return 0;
    }
    let now = Instant::now();
    let mut libraries = lock_unpoisoned(&shared.libraries);
    let ordered_ids: Vec<LibraryId> = libraries
        .iter()
        .filter_map(|(library_id, entry)| entry.registered.then_some(*library_id))
        .collect();
    if ordered_ids.is_empty() {
        return 0;
    }
    let start_index = round_robin_cursor
        .and_then(|cursor| ordered_ids.iter().position(|id| *id == cursor))
        .map_or(0, |index| (index + 1) % ordered_ids.len());

    let mut started = 0;
    for offset in 0..ordered_ids.len() {
        if started == available {
            break;
        }
        let library_id = ordered_ids[(start_index + offset) % ordered_ids.len()];
        let Some(entry) = libraries.get_mut(&library_id) else {
            continue;
        };
        if !entry.registered
            || entry.state.running
            || !entry.state.next_due.is_some_and(|next_due| next_due <= now)
        {
            continue;
        }
        entry.state.running = true;
        entry.state.phase = SyncRuntimeLibraryPhase::Running;
        entry.state.next_due = None;
        entry.state.pending_wake = None;
        let runner = Arc::clone(&entry.runner);
        shared.emit(SyncRuntimeEvent::CycleStarted { library_id });
        let sender = completion_sender.clone();
        tokio::spawn(async move {
            let execution = AssertUnwindSafe(runner.run_once(Timestamp::now()))
                .catch_unwind()
                .await
                .map_or(CycleExecution::Panicked, CycleExecution::Returned);
            let _ = sender
                .send(CycleCompletion {
                    library_id,
                    execution,
                })
                .await;
        });
        started += 1;
        *round_robin_cursor = Some(library_id);
    }
    started
}

fn earliest_due(shared: &Arc<RuntimeShared>) -> Option<Instant> {
    let libraries = lock_unpoisoned(&shared.libraries);
    libraries
        .values()
        .filter(|entry| entry.registered && !entry.state.running)
        .filter_map(|entry| entry.state.next_due)
        .min()
}

fn finish_cycle(shared: &Arc<RuntimeShared>, completion: CycleCompletion, stopping: bool) {
    let outcome = classify_execution(&completion.execution);
    let now = Instant::now();
    let mut backoff = None;
    let mut auth_blocked = false;
    let mut faulted = false;
    let mut remove = false;

    {
        let mut libraries = lock_unpoisoned(&shared.libraries);
        let Some(entry) = libraries.get_mut(&completion.library_id) else {
            return;
        };
        entry.state.running = false;
        entry.state.last_outcome = Some(outcome);
        if stopping || !entry.registered {
            entry.state.phase = SyncRuntimeLibraryPhase::Stopped;
            entry.state.next_due = None;
            entry.state.pending_wake = None;
            remove = !entry.registered;
        } else {
            let pending = entry.state.pending_wake.take();
            let decision =
                schedule_after_cycle(&mut entry.state, shared.config, outcome, pending, now);
            backoff = decision.backoff;
            auth_blocked = decision.auth_blocked;
            faulted = decision.faulted;
        }
        if remove {
            libraries.remove(&completion.library_id);
        }
    }

    shared.emit(SyncRuntimeEvent::CycleFinished {
        library_id: completion.library_id,
        outcome,
    });
    if let Some(delay) = backoff {
        shared.emit(SyncRuntimeEvent::BackoffScheduled {
            library_id: completion.library_id,
            outcome,
            delay,
        });
    }
    if auth_blocked {
        shared.emit(SyncRuntimeEvent::AuthBlocked {
            library_id: completion.library_id,
        });
    }
    if faulted {
        shared.emit(SyncRuntimeEvent::LibraryFaulted {
            library_id: completion.library_id,
        });
    }
    shared.wake_notify.notify_one();
}

struct ScheduleDecision {
    backoff: Option<Duration>,
    auth_blocked: bool,
    faulted: bool,
}

fn schedule_after_cycle(
    state: &mut RuntimeLibraryState,
    config: SyncRuntimeConfig,
    outcome: SyncRuntimeOutcome,
    pending: Option<SyncRuntimeWakeReason>,
    now: Instant,
) -> ScheduleDecision {
    let mut decision = ScheduleDecision {
        backoff: None,
        auth_blocked: false,
        faulted: false,
    };
    match outcome {
        SyncRuntimeOutcome::Idle | SyncRuntimeOutcome::ConflictBlocked => {
            state.transient_failures = 0;
            state.phase = SyncRuntimeLibraryPhase::Idle;
            state.next_due = Some(now + config.poll_interval());
            if pending.is_some() {
                state.phase = SyncRuntimeLibraryPhase::Scheduled;
                state.next_due = Some(now + FAIR_FOLLOW_UP_DELAY);
            }
        }
        SyncRuntimeOutcome::Progress => {
            state.transient_failures = 0;
            state.phase = SyncRuntimeLibraryPhase::Scheduled;
            state.next_due = Some(now + FAIR_FOLLOW_UP_DELAY);
        }
        SyncRuntimeOutcome::Offline | SyncRuntimeOutcome::ServerTransient => {
            state.transient_failures = state.transient_failures.saturating_add(1);
            let delay = exponential_backoff(
                config.transient_backoff_initial(),
                config.transient_backoff_max(),
                state.transient_failures,
            );
            state.phase = SyncRuntimeLibraryPhase::BackingOff;
            state.next_due = Some(now + delay);
            decision.backoff = Some(delay);
            if pending.is_some_and(bypasses_transient_backoff) {
                state.phase = SyncRuntimeLibraryPhase::Scheduled;
                state.next_due = Some(now);
            }
        }
        SyncRuntimeOutcome::RateLimited => {
            state.phase = SyncRuntimeLibraryPhase::BackingOff;
            state.next_due = Some(now + config.rate_limit_fallback());
            decision.backoff = Some(config.rate_limit_fallback());
        }
        SyncRuntimeOutcome::RecoveryBlocked => {
            state.transient_failures = state.transient_failures.saturating_add(1);
            let delay = exponential_backoff(
                config.transient_backoff_initial(),
                config.transient_backoff_max(),
                state.transient_failures,
            );
            state.phase = SyncRuntimeLibraryPhase::BackingOff;
            state.next_due = Some(now + delay);
            decision.backoff = Some(delay);
            if pending.is_some_and(bypasses_transient_backoff) {
                state.phase = SyncRuntimeLibraryPhase::Scheduled;
                state.next_due = Some(now);
            }
        }
        SyncRuntimeOutcome::AuthBlocked => {
            state.phase = SyncRuntimeLibraryPhase::AuthBlocked;
            state.next_due = None;
            decision.auth_blocked = true;
            if pending.is_some_and(bypasses_auth_block) {
                state.phase = SyncRuntimeLibraryPhase::Scheduled;
                state.next_due = Some(now);
            } else {
                state.pending_wake = pending;
            }
        }
        SyncRuntimeOutcome::RootUnavailable => {
            if pending == Some(SyncRuntimeWakeReason::RootAvailable) {
                state.phase = SyncRuntimeLibraryPhase::Scheduled;
                state.next_due = Some(now);
                state.pending_wake = None;
            } else {
                state.phase = SyncRuntimeLibraryPhase::RootBlocked;
                state.next_due = None;
                state.pending_wake = pending;
            }
        }
        SyncRuntimeOutcome::FatalLocal | SyncRuntimeOutcome::Panicked => {
            state.phase = SyncRuntimeLibraryPhase::Faulted;
            state.next_due = None;
            decision.faulted = true;
            if pending == Some(SyncRuntimeWakeReason::Manual) {
                state.phase = SyncRuntimeLibraryPhase::Scheduled;
                state.next_due = Some(now);
            } else {
                state.pending_wake = pending;
            }
        }
    }
    decision
}

fn apply_wake(
    state: &mut RuntimeLibraryState,
    reason: SyncRuntimeWakeReason,
    now: Instant,
) -> SyncRuntimeWakeResult {
    if state.running {
        state.pending_wake = Some(merge_wake(state.pending_wake, reason));
        return SyncRuntimeWakeResult::AlreadyRunningFollowupRecorded;
    }
    match state.phase {
        SyncRuntimeLibraryPhase::AuthBlocked => {
            if bypasses_auth_block(reason) {
                state.phase = SyncRuntimeLibraryPhase::Scheduled;
                state.next_due = Some(now);
                state.pending_wake = None;
                SyncRuntimeWakeResult::Queued
            } else {
                state.pending_wake = Some(merge_wake(state.pending_wake, reason));
                SyncRuntimeWakeResult::Coalesced
            }
        }
        SyncRuntimeLibraryPhase::RootBlocked => {
            if matches!(
                reason,
                SyncRuntimeWakeReason::RootAvailable | SyncRuntimeWakeReason::Manual
            ) {
                state.phase = SyncRuntimeLibraryPhase::Scheduled;
                state.next_due = Some(now);
                state.pending_wake = None;
                SyncRuntimeWakeResult::Queued
            } else {
                state.pending_wake = Some(merge_wake(state.pending_wake, reason));
                SyncRuntimeWakeResult::Coalesced
            }
        }
        SyncRuntimeLibraryPhase::Faulted => {
            if reason == SyncRuntimeWakeReason::Manual {
                state.phase = SyncRuntimeLibraryPhase::Scheduled;
                state.next_due = Some(now);
                state.pending_wake = None;
                SyncRuntimeWakeResult::Queued
            } else {
                state.pending_wake = Some(merge_wake(state.pending_wake, reason));
                SyncRuntimeWakeResult::Coalesced
            }
        }
        SyncRuntimeLibraryPhase::BackingOff
            if state.last_outcome == Some(SyncRuntimeOutcome::RateLimited) =>
        {
            // Prompt 91 does not expose Retry-After, so the runtime honors its
            // bounded fallback even for manual/network wakes.
            state.pending_wake = Some(merge_wake(state.pending_wake, reason));
            SyncRuntimeWakeResult::Coalesced
        }
        SyncRuntimeLibraryPhase::BackingOff
            if matches!(
                reason,
                SyncRuntimeWakeReason::Manual
                    | SyncRuntimeWakeReason::NetworkAvailable
                    | SyncRuntimeWakeReason::CredentialChanged
            ) =>
        {
            state.phase = SyncRuntimeLibraryPhase::Scheduled;
            state.next_due = Some(now);
            state.pending_wake = None;
            SyncRuntimeWakeResult::Queued
        }
        SyncRuntimeLibraryPhase::BackingOff => {
            state.pending_wake = Some(merge_wake(state.pending_wake, reason));
            SyncRuntimeWakeResult::Coalesced
        }
        _ => {
            let already_scheduled = state.phase == SyncRuntimeLibraryPhase::Scheduled
                && state.next_due.is_some_and(|next_due| next_due <= now)
                && state.pending_wake.is_none();
            state.phase = SyncRuntimeLibraryPhase::Scheduled;
            state.next_due = Some(now);
            state.pending_wake = None;
            if already_scheduled {
                SyncRuntimeWakeResult::Coalesced
            } else {
                SyncRuntimeWakeResult::Queued
            }
        }
    }
}

fn merge_wake(
    existing: Option<SyncRuntimeWakeReason>,
    incoming: SyncRuntimeWakeReason,
) -> SyncRuntimeWakeReason {
    existing.map_or(incoming, |current| {
        if incoming.priority() >= current.priority() {
            incoming
        } else {
            current
        }
    })
}

fn bypasses_transient_backoff(reason: SyncRuntimeWakeReason) -> bool {
    matches!(
        reason,
        SyncRuntimeWakeReason::Manual
            | SyncRuntimeWakeReason::NetworkAvailable
            | SyncRuntimeWakeReason::CredentialChanged
    )
}

fn bypasses_auth_block(reason: SyncRuntimeWakeReason) -> bool {
    matches!(
        reason,
        SyncRuntimeWakeReason::Manual | SyncRuntimeWakeReason::CredentialChanged
    )
}

fn exponential_backoff(initial: Duration, maximum: Duration, failures: u32) -> Duration {
    let mut delay = initial;
    // The configured maximum is at most one hour, so no more than 32 doublings
    // can affect a result. Capping the loop also keeps corrupted ephemeral
    // counters from creating an unbounded scheduler decision.
    for _ in 1..failures.min(32) {
        delay = delay.checked_mul(2).unwrap_or(maximum);
        if delay >= maximum {
            return maximum;
        }
    }
    delay.min(maximum)
}

fn classify_execution(execution: &CycleExecution) -> SyncRuntimeOutcome {
    match execution {
        CycleExecution::Panicked => SyncRuntimeOutcome::Panicked,
        CycleExecution::Returned(Err(error)) => classify_error(error),
        CycleExecution::Returned(Ok(result)) => classify_result(*result),
    }
}

fn classify_result(result: SyncCycleResult) -> SyncRuntimeOutcome {
    if result.requires_authentication() {
        return SyncRuntimeOutcome::AuthBlocked;
    }
    match result.inbound() {
        InboundCycleOutcome::RateLimited => return SyncRuntimeOutcome::RateLimited,
        InboundCycleOutcome::Offline => return SyncRuntimeOutcome::Offline,
        InboundCycleOutcome::RemoteFailure(kind) => return classify_remote_kind(kind),
        InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::RecoveryBlocked {
            reason,
        }) => {
            return match reason {
                RebaselineRecoveryBlockedReason::RateLimited => SyncRuntimeOutcome::RateLimited,
                RebaselineRecoveryBlockedReason::DidNotConverge => {
                    SyncRuntimeOutcome::RecoveryBlocked
                }
            };
        }
        InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::IncrementalProgress(
            SyncOutcome::Blocked,
        )) => return SyncRuntimeOutcome::FatalLocal,
        _ => {}
    }
    match result.outbound() {
        OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::AuthRequired) => {
            return SyncRuntimeOutcome::AuthBlocked;
        }
        OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Offline) => {
            return SyncRuntimeOutcome::Offline;
        }
        OutboundCycleOutcome::Attempted(
            OutboundSubmissionOutcome::Conflict { .. }
            | OutboundSubmissionOutcome::BlockedByConflict(_),
        ) => return SyncRuntimeOutcome::ConflictBlocked,
        OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Blocked(_)) => {
            return SyncRuntimeOutcome::FatalLocal;
        }
        OutboundCycleOutcome::NotAttempted(
            OutboundSkipReason::LocalIssue | OutboundSkipReason::ReplicaNotReady,
        ) => return SyncRuntimeOutcome::FatalLocal,
        _ => {}
    }
    if result.made_durable_progress() || result.more_work_likely() {
        SyncRuntimeOutcome::Progress
    } else {
        SyncRuntimeOutcome::Idle
    }
}

fn classify_error(error: &ClientSyncError) -> SyncRuntimeOutcome {
    match error {
        ClientSyncError::AuthenticationRequired => SyncRuntimeOutcome::AuthBlocked,
        ClientSyncError::RootUnavailable
        | ClientSyncError::InvalidRoot
        | ClientSyncError::WrongRootBinding => SyncRuntimeOutcome::RootUnavailable,
        ClientSyncError::HandoffTransport => SyncRuntimeOutcome::Offline,
        ClientSyncError::Remote(remote) => classify_remote_kind(remote.kind()),
        ClientSyncError::HandoffSnapshotUnavailable
        | ClientSyncError::HandoffCheckpointConflict
        | ClientSyncError::RebaselineInProgress
        | ClientSyncError::RebaselinePendingHandoff
        | ClientSyncError::CandidateIncomplete => SyncRuntimeOutcome::RecoveryBlocked,
        _ => SyncRuntimeOutcome::FatalLocal,
    }
}

fn classify_remote_kind(kind: RemoteErrorKind) -> SyncRuntimeOutcome {
    match kind {
        RemoteErrorKind::RateLimited => SyncRuntimeOutcome::RateLimited,
        RemoteErrorKind::Offline => SyncRuntimeOutcome::Offline,
        RemoteErrorKind::Unavailable | RemoteErrorKind::Internal | RemoteErrorKind::Timeout => {
            SyncRuntimeOutcome::ServerTransient
        }
        RemoteErrorKind::AuthRequired
        | RemoteErrorKind::DeviceRevoked
        | RemoteErrorKind::Forbidden => SyncRuntimeOutcome::AuthBlocked,
        _ => SyncRuntimeOutcome::FatalLocal,
    }
}

async fn join_shared(shared: Arc<RuntimeShared>) -> Result<(), SyncRuntimeError> {
    loop {
        let notified = shared.completion_notify.notified();
        let join = {
            let mut lifecycle = lock_unpoisoned(&shared.lifecycle);
            if let Some(join) = lifecycle.join.take() {
                Some(join)
            } else if lifecycle.phase == LifecyclePhase::Stopped {
                return result_from_terminal_failure(lifecycle.terminal_failure);
            } else {
                None
            }
        };
        if let Some(join) = join {
            if join.await.is_err() {
                let mut lifecycle = lock_unpoisoned(&shared.lifecycle);
                lifecycle.phase = LifecyclePhase::Stopped;
                lifecycle.terminal_failure = Some(SyncRuntimeError::TaskPanicked);
                drop(lifecycle);
                shared.completion_notify.notify_waiters();
                return Err(SyncRuntimeError::TaskPanicked);
            }
            let lifecycle = lock_unpoisoned(&shared.lifecycle);
            return result_from_terminal_failure(lifecycle.terminal_failure);
        }
        notified.await;
    }
}

fn result_from_terminal_failure(error: Option<SyncRuntimeError>) -> Result<(), SyncRuntimeError> {
    error.map_or(Ok(()), Err)
}

fn request_shutdown_on_last_controller(shared: &Arc<RuntimeShared>) {
    // The supervisor itself owns one Arc. When only one external controller is
    // left, dropping it is a safe best-effort graceful shutdown request. This
    // does not abort an in-flight Prompt 91 future.
    if Arc::strong_count(shared) == 2 {
        let _ = shared.request_shutdown();
    }
}

fn lock_unpoisoned<T>(mutex: &StdMutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use synveil_core::{DeviceId, LibraryId, UserId};
    use tokio::sync::Notify;

    use super::*;

    fn scope() -> ReplicaScope {
        ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new())
    }

    fn idle_result() -> SyncCycleResult {
        SyncCycleResult::new(
            Timestamp::parse("2026-09-12T00:00:00Z").expect("fixed timestamp"),
            InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::IncrementalReady),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::NoReadyIntent),
        )
    }

    fn progress_result() -> SyncCycleResult {
        SyncCycleResult::new(
            Timestamp::parse("2026-09-12T00:00:00Z").expect("fixed timestamp"),
            InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::IncrementalProgress(
                SyncOutcome::MoreAvailable,
            )),
            OutboundCycleOutcome::NotAttempted(OutboundSkipReason::InboundWorkPending),
        )
    }

    struct FakeExecutor {
        scope: ReplicaScope,
        outcomes: Mutex<VecDeque<Result<SyncCycleResult, ClientSyncError>>>,
        calls: AtomicUsize,
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
        global_active: Arc<AtomicUsize>,
        global_max_active: Arc<AtomicUsize>,
        started: Notify,
        release: Notify,
        hold: AtomicBool,
    }

    impl FakeExecutor {
        fn new(
            scope: ReplicaScope,
            outcomes: Vec<Result<SyncCycleResult, ClientSyncError>>,
        ) -> Arc<Self> {
            Self::with_global_activity(
                scope,
                outcomes,
                Arc::new(AtomicUsize::new(0)),
                Arc::new(AtomicUsize::new(0)),
            )
        }

        fn with_global_activity(
            scope: ReplicaScope,
            outcomes: Vec<Result<SyncCycleResult, ClientSyncError>>,
            global_active: Arc<AtomicUsize>,
            global_max_active: Arc<AtomicUsize>,
        ) -> Arc<Self> {
            Arc::new(Self {
                scope,
                outcomes: Mutex::new(outcomes.into_iter().collect()),
                calls: AtomicUsize::new(0),
                active: Arc::new(AtomicUsize::new(0)),
                max_active: Arc::new(AtomicUsize::new(0)),
                global_active,
                global_max_active,
                started: Notify::new(),
                release: Notify::new(),
                hold: AtomicBool::new(false),
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn max_active(&self) -> usize {
            self.max_active.load(Ordering::SeqCst)
        }

        fn release(&self) {
            self.hold.store(false, Ordering::SeqCst);
            self.release.notify_waiters();
        }
    }

    #[async_trait]
    impl SyncCycleExecutor for FakeExecutor {
        fn scope(&self) -> ReplicaScope {
            self.scope
        }

        async fn run_once(
            &self,
            _observed_at: Timestamp,
        ) -> Result<SyncCycleResult, ClientSyncError> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            let global_active = self.global_active.fetch_add(1, Ordering::SeqCst) + 1;
            self.global_max_active
                .fetch_max(global_active, Ordering::SeqCst);
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.started.notify_waiters();
            if self.hold.load(Ordering::SeqCst) {
                self.release.notified().await;
            }
            let result = self
                .outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(idle_result()));
            self.active.fetch_sub(1, Ordering::SeqCst);
            self.global_active.fetch_sub(1, Ordering::SeqCst);
            result
        }
    }

    fn config() -> SyncRuntimeConfig {
        SyncRuntimeConfig::new(
            Duration::from_secs(10),
            Duration::from_secs(1),
            Duration::from_secs(8),
            Duration::from_secs(5),
            2,
        )
        .expect("test config")
    }

    async fn settle() {
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
    }

    #[test]
    fn config_rejects_unsafe_bounds() {
        assert_eq!(
            SyncRuntimeConfig::new(
                Duration::ZERO,
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(1),
                1,
            ),
            Err(SyncRuntimeConfigError::PollIntervalTooShort)
        );
        assert_eq!(
            SyncRuntimeConfig::new(
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(1),
                Duration::from_secs(1),
                1,
            ),
            Err(SyncRuntimeConfigError::TransientBackoffOrder)
        );
        assert_eq!(
            SyncRuntimeConfig::new(
                Duration::from_secs(1),
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(1),
                0,
            ),
            Err(SyncRuntimeConfigError::ConcurrencyZero)
        );
    }

    #[test]
    fn backoff_is_deterministic_and_capped() {
        assert_eq!(
            exponential_backoff(Duration::from_secs(1), Duration::from_secs(60), 1),
            Duration::from_secs(1)
        );
        assert_eq!(
            exponential_backoff(Duration::from_secs(1), Duration::from_secs(60), 6),
            Duration::from_secs(32)
        );
        assert_eq!(
            exponential_backoff(Duration::from_secs(1), Duration::from_secs(60), 7),
            Duration::from_secs(60)
        );
        assert_eq!(
            exponential_backoff(Duration::from_secs(1), Duration::from_secs(60), u32::MAX),
            Duration::from_secs(60)
        );
    }

    #[test]
    fn result_categories_select_safe_scheduler_policies() {
        let observed_at = Timestamp::parse("2026-09-12T00:00:00Z").expect("fixed timestamp");
        let conflict = SyncCycleResult::new(
            observed_at,
            InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::IncrementalReady),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::BlockedByConflict(
                synveil_core::SyncConflictId::new(),
            )),
        );
        assert_eq!(
            classify_result(conflict),
            SyncRuntimeOutcome::ConflictBlocked
        );
        assert_eq!(classify_result(idle_result()), SyncRuntimeOutcome::Idle);
        assert_eq!(
            classify_result(progress_result()),
            SyncRuntimeOutcome::Progress
        );
        assert_eq!(
            classify_result(SyncCycleResult::new(
                observed_at,
                InboundCycleOutcome::RateLimited,
                OutboundCycleOutcome::NotAttempted(OutboundSkipReason::InboundRateLimited),
            )),
            SyncRuntimeOutcome::RateLimited
        );
        assert_eq!(
            classify_result(SyncCycleResult::new(
                observed_at,
                InboundCycleOutcome::AuthRequired,
                OutboundCycleOutcome::NotAttempted(
                    OutboundSkipReason::InboundAuthenticationRequired,
                ),
            )),
            SyncRuntimeOutcome::AuthBlocked
        );
        assert_eq!(
            classify_result(SyncCycleResult::new(
                observed_at,
                InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::RecoveryBlocked {
                    reason: RebaselineRecoveryBlockedReason::DidNotConverge,
                },),
                OutboundCycleOutcome::NotAttempted(OutboundSkipReason::RecoveryBlocked(
                    RebaselineRecoveryBlockedReason::DidNotConverge,
                )),
            )),
            SyncRuntimeOutcome::RecoveryBlocked
        );
        assert_eq!(
            classify_error(&ClientSyncError::InvalidState),
            SyncRuntimeOutcome::FatalLocal
        );
        assert_eq!(
            classify_error(&ClientSyncError::HandoffTransport),
            SyncRuntimeOutcome::Offline
        );
    }

    #[test]
    fn schedule_policy_has_exact_poll_backoff_and_reset_boundaries() {
        let now = Instant::now();
        let runtime_config = config();
        let mut state = RuntimeLibraryState::new();
        state.reset_for_start(now);

        let first = schedule_after_cycle(
            &mut state,
            runtime_config,
            SyncRuntimeOutcome::ServerTransient,
            None,
            now,
        );
        assert_eq!(first.backoff, Some(Duration::from_secs(1)));
        assert_eq!(state.phase, SyncRuntimeLibraryPhase::BackingOff);
        assert_eq!(
            state.next_due.expect("backoff due") - now,
            Duration::from_secs(1)
        );
        assert_eq!(state.transient_failures, 1);

        let second_now = now + Duration::from_secs(1);
        let second = schedule_after_cycle(
            &mut state,
            runtime_config,
            SyncRuntimeOutcome::Offline,
            None,
            second_now,
        );
        assert_eq!(second.backoff, Some(Duration::from_secs(2)));
        assert_eq!(
            state.next_due.expect("second backoff due") - second_now,
            Duration::from_secs(2)
        );
        assert_eq!(state.transient_failures, 2);

        let success_now = second_now + Duration::from_secs(2);
        let success = schedule_after_cycle(
            &mut state,
            runtime_config,
            SyncRuntimeOutcome::Progress,
            None,
            success_now,
        );
        assert_eq!(success.backoff, None);
        assert_eq!(state.phase, SyncRuntimeLibraryPhase::Scheduled);
        assert_eq!(state.transient_failures, 0);
        assert_eq!(
            state.next_due.expect("fair follow-up due") - success_now,
            FAIR_FOLLOW_UP_DELAY
        );
    }

    #[test]
    fn rate_limit_auth_and_fault_wakes_follow_their_safety_policies() {
        let now = Instant::now();
        let runtime_config = config();
        let mut state = RuntimeLibraryState::new();
        state.reset_for_start(now);
        state.last_outcome = Some(SyncRuntimeOutcome::RateLimited);
        let rate = schedule_after_cycle(
            &mut state,
            runtime_config,
            SyncRuntimeOutcome::RateLimited,
            Some(SyncRuntimeWakeReason::Manual),
            now,
        );
        assert_eq!(rate.backoff, Some(runtime_config.rate_limit_fallback()));
        assert_eq!(state.phase, SyncRuntimeLibraryPhase::BackingOff);
        assert_eq!(state.pending_wake, None);
        let rate_due = state.next_due.expect("rate-limit due");
        apply_wake(&mut state, SyncRuntimeWakeReason::Manual, now);
        assert_eq!(state.phase, SyncRuntimeLibraryPhase::BackingOff);
        assert!(state.pending_wake.is_some());
        assert_eq!(state.next_due, Some(rate_due));

        state.last_outcome = Some(SyncRuntimeOutcome::Offline);
        let offline_due = now + Duration::from_secs(1);
        state.next_due = Some(offline_due);
        apply_wake(&mut state, SyncRuntimeWakeReason::LocalChange, now);
        assert_eq!(state.phase, SyncRuntimeLibraryPhase::BackingOff);
        assert_eq!(state.next_due, Some(offline_due));
        apply_wake(&mut state, SyncRuntimeWakeReason::NetworkAvailable, now);
        assert_eq!(state.phase, SyncRuntimeLibraryPhase::Scheduled);
        assert_eq!(state.next_due, Some(now));

        state = RuntimeLibraryState::new();
        state.reset_for_start(now);
        let auth = schedule_after_cycle(
            &mut state,
            runtime_config,
            SyncRuntimeOutcome::AuthBlocked,
            Some(SyncRuntimeWakeReason::LocalChange),
            now,
        );
        assert!(auth.auth_blocked);
        assert_eq!(state.phase, SyncRuntimeLibraryPhase::AuthBlocked);
        assert_eq!(state.next_due, None);
        assert_eq!(state.pending_wake, Some(SyncRuntimeWakeReason::LocalChange));
        apply_wake(&mut state, SyncRuntimeWakeReason::CredentialChanged, now);
        assert_eq!(state.phase, SyncRuntimeLibraryPhase::Scheduled);
        assert_eq!(state.next_due, Some(now));
        assert_eq!(state.pending_wake, None);

        state = RuntimeLibraryState::new();
        state.reset_for_start(now);
        let fault = schedule_after_cycle(
            &mut state,
            runtime_config,
            SyncRuntimeOutcome::FatalLocal,
            Some(SyncRuntimeWakeReason::NetworkAvailable),
            now,
        );
        assert!(fault.faulted);
        assert_eq!(state.phase, SyncRuntimeLibraryPhase::Faulted);
        assert_eq!(state.next_due, None);
        apply_wake(&mut state, SyncRuntimeWakeReason::Manual, now);
        assert_eq!(state.phase, SyncRuntimeLibraryPhase::Scheduled);
        assert_eq!(state.next_due, Some(now));
    }

    #[test]
    fn root_unavailable_blocks_periodic_work_but_manual_and_return_wakes_are_safe() {
        let now = Instant::now();
        let mut state = RuntimeLibraryState::new();
        state.reset_for_start(now);

        let decision = schedule_after_cycle(
            &mut state,
            config(),
            SyncRuntimeOutcome::RootUnavailable,
            None,
            now,
        );
        assert_eq!(decision.backoff, None);
        assert_eq!(state.phase, SyncRuntimeLibraryPhase::RootBlocked);
        assert_eq!(state.next_due, None);

        assert_eq!(
            apply_wake(&mut state, SyncRuntimeWakeReason::LocalChange, now),
            SyncRuntimeWakeResult::Coalesced
        );
        assert_eq!(state.phase, SyncRuntimeLibraryPhase::RootBlocked);
        assert_eq!(
            apply_wake(&mut state, SyncRuntimeWakeReason::Manual, now),
            SyncRuntimeWakeResult::Queued
        );
        assert_eq!(state.phase, SyncRuntimeLibraryPhase::Scheduled);
        assert_eq!(state.next_due, Some(now));

        state.phase = SyncRuntimeLibraryPhase::RootBlocked;
        state.next_due = None;
        state.pending_wake = None;
        assert_eq!(
            apply_wake(&mut state, SyncRuntimeWakeReason::RootAvailable, now),
            SyncRuntimeWakeResult::Queued
        );
        assert_eq!(state.phase, SyncRuntimeLibraryPhase::Scheduled);
        assert_eq!(state.next_due, Some(now));
    }

    #[test]
    fn wake_during_conflict_is_retained_as_a_bounded_follow_up() {
        let now = Instant::now();
        let mut state = RuntimeLibraryState::new();
        state.reset_for_start(now);

        let decision = schedule_after_cycle(
            &mut state,
            config(),
            SyncRuntimeOutcome::ConflictBlocked,
            Some(SyncRuntimeWakeReason::Manual),
            now,
        );

        assert_eq!(decision.backoff, None);
        assert_eq!(state.phase, SyncRuntimeLibraryPhase::Scheduled);
        assert_eq!(state.pending_wake, None);
        assert_eq!(state.next_due, Some(now + FAIR_FOLLOW_UP_DELAY));
    }

    #[tokio::test(start_paused = true)]
    async fn start_is_idempotent_and_registration_is_type_erased_and_removable() {
        let executor = FakeExecutor::new(scope(), vec![Ok(idle_result())]);
        let library_id = executor.scope.library_id();
        let duplicate = FakeExecutor::new(executor.scope, vec![Ok(idle_result())]);
        let runtime = SyncRuntime::new(config());
        assert_eq!(
            runtime
                .register_library(executor.clone())
                .expect("register"),
            SyncRuntimeRegistration::Registered
        );
        let erased: Arc<dyn SyncCycleExecutor> = duplicate;
        assert_eq!(
            runtime
                .register_executor(erased)
                .expect("duplicate register"),
            SyncRuntimeRegistration::AlreadyRegistered
        );

        let handle = runtime.start().expect("start");
        assert!(matches!(
            runtime.start(),
            Err(SyncRuntimeError::AlreadyRunning)
        ));
        settle().await;
        assert_eq!(executor.calls(), 1);
        handle.shutdown().await.expect("shutdown");
        assert_eq!(
            runtime.unregister_library(library_id).expect("unregister"),
            SyncRuntimeUnregistration::Unregistered
        );
        assert_eq!(
            runtime
                .unregister_library(library_id)
                .expect("repeat unregister"),
            SyncRuntimeUnregistration::AlreadyUnregistered
        );
        assert!(runtime.status(library_id).is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn idle_periodic_poll_uses_the_configured_virtual_duration() {
        let executor = FakeExecutor::new(scope(), vec![Ok(idle_result()), Ok(idle_result())]);
        let library_id = executor.scope.library_id();
        let runtime = SyncRuntime::new(config());
        runtime
            .register_library(executor.clone())
            .expect("register");
        let handle = runtime.start().expect("start");
        settle().await;
        assert_eq!(executor.calls(), 1);
        assert_eq!(
            handle.status(library_id).expect("status").next_due_in(),
            Some(Duration::from_secs(10))
        );
        tokio::time::advance(Duration::from_secs(9)).await;
        settle().await;
        assert_eq!(executor.calls(), 1);
        tokio::time::advance(Duration::from_secs(1)).await;
        settle().await;
        assert_eq!(executor.calls(), 2);
        handle.shutdown().await.expect("shutdown");
    }

    #[tokio::test(start_paused = true)]
    async fn transient_backoff_doubles_and_success_resets_it() {
        let executor = FakeExecutor::new(
            scope(),
            vec![
                Err(ClientSyncError::HandoffTransport),
                Err(ClientSyncError::HandoffTransport),
                Ok(idle_result()),
            ],
        );
        let library_id = executor.scope.library_id();
        let runtime = SyncRuntime::new(config());
        runtime
            .register_library(executor.clone())
            .expect("register");
        let handle = runtime.start().expect("start");
        settle().await;
        assert_eq!(executor.calls(), 1);
        assert_eq!(
            handle.status(library_id).expect("status").last_outcome(),
            Some(SyncRuntimeOutcome::Offline)
        );
        assert_eq!(
            handle
                .status(library_id)
                .expect("status")
                .transient_failures(),
            1
        );
        tokio::time::advance(Duration::from_millis(999)).await;
        settle().await;
        assert_eq!(executor.calls(), 1);
        tokio::time::advance(Duration::from_millis(1)).await;
        settle().await;
        assert_eq!(executor.calls(), 2);
        assert_eq!(
            handle
                .status(library_id)
                .expect("status")
                .transient_failures(),
            2
        );
        assert_eq!(
            handle.status(library_id).expect("status").next_due_in(),
            Some(Duration::from_secs(2))
        );
        tokio::time::advance(Duration::from_secs(2)).await;
        settle().await;
        assert_eq!(executor.calls(), 3);
        assert_eq!(
            handle
                .status(library_id)
                .expect("status")
                .transient_failures(),
            0
        );
        handle.shutdown().await.expect("shutdown");
    }

    #[tokio::test(start_paused = true)]
    async fn network_wake_bypasses_transient_delay_but_rate_limit_does_not() {
        let offline = FakeExecutor::new(
            scope(),
            vec![Err(ClientSyncError::HandoffTransport), Ok(idle_result())],
        );
        let offline_id = offline.scope.library_id();
        let runtime = SyncRuntime::new(config());
        runtime
            .register_library(offline.clone())
            .expect("register offline");
        let offline_handle = runtime.start().expect("start offline");
        settle().await;
        assert_eq!(offline.calls(), 1);
        tokio::time::advance(Duration::from_millis(500)).await;
        offline_handle
            .wake_library(offline_id, SyncRuntimeWakeReason::NetworkAvailable)
            .expect("network wake");
        settle().await;
        assert_eq!(offline.calls(), 2);
        offline_handle.shutdown().await.expect("shutdown offline");

        let rate_limited = FakeExecutor::new(
            scope(),
            vec![
                Ok(SyncCycleResult::new(
                    Timestamp::parse("2026-09-12T00:00:00Z").expect("fixed timestamp"),
                    InboundCycleOutcome::RateLimited,
                    OutboundCycleOutcome::NotAttempted(OutboundSkipReason::InboundRateLimited),
                )),
                Ok(idle_result()),
            ],
        );
        let rate_limited_id = rate_limited.scope.library_id();
        let rate_runtime = SyncRuntime::new(config());
        rate_runtime
            .register_library(rate_limited.clone())
            .expect("register rate-limited");
        let rate_handle = rate_runtime.start().expect("start rate-limited");
        settle().await;
        assert_eq!(rate_limited.calls(), 1);
        rate_handle
            .wake_library(rate_limited_id, SyncRuntimeWakeReason::Manual)
            .expect("manual wake");
        settle().await;
        assert_eq!(rate_limited.calls(), 1);
        tokio::time::advance(Duration::from_secs(4)).await;
        settle().await;
        assert_eq!(rate_limited.calls(), 1);
        tokio::time::advance(Duration::from_secs(1)).await;
        settle().await;
        assert_eq!(rate_limited.calls(), 2);
        rate_handle.shutdown().await.expect("shutdown rate-limited");
    }

    #[tokio::test(start_paused = true)]
    async fn conflict_keeps_inbound_polling_without_immediate_retry() {
        let executor = FakeExecutor::new(
            scope(),
            vec![
                Ok(SyncCycleResult::new(
                    Timestamp::parse("2026-09-12T00:00:00Z").expect("fixed timestamp"),
                    InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::IncrementalReady),
                    OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::BlockedByConflict(
                        synveil_core::SyncConflictId::new(),
                    )),
                )),
                Ok(idle_result()),
            ],
        );
        let library_id = executor.scope.library_id();
        let runtime = SyncRuntime::new(config());
        runtime
            .register_library(executor.clone())
            .expect("register");
        let handle = runtime.start().expect("start");
        settle().await;
        assert_eq!(executor.calls(), 1);
        assert_eq!(
            handle.status(library_id).expect("status").last_outcome(),
            Some(SyncRuntimeOutcome::ConflictBlocked)
        );
        tokio::time::advance(Duration::from_secs(9)).await;
        settle().await;
        assert_eq!(executor.calls(), 1);
        tokio::time::advance(Duration::from_secs(1)).await;
        settle().await;
        assert_eq!(executor.calls(), 2);
        handle.shutdown().await.expect("shutdown");
    }

    #[tokio::test(start_paused = true)]
    async fn local_fault_isolated_to_one_library_and_requires_manual_wake() {
        let faulty = FakeExecutor::new(scope(), vec![Err(ClientSyncError::InvalidState)]);
        let healthy = FakeExecutor::new(scope(), vec![Ok(idle_result()), Ok(idle_result())]);
        let faulty_id = faulty.scope.library_id();
        let healthy_id = healthy.scope.library_id();
        let runtime = SyncRuntime::new(config());
        runtime
            .register_library(faulty.clone())
            .expect("register faulty");
        runtime
            .register_library(healthy.clone())
            .expect("register healthy");
        let handle = runtime.start().expect("start");
        settle().await;
        assert_eq!(faulty.calls(), 1);
        assert_eq!(healthy.calls(), 1);
        assert_eq!(
            handle.status(faulty_id).expect("fault status").phase(),
            SyncRuntimeLibraryPhase::Faulted
        );
        assert_eq!(
            handle.status(healthy_id).expect("healthy status").phase(),
            SyncRuntimeLibraryPhase::Idle
        );
        tokio::time::advance(Duration::from_secs(20)).await;
        settle().await;
        assert_eq!(faulty.calls(), 1);
        assert_eq!(healthy.calls(), 2);
        handle
            .wake_library(faulty_id, SyncRuntimeWakeReason::Manual)
            .expect("manual fault wake");
        settle().await;
        assert_eq!(faulty.calls(), 2);
        handle.shutdown().await.expect("shutdown");
    }

    #[tokio::test(start_paused = true)]
    async fn startup_runs_one_cycle_and_manual_wake_interrupts_idle_poll() {
        let executor = FakeExecutor::new(scope(), vec![Ok(idle_result()), Ok(idle_result())]);
        let library_id = executor.scope.library_id();
        let runtime = SyncRuntime::new(config());
        assert_eq!(
            runtime
                .register_library(executor.clone())
                .expect("register"),
            SyncRuntimeRegistration::Registered
        );
        let handle = runtime.start().expect("start");
        settle().await;
        assert_eq!(executor.calls(), 1);
        assert_eq!(
            handle.status(library_id).expect("status").phase(),
            SyncRuntimeLibraryPhase::Idle
        );
        handle
            .wake_library(library_id, SyncRuntimeWakeReason::Manual)
            .expect("manual wake");
        settle().await;
        assert_eq!(executor.calls(), 2);
        handle.shutdown().await.expect("shutdown");
    }

    #[tokio::test(start_paused = true)]
    async fn wake_results_distinguish_queue_coalescing_running_stopped_and_unknown() {
        let executor = FakeExecutor::new(scope(), vec![Ok(idle_result()), Ok(idle_result())]);
        let library_id = executor.scope.library_id();
        let unknown = LibraryId::new();
        let runtime = SyncRuntime::new(config());
        assert_eq!(
            runtime.wake_library_status(library_id, SyncRuntimeWakeReason::Manual),
            SyncRuntimeWakeResult::RuntimeStopped
        );
        runtime
            .register_library(executor.clone())
            .expect("register");
        let handle = runtime.start().expect("start");
        settle().await;
        assert_eq!(
            handle.wake_library_status(unknown, SyncRuntimeWakeReason::Manual),
            SyncRuntimeWakeResult::UnknownLibrary
        );
        assert_eq!(handle.sync_now(library_id), SyncRuntimeWakeResult::Queued);
        assert_eq!(
            handle.wake_library_status(library_id, SyncRuntimeWakeReason::LocalChange),
            SyncRuntimeWakeResult::Coalesced
        );
        settle().await;
        handle.shutdown().await.expect("shutdown");
        assert_eq!(
            handle.wake_library_status(library_id, SyncRuntimeWakeReason::Manual),
            SyncRuntimeWakeResult::RuntimeStopped
        );

        let running_executor = FakeExecutor::new(scope(), vec![Ok(idle_result())]);
        running_executor.hold.store(true, Ordering::SeqCst);
        let running_id = running_executor.scope.library_id();
        let running_runtime = SyncRuntime::new(config());
        running_runtime
            .register_library(running_executor.clone())
            .expect("register running executor");
        let running_handle = running_runtime.start().expect("start running executor");
        settle().await;
        assert_eq!(
            running_handle.wake_library_status(running_id, SyncRuntimeWakeReason::Manual),
            SyncRuntimeWakeResult::AlreadyRunningFollowupRecorded
        );
        running_executor.release();
        settle().await;
        running_handle.shutdown().await.expect("running shutdown");
    }

    #[tokio::test(start_paused = true)]
    async fn runtime_and_handle_clones_share_one_supervisor() {
        let executor = FakeExecutor::new(scope(), vec![Ok(idle_result()), Ok(idle_result())]);
        let library_id = executor.scope.library_id();
        let runtime = SyncRuntime::new(config());
        runtime
            .register_library(executor.clone())
            .expect("register clone test executor");
        let runtime_clone = runtime.clone();
        let handle = runtime.start().expect("start clone test runtime");
        let handle_clone = handle.clone();

        assert!(matches!(
            runtime_clone.start(),
            Err(SyncRuntimeError::AlreadyRunning)
        ));
        settle().await;
        assert_eq!(executor.calls(), 1);
        assert_eq!(
            handle_clone.sync_now(library_id),
            SyncRuntimeWakeResult::Queued
        );
        settle().await;
        assert_eq!(executor.calls(), 2);
        assert_eq!(
            runtime_clone.status(library_id),
            handle_clone.status(library_id)
        );

        handle_clone
            .shutdown()
            .await
            .expect("clone shutdown must join the shared supervisor");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn registration_and_unregistration_races_keep_wakes_typed_and_recoverable() {
        let executor = FakeExecutor::new(scope(), vec![Ok(idle_result()), Ok(idle_result())]);
        let library_id = executor.scope.library_id();
        let runtime = SyncRuntime::new(config());
        let handle = runtime.start().expect("start race test runtime");
        let first_cycle_started = executor.started.notified();

        let registration_barrier = Arc::new(tokio::sync::Barrier::new(2));
        let registration_task = {
            let barrier = registration_barrier.clone();
            let runtime = runtime.clone();
            let executor = executor.clone();
            tokio::spawn(async move {
                barrier.wait().await;
                runtime.register_library(executor)
            })
        };
        let wake_task = {
            let barrier = registration_barrier.clone();
            let handle = handle.clone();
            tokio::spawn(async move {
                barrier.wait().await;
                handle.wake_library_status(library_id, SyncRuntimeWakeReason::LocalChange)
            })
        };
        let (registration, wake) = tokio::join!(registration_task, wake_task);
        assert_eq!(
            registration.expect("registration task"),
            Ok(SyncRuntimeRegistration::Registered)
        );
        assert!(matches!(
            wake.expect("wake task"),
            SyncRuntimeWakeResult::UnknownLibrary
                | SyncRuntimeWakeResult::Queued
                | SyncRuntimeWakeResult::Coalesced
                | SyncRuntimeWakeResult::AlreadyRunningFollowupRecorded
        ));
        assert!(handle.status(library_id).is_some());

        tokio::time::timeout(Duration::from_secs(1), first_cycle_started)
            .await
            .expect("registered library must start after the registration/wake race");
        settle().await;
        let calls_after_registration_race = executor.calls();
        assert!(
            (1..=2).contains(&calls_after_registration_race),
            "registration/wake race may schedule at most one bounded follow-up, got {calls_after_registration_race} calls"
        );

        let unregistration_barrier = Arc::new(tokio::sync::Barrier::new(2));
        let unregistration_task = {
            let barrier = unregistration_barrier.clone();
            let runtime = runtime.clone();
            tokio::spawn(async move {
                barrier.wait().await;
                runtime.unregister_library(library_id)
            })
        };
        let wake_task = {
            let barrier = unregistration_barrier.clone();
            let handle = handle.clone();
            tokio::spawn(async move {
                barrier.wait().await;
                handle.wake_library_status(library_id, SyncRuntimeWakeReason::Manual)
            })
        };
        let (unregistration, wake) = tokio::join!(unregistration_task, wake_task);
        assert_eq!(
            unregistration
                .expect("unregistration task")
                .expect("unregister"),
            SyncRuntimeUnregistration::Unregistered
        );
        assert!(matches!(
            wake.expect("wake task"),
            SyncRuntimeWakeResult::UnknownLibrary
                | SyncRuntimeWakeResult::Queued
                | SyncRuntimeWakeResult::Coalesced
                | SyncRuntimeWakeResult::AlreadyRunningFollowupRecorded
        ));
        tokio::time::timeout(Duration::from_secs(1), async {
            while handle.status(library_id).is_some() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("unregistered in-flight cycle must be removed after completion");
        assert!(handle.status(library_id).is_none());

        let second_cycle_started = executor.started.notified();
        runtime
            .register_library(executor.clone())
            .expect("re-register after unregistration");
        tokio::time::timeout(Duration::from_secs(1), second_cycle_started)
            .await
            .expect("re-registered library must start after the unregistration race");
        settle().await;
        assert!((2..=3).contains(&executor.calls()));
        assert_eq!(executor.max_active(), 1);
        handle.shutdown().await.expect("race test shutdown");
    }

    #[tokio::test(start_paused = true)]
    async fn network_available_wakes_registered_libraries_without_creating_parallel_cycles() {
        let global_active = Arc::new(AtomicUsize::new(0));
        let global_max_active = Arc::new(AtomicUsize::new(0));
        let executors: Vec<_> = (0..5)
            .map(|_| {
                let executor = FakeExecutor::with_global_activity(
                    scope(),
                    vec![Ok(idle_result()), Ok(idle_result())],
                    global_active.clone(),
                    global_max_active.clone(),
                );
                executor.hold.store(true, Ordering::SeqCst);
                executor
            })
            .collect();
        let runtime = SyncRuntime::new(config());
        for executor in &executors {
            runtime
                .register_library(executor.clone())
                .expect("register network library");
        }
        let handle = runtime.start().expect("start network runtime");
        settle().await;
        let wakes = handle.network_available();
        assert_eq!(wakes.len(), 5);
        assert!(wakes.iter().all(|(_, result)| {
            matches!(
                result,
                SyncRuntimeWakeResult::AlreadyRunningFollowupRecorded
                    | SyncRuntimeWakeResult::Queued
                    | SyncRuntimeWakeResult::Coalesced
            )
        }));
        assert_eq!(global_max_active.load(Ordering::SeqCst), 2);
        for executor in &executors {
            executor.release();
        }
        settle().await;
        assert!(executors.iter().all(|executor| executor.max_active() <= 1));
        handle.shutdown().await.expect("network shutdown");
    }

    #[tokio::test(start_paused = true)]
    async fn wake_storm_coalesces_and_wake_during_cycle_is_retained() {
        let executor = FakeExecutor::new(scope(), vec![Ok(idle_result()), Ok(idle_result())]);
        let library_id = executor.scope.library_id();
        let runtime = SyncRuntime::new(config());
        runtime
            .register_library(executor.clone())
            .expect("register");
        let handle = runtime.start().expect("start");
        settle().await;
        for _ in 0..1_000 {
            handle
                .wake_library(library_id, SyncRuntimeWakeReason::LocalChange)
                .expect("wake");
        }
        settle().await;
        assert_eq!(executor.calls(), 2);

        executor.hold.store(true, Ordering::SeqCst);
        handle
            .wake_library(library_id, SyncRuntimeWakeReason::Manual)
            .expect("manual wake");
        tokio::time::advance(Duration::from_secs(10)).await;
        settle().await;
        assert_eq!(executor.calls(), 3);
        handle
            .wake_library(library_id, SyncRuntimeWakeReason::LocalChange)
            .expect("wake during cycle");
        executor.release();
        settle().await;
        tokio::time::advance(FAIR_FOLLOW_UP_DELAY).await;
        settle().await;
        assert_eq!(executor.calls(), 4);
        handle.shutdown().await.expect("shutdown");
    }

    #[tokio::test(start_paused = true)]
    async fn auth_block_suspends_periodic_poll_until_credential_wake() {
        let executor = FakeExecutor::new(
            scope(),
            vec![Ok(SyncCycleResult::new(
                Timestamp::parse("2026-09-12T00:00:00Z").unwrap(),
                InboundCycleOutcome::AuthRequired,
                OutboundCycleOutcome::NotAttempted(
                    OutboundSkipReason::InboundAuthenticationRequired,
                ),
            ))],
        );
        let library_id = executor.scope.library_id();
        let runtime = SyncRuntime::new(config());
        runtime
            .register_library(executor.clone())
            .expect("register");
        let handle = runtime.start().expect("start");
        settle().await;
        assert_eq!(executor.calls(), 1);
        tokio::time::advance(Duration::from_secs(60)).await;
        settle().await;
        assert_eq!(executor.calls(), 1);
        assert_eq!(
            handle.status(library_id).expect("status").phase(),
            SyncRuntimeLibraryPhase::AuthBlocked
        );
        handle
            .wake_library(library_id, SyncRuntimeWakeReason::CredentialChanged)
            .expect("credential wake");
        settle().await;
        assert_eq!(executor.calls(), 2);
        handle.shutdown().await.expect("shutdown");
    }

    #[tokio::test(start_paused = true)]
    async fn fairness_and_global_bound_hold_for_multiple_libraries() {
        let first = FakeExecutor::new(scope(), (0..20).map(|_| Ok(progress_result())).collect());
        // Give each fake a distinct library identity.
        let second = FakeExecutor::new(scope(), vec![Ok(idle_result())]);
        let third = FakeExecutor::new(scope(), vec![Ok(idle_result())]);
        let runtime = SyncRuntime::new(config());
        runtime
            .register_library(first.clone())
            .expect("register first");
        runtime
            .register_library(second.clone())
            .expect("register second");
        runtime
            .register_library(third.clone())
            .expect("register third");
        let handle = runtime.start().expect("start");
        settle().await;
        tokio::time::advance(FAIR_FOLLOW_UP_DELAY).await;
        settle().await;
        assert_eq!(second.calls(), 1);
        assert_eq!(third.calls(), 1);
        assert!(first.calls() <= 2, "large library must yield to peers");
        assert_eq!(first.max_active(), 1);
        assert_eq!(second.max_active(), 1);
        assert_eq!(third.max_active(), 1);
        handle.shutdown().await.expect("shutdown");
    }

    #[tokio::test(start_paused = true)]
    async fn five_runnable_libraries_observe_global_two_library_bound() {
        let global_active = Arc::new(AtomicUsize::new(0));
        let global_max_active = Arc::new(AtomicUsize::new(0));
        let executors: Vec<_> = (0..5)
            .map(|_| {
                FakeExecutor::with_global_activity(
                    scope(),
                    vec![Ok(idle_result())],
                    global_active.clone(),
                    global_max_active.clone(),
                )
            })
            .collect();
        for executor in &executors {
            executor.hold.store(true, Ordering::SeqCst);
        }
        let runtime = SyncRuntime::new(config());
        for executor in &executors {
            runtime
                .register_library(executor.clone())
                .expect("register");
        }
        let handle = runtime.start().expect("start");
        settle().await;
        assert_eq!(
            executors
                .iter()
                .map(|executor| executor.calls())
                .sum::<usize>(),
            2
        );
        assert_eq!(global_max_active.load(Ordering::SeqCst), 2);

        for executor in &executors {
            executor.release();
        }
        for _ in 0..20 {
            settle().await;
        }
        assert!(executors.iter().all(|executor| executor.calls() == 1));
        assert_eq!(global_active.load(Ordering::SeqCst), 0);
        assert!(executors.iter().all(|executor| executor.max_active() <= 1));
        handle.shutdown().await.expect("shutdown");
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_waits_for_in_flight_cycle_and_is_idempotent() {
        let executor = FakeExecutor::new(scope(), vec![Ok(idle_result())]);
        executor.hold.store(true, Ordering::SeqCst);
        let runtime = SyncRuntime::new(config());
        runtime
            .register_library(executor.clone())
            .expect("register");
        let handle = runtime.start().expect("start");
        settle().await;
        assert_eq!(executor.calls(), 1);
        let shutdown_finished = Arc::new(AtomicBool::new(false));
        let shutdown_finished_task = shutdown_finished.clone();
        let shutdown_handle = handle.clone();
        let shutdown = tokio::spawn(async move {
            shutdown_handle.shutdown().await.expect("shutdown");
            shutdown_finished_task.store(true, Ordering::SeqCst);
        });
        settle().await;
        assert!(!shutdown_finished.load(Ordering::SeqCst));
        executor.release();
        shutdown.await.expect("shutdown task");
        handle.shutdown().await.expect("repeated shutdown");
        assert!(runtime.start().is_ok());
        runtime.shutdown().await.expect("restart shutdown");
    }
}
