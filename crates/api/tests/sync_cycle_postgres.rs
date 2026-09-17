//! Prompt 91 live PostgreSQL 17 acceptance target.
//!
//! The existing Prompt 87 live fixture is included so these tests exercise
//! the same real PostgreSQL -> Axum -> authenticated device -> `HttpSyncRemote`
//! -> SQLite path. The test filter used by CI selects only the eight
//! `live_pg17_prompt91_*` tests below; retaining the fixture's helpers here
//! keeps the production cycle target transport-neutral and avoids a second
//! live-server implementation.
//!
//! Run with:
//!
//! ```text
//! SYNVEIL_TEST_DATABASE_URL=postgresql://... cargo test -p synveil-api \
//!   --test sync_cycle_postgres --locked -- --ignored live_pg17_prompt91_
//! ```

mod prompt87_fixture {
    include!("rebaseline_convergence_postgres.rs");

    use synveil_client_sync::{
        BidirectionalSyncCycleRunner, InboundCycleOutcome, OutboundCycleOutcome,
        OutboundSkipReason, OutboundSubmissionEngine, OutboundSubmissionOutcome,
        RemoteMutationConflict,
    };

    async fn cycle_for(
        fixture: &LiveFixture,
        client: &ClientLibrary,
    ) -> std::sync::Arc<BidirectionalSyncCycleRunner> {
        let convergence = std::sync::Arc::new(
            RebaselineConvergenceCoordinator::new(
                client.engine.clone(),
                fixture.remote.clone(),
                fixture.local.clone(),
                2,
            )
            .expect("Prompt 87 convergence coordinator must build"),
        );
        let outbound = std::sync::Arc::new(
            OutboundSubmissionEngine::new(
                client.engine.scope(),
                fixture.remote.clone(),
                client.replica.clone(),
                fixture.local.clone(),
            )
            .await
            .expect("Prompt 88 outbound engine must build"),
        );
        std::sync::Arc::new(
            BidirectionalSyncCycleRunner::new(convergence, outbound)
                .expect("Prompt 91 cycle runner must compose"),
        )
    }

