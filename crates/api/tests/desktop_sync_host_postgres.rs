//! Prompt 94 live PostgreSQL 17 desktop-host acceptance target.
//!
//! The target includes the established Prompt 87 fixture so every host test
//! uses the real PostgreSQL -> Axum -> device-authenticated HTTP -> SQLite ->
//! Prompt 91 path. Prompt 94 owns only the composition under test: one
//! `DesktopSyncHost`, one `SyncRuntime`, and the Prompt 93 control/signal
//! boundaries. The host tests are selected with the `live_pg17_host` filter;
//! the included fixture tests remain available as historical live coverage.
//!
//! Run with a fresh disposable PostgreSQL 17 database URL:
//!
//! ```text
//! SYNVEIL_TEST_DATABASE_URL=postgresql://... cargo test -p synveil-api \
//!   --test desktop_sync_host_postgres --locked -- --ignored live_pg17_host
//! ```

mod prompt94_fixture {
    include!("rebaseline_convergence_postgres.rs");

    mod host_tests {
        use std::{collections::BTreeMap, fs, sync::Arc, time::Duration};

        use synveil_auth::{DeviceAuthenticationService, DeviceEnrollmentTarget};
        use synveil_client_sync::{
            DesktopSyncHost, DesktopSyncHostConfig, DesktopSyncLibraryConfig, DurableChangeResult,
            EngineConfig, HttpClientConfig, LocalFingerprint, LocalReplica, ManagedRelativePath,
            ManualChangeWatcher, ObservationConfig, OutboundIntent, OutboundIntentKind,
            SyncRuntimeConfig, SyncRuntimeEvent, SyncRuntimeLibraryPhase, SyncRuntimeOutcome,
            SyncRuntimeWakeResult, WatchHint, WatchHintKind,
        };
        use synveil_core::{Revision, Sequence};
        use synveil_platform::SecretStore;

        use super::{ClientLibrary, LiveFixture, TestSecretStore};

        fn host_config() -> DesktopSyncHostConfig {
            DesktopSyncHostConfig::new(
                SyncRuntimeConfig::new(
                    Duration::from_secs(30),
                    Duration::from_secs(1),
                    Duration::from_secs(4),
                    Duration::from_secs(2),
                    2,
                )
                .expect("host runtime config must validate"),
            )
            .with_engine_config(EngineConfig::new(2, 2).expect("host engine config"))
            .with_rebaseline_page_limit(2)
            .with_observation_poll_interval(Duration::from_millis(10))
        }

        async fn host_for_remote(fixture: &LiveFixture, client: &ClientLibrary) -> DesktopSyncHost {
            let library = DesktopSyncLibraryConfig::from_remote(
                client.engine.scope(),
                fixture.remote.clone(),
                client.replica.clone(),
            )
            .expect("remote-backed host library must compose");
            DesktopSyncHost::new(
                client.state.clone(),
                Arc::new(TestSecretStore::default()) as Arc<dyn SecretStore>,
                host_config(),
                [library],
            )
            .await
            .expect("desktop host must compose")
        }

        async fn wait_for_cycle(
            events: &mut tokio::sync::broadcast::Receiver<SyncRuntimeEvent>,
            library_id: synveil_core::LibraryId,
        ) -> SyncRuntimeOutcome {
            tokio::time::timeout(Duration::from_secs(30), async {
                loop {
                    match events.recv().await {
                        Ok(SyncRuntimeEvent::CycleFinished {
                            library_id: event_library,
                            outcome,
                        }) if event_library == library_id => {
                            return outcome;
                        }
                        Ok(_) => {}
                        Err(error) => panic!("host runtime event stream failed: {error}"),
                    }
                }
            })
            .await
            .expect("host cycle must finish within the live timeout")
        }

