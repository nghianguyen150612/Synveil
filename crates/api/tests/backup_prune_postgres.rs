//! PostgreSQL-backed verification for the explicit two-phase backup prune API.
//! Run against a fresh disposable PostgreSQL 17 database with
//! `SYNVEIL_TEST_DATABASE_URL=... cargo test -p synveil-api --test backup_prune_postgres -- --ignored`.

use std::{sync::Arc, time::Duration};

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
    BackupSetId, DedupDomainId, FileVersion, FileVersionId, Library, LibraryId, LogicalName,
    LoginIdentifier, Node, NodeId, NodeKind, ObjectId, ObjectReference, Sha256Digest, SnapshotId,
    Timestamp, TrashRetentionPolicy, User, UserId, UserStatus,
};
use synveil_metadata::{
    BackupService, DatabaseConfig, DatabasePool, DomainRepository, FileMetadataService,
    MigrationRunner, PurgeExecutionResult, TrashRetentionService,
};
use tower::ServiceExt;

const PASSWORD: &str = "prompt-54-isolated-password";
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

fn key() -> String {
    uuid::Uuid::now_v7().hyphenated().to_string()
}

fn authenticated_get(uri: &str, session: &str, csrf: &str) -> Request<Body> {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .expect("GET request must be valid");
    request.headers_mut().insert(
        "cookie",
        HeaderValue::from_str(&format!("synveil_session={session}; synveil_csrf={csrf}"))
            .expect("test cookies must be valid"),
    );
    request
}

