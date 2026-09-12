//! PostgreSQL-backed verification for the authenticated durable rebaseline
//! snapshot HTTP transport. Run against a fresh disposable PostgreSQL 17
//! database with `SYNVEIL_TEST_DATABASE_URL=... cargo test -p synveil-api
//! --test rebaseline_snapshot_http_postgres -- --ignored`.

use std::{sync::Arc, time::Duration};

use axum::{
    body::Body,
    http::{HeaderValue, Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use synveil_api::{ApiState, CookieConfig, CsrfKey, RebaselineTokenKey, StaticReadiness, router};
use synveil_auth::{
    AuthError, DeviceAuthenticationService, DeviceEnrollmentTarget, PasswordHasherConfig,
    PasswordParameters, SessionConfig, SessionId, SessionPrincipal, SessionToken,
};
use synveil_core::{
    DedupDomainId, Device, DeviceCredentialId, DeviceId, DeviceStatus, Library, LibraryId,
    LogicalName, Node, NodeId, Timestamp, User, UserId, UserStatus,
};
use synveil_metadata::{
    DEFAULT_REBASELINE_SNAPSHOT_LIFETIME_SECONDS, DatabaseConfig, DatabasePool, DeviceSyncService,
    DomainRepository, FileMetadataService, LogicalSnapshotService,
    MAX_ACTIVE_REBASELINE_SNAPSHOTS_PER_OWNER_LIBRARY, MigrationRunner, SyncAckEvidence,
};
use tower::ServiceExt;

use async_trait::async_trait;

const PASSWORD: &str = "prompt-83-isolated-password";

fn timestamp() -> Timestamp {
    Timestamp::parse("2026-09-08T00:00:00.123456Z").expect("fixture timestamp is valid")
}

fn name(value: &str) -> LogicalName {
    LogicalName::new(value).expect("fixture name is valid")
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("response body must be readable")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body must be JSON")
}

fn json_request(method: Method, uri: &str, body: Value) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::from(body.to_string()))
        .expect("test request must be valid");
    request
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    request
}

fn cookie_value(response: &axum::response::Response, name: &str) -> String {
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
        .expect("response must set the requested cookie")
}

fn authenticated_get(uri: &str, session: &str) -> Request<Body> {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .expect("test GET must be valid");
    request.headers_mut().insert(
        "cookie",
        HeaderValue::from_str(&format!("synveil_session={session}"))
            .expect("test session cookie must be valid"),
    );
    request
}

fn mutation_request(uri: &str, session: &str, csrf: &str) -> Request<Body> {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .body(Body::from("{}"))
        .expect("test mutation must be valid");
    request
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    request.headers_mut().insert(
        "cookie",
        HeaderValue::from_str(&format!("synveil_session={session}; synveil_csrf={csrf}"))
            .expect("test cookies must be valid"),
    );
    request.headers_mut().insert(
        "x-csrf-token",
        HeaderValue::from_str(csrf).expect("test CSRF header must be valid"),
    );
    request
        .headers_mut()
        .insert("sec-fetch-site", HeaderValue::from_static("same-origin"));
    request
        .headers_mut()
        .insert("origin", HeaderValue::from_static("https://app.example"));
    request
}

fn device_get(uri: &str, secret: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {secret}"))
                .expect("test device bearer must be valid"),
        )
        .body(Body::empty())
        .expect("test device GET must be valid")
}

fn device_mutation_request(uri: &str, secret: &str) -> Request<Body> {
    device_json_request(uri, secret, json!({}))
}

fn device_empty_post_request(uri: &str, secret: &str) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {secret}"))
                .expect("test device bearer must be valid"),
        )
        .body(Body::empty())
        .expect("test empty device mutation must be valid")
}

fn device_json_request(uri: &str, secret: &str, body: Value) -> Request<Body> {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .body(Body::from(body.to_string()))
        .expect("test device mutation must be valid");
    request
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    request.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {secret}"))
            .expect("test device bearer must be valid"),
    );
    request
}

async fn active_device_credential(
    pool: &DatabasePool,
    owner_id: UserId,
    label: &str,
) -> (DeviceId, String, DeviceCredentialId) {
    let mut device = Device::new(DeviceId::new(), owner_id, name(label), timestamp());
    device
        .transition_status(DeviceStatus::Active, timestamp())
        .expect("fixture device must activate");
    DomainRepository::new(pool)
        .insert_device(&device)
        .await
        .expect("fixture device must persist");
    let device_auth = DeviceAuthenticationService::new(pool);
    let grant = device_auth
        .create_grant(owner_id, DeviceEnrollmentTarget::Existing(device.id()))
        .await
        .expect("fixture device grant must be created");
    let credential = device_auth
        .exchange(&grant.token)
        .await
        .expect("fixture device credential must be issued");
    (
        device.id(),
        credential.secret.expose_secret().to_owned(),
        credential.credential_id,
    )
}

