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

#[derive(Default)]
struct LaunchGate {
    in_flight: bool,
    retry_after: Option<Instant>,
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
            if gate.in_flight || gate.retry_after.is_some_and(|until| until > Instant::now()) {
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
            | BackgroundClientAvailability::WriterConflict => BackgroundLaunchResult::UnsafeState,
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
            return match self.linux_supervisor_state() {
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
                WindowsTaskState::Registered => BackgroundClientAvailability::SupervisorInactive,
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
            return match self.linux_supervisor_state() {
                LinuxSupervisorState::LoadedActive | LinuxSupervisorState::LoadedInactive => {
                    systemctl_user_action("start").map(|()| BackgroundStartMode::Supervised)
                }
                LinuxSupervisorState::Absent | LinuxSupervisorState::Unavailable => {
                    self.direct_start()
                }
            };
        }

        #[cfg(target_os = "windows")]
        {
            return match self.windows_task_state() {
                WindowsTaskState::Registered => {
                    windows_task_action(self.profile_id, WindowsTaskAction::Run)
                        .map(|()| BackgroundStartMode::Supervised)
                }
                WindowsTaskState::Absent | WindowsTaskState::Unavailable => self.direct_start(),
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
                WindowsTaskState::Registered => AutostartState::Enabled,
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
                WindowsTaskState::Registered => Ok(AutostartState::Enabled),
                WindowsTaskState::Absent | WindowsTaskState::Unavailable => {
                    let definition = native_windows_task_definition(self.profile_id)?;
                    windows_register_task(&definition)?;
                    Ok(AutostartState::Enabled)
                }
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
                WindowsTaskState::Registered => {
                    windows_task_action(self.profile_id, WindowsTaskAction::Delete)?;
                    Ok(AutostartState::Disabled)
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
                WindowsTaskState::Registered => {
                    windows_task_action(self.profile_id, WindowsTaskAction::Run)
                        .map(|()| BackgroundStartMode::Supervised)
                }
                WindowsTaskState::Absent => Err(BackgroundLaunchError::NotInstalled),
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
                WindowsTaskState::Registered => {
                    windows_task_action(self.profile_id, WindowsTaskAction::Stop)
                }
                WindowsTaskState::Absent => Ok(()),
                WindowsTaskState::Unavailable => Err(BackgroundLaunchError::SupervisorUnavailable),
            };
        }
        #[allow(unreachable_code)]
        Err(BackgroundLaunchError::Unsupported)
    }
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
            || principal.contains(['\r', '\n'])
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
  <Triggers><LogonTrigger><Enabled>true</Enabled></LogonTrigger></Triggers>\n\
  <Principals><Principal id=\"SynveilUser\"><UserId>{}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>\n\
  <Settings>\n\
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\n\
    <RestartOnFailure><Interval>PT30S</Interval><Count>5</Count></RestartOnFailure>\n\
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit><StartWhenAvailable>true</StartWhenAvailable>\n\
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\n\
  </Settings>\n\
  <Actions Context=\"SynveilUser\"><Exec><Command>{command}</Command><WorkingDirectory>{working_directory}</WorkingDirectory></Exec></Actions>\n\
</Task>\n",
            xml_escape(&self.principal)
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
    Registered,
    Absent,
    Unavailable,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsTaskAction {
    Run,
    Stop,
    Delete,
}

#[cfg(target_os = "windows")]
fn schtasks_path() -> PathBuf {
    std::env::var_os("SystemRoot")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join("schtasks.exe")
}

#[cfg(target_os = "windows")]
fn schtasks_command(args: &[String]) -> Result<std::process::Output, BackgroundLaunchError> {
    Command::new(schtasks_path())
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| BackgroundLaunchError::SupervisorUnavailable)
}

#[cfg(target_os = "windows")]
fn windows_task_state(profile_id: ServerProfileId) -> WindowsTaskState {
    let task_name = windows_task_name(profile_id);
    let args = vec![
        "/Query".to_string(),
        "/TN".to_string(),
        task_name,
        "/FO".to_string(),
        "LIST".to_string(),
        "/NH".to_string(),
    ];
    match schtasks_command(&args) {
        Ok(output) if output.status.success() => WindowsTaskState::Registered,
        Ok(_) => WindowsTaskState::Absent,
        Err(_) => WindowsTaskState::Unavailable,
    }
}

#[cfg(target_os = "windows")]
fn current_windows_user() -> Result<String, BackgroundLaunchError> {
    use windows_sys::Win32::System::WindowsProgramming::GetUserNameW;
    let mut buffer = [0_u16; 512];
    let mut length = buffer.len() as u32;
    // SAFETY: buffer is valid writable storage and the length includes its
    // capacity as required by GetUserNameW. No user-controlled pointer exists.
    let success = unsafe { GetUserNameW(buffer.as_mut_ptr(), &mut length) } != 0;
    if !success || length == 0 {
        return Err(BackgroundLaunchError::SupervisorUnavailable);
    }
    String::from_utf16(&buffer[..length as usize])
        .map_err(|_| BackgroundLaunchError::SupervisorUnavailable)
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
        WindowsTaskAction::Stop => "/End",
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
    use std::io::Write;

    let temporary = std::env::temp_dir().join(format!(
        "synveil-task-{}-{}.xml",
        std::process::id(),
        definition
            .task_name()
            .rsplit('-')
            .next()
            .unwrap_or("profile")
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|_| BackgroundLaunchError::Failed)?;
    file.write_all(definition.to_xml().as_bytes())
        .map_err(|_| BackgroundLaunchError::Failed)?;
    file.sync_all().map_err(|_| BackgroundLaunchError::Failed)?;
    drop(file);

    let args = vec![
        "/Create".to_string(),
        "/TN".to_string(),
        definition.task_name().to_string(),
        "/XML".to_string(),
        temporary.to_string_lossy().into_owned(),
        "/F".to_string(),
    ];
    let command_result = schtasks_command(&args);
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
        let backend = FakeBackend::new(
            BackgroundClientAvailability::EndpointSecurity,
            Ok(BackgroundStartMode::Direct),
        );
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

    #[test]
    fn launch10_canonical_sibling_path_is_required() {
        let root =
            std::env::temp_dir().join(format!("synveil-launch-path-{}", ServerProfileId::new()));
        fs::create_dir_all(&root).expect("path root");
        let desktop = root.join(SYNVEIL_DESKTOP_EXECUTABLE);
        let client = root.join(SYNVEIL_CLIENT_EXECUTABLE);
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
            manager.autostart_status().await.unwrap(),
            AutostartState::Enabled
        );
        assert_eq!(
            manager.disable_autostart().await.unwrap(),
            AutostartState::Disabled
        );
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
        assert_eq!(definition.task_name(), windows_task_name(profile));
        assert!(xml.contains("<LogonTrigger>"));
        assert!(xml.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(xml.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"));
        assert!(xml.contains("<Interval>PT30S</Interval><Count>5</Count>"));
        assert!(!xml.contains("<Password>"));
        assert!(!xml.contains("/RU"));
        assert!(!xml.contains("DATABASE_URL"));
        assert!(!definition.task_name().contains("CURRENT_USER"));
        let _ = fs::remove_dir_all(root);
    }
}
