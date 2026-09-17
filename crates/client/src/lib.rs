#![cfg_attr(not(target_os = "windows"), forbid(unsafe_code))]

//! Production foreground desktop/client process bootstrap.
//!
//! This crate owns only process lifecycle, non-secret configuration, and
//! platform hint adapters. Synchronization composition remains in the single
//! accepted `synveil-client-sync::DesktopSyncHost` root.

mod config;
mod control;
mod controller;
mod launch;
mod lifecycle;
mod network;
mod process;

pub use config::{
    DEFAULT_DESKTOP_CLIENT_CONFIG_FILE, DesktopClientConfig, DesktopClientConfigError,
    DesktopClientLibrary,
};
pub use control::{
    ControlAuthState, ControlCapability, ControlClient, ControlClientError, ControlClientHello,
    ControlCommand, ControlConflictState, ControlEndpointKind, ControlErrorCode, ControlEvent,
    ControlEventStream, ControlFrameError, ControlLibraryList, ControlLibraryStatus,
    ControlProcessStatus, ControlRequest, ControlResponse, ControlResponseBody, ControlRootState,
    ControlRuntimeState, ControlServer, ControlServerHello, ControlSyncOutcome,
    ControlSyncScheduleResult, DESKTOP_CONTROL_EVENT_CAPACITY, DESKTOP_CONTROL_IPC_READINESS,
    DESKTOP_CONTROL_MAX_CONNECTIONS, DESKTOP_CONTROL_MAX_FRAME_BYTES,
    DESKTOP_CONTROL_PROTOCOL_VERSION, DesktopControlClient, DesktopControlEndpoint,
    DesktopControlEventStream, DesktopControlHandle, DesktopControlServer,
    DesktopControlServerError, encode_frame, read_frame, write_frame,
};
pub use controller::{
    DEFAULT_DESKTOP_CONTROLLER_COMMAND_TIMEOUT, DEFAULT_DESKTOP_CONTROLLER_OPERATION_TIMEOUT,
    DEFAULT_DESKTOP_CONTROLLER_RECONNECT_CAP, DEFAULT_DESKTOP_CONTROLLER_RECONNECT_INITIAL,
    DEFAULT_DESKTOP_CONTROLLER_REFRESH_INTERVAL, DESKTOP_CONTROLLER_COMMAND_CAPACITY,
    DESKTOP_CONTROLLER_CORE_READINESS, DesktopController, DesktopControllerAuthState,
    DesktopControllerCommandResult, DesktopControllerConfig, DesktopControllerConflictState,
    DesktopControllerConnectionState, DesktopControllerError, DesktopControllerErrorKind,
    DesktopControllerFreshness, DesktopControllerLibraryStatus, DesktopControllerProcessStatus,
    DesktopControllerRootState, DesktopControllerRuntimeState, DesktopControllerSnapshot,
    DesktopControllerSyncOutcome, DesktopControllerTiming,
};
pub use launch::{
    AutostartState, BackgroundClientAvailability, BackgroundClientManager, BackgroundLaunchBackend,
    BackgroundLaunchError, BackgroundLaunchResult, BackgroundLaunchTiming, BackgroundStartMode,
    DESKTOP_LAUNCH_MANAGER_READINESS, LINUX_USER_SERVICE_NAME, LaunchStats,
    NativeBackgroundLaunchBackend, SupervisorState, WindowsTaskDefinition,
    packaged_client_path_from,
};
pub use lifecycle::{NativeProcessLifecycleSource, ProcessLifecycleSource};
pub use network::{
    DEFAULT_DESKTOP_NETWORK_HINT_INTERVAL, NativeNetworkHintSource, NetworkHintSource,
    PeriodicNetworkHintSource,
};
pub use process::{
    DesktopClientProcess, DesktopProcessError, DesktopProcessRunner, DesktopProcessStatus,
};
pub use synveil_client_sync::{
    DesktopLifecycleEvent, DesktopSyncHost, DesktopSyncHostHandle, RootAvailability,
    ServerProfileId,
};
pub use synveil_core::LibraryId;

use std::{process::ExitCode, sync::Arc};

use synveil_platform::PlatformRuntime;
use tracing::{error, info};

/// Exit status for a valid executable whose non-secret process configuration
/// cannot be loaded. The Linux user service uses this value to avoid an
/// endless restart loop for permanent configuration failure.
pub const DESKTOP_CLIENT_CONFIG_EXIT_CODE: u8 = 78;

/// Generic bootstrap/runtime failure. Supervisor restart policy bounds retries
/// for this class while the controller reports the process as unavailable.
pub const DESKTOP_CLIENT_RUNTIME_EXIT_CODE: u8 = 70;

/// Async process entry used by `main_entry` and integration tests.
pub async fn run_desktop_client(
    platform: Arc<dyn PlatformRuntime>,
    config: DesktopClientConfig,
) -> Result<(), DesktopProcessError> {
    let mut process = DesktopClientProcess::bootstrap(platform, config).await?;
    process.run_until_shutdown().await
}

/// Thin binary entrypoint: platform/config acquisition, logging, one Tokio
/// runtime boundary, process runner invocation, and exit-code mapping.
pub fn main_entry() -> ExitCode {
    init_logging();
    let platform: Arc<dyn PlatformRuntime> = Arc::from(synveil_platform::current());
    let config = match DesktopClientConfig::from_platform(platform.as_ref()) {
        Ok(config) => config,
        Err(error) => {
            error!(error = %error, "desktop process configuration failed");
            return ExitCode::from(DESKTOP_CLIENT_CONFIG_EXIT_CODE);
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            error!("desktop process async runtime initialization failed");
            return ExitCode::from(DESKTOP_CLIENT_RUNTIME_EXIT_CODE);
        }
    };
    match runtime.block_on(run_desktop_client(platform, config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error!(error = %error, "desktop process terminated with a bootstrap/runtime failure");
            ExitCode::from(DESKTOP_CLIENT_RUNTIME_EXIT_CODE)
        }
    }
}

fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("synveil_client=info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
    info!("desktop process diagnostics initialized");
}

/// Adapter failures expose only stable safe categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopAdapterError {
    Initialization,
    Closed,
    Stopped,
}

impl DesktopAdapterError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Initialization => "DESKTOP_ADAPTER_INITIALIZATION_FAILED",
            Self::Closed => "DESKTOP_ADAPTER_CLOSED",
            Self::Stopped => "DESKTOP_ADAPTER_STOPPED",
        }
    }
}

impl std::fmt::Display for DesktopAdapterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for DesktopAdapterError {}