        async fn wait_for_cycles(
            events: &mut tokio::sync::broadcast::Receiver<SyncRuntimeEvent>,
            library_ids: &[synveil_core::LibraryId],
        ) -> BTreeMap<synveil_core::LibraryId, SyncRuntimeOutcome> {
            let expected = library_ids.to_vec();
            tokio::time::timeout(Duration::from_secs(30), async {
                let mut completed = BTreeMap::new();
                while completed.len() < expected.len() {
                    match events.recv().await {
                        Ok(SyncRuntimeEvent::CycleFinished {
                            library_id,
                            outcome,
                        }) if expected.contains(&library_id) => {
                            completed.entry(library_id).or_insert(outcome);
                        }
                        Ok(_) => {}
                        Err(error) => panic!("host runtime event stream failed: {error}"),
                    }
                }
                completed
            })
            .await
            .expect("all host cycles must finish within the live timeout")
        }

        fn new_local_create_intent(
            client: &ClientLibrary,
            name_value: &str,
            epoch: Sequence,
            head: Sequence,
        ) -> OutboundIntent {
            OutboundIntent::new(
                client.seed.library_id,
                None,
                Some(client.seed.root.id()),
                OutboundIntentKind::CreateDirectory,
                ManagedRelativePath::new(name_value).expect("local path must be valid"),
                None,
                Some(LocalFingerprint::directory()),
                epoch,
                head,
                None,
                None,
                Some(Revision::new(0)),
            )
            .expect("local intent must be valid")
        }

        async fn persist_local_create_intent(
            fixture: &LiveFixture,
            client: &ClientLibrary,
            name_value: &str,
        ) -> OutboundIntent {
            let relative = ManagedRelativePath::new(name_value).expect("local path must be valid");
            fs::create_dir(client.replica.root_path().join(relative.as_path()))
                .expect("local directory must be visible");
            let (epoch, head, _) = fixture.server_head(&client.seed).await;
            let intent = new_local_create_intent(client, name_value, epoch, head);
            client
                .state
                .upsert_outbound_intent(&intent)
                .await
                .expect("local intent must persist")
        }

