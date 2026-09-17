//! Prompt 92 live PostgreSQL 17 runtime acceptance target.
//!
//! The fixture is the existing Prompt 87 real-server path. The test below
//! adds the Prompt 92 scheduler on top of the real Prompt 91 cycle, so the
//! central operation remains PostgreSQL -> Axum -> authenticated device ->
//! `HttpSyncRemote` -> SQLite -> `BidirectionalSyncCycleRunner`.
//!
//! Run with:
//!
//! ```text
//! SYNVEIL_TEST_DATABASE_URL=postgresql://... cargo test -p synveil-api \
//!   --test sync_runtime_postgres --locked -- --ignored live_pg17_runtime_
//! ```

mod prompt92_fixture {
    include!("rebaseline_convergence_postgres.rs");

    mod runtime_tests {
        use std::{
            fs,
            sync::{Arc, atomic::Ordering},
            time::Duration,
        };

        use synveil_client_sync::{
            BidirectionalSyncCycleRunner, LocalFingerprint, LocalReplica, ManagedRelativePath,
            OutboundIntent, OutboundIntentKind, OutboundSubmissionEngine,
            RebaselineConvergenceCoordinator, RemoteMutationConflict, SyncRuntime,
            SyncRuntimeConfig, SyncRuntimeEvent, SyncRuntimeOutcome, SyncRuntimeWakeReason,
        };
        use synveil_core::{NodeState, Revision, SyncConflictId};
        use synveil_metadata::SyncRetentionService;

        use super::{ClientLibrary, LiveFixture, create_and_apply_snapshot, now_plus};

        async fn local_create_intent(
            client: &ClientLibrary,
            name_value: &str,
            base_epoch: synveil_core::Sequence,
            base_sequence: synveil_core::Sequence,
        ) -> OutboundIntent {
            let relative = ManagedRelativePath::new(name_value).expect("local name must be valid");
            fs::create_dir(client.replica.root_path().join(relative.as_path()))
                .expect("local directory intent must have a visible directory");
            let intent = OutboundIntent::new(
                client.seed.library_id,
                None,
                Some(client.seed.root.id()),
                OutboundIntentKind::CreateDirectory,
                relative,
                None,
                Some(LocalFingerprint::directory()),
                base_epoch,
                base_sequence,
                None,
                None,
                Some(root_revision()),
            )
            .expect("local create intent must be valid");
            client
                .state
                .upsert_outbound_intent(&intent)
                .await
                .expect("local create intent must persist")
        }

