//! Process-owned desktop synchronization composition.
//!
//! [`DesktopSyncHost`] is the application composition root for the existing
//! Prompt 91--93 stack. It owns one [`SyncRuntime`] for its configured
//! process context, constructs Prompt 91 runners from the existing lower-level
//! engines, attaches the durable-change-first producers, and owns the lifetime
//! of local filesystem observers. It deliberately contains no synchronization
//! correctness policy of its own.

use std::{
    collections::BTreeMap,
    fmt,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

#[cfg(feature = "test-support")]
use std::sync::atomic::AtomicUsize;

use async_trait::async_trait;
use synveil_core::{DeviceCredentialId, DeviceId, LibraryId, Timestamp, UserId};
use synveil_platform::{
    PlatformRuntime, SecretName, SecretStore, SecretStoreError, SecretStoreState, SecretValue,
};
use tokio::{
    sync::{Mutex as AsyncMutex, Notify},
    task::JoinHandle,
};

use crate::{
    BidirectionalSyncCycleRunner, ClientSyncError, CredentialLifecycleController, EngineConfig,
    HttpClientConfig, HttpSyncRemote, InboundCycleOutcome, InboundSyncEngine, LocalChangeWatcher,
    LocalReplica, LocalStateConfig, LocalStateStore, ObservationConfig, OutboundCycleOutcome,
    OutboundIntentProducer, OutboundObservationEngine, OutboundSkipReason,
    RebaselineConvergenceCoordinator, RebaselineSnapshotRemote, ReplicaScope, SyncCycleExecutor,
    SyncCycleResult, SyncRemote, SyncRuntime, SyncRuntimeConfig, SyncRuntimeError,
    SyncRuntimeEvent, SyncRuntimeHandle, SyncRuntimeIdentity, SyncRuntimeLibraryStatus,
    SyncRuntimeRegistration, SyncRuntimeUnregistration, SyncRuntimeWakeResult, SyncWakeNotifier,
};

/// Default interval at which a host drains each native/manual observer queue.
/// The filesystem watcher itself is only a hint source; reconciliation remains
/// bounded and durable in [`OutboundObservationEngine`].
pub const DEFAULT_DESKTOP_SYNC_OBSERVATION_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Default bounded probe interval for roots that are unavailable or are being
/// reattached. The probe checks only the root binding; it never recursively
/// scans the tree.
pub const DEFAULT_DESKTOP_SYNC_ROOT_PROBE_INTERVAL: Duration = Duration::from_secs(1);

/// Readiness gate name for the completed desktop-host composition contract.
/// The value is intentionally a string so release/checkpoint automation can
/// record the gate without coupling to a process-global mutable flag.
pub const DESKTOP_SYNC_HOST_READINESS: &str = "SYNVEIL_DESKTOP_SYNC_HOST_READY";

const MIN_DESKTOP_SYNC_OBSERVATION_POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_DESKTOP_SYNC_OBSERVATION_POLL_INTERVAL: Duration = Duration::from_secs(60 * 60);

#[cfg(feature = "test-support")]
struct RootRecoveryGate {
    reached: AtomicBool,
    released: AtomicBool,
    entries: AtomicUsize,
    reached_notify: Notify,
    release_notify: Notify,
}

#[cfg(feature = "test-support")]
impl RootRecoveryGate {
    fn new() -> Self {
        Self {
            reached: AtomicBool::new(false),
            released: AtomicBool::new(false),
            entries: AtomicUsize::new(0),
            reached_notify: Notify::new(),
            release_notify: Notify::new(),
        }
    }

    async fn hold_until_released(&self) {
        self.entries.fetch_add(1, Ordering::AcqRel);
        self.reached.store(true, Ordering::Release);
        self.reached_notify.notify_waiters();
        loop {
            if self.released.load(Ordering::Acquire) {
                return;
            }
            let notified = self.release_notify.notified();
            if self.released.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }

    async fn wait_until_reached(&self) {
        loop {
            if self.reached.load(Ordering::Acquire) {
                return;
            }
            let notified = self.reached_notify.notified();
            if self.reached.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }
}

/// Test-only synchronization for holding a real root recovery transition
/// after the host has entered `Recovering` and before it publishes `Available`.
/// This type is available only with the `test-support` feature and has no
/// effect on production hosts unless an embedding explicitly installs it.
#[cfg(feature = "test-support")]
pub struct DesktopRootRecoveryGate {
    inner: Arc<RootRecoveryGate>,
}

#[cfg(feature = "test-support")]
impl DesktopRootRecoveryGate {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RootRecoveryGate::new()),
        }
    }

    /// Wait until the host has entered the canonical recovery boundary.
    pub async fn wait_until_recovering(&self) {
        self.inner.wait_until_reached().await;
    }

    /// Release the held recovery boundary so the host can publish `Available`.
    pub fn release(&self) {
        self.inner.released.store(true, Ordering::Release);
        self.inner.release_notify.notify_waiters();
    }

    /// Return the number of real recovery transitions that reached the gate.
    #[must_use]
    pub fn recovery_entries(&self) -> usize {
        self.inner.entries.load(Ordering::Acquire)
    }
}

#[cfg(feature = "test-support")]
impl Default for DesktopRootRecoveryGate {
    fn default() -> Self {
        Self::new()
    }
}

/// Validation failures for [`DesktopSyncHostConfig`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopSyncHostConfigError {
    ObservationPollIntervalTooShort,
    ObservationPollIntervalTooLong,
    RootProbeIntervalTooShort,
    RootProbeIntervalTooLong,
    RebaselinePageLimitInvalid,
}

impl fmt::Display for DesktopSyncHostConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ObservationPollIntervalTooShort => {
                "desktop sync observer poll interval is below the minimum"
            }
            Self::ObservationPollIntervalTooLong => {
                "desktop sync observer poll interval exceeds the maximum"
            }
            Self::RootProbeIntervalTooShort => {
                "desktop sync root probe interval is below the minimum"
            }
            Self::RootProbeIntervalTooLong => {
                "desktop sync root probe interval exceeds the maximum"
            }
            Self::RebaselinePageLimitInvalid => "desktop sync rebaseline page limit is invalid",
        })
    }
}

impl std::error::Error for DesktopSyncHostConfigError {}

/// Non-secret application-level settings for one desktop synchronization host.
///
/// The lower-level engines remain the owners of their own correctness bounds.
/// These settings only tell the composition root which already-validated
/// policies to pass into them and how often to drain observer hints.
#[derive(Clone, Copy, Debug)]
pub struct DesktopSyncHostConfig {
    runtime: SyncRuntimeConfig,
    engine: EngineConfig,
    http: HttpClientConfig,
    rebaseline_page_limit: u32,
    observation_poll_interval: Duration,
    root_probe_interval: Duration,
}

impl DesktopSyncHostConfig {
    #[must_use]
    pub fn new(runtime: SyncRuntimeConfig) -> Self {
        Self {
            runtime,
            ..Self::default()
        }
    }

    #[must_use]
    pub const fn runtime(self) -> SyncRuntimeConfig {
        self.runtime
    }

    #[must_use]
    pub const fn engine(self) -> EngineConfig {
        self.engine
    }

    #[must_use]
    pub const fn http(self) -> HttpClientConfig {
        self.http
    }

    #[must_use]
    pub const fn rebaseline_page_limit(self) -> u32 {
        self.rebaseline_page_limit
    }

    #[must_use]
    pub const fn observation_poll_interval(self) -> Duration {
        self.observation_poll_interval
    }

    #[must_use]
    pub const fn root_probe_interval(self) -> Duration {
        self.root_probe_interval
    }

    #[must_use]
    pub const fn with_engine_config(mut self, engine: EngineConfig) -> Self {
        self.engine = engine;
        self
    }

    #[must_use]
    pub const fn with_http_config(mut self, http: HttpClientConfig) -> Self {
        self.http = http;
        self
    }

    #[must_use]
    pub const fn with_rebaseline_page_limit(mut self, page_limit: u32) -> Self {
        self.rebaseline_page_limit = page_limit;
        self
    }

    #[must_use]
    pub const fn with_observation_poll_interval(mut self, interval: Duration) -> Self {
        self.observation_poll_interval = interval;
        self
    }

    #[must_use]
    pub const fn with_root_probe_interval(mut self, interval: Duration) -> Self {
        self.root_probe_interval = interval;
        self
    }

    fn validate(self) -> Result<Self, DesktopSyncHostConfigError> {
        if self.observation_poll_interval < MIN_DESKTOP_SYNC_OBSERVATION_POLL_INTERVAL {
            return Err(DesktopSyncHostConfigError::ObservationPollIntervalTooShort);
        }
        if self.observation_poll_interval > MAX_DESKTOP_SYNC_OBSERVATION_POLL_INTERVAL {
            return Err(DesktopSyncHostConfigError::ObservationPollIntervalTooLong);
        }
        if self.root_probe_interval < MIN_DESKTOP_SYNC_OBSERVATION_POLL_INTERVAL {
            return Err(DesktopSyncHostConfigError::RootProbeIntervalTooShort);
        }
        if self.root_probe_interval > MAX_DESKTOP_SYNC_OBSERVATION_POLL_INTERVAL {
            return Err(DesktopSyncHostConfigError::RootProbeIntervalTooLong);
        }
        if !(1..=crate::MAX_PAGE_ITEMS as u32).contains(&self.rebaseline_page_limit) {
            return Err(DesktopSyncHostConfigError::RebaselinePageLimitInvalid);
        }
        Ok(self)
    }
}

impl Default for DesktopSyncHostConfig {
    fn default() -> Self {
        Self {
            runtime: SyncRuntimeConfig::default(),
            engine: EngineConfig::default(),
            http: HttpClientConfig::default(),
            rebaseline_page_limit: 256,
            observation_poll_interval: DEFAULT_DESKTOP_SYNC_OBSERVATION_POLL_INTERVAL,
            root_probe_interval: DEFAULT_DESKTOP_SYNC_ROOT_PROBE_INTERVAL,
        }
    }
}

/// A transport that can supply both the ordinary sync port and the
/// rebaseline snapshot port required by Prompt 87. `HttpSyncRemote` already
/// implements this contract; test/in-process transports can implement it
/// without exposing any host-specific API.
pub trait DesktopSyncRemote: SyncRemote + RebaselineSnapshotRemote {}

impl<T> DesktopSyncRemote for T where T: SyncRemote + RebaselineSnapshotRemote {}

/// The source from which the host obtains a library's Prompt 91 executor.
///
/// `Http` is the production composition path. It loads credentials lazily
/// from the existing profile-bound secure-store lifecycle, so a missing
/// credential does not prevent host construction. `Remote` is the
/// transport-neutral injection path used by deterministic tests and
/// in-process embedders. `Executor` is the narrowest path for callers that
/// already own a reviewed Prompt 91 executor.
pub enum DesktopSyncLibrarySource {
    Executor(Arc<dyn SyncCycleExecutor>),
    Remote(Arc<dyn DesktopSyncRemote>),
    Http { profile: crate::ServerProfile },
}

impl fmt::Debug for DesktopSyncLibrarySource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Executor(_) => "executor",
            Self::Remote(_) => "remote",
            Self::Http { .. } => "http",
        };
        formatter
            .debug_tuple("DesktopSyncLibrarySource")
            .field(&name)
            .finish()
    }
}

/// Declarative configuration for one library owned by a host.
///
/// The library ID is taken from [`ReplicaScope`] and is the sole key used for
/// runtime registration. A watcher is optional because an embedder may use a
/// host for inbound/manual synchronization only, or may attach an observer
/// through a platform-specific control surface later.
pub struct DesktopSyncLibraryConfig {
    scope: ReplicaScope,
    replica: Arc<dyn LocalReplica>,
    source: DesktopSyncLibrarySource,
    watcher: Option<Box<dyn LocalChangeWatcher>>,
    observation_config: ObservationConfig,
}

impl fmt::Debug for DesktopSyncLibraryConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopSyncLibraryConfig")
            .field("scope", &self.scope)
            .field("source", &self.source)
            .field("has_watcher", &self.watcher.is_some())
            .field("observation_config", &self.observation_config)
            .finish()
    }
}

impl DesktopSyncLibraryConfig {
    /// Configure a library from an already-composed Prompt 91 executor.
    pub fn from_executor(
        scope: ReplicaScope,
        executor: Arc<dyn SyncCycleExecutor>,
        replica: Arc<dyn LocalReplica>,
    ) -> Result<Self, ClientSyncError> {
        if executor.scope() != scope || replica.scope() != scope {
            return Err(ClientSyncError::WrongScope);
        }
        Ok(Self {
            scope,
            replica,
            source: DesktopSyncLibrarySource::Executor(executor),
            watcher: None,
            observation_config: ObservationConfig::default(),
        })
    }

    /// Configure a library from an in-process transport. The host constructs
    /// the inbound engine, convergence coordinator, outbound engine, and
    /// bidirectional cycle around this transport.
    pub fn from_remote(
        scope: ReplicaScope,
        remote: Arc<dyn DesktopSyncRemote>,
        replica: Arc<dyn LocalReplica>,
    ) -> Result<Self, ClientSyncError> {
        if replica.scope() != scope {
            return Err(ClientSyncError::WrongScope);
        }
        Ok(Self {
            scope,
            replica,
            source: DesktopSyncLibrarySource::Remote(remote),
            watcher: None,
            observation_config: ObservationConfig::default(),
        })
    }

    /// Configure a production profile-bound HTTP library. Credential loading
    /// and Prompt 91 construction occur at the first runtime cycle, not here.
    #[must_use]
    pub fn http(
        scope: ReplicaScope,
        profile: crate::ServerProfile,
        replica: Arc<dyn LocalReplica>,
    ) -> Self {
        Self {
            scope,
            replica,
            source: DesktopSyncLibrarySource::Http { profile },
            watcher: None,
            observation_config: ObservationConfig::default(),
        }
    }

    #[must_use]
    pub const fn scope(&self) -> ReplicaScope {
        self.scope
    }

    #[must_use]
    pub const fn library_id(&self) -> LibraryId {
        self.scope.library_id()
    }

    #[must_use]
    pub fn has_watcher(&self) -> bool {
        self.watcher.is_some()
    }

    /// Attach the already-selected watcher implementation. The host starts
    /// it only after all library registrations have been accepted by the
    /// single runtime.
    #[must_use]
    pub fn with_watcher(
        mut self,
        watcher: Box<dyn LocalChangeWatcher>,
        observation_config: ObservationConfig,
    ) -> Self {
        self.watcher = Some(watcher);
        self.observation_config = observation_config;
        self
    }

