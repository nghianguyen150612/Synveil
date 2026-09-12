//! Prompt 87 live convergence evidence.
//!
//! Each ignored test provisions a child database from the caller-supplied
//! PostgreSQL instance, starts the real Axum router, and drives the real
//! `HttpSyncRemote` through a loopback proxy.  The proxy is deliberately only
//! an observation/fault-injection boundary for this test target: it counts
//! bounded HTTP calls and can pause or return a canonical error response at a
//! named point without adding a production route, retry, or policy.
//!
//! Run with:
//!
//! ```text
//! SYNVEIL_TEST_DATABASE_URL=postgresql://... cargo test -p synveil-api \
//!   --test rebaseline_convergence_postgres --locked -- --ignored
//! ```

use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{Request, StatusCode, header},
    response::Response,
};
use http_body_util::BodyExt;
use reqwest::{Client, Method};
use serde_json::json;
use sqlx::{PgPool, sqlite::SqlitePoolOptions};
use synveil_api::{ApiState, CookieConfig, RebaselineTokenKey, StaticReadiness, router};
use synveil_auth::{
    DeviceAuthenticationService, DeviceEnrollmentTarget, PasswordHasherConfig, PasswordParameters,
    SessionConfig,
};
use synveil_client_sync::{
    CanonicalBaseUrl, ClientSyncError, EngineConfig, FilesystemLocalReplica, HttpClientConfig,
    HttpEnrollmentClient, HttpSyncRemote, InboundSyncEngine, LocalFingerprint, LocalNode,
    LocalStateConfig, LocalStateStore, ManagedRelativePath, OutboundIntent, OutboundIntentKind,
    RebaselineApplier, RebaselineConvergenceCoordinator, RebaselineConvergenceOutcome,
    RebaselineSnapshotRemote, RemoteErrorKind, ReplicaScope, ServerProfile, SyncOutcome,
};
use synveil_core::{
    DedupDomainId, Device, DeviceId, DeviceStatus, Library, LibraryId, LogicalName,
    LoginIdentifier, Node, NodeId, Revision, Sequence, Timestamp, User, UserId, UserStatus,
};
use synveil_metadata::{
    DatabaseConfig, DatabasePool, DeviceSyncService, DomainRepository, FileMetadataService,
    MigrationRunner, SyncAckEvidence, SyncRetentionService,
};
use synveil_platform::{SecretName, SecretStore, SecretStoreError, SecretStoreState, SecretValue};
use uuid::Uuid;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

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

#[derive(Clone)]
struct ProxyState {
    target: String,
    client: Client,
    snapshot_posts: Arc<AtomicUsize>,
    snapshot_pages: Arc<AtomicUsize>,
    feed_requests: Arc<AtomicUsize>,
    checkpoint_requests: Arc<AtomicUsize>,
    handoff_requests: Arc<AtomicUsize>,
    hold_next_page: Arc<AtomicBool>,
    page_started: Arc<tokio::sync::Notify>,
    release_page: Arc<tokio::sync::Notify>,
    fail_page_call: Arc<AtomicUsize>,
    fail_handoff_call: Arc<AtomicUsize>,
}

impl ProxyState {
    fn new(target: String, client: Client) -> Self {
        Self {
            target,
            client,
            snapshot_posts: Arc::new(AtomicUsize::new(0)),
            snapshot_pages: Arc::new(AtomicUsize::new(0)),
            feed_requests: Arc::new(AtomicUsize::new(0)),
            checkpoint_requests: Arc::new(AtomicUsize::new(0)),
            handoff_requests: Arc::new(AtomicUsize::new(0)),
            hold_next_page: Arc::new(AtomicBool::new(false)),
            page_started: Arc::new(tokio::sync::Notify::new()),
            release_page: Arc::new(tokio::sync::Notify::new()),
            fail_page_call: Arc::new(AtomicUsize::new(0)),
            fail_handoff_call: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn reset_counts(&self) {
        self.snapshot_posts.store(0, Ordering::SeqCst);
        self.snapshot_pages.store(0, Ordering::SeqCst);
        self.feed_requests.store(0, Ordering::SeqCst);
        self.checkpoint_requests.store(0, Ordering::SeqCst);
        self.handoff_requests.store(0, Ordering::SeqCst);
        self.fail_page_call.store(0, Ordering::SeqCst);
        self.fail_handoff_call.store(0, Ordering::SeqCst);
    }

    fn snapshot_posts(&self) -> usize {
        self.snapshot_posts.load(Ordering::SeqCst)
    }

    fn snapshot_pages(&self) -> usize {
        self.snapshot_pages.load(Ordering::SeqCst)
    }

    fn handoffs(&self) -> usize {
        self.handoff_requests.load(Ordering::SeqCst)
    }
}

fn controlled_error(status: StatusCode, code: &str) -> Response<Body> {
    let body = serde_json::to_vec(&json!({
        "error": {
            "code": code,
            "message": "controlled integration-test response",
            "request_id": Uuid::now_v7().to_string(),
            "retryable": false,
            "details": {}
        }
    }))
    .expect("controlled error JSON must serialize");
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "private, no-store")
        .body(Body::from(body))
        .expect("controlled response must build")
}

async fn proxy_request(State(state): State<ProxyState>, request: Request<Body>) -> Response<Body> {
    let (parts, body) = request.into_parts();
    let path = parts.uri.path().to_owned();
    let query = parts
        .uri
        .query()
        .map_or_else(String::new, |value| format!("?{value}"));
    let method = parts.method.clone();

    let is_snapshot_create = method == Method::POST
        && path.starts_with("/api/v1/libraries/")
        && path.ends_with("/rebaseline-snapshots");
    let is_snapshot_page = method == Method::GET
        && path.starts_with("/api/v1/rebaseline-snapshots/")
        && path.ends_with("/entries");
    let is_handoff = method == Method::POST
        && path.starts_with("/api/v1/rebaseline-snapshots/")
        && path.ends_with("/handoff");
    let is_feed =
        method == Method::GET && path.starts_with("/api/v1/devices/") && path.ends_with("/changes");
    let is_checkpoint = method == Method::GET
        && path.starts_with("/api/v1/devices/")
        && path.ends_with("/checkpoint");

    if is_snapshot_create {
        state.snapshot_posts.fetch_add(1, Ordering::SeqCst);
    }
    if is_snapshot_page {
        let call = state.snapshot_pages.fetch_add(1, Ordering::SeqCst) + 1;
        let fail_call = state.fail_page_call.load(Ordering::SeqCst);
        if fail_call == call {
            return controlled_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
        }
        if state.hold_next_page.swap(false, Ordering::SeqCst) {
            state.page_started.notify_one();
            state.release_page.notified().await;
        }
    }
    if is_feed {
        state.feed_requests.fetch_add(1, Ordering::SeqCst);
    }
    if is_checkpoint {
        state.checkpoint_requests.fetch_add(1, Ordering::SeqCst);
    }
    if is_handoff {
        let call = state.handoff_requests.fetch_add(1, Ordering::SeqCst) + 1;
        if state.fail_handoff_call.load(Ordering::SeqCst) == call {
            return controlled_error(StatusCode::NOT_FOUND, "not_found");
        }
    }

    let body = match body.collect().await {
        Ok(body) => body.to_bytes(),
        Err(_) => return controlled_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
    };
    let mut upstream = state
        .client
        .request(method, format!("{}{}{}", state.target, path, query));
    for (name, value) in &parts.headers {
        if name != header::HOST {
            upstream = upstream.header(name, value);
        }
    }
    let response = match upstream.body(body).send().await {
        Ok(response) => response,
        Err(_) => return controlled_error(StatusCode::BAD_GATEWAY, "dependency_unavailable"),
    };
    let status = response.status();
    let headers = response.headers().clone();
    let body = match response.bytes().await {
        Ok(body) => body,
        Err(_) => return controlled_error(StatusCode::BAD_GATEWAY, "dependency_unavailable"),
    };
    let mut output = Response::builder().status(status);
    for (name, value) in &headers {
        output = output.header(name, value);
    }
    output
        .body(Body::from(body))
        .expect("proxy response must build")
}

fn now_plus(_days: u64) -> Timestamp {
    Timestamp::parse("2030-01-01T00:00:00Z")
        .expect("maintenance observation timestamp must be valid")
}

fn fixture_timestamp() -> Timestamp {
    Timestamp::parse("2026-09-11T00:00:00.123456Z")
        .expect("fixture timestamp must have PostgreSQL precision")
}

fn name(value: &str) -> LogicalName {
    LogicalName::new(value).expect("fixture logical name must be valid")
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "synveil-87b-convergence-{label}-{}",
        Uuid::now_v7()
    ));
    fs::create_dir(&dir).expect("fixture directory must be created");
    dir
}

