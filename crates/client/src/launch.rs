//! Bounded background-client launch and user-autostart orchestration.
//!
//! This module is deliberately below the Qt shell.  It owns only process
//! availability, user-level supervisor operations, canonical executable
//! resolution, and one bounded launch gate.  It does not open the client
//! database, inspect a library root, load credentials, or retry a sync.

use std::{
    fmt, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use synveil_client_sync::ServerProfileId;
use synveil_platform::PlatformRuntime;
use tokio::{sync::Mutex, time};

use crate::control::DesktopControlClientError;
use crate::{
    DesktopControlClient, DesktopControlEndpoint, DesktopControlServerError, DesktopProcessStatus,
};

/// Readiness marker for the production desktop launch-management boundary.
pub const DESKTOP_LAUNCH_MANAGER_READINESS: &str = "SYNVEIL_DESKTOP_LAUNCH_MANAGER_READY";

/// The packaged process names are part of the install/package contract.  They
/// are never accepted from QML, a server, IPC, or library metadata.
pub const SYNVEIL_DESKTOP_EXECUTABLE: &str = "synveil-desktop";
pub const SYNVEIL_CLIENT_EXECUTABLE: &str = "synveil-client";

/// The supervisor service name is intentionally fixed and profile-independent.
/// The client reads its profile-bound non-secret configuration through the
/// existing platform config contract; the unit does not receive credentials or
/// a root path as arguments.
pub const LINUX_USER_SERVICE_NAME: &str = "synveil-client.service";

const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_millis(750);
const DEFAULT_RETRY_COOLDOWN: Duration = Duration::from_secs(5);

/// What the manager learned about the local background process without
/// exposing a path, stderr string, PID, credential, or remote detail.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackgroundClientAvailability {
    Running,
    Starting,
    Stopped,
    Absent,
    SupervisorInactive,
    SupervisorUnavailable,
    EndpointSecurity,
    ProtocolIncompatible,
    MalformedControl,
    WriterConflict,
    UnsafeState,
    TerminalFault,
}

/// User-level supervisor state, kept distinct from process endpoint state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupervisorState {
    Active,
    Available,
    Inactive,
    Unavailable,
    Unsupported,
}

/// User-autostart registration state.  Registration is process-management
/// metadata only; it never mutates sync state or credentials.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutostartState {
    Enabled,
    Disabled,
    NotInstalled,
    Unavailable,
    Unsupported,
}

impl AutostartState {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::NotInstalled => "not_installed",
            Self::Unavailable => "unavailable",
            Self::Unsupported => "unsupported",
        }
    }
}

/// Public, bounded outcome of one automatic start request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackgroundLaunchResult {
    AlreadyRunning,
    StartRequested,
    StartedSupervised,
    StartedDirect,
    AlreadyStarting,
    NotInstalled,
    SupervisorUnavailable,
    LaunchDenied,
    UnsafeState,
    Failed,
}

impl BackgroundLaunchResult {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::AlreadyRunning => "already_running",
            Self::StartRequested => "start_requested",
            Self::StartedSupervised => "started_supervised",
            Self::StartedDirect => "started_direct",
            Self::AlreadyStarting => "already_starting",
            Self::NotInstalled => "not_installed",
            Self::SupervisorUnavailable => "supervisor_unavailable",
            Self::LaunchDenied => "launch_denied",
            Self::UnsafeState => "unsafe_state",
            Self::Failed => "failed",
        }
    }

    #[must_use]
    const fn starts_or_requests_process(self) -> bool {
        matches!(
            self,
            Self::StartRequested | Self::StartedSupervised | Self::StartedDirect
        )
    }
}

/// Safe internal error taxonomy.  No OS diagnostic, command output, path, or
/// user-provided value is retained or formatted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackgroundLaunchError {
    NotInstalled,
    SupervisorUnavailable,
    Unsupported,
    LaunchDenied,
    UnsafeState,
    Failed,
}

impl BackgroundLaunchError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::NotInstalled => "DESKTOP_LAUNCH_NOT_INSTALLED",
            Self::SupervisorUnavailable => "DESKTOP_LAUNCH_SUPERVISOR_UNAVAILABLE",
            Self::Unsupported => "DESKTOP_LAUNCH_UNSUPPORTED",
            Self::LaunchDenied => "DESKTOP_LAUNCH_DENIED",
            Self::UnsafeState => "DESKTOP_LAUNCH_UNSAFE_STATE",
            Self::Failed => "DESKTOP_LAUNCH_FAILED",
        }
    }
}

impl fmt::Display for BackgroundLaunchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for BackgroundLaunchError {}

/// How a supervisor/direct fallback accepted a process start request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackgroundStartMode {
    Requested,
    Supervised,
    Direct,
}

/// Explicit bounds for one manager attempt.  The timeout is applied around
/// supervisor and endpoint probes, and the cooldown prevents immediate retry
/// storms after a failed or not-yet-visible start.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackgroundLaunchTiming {
    startup_timeout: Duration,
    probe_timeout: Duration,
    retry_cooldown: Duration,
}

impl Default for BackgroundLaunchTiming {
    fn default() -> Self {
        Self {
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
            probe_timeout: DEFAULT_PROBE_TIMEOUT,
            retry_cooldown: DEFAULT_RETRY_COOLDOWN,
        }
    }
}

impl BackgroundLaunchTiming {
    /// Build a bounded timing policy.  Zero values are rejected so a caller
    /// cannot accidentally turn a launch request into an unbounded loop.
    pub fn new(
        startup_timeout: Duration,
        probe_timeout: Duration,
        retry_cooldown: Duration,
    ) -> Result<Self, BackgroundLaunchError> {
        if startup_timeout.is_zero() || probe_timeout.is_zero() || retry_cooldown.is_zero() {
            return Err(BackgroundLaunchError::Failed);
        }
        if probe_timeout > startup_timeout {
            return Err(BackgroundLaunchError::Failed);
        }
        Ok(Self {
            startup_timeout,
            probe_timeout,
            retry_cooldown,
        })
    }

    #[must_use]
    pub const fn startup_timeout(self) -> Duration {
        self.startup_timeout
    }

    #[must_use]
    pub const fn probe_timeout(self) -> Duration {
        self.probe_timeout
    }

    #[must_use]
    pub const fn retry_cooldown(self) -> Duration {
        self.retry_cooldown
    }
}

/// Snapshot of the manager's boundedness counters.  `max_active_attempts`
/// should remain one for a manager, even when many callers race.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LaunchStats {
    pub ensure_requests: u64,
    pub coalesced_requests: u64,
    pub launch_attempts: u64,
    pub active_attempts: u64,
    pub max_active_attempts: u64,
}

/// OS adapter used by [`BackgroundClientManager`].  The Qt shell only sees the
/// manager's typed API; tests can provide a deterministic implementation here
/// without opening SQLite or running a real supervisor.
#[async_trait]
pub trait BackgroundLaunchBackend: Send + Sync {
    async fn inspect(&self) -> BackgroundClientAvailability;

    /// True only when this owner can prove graceful stop before relaunch.
    async fn restart_supported(&self) -> bool {
        false
    }

    async fn stop_for_restart(&self) -> Result<(), BackgroundLaunchError> {
        self.stop_supervised().await
    }

    /// Endpoint disappearance alone is never stop evidence.
    async fn stopped_for_restart(&self) -> bool {
        false
    }

    async fn request_start(&self) -> Result<BackgroundStartMode, BackgroundLaunchError>;

    async fn autostart_status(&self) -> Result<AutostartState, BackgroundLaunchError> {
        Err(BackgroundLaunchError::Unsupported)
    }

    async fn enable_autostart(&self) -> Result<AutostartState, BackgroundLaunchError> {
        Err(BackgroundLaunchError::Unsupported)
    }

    async fn disable_autostart(&self) -> Result<AutostartState, BackgroundLaunchError> {
        Err(BackgroundLaunchError::Unsupported)
    }

    async fn run_autostart(&self) -> Result<BackgroundStartMode, BackgroundLaunchError> {
        Err(BackgroundLaunchError::Unsupported)
    }

    async fn stop_supervised(&self) -> Result<(), BackgroundLaunchError> {
        Err(BackgroundLaunchError::Unsupported)
    }
}

/// Bounded lifecycle outcomes; acknowledgement alone is not success.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackgroundRestartResult {
    Recovered,
    Reconciling,
    Busy,
    GuidanceOnly,
    Failed,
}

#[derive(Clone, Copy)]
enum RestartPhase {
    Stopping,
    Starting,
}

#[derive(Default)]
struct LaunchGate {
    in_flight: bool,
    retry_after: Option<Instant>,
    restart_pending: Option<RestartPhase>,
}

#[derive(Default)]
struct LaunchCounters {
    ensure_requests: AtomicU64,
    coalesced_requests: AtomicU64,
    launch_attempts: AtomicU64,
    active_attempts: AtomicUsize,
    max_active_attempts: AtomicUsize,
}

/// Profile-scoped, cloneable launch manager.  Clones share one in-flight gate,
/// so two Qt objects or two desktop-controller paths cannot each spawn a
/// process for the same profile.
#[derive(Clone)]
pub struct BackgroundClientManager {
    profile_id: ServerProfileId,
    backend: Arc<dyn BackgroundLaunchBackend>,
    timing: BackgroundLaunchTiming,
    gate: Arc<Mutex<LaunchGate>>,
    counters: Arc<LaunchCounters>,
}

impl fmt::Debug for BackgroundClientManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BackgroundClientManager")
            .field("profile_id", &self.profile_id)
            .field("timing", &self.timing)
            .finish_non_exhaustive()
    }
}

impl BackgroundClientManager {
    /// Construct the production manager.  Construction performs no process,
    /// systemd, Task Scheduler, filesystem, or database operation.
    #[must_use]
    pub fn for_profile(profile_id: ServerProfileId) -> Self {
        Self::with_backend(
            profile_id,
            Arc::new(NativeBackgroundLaunchBackend::for_profile(profile_id)),
            BackgroundLaunchTiming::default(),
        )
    }

    /// Construct a manager around a test/embedding backend.
    #[must_use]
    pub fn with_backend(
        profile_id: ServerProfileId,
        backend: Arc<dyn BackgroundLaunchBackend>,
        timing: BackgroundLaunchTiming,
    ) -> Self {
        Self {
            profile_id,
            backend,
            timing,
            gate: Arc::new(Mutex::new(LaunchGate::default())),
            counters: Arc::new(LaunchCounters::default()),
        }
    }

    #[must_use]
    pub const fn profile_id(&self) -> ServerProfileId {
        self.profile_id
    }

    #[must_use]
    pub const fn timing(&self) -> BackgroundLaunchTiming {
        self.timing
    }

