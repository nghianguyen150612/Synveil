//! Prompt 93 live PostgreSQL 17 signal-integration acceptance target.
//!
//! This target deliberately reuses the Prompt 87/91 fixture instead of
//! inventing a second server harness. The tests exercise the real PostgreSQL
//! -> Axum -> device-authenticated HTTP -> SQLite -> Prompt 91 -> Prompt 92
//! path, with Prompt 93's canonical durable producers attached to the shared
//! runtime.
//!
//! Run with:
//!
//! ```text
//! SYNVEIL_TEST_DATABASE_URL=postgresql://... cargo test -p synveil-api \
//!   --test sync_runtime_signals_postgres --locked -- --ignored live_pg17_signal_
//! ```

mod prompt93_fixture {
    include!("rebaseline_convergence_postgres.rs");

    mod signal_tests {
        use std::{
            collections::BTreeSet,
            fs,
            sync::{
                Arc, Mutex as StdMutex,
                atomic::{AtomicBool, Ordering},
            },
            time::Duration,
        };

        use async_trait::async_trait;
        use sqlx::sqlite::SqlitePoolOptions;
        use synveil_auth::{DeviceAuthenticationService, DeviceEnrollmentTarget};
        use synveil_client_sync::{
            BidirectionalSyncCycleRunner, ClientSyncError, CredentialLifecycleController,
            DurableChangeResult, EngineConfig, HttpClientConfig, HttpEnrollmentClient,
            HttpSyncRemote, InboundSyncEngine, LocalFingerprint, LocalReplica, LocalStateStore,
            ManagedRelativePath, ManualChangeWatcher, ObservationConfig, OutboundIntent,
            OutboundIntentKind, OutboundIntentProducer, OutboundObservationEngine,
            OutboundSubmissionEngine, RebaselineConvergenceCoordinator, ReplicaScope,
            ServerProfileId, SyncCycleExecutor, SyncRuntime, SyncRuntimeConfig, SyncRuntimeEvent,
            SyncRuntimeOutcome, SyncRuntimeWakeReason, SyncRuntimeWakeResult, SyncWakeNotifier,
            WatchHint, WatchHintKind,
        };
        use synveil_core::Revision;
        use synveil_platform::SecretStore;

        use super::{ClientLibrary, LiveFixture, TestSecretStore};

        struct DroppingNotifier;

        impl SyncWakeNotifier for DroppingNotifier {
            fn wake_library(
                &self,
                _library_id: synveil_core::LibraryId,
                _reason: SyncRuntimeWakeReason,
            ) -> SyncRuntimeWakeResult {
                SyncRuntimeWakeResult::RuntimeStopped
            }
        }

        /// `HttpSyncRemote` deliberately binds one immutable credential. A
        /// real embedding rebuilds its authenticated Prompt 91 runner after a
        /// successful enrollment replacement; this test-only executor models
        /// that explicit rebinding at the same post-persistence wake boundary.
        struct CredentialRefreshingCycle {
            current: StdMutex<Arc<BidirectionalSyncCycleRunner>>,
            scope: ReplicaScope,
            profile_id: ServerProfileId,
            state: Arc<LocalStateStore>,
            replica: Arc<dyn LocalReplica>,
            secret_store: Arc<TestSecretStore>,
            refresh_requested: Arc<AtomicBool>,
        }

        impl CredentialRefreshingCycle {
            fn new(
                initial: Arc<BidirectionalSyncCycleRunner>,
                client: &ClientLibrary,
                profile_id: ServerProfileId,
                secret_store: Arc<TestSecretStore>,
                refresh_requested: Arc<AtomicBool>,
            ) -> Self {
                let replica: Arc<dyn LocalReplica> = client.replica.clone();
                Self {
                    current: StdMutex::new(initial),
                    scope: client.engine.scope(),
                    profile_id,
                    state: client.state.clone(),
                    replica,
                    secret_store,
                    refresh_requested,
                }
            }

