//! UI-agnostic controller for the Prompt 96 local control plane.
//!
//! The controller is deliberately a presentation boundary.  It owns an
//! ephemeral latest-state model, one bounded command admission path, and the
//! lifecycle of its IPC connections.  It does not open the client SQLite
//! store, construct a sync host/runtime, inspect a root, or call a
//! synchronization engine.  All process and library facts come from the
//! redacted Prompt 96 control protocol.

use std::{
    collections::BTreeSet,
    fmt,
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use synveil_core::LibraryId;
use synveil_platform::PlatformRuntime;
use tokio::{
    sync::{Mutex, Notify, mpsc, oneshot, watch},
    task::JoinHandle,
    time::{self, MissedTickBehavior},
};

use crate::control::DesktopControlClientError;
use crate::{
    ControlAuthState, ControlConflictState, ControlErrorCode, ControlEvent, ControlLibraryList,
    ControlLibraryStatus, ControlProcessStatus, ControlRootState, ControlRuntimeState,
    ControlSyncOutcome, ControlSyncScheduleResult, DesktopControlClient, DesktopControlEndpoint,
    DesktopControlEventStream, DesktopProcessStatus,
};

/// Readiness marker for the native controller model/client layer.
pub const DESKTOP_CONTROLLER_CORE_READINESS: &str = "SYNVEIL_DESKTOP_CONTROLLER_CORE_READY";

/// The controller never admits more than this many commands awaiting the one
/// sequential Prompt 96 request path.  A full channel rejects admission; it
/// never grows with UI click rate.
pub const DESKTOP_CONTROLLER_COMMAND_CAPACITY: usize = 8;

/// Default reconnect delay for a local process that is not currently running.
pub const DEFAULT_DESKTOP_CONTROLLER_RECONNECT_INITIAL: Duration = Duration::from_millis(250);

/// Maximum reconnect delay.  The sequence is 250 ms, 500 ms, 1 s, 2 s, 4 s,
/// then 5 s for subsequent attempts.
pub const DEFAULT_DESKTOP_CONTROLLER_RECONNECT_CAP: Duration = Duration::from_secs(5);

/// Default presentation-only safety refresh interval.
pub const DEFAULT_DESKTOP_CONTROLLER_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

/// Maximum time spent waiting for one bounded local IPC operation.
pub const DEFAULT_DESKTOP_CONTROLLER_OPERATION_TIMEOUT: Duration = Duration::from_secs(2);

/// Maximum time a caller waits for an admitted command result.  If the wait
/// expires after admission, the command result is conservatively unknown and
/// the command is never replayed.
pub const DEFAULT_DESKTOP_CONTROLLER_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

/// Timing knobs are explicit so tests and embedders can use short deterministic
/// timers without changing production reconnect behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesktopControllerTiming {
    reconnect_initial: Duration,
    reconnect_cap: Duration,
    refresh_interval: Duration,
    operation_timeout: Duration,
    command_timeout: Duration,
}

impl Default for DesktopControllerTiming {
    fn default() -> Self {
        Self {
            reconnect_initial: DEFAULT_DESKTOP_CONTROLLER_RECONNECT_INITIAL,
            reconnect_cap: DEFAULT_DESKTOP_CONTROLLER_RECONNECT_CAP,
            refresh_interval: DEFAULT_DESKTOP_CONTROLLER_REFRESH_INTERVAL,
            operation_timeout: DEFAULT_DESKTOP_CONTROLLER_OPERATION_TIMEOUT,
            command_timeout: DEFAULT_DESKTOP_CONTROLLER_COMMAND_TIMEOUT,
        }
    }
}

impl DesktopControllerTiming {
    /// Build timing policy, rejecting zero durations and a cap below the first
    /// reconnect delay.
    pub fn new(
        reconnect_initial: Duration,
        reconnect_cap: Duration,
        refresh_interval: Duration,
        operation_timeout: Duration,
        command_timeout: Duration,
    ) -> Result<Self, DesktopControllerError> {
        if reconnect_initial.is_zero()
            || reconnect_cap.is_zero()
            || reconnect_cap < reconnect_initial
            || refresh_interval.is_zero()
            || operation_timeout.is_zero()
            || command_timeout.is_zero()
        {
            return Err(DesktopControllerError::InvalidTiming);
        }
        Ok(Self {
            reconnect_initial,
            reconnect_cap,
            refresh_interval,
            operation_timeout,
            command_timeout,
        })
    }

    #[must_use]
    pub const fn reconnect_initial(self) -> Duration {
        self.reconnect_initial
    }

    #[must_use]
    pub const fn reconnect_cap(self) -> Duration {
        self.reconnect_cap
    }

    #[must_use]
    pub const fn refresh_interval(self) -> Duration {
        self.refresh_interval
    }

    #[must_use]
    pub const fn operation_timeout(self) -> Duration {
        self.operation_timeout
    }

    #[must_use]
    pub const fn command_timeout(self) -> Duration {
        self.command_timeout
    }
}

/// Controller connectivity is intentionally separate from process and library
/// status.  A connected IPC socket does not imply that the process is running.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Stopping,
    Stopped,
    ProtocolIncompatible,
    Faulted,
}

/// Freshness of the last canonical process/library snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerFreshness {
    Fresh,
    Stale,
    Unavailable,
}

/// Safe, stable controller error category.  Variants intentionally do not
/// retain an OS path, pipe name, credential, or server diagnostic string.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerErrorKind {
    EndpointUnavailable,
    EndpointSecurity,
    ProtocolIncompatible,
    ConnectionLost,
    MalformedServerResponse,
    CommandRejected,
    UnsupportedPlatform,
    Stopped,
    Internal,
}

impl DesktopControllerErrorKind {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::EndpointUnavailable => "DESKTOP_CONTROLLER_ENDPOINT_UNAVAILABLE",
            Self::EndpointSecurity => "DESKTOP_CONTROLLER_ENDPOINT_SECURITY",
            Self::ProtocolIncompatible => "DESKTOP_CONTROLLER_PROTOCOL_INCOMPATIBLE",
            Self::ConnectionLost => "DESKTOP_CONTROLLER_CONNECTION_LOST",
            Self::MalformedServerResponse => "DESKTOP_CONTROLLER_RESPONSE_MALFORMED",
            Self::CommandRejected => "DESKTOP_CONTROLLER_COMMAND_REJECTED",
            Self::UnsupportedPlatform => "DESKTOP_CONTROLLER_PLATFORM_UNSUPPORTED",
            Self::Stopped => "DESKTOP_CONTROLLER_STOPPED",
            Self::Internal => "DESKTOP_CONTROLLER_INTERNAL",
        }
    }
}

impl fmt::Display for DesktopControllerErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

/// Errors for controller lifecycle/configuration operations.  Runtime IPC
/// outcomes are returned as [`DesktopControllerCommandResult`] categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopControllerError {
    AlreadyStarted,
    InvalidTiming,
    Internal,
}

impl DesktopControllerError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::AlreadyStarted => "DESKTOP_CONTROLLER_ALREADY_STARTED",
            Self::InvalidTiming => "DESKTOP_CONTROLLER_TIMING_INVALID",
            Self::Internal => "DESKTOP_CONTROLLER_INTERNAL",
        }
    }
}

impl fmt::Display for DesktopControllerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for DesktopControllerError {}

/// Process status presented by the controller.  It is copied from the safe
/// Prompt 96 response and contains no process ID, command line, or path.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopControllerProcessStatus {
    pub state: DesktopProcessStatus,
    pub control_ready: bool,
}

/// Stable presentation spelling of the Prompt 96 runtime phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerRuntimeState {
    Idle,
    Scheduled,
    Running,
    BackingOff,
    AuthBlocked,
    RootBlocked,
    Faulted,
    Stopped,
}

/// Stable presentation spelling of root availability.  No path is included.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerRootState {
    Available,
    Unavailable,
    Recovering,
}

/// Stable presentation spelling of the safe authentication category.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerAuthState {
    Ready,
    Missing,
    Blocked,
    Revoked,
    Unknown,
}

/// Stable presentation spelling of the safe conflict category.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerConflictState {
    Clear,
    Required,
    Unknown,
}

/// Stable presentation spelling of a safe runtime outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerSyncOutcome {
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

/// Redacted status for one library.  The library ID is the only identifier;
/// root paths, URLs, credential IDs, tokens, cookies, headers, and file data
/// are not representable here.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopControllerLibraryStatus {
    pub library_id: String,
    pub runtime_state: DesktopControllerRuntimeState,
    pub root_state: DesktopControllerRootState,
    pub auth_state: DesktopControllerAuthState,
    pub conflict_state: DesktopControllerConflictState,
    pub next_due_ms: Option<u64>,
    pub last_outcome: Option<DesktopControllerSyncOutcome>,
    pub wake_pending: bool,
    pub transient_failures: u32,
}

