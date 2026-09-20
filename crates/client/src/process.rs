//! Production foreground desktop process runner.

use std::{fmt, sync::Arc, time::Duration};

use crate::DesktopAdapterError;
use serde::{Deserialize, Serialize};
#[cfg(feature = "test-support")]
use synveil_client_sync::DesktopRootRecoveryGate;
use synveil_client_sync::{
    ClientSyncError, DesktopLifecycleEvent, DesktopSyncHost, DesktopSyncHostError,
    DesktopSyncHostHandle, RootAvailability, SyncRuntimeWakeResult,
};
use synveil_platform::PlatformRuntime;
use tracing::{error, info, warn};

use crate::{
    DesktopClientConfig, DesktopClientConfigError, DesktopSyncPauseStore, NativeNetworkHintSource,
    NativeProcessLifecycleSource, NetworkHintSource, PeriodicNetworkHintSource,
    ProcessLifecycleSource,
};

/// Ephemeral production process status. It is not persisted in SQLite.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DesktopProcessStatus {
    Starting,
    Running,
    Stopping,
    Stopped,
    Faulted,
}

impl DesktopProcessStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "STARTING",
            Self::Running => "RUNNING",
            Self::Stopping => "STOPPING",
            Self::Stopped => "STOPPED",
            Self::Faulted => "FAULTED",
        }
    }
}

impl fmt::Display for DesktopProcessStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Safe process bootstrap/runtime error categories.
#[derive(Debug)]
pub enum DesktopProcessError {
    Configuration(DesktopClientConfigError),
    State(ClientSyncError),
    Host(DesktopSyncHostError),
    Lifecycle(DesktopAdapterError),
    Network(DesktopAdapterError),
    Control(crate::DesktopControlServerError),
    InvalidState,
}

impl DesktopProcessError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Configuration(error) => error_code(error),
            Self::State(error) => error.code(),
            Self::Host(_) => "DESKTOP_HOST_FAILURE",
            Self::Lifecycle(error) => error.code(),
            Self::Network(error) => error.code(),
            Self::Control(error) => error.code(),
            Self::InvalidState => "DESKTOP_PROCESS_STATE_INVALID",
        }
    }
}

impl fmt::Display for DesktopProcessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(error) => formatter.write_str(&error.to_string()),
            Self::State(error) => formatter.write_str(error.code()),
            Self::Host(error) => formatter.write_str(&error.to_string()),
            Self::Lifecycle(error) | Self::Network(error) => formatter.write_str(error.code()),
            Self::Control(error) => formatter.write_str(error.code()),
            Self::InvalidState => formatter.write_str("DESKTOP_PROCESS_STATE_INVALID"),
        }
    }
}

impl std::error::Error for DesktopProcessError {}

impl From<ClientSyncError> for DesktopProcessError {
    fn from(error: ClientSyncError) -> Self {
        Self::State(error)
    }
}

impl From<DesktopSyncHostError> for DesktopProcessError {
    fn from(error: DesktopSyncHostError) -> Self {
        Self::Host(error)
    }
}

impl From<DesktopClientConfigError> for DesktopProcessError {
    fn from(error: DesktopClientConfigError) -> Self {
        Self::Configuration(error)
    }
}

impl From<crate::DesktopControlServerError> for DesktopProcessError {
    fn from(error: crate::DesktopControlServerError) -> Self {
        Self::Control(error)
    }
}

/// One process-owned runner around exactly one Prompt 94 host.
pub struct DesktopClientProcess {
    host: DesktopSyncHost,
    lifecycle: Box<dyn ProcessLifecycleSource>,
    network: Box<dyn NetworkHintSource>,
    network_hint_interval: Duration,
    control: crate::DesktopControlHandle,
    control_server: Option<crate::DesktopControlServer>,
}

impl fmt::Debug for DesktopClientProcess {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopClientProcess")
            .field("status", &self.status())
            .field("host_lifecycle", &self.host.lifecycle())
            .field("runtime_identity", &self.host.runtime_identity())
            .finish_non_exhaustive()
    }
}