fn child_database_url(base: &str, name: &str) -> String {
    let (origin, _) = base
        .rsplit_once('/')
        .expect("test database URL must include a database path");
    format!("{origin}/{name}")
}

struct LiveFixture {
    database_name: String,
    base_url: String,
    pool: Arc<DatabasePool>,
    inspection: PgPool,
    local: Arc<LocalStateStore>,
    remote: Arc<HttpSyncRemote>,
    owner_id: UserId,
    device_id: DeviceId,
    profile_id: synveil_client_sync::ServerProfileId,
    client_dir: PathBuf,
    proxy: ProxyState,
    _server: ServerTask,
    _proxy: ServerTask,
}

impl LiveFixture {
    async fn new(label: &str) -> Self {
        let base_url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
            .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable PostgreSQL 17 database");
        let database_name = format!("p87b_{}_{}", label, Uuid::now_v7().simple());
        let maintenance = PgPool::connect(&base_url)
            .await
            .expect("maintenance connection must succeed");
        sqlx::query(&format!("CREATE DATABASE \"{database_name}\""))
            .execute(&maintenance)
            .await
            .expect("isolated child database must be created");
        maintenance.close().await;

        let url = child_database_url(&base_url, &database_name);
        let config = DatabaseConfig::from_url(&url).expect("child URL must be PostgreSQL");
        let pool = Arc::new(
            DatabasePool::connect(&config)
                .await
                .expect("child PostgreSQL connection must succeed"),
        );
        let migration = MigrationRunner::new()
            .run(pool.as_ref())
            .await
            .expect("child database migrations must apply");
        assert!(migration.is_current());
        assert_eq!(migration.applied_versions().len(), 36);
        assert_eq!(migration.latest_applied_version(), Some(20260910000000));
        let inspection = PgPool::connect(&url)
            .await
            .expect("inspection connection must succeed");
        let version: String = sqlx::query_scalar("SHOW server_version")
            .fetch_one(&inspection)
            .await
            .expect("server version must be readable");
        assert!(
            version.starts_with("17."),
            "expected PostgreSQL 17, got {version}"
        );
        println!("Prompt 87B live PostgreSQL version: {version}");

        let http = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(TEST_TIMEOUT)
            .build()
            .expect("test HTTP client must build");
        let real_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("real API listener must bind");
        let real_origin = format!("http://{}", real_listener.local_addr().unwrap());
        let api_state = ApiState::from_current_platform()
            .with_postgres_auth(
                pool.clone(),
                PasswordHasherConfig::new(PasswordParameters::new(8 * 1024, 1, 1, 32).unwrap())
                    .unwrap(),
                SessionConfig::new(Duration::from_secs(3600)).unwrap(),
                RebaselineTokenKey::from_bytes([0x87; 32]),
            )
            .with_readiness(Arc::new(StaticReadiness::new(true)))
            .with_cookie_config(CookieConfig::development());
        let _server = ServerTask(tokio::spawn(async move {
            axum::serve(real_listener, router(api_state))
                .await
                .expect("real API server must run");
        }));

        let proxy_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("proxy listener must bind");
        let proxy_origin = format!("http://{}", proxy_listener.local_addr().unwrap());
        let proxy = ProxyState::new(real_origin, http.clone());
        let proxy_for_server = proxy.clone();
        let _proxy = ServerTask(tokio::spawn(async move {
            axum::serve(
                proxy_listener,
                Router::new()
                    .fallback(proxy_request)
                    .with_state(proxy_for_server),
            )
            .await
            .expect("proxy server must run");
        }));

        let owner_id = UserId::new();
        let owner_login = LoginIdentifier::new(
            format!("prompt-87b-{owner_id}"),
            format!("prompt-87b-{owner_id}"),
        )
        .expect("fixture login must be valid");
        DomainRepository::new(pool.as_ref())
            .insert_user(&User::new(
                owner_id,
                owner_login,
                UserStatus::Active,
                fixture_timestamp(),
            ))
            .await
            .expect("fixture owner must persist");
        let device_id = DeviceId::new();
        let mut device = Device::new(
            device_id,
            owner_id,
            name("Prompt 87B device"),
            fixture_timestamp(),
        );
        device
            .transition_status(DeviceStatus::Active, fixture_timestamp())
            .expect("fixture device must activate");
        DomainRepository::new(pool.as_ref())
            .insert_device(&device)
            .await
            .expect("fixture device must persist");
        let grant = DeviceAuthenticationService::new(pool.as_ref())
            .create_grant(owner_id, DeviceEnrollmentTarget::Existing(device_id))
            .await
            .expect("fixture enrollment grant must issue");

        let profile = ServerProfile::new(
            CanonicalBaseUrl::parse_for_loopback_test(&proxy_origin)
                .expect("proxy origin must satisfy loopback profile policy"),
            "Prompt 87B PostgreSQL 17 proxy",
        )
        .expect("fixture profile must build");
        let enrollment = HttpEnrollmentClient::new(profile.clone(), HttpClientConfig::default())
            .expect("fixture enrollment client must build")
            .exchange(&grant.token)
            .await
            .expect("real HTTP enrollment exchange must succeed");
        assert_eq!(enrollment.owner_user_id(), owner_id);
        assert_eq!(enrollment.device_id(), device_id);
        let secrets = Arc::new(TestSecretStore::default());
        let client_dir = temp_dir(label);
        let local_config = LocalStateConfig::new(client_dir.join("state.sqlite3"));
        let local = Arc::new(
            LocalStateStore::open(&local_config)
                .await
                .expect("client SQLite store must open"),
        );
        local
            .save_server_profile(&profile)
            .await
            .expect("profile must persist");
        local
            .store_enrollment(&enrollment, secrets.as_ref())
            .await
            .expect("enrollment must persist");
        let loaded = local
            .load_device_credential(profile.profile_id(), secrets.as_ref())
            .await
            .expect("stored credential must load")
            .expect("stored credential must exist");
        let remote = Arc::new(
            HttpSyncRemote::new(
                profile.clone(),
                device_id,
                loaded,
                HttpClientConfig::default(),
            )
            .expect("HTTP sync remote must build"),
        );

        Self {
            database_name,
            base_url,
            pool,
            inspection,
            local,
            remote,
            owner_id,
            device_id,
            profile_id: profile.profile_id(),
            client_dir,
            proxy,
            _server,
            _proxy,
        }
    }