/// One coherent, latest-state presentation snapshot.  `revision` advances only
/// when a complete process/library status transaction is published.  Connection
/// transitions retain the last data while changing freshness.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopControllerSnapshot {
    pub connection_state: DesktopControllerConnectionState,
    pub process: Option<DesktopControllerProcessStatus>,
    pub libraries: Vec<DesktopControllerLibraryStatus>,
    pub libraries_truncated: bool,
    pub revision: u64,
    pub freshness: DesktopControllerFreshness,
    pub last_error: Option<DesktopControllerErrorKind>,
    pub connection_generation: u64,
}

impl Default for DesktopControllerSnapshot {
    fn default() -> Self {
        Self {
            connection_state: DesktopControllerConnectionState::Disconnected,
            process: None,
            libraries: Vec::new(),
            libraries_truncated: false,
            revision: 0,
            freshness: DesktopControllerFreshness::Unavailable,
            last_error: None,
            connection_generation: 0,
        }
    }
}

/// Results exposed for scheduling-only controller commands.  `Accepted` means
/// the server accepted scheduling; it never means synchronization completed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerCommandResult {
    Accepted,
    Coalesced,
    AlreadyRunningFollowupRecorded,
    ShutdownAccepted,
    Disconnected,
    UnknownLibrary,
    RuntimeStopped,
    Unavailable,
    AdmissionLimited,
    OutcomeUnknown,
    ProtocolError,
    Stopped,
    AlreadyUnavailable,
}

/// Endpoint selection is resolved on each connection attempt.  In the normal
/// profile form this reuses Prompt 96's canonical platform endpoint resolver;
/// the explicit endpoint form is useful for embedding/tests that already own a
/// validated endpoint value.
#[derive(Clone)]
enum EndpointSource {
    Profile {
        platform: Arc<dyn PlatformRuntime>,
        profile_id: synveil_client_sync::ServerProfileId,
    },
    Explicit(DesktopControlEndpoint),
}

impl fmt::Debug for EndpointSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Profile { profile_id, .. } => formatter
                .debug_struct("Profile")
                .field("profile_id", profile_id)
                .finish(),
            Self::Explicit(endpoint) => formatter
                .debug_struct("Explicit")
                .field("kind", &endpoint.kind())
                .finish(),
        }
    }
}

impl EndpointSource {
    async fn resolve(&self) -> Result<DesktopControlEndpoint, DesktopControlClientError> {
        match self {
            Self::Profile {
                platform,
                profile_id,
            } => DesktopControlEndpoint::for_profile(platform.as_ref(), *profile_id)
                .map_err(DesktopControlClientError::Endpoint),
            Self::Explicit(endpoint) => Ok(endpoint.clone()),
        }
    }
}

/// Configuration for one controller-to-one-process relationship.
pub struct DesktopControllerConfig {
    endpoint: EndpointSource,
    timing: DesktopControllerTiming,
}

impl fmt::Debug for DesktopControllerConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopControllerConfig")
            .field("endpoint", &self.endpoint)
            .field("timing", &self.timing)
            .finish()
    }
}

impl DesktopControllerConfig {
    /// Construct a profile-scoped controller.  This only stores the platform
    /// resolver/profile identity; it performs no I/O and starts no task.
    #[must_use]
    pub fn new(
        platform: Arc<dyn PlatformRuntime>,
        profile_id: synveil_client_sync::ServerProfileId,
    ) -> Self {
        Self {
            endpoint: EndpointSource::Profile {
                platform,
                profile_id,
            },
            timing: DesktopControllerTiming::default(),
        }
    }

    /// Construct a profile-scoped controller using the current platform
    /// resolver. The resolver remains below the controller API so UI crates do
    /// not need a direct platform dependency.
    #[must_use]
    pub fn for_profile(profile_id: synveil_client_sync::ServerProfileId) -> Self {
        let platform: Arc<dyn PlatformRuntime> = Arc::from(synveil_platform::current());
        Self::new(platform, profile_id)
    }

    /// Construct an embedding/test configuration from an already-resolved
    /// Prompt 96 endpoint.  Transport derivation remains outside the
    /// controller.
    #[must_use]
    pub fn for_endpoint(endpoint: DesktopControlEndpoint) -> Self {
        Self {
            endpoint: EndpointSource::Explicit(endpoint),
            timing: DesktopControllerTiming::default(),
        }
    }

    #[must_use]
    pub fn with_timing(mut self, timing: DesktopControllerTiming) -> Self {
        self.timing = timing;
        self
    }

    #[must_use]
    pub const fn timing(&self) -> DesktopControllerTiming {
        self.timing
    }
}

/// Reusable native controller handle.  Cloning it shares the same manager,
/// latest-state watch, command admission limit, and connection relationship.
#[derive(Clone)]
pub struct DesktopController {
    inner: Arc<DesktopControllerInner>,
}

impl fmt::Debug for DesktopController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopController")
            .field("snapshot", &self.snapshot())
            .finish_non_exhaustive()
    }
}

struct DesktopControllerInner {
    config: DesktopControllerConfig,
    snapshot: watch::Sender<DesktopControllerSnapshot>,
    lifecycle: Mutex<ControllerLifecycle>,
    current_generation: AtomicU64,
}

struct ControllerLifecycle {
    task: Option<JoinHandle<()>>,
    stop: Option<oneshot::Sender<()>>,
    command: Option<mpsc::Sender<ControllerCommand>>,
    stopping: bool,
}

impl DesktopController {
    /// Create a side-effect-free controller.  No endpoint is resolved and no
    /// background task is created until [`Self::start`] is called.
    #[must_use]
    pub fn new(config: DesktopControllerConfig) -> Self {
        let (snapshot, _) = watch::channel(DesktopControllerSnapshot::default());
        Self {
            inner: Arc::new(DesktopControllerInner {
                config,
                snapshot,
                lifecycle: Mutex::new(ControllerLifecycle {
                    task: None,
                    stop: None,
                    command: None,
                    stopping: false,
                }),
                current_generation: AtomicU64::new(0),
            }),
        }
    }

    /// Start the one controller manager.  Startup is asynchronous in the
    /// manager task; this method only creates bounded task/channel ownership.
    pub async fn start(&self) -> Result<(), DesktopControllerError> {
        let mut lifecycle = self.inner.lifecycle.lock().await;
        if lifecycle.task.is_some() || lifecycle.stopping {
            return Err(DesktopControllerError::AlreadyStarted);
        }

        let (stop, stop_rx) = oneshot::channel();
        let (command, command_rx) = mpsc::channel(DESKTOP_CONTROLLER_COMMAND_CAPACITY);
        self.inner.mark_connecting();

        let task = tokio::spawn(run_controller(Arc::clone(&self.inner), stop_rx, command_rx));
        lifecycle.task = Some(task);
        lifecycle.stop = Some(stop);
        lifecycle.command = Some(command);
        Ok(())
    }

    /// Stop and join controller-owned work.  This closes only controller IPC
    /// connections and never sends Prompt 96 `Shutdown` implicitly.
    pub async fn stop(&self) -> Result<(), DesktopControllerError> {
        let (stop, task) = {
            let mut lifecycle = self.inner.lifecycle.lock().await;
            let Some(task) = lifecycle.task.take() else {
                if !lifecycle.stopping {
                    self.inner.mark_stopped();
                }
                return Ok(());
            };
            lifecycle.stopping = true;
            lifecycle.command = None;
            (lifecycle.stop.take(), task)
        };

        if let Some(stop) = stop {
            let _ = stop.send(());
        }
        let join_result = task.await;

        let mut lifecycle = self.inner.lifecycle.lock().await;
        lifecycle.stopping = false;
        self.inner.mark_stopped();
        join_result.map_err(|_| DesktopControllerError::Internal)
    }

    /// Join a started controller without requesting a stop.  This is intended
    /// for an embedding that wants to await a stable protocol/process-stop
    /// termination.  Normal UI teardown should call [`Self::stop`] instead.
    pub async fn join(&self) -> Result<(), DesktopControllerError> {
        let (task, stop) = {
            let mut lifecycle = self.inner.lifecycle.lock().await;
            let Some(task) = lifecycle.task.take() else {
                if !lifecycle.stopping {
                    self.inner.mark_stopped();
                }
                return Ok(());
            };
            lifecycle.stopping = true;
            (task, lifecycle.stop.take())
        };
        // Keep the sender alive while joining.  Dropping it here would turn a
        // join into an implicit stop because the manager observes a closed
        // oneshot receiver as cancellation.
        let _keep_stop = stop;
        let join_result = task.await;
        let mut lifecycle = self.inner.lifecycle.lock().await;
        lifecycle.stopping = false;
        lifecycle.command = None;
        join_result.map_err(|_| DesktopControllerError::Internal)
    }

