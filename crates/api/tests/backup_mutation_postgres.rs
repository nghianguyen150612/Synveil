//! PostgreSQL-backed verification for the authenticated, CSRF-protected,
//! idempotent manual backup mutation API. Run against a fresh disposable
//! PostgreSQL 17 database with:
//! `SYNVEIL_TEST_DATABASE_URL=... cargo test -p synveil-api --test backup_mutation_postgres -- --ignored`.

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{HeaderValue, Method, Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use synveil_api::{
    ApiState, AuthenticationBackend, BootstrapStatus, CookieConfig, CsrfKey, IssuedSession,
    RebaselineTokenKey, StaticReadiness, router,
};
use synveil_auth::{
    AuthError, PasswordHasherConfig, PasswordParameters, PlaintextPassword, SessionConfig,
    SessionId, SessionPrincipal, SessionToken,
};
use synveil_core::{
    DedupDomainId, FileVersion, FileVersionId, Library, LibraryId, LogicalName, LoginIdentifier,
    Node, NodeId, NodeKind, ObjectId, ObjectReference, Sha256Digest, Timestamp, User, UserId,
    UserStatus,
};
use synveil_metadata::{DatabaseConfig, DatabasePool, DomainRepository, MigrationRunner};
use tower::ServiceExt;

const PASSWORD: &str = "prompt-52-isolated-password";
const ORIGIN: &str = "https://app.example";

struct FixedAuthenticationBackend {
    token: SessionToken,
    principal: SessionPrincipal,
}

#[async_trait]
impl AuthenticationBackend for FixedAuthenticationBackend {
    async fn bootstrap_state(&self) -> Result<BootstrapStatus, AuthError> {
        Ok(BootstrapStatus::Closed)
    }

    async fn create_first_admin(
        &self,
        _login: LoginIdentifier,
        _password: PlaintextPassword,
    ) -> Result<(), AuthError> {
        Err(AuthError::BootstrapClosed)
    }

    async fn login(
        &self,
        _login: LoginIdentifier,
        _password: PlaintextPassword,
    ) -> Result<IssuedSession, AuthError> {
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

async fn json_body(response: Response) -> Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("response body must be readable")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body must be JSON")
}

fn json_request(method: Method, uri: &str, body: Value) -> Request<Body> {
    raw_json_request(method, uri, body.to_string())
}

fn raw_json_request(method: Method, uri: &str, body: impl Into<String>) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::from(body.into()))
        .expect("test request must be valid");
    request
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    request
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
        .expect("response must set the requested cookie")
}

fn authenticated_get(uri: &str, session: &str, csrf: &str) -> Request<Body> {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .expect("test GET must be valid");
    request.headers_mut().insert(
        "cookie",
        HeaderValue::from_str(&format!("synveil_session={session}; synveil_csrf={csrf}"))
            .expect("test cookie must be valid"),
    );
    request
}

fn mutation_request(
    method: Method,
    uri: &str,
    body: Option<Value>,
    session: Option<&str>,
    csrf_cookie: Option<&str>,
    csrf_header: Option<&str>,
    idempotency_key: Option<&str>,
) -> Request<Body> {
    let mut request = if let Some(body) = body {
        json_request(method, uri, body)
    } else {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("empty mutation request must be valid")
    };
    if let (Some(session), Some(csrf_cookie)) = (session, csrf_cookie) {
        request.headers_mut().insert(
            "cookie",
            HeaderValue::from_str(&format!(
                "synveil_session={session}; synveil_csrf={csrf_cookie}"
            ))
            .expect("test cookie must be valid"),
        );
    }
    if let Some(csrf_header) = csrf_header {
        request.headers_mut().insert(
            "x-csrf-token",
            HeaderValue::from_str(csrf_header).expect("test CSRF header must be valid"),
        );
    }
    if let Some(idempotency_key) = idempotency_key {
        request.headers_mut().insert(
            "idempotency-key",
            HeaderValue::from_str(idempotency_key).expect("test idempotency header must be valid"),
        );
    }
    request
        .headers_mut()
        .insert("sec-fetch-site", HeaderValue::from_static("same-origin"));
    request
        .headers_mut()
        .insert("origin", HeaderValue::from_static(ORIGIN));
    request
}

async fn send(state: &ApiState, request: Request<Body>) -> Response {
    router(state.clone())
        .oneshot(request)
        .await
        .expect("backup HTTP request must not fail")
}

