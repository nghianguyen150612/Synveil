//! P015: real PostgreSQL 17 owner-to-byte authorization; explicitly run ignored test.

use std::{panic::AssertUnwindSafe, sync::Arc, time::Duration};

use axum::{
    body::Body,
    http::{HeaderValue, Method, Request, StatusCode},
    response::Response,
};
use futures_util::FutureExt;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use synveil_api::{ApiState, CookieConfig, EtagKey, RebaselineTokenKey, StaticReadiness, router};
use synveil_auth::{
    PasswordHasherConfig, PasswordParameters, PlaintextPassword, SessionConfig, StoredPasswordHash,
};
use synveil_core::{
    DedupDomainId, Library, LibraryId, LogicalName, LoginIdentifier, Node, NodeId, Timestamp, User,
    UserId, UserStatus,
};
use synveil_metadata::{DatabaseConfig, DatabasePool, DomainRepository, MigrationRunner};
use tokio::runtime::Handle;
use tower::ServiceExt;
use uuid::Uuid;

const TEST_PASSWORD: &str = "p015-disposable-password";
const TEST_ORIGIN: &str = "http://p015.test";

struct DisposableDatabase {
    base_url: String,
    database_name: String,
    pool: Option<Arc<DatabasePool>>,
    inspection: Option<PgPool>,
    cleaned: bool,
}

impl DisposableDatabase {
    async fn new() -> Self {
        let base_url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
            .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable PostgreSQL 17 server");
        let database_name = format!("p015_authz_{}", Uuid::now_v7().simple());
        let maintenance = PgPool::connect(&base_url)
            .await
            .expect("disposable PostgreSQL maintenance connection must succeed");
        sqlx::query(&format!("CREATE DATABASE \"{database_name}\""))
            .execute(&maintenance)
            .await
            .expect("isolated P015 database must be created");
        maintenance.close().await;

        let mut database = Self {
            base_url,
            database_name,
            pool: None,
            inspection: None,
            cleaned: false,
        };
        let url = child_database_url(&database.base_url, &database.database_name);
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
        database.pool = Some(pool);
        database.inspection = Some(
            PgPool::connect(&url)
                .await
                .expect("inspection connection must succeed"),
        );
        let version: String = sqlx::query_scalar("SHOW server_version")
            .fetch_one(database.inspection.as_ref().unwrap())
            .await
            .expect("PostgreSQL version must be readable");
        assert!(
            version.starts_with("17."),
            "P015 live authorization coverage requires PostgreSQL 17, got {version}"
        );
        database
    }

    fn pool(&self) -> &Arc<DatabasePool> {
        self.pool.as_ref().expect("database pool must be open")
    }

    fn inspection(&self) -> &PgPool {
        self.inspection
            .as_ref()
            .expect("inspection pool must be open")
    }

    async fn cleanup(mut self) {
        if let Some(pool) = self.pool.take() {
            pool.as_ref().clone().close().await;
        }
        if let Some(pool) = self.inspection.take() {
            pool.close().await;
        }
        self.drop_child_database().await;
        self.cleaned = true;
    }

    async fn drop_child_database(&self) {
        let maintenance = PgPool::connect(&self.base_url)
            .await
            .expect("cleanup maintenance connection must succeed");
        sqlx::query(&format!(
            "DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)",
            self.database_name
        ))
        .execute(&maintenance)
        .await
        .expect("isolated P015 database must be removed");
        maintenance.close().await;
    }
}

impl Drop for DisposableDatabase {
    fn drop(&mut self) {
        if self.cleaned {
            return;
        }
        let base_url = self.base_url.clone();
        let database_name = self.database_name.clone();
        if let Ok(handle) = Handle::try_current() {
            handle.spawn(async move {
                if let Ok(maintenance) = PgPool::connect(&base_url).await {
                    let _ = sqlx::query(&format!(
                        "DROP DATABASE IF EXISTS \"{database_name}\" WITH (FORCE)"
                    ))
                    .execute(&maintenance)
                    .await;
                    maintenance.close().await;
                }
            });
        }
    }
}

struct ApiFixture {
    // Field order drops router state before the database cleanup guard.
    state: ApiState,
    database: DisposableDatabase,
    password_config: PasswordHasherConfig,
}