    /// Ensure that the background client is available.  All endpoint and
    /// supervisor failures are translated to stable typed outcomes.  The
    /// caller never receives raw command output or an executable path.
    pub async fn ensure_running(&self) -> BackgroundLaunchResult {
        self.counters
            .ensure_requests
            .fetch_add(1, Ordering::Relaxed);

        {
            let mut gate = self.gate.lock().await;
            if gate.in_flight
                || gate.restart_pending.is_some()
                || gate.retry_after.is_some_and(|until| until > Instant::now())
            {
                self.counters
                    .coalesced_requests
                    .fetch_add(1, Ordering::Relaxed);
                return BackgroundLaunchResult::AlreadyStarting;
            }
            gate.in_flight = true;
        }

        let active = self
            .counters
            .active_attempts
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        self.counters
            .max_active_attempts
            .fetch_max(active, Ordering::Relaxed);

        let result = match time::timeout(self.timing.startup_timeout, self.ensure_inner()).await {
            Ok(result) => result,
            Err(_) => BackgroundLaunchResult::Failed,
        };

        self.counters
            .active_attempts
            .fetch_sub(1, Ordering::Relaxed);
        {
            let mut gate = self.gate.lock().await;
            gate.in_flight = false;
            if result.starts_or_requests_process() || result == BackgroundLaunchResult::Failed {
                gate.retry_after = Some(Instant::now() + self.timing.retry_cooldown);
            } else {
                gate.retry_after = None;
            }
        }
        result
    }

    async fn ensure_inner(&self) -> BackgroundLaunchResult {
        let availability =
            match time::timeout(self.timing.probe_timeout, self.backend.inspect()).await {
                Ok(availability) => availability,
                Err(_) => BackgroundClientAvailability::SupervisorUnavailable,
            };

        match availability {
            BackgroundClientAvailability::Running => BackgroundLaunchResult::AlreadyRunning,
            BackgroundClientAvailability::Starting => BackgroundLaunchResult::AlreadyStarting,
            BackgroundClientAvailability::EndpointSecurity
            | BackgroundClientAvailability::WriterConflict
            | BackgroundClientAvailability::UnsafeState => BackgroundLaunchResult::UnsafeState,
            BackgroundClientAvailability::ProtocolIncompatible
            | BackgroundClientAvailability::MalformedControl
            | BackgroundClientAvailability::TerminalFault => BackgroundLaunchResult::LaunchDenied,
            BackgroundClientAvailability::Stopped
            | BackgroundClientAvailability::Absent
            | BackgroundClientAvailability::SupervisorInactive
            | BackgroundClientAvailability::SupervisorUnavailable => {
                self.counters
                    .launch_attempts
                    .fetch_add(1, Ordering::Relaxed);
                match time::timeout(self.timing.startup_timeout, self.backend.request_start()).await
                {
                    Ok(Ok(BackgroundStartMode::Requested)) => {
                        BackgroundLaunchResult::StartRequested
                    }
                    Ok(Ok(BackgroundStartMode::Supervised)) => {
                        BackgroundLaunchResult::StartedSupervised
                    }
                    Ok(Ok(BackgroundStartMode::Direct)) => BackgroundLaunchResult::StartedDirect,
                    Ok(Err(error)) => result_from_error(error),
                    Err(_) => BackgroundLaunchResult::Failed,
                }
            }
        }
    }

    pub async fn restart_supported(&self) -> bool {
        time::timeout(self.timing.probe_timeout, self.backend.restart_supported())
            .await
            .unwrap_or(false)
    }

    /// Snapshot-driven readiness reconciliation never issues stop or start.
    pub async fn reconcile_restart_readiness(&self) -> bool {
        let mut gate = self.gate.lock().await;
        if gate.in_flight || !matches!(gate.restart_pending, Some(RestartPhase::Starting)) {
            return false;
        }
        let running = matches!(
            time::timeout(self.timing.probe_timeout, self.backend.inspect()).await,
            Ok(BackgroundClientAvailability::Running)
        );
        if running {
            gate.restart_pending = None;
        }
        running
    }

    /// Compose the existing supervisor stop/start authority under the same
    /// launch gate. Uncertain stop/start is inspected, never blindly replayed.
    pub async fn restart(&self) -> BackgroundRestartResult {
        self.restart_attempt(false).await
    }

    /// Continue a pending attempt, or inspect readiness if it already resolved.
    /// A delayed UI status check must never initiate a fresh stop/start cycle.
    pub async fn check_restart_status(&self) -> BackgroundRestartResult {
        self.restart_attempt(true).await
    }

    async fn restart_attempt(&self, status_check: bool) -> BackgroundRestartResult {
        let pending = {
            let mut gate = self.gate.lock().await;
            if gate.in_flight || gate.retry_after.is_some_and(|until| until > Instant::now()) {
                return BackgroundRestartResult::Busy;
            }
            gate.in_flight = true;
            gate.restart_pending
        };
        let operation = async {
            if status_check && pending.is_none() {
                return if self.backend.inspect().await == BackgroundClientAvailability::Running {
                    BackgroundRestartResult::Recovered
                } else {
                    BackgroundRestartResult::Failed
                };
            }
            self.restart_inner(pending).await
        };
        let result = time::timeout(self.timing.startup_timeout, operation)
            .await
            .unwrap_or(BackgroundRestartResult::Reconciling);
        let mut gate = self.gate.lock().await;
        gate.in_flight = false;
        if result != BackgroundRestartResult::Reconciling {
            gate.restart_pending = None;
        }
        gate.retry_after = Some(Instant::now() + self.timing.retry_cooldown);
        result
    }

    async fn restart_inner(&self, pending: Option<RestartPhase>) -> BackgroundRestartResult {
        if matches!(pending, Some(RestartPhase::Starting)) {
            return if self.backend.inspect().await == BackgroundClientAvailability::Running {
                BackgroundRestartResult::Recovered
            } else {
                // Starting may have committed; no second launch here.
                BackgroundRestartResult::Reconciling
            };
        }
        if pending.is_none() {
            if !self.backend.restart_supported().await {
                return BackgroundRestartResult::GuidanceOnly;
            }
            self.gate.lock().await.restart_pending = Some(RestartPhase::Stopping);
            // Even an error may mean stop completed. Inspect before deciding.
            let _ = self.backend.stop_for_restart().await;
        }
        if !self.backend.stopped_for_restart().await {
            return BackgroundRestartResult::Reconciling;
        }
        self.gate.lock().await.restart_pending = Some(RestartPhase::Starting);
        let started = self.ensure_inner().await;
        if self.backend.inspect().await == BackgroundClientAvailability::Running {
            BackgroundRestartResult::Recovered
        } else if matches!(
            started,
            BackgroundLaunchResult::NotInstalled
                | BackgroundLaunchResult::LaunchDenied
                | BackgroundLaunchResult::UnsafeState
        ) {
            BackgroundRestartResult::Failed
        } else {
            BackgroundRestartResult::Reconciling
        }
    }

    pub async fn autostart_status(&self) -> Result<AutostartState, BackgroundLaunchError> {
        self.bounded_backend_call(self.backend.autostart_status())
            .await
    }

    pub async fn enable_autostart(&self) -> Result<AutostartState, BackgroundLaunchError> {
        self.bounded_backend_call(self.backend.enable_autostart())
            .await
    }

    pub async fn disable_autostart(&self) -> Result<AutostartState, BackgroundLaunchError> {
        self.bounded_backend_call(self.backend.disable_autostart())
            .await
    }

    pub async fn run_autostart(&self) -> Result<BackgroundLaunchResult, BackgroundLaunchError> {
        let mode = self
            .bounded_backend_call(self.backend.run_autostart())
            .await?;
        Ok(match mode {
            BackgroundStartMode::Requested => BackgroundLaunchResult::StartRequested,
            BackgroundStartMode::Supervised => BackgroundLaunchResult::StartedSupervised,
            BackgroundStartMode::Direct => BackgroundLaunchResult::StartedDirect,
        })
    }

    pub async fn stop_supervised(&self) -> Result<(), BackgroundLaunchError> {
        self.bounded_backend_call(self.backend.stop_supervised())
            .await
    }

    async fn bounded_backend_call<T>(
        &self,
        operation: impl std::future::Future<Output = Result<T, BackgroundLaunchError>>,
    ) -> Result<T, BackgroundLaunchError> {
        time::timeout(self.timing.startup_timeout, operation)
            .await
            .map_err(|_| BackgroundLaunchError::Failed)?
    }

    #[must_use]
    pub fn stats(&self) -> LaunchStats {
        LaunchStats {
            ensure_requests: self.counters.ensure_requests.load(Ordering::Relaxed),
            coalesced_requests: self.counters.coalesced_requests.load(Ordering::Relaxed),
            launch_attempts: self.counters.launch_attempts.load(Ordering::Relaxed),
            active_attempts: self.counters.active_attempts.load(Ordering::Relaxed) as u64,
            max_active_attempts: self.counters.max_active_attempts.load(Ordering::Relaxed) as u64,
        }
    }
}

fn result_from_error(error: BackgroundLaunchError) -> BackgroundLaunchResult {
    match error {
        BackgroundLaunchError::NotInstalled => BackgroundLaunchResult::NotInstalled,
        BackgroundLaunchError::SupervisorUnavailable => {
            BackgroundLaunchResult::SupervisorUnavailable
        }
        BackgroundLaunchError::Unsupported => BackgroundLaunchResult::SupervisorUnavailable,
        BackgroundLaunchError::LaunchDenied => BackgroundLaunchResult::LaunchDenied,
        BackgroundLaunchError::UnsafeState => BackgroundLaunchResult::UnsafeState,
        BackgroundLaunchError::Failed => BackgroundLaunchResult::Failed,
    }
}

/// Resolve the packaged client strictly beside the packaged desktop binary.
///
/// The desktop executable must have the canonical product name and both paths
/// must resolve to regular files.  No PATH lookup, QML-provided path, server
/// value, IPC value, or mutable library metadata participates in this function.
pub fn packaged_client_path_from(
    desktop_executable: impl AsRef<Path>,
) -> Result<PathBuf, BackgroundLaunchError> {
    let desktop = fs::canonicalize(desktop_executable.as_ref())
        .map_err(|_| BackgroundLaunchError::NotInstalled)?;
    if !desktop.is_file() || !is_expected_desktop_name(&desktop) {
        return Err(BackgroundLaunchError::NotInstalled);
    }
    let client_name = if cfg!(windows) {
        format!("{SYNVEIL_CLIENT_EXECUTABLE}.exe")
    } else {
        SYNVEIL_CLIENT_EXECUTABLE.to_string()
    };
    let client = fs::canonicalize(
        desktop
            .parent()
            .ok_or(BackgroundLaunchError::NotInstalled)?
            .join(client_name),
    )
    .map_err(|_| BackgroundLaunchError::NotInstalled)?;
    if !client.is_file() || !is_expected_client_name(&client) {
        return Err(BackgroundLaunchError::NotInstalled);
    }
    if client.parent() != desktop.parent() {
        return Err(BackgroundLaunchError::NotInstalled);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if client
            .metadata()
            .map_err(|_| BackgroundLaunchError::NotInstalled)?
            .permissions()
            .mode()
            & 0o111
            == 0
        {
            return Err(BackgroundLaunchError::LaunchDenied);
        }
    }
    Ok(client)
}