    async fn create_library(&self, label: &str) -> LibrarySeed {
        let library_id = LibraryId::new();
        let root = Node::new_root(
            NodeId::new(),
            library_id,
            name(&format!("{label}-root")),
            fixture_timestamp(),
        );
        let library = Library::new(
            library_id,
            self.owner_id,
            name(&format!("{label}-library")),
            &root,
            DedupDomainId::new(),
            fixture_timestamp(),
        )
        .expect("fixture library must build");
        DomainRepository::new(self.pool.as_ref())
            .insert_library_with_root(&library, &root)
            .await
            .expect("fixture library must persist");
        LibrarySeed { library_id, root }
    }

    async fn create_directory(&self, seed: &LibrarySeed, value: &str) -> Node {
        FileMetadataService::new(self.pool.as_ref().clone())
            .create_directory(
                self.owner_id,
                seed.library_id,
                Some(seed.root.id()),
                name(value),
            )
            .await
            .expect("fixture directory must persist")
    }

    async fn client_for(&self, seed: &LibrarySeed, label: &str) -> ClientLibrary {
        let managed = self.client_dir.join(format!("{label}-managed"));
        fs::create_dir(&managed).expect("managed root must be created");
        let scope = ReplicaScope::new(self.owner_id, self.device_id, seed.library_id);
        let replica = Arc::new(
            FilesystemLocalReplica::initialize_for_profile(&managed, scope, self.profile_id)
                .expect("managed replica must initialize"),
        );
        let engine = Arc::new(
            InboundSyncEngine::new(
                scope,
                self.remote.clone(),
                replica.clone(),
                self.local.clone(),
                EngineConfig::new(2, 2).expect("fixture engine config must build"),
            )
            .await
            .expect("client engine must bind"),
        );
        ClientLibrary {
            seed: seed.clone(),
            engine,
            state: self.local.clone(),
        }
    }

    async fn compact(&self, seed: &LibrarySeed) {
        SyncRetentionService::new(self.pool.as_ref().clone())
            .compact_journal_step(self.owner_id, seed.library_id, now_plus(31))
            .await
            .expect("fixture compaction must succeed");
    }

    async fn server_head(&self, seed: &LibrarySeed) -> (Sequence, Sequence, Sequence) {
        let row: (i64, i64, i64) = sqlx::query_as(
            "SELECT journal_epoch, sync_head, minimum_retained_sequence
             FROM libraries WHERE id = $1",
        )
        .bind(seed.library_id.into_uuid())
        .fetch_one(&self.inspection)
        .await
        .expect("server library head must be readable");
        (
            Sequence::new(row.0 as u64),
            Sequence::new(row.1 as u64),
            Sequence::new(row.2 as u64),
        )
    }