impl ApiFixture {
    async fn new() -> Self {
        let database = DisposableDatabase::new().await;
        let password_config = PasswordHasherConfig::new(
            PasswordParameters::new(8 * 1024, 1, 1, 32)
                .expect("test Argon2 parameters must be valid"),
        )
        .expect("test password config must be valid");
        let state = ApiState::from_current_platform()
            .with_postgres_auth(
                Arc::clone(database.pool()),
                password_config,
                SessionConfig::new(Duration::from_secs(3600))
                    .expect("test session duration must be valid"),
                RebaselineTokenKey::from_bytes([0x50; 32]),
            )
            .with_readiness(Arc::new(StaticReadiness::new(true)))
            .with_cookie_config(CookieConfig::development())
            .with_allowed_origin(TEST_ORIGIN)
            .with_etag_key(EtagKey::from_bytes([0x51; 32]));
        Self {
            state,
            database,
            password_config,
        }
    }

    async fn cleanup(self) {
        let Self {
            state,
            database,
            password_config: _,
        } = self;
        drop(state);
        database.cleanup().await;
    }
}

struct Browser {
    session: String,
    csrf: String,
    owner: UserId,
}

fn child_database_url(base: &str, database_name: &str) -> String {
    let (origin, _) = base
        .rsplit_once('/')
        .expect("test database URL must contain a database path");
    format!("{origin}/{database_name}")
}

fn json_request(method: Method, uri: &str, body: Option<Value>) -> Request<Body> {
    let has_body = body.is_some();
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
        .expect("test request must be valid");
    if has_body {
        request
            .headers_mut()
            .insert("content-type", HeaderValue::from_static("application/json"));
    }
    request
}

fn browser_request(
    method: Method,
    uri: &str,
    body: Option<Value>,
    browser: &Browser,
) -> Request<Body> {
    let mut request = json_request(method, uri, body);
    request.headers_mut().insert(
        "cookie",
        HeaderValue::from_str(&format!(
            "synveil_session={}; synveil_csrf={}",
            browser.session, browser.csrf
        ))
        .expect("synthetic browser cookies must be valid headers"),
    );
    request
}

fn browser_mutation_request(
    method: Method,
    uri: &str,
    body: Option<Value>,
    browser: &Browser,
) -> Request<Body> {
    let mut request = browser_request(method, uri, body, browser);
    request.headers_mut().insert(
        "x-csrf-token",
        HeaderValue::from_str(&browser.csrf).expect("synthetic CSRF token must be valid"),
    );
    request
        .headers_mut()
        .insert("origin", HeaderValue::from_static(TEST_ORIGIN));
    request
        .headers_mut()
        .insert("sec-fetch-site", HeaderValue::from_static("same-origin"));
    request
}

async fn call(state: &ApiState, request: Request<Body>) -> Response {
    router(state.clone())
        .oneshot(request)
        .await
        .expect("in-process API router must respond")
}

fn cookie_value(response: &Response, name: &str) -> String {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .find_map(|value| {
            let value = value.to_str().ok()?;
            let (cookie, _) = value.split_once(';')?;
            let (cookie_name, cookie_value) = cookie.split_once('=')?;
            (cookie_name == name).then_some(cookie_value.to_owned())
        })
        .expect("authentication response must set the requested cookie")
}

async fn json_body(response: Response) -> Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("response body must be readable")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body must be JSON")
}