            async fn build_current_cycle(
                &self,
            ) -> Result<Arc<BidirectionalSyncCycleRunner>, ClientSyncError> {
                let profile = self
                    .state
                    .server_profile(self.profile_id)
                    .await?
                    .ok_or(ClientSyncError::InvalidServerProfile)?;
                let loaded = self
                    .state
                    .load_device_credential(self.profile_id, self.secret_store.as_ref())
                    .await?
                    .ok_or(ClientSyncError::AuthenticationRequired)?;
                let remote = Arc::new(HttpSyncRemote::new(
                    profile,
                    self.scope.device_id(),
                    loaded,
                    HttpClientConfig::default(),
                )?);
                let engine = Arc::new(
                    InboundSyncEngine::new(
                        self.scope,
                        remote.clone(),
                        self.replica.clone(),
                        self.state.clone(),
                        EngineConfig::new(2, 2)?,
                    )
                    .await?,
                );
                let convergence = Arc::new(RebaselineConvergenceCoordinator::new(
                    engine,
                    remote.clone(),
                    self.state.clone(),
                    2,
                )?);
                let outbound = Arc::new(
                    OutboundSubmissionEngine::new(
                        self.scope,
                        remote,
                        self.replica.clone(),
                        self.state.clone(),
                    )
                    .await?,
                );
                Ok(Arc::new(BidirectionalSyncCycleRunner::new(
                    convergence,
                    outbound,
                )?))
            }
        }

        #[async_trait]
        impl SyncCycleExecutor for CredentialRefreshingCycle {
            fn scope(&self) -> ReplicaScope {
                self.scope
            }

            async fn run_once(
                &self,
                observed_at: synveil_core::Timestamp,
            ) -> Result<synveil_client_sync::SyncCycleResult, ClientSyncError> {
                let cycle = if self.refresh_requested.swap(false, Ordering::AcqRel) {
                    let refreshed = self.build_current_cycle().await?;
                    *self.current.lock().unwrap() = refreshed.clone();
                    refreshed
                } else {
                    self.current.lock().unwrap().clone()
                };
                cycle.run_once(observed_at).await
            }
        }

        struct CredentialRefreshNotifier {
            runtime: SyncRuntime,
            refresh_requested: Arc<AtomicBool>,
        }

        impl SyncWakeNotifier for CredentialRefreshNotifier {
            fn wake_library(
                &self,
                library_id: synveil_core::LibraryId,
                reason: SyncRuntimeWakeReason,
            ) -> SyncRuntimeWakeResult {
                if reason == SyncRuntimeWakeReason::CredentialChanged {
                    self.refresh_requested.store(true, Ordering::Release);
                }
                self.runtime.wake_library_status(library_id, reason)
            }
        }

        struct ConcurrencyCheckedCycle {
            inner: Arc<BidirectionalSyncCycleRunner>,
            active: Arc<AtomicBool>,
            overlap: Arc<AtomicBool>,
        }

        #[async_trait]
        impl SyncCycleExecutor for ConcurrencyCheckedCycle {
            fn scope(&self) -> ReplicaScope {
                self.inner.scope()
            }

            async fn run_once(
                &self,
                observed_at: synveil_core::Timestamp,
            ) -> Result<synveil_client_sync::SyncCycleResult, ClientSyncError> {
                if self.active.swap(true, Ordering::AcqRel) {
                    self.overlap.store(true, Ordering::Release);
                }
                let result = self.inner.run_once(observed_at).await;
                self.active.store(false, Ordering::Release);
                result
            }
        }

        struct CountingRuntimeNotifier {
            runtime: SyncRuntime,
            calls: Arc<std::sync::atomic::AtomicUsize>,
        }

        impl SyncWakeNotifier for CountingRuntimeNotifier {
            fn wake_library(
                &self,
                library_id: synveil_core::LibraryId,
                reason: SyncRuntimeWakeReason,
            ) -> SyncRuntimeWakeResult {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.runtime.wake_library_status(library_id, reason)
            }
        }

