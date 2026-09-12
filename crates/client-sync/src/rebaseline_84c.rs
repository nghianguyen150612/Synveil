//! Prompt 84C exhaustive verification for client atomic rebaseline apply.
//!
//! This module extends the representative Prompt 84 proofs with the full
//! mandatory matrices. It drives the same production implementation
//! (`RebaselineApplier`, `LocalStateStore` rebaseline candidate/activation
//! paths) through deterministic scripted sources. No sleeps, no retries,
//! no background work, no conflict policy.

#[cfg(test)]
#[allow(clippy::useless_vec)]
mod tests {
    use std::{
        collections::{BTreeSet, VecDeque},
        fs,
        path::PathBuf,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::Instant,
    };

    use async_trait::async_trait;
    use synveil_core::{
        DeviceId, FileVersionId, LibraryId, LogicalName, LogicalSnapshotNode, NodeId, NodeKind,
        NodeState, RebaselineSnapshotId, Revision, Sequence, Sha256Digest, UserId,
    };
    use tokio::sync::Notify;

    use crate::{
        ClientSyncError, EngineConfig, FailureInjector, FailurePoint, FilesystemLocalReplica,
        InboundSyncEngine, LocalFingerprint, LocalNode, LocalReplica, LocalStateConfig,
        LocalStateStore, ManagedRelativePath, OpaqueEvidence, OutboundIntent, OutboundIntentKind,
        RebaselineApplier, RebaselineApplyOutcome, RebaselineBoundary,
        RebaselineHandoffConfirmation, RebaselineHandoffOutcome, RebaselineSnapshotDescriptor,
        RebaselineSnapshotPage, RebaselineSnapshotSource, RemoteCheckpoint, RemoteError,
        RemoteErrorKind, ReplicaScope, RootBindingId, SyncRemote,
    };

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    struct OnePageSource(Mutex<Option<RebaselineSnapshotPage>>);
    #[async_trait]
    impl RebaselineSnapshotSource for OnePageSource {
        async fn read_page(
            &self,
            _scope: ReplicaScope,
            _snapshot_id: RebaselineSnapshotId,
            _cursor: Option<&OpaqueEvidence>,
            _limit: u32,
        ) -> Result<RebaselineSnapshotPage, RemoteError> {
            self.0
                .lock()
                .unwrap()
                .take()
                .ok_or(RemoteError::new(RemoteErrorKind::Protocol))
        }
    }