async fn login_browser(state: &ApiState, login: &str, password: &str) -> Browser {
    let response = call(
        state,
        json_request(
            Method::POST,
            "/api/v1/auth/login",
            Some(json!({"login": login, "login_key": login, "password": password})),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let session = cookie_value(&response, "synveil_session");
    let csrf = cookie_value(&response, "synveil_csrf");
    let body = json_body(response).await;
    let owner = body["data"]["user_id"]
        .as_str()
        .expect("login response must include a user ID")
        .parse()
        .expect("user ID must be canonical");
    Browser {
        session,
        csrf,
        owner,
    }
}

async fn create_standard_owner(fixture: &ApiFixture, label: &str) -> (Browser, LibraryId, NodeId) {
    let user_id = UserId::new();
    let login = format!("p015-{label}-{user_id}");
    let login_identifier =
        LoginIdentifier::new(&login, &login).expect("synthetic owner login must be valid");
    let observed_at = fixture_timestamp();
    let user = User::new(user_id, login_identifier, UserStatus::Active, observed_at);
    DomainRepository::new(fixture.database.pool().as_ref())
        .insert_user(&user)
        .await
        .expect("second synthetic owner must persist");

    let password =
        PlaintextPassword::new(TEST_PASSWORD).expect("synthetic test password must be valid");
    let hash = StoredPasswordHash::hash(fixture.password_config, &password)
        .expect("synthetic test password hash must be generated");
    sqlx::query(
        "INSERT INTO user_credentials (user_id, password_hash, created_at, updated_at)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(user_id.into_uuid())
    .bind(hash.as_phc_str())
    .bind(observed_at.as_offset_datetime())
    .bind(observed_at.as_offset_datetime())
    .execute(fixture.database.inspection())
    .await
    .expect("second synthetic owner password must persist");

    let library_id = LibraryId::new();
    let root = Node::new_root(
        NodeId::new(),
        library_id,
        LogicalName::new(format!("{label}-root")).expect("root name must be valid"),
        observed_at,
    );
    let library = Library::new(
        library_id,
        user_id,
        LogicalName::new(format!("{label}-library")).expect("library name must be valid"),
        &root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("foreign library fixture must be valid");
    DomainRepository::new(fixture.database.pool().as_ref())
        .insert_library_with_root(&library, &root)
        .await
        .expect("second owner library and root must persist");
    let browser = login_browser(&fixture.state, &login, TEST_PASSWORD).await;
    assert_eq!(browser.owner, user_id);
    (browser, library_id, root.id())
}

fn fixture_timestamp() -> Timestamp {
    Timestamp::parse("2026-09-15T00:00:00.123456Z")
        .expect("fixture timestamp must have PostgreSQL precision")
}

use bytes::Bytes;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicUsize, Ordering};
use synveil_core::{
    FileVersion, FileVersionId, NodeKind, ObjectId, ObjectReference, ObjectReplicaId, Sha256Digest,
};
use synveil_metadata::PostgresContentReadRepository;
use synveil_storage::*;

// Delegates every operation to the real local adapter; counts every read attempt,
// including failed calls. It never makes an authorization decision.
struct ObservedStore {
    inner: Arc<dyn ObjectStore>,
    gets: AtomicUsize,
    ranges: AtomicUsize,
}
impl ObservedStore {
    fn counts(&self) -> (usize, usize) {
        (
            self.gets.load(Ordering::SeqCst),
            self.ranges.load(Ordering::SeqCst),
        )
    }
}
#[async_trait::async_trait]
impl ObjectStore for ObservedStore {
    fn capabilities(&self) -> StorageCapabilities {
        self.inner.capabilities()
    }
    async fn put(&self, request: PutRequest) -> Result<ObjectMetadata, ObjectStoreError> {
        self.inner.put(request).await
    }
    async fn begin_staged_write(&self) -> Result<StagingHandle, ObjectStoreError> {
        self.inner.begin_staged_write().await
    }
    async fn write_staged(
        &self,
        handle: &StagingHandle,
        body: ByteStream,
        integrity: IntegrityExpectation,
    ) -> Result<StagedMetadata, ObjectStoreError> {
        self.inner.write_staged(handle, body, integrity).await
    }
    async fn append_staged(
        &self,
        handle: &StagingHandle,
        expected_offset: u64,
        chunk: Bytes,
        maximum_length: u64,
    ) -> Result<StagingProgress, ObjectStoreError> {
        self.inner
            .append_staged(handle, expected_offset, chunk, maximum_length)
            .await
    }
    async fn staging_progress(
        &self,
        handle: &StagingHandle,
    ) -> Result<StagingProgress, ObjectStoreError> {
        self.inner.staging_progress(handle).await
    }
    async fn finalize_staged(
        &self,
        handle: &StagingHandle,
        integrity: IntegrityExpectation,
    ) -> Result<StagedMetadata, ObjectStoreError> {
        self.inner.finalize_staged(handle, integrity).await
    }
    async fn promote_temp(
        &self,
        handle: &StagingHandle,
        destination: &ObjectKey,
    ) -> Result<PromotionReceipt, ObjectStoreError> {
        self.inner.promote_temp(handle, destination).await
    }
    async fn abort_staged(&self, handle: &StagingHandle) -> Result<(), ObjectStoreError> {
        self.inner.abort_staged(handle).await
    }
    async fn get(&self, key: &ObjectKey) -> Result<ObjectRead, ObjectStoreError> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        self.inner.get(key).await
    }
    async fn range_read(
        &self,
        key: &ObjectKey,
        range: ByteRange,
    ) -> Result<ObjectRead, ObjectStoreError> {
        self.ranges.fetch_add(1, Ordering::SeqCst);
        self.inner.range_read(key, range).await
    }
    async fn exists(&self, key: &ObjectKey) -> Result<bool, ObjectStoreError> {
        self.inner.exists(key).await
    }
    async fn metadata(&self, key: &ObjectKey) -> Result<ObjectMetadata, ObjectStoreError> {
        self.inner.metadata(key).await
    }
    async fn delete(&self, key: &ObjectKey) -> Result<DeleteOutcome, ObjectStoreError> {
        self.inner.delete(key).await
    }
    async fn conditional_delete(
        &self,
        key: &ObjectKey,
        expected_version: &ObjectVersion,
    ) -> Result<DeleteOutcome, ObjectStoreError> {
        self.inner.conditional_delete(key, expected_version).await
    }
}

struct FileFixture {
    node: NodeId,
    versions: Vec<FileVersionId>,
    keys: Vec<ObjectKey>,
    bodies: Vec<Vec<u8>>,
}

async fn seed_file(
    f: &ApiFixture,
    library_id: LibraryId,
    root_id: NodeId,
    store: &ObservedStore,
) -> FileFixture {
    let repo = DomainRepository::new(f.database.pool().as_ref());
    // Read the canonical library/root rather than inventing an authorization projection.
    let root = repo.find_node(root_id).await.unwrap().unwrap();
    let library = repo.find_library(library_id).await.unwrap().unwrap();
    let mut node = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::File,
        LogicalName::new("private.txt").unwrap(),
        fixture_timestamp(),
    )
    .unwrap();
    repo.insert_node(&node).await.unwrap();
    let mut result = FileFixture {
        node: node.id(),
        versions: vec![],
        keys: vec![],
        bodies: vec![],
    };
    for i in 0..2 {
        let bytes = format!("p015-private-content-{i}-{}", Uuid::now_v7()).into_bytes();
        let sha = Sha256Digest::try_from(Sha256::digest(&bytes).as_slice()).unwrap();
        let object = ObjectReference::new(
            ObjectId::new(),
            library.dedup_domain_id(),
            sha,
            bytes.len() as u64,
        );
        let key = ObjectKey::new(format!("objects/v1/{}", object.object_id())).unwrap();
        store
            .put(
                PutRequest::new(
                    key.clone(),
                    boxed_stream(futures_util::stream::iter([Ok(Bytes::from(bytes.clone()))])),
                )
                .with_integrity(
                    IntegrityExpectation::none()
                        .with_length(bytes.len() as u64)
                        .with_sha256(sha),
                ),
            )
            .await
            .unwrap();
        repo.insert_object(object, fixture_timestamp())
            .await
            .unwrap();
        sqlx::query("INSERT INTO object_replicas (id, object_id, object_dedup_domain_id, backend_kind, storage_key, stored_length, stored_sha256, state, created_at, verified_at) VALUES ($1,$2,$3,'LOCAL_FILESYSTEM',$4,$5::NUMERIC,$6,'VERIFIED',$7,$7)")
            .bind(ObjectReplicaId::new().into_uuid()).bind(object.object_id().into_uuid())
            .bind(object.dedup_domain_id().into_uuid()).bind(key.as_str()).bind(bytes.len().to_string())
            .bind(sha.as_bytes().to_vec()).bind(fixture_timestamp().as_offset_datetime())
            .execute(f.database.inspection()).await.unwrap();
        let version = FileVersion::new(
            FileVersionId::new(),
            &library,
            &node,
            object,
            result.versions.last().copied(),
            fixture_timestamp(),
        )
        .unwrap();
        repo.insert_file_version(version).await.unwrap();
        node = node
            .with_current_version(&version, fixture_timestamp())
            .unwrap();
        repo.update_node(&node).await.unwrap();
        result.versions.push(version.id());
        result.keys.push(key);
        result.bodies.push(bytes);
    }
    result
}

async fn body(response: Response) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec()
}