        fn root_revision() -> synveil_core::Revision {
            synveil_core::Revision::new(0)
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

        async fn start_runtime(
            cycle: Arc<BidirectionalSyncCycleRunner>,
        ) -> (
            SyncRuntime,
            synveil_client_sync::SyncRuntimeHandle,
            tokio::sync::broadcast::Receiver<SyncRuntimeEvent>,
        ) {
            let runtime = SyncRuntime::new(runtime_config());
            runtime
                .register_library(cycle)
                .expect("runtime registration must succeed");
            let events = runtime.events();
            let handle = runtime.start().expect("runtime startup must succeed");
            (runtime, handle, events)
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

        fn runtime_config() -> SyncRuntimeConfig {
            SyncRuntimeConfig::new(
                Duration::from_secs(30),
                Duration::from_secs(1),
                Duration::from_secs(60),
                Duration::from_secs(30),
                2,
            )
            .expect("Prompt 92 live config must validate")
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_runtime_startup_local_wake_shutdown_and_restart() {
            let fixture = LiveFixture::new("runtime-startup").await;
            let seed = fixture.create_library("runtime-library").await;
            let client = fixture.client_for(&seed, "runtime-library").await;
            client.seed_ready_state(&[]).await;
            let remote_node = fixture.create_directory(&seed, "remote-startup").await;
            let (epoch, head, _) = fixture.server_head(&seed).await;
            let first_intent = local_create_intent(&client, "local-startup", epoch, head).await;
            let cycle = cycle_for(&fixture, &client).await;
            let library_id = seed.library_id;
            let (_runtime, handle, mut events) = start_runtime(cycle.clone()).await;
            assert_eq!(
                wait_for_cycle(&mut events, library_id).await,
                SyncRuntimeOutcome::Progress
            );
            assert!(
                client
                    .state
                    .local_node(seed.library_id, remote_node.id())
                    .await
                    .expect("remote node must be readable")
                    .is_some()
            );
            assert_eq!(fixture.server_node_count(&seed).await, 3);
            assert_eq!(fixture.proxy.mutation_requests(), 1);

            handle
                .shutdown()
                .await
                .expect("startup runtime must drain and join before reseeding");

            let wake_seed = fixture.create_library("runtime-wake-library").await;
            let wake_client = fixture.client_for(&wake_seed, "runtime-wake-library").await;
            wake_client.seed_ready_state(&[]).await;
            let wake_cycle = cycle_for(&fixture, &wake_client).await;
            let wake_library_id = wake_seed.library_id;
            let (_wake_runtime, wake_handle, mut wake_events) =
                start_runtime(wake_cycle.clone()).await;
            assert_eq!(
                wait_for_cycle(&mut wake_events, wake_library_id).await,
                SyncRuntimeOutcome::Idle
            );
            let (epoch, head, _) = fixture.server_head(&wake_seed).await;
            let second_intent = local_create_intent(&wake_client, "local-wake", epoch, head).await;
            wake_handle
                .wake_library(wake_library_id, SyncRuntimeWakeReason::LocalChange)
                .expect("local wake must be accepted");
            assert_eq!(
                wait_for_cycle(&mut wake_events, wake_library_id).await,
                SyncRuntimeOutcome::Progress
            );
            assert_ne!(first_intent.intent_id(), second_intent.intent_id());
            assert_eq!(fixture.proxy.mutation_requests(), 2);

            wake_handle
                .shutdown()
                .await
                .expect("runtime shutdown must drain and join");
            wake_handle
                .shutdown()
                .await
                .expect("repeated runtime shutdown must be safe");

            let restart_seed = fixture.create_library("runtime-restart-library").await;
            let restart_client = fixture
                .client_for(&restart_seed, "runtime-restart-library")
                .await;
            restart_client.seed_ready_state(&[]).await;
            let restart_cycle = cycle_for(&fixture, &restart_client).await;
            let stopped = SyncRuntime::new(runtime_config());
            stopped
                .register_library(restart_cycle.clone())
                .expect("pre-restart registration must succeed");
            let stopped_handle = stopped.start().expect("pre-restart runtime must start");
            stopped_handle
                .shutdown()
                .await
                .expect("pre-restart runtime must stop cleanly");

            let restarted = SyncRuntime::new(runtime_config());
            restarted
                .register_library(restart_cycle)
                .expect("restart registration must succeed");
            let mut restart_events = restarted.events();
            let restart_handle = restarted.start().expect("runtime restart must succeed");
            assert_eq!(
                wait_for_cycle(&mut restart_events, restart_seed.library_id).await,
                SyncRuntimeOutcome::Idle
            );
            restart_handle
                .shutdown()
                .await
                .expect("restarted runtime must shut down");

            fixture.cleanup().await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_runtime_offline_auth_conflict_and_wake_storm() {
            let fixture = LiveFixture::new("runtime-policy").await;

            let offline_seed = fixture.create_library("runtime-offline").await;
            let offline_client = fixture.client_for(&offline_seed, "runtime-offline").await;
            offline_client.seed_ready_state(&[]).await;
            fixture.proxy.fail_feed_call.store(1, Ordering::SeqCst);
            let offline_cycle = cycle_for(&fixture, &offline_client).await;
            let (_offline_runtime, offline_handle, mut offline_events) =
                start_runtime(offline_cycle).await;
            assert_eq!(
                wait_for_cycle(&mut offline_events, offline_seed.library_id).await,
                SyncRuntimeOutcome::Offline
            );
            fixture.proxy.fail_feed_call.store(0, Ordering::SeqCst);
            offline_handle
                .wake_library(
                    offline_seed.library_id,
                    SyncRuntimeWakeReason::NetworkAvailable,
                )
                .expect("network wake must be accepted");
            assert_eq!(
                wait_for_cycle(&mut offline_events, offline_seed.library_id).await,
                SyncRuntimeOutcome::Idle
            );
            offline_handle.shutdown().await.expect("offline shutdown");

            let auth_seed = fixture.create_library("runtime-auth").await;
            let auth_client = fixture.client_for(&auth_seed, "runtime-auth").await;
            auth_client.seed_ready_state(&[]).await;
            sqlx::query("UPDATE devices SET status = 'PAUSED' WHERE id = $1")
                .bind(fixture.device_id.into_uuid())
                .execute(&fixture.inspection)
                .await
                .expect("fixture device pause must persist");
            let auth_cycle = cycle_for(&fixture, &auth_client).await;
            let (_auth_runtime, auth_handle, mut auth_events) = start_runtime(auth_cycle).await;
            assert_eq!(
                wait_for_cycle(&mut auth_events, auth_seed.library_id).await,
                SyncRuntimeOutcome::AuthBlocked
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
            let requests_after_auth_block = fixture.proxy.feed_requests.load(Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert_eq!(
                fixture.proxy.feed_requests.load(Ordering::SeqCst),
                requests_after_auth_block
            );
            sqlx::query("UPDATE devices SET status = 'ACTIVE' WHERE id = $1")
                .bind(fixture.device_id.into_uuid())
                .execute(&fixture.inspection)
                .await
                .expect("fixture device must reactivate");
            auth_handle
                .wake_library(
                    auth_seed.library_id,
                    SyncRuntimeWakeReason::CredentialChanged,
                )
                .expect("credential wake must be accepted");
            assert_eq!(
                wait_for_cycle(&mut auth_events, auth_seed.library_id).await,
                SyncRuntimeOutcome::Idle
            );
            auth_handle.shutdown().await.expect("auth shutdown");

            let conflict_seed = fixture.create_library("runtime-conflict").await;
            let conflict_client = fixture.client_for(&conflict_seed, "runtime-conflict").await;
            conflict_client.seed_ready_state(&[]).await;
            let (epoch, head, _) = fixture.server_head(&conflict_seed).await;
            let intent = local_create_intent(&conflict_client, "conflict-local", epoch, head).await;
            let conflict = RemoteMutationConflict::with_evidence(
                SyncConflictId::new(),
                "NAME_COLLISION",
                false,
                conflict_seed.root.id(),
                Some(root_revision()),
                Some(Revision::new(2)),
                Some(NodeState::Active),
                None,
                epoch,
                head,
            )
            .expect("durable conflict evidence must be valid");
            fixture
                .local
                .record_mutation_conflict(
                    intent.intent_id(),
                    synveil_core::ClientMutationId::new(),
                    &conflict,
                )
                .await
                .expect("existing conflict must persist");
            let inbound = fixture
                .create_directory(&conflict_seed, "conflict-inbound")
                .await;
            let conflict_cycle = cycle_for(&fixture, &conflict_client).await;
            let (_conflict_runtime, conflict_handle, mut conflict_events) =
                start_runtime(conflict_cycle).await;
            assert_eq!(
                wait_for_cycle(&mut conflict_events, conflict_seed.library_id).await,
                SyncRuntimeOutcome::ConflictBlocked
            );
            assert!(
                conflict_client
                    .state
                    .local_node(conflict_seed.library_id, inbound.id())
                    .await
                    .expect("inbound node must be readable")
                    .is_some()
            );
            conflict_handle.shutdown().await.expect("conflict shutdown");

            let storm_seed = fixture.create_library("runtime-storm").await;
            let storm_client = fixture.client_for(&storm_seed, "runtime-storm").await;
            storm_client.seed_ready_state(&[]).await;
            let storm_cycle = cycle_for(&fixture, &storm_client).await;
            let (_storm_runtime, storm_handle, mut storm_events) = start_runtime(storm_cycle).await;
            assert_eq!(
                wait_for_cycle(&mut storm_events, storm_seed.library_id).await,
                SyncRuntimeOutcome::Idle
            );
            let (epoch, head, _) = fixture.server_head(&storm_seed).await;
            let _storm_intent =
                local_create_intent(&storm_client, "storm-local", epoch, head).await;
            let before = fixture.proxy.mutation_requests();
            for _ in 0..100 {
                storm_handle
                    .wake_library(storm_seed.library_id, SyncRuntimeWakeReason::LocalChange)
                    .expect("storm wake must be accepted");
            }
            assert_eq!(
                wait_for_cycle(&mut storm_events, storm_seed.library_id).await,
                SyncRuntimeOutcome::Progress
            );
            assert_eq!(fixture.proxy.mutation_requests(), before + 1);
            storm_handle.shutdown().await.expect("storm shutdown");

            fixture.cleanup().await;
        }

        #[tokio::test]
        #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
        async fn live_pg17_runtime_retention_proof_loss_and_multi_library_fairness() {
            let fixture = LiveFixture::new("runtime-recovery-fairness").await;

            let retention_seed = fixture.create_library("runtime-retention").await;
            let first = fixture.create_directory(&retention_seed, "first").await;
            let _second = fixture.create_directory(&retention_seed, "second").await;
            let _third = fixture.create_directory(&retention_seed, "third").await;
            fixture.checkpoint(&retention_seed).await;
            let retention_client = fixture
                .client_for(&retention_seed, "runtime-retention")
                .await;
            retention_client.seed_stale_state(Some(&first)).await;
            fixture.compact(&retention_seed).await;
            let retention_cycle = cycle_for(&fixture, &retention_client).await;
            let (_retention_runtime, retention_handle, mut retention_events) =
                start_runtime(retention_cycle).await;
            assert_eq!(
                wait_for_cycle(&mut retention_events, retention_seed.library_id).await,
                SyncRuntimeOutcome::Progress
            );
            assert_eq!(fixture.sqlite_counts(&retention_seed).await, (0, 0));
            retention_handle
                .shutdown()
                .await
                .expect("retention shutdown");

            let proof_seed = fixture.create_library("runtime-proof-loss").await;
            let proof_first = fixture.create_directory(&proof_seed, "proof-first").await;
            let _proof_second = fixture.create_directory(&proof_seed, "proof-second").await;
            let _proof_third = fixture.create_directory(&proof_seed, "proof-third").await;
            fixture.checkpoint(&proof_seed).await;
            let proof_client = fixture.client_for(&proof_seed, "runtime-proof-loss").await;
            proof_client.seed_stale_state(Some(&proof_first)).await;
            let s1 = create_and_apply_snapshot(&fixture, &proof_client).await;
            let retention = SyncRetentionService::new(fixture.pool.as_ref().clone());
            retention
                .cleanup_snapshot_payloads_step(now_plus(31))
                .await
                .expect("S1 payload cleanup must succeed");
            retention
                .cleanup_handoff_proofs_step(now_plus(31))
                .await
                .expect("S1 proof cleanup must succeed");
            proof_client.seed_stale_state(Some(&proof_first)).await;
            let between = fixture.create_directory(&proof_seed, "proof-between").await;
            let ahead = fixture.advance_checkpoint(&proof_seed).await;
            assert!(ahead.acknowledged_sequence() > s1.boundary().resume_sequence());
            let proof_cycle = cycle_for(&fixture, &proof_client).await;
            let (_proof_runtime, proof_handle, mut proof_events) = start_runtime(proof_cycle).await;
            assert_eq!(
                wait_for_cycle(&mut proof_events, proof_seed.library_id).await,
                SyncRuntimeOutcome::Progress
            );
            assert!(
                proof_client
                    .state
                    .local_node(proof_seed.library_id, between.id())
                    .await
                    .expect("proof-loss inbound must be readable")
                    .is_some()
            );
            assert_eq!(fixture.sqlite_counts(&proof_seed).await, (0, 0));
            proof_handle.shutdown().await.expect("proof shutdown");

            let first_seed = fixture.create_library("runtime-fair-a").await;
            let second_seed = fixture.create_library("runtime-fair-b").await;
            let third_seed = fixture.create_library("runtime-fair-c").await;
            let first_client = fixture.client_for(&first_seed, "runtime-fair-a").await;
            let second_client = fixture.client_for(&second_seed, "runtime-fair-b").await;
            let third_client = fixture.client_for(&third_seed, "runtime-fair-c").await;
            first_client.seed_ready_state(&[]).await;
            second_client.seed_ready_state(&[]).await;
            third_client.seed_ready_state(&[]).await;
            let first_cycle = cycle_for(&fixture, &first_client).await;
            let second_cycle = cycle_for(&fixture, &second_client).await;
            let third_cycle = cycle_for(&fixture, &third_client).await;
            let runtime = SyncRuntime::new(runtime_config());
            runtime
                .register_library(first_cycle)
                .expect("first register");
            runtime
                .register_library(second_cycle)
                .expect("second register");
            runtime
                .register_library(third_cycle)
                .expect("third register");
            let mut events = runtime.events();
            let handle = runtime.start().expect("fair runtime start");
            let mut seen = std::collections::BTreeSet::new();
            for _ in 0..12 {
                if let SyncRuntimeEvent::CycleFinished { library_id, .. } =
                    tokio::time::timeout(Duration::from_secs(15), events.recv())
                        .await
                        .expect("fairness event timeout")
                        .expect("fairness event stream")
                {
                    seen.insert(library_id);
                }
                if seen.len() == 3 {
                    break;
                }
            }
            assert_eq!(seen.len(), 3);
            handle.shutdown().await.expect("fair shutdown");

            fixture.cleanup().await;
        }
    }
}