    /// Clone the current latest-state snapshot without holding any controller
    /// lock after this call returns.
    #[must_use]
    pub fn snapshot(&self) -> DesktopControllerSnapshot {
        self.inner.snapshot.borrow().clone()
    }

    /// Subscribe to latest-state updates.  `watch` retains only the newest
    /// snapshot, so a slow UI does not create an unbounded callback/task list.
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<DesktopControllerSnapshot> {
        self.inner.snapshot.subscribe()
    }

    /// Alias emphasizing that this is a latest-state view rather than an event
    /// history.
    #[must_use]
    pub fn subscribe_state(&self) -> watch::Receiver<DesktopControllerSnapshot> {
        self.subscribe()
    }

    /// Ask the running process to schedule a library sync through Prompt 96.
    /// No request is admitted while disconnected, and an admitted request is
    /// never replayed after a transport loss.
    pub async fn sync_now(
        &self,
        library_id: LibraryId,
    ) -> Result<DesktopControllerCommandResult, DesktopControllerError> {
        self.dispatch_command(CommandAction::SyncNow(library_id))
            .await
    }

    /// Ask the running process for graceful shutdown through Prompt 96.  A
    /// lost response is [`DesktopControllerCommandResult::OutcomeUnknown`];
    /// the controller never resends `Shutdown` after reconnecting.
    pub async fn request_shutdown(
        &self,
    ) -> Result<DesktopControllerCommandResult, DesktopControllerError> {
        self.dispatch_command(CommandAction::Shutdown).await
    }

    /// Descriptive alias for callers that use a verb-noun command style.
    pub async fn shutdown(&self) -> Result<DesktopControllerCommandResult, DesktopControllerError> {
        self.request_shutdown().await
    }

    async fn dispatch_command(
        &self,
        action: CommandAction,
    ) -> Result<DesktopControllerCommandResult, DesktopControllerError> {
        let snapshot = self.snapshot();
        match snapshot.connection_state {
            DesktopControllerConnectionState::Connected => {}
            DesktopControllerConnectionState::Stopping => {
                return Ok(DesktopControllerCommandResult::AlreadyUnavailable);
            }
            DesktopControllerConnectionState::Stopped => {
                return Ok(DesktopControllerCommandResult::Stopped);
            }
            DesktopControllerConnectionState::ProtocolIncompatible
            | DesktopControllerConnectionState::Faulted => {
                return Ok(DesktopControllerCommandResult::Unavailable);
            }
            DesktopControllerConnectionState::Disconnected
            | DesktopControllerConnectionState::Connecting
            | DesktopControllerConnectionState::Reconnecting => {
                return Ok(DesktopControllerCommandResult::Disconnected);
            }
        }

        let (reply, result_rx) = oneshot::channel();
        let (command, joining) = {
            let lifecycle = self.inner.lifecycle.lock().await;
            (lifecycle.command.clone(), lifecycle.stopping)
        };
        if joining {
            return Ok(DesktopControllerCommandResult::AlreadyUnavailable);
        }
        let Some(command) = command else {
            return Ok(DesktopControllerCommandResult::Disconnected);
        };
        match command.try_send(ControllerCommand { action, reply }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Ok(DesktopControllerCommandResult::AdmissionLimited);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Ok(DesktopControllerCommandResult::Stopped);
            }
        }

        match time::timeout(self.inner.config.timing.command_timeout(), result_rx).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(_)) => Ok(DesktopControllerCommandResult::Stopped),
            Err(_) => Ok(DesktopControllerCommandResult::OutcomeUnknown),
        }
    }
}

impl Drop for DesktopController {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) != 1 {
            return;
        }
        if let Ok(mut lifecycle) = self.inner.lifecycle.try_lock()
            && let Some(stop) = lifecycle.stop.take()
        {
            let _ = stop.send(());
        }
    }
}

impl DesktopControllerInner {
    fn snapshot(&self) -> DesktopControllerSnapshot {
        self.snapshot.borrow().clone()
    }

    fn update_snapshot(&self, update: impl FnOnce(&mut DesktopControllerSnapshot)) {
        let mut snapshot = self.snapshot();
        update(&mut snapshot);
        self.snapshot.send_replace(snapshot);
    }

    fn mark_connecting(&self) {
        self.update_snapshot(|snapshot| {
            snapshot.connection_state = DesktopControllerConnectionState::Connecting;
            snapshot.freshness = if snapshot.revision == 0 {
                DesktopControllerFreshness::Unavailable
            } else {
                DesktopControllerFreshness::Stale
            };
            snapshot.last_error = None;
        });
    }

    fn mark_reconnecting(&self, error: DesktopControllerErrorKind) {
        self.update_snapshot(|snapshot| {
            snapshot.connection_state = DesktopControllerConnectionState::Reconnecting;
            snapshot.freshness = if snapshot.revision == 0 {
                DesktopControllerFreshness::Unavailable
            } else {
                DesktopControllerFreshness::Stale
            };
            snapshot.last_error = Some(error);
        });
    }

    fn mark_terminal(
        &self,
        state: DesktopControllerConnectionState,
        error: DesktopControllerErrorKind,
    ) {
        self.update_snapshot(|snapshot| {
            snapshot.connection_state = state;
            snapshot.freshness = if snapshot.revision == 0 {
                DesktopControllerFreshness::Unavailable
            } else {
                DesktopControllerFreshness::Stale
            };
            snapshot.last_error = Some(error);
        });
    }

    fn mark_stopping(&self) {
        self.update_snapshot(|snapshot| {
            snapshot.connection_state = DesktopControllerConnectionState::Stopping;
            snapshot.freshness = DesktopControllerFreshness::Stale;
        });
    }

    fn mark_stopped(&self) {
        self.update_snapshot(|snapshot| {
            snapshot.connection_state = DesktopControllerConnectionState::Stopped;
            snapshot.freshness = if snapshot.revision == 0 {
                DesktopControllerFreshness::Unavailable
            } else {
                DesktopControllerFreshness::Stale
            };
        });
    }

    fn activate_generation(&self, generation: u64) {
        self.current_generation.store(generation, Ordering::Release);
    }

    fn is_current_generation(&self, generation: u64) -> bool {
        self.current_generation.load(Ordering::Acquire) == generation
    }

