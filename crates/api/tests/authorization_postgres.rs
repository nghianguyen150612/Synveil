//! P005 legacy authorization regression probe.
//!
//! Run against a disposable PostgreSQL 17 server with:
//!
//!     SYNVEIL_TEST_DATABASE_URL=postgresql://... cargo test -p synveil-api \
//!       --test authorization_postgres --locked -- --ignored --nocapture
//!
//! This target uses a fresh child database, the real Axum router, PostgreSQL
//! authentication/device backends, and real metadata/sync/mutation services.
//! It is intentionally test-only. The final assertion records a suspected
//! legacy device-binding defect without changing production behavior.

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
    PasswordHasherConfig, PasswordParameters, PlaintextPassword, SessionConfig, SessionToken,
    StoredPasswordHash,
};
use synveil_core::{
    DedupDomainId, DeviceCredentialId, DeviceCredentialSecret, DeviceId, Library, LibraryId,
    LogicalName, LoginIdentifier, Node, NodeId, Revision, Timestamp, User, UserId, UserStatus,
};
use synveil_metadata::{DatabaseConfig, DatabasePool, DomainRepository, MigrationRunner};
use tokio::runtime::Handle;
use tower::ServiceExt;
use uuid::Uuid;

const TEST_PASSWORD: &str = "p005-disposable-password";
const TEST_ORIGIN: &str = "http://p005.test";

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
        let database_name = format!("p005_authz_{}", Uuid::now_v7().simple());
        let maintenance = PgPool::connect(&base_url)
            .await
            .expect("disposable PostgreSQL maintenance connection must succeed");
        sqlx::query(&format!("CREATE DATABASE \"{database_name}\""))
            .execute(&maintenance)
            .await
            .expect("isolated P005 database must be created");
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
            "P005 live authorization coverage requires PostgreSQL 17, got {version}"
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
        .expect("isolated P005 database must be removed");
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
    login: String,
    session: String,
    csrf: String,
    owner: UserId,
}