impl DesktopClientProcess {
    /// Open state, materialize profile-bound libraries, compose the existing
    /// host, initialize foreground adapters, and start the host. No lower-level
    /// synchronization engine is constructed in this crate.
    pub async fn bootstrap(
        platform: Arc<dyn PlatformRuntime>,
        config: DesktopClientConfig,
    ) -> Result<Self, DesktopProcessError> {
        let state_config = synveil_client_sync::LocalStateConfig::from_platform(platform.as_ref())?;
        let sync_pause_store = DesktopSyncPauseStore::for_platform(platform.as_ref())?;
        let sync_paused = config.sync_paused();
        let state = Arc::new(
            synveil_client_sync::LocalStateStore::open(&state_config)
                .await
                .map_err(DesktopProcessError::State)?,
        );
        let libraries = match config.materialize_libraries(state.as_ref()).await {
            Ok(libraries) => libraries,
            Err(error) => {
                state.close_pool().await;
                return Err(error.into());
            }
        };
        let host = match DesktopSyncHost::from_platform_with_state(
            Arc::clone(&platform),
            state,
            config.host(),
            libraries,
        )
        .await
        {
            Ok(host) => host,
            Err(error) => return Err(error.into()),
        };
        let _ = host.handle().set_user_paused(sync_paused);
        if let Err(error) = host.handle().bind_profile_id(config.profile_id()) {
            let _ = host.shutdown().await;
            return Err(DesktopProcessError::State(error));
        }

        #[cfg(feature = "test-support")]
        install_external_test_recovery_gate(&host);

        let mut lifecycle = match NativeProcessLifecycleSource::new() {
            Ok(source) => Box::new(source) as Box<dyn ProcessLifecycleSource>,
            Err(error) => {
                let _ = host.shutdown().await;
                return Err(DesktopProcessError::Lifecycle(error));
            }
        };
        let network_hint_interval = config.network_hint_interval();
        let mut network: Box<dyn NetworkHintSource> =
            match NativeNetworkHintSource::new(network_hint_interval) {
                Ok(source) => Box::new(source),
                Err(error) => {
                    warn!(
                        error = %error,
                        "native network hint source unavailable; using periodic safety polling"
                    );
                    Box::new(PeriodicNetworkHintSource::new(network_hint_interval))
                }
            };

        let control = crate::DesktopControlHandle::new_with_library_setup_and_sync_store(
            host.handle(),
            DesktopProcessStatus::Starting,
            Arc::clone(&platform),
            config.profile_id(),
            sync_pause_store,
        );
        let endpoint = match crate::DesktopControlEndpoint::for_profile(
            platform.as_ref(),
            config.profile_id(),
        ) {
            Ok(endpoint) => endpoint,
            Err(error) => {
                let _ = lifecycle.stop().await;
                let _ = network.stop().await;
                let _ = host.shutdown().await;
                return Err(error.into());
            }
        };
        let mut control_server = match crate::DesktopControlServer::bind(endpoint).await {
            Ok(server) => server,
            Err(error) => {
                let _ = lifecycle.stop().await;
                let _ = network.stop().await;
                let _ = host.shutdown().await;
                return Err(error.into());
            }
        };

        if let Err(error) = host.start().await {
            let _ = control_server.stop().await;
            let _ = lifecycle.stop().await;
            let _ = network.stop().await;
            let _ = host.shutdown().await;
            return Err(error.into());
        }
        if let Err(error) = control_server.start(control.clone()) {
            let _ = control_server.stop().await;
            let _ = lifecycle.stop().await;
            let _ = network.stop().await;
            let _ = host.shutdown().await;
            return Err(error.into());
        }
        control.set_process_status(DesktopProcessStatus::Running);
        info!(
            runtime_identity = host.runtime_identity().as_u64(),
            libraries = host.registered_libraries().len(),
            "desktop sync process is running"
        );
        Ok(Self {
            host,
            lifecycle,
            network,
            network_hint_interval,
            control,
            control_server: Some(control_server),
        })
    }

    /// Test/embedding constructor for an already-started canonical host.
    pub fn from_started_host(
        host: DesktopSyncHost,
        lifecycle: Box<dyn ProcessLifecycleSource>,
        network: Box<dyn NetworkHintSource>,
        network_hint_interval: Duration,
    ) -> Self {
        let control =
            crate::DesktopControlHandle::new(host.handle(), DesktopProcessStatus::Running);
        Self {
            host,
            lifecycle,
            network,
            network_hint_interval,
            control,
            control_server: None,
        }
    }