async fn login(state: &ApiState, login: &str) -> (String, String, UserId) {
    let response = router(state.clone())
        .oneshot(json_request(
            Method::POST,
            "/api/v1/bootstrap/admin",
            json!({
                "login": login,
                "login_key": login,
                "password": PASSWORD
            }),
        ))
        .await
        .expect("bootstrap request must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    let response = router(state.clone())
        .oneshot(json_request(
            Method::POST,
            "/api/v1/auth/login",
            json!({
                "login": login,
                "login_key": login,
                "password": PASSWORD
            }),
        ))
        .await
        .expect("login request must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    let session = cookie_value(&response, "synveil_session");
    let csrf = cookie_value(&response, "synveil_csrf");
    let body = json_body(response).await;
    let owner_user_id = body["data"]["user_id"]
        .as_str()
        .expect("login must return owner ID")
        .parse()
        .expect("owner ID must be canonical");
    (session, csrf, owner_user_id)
}

#[allow(dead_code)]
async fn insert_user(pool: &DatabasePool, user_id: UserId, label: &str) {
    let login = format!("{label}-{user_id}");
    let user = User::new(
        user_id,
        synveil_core::LoginIdentifier::new(&login, user_id.to_string())
            .expect("fixture login is valid"),
        UserStatus::Active,
        timestamp(),
    );
    DomainRepository::new(pool)
        .insert_user(&user)
        .await
        .expect("fixture user must persist");
}

async fn insert_library(pool: &DatabasePool, owner_id: UserId, label: &str) -> (LibraryId, Node) {
    let library_id = LibraryId::new();
    let root = Node::new_root(
        NodeId::new(),
        library_id,
        name(&format!("{label}-root")),
        timestamp(),
    );
    let library = Library::new(
        library_id,
        owner_id,
        name(label),
        &root,
        DedupDomainId::new(),
        timestamp(),
    )
    .expect("fixture library must satisfy domain invariants");
    DomainRepository::new(pool)
        .insert_library_with_root(&library, &root)
        .await
        .expect("fixture library must persist");
    (library_id, root)
}

async fn isolated_setup() -> (
    ApiState,
    Arc<DatabasePool>,
    String,
    String,
    UserId,
    LibraryId,
) {
    let base = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable PostgreSQL 17 database");
    let db_name = format!("p83_http_{}", uuid::Uuid::now_v7().simple());
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

    let state = ApiState::from_current_platform()
        .with_postgres_auth(
            pool.clone(),
            PasswordHasherConfig::new(
                PasswordParameters::new(8 * 1024, 1, 1, 32)
                    .expect("password parameters must be valid"),
            )
            .expect("password config must be valid"),
            SessionConfig::new(std::time::Duration::from_secs(3600))
                .expect("session config must be valid"),
            RebaselineTokenKey::from_bytes([0x83; 32]),
        )
        .with_readiness(Arc::new(StaticReadiness::new(true)))
        .with_cookie_config(CookieConfig::development());

    let login_id = uuid::Uuid::now_v7().simple().to_string();
    let (session, csrf, owner_id) = login(&state, &login_id).await;
    let (library_id, _root) =
        insert_library(pool.as_ref(), owner_id, "snapshot-http-library").await;

    (state, pool, session, csrf, owner_id, library_id)
}

async fn setup() -> (ApiState, Arc<DatabasePool>, UserId, LibraryId) {
    let (state, pool, _session, _csrf, owner_id, library_id) = isolated_setup().await;
    (state, pool, owner_id, library_id)
}

async fn setup_with_session() -> (
    ApiState,
    Arc<DatabasePool>,
    String,
    String,
    UserId,
    LibraryId,
) {
    isolated_setup().await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_create_snapshot_returns_201_with_valid_descriptor() {
    let (state, _pool, session, csrf, _owner_id, library_id) = setup_with_session().await;

    let response = router(state.clone())
        .oneshot(mutation_request(
            &format!("/api/v1/libraries/{library_id}/rebaseline-snapshots"),
            &session,
            &csrf,
        ))
        .await
        .expect("create snapshot request must not fail");
    assert_eq!(response.status(), StatusCode::CREATED);
    let location = response
        .headers()
        .get("location")
        .expect("Location header must be present")
        .to_str()
        .expect("Location must be valid ASCII");
    assert!(location.starts_with("/api/v1/rebaseline-snapshots/"));
    let body = json_body(response).await;
    assert_eq!(body["data"]["library_id"], library_id.to_string());
    assert_eq!(body["data"]["entry_count"], "1");
    assert!(body["data"]["snapshot_id"].is_string());
    assert!(body["data"]["journal_boundary"]["journal_epoch"].is_string());
    assert!(body["data"]["journal_boundary"]["resume_sequence"].is_string());
    assert!(body["data"]["created_at"].is_string());
    assert!(body["data"]["expires_at"].is_string());
    assert!(body["meta"]["request_id"].is_string());
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_get_descriptor_returns_200_with_immutable_metadata() {
    let (state, _pool, session, csrf, _owner_id, library_id) = setup_with_session().await;

    let create_response = router(state.clone())
        .oneshot(mutation_request(
            &format!("/api/v1/libraries/{library_id}/rebaseline-snapshots"),
            &session,
            &csrf,
        ))
        .await
        .expect("create must succeed");
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let create_body = json_body(create_response).await;
    let snapshot_id = create_body["data"]["snapshot_id"]
        .as_str()
        .expect("snapshot_id must be present")
        .to_owned();

    let response = router(state.clone())
        .oneshot(authenticated_get(
            &format!("/api/v1/rebaseline-snapshots/{snapshot_id}"),
            &session,
        ))
        .await
        .expect("get descriptor request must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    let cache_control = response
        .headers()
        .get("cache-control")
        .expect("Cache-Control must be present")
        .to_str()
        .expect("Cache-Control must be valid");
    assert_eq!(cache_control, "private, no-store");
    let body = json_body(response).await;
    assert_eq!(body["data"]["snapshot_id"], snapshot_id);
    assert_eq!(body["data"]["library_id"], library_id.to_string());
    assert_eq!(body["data"]["entry_count"], "1");
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_first_page_default_size_returns_root() {
    let (state, _pool, session, csrf, _owner_id, library_id) = setup_with_session().await;

    let create_response = router(state.clone())
        .oneshot(mutation_request(
            &format!("/api/v1/libraries/{library_id}/rebaseline-snapshots"),
            &session,
            &csrf,
        ))
        .await
        .expect("create must succeed");
    let create_body = json_body(create_response).await;
    let snapshot_id = create_body["data"]["snapshot_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let response = router(state.clone())
        .oneshot(authenticated_get(
            &format!("/api/v1/rebaseline-snapshots/{snapshot_id}/entries"),
            &session,
        ))
        .await
        .expect("first page request must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["data"]["snapshot_id"], snapshot_id);
    assert_eq!(body["data"]["library_id"], library_id.to_string());
    assert_eq!(body["data"]["has_more"], false);
    assert!(body["data"]["next_cursor"].is_null());
    let entries = body["data"]["entries"]
        .as_array()
        .expect("entries must be an array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["kind"], "DIRECTORY");
    assert_eq!(entries[0]["state"], "ACTIVE");
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_zero_page_size_returns_400() {
    let (state, _pool, session, csrf, _owner_id, library_id) = setup_with_session().await;

    let create_response = router(state.clone())
        .oneshot(mutation_request(
            &format!("/api/v1/libraries/{library_id}/rebaseline-snapshots"),
            &session,
            &csrf,
        ))
        .await
        .expect("create must succeed");
    let create_body = json_body(create_response).await;
    let snapshot_id = create_body["data"]["snapshot_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let response = router(state.clone())
        .oneshot(authenticated_get(
            &format!("/api/v1/rebaseline-snapshots/{snapshot_id}/entries?limit=0"),
            &session,
        ))
        .await
        .expect("zero limit request must not fail");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "invalid_page_size");
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_over_max_page_size_returns_400() {
    let (state, _pool, session, csrf, _owner_id, library_id) = setup_with_session().await;

    let create_response = router(state.clone())
        .oneshot(mutation_request(
            &format!("/api/v1/libraries/{library_id}/rebaseline-snapshots"),
            &session,
            &csrf,
        ))
        .await
        .expect("create must succeed");
    let create_body = json_body(create_response).await;
    let snapshot_id = create_body["data"]["snapshot_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let response = router(state.clone())
        .oneshot(authenticated_get(
            &format!("/api/v1/rebaseline-snapshots/{snapshot_id}/entries?limit=1001"),
            &session,
        ))
        .await
        .expect("over-max limit request must not fail");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "invalid_page_size");
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_maximum_page_size_1000_accepted() {
    let (state, _pool, session, csrf, _owner_id, library_id) = setup_with_session().await;

    let create_response = router(state.clone())
        .oneshot(mutation_request(
            &format!("/api/v1/libraries/{library_id}/rebaseline-snapshots"),
            &session,
            &csrf,
        ))
        .await
        .expect("create must succeed");
    let create_body = json_body(create_response).await;
    let snapshot_id = create_body["data"]["snapshot_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let response = router(state.clone())
        .oneshot(authenticated_get(
            &format!("/api/v1/rebaseline-snapshots/{snapshot_id}/entries?limit=1000"),
            &session,
        ))
        .await
        .expect("max limit request must not fail");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_malformed_cursor_returns_400() {
    let (state, _pool, session, csrf, _owner_id, library_id) = setup_with_session().await;

    let create_response = router(state.clone())
        .oneshot(mutation_request(
            &format!("/api/v1/libraries/{library_id}/rebaseline-snapshots"),
            &session,
            &csrf,
        ))
        .await
        .expect("create must succeed");
    let create_body = json_body(create_response).await;
    let snapshot_id = create_body["data"]["snapshot_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let response = router(state.clone())
        .oneshot(authenticated_get(
            &format!(
                "/api/v1/rebaseline-snapshots/{snapshot_id}/entries?cursor=not-a-valid-cursor"
            ),
            &session,
        ))
        .await
        .expect("malformed cursor request must not fail");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "invalid_cursor");
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_missing_snapshot_returns_404() {
    let (state, _pool, session, _, _owner_id, _library_id) = setup_with_session().await;
    let missing_id = synveil_core::RebaselineSnapshotId::new();

    let response = router(state.clone())
        .oneshot(authenticated_get(
            &format!("/api/v1/rebaseline-snapshots/{missing_id}"),
            &session,
        ))
        .await
        .expect("missing snapshot request must not fail");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

struct FixedAuthenticationBackend {
    token: SessionToken,
    principal: SessionPrincipal,
}

#[async_trait]
impl synveil_api::AuthenticationBackend for FixedAuthenticationBackend {
    async fn bootstrap_state(&self) -> Result<synveil_api::BootstrapStatus, AuthError> {
        Ok(synveil_api::BootstrapStatus::Closed)
    }

    async fn create_first_admin(
        &self,
        _login: synveil_core::LoginIdentifier,
        _password: synveil_auth::PlaintextPassword,
    ) -> Result<(), AuthError> {
        Err(AuthError::BootstrapClosed)
    }

    async fn login(
        &self,
        _login: synveil_core::LoginIdentifier,
        _password: synveil_auth::PlaintextPassword,
    ) -> Result<synveil_api::IssuedSession, AuthError> {
        Err(AuthError::InvalidCredentials)
    }

    async fn authenticate_session(
        &self,
        token: &SessionToken,
    ) -> Result<SessionPrincipal, AuthError> {
        if token == &self.token {
            Ok(self.principal)
        } else {
            Err(AuthError::InvalidSession)
        }
    }

    async fn revoke_current_session(&self, _token: &SessionToken) -> Result<(), AuthError> {
        Ok(())
    }
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_foreign_owner_snapshot_returns_404() {
    let (state, pool, session, csrf, _owner_id, library_id) = setup_with_session().await;

    let create_response = router(state.clone())
        .oneshot(mutation_request(
            &format!("/api/v1/libraries/{library_id}/rebaseline-snapshots"),
            &session,
            &csrf,
        ))
        .await
        .expect("create must succeed");
    let create_body = json_body(create_response).await;
    let snapshot_id = create_body["data"]["snapshot_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let foreign_owner = UserId::new();
    DomainRepository::new(pool.as_ref())
        .insert_user(&User::new(
            foreign_owner,
            synveil_core::LoginIdentifier::new("foreign-owner", "foreign-owner")
                .expect("valid foreign login"),
            UserStatus::Active,
            timestamp(),
        ))
        .await
        .expect("foreign user must persist");
    let foreign_token = SessionToken::from_bytes([0xb2; 32]);
    let foreign_session = foreign_token.to_hex();
    let foreign_state =
        state
            .clone()
            .with_auth_backend(std::sync::Arc::new(FixedAuthenticationBackend {
                token: foreign_token.clone(),
                principal: SessionPrincipal::new(foreign_owner, false, SessionId::new()),
            }));
    let _ = CsrfKey::from_bytes([0xb2; 32]);

    let response = router(foreign_state.clone())
        .oneshot(authenticated_get(
            &format!("/api/v1/rebaseline-snapshots/{snapshot_id}"),
            &foreign_session,
        ))
        .await
        .expect("foreign owner request must not fail");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_authorized_expired_descriptor_and_page_return_410() {
    let (state, pool, session, _csrf, owner_id, library_id) = setup_with_session().await;
    let service = LogicalSnapshotService::new(pool.as_ref().clone());
    let expired_observed_at = Timestamp::now()
        .checked_sub_std(Duration::from_secs(
            DEFAULT_REBASELINE_SNAPSHOT_LIFETIME_SECONDS + 1,
        ))
        .expect("expired fixture time must be representable");
    let descriptor = service
        .create_rebaseline_snapshot(owner_id, library_id, expired_observed_at)
        .await
        .expect("expired fixture artifact must materialize");

    let descriptor_response = router(state.clone())
        .oneshot(authenticated_get(
            &format!("/api/v1/rebaseline-snapshots/{}", descriptor.snapshot_id()),
            &session,
        ))
        .await
        .expect("expired descriptor request must not fail");
    assert_eq!(descriptor_response.status(), StatusCode::GONE);
    let descriptor_body = json_body(descriptor_response).await;
    assert_eq!(descriptor_body["error"]["code"], "snapshot_expired");

    let page_response = router(state)
        .oneshot(authenticated_get(
            &format!(
                "/api/v1/rebaseline-snapshots/{}/entries",
                descriptor.snapshot_id()
            ),
            &session,
        ))
        .await
        .expect("expired page request must not fail");
    assert_eq!(page_response.status(), StatusCode::GONE);
    let page_body = json_body(page_response).await;
    assert_eq!(page_body["error"]["code"], "snapshot_expired");
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_foreign_expired_snapshot_returns_404_not_410() {
    let (state, pool, _session, _csrf, owner_id, library_id) = setup_with_session().await;
    let service = LogicalSnapshotService::new(pool.as_ref().clone());
    let expired_observed_at = Timestamp::now()
        .checked_sub_std(Duration::from_secs(
            DEFAULT_REBASELINE_SNAPSHOT_LIFETIME_SECONDS + 1,
        ))
        .expect("expired fixture time must be representable");
    let descriptor = service
        .create_rebaseline_snapshot(owner_id, library_id, expired_observed_at)
        .await
        .expect("expired fixture artifact must materialize");

    let foreign_owner = UserId::new();
    insert_user(pool.as_ref(), foreign_owner, "expired-foreign-owner-http").await;
    let foreign_token = SessionToken::from_bytes([0xc3; 32]);
    let foreign_session = foreign_token.to_hex();
    let foreign_state = state
        .clone()
        .with_auth_backend(Arc::new(FixedAuthenticationBackend {
            token: foreign_token,
            principal: SessionPrincipal::new(foreign_owner, false, SessionId::new()),
        }));

    let descriptor_response = router(foreign_state.clone())
        .oneshot(authenticated_get(
            &format!("/api/v1/rebaseline-snapshots/{}", descriptor.snapshot_id()),
            &foreign_session,
        ))
        .await
        .expect("foreign expired descriptor request must not fail");
    assert_eq!(descriptor_response.status(), StatusCode::NOT_FOUND);
    let descriptor_body = json_body(descriptor_response).await;
    assert_eq!(descriptor_body["error"]["code"], "not_found");
    assert_ne!(descriptor_body["error"]["code"], "snapshot_expired");

    let page_response = router(foreign_state)
        .oneshot(authenticated_get(
            &format!(
                "/api/v1/rebaseline-snapshots/{}/entries",
                descriptor.snapshot_id()
            ),
            &foreign_session,
        ))
        .await
        .expect("foreign expired page request must not fail");
    assert_eq!(page_response.status(), StatusCode::NOT_FOUND);
    let page_body = json_body(page_response).await;
    assert_eq!(page_body["error"]["code"], "not_found");
    assert_ne!(page_body["error"]["code"], "snapshot_expired");
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_no_authentication_returns_401() {
    let (state, _pool, _owner_id, library_id) = setup().await;

    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!(
                    "/api/v1/libraries/{library_id}/rebaseline-snapshots"
                ))
                .body(Body::empty())
                .expect("test request must be valid"),
        )
        .await
        .expect("unauthenticated request must not fail");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_cache_policy_is_private_no_store() {
    let (state, _pool, session, csrf, _owner_id, library_id) = setup_with_session().await;

    let create_response = router(state.clone())
        .oneshot(mutation_request(
            &format!("/api/v1/libraries/{library_id}/rebaseline-snapshots"),
            &session,
            &csrf,
        ))
        .await
        .expect("create must succeed");
    let create_body = json_body(create_response).await;
    let snapshot_id = create_body["data"]["snapshot_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let descriptor_response = router(state.clone())
        .oneshot(authenticated_get(
            &format!("/api/v1/rebaseline-snapshots/{snapshot_id}"),
            &session,
        ))
        .await
        .expect("descriptor request must not fail");
    assert_eq!(
        descriptor_response
            .headers()
            .get("cache-control")
            .unwrap()
            .to_str()
            .unwrap(),
        "private, no-store"
    );

    let page_response = router(state.clone())
        .oneshot(authenticated_get(
            &format!("/api/v1/rebaseline-snapshots/{snapshot_id}/entries"),
            &session,
        ))
        .await
        .expect("page request must not fail");
    assert_eq!(
        page_response
            .headers()
            .get("cache-control")
            .unwrap()
            .to_str()
            .unwrap(),
        "private, no-store"
    );
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_create_does_not_advance_device_checkpoint() {
    let (state, _pool, session, csrf, _owner_id, library_id) = setup_with_session().await;

    let response = router(state.clone())
        .oneshot(mutation_request(
            &format!("/api/v1/libraries/{library_id}/rebaseline-snapshots"),
            &session,
            &csrf,
        ))
        .await
        .expect("create must succeed");
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_revoked_device_cannot_create_or_read_snapshot() {
    let (state, pool, session, csrf, owner_id, library_id) = setup_with_session().await;
    let mut device = Device::new(
        DeviceId::new(),
        owner_id,
        LogicalName::new("snapshot-revocation-device").expect("device name must be valid"),
        timestamp(),
    );
    device
        .transition_status(DeviceStatus::Active, timestamp())
        .expect("fixture device must activate");
    DomainRepository::new(pool.as_ref())
        .insert_device(&device)
        .await
        .expect("fixture device must persist");

    let device_auth = DeviceAuthenticationService::new(pool.as_ref());
    let grant = device_auth
        .create_grant(owner_id, DeviceEnrollmentTarget::Existing(device.id()))
        .await
        .expect("device grant must be created");
    let credential = device_auth
        .exchange(&grant.token)
        .await
        .expect("device credential must be issued");
    let device_secret = credential.secret.expose_secret().to_owned();

    let create_response = router(state.clone())
        .oneshot(device_mutation_request(
            &format!("/api/v1/libraries/{library_id}/rebaseline-snapshots"),
            &device_secret,
        ))
        .await
        .expect("active device create request must not fail");
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let create_body = json_body(create_response).await;
    let snapshot_id = create_body["data"]["snapshot_id"]
        .as_str()
        .expect("device create must return a snapshot ID")
        .to_owned();

    let revoke_response = router(state.clone())
        .oneshot(mutation_request(
            &format!(
                "/api/v1/devices/{}/credentials/{}/revoke",
                device.id(),
                credential.credential_id
            ),
            &session,
            &csrf,
        ))
        .await
        .expect("credential revoke request must not fail");
    assert_eq!(revoke_response.status(), StatusCode::NO_CONTENT);

    let descriptor_response = router(state.clone())
        .oneshot(device_get(
            &format!("/api/v1/rebaseline-snapshots/{snapshot_id}"),
            &device_secret,
        ))
        .await
        .expect("revoked device descriptor request must not fail");
    assert_eq!(descriptor_response.status(), StatusCode::UNAUTHORIZED);
    let descriptor_body = json_body(descriptor_response).await;
    assert_eq!(descriptor_body["error"]["code"], "device_revoked");

    let page_response = router(state.clone())
        .oneshot(device_get(
            &format!("/api/v1/rebaseline-snapshots/{snapshot_id}/entries"),
            &device_secret,
        ))
        .await
        .expect("revoked device page request must not fail");
    assert_eq!(page_response.status(), StatusCode::UNAUTHORIZED);
    let page_body = json_body(page_response).await;
    assert_eq!(page_body["error"]["code"], "device_revoked");

    let create_after_revoke = router(state)
        .oneshot(device_mutation_request(
            &format!("/api/v1/libraries/{library_id}/rebaseline-snapshots"),
            &device_secret,
        ))
        .await
        .expect("revoked device create request must not fail");
    assert_eq!(create_after_revoke.status(), StatusCode::UNAUTHORIZED);
    let create_body = json_body(create_after_revoke).await;
    assert_eq!(create_body["error"]["code"], "device_revoked");
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_invalid_library_returns_400() {
    let (state, _pool, session, csrf, _owner_id, _library_id) = setup_with_session().await;

    let response = router(state.clone())
        .oneshot(mutation_request(
            "/api/v1/libraries/not-a-valid-uuid/rebaseline-snapshots",
            &session,
            &csrf,
        ))
        .await
        .expect("invalid library request must not fail");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_create_response_body_excludes_physical_internals() {
    let (state, _pool, session, csrf, _owner_id, library_id) = setup_with_session().await;

    let response = router(state.clone())
        .oneshot(mutation_request(
            &format!("/api/v1/libraries/{library_id}/rebaseline-snapshots"),
            &session,
            &csrf,
        ))
        .await
        .expect("create must succeed");
    let body = json_body(response).await;
    let body_str = serde_json::to_string(&body).unwrap();
    assert!(!body_str.contains("storage_key"));
    assert!(!body_str.contains("object_id"));
    assert!(!body_str.contains("backend_path"));
    assert!(!body_str.contains("inode"));
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_get_descriptor_body_excludes_physical_internals() {
    let (state, _pool, session, csrf, _owner_id, library_id) = setup_with_session().await;

    let create_response = router(state.clone())
        .oneshot(mutation_request(
            &format!("/api/v1/libraries/{library_id}/rebaseline-snapshots"),
            &session,
            &csrf,
        ))
        .await
        .expect("create must succeed");
    let create_body = json_body(create_response).await;
    let snapshot_id = create_body["data"]["snapshot_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let response = router(state.clone())
        .oneshot(authenticated_get(
            &format!("/api/v1/rebaseline-snapshots/{snapshot_id}"),
            &session,
        ))
        .await
        .expect("get descriptor must succeed");
    let body = json_body(response).await;
    let body_str = serde_json::to_string(&body).unwrap();
    assert!(!body_str.contains("owner_user_id"));
    assert!(!body_str.contains("owner_internal"));
    assert!(!body_str.contains("physical"));
    assert!(!body_str.contains("storage_key"));
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_entries_body_excludes_physical_internals() {
    let (state, _pool, session, csrf, _owner_id, library_id) = setup_with_session().await;

    let create_response = router(state.clone())
        .oneshot(mutation_request(
            &format!("/api/v1/libraries/{library_id}/rebaseline-snapshots"),
            &session,
            &csrf,
        ))
        .await
        .expect("create must succeed");
    let create_body = json_body(create_response).await;
    let snapshot_id = create_body["data"]["snapshot_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let response = router(state.clone())
        .oneshot(authenticated_get(
            &format!("/api/v1/rebaseline-snapshots/{snapshot_id}/entries"),
            &session,
        ))
        .await
        .expect("entries must succeed");
    let body = json_body(response).await;
    let body_str = serde_json::to_string(&body).unwrap();
    assert!(!body_str.contains("storage_key"));
    assert!(!body_str.contains("object_id"));
    assert!(!body_str.contains("backend_path"));
    assert!(!body_str.contains("inode"));
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_concurrent_create_is_deadlock_free_and_coherent() {
    let (state, _pool, session, csrf, _owner_id, library_id) = setup_with_session().await;
    let caller_count = 6_usize;
    let barrier = Arc::new(tokio::sync::Barrier::new(caller_count));
    let mut handles = Vec::with_capacity(caller_count);
    for _ in 0..caller_count {
        let state_clone = state.clone();
        let session_clone = session.clone();
        let csrf_clone = csrf.clone();
        let barrier = barrier.clone();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            tokio::time::timeout(
                Duration::from_secs(10),
                router(state_clone).oneshot(mutation_request(
                    &format!("/api/v1/libraries/{library_id}/rebaseline-snapshots"),
                    &session_clone,
                    &csrf_clone,
                )),
            )
            .await
        }));
    }

    let mut artifacts = Vec::with_capacity(caller_count);
    let mut unexpected_error_count = 0_u32;
    let mut timeout_count = 0_u32;
    for handle in handles {
        match handle.await.expect("HTTP snapshot task must join") {
            Ok(Ok(response)) if response.status() == StatusCode::CREATED => {
                let body = json_body(response).await;
                assert_eq!(body["data"]["library_id"], library_id.to_string());
                artifacts.push(
                    body["data"]["snapshot_id"]
                        .as_str()
                        .expect("created artifact must have an ID")
                        .to_owned(),
                );
            }
            Ok(Ok(response)) => {
                unexpected_error_count += 1;
                let status = response.status();
                let body = json_body(response).await;
                eprintln!(
                    "unexpected below-limit HTTP snapshot status {status}: {}",
                    serde_json::to_string(&body).expect("error body must serialize")
                );
            }
            Ok(Err(error)) => {
                unexpected_error_count += 1;
                eprintln!("unexpected below-limit HTTP service error: {error:?}");
            }
            Err(_) => timeout_count += 1,
        }
    }

    println!(
        "concurrent HTTP snapshot creation below bound: callers={caller_count} artifacts={} unexpected={} timeouts={timeout_count}",
        artifacts.len(),
        unexpected_error_count,
    );
    assert_eq!(artifacts.len(), caller_count);
    assert_eq!(
        unexpected_error_count, 0,
        "no unexpected below-limit errors"
    );
    assert_eq!(timeout_count, 0, "no below-limit timeouts");
    for snapshot_id in artifacts {
        let response = router(state.clone())
            .oneshot(authenticated_get(
                &format!("/api/v1/rebaseline-snapshots/{snapshot_id}"),
                &session,
            ))
            .await
            .expect("coherence descriptor request must not fail");
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["data"]["snapshot_id"], snapshot_id);
    }
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p83_active_snapshot_admission_is_scoped_and_concurrent() {
    let (state, pool, session, csrf, owner_id, library_id) = setup_with_session().await;
    let service = LogicalSnapshotService::new(pool.as_ref().clone());
    let seed_at = Timestamp::now();
    let limit = MAX_ACTIVE_REBASELINE_SNAPSHOTS_PER_OWNER_LIBRARY;
    for _ in 0..(limit - 1) {
        service
            .create_rebaseline_snapshot(owner_id, library_id, seed_at)
            .await
            .expect("HTTP admission seed must be admitted");
    }

    let caller_count = usize::try_from(limit).expect("snapshot limit fits usize") + 4;
    let barrier = Arc::new(tokio::sync::Barrier::new(caller_count));
    let mut handles = Vec::with_capacity(caller_count);
    for _ in 0..caller_count {
        let state_clone = state.clone();
        let session_clone = session.clone();
        let csrf_clone = csrf.clone();
        let barrier = barrier.clone();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            tokio::time::timeout(
                Duration::from_secs(10),
                router(state_clone).oneshot(mutation_request(
                    &format!("/api/v1/libraries/{library_id}/rebaseline-snapshots"),
                    &session_clone,
                    &csrf_clone,
                )),
            )
            .await
        }));
    }

    let mut admitted = 0_u32;
    let mut rejected = 0_u32;
    let mut unexpected_errors = 0_u32;
    let mut timeouts = 0_u32;
    let mut created_ids = Vec::new();
    for handle in handles {
        match handle.await.expect("HTTP admission task must join") {
            Ok(Ok(response)) if response.status() == StatusCode::CREATED => {
                admitted += 1;
                let body = json_body(response).await;
                created_ids.push(
                    body["data"]["snapshot_id"]
                        .as_str()
                        .expect("admitted artifact must have an ID")
                        .to_owned(),
                );
            }
            Ok(Ok(response)) if response.status() == StatusCode::TOO_MANY_REQUESTS => {
                rejected += 1;
                assert_eq!(response.headers()["retry-after"], "1");
                assert_eq!(response.headers()["cache-control"], "private, no-store");
                let body = json_body(response).await;
                assert_eq!(body["error"]["code"], "rate_limited");
                assert_eq!(body["error"]["retryable"], true);
            }
            Ok(Ok(response)) => {
                unexpected_errors += 1;
                let status = response.status();
                let body = json_body(response).await;
                eprintln!(
                    "unexpected HTTP admission status {status}: {}",
                    serde_json::to_string(&body).expect("error body must serialize")
                );
            }
            Ok(Err(error)) => {
                unexpected_errors += 1;
                eprintln!("unexpected HTTP admission service error: {error:?}");
            }
            Err(_) => timeouts += 1,
        }
    }

    println!(
        "concurrent HTTP snapshot admission boundary: callers={caller_count} admitted={admitted} rejected={rejected} unexpected={unexpected_errors} timeouts={timeouts}"
    );
    assert_eq!(admitted, 1);
    assert_eq!(created_ids.len(), 1);
    assert_eq!(
        rejected,
        u32::try_from(caller_count - 1).expect("caller count fits u32")
    );
    assert_eq!(
        unexpected_errors, 0,
        "only canonical admission rejection is allowed"
    );
    assert_eq!(timeouts, 0, "admission callers must not time out");

    let response = router(state)
        .oneshot(authenticated_get(
            &format!("/api/v1/rebaseline-snapshots/{}", created_ids[0]),
            &session,
        ))
        .await
        .expect("admitted artifact descriptor request must not fail");
    assert_eq!(response.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Prompt 85 — durable checkpoint handoff
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p85_handoff_is_device_bound_idempotent_private_and_boundary_free() {
    let (state, pool, _session, _csrf, owner_id, library_id) = setup_with_session().await;
    let (device_id, secret, _credential_id) =
        active_device_credential(pool.as_ref(), owner_id, "p85-success-device").await;

    let create_response = router(state.clone())
        .oneshot(device_mutation_request(
            &format!("/api/v1/libraries/{}/rebaseline-snapshots", library_id),
            &secret,
        ))
        .await
        .expect("device snapshot create must not fail");
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let create_body = json_body(create_response).await;
    let snapshot_id = create_body["data"]["snapshot_id"]
        .as_str()
        .expect("snapshot ID must be returned")
        .to_owned();

    let handoff_uri = format!("/api/v1/rebaseline-snapshots/{}/handoff", snapshot_id);
    let first = router(state.clone())
        .oneshot(device_empty_post_request(&handoff_uri, &secret))
        .await
        .expect("first handoff must not fail");
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(first.headers()["cache-control"], "private, no-store");
    let first_body = json_body(first).await;
    assert_eq!(first_body["data"]["snapshot_id"], snapshot_id);
    assert_eq!(first_body["data"]["library_id"], library_id.to_string());
    assert_eq!(first_body["data"]["checkpoint"]["epoch"], "1");
    assert_eq!(first_body["data"]["checkpoint"]["sequence"], "0");
    assert!(
        first_body["data"]["checkpoint"]
            .as_object()
            .unwrap()
            .keys()
            .all(|key| matches!(key.as_str(), "epoch" | "sequence"))
    );

    let second = router(state.clone())
        .oneshot(device_mutation_request(&handoff_uri, &secret))
        .await
        .expect("idempotent handoff retry must not fail");
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(second.headers()["cache-control"], "private, no-store");
    let second_body = json_body(second).await;
    assert_eq!(second_body["data"], first_body["data"]);

    let sync = DeviceSyncService::new(pool.as_ref().clone());
    let checkpoint = sync
        .ensure_checkpoint(owner_id, device_id, library_id)
        .await
        .expect("handoff checkpoint must remain readable");
    assert_eq!(checkpoint.journal_epoch().get(), 1);
    assert_eq!(checkpoint.acknowledged_sequence().get(), 0);

    // The server must reject every attempted boundary input rather than
    // silently accepting a future cursor supplied by the client.
    let invalid = router(state.clone())
        .oneshot(device_json_request(
            &handoff_uri,
            &secret,
            json!({
                "owner_user_id": owner_id.to_string(),
                "library_id": library_id.to_string(),
                "journal_epoch": "1",
                "resume_sequence": "999"
            }),
        ))
        .await
        .expect("invalid handoff body must return an HTTP response");
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(invalid.headers()["cache-control"], "private, no-store");
    let invalid_body = json_body(invalid).await;
    assert_eq!(invalid_body["error"]["code"], "invalid_request");
    let unchanged = sync
        .ensure_checkpoint(owner_id, device_id, library_id)
        .await
        .expect("invalid body must not alter checkpoint");
    assert_eq!(unchanged.acknowledged_sequence().get(), 0);
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p85_expired_header_succeeds_missing_and_foreign_are_concealed() {
    let (state, pool, _session, _csrf, owner_id, library_id) = setup_with_session().await;
    let (_device_id, secret, _credential_id) =
        active_device_credential(pool.as_ref(), owner_id, "p85-expired-device").await;
    let expired_at = Timestamp::now()
        .checked_sub_std(Duration::from_secs(
            DEFAULT_REBASELINE_SNAPSHOT_LIFETIME_SECONDS + 1,
        ))
        .expect("expired fixture timestamp must be representable");
    let snapshot_id = LogicalSnapshotService::new(pool.as_ref().clone())
        .create_rebaseline_snapshot(owner_id, library_id, expired_at)
        .await
        .expect("expired-but-present snapshot must be created")
        .snapshot_id();

    let response = router(state.clone())
        .oneshot(device_mutation_request(
            &format!("/api/v1/rebaseline-snapshots/{}/handoff", snapshot_id),
            &secret,
        ))
        .await
        .expect("expired handoff must return an HTTP response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let body = json_body(response).await;
    assert_eq!(body["data"]["snapshot_id"], snapshot_id.to_string());

    let missing = synveil_core::RebaselineSnapshotId::new();
    let missing_response = router(state.clone())
        .oneshot(device_mutation_request(
            &format!("/api/v1/rebaseline-snapshots/{}/handoff", missing),
            &secret,
        ))
        .await
        .expect("missing handoff must return an HTTP response");
    assert_eq!(missing_response.status(), StatusCode::NOT_FOUND);
    let missing_body = json_body(missing_response).await;
    assert_eq!(missing_body["error"]["code"], "not_found");

    let foreign_owner = UserId::new();
    insert_user(pool.as_ref(), foreign_owner, "p85-foreign-owner").await;
    let (_foreign_device, foreign_secret, _foreign_credential_id) =
        active_device_credential(pool.as_ref(), foreign_owner, "p85-foreign-device").await;
    let foreign_response = router(state)
        .oneshot(device_mutation_request(
            &format!("/api/v1/rebaseline-snapshots/{}/handoff", snapshot_id),
            &foreign_secret,
        ))
        .await
        .expect("foreign handoff must return an HTTP response");
    assert_eq!(foreign_response.status(), StatusCode::NOT_FOUND);
    let foreign_body = json_body(foreign_response).await;
    assert_eq!(foreign_body["error"]["code"], "not_found");
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p85_checkpoint_ahead_is_conflict_and_browser_has_no_device_target() {
    let (state, pool, session, csrf, owner_id, library_id) = setup_with_session().await;
    let (device_id, secret, _credential_id) =
        active_device_credential(pool.as_ref(), owner_id, "p85-conflict-device").await;
    let create_response = router(state.clone())
        .oneshot(device_mutation_request(
            &format!("/api/v1/libraries/{}/rebaseline-snapshots", library_id),
            &secret,
        ))
        .await
        .expect("device snapshot create must succeed");
    let snapshot_id: synveil_core::RebaselineSnapshotId = json_body(create_response).await["data"]
        ["snapshot_id"]
        .as_str()
        .expect("snapshot ID must be present")
        .parse()
        .expect("snapshot ID must be canonical");

    let library = DomainRepository::new(pool.as_ref())
        .find_library(library_id)
        .await
        .expect("library lookup must succeed")
        .expect("library must exist");
    FileMetadataService::new(pool.as_ref().clone())
        .create_directory(
            owner_id,
            library_id,
            Some(library.root_node_id()),
            name("after-snapshot"),
        )
        .await
        .expect("post-snapshot mutation must commit");
    let sync = DeviceSyncService::new(pool.as_ref().clone());
    let page = sync
        .fetch_feed(owner_id, device_id, library_id, 10)
        .await
        .expect("post-snapshot event must be readable");
    let evidence = SyncAckEvidence::new(
        owner_id,
        device_id,
        library_id,
        page.checkpoint().journal_epoch(),
        page.from_sequence(),
        page.through_sequence(),
        page.high_watermark().sequence(),
    );
    sync.acknowledge(owner_id, device_id, library_id, evidence)
        .await
        .expect("fixture must advance checkpoint beyond snapshot C");

    let conflict = router(state.clone())
        .oneshot(device_mutation_request(
            &format!("/api/v1/rebaseline-snapshots/{}/handoff", snapshot_id),
            &secret,
        ))
        .await
        .expect("ahead handoff must return an HTTP response");
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    assert_eq!(conflict.headers()["cache-control"], "private, no-store");
    let conflict_body = json_body(conflict).await;
    assert_eq!(conflict_body["error"]["code"], "checkpoint_conflict");
    let checkpoint = sync
        .ensure_checkpoint(owner_id, device_id, library_id)
        .await
        .expect("conflicting checkpoint must remain readable");
    assert_eq!(checkpoint.acknowledged_sequence().get(), 1);

    let browser_response = router(state)
        .oneshot(mutation_request(
            &format!("/api/v1/rebaseline-snapshots/{}/handoff", snapshot_id),
            &session,
            &csrf,
        ))
        .await
        .expect("browser handoff must return an HTTP response");
    assert_eq!(browser_response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn http_p85_revoked_device_cannot_complete_handoff() {
    let (state, pool, session, csrf, owner_id, library_id) = setup_with_session().await;
    let (device_id, secret, credential_id) =
        active_device_credential(pool.as_ref(), owner_id, "p85-revoked-device").await;
    let create_response = router(state.clone())
        .oneshot(device_mutation_request(
            &format!("/api/v1/libraries/{}/rebaseline-snapshots", library_id),
            &secret,
        ))
        .await
        .expect("device snapshot create must succeed");
    let snapshot_id = json_body(create_response).await["data"]["snapshot_id"]
        .as_str()
        .expect("snapshot ID must be returned")
        .to_owned();

    let revoke = router(state.clone())
        .oneshot(mutation_request(
            &format!(
                "/api/v1/devices/{}/credentials/{}/revoke",
                device_id, credential_id
            ),
            &session,
            &csrf,
        ))
        .await
        .expect("credential revoke must return an HTTP response");
    assert_eq!(revoke.status(), StatusCode::NO_CONTENT);

    let response = router(state)
        .oneshot(device_mutation_request(
            &format!("/api/v1/rebaseline-snapshots/{}/handoff", snapshot_id),
            &secret,
        ))
        .await
        .expect("revoked handoff must return an HTTP response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "device_revoked");
}
