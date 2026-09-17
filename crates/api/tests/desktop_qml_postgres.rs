#![cfg(target_os = "linux")]

// Prompt 98C acceptance target. It launches the production client
// and the production Qt shell as separate processes around the real PostgreSQL
// fixture. The target is intentionally ignored: it needs a disposable PG17
// server, an unlocked native Secret Service, and Qt's offscreen platform.
mod live_fixture {
    include!("rebaseline_convergence_postgres.rs");

    mod qml_tests {
        use std::{
            collections::BTreeSet,
            fs::{self, File},
            io::Read,
            os::unix::fs::PermissionsExt,
            path::{Path, PathBuf},
            process::{Child, Command, ExitStatus, Stdio},
            sync::Arc,
            time::{Duration, Instant},
        };

        use synveil_auth::DeviceAuthenticationService;
        use synveil_client::{DesktopControlClient, DesktopControlEndpoint, DesktopProcessStatus};
        use synveil_client_sync::{
            FilesystemLocalReplica, LocalNode, LocalReplica, LocalStateConfig, LocalStateStore,
            ManagedRelativePath,
        };
        use synveil_core::{NodeKind, NodeState, Sequence};
        use synveil_metadata::DeviceSyncService;
        use synveil_platform::{NativeSecureSecretStore, SecretStore, SecretStoreState};
        use uuid::Uuid;

        use super::{LibrarySeed, LiveFixture};

        const WAIT: Duration = Duration::from_secs(30);

        struct ChildCapture {
            child: Child,
            log: PathBuf,
        }

        struct ProcessPaths {
            process_dir: PathBuf,
            data_dir: PathBuf,
            config_dir: PathBuf,
            cache_dir: PathBuf,
            runtime_dir: PathBuf,
            config_file: PathBuf,
            recovery_gate_dir: PathBuf,
            roots: Vec<PathBuf>,
        }

        fn binary_path(name: &str, override_name: &str) -> PathBuf {
            std::env::var_os(override_name).map_or_else(
                || {
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../target/debug")
                        .join(name)
                },
                PathBuf::from,
            )
        }

        fn desktop_client_binary() -> PathBuf {
            binary_path("synveil-client", "SYNVEIL_CLIENT_BIN")
        }

        fn desktop_shell_binary() -> PathBuf {
            binary_path("synveil-desktop", "SYNVEIL_DESKTOP_BIN")
        }