    async fn local_create_intent(
        client: &ClientLibrary,
        name_value: &str,
        base_epoch: Sequence,
        base_sequence: Sequence,
        parent_revision: Revision,
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
            Some(parent_revision),
        )
        .expect("local create intent must be valid");
        client
            .state
            .upsert_outbound_intent(&intent)
            .await
            .expect("local create intent must persist")
    }

    async fn local_rename_intent(
        client: &ClientLibrary,
        node: &Node,
        old_name: &str,
        new_name: &str,
        base_sequence: Sequence,
    ) -> OutboundIntent {
        fs::rename(
            client.replica.root_path().join(old_name),
            client.replica.root_path().join(new_name),
        )
        .expect("local rename must be visible");
        let root = client
            .state
            .local_node(client.seed.library_id, client.seed.root.id())
            .await
            .expect("local root must be readable")
            .expect("local root must exist");
        let intent = OutboundIntent::new(
            client.seed.library_id,
            Some(node.id()),
            Some(client.seed.root.id()),
            OutboundIntentKind::RenameNode,
            ManagedRelativePath::new(new_name).expect("new name must be valid"),
            Some(ManagedRelativePath::new(old_name).expect("old name must be valid")),
            Some(LocalFingerprint::directory()),
            Sequence::new(1),
            base_sequence,
            Some(node.revision()),
            None,
            Some(root.revision()),
        )
        .expect("local rename intent must be valid");
        client
            .state
            .upsert_outbound_intent(&intent)
            .await
            .expect("local rename intent must persist")
    }

    fn root_revision() -> Revision {
        Revision::new(0)
    }

    #[tokio::test]
    #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
    async fn live_pg17_prompt91_healthy_bidirectional_cycle() {
        let fixture = LiveFixture::new("cycle-healthy").await;
        let seed = fixture.create_library("cycle-healthy").await;
        let client = fixture.client_for(&seed, "cycle-healthy").await;
        client.seed_ready_state(&[]).await;
        let remote_node = fixture.create_directory(&seed, "remote").await;
        let (epoch, head, _) = fixture.server_head(&seed).await;
        let intent = local_create_intent(&client, "local", epoch, head, root_revision()).await;
        fixture.proxy.reset_counts();

        let cycle = cycle_for(&fixture, &client).await;
        let result = cycle
            .run_once(fixture_timestamp())
            .await
            .expect("healthy bidirectional cycle must complete");
        assert!(matches!(
            result.inbound(),
            InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::IncrementalProgress(
                SyncOutcome::Progressed
            ))
        ));
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Submitted(
                intent.intent_id()
            ))
        );
        assert!(
            client
                .state
                .local_node(seed.library_id, remote_node.id())
                .await
                .expect("remote inbound node must be readable")
                .is_some()
        );
        assert_eq!(fixture.server_node_count(&seed).await, 3);
        assert_eq!(fixture.proxy.feed_requests.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.proxy.mutation_requests(), 1);
        assert_eq!(fixture.proxy.snapshot_posts(), 0);
        assert_eq!(fixture.proxy.handoffs(), 0);

        drop(cycle);
        drop(client);
        fixture.cleanup().await;
    }

    #[tokio::test]
    #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
    async fn live_pg17_prompt91_inbound_conflict_is_fenced_before_submission() {
        let fixture = LiveFixture::new("cycle-inbound-conflict").await;
        let seed = fixture.create_library("cycle-inbound-conflict").await;
        let target = fixture.create_directory(&seed, "target").await;
        let checkpoint = fixture.advance_checkpoint(&seed).await;
        let client = fixture.client_for(&seed, "cycle-inbound-conflict").await;
        client
            .seed_ready_state_at(&[&target], checkpoint.acknowledged_sequence())
            .await;
        let initial = client
            .state
            .local_node(seed.library_id, target.id())
            .await
            .expect("target must be readable")
            .expect("target must be present locally");
        let (epoch, base_sequence, _) = fixture.server_head(&seed).await;
        let intent =
            local_rename_intent(&client, &target, "target", "local-target", base_sequence).await;
        assert_eq!(initial.revision(), target.revision());
        FileMetadataService::new(fixture.pool.as_ref().clone())
            .rename_node(
                fixture.owner_id,
                target.id(),
                name("server-target"),
                target.revision(),
            )
            .await
            .expect("remote conflicting rename must persist");
        let (_, head, _) = fixture.server_head(&seed).await;
        assert!(head > base_sequence);
        fixture.proxy.reset_counts();

        let cycle = cycle_for(&fixture, &client).await;
        let result = cycle
            .run_once(fixture_timestamp())
            .await
            .expect("inbound conflict fence must be a typed cycle result");
        assert!(
            matches!(
                result.inbound(),
                InboundCycleOutcome::Converged(RebaselineConvergenceOutcome::IncrementalProgress(
                    SyncOutcome::Blocked
                ))
            ),
            "inbound={:?} outbound={:?} checkpoint={} feed={} snapshots={} pages={} handoffs={}",
            result.inbound(),
            result.outbound(),
            fixture.proxy.checkpoint_requests.load(Ordering::SeqCst),
            fixture.proxy.feed_requests.load(Ordering::SeqCst),
            fixture.proxy.snapshot_posts(),
            fixture.proxy.snapshot_pages(),
            fixture.proxy.handoffs()
        );
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::NotAttempted(OutboundSkipReason::IncrementalNotReady(
                SyncOutcome::Blocked
            ))
        );
        assert_eq!(fixture.proxy.mutation_requests(), 0);
        assert_eq!(
            client
                .state
                .outbound_intent(intent.intent_id())
                .await
                .expect("fenced intent must be readable")
                .expect("fenced intent must remain durable")
                .state(),
            synveil_client_sync::OutboundIntentState::NeedsRebaseValidation
        );
        assert!(
            client
                .state
                .list_unresolved_conflicts(seed.library_id, None, None)
                .await
                .expect("conflict ledger must be readable")
                .items()
                .is_empty(),
            "ordinary inbound uses the existing observation fence; Prompt 88
             classification remains owned by its canonical rebaseline path"
        );
        assert_eq!(epoch, Sequence::new(1));

        drop(cycle);
        drop(client);
        fixture.cleanup().await;
    }

    #[tokio::test]
    #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
    async fn live_pg17_prompt91_retention_recovery_then_one_outbound_step() {
        let fixture = LiveFixture::new("cycle-retention").await;
        let seed = fixture.create_library("cycle-retention").await;
        let first = fixture.create_directory(&seed, "first").await;
        let _second = fixture.create_directory(&seed, "second").await;
        let _third = fixture.create_directory(&seed, "third").await;
        fixture.checkpoint(&seed).await;
        let client = fixture.client_for(&seed, "cycle-retention").await;
        client.seed_stale_state(Some(&first)).await;
        fixture.compact(&seed).await;
        let (epoch, head, floor) = fixture.server_head(&seed).await;
        assert_eq!(floor, head);
        let intent =
            local_create_intent(&client, "after-recovery", epoch, head, root_revision()).await;
        fixture.proxy.reset_counts();

        let cycle = cycle_for(&fixture, &client).await;
        let result = cycle
            .run_once(fixture_timestamp())
            .await
            .expect("retention recovery cycle must complete");
        assert!(matches!(
            result.inbound(),
            InboundCycleOutcome::Converged(
                RebaselineConvergenceOutcome::RebaselineConverged { .. }
            )
        ));
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Submitted(
                intent.intent_id()
            ))
        );
        assert_eq!(fixture.proxy.snapshot_posts(), 1);
        assert_eq!(fixture.proxy.handoffs(), 1);
        assert_eq!(fixture.proxy.mutation_requests(), 1);
        assert_eq!(fixture.sqlite_counts(&seed).await, (0, 0));
        assert_eq!(fixture.server_node_count(&seed).await, 5);

        drop(cycle);
        drop(client);
        fixture.cleanup().await;
    }

    #[tokio::test]
    #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
    async fn live_pg17_prompt91_proof_loss_recovery_preserves_outbound_work() {
        let fixture = LiveFixture::new("cycle-proof-loss").await;
        let seed = fixture.create_library("cycle-proof-loss").await;
        let first = fixture.create_directory(&seed, "first").await;
        let _second = fixture.create_directory(&seed, "second").await;
        let _third = fixture.create_directory(&seed, "third").await;
        fixture.checkpoint(&seed).await;
        let client = fixture.client_for(&seed, "cycle-proof-loss").await;

        let s1 = create_and_apply_snapshot(&fixture, &client).await;
        let retention = SyncRetentionService::new(fixture.pool.as_ref().clone());
        retention
            .cleanup_snapshot_payloads_step(now_plus(31))
            .await
            .expect("S1 payload cleanup must succeed");
        retention
            .cleanup_handoff_proofs_step(now_plus(31))
            .await
            .expect("S1 proof cleanup must succeed");
        assert_eq!(fixture.sqlite_counts(&seed).await, (0, 1));
        client.seed_stale_state(Some(&first)).await;
        let between = fixture.create_directory(&seed, "between").await;
        let ahead = fixture.advance_checkpoint(&seed).await;
        assert!(ahead.acknowledged_sequence() > s1.boundary().resume_sequence());
        let intent = local_create_intent(
            &client,
            "after-proof-loss",
            ahead.journal_epoch(),
            ahead.acknowledged_sequence(),
            root_revision(),
        )
        .await;
        fixture.proxy.reset_counts();

        let cycle = cycle_for(&fixture, &client).await;
        let result = cycle
            .run_once(fixture_timestamp())
            .await
            .expect("proof-loss recovery cycle must complete");
        assert!(matches!(
            result.inbound(),
            InboundCycleOutcome::Converged(
                RebaselineConvergenceOutcome::RebaselineConverged { .. }
            )
        ));
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Submitted(
                intent.intent_id()
            ))
        );
        assert!(
            client
                .state
                .local_node(seed.library_id, between.id())
                .await
                .expect("post-proof-loss inbound node must be readable")
                .is_some()
        );
        assert_eq!(fixture.proxy.snapshot_posts(), 1);
        assert_eq!(fixture.proxy.handoffs(), 2);
        assert_eq!(fixture.proxy.mutation_requests(), 1);
        assert_eq!(fixture.sqlite_counts(&seed).await, (0, 0));

        drop(cycle);
        drop(client);
        fixture.cleanup().await;
    }

    #[tokio::test]
    #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
    async fn live_pg17_prompt91_existing_conflict_allows_new_inbound_and_fences_outbound() {
        let fixture = LiveFixture::new("cycle-existing-conflict").await;
        let seed = fixture.create_library("cycle-existing-conflict").await;
        let client = fixture.client_for(&seed, "cycle-existing-conflict").await;
        client.seed_ready_state(&[]).await;
        let (epoch, head, _) = fixture.server_head(&seed).await;
        let intent = local_create_intent(&client, "local", epoch, head, root_revision()).await;
        let conflict = RemoteMutationConflict::with_evidence(
            synveil_core::SyncConflictId::new(),
            "NAME_COLLISION",
            false,
            seed.root.id(),
            Some(root_revision()),
            Some(Revision::new(2)),
            Some(synveil_core::NodeState::Active),
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
        let inbound_node = fixture.create_directory(&seed, "remote").await;
        fixture.proxy.reset_counts();

        let cycle = cycle_for(&fixture, &client).await;
        let result = cycle
            .run_once(fixture_timestamp())
            .await
            .expect("existing-conflict cycle must complete");
        assert!(result.inbound().made_durable_progress());
        assert!(matches!(
            result.outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::BlockedByConflict(_))
        ));
        assert!(
            client
                .state
                .local_node(seed.library_id, inbound_node.id())
                .await
                .expect("new inbound node must be readable")
                .is_some()
        );
        assert_eq!(fixture.proxy.mutation_requests(), 0);
        assert_eq!(
            fixture
                .local
                .list_unresolved_conflicts(seed.library_id, None, None)
                .await
                .expect("existing conflict must remain readable")
                .items()
                .len(),
            1
        );

        drop(cycle);
        drop(client);
        fixture.cleanup().await;
    }

    #[tokio::test]
    #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
    async fn live_pg17_prompt91_revoked_device_skips_outbound_http() {
        let fixture = LiveFixture::new("cycle-revoked").await;
        let seed = fixture.create_library("cycle-revoked").await;
        let client = fixture.client_for(&seed, "cycle-revoked").await;
        client.seed_ready_state(&[]).await;
        let (epoch, head, _) = fixture.server_head(&seed).await;
        let intent = local_create_intent(&client, "retained", epoch, head, root_revision()).await;
        let checkpoint_before = fixture.checkpoint(&seed).await;
        synveil_auth::DeviceAuthenticationService::new(fixture.pool.as_ref())
            .revoke_device(fixture.owner_id, fixture.device_id)
            .await
            .expect("device revocation must persist");
        fixture.proxy.reset_counts();

        let cycle = cycle_for(&fixture, &client).await;
        let result = cycle
            .run_once(fixture_timestamp())
            .await
            .expect("revoked-device cycle must return typed auth state");
        assert_eq!(result.inbound(), InboundCycleOutcome::AuthRequired);
        assert_eq!(
            result.outbound(),
            OutboundCycleOutcome::NotAttempted(OutboundSkipReason::InboundAuthenticationRequired)
        );
        assert_eq!(fixture.proxy.mutation_requests(), 0);
        let local_after = client
            .state
            .replica(seed.library_id)
            .await
            .expect("local replica must remain readable")
            .expect("local replica must remain durable");
        assert_eq!(
            local_after.acknowledged_sequence(),
            checkpoint_before.acknowledged_sequence()
        );
        assert_eq!(fixture.proxy.checkpoint_requests.load(Ordering::SeqCst), 1);
        assert_eq!(
            client
                .state
                .outbound_intent(intent.intent_id())
                .await
                .expect("retained intent must be readable")
                .expect("retained intent must remain durable")
                .intent_id(),
            intent.intent_id()
        );

        drop(cycle);
        drop(client);
        fixture.cleanup().await;
    }

    #[tokio::test]
    #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
    async fn live_pg17_prompt91_same_library_callers_submit_once() {
        let fixture = LiveFixture::new("cycle-same-library").await;
        let seed = fixture.create_library("cycle-same-library").await;
        let client = fixture.client_for(&seed, "cycle-same-library").await;
        client.seed_ready_state(&[]).await;
        let (epoch, head, _) = fixture.server_head(&seed).await;
        let intent = local_create_intent(&client, "one", epoch, head, root_revision()).await;
        fixture.proxy.reset_counts();
        let cycle = cycle_for(&fixture, &client).await;
        let (first, second) = tokio::join!(
            cycle.run_once(fixture_timestamp()),
            cycle.run_once(fixture_timestamp())
        );
        let first = first.expect("first same-library cycle must complete");
        let second = second.expect("second same-library cycle must complete");
        let submitted = [first, second]
            .into_iter()
            .filter(|result| {
                result.outbound()
                    == OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Submitted(
                        intent.intent_id(),
                    ))
            })
            .count();
        assert!(submitted <= 1);
        assert_eq!(fixture.proxy.mutation_requests(), 1);
        assert_eq!(fixture.server_node_count(&seed).await, 2);
        assert_eq!(fixture.sqlite_counts(&seed).await, (0, 0));

        drop(cycle);
        drop(client);
        fixture.cleanup().await;
    }

    #[tokio::test]
    #[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
    async fn live_pg17_prompt91_two_libraries_progress_independently() {
        let fixture = LiveFixture::new("cycle-two-libraries").await;
        let first = fixture.create_library("cycle-library-a").await;
        let second = fixture.create_library("cycle-library-b").await;
        let client_a = fixture.client_for(&first, "cycle-library-a").await;
        let client_b = fixture.client_for(&second, "cycle-library-b").await;
        client_a.seed_ready_state(&[]).await;
        client_b.seed_ready_state(&[]).await;
        let (epoch_a, head_a, _) = fixture.server_head(&first).await;
        let (epoch_b, head_b, _) = fixture.server_head(&second).await;
        let intent_a = local_create_intent(&client_a, "a", epoch_a, head_a, root_revision()).await;
        let intent_b = local_create_intent(&client_b, "b", epoch_b, head_b, root_revision()).await;
        fixture.proxy.reset_counts();
        let cycle_a = cycle_for(&fixture, &client_a).await;
        let cycle_b = cycle_for(&fixture, &client_b).await;
        let (result_a, result_b) = tokio::join!(
            cycle_a.run_once(fixture_timestamp()),
            cycle_b.run_once(fixture_timestamp())
        );
        assert_eq!(
            result_a.expect("library A cycle must complete").outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Submitted(
                intent_a.intent_id()
            ))
        );
        assert_eq!(
            result_b.expect("library B cycle must complete").outbound(),
            OutboundCycleOutcome::Attempted(OutboundSubmissionOutcome::Submitted(
                intent_b.intent_id()
            ))
        );
        assert_eq!(fixture.proxy.mutation_requests(), 2);
        assert_eq!(fixture.server_node_count(&first).await, 2);
        assert_eq!(fixture.server_node_count(&second).await, 2);

        drop(cycle_a);
        drop(cycle_b);
        drop(client_a);
        drop(client_b);
        fixture.cleanup().await;
    }
}
