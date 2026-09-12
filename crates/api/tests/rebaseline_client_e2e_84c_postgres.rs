//! Prompt 85 live proof: fresh PostgreSQL 17 -> real Prompt 83/85 HTTP routes
//! -> real `HttpSyncRemote` (Prompt 84/85 client transport) -> real client
//! SQLite store -> real rebaseline applier and incremental resume.
//!
//! The snapshot crosses the actual HTTP serialization boundary via
//! `HttpSyncRemote::read_page`. The metadata snapshot service is used only
//! for independent server-side verification (local/server equality, checkpoint
//! immutability), never to supply pages to the client.
//!
//! Run with `SYNVEIL_TEST_DATABASE_URL=... cargo test -p synveil-api
//! --test rebaseline_client_e2e_84c_postgres -- --ignored`.

use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc, time::Duration};

use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use synveil_api::{ApiState, CookieConfig, RebaselineTokenKey, StaticReadiness, router};
use synveil_auth::{PasswordHasherConfig, PasswordParameters, SessionConfig};
use synveil_client_sync::{
    CanonicalBaseUrl, ClientSyncError, EngineConfig, FilesystemLocalReplica, HttpClientConfig,
    HttpEnrollmentClient, HttpSyncRemote, InboundSyncEngine, LocalFingerprint, LocalNode,
    LocalStateConfig, LocalStateStore, ManagedRelativePath, OutboundIntent, OutboundIntentKind,
    RebaselineApplier, RebaselineApplyOutcome, RebaselineBoundary,
    RebaselineConvergenceCoordinator, RebaselineConvergenceOutcome, RebaselineHandoffOutcome,
    RebaselineSnapshotDescriptor, ReplicaScope, ServerProfile, SyncOutcome,
};
use synveil_core::{
    DedupDomainId, DeviceId, EnrollmentSecret, Library, LibraryId, LogicalName, Node, NodeId,
    RebaselineSnapshotId, Revision, Sequence, Timestamp, UserId,
};
use synveil_metadata::{
    DatabaseConfig, DatabasePool, DeviceSyncService, DomainRepository, FileMetadataService,
    LogicalSnapshotService, MigrationRunner,
};
use synveil_platform::{SecretName, SecretStore, SecretStoreError, SecretStoreState, SecretValue};

const PASSWORD: &str = "prompt-84c-isolated-password";

#[derive(Default)]
struct TestSecretStore(std::sync::Mutex<BTreeMap<SecretName, SecretValue>>);

impl SecretStore for TestSecretStore {
    fn state(&self) -> SecretStoreState {
        SecretStoreState::Available
    }
    fn put_secret(&self, name: &SecretName, value: &[u8]) -> Result<(), SecretStoreError> {
        self.0
            .lock()
            .unwrap()
            .insert(name.clone(), SecretValue::new(value));
        Ok(())
    }
    fn get_secret(&self, name: &SecretName) -> Result<Option<SecretValue>, SecretStoreError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(name)
            .map(|secret| SecretValue::new(secret.as_bytes())))
    }
    fn delete_secret(&self, name: &SecretName) -> Result<bool, SecretStoreError> {
        Ok(self.0.lock().unwrap().remove(name).is_some())
    }
}

struct ServerTask(tokio::task::JoinHandle<()>);
impl Drop for ServerTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn timestamp() -> Timestamp {
    Timestamp::parse("2026-09-08T00:00:00.123456Z").expect("fixture timestamp is valid")
}

fn name(value: &str) -> LogicalName {
    LogicalName::new(value).expect("fixture name is valid")
}