    async fn journal_row_count(&self, seed: &LibrarySeed) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM change_journal WHERE library_id = $1")
            .bind(seed.library_id.into_uuid())
            .fetch_one(&self.inspection)
            .await
            .expect("journal row count must be readable")
    }

    async fn server_node_count(&self, seed: &LibrarySeed) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM nodes WHERE library_id = $1")
            .bind(seed.library_id.into_uuid())
            .fetch_one(&self.inspection)
            .await
            .expect("server node count must be readable")
    }

    async fn checkpoint(&self, seed: &LibrarySeed) -> synveil_core::DeviceSyncCheckpoint {
        DeviceSyncService::new(self.pool.as_ref().clone())
            .ensure_checkpoint(self.owner_id, self.device_id, seed.library_id)
            .await
            .expect("server checkpoint must be readable")
    }

    async fn advance_checkpoint(&self, seed: &LibrarySeed) -> synveil_core::DeviceSyncCheckpoint {
        let sync = DeviceSyncService::new(self.pool.as_ref().clone());
        let page = sync
            .fetch_feed(self.owner_id, self.device_id, seed.library_id, 100)
            .await
            .expect("server feed must be readable");
        assert!(!page.has_more(), "fixture feed must fit in one page");
        let evidence = SyncAckEvidence::new(
            self.owner_id,
            self.device_id,
            seed.library_id,
            page.checkpoint().journal_epoch(),
            page.from_sequence(),
            page.through_sequence(),
            page.high_watermark().sequence(),
        );
        sync.acknowledge(self.owner_id, self.device_id, seed.library_id, evidence)
            .await
            .expect("fixture checkpoint must advance")
    }

    async fn snapshot_payload_and_proof(
        &self,
        snapshot_id: synveil_core::RebaselineSnapshotId,
    ) -> (i64, i64) {
        let payload: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM rebaseline_snapshots WHERE id = $1")
                .bind(snapshot_id.into_uuid())
                .fetch_one(&self.inspection)
                .await
                .expect("snapshot payload count must be readable");
        let proof: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM rebaseline_snapshot_handoff_proofs WHERE snapshot_id = $1",
        )
        .bind(snapshot_id.into_uuid())
        .fetch_one(&self.inspection)
        .await
        .expect("snapshot proof count must be readable");
        (payload, proof)
    }

    async fn sqlite_counts(&self, seed: &LibrarySeed) -> (i64, i64) {
        let sqlite = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!(
                "sqlite:{}?mode=ro",
                self.local.database_path().display()
            ))
            .await
            .expect("client SQLite inspection connection must open");
        let candidate: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM rebaseline_candidates WHERE library_id = ?")
                .bind(seed.library_id.to_string())
                .fetch_one(&sqlite)
                .await
                .expect("candidate count must be readable");
        let handoff: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM rebaseline_applied_handoffs WHERE library_id = ?",
        )
        .bind(seed.library_id.to_string())
        .fetch_one(&sqlite)
        .await
        .expect("handoff count must be readable");
        sqlite.close().await;
        (candidate, handoff)
    }

    async fn sqlite_candidate_progress(&self, seed: &LibrarySeed) -> Option<(String, i64, i64)> {
        let sqlite = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!(
                "sqlite:{}?mode=ro",
                self.local.database_path().display()
            ))
            .await
            .expect("client SQLite inspection connection must open");
        let candidate: Option<(String, i64)> = sqlx::query_as(
            "SELECT state, received_count FROM rebaseline_candidates WHERE library_id = ?",
        )
        .bind(seed.library_id.to_string())
        .fetch_optional(&sqlite)
        .await
        .expect("candidate state must be readable");
        let Some((state, received)) = candidate else {
            sqlite.close().await;
            return None;
        };
        let nodes: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM rebaseline_candidate_nodes WHERE library_id = ?",
        )
        .bind(seed.library_id.to_string())
        .fetch_one(&sqlite)
        .await
        .expect("candidate node count must be readable");
        sqlite.close().await;
        Some((state, received, nodes))
    }

    async fn cleanup(self) {
        let LiveFixture {
            database_name,
            base_url,
            pool,
            inspection,
            local,
            _server,
            _proxy,
            client_dir,
            ..
        } = self;
        drop(_proxy);
        drop(_server);
        local.close_pool().await;
        drop(local);
        pool.as_ref().clone().close().await;
        inspection.close().await;
        let maintenance = PgPool::connect(&base_url)
            .await
            .expect("cleanup maintenance connection must succeed");
        sqlx::query(&format!(
            "DROP DATABASE IF EXISTS \"{database_name}\" WITH (FORCE)"
        ))
        .execute(&maintenance)
        .await
        .expect("child database must be removed");
        maintenance.close().await;
        fs::remove_dir_all(client_dir).expect("client fixture directory must be removed");
    }
}

#[derive(Clone)]
struct LibrarySeed {
    library_id: LibraryId,
    root: Node,
}

struct ClientLibrary {
    seed: LibrarySeed,
    engine: Arc<InboundSyncEngine>,
    state: Arc<LocalStateStore>,
}

impl ClientLibrary {
    async fn seed_stale_state(&self, stale_node: Option<&Node>) {
        let root = LocalNode::new(
            self.seed.library_id,
            self.seed.root.id(),
            None,
            ManagedRelativePath::root(),
            self.seed.root.name().clone(),
            synveil_core::NodeKind::Directory,
            synveil_core::NodeState::Active,
            Revision::new(0),
            None,
            None,
            None,
            None,
            None,
            Sequence::new(0),
            true,
            None,
        );
        self.state
            .upsert_local_node(&root)
            .await
            .expect("stale root must persist");
        if let Some(node) = stale_node {
            let stale = LocalNode::new(
                self.seed.library_id,
                node.id(),
                Some(self.seed.root.id()),
                ManagedRelativePath::new("stale").expect("stale path must be valid"),
                name("stale"),
                node.kind(),
                node.state(),
                Revision::new(0),
                None,
                None,
                None,
                None,
                None,
                Sequence::new(0),
                true,
                None,
            );
            self.state
                .upsert_local_node(&stale)
                .await
                .expect("stale node must persist");
        }
        let sqlite = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!("sqlite:{}", self.state.database_path().display()))
            .await
            .expect("client SQLite update connection must open");
        sqlx::query(
            "UPDATE replicas SET root_node_id = ?, journal_epoch = 1,
             applied_sequence = 0, acknowledged_sequence = 0, status = 'IDLE'
             WHERE library_id = ?",
        )
        .bind(self.seed.root.id().to_string())
        .bind(self.seed.library_id.to_string())
        .execute(&sqlite)
        .await
        .expect("stale replica cursor must persist");
        sqlite.close().await;
    }
}