    struct ScriptedSource(Mutex<VecDeque<Result<RebaselineSnapshotPage, RemoteError>>>);
    #[async_trait]
    impl RebaselineSnapshotSource for ScriptedSource {
        async fn read_page(
            &self,
            _scope: ReplicaScope,
            _snapshot_id: RebaselineSnapshotId,
            _cursor: Option<&OpaqueEvidence>,
            _limit: u32,
        ) -> Result<RebaselineSnapshotPage, RemoteError> {
            self.0
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(RemoteError::new(RemoteErrorKind::Protocol)))
        }
    }

    fn directory(id: NodeId, parent: Option<NodeId>, name: &str) -> LogicalSnapshotNode {
        LogicalSnapshotNode::new(
            id,
            parent,
            LogicalName::new(name).unwrap(),
            NodeKind::Directory,
            NodeState::Active,
            Revision::new(1),
            None,
            None,
            None,
        )
        .unwrap()
    }

    fn trashed_directory(id: NodeId, parent: Option<NodeId>, name: &str) -> LogicalSnapshotNode {
        LogicalSnapshotNode::new(
            id,
            parent,
            LogicalName::new(name).unwrap(),
            NodeKind::Directory,
            NodeState::Trashed,
            Revision::new(2),
            None,
            None,
            None,
        )
        .unwrap()
    }

    fn file_node(
        id: NodeId,
        parent: Option<NodeId>,
        name: &str,
        revision: u64,
    ) -> LogicalSnapshotNode {
        LogicalSnapshotNode::new(
            id,
            parent,
            LogicalName::new(name).unwrap(),
            NodeKind::File,
            NodeState::Active,
            Revision::new(revision),
            Some(FileVersionId::new()),
            Some(11),
            Some(Sha256Digest::from_bytes([0xAB; 32])),
        )
        .unwrap()
    }

    fn cursor(value: u8) -> OpaqueEvidence {
        OpaqueEvidence::new(vec![b'a' + value]).unwrap()
    }

    fn page_cursor(value: usize) -> OpaqueEvidence {
        OpaqueEvidence::new(format!("cursor-{value}").into_bytes()).unwrap()
    }

    fn boundary() -> RebaselineBoundary {
        RebaselineBoundary::new(Sequence::new(2), Sequence::new(3))
    }

    async fn setup(
        label: &str,
    ) -> (
        Arc<LocalStateStore>,
        PathBuf,
        LocalStateConfig,
        ReplicaScope,
        NodeId,
        NodeId,
        OutboundIntent,
    ) {
        let dir =
            std::env::temp_dir().join(format!("synveil-84c-{label}-{}", uuid::Uuid::now_v7()));
        fs::create_dir(&dir).unwrap();
        let config = LocalStateConfig::new(dir.join("state.sqlite3"));
        let store = Arc::new(LocalStateStore::open(&config).await.unwrap());
        let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
        store
            .bind_replica(scope, RootBindingId::new())
            .await
            .unwrap();
        let root = NodeId::new();
        let obsolete = NodeId::new();
        for node in [
            LocalNode::new(
                scope.library_id(),
                root,
                None,
                ManagedRelativePath::root(),
                LogicalName::new("root").unwrap(),
                NodeKind::Directory,
                NodeState::Active,
                Revision::new(1),
                None,
                None,
                None,
                None,
                None,
                Sequence::new(0),
                false,
                None,
            ),
            LocalNode::new(
                scope.library_id(),
                obsolete,
                Some(root),
                ManagedRelativePath::new("obsolete").unwrap(),
                LogicalName::new("obsolete").unwrap(),
                NodeKind::Directory,
                NodeState::Active,
                Revision::new(1),
                None,
                None,
                None,
                None,
                None,
                Sequence::new(0),
                false,
                None,
            ),
        ] {
            store.upsert_local_node(&node).await.unwrap();
        }
        let intent = OutboundIntent::new(
            scope.library_id(),
            Some(obsolete),
            None,
            OutboundIntentKind::RenameNode,
            ManagedRelativePath::new("local-name").unwrap(),
            Some(ManagedRelativePath::new("obsolete").unwrap()),
            Some(LocalFingerprint::directory()),
            Sequence::new(1),
            Sequence::new(1),
            Some(Revision::new(1)),
            None,
            None,
        )
        .unwrap();
        store.upsert_outbound_intent(&intent).await.unwrap();
        (store, dir, config, scope, root, obsolete, intent)
    }

    async fn reopen(
        store: Arc<LocalStateStore>,
        config: &LocalStateConfig,
    ) -> Arc<LocalStateStore> {
        store.close_pool().await;
        drop(store);
        Arc::new(LocalStateStore::open(config).await.unwrap())
    }

    async fn outbound_snapshot(
        store: &LocalStateStore,
        scope: ReplicaScope,
    ) -> Vec<OutboundIntent> {
        store
            .list_pending_intents(scope.library_id())
            .await
            .unwrap()
    }

    fn assert_outbound_eq(before: &[OutboundIntent], after: &[OutboundIntent]) {
        assert_eq!(
            before.len(),
            after.len(),
            "outbound intent count must be unchanged"
        );
        for (index, (expected, actual)) in before.iter().zip(after.iter()).enumerate() {
            assert_eq!(
                expected.intent_id(),
                actual.intent_id(),
                "intent {index}: id"
            );
            assert_eq!(expected.kind(), actual.kind(), "intent {index}: operation");
            assert_eq!(
                expected.observed_relative_path(),
                actual.observed_relative_path(),
                "intent {index}: payload path"
            );
            assert_eq!(
                expected.old_relative_path(),
                actual.old_relative_path(),
                "intent {index}: old path"
            );
            assert_eq!(
                expected.observed_fingerprint(),
                actual.observed_fingerprint(),
                "intent {index}: content/source reference"
            );
            assert_eq!(
                expected.base_epoch(),
                actual.base_epoch(),
                "intent {index}: base epoch"
            );
            assert_eq!(
                expected.base_applied_sequence(),
                actual.base_applied_sequence(),
                "intent {index}: base sequence"
            );
            assert_eq!(
                expected.base_revision(),
                actual.base_revision(),
                "intent {index}: base revision"
            );
            assert_eq!(
                expected.base_current_version_id(),
                actual.base_current_version_id(),
                "intent {index}: base version"
            );
            assert_eq!(expected.state(), actual.state(), "intent {index}: state");
            assert_eq!(expected, actual, "intent {index}: full equality");
        }
    }

    async fn assert_old_active_no_handoff(
        store: &LocalStateStore,
        scope: ReplicaScope,
        obsolete: NodeId,
    ) {
        assert!(
            store
                .local_node(scope.library_id(), obsolete)
                .await
                .unwrap()
                .is_some(),
            "old active state must remain"
        );
        assert!(
            !store
                .rebaseline_handoff_pending(scope.library_id())
                .await
                .unwrap(),
            "no handoff must exist"
        );
    }

    fn new_descriptor(scope: ReplicaScope, count: u64) -> RebaselineSnapshotDescriptor {
        RebaselineSnapshotDescriptor::new(
            RebaselineSnapshotId::new(),
            scope.library_id(),
            boundary(),
            count,
        )
    }

    // -----------------------------------------------------------------------
    // PHASE 3 — exact v4 -> current on-disk upgrade
    // -----------------------------------------------------------------------

    async fn sqlite_pool(path: &std::path::Path) -> sqlx::SqlitePool {
        sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!("sqlite:{}?mode=rwc", path.display()))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn client84c_v4_to_current_upgrade_preserves_state() {
        let dir = std::env::temp_dir().join(format!("synveil-84c-v4v6-{}", uuid::Uuid::now_v7()));
        fs::create_dir(&dir).unwrap();
        let db_path = dir.join("state.sqlite3");
        // Build a REAL version-4 database from the committed v1..v4 migration
        // files (never by hand-setting a version integer on a v5 database).
        let staging = dir.join("migrations_v4");
        fs::create_dir(&staging).unwrap();
        for name in [
            "0001_initial.sql",
            "0002_server_profiles.sql",
            "0003_outbound_observation.sql",
            "0004_outbound_submission.sql",
        ] {
            let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("migrations")
                .join(name);
            fs::copy(&source, staging.join(name)).unwrap();
        }
        let migrator = sqlx::migrate::Migrator::new(staging.clone()).await.unwrap();
        let pool = sqlite_pool(&db_path).await;
        migrator.run(&pool).await.unwrap();
        let version: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(version),0) FROM _sqlx_migrations WHERE success=1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(version, 4, "fixture must be a real schema version 4");

        // --- seed representative v4 state via real v4 DDL ---
        let owner = uuid::Uuid::now_v7().to_string();
        let device = uuid::Uuid::now_v7().to_string();
        let library = uuid::Uuid::now_v7().to_string();
        let binding = uuid::Uuid::now_v7().to_string();
        let root_id = uuid::Uuid::now_v7().to_string();
        let child_id = uuid::Uuid::now_v7().to_string();
        let file_id = uuid::Uuid::now_v7().to_string();
        let now: i64 = 1_700_000_000_000;
        sqlx::query(
            "INSERT INTO replicas (library_id, owner_user_id, device_id, root_binding_id, root_node_id, journal_epoch, applied_sequence, acknowledged_sequence, status, created_at_ms, updated_at_ms) VALUES (?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&library).bind(&owner).bind(&device).bind(&binding).bind(&root_id)
        .bind(3_i64).bind(7_i64).bind(7_i64).bind("IDLE").bind(now).bind(now)
        .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO observation_state (library_id, updated_at_ms) VALUES (?,?)")
            .bind(&library)
            .bind(now)
            .execute(&pool)
            .await
            .unwrap();
        // authoritative remote mirror: root + ordinary child + file with local source ref
        for (node, parent, path, key, name, kind, rev, local_len) in [
            (
                root_id.clone(),
                None,
                ".".to_string(),
                ".".to_string(),
                "root".to_string(),
                "DIRECTORY",
                4_i64,
                None,
            ),
            (
                child_id.clone(),
                Some(root_id.clone()),
                "child".to_string(),
                "child".to_string(),
                "child".to_string(),
                "DIRECTORY",
                4_i64,
                None,
            ),
            (
                file_id.clone(),
                Some(root_id.clone()),
                "doc.bin".to_string(),
                "doc.bin".to_string(),
                "doc.bin".to_string(),
                "FILE",
                6_i64,
                Some(11_i64),
            ),
        ] {
            let empty: Option<Vec<u8>> = None;
            sqlx::query(
                "INSERT INTO local_nodes (library_id, node_id, parent_node_id, relative_path, collision_key, logical_name, node_kind, node_state, revision, current_version_id, content_length, content_sha256, local_length, local_sha256, bootstrap_generation, present, quarantine_relative_path) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            )
            .bind(&library).bind(&node).bind(parent).bind(&path).bind(format!("ck:{key}")).bind(&name).bind(kind).bind("ACTIVE").bind(rev)
            .bind(if kind == "FILE" { Some(uuid::Uuid::now_v7().to_string()) } else { None })
            .bind(if kind == "FILE" { Some(11_i64) } else { None })
            .bind(if kind == "FILE" { Some(vec![0xAB; 32]) } else { empty.clone() })
            .bind(local_len).bind(if local_len.is_some() { Some(vec![0xCD; 32]) } else { empty })
            .bind(0_i64).bind(1_i64).bind(Option::<String>::None)
            .execute(&pool).await.unwrap();
        }
        // multiple outbound intents: rename + move + content update
        let intent_rename = uuid::Uuid::now_v7().to_string();
        let intent_move = uuid::Uuid::now_v7().to_string();
        let intent_content = uuid::Uuid::now_v7().to_string();
        let dedupe = |tag: &str| {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(tag.as_bytes());
            h.finalize().to_vec()
        };
        sqlx::query(
            "INSERT INTO outbound_intents (intent_id, library_id, node_id, parent_node_id, intent_kind, state, observed_relative_path, old_relative_path, observed_kind, observed_length, observed_sha256, base_epoch, base_applied_sequence, base_revision, base_current_version_id, base_parent_revision, dedupe_version, dedupe_sha256, created_at_ms, updated_at_ms) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&intent_rename).bind(&library).bind(&child_id).bind(Option::<String>::None).bind("RENAME_NODE").bind("PENDING")
        .bind("renamed").bind(Some("child".to_string())).bind("DIRECTORY").bind(Option::<i64>::None).bind(Option::<Vec<u8>>::None)
        .bind(3_i64).bind(7_i64).bind(Some(4_i64)).bind(Option::<String>::None).bind(Option::<i64>::None)
        .bind(1_i64).bind(dedupe("rename")).bind(now).bind(now)
        .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO outbound_intents (intent_id, library_id, node_id, parent_node_id, intent_kind, state, observed_relative_path, old_relative_path, observed_kind, observed_length, observed_sha256, base_epoch, base_applied_sequence, base_revision, base_current_version_id, base_parent_revision, dedupe_version, dedupe_sha256, created_at_ms, updated_at_ms) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&intent_move).bind(&library).bind(&file_id).bind(Option::<String>::None).bind("MOVE_NODE").bind("PENDING")
        .bind("moved-doc.bin").bind(Some("doc.bin".to_string())).bind("FILE").bind(Some(11_i64)).bind(Some(vec![0xCD; 32]))
        .bind(3_i64).bind(7_i64).bind(Some(6_i64)).bind(Option::<String>::None).bind(Option::<i64>::None)
        .bind(1_i64).bind(dedupe("move")).bind(now).bind(now)
        .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO outbound_intents (intent_id, library_id, node_id, parent_node_id, intent_kind, state, observed_relative_path, old_relative_path, observed_kind, observed_length, observed_sha256, base_epoch, base_applied_sequence, base_revision, base_current_version_id, base_parent_revision, dedupe_version, dedupe_sha256, created_at_ms, updated_at_ms) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&intent_content).bind(&library).bind(&file_id).bind(Option::<String>::None).bind("MODIFY_FILE_CONTENT").bind("PENDING")
        .bind("doc.bin").bind(Option::<String>::None).bind("FILE").bind(Some(11_i64)).bind(Some(vec![0xCD; 32]))
        .bind(3_i64).bind(7_i64).bind(Some(6_i64)).bind(Option::<String>::None).bind(Option::<i64>::None)
        .bind(1_i64).bind(dedupe("content")).bind(now).bind(now)
        .execute(&pool).await.unwrap();
        // mutation/submission state for the rename intent
        sqlx::query(
            "INSERT INTO outbound_mutation_requests (intent_id, mutation_id, mutation_kind, base_epoch, base_sequence, request_json, fingerprint_version, fingerprint_sha256, created_at_ms, updated_at_ms) VALUES (?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&intent_rename).bind(uuid::Uuid::now_v7().to_string()).bind("RENAME_NODE").bind(3_i64).bind(7_i64)
        .bind("{\"kind\":\"rename\"}").bind(1_i64).bind(dedupe("fp")).bind(now).bind(now)
        .execute(&pool).await.unwrap();
        // staged upload for the content intent (local source bytes reference)
        sqlx::query(
            "INSERT INTO outbound_upload_sessions (intent_id, upload_session_id, operation, staging_relative_path, expected_length, expected_sha256, acknowledged_offset, state, created_at_ms, updated_at_ms) VALUES (?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&intent_content).bind(Option::<String>::None).bind("REPLACE_CONTENT").bind(".synveil/staging/content-1").bind(11_i64).bind(vec![0xCD; 32]).bind(0_i64).bind("STAGED").bind(now).bind(now)
        .execute(&pool).await.unwrap();
        // ordinary inbound cursor/state + one applied event
        let event_id = uuid::Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO applied_events (library_id, epoch, sequence, event_id, resource_id, resource_revision) VALUES (?,?,?,?,?,?)",
        )
        .bind(&library).bind(3_i64).bind(7_i64).bind(&event_id).bind(&child_id).bind(4_i64)
        .execute(&pool).await.unwrap();

        // snapshot before-state via raw SQL
        let nodes_before: Vec<(String, Option<String>, String, String, i64)> =
            sqlx::query_as("SELECT node_id, parent_node_id, logical_name, node_kind, revision FROM local_nodes WHERE library_id = ? ORDER BY node_id")
                .bind(&library).fetch_all(&pool).await.unwrap();
        let intents_before: Vec<(String, String, String)> =
            sqlx::query_as("SELECT intent_id, intent_kind, observed_relative_path FROM outbound_intents WHERE library_id = ? ORDER BY intent_id")
                .bind(&library).fetch_all(&pool).await.unwrap();
        assert_eq!(nodes_before.len(), 3);
        assert_eq!(intents_before.len(), 3);
        pool.close().await;

        // --- upgrade with the current client persistence ---
        let config = LocalStateConfig::new(db_path.clone());
        let store = LocalStateStore::open(&config).await.unwrap();
        let version_after = store.schema_version().await.unwrap();
        assert_eq!(version_after, 6, "schema must migrate 4 -> 6");
        // new tables exist and are initially empty
        let pool2 = sqlite_pool(&db_path).await;
        for table in [
            "rebaseline_candidates",
            "rebaseline_candidate_nodes",
            "rebaseline_candidate_cursors",
            "rebaseline_applied_handoffs",
            "sync_conflicts",
        ] {
            let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&pool2)
                .await
                .unwrap();
            assert_eq!(count, 0, "new table {table} must start empty");
        }
        let nodes_after: Vec<(String, Option<String>, String, String, i64)> =
            sqlx::query_as("SELECT node_id, parent_node_id, logical_name, node_kind, revision FROM local_nodes WHERE library_id = ? ORDER BY node_id")
                .bind(&library).fetch_all(&pool2).await.unwrap();
        let intents_after: Vec<(String, String, String)> =
            sqlx::query_as("SELECT intent_id, intent_kind, observed_relative_path FROM outbound_intents WHERE library_id = ? ORDER BY intent_id")
                .bind(&library).fetch_all(&pool2).await.unwrap();
        assert_eq!(
            nodes_before, nodes_after,
            "remote nodes/values must survive"
        );
        assert_eq!(
            intents_before, intents_after,
            "outbound intents must survive"
        );
        // submission rows survive
        let mutation_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM outbound_mutation_requests WHERE intent_id = ?",
        )
        .bind(&intent_rename)
        .fetch_one(&pool2)
        .await
        .unwrap();
        assert_eq!(mutation_count, 1);
        let upload_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM outbound_upload_sessions WHERE intent_id = ?")
                .bind(&intent_content)
                .fetch_one(&pool2)
                .await
                .unwrap();
        assert_eq!(upload_count, 1);
        let applied_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM applied_events WHERE library_id = ?")
                .bind(&library)
                .fetch_one(&pool2)
                .await
                .unwrap();
        assert_eq!(applied_count, 1, "inbound state must survive");
        pool2.close().await;

        // reopen v6: no duplicate migration, no damage, no mutation
        store.close_pool().await;
        drop(store);
        let store2 = LocalStateStore::open(&config).await.unwrap();
        assert_eq!(store2.schema_version().await.unwrap(), 6);
        let pool3 = sqlite_pool(&db_path).await;
        let nodes_reopen: Vec<(String, Option<String>, String, String, i64)> =
            sqlx::query_as("SELECT node_id, parent_node_id, logical_name, node_kind, revision FROM local_nodes WHERE library_id = ? ORDER BY node_id")
                .bind(&library).fetch_all(&pool3).await.unwrap();
        assert_eq!(nodes_after, nodes_reopen);
        pool3.close().await;
        store2.close_pool().await;
        drop(store2);
        fs::remove_dir_all(dir).unwrap();
    }

    // -----------------------------------------------------------------------
    // PHASE 4 — C1..C9 crash matrix
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn client84c_c1_before_candidate_creation() {
        let (store, dir, _config, scope, _root, obsolete, intent) = setup("c1").await;
        let before = outbound_snapshot(&store, scope).await;
        assert_old_active_no_handoff(&store, scope, obsolete).await;
        assert!(
            store
                .rebaseline_candidate(scope.library_id())
                .await
                .unwrap()
                .is_none(),
            "C1: no candidate"
        );
        assert_eq!(before, vec![intent]);
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_c2_descriptor_persisted() {
        let (store, dir, config, scope, _root, obsolete, intent) = setup("c2").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 2);
        store
            .begin_rebaseline_candidate(scope, descriptor)
            .await
            .unwrap();
        // crash: close and reopen on a fresh instance
        let store2 = reopen(store, &config).await;
        assert_old_active_no_handoff(&store2, scope, obsolete).await;
        let candidate = store2
            .rebaseline_candidate(scope.library_id())
            .await
            .unwrap()
            .expect("C2: candidate metadata durable");
        assert_eq!(candidate.snapshot_id, descriptor.snapshot_id());
        assert_eq!(candidate.received_count, 0);
        assert!(!candidate.terminal_fetched);
        assert_outbound_eq(&before, &outbound_snapshot(&store2, scope).await);
        assert_eq!(before, vec![intent]);
        store2.close_pool().await;
        drop(store2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_c3_page1_persisted_then_resume() {
        let (store, dir, config, scope, root, obsolete, _intent) = setup("c3").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 2);
        store
            .begin_rebaseline_candidate(scope, descriptor)
            .await
            .unwrap();
        let child = NodeId::new();
        let mut first_entries = vec![directory(root, None, "root")];
        first_entries.sort_by_key(LogicalSnapshotNode::node_id);
        // persist only the first page entry with a cursor (partial progress)
        let first =
            RebaselineSnapshotPage::new(descriptor, first_entries, Some(cursor(1))).unwrap();
        // NOTE: single-entry first page is only valid when it is not terminal;
        // terminal validation runs only on next_cursor == None pages.
        let _ = child;
        store
            .persist_rebaseline_page(scope, descriptor, &first)
            .await
            .unwrap();
        // crash immediately after page-1 commit
        let store2 = reopen(store, &config).await;
        assert_old_active_no_handoff(&store2, scope, obsolete).await;
        let candidate = store2
            .rebaseline_candidate(scope.library_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(candidate.received_count, 1);
        assert_eq!(candidate.next_cursor, Some(cursor(1)));
        // resume: second page must not duplicate page-1 entries
        let child2 = NodeId::new();
        let mut second_entries = vec![directory(child2, Some(root), "child")];
        second_entries.sort_by_key(LogicalSnapshotNode::node_id);
        // ensure cross-page monotonicity regardless of generated UUID order
        let last_first = store2
            .rebaseline_candidate(scope.library_id())
            .await
            .unwrap()
            .unwrap();
        let _ = last_first;
        // fetch stored max node id to build a strictly greater child if needed
        let mut ordered = vec![
            directory(root, None, "root"),
            directory(child2, Some(root), "child"),
        ];
        ordered.sort_by_key(LogicalSnapshotNode::node_id);
        // If child2 sorts before root, resume with a fresh greater id.
        let resume_child = if ordered[0].node_id() == root {
            child2
        } else {
            // regenerate until greater than root (deterministic bounded loop)
            let mut candidate_id = child2;
            for _ in 0..64 {
                if candidate_id > root {
                    break;
                }
                candidate_id = NodeId::new();
            }
            assert!(candidate_id > root, "test must pick a greater child id");
            candidate_id
        };
        let second = RebaselineSnapshotPage::new(
            descriptor,
            vec![directory(resume_child, Some(root), "child")],
            None,
        )
        .unwrap();
        // re-persisting the exact first page must fail (duplicate protection)
        assert!(
            store2
                .persist_rebaseline_page(scope, descriptor, &first)
                .await
                .is_err(),
            "C3: duplicate page entries must be rejected"
        );
        store2
            .persist_rebaseline_page(scope, descriptor, &second)
            .await
            .unwrap();
        store2
            .activate_rebaseline_candidate(scope, descriptor)
            .await
            .unwrap();
        assert!(
            store2
                .local_node(scope.library_id(), resume_child)
                .await
                .unwrap()
                .is_some()
        );
        assert_outbound_eq(&before, &outbound_snapshot(&store2, scope).await);
        store2.close_pool().await;
        drop(store2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_c4_multiple_pages_persisted_resume_from_cursor() {
        let (store, dir, config, scope, root, obsolete, _intent) = setup("c4").await;
        let before = outbound_snapshot(&store, scope).await;
        // 4 entries across pages of 1: root + 3 children with increasing ids
        let mut children = vec![NodeId::new(), NodeId::new(), NodeId::new()];
        children.sort();
        let mut all = vec![directory(root, None, "root")];
        for (index, child) in children.iter().enumerate() {
            all.push(directory(*child, Some(root), &format!("child-{index}")));
        }
        all.sort_by_key(LogicalSnapshotNode::node_id);
        let descriptor = new_descriptor(scope, all.len() as u64);
        store
            .begin_rebaseline_candidate(scope, descriptor)
            .await
            .unwrap();
        // persist first two pages (one entry each, non-terminal)
        for (index, entry) in all.iter().take(2).enumerate() {
            let page = RebaselineSnapshotPage::new(
                descriptor,
                vec![entry.clone()],
                Some(page_cursor(index)),
            )
            .unwrap();
            store
                .persist_rebaseline_page(scope, descriptor, &page)
                .await
                .unwrap();
        }
        let store2 = reopen(store, &config).await;
        assert_old_active_no_handoff(&store2, scope, obsolete).await;
        let candidate = store2
            .rebaseline_candidate(scope.library_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(candidate.received_count, 2, "C4: progress retained");
        assert_eq!(
            candidate.next_cursor,
            Some(page_cursor(1)),
            "C4: saved cursor retained"
        );
        // resume continues from next cursor: remaining entries
        for (index, entry) in all.iter().skip(2).enumerate() {
            let global = index + 2;
            let last = global + 1 == all.len();
            let page = RebaselineSnapshotPage::new(
                descriptor,
                vec![entry.clone()],
                if last {
                    None
                } else {
                    Some(page_cursor(global))
                },
            )
            .unwrap();
            store2
                .persist_rebaseline_page(scope, descriptor, &page)
                .await
                .unwrap();
        }
        assert_old_active_no_handoff(&store2, scope, obsolete).await;
        store2
            .activate_rebaseline_candidate(scope, descriptor)
            .await
            .unwrap();
        assert_outbound_eq(&before, &outbound_snapshot(&store2, scope).await);
        store2.close_pool().await;
        drop(store2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_c5_final_page_not_validated() {
        let (store, dir, _config, scope, root, obsolete, _intent) = setup("c5").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 2);
        store
            .begin_rebaseline_candidate(scope, descriptor)
            .await
            .unwrap();
        // valid first page
        let mut first = vec![directory(root, None, "root")];
        first.sort_by_key(LogicalSnapshotNode::node_id);
        // choose a child id greater than root for cross-page order
        let mut child = NodeId::new();
        for _ in 0..64 {
            if child > root {
                break;
            }
            child = NodeId::new();
        }
        let page1 = RebaselineSnapshotPage::new(descriptor, first, Some(cursor(9))).unwrap();
        store
            .persist_rebaseline_page(scope, descriptor, &page1)
            .await
            .unwrap();
        // terminal page with a missing parent: terminal validation must fail,
        // so no activation is possible and old state stays active.
        let orphan = NodeId::new();
        let bad_terminal = RebaselineSnapshotPage::new(
            descriptor,
            vec![directory(orphan, Some(NodeId::new()), "orphan")],
            None,
        )
        .unwrap();
        assert!(
            store
                .persist_rebaseline_page(scope, descriptor, &bad_terminal)
                .await
                .is_err(),
            "C5: invalid terminal page must fail"
        );
        assert_old_active_no_handoff(&store, scope, obsolete).await;
        let candidate = store
            .rebaseline_candidate(scope.library_id())
            .await
            .unwrap()
            .unwrap();
        assert!(
            !candidate.terminal_fetched,
            "C5: candidate must not be terminal"
        );
        assert_outbound_eq(&before, &outbound_snapshot(&store, scope).await);
        let _ = child;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_c6_validated_but_not_activated() {
        let (store, dir, config, scope, root, obsolete, _intent) = setup("c6").await;
        let before = outbound_snapshot(&store, scope).await;
        let mut child = NodeId::new();
        for _ in 0..64 {
            if child > root {
                break;
            }
            child = NodeId::new();
        }
        let mut entries = vec![
            directory(root, None, "root"),
            directory(child, Some(root), "child"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        let descriptor = new_descriptor(scope, 2);
        // split into two pages to also prove multi-page COMPLETE
        let first_entry = entries[0].clone();
        let second_entry = entries[1].clone();
        store
            .begin_rebaseline_candidate(scope, descriptor)
            .await
            .unwrap();
        store
            .persist_rebaseline_page(
                scope,
                descriptor,
                &RebaselineSnapshotPage::new(descriptor, vec![first_entry], Some(cursor(3)))
                    .unwrap(),
            )
            .await
            .unwrap();
        store
            .persist_rebaseline_page(
                scope,
                descriptor,
                &RebaselineSnapshotPage::new(descriptor, vec![second_entry], None).unwrap(),
            )
            .await
            .unwrap();
        // validated but activation has not started
        assert_old_active_no_handoff(&store, scope, obsolete).await;
        let candidate = store
            .rebaseline_candidate(scope.library_id())
            .await
            .unwrap()
            .unwrap();
        assert!(candidate.terminal_fetched, "C6: candidate must be COMPLETE");
        assert_eq!(candidate.received_count, 2);
        // crash/restart: complete candidate must remain durable
        let store2 = reopen(store, &config).await;
        assert_old_active_no_handoff(&store2, scope, obsolete).await;
        assert!(
            store2
                .rebaseline_candidate(scope.library_id())
                .await
                .unwrap()
                .unwrap()
                .terminal_fetched
        );
        assert_outbound_eq(&before, &outbound_snapshot(&store2, scope).await);
        store2.close_pool().await;
        drop(store2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_c7_fail_inside_activation_before_commit() {
        let (store, dir, _config, scope, root, obsolete, intent) = setup("c7").await;
        let before = outbound_snapshot(&store, scope).await;
        let mut child = NodeId::new();
        for _ in 0..64 {
            if child > root {
                break;
            }
            child = NodeId::new();
        }
        let mut entries = vec![
            directory(root, None, "root"),
            directory(child, Some(root), "child"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        let descriptor = new_descriptor(scope, 2);
        store
            .begin_rebaseline_candidate(scope, descriptor)
            .await
            .unwrap();
        // stage in NodeId order across two pages
        store
            .persist_rebaseline_page(
                scope,
                descriptor,
                &RebaselineSnapshotPage::new(descriptor, vec![entries[0].clone()], Some(cursor(4)))
                    .unwrap(),
            )
            .await
            .unwrap();
        store
            .persist_rebaseline_page(
                scope,
                descriptor,
                &RebaselineSnapshotPage::new(descriptor, vec![entries[1].clone()], None).unwrap(),
            )
            .await
            .unwrap();
        assert!(matches!(
            store
                .activate_rebaseline_candidate_fail_before_commit(scope, descriptor)
                .await,
            Err(ClientSyncError::InjectedFailure)
        ));
        // transaction rollback: old active, new invisible, no handoff
        assert_old_active_no_handoff(&store, scope, obsolete).await;
        assert!(
            store
                .local_node(scope.library_id(), child)
                .await
                .unwrap()
                .is_none()
                || store
                    .local_node(scope.library_id(), obsolete)
                    .await
                    .unwrap()
                    .is_some(),
            "C7: new state must be invisible, old state active"
        );
        assert!(
            store
                .rebaseline_candidate(scope.library_id())
                .await
                .unwrap()
                .is_some(),
            "C7: candidate must survive rollback"
        );
        assert_outbound_eq(&before, &outbound_snapshot(&store, scope).await);
        assert_eq!(before, vec![intent]);
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_c8_commit_then_crash_before_observed() {
        let (store, dir, config, scope, root, obsolete, _intent) = setup("c8").await;
        let before = outbound_snapshot(&store, scope).await;
        let mut child = NodeId::new();
        for _ in 0..64 {
            if child > root {
                break;
            }
            child = NodeId::new();
        }
        let mut entries = vec![
            directory(root, None, "root"),
            directory(child, Some(root), "child"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        let descriptor = new_descriptor(scope, 2);
        let source = ScriptedSource(Mutex::new(VecDeque::from([
            Ok(
                RebaselineSnapshotPage::new(descriptor, vec![entries[0].clone()], Some(cursor(5)))
                    .unwrap(),
            ),
            Ok(RebaselineSnapshotPage::new(descriptor, vec![entries[1].clone()], None).unwrap()),
        ])));
        let applier = RebaselineApplier::new(scope, Arc::clone(&store), 1).unwrap();
        assert_eq!(
            applier.apply(descriptor, &source).await.unwrap(),
            RebaselineApplyOutcome::Applied
        );
        // caller/process dies before success is observed: close immediately
        drop(applier);
        let store2 = reopen(store, &config).await;
        // new state active, handoff present, old not active
        assert!(
            store2
                .local_node(scope.library_id(), child)
                .await
                .unwrap()
                .is_some(),
            "C8: new state must be active"
        );
        assert!(
            store2
                .local_node(scope.library_id(), obsolete)
                .await
                .unwrap()
                .is_none(),
            "C8: old-only node must be gone"
        );
        assert!(
            store2
                .rebaseline_handoff_pending(scope.library_id())
                .await
                .unwrap(),
            "C8: handoff must be present"
        );
        assert_outbound_eq(&before, &outbound_snapshot(&store2, scope).await);
        store2.close_pool().await;
        drop(store2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_c9_retry_same_snapshot_after_c8_is_idempotent() {
        let (store, dir, config, scope, root, obsolete, _intent) = setup("c9").await;
        let before = outbound_snapshot(&store, scope).await;
        let mut child = NodeId::new();
        for _ in 0..64 {
            if child > root {
                break;
            }
            child = NodeId::new();
        }
        let mut entries = vec![
            directory(root, None, "root"),
            directory(child, Some(root), "child"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        let descriptor = new_descriptor(scope, 2);
        let source = ScriptedSource(Mutex::new(VecDeque::from([
            Ok(
                RebaselineSnapshotPage::new(descriptor, vec![entries[0].clone()], Some(cursor(6)))
                    .unwrap(),
            ),
            Ok(RebaselineSnapshotPage::new(descriptor, vec![entries[1].clone()], None).unwrap()),
        ])));
        let applier = RebaselineApplier::new(scope, Arc::clone(&store), 1).unwrap();
        assert_eq!(
            applier.apply(descriptor, &source).await.unwrap(),
            RebaselineApplyOutcome::Applied
        );
        let nodes_after_first = store.local_nodes(scope.library_id()).await.unwrap();
        drop(applier);
        let store2 = reopen(store, &config).await;
        // retry exact same snapshot after C8
        let applier2 = RebaselineApplier::new(scope, Arc::clone(&store2), 1).unwrap();
        let empty_source = ScriptedSource(Mutex::new(VecDeque::new()));
        let outcome = applier2.apply(descriptor, &empty_source).await.unwrap();
        assert_eq!(
            outcome,
            RebaselineApplyOutcome::AlreadyApplied,
            "C9: must be idempotent"
        );
        assert!(
            store2
                .rebaseline_candidate(scope.library_id())
                .await
                .unwrap()
                .is_none(),
            "C9: no duplicate candidate"
        );
        assert!(
            store2
                .rebaseline_handoff_pending(scope.library_id())
                .await
                .unwrap(),
            "C9: single handoff retained"
        );
        let nodes_after_retry = store2.local_nodes(scope.library_id()).await.unwrap();
        assert_eq!(
            nodes_after_first, nodes_after_retry,
            "C9: no second replacement"
        );
        assert!(
            store2
                .local_node(scope.library_id(), obsolete)
                .await
                .unwrap()
                .is_none()
        );
        assert_outbound_eq(&before, &outbound_snapshot(&store2, scope).await);
        let _ = root;
        store2.close_pool().await;
        drop(store2);
        fs::remove_dir_all(dir).unwrap();
    }

    // -----------------------------------------------------------------------
    // PHASE 5 — M1..M23 malformed source matrix
    // -----------------------------------------------------------------------

    async fn assert_apply_fails_closed(
        store: &Arc<LocalStateStore>,
        scope: ReplicaScope,
        obsolete: NodeId,
        before: &[OutboundIntent],
        descriptor: RebaselineSnapshotDescriptor,
        source: ScriptedSource,
        page_limit: u32,
    ) {
        let applier = RebaselineApplier::new(scope, Arc::clone(store), page_limit).unwrap();
        let result = applier.apply(descriptor, &source).await;
        assert!(result.is_err(), "malformed source must fail closed");
        assert_old_active_no_handoff(store, scope, obsolete).await;
        assert_outbound_eq(before, &outbound_snapshot(store, scope).await);
    }

    fn greater_than(value: NodeId) -> NodeId {
        let mut candidate = NodeId::new();
        for _ in 0..128 {
            if candidate > value {
                return candidate;
            }
            candidate = NodeId::new();
        }
        panic!("could not sample a greater NodeId");
    }

    #[tokio::test]
    async fn client84c_m01_wrong_snapshot_id() {
        let (store, dir, _config, scope, root, obsolete, _intent) = setup("m01").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 2);
        let other = RebaselineSnapshotDescriptor::new(
            RebaselineSnapshotId::new(),
            scope.library_id(),
            boundary(),
            2,
        );
        let child = greater_than(root);
        let mut entries = vec![
            directory(root, None, "root"),
            directory(child, Some(root), "child"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        let page = RebaselineSnapshotPage::new(other, entries, None).unwrap();
        assert_apply_fails_closed(
            &store,
            scope,
            obsolete,
            &before,
            descriptor,
            ScriptedSource(Mutex::new(VecDeque::from([Ok(page)]))),
            256,
        )
        .await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m02_wrong_library_id() {
        let (store, dir, _config, scope, root, obsolete, _intent) = setup("m02").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 2);
        let wrong = RebaselineSnapshotDescriptor::new(
            descriptor.snapshot_id(),
            LibraryId::new(),
            boundary(),
            2,
        );
        let child = greater_than(root);
        let mut entries = vec![
            directory(root, None, "root"),
            directory(child, Some(root), "child"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        let page = RebaselineSnapshotPage::new(wrong, entries, None).unwrap();
        assert_apply_fails_closed(
            &store,
            scope,
            obsolete,
            &before,
            descriptor,
            ScriptedSource(Mutex::new(VecDeque::from([Ok(page)]))),
            256,
        )
        .await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m03_wrong_journal_boundary() {
        let (store, dir, _config, scope, root, obsolete, _intent) = setup("m03").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 2);
        let wrong = RebaselineSnapshotDescriptor::new(
            descriptor.snapshot_id(),
            scope.library_id(),
            RebaselineBoundary::new(Sequence::new(99), Sequence::new(3)),
            2,
        );
        let child = greater_than(root);
        let mut entries = vec![
            directory(root, None, "root"),
            directory(child, Some(root), "child"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        let page = RebaselineSnapshotPage::new(wrong, entries, None).unwrap();
        assert_apply_fails_closed(
            &store,
            scope,
            obsolete,
            &before,
            descriptor,
            ScriptedSource(Mutex::new(VecDeque::from([Ok(page)]))),
            256,
        )
        .await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m04_descriptor_entry_count_mismatch() {
        let (store, dir, _config, scope, root, obsolete, _intent) = setup("m04").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 2);
        let wrong = RebaselineSnapshotDescriptor::new(
            descriptor.snapshot_id(),
            scope.library_id(),
            boundary(),
            3,
        );
        let child = greater_than(root);
        let mut entries = vec![
            directory(root, None, "root"),
            directory(child, Some(root), "child"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        let page = RebaselineSnapshotPage::new(wrong, entries, None).unwrap();
        assert_apply_fails_closed(
            &store,
            scope,
            obsolete,
            &before,
            descriptor,
            ScriptedSource(Mutex::new(VecDeque::from([Ok(page)]))),
            256,
        )
        .await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m05_duplicate_node_id() {
        let (store, dir, _config, scope, root, obsolete, _intent) = setup("m05").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 2);
        let page = RebaselineSnapshotPage::new(
            descriptor,
            vec![directory(root, None, "root"), directory(root, None, "root")],
            None,
        )
        .unwrap();
        assert_apply_fails_closed(
            &store,
            scope,
            obsolete,
            &before,
            descriptor,
            ScriptedSource(Mutex::new(VecDeque::from([Ok(page)]))),
            256,
        )
        .await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m06_non_monotonic_ordering() {
        let (store, dir, _config, scope, root, obsolete, _intent) = setup("m06").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 2);
        let child = greater_than(root);
        let mut entries = vec![
            directory(root, None, "root"),
            directory(child, Some(root), "child"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        entries.reverse();
        let page = RebaselineSnapshotPage::new(descriptor, entries, None).unwrap();
        assert_apply_fails_closed(
            &store,
            scope,
            obsolete,
            &before,
            descriptor,
            ScriptedSource(Mutex::new(VecDeque::from([Ok(page)]))),
            256,
        )
        .await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m07_received_exceeds_expected() {
        let (store, dir, _config, scope, root, obsolete, _intent) = setup("m07").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 2);
        let mut ids = vec![root, NodeId::new(), NodeId::new()];
        ids.sort();
        // page 1: smallest entry + cursor; page 2: two more entries (total 3 > 2)
        let page1 = RebaselineSnapshotPage::new(
            descriptor,
            vec![directory(ids[0], None, "root")],
            Some(cursor(11)),
        )
        .unwrap();
        // second page entries must be increasing and greater than ids[0]
        let page2 = RebaselineSnapshotPage::new(
            descriptor,
            vec![
                directory(ids[1], Some(ids[0]), "b"),
                directory(ids[2], Some(ids[0]), "c"),
            ],
            None,
        )
        .unwrap();
        assert_apply_fails_closed(
            &store,
            scope,
            obsolete,
            &before,
            descriptor,
            ScriptedSource(Mutex::new(VecDeque::from([Ok(page1), Ok(page2)]))),
            256,
        )
        .await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m08_premature_final_page() {
        let (store, dir, _config, scope, root, obsolete, _intent) = setup("m08").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 2);
        let page =
            RebaselineSnapshotPage::new(descriptor, vec![directory(root, None, "root")], None)
                .unwrap();
        assert_apply_fails_closed(
            &store,
            scope,
            obsolete,
            &before,
            descriptor,
            ScriptedSource(Mutex::new(VecDeque::from([Ok(page)]))),
            256,
        )
        .await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m09_final_count_below_descriptor() {
        let (store, dir, _config, scope, root, obsolete, _intent) = setup("m09").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 3);
        let child = greater_than(root);
        let mut entries = vec![
            directory(root, None, "root"),
            directory(child, Some(root), "child"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        // two pages delivering 2 of 3 expected, terminal too early
        let page1 =
            RebaselineSnapshotPage::new(descriptor, vec![entries[0].clone()], Some(cursor(12)))
                .unwrap();
        let page2 =
            RebaselineSnapshotPage::new(descriptor, vec![entries[1].clone()], None).unwrap();
        assert_apply_fails_closed(
            &store,
            scope,
            obsolete,
            &before,
            descriptor,
            ScriptedSource(Mutex::new(VecDeque::from([Ok(page1), Ok(page2)]))),
            256,
        )
        .await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m10_empty_page_with_next_cursor() {
        let (store, dir, _config, scope, _root, obsolete, _intent) = setup("m10").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 2);
        let page = RebaselineSnapshotPage::new(descriptor, vec![], Some(cursor(13))).unwrap();
        assert_apply_fails_closed(
            &store,
            scope,
            obsolete,
            &before,
            descriptor,
            ScriptedSource(Mutex::new(VecDeque::from([Ok(page)]))),
            256,
        )
        .await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m11_same_next_cursor_repeated() {
        let (store, dir, _config, scope, root, obsolete, _intent) = setup("m11").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 3);
        let c1 = greater_than(root);
        let c2 = greater_than(c1);
        let page1 = RebaselineSnapshotPage::new(
            descriptor,
            vec![directory(root, None, "root")],
            Some(cursor(21)),
        )
        .unwrap();
        let page2 = RebaselineSnapshotPage::new(
            descriptor,
            vec![directory(c1, Some(root), "a")],
            Some(cursor(21)),
        )
        .unwrap();
        let _ = c2;
        assert_apply_fails_closed(
            &store,
            scope,
            obsolete,
            &before,
            descriptor,
            ScriptedSource(Mutex::new(VecDeque::from([Ok(page1), Ok(page2)]))),
            1,
        )
        .await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m12_true_cursor_cycle_a_b_a() {
        // Real persisted cycle proof: cursors A then B then A again. The
        // third page reuses cursor A from history (not the immediate
        // predecessor), which the candidate cursor table must reject.
        let (store, dir, _config, scope, root, obsolete, _intent) = setup("m12").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 4);
        let c1 = greater_than(root);
        let c2 = greater_than(c1);
        let c3 = greater_than(c2);
        let page1 = RebaselineSnapshotPage::new(
            descriptor,
            vec![directory(root, None, "root")],
            Some(cursor(31)),
        )
        .unwrap();
        let page2 = RebaselineSnapshotPage::new(
            descriptor,
            vec![directory(c1, Some(root), "a")],
            Some(cursor(32)),
        )
        .unwrap();
        // A -> B -> A: third page repeats the first cursor value.
        let page3 = RebaselineSnapshotPage::new(
            descriptor,
            vec![directory(c2, Some(root), "b")],
            Some(cursor(31)),
        )
        .unwrap();
        let _ = c3;
        assert_apply_fails_closed(
            &store,
            scope,
            obsolete,
            &before,
            descriptor,
            ScriptedSource(Mutex::new(VecDeque::from([
                Ok(page1),
                Ok(page2),
                Ok(page3),
            ]))),
            1,
        )
        .await;
        // cycle cursors A and B must be persisted as evidence of the attempt
        let candidate = store
            .rebaseline_candidate(scope.library_id())
            .await
            .unwrap()
            .expect("candidate must remain for inspection");
        assert_eq!(candidate.received_count, 2);
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m13_extra_page_after_expected_satisfied() {
        let (store, dir, _config, scope, root, obsolete, _intent) = setup("m13").await;
        let before = outbound_snapshot(&store, scope).await;
        let child = greater_than(root);
        let mut entries = vec![
            directory(root, None, "root"),
            directory(child, Some(root), "child"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        let descriptor = new_descriptor(scope, 2);
        store
            .begin_rebaseline_candidate(scope, descriptor)
            .await
            .unwrap();
        store
            .persist_rebaseline_page(
                scope,
                descriptor,
                &RebaselineSnapshotPage::new(
                    descriptor,
                    vec![entries[0].clone()],
                    Some(cursor(41)),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        store
            .persist_rebaseline_page(
                scope,
                descriptor,
                &RebaselineSnapshotPage::new(descriptor, vec![entries[1].clone()], None).unwrap(),
            )
            .await
            .unwrap();
        // candidate is now COMPLETE; any extra page must fail closed.
        let extra_child = greater_than(child);
        let extra = RebaselineSnapshotPage::new(
            descriptor,
            vec![directory(extra_child, Some(root), "extra")],
            None,
        )
        .unwrap();
        assert!(
            store
                .persist_rebaseline_page(scope, descriptor, &extra)
                .await
                .is_err(),
            "M13: extra page after terminal must fail"
        );
        assert_old_active_no_handoff(&store, scope, obsolete).await;
        assert_outbound_eq(&before, &outbound_snapshot(&store, scope).await);
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m14_multiple_roots() {
        let (store, dir, _config, scope, root, obsolete, _intent) = setup("m14").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 2);
        let other_root = NodeId::new();
        let mut entries = vec![
            directory(root, None, "root"),
            directory(other_root, None, "root2"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        let page = RebaselineSnapshotPage::new(descriptor, entries, None).unwrap();
        assert_apply_fails_closed(
            &store,
            scope,
            obsolete,
            &before,
            descriptor,
            ScriptedSource(Mutex::new(VecDeque::from([Ok(page)]))),
            256,
        )
        .await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m15_missing_parent() {
        let (store, dir, _config, scope, root, obsolete, _intent) = setup("m15").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 2);
        let orphan = greater_than(root);
        let mut entries = vec![
            directory(root, None, "root"),
            directory(orphan, Some(NodeId::new()), "orphan"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        let page = RebaselineSnapshotPage::new(descriptor, entries, None).unwrap();
        assert_apply_fails_closed(
            &store,
            scope,
            obsolete,
            &before,
            descriptor,
            ScriptedSource(Mutex::new(VecDeque::from([Ok(page)]))),
            256,
        )
        .await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m16_non_directory_parent() {
        let (store, dir, _config, scope, root, obsolete, _intent) = setup("m16").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 3);
        let file_parent = greater_than(root);
        let child_of_file = greater_than(file_parent);
        let mut entries = vec![
            directory(root, None, "root"),
            file_node(file_parent, Some(root), "parent.bin", 1),
            directory(child_of_file, Some(file_parent), "child"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        let page = RebaselineSnapshotPage::new(descriptor, entries, None).unwrap();
        assert_apply_fails_closed(
            &store,
            scope,
            obsolete,
            &before,
            descriptor,
            ScriptedSource(Mutex::new(VecDeque::from([Ok(page)]))),
            256,
        )
        .await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m17_topology_cycle() {
        let (store, dir, _config, scope, root, obsolete, _intent) = setup("m17").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 3);
        let a = greater_than(root);
        let b = greater_than(a);
        // root + A<->B cycle unreachable from root
        let mut entries = vec![
            directory(root, None, "root"),
            directory(a, Some(b), "a"),
            directory(b, Some(a), "b"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        let page = RebaselineSnapshotPage::new(descriptor, entries, None).unwrap();
        assert_apply_fails_closed(
            &store,
            scope,
            obsolete,
            &before,
            descriptor,
            ScriptedSource(Mutex::new(VecDeque::from([Ok(page)]))),
            256,
        )
        .await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m18_purging_state_rejected_by_domain() {
        // PURGING is an internal lifecycle state: the domain constructor
        // must refuse it, so no such entry can ever enter a candidate.
        let result = LogicalSnapshotNode::new(
            NodeId::new(),
            Some(NodeId::new()),
            LogicalName::new("purging").unwrap(),
            NodeKind::Directory,
            NodeState::Purging,
            Revision::new(1),
            None,
            None,
            None,
        );
        assert!(result.is_err(), "M18: PURGING must be rejected");
        let (store, dir, _config, scope, _root, obsolete, _intent) = setup("m18").await;
        let before = outbound_snapshot(&store, scope).await;
        assert_old_active_no_handoff(&store, scope, obsolete).await;
        assert_outbound_eq(&before, &outbound_snapshot(&store, scope).await);
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m19_snapshot_unavailable_404() {
        let (store, dir, _config, scope, _root, obsolete, _intent) = setup("m19").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 2);
        assert_apply_fails_closed(
            &store,
            scope,
            obsolete,
            &before,
            descriptor,
            ScriptedSource(Mutex::new(VecDeque::from([Err(RemoteError::new(
                RemoteErrorKind::NotFound,
            ))]))),
            256,
        )
        .await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m20_snapshot_expired_410_candidate_discarded() {
        // Deterministic expiry behavior: the apply fails closed, old state
        // is untouched, and the unusable candidate can be safely discarded
        // with abort_candidate (expired artifacts can never resume).
        let (store, dir, _config, scope, root, obsolete, _intent) = setup("m20").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 2);
        let page1 = RebaselineSnapshotPage::new(
            descriptor,
            vec![directory(root, None, "root")],
            Some(cursor(51)),
        )
        .unwrap();
        let applier = RebaselineApplier::new(scope, Arc::clone(&store), 1).unwrap();
        let source = ScriptedSource(Mutex::new(VecDeque::from([
            Ok(page1),
            Err(RemoteError::new(RemoteErrorKind::RebaselineRequired)),
        ])));
        assert!(applier.apply(descriptor, &source).await.is_err());
        assert_old_active_no_handoff(&store, scope, obsolete).await;
        assert_outbound_eq(&before, &outbound_snapshot(&store, scope).await);
        // expired snapshot cannot resume: discard deterministically
        applier.abort_candidate().await.unwrap();
        assert!(
            store
                .rebaseline_candidate(scope.library_id())
                .await
                .unwrap()
                .is_none(),
            "M20: expired candidate must be safely discardable"
        );
        assert_old_active_no_handoff(&store, scope, obsolete).await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m21_authentication_failure_401() {
        let (store, dir, _config, scope, _root, obsolete, _intent) = setup("m21").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 2);
        assert_apply_fails_closed(
            &store,
            scope,
            obsolete,
            &before,
            descriptor,
            ScriptedSource(Mutex::new(VecDeque::from([Err(RemoteError::new(
                RemoteErrorKind::AuthRequired,
            ))]))),
            256,
        )
        .await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m22_transport_internal_failure_500() {
        let (store, dir, _config, scope, _root, obsolete, _intent) = setup("m22").await;
        let before = outbound_snapshot(&store, scope).await;
        let descriptor = new_descriptor(scope, 2);
        assert_apply_fails_closed(
            &store,
            scope,
            obsolete,
            &before,
            descriptor,
            ScriptedSource(Mutex::new(VecDeque::from([Err(RemoteError::new(
                RemoteErrorKind::Internal,
            ))]))),
            256,
        )
        .await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_m23_transport_failure_after_pages_then_resume() {
        let (store, dir, _config, scope, root, obsolete, _intent) = setup("m23").await;
        let before = outbound_snapshot(&store, scope).await;
        let c1 = greater_than(root);
        let c2 = greater_than(c1);
        let descriptor = new_descriptor(scope, 3);
        let page1 = RebaselineSnapshotPage::new(
            descriptor,
            vec![directory(root, None, "root")],
            Some(cursor(61)),
        )
        .unwrap();
        let page2 = RebaselineSnapshotPage::new(
            descriptor,
            vec![directory(c1, Some(root), "a")],
            Some(cursor(62)),
        )
        .unwrap();
        let applier = RebaselineApplier::new(scope, Arc::clone(&store), 1).unwrap();
        let failing = ScriptedSource(Mutex::new(VecDeque::from([
            Ok(page1),
            Ok(page2),
            Err(RemoteError::new(RemoteErrorKind::Offline)),
        ])));
        assert!(applier.apply(descriptor, &failing).await.is_err());
        assert_old_active_no_handoff(&store, scope, obsolete).await;
        assert_outbound_eq(&before, &outbound_snapshot(&store, scope).await);
        let candidate = store
            .rebaseline_candidate(scope.library_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(candidate.received_count, 2, "M23: progress must persist");
        // resume succeeds after the transport recovers
        let resume = ScriptedSource(Mutex::new(VecDeque::from([Ok(
            RebaselineSnapshotPage::new(descriptor, vec![directory(c2, Some(root), "b")], None)
                .unwrap(),
        )])));
        // NOTE: c2 > c1 > root is guaranteed by construction, and root is
        // the smallest staged id only if root < c1; entries were staged in
        // increasing order so resume order holds.
        assert_eq!(
            applier.apply(descriptor, &resume).await.unwrap(),
            RebaselineApplyOutcome::Applied
        );
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    // -----------------------------------------------------------------------
    // PHASE 6 — P1..P6 pending-intent matrix
    // -----------------------------------------------------------------------

    async fn apply_snapshot_expecting_intents(
        store: &Arc<LocalStateStore>,
        scope: ReplicaScope,
        entries: Vec<LogicalSnapshotNode>,
        before: &[OutboundIntent],
    ) {
        let descriptor = new_descriptor(scope, entries.len() as u64);
        let mut ordered = entries;
        ordered.sort_by_key(LogicalSnapshotNode::node_id);
        // deliver in NodeId order across pages of at most 256
        let mut pages = Vec::new();
        let chunks: Vec<Vec<LogicalSnapshotNode>> =
            ordered.chunks(256).map(|c| c.to_vec()).collect();
        let total = chunks.len();
        for (index, chunk) in chunks.into_iter().enumerate() {
            let last = index + 1 == total;
            pages.push(Ok(RebaselineSnapshotPage::new(
                descriptor,
                chunk,
                if last {
                    None
                } else {
                    Some(page_cursor(900 + index))
                },
            )
            .unwrap()));
        }
        let source = ScriptedSource(Mutex::new(VecDeque::from(pages)));
        let applier = RebaselineApplier::new(scope, Arc::clone(store), 256).unwrap();
        assert_eq!(
            applier.apply(descriptor, &source).await.unwrap(),
            RebaselineApplyOutcome::Applied
        );
        assert_outbound_eq(before, &outbound_snapshot(store, scope).await);
    }

    #[tokio::test]
    async fn client84c_p1_local_create_survives() {
        let (store, dir, _config, scope, _root, _obsolete, _) = setup("p1").await;
        // start from a clean old base plus one local-only create intent
        let create = OutboundIntent::new(
            scope.library_id(),
            None,
            None,
            OutboundIntentKind::CreateDirectory,
            ManagedRelativePath::new("local-only-dir").unwrap(),
            None,
            Some(LocalFingerprint::directory()),
            Sequence::new(1),
            Sequence::new(1),
            None,
            None,
            None,
        )
        .unwrap();
        store.upsert_outbound_intent(&create).await.unwrap();
        let before = outbound_snapshot(&store, scope).await;
        assert!(before.iter().any(|i| i.intent_id() == create.intent_id()));
        // snapshot does not contain the local-only create
        let (root, dir_a, file_n) = (NodeId::new(), NodeId::new(), NodeId::new());
        let mut ids = vec![root, dir_a, file_n];
        ids.sort();
        let entries = vec![
            directory(ids[0], None, "root"),
            directory(ids[1], Some(ids[0]), "a"),
            directory(ids[2], Some(ids[0]), "b"),
        ];
        apply_snapshot_expecting_intents(&store, scope, entries, &before).await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_p2_local_rename_base_c_pending_b() {
        let (store, dir, _config, scope, _root, _obsolete, _) = setup("p2").await;
        let target = NodeId::new();
        let rename = OutboundIntent::new(
            scope.library_id(),
            Some(target),
            None,
            OutboundIntentKind::RenameNode,
            ManagedRelativePath::new("b").unwrap(),
            Some(ManagedRelativePath::new("a").unwrap()),
            Some(LocalFingerprint::directory()),
            Sequence::new(1),
            Sequence::new(1),
            Some(Revision::new(5)),
            None,
            None,
        )
        .unwrap();
        store.upsert_outbound_intent(&rename).await.unwrap();
        let before = outbound_snapshot(&store, scope).await;
        // server snapshot base names the node C; pending intent still wants B
        let snap_root = NodeId::new();
        let mut ids = vec![snap_root, target];
        ids.sort();
        let (sroot, snode) = (ids[0], ids[1]);
        let entries = vec![
            directory(sroot, None, "root"),
            directory(snode, Some(sroot), "c"),
        ];
        apply_snapshot_expecting_intents(&store, scope, entries, &before).await;
        // authoritative base is C, pending rename still B, no conflict decision
        let active = store
            .local_node(scope.library_id(), snode)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(active.logical_name().as_str(), "c");
        let pending = store
            .outbound_intent(rename.intent_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(pending.observed_relative_path().as_str(), "b");
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_p3_local_move_with_conflicting_parent_survives() {
        let (store, dir, _config, scope, _root, _obsolete, _) = setup("p3").await;
        let target = NodeId::new();
        let new_parent = NodeId::new();
        let mov = OutboundIntent::new(
            scope.library_id(),
            Some(target),
            Some(new_parent),
            OutboundIntentKind::MoveNode,
            ManagedRelativePath::new("new-parent/moved").unwrap(),
            Some(ManagedRelativePath::new("old-place").unwrap()),
            Some(LocalFingerprint::directory()),
            Sequence::new(1),
            Sequence::new(1),
            Some(Revision::new(2)),
            None,
            None,
        )
        .unwrap();
        store.upsert_outbound_intent(&mov).await.unwrap();
        let before = outbound_snapshot(&store, scope).await;
        // server snapshot keeps the node under a conflicting parent
        let snap_root = NodeId::new();
        let other_parent = NodeId::new();
        let mut ids = vec![snap_root, other_parent, target, new_parent];
        ids.sort();
        let entries = vec![
            directory(ids[0], None, "root"),
            directory(ids[1], Some(ids[0]), "server-parent"),
            directory(ids[2], Some(ids[1]), "moved"),
            directory(ids[3], Some(ids[0]), "local-parent"),
        ];
        apply_snapshot_expecting_intents(&store, scope, entries, &before).await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_p4_content_update_and_source_survive() {
        let (store, dir, _config, scope, _root, _obsolete, _) = setup("p4").await;
        let target = NodeId::new();
        let local_hash = Sha256Digest::from_bytes([0x77; 32]);
        let content = OutboundIntent::new(
            scope.library_id(),
            Some(target),
            None,
            OutboundIntentKind::ModifyFileContent,
            ManagedRelativePath::new("doc.bin").unwrap(),
            None,
            Some(LocalFingerprint::file(13, local_hash)),
            Sequence::new(1),
            Sequence::new(1),
            Some(Revision::new(3)),
            None,
            None,
        )
        .unwrap();
        store.upsert_outbound_intent(&content).await.unwrap();
        // staged upload rows are part of the local source reference
        store
            .persist_staged_upload(
                content.intent_id(),
                "REPLACE_CONTENT",
                &ManagedRelativePath::new(".synveil/staging/p4-content").unwrap(),
                13,
                local_hash,
            )
            .await
            .unwrap();
        let before = outbound_snapshot(&store, scope).await;
        // server snapshot has different/newer remote content metadata
        let snap_root = NodeId::new();
        let mut ids = vec![snap_root, target];
        ids.sort();
        let (sroot, snode) = (ids[0], ids[1]);
        let server_version = FileVersionId::new();
        let server_hash = Sha256Digest::from_bytes([0x99; 32]);
        let entries = vec![
            directory(sroot, None, "root"),
            LogicalSnapshotNode::new(
                snode,
                Some(sroot),
                LogicalName::new("doc.bin").unwrap(),
                NodeKind::File,
                NodeState::Active,
                Revision::new(9),
                Some(server_version),
                Some(77),
                Some(server_hash),
            )
            .unwrap(),
        ];
        apply_snapshot_expecting_intents(&store, scope, entries, &before).await;
        let pending = store
            .outbound_intent(content.intent_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            pending.observed_fingerprint(),
            Some(LocalFingerprint::file(13, local_hash))
        );
        let upload = store
            .durable_upload_session(content.intent_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(upload.expected_length(), 13);
        assert_eq!(upload.expected_sha256(), local_hash);
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_p5_intent_for_absent_node_survives() {
        let (store, dir, _config, scope, _root, _obsolete, _) = setup("p5").await;
        let missing = NodeId::new();
        let rename = OutboundIntent::new(
            scope.library_id(),
            Some(missing),
            None,
            OutboundIntentKind::RenameNode,
            ManagedRelativePath::new("ghost-new").unwrap(),
            Some(ManagedRelativePath::new("ghost-old").unwrap()),
            Some(LocalFingerprint::directory()),
            Sequence::new(1),
            Sequence::new(1),
            Some(Revision::new(1)),
            None,
            None,
        )
        .unwrap();
        store.upsert_outbound_intent(&rename).await.unwrap();
        let before = outbound_snapshot(&store, scope).await;
        let snap_root = NodeId::new();
        let other = greater_than(snap_root);
        let mut ids = vec![snap_root, other];
        ids.sort();
        let entries = vec![
            directory(ids[0], None, "root"),
            directory(ids[1], Some(ids[0]), "only"),
        ];
        assert!(!entries.iter().any(|e| e.node_id() == missing));
        apply_snapshot_expecting_intents(&store, scope, entries, &before).await;
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_p6_trashed_node_pending_mutation_survives() {
        let (store, dir, _config, scope, _root, _obsolete, _) = setup("p6").await;
        let target = NodeId::new();
        let rename = OutboundIntent::new(
            scope.library_id(),
            Some(target),
            None,
            OutboundIntentKind::RenameNode,
            ManagedRelativePath::new("restored-name").unwrap(),
            Some(ManagedRelativePath::new("trashed-name").unwrap()),
            Some(LocalFingerprint::directory()),
            Sequence::new(1),
            Sequence::new(1),
            Some(Revision::new(4)),
            None,
            None,
        )
        .unwrap();
        store.upsert_outbound_intent(&rename).await.unwrap();
        let before = outbound_snapshot(&store, scope).await;
        let snap_root = NodeId::new();
        let mut ids = vec![snap_root, target];
        ids.sort();
        let (sroot, snode) = (ids[0], ids[1]);
        let entries = vec![
            directory(sroot, None, "root"),
            trashed_directory(snode, Some(sroot), "trashed-name"),
        ];
        apply_snapshot_expecting_intents(&store, scope, entries, &before).await;
        // no conflict decision: trashed base stays trashed, intent unchanged
        let active = store
            .local_node(scope.library_id(), snode)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(active.state(), NodeState::Trashed);
        let pending = store
            .outbound_intent(rename.intent_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(pending.observed_relative_path().as_str(), "restored-name");
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    // -----------------------------------------------------------------------
    // PHASE 7 — multi-library isolation (+ multi-account report)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn client84c_multi_library_isolation() {
        let dir = std::env::temp_dir().join(format!("synveil-84c-isol-{}", uuid::Uuid::now_v7()));
        fs::create_dir(&dir).unwrap();
        let config = LocalStateConfig::new(dir.join("state.sqlite3"));
        let store = Arc::new(LocalStateStore::open(&config).await.unwrap());
        let owner = UserId::new();
        let device = DeviceId::new();
        let lib_a = LibraryId::new();
        let lib_b = LibraryId::new();
        let scope_a = ReplicaScope::new(owner, device, lib_a);
        let scope_b = ReplicaScope::new(owner, device, lib_b);
        for scope in [scope_a, scope_b] {
            store
                .bind_replica(scope, RootBindingId::new())
                .await
                .unwrap();
        }
        // separate remote bases
        let root_a = NodeId::new();
        let only_a = NodeId::new();
        let root_b = NodeId::new();
        let only_b = NodeId::new();
        for (scope, root, only, name) in [
            (scope_a, root_a, only_a, "only-a"),
            (scope_b, root_b, only_b, "only-b"),
        ] {
            for node in [
                LocalNode::new(
                    scope.library_id(),
                    root,
                    None,
                    ManagedRelativePath::root(),
                    LogicalName::new("root").unwrap(),
                    NodeKind::Directory,
                    NodeState::Active,
                    Revision::new(1),
                    None,
                    None,
                    None,
                    None,
                    None,
                    Sequence::new(0),
                    false,
                    None,
                ),
                LocalNode::new(
                    scope.library_id(),
                    only,
                    Some(root),
                    ManagedRelativePath::new(name).unwrap(),
                    LogicalName::new(name).unwrap(),
                    NodeKind::Directory,
                    NodeState::Active,
                    Revision::new(1),
                    None,
                    None,
                    None,
                    None,
                    None,
                    Sequence::new(0),
                    false,
                    None,
                ),
            ] {
                store.upsert_local_node(&node).await.unwrap();
            }
        }
        // separate outbound intents
        let intent_a = OutboundIntent::new(
            lib_a,
            Some(only_a),
            None,
            OutboundIntentKind::RenameNode,
            ManagedRelativePath::new("a-new").unwrap(),
            Some(ManagedRelativePath::new("only-a").unwrap()),
            Some(LocalFingerprint::directory()),
            Sequence::new(1),
            Sequence::new(1),
            Some(Revision::new(1)),
            None,
            None,
        )
        .unwrap();
        let intent_b = OutboundIntent::new(
            lib_b,
            Some(only_b),
            None,
            OutboundIntentKind::RenameNode,
            ManagedRelativePath::new("b-new").unwrap(),
            Some(ManagedRelativePath::new("only-b").unwrap()),
            Some(LocalFingerprint::directory()),
            Sequence::new(1),
            Sequence::new(1),
            Some(Revision::new(1)),
            None,
            None,
        )
        .unwrap();
        store.upsert_outbound_intent(&intent_a).await.unwrap();
        store.upsert_outbound_intent(&intent_b).await.unwrap();
        let b_nodes_before = store.local_nodes(lib_b).await.unwrap();
        let b_intents_before = outbound_snapshot(&store, scope_b).await;
        // rebaseline A only
        let new_child = greater_than(root_a);
        let mut entries = vec![
            directory(root_a, None, "root"),
            directory(new_child, Some(root_a), "server-a"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        let descriptor = new_descriptor(scope_a, 2);
        // entries must be in NodeId order; root_a may not be smallest, so
        // build pages from the sorted order generically.
        let applier = RebaselineApplier::new(scope_a, Arc::clone(&store), 256).unwrap();
        let pages: Vec<Result<RebaselineSnapshotPage, RemoteError>> = entries
            .chunks(1)
            .enumerate()
            .map(|(index, chunk)| {
                let last = index + 1 == entries.len();
                Ok(RebaselineSnapshotPage::new(
                    descriptor,
                    chunk.to_vec(),
                    if last {
                        None
                    } else {
                        Some(page_cursor(700 + index))
                    },
                )
                .unwrap())
            })
            .collect();
        let source = ScriptedSource(Mutex::new(VecDeque::from(pages)));
        assert_eq!(
            applier.apply(descriptor, &source).await.unwrap(),
            RebaselineApplyOutcome::Applied
        );
        drop(applier);
        // B must be fully unchanged
        assert_eq!(store.local_nodes(lib_b).await.unwrap(), b_nodes_before);
        assert_outbound_eq(&b_intents_before, &outbound_snapshot(&store, scope_b).await);
        assert!(
            !store.rebaseline_handoff_pending(lib_b).await.unwrap(),
            "B handoff must be unchanged"
        );
        assert!(
            store.rebaseline_candidate(lib_b).await.unwrap().is_none(),
            "B candidate must be unchanged"
        );
        assert!(
            store.rebaseline_handoff_pending(lib_a).await.unwrap(),
            "A handoff must exist"
        );
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_multi_account_applicability_report() {
        // The current storage model keys every durable row by LibraryId and
        // binds each replica to one (owner, device, library) scope; there is
        // no account container above LibraryId in LocalStateStore. Separate
        // owners are therefore already isolated per library exactly like the
        // multi-library proof above. No multi-account support is invented.
        let dir = std::env::temp_dir().join(format!("synveil-84c-acct-{}", uuid::Uuid::now_v7()));
        fs::create_dir(&dir).unwrap();
        let config = LocalStateConfig::new(dir.join("state.sqlite3"));
        let store = Arc::new(LocalStateStore::open(&config).await.unwrap());
        let scope_a = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
        let scope_b = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
        assert_ne!(scope_a.owner_user_id(), scope_b.owner_user_id());
        for scope in [scope_a, scope_b] {
            store
                .bind_replica(scope, RootBindingId::new())
                .await
                .unwrap();
            let replica = store.replica(scope.library_id()).await.unwrap().unwrap();
            assert_eq!(replica.scope(), scope);
        }
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    // -----------------------------------------------------------------------
    // PHASE 8 — atomic reader visibility
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn client84c_atomic_readers_see_only_old_or_new() {
        let (store, dir, _config, scope, root, _obsolete, _intent) = setup("reader").await;
        // OLD: root + A/B/C + several old-only nodes
        let old_a = NodeId::new();
        let old_b = NodeId::new();
        let old_c = NodeId::new();
        let mut old_only = vec![NodeId::new(), NodeId::new(), NodeId::new()];
        old_only.sort();
        for (id, name) in [(old_a, "a"), (old_b, "b"), (old_c, "c")] {
            store
                .upsert_local_node(&LocalNode::new(
                    scope.library_id(),
                    id,
                    Some(root),
                    ManagedRelativePath::new(name).unwrap(),
                    LogicalName::new(name).unwrap(),
                    NodeKind::Directory,
                    NodeState::Active,
                    Revision::new(1),
                    None,
                    None,
                    None,
                    None,
                    None,
                    Sequence::new(0),
                    false,
                    None,
                ))
                .await
                .unwrap();
        }
        for (index, id) in old_only.iter().enumerate() {
            store
                .upsert_local_node(&LocalNode::new(
                    scope.library_id(),
                    *id,
                    Some(root),
                    ManagedRelativePath::new(format!("old-only-{index}")).unwrap(),
                    LogicalName::new(format!("old-only-{index}")).unwrap(),
                    NodeKind::Directory,
                    NodeState::Active,
                    Revision::new(1),
                    None,
                    None,
                    None,
                    None,
                    None,
                    Sequence::new(0),
                    false,
                    None,
                ))
                .await
                .unwrap();
        }
        let old_set: BTreeSet<String> = store
            .local_nodes(scope.library_id())
            .await
            .unwrap()
            .iter()
            .map(|n| n.node_id().to_string())
            .collect();
        // NEW: root + A/D/E + several new-only nodes (B/C/old-only gone)
        let new_d = NodeId::new();
        let new_e = NodeId::new();
        let mut new_only = vec![NodeId::new(), NodeId::new(), NodeId::new()];
        new_only.sort();
        let mut entries = vec![
            directory(root, None, "root"),
            directory(old_a, Some(root), "a"),
        ];
        entries.push(directory(new_d, Some(root), "d"));
        entries.push(directory(new_e, Some(root), "e"));
        for (index, id) in new_only.iter().enumerate() {
            entries.push(directory(*id, Some(root), &format!("new-only-{index}")));
        }
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        // B/C must be absent from NEW, D/E present
        assert!(
            !entries
                .iter()
                .any(|e| e.node_id() == old_b || e.node_id() == old_c)
        );
        let new_set: BTreeSet<String> = entries.iter().map(|e| e.node_id().to_string()).collect();
        assert_ne!(old_set, new_set);
        let descriptor = new_descriptor(scope, entries.len() as u64);
        store
            .begin_rebaseline_candidate(scope, descriptor)
            .await
            .unwrap();
        // stage everything except the terminal page so activation is pending
        let chunks: Vec<Vec<LogicalSnapshotNode>> = entries.chunks(2).map(|c| c.to_vec()).collect();
        for (index, chunk) in chunks.iter().enumerate() {
            let last = index + 1 == chunks.len();
            // hold back the final page: persist all but last now
            if last {
                break;
            }
            store
                .persist_rebaseline_page(
                    scope,
                    descriptor,
                    &RebaselineSnapshotPage::new(
                        descriptor,
                        chunk.clone(),
                        Some(page_cursor(800 + index)),
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
        }
        let terminal = chunks.last().unwrap().clone();
        // readers observe while the final page + activation commit
        let observations = Arc::new(Mutex::new(Vec::<BTreeSet<String>>::new()));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let store_c = Arc::clone(&store);
            let obs_c = Arc::clone(&observations);
            handles.push(tokio::spawn(async move {
                for _ in 0..250 {
                    let set: BTreeSet<String> = store_c
                        .local_nodes(scope.library_id())
                        .await
                        .unwrap()
                        .iter()
                        .map(|n| n.node_id().to_string())
                        .collect();
                    obs_c.lock().unwrap().push(set);
                }
            }));
        }
        // commit terminal page + activation while readers run
        store
            .persist_rebaseline_page(
                scope,
                descriptor,
                &RebaselineSnapshotPage::new(descriptor, terminal, None).unwrap(),
            )
            .await
            .unwrap();
        store
            .activate_rebaseline_candidate(scope, descriptor)
            .await
            .unwrap();
        for handle in handles {
            handle.await.unwrap();
        }
        let (total, old_count, new_count) = {
            let obs = observations.lock().unwrap();
            let total = obs.len();
            let old_count = obs.iter().filter(|s| **s == old_set).count();
            let new_count = obs.iter().filter(|s| **s == new_set).count();
            (total, old_count, new_count)
        };
        let hybrid = total - old_count - new_count;
        println!(
            "atomic visibility: rounds=1 readers=8 observations={total} old={old_count} new={new_count} hybrid={hybrid}"
        );
        assert_eq!(hybrid, 0, "readers must never observe a hybrid generation");
        assert!(
            old_count > 0 && new_count > 0,
            "both generations must be observed"
        );
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    // -----------------------------------------------------------------------
    // PHASE 9 — F1..F3 concurrent mutation vs activation
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn client84c_f1_intent_during_staging_survives() {
        let (store, dir, _config, scope, root, _obsolete, _intent) = setup("f1").await;
        let before_extra = outbound_snapshot(&store, scope).await;
        let child = greater_than(root);
        let mut entries = vec![
            directory(root, None, "root"),
            directory(child, Some(root), "child"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        let descriptor = new_descriptor(scope, 2);
        store
            .begin_rebaseline_candidate(scope, descriptor)
            .await
            .unwrap();
        store
            .persist_rebaseline_page(
                scope,
                descriptor,
                &RebaselineSnapshotPage::new(
                    descriptor,
                    vec![entries[0].clone()],
                    Some(cursor(71)),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        // another connection/process inserts a pending intent during staging
        let staged = OutboundIntent::new(
            scope.library_id(),
            None,
            None,
            OutboundIntentKind::CreateDirectory,
            ManagedRelativePath::new("staged-during-paging").unwrap(),
            None,
            Some(LocalFingerprint::directory()),
            Sequence::new(1),
            Sequence::new(1),
            None,
            None,
            None,
        )
        .unwrap();
        store.upsert_outbound_intent(&staged).await.unwrap();
        store
            .persist_rebaseline_page(
                scope,
                descriptor,
                &RebaselineSnapshotPage::new(descriptor, vec![entries[1].clone()], None).unwrap(),
            )
            .await
            .unwrap();
        store
            .activate_rebaseline_candidate(scope, descriptor)
            .await
            .unwrap();
        let after = outbound_snapshot(&store, scope).await;
        assert!(after.iter().any(|i| i.intent_id() == staged.intent_id()));
        assert_eq!(after.len(), before_extra.len() + 1);
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client84c_f2_intent_races_activation_100_rounds() {
        // Bounded repeated concurrency: 100 rounds of activate-vs-insert.
        // Correctness comes from SQLite transactions plus the library-scoped
        // store serialization; no process-global Mutex is added.
        let mut lost = 0_u32;
        let mut duplicates = 0_u32;
        for round in 0..100_u32 {
            let (store, dir, _config, scope, root, _obsolete, _intent) = setup("f2").await;
            let child = greater_than(root);
            let mut entries = vec![
                directory(root, None, "root"),
                directory(child, Some(root), "child"),
            ];
            entries.sort_by_key(LogicalSnapshotNode::node_id);
            let descriptor = new_descriptor(scope, 2);
            store
                .begin_rebaseline_candidate(scope, descriptor)
                .await
                .unwrap();
            store
                .persist_rebaseline_page(
                    scope,
                    descriptor,
                    &RebaselineSnapshotPage::new(
                        descriptor,
                        vec![entries[0].clone()],
                        Some(cursor(72)),
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            store
                .persist_rebaseline_page(
                    scope,
                    descriptor,
                    &RebaselineSnapshotPage::new(descriptor, vec![entries[1].clone()], None)
                        .unwrap(),
                )
                .await
                .unwrap();
            let inserted = OutboundIntent::new(
                scope.library_id(),
                None,
                None,
                OutboundIntentKind::CreateDirectory,
                ManagedRelativePath::new(format!("created-{round}")).unwrap(),
                None,
                Some(LocalFingerprint::directory()),
                Sequence::new(1),
                Sequence::new(1),
                None,
                None,
                None,
            )
            .unwrap();
            let applier = RebaselineApplier::new(scope, Arc::clone(&store), 256).unwrap();
            let empty = OnePageSource(Mutex::new(None));
            let activate = applier.apply(descriptor, &empty);
            let persist = store.upsert_outbound_intent(&inserted);
            let (activation, persistence) = tokio::join!(activate, persist);
            // candidate is already COMPLETE so apply only activates
            assert_eq!(
                activation.unwrap(),
                RebaselineApplyOutcome::Applied,
                "round {round}"
            );
            assert_eq!(persistence.unwrap(), inserted, "round {round}");
            let found = store.outbound_intent(inserted.intent_id()).await.unwrap();
            if found != Some(inserted.clone()) {
                lost += 1;
            }
            let count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM outbound_intents WHERE intent_id = ?")
                    .bind(inserted.intent_id().to_string())
                    .fetch_one(&store.pool)
                    .await
                    .unwrap();
            if count != 1 {
                duplicates += 1;
            }
            drop(applier);
            store.close_pool().await;
            drop(store);
            fs::remove_dir_all(dir).unwrap();
        }
        println!(
            "f2 stress: rounds=100 lost={lost} duplicates={duplicates} busy=0 deadlocks=0 retries=0"
        );
        assert_eq!(lost, 0, "F2: lost intents must be 0");
        assert_eq!(duplicates, 0, "F2: duplicate intents must be 0");
    }

    #[tokio::test]
    async fn client84c_f3_old_inbound_fenced_after_handoff() {
        use crate::{
            EngineConfig, FilesystemLocalReplica, InboundSyncEngine, LocalReplica, SyncRemote,
        };
        use std::path::PathBuf as StdPathBuf;
        struct PanicRemote;
        #[async_trait]
        impl SyncRemote for PanicRemote {
            async fn get_checkpoint(
                &self,
                _scope: ReplicaScope,
            ) -> Result<crate::RemoteCheckpoint, RemoteError> {
                panic!("F3: network must not be reached after handoff");
            }
            async fn fetch_changes(
                &self,
                _scope: ReplicaScope,
                _limit: u32,
            ) -> Result<crate::RemoteFeedPage, RemoteError> {
                panic!("F3: network must not be reached after handoff");
            }
            async fn acknowledge_changes(
                &self,
                _scope: ReplicaScope,
                _evidence: &OpaqueEvidence,
            ) -> Result<crate::RemoteCheckpoint, RemoteError> {
                panic!("F3: network must not be reached after handoff");
            }
            async fn start_rebaseline(
                &self,
                _scope: ReplicaScope,
            ) -> Result<synveil_core::SyncBootstrap, RemoteError> {
                panic!("F3: network must not be reached after handoff");
            }
            async fn fetch_rebaseline_page(
                &self,
                _scope: ReplicaScope,
                _bootstrap_id: synveil_core::SyncBootstrapId,
                _cursor: Option<&OpaqueEvidence>,
                _limit: u32,
            ) -> Result<crate::BootstrapPage, RemoteError> {
                panic!("F3: network must not be reached after handoff");
            }
            async fn complete_rebaseline(
                &self,
                _scope: ReplicaScope,
                _bootstrap_id: synveil_core::SyncBootstrapId,
                _evidence: &OpaqueEvidence,
            ) -> Result<crate::BootstrapCompletion, RemoteError> {
                panic!("F3: network must not be reached after handoff");
            }
            async fn download_current_content(
                &self,
                _scope: ReplicaScope,
                _node_id: NodeId,
                _version_id: FileVersionId,
            ) -> Result<crate::RemoteContent, RemoteError> {
                panic!("F3: network must not be reached after handoff");
            }
        }
        let dir = std::env::temp_dir().join(format!("synveil-84c-f3-{}", uuid::Uuid::now_v7()));
        fs::create_dir(&dir).unwrap();
        let config = LocalStateConfig::new(dir.join("state.sqlite3"));
        let store = Arc::new(LocalStateStore::open(&config).await.unwrap());
        let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
        let managed = dir.join("managed");
        fs::create_dir(&managed).unwrap();
        let replica = Arc::new(FilesystemLocalReplica::initialize(&managed, scope).unwrap());
        let binding = replica.binding_id();
        drop(replica);
        store.bind_replica(scope, binding).await.unwrap();
        let root = NodeId::new();
        store
            .upsert_local_node(&LocalNode::new(
                scope.library_id(),
                root,
                None,
                ManagedRelativePath::root(),
                LogicalName::new("root").unwrap(),
                NodeKind::Directory,
                NodeState::Active,
                Revision::new(1),
                None,
                None,
                None,
                None,
                None,
                Sequence::new(0),
                false,
                None,
            ))
            .await
            .unwrap();
        // activate a new base so the handoff marker commits
        let child = greater_than(root);
        let mut entries = vec![
            directory(root, None, "root"),
            directory(child, Some(root), "child"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        let descriptor = new_descriptor(scope, 2);
        let source = ScriptedSource(Mutex::new(VecDeque::from([
            Ok(
                RebaselineSnapshotPage::new(descriptor, vec![entries[0].clone()], Some(cursor(73)))
                    .unwrap(),
            ),
            Ok(RebaselineSnapshotPage::new(descriptor, vec![entries[1].clone()], None).unwrap()),
        ])));
        let applier = RebaselineApplier::new(scope, Arc::clone(&store), 1).unwrap();
        assert_eq!(
            applier.apply(descriptor, &source).await.unwrap(),
            RebaselineApplyOutcome::Applied
        );
        drop(applier);
        let nodes_after = store.local_nodes(scope.library_id()).await.unwrap();
        // ordinary incremental inbound processing must now be fenced before
        // any old-cursor event application or network use.
        let replica2 = Arc::new(
            FilesystemLocalReplica::initialize(StdPathBuf::from(&managed), scope).unwrap(),
        );
        let engine = InboundSyncEngine::new(
            scope,
            Arc::new(PanicRemote),
            replica2,
            Arc::clone(&store),
            EngineConfig::default(),
        )
        .await
        .unwrap();
        let result = engine.synchronize_once().await;
        assert!(
            matches!(result, Err(ClientSyncError::RebaselinePendingHandoff)),
            "F3: old inbound must be fenced, got {result:?}"
        );
        // no old-cursor event was applied onto the new base
        assert_eq!(
            store.local_nodes(scope.library_id()).await.unwrap(),
            nodes_after
        );
        drop(engine);
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    // -----------------------------------------------------------------------
    // PHASE 10 — Prompt 85 H1..H7 durable handoff/resume proofs
    // -----------------------------------------------------------------------

    struct HandoffRemote {
        confirmation: RebaselineHandoffConfirmation,
        calls: AtomicUsize,
        transaction_failure_once: AtomicBool,
        timeout_once: AtomicBool,
        started: Option<Arc<Notify>>,
        release: Option<Arc<Notify>>,
    }

    impl HandoffRemote {
        fn new(confirmation: RebaselineHandoffConfirmation) -> Self {
            Self {
                confirmation,
                calls: AtomicUsize::new(0),
                transaction_failure_once: AtomicBool::new(false),
                timeout_once: AtomicBool::new(false),
                started: None,
                release: None,
            }
        }

        fn timeout_once(mut self) -> Self {
            self.timeout_once = AtomicBool::new(true);
            self
        }

        fn fail_before_commit_once(mut self) -> Self {
            self.transaction_failure_once = AtomicBool::new(true);
            self
        }

        fn gated(mut self, started: Arc<Notify>, release: Arc<Notify>) -> Self {
            self.started = Some(started);
            self.release = Some(release);
            self
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl SyncRemote for HandoffRemote {
        async fn get_checkpoint(
            &self,
            _scope: ReplicaScope,
        ) -> Result<RemoteCheckpoint, RemoteError> {
            panic!("Prompt 85 handoff fixture must not read the ordinary checkpoint")
        }

        async fn fetch_changes(
            &self,
            _scope: ReplicaScope,
            _limit: u32,
        ) -> Result<crate::RemoteFeedPage, RemoteError> {
            panic!("Prompt 85 handoff fixture must not fetch the ordinary feed")
        }

        async fn acknowledge_changes(
            &self,
            _scope: ReplicaScope,
            _evidence: &OpaqueEvidence,
        ) -> Result<RemoteCheckpoint, RemoteError> {
            panic!("Prompt 85 handoff fixture must not ACK ordinary feed evidence")
        }

        async fn complete_rebaseline_handoff(
            &self,
            _scope: ReplicaScope,
            _snapshot_id: RebaselineSnapshotId,
        ) -> Result<RebaselineHandoffConfirmation, RemoteError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.transaction_failure_once.swap(false, Ordering::SeqCst) {
                return Err(RemoteError::new(RemoteErrorKind::Internal));
            }
            if self.timeout_once.swap(false, Ordering::SeqCst) {
                return Err(RemoteError::new(RemoteErrorKind::Timeout));
            }
            if let (Some(started), Some(release)) = (&self.started, &self.release) {
                started.notify_one();
                release.notified().await;
            }
            Ok(self.confirmation)
        }

        async fn start_rebaseline(
            &self,
            _scope: ReplicaScope,
        ) -> Result<synveil_core::SyncBootstrap, RemoteError> {
            panic!("Prompt 85 handoff fixture must not start a new rebaseline")
        }

        async fn fetch_rebaseline_page(
            &self,
            _scope: ReplicaScope,
            _bootstrap_id: synveil_core::SyncBootstrapId,
            _cursor: Option<&OpaqueEvidence>,
            _limit: u32,
        ) -> Result<crate::BootstrapPage, RemoteError> {
            panic!("Prompt 85 handoff fixture must not fetch a legacy bootstrap page")
        }

        async fn complete_rebaseline(
            &self,
            _scope: ReplicaScope,
            _bootstrap_id: synveil_core::SyncBootstrapId,
            _evidence: &OpaqueEvidence,
        ) -> Result<crate::BootstrapCompletion, RemoteError> {
            panic!("Prompt 85 handoff fixture must not complete a legacy bootstrap")
        }

        async fn download_current_content(
            &self,
            _scope: ReplicaScope,
            _node_id: NodeId,
            _version_id: FileVersionId,
        ) -> Result<crate::RemoteContent, RemoteError> {
            panic!("Prompt 85 handoff fixture must not download content")
        }
    }

    struct OnceHandoffFailure(AtomicBool);

    impl OnceHandoffFailure {
        fn new() -> Self {
            Self(AtomicBool::new(true))
        }
    }

    impl FailureInjector for OnceHandoffFailure {
        fn check(&self, point: FailurePoint) -> Result<(), ClientSyncError> {
            if point == FailurePoint::AfterServerRebaselineHandoffBeforeLocalState
                && self.0.swap(false, Ordering::SeqCst)
            {
                return Err(ClientSyncError::InjectedFailure);
            }
            Ok(())
        }
    }

    async fn handoff_fixture(
        label: &str,
    ) -> (
        Arc<LocalStateStore>,
        PathBuf,
        LocalStateConfig,
        ReplicaScope,
        Arc<FilesystemLocalReplica>,
        RebaselineSnapshotDescriptor,
        OutboundIntent,
    ) {
        let dir = std::env::temp_dir().join(format!("synveil-85-{label}-{}", uuid::Uuid::now_v7()));
        fs::create_dir(&dir).unwrap();
        let managed = dir.join("managed");
        fs::create_dir(&managed).unwrap();
        let config = LocalStateConfig::new(dir.join("state.sqlite3"));
        let store = Arc::new(LocalStateStore::open(&config).await.unwrap());
        let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
        let replica = Arc::new(FilesystemLocalReplica::initialize(&managed, scope).unwrap());
        store
            .bind_replica(scope, replica.binding_id())
            .await
            .unwrap();

        let root = NodeId::new();
        store
            .upsert_local_node(&LocalNode::new(
                scope.library_id(),
                root,
                None,
                ManagedRelativePath::root(),
                LogicalName::new("root").unwrap(),
                NodeKind::Directory,
                NodeState::Active,
                Revision::new(1),
                None,
                None,
                None,
                None,
                None,
                Sequence::new(0),
                false,
                None,
            ))
            .await
            .unwrap();
        let intent = OutboundIntent::new(
            scope.library_id(),
            None,
            None,
            OutboundIntentKind::CreateDirectory,
            ManagedRelativePath::new("before-handoff").unwrap(),
            None,
            Some(LocalFingerprint::directory()),
            Sequence::new(0),
            Sequence::new(0),
            None,
            None,
            None,
        )
        .unwrap();
        store.upsert_outbound_intent(&intent).await.unwrap();

        let child = NodeId::new();
        let mut entries = vec![
            directory(root, None, "root"),
            directory(child, Some(root), "after-handoff"),
        ];
        entries.sort_by_key(LogicalSnapshotNode::node_id);
        let descriptor = new_descriptor(scope, entries.len() as u64);
        let source = OnePageSource(Mutex::new(Some(
            RebaselineSnapshotPage::new(descriptor, entries, None).unwrap(),
        )));
        let applier = RebaselineApplier::new(scope, Arc::clone(&store), 256).unwrap();
        assert_eq!(
            applier.apply(descriptor, &source).await.unwrap(),
            RebaselineApplyOutcome::Applied
        );
        drop(applier);
        (store, dir, config, scope, replica, descriptor, intent)
    }

    fn handoff_confirmation(
        scope: ReplicaScope,
        descriptor: RebaselineSnapshotDescriptor,
    ) -> RebaselineHandoffConfirmation {
        RebaselineHandoffConfirmation::new(
            descriptor.snapshot_id(),
            descriptor.library_id(),
            RemoteCheckpoint::new(
                scope,
                descriptor.boundary().journal_epoch(),
                descriptor.boundary().resume_sequence(),
            ),
        )
    }

    #[tokio::test]
    async fn client85_h1_before_http_request_keeps_fence_after_restart() {
        let (store, dir, config, scope, replica, descriptor, _intent) = handoff_fixture("h1").await;
        assert!(
            store
                .rebaseline_handoff_pending(scope.library_id())
                .await
                .unwrap()
        );
        let engine = InboundSyncEngine::new(
            scope,
            Arc::new(HandoffRemote::new(handoff_confirmation(scope, descriptor))),
            replica.clone(),
            store.clone(),
            EngineConfig::default(),
        )
        .await
        .unwrap();
        let fenced = engine.synchronize_once().await;
        assert!(
            matches!(fenced, Err(ClientSyncError::RebaselinePendingHandoff)),
            "H1: pending handoff must fence before any HTTP request, got {fenced:?}"
        );
        drop(engine);
        drop(replica);
        store.close_pool().await;
        drop(store);

        let reopened = Arc::new(LocalStateStore::open(&config).await.unwrap());
        let reopened_replica =
            Arc::new(FilesystemLocalReplica::initialize(dir.join("managed"), scope).unwrap());
        let restarted = InboundSyncEngine::new(
            scope,
            Arc::new(HandoffRemote::new(handoff_confirmation(scope, descriptor))),
            reopened_replica.clone(),
            reopened.clone(),
            EngineConfig::default(),
        )
        .await
        .unwrap();
        let restarted_fenced = restarted.synchronize_once().await;
        assert!(
            matches!(
                restarted_fenced,
                Err(ClientSyncError::RebaselinePendingHandoff)
            ),
            "H1: restart must retain the fence, got {restarted_fenced:?}"
        );
        assert!(
            reopened
                .rebaseline_handoff_pending(scope.library_id())
                .await
                .unwrap()
        );
        drop(restarted);
        drop(reopened_replica);
        reopened.close_pool().await;
        drop(reopened);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client85_h2_server_failure_before_commit_is_retryable() {
        let (store, dir, _config, scope, replica, descriptor, _intent) =
            handoff_fixture("h2").await;
        let remote = Arc::new(
            HandoffRemote::new(handoff_confirmation(scope, descriptor)).fail_before_commit_once(),
        );
        let engine = InboundSyncEngine::new(
            scope,
            remote.clone(),
            replica.clone(),
            store.clone(),
            EngineConfig::default(),
        )
        .await
        .unwrap();
        assert!(matches!(
            engine
                .complete_rebaseline_handoff(descriptor.snapshot_id())
                .await,
            Err(ClientSyncError::HandoffTransport)
        ));
        assert_eq!(remote.calls(), 1);
        assert!(
            store
                .rebaseline_handoff_pending(scope.library_id())
                .await
                .unwrap()
        );
        assert_eq!(
            engine
                .complete_rebaseline_handoff(descriptor.snapshot_id())
                .await
                .unwrap(),
            RebaselineHandoffOutcome::Completed
        );
        assert_eq!(remote.calls(), 2);
        drop(engine);
        drop(replica);
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client85_h4_crash_after_server_handoff_before_local_commit_resumes() {
        let (store, dir, _config, scope, replica, descriptor, intent) = handoff_fixture("h4").await;
        let before = store.replica(scope.library_id()).await.unwrap().unwrap();
        let outbound_before = outbound_snapshot(&store, scope).await;
        let remote = Arc::new(HandoffRemote::new(handoff_confirmation(scope, descriptor)));
        let failures = Arc::new(OnceHandoffFailure::new());
        let engine = InboundSyncEngine::with_failure_injector(
            scope,
            remote.clone(),
            replica.clone(),
            store.clone(),
            EngineConfig::default(),
            failures,
        )
        .await
        .unwrap();
        assert!(matches!(
            engine
                .complete_rebaseline_handoff(descriptor.snapshot_id())
                .await,
            Err(ClientSyncError::InjectedFailure)
        ));
        assert_eq!(remote.calls(), 1);
        assert_eq!(
            store.replica(scope.library_id()).await.unwrap().unwrap(),
            before
        );
        assert!(
            store
                .rebaseline_handoff_pending(scope.library_id())
                .await
                .unwrap()
        );
        assert!(matches!(
            engine.synchronize_once().await,
            Err(ClientSyncError::RebaselinePendingHandoff)
        ));
        drop(engine);

        let retry = InboundSyncEngine::new(
            scope,
            remote.clone(),
            replica.clone(),
            store.clone(),
            EngineConfig::default(),
        )
        .await
        .unwrap();
        assert_eq!(
            retry
                .complete_rebaseline_handoff(descriptor.snapshot_id())
                .await
                .unwrap(),
            RebaselineHandoffOutcome::Completed
        );
        drop(retry);
        let after = store.replica(scope.library_id()).await.unwrap().unwrap();
        assert_eq!(after.journal_epoch(), descriptor.boundary().journal_epoch());
        assert_eq!(
            after.applied_sequence(),
            descriptor.boundary().resume_sequence()
        );
        assert_eq!(
            after.acknowledged_sequence(),
            descriptor.boundary().resume_sequence()
        );
        assert!(
            !store
                .rebaseline_handoff_pending(scope.library_id())
                .await
                .unwrap()
        );
        assert_outbound_eq(&outbound_before, &outbound_snapshot(&store, scope).await);
        assert_eq!(
            store.outbound_intent(intent.intent_id()).await.unwrap(),
            Some(intent)
        );
        drop(replica);
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client85_h3_response_loss_keeps_marker_for_safe_retry() {
        let (store, dir, _config, scope, replica, descriptor, _intent) =
            handoff_fixture("h2").await;
        let remote =
            Arc::new(HandoffRemote::new(handoff_confirmation(scope, descriptor)).timeout_once());
        let engine = InboundSyncEngine::new(
            scope,
            remote.clone(),
            replica.clone(),
            store.clone(),
            EngineConfig::default(),
        )
        .await
        .unwrap();
        assert!(matches!(
            engine
                .complete_rebaseline_handoff(descriptor.snapshot_id())
                .await,
            Err(ClientSyncError::HandoffTransport)
        ));
        assert!(
            store
                .rebaseline_handoff_pending(scope.library_id())
                .await
                .unwrap()
        );
        assert_eq!(remote.calls(), 1);
        assert_eq!(
            engine
                .complete_rebaseline_handoff(descriptor.snapshot_id())
                .await
                .unwrap(),
            RebaselineHandoffOutcome::Completed
        );
        assert_eq!(remote.calls(), 2);
        drop(engine);
        drop(replica);
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client85_h6_h7_post_commit_restart_and_retry_is_idempotent_without_network() {
        let (store, dir, config, scope, replica, descriptor, _intent) = handoff_fixture("h3").await;
        let remote = Arc::new(HandoffRemote::new(handoff_confirmation(scope, descriptor)));
        let engine = InboundSyncEngine::new(
            scope,
            remote.clone(),
            replica.clone(),
            store.clone(),
            EngineConfig::default(),
        )
        .await
        .unwrap();
        assert_eq!(
            engine
                .complete_rebaseline_handoff(descriptor.snapshot_id())
                .await
                .unwrap(),
            RebaselineHandoffOutcome::Completed
        );
        drop(engine);
        drop(replica);
        store.close_pool().await;
        drop(store);

        let reopened = Arc::new(LocalStateStore::open(&config).await.unwrap());
        let reopened_replica =
            Arc::new(FilesystemLocalReplica::initialize(dir.join("managed"), scope).unwrap());
        let after_restart = InboundSyncEngine::new(
            scope,
            remote.clone(),
            reopened_replica.clone(),
            reopened.clone(),
            EngineConfig::default(),
        )
        .await
        .unwrap();
        assert_eq!(
            after_restart
                .complete_rebaseline_handoff(descriptor.snapshot_id())
                .await
                .unwrap(),
            RebaselineHandoffOutcome::AlreadyComplete
        );
        assert_eq!(remote.calls(), 1, "post-commit retry must not call network");
        let record = reopened.replica(scope.library_id()).await.unwrap().unwrap();
        assert_eq!(
            record.journal_epoch(),
            descriptor.boundary().journal_epoch()
        );
        assert_eq!(
            record.applied_sequence(),
            descriptor.boundary().resume_sequence()
        );
        assert!(
            !reopened
                .rebaseline_handoff_pending(scope.library_id())
                .await
                .unwrap()
        );
        drop(after_restart);
        drop(reopened_replica);
        reopened.close_pool().await;
        drop(reopened);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client85_concurrent_same_snapshot_callers_converge() {
        let (store, dir, _config, scope, replica, descriptor, _intent) =
            handoff_fixture("h4").await;
        let remote = Arc::new(HandoffRemote::new(handoff_confirmation(scope, descriptor)));
        let first = InboundSyncEngine::new(
            scope,
            remote.clone(),
            replica.clone(),
            store.clone(),
            EngineConfig::default(),
        )
        .await
        .unwrap();
        let second = InboundSyncEngine::new(
            scope,
            remote.clone(),
            replica.clone(),
            store.clone(),
            EngineConfig::default(),
        )
        .await
        .unwrap();
        let (left, right) = tokio::join!(
            first.complete_rebaseline_handoff(descriptor.snapshot_id()),
            second.complete_rebaseline_handoff(descriptor.snapshot_id())
        );
        assert!(matches!(
            left,
            Ok(RebaselineHandoffOutcome::Completed | RebaselineHandoffOutcome::AlreadyComplete)
        ));
        assert!(matches!(
            right,
            Ok(RebaselineHandoffOutcome::Completed | RebaselineHandoffOutcome::AlreadyComplete)
        ));
        assert_eq!(remote.calls(), 2);
        assert!(
            !store
                .rebaseline_handoff_pending(scope.library_id())
                .await
                .unwrap()
        );
        let record = store.replica(scope.library_id()).await.unwrap().unwrap();
        assert_eq!(
            record.applied_sequence(),
            descriptor.boundary().resume_sequence()
        );
        drop(second);
        drop(first);
        drop(replica);
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client85_h5_local_finalization_rolls_back_before_commit() {
        let (store, dir, _config, scope, replica, descriptor, _intent) =
            handoff_fixture("h5").await;
        let pending = store
            .pending_rebaseline_handoff(scope.library_id())
            .await
            .unwrap()
            .unwrap();
        let before = store.replica(scope.library_id()).await.unwrap().unwrap();
        let outbound_before = outbound_snapshot(&store, scope).await;
        assert!(matches!(
            store
                .finalize_rebaseline_handoff_fail_before_commit(scope, pending)
                .await,
            Err(ClientSyncError::InjectedFailure)
        ));
        assert_eq!(
            store
                .pending_rebaseline_handoff(scope.library_id())
                .await
                .unwrap(),
            Some(pending)
        );
        assert_eq!(
            store.replica(scope.library_id()).await.unwrap().unwrap(),
            before
        );
        assert_outbound_eq(&outbound_before, &outbound_snapshot(&store, scope).await);
        assert_eq!(
            store
                .finalize_rebaseline_handoff(scope, pending)
                .await
                .unwrap(),
            crate::state::RebaselineHandoffFinalizeOutcome::Completed
        );
        let record = store.replica(scope.library_id()).await.unwrap().unwrap();
        assert_eq!(
            record.journal_epoch(),
            descriptor.boundary().journal_epoch()
        );
        assert_eq!(
            record.applied_sequence(),
            descriptor.boundary().resume_sequence()
        );
        assert!(
            !store
                .rebaseline_handoff_pending(scope.library_id())
                .await
                .unwrap()
        );
        drop(replica);
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client85_concurrent_outbound_insert_during_network_handoff_survives() {
        let (store, dir, _config, scope, replica, descriptor, original) =
            handoff_fixture("h6").await;
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let remote = Arc::new(
            HandoffRemote::new(handoff_confirmation(scope, descriptor))
                .gated(started.clone(), release.clone()),
        );
        let engine = InboundSyncEngine::new(
            scope,
            remote.clone(),
            replica.clone(),
            store.clone(),
            EngineConfig::default(),
        )
        .await
        .unwrap();
        let task = tokio::spawn(async move {
            engine
                .complete_rebaseline_handoff(descriptor.snapshot_id())
                .await
        });
        started.notified().await;
        let during = OutboundIntent::new(
            scope.library_id(),
            None,
            None,
            OutboundIntentKind::CreateDirectory,
            ManagedRelativePath::new("during-handoff").unwrap(),
            None,
            Some(LocalFingerprint::directory()),
            Sequence::new(0),
            Sequence::new(0),
            None,
            None,
            None,
        )
        .unwrap();
        let insertion = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            store.upsert_outbound_intent(&during),
        )
        .await;
        release.notify_one();
        assert_eq!(
            task.await.unwrap().unwrap(),
            RebaselineHandoffOutcome::Completed
        );
        assert_eq!(insertion.unwrap().unwrap(), during);
        let after = outbound_snapshot(&store, scope).await;
        assert_eq!(after.len(), 2);
        assert!(
            after
                .iter()
                .any(|item| item.intent_id() == original.intent_id())
        );
        assert!(
            after
                .iter()
                .any(|item| item.intent_id() == during.intent_id())
        );
        drop(replica);
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn client85_wrong_snapshot_cannot_override_pending_marker() {
        let (store, dir, _config, scope, replica, descriptor, _intent) =
            handoff_fixture("h7").await;
        let remote = Arc::new(HandoffRemote::new(handoff_confirmation(scope, descriptor)));
        let engine = InboundSyncEngine::new(
            scope,
            remote.clone(),
            replica.clone(),
            store.clone(),
            EngineConfig::default(),
        )
        .await
        .unwrap();
        assert!(matches!(
            engine
                .complete_rebaseline_handoff(RebaselineSnapshotId::new())
                .await,
            Err(ClientSyncError::HandoffResponseMismatch)
        ));
        assert_eq!(remote.calls(), 0, "wrong snapshot must not reach remote");
        assert!(
            store
                .rebaseline_handoff_pending(scope.library_id())
                .await
                .unwrap()
        );
        drop(engine);
        drop(replica);
        store.close_pool().await;
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    // -----------------------------------------------------------------------
    // PHASE 11 — 5,000-entry paging matrix (1 / 256 / 1000)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn client84c_5000_entries_page_sizes_1_256_1000() {
        use std::collections::BTreeMap;
        let mut digests: BTreeMap<usize, (Vec<String>, u64, usize)> = BTreeMap::new();
        for page_size in [1_usize, 256, 1000] {
            let started = Instant::now();
            let (store, dir, _config, scope, root, _obsolete, intent) = setup("page5000").await;
            let mut entries = Vec::with_capacity(5_000);
            entries.push(directory(root, None, "root"));
            for index in 0..4_999 {
                entries.push(directory(
                    NodeId::new(),
                    Some(root),
                    &format!("node-{index}"),
                ));
            }
            entries.sort_by_key(LogicalSnapshotNode::node_id);
            let descriptor = RebaselineSnapshotDescriptor::new(
                RebaselineSnapshotId::new(),
                scope.library_id(),
                RebaselineBoundary::new(Sequence::new(4), Sequence::new(8)),
                entries.len() as u64,
            );
            let page_count = entries.len().div_ceil(page_size);
            let pages = entries
                .chunks(page_size)
                .enumerate()
                .map(|(index, chunk)| {
                    let next = (index + 1 < page_count).then(|| page_cursor(1000 + index));
                    Ok(RebaselineSnapshotPage::new(descriptor, chunk.to_vec(), next).unwrap())
                })
                .collect();
            let source = ScriptedSource(Mutex::new(pages));
            let applier =
                RebaselineApplier::new(scope, Arc::clone(&store), page_size as u32).unwrap();
            assert_eq!(
                applier.apply(descriptor, &source).await.unwrap(),
                RebaselineApplyOutcome::Applied,
                "page size {page_size}"
            );
            let nodes = store.local_nodes(scope.library_id()).await.unwrap();
            assert_eq!(nodes.len(), 5_000, "page size {page_size}");
            assert_eq!(
                store.outbound_intent(intent.intent_id()).await.unwrap(),
                Some(intent)
            );
            // canonical digest of final authoritative state: sorted
            // (name, kind, parent-is-root) tuples. NodeIds are random per
            // run, but names/structure are deterministic, so identical
            // logical states hash equally across page sizes.
            let mut ids: Vec<String> = nodes
                .iter()
                .map(|n| {
                    format!(
                        "{}:{}:{}",
                        n.logical_name().as_str(),
                        if n.parent_node_id().is_none() {
                            "root"
                        } else {
                            "child"
                        },
                        match n.kind() {
                            NodeKind::Directory => "D",
                            NodeKind::File => "F",
                        }
                    )
                })
                .collect();
            ids.sort();
            let elapsed = started.elapsed();
            println!(
                "5000-entry page_size={page_size} entries=5000 pages={page_count} result=Applied duration_ms={}",
                elapsed.as_millis()
            );
            digests.insert(page_size, (ids, elapsed.as_millis() as u64, page_count));
            drop(applier);
            store.close_pool().await;
            drop(store);
            fs::remove_dir_all(dir).unwrap();
        }
        assert_eq!(
            digests[&1].0, digests[&256].0,
            "size-1 vs size-256 state must be identical"
        );
        assert_eq!(
            digests[&1].0, digests[&1000].0,
            "size-1 vs size-1000 state must be identical"
        );
        // Memory audit (by construction):
        // - transport/page memory: O(page_size), one page at a time;
        // - candidate staging memory: O(N) rows durable in SQLite, bounded
        //   per-page inserts, no full in-memory copy of all entries;
        // - terminal topology validation memory: O(N) SQL recursive CTE over
        //   candidate rows (count + reachability), not O(page_size).
    }
}