async fn denied(
    f: &ApiFixture,
    store: &ObservedStore,
    file: &FileFixture,
    request: Request<Body>,
    status: StatusCode,
) {
    let before = store.counts();
    let response = call(&f.state, request).await;
    assert_eq!(response.status(), status);
    for header in [
        "etag",
        "content-range",
        "last-modified",
        "content-disposition",
        "accept-ranges",
    ] {
        assert!(
            !response.headers().contains_key(header),
            "denial exposed {header}"
        );
    }
    let bytes = body(response).await;
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        json["error"]["code"],
        if status == StatusCode::NOT_FOUND {
            "not_found"
        } else {
            "authentication_failed"
        }
    );
    let text = String::from_utf8(bytes).unwrap();
    for (key, private) in file.keys.iter().zip(&file.bodies) {
        assert!(!text.contains(key.as_str()));
        assert!(!text.contains(std::str::from_utf8(private).unwrap()));
        assert!(!text.contains(&format!("{:x}", Sha256::digest(private))));
    }
    for field in [
        "storage_key",
        "backend_version",
        "stored_sha256",
        "object_id",
    ] {
        assert!(!text.contains(field));
    }
    assert_eq!(
        store.counts(),
        before,
        "denial attempted physical storage read"
    );
}

async fn assert_downloads(
    f: &ApiFixture,
    store: &ObservedStore,
    owner: &Browser,
    file: &FileFixture,
) -> Vec<String> {
    let paths = [
        format!("/api/v1/nodes/{}/content", file.node),
        format!("/api/v1/versions/{}/content", file.versions[0]),
        format!("/api/v1/versions/{}/content", file.versions[1]),
    ];
    let mut etags = vec![];
    for (path, index) in paths.iter().zip([1, 0, 1]) {
        let before = store.counts();
        let response = call(&f.state, browser_request(Method::GET, path, None, owner)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["content-length"],
            file.bodies[index].len().to_string()
        );
        let etag = response.headers()["etag"].to_str().unwrap().to_owned();
        assert_eq!(body(response).await, file.bodies[index]);
        assert_eq!(store.counts(), (before.0 + 1, before.1));
        for (range, start, end) in [
            ("bytes=0-3", 0, 4),
            ("bytes=4-", 4, file.bodies[index].len()),
            (
                "bytes=-4",
                file.bodies[index].len() - 4,
                file.bodies[index].len(),
            ),
        ] {
            let mut request = browser_request(Method::GET, path, None, owner);
            request
                .headers_mut()
                .insert("range", HeaderValue::from_static(range));
            let response = call(&f.state, request).await;
            assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
            assert_eq!(
                response.headers()["content-range"],
                format!("bytes {start}-{}/{}", end - 1, file.bodies[index].len())
            );
            assert_eq!(body(response).await, file.bodies[index][start..end]);
        }
        assert_eq!(store.counts(), (before.0 + 1, before.1 + 3));
        let mut request = browser_request(Method::GET, path, None, owner);
        request
            .headers_mut()
            .insert("if-none-match", HeaderValue::from_str(&etag).unwrap());
        let before = store.counts();
        let response = call(&f.state, request).await;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert!(body(response).await.is_empty());
        assert_eq!(store.counts(), before);
        etags.push(etag);
    }
    etags
}