    #[must_use]
    pub fn status(&self) -> DesktopProcessStatus {
        self.control.process_status()
    }

    #[must_use]
    pub const fn host(&self) -> &DesktopSyncHost {
        &self.host
    }

    #[must_use]
    pub fn handle(&self) -> DesktopSyncHostHandle {
        self.host.handle()
    }

    #[must_use]
    pub fn control(&self) -> crate::DesktopControlHandle {
        self.control.clone()
    }

    #[must_use]
    pub fn control_endpoint(&self) -> Option<&crate::DesktopControlEndpoint> {
        self.control_server
            .as_ref()
            .map(crate::DesktopControlServer::endpoint)
    }

    #[must_use]
    pub fn root_status(&self, library_id: synveil_core::LibraryId) -> Option<RootAvailability> {
        self.host.root_status(library_id)
    }

    #[must_use]
    pub fn root_statuses(&self) -> Vec<(synveil_core::LibraryId, RootAvailability)> {
        self.host.root_statuses()
    }

    #[must_use]
    pub fn sync_now(&self, library_id: synveil_core::LibraryId) -> SyncRuntimeWakeResult {
        self.host.sync_now(library_id)
    }

    /// Run until the native/embedding lifecycle source asks for shutdown.
    /// Network events only deliver bounded Prompt 92 hints.
    pub async fn run_until_shutdown(&mut self) -> Result<(), DesktopProcessError> {
        if self.status() != DesktopProcessStatus::Running {
            return Err(DesktopProcessError::InvalidState);
        }
        loop {
            tokio::select! {
                lifecycle = self.lifecycle.next_event() => {
                    match lifecycle {
                        Ok(DesktopLifecycleEvent::ShutdownRequested) => {
                            return self.shutdown().await;
                        }
                        Err(error) => {
                            self.control.set_process_status(DesktopProcessStatus::Faulted);
                            let shutdown = self.shutdown().await;
                            if let Err(shutdown_error) = shutdown {
                                error!(error = %shutdown_error, "desktop process shutdown after lifecycle failure failed");
                            }
                            return Err(DesktopProcessError::Lifecycle(error));
                        }
                    }
                }
                _ = self.control.wait_for_shutdown_request() => {
                    return self.shutdown().await;
                }
                network = self.network.next_hint() => {
                    match network {
                        Ok(()) => {
                            let _ = self.host.network_available();
                        }
                        Err(error) => {
                            warn!(error = %error, "network hint source failed; switching to periodic safety polling");
                            self.network = Box::new(PeriodicNetworkHintSource::new(self.network_hint_interval));
                        }
                    }
                }
            }
        }
    }

    /// Stop lifecycle/network adapters first, then follow the host's one
    /// idempotent graceful shutdown path. Active bounded Prompt 91 work is
    /// allowed to finish by Prompt 92; no future is forcibly aborted here.
    pub async fn shutdown(&mut self) -> Result<(), DesktopProcessError> {
        if self.status() == DesktopProcessStatus::Stopped {
            return Ok(());
        }
        if self.status() != DesktopProcessStatus::Faulted {
            self.control
                .set_process_status(DesktopProcessStatus::Stopping);
        }
        let control_result = if let Some(mut server) = self.control_server.take() {
            server.stop().await
        } else {
            Ok(())
        };
        let lifecycle_result = self.lifecycle.stop().await;
        let network_result = self.network.stop().await;
        let host_result = self
            .host
            .handle()
            .on_lifecycle_event(DesktopLifecycleEvent::ShutdownRequested)
            .await;

        if let Err(error) = control_result {
            self.control
                .set_process_status(DesktopProcessStatus::Faulted);
            return Err(error.into());
        }
        if let Err(error) = host_result {
            self.control
                .set_process_status(DesktopProcessStatus::Faulted);
            return Err(error.into());
        }
        if let Err(error) = lifecycle_result {
            self.control
                .set_process_status(DesktopProcessStatus::Faulted);
            return Err(DesktopProcessError::Lifecycle(error));
        }
        if let Err(error) = network_result {
            self.control
                .set_process_status(DesktopProcessStatus::Faulted);
            return Err(DesktopProcessError::Network(error));
        }
        self.control
            .set_process_status(DesktopProcessStatus::Stopped);
        info!("desktop sync process stopped gracefully");
        Ok(())
    }
}