struct DeviceCredential {
    id: DeviceId,
    credential_id: DeviceCredentialId,
    secret: DeviceCredentialSecret,
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

fn device_request(
    method: Method,
    uri: &str,
    body: Option<Value>,
    device: &DeviceCredential,
) -> Request<Body> {
    let mut request = json_request(method, uri, body);
    let mut value = HeaderValue::from_str(&format!("Bearer {}", device.secret.expose_secret()))
        .expect("synthetic device bearer must be valid");
    value.set_sensitive(true);
    request.headers_mut().insert("authorization", value);
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

async fn bootstrap_and_login(state: &ApiState, label: &str) -> Browser {
    let login = format!("p005-{label}-{}", Uuid::now_v7().simple());
    let body = json!({
        "login": login,
        "login_key": login,
        "password": TEST_PASSWORD
    });
    let response = call(
        state,
        json_request(Method::POST, "/api/v1/bootstrap/admin", Some(body)),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    login_browser(state, &login, TEST_PASSWORD).await
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
        login: login.to_owned(),
        session,
        csrf,
        owner,
    }
}

async fn create_standard_owner(fixture: &ApiFixture, label: &str) -> (Browser, LibraryId, NodeId) {
    let user_id = UserId::new();
    let login = format!("p005-{label}-{user_id}");
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

async fn enroll_device(state: &ApiState, browser: &Browser, label: &str) -> DeviceCredential {
    let response = call(
        state,
        browser_mutation_request(
            Method::POST,
            "/api/v1/devices/enrollment-grants",
            Some(json!({"target":{"kind":"new","display_name":label}})),
            browser,
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let grant = json_body(response).await;
    let device_id: DeviceId = grant["data"]["device_id"]
        .as_str()
        .expect("grant must return device ID")
        .parse()
        .expect("device ID must be canonical");
    let token = grant["data"]["enrollment_token"]
        .as_str()
        .expect("grant must return one-time enrollment token");
    let response = call(
        state,
        json_request(
            Method::POST,
            "/api/v1/device-enrollment/exchange",
            Some(json!({"enrollment_token":token})),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let credential = json_body(response).await;
    let credential_device_id: DeviceId = credential["data"]["device_id"]
        .as_str()
        .expect("exchange must return device ID")
        .parse()
        .expect("device ID must be canonical");
    assert_eq!(credential_device_id, device_id);
    DeviceCredential {
        id: credential_device_id,
        credential_id: credential["data"]["credential_id"]
            .as_str()
            .expect("exchange must return credential ID")
            .parse()
            .expect("credential ID must be canonical"),
        secret: DeviceCredentialSecret::parse(
            credential["data"]["device_credential"]
                .as_str()
                .expect("exchange must return device credential"),
        )
        .expect("synthetic device credential must have canonical form"),
    }
}

fn fixture_timestamp() -> Timestamp {
    Timestamp::parse("2026-09-15T00:00:00.123456Z")
        .expect("fixture timestamp must have PostgreSQL precision")
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a disposable PostgreSQL 17 server"]
async fn p005_owner_and_device_authorization_probe_reveals_mutation_binding_gap() {
    let fixture = ApiFixture::new().await;
    let result = AssertUnwindSafe(async {
    let owner_a = bootstrap_and_login(&fixture.state, "owner-a").await;
    let owner_a_catalog = call(
        &fixture.state,
        browser_request(Method::GET, "/api/v1/libraries", None, &owner_a),
    )
    .await;
    assert_eq!(owner_a_catalog.status(), StatusCode::OK);
    let owner_a_catalog = json_body(owner_a_catalog).await;
    assert_eq!(owner_a_catalog["data"].as_array().unwrap().len(), 1);
    let owner_a_library: LibraryId = owner_a_catalog["data"][0]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let owner_a_root: NodeId = owner_a_catalog["data"][0]["attributes"]["root_node_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    let (owner_b, owner_b_library, owner_b_root) = create_standard_owner(&fixture, "owner-b").await;
    let owner_b_self_read = call(
        &fixture.state,
        browser_request(
            Method::GET,
            &format!("/api/v1/nodes/{owner_b_root}"),
            None,
            &owner_b,
        ),
    )
    .await;
    assert_eq!(owner_b_self_read.status(), StatusCode::OK);
    let owner_a_foreign_node = call(
        &fixture.state,
        browser_request(
            Method::GET,
            &format!("/api/v1/nodes/{owner_b_root}"),
            None,
            &owner_a,
        ),
    )
    .await;
    assert_eq!(owner_a_foreign_node.status(), StatusCode::NOT_FOUND);
    let owner_a_foreign_library = call(
        &fixture.state,
        browser_request(
            Method::GET,
            &format!("/api/v1/libraries/{owner_b_library}/nodes"),
            None,
            &owner_a,
        ),
    )
    .await;
    assert_eq!(owner_a_foreign_library.status(), StatusCode::NOT_FOUND);
    let catalog = call(
        &fixture.state,
        browser_request(Method::GET, "/api/v1/libraries", None, &owner_a),
    )
    .await;
    let catalog = json_body(catalog).await;
    assert_eq!(catalog["data"].as_array().unwrap().len(), 1);
    assert_eq!(catalog["data"][0]["id"], owner_a_library.to_string());

    let owner_a_device_one = enroll_device(&fixture.state, &owner_a, "owner-a-device-one").await;
    let owner_a_device_two = enroll_device(&fixture.state, &owner_a, "owner-a-device-two").await;
    let owner_b_device = enroll_device(&fixture.state, &owner_b, "owner-b-device").await;
    assert_ne!(owner_a_device_one.id, owner_a_device_two.id);
    assert_ne!(
        owner_a_device_one.credential_id,
        owner_a_device_two.credential_id
    );

    let missing_auth = call(
        &fixture.state,
        json_request(Method::GET, "/api/v1/libraries", None),
    )
    .await;
    assert_eq!(missing_auth.status(), StatusCode::UNAUTHORIZED);
    let invalid_credential = call(&fixture.state, {
        let mut request = json_request(Method::GET, "/api/v1/libraries", None);
        request
            .headers_mut()
            .insert("authorization", HeaderValue::from_static("Bearer invalid"));
        request
    })
    .await;
    assert_eq!(invalid_credential.status(), StatusCode::UNAUTHORIZED);
    let device_bearer_on_browser_only_route = call(
        &fixture.state,
        device_request(
            Method::GET,
            "/api/v1/auth/session",
            None,
            &owner_a_device_one,
        ),
    )
    .await;
    assert_eq!(
        device_bearer_on_browser_only_route.status(),
        StatusCode::UNAUTHORIZED
    );
    let browser_session_view = call(
        &fixture.state,
        browser_request(Method::GET, "/api/v1/auth/session", None, &owner_a),
    )
    .await;
    assert_eq!(browser_session_view.status(), StatusCode::OK);
    let browser_session_view = json_body(browser_session_view).await;
    assert!(browser_session_view["data"].get("device_id").is_none());

    let authorized_checkpoint = call(
        &fixture.state,
        device_request(
            Method::GET,
            &format!(
                "/api/v1/devices/{}/libraries/{owner_a_library}/checkpoint",
                owner_a_device_two.id
            ),
            None,
            &owner_a_device_two,
        ),
    )
    .await;
    assert_eq!(authorized_checkpoint.status(), StatusCode::OK);
    let owner_session_on_device_scoped_read = call(
        &fixture.state,
        browser_request(
            Method::GET,
            &format!(
                "/api/v1/devices/{}/libraries/{owner_a_library}/checkpoint",
                owner_a_device_two.id
            ),
            None,
            &owner_a,
        ),
    )
    .await;
    assert_eq!(owner_session_on_device_scoped_read.status(), StatusCode::OK);
    let owner_session_checkpoint = json_body(owner_session_on_device_scoped_read).await;
    assert_eq!(
        owner_session_checkpoint["data"]["device_id"],
        owner_a_device_two.id.to_string()
    );
    let bearer_claiming_other_device = call(
        &fixture.state,
        device_request(
            Method::GET,
            &format!(
                "/api/v1/devices/{}/libraries/{owner_a_library}/checkpoint",
                owner_a_device_two.id
            ),
            None,
            &owner_a_device_one,
        ),
    )
    .await;
    assert_eq!(bearer_claiming_other_device.status(), StatusCode::NOT_FOUND);
    let mut mixed_browser_and_device = device_request(
        Method::GET,
        &format!(
            "/api/v1/devices/{}/libraries/{owner_a_library}/checkpoint",
            owner_a_device_one.id
        ),
        None,
        &owner_a_device_one,
    );
    mixed_browser_and_device.headers_mut().insert(
        "cookie",
        HeaderValue::from_str(&format!(
            "synveil_session={}; synveil_csrf={}",
            owner_a.session, owner_a.csrf
        ))
        .expect("synthetic browser cookies must be valid headers"),
    );
    let mixed_browser_and_device = call(&fixture.state, mixed_browser_and_device).await;
    assert_eq!(
        mixed_browser_and_device.status(),
        StatusCode::UNAUTHORIZED
    );
    let foreign_library_checkpoint = call(
        &fixture.state,
        device_request(
            Method::GET,
            &format!(
                "/api/v1/devices/{}/libraries/{owner_b_library}/checkpoint",
                owner_a_device_one.id
            ),
            None,
            &owner_a_device_one,
        ),
    )
    .await;
    assert_eq!(foreign_library_checkpoint.status(), StatusCode::NOT_FOUND);

    let owner_b_checkpoint = call(
        &fixture.state,
        device_request(
            Method::GET,
            &format!(
                "/api/v1/devices/{}/libraries/{owner_b_library}/checkpoint",
                owner_b_device.id
            ),
            None,
            &owner_b_device,
        ),
    )
    .await;
    assert_eq!(owner_b_checkpoint.status(), StatusCode::OK);
    let revoke_device = call(
        &fixture.state,
        browser_mutation_request(
            Method::POST,
            &format!(
                "/api/v1/devices/{}/credentials/{}/revoke",
                owner_b_device.id, owner_b_device.credential_id
            ),
            Some(json!({})),
            &owner_b,
        ),
    )
    .await;
    assert_eq!(revoke_device.status(), StatusCode::NO_CONTENT);
    let revoked_device_use = call(
        &fixture.state,
        device_request(
            Method::GET,
            &format!(
                "/api/v1/devices/{}/libraries/{owner_b_library}/checkpoint",
                owner_b_device.id
            ),
            None,
            &owner_b_device,
        ),
    )
    .await;
    assert_eq!(revoked_device_use.status(), StatusCode::UNAUTHORIZED);

    // An exact copied resource ID plus a valid server-issued ETag still does
    // not authorize a foreign metadata mutation.
    let mut foreign_rename = browser_mutation_request(
        Method::PATCH,
        &format!("/api/v1/nodes/{owner_b_root}"),
        Some(json!({"name":"p005-unauthorized-rename"})),
        &owner_a,
    );
    foreign_rename.headers_mut().insert(
        "if-match",
        HeaderValue::from_str(
            &EtagKey::from_bytes([0x51; 32]).issue(owner_b_root, Revision::new(0)),
        )
        .expect("server-issued test ETag must be valid"),
    );
    let foreign_rename = call(&fixture.state, foreign_rename).await;
    assert_eq!(foreign_rename.status(), StatusCode::NOT_FOUND);
    let foreign_name: String = sqlx::query_scalar("SELECT name FROM nodes WHERE id = $1")
        .bind(owner_b_root.into_uuid())
        .fetch_one(fixture.database.inspection())
        .await
        .expect("foreign node must remain queryable in isolated test database");
    assert_eq!(foreign_name, "owner-b-root");

    let expiring_session = login_browser(&fixture.state, &owner_a.login, TEST_PASSWORD).await;
    let session_token = SessionToken::try_from_hex(&expiring_session.session)
        .expect("session cookie must be a canonical synthetic token");
    let expired = sqlx::query(
        "UPDATE sessions
         SET created_at = CURRENT_TIMESTAMP - INTERVAL '2 seconds',
             expires_at = CURRENT_TIMESTAMP - INTERVAL '1 second'
         WHERE token_digest = $1",
    )
    .bind(session_token.digest().as_slice())
    .execute(fixture.database.inspection())
    .await
    .expect("test session expiry must be set directly without a sleep");
    assert_eq!(expired.rows_affected(), 1);
    let expired_session_use = call(
        &fixture.state,
        browser_request(Method::GET, "/api/v1/auth/session", None, &expiring_session),
    )
    .await;
    assert_eq!(expired_session_use.status(), StatusCode::UNAUTHORIZED);

    let revoking_session = login_browser(&fixture.state, &owner_a.login, TEST_PASSWORD).await;
    let logout = call(
        &fixture.state,
        browser_mutation_request(
            Method::POST,
            "/api/v1/auth/logout",
            None,
            &revoking_session,
        ),
    )
    .await;
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    let revoked_session_use = call(
        &fixture.state,
        browser_request(Method::GET, "/api/v1/auth/session", None, &revoking_session),
    )
    .await;
    assert_eq!(revoked_session_use.status(), StatusCode::UNAUTHORIZED);

    // Establish a valid device-two mutation, then present device one's bearer
    // while naming device two in the path. The API currently passes the path
    // device ID to the metadata service without binding it to the bearer.
    let valid_mutation = mutation_body(owner_a_root, Revision::new(0), "device-two-write", "0");
    let valid_device_two_mutation = call(
        &fixture.state,
        device_request(
            Method::POST,
            &format!(
                "/api/v1/devices/{}/libraries/{owner_a_library}/mutations",
                owner_a_device_two.id
            ),
            Some(valid_mutation),
            &owner_a_device_two,
        ),
    )
    .await;
    assert_eq!(valid_device_two_mutation.status(), StatusCode::OK);
    let valid_result = json_body(valid_device_two_mutation).await;
    let base_sequence = valid_result["data"]["journal_sequence"]
        .as_str()
        .expect("accepted mutation must return journal sequence");
    let root_response = call(
        &fixture.state,
        browser_request(
            Method::GET,
            &format!("/api/v1/nodes/{owner_a_root}"),
            None,
            &owner_a,
        ),
    )
    .await;
    assert_eq!(root_response.status(), StatusCode::OK);
    let root_body = json_body(root_response).await;
    let root_revision: Revision = root_body["data"]["revision"]
        .as_str()
        .expect("root metadata must expose current revision")
        .parse()
        .expect("root revision must be canonical");
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM nodes WHERE library_id = $1")
        .bind(owner_a_library.into_uuid())
        .fetch_one(fixture.database.inspection())
        .await
        .expect("node count must be readable before rejected mutation");
    let forged = mutation_body(
        owner_a_root,
        root_revision,
        "forged-device-two-write",
        base_sequence,
    );
    let forged_response = call(
        &fixture.state,
        device_request(
            Method::POST,
            &format!(
                "/api/v1/devices/{}/libraries/{owner_a_library}/mutations",
                owner_a_device_two.id
            ),
            Some(forged),
            &owner_a_device_one,
        ),
    )
    .await;
    let observed_status = forged_response.status();
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM nodes WHERE library_id = $1")
        .bind(owner_a_library.into_uuid())
        .fetch_one(fixture.database.inspection())
        .await
        .expect("node count must be readable after unauthorized mutation attempt");
    assert!(
        observed_status == StatusCode::NOT_FOUND && before == after,
        "device-one bearer impersonated device-two at /api/v1/devices/{{device_id}}/libraries/{{library_id}}/mutations: observed HTTP {observed_status}, node count {before}->{after}; expected concealed denial and unchanged state"
    );

    })
    .catch_unwind()
    .await;
    fixture.cleanup().await;
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
}

fn mutation_body(
    root: NodeId,
    expected_parent_revision: Revision,
    name: &str,
    base_sequence: &str,
) -> Value {
    json!({
        "mutation_id": synveil_core::ClientMutationId::new().to_string(),
        "base_epoch": "1",
        "base_sequence": base_sequence,
        "kind": "CREATE_DIRECTORY",
        "payload": {
            "parent_node_id": root.to_string(),
            "expected_parent_revision": expected_parent_revision.to_string(),
            "name": name
        }
    })
}