async fn snapshot(f: &ApiFixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('nodes',(SELECT jsonb_agg(to_jsonb(n) ORDER BY id) FROM nodes n),'versions',(SELECT jsonb_agg(to_jsonb(v) ORDER BY id) FROM file_versions v),'objects',(SELECT jsonb_agg(to_jsonb(o) ORDER BY id) FROM objects o),'replicas',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM object_replicas r))")
        .fetch_one(f.database.inspection()).await.unwrap()
}

#[tokio::test]
#[ignore = "requires SYNVEIL_TEST_DATABASE_URL pointing to disposable PostgreSQL 17"]
async fn p015_postgres_objectstore_owner_to_byte() {
    let mut fixture = ApiFixture::new().await;
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(ObservedStore {
        inner: Arc::new(open_local_object_store(root.path()).unwrap()),
        gets: AtomicUsize::new(0),
        ranges: AtomicUsize::new(0),
    });
    fixture.state =
        fixture
            .state
            .clone()
            .with_download_backend(Arc::new(ContentReadApplicationService::new(
                Arc::new(PostgresContentReadRepository::new(
                    fixture.database.pool().as_ref().clone(),
                )),
                store.clone(),
            )));
    let result = AssertUnwindSafe(run_matrix(&fixture, &store))
        .catch_unwind()
        .await;

    drop(store);
    fixture.cleanup().await;
    root.close().unwrap();
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
}