async fn bootstrap_and_login(state: &ApiState, login: &str) -> (String, String, UserId) {
    let response = send(
        state,
        json_request(
            Method::POST,
            "/api/v1/bootstrap/admin",
            json!({"login": login, "login_key": login, "password": PASSWORD}),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = send(
        state,
        json_request(
            Method::POST,
            "/api/v1/auth/login",
            json!({"login": login, "login_key": login, "password": PASSWORD}),
        ),
    )
    .await;
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

async fn seed_library(
    pool: &DatabasePool,
    owner_user_id: UserId,
    label: &str,
    with_content: bool,
) -> LibraryId {
    let observed_at =
        Timestamp::parse("2026-08-31T00:00:00.123456Z").expect("fixture timestamp must be valid");
    let repository = DomainRepository::new(pool);
    let library_id = LibraryId::new();
    let root = Node::new_root(
        NodeId::new(),
        library_id,
        LogicalName::new("root").expect("valid root name"),
        observed_at,
    );
    let library = Library::new(
        library_id,
        owner_user_id,
        LogicalName::new(label).expect("valid library name"),
        &root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("valid library");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("library must persist");
    if with_content {
        let file = Node::new_child(
            NodeId::new(),
            library_id,
            &root,
            NodeKind::File,
            LogicalName::new("manual-maintenance.txt").expect("valid file name"),
            observed_at,
        )
        .expect("valid file");
        repository
            .insert_node(&file)
            .await
            .expect("file must persist");
        let object = ObjectReference::new(
            ObjectId::new(),
            library.dedup_domain_id(),
            Sha256Digest::from_bytes([0x52; 32]),
            52,
        );
        repository
            .insert_object(object, observed_at)
            .await
            .expect("object must persist");
        let version = FileVersion::new(
            FileVersionId::new(),
            &library,
            &file,
            object,
            None,
            observed_at,
        )
        .expect("valid file version");
        repository
            .insert_file_version(version)
            .await
            .expect("file version must persist");
        repository
            .update_node(
                &file
                    .with_current_version(&version, observed_at)
                    .expect("version must bind to file"),
            )
            .await
            .expect("file head must persist");
    }
    library_id
}

async fn count(pool: &PgPool, table: &str) -> i64 {
    let query = format!("SELECT count(*)::BIGINT FROM {table}");
    sqlx::query_scalar(&query)
        .fetch_one(pool)
        .await
        .expect("audit count must be readable")
}

async fn state_counts(pool: &PgPool) -> BTreeMap<&'static str, i64> {
    let mut counts = BTreeMap::new();
    for table in [
        "backup_sets",
        "backup_snapshot_retention_policy_revisions",
        "backup_maintenance_runs",
        "backup_snapshots",
        "backup_snapshot_content_pins",
        "backup_snapshot_expiry_plans",
        "backup_snapshot_expiry_executions",
        "backup_prune_plans",
        "backup_prune_executions",
        "object_gc_candidates",
        "object_gc_operations",
        "change_journal",
        "device_sync_checkpoints",
    ] {
        counts.insert(table, count(pool, table).await);
    }
    let (head, epoch): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(sync_head), 0)::BIGINT,
                COALESCE(SUM(journal_epoch), 0)::BIGINT FROM libraries",
    )
    .fetch_one(pool)
    .await
    .expect("library journal counters must be readable");
    counts.insert("library_sync_head", head);
    counts.insert("library_journal_epoch", epoch);
    counts
}

fn assert_no_physical_identity(value: &Value) {
    const FORBIDDEN: [&str; 10] = [
        "object_id",
        "objectId",
        "object_replica_id",
        "objectReplicaId",
        "storage_key",
        "storageKey",
        "backend_locator",
        "dedup_domain_id",
        "gc_candidate_id",
        "capture_operation_id",
    ];
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                assert!(!FORBIDDEN.contains(&key.as_str()), "forbidden key: {key}");
                assert_no_physical_identity(value);
            }
        }
        Value::Array(values) => values.iter().for_each(assert_no_physical_identity),
        _ => {}
    }
}