        async fn cycle_for(
            fixture: &LiveFixture,
            client: &ClientLibrary,
        ) -> Arc<BidirectionalSyncCycleRunner> {
            let convergence = Arc::new(
                RebaselineConvergenceCoordinator::new(
                    client.engine.clone(),
                    fixture.remote.clone(),
                    fixture.local.clone(),
                    2,
                )
                .expect("Prompt 87 convergence coordinator must build"),
            );
            let outbound = Arc::new(
                OutboundSubmissionEngine::new(
                    client.engine.scope(),
                    fixture.remote.clone(),
                    client.replica.clone(),
                    fixture.local.clone(),
                )
                .await
                .expect("Prompt 88 outbound engine must build"),
            );
            Arc::new(
                BidirectionalSyncCycleRunner::new(convergence, outbound)
                    .expect("Prompt 91 cycle runner must compose"),
            )
        }

        fn runtime_config() -> SyncRuntimeConfig {
            SyncRuntimeConfig::new(
                Duration::from_secs(1),
                Duration::from_secs(1),
                Duration::from_secs(4),
                Duration::from_secs(2),
                2,
            )
            .expect("Prompt 93 live config must validate")
        }

        async fn wait_for_cycle(
            events: &mut tokio::sync::broadcast::Receiver<SyncRuntimeEvent>,
            library_id: synveil_core::LibraryId,
        ) -> SyncRuntimeOutcome {
            tokio::time::timeout(Duration::from_secs(15), async {
                loop {
                    match events.recv().await {
                        Ok(SyncRuntimeEvent::CycleFinished {
                            library_id: event_library,
                            outcome,
                        }) if event_library == library_id => return outcome,
                        Ok(_) => {}
                        Err(error) => panic!("runtime event stream failed: {error}"),
                    }
                }
            })
            .await
            .expect("runtime cycle must finish within the bounded live timeout")
        }

        async fn wait_for_cycle_count(
            events: &mut tokio::sync::broadcast::Receiver<SyncRuntimeEvent>,
            library_id: synveil_core::LibraryId,
            count: usize,
        ) -> Vec<SyncRuntimeOutcome> {
            tokio::time::timeout(Duration::from_secs(20), async {
                let mut outcomes = Vec::with_capacity(count);
                while outcomes.len() < count {
                    match events.recv().await {
                        Ok(SyncRuntimeEvent::CycleFinished {
                            library_id: event_library,
                            outcome,
                        }) if event_library == library_id => outcomes.push(outcome),
                        Ok(_) => {}
                        Err(error) => panic!("runtime event stream failed: {error}"),
                    }
                }
                outcomes
            })
            .await
            .expect("live runtime follow-up cycles must finish within the bounded timeout")
        }

        async fn wait_for_libraries(
            events: &mut tokio::sync::broadcast::Receiver<SyncRuntimeEvent>,
            library_ids: &[synveil_core::LibraryId],
        ) {
            let expected = library_ids.iter().copied().collect::<BTreeSet<_>>();
            tokio::time::timeout(Duration::from_secs(15), async {
                let mut seen = BTreeSet::new();
                while seen != expected {
                    match events.recv().await {
                        Ok(SyncRuntimeEvent::CycleFinished { library_id, .. })
                            if expected.contains(&library_id) =>
                        {
                            seen.insert(library_id);
                        }
                        Ok(_) => {}
                        Err(error) => panic!("runtime event stream failed: {error}"),
                    }
                }
            })
            .await
            .expect("every live library must receive a bounded startup cycle");
        }

        async fn outbound_state_counts(client: &ClientLibrary) -> Vec<(String, i64)> {
            let sqlite = SqlitePoolOptions::new()
                .max_connections(1)
                .connect(&format!(
                    "sqlite:{}?mode=ro",
                    client.state.database_path().display()
                ))
                .await
                .expect("client SQLite inspection connection must open");
            let states = sqlx::query_as(
                "SELECT state, COUNT(*) FROM outbound_intents GROUP BY state ORDER BY state",
            )
            .fetch_all(&sqlite)
            .await
            .expect("outbound intent state counts must be readable");
            sqlite.close().await;
            states
        }