/// Connect the accepted in-process [`DesktopRootRecoveryGate`] to the ignored
/// cross-process QML acceptance target. This code is compiled only when the
/// `test-support` feature is explicitly requested; the default production
/// client has no test-control environment variable or filesystem signalling.
#[cfg(feature = "test-support")]
fn install_external_test_recovery_gate(host: &DesktopSyncHost) {
    let Some(control_dir) = std::env::var_os("SYNVEIL_TEST_ROOT_RECOVERY_GATE") else {
        return;
    };

    let control_dir = std::path::PathBuf::from(control_dir);
    let entered = control_dir.join("entered");
    let release = control_dir.join("release");
    let gate = std::sync::Arc::new(DesktopRootRecoveryGate::new());
    host.install_test_recovery_gate(std::sync::Arc::clone(&gate));

    tokio::spawn(async move {
        gate.wait_until_recovering().await;
        if let Err(error) = std::fs::write(&entered, gate.recovery_entries().to_string()) {
            warn!(error = %error, "desktop recovery acceptance marker could not be written");
        }
        while !release.exists() {
            // This polling loop is test-support-only. It does not exist in
            // the production/default binary and does not alter recovery
            // timing or retry policy.
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        gate.release();
    });
}

/// Compatibility name for callers that prefer a runner noun.
pub type DesktopProcessRunner = DesktopClientProcess;

fn error_code(error: &DesktopClientConfigError) -> &'static str {
    match error {
        DesktopClientConfigError::MissingConfiguration => "DESKTOP_CONFIG_MISSING",
        DesktopClientConfigError::ConfigurationUnreadable => "DESKTOP_CONFIG_UNREADABLE",
        DesktopClientConfigError::ConfigurationTooLarge => "DESKTOP_CONFIG_TOO_LARGE",
        DesktopClientConfigError::ConfigurationMalformed => "DESKTOP_CONFIG_MALFORMED",
        DesktopClientConfigError::MalformedLine(_) => "DESKTOP_CONFIG_MALFORMED_LINE",
        DesktopClientConfigError::UnknownKey => "DESKTOP_CONFIG_UNKNOWN_KEY",
        DesktopClientConfigError::DuplicateProfile => "DESKTOP_CONFIG_DUPLICATE_PROFILE",
        DesktopClientConfigError::DuplicateLibrary => "DESKTOP_CONFIG_DUPLICATE_LIBRARY",
        DesktopClientConfigError::MissingProfileId => "DESKTOP_CONFIG_PROFILE_REQUIRED",
        DesktopClientConfigError::InvalidProfileId => "DESKTOP_CONFIG_PROFILE_INVALID",
        DesktopClientConfigError::InvalidLibraryId => "DESKTOP_CONFIG_LIBRARY_INVALID",
        DesktopClientConfigError::InvalidRootPath => "DESKTOP_CONFIG_ROOT_INVALID",
        DesktopClientConfigError::ConfigPathNotAbsolute => "DESKTOP_CONFIG_PATH_INVALID",
        DesktopClientConfigError::PlatformPaths => "DESKTOP_CONFIG_PLATFORM_PATHS_INVALID",
        DesktopClientConfigError::ProfileNotFound => "DESKTOP_CONFIG_PROFILE_NOT_FOUND",
        DesktopClientConfigError::LibraryNotConfigured => "DESKTOP_CONFIG_LIBRARY_NOT_CONFIGURED",
        DesktopClientConfigError::WrongProfileBinding => "DESKTOP_CONFIG_PROFILE_BINDING_MISMATCH",
        DesktopClientConfigError::MixedProcessScope => "DESKTOP_CONFIG_SCOPE_MIXED",
        DesktopClientConfigError::InvalidNetworkHintInterval => {
            "DESKTOP_CONFIG_NETWORK_INTERVAL_INVALID"
        }
        DesktopClientConfigError::ProfileMismatch => "DESKTOP_CONFIG_PROFILE_MISMATCH",
        DesktopClientConfigError::ConfigurationWriteFailed => "DESKTOP_CONFIG_WRITE_FAILED",
        DesktopClientConfigError::LibraryBindingConflict => {
            "DESKTOP_CONFIG_LIBRARY_BINDING_CONFLICT"
        }
        DesktopClientConfigError::SyncStateUnreadable => "DESKTOP_SYNC_STATE_UNREADABLE",
        DesktopClientConfigError::SyncStateMalformed => "DESKTOP_SYNC_STATE_MALFORMED",
        DesktopClientConfigError::SyncStateWriteFailed => "DESKTOP_SYNC_STATE_WRITE_FAILED",
        DesktopClientConfigError::Client(error) => error.code(),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use async_trait::async_trait;
    use synveil_client_sync::{DesktopSyncHostConfig, LocalStateConfig, LocalStateStore};
    use tokio::sync::oneshot;

    use super::*;

    struct OneShotLifecycle {
        receiver: Option<oneshot::Receiver<DesktopLifecycleEvent>>,
        stops: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ProcessLifecycleSource for OneShotLifecycle {
        async fn next_event(&mut self) -> Result<DesktopLifecycleEvent, DesktopAdapterError> {
            self.receiver
                .take()
                .ok_or(DesktopAdapterError::Closed)?
                .await
                .map_err(|_| DesktopAdapterError::Closed)
        }

        async fn stop(&mut self) -> Result<(), DesktopAdapterError> {
            self.stops.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }
    }

    struct NeverNetwork {
        stops: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl NetworkHintSource for NeverNetwork {
        async fn next_hint(&mut self) -> Result<(), DesktopAdapterError> {
            std::future::pending::<Result<(), DesktopAdapterError>>().await
        }

        async fn stop(&mut self) -> Result<(), DesktopAdapterError> {
            self.stops.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }
    }

    #[tokio::test]
    async fn process_runner_has_one_host_and_stops_both_adapters_gracefully() {
        let directory = std::env::temp_dir().join(format!(
            "synveil-client-process-test-{}",
            uuid::Uuid::now_v7()
        ));
        fs::create_dir_all(&directory).expect("process fixture directory");
        let state = Arc::new(
            LocalStateStore::open(&LocalStateConfig::new(directory.join("state.sqlite3")))
                .await
                .expect("process fixture state"),
        );
        let host = DesktopSyncHost::new(
            state.clone(),
            Arc::new(synveil_platform::UnsupportedSecureSecretStore::new()),
            DesktopSyncHostConfig::default(),
            Vec::<synveil_client_sync::DesktopSyncLibraryConfig>::new(),
        )
        .await
        .expect("empty canonical host");
        let identity = host.runtime_identity();
        host.start().await.expect("host start");

        let (sender, receiver) = oneshot::channel();
        let lifecycle_stops = Arc::new(AtomicUsize::new(0));
        let network_stops = Arc::new(AtomicUsize::new(0));
        let mut process = DesktopClientProcess::from_started_host(
            host.clone(),
            Box::new(OneShotLifecycle {
                receiver: Some(receiver),
                stops: Arc::clone(&lifecycle_stops),
            }),
            Box::new(NeverNetwork {
                stops: Arc::clone(&network_stops),
            }),
            Duration::from_secs(1),
        );
        assert_eq!(process.status(), DesktopProcessStatus::Running);
        assert_eq!(process.host().runtime_identity(), identity);
        sender
            .send(DesktopLifecycleEvent::ShutdownRequested)
            .expect("shutdown event");
        process
            .run_until_shutdown()
            .await
            .expect("graceful process stop");
        assert_eq!(process.status(), DesktopProcessStatus::Stopped);
        assert_eq!(lifecycle_stops.load(Ordering::Acquire), 1);
        assert_eq!(network_stops.load(Ordering::Acquire), 1);
        assert_eq!(
            host.lifecycle(),
            synveil_client_sync::DesktopSyncHostLifecycle::Stopped
        );
        process
            .shutdown()
            .await
            .expect("idempotent process shutdown");

        state.close_pool().await;
        drop(process);
        drop(host);
        drop(state);
        fs::remove_dir_all(&directory).expect("process fixture cleanup");
    }

    #[test]
    fn process_error_contract_is_stable_and_does_not_echo_internal_values() {
        assert_eq!(
            DesktopProcessError::InvalidState.code(),
            "DESKTOP_PROCESS_STATE_INVALID"
        );
        assert_eq!(
            DesktopProcessError::InvalidState.to_string(),
            "DESKTOP_PROCESS_STATE_INVALID"
        );
        assert!(!format!("{:?}", DesktopProcessError::InvalidState).contains("state.sqlite3"));
    }
}