async fn create_and_apply_snapshot(
    fixture: &LiveFixture,
    client: &ClientLibrary,
) -> synveil_client_sync::RebaselineSnapshotDescriptor {
    let descriptor = fixture
        .remote
        .create_snapshot(client.engine.scope())
        .await
        .expect("direct fixture snapshot creation must succeed");
    RebaselineApplier::new(client.engine.scope(), fixture.local.clone(), 2)
        .expect("fixture applier must build")
        .apply(descriptor, fixture.remote.as_ref())
        .await
        .expect("direct fixture snapshot apply must succeed");
    descriptor
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn live_pg17_prompt87_retained_floor_empty_journal_and_epoch_recovery() {
    let fixture = LiveFixture::new("floor").await;
    let seed = fixture.create_library("floor").await;
    let first = fixture.create_directory(&seed, "first").await;
    let _second = fixture.create_directory(&seed, "second").await;
    let _third = fixture.create_directory(&seed, "third").await;
    let sync = DeviceSyncService::new(fixture.pool.as_ref().clone());
    let before = sync
        .ensure_checkpoint(fixture.owner_id, fixture.device_id, seed.library_id)
        .await
        .expect("initial checkpoint must exist");
    assert_eq!(before.acknowledged_sequence(), Sequence::new(0));

    let client = fixture.client_for(&seed, "floor").await;
    client.seed_stale_state(Some(&first)).await;
    let pending = OutboundIntent::new(
        seed.library_id,
        None,
        None,
        OutboundIntentKind::CreateDirectory,
        ManagedRelativePath::new("local-pending").unwrap(),
        None,
        Some(LocalFingerprint::directory()),
        Sequence::new(1),
        Sequence::new(1),
        None,
        None,
        None,
    )
    .expect("pending intent must build");
    fixture
        .local
        .upsert_outbound_intent(&pending)
        .await
        .expect("pending intent must persist");
    let outbound_before = fixture
        .local
        .list_pending_intents(seed.library_id)
        .await
        .expect("pending intents must be readable");

    fixture.compact(&seed).await;
    let (epoch, head, floor) = fixture.server_head(&seed).await;
    assert!(head.get() > 0);
    assert_eq!(
        floor, head,
        "empty-journal fixture must compact through head"
    );
    assert_eq!(fixture.journal_row_count(&seed).await, 0);

    fixture.proxy.reset_counts();
    let coordinator = RebaselineConvergenceCoordinator::new(
        client.engine.clone(),
        fixture.remote.clone(),
        fixture.local.clone(),
        2,
    )
    .expect("coordinator must build");
    let outcome = coordinator
        .run_convergence_once()
        .await
        .expect("stale retained cursor must converge");
    let (snapshot_id, boundary) = match outcome {
        RebaselineConvergenceOutcome::RebaselineConverged {
            snapshot_id,
            boundary,
        } => (snapshot_id, boundary),
        other => panic!("expected converged rebaseline, got {other:?}"),
    };
    assert_eq!(boundary.journal_epoch(), epoch);
    assert_eq!(boundary.resume_sequence(), head);
    assert_eq!(fixture.proxy.snapshot_posts(), 1);
    assert!(fixture.proxy.snapshot_pages() >= 2);
    assert_eq!(fixture.proxy.handoffs(), 1);
    assert_eq!(fixture.server_node_count(&seed).await, 4);
    assert_eq!(
        fixture
            .local
            .local_nodes(seed.library_id)
            .await
            .unwrap()
            .len(),
        4
    );
    assert_eq!(fixture.sqlite_counts(&seed).await, (0, 0));
    assert_eq!(
        fixture
            .local
            .list_pending_intents(seed.library_id)
            .await
            .unwrap(),
        outbound_before
    );
    let after = fixture.checkpoint(&seed).await;
    assert_eq!(after.journal_epoch(), epoch);
    assert_eq!(after.acknowledged_sequence(), head);
    let local = fixture
        .local
        .replica(seed.library_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(local.journal_epoch(), epoch);
    assert_eq!(local.applied_sequence(), head);
    assert_eq!(local.acknowledged_sequence(), head);
    assert_eq!(snapshot_id.to_string().len(), 36);
    assert_eq!(
        client.engine.synchronize_incremental_once().await.unwrap(),
        SyncOutcome::Idle
    );

    let after_floor = fixture.create_directory(&seed, "after-floor").await;
    fixture.proxy.reset_counts();
    assert_eq!(
        client.engine.synchronize_incremental_once().await.unwrap(),
        SyncOutcome::Progressed
    );
    assert_eq!(
        fixture.proxy.snapshot_posts(),
        0,
        "cursor exactly at floor must not rebaseline"
    );
    assert!(
        fixture
            .local
            .local_node(seed.library_id, after_floor.id())
            .await
            .unwrap()
            .is_some()
    );

    let after_ahead = fixture.create_directory(&seed, "after-ahead").await;
    assert_eq!(
        client.engine.synchronize_incremental_once().await.unwrap(),
        SyncOutcome::Progressed
    );
    assert_eq!(
        fixture.proxy.snapshot_posts(),
        0,
        "cursor above floor must remain incremental"
    );
    assert!(
        fixture
            .local
            .local_node(seed.library_id, after_ahead.id())
            .await
            .unwrap()
            .is_some()
    );

    let epoch_seed = fixture.create_library("epoch").await;
    let epoch_dir = fixture.create_directory(&epoch_seed, "epoch-dir").await;
    let _ = sync
        .ensure_checkpoint(fixture.owner_id, fixture.device_id, epoch_seed.library_id)
        .await
        .expect("epoch fixture checkpoint must exist");
    let epoch_client = fixture.client_for(&epoch_seed, "epoch").await;
    epoch_client.seed_stale_state(Some(&epoch_dir)).await;
    sqlx::query(
        "UPDATE libraries SET journal_epoch = 2, sync_head = 0,
         minimum_retained_sequence = 0 WHERE id = $1",
    )
    .bind(epoch_seed.library_id.into_uuid())
    .execute(&fixture.inspection)
    .await
    .expect("epoch fixture must become incompatible");
    fixture.proxy.reset_counts();
    let epoch_coordinator = RebaselineConvergenceCoordinator::new(
        epoch_client.engine.clone(),
        fixture.remote.clone(),
        fixture.local.clone(),
        2,
    )
    .unwrap();
    let epoch_outcome = epoch_coordinator
        .run_convergence_once()
        .await
        .expect("epoch mismatch must converge");
    assert!(matches!(
        epoch_outcome,
        RebaselineConvergenceOutcome::RebaselineConverged { .. }
    ));
    assert_eq!(fixture.proxy.snapshot_posts(), 1);
    assert_eq!(fixture.proxy.handoffs(), 1);
    assert_eq!(fixture.sqlite_counts(&epoch_seed).await, (0, 0));
    assert!(
        fixture
            .local
            .local_node(epoch_seed.library_id, epoch_dir.id())
            .await
            .unwrap()
            .is_some()
    );

    drop(epoch_coordinator);
    drop(epoch_client);
    drop(coordinator);
    drop(client);
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn live_pg17_prompt87_proof_loss_checkpoint_conflict_replacement_and_bounded_s3() {
    let fixture = LiveFixture::new("proof-loss").await;
    let seed = fixture.create_library("proof-loss").await;
    let first = fixture.create_directory(&seed, "first").await;
    let _second = fixture.create_directory(&seed, "second").await;
    let _third = fixture.create_directory(&seed, "third").await;
    let sync = DeviceSyncService::new(fixture.pool.as_ref().clone());
    sync.ensure_checkpoint(fixture.owner_id, fixture.device_id, seed.library_id)
        .await
        .expect("initial checkpoint must exist");
    let client = fixture.client_for(&seed, "proof-loss").await;
    client.seed_stale_state(Some(&first)).await;

    let outbound = OutboundIntent::new(
        seed.library_id,
        Some(first.id()),
        None,
        OutboundIntentKind::RenameNode,
        ManagedRelativePath::new("renamed-locally").unwrap(),
        Some(ManagedRelativePath::new("stale").unwrap()),
        Some(LocalFingerprint::directory()),
        Sequence::new(1),
        Sequence::new(1),
        Some(first.revision()),
        None,
        None,
    )
    .expect("outbound rename must build");
    fixture
        .local
        .upsert_outbound_intent(&outbound)
        .await
        .expect("outbound intent must persist");
    let outbound_before = fixture
        .local
        .list_pending_intents(seed.library_id)
        .await
        .expect("outbound intents must be readable");

    // S1 is applied but its handoff proof is then legitimately cleaned up
    // after the payload expires. The local H1 marker remains the inbound
    // fence even though neither the payload nor proof can complete handoff.
    let s1 = create_and_apply_snapshot(&fixture, &client).await;
    let (s1_payload, s1_proof) = fixture.snapshot_payload_and_proof(s1.snapshot_id()).await;
    assert_eq!((s1_payload, s1_proof), (1, 1));
    let retention = SyncRetentionService::new(fixture.pool.as_ref().clone());
    retention
        .cleanup_snapshot_payloads_step(now_plus(31))
        .await
        .expect("expired S1 payload cleanup must succeed");
    retention
        .cleanup_handoff_proofs_step(now_plus(31))
        .await
        .expect("expired S1 proof cleanup must succeed");
    assert_eq!(
        fixture.snapshot_payload_and_proof(s1.snapshot_id()).await,
        (0, 0)
    );
    assert_eq!(fixture.sqlite_counts(&seed).await, (0, 1));
    client.seed_stale_state(Some(&first)).await;

    let between = fixture.create_directory(&seed, "between").await;
    let ahead = fixture.advance_checkpoint(&seed).await;
    assert!(ahead.acknowledged_sequence() > s1.boundary().resume_sequence());

    // Pause the first S2 page so the test can inspect H1 + a FETCHING S2
    // candidate and exercise the inbound fence during recovery. The second
    // handoff is deliberately failed as a canonical 404: the invocation must
    // return DidNotConverge and must not create S3.
    fixture.proxy.reset_counts();
    fixture.proxy.hold_next_page.store(true, Ordering::SeqCst);
    fixture.proxy.fail_handoff_call.store(2, Ordering::SeqCst);
    let started = fixture.proxy.page_started.notified();
    let engine = client.engine.clone();
    let remote = fixture.remote.clone();
    let local = fixture.local.clone();
    let task = tokio::spawn(async move {
        let coordinator = RebaselineConvergenceCoordinator::new(engine, remote, local, 2)
            .expect("coordinator must build");
        coordinator.run_convergence_once().await
    });
    started.await;
    assert_eq!(fixture.sqlite_counts(&seed).await, (1, 1));
    assert!(matches!(
        client.engine.synchronize_incremental_once().await,
        Err(ClientSyncError::RebaselinePendingHandoff)
    ));
    assert_eq!(
        fixture
            .local
            .local_node(seed.library_id, first.id())
            .await
            .unwrap()
            .unwrap()
            .logical_name()
            .as_str(),
        "stale"
    );
    fixture.proxy.release_page.notify_one();
    let blocked = task
        .await
        .expect("coordinator task must join")
        .expect("bounded second conflict must be an explicit outcome");
    assert_eq!(
        blocked,
        RebaselineConvergenceOutcome::RecoveryBlocked {
            reason: synveil_client_sync::RebaselineRecoveryBlockedReason::DidNotConverge,
        }
    );
    assert_eq!(
        fixture.proxy.snapshot_posts(),
        1,
        "S2 is the only replacement POST"
    );
    assert!(fixture.proxy.snapshot_pages() >= 2);
    assert_eq!(
        fixture.proxy.handoffs(),
        2,
        "H1 and one failed H2 handoff only"
    );
    assert_eq!(fixture.sqlite_counts(&seed).await, (0, 1));
    assert!(
        fixture
            .local
            .local_node(seed.library_id, between.id())
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        fixture
            .local
            .list_pending_intents(seed.library_id)
            .await
            .unwrap(),
        outbound_before
    );

    // A later call resumes H2's already activated marker. It performs no
    // further create POST and finalizes the existing server proof/checkpoint.
    fixture.proxy.fail_handoff_call.store(0, Ordering::SeqCst);
    let retry = RebaselineConvergenceCoordinator::new(
        client.engine.clone(),
        fixture.remote.clone(),
        fixture.local.clone(),
        2,
    )
    .unwrap()
    .run_convergence_once()
    .await
    .expect("H2 retry must converge");
    assert!(matches!(
        retry,
        RebaselineConvergenceOutcome::RebaselineConverged { .. }
    ));
    assert_eq!(
        fixture.proxy.snapshot_posts(),
        1,
        "H2 retry must not create S3"
    );
    assert_eq!(fixture.proxy.handoffs(), 3);
    assert_eq!(fixture.sqlite_counts(&seed).await, (0, 0));
    let final_checkpoint = fixture.checkpoint(&seed).await;
    assert_eq!(final_checkpoint, ahead);
    let final_replica = fixture
        .local
        .replica(seed.library_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(final_replica.journal_epoch(), ahead.journal_epoch());
    assert_eq!(
        final_replica.applied_sequence(),
        ahead.acknowledged_sequence()
    );
    assert_eq!(
        final_replica.acknowledged_sequence(),
        ahead.acknowledged_sequence()
    );

    drop(client);
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn live_pg17_prompt87_retention_is_pinned_during_s2_download() {
    let fixture = LiveFixture::new("retention-race").await;
    let seed = fixture.create_library("retention-race").await;
    let first = fixture.create_directory(&seed, "first").await;
    let _second = fixture.create_directory(&seed, "second").await;
    let _third = fixture.create_directory(&seed, "third").await;
    fixture.checkpoint(&seed).await;
    let client = fixture.client_for(&seed, "retention-race").await;
    client.seed_stale_state(Some(&first)).await;

    let s1 = create_and_apply_snapshot(&fixture, &client).await;
    let s1_boundary = s1.boundary();
    client.seed_stale_state(Some(&first)).await;
    let between = fixture.create_directory(&seed, "between").await;
    let ahead = fixture.advance_checkpoint(&seed).await;
    assert!(ahead.acknowledged_sequence() > s1_boundary.resume_sequence());

    fixture.proxy.reset_counts();
    fixture.proxy.hold_next_page.store(true, Ordering::SeqCst);
    let started = fixture.proxy.page_started.notified();
    let engine = client.engine.clone();
    let remote = fixture.remote.clone();
    let local = fixture.local.clone();
    let task = tokio::spawn(async move {
        RebaselineConvergenceCoordinator::new(engine, remote, local, 2)
            .expect("coordinator must build")
            .run_convergence_once()
            .await
    });
    started.await;

    let compacted = SyncRetentionService::new(fixture.pool.as_ref().clone())
        .compact_journal_step(fixture.owner_id, seed.library_id, now_plus(31))
        .await
        .expect("retention during S2 download must complete");
    assert!(compacted.blocked_by_handoff_proof());
    assert_eq!(
        compacted.new_compacted_through(),
        s1_boundary.resume_sequence()
    );
    let (_, head, floor) = fixture.server_head(&seed).await;
    assert!(head > s1_boundary.resume_sequence());
    assert_eq!(floor, s1_boundary.resume_sequence());
    assert!(fixture.journal_row_count(&seed).await > 0);

    fixture.proxy.release_page.notify_one();
    let outcome = task
        .await
        .expect("coordinator task must join")
        .expect("retention race recovery must converge");
    assert!(matches!(
        outcome,
        RebaselineConvergenceOutcome::RebaselineConverged { .. }
    ));
    assert_eq!(fixture.proxy.snapshot_posts(), 1);
    assert_eq!(fixture.proxy.handoffs(), 2);
    assert!(
        fixture
            .local
            .local_node(seed.library_id, between.id())
            .await
            .unwrap()
            .is_some()
    );
    let checkpoint = fixture.checkpoint(&seed).await;
    assert_eq!(checkpoint, ahead);
    assert_eq!(fixture.sqlite_counts(&seed).await, (0, 0));

    let after = fixture.create_directory(&seed, "after-recovery").await;
    assert_eq!(
        client.engine.synchronize_incremental_once().await.unwrap(),
        SyncOutcome::Progressed
    );
    assert!(
        fixture
            .local
            .local_node(seed.library_id, after.id())
            .await
            .unwrap()
            .is_some()
    );
    drop(client);
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn live_pg17_prompt87_candidate_resume_after_real_page_failure() {
    let fixture = LiveFixture::new("candidate-resume").await;
    let seed = fixture.create_library("candidate-resume").await;
    let first = fixture.create_directory(&seed, "first").await;
    let _second = fixture.create_directory(&seed, "second").await;
    let _third = fixture.create_directory(&seed, "third").await;
    fixture.checkpoint(&seed).await;
    let client = fixture.client_for(&seed, "candidate-resume").await;
    client.seed_stale_state(Some(&first)).await;
    fixture.compact(&seed).await;

    fixture.proxy.reset_counts();
    fixture.proxy.fail_page_call.store(2, Ordering::SeqCst);
    let engine = client.engine.clone();
    let remote = fixture.remote.clone();
    let local = fixture.local.clone();
    let first_result = RebaselineConvergenceCoordinator::new(engine, remote, local, 2)
        .expect("coordinator must build")
        .run_convergence_once()
        .await;
    assert!(matches!(
        first_result,
        Err(ClientSyncError::Remote(error)) if error.kind() == RemoteErrorKind::Internal
    ));
    assert_eq!(fixture.proxy.snapshot_posts(), 1);
    assert_eq!(fixture.proxy.snapshot_pages(), 2);
    assert_eq!(
        fixture.sqlite_candidate_progress(&seed).await,
        Some(("FETCHING".to_owned(), 2, 2))
    );
    assert_eq!(fixture.sqlite_counts(&seed).await, (1, 0));
    assert_eq!(
        fixture
            .local
            .local_node(seed.library_id, first.id())
            .await
            .unwrap()
            .unwrap()
            .logical_name()
            .as_str(),
        "stale"
    );

    // Candidate precedence resumes the persisted descriptor/cursor. No
    // second non-idempotent create is possible, even though the first attempt
    // stopped after a real page had been committed.
    fixture.proxy.fail_page_call.store(0, Ordering::SeqCst);
    let resumed = RebaselineConvergenceCoordinator::new(
        client.engine.clone(),
        fixture.remote.clone(),
        fixture.local.clone(),
        2,
    )
    .unwrap()
    .run_convergence_once()
    .await
    .expect("persisted candidate must resume");
    assert!(matches!(
        resumed,
        RebaselineConvergenceOutcome::RebaselineConverged { .. }
    ));
    assert_eq!(fixture.proxy.snapshot_posts(), 1);
    assert_eq!(fixture.proxy.snapshot_pages(), 3);
    assert_eq!(fixture.proxy.handoffs(), 1);
    assert_eq!(fixture.sqlite_counts(&seed).await, (0, 0));
    let checkpoint = fixture.checkpoint(&seed).await;
    let local = fixture
        .local
        .replica(seed.library_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(local.applied_sequence(), checkpoint.acknowledged_sequence());
    assert_eq!(local.journal_epoch(), checkpoint.journal_epoch());
    drop(client);
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn live_pg17_prompt87_rate_limit_and_revoked_device_are_non_triggers() {
    let fixture = LiveFixture::new("rate-limit").await;
    let seed = fixture.create_library("rate-limit").await;
    let first = fixture.create_directory(&seed, "first").await;
    let _second = fixture.create_directory(&seed, "second").await;
    let _third = fixture.create_directory(&seed, "third").await;
    fixture.checkpoint(&seed).await;
    let client = fixture.client_for(&seed, "rate-limit").await;
    client.seed_stale_state(Some(&first)).await;
    fixture.compact(&seed).await;

    // Fill the existing Prompt 83C durable active-artifact budget through the
    // real authenticated HTTP create operation. The coordinator's next POST
    // must receive 429 and release its inert local claim.
    for _ in 0..8 {
        fixture
            .remote
            .create_snapshot(client.engine.scope())
            .await
            .expect("the first eight active artifacts must be admitted");
    }
    let stale_nodes_before = fixture.local.local_nodes(seed.library_id).await.unwrap();
    fixture.proxy.reset_counts();
    let limited = RebaselineConvergenceCoordinator::new(
        client.engine.clone(),
        fixture.remote.clone(),
        fixture.local.clone(),
        2,
    )
    .unwrap()
    .run_convergence_once()
    .await
    .expect("rate limiting must be a bounded recovery outcome");
    assert_eq!(
        limited,
        RebaselineConvergenceOutcome::RecoveryBlocked {
            reason: synveil_client_sync::RebaselineRecoveryBlockedReason::RateLimited,
        }
    );
    assert_eq!(fixture.proxy.snapshot_posts(), 1);
    assert_eq!(fixture.proxy.snapshot_pages(), 0);
    assert_eq!(fixture.proxy.handoffs(), 0);
    assert_eq!(fixture.sqlite_counts(&seed).await, (0, 0));
    assert_eq!(
        fixture.local.local_nodes(seed.library_id).await.unwrap(),
        stale_nodes_before
    );

    drop(client);
    fixture.cleanup().await;

    let revoked_fixture = LiveFixture::new("revoked").await;
    let revoked_seed = revoked_fixture.create_library("revoked").await;
    let revoked_first = revoked_fixture
        .create_directory(&revoked_seed, "first")
        .await;
    let _revoked_second = revoked_fixture
        .create_directory(&revoked_seed, "second")
        .await;
    revoked_fixture.checkpoint(&revoked_seed).await;
    let revoked_client = revoked_fixture.client_for(&revoked_seed, "revoked").await;
    revoked_client.seed_stale_state(Some(&revoked_first)).await;
    revoked_fixture.compact(&revoked_seed).await;
    let nodes_before = revoked_fixture
        .local
        .local_nodes(revoked_seed.library_id)
        .await
        .unwrap();
    synveil_auth::DeviceAuthenticationService::new(revoked_fixture.pool.as_ref())
        .revoke_device(revoked_fixture.owner_id, revoked_fixture.device_id)
        .await
        .expect("fixture device revocation must persist");
    revoked_fixture.proxy.reset_counts();
    let error = RebaselineConvergenceCoordinator::new(
        revoked_client.engine.clone(),
        revoked_fixture.remote.clone(),
        revoked_fixture.local.clone(),
        2,
    )
    .unwrap()
    .run_convergence_once()
    .await
    .expect_err("revoked device must not recover by snapshot");
    assert!(matches!(
        error,
        ClientSyncError::Remote(remote) if remote.kind() == RemoteErrorKind::DeviceRevoked
    ));
    assert_eq!(revoked_fixture.proxy.snapshot_posts(), 0);
    assert_eq!(revoked_fixture.sqlite_counts(&revoked_seed).await, (0, 0));
    assert_eq!(
        revoked_fixture
            .local
            .local_nodes(revoked_seed.library_id)
            .await
            .unwrap(),
        nodes_before
    );
    drop(revoked_client);
    revoked_fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn live_pg17_prompt87_same_library_concurrent_calls_have_one_recovery_owner() {
    let fixture = LiveFixture::new("concurrency").await;
    let seed = fixture.create_library("concurrency").await;
    let first = fixture.create_directory(&seed, "first").await;
    let _second = fixture.create_directory(&seed, "second").await;
    let _third = fixture.create_directory(&seed, "third").await;
    fixture.checkpoint(&seed).await;
    let client = fixture.client_for(&seed, "concurrency").await;
    client.seed_stale_state(Some(&first)).await;
    fixture.compact(&seed).await;
    fixture.proxy.reset_counts();

    let first_engine = client.engine.clone();
    let first_remote = fixture.remote.clone();
    let first_local = fixture.local.clone();
    let second_engine = client.engine.clone();
    let second_remote = fixture.remote.clone();
    let second_local = fixture.local.clone();
    let (first_result, second_result) = tokio::join!(
        async move {
            RebaselineConvergenceCoordinator::new(first_engine, first_remote, first_local, 2)
                .expect("first coordinator must build")
                .run_convergence_once()
                .await
        },
        async move {
            RebaselineConvergenceCoordinator::new(second_engine, second_remote, second_local, 2)
                .expect("second coordinator must build")
                .run_convergence_once()
                .await
        }
    );
    assert!(
        first_result.is_ok(),
        "first concurrent call must complete: {first_result:?}"
    );
    assert!(
        second_result.is_ok(),
        "second concurrent call must complete: {second_result:?}"
    );
    assert!(
        [first_result, second_result]
            .into_iter()
            .any(|result| matches!(
                result,
                Ok(RebaselineConvergenceOutcome::RebaselineConverged { .. })
            ))
    );
    assert_eq!(fixture.proxy.snapshot_posts(), 1);
    assert_eq!(fixture.proxy.handoffs(), 1);
    assert_eq!(fixture.sqlite_counts(&seed).await, (0, 0));
    let checkpoint = fixture.checkpoint(&seed).await;
    let local = fixture
        .local
        .replica(seed.library_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(local.journal_epoch(), checkpoint.journal_epoch());
    assert_eq!(local.applied_sequence(), checkpoint.acknowledged_sequence());
    drop(client);
    fixture.cleanup().await;
}