        async fn new_directory_intent(
            fixture: &LiveFixture,
            client: &ClientLibrary,
            name_value: &str,
        ) -> OutboundIntent {
            let relative = ManagedRelativePath::new(name_value).expect("local name must be valid");
            fs::create_dir(client.replica.root_path().join(relative.as_path()))
                .expect("local directory must be visible");
            let (epoch, head, _) = fixture.server_head(&client.seed).await;
            OutboundIntent::new(
                client.seed.library_id,
                None,
                Some(client.seed.root.id()),
                OutboundIntentKind::CreateDirectory,
                relative,
                None,
                Some(LocalFingerprint::directory()),
                epoch,
                head,
                None,
                None,
                Some(Revision::new(0)),
            )
            .expect("local directory intent must be valid")
        }

        async fn start_runtime(
            fixture: &LiveFixture,
            client: &ClientLibrary,
        ) -> (
            SyncRuntime,
            synveil_client_sync::SyncRuntimeHandle,
            tokio::sync::broadcast::Receiver<SyncRuntimeEvent>,
        ) {
            let runtime = SyncRuntime::new(runtime_config());
            runtime
                .register_library(cycle_for(fixture, client).await)
                .expect("runtime registration must succeed");
            let events = runtime.events();
            let handle = runtime.start().expect("runtime startup must succeed");
            (runtime, handle, events)
        }

        fn start_runtime_with_executor(
            executor: Arc<dyn SyncCycleExecutor>,
        ) -> (
            SyncRuntime,
            synveil_client_sync::SyncRuntimeHandle,
            tokio::sync::broadcast::Receiver<SyncRuntimeEvent>,
        ) {
            let runtime = SyncRuntime::new(runtime_config());
            runtime
                .register_executor(executor)
                .expect("runtime registration must succeed");
            let events = runtime.events();
            let handle = runtime.start().expect("runtime startup must succeed");
            (runtime, handle, events)
        }

        async fn settle_observation(observer: &OutboundObservationEngine) {
            for _ in 0..128 {
                let state = observer
                    .reconcile_once()
                    .await
                    .expect("observation startup reconciliation must succeed");
                if !state.scan_active() && !state.rescan_required() {
                    return;
                }
            }
            panic!("live observation startup reconciliation did not converge");
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_signal_local_intent_wake() {
            let fixture = LiveFixture::new("signal-local-intent").await;
            let seed = fixture.create_library("signal-local-intent").await;
            let client = fixture.client_for(&seed, "signal-local-intent").await;
            client.seed_ready_state(&[]).await;
            let (runtime, handle, mut events) = start_runtime(&fixture, &client).await;
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Idle
            );

            let intent = new_directory_intent(&fixture, &client, "producer-wake").await;
            let producer = OutboundIntentProducer::new(
                client.state.clone(),
                Arc::new(runtime.clone()) as Arc<dyn SyncWakeNotifier>,
            );
            let result = producer
                .upsert(&intent)
                .await
                .expect("durable producer must persist");
            assert_eq!(
                result.notification().durable_result(),
                DurableChangeResult::Committed
            );
            assert!(result.notification().wake_was_accepted());
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Progress
            );
            assert_eq!(fixture.proxy.mutation_requests(), 1);

            handle
                .shutdown()
                .await
                .expect("signal runtime must shut down");
            fixture.cleanup().await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_signal_lost_wake_recovers_on_periodic_poll() {
            let fixture = LiveFixture::new("signal-periodic-recovery").await;
            let seed = fixture.create_library("signal-periodic-recovery").await;
            let client = fixture.client_for(&seed, "signal-periodic-recovery").await;
            client.seed_ready_state(&[]).await;
            let (runtime, handle, mut events) = start_runtime(&fixture, &client).await;
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Idle
            );

            let intent = new_directory_intent(&fixture, &client, "periodic-recovery").await;
            let producer = OutboundIntentProducer::new(
                client.state.clone(),
                Arc::new(DroppingNotifier) as Arc<dyn SyncWakeNotifier>,
            );
            let result = producer
                .upsert(&intent)
                .await
                .expect("durable producer must persist despite dropped wake");
            assert_eq!(
                result.notification().wake_result(),
                Some(SyncRuntimeWakeResult::RuntimeStopped)
            );
            assert_eq!(fixture.proxy.mutation_requests(), 0);

            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Progress
            );
            assert_eq!(fixture.proxy.mutation_requests(), 1);