fn mutation_request(
    uri: &str,
    body: Option<Value>,
    session: Option<&str>,
    csrf_cookie: Option<&str>,
    csrf_header: Option<&str>,
    idempotency_key: Option<&str>,
) -> Request<Body> {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .body(
            body.as_ref()
                .map_or_else(Body::empty, |body| Body::from(body.to_string())),
        )
        .expect("POST request must be valid");
    if body.is_some() {
        request
            .headers_mut()
            .insert("content-type", HeaderValue::from_static("application/json"));
    }
    if let (Some(session), Some(csrf_cookie)) = (session, csrf_cookie) {
        request.headers_mut().insert(
            "cookie",
            HeaderValue::from_str(&format!(
                "synveil_session={session}; synveil_csrf={csrf_cookie}"
            ))
            .expect("test cookies must be valid"),
        );
    }
    if let Some(csrf_header) = csrf_header {
        request.headers_mut().insert(
            "x-csrf-token",
            HeaderValue::from_str(csrf_header).expect("CSRF token must be valid"),
        );
    }
    if let Some(idempotency_key) = idempotency_key {
        request.headers_mut().insert(
            "idempotency-key",
            HeaderValue::from_str(idempotency_key).expect("idempotency key must be valid"),
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
        .expect("backup prune HTTP request must not fail")
}

async fn bootstrap_and_login(state: &ApiState, login: &str) -> (String, String, UserId) {
    for (path, body) in [
        (
            "/api/v1/bootstrap/admin",
            json!({"login": login, "login_key": login, "password": PASSWORD}),
        ),
        (
            "/api/v1/auth/login",
            json!({"login": login, "login_key": login, "password": PASSWORD}),
        ),
    ] {
        let mut request = mutation_request(path, Some(body), None, None, None, None);
        request.headers_mut().remove("origin");
        request.headers_mut().remove("sec-fetch-site");
        let response = send(state, request).await;
        assert_eq!(response.status(), StatusCode::OK, "path: {path}");
        if path.ends_with("login") {
            let session = cookie_value(&response, "synveil_session");
            let csrf = cookie_value(&response, "synveil_csrf");
            let body = json_body(response).await;
            let owner = body["data"]["user_id"]
                .as_str()
                .expect("login must return owner ID")
                .parse()
                .expect("owner ID must be canonical");
            return (session, csrf, owner);
        }
    }
    unreachable!("the login request always returns")
}

struct Seed {
    backup_set_id: BackupSetId,
    expired_snapshot_id: SnapshotId,
    zero_content_snapshot_id: SnapshotId,
}

async fn seed_expired_content_and_zero_content_snapshots(
    pool: &DatabasePool,
    audit_pool: &PgPool,
    owner_user_id: UserId,
) -> Seed {
    let observed_at =
        Timestamp::parse("2026-08-31T00:00:00.123456Z").expect("fixture time must be valid");
    let repository = DomainRepository::new(pool);
    let library_id = LibraryId::new();
    let root = Node::new_root(
        NodeId::new(),
        library_id,
        LogicalName::new("root").expect("root name must be valid"),
        observed_at,
    );
    let library = Library::new(
        library_id,
        owner_user_id,
        LogicalName::new("Prompt 54 library").expect("library name must be valid"),
        &root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("library must be valid");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("library must persist");
    let file = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::File,
        LogicalName::new("only-in-history.bin").expect("file name must be valid"),
        observed_at,
    )
    .expect("file must be valid");
    repository
        .insert_node(&file)
        .await
        .expect("file must persist");
    let object = ObjectReference::new(
        ObjectId::new(),
        library.dedup_domain_id(),
        Sha256Digest::from_bytes([0x54; 32]),
        54,
    );
    repository
        .insert_object(object, observed_at)
        .await
        .expect("Object metadata must persist");
    let version = FileVersion::new(
        FileVersionId::new(),
        &library,
        &file,
        object,
        None,
        observed_at,
    )
    .expect("file version must be valid");
    repository
        .insert_file_version(version)
        .await
        .expect("file version must persist");
    let file = file
        .with_current_version(&version, observed_at)
        .expect("file version must bind to file");
    repository
        .update_node(&file)
        .await
        .expect("file head must persist");

    let backup = BackupService::new(pool.clone());
    let backup_set = backup
        .create_backup_set(
            owner_user_id,
            BackupSetId::new(),
            LogicalName::new("prompt-54-backup").expect("backup name must be valid"),
            library_id,
            Some(30),
            observed_at,
        )
        .await
        .expect("backup set must persist");
    backup
        .configure_snapshot_retention_policy(
            owner_user_id,
            "prompt54-policy".to_owned(),
            backup_set.id(),
            1,
            1,
        )
        .await
        .expect("retention policy must persist");
    let expired_snapshot = backup
        .capture_snapshot(
            owner_user_id,
            backup_set.id(),
            SnapshotId::new(),
            "prompt54-capture-content".to_owned(),
        )
        .await
        .expect("content snapshot must persist");

    // Remove the only live FileVersion through the accepted metadata purge
    // lifecycle. The snapshot's retention pin remains, so this is a genuine
    // zero-reference-after-prune case rather than a direct reference edit.
    let files = FileMetadataService::new(pool.clone());
    let trashed = files
        .delete_node(owner_user_id, file.id(), file.revision())
        .await
        .expect("fixture file must enter Trash");
    sqlx::query(
        "UPDATE nodes SET trashed_at = clock_timestamp() - INTERVAL '5 seconds'
         WHERE id = $1 AND library_id = $2",
    )
    .bind(trashed.id().into_uuid())
    .bind(library_id.into_uuid())
    .execute(audit_pool)
    .await
    .expect("fixture trash age must update");
    let retention = TrashRetentionService::new(
        pool.clone(),
        TrashRetentionPolicy::new(Duration::from_secs(1)).expect("retention must be valid"),
    );
    let purging = retention
        .begin_node_purge(owner_user_id, trashed.id(), trashed.revision())
        .await
        .expect("fixture file must begin purge");
    assert_eq!(
        retention
            .execute_metadata_purge(owner_user_id, purging.id(), purging.revision())
            .await
            .expect("fixture metadata purge must complete"),
        PurgeExecutionResult::Completed
    );

    // A newer, zero-content snapshot makes the older content snapshot
    // canonically eligible for expiry. This uses Prompt 45's accepted expiry
    // service; maintenance orchestration is separately regression-tested by
    // Prompt 52 and must not create a prune plan automatically.
    let zero_content_snapshot = backup
        .capture_snapshot(
            owner_user_id,
            backup_set.id(),
            SnapshotId::new(),
            "prompt54-capture-zero".to_owned(),
        )
        .await
        .expect("zero-content snapshot must persist");
    sqlx::query(
        "UPDATE backup_snapshots
         SET created_at = '2020-01-01T00:00:00Z', committed_at = '2020-01-01T00:00:01Z'
         WHERE id = $1",
    )
    .bind(expired_snapshot.id().into_uuid())
    .execute(audit_pool)
    .await
    .expect("old snapshot time must update");
    let expiry_plan = backup
        .create_snapshot_expiry_plan(
            owner_user_id,
            "prompt54-expiry-plan".to_owned(),
            backup_set.id(),
        )
        .await
        .expect("expiry plan must persist");
    backup
        .execute_snapshot_expiry_plan(owner_user_id, expiry_plan.id())
        .await
        .expect("expiry execution must complete");
    let expired_state: String =
        sqlx::query_scalar("SELECT state FROM backup_snapshots WHERE id = $1")
            .bind(expired_snapshot.id().into_uuid())
            .fetch_one(audit_pool)
            .await
            .expect("expired snapshot state must load");
    assert_eq!(expired_state, "EXPIRED");
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*)::BIGINT FROM backup_snapshot_content_pins WHERE snapshot_id = $1",
        )
        .bind(expired_snapshot.id().into_uuid())
        .fetch_one(audit_pool)
        .await
        .expect("snapshot pin count must load"),
        1,
        "expiry must retain the target snapshot pin"
    );

    Seed {
        backup_set_id: backup_set.id(),
        expired_snapshot_id: expired_snapshot.id(),
        zero_content_snapshot_id: zero_content_snapshot.id(),
    }
}