fn is_expected_desktop_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            if cfg!(windows) {
                name.eq_ignore_ascii_case("synveil-desktop.exe")
            } else {
                name == SYNVEIL_DESKTOP_EXECUTABLE
            }
        })
}

fn is_expected_client_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            if cfg!(windows) {
                name.eq_ignore_ascii_case("synveil-client.exe")
            } else {
                name == SYNVEIL_CLIENT_EXECUTABLE
            }
        })
}

/// Native production backend.  It is intentionally private in behavior but
/// public in type so embedders can identify the production adapter without
/// reaching into process/database internals.
pub struct NativeBackgroundLaunchBackend {
    profile_id: ServerProfileId,
    platform: Arc<dyn PlatformRuntime>,
}

impl fmt::Debug for NativeBackgroundLaunchBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeBackgroundLaunchBackend")
            .field("profile_id", &self.profile_id)
            .field("platform", &self.platform.platform())
            .finish_non_exhaustive()
    }
}

impl NativeBackgroundLaunchBackend {
    #[must_use]
    pub fn for_profile(profile_id: ServerProfileId) -> Self {
        Self::with_platform(profile_id, Arc::from(synveil_platform::current()))
    }

    #[must_use]
    pub fn with_platform(profile_id: ServerProfileId, platform: Arc<dyn PlatformRuntime>) -> Self {
        Self {
            profile_id,
            platform,
        }
    }

    async fn inspect_endpoint(&self) -> EndpointInspection {
        let endpoint =
            match DesktopControlEndpoint::for_profile(self.platform.as_ref(), self.profile_id) {
                Ok(endpoint) => endpoint,
                Err(error) => return endpoint_error_inspection(error),
            };
        let mut client = match time::timeout(
            DEFAULT_PROBE_TIMEOUT,
            DesktopControlClient::connect(endpoint),
        )
        .await
        {
            Ok(Ok(client)) => client,
            Ok(Err(error)) => return client_error_inspection(error),
            Err(_) => return EndpointInspection::Unavailable,
        };
        match time::timeout(DEFAULT_PROBE_TIMEOUT, client.ping()).await {
            Ok(Ok(status)) => match status {
                DesktopProcessStatus::Running => EndpointInspection::Running,
                DesktopProcessStatus::Starting | DesktopProcessStatus::Stopping => {
                    EndpointInspection::Starting
                }
                DesktopProcessStatus::Stopped => EndpointInspection::Stopped,
                DesktopProcessStatus::Faulted => EndpointInspection::TerminalFault,
            },
            Ok(Err(error)) => client_error_inspection(error),
            Err(_) => EndpointInspection::Unavailable,
        }
    }

    #[cfg(target_os = "linux")]
    fn linux_supervisor_state(&self) -> LinuxSupervisorState {
        linux_unit_state()
    }

    #[cfg(target_os = "windows")]
    fn windows_task_state(&self) -> WindowsTaskState {
        windows_task_state(self.profile_id)
    }

    fn direct_start(&self) -> Result<BackgroundStartMode, BackgroundLaunchError> {
        let desktop = std::env::current_exe().map_err(|_| BackgroundLaunchError::NotInstalled)?;
        let client = packaged_client_path_from(desktop)?;
        let mut command = Command::new(client);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            use windows_sys::Win32::System::Threading::{
                CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW,
            };
            command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
        }
        command
            .spawn()
            .map(|_| BackgroundStartMode::Direct)
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::PermissionDenied => BackgroundLaunchError::LaunchDenied,
                std::io::ErrorKind::NotFound => BackgroundLaunchError::NotInstalled,
                _ => BackgroundLaunchError::Failed,
            })
    }
}

#[async_trait]
impl BackgroundLaunchBackend for NativeBackgroundLaunchBackend {
    async fn restart_supported(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            // Direct processes and Windows tasks lack a full-stop proof here.
            let Ok(output) = systemctl_recovery_command(&[
                "--user",
                "show",
                LINUX_USER_SERVICE_NAME,
                "--property=LoadState",
                "--property=ActiveState",
                "--property=MainPID",
            ])
            .await
            else {
                return false;
            };
            if !output.status.success() {
                return false;
            }
            let state = String::from_utf8_lossy(&output.stdout);
            let Some(service_pid) = linux_running_service_pid(&state) else {
                return false;
            };
            let Ok(endpoint) =
                DesktopControlEndpoint::for_profile(self.platform.as_ref(), self.profile_id)
            else {
                return false;
            };
            let Ok(mut client) = DesktopControlClient::connect(endpoint).await else {
                return false;
            };
            return client.peer_process_id() == Some(service_pid)
                && matches!(client.ping().await, Ok(DesktopProcessStatus::Running));
        }
        #[allow(unreachable_code)]
        false
    }

    async fn stopped_for_restart(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            // systemctl stop waits for the supervised process group to exit.
            // Also reject a separately running profile endpoint.
            return linux_restart_stopped().await
                && self.inspect_endpoint().await == EndpointInspection::Unavailable;
        }
        #[allow(unreachable_code)]
        false
    }

    async fn stop_for_restart(&self) -> Result<(), BackgroundLaunchError> {
        #[cfg(target_os = "linux")]
        {
            // Cancellation leaves an uncertain result; the manager keeps its
            // pending stop fence and inspects before any subsequent start.
            return systemctl_recovery_action("stop").await;
        }
        #[allow(unreachable_code)]
        Err(BackgroundLaunchError::Unsupported)
    }

    async fn inspect(&self) -> BackgroundClientAvailability {
        match self.inspect_endpoint().await {
            EndpointInspection::Running => return BackgroundClientAvailability::Running,
            EndpointInspection::Starting => return BackgroundClientAvailability::Starting,
            EndpointInspection::Stopped => return BackgroundClientAvailability::Stopped,
            EndpointInspection::Security => return BackgroundClientAvailability::EndpointSecurity,
            EndpointInspection::Protocol => {
                return BackgroundClientAvailability::ProtocolIncompatible;
            }
            EndpointInspection::Malformed => {
                return BackgroundClientAvailability::MalformedControl;
            }
            EndpointInspection::TerminalFault => {
                return BackgroundClientAvailability::TerminalFault;
            }
            EndpointInspection::Unavailable => {}
        }

        #[cfg(target_os = "linux")]
        {
            return match linux_unit_state_async().await {
                LinuxSupervisorState::LoadedActive => BackgroundClientAvailability::Starting,
                LinuxSupervisorState::LoadedInactive => {
                    BackgroundClientAvailability::SupervisorInactive
                }
                LinuxSupervisorState::Absent => BackgroundClientAvailability::Absent,
                LinuxSupervisorState::Unavailable => {
                    BackgroundClientAvailability::SupervisorUnavailable
                }
            };
        }

        #[cfg(target_os = "windows")]
        {
            return match self.windows_task_state() {
                WindowsTaskState::Authoritative => BackgroundClientAvailability::SupervisorInactive,
                WindowsTaskState::Stale => BackgroundClientAvailability::UnsafeState,
                WindowsTaskState::Absent => BackgroundClientAvailability::Absent,
                WindowsTaskState::Unavailable => {
                    BackgroundClientAvailability::SupervisorUnavailable
                }
            };
        }

        #[allow(unreachable_code)]
        BackgroundClientAvailability::SupervisorUnavailable
    }

    async fn request_start(&self) -> Result<BackgroundStartMode, BackgroundLaunchError> {
        #[cfg(target_os = "linux")]
        {
            return match linux_unit_state_async().await {
                LinuxSupervisorState::LoadedActive | LinuxSupervisorState::LoadedInactive => {
                    systemctl_recovery_action("start")
                        .await
                        .map(|()| BackgroundStartMode::Supervised)
                }
                LinuxSupervisorState::Absent | LinuxSupervisorState::Unavailable => {
                    self.direct_start()
                }
            };
        }

        #[cfg(target_os = "windows")]
        {
            return match self.windows_task_state() {
                WindowsTaskState::Authoritative => {
                    windows_task_action(self.profile_id, WindowsTaskAction::Run)
                        .map(|()| BackgroundStartMode::Supervised)
                }
                WindowsTaskState::Absent
                | WindowsTaskState::Stale
                | WindowsTaskState::Unavailable => self.direct_start(),
            };
        }

        #[allow(unreachable_code)]
        Err(BackgroundLaunchError::Unsupported)
    }

    async fn autostart_status(&self) -> Result<AutostartState, BackgroundLaunchError> {
        #[cfg(target_os = "linux")]
        {
            return match self.linux_supervisor_state() {
                LinuxSupervisorState::Unavailable => {
                    Err(BackgroundLaunchError::SupervisorUnavailable)
                }
                LinuxSupervisorState::Absent => Ok(AutostartState::NotInstalled),
                LinuxSupervisorState::LoadedActive | LinuxSupervisorState::LoadedInactive => {
                    systemctl_user_is_enabled()
                }
            };
        }
        #[cfg(target_os = "windows")]
        {
            return Ok(match self.windows_task_state() {
                WindowsTaskState::Authoritative => AutostartState::Enabled,
                WindowsTaskState::Stale => AutostartState::Disabled,
                WindowsTaskState::Absent => AutostartState::Disabled,
                WindowsTaskState::Unavailable => AutostartState::Unavailable,
            });
        }
        #[allow(unreachable_code)]
        Err(BackgroundLaunchError::Unsupported)
    }

    async fn enable_autostart(&self) -> Result<AutostartState, BackgroundLaunchError> {
        #[cfg(target_os = "linux")]
        {
            return match self.linux_supervisor_state() {
                LinuxSupervisorState::Unavailable => {
                    Err(BackgroundLaunchError::SupervisorUnavailable)
                }
                LinuxSupervisorState::Absent => Ok(AutostartState::NotInstalled),
                LinuxSupervisorState::LoadedActive | LinuxSupervisorState::LoadedInactive => {
                    systemctl_user_action("enable")?;
                    Ok(AutostartState::Enabled)
                }
            };
        }
        #[cfg(target_os = "windows")]
        {
            return match self.windows_task_state() {
                WindowsTaskState::Authoritative
                | WindowsTaskState::Stale
                | WindowsTaskState::Absent => {
                    // Replace this same profile task on explicit enable so
                    // an upgraded or relocated payload refreshes its action.
                    let definition = native_windows_task_definition(self.profile_id)?;
                    windows_register_task(&definition)?;
                    match self.windows_task_state() {
                        WindowsTaskState::Authoritative => Ok(AutostartState::Enabled),
                        _ => Err(BackgroundLaunchError::UnsafeState),
                    }
                }
                WindowsTaskState::Unavailable => Err(BackgroundLaunchError::SupervisorUnavailable),
            };
        }
        #[allow(unreachable_code)]
        Err(BackgroundLaunchError::Unsupported)
    }

    async fn disable_autostart(&self) -> Result<AutostartState, BackgroundLaunchError> {
        #[cfg(target_os = "linux")]
        {
            return match self.linux_supervisor_state() {
                LinuxSupervisorState::Unavailable => {
                    Err(BackgroundLaunchError::SupervisorUnavailable)
                }
                LinuxSupervisorState::Absent => Ok(AutostartState::NotInstalled),
                LinuxSupervisorState::LoadedActive | LinuxSupervisorState::LoadedInactive => {
                    systemctl_user_action("disable")?;
                    Ok(AutostartState::Disabled)
                }
            };
        }
        #[cfg(target_os = "windows")]
        {
            return match self.windows_task_state() {
                WindowsTaskState::Absent => Ok(AutostartState::Disabled),
                WindowsTaskState::Unavailable => Ok(AutostartState::Unavailable),
                WindowsTaskState::Authoritative | WindowsTaskState::Stale => {
                    windows_task_action(self.profile_id, WindowsTaskAction::Delete)?;
                    match self.windows_task_state() {
                        WindowsTaskState::Absent => Ok(AutostartState::Disabled),
                        _ => Err(BackgroundLaunchError::UnsafeState),
                    }
                }
            };
        }
        #[allow(unreachable_code)]
        Err(BackgroundLaunchError::Unsupported)
    }

    async fn run_autostart(&self) -> Result<BackgroundStartMode, BackgroundLaunchError> {
        #[cfg(target_os = "linux")]
        {
            return match self.linux_supervisor_state() {
                LinuxSupervisorState::LoadedActive | LinuxSupervisorState::LoadedInactive => {
                    systemctl_user_action("start").map(|()| BackgroundStartMode::Supervised)
                }
                LinuxSupervisorState::Absent => Err(BackgroundLaunchError::NotInstalled),
                LinuxSupervisorState::Unavailable => {
                    Err(BackgroundLaunchError::SupervisorUnavailable)
                }
            };
        }
        #[cfg(target_os = "windows")]
        {
            return match self.windows_task_state() {
                WindowsTaskState::Authoritative => {
                    windows_task_action(self.profile_id, WindowsTaskAction::Run)
                        .map(|()| BackgroundStartMode::Supervised)
                }
                WindowsTaskState::Absent => Err(BackgroundLaunchError::NotInstalled),
                WindowsTaskState::Stale => Err(BackgroundLaunchError::UnsafeState),
                WindowsTaskState::Unavailable => Err(BackgroundLaunchError::SupervisorUnavailable),
            };
        }
        #[allow(unreachable_code)]
        Err(BackgroundLaunchError::Unsupported)
    }

    async fn stop_supervised(&self) -> Result<(), BackgroundLaunchError> {
        #[cfg(target_os = "linux")]
        {
            return match self.linux_supervisor_state() {
                LinuxSupervisorState::LoadedActive | LinuxSupervisorState::LoadedInactive => {
                    systemctl_user_action("stop")
                }
                LinuxSupervisorState::Absent => Ok(()),
                LinuxSupervisorState::Unavailable => {
                    Err(BackgroundLaunchError::SupervisorUnavailable)
                }
            };
        }
        #[cfg(target_os = "windows")]
        {
            return match self.windows_task_state() {
                WindowsTaskState::Authoritative | WindowsTaskState::Stale => {
                    // Task Scheduler /End forcibly terminates the process.
                    // Use the profile-bound transport so the client follows
                    // the same bounded host shutdown path as Linux signals.
                    let endpoint = DesktopControlEndpoint::for_profile(
                        self.platform.as_ref(),
                        self.profile_id,
                    )
                    .map_err(|_| BackgroundLaunchError::LaunchDenied)?;
                    request_graceful_client_stop(endpoint).await
                }
                WindowsTaskState::Absent => Ok(()),
                WindowsTaskState::Unavailable => Err(BackgroundLaunchError::SupervisorUnavailable),
            };
        }
        #[allow(unreachable_code)]
        Err(BackgroundLaunchError::Unsupported)
    }
}