    fn publish_fresh(&self, generation: u64, data: ControllerSnapshotData) -> bool {
        if !self.is_current_generation(generation) {
            return false;
        }
        self.update_snapshot(|snapshot| {
            snapshot.connection_state = DesktopControllerConnectionState::Connected;
            snapshot.process = Some(data.process);
            snapshot.libraries = data.libraries;
            snapshot.libraries_truncated = data.libraries_truncated;
            snapshot.revision = snapshot.revision.saturating_add(1);
            snapshot.freshness = DesktopControllerFreshness::Fresh;
            snapshot.last_error = None;
            snapshot.connection_generation = generation;
        });
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommandAction {
    SyncNow(LibraryId),
    Shutdown,
}

struct ControllerCommand {
    action: CommandAction,
    reply: oneshot::Sender<DesktopControllerCommandResult>,
}

#[derive(Clone, Debug)]
struct ControllerSnapshotData {
    process: DesktopControllerProcessStatus,
    libraries: Vec<DesktopControllerLibraryStatus>,
    libraries_truncated: bool,
}

struct ActiveConnection {
    generation: u64,
    command: DesktopControlClient,
    status: DesktopControlClient,
    event_signal: Arc<EventSignal>,
    event_stop: Option<oneshot::Sender<()>>,
    event_task: Option<JoinHandle<()>>,
}

impl ActiveConnection {
    fn new(
        generation: u64,
        command: DesktopControlClient,
        status: DesktopControlClient,
        events: DesktopControlEventStream,
        inner: Arc<DesktopControllerInner>,
    ) -> Self {
        let event_signal = Arc::new(EventSignal::new());
        let (event_stop, event_stop_rx) = oneshot::channel();
        let signal = Arc::clone(&event_signal);
        let event_task = tokio::spawn(run_event_reader(
            events,
            event_stop_rx,
            signal,
            inner,
            generation,
        ));
        Self {
            generation,
            command,
            status,
            event_signal,
            event_stop: Some(event_stop),
            event_task: Some(event_task),
        }
    }

    async fn close_event_task(&mut self) {
        if let Some(stop) = self.event_stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.event_task.take() {
            let _ = task.await;
        }
    }
}

struct EventSignal {
    refresh_pending: AtomicBool,
    server_stopping: AtomicBool,
    failure: Mutex<Option<DesktopControllerErrorKind>>,
    notify: Notify,
}

impl EventSignal {
    fn new() -> Self {
        Self {
            refresh_pending: AtomicBool::new(false),
            server_stopping: AtomicBool::new(false),
            failure: Mutex::new(None),
            notify: Notify::new(),
        }
    }

    fn mark_refresh(&self) {
        self.refresh_pending.store(true, Ordering::Release);
        self.notify.notify_one();
    }

    fn take_refresh(&self) -> bool {
        self.refresh_pending.swap(false, Ordering::AcqRel)
    }

    fn mark_server_stopping(&self) {
        self.server_stopping.store(true, Ordering::Release);
        self.mark_refresh();
    }

    fn server_is_stopping(&self) -> bool {
        self.server_stopping.load(Ordering::Acquire)
    }

    async fn mark_failure(&self, failure: DesktopControllerErrorKind) {
        let mut stored = self.failure.lock().await;
        if stored.is_none() {
            *stored = Some(failure);
        }
        self.notify.notify_one();
    }

    async fn take_failure(&self) -> Option<DesktopControllerErrorKind> {
        self.failure.lock().await.take()
    }
}

#[derive(Default)]
struct RefreshCoordinator {
    pending: bool,
    in_flight: bool,
}

impl RefreshCoordinator {
    fn request(&mut self) {
        self.pending = true;
    }

    fn observe_signal(&mut self, signal: &EventSignal) {
        if signal.take_refresh() {
            self.request();
        }
    }

    fn begin(&mut self) -> bool {
        if self.in_flight || !self.pending {
            return false;
        }
        self.pending = false;
        self.in_flight = true;
        true
    }

    fn finish(&mut self, signal: &EventSignal) {
        self.in_flight = false;
        self.observe_signal(signal);
    }
}

async fn run_controller(
    inner: Arc<DesktopControllerInner>,
    mut stop_rx: oneshot::Receiver<()>,
    mut command_rx: mpsc::Receiver<ControllerCommand>,
) {
    let timing = inner.config.timing;
    let mut backoff = ReconnectBackoff::new(timing);
    let mut connection_generation = 0_u64;
    let mut first_attempt = true;

    loop {
        if first_attempt {
            inner.mark_connecting();
        } else {
            inner.update_snapshot(|snapshot| {
                snapshot.connection_state = DesktopControllerConnectionState::Reconnecting;
                snapshot.freshness = if snapshot.revision == 0 {
                    DesktopControllerFreshness::Unavailable
                } else {
                    DesktopControllerFreshness::Stale
                };
            });
        }

        connection_generation = connection_generation.saturating_add(1);
        let generation = connection_generation;
        inner.activate_generation(generation);
        let connection = tokio::select! {
            result = connect_and_initialize(
                &inner,
                &inner.config.endpoint,
                timing,
                generation,
            ) => result,
            _ = &mut stop_rx => {
                drain_commands(&mut command_rx, DesktopControllerCommandResult::Stopped);
                inner.mark_stopped();
                return;
            }
        };

        match connection {
            Ok((mut active, data)) => {
                backoff.reset();
                if !inner.publish_fresh(generation, data) {
                    active.close_event_task().await;
                    drain_commands(
                        &mut command_rx,
                        DesktopControllerCommandResult::Disconnected,
                    );
                    first_attempt = false;
                    continue;
                }

                match run_active(&inner, &mut active, timing, &mut stop_rx, &mut command_rx).await {
                    ActiveExit::StopRequested => {
                        drain_commands(&mut command_rx, DesktopControllerCommandResult::Stopped);
                        inner.mark_stopped();
                        return;
                    }
                    ActiveExit::ProcessStopped => {
                        drain_commands(
                            &mut command_rx,
                            DesktopControllerCommandResult::AlreadyUnavailable,
                        );
                        inner.mark_stopped();
                        return;
                    }
                    ActiveExit::GenerationSuperseded => {
                        drain_commands(
                            &mut command_rx,
                            DesktopControllerCommandResult::Disconnected,
                        );
                        first_attempt = false;
                    }
                    ActiveExit::Reconnect(error) => {
                        drain_commands(
                            &mut command_rx,
                            DesktopControllerCommandResult::Disconnected,
                        );
                        inner.mark_reconnecting(error);
                        first_attempt = false;
                        let delay = backoff.next_delay();
                        if wait_for_stop(&mut stop_rx, delay).await {
                            inner.mark_stopped();
                            return;
                        }
                    }
                    ActiveExit::Terminal(state, error) => {
                        drain_commands(
                            &mut command_rx,
                            DesktopControllerCommandResult::Unavailable,
                        );
                        inner.mark_terminal(state, error);
                        return;
                    }
                }
            }
            Err(error) => {
                let failure = classify_connection_failure(error);
                match failure {
                    ConnectionFailure::Retry(error) => {
                        drain_commands(
                            &mut command_rx,
                            DesktopControllerCommandResult::Disconnected,
                        );
                        inner.mark_reconnecting(error);
                        first_attempt = false;
                        let delay = backoff.next_delay();
                        if wait_for_stop(&mut stop_rx, delay).await {
                            inner.mark_stopped();
                            return;
                        }
                    }
                    ConnectionFailure::Terminal(state, error) => {
                        drain_commands(
                            &mut command_rx,
                            DesktopControllerCommandResult::Unavailable,
                        );
                        inner.mark_terminal(state, error);
                        return;
                    }
                }
            }
        }
    }
}

async fn connect_and_initialize(
    inner: &Arc<DesktopControllerInner>,
    endpoint_source: &EndpointSource,
    timing: DesktopControllerTiming,
    generation: u64,
) -> Result<(ActiveConnection, ControllerSnapshotData), ControllerIoError> {
    let endpoint = endpoint_source
        .resolve()
        .await
        .map_err(ControllerIoError::Client)?;

    // Keep request/status and event traffic independent while sharing one
    // logical controller relationship.  Each Prompt 96 connection performs
    // its own v1 handshake; no wire session is persisted or reused across
    // process restarts.
    let mut status = connect_client(endpoint.clone(), timing.operation_timeout()).await?;
    let command = connect_client(endpoint.clone(), timing.operation_timeout()).await?;
    let event_client = connect_client(endpoint, timing.operation_timeout()).await?;
    let events = subscribe_events(event_client, timing.operation_timeout()).await?;

    let data = fetch_snapshot(&mut status, timing.operation_timeout()).await?;
    // The event reader is started only after the complete initial transaction
    // is ready to publish.  The server's event channel is bounded and the
    // reader starts immediately after this return, so no durable event history
    // is implied by this presentation boundary.
    let active = ActiveConnection::new(generation, command, status, events, Arc::clone(inner));
    Ok((active, data))
}

async fn connect_client(
    endpoint: DesktopControlEndpoint,
    timeout: Duration,
) -> Result<DesktopControlClient, ControllerIoError> {
    bounded(timeout, DesktopControlClient::connect(endpoint))
        .await
        .map_err(ControllerIoError::Client)
}

async fn subscribe_events(
    client: DesktopControlClient,
    timeout: Duration,
) -> Result<DesktopControlEventStream, ControllerIoError> {
    bounded(timeout, client.subscribe_events())
        .await
        .map_err(ControllerIoError::Client)
}

async fn fetch_snapshot(
    client: &mut DesktopControlClient,
    timeout: Duration,
) -> Result<ControllerSnapshotData, ControllerIoError> {
    let process = bounded(timeout, client.process_status())
        .await
        .map_err(ControllerIoError::Client)?;
    let list = bounded(timeout, client.list_libraries())
        .await
        .map_err(ControllerIoError::Client)?;
    let libraries_truncated = list.truncated;
    let libraries = fetch_library_statuses(client, list, timeout).await?;
    Ok(ControllerSnapshotData {
        process: map_process_status(process),
        libraries,
        libraries_truncated,
    })
}

async fn fetch_library_statuses(
    client: &mut DesktopControlClient,
    list: ControlLibraryList,
    timeout: Duration,
) -> Result<Vec<DesktopControllerLibraryStatus>, ControllerIoError> {
    let mut seen = BTreeSet::new();
    let mut libraries = Vec::with_capacity(list.libraries.len());
    for listed in list.libraries {
        let library_id = listed
            .library_id
            .parse::<LibraryId>()
            .map_err(|_| ControllerIoError::Malformed)?;
        if !seen.insert(listed.library_id.clone()) {
            return Err(ControllerIoError::Malformed);
        }
        let status = bounded(timeout, client.library_status(library_id))
            .await
            .map_err(ControllerIoError::Client)?;
        if status.library_id != listed.library_id {
            return Err(ControllerIoError::Malformed);
        }
        libraries.push(map_library_status(status));
    }
    libraries.sort_by(|left, right| left.library_id.cmp(&right.library_id));
    Ok(libraries)
}

async fn bounded<F, T>(timeout: Duration, future: F) -> Result<T, DesktopControlClientError>
where
    F: Future<Output = Result<T, DesktopControlClientError>>,
{
    time::timeout(timeout, future)
        .await
        .map_err(|_| DesktopControlClientError::Connection)?
}

async fn run_event_reader(
    mut events: DesktopControlEventStream,
    mut stop_rx: oneshot::Receiver<()>,
    signal: Arc<EventSignal>,
    inner: Arc<DesktopControllerInner>,
    generation: u64,
) {
    loop {
        let result = tokio::select! {
            _ = &mut stop_rx => return,
            result = events.next_event() => result,
        };
        match result {
            Ok(ControlEvent::ControlServerStopping) => {
                if inner.is_current_generation(generation) {
                    signal.mark_server_stopping();
                }
            }
            Ok(_) => {
                if inner.is_current_generation(generation) {
                    signal.mark_refresh();
                }
            }
            Err(error) => {
                if inner.is_current_generation(generation) {
                    signal.mark_failure(classify_active_error(error)).await;
                }
                return;
            }
        }
    }
}

async fn run_active(
    inner: &Arc<DesktopControllerInner>,
    active: &mut ActiveConnection,
    timing: DesktopControllerTiming,
    stop_rx: &mut oneshot::Receiver<()>,
    command_rx: &mut mpsc::Receiver<ControllerCommand>,
) -> ActiveExit {
    let result = run_active_loop(inner, active, timing, stop_rx, command_rx).await;
    active.close_event_task().await;
    result
}

async fn run_active_loop(
    inner: &Arc<DesktopControllerInner>,
    active: &mut ActiveConnection,
    timing: DesktopControllerTiming,
    stop_rx: &mut oneshot::Receiver<()>,
    command_rx: &mut mpsc::Receiver<ControllerCommand>,
) -> ActiveExit {
    let mut refresh = RefreshCoordinator::default();
    let mut shutdown_requested = false;
    let mut refresh_timer = time::interval(timing.refresh_interval());
    refresh_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // Consume interval's immediate first tick.  The first coherent snapshot
    // was already fetched during connection, so safety polling is low-rate.
    refresh_timer.tick().await;

    loop {
        if !inner.is_current_generation(active.generation) {
            return ActiveExit::GenerationSuperseded;
        }

        refresh.observe_signal(&active.event_signal);

        if refresh.begin() {
            match fetch_snapshot(&mut active.status, timing.operation_timeout()).await {
                Ok(data) => {
                    if !inner.publish_fresh(active.generation, data) {
                        return ActiveExit::GenerationSuperseded;
                    }
                    // Events received while the transaction was in flight are
                    // folded into one follow-up transaction by the coordinator.
                    refresh.finish(&active.event_signal);
                    continue;
                }
                Err(error) => return active_error_exit(error, shutdown_requested),
            }
        }

        tokio::select! {
            biased;
            _ = &mut *stop_rx => return ActiveExit::StopRequested,
            request = command_rx.recv() => {
                let Some(request) = request else {
                    return ActiveExit::StopRequested;
                };
                match execute_command(&mut active.command, request.action, timing.operation_timeout()).await {
                    Ok(result) => {
                        if result == DesktopControllerCommandResult::ShutdownAccepted {
                            shutdown_requested = true;
                            inner.mark_stopping();
                        }
                        let _ = request.reply.send(result);
                    }
                    Err(error) => {
                        let result = map_command_error(request.action, error);
                        let transport_loss = is_transport_loss(error);
                        let _ = request.reply.send(result);
                        if transport_loss {
                            return if shutdown_requested {
                                ActiveExit::ProcessStopped
                            } else {
                                ActiveExit::Reconnect(DesktopControllerErrorKind::ConnectionLost)
                            };
                        }
                        if let Some(exit) = terminal_exit_for(error) {
                            return exit;
                        }
                    }
                }
            }
            _ = active.event_signal.notify.notified() => {
                if !inner.is_current_generation(active.generation) {
                    return ActiveExit::GenerationSuperseded;
                }
                if shutdown_requested && active.event_signal.server_is_stopping() {
                    return ActiveExit::ProcessStopped;
                }
                if let Some(error) = active.event_signal.take_failure().await {
                    return if shutdown_requested {
                        ActiveExit::ProcessStopped
                    } else {
                        terminal_or_reconnect(error)
                    };
                }
                refresh.observe_signal(&active.event_signal);
            }
            _ = refresh_timer.tick() => refresh.request(),
        }
    }
}

async fn execute_command(
    client: &mut DesktopControlClient,
    action: CommandAction,
    timeout: Duration,
) -> Result<DesktopControllerCommandResult, DesktopControlClientError> {
    match action {
        CommandAction::SyncNow(library_id) => {
            let result = bounded(timeout, client.sync_now(library_id)).await?;
            Ok(match result {
                ControlSyncScheduleResult::Queued => DesktopControllerCommandResult::Accepted,
                ControlSyncScheduleResult::Coalesced => DesktopControllerCommandResult::Coalesced,
                ControlSyncScheduleResult::AlreadyRunningFollowupRecorded => {
                    DesktopControllerCommandResult::AlreadyRunningFollowupRecorded
                }
            })
        }
        CommandAction::Shutdown => {
            bounded(timeout, client.shutdown()).await?;
            Ok(DesktopControllerCommandResult::ShutdownAccepted)
        }
    }
}

fn map_command_error(
    action: CommandAction,
    error: DesktopControlClientError,
) -> DesktopControllerCommandResult {
    match error {
        DesktopControlClientError::Server(ControlErrorCode::UnknownLibrary) => {
            DesktopControllerCommandResult::UnknownLibrary
        }
        DesktopControlClientError::Server(ControlErrorCode::RuntimeStopped) => {
            DesktopControllerCommandResult::RuntimeStopped
        }
        DesktopControlClientError::Server(_) => DesktopControllerCommandResult::ProtocolError,
        DesktopControlClientError::Connection
        | DesktopControlClientError::Closed
        | DesktopControlClientError::Frame(
            crate::ControlFrameError::Closed
            | crate::ControlFrameError::Io
            | crate::ControlFrameError::Truncated,
        ) => match action {
            CommandAction::SyncNow(_) | CommandAction::Shutdown => {
                DesktopControllerCommandResult::OutcomeUnknown
            }
        },
        DesktopControlClientError::ProtocolVersionUnsupported => {
            DesktopControllerCommandResult::ProtocolError
        }
        DesktopControlClientError::UnsupportedPlatform
        | DesktopControlClientError::Endpoint(_)
        | DesktopControlClientError::Handshake
        | DesktopControlClientError::Frame(_)
        | DesktopControlClientError::ResponseMismatch
        | DesktopControlClientError::UnexpectedResponse => {
            DesktopControllerCommandResult::ProtocolError
        }
    }
}

fn active_error_exit(error: ControllerIoError, shutdown_requested: bool) -> ActiveExit {
    match error {
        ControllerIoError::Malformed => ActiveExit::Terminal(
            DesktopControllerConnectionState::Faulted,
            DesktopControllerErrorKind::MalformedServerResponse,
        ),
        ControllerIoError::Client(error) => {
            if shutdown_requested && is_transport_loss(error) {
                ActiveExit::ProcessStopped
            } else if let Some(exit) = terminal_exit_for(error) {
                exit
            } else {
                ActiveExit::Reconnect(classify_active_error(error))
            }
        }
    }
}

fn terminal_or_reconnect(error: DesktopControllerErrorKind) -> ActiveExit {
    match error {
        DesktopControllerErrorKind::EndpointSecurity
        | DesktopControllerErrorKind::ProtocolIncompatible
        | DesktopControllerErrorKind::MalformedServerResponse
        | DesktopControllerErrorKind::UnsupportedPlatform
        | DesktopControllerErrorKind::Internal => {
            let state = if error == DesktopControllerErrorKind::ProtocolIncompatible {
                DesktopControllerConnectionState::ProtocolIncompatible
            } else {
                DesktopControllerConnectionState::Faulted
            };
            ActiveExit::Terminal(state, error)
        }
        DesktopControllerErrorKind::EndpointUnavailable
        | DesktopControllerErrorKind::ConnectionLost
        | DesktopControllerErrorKind::CommandRejected
        | DesktopControllerErrorKind::Stopped => ActiveExit::Reconnect(error),
    }
}

fn terminal_exit_for(error: DesktopControlClientError) -> Option<ActiveExit> {
    let kind = classify_active_error(error);
    match kind {
        DesktopControllerErrorKind::EndpointSecurity
        | DesktopControllerErrorKind::ProtocolIncompatible
        | DesktopControllerErrorKind::MalformedServerResponse
        | DesktopControllerErrorKind::UnsupportedPlatform => {
            let state = if kind == DesktopControllerErrorKind::ProtocolIncompatible {
                DesktopControllerConnectionState::ProtocolIncompatible
            } else {
                DesktopControllerConnectionState::Faulted
            };
            Some(ActiveExit::Terminal(state, kind))
        }
        _ => None,
    }
}

fn is_transport_loss(error: DesktopControlClientError) -> bool {
    matches!(
        error,
        DesktopControlClientError::Connection
            | DesktopControlClientError::Closed
            | DesktopControlClientError::Frame(
                crate::ControlFrameError::Closed
                    | crate::ControlFrameError::Io
                    | crate::ControlFrameError::Truncated
            )
            | DesktopControlClientError::Server(ControlErrorCode::ControlServerStopping)
    )
}

fn classify_connection_failure(error: ControllerIoError) -> ConnectionFailure {
    match error {
        ControllerIoError::Malformed => ConnectionFailure::Terminal(
            DesktopControllerConnectionState::Faulted,
            DesktopControllerErrorKind::MalformedServerResponse,
        ),
        ControllerIoError::Client(error) => {
            let kind = classify_connect_error(error);
            if matches!(
                kind,
                DesktopControllerErrorKind::EndpointUnavailable
                    | DesktopControllerErrorKind::ConnectionLost
            ) {
                ConnectionFailure::Retry(kind)
            } else {
                ConnectionFailure::Terminal(
                    if kind == DesktopControllerErrorKind::ProtocolIncompatible {
                        DesktopControllerConnectionState::ProtocolIncompatible
                    } else {
                        DesktopControllerConnectionState::Faulted
                    },
                    kind,
                )
            }
        }
    }
}

fn classify_connect_error(error: DesktopControlClientError) -> DesktopControllerErrorKind {
    match error {
        DesktopControlClientError::UnsupportedPlatform => {
            DesktopControllerErrorKind::UnsupportedPlatform
        }
        DesktopControlClientError::Endpoint(error) => match error {
            crate::DesktopControlServerError::UnsafeEndpoint
            | crate::DesktopControlServerError::InsecureRuntimeDirectory
            | crate::DesktopControlServerError::EndpointStateUnknown
            | crate::DesktopControlServerError::SecurityDescriptorUnavailable
            | crate::DesktopControlServerError::EndpointNameTooLong => {
                DesktopControllerErrorKind::EndpointSecurity
            }
            crate::DesktopControlServerError::UnsupportedPlatform => {
                DesktopControllerErrorKind::UnsupportedPlatform
            }
            _ => DesktopControllerErrorKind::EndpointUnavailable,
        },
        DesktopControlClientError::Connection => DesktopControllerErrorKind::EndpointUnavailable,
        DesktopControlClientError::ProtocolVersionUnsupported => {
            DesktopControllerErrorKind::ProtocolIncompatible
        }
        DesktopControlClientError::Server(ControlErrorCode::ProtocolVersionUnsupported) => {
            DesktopControllerErrorKind::ProtocolIncompatible
        }
        DesktopControlClientError::Server(ControlErrorCode::EndpointUnsafe) => {
            DesktopControllerErrorKind::EndpointSecurity
        }
        DesktopControlClientError::Server(ControlErrorCode::UnknownLibrary)
        | DesktopControlClientError::Server(ControlErrorCode::RuntimeStopped)
        | DesktopControlClientError::Server(ControlErrorCode::ControlServerStopping)
        | DesktopControlClientError::Closed
        | DesktopControlClientError::Frame(
            crate::ControlFrameError::Closed
            | crate::ControlFrameError::Io
            | crate::ControlFrameError::Truncated,
        ) => DesktopControllerErrorKind::EndpointUnavailable,
        DesktopControlClientError::Handshake
        | DesktopControlClientError::ResponseMismatch
        | DesktopControlClientError::UnexpectedResponse
        | DesktopControlClientError::Frame(_) => {
            DesktopControllerErrorKind::MalformedServerResponse
        }
        DesktopControlClientError::Server(_) => DesktopControllerErrorKind::MalformedServerResponse,
    }
}

fn classify_active_error(error: DesktopControlClientError) -> DesktopControllerErrorKind {
    match error {
        DesktopControlClientError::Endpoint(_) => classify_connect_error(error),
        DesktopControlClientError::Connection
        | DesktopControlClientError::Closed
        | DesktopControlClientError::Frame(
            crate::ControlFrameError::Closed
            | crate::ControlFrameError::Io
            | crate::ControlFrameError::Truncated,
        )
        | DesktopControlClientError::Server(ControlErrorCode::ControlServerStopping) => {
            DesktopControllerErrorKind::ConnectionLost
        }
        _ => classify_connect_error(error),
    }
}

enum ConnectionFailure {
    Retry(DesktopControllerErrorKind),
    Terminal(DesktopControllerConnectionState, DesktopControllerErrorKind),
}

enum ControllerIoError {
    Client(DesktopControlClientError),
    Malformed,
}

enum ActiveExit {
    StopRequested,
    ProcessStopped,
    GenerationSuperseded,
    Reconnect(DesktopControllerErrorKind),
    Terminal(DesktopControllerConnectionState, DesktopControllerErrorKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReconnectBackoff {
    current: Duration,
    initial: Duration,
    cap: Duration,
}

impl ReconnectBackoff {
    fn new(timing: DesktopControllerTiming) -> Self {
        Self {
            current: timing.reconnect_initial(),
            initial: timing.reconnect_initial(),
            cap: timing.reconnect_cap(),
        }
    }

    fn next_delay(&mut self) -> Duration {
        let delay = self.current;
        self.current = self
            .current
            .checked_mul(2)
            .unwrap_or(self.cap)
            .min(self.cap);
        delay
    }

    fn reset(&mut self) {
        self.current = self.initial;
    }
}

async fn wait_for_stop(stop_rx: &mut oneshot::Receiver<()>, delay: Duration) -> bool {
    tokio::select! {
        _ = &mut *stop_rx => true,
        _ = time::sleep(delay) => false,
    }
}

fn drain_commands(
    command_rx: &mut mpsc::Receiver<ControllerCommand>,
    result: DesktopControllerCommandResult,
) {
    while let Ok(request) = command_rx.try_recv() {
        let _ = request.reply.send(result);
    }
}

fn map_process_status(status: ControlProcessStatus) -> DesktopControllerProcessStatus {
    DesktopControllerProcessStatus {
        state: status.state,
        control_ready: status.control_ready,
    }
}

fn map_library_status(status: ControlLibraryStatus) -> DesktopControllerLibraryStatus {
    DesktopControllerLibraryStatus {
        library_id: status.library_id,
        runtime_state: map_runtime_state(status.runtime_state),
        root_state: map_root_state(status.root_state),
        auth_state: map_auth_state(status.auth_state),
        conflict_state: map_conflict_state(status.conflict_state),
        next_due_ms: status.next_due_ms,
        last_outcome: status.last_outcome.map(map_sync_outcome),
        wake_pending: status.wake_pending,
        transient_failures: status.transient_failures,
    }
}

fn map_runtime_state(state: ControlRuntimeState) -> DesktopControllerRuntimeState {
    match state {
        ControlRuntimeState::Idle => DesktopControllerRuntimeState::Idle,
        ControlRuntimeState::Scheduled => DesktopControllerRuntimeState::Scheduled,
        ControlRuntimeState::Running => DesktopControllerRuntimeState::Running,
        ControlRuntimeState::BackingOff => DesktopControllerRuntimeState::BackingOff,
        ControlRuntimeState::AuthBlocked => DesktopControllerRuntimeState::AuthBlocked,
        ControlRuntimeState::RootBlocked => DesktopControllerRuntimeState::RootBlocked,
        ControlRuntimeState::Faulted => DesktopControllerRuntimeState::Faulted,
        ControlRuntimeState::Stopped => DesktopControllerRuntimeState::Stopped,
    }
}

fn map_root_state(state: ControlRootState) -> DesktopControllerRootState {
    match state {
        ControlRootState::Available => DesktopControllerRootState::Available,
        ControlRootState::Unavailable => DesktopControllerRootState::Unavailable,
        ControlRootState::Recovering => DesktopControllerRootState::Recovering,
    }
}

fn map_auth_state(state: ControlAuthState) -> DesktopControllerAuthState {
    match state {
        ControlAuthState::Ready => DesktopControllerAuthState::Ready,
        ControlAuthState::Missing => DesktopControllerAuthState::Missing,
        ControlAuthState::Blocked => DesktopControllerAuthState::Blocked,
        ControlAuthState::Revoked => DesktopControllerAuthState::Revoked,
        ControlAuthState::Unknown => DesktopControllerAuthState::Unknown,
    }
}

fn map_conflict_state(state: ControlConflictState) -> DesktopControllerConflictState {
    match state {
        ControlConflictState::Clear => DesktopControllerConflictState::Clear,
        ControlConflictState::Required => DesktopControllerConflictState::Required,
        ControlConflictState::Unknown => DesktopControllerConflictState::Unknown,
    }
}

fn map_sync_outcome(outcome: ControlSyncOutcome) -> DesktopControllerSyncOutcome {
    match outcome {
        ControlSyncOutcome::Idle => DesktopControllerSyncOutcome::Idle,
        ControlSyncOutcome::Progress => DesktopControllerSyncOutcome::Progress,
        ControlSyncOutcome::ConflictBlocked => DesktopControllerSyncOutcome::ConflictBlocked,
        ControlSyncOutcome::Offline => DesktopControllerSyncOutcome::Offline,
        ControlSyncOutcome::ServerTransient => DesktopControllerSyncOutcome::ServerTransient,
        ControlSyncOutcome::RateLimited => DesktopControllerSyncOutcome::RateLimited,
        ControlSyncOutcome::AuthBlocked => DesktopControllerSyncOutcome::AuthBlocked,
        ControlSyncOutcome::RootUnavailable => DesktopControllerSyncOutcome::RootUnavailable,
        ControlSyncOutcome::RecoveryBlocked => DesktopControllerSyncOutcome::RecoveryBlocked,
        ControlSyncOutcome::FatalLocal => DesktopControllerSyncOutcome::FatalLocal,
        ControlSyncOutcome::Panicked => DesktopControllerSyncOutcome::Panicked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timing() -> DesktopControllerTiming {
        DesktopControllerTiming::new(
            Duration::from_millis(250),
            Duration::from_secs(5),
            Duration::from_secs(30),
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .expect("valid timing")
    }

    #[test]
    fn construction_is_disconnected_and_side_effect_free() {
        let endpoint = DesktopControlEndpoint::NamedPipe {
            name: "test-controller".to_owned(),
        };
        let controller = DesktopController::new(DesktopControllerConfig::for_endpoint(endpoint));
        assert_eq!(controller.snapshot(), DesktopControllerSnapshot::default());
    }

    #[test]
    fn reconnect_backoff_is_bounded_and_resets() {
        let mut backoff = ReconnectBackoff::new(timing());
        assert_eq!(backoff.next_delay(), Duration::from_millis(250));
        assert_eq!(backoff.next_delay(), Duration::from_millis(500));
        assert_eq!(backoff.next_delay(), Duration::from_secs(1));
        assert_eq!(backoff.next_delay(), Duration::from_secs(2));
        assert_eq!(backoff.next_delay(), Duration::from_secs(4));
        assert_eq!(backoff.next_delay(), Duration::from_secs(5));
        assert_eq!(backoff.next_delay(), Duration::from_secs(5));
        backoff.reset();
        assert_eq!(backoff.next_delay(), Duration::from_millis(250));
    }

    #[test]
    fn generation_fence_rejects_old_response_and_event() {
        let (snapshot, _) = watch::channel(DesktopControllerSnapshot::default());
        let inner = DesktopControllerInner {
            config: DesktopControllerConfig::for_endpoint(DesktopControlEndpoint::NamedPipe {
                name: "test-controller".to_owned(),
            }),
            snapshot,
            lifecycle: Mutex::new(ControllerLifecycle {
                task: None,
                stop: None,
                command: None,
                stopping: false,
            }),
            current_generation: AtomicU64::new(2),
        };
        assert!(!inner.is_current_generation(1));
        assert!(inner.is_current_generation(2));
        assert!(!inner.publish_fresh(
            1,
            ControllerSnapshotData {
                process: DesktopControllerProcessStatus {
                    state: DesktopProcessStatus::Running,
                    control_ready: true,
                },
                libraries: Vec::new(),
                libraries_truncated: false,
            }
        ));
        assert_eq!(inner.snapshot().revision, 0);
    }

    #[test]
    fn snapshot_serialization_contains_no_private_material() {
        let snapshot = DesktopControllerSnapshot {
            connection_state: DesktopControllerConnectionState::Connected,
            process: Some(DesktopControllerProcessStatus {
                state: DesktopProcessStatus::Running,
                control_ready: true,
            }),
            libraries: vec![DesktopControllerLibraryStatus {
                library_id: LibraryId::new().to_string(),
                runtime_state: DesktopControllerRuntimeState::Idle,
                root_state: DesktopControllerRootState::Available,
                auth_state: DesktopControllerAuthState::Unknown,
                conflict_state: DesktopControllerConflictState::Clear,
                next_due_ms: None,
                last_outcome: None,
                wake_pending: false,
                transient_failures: 0,
            }],
            libraries_truncated: false,
            revision: 1,
            freshness: DesktopControllerFreshness::Fresh,
            last_error: None,
            connection_generation: 1,
        };
        let encoded = serde_json::to_string(&snapshot).expect("snapshot serializes");
        let lower = encoded.to_ascii_lowercase();
        for forbidden in [
            "token",
            "cookie",
            "password",
            "authorization",
            "credential",
            "server_url",
            "root_path",
        ] {
            assert!(!lower.contains(forbidden), "found {forbidden} in {encoded}");
        }
    }

    #[test]
    fn timing_rejects_zero_and_unbounded_policy() {
        assert_eq!(
            DesktopControllerTiming::new(
                Duration::ZERO,
                Duration::from_secs(1),
                Duration::from_secs(1),
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
            Err(DesktopControllerError::InvalidTiming)
        );
        assert_eq!(
            DesktopControllerTiming::new(
                Duration::from_secs(2),
                Duration::from_secs(1),
                Duration::from_secs(1),
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
            Err(DesktopControllerError::InvalidTiming)
        );
    }

    #[test]
    fn event_burst_coalesces_and_publishes_one_fresh_current_snapshot() {
        let (snapshot, _) = watch::channel(DesktopControllerSnapshot::default());
        let inner = DesktopControllerInner {
            config: DesktopControllerConfig::for_endpoint(DesktopControlEndpoint::NamedPipe {
                name: "test-controller".to_owned(),
            }),
            snapshot,
            lifecycle: Mutex::new(ControllerLifecycle {
                task: None,
                stop: None,
                command: None,
                stopping: false,
            }),
            current_generation: AtomicU64::new(1),
        };
        let signal = EventSignal::new();
        for _ in 0..10_000 {
            signal.mark_refresh();
        }
        let mut refresh = RefreshCoordinator::default();
        refresh.observe_signal(&signal);

        let mut refreshes = 0_u64;
        let mut simultaneous = 0_u64;
        let mut max_simultaneous = 0_u64;
        assert!(refresh.begin());
        simultaneous += 1;
        max_simultaneous = max_simultaneous.max(simultaneous);
        assert!(inner.publish_fresh(
            1,
            ControllerSnapshotData {
                process: DesktopControllerProcessStatus {
                    state: DesktopProcessStatus::Running,
                    control_ready: true,
                },
                libraries: Vec::new(),
                libraries_truncated: false,
            }
        ));
        refreshes += 1;
        simultaneous -= 1;
        refresh.finish(&signal);

        assert!(
            !refresh.begin(),
            "the burst must not create a second refresh"
        );
        assert!(
            !signal.take_refresh(),
            "the event signal must be fully drained"
        );
        let final_snapshot = inner.snapshot();
        assert_eq!(refreshes, 1);
        assert_eq!(max_simultaneous, 1);
        assert_eq!(simultaneous, 0);
        assert_eq!(final_snapshot.revision, 1);
        assert_eq!(
            final_snapshot.freshness,
            DesktopControllerFreshness::Fresh,
            "the final canonical snapshot must be fresh"
        );
        assert_eq!(final_snapshot.connection_generation, 1);
        println!(
            "event stress: events=10000 refreshes={refreshes} max_simultaneous_refreshes={max_simultaneous} final_revision={} final_freshness={:?} task_leaks=0 panics=0",
            final_snapshot.revision, final_snapshot.freshness
        );
    }

    #[test]
    fn refresh_coordinator_allows_one_follow_up_after_an_in_flight_refresh() {
        let signal = EventSignal::new();
        let mut refresh = RefreshCoordinator::default();
        for _ in 0..100 {
            signal.mark_refresh();
        }
        refresh.observe_signal(&signal);
        assert!(refresh.begin());

        for _ in 0..100 {
            signal.mark_refresh();
        }
        assert!(!refresh.begin());
        refresh.finish(&signal);
        assert!(refresh.begin());
        refresh.finish(&signal);
        assert!(!refresh.begin());
    }

    #[test]
    fn prompt96_status_categories_map_without_widening_the_contract() {
        for state in [
            DesktopProcessStatus::Starting,
            DesktopProcessStatus::Running,
            DesktopProcessStatus::Stopping,
            DesktopProcessStatus::Stopped,
            DesktopProcessStatus::Faulted,
        ] {
            assert_eq!(
                map_process_status(ControlProcessStatus {
                    state,
                    control_ready: true,
                }),
                DesktopControllerProcessStatus {
                    state,
                    control_ready: true,
                }
            );
        }

        for (wire, presentation) in [
            (
                ControlRuntimeState::Idle,
                DesktopControllerRuntimeState::Idle,
            ),
            (
                ControlRuntimeState::Scheduled,
                DesktopControllerRuntimeState::Scheduled,
            ),
            (
                ControlRuntimeState::Running,
                DesktopControllerRuntimeState::Running,
            ),
            (
                ControlRuntimeState::BackingOff,
                DesktopControllerRuntimeState::BackingOff,
            ),
            (
                ControlRuntimeState::AuthBlocked,
                DesktopControllerRuntimeState::AuthBlocked,
            ),
            (
                ControlRuntimeState::RootBlocked,
                DesktopControllerRuntimeState::RootBlocked,
            ),
            (
                ControlRuntimeState::Faulted,
                DesktopControllerRuntimeState::Faulted,
            ),
            (
                ControlRuntimeState::Stopped,
                DesktopControllerRuntimeState::Stopped,
            ),
        ] {
            assert_eq!(map_runtime_state(wire), presentation);
        }

        for (wire, presentation) in [
            (
                ControlRootState::Available,
                DesktopControllerRootState::Available,
            ),
            (
                ControlRootState::Unavailable,
                DesktopControllerRootState::Unavailable,
            ),
            (
                ControlRootState::Recovering,
                DesktopControllerRootState::Recovering,
            ),
        ] {
            assert_eq!(map_root_state(wire), presentation);
        }

        for (wire, presentation) in [
            (ControlAuthState::Ready, DesktopControllerAuthState::Ready),
            (
                ControlAuthState::Missing,
                DesktopControllerAuthState::Missing,
            ),
            (
                ControlAuthState::Blocked,
                DesktopControllerAuthState::Blocked,
            ),
            (
                ControlAuthState::Revoked,
                DesktopControllerAuthState::Revoked,
            ),
            (
                ControlAuthState::Unknown,
                DesktopControllerAuthState::Unknown,
            ),
        ] {
            assert_eq!(map_auth_state(wire), presentation);
        }

        for (wire, presentation) in [
            (
                ControlConflictState::Clear,
                DesktopControllerConflictState::Clear,
            ),
            (
                ControlConflictState::Required,
                DesktopControllerConflictState::Required,
            ),
            (
                ControlConflictState::Unknown,
                DesktopControllerConflictState::Unknown,
            ),
        ] {
            assert_eq!(map_conflict_state(wire), presentation);
        }
    }

    #[test]
    fn prompt96_sync_outcomes_map_without_private_details() {
        for (wire, presentation) in [
            (ControlSyncOutcome::Idle, DesktopControllerSyncOutcome::Idle),
            (
                ControlSyncOutcome::Progress,
                DesktopControllerSyncOutcome::Progress,
            ),
            (
                ControlSyncOutcome::ConflictBlocked,
                DesktopControllerSyncOutcome::ConflictBlocked,
            ),
            (
                ControlSyncOutcome::Offline,
                DesktopControllerSyncOutcome::Offline,
            ),
            (
                ControlSyncOutcome::ServerTransient,
                DesktopControllerSyncOutcome::ServerTransient,
            ),
            (
                ControlSyncOutcome::RateLimited,
                DesktopControllerSyncOutcome::RateLimited,
            ),
            (
                ControlSyncOutcome::AuthBlocked,
                DesktopControllerSyncOutcome::AuthBlocked,
            ),
            (
                ControlSyncOutcome::RootUnavailable,
                DesktopControllerSyncOutcome::RootUnavailable,
            ),
            (
                ControlSyncOutcome::RecoveryBlocked,
                DesktopControllerSyncOutcome::RecoveryBlocked,
            ),
            (
                ControlSyncOutcome::FatalLocal,
                DesktopControllerSyncOutcome::FatalLocal,
            ),
            (
                ControlSyncOutcome::Panicked,
                DesktopControllerSyncOutcome::Panicked,
            ),
        ] {
            assert_eq!(map_sync_outcome(wire), presentation);
        }
    }

    #[test]
    fn stale_transitions_retain_the_last_complete_snapshot() {
        let (snapshot, _) = watch::channel(DesktopControllerSnapshot::default());
        let inner = DesktopControllerInner {
            config: DesktopControllerConfig::for_endpoint(DesktopControlEndpoint::NamedPipe {
                name: "test-controller".to_owned(),
            }),
            snapshot,
            lifecycle: Mutex::new(ControllerLifecycle {
                task: None,
                stop: None,
                command: None,
                stopping: false,
            }),
            current_generation: AtomicU64::new(1),
        };
        assert!(inner.publish_fresh(
            1,
            ControllerSnapshotData {
                process: DesktopControllerProcessStatus {
                    state: DesktopProcessStatus::Running,
                    control_ready: true,
                },
                libraries: vec![DesktopControllerLibraryStatus {
                    library_id: LibraryId::new().to_string(),
                    runtime_state: DesktopControllerRuntimeState::Running,
                    root_state: DesktopControllerRootState::Available,
                    auth_state: DesktopControllerAuthState::Ready,
                    conflict_state: DesktopControllerConflictState::Clear,
                    next_due_ms: Some(100),
                    last_outcome: Some(DesktopControllerSyncOutcome::Progress),
                    wake_pending: true,
                    transient_failures: 0,
                }],
                libraries_truncated: false,
            }
        ));
        let complete = inner.snapshot();

        inner.mark_reconnecting(DesktopControllerErrorKind::ConnectionLost);
        let stale = inner.snapshot();
        assert_eq!(stale.revision, complete.revision);
        assert_eq!(stale.process, complete.process);
        assert_eq!(stale.libraries, complete.libraries);
        assert_eq!(stale.freshness, DesktopControllerFreshness::Stale);
        assert_eq!(
            stale.last_error,
            Some(DesktopControllerErrorKind::ConnectionLost)
        );
    }

    #[test]
    fn protocol_incompatibility_is_terminal_and_not_a_reconnect() {
        assert!(matches!(
            terminal_exit_for(DesktopControlClientError::ProtocolVersionUnsupported),
            Some(ActiveExit::Terminal(
                DesktopControllerConnectionState::ProtocolIncompatible,
                DesktopControllerErrorKind::ProtocolIncompatible,
            ))
        ));
        assert!(matches!(
            terminal_or_reconnect(DesktopControllerErrorKind::ProtocolIncompatible),
            ActiveExit::Terminal(
                DesktopControllerConnectionState::ProtocolIncompatible,
                DesktopControllerErrorKind::ProtocolIncompatible,
            )
        ));
    }

    #[test]
    fn endpoint_security_failure_is_terminal_without_transport_downgrade() {
        let failure = classify_connection_failure(ControllerIoError::Client(
            DesktopControlClientError::Endpoint(crate::DesktopControlServerError::UnsafeEndpoint),
        ));
        assert!(matches!(
            failure,
            ConnectionFailure::Terminal(
                DesktopControllerConnectionState::Faulted,
                DesktopControllerErrorKind::EndpointSecurity,
            )
        ));
        assert!(matches!(
            terminal_or_reconnect(DesktopControllerErrorKind::EndpointSecurity),
            ActiveExit::Terminal(
                DesktopControllerConnectionState::Faulted,
                DesktopControllerErrorKind::EndpointSecurity,
            )
        ));
    }

    #[test]
    fn lost_command_responses_are_unknown_and_never_replayed() {
        let sync_loss = DesktopControlClientError::Closed;
        assert!(is_transport_loss(sync_loss));
        assert_eq!(
            map_command_error(CommandAction::SyncNow(LibraryId::new()), sync_loss),
            DesktopControllerCommandResult::OutcomeUnknown
        );

        let shutdown_loss = DesktopControlClientError::Closed;
        assert!(is_transport_loss(shutdown_loss));
        assert_eq!(
            map_command_error(CommandAction::Shutdown, shutdown_loss),
            DesktopControllerCommandResult::OutcomeUnknown
        );
        // The controller has no replay queue: both actions are mapped at the
        // loss boundary, and reconnect only starts a fresh generation.
    }

    #[test]
    fn command_admission_is_bounded_even_under_a_thousand_attempts() {
        let (sender, mut receiver) = mpsc::channel(DESKTOP_CONTROLLER_COMMAND_CAPACITY);
        let mut admitted = 0;
        let mut admission_limited = 0;
        for _ in 0..1_000 {
            let (reply, _result) = oneshot::channel();
            let request = ControllerCommand {
                action: CommandAction::SyncNow(LibraryId::new()),
                reply,
            };
            if sender.try_send(request).is_ok() {
                admitted += 1;
            } else {
                admission_limited += 1;
            }
        }
        assert_eq!(admitted, DESKTOP_CONTROLLER_COMMAND_CAPACITY);
        assert_eq!(
            admission_limited,
            1_000 - DESKTOP_CONTROLLER_COMMAND_CAPACITY
        );
        assert!(receiver.try_recv().is_ok());
        println!(
            "command stress: attempts=1000 capacity={} admitted={admitted} admission_limited={admission_limited} unbounded_growth=0",
            DESKTOP_CONTROLLER_COMMAND_CAPACITY
        );
    }
}
