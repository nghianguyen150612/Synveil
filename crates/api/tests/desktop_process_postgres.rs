#![cfg(target_os = "linux")]

// Prompt 95 live production-process evidence. The fixture is shared with the
// existing Prompt 87B PostgreSQL target so this test provisions a fresh child
// database, real Axum routes, and the real HTTP sync transport without using
// a user's database or credential store.
mod live_fixture {
    include!("rebaseline_convergence_postgres.rs");

    mod process_tests {
        use std::{
            fs,
            path::Path,
            sync::Arc,
            time::{Duration, Instant},
        };

        use synveil_auth::{DeviceAuthenticationService, DeviceEnrollmentTarget};
        use synveil_client::{
            ControlAuthOutcome, ControlEvent, ControlRootState, ControlSyncScheduleResult,
            DEFAULT_DESKTOP_CLIENT_CONFIG_FILE, DesktopClientConfig, DesktopClientProcess,
            DesktopControlClient, DesktopControlServer, DesktopController,
            DesktopControllerCommandResult, DesktopControllerConfig,
            DesktopControllerConnectionState, DesktopControllerFreshness,
            DesktopControllerRootState, DesktopProcessError, DesktopProcessStatus,
        };
        use synveil_client_sync::{
            ClientSyncError, DesktopRootRecoveryGate, DesktopSyncHostConfig,
            FilesystemLocalReplica, LocalNode, LocalReplica, LocalStateConfig, LocalStateStore,
            ManagedRelativePath, OutboundIntentKind, ReplicaScope,
        };
        use synveil_core::{EnrollmentSecret, NodeKind, NodeState, Sequence};
        use synveil_platform::{
            ComponentHealth, FixedPathResolver, HealthComponent, HealthInfo, HealthState, HostInfo,
            PathResolver, Platform, PlatformPaths, PlatformRuntime, ReadOnlyStorageDiscovery,
            SecretStore, ServiceLifecycle, UnsupportedServiceLifecycle,
        };

        use super::{DeviceSyncService, HttpClientConfig, HttpEnrollmentClient, TestSecretStore};

        struct TestPlatformRuntime {
            host_info: HostInfo,
            paths: FixedPathResolver,
            storage: ReadOnlyStorageDiscovery,
            secrets: Arc<TestSecretStore>,
            lifecycle: UnsupportedServiceLifecycle,
            runtime_root: std::path::PathBuf,
        }

        impl TestPlatformRuntime {
            fn new(root: &Path, secrets: Arc<TestSecretStore>) -> Self {
                let data = root.join("data");
                let config = root.join("config");
                let cache = root.join("cache");
                // Linux Unix-domain socket names are bounded by sun_path. Keep
                // this disposable test runtime independent from the longer
                // client fixture path so the production endpoint contract is
                // exercised rather than rejected for an artificial path.
                let runtime = std::env::temp_dir()
                    .join(format!("sv96-runtime-{}", uuid::Uuid::now_v7().simple()));
                fs::create_dir_all(&data).expect("process data directory must be created");
                fs::create_dir_all(&config).expect("process config directory must be created");
                fs::create_dir_all(&cache).expect("process cache directory must be created");
                fs::create_dir_all(&runtime).expect("process runtime directory must be created");
                Self {
                    host_info: HostInfo::for_platform(Platform::Linux),
                    paths: FixedPathResolver::new(PlatformPaths::new(
                        data,
                        config,
                        cache,
                        runtime.clone(),
                    )),
                    storage: ReadOnlyStorageDiscovery,
                    secrets,
                    lifecycle: UnsupportedServiceLifecycle::new(),
                    runtime_root: runtime,
                }
            }
        }