#[cfg(any(windows, test))]
async fn request_graceful_client_stop(
    endpoint: DesktopControlEndpoint,
) -> Result<(), BackgroundLaunchError> {
    let shutdown = async {
        let mut client = DesktopControlClient::connect(endpoint)
            .await
            .map_err(|_| BackgroundLaunchError::SupervisorUnavailable)?;
        client
            .shutdown()
            .await
            .map_err(|_| BackgroundLaunchError::Failed)
    };
    time::timeout(DEFAULT_PROBE_TIMEOUT, shutdown)
        .await
        .map_err(|_| BackgroundLaunchError::Failed)?
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EndpointInspection {
    Running,
    Starting,
    Stopped,
    Security,
    Protocol,
    Malformed,
    TerminalFault,
    Unavailable,
}

fn endpoint_error_inspection(error: DesktopControlServerError) -> EndpointInspection {
    match error {
        DesktopControlServerError::InsecureRuntimeDirectory
        | DesktopControlServerError::UnsafeEndpoint
        | DesktopControlServerError::EndpointAlreadyActive
        | DesktopControlServerError::EndpointStateUnknown
        | DesktopControlServerError::SecurityDescriptorUnavailable => EndpointInspection::Security,
        DesktopControlServerError::UnsupportedPlatform => EndpointInspection::Malformed,
        _ => EndpointInspection::Unavailable,
    }
}

fn client_error_inspection(error: DesktopControlClientError) -> EndpointInspection {
    match error {
        DesktopControlClientError::ProtocolVersionUnsupported
        | DesktopControlClientError::Server(crate::ControlErrorCode::ProtocolVersionUnsupported) => {
            EndpointInspection::Protocol
        }
        DesktopControlClientError::Endpoint(error) => endpoint_error_inspection(error),
        DesktopControlClientError::Handshake
        | DesktopControlClientError::Frame(crate::ControlFrameError::PayloadMalformed)
        | DesktopControlClientError::UnexpectedResponse
        | DesktopControlClientError::ResponseMismatch => EndpointInspection::Malformed,
        DesktopControlClientError::Server(crate::ControlErrorCode::EndpointUnsafe)
        | DesktopControlClientError::Server(crate::ControlErrorCode::EndpointAlreadyActive) => {
            EndpointInspection::Security
        }
        DesktopControlClientError::UnsupportedPlatform => EndpointInspection::Malformed,
        DesktopControlClientError::Connection
        | DesktopControlClientError::Frame(_)
        | DesktopControlClientError::Server(_)
        | DesktopControlClientError::Closed => EndpointInspection::Unavailable,
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LinuxSupervisorState {
    LoadedActive,
    LoadedInactive,
    Absent,
    Unavailable,
}

#[cfg(target_os = "linux")]
fn systemctl_path() -> Option<PathBuf> {
    ["/usr/bin/systemctl", "/bin/systemctl"]
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

#[cfg(target_os = "linux")]
fn systemctl_command(args: &[&str]) -> Result<std::process::Output, BackgroundLaunchError> {
    let executable = systemctl_path().ok_or(BackgroundLaunchError::SupervisorUnavailable)?;
    Command::new(executable)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| BackgroundLaunchError::SupervisorUnavailable)
}

#[cfg(any(target_os = "linux", test))]
fn linux_running_service_pid(state: &str) -> Option<u32> {
    if !state.lines().any(|line| line == "LoadState=loaded")
        || !state.lines().any(|line| line == "ActiveState=active")
    {
        return None;
    }
    state
        .lines()
        .find_map(|line| line.strip_prefix("MainPID="))
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|pid| *pid != 0)
}

#[cfg(target_os = "linux")]
async fn systemctl_recovery_command(
    args: &[&str],
) -> Result<std::process::Output, BackgroundLaunchError> {
    let executable = systemctl_path().ok_or(BackgroundLaunchError::SupervisorUnavailable)?;
    let mut command = tokio::process::Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    time::timeout(DEFAULT_STARTUP_TIMEOUT, command.output())
        .await
        .map_err(|_| BackgroundLaunchError::Failed)?
        .map_err(|_| BackgroundLaunchError::SupervisorUnavailable)
}

#[cfg(target_os = "linux")]
async fn systemctl_recovery_action(action: &str) -> Result<(), BackgroundLaunchError> {
    let output = systemctl_recovery_command(&["--user", action, LINUX_USER_SERVICE_NAME]).await?;
    if output.status.success() {
        Ok(())
    } else {
        Err(BackgroundLaunchError::Failed)
    }
}

#[cfg(target_os = "linux")]
async fn linux_unit_state_async() -> LinuxSupervisorState {
    let Ok(output) = systemctl_recovery_command(&[
        "--user",
        "show",
        LINUX_USER_SERVICE_NAME,
        "--property=LoadState",
        "--property=ActiveState",
    ])
    .await
    else {
        return LinuxSupervisorState::Unavailable;
    };
    if !output.status.success() {
        return LinuxSupervisorState::Unavailable;
    }
    let state = String::from_utf8_lossy(&output.stdout);
    if state.lines().any(|line| line == "LoadState=loaded") {
        if state.lines().any(|line| {
            matches!(
                line,
                "ActiveState=active"
                    | "ActiveState=activating"
                    | "ActiveState=deactivating"
                    | "ActiveState=reloading"
            )
        }) {
            LinuxSupervisorState::LoadedActive
        } else if state
            .lines()
            .any(|line| matches!(line, "ActiveState=inactive" | "ActiveState=failed"))
        {
            LinuxSupervisorState::LoadedInactive
        } else {
            LinuxSupervisorState::Unavailable
        }
    } else if state
        .lines()
        .any(|line| line == "LoadState=not-found" || line == "LoadState=masked")
    {
        LinuxSupervisorState::Absent
    } else {
        LinuxSupervisorState::Unavailable
    }
}

#[cfg(target_os = "linux")]
async fn linux_restart_stopped() -> bool {
    let Ok(output) = systemctl_recovery_command(&[
        "--user",
        "show",
        LINUX_USER_SERVICE_NAME,
        "--property=LoadState",
        "--property=ActiveState",
        "--property=SubState",
        "--property=MainPID",
        "--property=ControlPID",
    ])
    .await
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let state = String::from_utf8_lossy(&output.stdout);
    linux_restart_stop_proven(&state)
}

#[cfg(any(target_os = "linux", test))]
fn linux_restart_stop_proven(state: &str) -> bool {
    // A failed is-active probe, deactivating service, or missing property is
    // not a stop proof. No arbitrary process identifier is acted on.
    [
        "LoadState=loaded",
        "ActiveState=inactive",
        "SubState=dead",
        "MainPID=0",
        "ControlPID=0",
    ]
    .iter()
    .all(|expected| state.lines().any(|line| line == *expected))
}

#[cfg(target_os = "linux")]
fn linux_unit_state() -> LinuxSupervisorState {
    let output = match systemctl_command(&[
        "--user",
        "show",
        LINUX_USER_SERVICE_NAME,
        "--property=LoadState",
        "--value",
    ]) {
        Ok(output) => output,
        Err(_) => return LinuxSupervisorState::Unavailable,
    };
    if !output.status.success() {
        return LinuxSupervisorState::Unavailable;
    }
    let load_state = String::from_utf8_lossy(&output.stdout);
    match load_state.trim() {
        "loaded" => {
            let active =
                systemctl_command(&["--user", "is-active", "--quiet", LINUX_USER_SERVICE_NAME])
                    .map(|output| output.status.success())
                    .unwrap_or(false);
            if active {
                LinuxSupervisorState::LoadedActive
            } else {
                LinuxSupervisorState::LoadedInactive
            }
        }
        "not-found" | "masked" => LinuxSupervisorState::Absent,
        _ => LinuxSupervisorState::Unavailable,
    }
}

#[cfg(target_os = "linux")]
fn systemctl_user_action(action: &str) -> Result<(), BackgroundLaunchError> {
    let output = systemctl_command(&["--user", action, LINUX_USER_SERVICE_NAME])?;
    if output.status.success() {
        Ok(())
    } else {
        Err(BackgroundLaunchError::SupervisorUnavailable)
    }
}

#[cfg(target_os = "linux")]
fn systemctl_user_is_enabled() -> Result<AutostartState, BackgroundLaunchError> {
    let output = systemctl_command(&["--user", "is-enabled", "--quiet", LINUX_USER_SERVICE_NAME])?;
    if output.status.success() {
        Ok(AutostartState::Enabled)
    } else {
        Ok(AutostartState::Disabled)
    }
}

/// Deterministic Windows Task Scheduler definition.  It is also available on
/// non-Windows targets for cross-build/static-policy tests; native registration
/// is compiled only on Windows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsTaskDefinition {
    task_name: String,
    executable: PathBuf,
    principal: String,
}

impl WindowsTaskDefinition {
    pub fn new(
        profile_id: ServerProfileId,
        executable: impl Into<PathBuf>,
        principal: impl Into<String>,
    ) -> Result<Self, BackgroundLaunchError> {
        let executable =
            fs::canonicalize(executable.into()).map_err(|_| BackgroundLaunchError::NotInstalled)?;
        Self::from_canonical_path(profile_id, executable, principal)
    }

    pub fn from_canonical_path(
        profile_id: ServerProfileId,
        executable: PathBuf,
        principal: impl Into<String>,
    ) -> Result<Self, BackgroundLaunchError> {
        let principal = principal.into();
        if !executable.is_absolute()
            || !executable.is_file()
            || !is_expected_windows_client_name(&executable)
            || principal.is_empty()
            || principal.chars().any(char::is_control)
            || executable
                .as_os_str()
                .to_str()
                .is_none_or(|value| value.chars().any(char::is_control))
        {
            return Err(BackgroundLaunchError::LaunchDenied);
        }
        Ok(Self {
            task_name: windows_task_name(profile_id),
            executable,
            principal,
        })
    }

    #[must_use]
    pub fn task_name(&self) -> &str {
        &self.task_name
    }

    #[must_use]
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    #[must_use]
    pub fn principal(&self) -> &str {
        &self.principal
    }

    /// UTF-8 XML accepted by `schtasks.exe /XML`; all dynamic values are
    /// escaped, and the task contains no password or credential element.
    #[must_use]
    pub fn to_xml(&self) -> String {
        let principal = xml_escape(&self.principal);
        let command = xml_escape(&self.executable.to_string_lossy());
        let working_directory = self
            .executable
            .parent()
            .map(|path| xml_escape(&path.to_string_lossy()))
            .unwrap_or_default();
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<Task version=\"1.4\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n\
  <RegistrationInfo><Description>Synveil background client</Description></RegistrationInfo>\n\
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{principal}</UserId></LogonTrigger></Triggers>\n\
  <Principals><Principal id=\"SynveilUser\"><UserId>{principal}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>\n\
  <Settings>\n\
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\n\
    <RestartOnFailure><Interval>PT30S</Interval><Count>5</Count></RestartOnFailure>\n\
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit><StartWhenAvailable>true</StartWhenAvailable>\n\
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\n\
  </Settings>\n\
  <Actions Context=\"SynveilUser\"><Exec><Command>{command}</Command><WorkingDirectory>{working_directory}</WorkingDirectory></Exec></Actions>\n\
</Task>\n"
        )
    }
}