async fn count(pool: &PgPool, table: &str) -> i64 {
    let query = format!("SELECT count(*)::BIGINT FROM {table}");
    sqlx::query_scalar(&query)
        .fetch_one(pool)
        .await
        .expect("audit count must be readable")
}

async fn stable_counts(pool: &PgPool) -> (i64, i64, i64, i64, i64, i64, i64) {
    let (journal_head, journal_epoch): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(sync_head), 0)::BIGINT,
                COALESCE(SUM(journal_epoch), 0)::BIGINT FROM libraries",
    )
    .fetch_one(pool)
    .await
    .expect("journal scope must be readable");
    (
        count(pool, "backup_snapshot_content_pins").await,
        count(pool, "backup_prune_plans").await,
        count(pool, "backup_prune_executions").await,
        count(pool, "object_gc_candidates").await,
        count(pool, "object_gc_operations").await,
        journal_head,
        journal_epoch,
    )
}

fn assert_no_physical_identity(value: &Value) {
    const FORBIDDEN: [&str; 9] = [
        "object_id",
        "object_replica_id",
        "storage_key",
        "backend_locator",
        "object_dedup_domain_id",
        "dedup_domain",
        "retention_pin_id",
        "gc_candidate_id",
        "pin_id",
    ];
    match value {
        Value::Object(values) => values.iter().for_each(|(key, value)| {
            assert!(!FORBIDDEN.contains(&key.as_str()), "forbidden key: {key}");
            assert_no_physical_identity(value);
        }),
        Value::Array(values) => values.iter().for_each(assert_no_physical_identity),
        _ => {}
    }
}