async fn run_matrix(fixture: &ApiFixture, store: &ObservedStore) {
    let (a, la, ra) = create_standard_owner(fixture, "owner-a").await;
    let (b, lb, rb) = create_standard_owner(fixture, "owner-b").await;
    let file = seed_file(fixture, la, ra, store).await;
    let other = seed_file(fixture, lb, rb, store).await;
    assert_ne!(file.keys[0], file.keys[1]);
    let persisted = snapshot(fixture).await;
    let etags = assert_downloads(fixture, store, &a, &file).await;
    let paths = [
        format!("/api/v1/nodes/{}/content", file.node),
        format!("/api/v1/versions/{}/content", file.versions[0]),
        format!("/api/v1/versions/{}/content", file.versions[1]),
    ];
    for (path, etag) in paths.iter().zip(&etags) {
        for headers in [
            vec![],
            vec![("range", "bytes=0-3")],
            vec![("if-none-match", etag.as_str())],
            vec![("if-none-match", etag.as_str()), ("range", "bytes=0-3")],
            vec![("if-range", etag.as_str()), ("range", "bytes=0-3")],
        ] {
            let mut request = browser_request(Method::GET, path, None, &b);
            for (name, value) in headers {
                request
                    .headers_mut()
                    .insert(name, HeaderValue::from_str(value).unwrap());
            }
            denied(fixture, store, &file, request, StatusCode::NOT_FOUND).await;
        }
        denied(
            fixture,
            store,
            &file,
            json_request(Method::GET, path, None),
            StatusCode::UNAUTHORIZED,
        )
        .await;
        let mut request = json_request(Method::GET, path, None);
        request.headers_mut().insert(
            "cookie",
            HeaderValue::from_static("synveil_session=invalid"),
        );
        denied(fixture, store, &file, request, StatusCode::UNAUTHORIZED).await;
    }
    let list = format!("/api/v1/nodes/{}/versions?limit=1", file.node);
    let response = call(
        &fixture.state,
        browser_request(Method::GET, &list, None, &a),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let json = json_body(response).await;
    let cursor = json["page"]["next_cursor"].as_str().unwrap();
    for path in [
        list,
        format!("/api/v1/nodes/{}/versions?cursor={cursor}", file.node),
        format!("/api/v1/versions/{}", file.versions[0]),
        format!("/api/v1/versions/{}", file.versions[1]),
    ] {
        denied(
            fixture,
            store,
            &file,
            browser_request(Method::GET, &path, None, &b),
            StatusCode::NOT_FOUND,
        )
        .await;
    }
    // A copied cursor on B's own node must not enumerate A's history.
    let response = call(
        &fixture.state,
        browser_request(
            Method::GET,
            &format!("/api/v1/nodes/{}/versions?cursor={cursor}", other.node),
            None,
            &b,
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let text = String::from_utf8(body(response).await).unwrap();
    for id in &file.versions {
        assert!(!text.contains(&id.to_string()));
    }
    for path in [
        format!("/api/v1/nodes/{}/content", NodeId::new()),
        format!("/api/v1/versions/{}/content", FileVersionId::new()),
    ] {
        denied(
            fixture,
            store,
            &file,
            browser_request(Method::GET, &path, None, &a),
            StatusCode::NOT_FOUND,
        )
        .await;
    }
    assert_downloads(fixture, store, &a, &file).await;
    assert_downloads(fixture, store, &b, &other).await;
    assert_eq!(snapshot(fixture).await, persisted);
    // Deleted node conceals both current and historical content before store access.
    sqlx::query("UPDATE nodes SET state='TRASHED', trashed_at=CURRENT_TIMESTAMP WHERE id=$1")
        .bind(file.node.into_uuid())
        .execute(fixture.database.inspection())
        .await
        .unwrap();
    for path in &paths {
        denied(
            fixture,
            store,
            &file,
            browser_request(Method::GET, path, None, &a),
            StatusCode::NOT_FOUND,
        )
        .await;
    }
    sqlx::query("UPDATE nodes SET state='ACTIVE', trashed_at=NULL WHERE id=$1")
        .bind(file.node.into_uuid())
        .execute(fixture.database.inspection())
        .await
        .unwrap();
    // A missing head fails closed, even while a historical object exists.
    sqlx::query("UPDATE nodes SET current_version_id=NULL WHERE id=$1")
        .bind(file.node.into_uuid())
        .execute(fixture.database.inspection())
        .await
        .unwrap();
    let before = store.counts();
    let response = call(
        &fixture.state,
        browser_request(Method::GET, &paths[0], None, &a),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let text = String::from_utf8(body(response).await).unwrap();
    assert!(!text.contains(file.keys[0].as_str()));
    assert_eq!(store.counts(), before);
    sqlx::query("UPDATE nodes SET current_version_id=$2 WHERE id=$1")
        .bind(file.node.into_uuid())
        .bind(file.versions[1].into_uuid())
        .execute(fixture.database.inspection())
        .await
        .unwrap();
    // Removing replica eligibility must never fall back to a different object's key.
    sqlx::query("UPDATE object_replicas SET state='MISSING' WHERE storage_key=$1")
        .bind(file.keys[1].as_str())
        .execute(fixture.database.inspection())
        .await
        .unwrap();
    for path in [&paths[0], &paths[2]] {
        let before = store.counts();
        let response = call(&fixture.state, browser_request(Method::GET, path, None, &a)).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(!response.headers().contains_key("etag"));
        assert!(!response.headers().contains_key("content-range"));
        let text = String::from_utf8(body(response).await).unwrap();
        for key in &file.keys {
            assert!(!text.contains(key.as_str()));
        }
        assert_eq!(store.counts(), before);
    }
    sqlx::query("UPDATE object_replicas SET state='VERIFIED' WHERE storage_key=$1")
        .bind(file.keys[1].as_str())
        .execute(fixture.database.inspection())
        .await
        .unwrap();
    // Relational constraints reject a head pointer into another owner's node.
    let rejected = sqlx::query("UPDATE nodes SET current_version_id=$2 WHERE id=$1")
        .bind(file.node.into_uuid())
        .bind(other.versions[0].into_uuid())
        .execute(fixture.database.inspection())
        .await;
    assert!(
        rejected
            .unwrap_err()
            .as_database_error()
            .unwrap()
            .is_foreign_key_violation()
    );
    // Integrity-inconsistent replica metadata must fail before any store read.
    sqlx::query("UPDATE object_replicas SET stored_length=stored_length+1 WHERE storage_key=$1")
        .bind(file.keys[1].as_str())
        .execute(fixture.database.inspection())
        .await
        .unwrap();
    for path in [&paths[0], &paths[2]] {
        let before = store.counts();
        let response = call(&fixture.state, browser_request(Method::GET, path, None, &a)).await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!response.headers().contains_key("etag"));
        assert!(!response.headers().contains_key("content-range"));
        let text = String::from_utf8(body(response).await).unwrap();
        for (key, bytes) in file.keys.iter().zip(&file.bodies) {
            assert!(!text.contains(key.as_str()));
            assert!(!text.contains(std::str::from_utf8(bytes).unwrap()));
        }
        assert_eq!(store.counts(), before);
    }
    sqlx::query("UPDATE object_replicas SET stored_length=stored_length-1 WHERE storage_key=$1")
        .bind(file.keys[1].as_str())
        .execute(fixture.database.inspection())
        .await
        .unwrap();
    // Logout revokes a genuine session; replay it against all byte endpoints.
    let response = call(
        &fixture.state,
        browser_mutation_request(Method::POST, "/api/v1/auth/logout", None, &b),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    for path in &paths {
        denied(
            fixture,
            store,
            &file,
            browser_request(Method::GET, path, None, &b),
            StatusCode::UNAUTHORIZED,
        )
        .await;
    }
    assert_downloads(fixture, store, &a, &file).await;
    assert_eq!(snapshot(fixture).await, persisted);
    println!(
        "P015_MATRIX_COMPLETE: PostgreSQL 17 + production router + local ObjectStore; no unauthorized get/range calls"
    );
}