fn windows_task_name(profile_id: ServerProfileId) -> String {
    format!(r"\Synveil\BackgroundClient\profile-{profile_id}")
}

fn is_expected_windows_client_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("synveil-client.exe"))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsTaskState {
    Authoritative,
    Stale,
    Absent,
    Unavailable,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsTaskAction {
    Run,
    Delete,
}

#[cfg(target_os = "windows")]
fn schtasks_path() -> Result<PathBuf, BackgroundLaunchError> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
    let mut buffer = [0_u16; 32768];
    // The OS supplies this directory; PATH and SystemRoot cannot select a tool.
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 || length >= buffer.len() {
        return Err(BackgroundLaunchError::SupervisorUnavailable);
    }
    let path = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length])).join("schtasks.exe");
    windows_owned_ancestry(&path)?;
    if !path.is_file() {
        return Err(BackgroundLaunchError::SupervisorUnavailable);
    }
    Ok(path)
}

#[cfg(target_os = "windows")]
fn windows_owned_ancestry(path: &Path) -> Result<(), BackgroundLaunchError> {
    use std::os::windows::fs::MetadataExt;
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(BackgroundLaunchError::LaunchDenied);
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata)
                if metadata.file_attributes() & 0x400 != 0
                    || (ancestor != path && !metadata.is_dir()) =>
            {
                return Err(BackgroundLaunchError::LaunchDenied);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(BackgroundLaunchError::LaunchDenied),
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_startup_staging() -> Result<tempfile::TempDir, BackgroundLaunchError> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath},
    };
    let mut pointer = std::ptr::null_mut();
    // This per-user OS authority is independent of HOME/TMP/TEMP/LOCALAPPDATA.
    let result = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_LocalAppData,
            0,
            std::ptr::null_mut(),
            &mut pointer,
        )
    };
    if result < 0 || pointer.is_null() {
        if !pointer.is_null() {
            unsafe {
                CoTaskMemFree(pointer.cast());
            }
        }
        return Err(BackgroundLaunchError::SupervisorUnavailable);
    }
    let mut length = 0;
    // The API owns a terminated UTF-16 string; release its allocation on every path.
    while length < 32768 && unsafe { *pointer.add(length) } != 0 {
        length += 1;
    }
    let root = if length < 32768 {
        Some(PathBuf::from(std::ffi::OsString::from_wide(unsafe {
            std::slice::from_raw_parts(pointer, length)
        })))
    } else {
        None
    };
    unsafe {
        CoTaskMemFree(pointer.cast());
    }
    let parent = root
        .ok_or(BackgroundLaunchError::SupervisorUnavailable)?
        .join("Synveil/startup-staging");
    windows_owned_ancestry(&parent)?;
    fs::create_dir_all(&parent).map_err(|_| BackgroundLaunchError::Failed)?;
    windows_owned_ancestry(&parent)?;
    tempfile::Builder::new()
        .prefix("task-")
        .tempdir_in(parent)
        .map_err(|_| BackgroundLaunchError::Failed)
}

#[cfg(target_os = "windows")]
fn schtasks_command(args: &[String]) -> Result<std::process::Output, BackgroundLaunchError> {
    Command::new(schtasks_path()?)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| BackgroundLaunchError::SupervisorUnavailable)
}

#[cfg(target_os = "windows")]
fn windows_task_state(profile_id: ServerProfileId) -> WindowsTaskState {
    let expected = match native_windows_task_definition(profile_id) {
        Ok(expected) => expected,
        Err(_) => return WindowsTaskState::Unavailable,
    };
    let task_name = windows_task_name(profile_id);
    let args = vec![
        "/Query".to_string(),
        "/TN".to_string(),
        task_name,
        "/XML".to_string(),
    ];
    let executable = match schtasks_path() {
        Ok(path) => path,
        Err(_) => return WindowsTaskState::Unavailable,
    };
    let mut child = match Command::new(executable)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return WindowsTaskState::Unavailable,
    };
    let mut bytes = Vec::new();
    let read = child.stdout.take().is_some_and(|stdout| {
        use std::io::Read;
        let mut limited = stdout.take(64 * 1024 + 1);
        limited.read_to_end(&mut bytes).is_ok()
    });
    if !read || bytes.len() > 64 * 1024 {
        let _ = child.kill();
        let _ = child.wait();
        return WindowsTaskState::Stale;
    }
    match child.wait() {
        Ok(status) if status.success() => {
            let Some(xml) = decode_task_xml(&bytes) else {
                return WindowsTaskState::Stale;
            };
            if windows_task_xml_is_authoritative(&xml, &expected) {
                WindowsTaskState::Authoritative
            } else {
                WindowsTaskState::Stale
            }
        }
        Ok(_) => WindowsTaskState::Absent,
        Err(_) => WindowsTaskState::Unavailable,
    }
}

#[cfg(any(windows, test))]
fn decode_task_xml(bytes: &[u8]) -> Option<String> {
    if let Some(payload) = bytes.strip_prefix(&[0xff, 0xfe]) {
        let (pairs, remainder) = payload.as_chunks::<2>();
        if !remainder.is_empty() {
            return None;
        }
        let words = pairs
            .iter()
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&words).ok()
    } else if let Some(payload) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        String::from_utf8(payload.to_vec()).ok()
    } else {
        String::from_utf8(bytes.to_vec()).ok()
    }
}

#[cfg(any(windows, test))]
fn windows_task_xml_is_authoritative(xml: &str, expected: &WindowsTaskDefinition) -> bool {
    use quick_xml::{Reader, events::Event};
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut current = None::<String>;
    let mut values: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    let mut logon_triggers = 0;
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let name = String::from_utf8_lossy(element.local_name().as_ref()).into_owned();
                if name == "Arguments" || name == "Password" || name == "BootTrigger" {
                    return false;
                }
                if name == "LogonTrigger" {
                    logon_triggers += 1;
                }
                current = Some(name);
            }
            Ok(Event::Empty(element)) => {
                let name = String::from_utf8_lossy(element.local_name().as_ref()).into_owned();
                if name == "Arguments" || name == "Password" || name.ends_with("Trigger") {
                    return false;
                }
            }
            Ok(Event::Text(text)) => {
                let Some(name) = current.as_ref() else {
                    continue;
                };
                let Ok(decoded) = text.decode() else {
                    return false;
                };
                let Ok(value) = quick_xml::escape::unescape(&decoded) else {
                    return false;
                };
                values
                    .entry(name.clone())
                    .or_default()
                    .push(value.into_owned());
            }
            Ok(Event::End(_)) => current = None,
            Ok(Event::Eof) => break,
            Ok(Event::DocType(_)) | Err(_) => return false,
            _ => {}
        }
    }
    let exact = |tag: &str, expected_values: &[&str]| {
        values.get(tag).is_some_and(|actual| {
            actual
                .iter()
                .map(String::as_str)
                .eq(expected_values.iter().copied())
        })
    };
    let executable = expected.executable().to_string_lossy();
    let working = expected
        .executable()
        .parent()
        .unwrap_or(Path::new(""))
        .to_string_lossy();
    logon_triggers == 1
        && exact("UserId", &[expected.principal(), expected.principal()])
        && exact("LogonType", &["InteractiveToken"])
        && exact("RunLevel", &["LeastPrivilege"])
        && exact("Command", &[&executable])
        && exact("WorkingDirectory", &[&working])
        && exact("MultipleInstancesPolicy", &["IgnoreNew"])
}