async fn isolated_pool(label: &str) -> (Arc<DatabasePool>, PgPool) {
    let base = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a fresh disposable PostgreSQL database");
    let db_name = format!(
        "p54_{}_{}",
        label
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            })
            .collect::<String>(),
        uuid::Uuid::now_v7().simple()
    );
    let maintenance = PgPool::connect(&base)
        .await
        .expect("maintenance connection must succeed");
    sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
        .execute(&maintenance)
        .await
        .expect("isolated test database must be created");
    maintenance.close().await;
    let url = if let Some((prefix, _)) = base.rsplit_once('/') {
        format!("{prefix}/{db_name}")
    } else {
        panic!("test database URL must contain a database path");
    };
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = Arc::new(
        DatabasePool::connect(&config)
            .await
            .expect("test PostgreSQL must accept a connection"),
    );
    let migration = MigrationRunner::new()
        .run(pool.as_ref())
        .await
        .expect("all migrations must apply from empty");
    assert!(migration.is_current());
    assert_eq!(migration.applied_versions().len(), 36);
    let audit_pool = PgPool::connect(&url)
        .await
        .expect("audit connection must succeed");
    (pool, audit_pool)
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_backup_prune_api_is_confirmed_two_phase_and_preserves_history() {
    let (pool, audit_pool) = isolated_pool("prune").await;

    let csrf_key = CsrfKey::from_bytes([0x54; 32]);
    let api_state = ApiState::from_current_platform()
        .with_csrf_key(csrf_key.clone())
        .with_postgres_auth(
            pool.clone(),
            PasswordHasherConfig::new(
                PasswordParameters::new(8 * 1024, 1, 1, 32)
                    .expect("password parameters must be valid"),
            )
            .expect("password config must be valid"),
            SessionConfig::default(),
            RebaselineTokenKey::from_bytes([0x54; 32]),
        )
        .with_readiness(Arc::new(StaticReadiness::new(true)))
        .with_cookie_config(CookieConfig::development())
        .with_allowed_origin(ORIGIN);
    let (session, csrf, owner_user_id) =
        bootstrap_and_login(&api_state, &format!("prompt54-{}", uuid::Uuid::now_v7())).await;
    let seed =
        seed_expired_content_and_zero_content_snapshots(pool.as_ref(), &audit_pool, owner_user_id)
            .await;
    let plan_path = format!(
        "/api/v1/backups/snapshots/{}/prune-plans",
        seed.expired_snapshot_id
    );
    let before_plan = stable_counts(&audit_pool).await;
    assert_eq!(before_plan.1, 0, "expiry must not auto-plan pruning");
    assert_eq!(before_plan.0, 1, "expiry must retain the target pin");

    let unauthenticated = send(
        &api_state,
        mutation_request(&plan_path, None, None, None, None, Some(&key())),
    )
    .await;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
    let missing_csrf = send(
        &api_state,
        mutation_request(
            &plan_path,
            None,
            Some(&session),
            Some(&csrf),
            None,
            Some(&key()),
        ),
    )
    .await;
    assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);
    let invalid_csrf = send(
        &api_state,
        mutation_request(
            &plan_path,
            None,
            Some(&session),
            Some(&csrf),
            Some(&"0".repeat(128)),
            Some(&key()),
        ),
    )
    .await;
    assert_eq!(invalid_csrf.status(), StatusCode::FORBIDDEN);
    for invalid_key in [
        None,
        Some("bad-key"),
        Some("00000000-0000-4000-8000-000000000000"),
    ] {
        let response = send(
            &api_state,
            mutation_request(
                &plan_path,
                None,
                Some(&session),
                Some(&csrf),
                Some(&csrf),
                invalid_key,
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    assert_eq!(stable_counts(&audit_pool).await, before_plan);

    let plan_key = key();
    let created = send(
        &api_state,
        mutation_request(
            &plan_path,
            None,
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&plan_key),
        ),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    assert_eq!(created.headers()["cache-control"], "private, no-store");
    let created = json_body(created).await;
    assert_no_physical_identity(&created);
    assert_eq!(created["data"]["state"], "PLANNED");
    assert_eq!(created["data"]["entry_count"], "1");
    assert_eq!(created["data"]["would_become_unreferenced_count"], "1");
    let plan_id = created["data"]["prune_plan_id"]
        .as_str()
        .expect("plan ID must be present")
        .to_owned();
    let replay = send(
        &api_state,
        mutation_request(
            &plan_path,
            None,
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&plan_key),
        ),
    )
    .await;
    assert_eq!(replay.status(), StatusCode::CREATED);
    assert_eq!(json_body(replay).await["data"]["prune_plan_id"], plan_id);
    let same_key_other_snapshot = send(
        &api_state,
        mutation_request(
            &format!(
                "/api/v1/backups/snapshots/{}/prune-plans",
                seed.zero_content_snapshot_id
            ),
            None,
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&plan_key),
        ),
    )
    .await;
    assert_eq!(same_key_other_snapshot.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(same_key_other_snapshot).await["error"]["code"],
        "idempotency_conflict"
    );
    let active_conflict = send(
        &api_state,
        mutation_request(
            &plan_path,
            None,
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&key()),
        ),
    )
    .await;
    assert_eq!(active_conflict.status(), StatusCode::CONFLICT);

    let plan_get_path = format!("/api/v1/backups/prune-plans/{plan_id}");
    let before_plan_get = stable_counts(&audit_pool).await;
    let plan_get = send(
        &api_state,
        authenticated_get(&plan_get_path, &session, &csrf),
    )
    .await;
    assert_eq!(plan_get.status(), StatusCode::OK);
    assert_eq!(json_body(plan_get).await["data"], created["data"]);
    assert_eq!(stable_counts(&audit_pool).await, before_plan_get);

    let foreign_user = UserId::new();
    DomainRepository::new(pool.as_ref())
        .insert_user(&User::new(
            foreign_user,
            LoginIdentifier::new(
                "prompt54-foreign",
                format!("prompt54-foreign-{foreign_user}"),
            )
            .expect("foreign login must be valid"),
            UserStatus::Active,
            Timestamp::parse("2026-08-31T00:00:00.123456Z").expect("fixture time must be valid"),
        ))
        .await
        .expect("foreign user must persist");
    let foreign_token = SessionToken::from_bytes([0xa4; 32]);
    let foreign_session = foreign_token.to_hex();
    let foreign_csrf = csrf_key.issue(&foreign_token);
    let foreign_state = api_state
        .clone()
        .with_auth_backend(Arc::new(FixedAuthenticationBackend {
            token: foreign_token,
            principal: SessionPrincipal::new(foreign_user, false, SessionId::new()),
        }));
    assert_eq!(
        send(
            &foreign_state,
            authenticated_get(&plan_get_path, &foreign_session, &foreign_csrf)
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &foreign_state,
            mutation_request(
                &plan_path,
                None,
                Some(&foreign_session),
                Some(&foreign_csrf),
                Some(&foreign_csrf),
                Some(&key()),
            ),
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    let execute_path = format!("/api/v1/backups/prune-plans/{plan_id}/execute");
    for confirmation in [
        None,
        Some(json!({})),
        Some(json!({"confirm_snapshot_id": "bad-id"})),
        Some(json!({"confirm_snapshot_id": SnapshotId::new().to_string()})),
    ] {
        let before = stable_counts(&audit_pool).await;
        let response = send(
            &api_state,
            mutation_request(
                &execute_path,
                confirmation,
                Some(&session),
                Some(&csrf),
                Some(&csrf),
                Some(&key()),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(stable_counts(&audit_pool).await, before);
    }
    assert_eq!(
        send(
            &foreign_state,
            mutation_request(
                &execute_path,
                Some(json!({"confirm_snapshot_id": seed.expired_snapshot_id.to_string()})),
                Some(&foreign_session),
                Some(&foreign_csrf),
                Some(&foreign_csrf),
                Some(&key()),
            ),
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    let execution_key = key();
    let lost_response = send(
        &api_state,
        mutation_request(
            &execute_path,
            Some(json!({"confirm_snapshot_id": seed.expired_snapshot_id.to_string()})),
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&execution_key),
        ),
    )
    .await;
    assert_eq!(lost_response.status(), StatusCode::OK);
    drop(lost_response);
    let recovered = send(
        &api_state,
        mutation_request(
            &execute_path,
            Some(json!({"confirm_snapshot_id": seed.expired_snapshot_id.to_string()})),
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&execution_key),
        ),
    )
    .await;
    assert_eq!(recovered.status(), StatusCode::OK);
    let recovered = json_body(recovered).await;
    assert_no_physical_identity(&recovered);
    assert_eq!(recovered["data"]["released_content_reference_count"], "1");
    assert_eq!(recovered["data"]["gc_handoff_count"], "1");
    let execution_id = recovered["data"]["prune_execution_id"]
        .as_str()
        .expect("execution ID must be present")
        .to_owned();
    let after_execution = stable_counts(&audit_pool).await;
    assert_eq!(
        after_execution.0, 0,
        "only target snapshot pins may release"
    );
    assert_eq!(after_execution.1, 1);
    assert_eq!(after_execution.2, 1);
    assert_eq!(after_execution.3, before_plan.3 + 1);
    assert_eq!(
        after_execution.4, before_plan.4,
        "prune must not run physical GC"
    );
    assert_eq!(after_execution.5, before_plan.5, "prune must not journal");
    assert_eq!(
        after_execution.6, before_plan.6,
        "prune must not change epoch"
    );

    let execution_get_path = format!("/api/v1/backups/prune-executions/{execution_id}");
    let before_receipt_get = stable_counts(&audit_pool).await;
    let receipt = send(
        &api_state,
        authenticated_get(&execution_get_path, &session, &csrf),
    )
    .await;
    assert_eq!(receipt.status(), StatusCode::OK);
    assert_eq!(json_body(receipt).await["data"], recovered["data"]);
    assert_eq!(stable_counts(&audit_pool).await, before_receipt_get);
    assert_eq!(
        send(
            &foreign_state,
            authenticated_get(&execution_get_path, &foreign_session, &foreign_csrf),
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    for path in [
        format!("/api/v1/backups/snapshots/{}", seed.expired_snapshot_id),
        format!(
            "/api/v1/backups/snapshots/{}/nodes",
            seed.expired_snapshot_id
        ),
    ] {
        let response = send(&api_state, authenticated_get(&path, &session, &csrf)).await;
        assert_eq!(response.status(), StatusCode::OK, "path: {path}");
        let body = json_body(response).await;
        assert_no_physical_identity(&body);
        if path.ends_with(&seed.expired_snapshot_id.to_string()) {
            assert_eq!(body["data"]["state"], "EXPIRED");
        }
    }
    assert!(
        count(&audit_pool, "backup_snapshot_nodes").await > 0,
        "historical manifest must remain"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*)::BIGINT FROM backup_prune_executions")
            .fetch_one(&audit_pool)
            .await
            .expect("receipt count must load"),
        1
    );

    // The newer source is a canonical zero-content snapshot. Make it eligible
    // through a second expiry plan, then prove execution still produces one
    // receipt with zero release and zero handoff.
    sqlx::query(
        "UPDATE backup_snapshots
         SET created_at = '2020-01-02T00:00:00Z', committed_at = '2020-01-02T00:00:01Z'
         WHERE id = $1",
    )
    .bind(seed.zero_content_snapshot_id.into_uuid())
    .execute(&audit_pool)
    .await
    .expect("zero-content snapshot time must update");
    let backup = BackupService::new(pool.as_ref().clone());
    backup
        .capture_snapshot(
            owner_user_id,
            seed.backup_set_id,
            SnapshotId::new(),
            "prompt54-capture-newest-zero".to_owned(),
        )
        .await
        .expect("newest zero-content snapshot must persist");
    let second_expiry = backup
        .create_snapshot_expiry_plan(
            owner_user_id,
            "prompt54-expiry-plan-zero".to_owned(),
            seed.backup_set_id,
        )
        .await
        .expect("second expiry plan must persist");
    backup
        .execute_snapshot_expiry_plan(owner_user_id, second_expiry.id())
        .await
        .expect("second expiry execution must persist");
    let zero_plan_path = format!(
        "/api/v1/backups/snapshots/{}/prune-plans",
        seed.zero_content_snapshot_id
    );
    let zero_plan_key = key();
    let zero_plan_a = mutation_request(
        &zero_plan_path,
        None,
        Some(&session),
        Some(&csrf),
        Some(&csrf),
        Some(&zero_plan_key),
    );
    let zero_plan_b = mutation_request(
        &zero_plan_path,
        None,
        Some(&session),
        Some(&csrf),
        Some(&csrf),
        Some(&zero_plan_key),
    );
    let (zero_plan_a, zero_plan_b) =
        tokio::join!(send(&api_state, zero_plan_a), send(&api_state, zero_plan_b));
    assert_eq!(zero_plan_a.status(), StatusCode::CREATED);
    assert_eq!(zero_plan_b.status(), StatusCode::CREATED);
    let zero_plan = json_body(zero_plan_a).await;
    assert_eq!(json_body(zero_plan_b).await["data"], zero_plan["data"]);
    assert_eq!(zero_plan["data"]["entry_count"], "0");
    let zero_plan_id = zero_plan["data"]["prune_plan_id"]
        .as_str()
        .expect("zero-content plan ID must be present");
    let zero_execute_path = format!("/api/v1/backups/prune-plans/{zero_plan_id}/execute");
    let zero_execution_key = key();
    let zero_execute_a = mutation_request(
        &zero_execute_path,
        Some(json!({"confirm_snapshot_id": seed.zero_content_snapshot_id.to_string()})),
        Some(&session),
        Some(&csrf),
        Some(&csrf),
        Some(&zero_execution_key),
    );
    let zero_execute_b = mutation_request(
        &zero_execute_path,
        Some(json!({"confirm_snapshot_id": seed.zero_content_snapshot_id.to_string()})),
        Some(&session),
        Some(&csrf),
        Some(&csrf),
        Some(&zero_execution_key),
    );
    let (zero_execution_a, zero_execution_b) = tokio::join!(
        send(&api_state, zero_execute_a),
        send(&api_state, zero_execute_b)
    );
    assert_eq!(zero_execution_a.status(), StatusCode::OK);
    assert_eq!(zero_execution_b.status(), StatusCode::OK);
    let zero_execution = json_body(zero_execution_a).await;
    assert_eq!(
        json_body(zero_execution_b).await["data"],
        zero_execution["data"]
    );
    assert_eq!(
        zero_execution["data"]["released_content_reference_count"],
        "0"
    );
    assert_eq!(zero_execution["data"]["gc_handoff_count"], "0");

    let already_pruned = send(
        &api_state,
        mutation_request(
            &plan_path,
            None,
            Some(&session),
            Some(&csrf),
            Some(&csrf),
            Some(&key()),
        ),
    )
    .await;
    assert_eq!(already_pruned.status(), StatusCode::CONFLICT);
    audit_pool.close().await;
}
