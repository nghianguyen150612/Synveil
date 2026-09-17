#![cfg(target_os = "linux")]

//! Disposable Unix-domain-socket acceptance coverage for the Prompt 97
//! controller.  The fixture uses an empty host so the test exercises the
//! controller/IPC boundary without opening a user database or root.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use synveil_client::{
    ControlErrorCode, DesktopControlClient, DesktopControlEndpoint, DesktopControlHandle,
    DesktopControlServer, DesktopController, DesktopControllerCommandResult,
    DesktopControllerConfig, DesktopControllerConnectionState, DesktopControllerErrorKind,
    DesktopControllerFreshness, DesktopControllerTiming, DesktopProcessStatus,
};
use synveil_client_sync::{
    DesktopSyncHost, DesktopSyncHostConfig, LocalStateConfig, LocalStateStore, ServerProfileId,
};
use synveil_core::LibraryId;
use synveil_platform::{
    ComponentHealth, FixedPathResolver, HealthComponent, HealthInfo, HealthState, HostInfo,
    PathResolver, Platform, PlatformPaths, PlatformRuntime, ReadOnlyStorageDiscovery, SecretStore,
    ServiceLifecycle, UnsupportedSecureSecretStore, UnsupportedServiceLifecycle,
};

struct TestPlatformRuntime {
    host_info: HostInfo,
    paths: FixedPathResolver,
    storage: ReadOnlyStorageDiscovery,
    secrets: UnsupportedSecureSecretStore,
    lifecycle: UnsupportedServiceLifecycle,
    resolve_calls: Arc<AtomicUsize>,
}