    /// Select the native `notify` watcher. `notify` chooses inotify on Linux
    /// and ReadDirectoryChangesW on Windows; the host does not add OS sync
    /// policy around either implementation.
    #[must_use]
    pub fn with_native_watcher(self) -> Self {
        let config = self.observation_config;
        self.with_watcher(
            Box::new(crate::NotifyLocalChangeWatcher::new(
                config.raw_queue_capacity(),
            )),
            config,
        )
    }

    #[must_use]
    pub fn without_watcher(mut self) -> Self {
        self.watcher = None;
        self
    }
}

/// Idempotent result from host-level library registration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopSyncLibraryRegistration {
    Registered,
    AlreadyRegistered,
}

/// Host lifecycle states are ephemeral and are never persisted in SQLite.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DesktopSyncHostLifecycle {
    Constructed,
    Starting,
    Running,
    Stopping,
    Stopped,
    Faulted,
}

impl DesktopSyncHostLifecycle {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Constructed => "CONSTRUCTED",
            Self::Starting => "STARTING",
            Self::Running => "RUNNING",
            Self::Stopping => "STOPPING",
            Self::Stopped => "STOPPED",
            Self::Faulted => "FAULTED",
        }
    }
}

impl fmt::Display for DesktopSyncHostLifecycle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Process-ephemeral availability of one configured filesystem root.
///
/// This state is deliberately not persisted and is independent from the
/// durable Prompt 91--94 synchronization state. `Unavailable` means that no
/// local observation/reconciliation is allowed to attribute absence to the
/// user. `Recovering` is the short transition in which the canonical watcher
/// and bounded rescan are being re-established.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RootAvailability {
    Available,
    Unavailable,
    Recovering,
}

impl RootAvailability {
    const fn code(self) -> u8 {
        match self {
            Self::Available => 0,
            Self::Unavailable => 1,
            Self::Recovering => 2,
        }
    }

    const fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Available,
            2 => Self::Recovering,
            _ => Self::Unavailable,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "AVAILABLE",
            Self::Unavailable => "UNAVAILABLE",
            Self::Recovering => "RECOVERING",
        }
    }
}

impl fmt::Display for RootAvailability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Compatibility spelling for callers that want the desktop qualifier in
/// status schemas.
pub type DesktopRootAvailability = RootAvailability;

/// Safe host control-plane failures. None contains a path, credential, token,
/// cookie, file content, or remote diagnostic string.
#[derive(Debug)]
pub enum DesktopSyncHostError {
    AlreadyStarted,
    StartCancelled,
    Stopping,
    Stopped,
    NotRunning,
    NotStopped,
    Faulted,
    LibraryNotRegistered,
    InvalidState,
    Configuration(DesktopSyncHostConfigError),
    Client(ClientSyncError),
    Runtime(SyncRuntimeError),
    ObserverTaskPanicked,
}

impl fmt::Display for DesktopSyncHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::AlreadyStarted => "DESKTOP_SYNC_HOST_ALREADY_STARTED",
            Self::StartCancelled => "DESKTOP_SYNC_HOST_START_CANCELLED",
            Self::Stopping => "DESKTOP_SYNC_HOST_STOPPING",
            Self::Stopped => "DESKTOP_SYNC_HOST_STOPPED",
            Self::NotRunning => "DESKTOP_SYNC_HOST_NOT_RUNNING",
            Self::NotStopped => "DESKTOP_SYNC_HOST_NOT_STOPPED",
            Self::Faulted => "DESKTOP_SYNC_HOST_FAULTED",
            Self::LibraryNotRegistered => "DESKTOP_SYNC_LIBRARY_NOT_REGISTERED",
            Self::InvalidState => "DESKTOP_SYNC_HOST_STATE_INVALID",
            Self::Configuration(error) => match error {
                DesktopSyncHostConfigError::ObservationPollIntervalTooShort => {
                    "DESKTOP_SYNC_HOST_OBSERVER_INTERVAL_TOO_SHORT"
                }
                DesktopSyncHostConfigError::ObservationPollIntervalTooLong => {
                    "DESKTOP_SYNC_HOST_OBSERVER_INTERVAL_TOO_LONG"
                }
                DesktopSyncHostConfigError::RootProbeIntervalTooShort => {
                    "DESKTOP_SYNC_HOST_ROOT_PROBE_INTERVAL_TOO_SHORT"
                }
                DesktopSyncHostConfigError::RootProbeIntervalTooLong => {
                    "DESKTOP_SYNC_HOST_ROOT_PROBE_INTERVAL_TOO_LONG"
                }
                DesktopSyncHostConfigError::RebaselinePageLimitInvalid => {
                    "DESKTOP_SYNC_HOST_REBASELINE_PAGE_LIMIT_INVALID"
                }
            },
            Self::Client(error) => error.code(),
            Self::Runtime(error) => match error {
                SyncRuntimeError::AlreadyRunning => "SYNC_RUNTIME_ALREADY_RUNNING",
                SyncRuntimeError::Stopping => "SYNC_RUNTIME_STOPPING",
                SyncRuntimeError::NotRunning => "SYNC_RUNTIME_NOT_RUNNING",
                SyncRuntimeError::NotRegistered => "SYNC_RUNTIME_NOT_REGISTERED",
                SyncRuntimeError::TooManyLibraries => "SYNC_RUNTIME_LIBRARY_LIMIT",
                SyncRuntimeError::TokioRuntimeUnavailable => "SYNC_RUNTIME_TOKIO_UNAVAILABLE",
                SyncRuntimeError::TaskPanicked => "SYNC_RUNTIME_TASK_PANICKED",
            },
            Self::ObserverTaskPanicked => "DESKTOP_SYNC_OBSERVER_TASK_PANICKED",
        })
    }
}

impl std::error::Error for DesktopSyncHostError {}

impl From<ClientSyncError> for DesktopSyncHostError {
    fn from(error: ClientSyncError) -> Self {
        Self::Client(error)
    }
}

impl From<SyncRuntimeError> for DesktopSyncHostError {
    fn from(error: SyncRuntimeError) -> Self {
        Self::Runtime(error)
    }
}

impl From<DesktopSyncHostConfigError> for DesktopSyncHostError {
    fn from(error: DesktopSyncHostConfigError) -> Self {
        Self::Configuration(error)
    }
}

/// A lifecycle event supplied by a process/platform adapter. There is no OS
/// monitor in this crate: Linux and Windows adapters deliver the same narrow
/// event to the host when their embedding process receives its shutdown hook.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopLifecycleEvent {
    ShutdownRequested,
}

struct ObserverCancellation {
    cancelled: AtomicBool,
    notify: Notify,
}

impl ObserverCancellation {
    fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            notify: Notify::new(),
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

struct HostLibrary {
    scope: ReplicaScope,
    replica: Arc<dyn LocalReplica>,
    cycle: Arc<dyn SyncCycleExecutor>,
    observer: Option<Arc<OutboundObservationEngine>>,
    observer_cancellation: Arc<ObserverCancellation>,
    root_availability: AtomicU8,
}

impl HostLibrary {
    fn new(
        scope: ReplicaScope,
        replica: Arc<dyn LocalReplica>,
        cycle: Arc<dyn SyncCycleExecutor>,
        observer: Option<Arc<OutboundObservationEngine>>,
    ) -> Self {
        let root_availability = if replica.validate_root().is_ok() {
            RootAvailability::Available
        } else {
            RootAvailability::Unavailable
        };
        Self {
            scope,
            replica,
            cycle,
            observer,
            observer_cancellation: Arc::new(ObserverCancellation::new()),
            root_availability: AtomicU8::new(root_availability.code()),
        }
    }

    fn root_availability(&self) -> RootAvailability {
        RootAvailability::from_code(self.root_availability.load(Ordering::Acquire))
    }