        async fn finish_host(host: DesktopSyncHost, fixture: LiveFixture) {
            host.shutdown().await.expect("host shutdown must complete");
            drop(host);
            fixture.cleanup().await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_host1_startup_runs_real_host_cycle_and_local_outbound() {
            let fixture = LiveFixture::new("host1-startup").await;
            let seed = fixture.create_library("host1-startup").await;
            let client = fixture.client_for(&seed, "host1-startup").await;
            client.seed_ready_state(&[]).await;
            let remote_node = fixture.create_directory(&seed, "remote-startup").await;
            let intent = persist_local_create_intent(&fixture, &client, "local-startup").await;
            fixture.proxy.reset_counts();

            let host = host_for_remote(&fixture, &client).await;
            let mut events = host.events();
            host.start().await.expect("host start");
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Progress
            );
            assert!(
                client
                    .state
                    .local_node(seed.library_id, remote_node.id())
                    .await
                    .expect("remote local node must be readable")
                    .is_some()
            );
            assert_eq!(fixture.server_node_count(&seed).await, 3);
            assert_eq!(fixture.proxy.mutation_requests(), 1);
            assert!(
                client
                    .state
                    .outbound_intent(intent.intent_id())
                    .await
                    .unwrap()
                    .is_some()
            );
            drop(client);
            finish_host(host, fixture).await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_host2_missing_credential_auth_blocks_then_same_host_resumes() {
            let fixture = LiveFixture::new("host2-credential").await;
            let seed = fixture.create_library("host2-credential").await;
            let client = fixture.client_for(&seed, "host2-credential").await;
            client.seed_ready_state(&[]).await;
            let profile = client
                .state
                .server_profile(fixture.profile_id)
                .await
                .expect("profile must be readable")
                .expect("profile must exist");
            let library = DesktopSyncLibraryConfig::http(
                client.engine.scope(),
                profile.clone(),
                client.replica.clone(),
            );
            let secrets = Arc::new(TestSecretStore::default());
            let host = DesktopSyncHost::new(
                client.state.clone(),
                secrets.clone() as Arc<dyn SecretStore>,
                host_config(),
                [library],
            )
            .await
            .expect("credential-gated host must construct");
            let mut events = host.events();
            host.start()
                .await
                .expect("host must start without a credential");
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::AuthBlocked
            );
            assert_eq!(
                host.status(seed.library_id).expect("status").phase(),
                SyncRuntimeLibraryPhase::AuthBlocked
            );

            let grant = DeviceAuthenticationService::new(fixture.pool.as_ref())
                .create_grant(
                    fixture.owner_id,
                    DeviceEnrollmentTarget::Existing(fixture.device_id),
                )
                .await
                .expect("replacement grant must issue");
            let enrollment = synveil_client_sync::HttpEnrollmentClient::new(
                profile,
                HttpClientConfig::default(),
            )
            .expect("enrollment client")
            .exchange(&grant.token)
            .await
            .expect("replacement enrollment must succeed");
            let credential_result = host
                .credential_controller()
                .replace_enrollment(&enrollment, &[seed.library_id])
                .await
                .expect("credential replacement must persist");
            assert_eq!(credential_result.wake_results().len(), 1);
            assert!(credential_result.wake_results()[0].1.was_accepted());
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Idle
            );
            drop(client);
            finish_host(host, fixture).await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_host3_observation_reaches_same_runtime_and_server() {
            let fixture = LiveFixture::new("host3-observation").await;
            let seed = fixture.create_library("host3-observation").await;
            let client = fixture.client_for(&seed, "host3-observation").await;
            client.seed_ready_state(&[]).await;
            let (watcher, source) = ManualChangeWatcher::with_capacity(16);
            let library = DesktopSyncLibraryConfig::from_remote(
                client.engine.scope(),
                fixture.remote.clone(),
                client.replica.clone(),
            )
            .expect("remote host library")
            .with_watcher(
                Box::new(watcher),
                ObservationConfig::new(16, 16, 4, Duration::ZERO).expect("observation config"),
            );
            let host = DesktopSyncHost::new(
                client.state.clone(),
                Arc::new(TestSecretStore::default()) as Arc<dyn SecretStore>,
                host_config(),
                [library],
            )
            .await
            .expect("host must construct");
            let identity = host.runtime_identity();
            assert_eq!(
                host.observer(seed.library_id)
                    .expect("observer must be registered")
                    .runtime_identity(),
                Some(identity)
            );
            let mut events = host.events();
            host.start().await.expect("host start");
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Idle
            );
            let name = "observed-directory";
            fs::create_dir(client.replica.root_path().join(name)).expect("visible directory");
            source
                .push(
                    WatchHint::new(
                        WatchHintKind::Create,
                        vec![ManagedRelativePath::new(name).expect("observation path")],
                    )
                    .expect("observation hint"),
                )
                .expect("observation source");
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Progress
            );
            assert_eq!(fixture.server_node_count(&seed).await, 2);
            drop(client);
            finish_host(host, fixture).await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_host4_manual_sync_now_uses_prompt92_host_runtime() {
            let fixture = LiveFixture::new("host4-manual").await;
            let seed = fixture.create_library("host4-manual").await;
            let client = fixture.client_for(&seed, "host4-manual").await;
            client.seed_ready_state(&[]).await;
            let host = host_for_remote(&fixture, &client).await;
            let mut events = host.events();
            host.start().await.expect("host start");
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Idle
            );
            let remote_node = fixture.create_directory(&seed, "manual-remote").await;
            assert!(host.sync_now(seed.library_id).was_accepted());
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Progress
            );
            assert!(
                client
                    .state
                    .local_node(seed.library_id, remote_node.id())
                    .await
                    .expect("manual local state")
                    .is_some()
            );
            drop(client);
            finish_host(host, fixture).await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_host5_network_hint_recovers_transient_runtime() {
            let fixture = LiveFixture::new("host5-network").await;
            let seed = fixture.create_library("host5-network").await;
            let client = fixture.client_for(&seed, "host5-network").await;
            client.seed_ready_state(&[]).await;
            let host = host_for_remote(&fixture, &client).await;
            let mut events = host.events();
            host.start().await.expect("host start");
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Idle
            );
            let next_feed = fixture
                .proxy
                .feed_requests
                .load(std::sync::atomic::Ordering::SeqCst)
                + 1;
            fixture
                .proxy
                .fail_feed_call
                .store(next_feed, std::sync::atomic::Ordering::SeqCst);
            assert!(host.sync_now(seed.library_id).was_accepted());
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Offline
            );
            fixture
                .proxy
                .fail_feed_call
                .store(0, std::sync::atomic::Ordering::SeqCst);
            assert!(host.network_available()[0].1.was_accepted());
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Idle
            );
            drop(client);
            finish_host(host, fixture).await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_host6_shutdown_waits_for_active_bounded_cycle() {
            let fixture = LiveFixture::new("host6-shutdown").await;
            let seed = fixture.create_library("host6-shutdown").await;
            let blocker = fixture.create_directory(&seed, "shutdown-blocker").await;
            let client = fixture.client_for(&seed, "host6-shutdown").await;
            client.seed_stale_state(Some(&blocker)).await;
            fixture.compact(&seed).await;
            fixture
                .proxy
                .hold_next_page
                .store(true, std::sync::atomic::Ordering::SeqCst);
            let page_started = fixture.proxy.page_started.notified();
            let host = host_for_remote(&fixture, &client).await;
            host.start().await.expect("host start");
            tokio::time::timeout(Duration::from_secs(30), page_started)
                .await
                .expect("active bounded cycle must start");
            let shutdown_host = host.clone();
            let shutdown = tokio::spawn(async move { shutdown_host.shutdown().await });
            tokio::task::yield_now().await;
            fixture.proxy.release_page.notify_one();
            shutdown
                .await
                .expect("shutdown task")
                .expect("active cycle shutdown");
            assert_eq!(
                host.lifecycle(),
                synveil_client_sync::DesktopSyncHostLifecycle::Stopped
            );
            drop(client);
            drop(host);
            fixture.cleanup().await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_host7_observation_after_shutdown_remains_durable() {
            let fixture = LiveFixture::new("host7-observation-shutdown").await;
            let seed = fixture.create_library("host7-observation-shutdown").await;
            let client = fixture
                .client_for(&seed, "host7-observation-shutdown")
                .await;
            client.seed_ready_state(&[]).await;
            let host = host_for_remote(&fixture, &client).await;
            host.start().await.expect("host start");
            let mut events = host.events();
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Idle
            );
            host.shutdown().await.expect("host shutdown");
            let intent = {
                let (epoch, head, _) = fixture.server_head(&seed).await;
                new_local_create_intent(&client, "after-shutdown", epoch, head)
            };
            fs::create_dir(client.replica.root_path().join("after-shutdown"))
                .expect("durable local directory");
            let result = host
                .outbound_intent_producer()
                .upsert(&intent)
                .await
                .expect("post-shutdown durable producer");
            assert_eq!(
                result.notification().durable_result(),
                DurableChangeResult::Committed
            );
            assert_eq!(
                result.notification().wake_result(),
                Some(SyncRuntimeWakeResult::RuntimeStopped)
            );
            drop(host);

            let restarted = host_for_remote(&fixture, &client).await;
            let mut restart_events = restarted.events();
            restarted.start().await.expect("restart host start");
            assert_eq!(
                wait_for_cycle(&mut restart_events, seed.library_id).await,
                SyncRuntimeOutcome::Progress
            );
            assert_eq!(fixture.proxy.mutation_requests(), 1);
            drop(client);
            finish_host(restarted, fixture).await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_host8_restart_reconstructs_ephemeral_runtime_only() {
            let fixture = LiveFixture::new("host8-restart").await;
            let seed = fixture.create_library("host8-restart").await;
            let client = fixture.client_for(&seed, "host8-restart").await;
            client.seed_ready_state(&[]).await;
            let intent = persist_local_create_intent(&fixture, &client, "restart-intent").await;
            let host = host_for_remote(&fixture, &client).await;
            let first_identity = host.runtime_identity();
            host.start().await.expect("first host start");
            host.shutdown().await.expect("first host shutdown");
            drop(host);

            let restarted = host_for_remote(&fixture, &client).await;
            assert_ne!(restarted.runtime_identity(), first_identity);
            let mut events = restarted.events();
            restarted.start().await.expect("restarted host start");
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Progress
            );
            assert!(
                client
                    .state
                    .outbound_intent(intent.intent_id())
                    .await
                    .unwrap()
                    .is_some()
            );
            drop(client);
            finish_host(restarted, fixture).await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_host9_three_libraries_share_one_runtime() {
            let fixture = LiveFixture::new("host9-multi-library").await;
            let first_seed = fixture.create_library("host9-first").await;
            let second_seed = fixture.create_library("host9-second").await;
            let third_seed = fixture.create_library("host9-third").await;
            let first = fixture.client_for(&first_seed, "host9-first").await;
            let second = fixture.client_for(&second_seed, "host9-second").await;
            let third = fixture.client_for(&third_seed, "host9-third").await;
            first.seed_ready_state(&[]).await;
            second.seed_ready_state(&[]).await;
            third.seed_ready_state(&[]).await;
            let libraries = [
                DesktopSyncLibraryConfig::from_remote(
                    first.engine.scope(),
                    fixture.remote.clone(),
                    first.replica.clone(),
                )
                .expect("first host library"),
                DesktopSyncLibraryConfig::from_remote(
                    second.engine.scope(),
                    fixture.remote.clone(),
                    second.replica.clone(),
                )
                .expect("second host library"),
                DesktopSyncLibraryConfig::from_remote(
                    third.engine.scope(),
                    fixture.remote.clone(),
                    third.replica.clone(),
                )
                .expect("third host library"),
            ];
            let host = DesktopSyncHost::new(
                fixture.local.clone(),
                Arc::new(TestSecretStore::default()) as Arc<dyn SecretStore>,
                host_config(),
                libraries,
            )
            .await
            .expect("multi-library host");
            let identity = host.runtime_identity();
            assert_eq!(host.registered_libraries().len(), 3);
            let mut events = host.events();
            host.start().await.expect("multi-library start");
            let completions = wait_for_cycles(
                &mut events,
                &[
                    first_seed.library_id,
                    second_seed.library_id,
                    third_seed.library_id,
                ],
            )
            .await;
            assert_eq!(completions.len(), 3);
            assert!(
                completions
                    .values()
                    .all(|outcome| *outcome == SyncRuntimeOutcome::Idle)
            );
            assert_eq!(host.runtime_identity(), identity);
            assert_eq!(host.statuses().len(), 3);
            drop(first);
            drop(second);
            drop(third);
            finish_host(host, fixture).await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_host10_repeated_lifecycle_has_no_duplicate_runtime_or_mutation() {
            let fixture = LiveFixture::new("host10-repeated").await;
            let seed = fixture.create_library("host10-repeated").await;
            let client = fixture.client_for(&seed, "host10-repeated").await;
            client.seed_ready_state(&[]).await;
            for _ in 0..100 {
                let host = host_for_remote(&fixture, &client).await;
                let identity = host.runtime_identity();
                let mut events = host.events();
                host.start().await.expect("repeated host start");
                wait_for_cycle(&mut events, seed.library_id).await;
                assert_eq!(host.runtime_identity(), identity);
                host.shutdown().await.expect("repeated host shutdown");
                assert_eq!(
                    host.lifecycle(),
                    synveil_client_sync::DesktopSyncHostLifecycle::Stopped
                );
                drop(host);
            }
            assert_eq!(fixture.proxy.mutation_requests(), 0);
            drop(client);
            fixture.cleanup().await;
        }
    }
}