fn key() -> String {
    uuid::Uuid::now_v7().hyphenated().to_string()
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_backup_mutations_are_secure_idempotent_and_orchestration_safe() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a fresh PostgreSQL database");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = Arc::new(
        DatabasePool::connect(&config)
            .await
            .expect("test PostgreSQL must accept a connection"),
    );
    let status = MigrationRunner::new()
        .run(pool.as_ref())
        .await
        .expect("all migrations must apply from empty");
    assert!(status.is_current());
    assert_eq!(status.applied_versions().len(), 34);
    let audit_pool = PgPool::connect(&url)
        .await
        .expect("audit connection must succeed");

    let csrf_key = CsrfKey::from_bytes([0x52; 32]);
    let login = format!("prompt52-{}", uuid::Uuid::now_v7());
    let api_state = ApiState::from_current_platform()
        .with_csrf_key(csrf_key.clone())
        .with_postgres_auth(
            pool.clone(),
            PasswordHasherConfig::new(
                PasswordParameters::new(8 * 1024, 1, 1, 32).expect("valid password parameters"),
            )
            .expect("valid password config"),
            SessionConfig::default(),
            RebaselineTokenKey::from_bytes([0x52; 32]),
        )
        .with_readiness(Arc::new(StaticReadiness::new(true)))
        .with_cookie_config(CookieConfig::development())
        .with_allowed_origin(ORIGIN);
    let (session, csrf, owner_user_id) = bootstrap_and_login(&api_state, &login).await;
    let library_id = seed_library(pool.as_ref(), owner_user_id, "Prompt 52 content", true).await;
    let second_library_id =
        seed_library(pool.as_ref(), owner_user_id, "Prompt 52 alternate", false).await;

    let foreign_owner = UserId::new();
    DomainRepository::new(pool.as_ref())
        .insert_user(&User::new(
            foreign_owner,
            LoginIdentifier::new("prompt52-foreign", "prompt52-foreign")
                .expect("valid foreign login"),
            UserStatus::Active,
            Timestamp::parse("2026-08-31T00:00:00.123456Z").expect("valid fixture time"),
        ))
        .await
        .expect("foreign user must persist");
    let foreign_library_id =
        seed_library(pool.as_ref(), foreign_owner, "Prompt 52 foreign", false).await;
    let foreign_token = SessionToken::from_bytes([0xb2; 32]);
    let foreign_session = foreign_token.to_hex();
    let foreign_csrf = csrf_key.issue(&foreign_token);
    let foreign_state = api_state
        .clone()
        .with_auth_backend(Arc::new(FixedAuthenticationBackend {
            token: foreign_token,
            principal: SessionPrincipal::new(foreign_owner, false, SessionId::new()),
        }));

    let create_path = "/api/v1/backups/sets";
    let create_key = key();
    let create_body = json!({"library_id": library_id.to_string(), "name": "daily-manual"});
    let before_invalid = state_counts(&audit_pool).await;

    let response = send(
        &api_state,
        mutation_request(
            Method::POST,
            create_path,
            Some(create_body.clone()),
            None,
            None,
            Some(&csrf),
            Some(&create_key),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = send(
        &api_state,
        mutation_request(
            Method::POST,
            create_path,
            Some(create_body.clone()),
            Some(&session),
            Some(&csrf),
            None,
            Some(&create_key),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let invalid_csrf = "0".repeat(128);
    let response = send(
        &api_state,
        mutation_request(
            Method::POST,
            create_path,
            Some(create_body.clone()),
            Some(&session),
            Some(&csrf),
            Some(&invalid_csrf),
            Some(&create_key),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    for idempotency_key in [
        None,
        Some("not-a-uuid"),
        Some("00000000-0000-4000-8000-000000000000"),
        Some("01890ABC-DEF0-7ABC-8ABC-0123456789AB"),
    ] {
        let response = send(
            &api_state,
            mutation_request(
                Method::POST,
                create_path,
                Some(create_body.clone()),
                Some(&session),
                Some(&csrf),
                Some(&csrf),
                idempotency_key,
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    let mut malformed_json = raw_json_request(Method::POST, create_path, "{");
    malformed_json.headers_mut().insert(
        "cookie",
        HeaderValue::from_str(&format!("synveil_session={session}; synveil_csrf={csrf}"))
            .expect("valid cookie"),
    );
    malformed_json.headers_mut().insert(
        "x-csrf-token",
        HeaderValue::from_str(&csrf).expect("valid CSRF"),
    );
    malformed_json.headers_mut().insert(
        "idempotency-key",
        HeaderValue::from_str(&create_key).expect("valid key"),
    );
    let response = send(&api_state, malformed_json).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(state_counts(&audit_pool).await, before_invalid);

    let first = send(
        &api_state,
        mutation_request(
            Method::POST,
            create_path,
            Some(create_body.clone()),
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&create_key),
        ),
    )
    .await;
    assert_eq!(first.status(), StatusCode::CREATED);
    assert_eq!(first.headers()["cache-control"], "private, no-store");
    let first = json_body(first).await;
    assert_no_physical_identity(&first);
    let backup_set_id = first["data"]["backup_set_id"]
        .as_str()
        .expect("create must return set ID")
        .to_owned();
    assert_eq!(first["data"]["state"], "CREATED");
    let replay = send(
        &api_state,
        mutation_request(
            Method::POST,
            create_path,
            Some(create_body.clone()),
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&create_key),
        ),
    )
    .await;
    assert_eq!(replay.status(), StatusCode::CREATED);
    assert_eq!(
        json_body(replay).await["data"]["backup_set_id"],
        backup_set_id
    );
    assert_eq!(count(&audit_pool, "backup_sets").await, 1);
    let conflict = send(
        &api_state,
        mutation_request(
            Method::POST,
            create_path,
            Some(json!({"library_id": second_library_id.to_string(), "name": "changed"})),
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&create_key),
        ),
    )
    .await;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(conflict).await["error"]["code"],
        "idempotency_conflict"
    );
    assert_eq!(count(&audit_pool, "backup_sets").await, 1);

    let secondary_set = send(
        &api_state,
        mutation_request(
            Method::POST,
            create_path,
            Some(json!({
                "library_id": second_library_id.to_string(),
                "name": "secondary-manual"
            })),
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&key()),
        ),
    )
    .await;
    assert_eq!(secondary_set.status(), StatusCode::CREATED);
    let secondary_set_id = json_body(secondary_set).await["data"]["backup_set_id"]
        .as_str()
        .expect("secondary create must return set ID")
        .to_owned();

    let foreign_same_key = send(
        &foreign_state,
        mutation_request(
            Method::POST,
            create_path,
            Some(json!({"library_id": foreign_library_id.to_string(), "name": "daily-manual"})),
            Some(&foreign_session),
            Some(&foreign_csrf),
            Some(&foreign_csrf),
            Some(&create_key),
        ),
    )
    .await;
    assert_eq!(foreign_same_key.status(), StatusCode::CREATED);
    let foreign_set_id = json_body(foreign_same_key).await["data"]["backup_set_id"]
        .as_str()
        .expect("foreign create must return set ID")
        .to_owned();
    assert_ne!(foreign_set_id, backup_set_id);
    let inaccessible_library = send(
        &api_state,
        mutation_request(
            Method::POST,
            create_path,
            Some(json!({"library_id": foreign_library_id.to_string(), "name": "inaccessible"})),
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&key()),
        ),
    )
    .await;
    assert_eq!(inaccessible_library.status(), StatusCode::NOT_FOUND);

    let set_get = send(
        &api_state,
        authenticated_get(
            &format!("/api/v1/backups/sets/{backup_set_id}"),
            &session,
            &csrf,
        ),
    )
    .await;
    assert_eq!(set_get.status(), StatusCode::OK);
    assert_eq!(
        json_body(set_get).await["data"]["backup_set_id"],
        backup_set_id
    );

    let policy_path = format!("/api/v1/backups/sets/{backup_set_id}/retention-policy");
    let policy_key = key();
    let policy_body = json!({"keep_latest_completed": 1, "expire_after_seconds": 1});
    let before_policy_rejections = state_counts(&audit_pool).await;
    for (session_value, header_value, expected) in [
        (Some(session.as_str()), None, StatusCode::FORBIDDEN),
        (
            Some(session.as_str()),
            Some(invalid_csrf.as_str()),
            StatusCode::FORBIDDEN,
        ),
        (None, Some(csrf.as_str()), StatusCode::UNAUTHORIZED),
    ] {
        let response = send(
            &api_state,
            mutation_request(
                Method::POST,
                &policy_path,
                Some(policy_body.clone()),
                session_value,
                session_value.map(|_| csrf.as_str()),
                header_value,
                Some(&policy_key),
            ),
        )
        .await;
        assert_eq!(response.status(), expected);
    }
    for invalid in [
        json!({"keep_latest_completed": 0, "expire_after_seconds": 1}),
        json!({"keep_latest_completed": 1, "expire_after_seconds": 0}),
    ] {
        let response = send(
            &api_state,
            mutation_request(
                Method::POST,
                &policy_path,
                Some(invalid),
                Some(&session),
                Some(&csrf),
                Some(&csrf),
                Some(&key()),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    let mut overflow = raw_json_request(
        Method::POST,
        &policy_path,
        r#"{"keep_latest_completed":1,"expire_after_seconds":18446744073709551616}"#,
    );
    overflow.headers_mut().insert(
        "cookie",
        HeaderValue::from_str(&format!("synveil_session={session}; synveil_csrf={csrf}"))
            .expect("valid cookie"),
    );
    overflow.headers_mut().insert(
        "x-csrf-token",
        HeaderValue::from_str(&csrf).expect("valid CSRF"),
    );
    overflow.headers_mut().insert(
        "idempotency-key",
        HeaderValue::from_str(&key()).expect("valid key"),
    );
    let overflow = send(&api_state, overflow).await;
    assert_eq!(overflow.status(), StatusCode::BAD_REQUEST);
    assert_eq!(state_counts(&audit_pool).await, before_policy_rejections);

    let before_policy = state_counts(&audit_pool).await;
    let first_policy = send(
        &api_state,
        mutation_request(
            Method::POST,
            &policy_path,
            Some(policy_body.clone()),
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&policy_key),
        ),
    )
    .await;
    assert_eq!(first_policy.status(), StatusCode::CREATED);
    let first_policy = json_body(first_policy).await;
    assert_no_physical_identity(&first_policy);
    let policy_revision_id = first_policy["data"]["policy_revision_id"].clone();
    let replay_policy = send(
        &api_state,
        mutation_request(
            Method::POST,
            &policy_path,
            Some(policy_body.clone()),
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&policy_key),
        ),
    )
    .await;
    assert_eq!(replay_policy.status(), StatusCode::CREATED);
    assert_eq!(
        json_body(replay_policy).await["data"]["policy_revision_id"],
        policy_revision_id
    );
    let changed_policy = send(
        &api_state,
        mutation_request(
            Method::POST,
            &policy_path,
            Some(json!({"keep_latest_completed": 2, "expire_after_seconds": 1})),
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&policy_key),
        ),
    )
    .await;
    assert_eq!(changed_policy.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(changed_policy).await["error"]["code"],
        "idempotency_conflict"
    );
    let after_policy = state_counts(&audit_pool).await;
    assert_eq!(
        after_policy["backup_snapshot_retention_policy_revisions"],
        before_policy["backup_snapshot_retention_policy_revisions"] + 1
    );
    for table in [
        "backup_snapshots",
        "backup_snapshot_expiry_plans",
        "backup_snapshot_expiry_executions",
        "backup_maintenance_runs",
        "backup_prune_plans",
        "backup_prune_executions",
        "object_gc_candidates",
        "object_gc_operations",
    ] {
        assert_eq!(after_policy[table], before_policy[table], "table: {table}");
    }
    let policy_get = send(&api_state, authenticated_get(&policy_path, &session, &csrf)).await;
    assert_eq!(policy_get.status(), StatusCode::OK);
    assert_eq!(
        json_body(policy_get).await["data"]["policy_revision_id"],
        policy_revision_id
    );

    let concurrent_policy_key = key();
    let policy_a = mutation_request(
        Method::POST,
        &policy_path,
        Some(policy_body.clone()),
        Some(&session),
        Some(&csrf),
        Some(&csrf),
        Some(&concurrent_policy_key),
    );
    let policy_b = mutation_request(
        Method::POST,
        &policy_path,
        Some(policy_body.clone()),
        Some(&session),
        Some(&csrf),
        Some(&csrf),
        Some(&concurrent_policy_key),
    );
    let policy_count = count(&audit_pool, "backup_snapshot_retention_policy_revisions").await;
    let (policy_a, policy_b) = tokio::join!(send(&api_state, policy_a), send(&api_state, policy_b));
    assert_eq!(policy_a.status(), StatusCode::CREATED);
    assert_eq!(policy_b.status(), StatusCode::CREATED);
    let policy_a = json_body(policy_a).await;
    let policy_b = json_body(policy_b).await;
    assert_eq!(
        policy_a["data"]["policy_revision_id"],
        policy_b["data"]["policy_revision_id"]
    );
    assert_eq!(
        count(&audit_pool, "backup_snapshot_retention_policy_revisions").await,
        policy_count + 1
    );

    let foreign_policy = send(
        &foreign_state,
        mutation_request(
            Method::POST,
            &policy_path,
            Some(policy_body.clone()),
            Some(&foreign_session),
            Some(&foreign_csrf),
            Some(&foreign_csrf),
            Some(&key()),
        ),
    )
    .await;
    assert_eq!(foreign_policy.status(), StatusCode::NOT_FOUND);

    let maintenance_path = format!("/api/v1/backups/sets/{backup_set_id}/maintenance-runs");
    let stale_create_key = key();
    let stale_run = send(
        &api_state,
        mutation_request(
            Method::POST,
            &maintenance_path,
            None,
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&stale_create_key),
        ),
    )
    .await;
    assert_eq!(stale_run.status(), StatusCode::CREATED);
    let stale_run = json_body(stale_run).await;
    assert_eq!(stale_run["data"]["state"], "CREATED");
    let stale_run_id = stale_run["data"]["maintenance_run_id"]
        .as_str()
        .expect("maintenance create must return ID")
        .to_owned();
    let stale_replay = send(
        &api_state,
        mutation_request(
            Method::POST,
            &maintenance_path,
            None,
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&stale_create_key),
        ),
    )
    .await;
    assert_eq!(stale_replay.status(), StatusCode::CREATED);
    assert_eq!(
        json_body(stale_replay).await["data"]["maintenance_run_id"],
        stale_run_id
    );
    let maintenance_key_conflict = send(
        &api_state,
        mutation_request(
            Method::POST,
            &format!("/api/v1/backups/sets/{secondary_set_id}/maintenance-runs"),
            None,
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&stale_create_key),
        ),
    )
    .await;
    assert_eq!(maintenance_key_conflict.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(maintenance_key_conflict).await["error"]["code"],
        "idempotency_conflict"
    );
    assert_eq!(count(&audit_pool, "backup_snapshots").await, 0);
    let active_conflict = send(
        &api_state,
        mutation_request(
            Method::POST,
            &maintenance_path,
            None,
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&key()),
        ),
    )
    .await;
    assert_eq!(active_conflict.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(active_conflict).await["error"]["code"],
        "maintenance_already_running"
    );
    let foreign_maintenance = send(
        &foreign_state,
        mutation_request(
            Method::POST,
            &maintenance_path,
            None,
            Some(&foreign_session),
            Some(&foreign_csrf),
            Some(&foreign_csrf),
            Some(&key()),
        ),
    )
    .await;
    assert_eq!(foreign_maintenance.status(), StatusCode::NOT_FOUND);

    let policy_change = send(
        &api_state,
        mutation_request(
            Method::POST,
            &policy_path,
            Some(json!({"keep_latest_completed": 1, "expire_after_seconds": 1})),
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&key()),
        ),
    )
    .await;
    assert_eq!(policy_change.status(), StatusCode::CREATED);
    let advance_path = format!("/api/v1/backups/maintenance-runs/{stale_run_id}/advance");
    let advance_key = key();
    let before_bad_advance = state_counts(&audit_pool).await;
    let bad_advance = send(
        &api_state,
        mutation_request(
            Method::POST,
            &advance_path,
            None,
            Some(&session),
            Some(&csrf),
            Some(&invalid_csrf),
            Some(&advance_key),
        ),
    )
    .await;
    assert_eq!(bad_advance.status(), StatusCode::FORBIDDEN);
    assert_eq!(state_counts(&audit_pool).await, before_bad_advance);
    let stale_advance = send(
        &api_state,
        mutation_request(
            Method::POST,
            &advance_path,
            None,
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&advance_key),
        ),
    )
    .await;
    assert_eq!(stale_advance.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(stale_advance).await["error"]["code"],
        "maintenance_run_stale"
    );
    let stale_get_path = format!("/api/v1/backups/maintenance-runs/{stale_run_id}");
    let stale_get = send(
        &api_state,
        authenticated_get(&stale_get_path, &session, &csrf),
    )
    .await;
    assert_eq!(stale_get.status(), StatusCode::OK);
    let stale_get = json_body(stale_get).await;
    assert_eq!(stale_get["data"]["state"], "STALE");
    let stale_snapshot_id = stale_get["data"]["captured_snapshot_id"]
        .as_str()
        .expect("stale run must retain captured snapshot ID")
        .to_owned();
    let snapshot_count = count(&audit_pool, "backup_snapshots").await;
    let stale_replay = send(
        &api_state,
        mutation_request(
            Method::POST,
            &advance_path,
            None,
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&advance_key),
        ),
    )
    .await;
    assert_eq!(stale_replay.status(), StatusCode::CONFLICT);
    assert_eq!(count(&audit_pool, "backup_snapshots").await, snapshot_count);
    let foreign_advance = send(
        &foreign_state,
        mutation_request(
            Method::POST,
            &advance_path,
            None,
            Some(&foreign_session),
            Some(&foreign_csrf),
            Some(&foreign_csrf),
            Some(&key()),
        ),
    )
    .await;
    assert_eq!(foreign_advance.status(), StatusCode::NOT_FOUND);

    sqlx::query(
        "UPDATE backup_snapshots
         SET created_at = '2020-01-01T00:00:00Z', committed_at = '2020-01-01T00:00:01Z'
         WHERE id = $1",
    )
    .bind(
        stale_snapshot_id
            .parse::<uuid::Uuid>()
            .expect("valid snapshot UUID"),
    )
    .execute(&audit_pool)
    .await
    .expect("old completed snapshot fixture must persist");

    let concurrent_create_key = key();
    let create_a = mutation_request(
        Method::POST,
        &maintenance_path,
        None,
        Some(&session),
        Some(&csrf),
        Some(&csrf),
        Some(&concurrent_create_key),
    );
    let create_b = mutation_request(
        Method::POST,
        &maintenance_path,
        None,
        Some(&session),
        Some(&csrf),
        Some(&csrf),
        Some(&concurrent_create_key),
    );
    let run_count = count(&audit_pool, "backup_maintenance_runs").await;
    let (create_a, create_b) = tokio::join!(send(&api_state, create_a), send(&api_state, create_b));
    assert_eq!(create_a.status(), StatusCode::CREATED);
    assert_eq!(create_b.status(), StatusCode::CREATED);
    let create_a = json_body(create_a).await;
    let create_b = json_body(create_b).await;
    assert_eq!(
        create_a["data"]["maintenance_run_id"],
        create_b["data"]["maintenance_run_id"]
    );
    assert_eq!(
        count(&audit_pool, "backup_maintenance_runs").await,
        run_count + 1
    );
    let completed_run_id = create_a["data"]["maintenance_run_id"]
        .as_str()
        .expect("concurrent create must return ID")
        .to_owned();

    let completed_advance_path =
        format!("/api/v1/backups/maintenance-runs/{completed_run_id}/advance");
    let concurrent_advance_key = key();
    let advance_a = mutation_request(
        Method::POST,
        &completed_advance_path,
        None,
        Some(&session),
        Some(&csrf),
        Some(&csrf),
        Some(&concurrent_advance_key),
    );
    let advance_b = mutation_request(
        Method::POST,
        &completed_advance_path,
        None,
        Some(&session),
        Some(&csrf),
        Some(&csrf),
        Some(&concurrent_advance_key),
    );
    let before_advance = state_counts(&audit_pool).await;
    let (advance_a, advance_b) =
        tokio::join!(send(&api_state, advance_a), send(&api_state, advance_b));
    assert_eq!(advance_a.status(), StatusCode::OK);
    assert_eq!(advance_b.status(), StatusCode::OK);
    let advance_a = json_body(advance_a).await;
    let advance_b = json_body(advance_b).await;
    assert_eq!(advance_a["data"], advance_b["data"]);
    assert_eq!(advance_a["data"]["state"], "COMPLETED");
    assert_no_physical_identity(&advance_a);
    let completed_snapshot_id = advance_a["data"]["captured_snapshot_id"].clone();
    let completed_plan_id = advance_a["data"]["expiry_plan_id"].clone();
    let completed_execution_id = advance_a["data"]["expiry_execution_id"].clone();
    let after_advance = state_counts(&audit_pool).await;
    assert_eq!(
        after_advance["backup_snapshots"],
        before_advance["backup_snapshots"] + 1
    );
    assert_eq!(
        after_advance["backup_snapshot_expiry_plans"],
        before_advance["backup_snapshot_expiry_plans"] + 1
    );
    assert_eq!(
        after_advance["backup_snapshot_expiry_executions"],
        before_advance["backup_snapshot_expiry_executions"] + 1
    );
    assert_eq!(
        after_advance["backup_prune_plans"],
        before_advance["backup_prune_plans"]
    );
    assert_eq!(
        after_advance["backup_prune_executions"],
        before_advance["backup_prune_executions"]
    );
    assert_eq!(
        after_advance["object_gc_candidates"],
        before_advance["object_gc_candidates"]
    );
    assert_eq!(
        after_advance["object_gc_operations"],
        before_advance["object_gc_operations"]
    );

    let completed_replay = send(
        &api_state,
        mutation_request(
            Method::POST,
            &completed_advance_path,
            None,
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&concurrent_advance_key),
        ),
    )
    .await;
    assert_eq!(completed_replay.status(), StatusCode::OK);
    let completed_replay = json_body(completed_replay).await;
    assert_eq!(
        completed_replay["data"]["captured_snapshot_id"],
        completed_snapshot_id
    );
    assert_eq!(
        completed_replay["data"]["expiry_plan_id"],
        completed_plan_id
    );
    assert_eq!(
        completed_replay["data"]["expiry_execution_id"],
        completed_execution_id
    );
    assert_eq!(state_counts(&audit_pool).await, after_advance);

    let expired_state: String =
        sqlx::query_scalar("SELECT state FROM backup_snapshots WHERE id = $1")
            .bind(
                stale_snapshot_id
                    .parse::<uuid::Uuid>()
                    .expect("valid snapshot UUID"),
            )
            .fetch_one(&audit_pool)
            .await
            .expect("expired snapshot state must be readable");
    assert_eq!(expired_state, "EXPIRED");
    let retained_pins: i64 = sqlx::query_scalar(
        "SELECT count(*)::BIGINT FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
    )
    .bind(
        stale_snapshot_id
            .parse::<uuid::Uuid>()
            .expect("valid snapshot UUID"),
    )
    .fetch_one(&audit_pool)
    .await
    .expect("retained pin count must be readable");
    assert!(retained_pins > 0, "expiry must retain snapshot pins");

    let completed_get = send(
        &api_state,
        authenticated_get(
            &format!("/api/v1/backups/maintenance-runs/{completed_run_id}"),
            &session,
            &csrf,
        ),
    )
    .await;
    assert_eq!(completed_get.status(), StatusCode::OK);
    assert_eq!(
        json_body(completed_get).await["data"],
        completed_replay["data"]
    );

    let lost_key = key();
    let lost_count = count(&audit_pool, "backup_maintenance_runs").await;
    let lost_response = send(
        &api_state,
        mutation_request(
            Method::POST,
            &maintenance_path,
            None,
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&lost_key),
        ),
    )
    .await;
    assert_eq!(lost_response.status(), StatusCode::CREATED);
    drop(lost_response);
    let recovered = send(
        &api_state,
        mutation_request(
            Method::POST,
            &maintenance_path,
            None,
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&lost_key),
        ),
    )
    .await;
    assert_eq!(recovered.status(), StatusCode::CREATED);
    let recovered_run_id = json_body(recovered).await["data"]["maintenance_run_id"]
        .as_str()
        .expect("recovered response must return run ID")
        .to_owned();
    assert_eq!(
        count(&audit_pool, "backup_maintenance_runs").await,
        lost_count + 1
    );
    let recovered_advance_path =
        format!("/api/v1/backups/maintenance-runs/{recovered_run_id}/advance");
    let recovered_advance = send(
        &api_state,
        mutation_request(
            Method::POST,
            &recovered_advance_path,
            None,
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&key()),
        ),
    )
    .await;
    assert_eq!(recovered_advance.status(), StatusCode::OK);

    let different_a_key = key();
    let different_b_key = key();
    let different_a = mutation_request(
        Method::POST,
        &maintenance_path,
        None,
        Some(&session),
        Some(&csrf),
        Some(&csrf),
        Some(&different_a_key),
    );
    let different_b = mutation_request(
        Method::POST,
        &maintenance_path,
        None,
        Some(&session),
        Some(&csrf),
        Some(&csrf),
        Some(&different_b_key),
    );
    let before_different = count(&audit_pool, "backup_maintenance_runs").await;
    let (different_a, different_b) =
        tokio::join!(send(&api_state, different_a), send(&api_state, different_b));
    let statuses = [different_a.status(), different_b.status()];
    assert!(statuses.contains(&StatusCode::CREATED));
    assert!(statuses.contains(&StatusCode::CONFLICT));
    assert_eq!(
        count(&audit_pool, "backup_maintenance_runs").await,
        before_different + 1
    );

    let final_counts = state_counts(&audit_pool).await;
    assert_eq!(final_counts["backup_prune_plans"], 0);
    assert_eq!(final_counts["backup_prune_executions"], 0);
    assert_eq!(final_counts["object_gc_candidates"], 0);
    assert_eq!(final_counts["object_gc_operations"], 0);
    assert_eq!(
        final_counts["change_journal"],
        before_invalid["change_journal"]
    );
    assert_eq!(
        final_counts["library_sync_head"],
        before_invalid["library_sync_head"]
    );
    assert_eq!(
        final_counts["library_journal_epoch"],
        before_invalid["library_journal_epoch"]
    );
    assert_eq!(
        final_counts["device_sync_checkpoints"],
        before_invalid["device_sync_checkpoints"]
    );
}