    fn set_root_availability(&self, availability: RootAvailability) {
        self.root_availability
            .store(availability.code(), Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HostProcessScope {
    owner_user_id: UserId,
    device_id: DeviceId,
}

impl HostProcessScope {
    const fn from_replica_scope(scope: ReplicaScope) -> Self {
        Self {
            owner_user_id: scope.owner_user_id(),
            device_id: scope.device_id(),
        }
    }
}

struct HostInner {
    state: Arc<LocalStateStore>,
    secret_store: Arc<dyn SecretStore>,
    platform: Option<Arc<dyn PlatformRuntime>>,
    config: DesktopSyncHostConfig,
    runtime: Arc<SyncRuntime>,
    notifier: Arc<dyn SyncWakeNotifier>,
    lifecycle: StdMutex<DesktopSyncHostLifecycle>,
    transition_notify: Notify,
    operation_gate: AsyncMutex<()>,
    stop_requested: AtomicBool,
    cancellation: Arc<ObserverCancellation>,
    libraries: AsyncMutex<BTreeMap<LibraryId, Arc<HostLibrary>>>,
    process_scope: StdMutex<Option<HostProcessScope>>,
    observer_tasks: AsyncMutex<BTreeMap<LibraryId, JoinHandle<()>>>,
    runtime_handle: StdMutex<Option<SyncRuntimeHandle>>,
    #[cfg(feature = "test-support")]
    recovery_gate: Arc<StdMutex<Option<Arc<RootRecoveryGate>>>>,
    close_state_on_shutdown: bool,
}

impl HostInner {
    fn new(
        state: Arc<LocalStateStore>,
        secret_store: Arc<dyn SecretStore>,
        platform: Option<Arc<dyn PlatformRuntime>>,
        config: DesktopSyncHostConfig,
        close_state_on_shutdown: bool,
    ) -> Arc<Self> {
        let runtime = Arc::new(SyncRuntime::new(config.runtime()));
        let notifier: Arc<dyn SyncWakeNotifier> = runtime.clone();
        Arc::new(Self {
            state,
            secret_store,
            platform,
            config,
            runtime,
            notifier,
            lifecycle: StdMutex::new(DesktopSyncHostLifecycle::Constructed),
            transition_notify: Notify::new(),
            operation_gate: AsyncMutex::new(()),
            stop_requested: AtomicBool::new(false),
            cancellation: Arc::new(ObserverCancellation::new()),
            libraries: AsyncMutex::new(BTreeMap::new()),
            process_scope: StdMutex::new(None),
            observer_tasks: AsyncMutex::new(BTreeMap::new()),
            runtime_handle: StdMutex::new(None),
            #[cfg(feature = "test-support")]
            recovery_gate: Arc::new(StdMutex::new(None)),
            close_state_on_shutdown,
        })
    }

    fn lifecycle(&self) -> DesktopSyncHostLifecycle {
        *self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn set_lifecycle(&self, lifecycle: DesktopSyncHostLifecycle) {
        *self
            .lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = lifecycle;
        self.transition_notify.notify_waiters();
    }

    fn runtime_handle(&self) -> Option<SyncRuntimeHandle> {
        self.runtime_handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn process_scope_matches(&self, scope: ReplicaScope) -> bool {
        let expected = HostProcessScope::from_replica_scope(scope);
        self.process_scope
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_none_or(|actual| actual == expected)
    }

    fn claim_process_scope(&self, scope: ReplicaScope) {
        let mut process_scope = self
            .process_scope
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if process_scope.is_none() {
            *process_scope = Some(HostProcessScope::from_replica_scope(scope));
        }
    }

    async fn build_library(
        self: &Arc<Self>,
        config: DesktopSyncLibraryConfig,
    ) -> Result<Arc<HostLibrary>, DesktopSyncHostError> {
        let DesktopSyncLibraryConfig {
            scope,
            replica,
            source,
            watcher,
            observation_config,
        } = config;

        if replica.scope() != scope {
            return Err(ClientSyncError::WrongScope.into());
        }

        let cycle: Arc<dyn SyncCycleExecutor> = match source {
            DesktopSyncLibrarySource::Executor(executor) => {
                if executor.scope() != scope {
                    return Err(ClientSyncError::WrongScope.into());
                }
                executor
            }
            DesktopSyncLibrarySource::Remote(remote) => Arc::new(ReloadableRemoteCycle::new(
                scope,
                remote,
                Arc::clone(&replica),
                Arc::clone(&self.state),
                self.config,
            )),
            DesktopSyncLibrarySource::Http { profile } => {
                if replica.server_profile_id() != Some(profile.profile_id()) {
                    return Err(ClientSyncError::WrongServerProfile.into());
                }
                if let Some(enrollment) =
                    self.state.profile_enrollment(profile.profile_id()).await?
                    && (enrollment.owner_user_id() != scope.owner_user_id()
                        || enrollment.device_id() != scope.device_id())
                {
                    return Err(ClientSyncError::WrongScope.into());
                }
                self.state.save_server_profile(&profile).await?;
                self.state
                    .prepare_replica_for_profile(scope, replica.binding_id(), profile.profile_id())
                    .await?;
                Arc::new(ReloadableHttpCycle::new(
                    scope,
                    profile.profile_id(),
                    Arc::clone(&replica),
                    Arc::clone(&self.state),
                    Arc::clone(&self.secret_store),
                    self.config,
                ))
            }
        };
        let cycle = Arc::new(RootGatedCycle::new(scope, Arc::clone(&replica), cycle));

        let observer = match watcher {
            Some(watcher) => Some(Arc::new(
                OutboundObservationEngine::new_with_wake_notifier(
                    scope,
                    Arc::clone(&replica),
                    Arc::clone(&self.state),
                    watcher,
                    observation_config,
                    Arc::clone(&self.notifier),
                )
                .await?,
            )),
            None => None,
        };

        Ok(Arc::new(HostLibrary::new(scope, replica, cycle, observer)))
    }

    async fn register_library(
        self: &Arc<Self>,
        config: DesktopSyncLibraryConfig,
    ) -> Result<DesktopSyncLibraryRegistration, DesktopSyncHostError> {
        let _operation = self.operation_gate.lock().await;
        let lifecycle = self.lifecycle();
        if matches!(
            lifecycle,
            DesktopSyncHostLifecycle::Stopping
                | DesktopSyncHostLifecycle::Stopped
                | DesktopSyncHostLifecycle::Faulted
        ) {
            return Err(match lifecycle {
                DesktopSyncHostLifecycle::Stopping => DesktopSyncHostError::Stopping,
                DesktopSyncHostLifecycle::Stopped => DesktopSyncHostError::Stopped,
                DesktopSyncHostLifecycle::Faulted => DesktopSyncHostError::Faulted,
                _ => DesktopSyncHostError::InvalidState,
            });
        }
        if lifecycle == DesktopSyncHostLifecycle::Starting {
            return Err(DesktopSyncHostError::Stopping);
        }

        let library_id = config.library_id();
        let scope = config.scope();
        if !self.process_scope_matches(scope) {
            return Err(ClientSyncError::WrongScope.into());
        }
        if let Some(existing_scope) = self
            .libraries
            .lock()
            .await
            .get(&library_id)
            .map(|library| library.scope)
        {
            return if existing_scope == scope {
                Ok(DesktopSyncLibraryRegistration::AlreadyRegistered)
            } else {
                Err(ClientSyncError::WrongScope.into())
            };
        }

        let library = self.build_library(config).await?;
        let registration = self.runtime.register_executor(Arc::clone(&library.cycle))?;
        if registration == SyncRuntimeRegistration::AlreadyRegistered {
            return Err(DesktopSyncHostError::InvalidState);
        }

        if lifecycle == DesktopSyncHostLifecycle::Running {
            if self.stop_requested.load(Ordering::Acquire) {
                let _ = self.runtime.unregister_library(library_id);
                return Err(DesktopSyncHostError::Stopping);
            }
            if let Err(error) = self.start_library_lifecycle(&library).await {
                let _ = self.runtime.unregister_library(library_id);
                return Err(error);
            }
            self.spawn_library_lifecycle_task(Arc::clone(&library))
                .await;
        }

        self.claim_process_scope(scope);
        self.libraries.lock().await.insert(library_id, library);
        Ok(DesktopSyncLibraryRegistration::Registered)
    }

    async fn start_library_lifecycle(
        &self,
        library: &Arc<HostLibrary>,
    ) -> Result<(), DesktopSyncHostError> {
        if self.stop_requested.load(Ordering::Acquire) {
            return Err(DesktopSyncHostError::StartCancelled);
        }
        if library.replica.validate_root().is_err() {
            self.mark_root_unavailable(library).await;
            return Ok(());
        }

        library.set_root_availability(RootAvailability::Recovering);
        if let Some(observer) = &library.observer {
            match observer.start().await {
                Ok(_) => {}
                Err(error) if is_root_lifecycle_error(&error) => {
                    self.mark_root_unavailable(library).await;
                    return Ok(());
                }
                Err(error) => return Err(error.into()),
            }
        }
        library.set_root_availability(RootAvailability::Available);
        Ok(())
    }

    async fn mark_root_unavailable(&self, library: &HostLibrary) {
        mark_library_root_unavailable(library).await;
    }

    async fn spawn_library_lifecycle_task(&self, library: Arc<HostLibrary>) {
        let global_cancellation = Arc::clone(&self.cancellation);
        let local_cancellation = Arc::clone(&library.observer_cancellation);
        let root_probe_interval = self.config.root_probe_interval();
        let observation_poll_interval = self.config.observation_poll_interval();
        let has_observer = library.observer.is_some();
        let runtime = Arc::clone(&self.runtime);
        let library_id = library.scope.library_id();
        #[cfg(feature = "test-support")]
        let recovery_gate = Arc::clone(&self.recovery_gate);
        let task = tokio::spawn(async move {
            let mut root_timer = tokio::time::interval(root_probe_interval);
            let mut observation_timer = tokio::time::interval(observation_poll_interval);
            loop {
                if global_cancellation.is_cancelled() || local_cancellation.is_cancelled() {
                    break;
                }
                tokio::select! {
                    biased;
                    _ = global_cancellation.notify.notified() => {}
                    _ = local_cancellation.notify.notified() => {}
                    _ = root_timer.tick() => {
                        if global_cancellation.is_cancelled() || local_cancellation.is_cancelled() {
                            break;
                        }
                        if library.replica.validate_root().is_err() {
                            mark_library_root_unavailable(&library).await;
                            continue;
                        }

                        if library.root_availability() != RootAvailability::Available {
                            library.set_root_availability(RootAvailability::Recovering);
                            if let Some(observer) = &library.observer {
                                match observer.start().await {
                                    Ok(_) => {}
                                    Err(error) if is_root_lifecycle_error(&error) => {
                                        library.set_root_availability(RootAvailability::Unavailable);
                                        let _ = observer.shutdown().await;
                                        continue;
                                    }
                                    Err(error) => {
                                        tracing::warn!(
                                            library_id = %library.scope.library_id(),
                                            error = %error,
                                            "desktop sync observer recovery is deferred"
                                        );
                                        continue;
                                    }
                                }
                            }
                            #[cfg(feature = "test-support")]
                            let recovery_gate = recovery_gate
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner())
                                .clone();
                            #[cfg(feature = "test-support")]
                            if let Some(gate) = recovery_gate {
                                gate.hold_until_released().await;
                            }
                            library.set_root_availability(RootAvailability::Available);
                            let _ = runtime.wake_library_status(
                                library_id,
                                crate::SyncRuntimeWakeReason::RootAvailable,
                            );
                        }
                    }
                    _ = observation_timer.tick(), if has_observer => {
                        if global_cancellation.is_cancelled() || local_cancellation.is_cancelled() {
                            break;
                        }
                        if library.root_availability() != RootAvailability::Available {
                            continue;
                        }

                        if let Some(observer) = &library.observer
                            && let Err(error) = observer.poll_once_with_notification().await
                        {
                            if is_root_lifecycle_error(&error) {
                                mark_library_root_unavailable(&library).await;
                            } else {
                                tracing::warn!(
                                    library_id = %library_id,
                                    error = %error,
                                    "desktop sync observer poll failed"
                                );
                            }
                        }
                    }
                }
            }
        });
        self.observer_tasks.lock().await.insert(library_id, task);
    }

    async fn start_components(&self) -> Result<SyncRuntimeHandle, DesktopSyncHostError> {
        if self.stop_requested.load(Ordering::Acquire) {
            return Err(DesktopSyncHostError::StartCancelled);
        }
        let handle = self.runtime.start()?;
        *self
            .runtime_handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(handle.clone());

        if self.stop_requested.load(Ordering::Acquire) {
            self.stop_components().await?;
            return Err(DesktopSyncHostError::StartCancelled);
        }

        let libraries: Vec<_> = self.libraries.lock().await.values().cloned().collect();
        for library in &libraries {
            if self.stop_requested.load(Ordering::Acquire) {
                self.stop_components().await?;
                return Err(DesktopSyncHostError::StartCancelled);
            }
            if let Err(error) = self.start_library_lifecycle(library).await {
                self.stop_components().await?;
                return Err(error);
            }
        }

        if self.stop_requested.load(Ordering::Acquire) {
            self.stop_components().await?;
            return Err(DesktopSyncHostError::StartCancelled);
        }
        for library in libraries {
            self.spawn_library_lifecycle_task(library).await;
        }
        Ok(handle)
    }

    async fn stop_components(&self) -> Result<(), DesktopSyncHostError> {
        self.cancellation.cancel();
        for library in self.libraries.lock().await.values() {
            library.observer_cancellation.cancel();
        }

        let tasks: Vec<_> = {
            let mut observer_tasks = self.observer_tasks.lock().await;
            std::mem::take(&mut *observer_tasks).into_values().collect()
        };
        let mut task_error = None;
        for task in tasks {
            if task.await.is_err() && task_error.is_none() {
                task_error = Some(DesktopSyncHostError::ObserverTaskPanicked);
            }
        }

        let libraries: Vec<_> = self.libraries.lock().await.values().cloned().collect();
        let mut observer_error = None;
        for library in libraries {
            if let Some(observer) = &library.observer
                && let Err(error) = observer.shutdown().await
                && observer_error.is_none()
            {
                observer_error = Some(DesktopSyncHostError::Client(error));
            }
        }

        let runtime_error = self
            .runtime
            .shutdown()
            .await
            .err()
            .map(DesktopSyncHostError::Runtime);
        *self
            .runtime_handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;

        if self.close_state_on_shutdown {
            self.state.close_pool().await;
        }

        task_error
            .or(observer_error)
            .or(runtime_error)
            .map_or(Ok(()), Err)
    }

    async fn unregister_library(
        &self,
        library_id: LibraryId,
    ) -> Result<SyncRuntimeUnregistration, DesktopSyncHostError> {
        let _operation = self.operation_gate.lock().await;
        let lifecycle = self.lifecycle();
        if matches!(
            lifecycle,
            DesktopSyncHostLifecycle::Starting
                | DesktopSyncHostLifecycle::Stopping
                | DesktopSyncHostLifecycle::Stopped
                | DesktopSyncHostLifecycle::Faulted
        ) {
            return Err(match lifecycle {
                DesktopSyncHostLifecycle::Starting => DesktopSyncHostError::Stopping,
                DesktopSyncHostLifecycle::Stopping => DesktopSyncHostError::Stopping,
                DesktopSyncHostLifecycle::Stopped => DesktopSyncHostError::Stopped,
                DesktopSyncHostLifecycle::Faulted => DesktopSyncHostError::Faulted,
                _ => DesktopSyncHostError::InvalidState,
            });
        }
        let Some(library) = self.libraries.lock().await.remove(&library_id) else {
            return Ok(SyncRuntimeUnregistration::AlreadyUnregistered);
        };
        library.observer_cancellation.cancel();
        if let Some(task) = self.observer_tasks.lock().await.remove(&library_id)
            && task.await.is_err()
        {
            return Err(DesktopSyncHostError::ObserverTaskPanicked);
        }
        if let Some(observer) = &library.observer {
            observer.shutdown().await?;
        }
        Ok(self.runtime.unregister_library(library_id)?)
    }
}

impl Drop for HostInner {
    fn drop(&mut self) {
        // Async graceful shutdown is the canonical application path. This is
        // only best-effort cancellation for an abandoned host/control handle;
        // it never aborts an active Prompt 91 future or deletes durable state.
        self.cancellation.cancel();
        let _ = self.runtime.request_shutdown();
    }
}

/// A process-owned synchronization host. Cloneable values are handles to the
/// same host; cloning never creates a runtime, observer, or worker.
#[derive(Clone)]
pub struct DesktopSyncHost {
    inner: Arc<HostInner>,
}

impl fmt::Debug for DesktopSyncHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopSyncHost")
            .field("lifecycle", &self.lifecycle())
            .field("runtime_identity", &self.runtime_identity())
            .finish_non_exhaustive()
    }
}

impl DesktopSyncHost {
    /// Construct a host around an already-open local state store. Construction
    /// performs no background spawn and starts no filesystem watcher.
    pub async fn new(
        state: Arc<LocalStateStore>,
        secret_store: Arc<dyn SecretStore>,
        config: DesktopSyncHostConfig,
        libraries: impl IntoIterator<Item = DesktopSyncLibraryConfig>,
    ) -> Result<Self, DesktopSyncHostError> {
        let config = config.validate()?;
        let inner = HostInner::new(state, secret_store, None, config, false);
        let host = Self { inner };
        for library in libraries {
            if let Err(error) = host.register_library(library).await {
                let _ = host.shutdown().await;
                return Err(error);
            }
        }
        Ok(host)
    }

    /// Open the canonical local SQLite store and transfer its pool ownership
    /// to the host. The pool is closed after observer/runtime shutdown.
    pub async fn open(
        state_config: LocalStateConfig,
        secret_store: Arc<dyn SecretStore>,
        config: DesktopSyncHostConfig,
        libraries: impl IntoIterator<Item = DesktopSyncLibraryConfig>,
    ) -> Result<Self, DesktopSyncHostError> {
        let config = config.validate()?;
        let state = Arc::new(LocalStateStore::open(&state_config).await?);
        let inner = HostInner::new(state, secret_store, None, config, true);
        let host = Self { inner };
        for library in libraries {
            if let Err(error) = host.register_library(library).await {
                let _ = host.shutdown().await;
                return Err(error);
            }
        }
        Ok(host)
    }

    /// Platform-oriented composition helper. The platform runtime remains
    /// owned by the host so its secure credential provider has the same
    /// lifetime as the process host. No platform service manager is invoked.
    pub async fn from_platform(
        platform: Arc<dyn PlatformRuntime>,
        config: DesktopSyncHostConfig,
        libraries: impl IntoIterator<Item = DesktopSyncLibraryConfig>,
    ) -> Result<Self, DesktopSyncHostError> {
        let config = config.validate()?;
        let state_config = LocalStateConfig::from_platform(platform.as_ref())?;
        let secret_store: Arc<dyn SecretStore> = Arc::new(PlatformSecretStore {
            platform: Arc::clone(&platform),
        });
        let state = Arc::new(LocalStateStore::open(&state_config).await?);
        Self::from_platform_with_state_inner(platform, state, secret_store, config, libraries).await
    }

    /// Compose a host around a state store that the process bootstrap has
    /// already opened. The host takes ownership of the pool's shutdown and
    /// closes it after all observer/runtime tasks have joined.
    pub async fn from_platform_with_state(
        platform: Arc<dyn PlatformRuntime>,
        state: Arc<LocalStateStore>,
        config: DesktopSyncHostConfig,
        libraries: impl IntoIterator<Item = DesktopSyncLibraryConfig>,
    ) -> Result<Self, DesktopSyncHostError> {
        let config = config.validate()?;
        let secret_store: Arc<dyn SecretStore> = Arc::new(PlatformSecretStore {
            platform: Arc::clone(&platform),
        });
        Self::from_platform_with_state_inner(platform, state, secret_store, config, libraries).await
    }

    async fn from_platform_with_state_inner(
        platform: Arc<dyn PlatformRuntime>,
        state: Arc<LocalStateStore>,
        secret_store: Arc<dyn SecretStore>,
        config: DesktopSyncHostConfig,
        libraries: impl IntoIterator<Item = DesktopSyncLibraryConfig>,
    ) -> Result<Self, DesktopSyncHostError> {
        let inner = HostInner::new(state, secret_store, Some(platform), config, true);
        let host = Self { inner };
        for library in libraries {
            if let Err(error) = host.register_library(library).await {
                let _ = host.shutdown().await;
                return Err(error);
            }
        }
        Ok(host)
    }

    #[must_use]
    pub fn lifecycle(&self) -> DesktopSyncHostLifecycle {
        self.inner.lifecycle()
    }

    #[must_use]
    pub fn runtime_identity(&self) -> SyncRuntimeIdentity {
        self.inner.runtime.identity()
    }

    #[must_use]
    pub fn runtime(&self) -> &SyncRuntime {
        self.inner.runtime.as_ref()
    }

    /// Return the worker handle once the host has started. The handle is
    /// intentionally absent before startup and after shutdown, while the
    /// host-owned runtime identity remains stable for the host lifetime.
    #[must_use]
    pub fn runtime_handle(&self) -> Option<SyncRuntimeHandle> {
        self.inner.runtime_handle()
    }

    #[must_use]
    pub fn wake_notifier(&self) -> Arc<dyn SyncWakeNotifier> {
        Arc::clone(&self.inner.notifier)
    }

    #[must_use]
    pub fn handle(&self) -> DesktopSyncHostHandle {
        DesktopSyncHostHandle {
            inner: Arc::clone(&self.inner),
        }
    }

    #[must_use]
    pub fn state(&self) -> Arc<LocalStateStore> {
        Arc::clone(&self.inner.state)
    }

    #[must_use]
    pub fn platform(&self) -> Option<Arc<dyn PlatformRuntime>> {
        self.inner.platform.clone()
    }

    #[must_use]
    pub fn events(&self) -> tokio::sync::broadcast::Receiver<SyncRuntimeEvent> {
        self.inner.runtime.events()
    }

    #[must_use]
    pub fn statuses(&self) -> Vec<SyncRuntimeLibraryStatus> {
        self.inner.runtime.statuses()
    }

    #[must_use]
    pub fn status(&self, library_id: LibraryId) -> Option<SyncRuntimeLibraryStatus> {
        self.inner.runtime.status(library_id)
    }

    /// Return process-ephemeral root availability without exposing the
    /// configured filesystem path. A missing root is a per-library degraded
    /// condition and does not alter the durable SQLite schema.
    #[must_use]
    pub fn root_status(&self, library_id: LibraryId) -> Option<RootAvailability> {
        self.inner.libraries.try_lock().ok().and_then(|libraries| {
            libraries
                .get(&library_id)
                .map(|library| library.root_availability())
        })
    }

    #[must_use]
    pub fn root_statuses(&self) -> Vec<(LibraryId, RootAvailability)> {
        self.inner
            .libraries
            .try_lock()
            .map(|libraries| {
                libraries
                    .iter()
                    .map(|(library_id, library)| (*library_id, library.root_availability()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Install a test-only recovery barrier on the existing host. The barrier
    /// is sampled by the already-running root lifecycle task and does not
    /// change production timing or root semantics when absent.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn install_test_recovery_gate(&self, gate: Arc<DesktopRootRecoveryGate>) {
        *self
            .inner
            .recovery_gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::clone(&gate.inner));
    }

    #[must_use]
    pub fn registered_libraries(&self) -> Vec<LibraryId> {
        self.inner.runtime.registered_libraries()
    }

    pub async fn register_library(
        &self,
        library: DesktopSyncLibraryConfig,
    ) -> Result<DesktopSyncLibraryRegistration, DesktopSyncHostError> {
        self.inner.register_library(library).await
    }

    pub async fn unregister_library(
        &self,
        library_id: LibraryId,
    ) -> Result<SyncRuntimeUnregistration, DesktopSyncHostError> {
        self.inner.unregister_library(library_id).await
    }

    #[must_use]
    pub fn observer(&self, library_id: LibraryId) -> Option<Arc<OutboundObservationEngine>> {
        self.inner
            .libraries
            .try_lock()
            .ok()
            .and_then(|libraries| libraries.get(&library_id).cloned())
            .and_then(|library| library.observer.clone())
    }

    #[must_use]
    pub fn outbound_intent_producer(&self) -> OutboundIntentProducer {
        OutboundIntentProducer::new(
            Arc::clone(&self.inner.state),
            Arc::clone(&self.inner.notifier),
        )
    }

    #[must_use]
    pub fn credential_controller(&self) -> CredentialLifecycleController {
        CredentialLifecycleController::new(
            Arc::clone(&self.inner.state),
            Arc::clone(&self.inner.secret_store),
            Arc::clone(&self.inner.notifier),
        )
    }

    pub fn sync_now(&self, library_id: LibraryId) -> SyncRuntimeWakeResult {
        self.handle().sync_now(library_id)
    }

    #[must_use]
    pub fn credential_changed(&self, library_id: LibraryId) -> SyncRuntimeWakeResult {
        self.handle().credential_changed(library_id)
    }

    #[must_use]
    pub fn network_available(&self) -> Vec<(LibraryId, SyncRuntimeWakeResult)> {
        self.handle().network_available()
    }

    /// Start the one runtime supervisor and then enable all configured
    /// observers. Duplicate calls wait for the first transition and return a
    /// typed `AlreadyStarted` result; they never create duplicate workers.
    pub async fn start(&self) -> Result<DesktopSyncHostHandle, DesktopSyncHostError> {
        let wait_for_existing = {
            let mut lifecycle = self
                .inner
                .lifecycle
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match *lifecycle {
                DesktopSyncHostLifecycle::Constructed => {
                    *lifecycle = DesktopSyncHostLifecycle::Starting;
                    self.inner.stop_requested.store(false, Ordering::Release);
                    false
                }
                DesktopSyncHostLifecycle::Starting => true,
                DesktopSyncHostLifecycle::Running => {
                    return Err(DesktopSyncHostError::AlreadyStarted);
                }
                DesktopSyncHostLifecycle::Stopping => return Err(DesktopSyncHostError::Stopping),
                DesktopSyncHostLifecycle::Stopped => return Err(DesktopSyncHostError::Stopped),
                DesktopSyncHostLifecycle::Faulted => return Err(DesktopSyncHostError::Faulted),
            }
        };

        if wait_for_existing {
            loop {
                // Register before reading the state. This closes the small
                // Notify race where the first starter could publish Running
                // between the original state check and future creation.
                let notified = self.inner.transition_notify.notified();
                match self.lifecycle() {
                    DesktopSyncHostLifecycle::Starting => notified.await,
                    DesktopSyncHostLifecycle::Running => {
                        return Err(DesktopSyncHostError::AlreadyStarted);
                    }
                    DesktopSyncHostLifecycle::Stopped => {
                        return Err(DesktopSyncHostError::StartCancelled);
                    }
                    DesktopSyncHostLifecycle::Stopping => {
                        return Err(DesktopSyncHostError::Stopping);
                    }
                    DesktopSyncHostLifecycle::Faulted => {
                        return Err(DesktopSyncHostError::Faulted);
                    }
                    DesktopSyncHostLifecycle::Constructed => {
                        return Err(DesktopSyncHostError::InvalidState);
                    }
                }
            }
        }

        let _operation = self.inner.operation_gate.lock().await;
        let result = self.inner.start_components().await;
        match result {
            Ok(_) => {
                if self.inner.stop_requested.load(Ordering::Acquire) {
                    let _ = self.inner.stop_components().await;
                    self.inner.set_lifecycle(DesktopSyncHostLifecycle::Stopped);
                    Err(DesktopSyncHostError::StartCancelled)
                } else {
                    self.inner.set_lifecycle(DesktopSyncHostLifecycle::Running);
                    Ok(self.handle())
                }
            }
            Err(DesktopSyncHostError::StartCancelled) => {
                self.inner.set_lifecycle(DesktopSyncHostLifecycle::Stopped);
                Err(DesktopSyncHostError::StartCancelled)
            }
            Err(error) => {
                self.inner.set_lifecycle(DesktopSyncHostLifecycle::Faulted);
                Err(error)
            }
        }
    }

    /// Drain observer producers, request Prompt 92 shutdown, join all active
    /// cycles, and close host-owned state. Repeated calls are idempotent.
    pub async fn shutdown(&self) -> Result<(), DesktopSyncHostError> {
        enum ShutdownAction {
            CloseOwnedState(bool),
            StopComponents,
            WaitForTransition,
        }

        loop {
            let action = {
                let mut lifecycle = self
                    .inner
                    .lifecycle
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                match *lifecycle {
                    DesktopSyncHostLifecycle::Constructed => {
                        self.inner.stop_requested.store(true, Ordering::Release);
                        self.inner.cancellation.cancel();
                        *lifecycle = DesktopSyncHostLifecycle::Stopped;
                        self.inner.transition_notify.notify_waiters();
                        ShutdownAction::CloseOwnedState(self.inner.close_state_on_shutdown)
                    }
                    DesktopSyncHostLifecycle::Starting => {
                        self.inner.stop_requested.store(true, Ordering::Release);
                        self.inner.cancellation.cancel();
                        ShutdownAction::WaitForTransition
                    }
                    DesktopSyncHostLifecycle::Running => {
                        self.inner.stop_requested.store(true, Ordering::Release);
                        self.inner.cancellation.cancel();
                        *lifecycle = DesktopSyncHostLifecycle::Stopping;
                        ShutdownAction::StopComponents
                    }
                    DesktopSyncHostLifecycle::Stopping => ShutdownAction::WaitForTransition,
                    DesktopSyncHostLifecycle::Stopped => return Ok(()),
                    DesktopSyncHostLifecycle::Faulted => {
                        *lifecycle = DesktopSyncHostLifecycle::Stopped;
                        self.inner.transition_notify.notify_waiters();
                        return Ok(());
                    }
                }
            };

            match action {
                ShutdownAction::CloseOwnedState(close_state) => {
                    if close_state {
                        self.inner.state.close_pool().await;
                    }
                    return Ok(());
                }
                ShutdownAction::StopComponents => {
                    let _operation = self.inner.operation_gate.lock().await;
                    let result = self.inner.stop_components().await;
                    self.inner.set_lifecycle(if result.is_ok() {
                        DesktopSyncHostLifecycle::Stopped
                    } else {
                        DesktopSyncHostLifecycle::Faulted
                    });
                    return result;
                }
                ShutdownAction::WaitForTransition => {
                    // Register before checking the state. This avoids a lost
                    // transition when startup or another shutdown caller
                    // completes between the state check and await.
                    loop {
                        let notified = self.inner.transition_notify.notified();
                        match self.lifecycle() {
                            DesktopSyncHostLifecycle::Starting
                            | DesktopSyncHostLifecycle::Stopping => notified.await,
                            DesktopSyncHostLifecycle::Stopped => break,
                            DesktopSyncHostLifecycle::Faulted => {
                                self.inner.set_lifecycle(DesktopSyncHostLifecycle::Stopped);
                                break;
                            }
                            DesktopSyncHostLifecycle::Constructed
                            | DesktopSyncHostLifecycle::Running => break,
                        }
                    }
                    continue;
                }
            }
        }
    }

    /// `join` is intentionally safe as the only call after a start: it
    /// requests the same graceful host shutdown and waits for completion.
    pub async fn join(&self) -> Result<(), DesktopSyncHostError> {
        self.shutdown().await
    }
}

/// Narrow cloneable application control surface. It contains no credential,
/// transport, engine, filesystem-path, or content access.
#[derive(Clone)]
pub struct DesktopSyncHostHandle {
    inner: Arc<HostInner>,
}

impl fmt::Debug for DesktopSyncHostHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopSyncHostHandle")
            .field("lifecycle", &self.lifecycle())
            .field("runtime_identity", &self.runtime_identity())
            .finish_non_exhaustive()
    }
}

impl DesktopSyncHostHandle {
    #[must_use]
    pub fn lifecycle(&self) -> DesktopSyncHostLifecycle {
        self.inner.lifecycle()
    }

    #[must_use]
    pub fn runtime_identity(&self) -> SyncRuntimeIdentity {
        self.inner.runtime.identity()
    }

    #[must_use]
    pub fn runtime_handle(&self) -> Option<SyncRuntimeHandle> {
        self.inner.runtime_handle()
    }

    #[must_use]
    pub fn wake_notifier(&self) -> Arc<dyn SyncWakeNotifier> {
        Arc::clone(&self.inner.notifier)
    }

    #[must_use]
    pub fn events(&self) -> tokio::sync::broadcast::Receiver<SyncRuntimeEvent> {
        self.inner.runtime.events()
    }

    #[must_use]
    pub fn statuses(&self) -> Vec<SyncRuntimeLibraryStatus> {
        self.inner.runtime.statuses()
    }

    #[must_use]
    pub fn registered_libraries(&self) -> Vec<LibraryId> {
        self.inner.runtime.registered_libraries()
    }

    #[must_use]
    pub fn status(&self, library_id: LibraryId) -> Option<SyncRuntimeLibraryStatus> {
        self.inner.runtime.status(library_id)
    }

    #[must_use]
    pub fn root_status(&self, library_id: LibraryId) -> Option<RootAvailability> {
        self.inner.libraries.try_lock().ok().and_then(|libraries| {
            libraries
                .get(&library_id)
                .map(|library| library.root_availability())
        })
    }

    #[must_use]
    pub fn root_statuses(&self) -> Vec<(LibraryId, RootAvailability)> {
        self.inner
            .libraries
            .try_lock()
            .map(|libraries| {
                libraries
                    .iter()
                    .map(|(library_id, library)| (*library_id, library.root_availability()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Manual control is a scheduling hint and never invokes Prompt 91
    /// directly. A host that is not running returns the runtime's typed
    /// stopped result rather than executing lower-level work.
    #[must_use]
    pub fn sync_now(&self, library_id: LibraryId) -> SyncRuntimeWakeResult {
        if self.lifecycle() != DesktopSyncHostLifecycle::Running {
            return SyncRuntimeWakeResult::RuntimeStopped;
        }
        self.inner.runtime.sync_now_status(library_id)
    }

    /// Deliver a network-available hint to every currently registered
    /// library. The hint cannot bypass authentication or conflict fences.
    #[must_use]
    pub fn network_available(&self) -> Vec<(LibraryId, SyncRuntimeWakeResult)> {
        let ids = self.inner.runtime.registered_libraries();
        if self.lifecycle() != DesktopSyncHostLifecycle::Running {
            return ids
                .into_iter()
                .map(|library_id| (library_id, SyncRuntimeWakeResult::RuntimeStopped))
                .collect();
        }
        self.inner.runtime.network_available()
    }

    #[must_use]
    pub fn credential_changed(&self, library_id: LibraryId) -> SyncRuntimeWakeResult {
        if self.lifecycle() != DesktopSyncHostLifecycle::Running {
            return SyncRuntimeWakeResult::RuntimeStopped;
        }
        self.inner.runtime.credential_changed(library_id)
    }

    #[must_use]
    pub fn credentials_changed(&self) -> Vec<(LibraryId, SyncRuntimeWakeResult)> {
        let ids = self.inner.runtime.registered_libraries();
        if self.lifecycle() != DesktopSyncHostLifecycle::Running {
            return ids
                .into_iter()
                .map(|library_id| (library_id, SyncRuntimeWakeResult::RuntimeStopped))
                .collect();
        }
        self.inner.runtime.credentials_changed()
    }

    /// Process adapters use this narrow operation for shutdown callbacks.
    pub async fn on_lifecycle_event(
        &self,
        event: DesktopLifecycleEvent,
    ) -> Result<(), DesktopSyncHostError> {
        match event {
            DesktopLifecycleEvent::ShutdownRequested => self.shutdown().await,
        }
    }

    #[must_use]
    pub fn outbound_intent_producer(&self) -> OutboundIntentProducer {
        OutboundIntentProducer::new(
            Arc::clone(&self.inner.state),
            Arc::clone(&self.inner.notifier),
        )
    }

    #[must_use]
    pub fn credential_controller(&self) -> CredentialLifecycleController {
        CredentialLifecycleController::new(
            Arc::clone(&self.inner.state),
            Arc::clone(&self.inner.secret_store),
            Arc::clone(&self.inner.notifier),
        )
    }

    pub async fn shutdown(&self) -> Result<(), DesktopSyncHostError> {
        DesktopSyncHost {
            inner: Arc::clone(&self.inner),
        }
        .shutdown()
        .await
    }

    pub async fn join(&self) -> Result<(), DesktopSyncHostError> {
        self.shutdown().await
    }
}

/// Common lifecycle adapter used by platform integrations. Linux and Windows
/// aliases below deliberately have identical semantics and contain no sync
/// policy or OS API calls.
#[derive(Clone)]
pub struct DesktopLifecycleAdapter {
    handle: DesktopSyncHostHandle,
}

impl DesktopLifecycleAdapter {
    #[must_use]
    pub fn new(handle: DesktopSyncHostHandle) -> Self {
        Self { handle }
    }

    pub async fn deliver(&self, event: DesktopLifecycleEvent) -> Result<(), DesktopSyncHostError> {
        self.handle.on_lifecycle_event(event).await
    }

    #[must_use]
    pub fn handle(&self) -> DesktopSyncHostHandle {
        self.handle.clone()
    }
}

/// Linux process lifecycle seam. The embedding process supplies SIGINT,
/// SIGTERM, or desktop-session events when available.
pub type LinuxLifecycleAdapter = DesktopLifecycleAdapter;

/// Windows process lifecycle seam. The embedding process supplies Ctrl-C or
/// application-close events when available.
pub type WindowsLifecycleAdapter = DesktopLifecycleAdapter;

/// Common network hint adapter. It intentionally has no connectivity probe;
/// callers deliver a positive hint and Prompt 92 remains authoritative for
/// transport/auth outcomes.
#[derive(Clone)]
pub struct DesktopNetworkAdapter {
    handle: DesktopSyncHostHandle,
}

impl DesktopNetworkAdapter {
    #[must_use]
    pub fn new(handle: DesktopSyncHostHandle) -> Self {
        Self { handle }
    }

    #[must_use]
    pub fn available(&self) -> Vec<(LibraryId, SyncRuntimeWakeResult)> {
        self.handle.network_available()
    }

    #[must_use]
    pub fn handle(&self) -> DesktopSyncHostHandle {
        self.handle.clone()
    }
}

pub type LinuxNetworkAdapter = DesktopNetworkAdapter;
pub type WindowsNetworkAdapter = DesktopNetworkAdapter;

fn is_root_lifecycle_error(error: &ClientSyncError) -> bool {
    matches!(
        error,
        ClientSyncError::InvalidRoot
            | ClientSyncError::RootUnavailable
            | ClientSyncError::WrongRootBinding
            | ClientSyncError::RootRedirected
            | ClientSyncError::WrongServerProfile
    )
}

async fn mark_library_root_unavailable(library: &HostLibrary) {
    if library.root_availability() != RootAvailability::Unavailable {
        library.set_root_availability(RootAvailability::Unavailable);
        if let Some(observer) = &library.observer {
            let _ = observer.shutdown().await;
        }
    }
}

fn validate_root_for_cycle(replica: &dyn LocalReplica) -> Result<(), ClientSyncError> {
    replica
        .validate_root()
        .map_err(|_| ClientSyncError::RootUnavailable)
}

struct PlatformSecretStore {
    platform: Arc<dyn PlatformRuntime>,
}

impl SecretStore for PlatformSecretStore {
    fn state(&self) -> SecretStoreState {
        self.platform.secret_store().state()
    }

    fn put_secret(&self, name: &SecretName, value: &[u8]) -> Result<(), SecretStoreError> {
        self.platform.secret_store().put_secret(name, value)
    }

    fn get_secret(&self, name: &SecretName) -> Result<Option<SecretValue>, SecretStoreError> {
        self.platform.secret_store().get_secret(name)
    }

    fn delete_secret(&self, name: &SecretName) -> Result<bool, SecretStoreError> {
        self.platform.secret_store().delete_secret(name)
    }
}

/// Root gate shared by every host-owned Prompt 91 executor. The gate is the
/// last local safety boundary before inbound or outbound filesystem work: a
/// disappearing, redirected, or rebound root is a blocked library, never an
/// empty tree that can be interpreted as mass deletion.
struct RootGatedCycle {
    scope: ReplicaScope,
    replica: Arc<dyn LocalReplica>,
    inner: Arc<dyn SyncCycleExecutor>,
}

impl RootGatedCycle {
    fn new(
        scope: ReplicaScope,
        replica: Arc<dyn LocalReplica>,
        inner: Arc<dyn SyncCycleExecutor>,
    ) -> Self {
        Self {
            scope,
            replica,
            inner,
        }
    }
}

#[async_trait]
impl SyncCycleExecutor for RootGatedCycle {
    fn scope(&self) -> ReplicaScope {
        self.scope
    }

    async fn run_once(&self, observed_at: Timestamp) -> Result<SyncCycleResult, ClientSyncError> {
        validate_root_for_cycle(self.replica.as_ref())?;
        self.inner.run_once(observed_at).await
    }
}

/// A transport-neutral library remains registered while its root is absent.
/// Lower-level engine constructors are deferred until the root binding passes
/// validation again, so startup does not turn a missing mount into a fatal
/// composition error or a destructive reconciliation.
struct ReloadableRemoteCycle {
    scope: ReplicaScope,
    remote: Arc<dyn DesktopSyncRemote>,
    replica: Arc<dyn LocalReplica>,
    state: Arc<LocalStateStore>,
    config: DesktopSyncHostConfig,
    current: AsyncMutex<Option<Arc<dyn SyncCycleExecutor>>>,
}

impl ReloadableRemoteCycle {
    fn new(
        scope: ReplicaScope,
        remote: Arc<dyn DesktopSyncRemote>,
        replica: Arc<dyn LocalReplica>,
        state: Arc<LocalStateStore>,
        config: DesktopSyncHostConfig,
    ) -> Self {
        Self {
            scope,
            remote,
            replica,
            state,
            config,
            current: AsyncMutex::new(None),
        }
    }

    async fn current_cycle(&self) -> Result<Arc<dyn SyncCycleExecutor>, ClientSyncError> {
        validate_root_for_cycle(self.replica.as_ref())?;
        let mut current = self.current.lock().await;
        if let Some(cycle) = current.as_ref() {
            return Ok(Arc::clone(cycle));
        }
        let cycle = build_cycle_from_remote(
            self.scope,
            Arc::clone(&self.remote),
            Arc::clone(&self.replica),
            Arc::clone(&self.state),
            self.config,
        )
        .await?;
        current.replace(Arc::clone(&cycle));
        Ok(cycle)
    }
}

#[async_trait]
impl SyncCycleExecutor for ReloadableRemoteCycle {
    fn scope(&self) -> ReplicaScope {
        self.scope
    }

    async fn run_once(&self, observed_at: Timestamp) -> Result<SyncCycleResult, ClientSyncError> {
        self.current_cycle().await?.run_once(observed_at).await
    }
}

async fn build_cycle_from_remote(
    scope: ReplicaScope,
    remote: Arc<dyn DesktopSyncRemote>,
    replica: Arc<dyn LocalReplica>,
    state: Arc<LocalStateStore>,
    config: DesktopSyncHostConfig,
) -> Result<Arc<dyn SyncCycleExecutor>, ClientSyncError> {
    let sync_remote: Arc<dyn SyncRemote> = remote.clone();
    let inbound = Arc::new(
        InboundSyncEngine::new(
            scope,
            sync_remote.clone(),
            Arc::clone(&replica),
            Arc::clone(&state),
            config.engine(),
        )
        .await?,
    );
    let snapshot_remote: Arc<dyn RebaselineSnapshotRemote> = remote;
    let convergence = Arc::new(RebaselineConvergenceCoordinator::new(
        inbound,
        snapshot_remote,
        Arc::clone(&state),
        config.rebaseline_page_limit(),
    )?);
    let outbound =
        Arc::new(crate::OutboundSubmissionEngine::new(scope, sync_remote, replica, state).await?);
    Ok(Arc::new(BidirectionalSyncCycleRunner::new(
        convergence,
        outbound,
    )?))
}

struct ReloadableHttpCycle {
    scope: ReplicaScope,
    profile_id: crate::ServerProfileId,
    replica: Arc<dyn LocalReplica>,
    state: Arc<LocalStateStore>,
    secret_store: Arc<dyn SecretStore>,
    config: DesktopSyncHostConfig,
    current: AsyncMutex<Option<LoadedHttpCycle>>,
}

struct LoadedHttpCycle {
    credential_id: DeviceCredentialId,
    cycle: Arc<BidirectionalSyncCycleRunner>,
}

impl ReloadableHttpCycle {
    fn new(
        scope: ReplicaScope,
        profile_id: crate::ServerProfileId,
        replica: Arc<dyn LocalReplica>,
        state: Arc<LocalStateStore>,
        secret_store: Arc<dyn SecretStore>,
        config: DesktopSyncHostConfig,
    ) -> Self {
        Self {
            scope,
            profile_id,
            replica,
            state,
            secret_store,
            config,
            current: AsyncMutex::new(None),
        }
    }

    async fn current_cycle(
        &self,
    ) -> Result<Option<Arc<BidirectionalSyncCycleRunner>>, ClientSyncError> {
        let loaded = self
            .state
            .load_device_credential(self.profile_id, self.secret_store.as_ref())
            .await?;
        let Some(loaded) = loaded else {
            // Forget/removal immediately invalidates the in-memory immutable
            // transport. Durable local work remains in SQLite for a future
            // enrollment.
            *self.current.lock().await = None;
            return Ok(None);
        };
        if loaded.owner_user_id() != self.scope.owner_user_id()
            || loaded.device_id() != self.scope.device_id()
        {
            // The secure-store envelope is profile-bound, but the host also
            // owns an explicit owner/device scope. Refuse the credential
            // before constructing a transport so a copied profile cannot
            // cross an account boundary even transiently at the HTTP layer.
            *self.current.lock().await = None;
            return Err(ClientSyncError::WrongScope);
        }
        let credential_id = loaded.credential_id();
        let mut current = self.current.lock().await;
        if current
            .as_ref()
            .is_some_and(|cycle| cycle.credential_id == credential_id)
        {
            return Ok(current.as_ref().map(|cycle| Arc::clone(&cycle.cycle)));
        }

        // HttpSyncRemote intentionally owns an immutable loaded credential.
        // Rebuild only this library's lower-level transport graph when the
        // secure-store generation changes; the surrounding Prompt 92 runtime,
        // notifier, observer, and host remain the same objects.
        current.take();
        let profile = self
            .state
            .server_profile(self.profile_id)
            .await?
            .ok_or(ClientSyncError::InvalidServerProfile)?;
        let remote = Arc::new(HttpSyncRemote::new(
            profile,
            self.scope.device_id(),
            loaded,
            self.config.http(),
        )?);
        let sync_remote: Arc<dyn SyncRemote> = remote.clone();
        let inbound = Arc::new(
            InboundSyncEngine::new(
                self.scope,
                sync_remote.clone(),
                Arc::clone(&self.replica),
                Arc::clone(&self.state),
                self.config.engine(),
            )
            .await?,
        );
        let snapshot_remote: Arc<dyn RebaselineSnapshotRemote> = remote;
        let convergence = Arc::new(RebaselineConvergenceCoordinator::new(
            inbound,
            snapshot_remote,
            Arc::clone(&self.state),
            self.config.rebaseline_page_limit(),
        )?);
        let outbound = Arc::new(
            crate::OutboundSubmissionEngine::new(
                self.scope,
                sync_remote,
                Arc::clone(&self.replica),
                Arc::clone(&self.state),
            )
            .await?,
        );
        let cycle = Arc::new(BidirectionalSyncCycleRunner::new(convergence, outbound)?);
        current.replace(LoadedHttpCycle {
            credential_id,
            cycle: Arc::clone(&cycle),
        });
        Ok(Some(cycle))
    }
}

#[async_trait]
impl SyncCycleExecutor for ReloadableHttpCycle {
    fn scope(&self) -> ReplicaScope {
        self.scope
    }

    async fn run_once(&self, observed_at: Timestamp) -> Result<SyncCycleResult, ClientSyncError> {
        match self.current_cycle().await? {
            Some(cycle) => cycle.run_once(observed_at).await,
            None => Ok(SyncCycleResult::new(
                observed_at,
                InboundCycleOutcome::AuthRequired,
                OutboundCycleOutcome::NotAttempted(
                    OutboundSkipReason::InboundAuthenticationRequired,
                ),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use async_trait::async_trait;
    use synveil_core::{
        DeviceCredentialId, DeviceCredentialSecret, DeviceId, LibraryId, LogicalName, NodeId,
        NodeKind, NodeState, Revision, Sequence, Timestamp, UserId,
    };
    use synveil_platform::{
        SecretName, SecretStoreError, SecretStoreState, SecretValue, UnsupportedSecureSecretStore,
    };

    use super::*;
    use crate::{
        CanonicalBaseUrl, DurableChangeResult, EnrollmentCredentials, FilesystemLocalReplica,
        InboundCycleOutcome, LocalFingerprint, LocalNode, LocalStateConfig, ManagedRelativePath,
        ManualChangeWatcher, OutboundCycleOutcome, OutboundIntent, OutboundIntentKind,
        OutboundSubmissionOutcome, RebaselineConvergenceOutcome, ServerProfile,
        SyncRuntimeLibraryPhase, SyncRuntimeOutcome, WatchHint, WatchHintKind,
        test_support::remove_dir_all_bounded,
    };

    struct HostFixture {
        directory: PathBuf,
        state: Arc<LocalStateStore>,
        scope: ReplicaScope,
        replica: Arc<FilesystemLocalReplica>,
    }

    impl HostFixture {
        async fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "synveil-desktop-sync-host-{}",
                uuid::Uuid::now_v7()
            ));
            fs::create_dir_all(&directory).expect("fixture directory");
            let root = directory.join("root");
            fs::create_dir(&root).expect("fixture root");
            let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
            let replica = Arc::new(
                FilesystemLocalReplica::initialize(&root, scope).expect("managed replica"),
            );
            let state = Arc::new(
                LocalStateStore::open(&LocalStateConfig::new(directory.join("state.sqlite3")))
                    .await
                    .expect("local state"),
            );
            state
                .bind_replica(scope, replica.binding_id())
                .await
                .expect("replica binding");
            state
                .upsert_local_node(&LocalNode::new(
                    scope.library_id(),
                    NodeId::new(),
                    None,
                    ManagedRelativePath::root(),
                    LogicalName::new("root").expect("root name"),
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
                .expect("root node");
            Self {
                directory,
                state,
                scope,
                replica,
            }
        }

        async fn add_library(&self, label: &str) -> (ReplicaScope, Arc<FilesystemLocalReplica>) {
            self.add_library_for_scope(label, self.scope.owner_user_id(), self.scope.device_id())
                .await
        }

        async fn add_library_for_scope(
            &self,
            label: &str,
            owner_user_id: UserId,
            device_id: DeviceId,
        ) -> (ReplicaScope, Arc<FilesystemLocalReplica>) {
            let root = self.directory.join(format!("root-{label}"));
            fs::create_dir_all(&root).expect("additional fixture root");
            let scope = ReplicaScope::new(owner_user_id, device_id, LibraryId::new());
            let replica = Arc::new(
                FilesystemLocalReplica::initialize(&root, scope)
                    .expect("additional managed replica"),
            );
            self.state
                .bind_replica(scope, replica.binding_id())
                .await
                .expect("additional replica binding");
            self.state
                .upsert_local_node(&LocalNode::new(
                    scope.library_id(),
                    NodeId::new(),
                    None,
                    ManagedRelativePath::root(),
                    LogicalName::new("root").expect("additional root name"),
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
                .expect("additional root node");
            (scope, replica)
        }

        async fn close(self) {
            let Self {
                directory,
                state,
                replica,
                ..
            } = self;
            state.close_pool().await;
            drop(replica);
            drop(state);
            remove_dir_all_bounded(&directory).expect("fixture cleanup");
        }
    }

    /// Profile-bound fixture used by root-availability tests. Production
    /// deferred replicas are only valid when the durable profile/root binding
    /// already exists; keeping that setup explicit prevents tests from
    /// accidentally exercising a legacy unbound root path.
    struct ProfiledHostFixture {
        directory: PathBuf,
        root: PathBuf,
        away_root: PathBuf,
        state: Arc<LocalStateStore>,
        scope: ReplicaScope,
        profile: ServerProfile,
        replica: Arc<FilesystemLocalReplica>,
    }

    impl ProfiledHostFixture {
        async fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "synveil-desktop-sync-host-root-{}",
                uuid::Uuid::now_v7()
            ));
            fs::create_dir_all(&directory).expect("profiled fixture directory");
            let root = directory.join("root");
            let away_root = directory.join("root-away");
            fs::create_dir(&root).expect("profiled fixture root");
            let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
            let profile = ServerProfile::new(
                CanonicalBaseUrl::parse("https://desktop-root-fixture.example")
                    .expect("profile fixture URL"),
                "desktop root fixture",
            )
            .expect("profile fixture");
            let replica = Arc::new(
                FilesystemLocalReplica::initialize_for_profile(&root, scope, profile.profile_id())
                    .expect("profiled managed replica"),
            );
            let state = Arc::new(
                LocalStateStore::open(&LocalStateConfig::new(directory.join("state.sqlite3")))
                    .await
                    .expect("profiled local state"),
            );
            state
                .save_server_profile(&profile)
                .await
                .expect("profile fixture profile");
            state
                .prepare_replica_for_profile(scope, replica.binding_id(), profile.profile_id())
                .await
                .expect("profile fixture binding");
            state
                .upsert_local_node(&LocalNode::new(
                    scope.library_id(),
                    NodeId::new(),
                    None,
                    ManagedRelativePath::root(),
                    LogicalName::new("root").expect("profile fixture root name"),
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
                .expect("profile fixture root node");
            Self {
                directory,
                root,
                away_root,
                state,
                scope,
                profile,
                replica,
            }
        }

        fn move_root_away(&self) {
            fs::rename(&self.root, &self.away_root).expect("move fixture root away");
        }

        fn restore_root(&self) {
            if self.away_root.exists() {
                fs::rename(&self.away_root, &self.root).expect("restore fixture root");
            }
        }

        fn deferred_replica(&self) -> Arc<FilesystemLocalReplica> {
            Arc::new(
                FilesystemLocalReplica::open_deferred_for_profile(
                    &self.root,
                    self.scope,
                    self.profile.profile_id(),
                    self.replica.binding_id(),
                )
                .expect("deferred profiled replica"),
            )
        }

        async fn add_library(&self, label: &str) -> (ReplicaScope, Arc<FilesystemLocalReplica>) {
            let root = self.directory.join(format!("root-{label}"));
            fs::create_dir(&root).expect("profiled additional root");
            let scope = ReplicaScope::new(
                self.scope.owner_user_id(),
                self.scope.device_id(),
                LibraryId::new(),
            );
            let replica = Arc::new(
                FilesystemLocalReplica::initialize_for_profile(
                    &root,
                    scope,
                    self.profile.profile_id(),
                )
                .expect("profiled additional replica"),
            );
            self.state
                .prepare_replica_for_profile(scope, replica.binding_id(), self.profile.profile_id())
                .await
                .expect("profiled additional binding");
            self.state
                .upsert_local_node(&LocalNode::new(
                    scope.library_id(),
                    NodeId::new(),
                    None,
                    ManagedRelativePath::root(),
                    LogicalName::new("root").expect("profiled additional root name"),
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
                .expect("profiled additional root node");
            (scope, replica)
        }

        async fn seed_file(&self, relative: &ManagedRelativePath) -> LocalFingerprint {
            let path = self.replica.root_path().join(relative.as_path());
            fs::write(&path, b"root lifecycle sentinel").expect("seed fixture file");
            let fingerprint = self
                .replica
                .inspect(relative)
                .expect("seed inspect")
                .expect("seed fingerprint");
            let root_node = self
                .state
                .local_nodes(self.scope.library_id())
                .await
                .expect("seed root nodes")
                .into_iter()
                .find(|node| node.parent_node_id().is_none())
                .expect("seed root node");
            let node = LocalNode::new(
                self.scope.library_id(),
                NodeId::new(),
                Some(root_node.node_id()),
                relative.clone(),
                LogicalName::new("root lifecycle sentinel").expect("seed logical name"),
                NodeKind::File,
                NodeState::Active,
                Revision::new(1),
                None,
                None,
                None,
                fingerprint.length(),
                fingerprint.sha256(),
                Sequence::new(0),
                true,
                None,
            );
            self.state
                .upsert_local_node(&node)
                .await
                .expect("seed local node");
            self.state
                .persist_observed_node(&node, relative, true, Some(fingerprint))
                .await
                .expect("seed observed node");
            fingerprint
        }

        async fn close(self) {
            self.restore_root();
            let Self {
                directory,
                state,
                replica,
                ..
            } = self;
            state.close_pool().await;
            drop(replica);
            drop(state);
            remove_dir_all_bounded(&directory).expect("profiled fixture cleanup");
        }
    }

    struct CountingExecutor {
        scope: ReplicaScope,
        calls: AtomicUsize,
    }

    impl CountingExecutor {
        fn new(scope: ReplicaScope) -> Arc<Self> {
            Arc::new(Self {
                scope,
                calls: AtomicUsize::new(0),
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::Acquire)
        }
    }

    #[async_trait]
    impl SyncCycleExecutor for CountingExecutor {
        fn scope(&self) -> ReplicaScope {
            self.scope
        }

        async fn run_once(
            &self,
            observed_at: Timestamp,
        ) -> Result<SyncCycleResult, ClientSyncError> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            Ok(SyncCycleResult::new(
                observed_at,
                InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::IncrementalReady),
                OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::NoReadyIntent),
            ))
        }
    }

    #[derive(Default)]
    struct HostTestSecretStore {
        values: StdMutex<BTreeMap<SecretName, SecretValue>>,
    }

    impl SecretStore for HostTestSecretStore {
        fn state(&self) -> SecretStoreState {
            SecretStoreState::Available
        }

        fn put_secret(&self, name: &SecretName, value: &[u8]) -> Result<(), SecretStoreError> {
            self.values
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(name.clone(), SecretValue::new(value));
            Ok(())
        }

        fn get_secret(&self, name: &SecretName) -> Result<Option<SecretValue>, SecretStoreError> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(name)
                .map(|value| SecretValue::new(value.as_bytes())))
        }

        fn delete_secret(&self, name: &SecretName) -> Result<bool, SecretStoreError> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(name)
                .is_some())
        }
    }

    struct BlockingExecutor {
        scope: ReplicaScope,
        calls: AtomicUsize,
        started: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }

    impl BlockingExecutor {
        fn new(scope: ReplicaScope) -> Arc<Self> {
            Arc::new(Self {
                scope,
                calls: AtomicUsize::new(0),
                started: Arc::new(tokio::sync::Notify::new()),
                release: Arc::new(tokio::sync::Notify::new()),
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::Acquire)
        }
    }

    #[async_trait]
    impl SyncCycleExecutor for BlockingExecutor {
        fn scope(&self) -> ReplicaScope {
            self.scope
        }

        async fn run_once(
            &self,
            observed_at: Timestamp,
        ) -> Result<SyncCycleResult, ClientSyncError> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            self.started.notify_one();
            self.release.notified().await;
            Ok(SyncCycleResult::new(
                observed_at,
                InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::IncrementalReady),
                OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::NoReadyIntent),
            ))
        }
    }

    struct BlockingStartWatcher {
        started: std::sync::mpsc::SyncSender<()>,
        release: std::sync::mpsc::Receiver<()>,
    }

    impl LocalChangeWatcher for BlockingStartWatcher {
        fn start(&mut self, _managed_root: &std::path::Path) -> Result<(), ClientSyncError> {
            self.started
                .send(())
                .map_err(|_| ClientSyncError::InvalidState)?;
            self.release
                .recv()
                .map_err(|_| ClientSyncError::InvalidState)?;
            Ok(())
        }

        fn poll(&mut self, _maximum: usize) -> Result<Vec<WatchHint>, ClientSyncError> {
            Ok(Vec::new())
        }

        fn stop(&mut self) -> Result<(), ClientSyncError> {
            Ok(())
        }
    }

    struct FailingStartWatcher;

    impl LocalChangeWatcher for FailingStartWatcher {
        fn start(&mut self, _managed_root: &std::path::Path) -> Result<(), ClientSyncError> {
            Err(ClientSyncError::InvalidState)
        }

        fn poll(&mut self, _maximum: usize) -> Result<Vec<WatchHint>, ClientSyncError> {
            Ok(Vec::new())
        }

        fn stop(&mut self) -> Result<(), ClientSyncError> {
            Ok(())
        }
    }

    struct CountingWatcher {
        starts: Arc<AtomicUsize>,
        stops: Arc<AtomicUsize>,
        started: bool,
    }

    impl CountingWatcher {
        fn new() -> (Self, Arc<AtomicUsize>, Arc<AtomicUsize>) {
            let starts = Arc::new(AtomicUsize::new(0));
            let stops = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    starts: Arc::clone(&starts),
                    stops: Arc::clone(&stops),
                    started: false,
                },
                starts,
                stops,
            )
        }
    }

    impl LocalChangeWatcher for CountingWatcher {
        fn start(&mut self, managed_root: &std::path::Path) -> Result<(), ClientSyncError> {
            if self.started || !managed_root.is_dir() {
                return Err(ClientSyncError::InvalidState);
            }
            self.started = true;
            self.starts.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }

        fn poll(&mut self, _maximum: usize) -> Result<Vec<WatchHint>, ClientSyncError> {
            if !self.started {
                return Err(ClientSyncError::InvalidState);
            }
            Ok(Vec::new())
        }

        fn stop(&mut self) -> Result<(), ClientSyncError> {
            if self.started {
                self.started = false;
                self.stops.fetch_add(1, Ordering::AcqRel);
            }
            Ok(())
        }
    }

    fn host_config() -> DesktopSyncHostConfig {
        DesktopSyncHostConfig::default().with_observation_poll_interval(Duration::from_millis(10))
    }

    fn root_probe_config() -> DesktopSyncHostConfig {
        host_config().with_root_probe_interval(Duration::from_millis(10))
    }

    fn root_probe_only_config() -> DesktopSyncHostConfig {
        DesktopSyncHostConfig::default()
            .with_observation_poll_interval(Duration::from_secs(60 * 60))
            .with_root_probe_interval(Duration::from_millis(10))
    }

    fn executor_library(
        fixture: &HostFixture,
        executor: Arc<dyn SyncCycleExecutor>,
    ) -> DesktopSyncLibraryConfig {
        DesktopSyncLibraryConfig::from_executor(fixture.scope, executor, fixture.replica.clone())
            .expect("executor library")
    }

    async fn wait_for_cycle(
        events: &mut tokio::sync::broadcast::Receiver<SyncRuntimeEvent>,
        library_id: LibraryId,
    ) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match events.recv().await.expect("runtime event") {
                    SyncRuntimeEvent::CycleFinished {
                        library_id: event_library,
                        ..
                    } if event_library == library_id => break,
                    _ => {}
                }
            }
        })
        .await
        .expect("startup cycle");
    }