        impl Drop for TestPlatformRuntime {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.runtime_root);
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
                self.secrets.as_ref()
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
                        ComponentHealth::new(
                            HealthComponent::StorageDiscovery,
                            HealthState::Healthy,
                        ),
                        ComponentHealth::new(HealthComponent::SecretStore, HealthState::Healthy),
                        ComponentHealth::new(
                            HealthComponent::ServiceLifecycle,
                            HealthState::Healthy,
                        ),
                    ],
                )
            }
        }

        async fn wait_for_feed(proxy: &super::ProxyState) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while proxy
                .feed_requests
                .load(std::sync::atomic::Ordering::SeqCst)
                == 0
                && Instant::now() < deadline
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert!(
                proxy
                    .feed_requests
                    .load(std::sync::atomic::Ordering::SeqCst)
                    > 0,
                "the started production process must drive a real HTTP feed request"
            );
        }

        async fn wait_for_control_event<F>(
            events: &mut synveil_client::DesktopControlEventStream,
            matches: F,
        ) -> ControlEvent
        where
            F: Fn(&ControlEvent) -> bool,
        {
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let event = events
                        .next_event()
                        .await
                        .expect("production control event stream must remain connected");
                    if matches(&event) {
                        return event;
                    }
                }
            })
            .await
            .expect("production control event must arrive")
        }

        async fn wait_for_controller_fresh(
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
                    .expect("production controller update timeout")
                    .expect("production controller watch remains open");
            }
            panic!("production controller did not publish a fresh snapshot");
        }

        async fn wait_for_controller_revision(
            controller: &DesktopController,
            receiver: &mut tokio::sync::watch::Receiver<synveil_client::DesktopControllerSnapshot>,
            previous_revision: u64,
        ) -> synveil_client::DesktopControllerSnapshot {
            for _ in 0..100 {
                let snapshot = controller.snapshot();
                if snapshot.revision > previous_revision
                    && snapshot.connection_state == DesktopControllerConnectionState::Connected
                    && snapshot.freshness == DesktopControllerFreshness::Fresh
                {
                    return snapshot;
                }
                tokio::time::timeout(Duration::from_secs(1), receiver.changed())
                    .await
                    .expect("production controller revision update timeout")
                    .expect("production controller watch remains open");
            }
            panic!("production controller did not publish a newer fresh revision");
        }

        async fn wait_for_controller_root_state(
            controller: &DesktopController,
            receiver: &mut tokio::sync::watch::Receiver<synveil_client::DesktopControllerSnapshot>,
            library_id: &str,
            expected: DesktopControllerRootState,
        ) -> synveil_client::DesktopControllerSnapshot {
            for _ in 0..100 {
                let snapshot = controller.snapshot();
                if snapshot.libraries.iter().any(|library| {
                    library.library_id == library_id && library.root_state == expected
                }) {
                    return snapshot;
                }
                tokio::time::timeout(Duration::from_secs(1), receiver.changed())
                    .await
                    .expect("production controller root update timeout")
                    .expect("production controller watch remains open");
            }
            panic!("production controller did not publish root state {expected:?}");
        }

        async fn wait_for_controller_reconnecting(
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
                    .expect("production controller reconnect update timeout")
                    .expect("production controller watch remains open");
            }
            panic!("production controller did not enter reconnecting state");
        }

        async fn wait_for_controller_generation(
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
                        "production controller generation must advance exactly once"
                    );
                    return snapshot;
                }
                tokio::time::timeout(Duration::from_secs(1), receiver.changed())
                    .await
                    .expect("production controller generation update timeout")
                    .expect("production controller watch remains open");
            }
            panic!("production controller did not reconnect at generation {generation}");
        }

        async fn wait_for_controller_stopped(
            controller: &DesktopController,
            receiver: &mut tokio::sync::watch::Receiver<synveil_client::DesktopControllerSnapshot>,
        ) -> synveil_client::DesktopControllerSnapshot {
            for _ in 0..100 {
                let snapshot = controller.snapshot();
                if snapshot.connection_state == DesktopControllerConnectionState::Stopped {
                    return snapshot;
                }
                tokio::time::timeout(Duration::from_secs(1), receiver.changed())
                    .await
                    .expect("production controller stopped update timeout")
                    .expect("production controller watch remains open");
            }
            panic!("production controller did not stop after process shutdown");
        }

        async fn wait_for_local_node(
            state: &synveil_client_sync::LocalStateStore,
            library_id: synveil_core::LibraryId,
            node_id: synveil_core::NodeId,
        ) {
            let deadline = Instant::now() + Duration::from_secs(30);
            while Instant::now() < deadline {
                if state
                    .local_node(library_id, node_id)
                    .await
                    .expect("production local node inspection must succeed")
                    .is_some()
                {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("production process did not apply the remote mutation");
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_process_authentication_and_graceful_shutdown() {
            let fixture = super::LiveFixture::new("process").await;
            let seed = fixture.create_library("process").await;
            DeviceSyncService::new(fixture.pool.as_ref().clone())
                .ensure_checkpoint(fixture.owner_id, fixture.device_id, seed.library_id)
                .await
                .expect("process fixture checkpoint must exist");

            let profile = fixture
                .local
                .server_profile(fixture.profile_id)
                .await
                .expect("process profile must load")
                .expect("process profile must exist");
            let grant = DeviceAuthenticationService::new(fixture.pool.as_ref())
                .create_grant(
                    fixture.owner_id,
                    DeviceEnrollmentTarget::Existing(fixture.device_id),
                )
                .await
                .expect("process enrollment grant must issue");
            let enrollment =
                HttpEnrollmentClient::new(profile.clone(), HttpClientConfig::default())
                    .expect("process enrollment client must build")
                    .exchange(&grant.token)
                    .await
                    .expect("process enrollment exchange must succeed");
            let replacement_grant = DeviceAuthenticationService::new(fixture.pool.as_ref())
                .create_grant(
                    fixture.owner_id,
                    DeviceEnrollmentTarget::Existing(fixture.device_id),
                )
                .await
                .expect("process replacement enrollment grant must issue");

            let process_dir = fixture.client_dir.join("process-bootstrap");
            fs::create_dir(&process_dir).expect("process fixture directory must be created");
            let managed_root = process_dir.join("managed");
            fs::create_dir(&managed_root).expect("process managed root must be created");
            let scope = ReplicaScope::new(fixture.owner_id, fixture.device_id, seed.library_id);
            let replica = FilesystemLocalReplica::initialize_for_profile(
                &managed_root,
                scope,
                fixture.profile_id,
            )
            .expect("process managed replica must initialize");
            let binding_id = replica.binding_id();

            let process_data = process_dir.join("data");
            let state_path = process_data.join("client-sync/state.sqlite3");
            let process_state = Arc::new(
                LocalStateStore::open(&LocalStateConfig::new(&state_path))
                    .await
                    .expect("process state store must open"),
            );
            process_state
                .save_server_profile(&profile)
                .await
                .expect("process profile must persist");
            let secrets = Arc::new(TestSecretStore::default());
            process_state
                .store_enrollment(&enrollment, secrets.as_ref())
                .await
                .expect("process enrollment must persist");
            process_state
                .bind_replica_to_profile(scope, binding_id, fixture.profile_id)
                .await
                .expect("process replica binding must persist");

            let local_root = LocalNode::new(
                seed.library_id,
                seed.root.id(),
                None,
                ManagedRelativePath::root(),
                seed.root.name().clone(),
                NodeKind::Directory,
                NodeState::Active,
                seed.root.revision(),
                None,
                None,
                None,
                None,
                None,
                Sequence::new(0),
                true,
                None,
            );
            process_state
                .upsert_local_node(&local_root)
                .await
                .expect("process local root must persist");
            let sqlite = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect(&format!("sqlite:{}", state_path.display()))
                .await
                .expect("process state inspection connection must open");
            sqlx::query(
                "UPDATE replicas SET root_node_id = ?, journal_epoch = 1,
                 applied_sequence = 0, acknowledged_sequence = 0, status = 'IDLE'
                 WHERE library_id = ?",
            )
            .bind(seed.root.id().to_string())
            .bind(seed.library_id.to_string())
            .execute(&sqlite)
            .await
            .expect("process replica cursor must persist");
            sqlite.close().await;
            drop(replica);
            // The preparation store owns the process-state writer lock. The
            // real production process must acquire that lock itself; the
            // later assertion opens a separate store to verify arbitration.
            drop(process_state);

            let config_path = process_dir
                .join("config")
                .join(DEFAULT_DESKTOP_CLIENT_CONFIG_FILE);
            fs::create_dir_all(config_path.parent().expect("config parent"))
                .expect("process config directory must exist");
            fs::write(
                &config_path,
                format!(
                    "profile_id={}\nlibrary.{}={}\n",
                    fixture.profile_id,
                    seed.library_id,
                    managed_root.display()
                ),
            )
            .expect("process config must persist");

            let platform: Arc<dyn PlatformRuntime> =
                Arc::new(TestPlatformRuntime::new(&process_dir, Arc::clone(&secrets)));
            let host_config = DesktopSyncHostConfig::default()
                .with_root_probe_interval(Duration::from_millis(10))
                .with_observation_poll_interval(Duration::from_millis(10));
            let config = DesktopClientConfig::from_platform(platform.as_ref())
                .expect("process config must load")
                .with_host(host_config);
            let mut process = DesktopClientProcess::bootstrap(Arc::clone(&platform), config)
                .await
                .expect("production process bootstrap must succeed");

            assert_eq!(process.status(), DesktopProcessStatus::Running);
            let control_endpoint = process
                .control_endpoint()
                .expect("production process must expose its bound control endpoint")
                .clone();
            let mut control = DesktopControlClient::connect(control_endpoint.clone())
                .await
                .expect("production control handshake must succeed");
            assert_eq!(
                control.ping().await.expect("production control ping"),
                DesktopProcessStatus::Running
            );
            let control_status = control
                .process_status()
                .await
                .expect("production control status");
            assert_eq!(control_status.state, DesktopProcessStatus::Running);
            assert!(control_status.control_ready);

            // LIVE-AUTH2: a syntactically valid but unissued enrollment
            // secret is rejected by the canonical server exchange without
            // changing the already durable credential.
            let credential_before_auth = process
                .host()
                .state()
                .profile_enrollment(fixture.profile_id)
                .await
                .expect("pre-auth enrollment read")
                .expect("pre-auth enrollment must exist");
            let wrong_secret = EnrollmentSecret::from_bytes([0x6a; 32]);
            assert_eq!(
                control
                    .authenticate(&wrong_secret)
                    .await
                    .expect("wrong enrollment secret result"),
                ControlAuthOutcome::InvalidCredentials
            );
            assert_eq!(
                process
                    .host()
                    .state()
                    .profile_enrollment(fixture.profile_id)
                    .await
                    .expect("post-failure enrollment read")
                    .expect("post-failure enrollment must remain")
                    .credential_id(),
                credential_before_auth.credential_id(),
                "invalid authentication must not replace the durable credential"
            );

            // LIVE-AUTH3/4: the real local command reaches the canonical
            // enrollment exchange, profile-bound SecretStore lifecycle, and
            // the existing runtime wake. The following SyncNow assertion
            // proves the replacement credential is usable by HTTP sync.
            assert_eq!(
                control
                    .authenticate(&replacement_grant.token)
                    .await
                    .expect("replacement enrollment result"),
                ControlAuthOutcome::Authenticated
            );
            let credential_after_auth = process
                .host()
                .state()
                .profile_enrollment(fixture.profile_id)
                .await
                .expect("post-auth enrollment read")
                .expect("post-auth enrollment must exist");
            assert_ne!(
                credential_after_auth.credential_id(),
                credential_before_auth.credential_id(),
                "successful authentication must persist the replacement credential"
            );
            assert_eq!(
                credential_after_auth.owner_user_id(),
                fixture.owner_id,
                "replacement must retain the fixture owner binding"
            );
            assert_eq!(
                credential_after_auth.device_id(),
                fixture.device_id,
                "replacement must retain the fixture device binding"
            );
            assert!(matches!(
                control.sync_now(seed.library_id).await,
                Ok(ControlSyncScheduleResult::Queued
                    | ControlSyncScheduleResult::Coalesced
                    | ControlSyncScheduleResult::AlreadyRunningFollowupRecorded)
            ));

            // LIVE-IPC10: multiple local clients use the same process-owned
            // endpoint without creating a second host or runtime.
            let mut second_client = DesktopControlClient::connect(control_endpoint.clone())
                .await
                .expect("second production control client must connect");
            assert_eq!(
                second_client
                    .ping()
                    .await
                    .expect("second production control ping"),
                DesktopProcessStatus::Running
            );
            assert!(
                second_client
                    .process_status()
                    .await
                    .expect("second production control status")
                    .control_ready
            );

            // LIVE-IPC8 and LIVE-IPC12: an ordinary client disconnect is
            // isolated, and a fresh connection can immediately reconnect.
            drop(second_client);
            let mut reconnected_client = DesktopControlClient::connect(control_endpoint.clone())
                .await
                .expect("production control client must reconnect");
            assert_eq!(
                reconnected_client
                    .ping()
                    .await
                    .expect("reconnected production control ping"),
                DesktopProcessStatus::Running
            );
            drop(reconnected_client);

            // LIVE-IPC11: the active process owns both the durable writer and
            // its deterministic IPC endpoint; neither may be claimed twice.
            assert!(matches!(
                DesktopControlServer::bind(control_endpoint.clone()).await,
                Err(synveil_client::DesktopControlServerError::EndpointAlreadyActive)
            ));
            let contender_config = DesktopClientConfig::from_platform(platform.as_ref())
                .expect("contention config must load")
                .with_host(host_config);
            assert!(matches!(
                DesktopClientProcess::bootstrap(Arc::clone(&platform), contender_config).await,
                Err(DesktopProcessError::State(
                    ClientSyncError::ConcurrentWriter
                ))
            ));

            assert_eq!(process.host().registered_libraries(), vec![seed.library_id]);
            assert_eq!(
                process.root_status(seed.library_id),
                Some(synveil_client_sync::RootAvailability::Available)
            );
            assert_eq!(process.host().state().schema_version().await.unwrap(), 7);
            let runtime_identity = process.host().runtime_identity();
            assert_eq!(
                process.host().runtime_identity(),
                runtime_identity,
                "bootstrap must expose one stable runtime identity"
            );
            assert!(matches!(
                LocalStateStore::open(&LocalStateConfig::new(&state_path)).await,
                Err(ClientSyncError::ConcurrentWriter)
            ));
            let library_id_text = seed.library_id.to_string();
            let libraries = control
                .list_libraries()
                .await
                .expect("production library status");
            assert_eq!(libraries.libraries.len(), 1);
            assert_eq!(libraries.libraries[0].library_id, library_id_text);
            let individual_status = control
                .library_status(seed.library_id)
                .await
                .expect("production individual library status");
            assert_eq!(individual_status.library_id, library_id_text);
            assert_eq!(
                individual_status.root_state,
                synveil_client::ControlRootState::Available
            );
            let serialized_status = serde_json::to_string(&libraries).expect("safe status JSON");
            let managed_root_text = managed_root.to_string_lossy().to_string();
            for forbidden in [
                managed_root_text.as_str(),
                "authorization",
                "cookie",
                "token",
            ] {
                assert!(
                    !serialized_status
                        .to_ascii_lowercase()
                        .contains(&forbidden.to_ascii_lowercase())
                );
            }
            assert!(matches!(
                control
                    .sync_now(seed.library_id)
                    .await
                    .expect("production Sync Now"),
                ControlSyncScheduleResult::Queued
                    | ControlSyncScheduleResult::Coalesced
                    | ControlSyncScheduleResult::AlreadyRunningFollowupRecorded
            ));

            // LIVE-CTRL1/2/7: the native controller consumes only the real
            // Prompt 96 process endpoint, presents the safe process/library
            // projection, schedules through IPC, and can stop independently.
            let controller = DesktopController::new(DesktopControllerConfig::for_endpoint(
                control_endpoint.clone(),
            ));
            let mut controller_state = controller.subscribe();
            controller
                .start()
                .await
                .expect("production controller must start");
            let controller_snapshot =
                wait_for_controller_fresh(&controller, &mut controller_state).await;
            assert_eq!(controller_snapshot.revision, 1);
            assert_eq!(controller_snapshot.connection_generation, 1);
            assert_eq!(
                controller_snapshot
                    .process
                    .expect("controller process status")
                    .state,
                DesktopProcessStatus::Running
            );
            assert_eq!(controller_snapshot.libraries.len(), 1);
            assert_eq!(controller_snapshot.libraries[0].library_id, library_id_text);
            let controller_json =
                serde_json::to_string(&controller_snapshot).expect("controller snapshot JSON");
            for forbidden in [
                managed_root_text.as_str(),
                "authorization",
                "cookie",
                "token",
                "credential",
            ] {
                assert!(
                    !controller_json
                        .to_ascii_lowercase()
                        .contains(&forbidden.to_ascii_lowercase()),
                    "controller snapshot leaked {forbidden}"
                );
            }
            assert!(matches!(
                controller
                    .sync_now(seed.library_id)
                    .await
                    .expect("controller Sync Now"),
                DesktopControllerCommandResult::Accepted
                    | DesktopControllerCommandResult::Coalesced
                    | DesktopControllerCommandResult::AlreadyRunningFollowupRecorded
            ));
            controller
                .stop()
                .await
                .expect("controller stop must not stop process");
            assert_eq!(
                control.ping().await.expect("process after controller stop"),
                DesktopProcessStatus::Running
            );
            wait_for_feed(&fixture.proxy).await;

            // Keep a replacement controller alive for the remaining live
            // controller scenarios. This proves that controller exit is not
            // process shutdown and that a new controller creates a fresh
            // connection relationship afterward.
            let controller = DesktopController::new(DesktopControllerConfig::for_endpoint(
                control_endpoint.clone(),
            ));
            let mut controller_state = controller.subscribe();
            // `watch` retains only the latest value: this subscriber is
            // intentionally never drained while the active one continues.
            let _slow_controller_state = controller.subscribe();
            controller
                .start()
                .await
                .expect("replacement production controller must start");
            let controller_snapshot =
                wait_for_controller_fresh(&controller, &mut controller_state).await;
            assert_eq!(controller_snapshot.connection_generation, 1);

            // LIVE-CTRL3/5: mutate the real PostgreSQL-backed server, issue
            // SyncNow through the controller, and require both the real
            // process stack to apply the mutation and the event-driven
            // controller refresh to publish a newer fresh revision.
            let remote_directory = fixture.create_directory(&seed, "controller-remote").await;
            let previous_revision = controller_snapshot.revision;
            assert!(matches!(
                controller
                    .sync_now(seed.library_id)
                    .await
                    .expect("controller remote-mutation SyncNow"),
                DesktopControllerCommandResult::Accepted
                    | DesktopControllerCommandResult::Coalesced
                    | DesktopControllerCommandResult::AlreadyRunningFollowupRecorded
            ));
            wait_for_local_node(
                process.host().state().as_ref(),
                seed.library_id,
                remote_directory.id(),
            )
            .await;
            let refreshed =
                wait_for_controller_revision(&controller, &mut controller_state, previous_revision)
                    .await;
            assert_eq!(refreshed.freshness, DesktopControllerFreshness::Fresh);
            assert_eq!(refreshed.connection_generation, 1);
            wait_for_feed(&fixture.proxy).await;

            // LIVE-IPC5: the event stream is attached to the existing host
            // and reports a real runtime transition caused by SyncNow.
            let event_client = DesktopControlClient::connect(control_endpoint.clone())
                .await
                .expect("production event client must connect");
            let mut events = event_client
                .subscribe_events()
                .await
                .expect("production event subscription must succeed");
            let event_refresh_revision = controller.snapshot().revision;
            assert!(matches!(
                control
                    .sync_now(seed.library_id)
                    .await
                    .expect("production event-triggering SyncNow"),
                ControlSyncScheduleResult::Queued
                    | ControlSyncScheduleResult::Coalesced
                    | ControlSyncScheduleResult::AlreadyRunningFollowupRecorded
            ));
            let status_event = wait_for_control_event(&mut events, |event| {
                matches!(
                    event,
                    ControlEvent::LibraryStatusChanged { library_id }
                        if library_id == &library_id_text
                )
            })
            .await;
            assert!(matches!(
                status_event,
                ControlEvent::LibraryStatusChanged { .. }
            ));
            let completed_event = wait_for_control_event(&mut events, |event| {
                matches!(
                    event,
                    ControlEvent::SyncCycleCompleted { library_id, .. }
                        if library_id == &library_id_text
                )
            })
            .await;
            assert!(matches!(
                completed_event,
                ControlEvent::SyncCycleCompleted { .. }
            ));
            let event_refreshed = wait_for_controller_revision(
                &controller,
                &mut controller_state,
                event_refresh_revision,
            )
            .await;
            assert_eq!(
                event_refreshed.freshness,
                DesktopControllerFreshness::Fresh,
                "a real Prompt 96 event must lead to a fresh controller refresh"
            );

            // LIVE-IPC6: a subscriber that does not drain its event stream
            // cannot block an independent request/response client.
            let slow_client = DesktopControlClient::connect(control_endpoint.clone())
                .await
                .expect("slow production event client must connect");
            let _slow_events = slow_client
                .subscribe_events()
                .await
                .expect("slow production event subscription must succeed");
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), control.ping())
                    .await
                    .expect("slow subscriber must not block control ping")
                    .expect("control ping after slow subscriber"),
                DesktopProcessStatus::Running
            );

            // LIVE-IPC4: root availability is observable through the safe
            // status/event projection while the canonical binding is absent,
            // then returns to Available after the exact directory is restored.
            let recovery_gate = Arc::new(DesktopRootRecoveryGate::new());
            process
                .host()
                .install_test_recovery_gate(Arc::clone(&recovery_gate));
            let recovery_generation = controller.snapshot().connection_generation;
            let registered_libraries_before_recovery = process.host().registered_libraries();
            let runtime_identity_before_recovery = process.host().runtime_identity();
            let pending_before_recovery = process
                .host()
                .state()
                .list_pending_intents(seed.library_id)
                .await
                .expect("pending intents before root recovery");
            let delete_count_before_recovery = pending_before_recovery
                .iter()
                .filter(|intent| intent.kind() == OutboundIntentKind::DeleteOrTrashNode)
                .count();
            let controller_initial = wait_for_controller_root_state(
                &controller,
                &mut controller_state,
                &library_id_text,
                DesktopControllerRootState::Available,
            )
            .await;
            assert_eq!(
                controller_initial.freshness,
                DesktopControllerFreshness::Fresh
            );
            assert_eq!(
                controller_initial.connection_generation,
                recovery_generation
            );
            let unavailable_root = process_dir.join("managed-away");
            fs::rename(&managed_root, &unavailable_root)
                .expect("production managed root must be temporarily detachable");
            let unavailable_event = wait_for_control_event(&mut events, |event| {
                matches!(
                    event,
                    ControlEvent::RootAvailabilityChanged {
                        library_id,
                        state: ControlRootState::Unavailable,
                    } if library_id == &library_id_text
                )
            })
            .await;
            assert!(matches!(
                unavailable_event,
                ControlEvent::RootAvailabilityChanged {
                    state: ControlRootState::Unavailable,
                    ..
                }
            ));
            assert_eq!(
                control
                    .library_status(seed.library_id)
                    .await
                    .expect("unavailable production library status")
                    .root_state,
                ControlRootState::Unavailable
            );
            let controller_unavailable = wait_for_controller_root_state(
                &controller,
                &mut controller_state,
                &library_id_text,
                DesktopControllerRootState::Unavailable,
            )
            .await;
            assert_eq!(
                controller_unavailable.freshness,
                DesktopControllerFreshness::Fresh
            );

            fs::rename(&unavailable_root, &managed_root)
                .expect("production managed root must be restored");
            tokio::time::timeout(
                Duration::from_secs(10),
                recovery_gate.wait_until_recovering(),
            )
            .await
            .expect("production root recovery must reach the deterministic gate");
            assert_eq!(
                recovery_gate.recovery_entries(),
                1,
                "one canonical recovery must start one observer lifecycle"
            );
            let recovering_event = wait_for_control_event(&mut events, |event| {
                matches!(
                    event,
                    ControlEvent::RootAvailabilityChanged {
                        library_id,
                        state: ControlRootState::Recovering,
                    } if library_id == &library_id_text
                )
            })
            .await;
            assert!(matches!(
                recovering_event,
                ControlEvent::RootAvailabilityChanged {
                    state: ControlRootState::Recovering,
                    ..
                }
            ));
            assert_eq!(
                control
                    .library_status(seed.library_id)
                    .await
                    .expect("recovering production library status")
                    .root_state,
                ControlRootState::Recovering
            );
            let controller_recovering = wait_for_controller_root_state(
                &controller,
                &mut controller_state,
                &library_id_text,
                DesktopControllerRootState::Recovering,
            )
            .await;
            assert_eq!(
                controller_recovering.freshness,
                DesktopControllerFreshness::Fresh
            );
            assert_eq!(
                controller_recovering.connection_generation, recovery_generation,
                "root recovery must not create a new controller connection"
            );
            recovery_gate.release();
            let available_event = wait_for_control_event(&mut events, |event| {
                matches!(
                    event,
                    ControlEvent::RootAvailabilityChanged {
                        library_id,
                        state: ControlRootState::Available,
                    } if library_id == &library_id_text
                )
            })
            .await;
            assert!(matches!(
                available_event,
                ControlEvent::RootAvailabilityChanged {
                    state: ControlRootState::Available,
                    ..
                }
            ));
            assert_eq!(
                control
                    .library_status(seed.library_id)
                    .await
                    .expect("restored production library status")
                    .root_state,
                ControlRootState::Available
            );
            let controller_available = wait_for_controller_root_state(
                &controller,
                &mut controller_state,
                &library_id_text,
                DesktopControllerRootState::Available,
            )
            .await;
            assert_eq!(
                controller_available.freshness,
                DesktopControllerFreshness::Fresh
            );
            assert_eq!(
                controller_available.connection_generation,
                recovery_generation
            );
            let pending_after_recovery = process
                .host()
                .state()
                .list_pending_intents(seed.library_id)
                .await
                .expect("pending intents after root recovery");
            let delete_count_after_recovery = pending_after_recovery
                .iter()
                .filter(|intent| intent.kind() == OutboundIntentKind::DeleteOrTrashNode)
                .count();
            assert_eq!(
                delete_count_after_recovery, delete_count_before_recovery,
                "root recovery must not synthesize delete/trash intents"
            );
            assert_eq!(
                process.host().registered_libraries(),
                registered_libraries_before_recovery,
                "root recovery must not register a duplicate runtime library"
            );
            assert_eq!(
                process.host().runtime_identity(),
                runtime_identity_before_recovery,
                "root recovery must retain one runtime identity"
            );

            drop(events);
            drop(_slow_events);

            // LIVE-CTRL6/8/9: race a scheduling command with a controlled
            // listener/process stop. The result remains a typed scheduling or
            // uncertainty result, then the same profile is bootstrapped again
            // while the controller remains alive.
            let current_revision_before_restart = controller.snapshot().revision;
            let controller_for_race = controller.clone();
            let race_task =
                tokio::spawn(async move { controller_for_race.sync_now(seed.library_id).await });
            process
                .shutdown()
                .await
                .expect("production process must stop cleanly during restart race");
            let race_result = race_task
                .await
                .expect("controller restart race task must join");
            assert!(
                race_result.is_ok(),
                "restart race must return a typed controller result: {race_result:?}"
            );
            let controller_stale =
                wait_for_controller_reconnecting(&controller, &mut controller_state).await;
            assert_eq!(
                controller_stale.revision, current_revision_before_restart,
                "restart must retain the last complete controller snapshot"
            );
            assert_eq!(
                controller_stale.freshness,
                DesktopControllerFreshness::Stale
            );
            drop(control);
            if let Some(socket) = control_endpoint.unix_path() {
                assert!(
                    fs::symlink_metadata(socket).is_err(),
                    "graceful process shutdown must remove its Unix socket"
                );
            }
            assert_eq!(process.status(), DesktopProcessStatus::Stopped);
            assert_eq!(
                process.host().lifecycle(),
                synveil_client_sync::DesktopSyncHostLifecycle::Stopped
            );
            process
                .shutdown()
                .await
                .expect("production process shutdown must be idempotent");

            // LIVE-IPC9 and LIVE-IPC12: leave an exact owned stale socket,
            // restart the same profile, and reconnect through the same
            // deterministic endpoint after stale recovery.
            let socket_path = control_endpoint
                .unix_path()
                .expect("Linux production endpoint must be a Unix socket")
                .to_path_buf();
            let stale_listener = std::os::unix::net::UnixListener::bind(&socket_path)
                .expect("stale production socket must bind at the exact endpoint");
            drop(stale_listener);
            assert!(fs::symlink_metadata(&socket_path).is_ok());

            drop(process);
            let restart_config = DesktopClientConfig::from_platform(platform.as_ref())
                .expect("restart config must load")
                .with_host(host_config);
            let mut restarted =
                DesktopClientProcess::bootstrap(Arc::clone(&platform), restart_config)
                    .await
                    .expect("same-profile process must recover the stale endpoint");
            assert_eq!(restarted.status(), DesktopProcessStatus::Running);
            let mut restart_control = DesktopControlClient::connect(control_endpoint)
                .await
                .expect("restarted production control client must connect");
            assert_eq!(
                restart_control
                    .ping()
                    .await
                    .expect("restarted production control ping"),
                DesktopProcessStatus::Running
            );

            // LIVE-AUTH7: explicit Sign Out uses the same process-owned
            // lifecycle, removes the secure-store value, and leaves process
            // shutdown as a separate operation.
            assert_eq!(
                restart_control
                    .sign_out()
                    .await
                    .expect("production sign-out result"),
                ControlAuthOutcome::SignedOut
            );

            let replacement_snapshot =
                wait_for_controller_generation(&controller, &mut controller_state, 2).await;
            assert_eq!(
                replacement_snapshot
                    .process
                    .expect("replacement process status")
                    .state,
                DesktopProcessStatus::Running
            );
            assert_eq!(replacement_snapshot.libraries.len(), 1);
            // LIVE-CTRL7: the replacement controller is usable after the
            // previous controller/process relationship was torn down.
            assert!(matches!(
                controller
                    .sync_now(seed.library_id)
                    .await
                    .expect("replacement controller SyncNow"),
                DesktopControllerCommandResult::Accepted
                    | DesktopControllerCommandResult::Coalesced
                    | DesktopControllerCommandResult::AlreadyRunningFollowupRecorded
            ));

            let restart_task = tokio::spawn(async move {
                let result = restarted.run_until_shutdown().await;
                (restarted, result)
            });
            // LIVE-CTRL10: the controller owns only the request; the real
            // process runner performs the graceful host/lifecycle shutdown.
            assert_eq!(
                controller
                    .request_shutdown()
                    .await
                    .expect("controller RequestShutdown result"),
                DesktopControllerCommandResult::ShutdownAccepted
            );
            let (mut restarted, restart_result) =
                tokio::time::timeout(Duration::from_secs(30), restart_task)
                    .await
                    .expect("restarted production process must stop")
                    .expect("restarted production process task must join");
            restart_result.expect("restarted production process must stop gracefully");
            let controller_stopped =
                wait_for_controller_stopped(&controller, &mut controller_state).await;
            assert_eq!(
                controller_stopped.connection_state,
                DesktopControllerConnectionState::Stopped
            );
            controller
                .stop()
                .await
                .expect("controller stop must join after graceful process shutdown");
            drop(restart_control);
            assert!(
                fs::symlink_metadata(&socket_path).is_err(),
                "restarted process shutdown must remove its Unix socket"
            );
            assert_eq!(restarted.status(), DesktopProcessStatus::Stopped);
            restarted
                .shutdown()
                .await
                .expect("restarted process shutdown must be idempotent");
            drop(restarted);
            let post_sign_out_state = LocalStateStore::open(&LocalStateConfig::new(&state_path))
                .await
                .expect("post-sign-out state inspection must open");
            let forgotten = post_sign_out_state
                .profile_enrollment(fixture.profile_id)
                .await
                .expect("post-sign-out enrollment read")
                .expect("forgotten enrollment marker must remain");
            assert!(
                forgotten.forgotten_at_ms().is_some(),
                "sign-out must retain a durable forgotten marker"
            );
            assert!(
                post_sign_out_state
                    .load_device_credential(fixture.profile_id, secrets.as_ref())
                    .await
                    .expect("post-sign-out credential read")
                    .is_none(),
                "sign-out must remove the reusable SecretStore credential"
            );
            post_sign_out_state.close_pool().await;
            fixture.cleanup().await;
        }
    }
}