#[cfg(target_os = "windows")]
fn current_windows_user() -> Result<String, BackgroundLaunchError> {
    use windows_sys::Win32::System::WindowsProgramming::GetUserNameW;
    let mut buffer = [0_u16; 512];
    let mut length = buffer.len() as u32;
    // SAFETY: buffer is valid writable storage and the length includes its
    // capacity as required by GetUserNameW. No user-controlled pointer exists.
    let success = unsafe { GetUserNameW(buffer.as_mut_ptr(), &mut length) } != 0;
    if !success {
        return Err(BackgroundLaunchError::SupervisorUnavailable);
    }
    decode_windows_username(&buffer, length as usize)
}

#[cfg(any(windows, test))]
fn decode_windows_username(buffer: &[u16], length: usize) -> Result<String, BackgroundLaunchError> {
    // GetUserNameW includes the terminating NUL in its successful length.
    let terminated = buffer
        .get(..length)
        .filter(|value| value.len() > 1 && value.last() == Some(&0))
        .ok_or(BackgroundLaunchError::SupervisorUnavailable)?;
    let username = String::from_utf16(&terminated[..terminated.len() - 1])
        .map_err(|_| BackgroundLaunchError::SupervisorUnavailable)?;
    if username.chars().any(char::is_control) {
        return Err(BackgroundLaunchError::SupervisorUnavailable);
    }
    Ok(username)
}

#[cfg(target_os = "windows")]
fn native_windows_task_definition(
    profile_id: ServerProfileId,
) -> Result<WindowsTaskDefinition, BackgroundLaunchError> {
    let desktop = std::env::current_exe().map_err(|_| BackgroundLaunchError::NotInstalled)?;
    let client = packaged_client_path_from(desktop)?;
    WindowsTaskDefinition::from_canonical_path(profile_id, client, current_windows_user()?)
}

#[cfg(target_os = "windows")]
fn windows_task_action(
    profile_id: ServerProfileId,
    action: WindowsTaskAction,
) -> Result<(), BackgroundLaunchError> {
    let verb = match action {
        WindowsTaskAction::Run => "/Run",
        WindowsTaskAction::Delete => "/Delete",
    };
    let mut args = vec![
        verb.to_string(),
        "/TN".to_string(),
        windows_task_name(profile_id),
    ];
    if matches!(action, WindowsTaskAction::Delete) {
        args.push("/F".to_string());
    }
    let output = schtasks_command(&args)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(BackgroundLaunchError::SupervisorUnavailable)
    }
}