    async fn wait_for_cycle_outcome(
        events: &mut tokio::sync::broadcast::Receiver<SyncRuntimeEvent>,
        library_id: LibraryId,
        expected: SyncRuntimeOutcome,
    ) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match events.recv().await.expect("runtime event") {
                    SyncRuntimeEvent::CycleFinished {
                        library_id: event_library,
                        outcome,
                    } if event_library == library_id && outcome == expected => break,
                    _ => {}
                }
            }
        })
        .await
        .expect("expected runtime outcome");
    }

    async fn wait_for_root_status(
        host: &DesktopSyncHost,
        library_id: LibraryId,
        expected: RootAvailability,
    ) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if host.root_status(library_id) == Some(expected) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("root status transition");
    }

    async fn wait_for_pending_intent(host: &DesktopSyncHost, library_id: LibraryId) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if host
                    .state()
                    .count_pending_intents(library_id)
                    .await
                    .expect("pending intent count")
                    > 0
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("bounded root rescan intent");
    }

    #[test]
    fn host1_configuration_rejects_unbounded_observer_settings() {
        assert!(matches!(
            DesktopSyncHostConfig::default()
                .with_observation_poll_interval(Duration::ZERO)
                .validate(),
            Err(DesktopSyncHostConfigError::ObservationPollIntervalTooShort)
        ));
        assert!(matches!(
            DesktopSyncHostConfig::default()
                .with_rebaseline_page_limit(0)
                .validate(),
            Err(DesktopSyncHostConfigError::RebaselinePageLimitInvalid)
        ));
    }

    #[tokio::test]
    async fn host2_construction_registers_without_starting_workers() {
        let fixture = HostFixture::new().await;
        let executor = CountingExecutor::new(fixture.scope);
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            host_config(),
            [executor_library(&fixture, executor.clone())],
        )
        .await
        .expect("host construction");

        assert_eq!(host.lifecycle(), DesktopSyncHostLifecycle::Constructed);
        assert!(host.runtime_handle().is_none());
        assert_eq!(
            host.registered_libraries(),
            vec![fixture.scope.library_id()]
        );
        assert_eq!(executor.calls(), 0);
        host.shutdown().await.expect("pre-start shutdown");
        assert_eq!(host.lifecycle(), DesktopSyncHostLifecycle::Stopped);
        assert!(host.runtime_handle().is_none());
        fixture.close().await;
    }

    #[tokio::test]
    async fn host3_start_is_singleton_and_all_control_surfaces_share_runtime() {
        let fixture = HostFixture::new().await;
        let executor = CountingExecutor::new(fixture.scope);
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            host_config(),
            [executor_library(&fixture, executor.clone())],
        )
        .await
        .expect("host construction");
        let identity = host.runtime_identity();
        let mut events = host.events();
        let handle = host.start().await.expect("host start");

        assert_eq!(handle.runtime_identity(), identity);
        assert_eq!(
            host.runtime_handle()
                .expect("host runtime handle after start")
                .identity(),
            identity
        );
        assert_eq!(handle.wake_notifier().runtime_identity(), Some(identity));
        assert_eq!(host.wake_notifier().runtime_identity(), Some(identity));
        assert_eq!(
            host.outbound_intent_producer().runtime_identity(),
            Some(identity)
        );
        assert_eq!(
            host.credential_controller().runtime_identity(),
            Some(identity)
        );
        assert!(matches!(
            host.start().await,
            Err(DesktopSyncHostError::AlreadyStarted)
        ));
        wait_for_cycle(&mut events, fixture.scope.library_id()).await;
        assert!(executor.calls() >= 1);
        host.shutdown().await.expect("host shutdown");
        fixture.close().await;
    }

    #[tokio::test]
    async fn host4_concurrent_start_callers_cannot_create_duplicate_supervisors() {
        let fixture = HostFixture::new().await;
        let executor = CountingExecutor::new(fixture.scope);
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            host_config(),
            [executor_library(&fixture, executor)],
        )
        .await
        .expect("host construction");
        let first_host = host.clone();
        let first = tokio::spawn(async move { first_host.start().await });
        tokio::task::yield_now().await;
        let second = host.start().await;
        let first = first.await.expect("first starter task");

        assert!(first.is_ok(), "one caller must start the host: {first:?}");
        assert!(matches!(second, Err(DesktopSyncHostError::AlreadyStarted)));
        host.shutdown().await.expect("host shutdown");
        fixture.close().await;
    }

    #[tokio::test]
    async fn host5_observer_starts_after_registration_and_targets_same_runtime() {
        let fixture = HostFixture::new().await;
        let executor = CountingExecutor::new(fixture.scope);
        let (watcher, source) = ManualChangeWatcher::with_capacity(8);
        let library = executor_library(&fixture, executor).with_watcher(
            Box::new(watcher),
            ObservationConfig::new(8, 8, 2, Duration::ZERO).expect("observer config"),
        );
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            host_config(),
            [library],
        )
        .await
        .expect("host construction");
        let identity = host.runtime_identity();
        let observer = host
            .observer(fixture.scope.library_id())
            .expect("registered observer");
        assert_eq!(observer.runtime_identity(), Some(identity));

        host.start().await.expect("host start");
        source
            .push(WatchHint::rescan_required())
            .expect("manual watcher hint");
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(
            fixture
                .state
                .observation_state(fixture.scope.library_id())
                .await
                .expect("observation state")
                .is_some()
        );
        host.shutdown().await.expect("host shutdown");
        fixture.close().await;
    }

    #[tokio::test]
    async fn host6_manual_network_and_credential_hints_are_runtime_wakes() {
        let fixture = HostFixture::new().await;
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            host_config(),
            [executor_library(
                &fixture,
                CountingExecutor::new(fixture.scope),
            )],
        )
        .await
        .expect("host construction");
        let library_id = fixture.scope.library_id();
        assert_eq!(
            host.sync_now(library_id),
            SyncRuntimeWakeResult::RuntimeStopped
        );
        host.start().await.expect("host start");
        assert!(host.sync_now(library_id).was_accepted());
        assert!(host.network_available()[0].1.was_accepted());
        assert!(host.credential_changed(library_id).was_accepted());
        host.shutdown().await.expect("host shutdown");
        assert_eq!(
            host.handle().sync_now(library_id),
            SyncRuntimeWakeResult::RuntimeStopped
        );
        assert_eq!(
            host.handle().network_available()[0].1,
            SyncRuntimeWakeResult::RuntimeStopped
        );
        fixture.close().await;
    }

    #[tokio::test]
    async fn host7_process_adapters_forward_shutdown_without_sync_policy() {
        let fixture = HostFixture::new().await;
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            host_config(),
            [executor_library(
                &fixture,
                CountingExecutor::new(fixture.scope),
            )],
        )
        .await
        .expect("host construction");
        let handle = host.start().await.expect("host start");
        let linux = LinuxLifecycleAdapter::new(handle.clone());
        let windows = WindowsNetworkAdapter::new(handle.clone());
        assert_eq!(linux.handle().runtime_identity(), host.runtime_identity());
        let _ = windows.available();
        linux
            .deliver(DesktopLifecycleEvent::ShutdownRequested)
            .await
            .expect("lifecycle shutdown");
        assert_eq!(host.lifecycle(), DesktopSyncHostLifecycle::Stopped);
        fixture.close().await;
    }

    #[tokio::test]
    async fn host8_duplicate_registration_and_terminal_unregister_are_typed() {
        let fixture = HostFixture::new().await;
        let executor = CountingExecutor::new(fixture.scope);
        let library = executor_library(&fixture, executor.clone());
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            host_config(),
            [library],
        )
        .await
        .expect("host construction");
        let duplicate = executor_library(&fixture, executor);
        assert_eq!(
            host.register_library(duplicate).await.expect("duplicate"),
            DesktopSyncLibraryRegistration::AlreadyRegistered
        );
        host.start().await.expect("host start");
        host.shutdown().await.expect("host shutdown");
        assert!(matches!(
            host.unregister_library(fixture.scope.library_id()).await,
            Err(DesktopSyncHostError::Stopped)
        ));
        fixture.close().await;
    }

    #[tokio::test]
    async fn host9_http_library_without_enrollment_is_auth_blocked_not_networked() {
        let fixture_directory = std::env::temp_dir().join(format!(
            "synveil-desktop-sync-host-http-{}",
            uuid::Uuid::now_v7()
        ));
        fs::create_dir_all(&fixture_directory).expect("fixture directory");
        let root = fixture_directory.join("root");
        fs::create_dir(&root).expect("fixture root");
        let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
        let profile = ServerProfile::new(
            CanonicalBaseUrl::parse_for_loopback_test("http://127.0.0.1:38194")
                .expect("loopback profile"),
            "host-test",
        )
        .expect("profile");
        let replica = Arc::new(
            FilesystemLocalReplica::initialize_for_profile(&root, scope, profile.profile_id())
                .expect("profiled replica"),
        );
        let state = Arc::new(
            LocalStateStore::open(&LocalStateConfig::new(
                fixture_directory.join("state.sqlite3"),
            ))
            .await
            .expect("local state"),
        );
        let library = DesktopSyncLibraryConfig::http(scope, profile, replica);
        let host = DesktopSyncHost::new(
            state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            host_config(),
            [library],
        )
        .await
        .expect("host construction");
        let mut events = host.events();
        host.start().await.expect("host start");
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if matches!(
                    events.recv().await.expect("runtime event"),
                    SyncRuntimeEvent::AuthBlocked { library_id }
                        if library_id == scope.library_id()
                ) {
                    break;
                }
            }
        })
        .await
        .expect("auth-blocked event");
        assert_eq!(
            host.status(scope.library_id())
                .expect("library status")
                .phase(),
            SyncRuntimeLibraryPhase::AuthBlocked
        );
        host.shutdown().await.expect("host shutdown");
        state.close_pool().await;
        drop(state);
        remove_dir_all_bounded(&fixture_directory).expect("fixture cleanup");
    }

    #[tokio::test]
    async fn host10_new_process_host_restarts_from_the_same_durable_state() {
        let fixture = HostFixture::new().await;
        let first = CountingExecutor::new(fixture.scope);
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            host_config(),
            [executor_library(&fixture, first)],
        )
        .await
        .expect("first host");
        host.start().await.expect("first start");
        host.shutdown().await.expect("first shutdown");
        drop(host);

        let second = CountingExecutor::new(fixture.scope);
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            host_config(),
            [executor_library(&fixture, second.clone())],
        )
        .await
        .expect("second host");
        assert_eq!(host.lifecycle(), DesktopSyncHostLifecycle::Constructed);
        let mut events = host.events();
        host.start().await.expect("second start");
        wait_for_cycle(&mut events, fixture.scope.library_id()).await;
        host.shutdown().await.expect("second shutdown");
        assert!(second.calls() >= 1);
        fixture.close().await;
    }

    #[tokio::test]
    async fn host11_join_is_the_safe_post_start_shutdown_path() {
        let fixture = HostFixture::new().await;
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            host_config(),
            [executor_library(
                &fixture,
                CountingExecutor::new(fixture.scope),
            )],
        )
        .await
        .expect("host construction");
        host.start().await.expect("host start");
        host.join().await.expect("host join");
        host.join().await.expect("idempotent host join");
        assert_eq!(host.lifecycle(), DesktopSyncHostLifecycle::Stopped);
        fixture.close().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host12_shutdown_waits_for_the_active_bounded_cycle_and_stops_new_wakes() {
        let fixture = HostFixture::new().await;
        let executor = BlockingExecutor::new(fixture.scope);
        let cycle_started = executor.started.notified();
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            host_config(),
            [executor_library(&fixture, executor.clone())],
        )
        .await
        .expect("host construction");
        host.start().await.expect("host start");
        tokio::time::timeout(Duration::from_secs(2), cycle_started)
            .await
            .expect("active cycle must start");

        let shutdown_host = host.clone();
        let shutdown = tokio::spawn(async move { shutdown_host.shutdown().await });
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if host.lifecycle() == DesktopSyncHostLifecycle::Stopping {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("shutdown must publish its stop boundary");
        assert_eq!(
            host.sync_now(fixture.scope.library_id()),
            SyncRuntimeWakeResult::RuntimeStopped
        );
        assert_eq!(
            host.network_available()[0].1,
            SyncRuntimeWakeResult::RuntimeStopped
        );
        executor.release.notify_one();
        shutdown
            .await
            .expect("shutdown task")
            .expect("graceful shutdown");
        assert_eq!(executor.calls(), 1);
        assert_eq!(host.lifecycle(), DesktopSyncHostLifecycle::Stopped);
        fixture.close().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host13_stop_during_start_has_one_deterministic_terminal_result() {
        let fixture = HostFixture::new().await;
        let (started_sender, started_receiver) = std::sync::mpsc::sync_channel(1);
        let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(1);
        let watcher = BlockingStartWatcher {
            started: started_sender,
            release: release_receiver,
        };
        let library = executor_library(&fixture, CountingExecutor::new(fixture.scope))
            .with_watcher(
                Box::new(watcher),
                ObservationConfig::new(8, 8, 2, Duration::ZERO).expect("observer config"),
            );
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            host_config(),
            [library],
        )
        .await
        .expect("host construction");
        let start_host = host.clone();
        let start = tokio::spawn(async move { start_host.start().await });
        started_receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("observer start must be reached");

        let shutdown_host = host.clone();
        let shutdown = tokio::spawn(async move { shutdown_host.shutdown().await });
        assert_eq!(
            host.sync_now(fixture.scope.library_id()),
            SyncRuntimeWakeResult::RuntimeStopped
        );
        release_sender.send(()).expect("release observer start");

        assert!(matches!(
            start.await.expect("start task"),
            Err(DesktopSyncHostError::StartCancelled)
        ));
        shutdown
            .await
            .expect("shutdown task")
            .expect("shutdown after cancelled start");
        assert_eq!(host.lifecycle(), DesktopSyncHostLifecycle::Stopped);
        assert!(host.runtime_handle().is_none());
        fixture.close().await;
    }

    #[tokio::test]
    async fn host14_startup_failure_rolls_back_runtime_and_observer_resources() {
        let fixture = HostFixture::new().await;
        let library = executor_library(&fixture, CountingExecutor::new(fixture.scope))
            .with_watcher(
                Box::new(FailingStartWatcher),
                ObservationConfig::new(8, 8, 2, Duration::ZERO).expect("observer config"),
            );
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            host_config(),
            [library],
        )
        .await
        .expect("host construction");
        assert!(matches!(
            host.start().await,
            Err(DesktopSyncHostError::Client(ClientSyncError::InvalidState))
        ));
        assert_eq!(host.lifecycle(), DesktopSyncHostLifecycle::Faulted);
        assert!(host.runtime_handle().is_none());
        host.shutdown().await.expect("faulted host shutdown");
        assert_eq!(host.lifecycle(), DesktopSyncHostLifecycle::Stopped);
        fixture.close().await;
    }

    #[tokio::test]
    async fn host17_durable_work_after_shutdown_is_not_lost_when_wake_is_rejected() {
        let fixture = HostFixture::new().await;
        let executor = CountingExecutor::new(fixture.scope);
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            host_config(),
            [executor_library(&fixture, executor)],
        )
        .await
        .expect("host construction");
        host.start().await.expect("host start");
        host.shutdown().await.expect("host shutdown");
        let root = fixture
            .state
            .local_nodes(fixture.scope.library_id())
            .await
            .expect("root nodes")
            .into_iter()
            .find(|node| node.parent_node_id().is_none())
            .expect("root node");
        let intent = OutboundIntent::new(
            fixture.scope.library_id(),
            None,
            Some(root.node_id()),
            OutboundIntentKind::CreateDirectory,
            ManagedRelativePath::new("after-shutdown").expect("relative path"),
            None,
            Some(LocalFingerprint::directory()),
            Sequence::new(0),
            Sequence::new(0),
            None,
            None,
            Some(Revision::new(0)),
        )
        .expect("intent shape");
        let result = host
            .outbound_intent_producer()
            .upsert(&intent)
            .await
            .expect("durable intent");
        assert_eq!(
            result.notification().durable_result(),
            DurableChangeResult::Committed
        );
        assert_eq!(
            result.notification().wake_result(),
            Some(SyncRuntimeWakeResult::RuntimeStopped)
        );
        assert!(
            fixture
                .state
                .outbound_intent(intent.intent_id())
                .await
                .expect("intent read")
                .is_some()
        );
        fixture.close().await;
    }

    #[tokio::test]
    async fn host18_credential_persistence_survives_shutdown_even_when_wake_is_closed() {
        let fixture = HostFixture::new().await;
        let profile = ServerProfile::new(
            CanonicalBaseUrl::parse("https://host-credential.example").expect("profile URL"),
            "host credential",
        )
        .expect("profile");
        fixture
            .state
            .save_server_profile(&profile)
            .await
            .expect("profile persistence");
        let secret_store = Arc::new(HostTestSecretStore::default());
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            secret_store.clone() as Arc<dyn SecretStore>,
            host_config(),
            [executor_library(
                &fixture,
                CountingExecutor::new(fixture.scope),
            )],
        )
        .await
        .expect("host construction");
        host.start().await.expect("host start");
        host.shutdown().await.expect("host shutdown");
        let enrollment = EnrollmentCredentials::for_test(
            profile.clone(),
            fixture.scope.owner_user_id(),
            fixture.scope.device_id(),
            DeviceCredentialId::new(),
            DeviceCredentialSecret::from_bytes([0x4d; 32]),
            Timestamp::parse("2026-09-13T00:00:00Z").expect("credential timestamp"),
        );
        let result = host
            .credential_controller()
            .store_enrollment(&enrollment, &[fixture.scope.library_id()])
            .await
            .expect("credential persistence");
        assert_eq!(
            result.wake_results()[0].1,
            SyncRuntimeWakeResult::RuntimeStopped
        );
        assert_eq!(
            fixture
                .state
                .profile_enrollment(profile.profile_id())
                .await
                .expect("enrollment read")
                .expect("enrollment")
                .credential_id(),
            enrollment.credential_id()
        );
        fixture.close().await;
    }

    #[tokio::test]
    async fn host24_dynamic_registration_starts_one_new_library_without_a_new_runtime() {
        let fixture = HostFixture::new().await;
        let first = CountingExecutor::new(fixture.scope);
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            host_config(),
            [executor_library(&fixture, first)],
        )
        .await
        .expect("host construction");
        let identity = host.runtime_identity();
        let mut events = host.events();
        host.start().await.expect("host start");
        wait_for_cycle(&mut events, fixture.scope.library_id()).await;

        let (second_scope, second_replica) = fixture.add_library("dynamic").await;
        let second = CountingExecutor::new(second_scope);
        assert_eq!(
            host.register_library(
                DesktopSyncLibraryConfig::from_executor(
                    second_scope,
                    second.clone(),
                    second_replica,
                )
                .expect("dynamic library")
            )
            .await
            .expect("dynamic registration"),
            DesktopSyncLibraryRegistration::Registered
        );
        wait_for_cycle(&mut events, second_scope.library_id()).await;
        assert_eq!(host.runtime_identity(), identity);
        assert_eq!(host.registered_libraries().len(), 2);
        assert!(second.calls() >= 1);
        assert_eq!(
            host.unregister_library(second_scope.library_id())
                .await
                .expect("dynamic unregister"),
            SyncRuntimeUnregistration::Unregistered
        );
        assert!(
            fixture
                .state
                .replica(second_scope.library_id())
                .await
                .expect("durable replica read")
                .is_some()
        );
        host.shutdown().await.expect("host shutdown");
        fixture.close().await;
    }

    #[tokio::test]
    async fn host29_one_thousand_start_shutdown_cycles_have_no_live_host_workers() {
        let fixture = HostFixture::new().await;
        for _ in 0..1_000 {
            let host = DesktopSyncHost::new(
                fixture.state.clone(),
                Arc::new(UnsupportedSecureSecretStore::new()),
                host_config(),
                [executor_library(
                    &fixture,
                    CountingExecutor::new(fixture.scope),
                )],
            )
            .await
            .expect("host construction");
            host.start().await.expect("host start");
            host.shutdown().await.expect("host shutdown");
            assert_eq!(host.lifecycle(), DesktopSyncHostLifecycle::Stopped);
        }
        fixture.close().await;
    }

    #[tokio::test]
    async fn host30_multiple_libraries_share_one_runtime_identity() {
        let fixture = HostFixture::new().await;
        let (second_scope, second_replica) = fixture.add_library("second").await;
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            host_config(),
            [
                executor_library(&fixture, CountingExecutor::new(fixture.scope)),
                DesktopSyncLibraryConfig::from_executor(
                    second_scope,
                    CountingExecutor::new(second_scope),
                    second_replica,
                )
                .expect("second library"),
            ],
        )
        .await
        .expect("host construction");
        let identity = host.runtime_identity();
        assert_eq!(host.registered_libraries().len(), 2);
        assert_eq!(host.handle().runtime_identity(), identity);
        host.start().await.expect("host start");
        assert_eq!(
            host.runtime_handle().expect("runtime handle").identity(),
            identity
        );
        assert_eq!(host.statuses().len(), 2);
        host.shutdown().await.expect("host shutdown");
        fixture.close().await;
    }

    #[tokio::test]
    async fn host31_rejects_cross_account_library_registration() {
        let fixture = HostFixture::new().await;
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            host_config(),
            [executor_library(
                &fixture,
                CountingExecutor::new(fixture.scope),
            )],
        )
        .await
        .expect("host construction");
        let (foreign_scope, foreign_replica) = fixture
            .add_library_for_scope("foreign", UserId::new(), DeviceId::new())
            .await;
        let result = host
            .register_library(
                DesktopSyncLibraryConfig::from_executor(
                    foreign_scope,
                    CountingExecutor::new(foreign_scope),
                    foreign_replica,
                )
                .expect("foreign library config"),
            )
            .await;
        assert!(matches!(
            result,
            Err(DesktopSyncHostError::Client(ClientSyncError::WrongScope))
        ));
        assert_eq!(host.registered_libraries().len(), 1);
        host.shutdown().await.expect("host shutdown");
        fixture.close().await;
    }

    #[tokio::test]
    async fn host32_missing_root_is_per_library_degraded_and_never_a_mass_delete() {
        let fixture = ProfiledHostFixture::new().await;
        fixture.move_root_away();
        assert!(!fixture.root.exists());
        let executor = CountingExecutor::new(fixture.scope);
        let library = DesktopSyncLibraryConfig::from_executor(
            fixture.scope,
            executor.clone(),
            fixture.deferred_replica(),
        )
        .expect("deferred executor library");
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            root_probe_config(),
            [library],
        )
        .await
        .expect("missing-root host construction");
        let mut events = host.events();
        assert_eq!(
            host.root_status(fixture.scope.library_id()),
            Some(RootAvailability::Unavailable)
        );
        host.start().await.expect("missing-root host start");
        wait_for_cycle_outcome(
            &mut events,
            fixture.scope.library_id(),
            SyncRuntimeOutcome::RootUnavailable,
        )
        .await;
        assert_eq!(
            host.status(fixture.scope.library_id())
                .expect("missing-root runtime status")
                .phase(),
            SyncRuntimeLibraryPhase::RootBlocked
        );
        assert_eq!(
            host.state()
                .count_pending_intents(fixture.scope.library_id())
                .await
                .expect("missing-root pending intents"),
            0
        );
        assert!(
            !fixture.root.exists(),
            "missing root must not be auto-created"
        );
        assert_eq!(
            host.sync_now(fixture.scope.library_id()),
            SyncRuntimeWakeResult::Queued
        );

        fixture.restore_root();
        wait_for_root_status(
            &host,
            fixture.scope.library_id(),
            RootAvailability::Available,
        )
        .await;
        wait_for_cycle(&mut events, fixture.scope.library_id()).await;
        assert!(
            executor.calls() >= 1,
            "root restoration must resume runtime work"
        );
        host.shutdown().await.expect("missing-root host shutdown");
        fixture.close().await;
    }

    #[tokio::test]
    async fn host33_root_loss_fences_queued_observation_without_losing_durable_work() {
        let fixture = ProfiledHostFixture::new().await;
        let sentinel = ManagedRelativePath::new("sentinel.txt").expect("sentinel path");
        fixture.seed_file(&sentinel).await;
        let root_node = fixture
            .state
            .local_nodes(fixture.scope.library_id())
            .await
            .expect("root nodes")
            .into_iter()
            .find(|node| node.parent_node_id().is_none())
            .expect("root node");
        let committed = OutboundIntent::new(
            fixture.scope.library_id(),
            None,
            Some(root_node.node_id()),
            OutboundIntentKind::CreateDirectory,
            ManagedRelativePath::new("already-committed").expect("committed path"),
            None,
            Some(LocalFingerprint::directory()),
            Sequence::new(0),
            Sequence::new(0),
            None,
            None,
            Some(Revision::new(1)),
        )
        .expect("committed intent");
        fixture
            .state
            .upsert_outbound_intent(&committed)
            .await
            .expect("durable committed intent");

        let (watcher, source) = ManualChangeWatcher::pair();
        let library = DesktopSyncLibraryConfig::from_executor(
            fixture.scope,
            CountingExecutor::new(fixture.scope),
            fixture.replica.clone(),
        )
        .expect("observed executor library")
        .with_watcher(
            Box::new(watcher),
            ObservationConfig::new(8, 8, 2, Duration::ZERO).expect("observer config"),
        );
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            root_probe_only_config(),
            [library],
        )
        .await
        .expect("root-loss host construction");
        host.start().await.expect("root-loss host start");
        let observer = host.observer(fixture.scope.library_id()).expect("observer");
        source
            .push(
                WatchHint::new(WatchHintKind::Remove, vec![sentinel.clone()])
                    .expect("queued remove hint"),
            )
            .expect("queue remove hint");
        fixture.move_root_away();

        let _ = observer.poll_once_with_notification().await;
        wait_for_root_status(
            &host,
            fixture.scope.library_id(),
            RootAvailability::Unavailable,
        )
        .await;
        let pending = host
            .state()
            .list_pending_intents(fixture.scope.library_id())
            .await
            .expect("root-loss pending intents");
        assert!(
            pending
                .iter()
                .all(|intent| intent.kind() != OutboundIntentKind::DeleteOrTrashNode),
            "root loss must not attribute a removal storm to the user"
        );
        assert!(
            host.state()
                .outbound_intent(committed.intent_id())
                .await
                .expect("committed intent read")
                .is_some(),
            "a pre-boundary durable intent must survive root loss"
        );
        fixture.restore_root();
        host.shutdown().await.expect("root-loss host shutdown");
        fixture.close().await;
    }

    #[tokio::test]
    async fn host34_root_reappearance_rescans_once_and_recovers_changes_during_outage() {
        let fixture = ProfiledHostFixture::new().await;
        fixture.move_root_away();
        let change = ManagedRelativePath::new("created-while-away.txt").expect("change path");
        fs::write(
            fixture.away_root.join(change.as_path()),
            b"created while away",
        )
        .expect("change while root is away");
        let (watcher, starts, _stops) = CountingWatcher::new();
        let executor = CountingExecutor::new(fixture.scope);
        let library = DesktopSyncLibraryConfig::from_executor(
            fixture.scope,
            executor,
            fixture.deferred_replica(),
        )
        .expect("recovery executor library")
        .with_watcher(
            Box::new(watcher),
            ObservationConfig::new(8, 8, 2, Duration::ZERO).expect("recovery observer config"),
        );
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            root_probe_config(),
            [library],
        )
        .await
        .expect("recovery host construction");
        host.start().await.expect("recovery host start");
        assert_eq!(
            host.root_status(fixture.scope.library_id()),
            Some(RootAvailability::Unavailable)
        );
        fixture.restore_root();
        wait_for_root_status(
            &host,
            fixture.scope.library_id(),
            RootAvailability::Available,
        )
        .await;
        wait_for_pending_intent(&host, fixture.scope.library_id()).await;
        assert_eq!(
            starts.load(Ordering::Acquire),
            1,
            "two recovery observations must not create duplicate watchers"
        );
        let pending = host
            .state()
            .list_pending_intents(fixture.scope.library_id())
            .await
            .expect("recovery pending intents");
        assert!(
            !host
                .state()
                .observation_issues(fixture.scope.library_id())
                .await
                .expect("recovery observation issues")
                .iter()
                .any(|issue| issue.kind() == crate::ObservationIssueKind::RootInvalid)
        );
        assert!(pending.iter().any(|intent| {
            intent.observed_relative_path() == &change
                && intent.kind() == OutboundIntentKind::CreateFile
        }));
        host.shutdown().await.expect("recovery host shutdown");
        fixture.close().await;
    }

    #[tokio::test]
    async fn host35_unavailable_library_does_not_stop_a_healthy_library() {
        let fixture = ProfiledHostFixture::new().await;
        fixture.move_root_away();
        let (healthy_scope, healthy_replica) = fixture.add_library("healthy").await;
        let unavailable_executor = CountingExecutor::new(fixture.scope);
        let healthy_executor = CountingExecutor::new(healthy_scope);
        let unavailable = DesktopSyncLibraryConfig::from_executor(
            fixture.scope,
            unavailable_executor,
            fixture.deferred_replica(),
        )
        .expect("unavailable library");
        let healthy = DesktopSyncLibraryConfig::from_executor(
            healthy_scope,
            healthy_executor.clone(),
            healthy_replica,
        )
        .expect("healthy library");
        let host = DesktopSyncHost::new(
            fixture.state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            root_probe_config(),
            [unavailable, healthy],
        )
        .await
        .expect("multi-library host construction");
        let mut events = host.events();
        host.start().await.expect("multi-library host start");
        wait_for_cycle_outcome(
            &mut events,
            fixture.scope.library_id(),
            SyncRuntimeOutcome::RootUnavailable,
        )
        .await;
        wait_for_cycle(&mut events, healthy_scope.library_id()).await;
        assert_eq!(
            host.root_status(fixture.scope.library_id()),
            Some(RootAvailability::Unavailable)
        );
        assert_eq!(
            host.root_status(healthy_scope.library_id()),
            Some(RootAvailability::Available)
        );
        assert!(healthy_executor.calls() >= 1);
        host.shutdown().await.expect("multi-library host shutdown");
        fixture.close().await;
    }

    #[test]
    fn host_readiness_gate_is_stable_and_safe_to_report() {
        assert_eq!(
            DESKTOP_SYNC_HOST_READINESS,
            "SYNVEIL_DESKTOP_SYNC_HOST_READY"
        );
    }
}