            handle
                .shutdown()
                .await
                .expect("periodic recovery runtime must shut down");
            drop(runtime);
            fixture.cleanup().await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_signal_observation_batch_wakes_once() {
            let fixture = LiveFixture::new("signal-observation").await;
            let seed = fixture.create_library("signal-observation").await;
            let client = fixture.client_for(&seed, "signal-observation").await;
            client.seed_ready_state(&[]).await;
            let (runtime, handle, mut events) = start_runtime(&fixture, &client).await;
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Idle
            );

            let (watcher, source) = ManualChangeWatcher::with_capacity(8);
            let observer = OutboundObservationEngine::new_with_wake_notifier(
                client.engine.scope(),
                client.replica.clone(),
                client.state.clone(),
                Box::new(watcher),
                ObservationConfig::new(8, 8, 2, Duration::ZERO)
                    .expect("observation config must validate"),
                Arc::new(runtime.clone()) as Arc<dyn SyncWakeNotifier>,
            )
            .await
            .expect("observer must build");
            observer.start().await.expect("observer must start");
            settle_observation(&observer).await;

            fs::create_dir(client.replica.root_path().join("observed-directory"))
                .expect("observed directory must be visible");
            source
                .push(
                    WatchHint::new(
                        WatchHintKind::Create,
                        vec![
                            ManagedRelativePath::new("observed-directory").expect("observed path"),
                        ],
                    )
                    .expect("watch hint must be valid"),
                )
                .expect("watch hint queue must accept event");
            let result = observer
                .poll_once_with_notification()
                .await
                .expect("observation poll must succeed");
            assert_eq!(
                result.notification().durable_result(),
                DurableChangeResult::Committed
            );
            assert!(result.notification().wake_was_accepted());
            assert_eq!(fixture.proxy.mutation_requests(), 0);
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Progress
            );
            assert_eq!(fixture.proxy.mutation_requests(), 1);

            observer.shutdown().await.expect("observer must shut down");
            handle
                .shutdown()
                .await
                .expect("observation runtime must shut down");
            fixture.cleanup().await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_signal_observation_burst_is_bounded() {
            let fixture = LiveFixture::new("signal-observation-burst").await;
            let seed = fixture.create_library("signal-observation-burst").await;
            let client = fixture.client_for(&seed, "signal-observation-burst").await;
            client.seed_ready_state(&[]).await;
            let cycle = cycle_for(&fixture, &client).await;
            let active = Arc::new(AtomicBool::new(false));
            let overlap = Arc::new(AtomicBool::new(false));
            let (runtime, handle, mut events) =
                start_runtime_with_executor(Arc::new(ConcurrencyCheckedCycle {
                    inner: cycle,
                    active,
                    overlap: overlap.clone(),
                }) as Arc<dyn SyncCycleExecutor>);
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Idle
            );

            let (watcher, source) = ManualChangeWatcher::with_capacity(32);
            let wake_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let notifier = Arc::new(CountingRuntimeNotifier {
                runtime: runtime.clone(),
                calls: wake_calls.clone(),
            }) as Arc<dyn SyncWakeNotifier>;
            let observer = OutboundObservationEngine::new_with_wake_notifier(
                client.engine.scope(),
                client.replica.clone(),
                client.state.clone(),
                Box::new(watcher),
                ObservationConfig::new(32, 32, 4, Duration::ZERO)
                    .expect("burst observation config must validate"),
                notifier,
            )
            .await
            .expect("burst observer must build");
            observer.start().await.expect("burst observer must start");
            settle_observation(&observer).await;

            for index in 0..8 {
                let name = format!("observed-burst-{index}");
                fs::create_dir(client.replica.root_path().join(&name))
                    .expect("burst directory must be visible");
                source
                    .push(
                        WatchHint::new(
                            WatchHintKind::Create,
                            vec![
                                ManagedRelativePath::new(&name).expect("burst path must be valid"),
                            ],
                        )
                        .expect("burst hint must be valid"),
                    )
                    .expect("burst hint queue must accept event");
            }
            let result = observer
                .poll_once_with_notification()
                .await
                .expect("burst observation poll must succeed");
            assert_eq!(
                result.notification().durable_result(),
                DurableChangeResult::Committed
            );
            assert!(result.notification().wake_was_accepted());
            assert_eq!(result.inspected_hints(), 8);

            let first = wait_for_cycle(&mut events, seed.library_id).await;
            assert_eq!(first, SyncRuntimeOutcome::Progress);
            let second = wait_for_cycle(&mut events, seed.library_id).await;
            assert_eq!(second, SyncRuntimeOutcome::FatalLocal);
            assert_eq!(fixture.proxy.mutation_requests(), 1);
            assert_eq!(fixture.server_node_count(&seed).await, 2);
            assert_eq!(observer.count_pending_intents().await.unwrap(), 6);
            assert_eq!(
                outbound_state_counts(&client).await,
                vec![
                    ("BLOCKED".to_owned(), 1),
                    ("PENDING".to_owned(), 6),
                    ("RECONCILED".to_owned(), 1),
                ]
            );
            assert_eq!(wake_calls.load(Ordering::SeqCst), 1);
            assert!(!overlap.load(Ordering::Acquire));
            observer
                .shutdown()
                .await
                .expect("burst observer must shut down");
            handle
                .shutdown()
                .await
                .expect("burst runtime must shut down");
            fixture.cleanup().await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_signal_credential_resume() {
            let fixture = LiveFixture::new("signal-credential").await;
            let seed = fixture.create_library("signal-credential").await;
            let client = fixture.client_for(&seed, "signal-credential").await;
            client.seed_ready_state(&[]).await;
            sqlx::query("UPDATE devices SET status = 'PAUSED' WHERE id = $1")
                .bind(fixture.device_id.into_uuid())
                .execute(&fixture.inspection)
                .await
                .expect("credential test must pause the device");

            let secret_store = Arc::new(TestSecretStore::default());
            let initial_cycle = cycle_for(&fixture, &client).await;
            let refresh_requested = Arc::new(AtomicBool::new(false));
            let refreshing_cycle = Arc::new(CredentialRefreshingCycle::new(
                initial_cycle,
                &client,
                fixture.profile_id,
                secret_store.clone(),
                refresh_requested.clone(),
            ));
            let (runtime, handle, mut events) =
                start_runtime_with_executor(refreshing_cycle as Arc<dyn SyncCycleExecutor>);
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::AuthBlocked
            );
            sqlx::query("UPDATE devices SET status = 'ACTIVE' WHERE id = $1")
                .bind(fixture.device_id.into_uuid())
                .execute(&fixture.inspection)
                .await
                .expect("credential test must reactivate the device");

            let profile = client
                .state
                .server_profile(fixture.profile_id)
                .await
                .expect("profile must be readable")
                .expect("profile must exist");
            let grant = DeviceAuthenticationService::new(fixture.pool.as_ref())
                .create_grant(
                    fixture.owner_id,
                    DeviceEnrollmentTarget::Existing(fixture.device_id),
                )
                .await
                .expect("replacement enrollment grant must issue");
            let enrollment = HttpEnrollmentClient::new(profile, HttpClientConfig::default())
                .expect("replacement enrollment client must build")
                .exchange(&grant.token)
                .await
                .expect("replacement enrollment exchange must succeed");
            let controller = CredentialLifecycleController::new(
                client.state.clone(),
                secret_store.clone() as Arc<dyn SecretStore>,
                Arc::new(CredentialRefreshNotifier {
                    runtime: runtime.clone(),
                    refresh_requested,
                }) as Arc<dyn SyncWakeNotifier>,
            );
            let result = controller
                .replace_enrollment(&enrollment, &[seed.library_id])
                .await
                .expect("usable replacement credential must persist");
            assert_eq!(result.wake_results().len(), 1);
            assert!(result.wake_results()[0].1.was_accepted());
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Idle
            );

            handle
                .shutdown()
                .await
                .expect("credential runtime must shut down");
            fixture.cleanup().await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_signal_network_recovery_bypasses_transient_wait() {
            let fixture = LiveFixture::new("signal-network").await;
            let seed = fixture.create_library("signal-network").await;
            let client = fixture.client_for(&seed, "signal-network").await;
            client.seed_ready_state(&[]).await;
            let (runtime, handle, mut events) = start_runtime(&fixture, &client).await;
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Idle
            );

            let next_feed_call = fixture.proxy.feed_requests.load(Ordering::SeqCst) + 1;
            fixture
                .proxy
                .fail_feed_call
                .store(next_feed_call, Ordering::SeqCst);
            assert_eq!(
                handle.sync_now(seed.library_id),
                SyncRuntimeWakeResult::Queued
            );
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Offline
            );
            fixture.proxy.fail_feed_call.store(0, Ordering::SeqCst);
            let wakes = handle.network_available();
            assert_eq!(wakes.len(), 1);
            assert!(wakes[0].1.was_accepted());
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Idle
            );

            handle
                .shutdown()
                .await
                .expect("network runtime must shut down");
            drop(runtime);
            fixture.cleanup().await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_signal_manual_active_cycle_coalesces_follow_up() {
            let fixture = LiveFixture::new("signal-manual-active").await;
            let seed = fixture.create_library("signal-manual-active").await;
            let first = fixture.create_directory(&seed, "manual-active-first").await;
            let _second = fixture
                .create_directory(&seed, "manual-active-second")
                .await;
            let _third = fixture.create_directory(&seed, "manual-active-third").await;
            fixture.checkpoint(&seed).await;
            let client = fixture.client_for(&seed, "signal-manual-active").await;
            client.seed_stale_state(Some(&first)).await;
            fixture.compact(&seed).await;

            fixture.proxy.hold_next_page.store(true, Ordering::SeqCst);
            let page_started = fixture.proxy.page_started.notified();
            let (runtime, handle, mut events) = start_runtime(&fixture, &client).await;
            tokio::time::timeout(Duration::from_secs(15), page_started)
                .await
                .expect("a live recovery page must enter the held cycle");

            for _ in 0..100 {
                assert_eq!(
                    handle.sync_now(seed.library_id),
                    SyncRuntimeWakeResult::AlreadyRunningFollowupRecorded
                );
            }
            fixture.proxy.release_page.notify_one();
            let outcomes = wait_for_cycle_count(&mut events, seed.library_id, 2).await;
            assert!(outcomes.len() >= 2);

            handle
                .shutdown()
                .await
                .expect("active manual runtime must shut down");
            drop(runtime);
            fixture.cleanup().await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_signal_restart_after_lost_wake_recovers() {
            let fixture = LiveFixture::new("signal-restart").await;
            let seed = fixture.create_library("signal-restart").await;
            let client = fixture.client_for(&seed, "signal-restart").await;
            client.seed_ready_state(&[]).await;
            let intent = new_directory_intent(&fixture, &client, "restart-after-wake-loss").await;
            let producer = OutboundIntentProducer::new(
                client.state.clone(),
                Arc::new(DroppingNotifier) as Arc<dyn SyncWakeNotifier>,
            );
            let result = producer
                .upsert(&intent)
                .await
                .expect("durable producer must persist before the simulated crash boundary");
            assert_eq!(
                result.notification().wake_result(),
                Some(SyncRuntimeWakeResult::RuntimeStopped)
            );
            assert_eq!(fixture.proxy.mutation_requests(), 0);

            let (runtime, handle, mut events) = start_runtime(&fixture, &client).await;
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Progress
            );
            assert_eq!(fixture.proxy.mutation_requests(), 1);
            handle
                .shutdown()
                .await
                .expect("restart recovery runtime must shut down");
            drop(runtime);
            fixture.cleanup().await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_signal_multi_library_sources_remain_isolated() {
            let fixture = LiveFixture::new("signal-multi-library").await;
            let first_seed = fixture.create_library("signal-multi-first").await;
            let second_seed = fixture.create_library("signal-multi-second").await;
            let third_seed = fixture.create_library("signal-multi-third").await;
            let first_client = fixture.client_for(&first_seed, "signal-multi-first").await;
            let second_client = fixture
                .client_for(&second_seed, "signal-multi-second")
                .await;
            let third_client = fixture.client_for(&third_seed, "signal-multi-third").await;
            first_client.seed_ready_state(&[]).await;
            second_client.seed_ready_state(&[]).await;
            third_client.seed_ready_state(&[]).await;

            let first_cycle = cycle_for(&fixture, &first_client).await;
            let second_cycle = cycle_for(&fixture, &second_client).await;
            let third_cycle = cycle_for(&fixture, &third_client).await;
            let runtime = SyncRuntime::new(runtime_config());
            runtime
                .register_library(first_cycle)
                .expect("first library must register");
            runtime
                .register_library(second_cycle)
                .expect("second library must register");
            runtime
                .register_library(third_cycle)
                .expect("third library must register");
            let mut events = runtime.events();
            let handle = runtime.start().expect("multi-library runtime must start");
            wait_for_libraries(
                &mut events,
                &[
                    first_seed.library_id,
                    second_seed.library_id,
                    third_seed.library_id,
                ],
            )
            .await;

            let first_intent = new_directory_intent(&fixture, &first_client, "source-local").await;
            let producer = OutboundIntentProducer::new(
                first_client.state.clone(),
                Arc::new(runtime.clone()) as Arc<dyn SyncWakeNotifier>,
            );
            assert!(
                producer
                    .upsert(&first_intent)
                    .await
                    .expect("first local producer must persist")
                    .notification()
                    .wake_was_accepted()
            );
            let second_remote = fixture
                .create_directory(&second_seed, "source-manual")
                .await;
            let third_remote = fixture
                .create_directory(&third_seed, "source-network")
                .await;
            assert_eq!(
                handle.sync_now(second_seed.library_id),
                SyncRuntimeWakeResult::Queued
            );
            let network_wakes = handle.network_available();
            assert_eq!(network_wakes.len(), 3);
            assert!(
                network_wakes
                    .iter()
                    .all(|(_, result)| result.was_accepted())
            );

            tokio::time::timeout(Duration::from_secs(30), async {
                loop {
                    let second_ready = second_client
                        .state
                        .local_node(second_seed.library_id, second_remote.id())
                        .await
                        .expect("second local state must be readable")
                        .is_some();
                    let third_ready = third_client
                        .state
                        .local_node(third_seed.library_id, third_remote.id())
                        .await
                        .expect("third local state must be readable")
                        .is_some();
                    if fixture.proxy.mutation_requests() >= 1 && second_ready && third_ready {
                        break;
                    }
                    let _ = events.recv().await;
                }
            })
            .await
            .expect("all signal sources must make bounded progress");
            assert_eq!(fixture.proxy.mutation_requests(), 1);

            handle
                .shutdown()
                .await
                .expect("multi-library runtime must shut down");
            fixture.cleanup().await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_signal_manual_sync_now_schedules_prompt91() {
            let fixture = LiveFixture::new("signal-manual").await;
            let seed = fixture.create_library("signal-manual").await;
            let client = fixture.client_for(&seed, "signal-manual").await;
            client.seed_ready_state(&[]).await;
            let (runtime, handle, mut events) = start_runtime(&fixture, &client).await;
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Idle
            );

            let remote_node = fixture.create_directory(&seed, "manual-remote").await;
            assert_eq!(
                handle.sync_now(seed.library_id),
                SyncRuntimeWakeResult::Queued
            );
            assert_eq!(
                wait_for_cycle(&mut events, seed.library_id).await,
                SyncRuntimeOutcome::Progress
            );
            assert!(
                client
                    .state
                    .local_node(seed.library_id, remote_node.id())
                    .await
                    .expect("manual local state must be readable")
                    .is_some()
            );

            handle
                .shutdown()
                .await
                .expect("manual runtime must shut down");
            drop(runtime);
            fixture.cleanup().await;
        }
    }
}