#[cfg(target_os = "windows")]
fn windows_register_task(definition: &WindowsTaskDefinition) -> Result<(), BackgroundLaunchError> {
    use std::fs::OpenOptions;
    use std::io::{Read, Write};
    use std::os::windows::fs::OpenOptionsExt;

    let staging = windows_startup_staging()?;
    let temporary = staging.path().join("task.xml");
    let expected = definition.to_xml().into_bytes();
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|_| BackgroundLaunchError::Failed)?;
    if file
        .write_all(&expected)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(BackgroundLaunchError::Failed);
    }
    drop(file);

    windows_owned_ancestry(&temporary)?;
    // FILE_SHARE_READ permits the native reader, while denying all replacement,
    // deletion and writes until consumption finishes. Verify this held handle.
    let mut guard = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&temporary)
        .map_err(|_| BackgroundLaunchError::Failed)?;
    if !guard
        .metadata()
        .map_err(|_| BackgroundLaunchError::Failed)?
        .is_file()
    {
        return Err(BackgroundLaunchError::LaunchDenied);
    }
    let mut actual = Vec::new();
    (&mut guard)
        .take(expected.len() as u64 + 1)
        .read_to_end(&mut actual)
        .map_err(|_| BackgroundLaunchError::Failed)?;
    if actual != expected {
        return Err(BackgroundLaunchError::LaunchDenied);
    }

    let args = vec![
        "/Create".to_string(),
        "/TN".to_string(),
        definition.task_name().to_string(),
        "/XML".to_string(),
        temporary.to_string_lossy().into_owned(),
        "/F".to_string(),
    ];
    let command_result = schtasks_command(&args);
    drop(guard);
    let cleanup_result = fs::remove_file(&temporary);
    if cleanup_result.is_err() {
        return Err(BackgroundLaunchError::Failed);
    }
    let output = command_result?;
    if output.status.success() {
        Ok(())
    } else {
        Err(BackgroundLaunchError::SupervisorUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    use tokio::time::sleep;

    struct FakeBackend {
        availability: Mutex<BackgroundClientAvailability>,
        start_mode: Mutex<Result<BackgroundStartMode, BackgroundLaunchError>>,
        starts: AtomicU64,
        active: AtomicUsize,
        max_active: AtomicUsize,
        autostart: Mutex<AutostartState>,
        allow_release: AtomicBool,
    }

    impl FakeBackend {
        fn new(
            availability: BackgroundClientAvailability,
            start_mode: Result<BackgroundStartMode, BackgroundLaunchError>,
        ) -> Arc<Self> {
            Arc::new(Self {
                availability: Mutex::new(availability),
                start_mode: Mutex::new(start_mode),
                starts: AtomicU64::new(0),
                active: AtomicUsize::new(0),
                max_active: AtomicUsize::new(0),
                autostart: Mutex::new(AutostartState::Disabled),
                allow_release: AtomicBool::new(true),
            })
        }
    }

    #[async_trait]
    impl BackgroundLaunchBackend for FakeBackend {
        async fn inspect(&self) -> BackgroundClientAvailability {
            *self.availability.lock().await
        }

        async fn request_start(&self) -> Result<BackgroundStartMode, BackgroundLaunchError> {
            self.starts.fetch_add(1, Ordering::Relaxed);
            let active = self
                .active
                .fetch_add(1, Ordering::Relaxed)
                .saturating_add(1);
            self.max_active.fetch_max(active, Ordering::Relaxed);
            while !self.allow_release.load(Ordering::Relaxed) {
                sleep(Duration::from_millis(1)).await;
            }
            self.active.fetch_sub(1, Ordering::Relaxed);
            self.start_mode.lock().await.to_owned()
        }

        async fn autostart_status(&self) -> Result<AutostartState, BackgroundLaunchError> {
            Ok(*self.autostart.lock().await)
        }

        async fn enable_autostart(&self) -> Result<AutostartState, BackgroundLaunchError> {
            *self.autostart.lock().await = AutostartState::Enabled;
            Ok(AutostartState::Enabled)
        }

        async fn disable_autostart(&self) -> Result<AutostartState, BackgroundLaunchError> {
            *self.autostart.lock().await = AutostartState::Disabled;
            Ok(AutostartState::Disabled)
        }
    }

    fn test_timing() -> BackgroundLaunchTiming {
        BackgroundLaunchTiming::new(
            Duration::from_millis(100),
            Duration::from_millis(20),
            Duration::from_millis(20),
        )
        .expect("test timing")
    }

    #[tokio::test]
    async fn launch1_construction_has_no_side_effects() {
        let backend = FakeBackend::new(
            BackgroundClientAvailability::Absent,
            Ok(BackgroundStartMode::Direct),
        );
        let manager = BackgroundClientManager::with_backend(
            ServerProfileId::new(),
            backend.clone(),
            test_timing(),
        );
        assert_eq!(manager.stats(), LaunchStats::default());
        assert_eq!(backend.starts.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn launch2_already_running_is_reused() {
        let backend = FakeBackend::new(
            BackgroundClientAvailability::Running,
            Ok(BackgroundStartMode::Direct),
        );
        let manager = BackgroundClientManager::with_backend(
            ServerProfileId::new(),
            backend.clone(),
            test_timing(),
        );
        assert_eq!(
            manager.ensure_running().await,
            BackgroundLaunchResult::AlreadyRunning
        );
        assert_eq!(backend.starts.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn launch3_absent_client_has_one_start_request() {
        let backend = FakeBackend::new(
            BackgroundClientAvailability::Absent,
            Ok(BackgroundStartMode::Requested),
        );
        let manager = BackgroundClientManager::with_backend(
            ServerProfileId::new(),
            backend.clone(),
            test_timing(),
        );
        assert_eq!(
            manager.ensure_running().await,
            BackgroundLaunchResult::StartRequested
        );
        assert_eq!(backend.starts.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn launch4_simultaneous_requests_coalesce() {
        let backend = FakeBackend::new(
            BackgroundClientAvailability::Absent,
            Ok(BackgroundStartMode::Direct),
        );
        backend.allow_release.store(false, Ordering::Relaxed);
        let manager = BackgroundClientManager::with_backend(
            ServerProfileId::new(),
            backend.clone(),
            test_timing(),
        );
        let first = {
            let manager = manager.clone();
            tokio::spawn(async move { manager.ensure_running().await })
        };
        for _ in 0..20 {
            if backend.starts.load(Ordering::Relaxed) == 1 {
                break;
            }
            sleep(Duration::from_millis(1)).await;
        }
        assert_eq!(
            manager.ensure_running().await,
            BackgroundLaunchResult::AlreadyStarting
        );
        backend.allow_release.store(true, Ordering::Relaxed);
        assert_eq!(
            first.await.expect("first launch"),
            BackgroundLaunchResult::StartedDirect
        );
        assert_eq!(backend.starts.load(Ordering::Relaxed), 1);
        assert_eq!(manager.stats().max_active_attempts, 1);
    }

    #[tokio::test]
    async fn launch5_thousand_requests_remain_bounded() {
        let backend = FakeBackend::new(
            BackgroundClientAvailability::Absent,
            Ok(BackgroundStartMode::Direct),
        );
        backend.allow_release.store(false, Ordering::Relaxed);
        let manager = BackgroundClientManager::with_backend(
            ServerProfileId::new(),
            backend.clone(),
            test_timing(),
        );
        let tasks = (0..1_000)
            .map(|_| {
                let manager = manager.clone();
                tokio::spawn(async move { manager.ensure_running().await })
            })
            .collect::<Vec<_>>();
        for _ in 0..50 {
            if backend.starts.load(Ordering::Relaxed) == 1 {
                break;
            }
            sleep(Duration::from_millis(1)).await;
        }
        backend.allow_release.store(true, Ordering::Relaxed);
        for task in tasks {
            let result = task.await.expect("bounded request");
            assert!(matches!(
                result,
                BackgroundLaunchResult::AlreadyStarting | BackgroundLaunchResult::StartedDirect
            ));
        }
        assert_eq!(backend.starts.load(Ordering::Relaxed), 1);
        assert_eq!(manager.stats().max_active_attempts, 1);
        assert_eq!(backend.max_active.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn launch6_security_failure_never_starts() {
        for availability in [
            BackgroundClientAvailability::EndpointSecurity,
            BackgroundClientAvailability::WriterConflict,
            BackgroundClientAvailability::UnsafeState,
        ] {
            let backend = FakeBackend::new(availability, Ok(BackgroundStartMode::Direct));
            let manager = BackgroundClientManager::with_backend(
                ServerProfileId::new(),
                backend.clone(),
                test_timing(),
            );
            assert_eq!(
                manager.ensure_running().await,
                BackgroundLaunchResult::UnsafeState
            );
            assert_eq!(backend.starts.load(Ordering::Relaxed), 0);
        }
    }

    #[tokio::test]
    async fn launch7_protocol_mismatch_never_starts() {
        let backend = FakeBackend::new(
            BackgroundClientAvailability::ProtocolIncompatible,
            Ok(BackgroundStartMode::Direct),
        );
        let manager = BackgroundClientManager::with_backend(
            ServerProfileId::new(),
            backend.clone(),
            test_timing(),
        );
        assert_eq!(
            manager.ensure_running().await,
            BackgroundLaunchResult::LaunchDenied
        );
        assert_eq!(backend.starts.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn launch8_failure_is_typed() {
        let backend = FakeBackend::new(
            BackgroundClientAvailability::Stopped,
            Err(BackgroundLaunchError::LaunchDenied),
        );
        let manager =
            BackgroundClientManager::with_backend(ServerProfileId::new(), backend, test_timing());
        assert_eq!(
            manager.ensure_running().await,
            BackgroundLaunchResult::LaunchDenied
        );
    }

    #[tokio::test]
    async fn launch9_manager_does_not_stop_on_gui_lifecycle() {
        let backend = FakeBackend::new(
            BackgroundClientAvailability::Running,
            Ok(BackgroundStartMode::Direct),
        );
        let manager = BackgroundClientManager::with_backend(
            ServerProfileId::new(),
            backend.clone(),
            test_timing(),
        );
        let _ = manager.ensure_running().await;
        assert_eq!(manager.stats().launch_attempts, 0);
        assert_eq!(backend.starts.load(Ordering::Relaxed), 0);
    }

    #[cfg(unix)]
    #[test]
    fn p043_client_link_outside_owned_siblings_is_rejected() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = std::env::temp_dir().join(format!("synveil-p043-{}", ServerProfileId::new()));
        fs::create_dir(&root).unwrap();
        let owned = root.join("owned");
        let outside = root.join("outside");
        fs::create_dir(&owned).unwrap();
        fs::create_dir(&outside).unwrap();
        let desktop = owned.join(SYNVEIL_DESKTOP_EXECUTABLE);
        let substitute = outside.join(SYNVEIL_CLIENT_EXECUTABLE);
        fs::write(&desktop, b"desktop").unwrap();
        fs::write(&substitute, b"substitute").unwrap();
        fs::set_permissions(&substitute, fs::Permissions::from_mode(0o755)).unwrap();
        symlink(&substitute, owned.join(SYNVEIL_CLIENT_EXECUTABLE)).unwrap();
        assert!(matches!(
            packaged_client_path_from(&desktop),
            Err(BackgroundLaunchError::NotInstalled)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn p043_windows_native_directories_ignore_environment_substitution() {
        const CHILD: &str = "SYNVEIL_P043_OS_DIRECTORY_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let fake = PathBuf::from(std::env::var_os("SystemRoot").unwrap());
            assert!(!schtasks_path().unwrap().starts_with(&fake));
            let staging = windows_startup_staging().unwrap();
            assert!(!staging.path().starts_with(&fake));
            return;
        }
        // Isolate hostile environment values in a child, keeping other tests safe.
        let fake = tempfile::tempdir().unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "launch::tests::p043_windows_native_directories_ignore_environment_substitution",
            ])
            .env(CHILD, "1")
            .env("SystemRoot", fake.path())
            .env("TEMP", fake.path())
            .env("TMP", fake.path())
            .env("LOCALAPPDATA", fake.path())
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[cfg(windows)]
    #[test]
    fn p043_windows_read_guard_prevents_native_xml_handoff_replacement() {
        use std::os::windows::fs::OpenOptionsExt;
        let staging = windows_startup_staging().unwrap();
        let path = staging.path().join("task.xml");
        fs::write(&path, b"verified owned XML").unwrap();
        let guard = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        assert!(fs::write(&path, b"attacker replacement").is_err());
        assert!(fs::remove_file(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"verified owned XML");
        drop(guard);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn launch10_canonical_sibling_path_is_required() {
        let root =
            std::env::temp_dir().join(format!("synveil-launch-path-{}", ServerProfileId::new()));
        fs::create_dir_all(&root).expect("path root");
        let desktop = root.join(if cfg!(windows) {
            "synveil-desktop.exe"
        } else {
            SYNVEIL_DESKTOP_EXECUTABLE
        });
        let client = root.join(if cfg!(windows) {
            "synveil-client.exe"
        } else {
            SYNVEIL_CLIENT_EXECUTABLE
        });
        fs::write(&desktop, b"desktop").expect("desktop fixture");
        fs::write(&client, b"client").expect("client fixture");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [&desktop, &client] {
                let mut permissions = fs::metadata(path).expect("metadata").permissions();
                permissions.set_mode(0o755);
                fs::set_permissions(path, permissions).expect("executable fixture");
            }
        }
        let resolved = packaged_client_path_from(&desktop).expect("sibling client");
        assert_eq!(
            resolved,
            fs::canonicalize(client).expect("canonical client")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn launch11_path_spoof_cannot_replace_packaged_name() {
        let root =
            std::env::temp_dir().join(format!("synveil-launch-spoof-{}", ServerProfileId::new()));
        fs::create_dir_all(&root).expect("path root");
        let desktop = root.join("developer-desktop");
        let client = root.join("client-from-path");
        fs::write(&desktop, b"desktop").expect("desktop fixture");
        fs::write(&client, b"client").expect("client fixture");
        assert_eq!(
            packaged_client_path_from(&desktop),
            Err(BackgroundLaunchError::NotInstalled)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn launch12_profile_identity_is_not_in_an_argument_vector() {
        let profile = ServerProfileId::new();
        let task_name = windows_task_name(profile);
        assert!(task_name.starts_with(r"\Synveil\BackgroundClient\profile-"));
        assert!(task_name.contains(&profile.to_string()));
        assert!(!task_name.contains("--"));
        assert!(!task_name.contains("http"));
        assert!(!task_name.contains("/"));
    }

    #[tokio::test]
    async fn autostart_enable_disable_are_explicit_and_reversible() {
        let backend = FakeBackend::new(
            BackgroundClientAvailability::Running,
            Ok(BackgroundStartMode::Direct),
        );
        let manager =
            BackgroundClientManager::with_backend(ServerProfileId::new(), backend, test_timing());
        assert_eq!(
            manager.autostart_status().await.unwrap(),
            AutostartState::Disabled
        );
        assert_eq!(
            manager.enable_autostart().await.unwrap(),
            AutostartState::Enabled
        );
        assert_eq!(
            manager.enable_autostart().await.unwrap(),
            AutostartState::Enabled
        );
        assert_eq!(
            manager.autostart_status().await.unwrap(),
            AutostartState::Enabled
        );
        assert_eq!(
            manager.disable_autostart().await.unwrap(),
            AutostartState::Disabled
        );
        assert_eq!(
            manager.disable_autostart().await.unwrap(),
            AutostartState::Disabled
        );
        assert_eq!(
            manager.autostart_status().await.unwrap(),
            AutostartState::Disabled
        );
    }

    #[test]
    fn platform_windows_username_removes_only_the_api_terminator() {
        let buffer: Vec<_> = "Nguyễn & User\0".encode_utf16().collect();
        assert_eq!(
            decode_windows_username(&buffer, buffer.len()).unwrap(),
            "Nguyễn & User"
        );
        for (buffer, length) in [
            (vec![0], 1),
            (vec![65], 1),
            (vec![65, 0], 3),
            (vec![65, 0, 66, 0], 4),
            (vec![0xd800, 0], 2),
        ] {
            assert!(decode_windows_username(&buffer, length).is_err());
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn platform_supervised_stop_requests_canonical_shutdown_over_ipc() {
        use std::os::unix::fs::PermissionsExt;
        use synveil_client_sync::{
            DesktopSyncHost, DesktopSyncHostConfig, LocalStateConfig, LocalStateStore,
        };
        let root =
            std::path::PathBuf::from("/tmp").join(format!("sv116-{}", ServerProfileId::new()));
        fs::create_dir_all(root.join("runtime/control")).unwrap();
        fs::set_permissions(root.join("runtime"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(
            root.join("runtime/control"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let state = Arc::new(
            LocalStateStore::open(&LocalStateConfig::new(root.join("state.sqlite3")))
                .await
                .unwrap(),
        );
        let host = DesktopSyncHost::new(
            state.clone(),
            Arc::new(synveil_platform::UnsupportedSecureSecretStore::new()),
            DesktopSyncHostConfig::default(),
            Vec::new(),
        )
        .await
        .unwrap();
        host.start().await.unwrap();
        let control =
            crate::DesktopControlHandle::new(host.handle(), DesktopProcessStatus::Running);
        let endpoint = DesktopControlEndpoint::UnixSocket {
            path: root.join("runtime/control/client.sock"),
        };
        let mut server = crate::DesktopControlServer::bind(endpoint.clone())
            .await
            .unwrap();
        server.start(control.clone()).unwrap();
        request_graceful_client_stop(endpoint.clone())
            .await
            .unwrap();
        time::timeout(Duration::from_secs(1), control.wait_for_shutdown_request())
            .await
            .unwrap();
        // The helper requests shutdown; the process owner drains and releases
        // resources through the existing host/server path.
        server.stop().await.unwrap();
        host.shutdown().await.unwrap();
        assert!(!endpoint.unix_path().unwrap().exists());
        state.close_pool().await;
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn windows_task_xml_decoding_preserves_endianness_and_rejects_malformed_utf16() {
        let expected = "<Task>Nguyễn 😀</Task>";
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend(expected.encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(decode_task_xml(&bytes).as_deref(), Some(expected));
        assert_eq!(decode_task_xml(&[0xff, 0xfe, 0x41]), None);
        assert_eq!(decode_task_xml(&[0xff, 0xfe, 0x00, 0xd8]), None);
        assert_eq!(decode_task_xml(&[0xfe, 0xff, 0x00, 0x41]), None);
    }

    #[test]
    fn windows_task_definition_is_user_scoped_bounded_and_secret_free() {
        let root = std::env::temp_dir().join(format!(
            "synveil-task-definition-{}",
            ServerProfileId::new()
        ));
        fs::create_dir_all(&root).expect("task root");
        let client = root.join("synveil-client.exe");
        fs::write(&client, b"client").expect("client fixture");
        let profile = ServerProfileId::new();
        let definition =
            WindowsTaskDefinition::new(profile, &client, "CURRENT_USER").expect("task definition");
        let xml = definition.to_xml();
        assert_eq!(
            decode_task_xml(xml.as_bytes()).as_deref(),
            Some(xml.as_str())
        );
        let utf16 = xml
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let mut utf16_bom = vec![0xff, 0xfe];
        utf16_bom.extend(utf16);
        assert_eq!(decode_task_xml(&utf16_bom).as_deref(), Some(xml.as_str()));
        assert_eq!(definition.task_name(), windows_task_name(profile));
        assert!(xml.contains("<LogonTrigger>"));
        // The triggerBaseType sequence precedes the logon-specific UserId.
        assert!(xml.contains("<LogonTrigger><Enabled>true</Enabled><UserId>CURRENT_USER</UserId>"));
        assert!(xml.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(xml.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"));
        assert!(xml.contains("<Interval>PT30S</Interval><Count>5</Count>"));
        assert!(!xml.contains("<Password>"));
        assert!(!xml.contains("/RU"));
        assert!(!xml.contains("DATABASE_URL"));
        assert!(!definition.task_name().contains("CURRENT_USER"));
        assert!(windows_task_xml_is_authoritative(&xml, &definition));
        for stale in [
            xml.replace("synveil-client.exe", "other.exe"),
            xml.replace("CURRENT_USER", "OTHER_USER"),
            xml.replace("LeastPrivilege", "HighestAvailable"),
            xml.replace("<LogonTrigger>", "<BootTrigger>"),
            xml.replace("</Exec>", "<Arguments>--unsafe</Arguments></Exec>"),
        ] {
            assert!(!windows_task_xml_is_authoritative(&stale, &definition));
        }
        let escaped = WindowsTaskDefinition::new(profile, &client, "User & <name>")
            .expect("XML principal")
            .to_xml();
        assert!(escaped.contains("User &amp; &lt;name&gt;"));
        for invalid in ["user\0", "user\u{1}", "user\r", "user\n"] {
            assert_eq!(
                WindowsTaskDefinition::new(profile, &client, invalid),
                Err(BackgroundLaunchError::LaunchDenied)
            );
        }
        let _ = fs::remove_dir_all(root);
    }
}

#[cfg(test)]
mod p042_tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use tokio::sync::Notify;

    struct Backend {
        running: AtomicBool,
        stopped: AtomicBool,
        supports: bool,
        stop_uncertain: bool,
        start_uncertain: bool,
        block_stop: bool,
        entered: Notify,
        release: Notify,
        stops: AtomicUsize,
        starts: AtomicUsize,
    }
    impl Backend {
        fn new() -> Self {
            Self {
                running: AtomicBool::new(true),
                stopped: AtomicBool::new(true),
                supports: true,
                stop_uncertain: false,
                start_uncertain: false,
                block_stop: false,
                entered: Notify::new(),
                release: Notify::new(),
                stops: AtomicUsize::new(0),
                starts: AtomicUsize::new(0),
            }
        }
    }
    #[async_trait]
    impl BackgroundLaunchBackend for Backend {
        async fn inspect(&self) -> BackgroundClientAvailability {
            if self.running.load(Ordering::SeqCst) {
                BackgroundClientAvailability::Running
            } else {
                BackgroundClientAvailability::Stopped
            }
        }
        async fn restart_supported(&self) -> bool {
            self.supports
        }
        async fn stopped_for_restart(&self) -> bool {
            self.stopped.load(Ordering::SeqCst)
        }
        async fn stop_supervised(&self) -> Result<(), BackgroundLaunchError> {
            self.stops.fetch_add(1, Ordering::SeqCst);
            self.entered.notify_one();
            if self.block_stop {
                self.release.notified().await;
            }
            self.running.store(false, Ordering::SeqCst);
            if self.stop_uncertain {
                Err(BackgroundLaunchError::Failed)
            } else {
                Ok(())
            }
        }
        async fn request_start(&self) -> Result<BackgroundStartMode, BackgroundLaunchError> {
            self.starts.fetch_add(1, Ordering::SeqCst);
            self.running.store(!self.start_uncertain, Ordering::SeqCst);
            Ok(BackgroundStartMode::Supervised)
        }
    }
    fn manager(backend: Arc<Backend>) -> BackgroundClientManager {
        BackgroundClientManager::with_backend(
            ServerProfileId::new(),
            backend,
            BackgroundLaunchTiming::default(),
        )
    }
    async fn allow_inspection(manager: &BackgroundClientManager) {
        // Deterministically advance admission; production retains its cooldown.
        manager.gate.lock().await.retry_after = None;
    }
    #[test]
    fn p042_restart_requires_a_running_managed_process_identity() {
        let running = "LoadState=loaded\nActiveState=active\nMainPID=42";
        assert_eq!(linux_running_service_pid(running), Some(42));
        for uncertain in [
            running.replace("MainPID=42", "MainPID=0"),
            running.replace("active", "deactivating"),
            running.replace("loaded", "not-found"),
            running.replace("MainPID=42", ""),
        ] {
            assert_eq!(linux_running_service_pid(&uncertain), None);
        }
    }
    #[test]
    fn p042_partial_or_deactivating_supervisor_state_is_not_stop_proof() {
        let stopped =
            "LoadState=loaded\nActiveState=inactive\nSubState=dead\nMainPID=0\nControlPID=0";
        assert!(linux_restart_stop_proven(stopped));
        for uncertain in [
            stopped.replace("inactive", "deactivating"),
            stopped.replace("MainPID=0", "MainPID=2"),
            stopped.replace("ControlPID=0", ""),
            stopped.replace("loaded", "not-found"),
        ] {
            assert!(!linux_restart_stop_proven(&uncertain));
        }
    }
    #[tokio::test]
    async fn p042_restart_requires_stop_proof_and_running_readiness() {
        let b = Arc::new(Backend::new());
        let m = manager(b.clone());
        assert_eq!(m.restart().await, BackgroundRestartResult::Recovered);
        assert_eq!(b.stops.load(Ordering::SeqCst), 1);
        assert_eq!(b.starts.load(Ordering::SeqCst), 1);
        assert_eq!(m.restart().await, BackgroundRestartResult::Busy);
    }
    #[tokio::test]
    async fn p042_unknown_stop_is_reconciled_without_repeating_shutdown() {
        let mut b = Backend::new();
        b.stop_uncertain = true;
        b.stopped.store(false, Ordering::SeqCst);
        let b = Arc::new(b);
        let m = manager(b.clone());
        assert_eq!(m.restart().await, BackgroundRestartResult::Reconciling);
        assert_eq!(b.starts.load(Ordering::SeqCst), 0);
        allow_inspection(&m).await;
        assert_eq!(
            m.ensure_running().await,
            BackgroundLaunchResult::AlreadyStarting
        );
        assert_eq!(
            m.check_restart_status().await,
            BackgroundRestartResult::Reconciling
        );
        assert_eq!(b.stops.load(Ordering::SeqCst), 1);
        b.stopped.store(true, Ordering::SeqCst);
        allow_inspection(&m).await;
        assert_eq!(
            m.check_restart_status().await,
            BackgroundRestartResult::Recovered
        );
        assert_eq!(b.stops.load(Ordering::SeqCst), 1);
        assert_eq!(b.starts.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn p042_unknown_start_never_replays_launch() {
        let mut b = Backend::new();
        b.start_uncertain = true;
        let b = Arc::new(b);
        let m = manager(b.clone());
        assert_eq!(m.restart().await, BackgroundRestartResult::Reconciling);
        allow_inspection(&m).await;
        assert_eq!(
            m.check_restart_status().await,
            BackgroundRestartResult::Reconciling
        );
        assert_eq!(b.starts.load(Ordering::SeqCst), 1);
        b.running.store(true, Ordering::SeqCst);
        assert!(m.reconcile_restart_readiness().await);
        assert!(!m.reconcile_restart_readiness().await);
        assert_eq!(b.starts.load(Ordering::SeqCst), 1);
        assert_eq!(b.stops.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn p042_status_check_without_pending_never_starts_stopped_client() {
        let b = Arc::new(Backend::new());
        b.running.store(false, Ordering::SeqCst);
        let m = manager(b.clone());
        assert_eq!(
            m.check_restart_status().await,
            BackgroundRestartResult::Failed
        );
        assert_eq!(b.starts.load(Ordering::SeqCst), 0);
        assert_eq!(b.stops.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn p042_status_check_after_readiness_never_restarts() {
        let mut b = Backend::new();
        b.start_uncertain = true;
        let b = Arc::new(b);
        let m = manager(b.clone());
        assert_eq!(m.restart().await, BackgroundRestartResult::Reconciling);
        b.running.store(true, Ordering::SeqCst);
        assert!(m.reconcile_restart_readiness().await);
        allow_inspection(&m).await;
        assert_eq!(
            m.check_restart_status().await,
            BackgroundRestartResult::Recovered
        );
        assert_eq!(b.starts.load(Ordering::SeqCst), 1);
        assert_eq!(b.stops.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn p042_restart_and_start_share_one_admission() {
        let mut b = Backend::new();
        b.block_stop = true;
        let b = Arc::new(b);
        let m = manager(b.clone());
        let other = m.clone();
        let task = tokio::spawn(async move { other.restart().await });
        b.entered.notified().await;
        assert_eq!(m.restart().await, BackgroundRestartResult::Busy);
        assert_eq!(
            m.ensure_running().await,
            BackgroundLaunchResult::AlreadyStarting
        );
        b.release.notify_one();
        assert_eq!(task.await.unwrap(), BackgroundRestartResult::Recovered);
        assert_eq!(b.stops.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn p042_unsupported_restart_is_guidance_without_mutation() {
        let mut b = Backend::new();
        b.supports = false;
        let b = Arc::new(b);
        let m = manager(b.clone());
        assert!(!m.restart_supported().await);
        assert_eq!(m.restart().await, BackgroundRestartResult::GuidanceOnly);
        assert_eq!(b.starts.load(Ordering::SeqCst), 0);
        assert_eq!(b.stops.load(Ordering::SeqCst), 0);
    }
    #[tokio::test(start_paused = true)]
    async fn p042_timeout_retains_stop_reconciliation_before_launch() {
        let mut b = Backend::new();
        b.block_stop = true;
        b.stopped.store(false, Ordering::SeqCst);
        let b = Arc::new(b);
        let m = manager(b.clone());
        assert_eq!(m.restart().await, BackgroundRestartResult::Reconciling);
        allow_inspection(&m).await;
        assert_eq!(m.restart().await, BackgroundRestartResult::Reconciling);
        assert_eq!(b.starts.load(Ordering::SeqCst), 0);
        assert_eq!(b.stops.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn p042_gui_reopen_does_not_replay_restart() {
        let b = Arc::new(Backend::new());
        let reopened = manager(b.clone());
        assert_eq!(
            reopened.ensure_running().await,
            BackgroundLaunchResult::AlreadyRunning
        );
        assert_eq!(b.stops.load(Ordering::SeqCst), 0);
        assert_eq!(b.starts.load(Ordering::SeqCst), 0);
    }
}