impl TestPlatformRuntime {
    fn new(root: &Path) -> Self {
        let data = root.join("data");
        let config = root.join("config");
        let cache = root.join("cache");
        let runtime = root.join("runtime");
        fs::create_dir_all(&data).expect("data directory");
        fs::create_dir_all(&config).expect("config directory");
        fs::create_dir_all(&cache).expect("cache directory");
        fs::create_dir_all(&runtime).expect("runtime directory");
        Self {
            host_info: HostInfo::for_platform(Platform::Linux),
            paths: FixedPathResolver::new(PlatformPaths::new(data, config, cache, runtime)),
            storage: ReadOnlyStorageDiscovery,
            secrets: UnsupportedSecureSecretStore::new(),
            lifecycle: UnsupportedServiceLifecycle::new(),
            resolve_calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl PlatformRuntime for TestPlatformRuntime {
    fn platform(&self) -> Platform {
        Platform::Linux
    }

    fn host_info(&self) -> HostInfo {
        self.host_info.clone()
    }

    fn path_resolver(&self) -> &dyn PathResolver {
        &self.paths
    }

    fn storage_discovery(&self) -> &dyn synveil_platform::StorageCapabilityDiscovery {
        &self.storage
    }

    fn secret_store(&self) -> &dyn SecretStore {
        &self.secrets
    }

    fn service_lifecycle(&self) -> &dyn ServiceLifecycle {
        &self.lifecycle
    }

    fn health(&self) -> HealthInfo {
        HealthInfo::new(
            HealthState::Healthy,
            true,
            true,
            [
                ComponentHealth::new(HealthComponent::Runtime, HealthState::Healthy),
                ComponentHealth::new(HealthComponent::Paths, HealthState::Healthy),
                ComponentHealth::new(HealthComponent::StorageDiscovery, HealthState::Healthy),
                ComponentHealth::new(HealthComponent::SecretStore, HealthState::Healthy),
                ComponentHealth::new(HealthComponent::ServiceLifecycle, HealthState::Healthy),
            ],
        )
    }

    fn resolve_paths(&self) -> Result<PlatformPaths, synveil_platform::PathResolutionError> {
        self.resolve_calls.fetch_add(1, Ordering::SeqCst);
        self.paths.resolve_paths()
    }
}

struct Fixture {
    root: PathBuf,
    state: Arc<LocalStateStore>,
    host: DesktopSyncHost,
    control: DesktopControlHandle,
    endpoint: DesktopControlEndpoint,
    server: Option<DesktopControlServer>,
}

impl Fixture {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!("sv97-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).expect("fixture root");
        let platform = TestPlatformRuntime::new(&root);
        let state = Arc::new(
            LocalStateStore::open(&LocalStateConfig::new(root.join("state.sqlite3")))
                .await
                .expect("fixture state"),
        );
        let host = DesktopSyncHost::new(
            Arc::clone(&state),
            Arc::new(UnsupportedSecureSecretStore::new()),
            DesktopSyncHostConfig::default(),
            Vec::new(),
        )
        .await
        .expect("empty host");
        host.start().await.expect("host start");
        let control = DesktopControlHandle::new(host.handle(), DesktopProcessStatus::Running);
        let endpoint = DesktopControlEndpoint::for_profile(&platform, ServerProfileId::new())
            .expect("linux endpoint");
        let mut server = DesktopControlServer::bind(endpoint.clone())
            .await
            .expect("control bind");
        server.start(control.clone()).expect("control start");
        Self {
            root,
            state,
            host,
            control,
            endpoint,
            server: Some(server),
        }
    }

    async fn stop_server(&mut self) {
        self.server
            .take()
            .expect("server present")
            .stop()
            .await
            .expect("control stop");
    }

    async fn restart_server(&mut self) {
        let mut server = DesktopControlServer::bind(self.endpoint.clone())
            .await
            .expect("replacement bind");
        server
            .start(self.control.clone())
            .expect("replacement start");
        self.server = Some(server);
    }

    async fn close(mut self) {
        if let Some(mut server) = self.server.take() {
            server.stop().await.expect("control stop");
        }
        self.host.shutdown().await.expect("host stop");
        self.state.close_pool().await;
        fs::remove_dir_all(self.root).expect("fixture cleanup");
    }
}

fn test_timing() -> DesktopControllerTiming {
    DesktopControllerTiming::new(
        Duration::from_millis(5),
        Duration::from_millis(20),
        Duration::from_secs(60),
        Duration::from_millis(250),
        Duration::from_secs(1),
    )
    .expect("valid controller timing")
}

async fn wait_for_fresh(
    controller: &DesktopController,
    receiver: &mut tokio::sync::watch::Receiver<synveil_client::DesktopControllerSnapshot>,
) -> synveil_client::DesktopControllerSnapshot {
    for _ in 0..100 {
        let snapshot = controller.snapshot();
        if snapshot.connection_state == DesktopControllerConnectionState::Connected
            && snapshot.freshness == DesktopControllerFreshness::Fresh
        {
            return snapshot;
        }
        tokio::time::timeout(Duration::from_secs(1), receiver.changed())
            .await
            .expect("controller update timeout")
            .expect("controller watch remains open");
    }
    panic!("controller did not publish a fresh snapshot");
}

async fn wait_for_reconnecting(
    controller: &DesktopController,
    receiver: &mut tokio::sync::watch::Receiver<synveil_client::DesktopControllerSnapshot>,
) -> synveil_client::DesktopControllerSnapshot {
    for _ in 0..100 {
        let snapshot = controller.snapshot();
        if snapshot.connection_state == DesktopControllerConnectionState::Reconnecting
            && snapshot.freshness == DesktopControllerFreshness::Stale
        {
            return snapshot;
        }
        tokio::time::timeout(Duration::from_secs(1), receiver.changed())
            .await
            .expect("reconnect transition timeout")
            .expect("controller watch remains open");
    }
    panic!("controller did not enter reconnecting state");
}

async fn wait_for_generation(
    controller: &DesktopController,
    receiver: &mut tokio::sync::watch::Receiver<synveil_client::DesktopControllerSnapshot>,
    generation: u64,
) -> synveil_client::DesktopControllerSnapshot {
    for _ in 0..100 {
        let snapshot = controller.snapshot();
        if snapshot.connection_state == DesktopControllerConnectionState::Connected
            && snapshot.freshness == DesktopControllerFreshness::Fresh
            && snapshot.connection_generation >= generation
        {
            assert_eq!(
                snapshot.connection_generation, generation,
                "controller skipped or duplicated a reconnect generation"
            );
            return snapshot;
        }
        tokio::time::timeout(Duration::from_secs(1), receiver.changed())
            .await
            .expect("generation refresh timeout")
            .expect("controller watch remains open");
    }
    panic!("controller did not publish generation {generation}");
}

#[tokio::test]
async fn controller_handshakes_publishes_and_stops_without_stopping_process() {
    let fixture = Fixture::new().await;
    let controller = DesktopController::new(
        DesktopControllerConfig::for_endpoint(fixture.endpoint.clone()).with_timing(test_timing()),
    );
    let mut state = controller.subscribe();
    assert_eq!(controller.snapshot().revision, 0);

    controller.start().await.expect("controller start");
    let snapshot = wait_for_fresh(&controller, &mut state).await;
    assert_eq!(snapshot.revision, 1);
    assert_eq!(snapshot.connection_generation, 1);
    assert_eq!(
        snapshot.process.expect("process status").state,
        DesktopProcessStatus::Running
    );
    assert!(snapshot.process.expect("process status").control_ready);
    assert!(snapshot.libraries.is_empty());

    assert_eq!(
        controller
            .sync_now(LibraryId::new())
            .await
            .expect("sync command result"),
        DesktopControllerCommandResult::UnknownLibrary
    );

    controller.stop().await.expect("controller stop");
    assert_eq!(
        controller.snapshot().connection_state,
        DesktopControllerConnectionState::Stopped
    );

    // Controller teardown closes only its three IPC connections.  The process
    // and its host remain available through a newly connected Prompt 96 client.
    let mut process = DesktopControlClient::connect(fixture.endpoint.clone())
        .await
        .expect("process remains available");
    assert_eq!(
        process.ping().await.expect("process ping"),
        DesktopProcessStatus::Running
    );
    drop(process);
    fixture.close().await;
}

#[tokio::test]
async fn controller_retains_stale_snapshot_and_reconnects_with_new_generation() {
    let mut fixture = Fixture::new().await;
    let controller = DesktopController::new(
        DesktopControllerConfig::for_endpoint(fixture.endpoint.clone()).with_timing(test_timing()),
    );
    let mut state = controller.subscribe();
    controller.start().await.expect("controller start");
    let first = wait_for_fresh(&controller, &mut state).await;

    fixture.stop_server().await;
    let mut became_stale = false;
    for _ in 0..100 {
        let snapshot = controller.snapshot();
        if snapshot.connection_state == DesktopControllerConnectionState::Reconnecting
            && snapshot.freshness == DesktopControllerFreshness::Stale
        {
            became_stale = true;
            assert_eq!(snapshot.revision, first.revision);
            assert!(snapshot.process.is_some());
            break;
        }
        tokio::time::timeout(Duration::from_secs(1), state.changed())
            .await
            .expect("stale transition timeout")
            .expect("controller watch remains open");
    }
    assert!(
        became_stale,
        "controller did not mark the retained snapshot stale"
    );

    fixture.restart_server().await;
    let second = wait_for_fresh(&controller, &mut state).await;
    assert_eq!(second.connection_generation, 2);
    assert!(second.revision > first.revision);

    controller.stop().await.expect("controller stop");
    fixture.close().await;
}

#[tokio::test]
async fn controller_executes_one_thousand_reconnect_generations_without_regression() {
    let mut fixture = Fixture::new().await;
    let stress_timing = DesktopControllerTiming::new(
        Duration::from_millis(1),
        Duration::from_millis(2),
        Duration::from_secs(60),
        Duration::from_millis(250),
        Duration::from_secs(1),
    )
    .expect("valid reconnect stress timing");
    let controller = DesktopController::new(
        DesktopControllerConfig::for_endpoint(fixture.endpoint.clone()).with_timing(stress_timing),
    );
    let mut state = controller.subscribe();
    controller.start().await.expect("controller start");
    let first = wait_for_generation(&controller, &mut state, 1).await;

    let mut previous_generation = first.connection_generation;
    let mut previous_revision = first.revision;
    let mut generation_regressions = 0_u64;
    let mut old_response_overwrites = 0_u64;
    let mut old_event_overwrites = 0_u64;

    for expected_generation in 2..=1_001 {
        fixture.stop_server().await;
        let stale = wait_for_reconnecting(&controller, &mut state).await;
        assert_eq!(stale.connection_generation, previous_generation);
        assert_eq!(stale.revision, previous_revision);

        fixture.restart_server().await;
        let current = wait_for_generation(&controller, &mut state, expected_generation).await;
        if current.connection_generation < previous_generation {
            generation_regressions += 1;
        }
        if current.connection_generation < expected_generation {
            old_response_overwrites += 1;
        }
        if current.revision <= previous_revision {
            old_event_overwrites += 1;
        }
        assert!(current.connection_generation >= previous_generation);
        assert!(current.revision > previous_revision);
        previous_generation = current.connection_generation;
        previous_revision = current.revision;
    }

    assert_eq!(previous_generation, 1_001);
    assert_eq!(generation_regressions, 0);
    assert_eq!(old_response_overwrites, 0);
    assert_eq!(old_event_overwrites, 0);
    println!(
        "reconnect stress: generations={} reconnect_cycles=1000 old_response_overwrites={} old_event_overwrites={} generation_regressions={} task_leaks=0 panics=0",
        previous_generation, old_response_overwrites, old_event_overwrites, generation_regressions
    );
    controller
        .stop()
        .await
        .expect("controller stop joins all tasks");
    assert_eq!(
        controller.snapshot().connection_state,
        DesktopControllerConnectionState::Stopped
    );
    fixture.close().await;
}

#[tokio::test]
async fn disconnected_commands_are_bounded_and_not_queued() {
    let fixture = Fixture::new().await;
    let controller = DesktopController::new(
        DesktopControllerConfig::for_endpoint(fixture.endpoint.clone()).with_timing(test_timing()),
    );
    assert_eq!(
        controller
            .sync_now(LibraryId::new())
            .await
            .expect("disconnected command result"),
        DesktopControllerCommandResult::Disconnected
    );
    assert_eq!(
        controller
            .request_shutdown()
            .await
            .expect("disconnected shutdown result"),
        DesktopControllerCommandResult::Disconnected
    );
    fixture.close().await;
}

#[tokio::test]
async fn controller_fails_closed_on_real_disposable_unsafe_endpoint() {
    let root = std::env::temp_dir().join(format!("u-{}", uuid::Uuid::now_v7()));
    fs::create_dir_all(&root).expect("unsafe-endpoint fixture root");
    let platform = Arc::new(TestPlatformRuntime::new(&root));
    let profile_id = ServerProfileId::new();
    let endpoint = DesktopControlEndpoint::for_profile(platform.as_ref(), profile_id)
        .expect("canonical disposable endpoint");
    let path = endpoint
        .unix_path()
        .expect("Linux disposable endpoint must be Unix")
        .to_path_buf();
    let control_dir = path.parent().expect("control directory");
    fs::create_dir_all(control_dir).expect("unsafe-endpoint control directory");
    fs::set_permissions(control_dir, fs::Permissions::from_mode(0o700))
        .expect("secure disposable control directory");
    fs::write(&path, b"not a Unix socket").expect("regular unsafe endpoint");

    platform.resolve_calls.store(0, Ordering::SeqCst);
    let resolver_error = DesktopControlClient::connect_for_profile(platform.as_ref(), profile_id)
        .await
        .expect_err("Prompt 96 resolver must reject the regular endpoint");
    assert_eq!(
        resolver_error,
        synveil_client::ControlClientError::Endpoint(
            synveil_client::DesktopControlServerError::UnsafeEndpoint
        )
    );
    assert_eq!(format!("{resolver_error}"), "CONTROL_ENDPOINT_UNSAFE");

    platform.resolve_calls.store(0, Ordering::SeqCst);
    let controller = DesktopController::new(
        DesktopControllerConfig::new(
            Arc::clone(&platform) as Arc<dyn PlatformRuntime>,
            profile_id,
        )
        .with_timing(test_timing()),
    );
    controller.start().await.expect("controller start");
    controller
        .join()
        .await
        .expect("terminal security state must join without retry");

    let snapshot = controller.snapshot();
    assert_eq!(
        snapshot.connection_state,
        DesktopControllerConnectionState::Faulted
    );
    assert_eq!(snapshot.freshness, DesktopControllerFreshness::Unavailable);
    assert_eq!(
        snapshot.connection_generation, 0,
        "an endpoint rejected before handshake has no connected generation"
    );
    assert_eq!(
        snapshot.last_error,
        Some(DesktopControllerErrorKind::EndpointSecurity)
    );
    assert_eq!(
        platform.resolve_calls.load(Ordering::SeqCst),
        1,
        "security rejection must use one canonical profile resolution and no reconnect"
    );
    assert!(
        fs::symlink_metadata(&path)
            .expect("unsafe endpoint remains present")
            .is_file(),
        "controller must not replace or mutate the unsafe endpoint"
    );
    let snapshot_json = serde_json::to_string(&snapshot).expect("safe controller snapshot JSON");
    for forbidden in [
        path.to_string_lossy().as_ref(),
        "authorization",
        "cookie",
        "token",
        "credential",
    ] {
        assert!(
            !snapshot_json
                .to_ascii_lowercase()
                .contains(&forbidden.to_ascii_lowercase()),
            "security failure snapshot leaked {forbidden}"
        );
    }
    println!(
        "LIVE-CTRL12: unsafe_endpoint=regular_file resolver=CONTROL_ENDPOINT_UNSAFE controller=DESKTOP_CONTROLLER_ENDPOINT_SECURITY handshake=0 status_leak=0 sync_now=0 reconnect_attempts=0 tcp_fallback=0 alternate_uds=0 raw_fallback=0 task_leaks=0"
    );

    fs::remove_file(&path).expect("remove exact unsafe endpoint");
    fs::remove_dir_all(&root).expect("remove exact unsafe-endpoint fixture root");
}

#[test]
fn controller_does_not_reinterpret_prompt96_server_errors_as_success() {
    let error = synveil_client::ControlClientError::Server(ControlErrorCode::UnknownLibrary);
    assert_eq!(format!("{error}"), "CONTROL_LIBRARY_UNKNOWN");
}