fn temp_dir(label: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("synveil-84c-e2e-{label}-{}", uuid::Uuid::now_v7()));
    fs::create_dir(&dir).unwrap();
    dir
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn live_pg17_prompt85_rebaseline_handoff_and_incremental_resume() {
    // --- fresh PostgreSQL 17 + real migrations (36/36/0, current) ---
    let base = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable PostgreSQL 17 database");
    let db_name = format!("p84c_e2e_{}", uuid::Uuid::now_v7().simple());
    let maintenance = sqlx::PgPool::connect(&base)
        .await
        .expect("maintenance connection must succeed");
    sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
        .execute(&maintenance)
        .await
        .expect("isolated test database must be created");
    maintenance.close().await;
    let url = format!(
        "{}/{}",
        base.rsplit_once('/').expect("test URL must have a path").0,
        db_name
    );
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = Arc::new(
        DatabasePool::connect(&config)
            .await
            .expect("test PostgreSQL must accept a connection"),
    );
    let status = MigrationRunner::new()
        .run(pool.as_ref())
        .await
        .expect("the migration set must apply");
    assert!(status.is_current());
    assert_eq!(status.applied_versions().len(), 36);
    assert_eq!(status.latest_applied_version(), Some(20260910000000));
    let inspection = sqlx::PgPool::connect(&url)
        .await
        .expect("inspection connection must succeed");
    let version: String = sqlx::query_scalar("SHOW server_version")
        .fetch_one(&inspection)
        .await
        .expect("server version must be readable");
    assert!(version.starts_with("17."), "expected PG17, got {version}");
    println!("live E2E PostgreSQL version: {version}");

    // --- real Synveil API/router on loopback ---
    let real_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", real_listener.local_addr().unwrap());
    let api_state = ApiState::from_current_platform()
        .with_postgres_auth(
            pool.clone(),
            PasswordHasherConfig::new(PasswordParameters::new(8 * 1024, 1, 1, 32).unwrap())
                .unwrap(),
            SessionConfig::new(Duration::from_secs(3600)).unwrap(),
            RebaselineTokenKey::from_bytes([0x84; 32]),
        )
        .with_readiness(Arc::new(StaticReadiness::new(true)))
        .with_cookie_config(CookieConfig::development())
        .with_allowed_origin(&origin);
    let _server = ServerTask(tokio::spawn(async move {
        axum::serve(real_listener, router(api_state)).await.unwrap();
    }));
    let http = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();

    // --- create admin/owner via real bootstrap + login ---
    let login = format!("e2e-owner-{}", uuid::Uuid::now_v7().simple());
    let body = json!({"login": login, "login_key": login, "password": PASSWORD});
    let setup = http
        .post(format!("{origin}/api/v1/bootstrap/admin"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(setup.status(), StatusCode::OK);
    let login_response = http
        .post(format!("{origin}/api/v1/auth/login"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(login_response.status(), StatusCode::OK);
    let mut session = None;
    let mut csrf = None;
    for cookie in login_response.headers().get_all("set-cookie") {
        let first = cookie.to_str().unwrap().split(';').next().unwrap();
        let (cookie_name, value) = first.split_once('=').unwrap();
        match cookie_name {
            "synveil_session" => session = Some(value.to_owned()),
            "synveil_csrf" => csrf = Some(value.to_owned()),
            _ => {}
        }
    }
    let (session, csrf) = (session.unwrap(), csrf.unwrap());
    let login_body: Value = login_response.json().await.unwrap();
    let owner_id: UserId = login_body["data"]["user_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let cookie = format!("synveil_session={session}; synveil_csrf={csrf}");

    // --- enroll active device via real grant + real exchange ---
    let grant_response = http
        .post(format!("{origin}/api/v1/devices/enrollment-grants"))
        .header("cookie", &cookie)
        .header("origin", &origin)
        .header("x-csrf-token", &csrf)
        .json(&json!({"target": {"kind": "new", "display_name": "84C e2e device"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(grant_response.status(), StatusCode::CREATED);
    let grant_body: Value = grant_response.json().await.unwrap();
    let device_id: DeviceId = grant_body["data"]["device_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let grant =
        EnrollmentSecret::parse(grant_body["data"]["enrollment_token"].as_str().unwrap()).unwrap();
    println!("live E2E device: {device_id}");

    // --- create library + server hierarchy (server current name = C) ---
    let library_id = LibraryId::new();
    let server_root = Node::new_root(NodeId::new(), library_id, name("e2e-root"), timestamp());
    let library = Library::new(
        library_id,
        owner_id,
        name("84C e2e library"),
        &server_root,
        DedupDomainId::new(),
        timestamp(),
    )
    .unwrap();
    DomainRepository::new(&pool)
        .insert_library_with_root(&library, &server_root)
        .await
        .unwrap();
    println!("live E2E library: {library_id}");
    let metadata = FileMetadataService::new(pool.as_ref().clone());
    let server_dir = metadata
        .create_directory(owner_id, library_id, Some(server_root.id()), name("c"))
        .await
        .expect("server directory C must be created");
    let _server_leaf = metadata
        .create_directory(owner_id, library_id, Some(server_dir.id()), name("leaf"))
        .await
        .expect("server leaf must be created");

    // --- real client identity: profile + exchanged credential ---
    // (exchange activates the device PENDING -> ACTIVE, which checkpoint
    // establishment requires)
    let profile = ServerProfile::new(
        CanonicalBaseUrl::parse_for_loopback_test(&origin).unwrap(),
        "84C disposable e2e server",
    )
    .unwrap();
    let profile_id = profile.profile_id();
    let enrollment_client =
        HttpEnrollmentClient::new(profile.clone(), HttpClientConfig::default()).unwrap();
    let credential = enrollment_client.exchange(&grant).await.unwrap();
    assert_eq!(credential.device_id(), device_id);
    assert_eq!(credential.owner_user_id(), owner_id);
    let device_bearer = credential.secret().expose_secret();
    {
        let device_status: String = sqlx::query_scalar("SELECT status FROM devices WHERE id = $1")
            .bind(device_id.into_uuid())
            .fetch_one(&inspection)
            .await
            .unwrap();
        assert_eq!(device_status, "ACTIVE", "exchange must activate the device");
    }

    // --- establish device checkpoint (captured before client apply) ---
    let sync_service = DeviceSyncService::new(pool.as_ref().clone());
    let checkpoint_before = sync_service
        .ensure_checkpoint(owner_id, device_id, library_id)
        .await
        .expect("checkpoint must initialize");
    let client_dir = temp_dir("client");
    let managed = client_dir.join("managed");
    fs::create_dir(&managed).unwrap();
    let secrets = TestSecretStore::default();
    let local_config = LocalStateConfig::new(client_dir.join("state.sqlite3"));
    let scope = ReplicaScope::new(owner_id, device_id, library_id);
    {
        let store = LocalStateStore::open(&local_config).await.unwrap();
        store.save_server_profile(&profile).await.unwrap();
        store.store_enrollment(&credential, &secrets).await.unwrap();
        let loaded = store
            .load_device_credential(profile_id, &secrets)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.device_id(), device_id);
        store.close_pool().await;
    }
    let local = Arc::new(LocalStateStore::open(&local_config).await.unwrap());
    let loaded = local
        .load_device_credential(profile_id, &secrets)
        .await
        .unwrap()
        .unwrap();
    // --- real HttpSyncRemote (used for every snapshot page read) ---
    let remote = Arc::new(
        HttpSyncRemote::new(
            profile.clone(),
            device_id,
            loaded,
            HttpClientConfig::default(),
        )
        .unwrap(),
    );
    let replica = Arc::new(
        FilesystemLocalReplica::initialize_for_profile(&managed, scope, profile_id).unwrap(),
    );
    // Profile-bound engine constructor also binds the replica row; the engine
    // is used at the end for the inbound-fence proof.
    let engine = InboundSyncEngine::new(
        scope,
        remote.clone(),
        replica.clone(),
        local.clone(),
        EngineConfig::new(2, 2).unwrap(),
    )
    .await
    .unwrap();
    drop(engine);

    // --- stale local remote mirror (old base name = A) ---
    for (node_id, parent, path, logical, revision) in [
        (
            server_root.id(),
            None,
            ManagedRelativePath::root(),
            name("e2e-root"),
            Revision::new(1),
        ),
        (
            server_dir.id(),
            Some(server_root.id()),
            ManagedRelativePath::new("a").unwrap(),
            name("a"),
            server_dir.revision(),
        ),
    ] {
        local
            .upsert_local_node(&LocalNode::new(
                library_id,
                node_id,
                parent,
                path,
                logical,
                synveil_core::NodeKind::Directory,
                synveil_core::NodeState::Active,
                revision,
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
    // --- pending local rename (A -> B) + local-only create ---
    let rename = OutboundIntent::new(
        library_id,
        Some(server_dir.id()),
        None,
        OutboundIntentKind::RenameNode,
        ManagedRelativePath::new("b").unwrap(),
        Some(ManagedRelativePath::new("a").unwrap()),
        Some(LocalFingerprint::directory()),
        Sequence::new(1),
        Sequence::new(1),
        Some(server_dir.revision()),
        None,
        None,
    )
    .unwrap();
    local.upsert_outbound_intent(&rename).await.unwrap();
    let create = OutboundIntent::new(
        library_id,
        None,
        None,
        OutboundIntentKind::CreateDirectory,
        ManagedRelativePath::new("local-only").unwrap(),
        None,
        Some(LocalFingerprint::directory()),
        Sequence::new(1),
        Sequence::new(1),
        None,
        None,
        None,
    )
    .unwrap();
    local.upsert_outbound_intent(&create).await.unwrap();

    // --- real Prompt 83 POST snapshot over HTTP (device bearer) ---
    let create_response = http
        .post(format!(
            "{origin}/api/v1/libraries/{library_id}/rebaseline-snapshots"
        ))
        .bearer_auth(device_bearer)
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let location = create_response
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(location.starts_with("/api/v1/rebaseline-snapshots/"));
    let create_body: Value = create_response.json().await.unwrap();
    let snapshot_id: RebaselineSnapshotId = create_body["data"]["snapshot_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let entry_count: u64 = create_body["data"]["entry_count"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let journal_epoch: Sequence = create_body["data"]["journal_boundary"]["journal_epoch"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let resume_sequence: Sequence = create_body["data"]["journal_boundary"]["resume_sequence"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    println!("live E2E snapshot: {snapshot_id} entries={entry_count}");
    assert!(entry_count >= 3, "hierarchy must be snapshotted");
    let descriptor = RebaselineSnapshotDescriptor::new(
        snapshot_id,
        library_id,
        RebaselineBoundary::new(journal_epoch, resume_sequence),
        entry_count,
    );

    // --- client downloads real pages via HttpSyncRemote and applies ---
    let applier = RebaselineApplier::new(scope, local.clone(), 2).unwrap();
    let outcome = applier.apply(descriptor, remote.as_ref()).await.unwrap();
    assert_eq!(outcome, RebaselineApplyOutcome::Applied);
    println!("live E2E client activation: Applied");
    drop(applier);

    // --- local/server equality against the durable server artifact ---
    let snapshots = LogicalSnapshotService::new(pool.as_ref().clone());
    let mut server_entries = Vec::new();
    let mut cursor = None;
    let mut page_count = 0_u32;
    loop {
        let page = snapshots
            .read_rebaseline_snapshot_page(owner_id, snapshot_id, cursor, 256, Timestamp::now())
            .await
            .expect("server artifact page must load");
        page_count += 1;
        cursor = page.next_cursor();
        server_entries.extend(page.into_entries());
        if cursor.is_none() {
            break;
        }
    }
    println!("live E2E server pages re-read: {page_count}");
    let local_nodes = local.local_nodes(library_id).await.unwrap();
    assert_eq!(local_nodes.len(), server_entries.len());
    let mut server_by_id: BTreeMap<String, (Option<String>, String, String)> = BTreeMap::new();
    for entry in &server_entries {
        server_by_id.insert(
            entry.node_id().to_string(),
            (
                entry.parent_node_id().map(|id| id.to_string()),
                entry.name().as_str().to_owned(),
                entry.revision().to_string(),
            ),
        );
    }
    for node in &local_nodes {
        let expected = server_by_id
            .get(&node.node_id().to_string())
            .expect("local node must come from the server artifact");
        assert_eq!(node.parent_node_id().map(|id| id.to_string()), expected.0);
        assert_eq!(node.logical_name().as_str(), expected.1.as_str());
        assert_eq!(node.revision().to_string(), expected.2.as_str());
    }
    // conflict seed outcome: authoritative base is C, pending rename still B
    let dir_node = local
        .local_node(library_id, server_dir.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(dir_node.logical_name().as_str(), "c");
    let pending = local
        .outbound_intent(rename.intent_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.observed_relative_path().as_str(), "b");
    assert_eq!(pending, rename);
    assert!(
        local
            .outbound_intent(create.intent_id())
            .await
            .unwrap()
            .is_some()
    );
    let outbound_before_handoff = local.list_pending_intents(library_id).await.unwrap();

    // --- server checkpoint exactly unchanged ---
    let checkpoint_after = sync_service
        .ensure_checkpoint(owner_id, device_id, library_id)
        .await
        .expect("checkpoint must remain readable");
    assert_eq!(
        checkpoint_before, checkpoint_after,
        "server checkpoint must not advance during local apply"
    );

    // --- AppliedPendingHandoff row matches the HTTP descriptor ---
    let sqlite = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&format!(
            "sqlite:{}?mode=ro",
            local_config.database_path().display()
        ))
        .await
        .unwrap();
    let row: (String, String, i64, i64) = sqlx::query_as(
        "SELECT snapshot_id, library_id, journal_epoch, resume_sequence FROM rebaseline_applied_handoffs WHERE library_id = ?",
    )
    .bind(library_id.to_string())
    .fetch_one(&sqlite)
    .await
    .unwrap();
    sqlite.close().await;
    assert_eq!(row.0, snapshot_id.to_string());
    assert_eq!(row.1, library_id.to_string());
    assert_eq!(row.2, journal_epoch.get() as i64);
    assert_eq!(row.3, resume_sequence.get() as i64);
    println!(
        "live E2E handoff: snapshot={} epoch={} resume={}",
        row.0, row.2, row.3
    );

    // --- inbound fence: ordinary inbound processing with old cursor ---
    let fence_engine = Arc::new(
        InboundSyncEngine::new(
            scope,
            remote.clone(),
            replica.clone(),
            local.clone(),
            EngineConfig::default(),
        )
        .await
        .unwrap(),
    );
    let fence = fence_engine.synchronize_once().await;
    assert!(
        matches!(fence, Err(ClientSyncError::RebaselinePendingHandoff)),
        "old-cursor inbound must be fenced, got {fence:?}"
    );
    println!("live E2E inbound fence: REBASELINE_PENDING_HANDOFF");

    // --- Prompt 84 idempotent retry while the handoff marker is present ---
    let applier2 = RebaselineApplier::new(scope, local.clone(), 2).unwrap();
    assert_eq!(
        applier2.apply(descriptor, remote.as_ref()).await.unwrap(),
        RebaselineApplyOutcome::AlreadyApplied
    );
    drop(applier2);

    // --- Prompt 87 consumes the existing P84 handoff through its bounded
    // coordinator. This is a real PostgreSQL + router + HttpSyncRemote
    // operation: existing candidate/handoff precedence produces no new
    // snapshot POST, and Prompt 85 remains the only checkpoint mutation. ---
    let convergence = RebaselineConvergenceCoordinator::new(
        fence_engine.clone(),
        remote.clone(),
        local.clone(),
        2,
    )
    .unwrap();
    assert_eq!(
        convergence.run_convergence_once().await.unwrap(),
        RebaselineConvergenceOutcome::RebaselineConverged {
            snapshot_id,
            boundary: descriptor.boundary(),
        }
    );
    drop(convergence);
    let checkpoint_handoff = sync_service
        .ensure_checkpoint(owner_id, device_id, library_id)
        .await
        .expect("handoff checkpoint must be readable");
    assert_eq!(checkpoint_handoff.journal_epoch(), journal_epoch);
    assert_eq!(checkpoint_handoff.acknowledged_sequence(), resume_sequence);
    let finalized = local.replica(library_id).await.unwrap().unwrap();
    assert_eq!(finalized.journal_epoch(), journal_epoch);
    assert_eq!(finalized.applied_sequence(), resume_sequence);
    assert_eq!(finalized.acknowledged_sequence(), resume_sequence);
    let sqlite = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&format!(
            "sqlite:{}?mode=ro",
            local_config.database_path().display()
        ))
        .await
        .unwrap();
    let marker_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM rebaseline_applied_handoffs WHERE library_id = ?")
            .bind(library_id.to_string())
            .fetch_one(&sqlite)
            .await
            .unwrap();
    sqlite.close().await;
    assert_eq!(marker_count, 0);
    assert_eq!(
        local.list_pending_intents(library_id).await.unwrap(),
        outbound_before_handoff
    );
    println!(
        "live E2E handoff finalized: epoch={} resume={}",
        journal_epoch, resume_sequence
    );

    // --- post-commit restart: the same handoff is already complete locally ---
    drop(fence_engine);
    drop(replica);
    local.close_pool().await;
    drop(local);
    let local = Arc::new(LocalStateStore::open(&local_config).await.unwrap());
    let replica = Arc::new(
        FilesystemLocalReplica::initialize_for_profile(&managed, scope, profile_id).unwrap(),
    );
    let resumed_engine = InboundSyncEngine::new(
        scope,
        remote.clone(),
        replica.clone(),
        local.clone(),
        EngineConfig::new(2, 2).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        resumed_engine
            .complete_rebaseline_handoff(snapshot_id)
            .await
            .unwrap(),
        RebaselineHandoffOutcome::AlreadyComplete
    );

    // --- only after C may ordinary incremental sync begin ---
    let after_handoff = metadata
        .create_directory(
            owner_id,
            library_id,
            Some(server_root.id()),
            name("after-handoff"),
        )
        .await
        .expect("post-handoff directory must be created");
    assert_eq!(
        resumed_engine.synchronize_once().await.unwrap(),
        SyncOutcome::Progressed
    );
    let resumed_node = local
        .local_node(library_id, after_handoff.id())
        .await
        .unwrap()
        .expect("post-handoff feed event must be applied");
    assert_eq!(resumed_node.logical_name().as_str(), "after-handoff");
    let checkpoint_after_incremental = sync_service
        .ensure_checkpoint(owner_id, device_id, library_id)
        .await
        .expect("incremental checkpoint must be readable");
    assert_eq!(checkpoint_after_incremental.journal_epoch(), journal_epoch);
    assert_eq!(
        checkpoint_after_incremental.acknowledged_sequence(),
        Sequence::new(resume_sequence.get() + 1)
    );
    let resumed_record = local.replica(library_id).await.unwrap().unwrap();
    assert_eq!(
        resumed_record.applied_sequence(),
        checkpoint_after_incremental.acknowledged_sequence()
    );
    assert_eq!(
        resumed_record.acknowledged_sequence(),
        checkpoint_after_incremental.acknowledged_sequence()
    );
    assert_eq!(
        local.list_pending_intents(library_id).await.unwrap(),
        outbound_before_handoff
    );
    println!(
        "live E2E incremental resume: sequence={}",
        checkpoint_after_incremental.acknowledged_sequence()
    );

    drop(resumed_engine);
    local.close_pool().await;
    drop(local);
    drop(replica);
    drop(remote);
    fs::remove_dir_all(client_dir).unwrap();
    (*pool).clone().close().await;
}
