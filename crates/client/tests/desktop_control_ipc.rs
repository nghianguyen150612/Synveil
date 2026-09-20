#![cfg(target_os = "linux")]

//! Disposable-runtime acceptance coverage for the local Prompt 96 control
//! plane. The fixture never uses XDG_RUNTIME_DIR or a user's state directory.

use std::{
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use synveil_client::{
    ControlClient, ControlClientHello, ControlCommand, ControlErrorCode, ControlEvent,
    ControlProcessStatus, ControlProfileConfigurationOutcome, ControlRequest, ControlResponse,
    ControlResponseBody, ControlServerHello, ControlSyncControlResult, ControlSyncControlState,
    DEFAULT_DESKTOP_CLIENT_SYNC_STATE_FILE, DESKTOP_CONTROL_PROTOCOL_VERSION, DesktopControlClient,
    DesktopControlEndpoint, DesktopControlHandle, DesktopControlServer, DesktopProcessStatus,
    DesktopSyncPauseStore, read_frame, write_frame,
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
use tokio::{io::AsyncWriteExt, net::UnixStream, time::timeout};

struct TestPlatformRuntime {
    host_info: HostInfo,
    paths: FixedPathResolver,
    storage: ReadOnlyStorageDiscovery,
    secrets: UnsupportedSecureSecretStore,
    lifecycle: UnsupportedServiceLifecycle,
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
}

struct Fixture {
    root: PathBuf,
    profile_id: ServerProfileId,
    state: Arc<LocalStateStore>,
    host: DesktopSyncHost,
    control: DesktopControlHandle,
    endpoint: DesktopControlEndpoint,
    server: DesktopControlServer,
}

impl Fixture {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!("sv96-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).expect("fixture root");
        let platform = Arc::new(TestPlatformRuntime::new(&root));
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
        let profile_id = ServerProfileId::new();
        let sync_pause_store = DesktopSyncPauseStore::from_path(
            root.join("config")
                .join(DEFAULT_DESKTOP_CLIENT_SYNC_STATE_FILE),
        );
        let control = DesktopControlHandle::new_with_library_setup_and_sync_store(
            host.handle(),
            DesktopProcessStatus::Running,
            platform.clone(),
            profile_id,
            sync_pause_store,
        );
        host.handle()
            .bind_profile_id(profile_id)
            .expect("bind fixture profile");
        let endpoint = DesktopControlEndpoint::for_profile(platform.as_ref(), profile_id)
            .expect("linux endpoint");
        let mut server = DesktopControlServer::bind(endpoint.clone())
            .await
            .expect("control bind");
        server.start(control.clone()).expect("control start");
        Self {
            root,
            profile_id,
            state,
            host,
            control,
            endpoint,
            server,
        }
    }

    async fn close(mut self) {
        self.server.stop().await.expect("control stop");
        self.host.shutdown().await.expect("host stop");
        self.state.close_pool().await;
        fs::remove_dir_all(self.root).expect("fixture cleanup");
    }
}

#[tokio::test]
async fn profile_configuration_ipc_is_typed_and_zero_library_safe() {
    let fixture = Fixture::new().await;
    let mut client = DesktopControlClient::connect(fixture.endpoint.clone())
        .await
        .expect("profile control client");

    let configuration = client
        .get_profile_configuration()
        .await
        .expect("profile configuration");
    assert!(!configuration.configured);
    assert!(!configuration.authenticated);
    assert!(
        client
            .list_libraries()
            .await
            .expect("library list")
            .libraries
            .is_empty()
    );

    assert_eq!(
        client
            .validate_profile_configuration("http://example.com/".to_owned(), "Example".to_owned())
            .await
            .expect("validation response"),
        ControlProfileConfigurationOutcome::InvalidServerAddress
    );
    assert_eq!(
        client
            .configure_profile(
                fixture.profile_id.to_string(),
                "https://bad path.example/".to_owned(),
                "Example".to_owned(),
            )
            .await
            .expect("configuration response"),
        ControlProfileConfigurationOutcome::InvalidServerAddress
    );

    fixture.close().await;
}

#[tokio::test]
async fn sync_control_ipc_persists_before_runtime_state_changes_and_reopens() {
    let fixture = Fixture::new().await;
    let state_path = fixture
        .root
        .join("config")
        .join(DEFAULT_DESKTOP_CLIENT_SYNC_STATE_FILE);
    let mut client = DesktopControlClient::connect(fixture.endpoint.clone())
        .await
        .expect("sync control client");

    assert_eq!(
        client
            .sync_control_state()
            .await
            .expect("initial sync control state"),
        ControlSyncControlState::Running
    );
    assert_eq!(
        client.pause_sync().await.expect("pause response"),
        ControlSyncControlResult::Paused
    );
    assert_eq!(
        fs::read_to_string(&state_path).expect("durable paused state"),
        "paused\n"
    );
    assert_eq!(
        client
            .sync_control_state()
            .await
            .expect("paused sync control state"),
        ControlSyncControlState::PausedByUser
    );
    assert_eq!(
        client
            .pause_sync()
            .await
            .expect("idempotent pause response"),
        ControlSyncControlResult::AlreadyPaused
    );
    assert_eq!(
        client.resume_sync().await.expect("resume response"),
        ControlSyncControlResult::Resumed
    );
    assert_eq!(
        fs::read_to_string(&state_path).expect("durable running state"),
        "running\n"
    );
    assert_eq!(
        client
            .sync_control_state()
            .await
            .expect("running sync control state"),
        ControlSyncControlState::Running
    );

    let reopened = DesktopSyncPauseStore::from_path(state_path);
    assert!(!reopened.is_paused().expect("reopened running state"));
    fixture.close().await;
}

async fn raw_hello(path: &Path, version: u16) -> (UnixStream, ControlServerHello) {
    let mut stream = UnixStream::connect(path).await.expect("raw connect");
    write_frame(
        &mut stream,
        &synveil_client::ControlClientHello {
            protocol_version: version,
        },
    )
    .await
    .expect("raw hello");
    let payload = read_frame(&mut stream).await.expect("server hello frame");
    let hello = serde_json::from_slice(&payload).expect("server hello payload");
    (stream, hello)
}

#[tokio::test]
async fn connection_limit_is_bounded_and_excess_peers_are_dropped() {
    let fixture = Fixture::new().await;
    let path = fixture
        .endpoint
        .unix_path()
        .expect("unix endpoint")
        .to_path_buf();

    let mut held = Vec::with_capacity(synveil_client::DESKTOP_CONTROL_MAX_CONNECTIONS);
    for _ in 0..synveil_client::DESKTOP_CONTROL_MAX_CONNECTIONS {
        let (stream, hello) = raw_hello(&path, DESKTOP_CONTROL_PROTOCOL_VERSION).await;
        assert_eq!(hello.protocol_version, DESKTOP_CONTROL_PROTOCOL_VERSION);
        assert!(hello.error.is_none());
        held.push(stream);
    }

    let mut excess = UnixStream::connect(&path)
        .await
        .expect("over-limit connect");
    let wrote_hello = write_frame(
        &mut excess,
        &ControlClientHello {
            protocol_version: DESKTOP_CONTROL_PROTOCOL_VERSION,
        },
    )
    .await
    .is_ok();
    let excess_rejected = if wrote_hello {
        matches!(
            timeout(Duration::from_secs(2), read_frame(&mut excess)).await,
            Ok(Err(_))
        )
    } else {
        true
    };
    assert!(excess_rejected, "33rd peer was not boundedly rejected");
    drop(excess);
    drop(held);

    let mut client = ControlClient::connect(fixture.endpoint.clone())
        .await
        .expect("server remains responsive after over-limit peer");
    assert_eq!(
        client.ping().await.expect("post-limit ping"),
        DesktopProcessStatus::Running
    );
    fixture.close().await;
}

#[tokio::test]
async fn desktop_control_ipc_uses_disposable_secure_uds_and_safe_protocol() {
    let fixture = Fixture::new().await;
    let path = fixture
        .endpoint
        .unix_path()
        .expect("unix endpoint")
        .to_path_buf();
    let control_dir = path.parent().expect("control directory");

    let control_metadata = fs::symlink_metadata(control_dir).expect("control metadata");
    assert!(control_metadata.is_dir());
    assert_eq!(control_metadata.uid(), nix::unistd::geteuid().as_raw());
    assert_eq!(control_metadata.permissions().mode() & 0o777, 0o700);

    let socket_metadata = fs::symlink_metadata(&path).expect("socket metadata");
    assert!(socket_metadata.file_type().is_socket());
    assert_eq!(socket_metadata.uid(), nix::unistd::geteuid().as_raw());
    assert_eq!(socket_metadata.permissions().mode() & 0o777, 0o600);

    let mut client = ControlClient::connect(fixture.endpoint.clone())
        .await
        .expect("v1 handshake");
    assert_eq!(
        client.ping().await.expect("ping"),
        DesktopProcessStatus::Running
    );
    let process_status: ControlProcessStatus = client.process_status().await.expect("status");
    assert_eq!(process_status.state, DesktopProcessStatus::Running);
    assert!(process_status.control_ready);
    assert!(
        client
            .list_libraries()
            .await
            .expect("libraries")
            .libraries
            .is_empty()
    );
    assert_eq!(
        client
            .library_status(LibraryId::new())
            .await
            .expect_err("unknown library"),
        synveil_client::ControlClientError::Server(ControlErrorCode::UnknownLibrary)
    );

    let (mut raw, hello) = raw_hello(&path, DESKTOP_CONTROL_PROTOCOL_VERSION).await;
    assert_eq!(hello.protocol_version, DESKTOP_CONTROL_PROTOCOL_VERSION);
    assert!(hello.error.is_none());
    write_frame(
        &mut raw,
        &ControlRequest {
            request_id: 77,
            command: ControlCommand::Unsupported,
        },
    )
    .await
    .expect("unknown command");
    let payload = read_frame(&mut raw)
        .await
        .expect("unknown command response");
    let response: ControlResponse = serde_json::from_slice(&payload).expect("response payload");
    assert_eq!(response.request_id, 77);
    assert_eq!(
        response.body,
        ControlResponseBody::Error {
            code: ControlErrorCode::UnknownCommand
        }
    );

    let (incompatible, hello) = raw_hello(&path, u16::MAX).await;
    assert_eq!(
        hello.error,
        Some(ControlErrorCode::ProtocolVersionUnsupported)
    );
    drop(incompatible);

    for bytes in [
        vec![0_u8, 0, 0, 0],
        vec![0_u8, 1, 0, 1],
        vec![0_u8, 0, 0, 5, b'{'],
        vec![0_u8, 0],
    ] {
        let mut malformed = UnixStream::connect(&path).await.expect("malformed connect");
        malformed
            .write_all(&bytes)
            .await
            .expect("malformed frame write");
        drop(malformed);
    }

    let mut after_malformed = ControlClient::connect(fixture.endpoint.clone())
        .await
        .expect("server survives malformed clients");
    assert_eq!(
        after_malformed.ping().await.expect("post-malformed ping"),
        DesktopProcessStatus::Running
    );
    drop(after_malformed);
    drop(client);

    let event_client = DesktopControlClient::connect(fixture.endpoint.clone())
        .await
        .expect("event client");
    let mut events = event_client
        .subscribe_events()
        .await
        .expect("event subscription");
    let mut shutdown = DesktopControlClient::connect(fixture.endpoint.clone())
        .await
        .expect("shutdown client");
    shutdown.shutdown().await.expect("shutdown accepted");
    assert_eq!(
        fixture.control.process_status(),
        DesktopProcessStatus::Stopping
    );
    let event = timeout(Duration::from_secs(1), events.next_event())
        .await
        .expect("event delivery timeout")
        .expect("event delivery");
    assert_eq!(
        event,
        ControlEvent::ProcessStateChanged {
            state: DesktopProcessStatus::Stopping
        }
    );

    fixture.close().await;
}
