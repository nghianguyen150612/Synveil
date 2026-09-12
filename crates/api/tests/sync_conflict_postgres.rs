//! Prompt 88 live PostgreSQL 17 conflict evidence.
//!
//! This target uses the real Axum router, device authentication,
//! `HttpSyncRemote`, outbound engine, PostgreSQL mutation/upload preconditions,
//! and client SQLite conflict ledger. Conflicts are caused by two enrolled
//! devices diverging from the same remote revision.

use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc, time::Duration};

use bytes::Bytes;
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use synveil_api::{ApiState, CookieConfig, RebaselineTokenKey, StaticReadiness, router};
use synveil_auth::{PasswordHasherConfig, PasswordParameters, SessionConfig};
use synveil_client_sync::{
    CanonicalBaseUrl, EngineConfig, FilesystemLocalReplica, HttpClientConfig, HttpEnrollmentClient,
    HttpSyncRemote, InboundSyncEngine, LocalStateConfig, LocalStateStore, ManagedRelativePath,
    ManualChangeWatcher, ObservationConfig, OutboundObservationEngine, OutboundSubmissionEngine,
    OutboundSubmissionOutcome, ReplicaScope, ServerProfile, SyncConflictKind,
    SyncConflictResolution, SyncConflictStatus, SyncOutcome, WatchHint, WatchHintKind,
};
use synveil_core::{
    DedupDomainId, DeviceId, EnrollmentSecret, Library, LibraryId, LogicalName, Node, NodeId,
    OutboundIntentId, Sha256Digest, Timestamp, UserId,
};
use synveil_metadata::{
    DatabaseConfig, DatabasePool, DomainRepository, FileMetadataService, MigrationRunner,
    PostgresContentReadRepository, PostgresUploadRepository, UploadCompletion,
};
use synveil_platform::{SecretName, SecretStore, SecretStoreError, SecretStoreState, SecretValue};
use synveil_storage::{
    ContentReadApplicationService, CreateUploadSessionRequest, ObjectStore,
    UploadApplicationService, UploadLimits, UploadTargetRequest, open_local_object_store,
};

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
            .map(|value| SecretValue::new(value.as_bytes())))
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

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("synveil-p88-conflict-{}", uuid::Uuid::now_v7()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Browser {
    client: Client,
    origin: String,
    cookie: String,
    csrf: String,
    owner: UserId,
}

impl Browser {
    async fn login(client: Client, origin: String) -> Self {
        let login = format!("prompt-88-{}", uuid::Uuid::now_v7().simple());
        let body = json!({
            "login": login,
            "login_key": login,
            "password": "prompt-88-disposable-password"
        });
        assert_eq!(
            client
                .post(format!("{origin}/api/v1/bootstrap/admin"))
                .json(&body)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        let response = client
            .post(format!("{origin}/api/v1/auth/login"))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let mut session = None;
        let mut csrf = None;
        for cookie in response.headers().get_all("set-cookie") {
            let first = cookie.to_str().unwrap().split(';').next().unwrap();
            let (name, value) = first.split_once('=').unwrap();
            match name {
                "synveil_session" => session = Some(value.to_owned()),
                "synveil_csrf" => csrf = Some(value.to_owned()),
                _ => {}
            }
        }
        let csrf = csrf.unwrap();
        let cookie = format!("synveil_session={}; synveil_csrf={csrf}", session.unwrap());
        let body: Value = response.json().await.unwrap();
        Self {
            client,
            origin,
            cookie,
            csrf,
            owner: body["data"]["user_id"].as_str().unwrap().parse().unwrap(),
        }
    }

    async fn grant(&self, label: &str) -> (DeviceId, EnrollmentSecret) {
        let response = self
            .client
            .post(format!("{}/api/v1/devices/enrollment-grants", self.origin))
            .header("cookie", &self.cookie)
            .header("origin", &self.origin)
            .header("x-csrf-token", &self.csrf)
            .json(&json!({"target":{"kind":"new","display_name":label}}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let body: Value = response.json().await.unwrap();
        (
            body["data"]["device_id"].as_str().unwrap().parse().unwrap(),
            EnrollmentSecret::parse(body["data"]["enrollment_token"].as_str().unwrap()).unwrap(),
        )
    }
}

struct ClientLibrary {
    scope: ReplicaScope,
    state: Arc<LocalStateStore>,
    replica: Arc<FilesystemLocalReplica>,
    inbound: Arc<InboundSyncEngine>,
    managed: PathBuf,
}

#[allow(clippy::too_many_arguments)]
async fn client_library(
    base: &std::path::Path,
    label: &str,
    owner: UserId,
    device: DeviceId,
    library: LibraryId,
    profile_id: synveil_client_sync::ServerProfileId,
    state: Arc<LocalStateStore>,
    remote: Arc<HttpSyncRemote>,
) -> ClientLibrary {
    let managed = base.join(format!("{label}-managed-{library}"));
    fs::create_dir(&managed).unwrap();
    let scope = ReplicaScope::new(owner, device, library);
    let replica = Arc::new(
        FilesystemLocalReplica::initialize_for_profile(&managed, scope, profile_id).unwrap(),
    );
    let inbound = Arc::new(
        InboundSyncEngine::new(
            scope,
            remote,
            replica.clone(),
            state.clone(),
            EngineConfig::new(2, 2).unwrap(),
        )
        .await
        .unwrap(),
    );
    converge(inbound.as_ref()).await;
    ClientLibrary {
        scope,
        state,
        replica,
        inbound,
        managed,
    }
}

async fn converge(engine: &InboundSyncEngine) {
    for _ in 0..64 {
        match engine.synchronize_once().await.unwrap() {
            SyncOutcome::Idle => return,
            SyncOutcome::Progressed
            | SyncOutcome::MoreAvailable
            | SyncOutcome::BootstrapRequired => {}
            outcome => panic!("unexpected sync outcome: {outcome:?}"),
        }
    }
    panic!("bounded sync did not converge");
}

async fn seed_file(
    service: &UploadApplicationService,
    owner: UserId,
    library: LibraryId,
    parent: NodeId,
    name: &str,
    bytes: &[u8],
) -> UploadCompletion {
    let session = service
        .create_upload_session(CreateUploadSessionRequest {
            idempotency_key: OutboundIntentId::new(),
            owner_user_id: owner,
            target: UploadTargetRequest::CreateFile {
                library_id: library,
                parent_node_id: parent,
                name: LogicalName::new(name).unwrap(),
            },
            expected_length: bytes.len() as u64,
            expected_sha256: Some(Sha256Digest::from_bytes(Sha256::digest(bytes).into())),
        })
        .await
        .unwrap();
    service
        .append_upload_chunk(owner, session.id, 0, Bytes::copy_from_slice(bytes))
        .await
        .unwrap();
    service.complete_upload(owner, session.id).await.unwrap()
}

fn library(owner: UserId, label: &str) -> (Library, Node) {
    let library_id = LibraryId::new();
    let now = Timestamp::parse("2026-09-12T00:00:00.123456Z").unwrap();
    let root = Node::new_root(
        NodeId::new(),
        library_id,
        LogicalName::new(format!("{label}-root")).unwrap(),
        now,
    );
    let library = Library::new(
        library_id,
        owner,
        LogicalName::new(format!("{label}-library")).unwrap(),
        &root,
        DedupDomainId::new(),
        now,
    )
    .unwrap();
    (library, root)
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn live_pg17_multi_device_rename_and_content_conflicts_are_durable_and_explicit() {
    let database_url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify disposable PostgreSQL 17");
    let database = DatabaseConfig::from_url(&database_url).unwrap();
    let pool = Arc::new(DatabasePool::connect(&database).await.unwrap());
    let migrations = MigrationRunner::new().run(pool.as_ref()).await.unwrap();
    assert!(migrations.is_current());
    assert_eq!(migrations.applied_versions().len(), 36);
    assert_eq!(migrations.latest_applied_version(), Some(20260910000000));
    let pg = sqlx::PgPool::connect(&database_url).await.unwrap();
    let version: String = sqlx::query_scalar("SHOW server_version")
        .fetch_one(&pg)
        .await
        .unwrap();
    assert!(
        version.starts_with("17."),
        "expected PostgreSQL 17, got {version}"
    );
    println!("Prompt 88 live PostgreSQL version: {version}");

    let temp = TempRoot::new();
    let object_store: Arc<dyn ObjectStore> =
        Arc::new(open_local_object_store(temp.0.join("objects")).unwrap());
    let uploads = UploadApplicationService::new(
        Arc::new(PostgresUploadRepository::new(pool.as_ref().clone())),
        object_store.clone(),
        UploadLimits::default(),
    )
    .unwrap();
    let downloads = ContentReadApplicationService::new(
        Arc::new(PostgresContentReadRepository::new(pool.as_ref().clone())),
        object_store,
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let api_state = ApiState::from_current_platform()
        .with_postgres_auth(
            pool.clone(),
            PasswordHasherConfig::new(PasswordParameters::new(8 * 1024, 1, 1, 32).unwrap())
                .unwrap(),
            SessionConfig::new(Duration::from_secs(3600)).unwrap(),
            RebaselineTokenKey::from_bytes([0x88; 32]),
        )
        .with_readiness(Arc::new(StaticReadiness::new(true)))
        .with_cookie_config(CookieConfig::development())
        .with_allowed_origin(&origin)
        .with_download_backend(Arc::new(downloads))
        .with_upload_backend(Arc::new(uploads.clone()));
    let _server = ServerTask(tokio::spawn(async move {
        axum::serve(listener, router(api_state)).await.unwrap();
    }));
    let http = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();
    let browser = Browser::login(http, origin.clone()).await;
    let profile = ServerProfile::new(
        CanonicalBaseUrl::parse_for_loopback_test(&origin).unwrap(),
        "Prompt 88 disposable PostgreSQL",
    )
    .unwrap();
    let profile_id = profile.profile_id();
    let enrollment =
        HttpEnrollmentClient::new(profile.clone(), HttpClientConfig::default()).unwrap();
    let (device_a, grant_a) = browser.grant("Prompt 88 device A").await;
    let (device_b, grant_b) = browser.grant("Prompt 88 device B").await;
    let credential_a = enrollment.exchange(&grant_a).await.unwrap();
    let credential_b = enrollment.exchange(&grant_b).await.unwrap();
    let secrets_a = TestSecretStore::default();
    let secrets_b = TestSecretStore::default();
    let state_a = Arc::new(
        LocalStateStore::open(&LocalStateConfig::new(temp.0.join("a.sqlite3")))
            .await
            .unwrap(),
    );
    let state_b = Arc::new(
        LocalStateStore::open(&LocalStateConfig::new(temp.0.join("b.sqlite3")))
            .await
            .unwrap(),
    );
    for (state, credential, secrets) in [
        (&state_a, &credential_a, &secrets_a),
        (&state_b, &credential_b, &secrets_b),
    ] {
        state.save_server_profile(&profile).await.unwrap();
        state.store_enrollment(credential, secrets).await.unwrap();
    }
    let remote_a = Arc::new(
        HttpSyncRemote::new(
            profile.clone(),
            device_a,
            state_a
                .load_device_credential(profile_id, &secrets_a)
                .await
                .unwrap()
                .unwrap(),
            HttpClientConfig::default(),
        )
        .unwrap(),
    );
    let remote_b = Arc::new(
        HttpSyncRemote::new(
            profile,
            device_b,
            state_b
                .load_device_credential(profile_id, &secrets_b)
                .await
                .unwrap()
                .unwrap(),
            HttpClientConfig::default(),
        )
        .unwrap(),
    );

    // Rename versus rename from one shared canonical base.
    let (rename_library, rename_root) = library(browser.owner, "rename");
    DomainRepository::new(pool.as_ref())
        .insert_library_with_root(&rename_library, &rename_root)
        .await
        .unwrap();
    let rename_seed = seed_file(
        &uploads,
        browser.owner,
        rename_library.id(),
        rename_root.id(),
        "name-a.txt",
        b"rename seed",
    )
    .await;
    let rename_a = client_library(
        &temp.0,
        "rename-a",
        browser.owner,
        device_a,
        rename_library.id(),
        profile_id,
        state_a.clone(),
        remote_a.clone(),
    )
    .await;
    let rename_b = client_library(
        &temp.0,
        "rename-b",
        browser.owner,
        device_b,
        rename_library.id(),
        profile_id,
        state_b.clone(),
        remote_b.clone(),
    )
    .await;
    let (watcher_a, source_a) = ManualChangeWatcher::with_capacity(16);
    let observer_a = OutboundObservationEngine::new(
        rename_a.scope,
        rename_a.replica.clone(),
        rename_a.state.clone(),
        Box::new(watcher_a),
        ObservationConfig::new(16, 16, 8, Duration::ZERO).unwrap(),
    )
    .await
    .unwrap();
    let (watcher_b, source_b) = ManualChangeWatcher::with_capacity(16);
    let observer_b = OutboundObservationEngine::new(
        rename_b.scope,
        rename_b.replica.clone(),
        rename_b.state.clone(),
        Box::new(watcher_b),
        ObservationConfig::new(16, 16, 8, Duration::ZERO).unwrap(),
    )
    .await
    .unwrap();
    observer_a.start().await.unwrap();
    observer_b.start().await.unwrap();
    fs::rename(
        rename_a.managed.join("name-a.txt"),
        rename_a.managed.join("name-b.txt"),
    )
    .unwrap();
    fs::rename(
        rename_b.managed.join("name-a.txt"),
        rename_b.managed.join("name-c.txt"),
    )
    .unwrap();
    for (source, destination) in [(&source_a, "name-b.txt"), (&source_b, "name-c.txt")] {
        source
            .push(
                WatchHint::new(
                    WatchHintKind::Rename,
                    vec![
                        ManagedRelativePath::new("name-a.txt").unwrap(),
                        ManagedRelativePath::new(destination).unwrap(),
                    ],
                )
                .unwrap(),
            )
            .unwrap();
    }
    observer_a.poll_once().await.unwrap();
    observer_b.poll_once().await.unwrap();
    let intent_a = observer_a.list_pending_intents().await.unwrap().remove(0);
    let intent_b = observer_b.list_pending_intents().await.unwrap().remove(0);
    let submit_b = OutboundSubmissionEngine::new(
        rename_b.scope,
        remote_b.clone(),
        rename_b.replica.clone(),
        rename_b.state.clone(),
    )
    .await
    .unwrap();
    assert_eq!(
        submit_b.process_next_ready_intent().await.unwrap(),
        OutboundSubmissionOutcome::Submitted(intent_b.intent_id())
    );
    let submit_a = OutboundSubmissionEngine::new(
        rename_a.scope,
        remote_a.clone(),
        rename_a.replica.clone(),
        rename_a.state.clone(),
    )
    .await
    .unwrap();
    let rename_conflict_id = match submit_a.process_next_ready_intent().await.unwrap() {
        OutboundSubmissionOutcome::Conflict {
            intent_id,
            conflict_id,
        } if intent_id == intent_a.intent_id() => conflict_id,
        other => panic!("expected rename conflict, got {other:?}"),
    };
    let rename_conflict = state_a
        .get_conflict(rename_conflict_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        rename_conflict.kind(),
        SyncConflictKind::RemoteRevisionChanged
    );
    assert_eq!(
        rename_conflict.local_base_revision(),
        Some(rename_seed.node_revision)
    );
    assert!(rename_conflict.remote_observed_revision().unwrap() > rename_seed.node_revision);
    assert_eq!(
        submit_a.process_next_ready_intent().await.unwrap(),
        OutboundSubmissionOutcome::BlockedByConflict(rename_conflict_id)
    );
    let server_renamed = FileMetadataService::new(pool.as_ref().clone())
        .get_node(browser.owner, rename_seed.node_id)
        .await
        .unwrap();
    assert_eq!(server_renamed.name().as_str(), "name-c.txt");
    assert_eq!(intent_a.observed_relative_path().as_str(), "name-b.txt");
    assert_eq!(intent_a.base_revision(), Some(rename_seed.node_revision));
    let accepted = state_a
        .resolve_conflict(rename_conflict_id, SyncConflictResolution::AcceptRemote)
        .await
        .unwrap();
    assert_eq!(accepted.status(), SyncConflictStatus::Resolved);
    assert_eq!(
        FileMetadataService::new(pool.as_ref().clone())
            .get_node(browser.owner, rename_seed.node_id)
            .await
            .unwrap()
            .name()
            .as_str(),
        "name-c.txt"
    );

    // Move versus move from the same server revision. The two device-backed
    // observers create genuine MOVE_NODE requests; device B wins and device A
    // receives the canonical metadata conflict.
    let metadata = FileMetadataService::new(pool.as_ref().clone());
    let (move_library, move_root) = library(browser.owner, "move");
    DomainRepository::new(pool.as_ref())
        .insert_library_with_root(&move_library, &move_root)
        .await
        .unwrap();
    let parent_a = metadata
        .create_directory(
            browser.owner,
            move_library.id(),
            Some(move_root.id()),
            LogicalName::new("parent-a").unwrap(),
        )
        .await
        .unwrap();
    let _parent_b = metadata
        .create_directory(
            browser.owner,
            move_library.id(),
            Some(move_root.id()),
            LogicalName::new("parent-b").unwrap(),
        )
        .await
        .unwrap();
    let parent_c = metadata
        .create_directory(
            browser.owner,
            move_library.id(),
            Some(move_root.id()),
            LogicalName::new("parent-c").unwrap(),
        )
        .await
        .unwrap();
    let move_seed = seed_file(
        &uploads,
        browser.owner,
        move_library.id(),
        parent_a.id(),
        "move.txt",
        b"move seed",
    )
    .await;
    let move_a = client_library(
        &temp.0,
        "move-a",
        browser.owner,
        device_a,
        move_library.id(),
        profile_id,
        state_a.clone(),
        remote_a.clone(),
    )
    .await;
    let move_b = client_library(
        &temp.0,
        "move-b",
        browser.owner,
        device_b,
        move_library.id(),
        profile_id,
        state_b.clone(),
        remote_b.clone(),
    )
    .await;
    let (move_watcher_a, move_source_a) = ManualChangeWatcher::with_capacity(16);
    let move_observer_a = OutboundObservationEngine::new(
        move_a.scope,
        move_a.replica.clone(),
        move_a.state.clone(),
        Box::new(move_watcher_a),
        ObservationConfig::new(16, 16, 8, Duration::ZERO).unwrap(),
    )
    .await
    .unwrap();
    let (move_watcher_b, move_source_b) = ManualChangeWatcher::with_capacity(16);
    let move_observer_b = OutboundObservationEngine::new(
        move_b.scope,
        move_b.replica.clone(),
        move_b.state.clone(),
        Box::new(move_watcher_b),
        ObservationConfig::new(16, 16, 8, Duration::ZERO).unwrap(),
    )
    .await
    .unwrap();
    move_observer_a.start().await.unwrap();
    move_observer_b.start().await.unwrap();
    fs::rename(
        move_a.managed.join("parent-a/move.txt"),
        move_a.managed.join("parent-b/move.txt"),
    )
    .unwrap();
    fs::rename(
        move_b.managed.join("parent-a/move.txt"),
        move_b.managed.join("parent-c/move.txt"),
    )
    .unwrap();
    move_source_a
        .push(
            WatchHint::new(
                WatchHintKind::Rename,
                vec![
                    ManagedRelativePath::new("parent-a/move.txt").unwrap(),
                    ManagedRelativePath::new("parent-b/move.txt").unwrap(),
                ],
            )
            .unwrap(),
        )
        .unwrap();
    move_source_b
        .push(
            WatchHint::new(
                WatchHintKind::Rename,
                vec![
                    ManagedRelativePath::new("parent-a/move.txt").unwrap(),
                    ManagedRelativePath::new("parent-c/move.txt").unwrap(),
                ],
            )
            .unwrap(),
        )
        .unwrap();
    move_observer_a.poll_once().await.unwrap();
    move_observer_b.poll_once().await.unwrap();
    let move_intent_a = move_observer_a
        .list_pending_intents()
        .await
        .unwrap()
        .remove(0);
    let move_intent_b = move_observer_b
        .list_pending_intents()
        .await
        .unwrap()
        .remove(0);
    let move_submit_b = OutboundSubmissionEngine::new(
        move_b.scope,
        remote_b.clone(),
        move_b.replica.clone(),
        move_b.state.clone(),
    )
    .await
    .unwrap();
    assert_eq!(
        move_submit_b.process_next_ready_intent().await.unwrap(),
        OutboundSubmissionOutcome::Submitted(move_intent_b.intent_id())
    );
    let move_submit_a = OutboundSubmissionEngine::new(
        move_a.scope,
        remote_a.clone(),
        move_a.replica.clone(),
        move_a.state.clone(),
    )
    .await
    .unwrap();
    let move_conflict = match move_submit_a.process_next_ready_intent().await.unwrap() {
        OutboundSubmissionOutcome::Conflict {
            intent_id,
            conflict_id,
        } if intent_id == move_intent_a.intent_id() => {
            state_a.get_conflict(conflict_id).await.unwrap().unwrap()
        }
        other => panic!("expected move conflict, got {other:?}"),
    };
    assert!(matches!(
        move_conflict.kind(),
        SyncConflictKind::RemoteRevisionChanged | SyncConflictKind::ParentChangedOrUnavailable
    ));
    assert_eq!(move_intent_a.base_revision(), Some(move_seed.node_revision));
    assert_eq!(
        metadata
            .get_node(browser.owner, move_seed.node_id)
            .await
            .unwrap()
            .parent_node_id(),
        Some(parent_c.id())
    );

    // Remote trash versus a local rename. The authoritative node state is
    // retained, and the local rename remains preserved in the conflict intent.
    let (trash_library, trash_root) = library(browser.owner, "trash");
    DomainRepository::new(pool.as_ref())
        .insert_library_with_root(&trash_library, &trash_root)
        .await
        .unwrap();
    let trash_seed = seed_file(
        &uploads,
        browser.owner,
        trash_library.id(),
        trash_root.id(),
        "trash-a.txt",
        b"trash seed",
    )
    .await;
    let trash_a = client_library(
        &temp.0,
        "trash-a",
        browser.owner,
        device_a,
        trash_library.id(),
        profile_id,
        state_a.clone(),
        remote_a.clone(),
    )
    .await;
    let (trash_watcher, trash_source) = ManualChangeWatcher::with_capacity(16);
    let trash_observer = OutboundObservationEngine::new(
        trash_a.scope,
        trash_a.replica.clone(),
        trash_a.state.clone(),
        Box::new(trash_watcher),
        ObservationConfig::new(16, 16, 8, Duration::ZERO).unwrap(),
    )
    .await
    .unwrap();
    trash_observer.start().await.unwrap();
    fs::rename(
        trash_a.managed.join("trash-a.txt"),
        trash_a.managed.join("trash-local.txt"),
    )
    .unwrap();
    trash_source
        .push(
            WatchHint::new(
                WatchHintKind::Rename,
                vec![
                    ManagedRelativePath::new("trash-a.txt").unwrap(),
                    ManagedRelativePath::new("trash-local.txt").unwrap(),
                ],
            )
            .unwrap(),
        )
        .unwrap();
    trash_observer.poll_once().await.unwrap();
    let trash_intent = trash_observer
        .list_pending_intents()
        .await
        .unwrap()
        .remove(0);
    let trashed = metadata
        .delete_node(browser.owner, trash_seed.node_id, trash_seed.node_revision)
        .await
        .unwrap();
    let trash_submit = OutboundSubmissionEngine::new(
        trash_a.scope,
        remote_a.clone(),
        trash_a.replica.clone(),
        trash_a.state.clone(),
    )
    .await
    .unwrap();
    let trash_conflict = match trash_submit.process_next_ready_intent().await.unwrap() {
        OutboundSubmissionOutcome::Conflict {
            intent_id,
            conflict_id,
        } if intent_id == trash_intent.intent_id() => {
            state_a.get_conflict(conflict_id).await.unwrap().unwrap()
        }
        other => panic!("expected remote-trash conflict, got {other:?}"),
    };
    assert!(matches!(
        trash_conflict.kind(),
        SyncConflictKind::RemoteRevisionChanged | SyncConflictKind::RemoteStateChanged
    ));
    assert_eq!(trashed.state(), synveil_core::NodeState::Trashed);
    assert_eq!(
        trash_intent.observed_relative_path().as_str(),
        "trash-local.txt"
    );

    // Content versus content. The losing bytes are staged before the canonical
    // upload precondition rejects completion, then survive inbound convergence.
    let (content_library, content_root) = library(browser.owner, "content");
    DomainRepository::new(pool.as_ref())
        .insert_library_with_root(&content_library, &content_root)
        .await
        .unwrap();
    let content_seed = seed_file(
        &uploads,
        browser.owner,
        content_library.id(),
        content_root.id(),
        "content.txt",
        b"version one",
    )
    .await;
    let content_a = client_library(
        &temp.0,
        "content-a",
        browser.owner,
        device_a,
        content_library.id(),
        profile_id,
        state_a.clone(),
        remote_a.clone(),
    )
    .await;
    let content_b = client_library(
        &temp.0,
        "content-b",
        browser.owner,
        device_b,
        content_library.id(),
        profile_id,
        state_b.clone(),
        remote_b.clone(),
    )
    .await;
    let (watcher_a, source_a) = ManualChangeWatcher::with_capacity(16);
    let observer_a = OutboundObservationEngine::new(
        content_a.scope,
        content_a.replica.clone(),
        content_a.state.clone(),
        Box::new(watcher_a),
        ObservationConfig::new(16, 16, 8, Duration::ZERO).unwrap(),
    )
    .await
    .unwrap();
    let (watcher_b, source_b) = ManualChangeWatcher::with_capacity(16);
    let observer_b = OutboundObservationEngine::new(
        content_b.scope,
        content_b.replica.clone(),
        content_b.state.clone(),
        Box::new(watcher_b),
        ObservationConfig::new(16, 16, 8, Duration::ZERO).unwrap(),
    )
    .await
    .unwrap();
    observer_a.start().await.unwrap();
    observer_b.start().await.unwrap();
    let local_loser = b"device A unsynced bytes";
    let remote_winner = b"device B accepted bytes";
    fs::write(content_a.managed.join("content.txt"), local_loser).unwrap();
    fs::write(content_b.managed.join("content.txt"), remote_winner).unwrap();
    for source in [&source_a, &source_b] {
        source
            .push(
                WatchHint::new(
                    WatchHintKind::Modify,
                    vec![ManagedRelativePath::new("content.txt").unwrap()],
                )
                .unwrap(),
            )
            .unwrap();
    }
    observer_a.poll_once().await.unwrap();
    observer_b.poll_once().await.unwrap();
    let content_intent_a = observer_a.list_pending_intents().await.unwrap().remove(0);
    let content_intent_b = observer_b.list_pending_intents().await.unwrap().remove(0);
    let content_submit_b = OutboundSubmissionEngine::new(
        content_b.scope,
        remote_b.clone(),
        content_b.replica.clone(),
        content_b.state.clone(),
    )
    .await
    .unwrap();
    assert_eq!(
        content_submit_b.process_next_ready_intent().await.unwrap(),
        OutboundSubmissionOutcome::Submitted(content_intent_b.intent_id())
    );
    let content_submit_a = OutboundSubmissionEngine::new(
        content_a.scope,
        remote_a.clone(),
        content_a.replica.clone(),
        content_a.state.clone(),
    )
    .await
    .unwrap();
    let content_conflict_id = match content_submit_a.process_next_ready_intent().await.unwrap() {
        OutboundSubmissionOutcome::Conflict {
            intent_id,
            conflict_id,
        } if intent_id == content_intent_a.intent_id() => conflict_id,
        other => panic!("expected content conflict, got {other:?}"),
    };
    let staged_before = state_a
        .durable_upload_session(content_intent_a.intent_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        state_a
            .get_conflict(content_conflict_id)
            .await
            .unwrap()
            .unwrap()
            .kind(),
        SyncConflictKind::RemoteContentChanged
    );
    converge(content_a.inbound.as_ref()).await;
    let staged_after = state_a
        .durable_upload_session(content_intent_a.intent_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(staged_before, staged_after);
    let current = state_a
        .local_node(content_library.id(), content_seed.node_id)
        .await
        .unwrap()
        .unwrap();
    assert!(current.revision() > content_seed.node_revision);
    let retried = state_a
        .resolve_conflict(
            content_conflict_id,
            SyncConflictResolution::RetryLocalAgainstCurrentBase,
        )
        .await
        .unwrap();
    let replacement = state_a
        .outbound_intent(retried.replacement_intent_id().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(replacement.base_revision(), Some(current.revision()));
    assert_eq!(
        state_a
            .durable_upload_session(replacement.intent_id())
            .await
            .unwrap()
            .unwrap()
            .staging_relative_path(),
        staged_before.staging_relative_path()
    );

    observer_a.shutdown().await.unwrap();
    observer_b.shutdown().await.unwrap();
    pg.close().await;
}