        fn create_process_paths(fixture: &LiveFixture, seeds: &[LibrarySeed]) -> ProcessPaths {
            let process_dir = fixture.client_dir.join("qml-process");
            let data_dir = process_dir.join("data");
            let config_dir = process_dir.join("config");
            let cache_dir = process_dir.join("cache");
            let runtime_dir = std::env::temp_dir().join(format!("sv98b-ui-{}", Uuid::now_v7()));
            let config_file = config_dir.join("client.conf");
            let recovery_gate_dir = process_dir.join("root-recovery-gate");
            fs::create_dir_all(&data_dir).expect("QML process data directory must be created");
            fs::create_dir_all(&config_dir).expect("QML process config directory must be created");
            fs::create_dir_all(&cache_dir).expect("QML process cache directory must be created");
            fs::create_dir(&runtime_dir).expect("QML process runtime directory must be created");
            fs::create_dir(&recovery_gate_dir)
                .expect("QML recovery gate directory must be created");
            fs::set_permissions(&runtime_dir, fs::Permissions::from_mode(0o700))
                .expect("QML process runtime directory must be private");
            fs::set_permissions(&recovery_gate_dir, fs::Permissions::from_mode(0o700))
                .expect("QML recovery gate directory must be private");
            let roots = seeds
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    let root = process_dir.join(format!("managed-{index}"));
                    fs::create_dir(&root).expect("QML managed root must be created");
                    root
                })
                .collect();
            ProcessPaths {
                process_dir,
                data_dir,
                config_dir,
                cache_dir,
                runtime_dir,
                config_file,
                recovery_gate_dir,
                roots,
            }
        }

        async fn prepare_process_state(
            fixture: &LiveFixture,
            seeds: &[LibrarySeed],
        ) -> (ProcessPaths, synveil_client_sync::ServerProfileId) {
            let paths = create_process_paths(fixture, seeds);
            let profile = fixture
                .local
                .server_profile(fixture.profile_id)
                .await
                .expect("QML profile must load")
                .expect("QML profile must exist");
            let grant = DeviceAuthenticationService::new(fixture.pool.as_ref())
                .create_grant(
                    fixture.owner_id,
                    synveil_auth::DeviceEnrollmentTarget::Existing(fixture.device_id),
                )
                .await
                .expect("QML enrollment grant must issue");
            let enrollment = synveil_client_sync::HttpEnrollmentClient::new(
                profile.clone(),
                synveil_client_sync::HttpClientConfig::default(),
            )
            .expect("QML enrollment client must build")
            .exchange(&grant.token)
            .await
            .expect("QML enrollment exchange must succeed");
            let state_path = paths.data_dir.join("client-sync/state.sqlite3");
            let state = Arc::new(
                LocalStateStore::open(&LocalStateConfig::new(&state_path))
                    .await
                    .expect("QML process state must open"),
            );
            state
                .save_server_profile(&profile)
                .await
                .expect("QML process profile must persist");
            let native = NativeSecureSecretStore::new();
            assert_eq!(native.state(), SecretStoreState::Available);
            state
                .store_enrollment(&enrollment, &native)
                .await
                .expect("QML process enrollment must persist in native Secret Service");

            for (seed, root) in seeds.iter().zip(&paths.roots) {
                DeviceSyncService::new(fixture.pool.as_ref().clone())
                    .ensure_checkpoint(fixture.owner_id, fixture.device_id, seed.library_id)
                    .await
                    .expect("QML server checkpoint must exist");
                let scope = synveil_client_sync::ReplicaScope::new(
                    fixture.owner_id,
                    fixture.device_id,
                    seed.library_id,
                );
                let replica =
                    FilesystemLocalReplica::initialize_for_profile(root, scope, fixture.profile_id)
                        .expect("QML managed replica must initialize");
                let binding_id = replica.binding_id();
                state
                    .bind_replica_to_profile(scope, binding_id, fixture.profile_id)
                    .await
                    .expect("QML replica binding must persist");
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
                state
                    .upsert_local_node(&local_root)
                    .await
                    .expect("QML local root must persist");
                drop(replica);
            }

            let sqlite = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect(&format!("sqlite:{}", state_path.display()))
                .await
                .expect("QML SQLite inspection connection must open");
            for seed in seeds {
                sqlx::query(
                    "UPDATE replicas SET root_node_id = ?, journal_epoch = 1,
                     applied_sequence = 0, acknowledged_sequence = 0, status = 'IDLE'
                     WHERE library_id = ?",
                )
                .bind(seed.root.id().to_string())
                .bind(seed.library_id.to_string())
                .execute(&sqlite)
                .await
                .expect("QML replica cursor must persist");
            }
            sqlite.close().await;
            state.close_pool().await;
            drop(state);
            fs::write(
                &paths.config_file,
                seeds.iter().zip(&paths.roots).fold(
                    format!("profile_id={}\n", fixture.profile_id),
                    |mut config, (seed, root)| {
                        config.push_str(&format!(
                            "library.{}={}\n",
                            seed.library_id,
                            root.display()
                        ));
                        config
                    },
                ),
            )
            .expect("QML process config must persist");
            (paths, fixture.profile_id)
        }

        fn apply_process_environment(command: &mut Command, paths: &ProcessPaths) {
            command
                .env("SYNVEIL_CLIENT_CONFIG", &paths.config_file)
                .env("SYNVEIL_DATA_DIR", &paths.data_dir)
                .env("SYNVEIL_CONFIG_DIR", &paths.config_dir)
                .env("SYNVEIL_CACHE_DIR", &paths.cache_dir)
                .env("SYNVEIL_RUNTIME_DIR", &paths.runtime_dir)
                .env_remove("SYNVEIL_QML_SMOKE_TEST")
                .env_remove("SYNVEIL_QML_LIVE_TEST")
                .env_remove("SYNVEIL_QML_LIVE_TEST_RAPID_CLICKS")
                .env_remove("SYNVEIL_QML_LIVE_TEST_EXIT_AFTER_SYNC")
                .env_remove("SYNVEIL_TEST_ROOT_RECOVERY_GATE");
        }

        fn apply_parent_environment(paths: &ProcessPaths) {
            // This ignored target is run with one test thread; the values are
            // scoped to the disposable fixture and are required for the
            // parent-side control client to resolve the child endpoint.
            unsafe {
                std::env::set_var("SYNVEIL_CLIENT_CONFIG", &paths.config_file);
                std::env::set_var("SYNVEIL_DATA_DIR", &paths.data_dir);
                std::env::set_var("SYNVEIL_CONFIG_DIR", &paths.config_dir);
                std::env::set_var("SYNVEIL_CACHE_DIR", &paths.cache_dir);
                std::env::set_var("SYNVEIL_RUNTIME_DIR", &paths.runtime_dir);
            }
        }

        fn open_log(path: &Path) -> (File, File) {
            let stdout = File::create(path).expect("child log must open");
            let stderr = stdout.try_clone().expect("child stderr log must clone");
            (stdout, stderr)
        }

        fn spawn_client(
            paths: &ProcessPaths,
            log: PathBuf,
            recovery_gate: Option<&Path>,
        ) -> ChildCapture {
            let (stdout, stderr) = open_log(&log);
            let mut command = Command::new(desktop_client_binary());
            apply_process_environment(&mut command, paths);
            if let Some(recovery_gate) = recovery_gate {
                command.env("SYNVEIL_TEST_ROOT_RECOVERY_GATE", recovery_gate);
            }
            command
                .stdout(Stdio::from(stdout))
                .stderr(Stdio::from(stderr));
            ChildCapture {
                child: command
                    .spawn()
                    .expect("production synveil-client must spawn"),
                log,
            }
        }

        fn spawn_shell(
            paths: &ProcessPaths,
            log: PathBuf,
            exit_after_sync: bool,
            rapid_clicks: bool,
        ) -> ChildCapture {
            let (stdout, stderr) = open_log(&log);
            let mut command = Command::new(desktop_shell_binary());
            apply_process_environment(&mut command, paths);
            command
                .env("QT_QPA_PLATFORM", "offscreen")
                .env("QT_FORCE_STDERR_LOGGING", "1")
                .env("QML_DISABLE_DISK_CACHE", "1")
                .env("SYNVEIL_QML_LIVE_TEST", "1");
            if exit_after_sync {
                command.env("SYNVEIL_QML_LIVE_TEST_EXIT_AFTER_SYNC", "1");
            }
            if rapid_clicks {
                command.env("SYNVEIL_QML_LIVE_TEST_RAPID_CLICKS", "1");
            }
            command
                .stdout(Stdio::from(stdout))
                .stderr(Stdio::from(stderr));
            ChildCapture {
                child: command
                    .spawn()
                    .expect("production synveil-desktop must spawn"),
                log,
            }
        }

        fn log_len(path: &Path) -> usize {
            fs::metadata(path)
                .map(|metadata| metadata.len() as usize)
                .unwrap_or_default()
        }

        fn log_delta(path: &Path, offset: usize) -> String {
            let mut file = match File::open(path) {
                Ok(file) => file,
                Err(_) => return String::new(),
            };
            let mut bytes = Vec::new();
            if file.read_to_end(&mut bytes).is_err() {
                return String::new();
            }
            String::from_utf8_lossy(bytes.get(offset..).unwrap_or_default()).into_owned()
        }

        async fn wait_for_log(path: &Path, offset: usize, needle: &str) {
            let deadline = Instant::now() + WAIT;
            while Instant::now() < deadline {
                if log_delta(path, offset).contains(needle) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            panic!("QML log did not contain expected safe evidence: {needle}");
        }

        async fn wait_for_recovery_gate(path: &Path) -> usize {
            let entered = path.join("entered");
            let deadline = Instant::now() + WAIT;
            while Instant::now() < deadline {
                if let Ok(value) = fs::read_to_string(&entered)
                    && let Ok(entries) = value.trim().parse::<usize>()
                {
                    return entries;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            panic!("production client did not reach the root recovery gate");
        }

        fn marker_field(output: &str, marker: &str, key: &str) -> Option<String> {
            output
                .lines()
                .filter(|line| line.contains(marker))
                .find_map(|line| {
                    line.split_whitespace()
                        .find_map(|field| field.strip_prefix(key))
                        .map(ToOwned::to_owned)
                })
        }

        async fn wait_for_control(
            profile_id: synveil_client_sync::ServerProfileId,
        ) -> DesktopControlClient {
            let endpoint = DesktopControlEndpoint::for_profile(
                synveil_platform::current().as_ref(),
                profile_id,
            )
            .expect("QML control endpoint must resolve");
            let deadline = Instant::now() + WAIT;
            loop {
                if let Ok(mut client) = DesktopControlClient::connect(endpoint.clone()).await
                    && client.ping().await == Ok(DesktopProcessStatus::Running)
                {
                    return client;
                }
                assert!(
                    Instant::now() < deadline,
                    "production client did not become ready"
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }

        async fn wait_for_mutation(fixture: &LiveFixture) {
            let deadline = Instant::now() + WAIT;
            while Instant::now() < deadline {
                if fixture.proxy.mutation_requests() > 0 {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            panic!("QML Sync Now did not produce a remote mutation within the acceptance window");
        }

        async fn log_control_status(client: &mut DesktopControlClient, label: &str) {
            let list = client
                .list_libraries()
                .await
                .expect("QML control status must be readable");
            let statuses: Vec<_> = list
                .libraries
                .iter()
                .map(|status| {
                    format!(
                        "runtime={:?} root={:?} auth={:?} outcome={:?} next_due_ms={:?} failures={}",
                        status.runtime_state,
                        status.root_state,
                        status.auth_state,
                        status.last_outcome,
                        status.next_due_ms,
                        status.transient_failures
                    )
                })
                .collect();
            eprintln!(
                "SYNVEIL-QML-ACCEPTANCE CONTROL label={label} libraries={} statuses={statuses:?}",
                list.libraries.len()
            );
        }

        fn report_qml_evidence(label: &str, output: &str) {
            let mut generations = BTreeSet::new();
            for line in output.lines() {
                if let Some(generation) = line
                    .split_whitespace()
                    .find_map(|field| field.strip_prefix("generation="))
                {
                    generations.insert(generation.to_owned());
                }
            }
            eprintln!(
                "SYNVEIL-QML-ACCEPTANCE UI label={label} lines={} not_connected={} connected_current={} reconnecting={} generations={generations:?} root_unavailable={} root_available={} recovering={} auth_blocked={} generic_transport={} sync_action={} sync_result={} rapid_clicks_1000={}",
                output.lines().count(),
                output.contains("SYNVEIL-QML-LIVE STATE connection=Not connected"),
                output.contains("SYNVEIL-QML-LIVE STATE connection=Connected freshness=Current"),
                output.contains("SYNVEIL-QML-LIVE STATE connection=Reconnecting"),
                output.lines().any(|line| {
                    line.contains("SYNVEIL-QML-LIVE STATE")
                        && line.contains("root=Folder unavailable")
                }),
                output.lines().any(|line| {
                    line.contains("SYNVEIL-QML-LIVE STATE")
                        && line.contains("root=Folder available")
                }),
                output.contains("SYNVEIL-QML-LIVE RECOVERING root_label=Checking folder changes"),
                output.contains("auth=Authentication blocked"),
                output.lines().any(|line| {
                    line.contains("SYNVEIL-QML-LIVE STATE")
                        && line.contains("freshness=Last known status")
                }),
                output.contains("SYNVEIL-QML-LIVE ACTION sync_now"),
                output.contains("SYNVEIL-QML-LIVE RESULT feedback="),
                output.contains("SYNVEIL-QML-LIVE ACTION sync_now_clicks=1000"),
            );
        }

        fn stop_child(capture: &mut ChildCapture) -> String {
            let _ = capture.child.kill();
            let _ = capture.child.wait();
            fs::read_to_string(&capture.log).unwrap_or_default()
        }

        async fn stop_process_gracefully(
            capture: &mut ChildCapture,
            profile_id: synveil_client_sync::ServerProfileId,
        ) -> String {
            let mut control = wait_for_control(profile_id).await;
            control
                .shutdown()
                .await
                .expect("production process shutdown request must be accepted");
            let deadline = Instant::now() + WAIT;
            while Instant::now() < deadline {
                if capture
                    .child
                    .try_wait()
                    .expect("production process status must be readable")
                    .is_some()
                {
                    return fs::read_to_string(&capture.log).unwrap_or_default();
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            panic!("production process did not stop gracefully");
        }

        async fn wait_for_exit(capture: &mut ChildCapture) -> (ExitStatus, String) {
            let deadline = Instant::now() + WAIT;
            loop {
                if let Some(status) = capture
                    .child
                    .try_wait()
                    .expect("QML child status must be readable")
                {
                    return (status, fs::read_to_string(&capture.log).unwrap_or_default());
                }
                assert!(Instant::now() < deadline, "QML shell did not exit");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }

        #[tokio::test]
        #[ignore = "requires live PostgreSQL 17, native Secret Service, and Qt"]
        async fn live_pg17_qml_production_shell_acceptance() {
            let fixture = LiveFixture::new("qml-production").await;
            let seeds = vec![
                fixture.create_library("qml-one").await,
                fixture.create_library("qml-two").await,
                fixture.create_library("qml-three").await,
            ];
            // Seed one authoritative remote change before the process starts.
            // The first real cycle then proves authenticated progress instead
            // of leaving the UI in the intentionally conservative no-op
            // authentication-unknown state.
            let _remote_seed = fixture.create_directory(&seeds[0], "qml-remote-seed").await;
            let (paths, profile_id) = prepare_process_state(&fixture, &seeds).await;
            apply_parent_environment(&paths);
            let client_log = paths.process_dir.join("client.log");
            let shell_one_log = paths.process_dir.join("shell-one.log");
            let shell_two_log = paths.process_dir.join("shell-two.log");
            let shell_three_log = paths.process_dir.join("shell-three.log");
            let shell_rapid_log = paths.process_dir.join("shell-rapid.log");

            // LIVE-UI2: start the production QML shell before its background
            // process. It must expose a safe disconnected state and then
            // transition to the same complete snapshot after the process is
            // started.
            let shell_before_process_log = paths.process_dir.join("shell-before-process.log");
            let mut shell_before_process =
                spawn_shell(&paths, shell_before_process_log.clone(), false, false);
            wait_for_log(
                &shell_before_process_log,
                0,
                "SYNVEIL-QML-LIVE START live_test=true",
            )
            .await;
            wait_for_log(
                &shell_before_process_log,
                0,
                "SYNVEIL-QML-LIVE STATE connection=Reconnecting",
            )
            .await;

            let mut client =
                spawn_client(&paths, client_log.clone(), Some(&paths.recovery_gate_dir));
            let mut control = wait_for_control(profile_id).await;
            assert_eq!(
                control
                    .list_libraries()
                    .await
                    .expect("QML process library list")
                    .libraries
                    .len(),
                3
            );
            log_control_status(&mut control, "immediately-after-process-ready").await;
            tokio::time::sleep(Duration::from_secs(3)).await;
            log_control_status(&mut control, "three-seconds-after-process-ready").await;
            drop(control);
            wait_for_log(
                &shell_before_process_log,
                0,
                "connection=Connected freshness=Current",
            )
            .await;
            let shell_before_process_output = stop_child(&mut shell_before_process);
            report_qml_evidence("before-process", &shell_before_process_output);

            // LIVE-UI1 and LIVE-UI11: the real process is now running before
            // this second shell opens and renders the complete three-library
            // model used for the mutation/action assertions.
            fs::create_dir(paths.roots[0].join("qml-live-directory"))
                .expect("QML live mutation directory must be created");
            let mut shell_one = spawn_shell(&paths, shell_one_log.clone(), false, false);
            wait_for_log(&shell_one_log, 0, "connection=Connected freshness=Current").await;
            wait_for_log(&shell_one_log, 0, "SYNVEIL-QML-LIVE START live_test=true").await;
            wait_for_log(&shell_one_log, 0, "SYNVEIL-QML-LIVE ACTION sync_now").await;
            wait_for_log(&shell_one_log, 0, "SYNVEIL-QML-LIVE RESULT feedback=").await;
            wait_for_mutation(&fixture).await;

            // LIVE-UI2 and LIVE-UI3: keep the UI alive while the production
            // process is absent, then restart the same process and require a
            // fresh controller generation.
            let before_restart = log_len(&shell_one_log);
            let _ = stop_process_gracefully(&mut client, profile_id).await;
            wait_for_log(&shell_one_log, before_restart, "SYNVEIL-QML-LIVE STATE").await;
            let mut restarted =
                spawn_client(&paths, client_log.clone(), Some(&paths.recovery_gate_dir));
            let _ = wait_for_control(profile_id).await;
            wait_for_log(
                &shell_one_log,
                before_restart,
                "connection=Connected freshness=Current",
            )
            .await;
            let before_root = log_len(&shell_one_log);
            let unavailable_root = paths.process_dir.join("managed-0-unavailable");
            fs::rename(&paths.roots[0], &unavailable_root).expect("QML root must detach");
            wait_for_log(
                &shell_one_log,
                before_root,
                "SYNVEIL-QML-LIVE STATE connection=Connected freshness=Current process=Background service running generation=2 libraries=3 root=Folder unavailable",
            )
            .await;
            fs::rename(&unavailable_root, &paths.roots[0]).expect("QML root must reattach");

            // LIVE-UI5: the real production client holds the canonical root
            // recovery transition after publishing Recovering. The marker
            // below is emitted only after QML observes the bridge property,
            // so releasing the gate cannot hide the intermediate state.
            wait_for_log(
                &shell_one_log,
                before_root,
                "SYNVEIL-QML-LIVE RECOVERING root_label=Checking folder changes",
            )
            .await;
            let recovery_entries = wait_for_recovery_gate(&paths.recovery_gate_dir).await;
            assert_eq!(
                recovery_entries, 1,
                "one root reappearance must enter the recovery gate once"
            );
            let recovery_output = log_delta(&shell_one_log, before_root);
            assert!(recovery_output.contains("root_label=Checking folder changes"));
            assert!(recovery_output.contains("freshness=Current"));
            assert_eq!(
                marker_field(
                    &recovery_output,
                    "SYNVEIL-QML-LIVE RECOVERING",
                    "same_controller_generation=",
                )
                .as_deref(),
                Some("true"),
                "Recovering must retain the active controller generation"
            );
            let recovering_generation = marker_field(
                &recovery_output,
                "SYNVEIL-QML-LIVE RECOVERING",
                "generation=",
            )
            .expect("QML Recovering marker must expose controller generation")
            .parse::<u64>()
            .expect("QML Recovering generation must be numeric");
            let previous_generation = marker_field(
                &recovery_output,
                "SYNVEIL-QML-LIVE RECOVERING",
                "prior_available_generation=",
            )
            .expect("QML Recovering marker must expose prior generation")
            .parse::<u64>()
            .expect("QML prior generation must be numeric");
            assert_eq!(recovering_generation, previous_generation);
            fs::write(paths.recovery_gate_dir.join("release"), b"release")
                .expect("QML recovery gate must release");
            wait_for_log(
                &shell_one_log,
                before_root,
                "SYNVEIL-QML-LIVE STATE connection=Connected freshness=Current process=Background service running generation=2 libraries=3 root=Folder available",
            )
            .await;

            // LIVE-UI7: replace the local endpoint with an unsafe socket. The
            // controller must render the generic security failure and no raw
            // socket path or transport diagnostic.
            let before_security = log_len(&shell_one_log);
            let endpoint = DesktopControlEndpoint::for_profile(
                synveil_platform::current().as_ref(),
                profile_id,
            )
            .expect("QML security endpoint must resolve");
            let endpoint_path = endpoint
                .unix_path()
                .expect("Linux QML endpoint must be a Unix socket");
            let _ = stop_process_gracefully(&mut restarted, profile_id).await;
            let unsafe_listener = std::os::unix::net::UnixListener::bind(endpoint_path)
                .expect("unsafe QML endpoint must bind");
            fs::set_permissions(endpoint_path, fs::Permissions::from_mode(0o666))
                .expect("unsafe QML endpoint must be made insecure");
            let security_deadline = Instant::now() + WAIT;
            while Instant::now() < security_deadline {
                let security_log = log_delta(&shell_one_log, before_security);
                if (security_log.contains("Connection unavailable")
                    || security_log.contains("Reconnecting"))
                    && security_log.contains("The local control connection is unavailable.")
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            let security_log = log_delta(&shell_one_log, before_security);
            assert!(
                security_log.contains("Connection unavailable")
                    || security_log.contains("Reconnecting")
            );
            assert!(security_log.contains("The local control connection is unavailable."));
            drop(unsafe_listener);
            let _ = fs::remove_file(endpoint_path);

            // LIVE-UI9 and LIVE-UI10: a GUI quit/reopen leaves the real
            // synveil-client process untouched and reconnects a new GUI.
            let mut client_after_security = spawn_client(&paths, client_log.clone(), None);
            let _ = wait_for_control(profile_id).await;
            let mut shell_two = spawn_shell(&paths, shell_two_log.clone(), true, false);
            let (status_two, output_two) = wait_for_exit(&mut shell_two).await;
            report_qml_evidence("gui-exit", &output_two);
            assert!(
                status_two.success(),
                "GUI quit must be graceful: {output_two}"
            );
            let mut surviving = wait_for_control(profile_id).await;
            assert_eq!(
                surviving
                    .ping()
                    .await
                    .expect("client must survive GUI quit"),
                DesktopProcessStatus::Running
            );
            drop(surviving);
            let mut shell_three = spawn_shell(&paths, shell_three_log.clone(), true, false);
            let (status_three, output_three) = wait_for_exit(&mut shell_three).await;
            report_qml_evidence("gui-reopen", &output_three);
            assert!(
                status_three.success(),
                "GUI reopen must be graceful: {output_three}"
            );
            let _ = wait_for_control(profile_id).await;

            // LIVE-UI12: one actual QML-side rapid-click loop reaches the
            // bridge 1,000 times. The bridge gate must keep one command in
            // flight and the production process must remain healthy.
            let mut shell_rapid = spawn_shell(&paths, shell_rapid_log.clone(), false, true);
            wait_for_log(
                &shell_rapid_log,
                0,
                "SYNVEIL-QML-LIVE ACTION sync_now_clicks=1000",
            )
            .await;
            wait_for_log(&shell_rapid_log, 0, "SYNVEIL-QML-LIVE RESULT feedback=").await;
            let mut rapid_surviving = wait_for_control(profile_id).await;
            assert_eq!(
                rapid_surviving
                    .ping()
                    .await
                    .expect("client must survive rapid UI requests"),
                DesktopProcessStatus::Running
            );
            drop(rapid_surviving);
            let shell_rapid_output = stop_child(&mut shell_rapid);
            report_qml_evidence("rapid-clicks", &shell_rapid_output);

            // LIVE-UI6: revoke the server credential, issue a real scheduling
            // command to the running process, and require the UI's generic
            // authentication-blocked presentation.
            let before_auth = log_len(&shell_one_log);
            DeviceAuthenticationService::new(fixture.pool.as_ref())
                .revoke_all_credentials(fixture.owner_id, fixture.device_id)
                .await
                .expect("QML fixture credentials must revoke");
            let mut auth_control = wait_for_control(profile_id).await;
            auth_control
                .sync_now(seeds[0].library_id)
                .await
                .expect("QML auth-blocking schedule must be accepted");
            drop(auth_control);
            wait_for_log(&shell_one_log, before_auth, "Authentication blocked").await;

            let shell_one_output = stop_child(&mut shell_one);
            report_qml_evidence("reconnect-root-security-auth", &shell_one_output);
            let client_output =
                stop_process_gracefully(&mut client_after_security, profile_id).await;
            assert!(!shell_one_output.contains("/tmp/") && !shell_one_output.contains("token"));
            assert!(!client_output.contains("SYNVEIL_NATIVE_SECRET_TEST"));

            let cleanup_state = LocalStateStore::open(&LocalStateConfig::new(
                paths.data_dir.join("client-sync/state.sqlite3"),
            ))
            .await
            .expect("QML cleanup state must open");
            cleanup_state
                .forget_device_credential(profile_id, &NativeSecureSecretStore::new())
                .await
                .expect("QML native credential must be deleted");
            cleanup_state.close_pool().await;
            drop(cleanup_state);
            fixture.cleanup().await;
        }
    }
}
