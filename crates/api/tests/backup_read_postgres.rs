//! PostgreSQL-backed verification for the authenticated read-only backup API.
//! Run against a fresh disposable PostgreSQL 17 database with
//! `SYNVEIL_TEST_DATABASE_URL=... cargo test -p synveil-api --test backup_read_postgres -- --ignored`.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{HeaderValue, Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use synveil_api::{ApiState, CookieConfig, RebaselineTokenKey, StaticReadiness, router};
use synveil_auth::{PasswordHasherConfig, PasswordParameters, SessionConfig};
use synveil_core::{
    BackupMaintenanceRun, BackupSetId, BackupSnapshot, DedupDomainId, FileVersion, FileVersionId,
    Library, LibraryId, LogicalName, Node, NodeId, NodeKind, ObjectId, ObjectReference,
    Sha256Digest, SnapshotId, Timestamp, UserId,
};
use synveil_metadata::{
    BackupService, DatabaseConfig, DatabasePool, DomainRepository, MigrationRunner,
};
use tower::ServiceExt;
use uuid::Uuid;

const PASSWORD: &str = "prompt-51-isolated-password";

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReadCounts {
    snapshots: i64,
    manifest_nodes: i64,
    retention_pins: i64,
    restore_plans: i64,
    restore_executions: i64,
    prune_plans: i64,
    prune_executions: i64,
    expiry_plans: i64,
    expiry_executions: i64,
    maintenance_runs: i64,
    gc_candidates: i64,
    journal_entries: i64,
    journal_head_sum: i64,
    journal_epoch_sum: i64,
    device_checkpoints: i64,
    device_ack_sum: i64,
}

struct SeededBackup {
    backup_set_id: BackupSetId,
    snapshot: BackupSnapshot,
    maintenance_run: BackupMaintenanceRun,
    target_library_id: LibraryId,
    target_root_node_id: NodeId,
    root_node_id: NodeId,
    docs_node_id: NodeId,
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

fn assert_no_physical_identity(value: &Value) {
    const FORBIDDEN_KEYS: [&str; 13] = [
        "ObjectId",
        "ObjectReplicaId",
        "object_id",
        "objectId",
        "object_replica_id",
        "objectReplicaId",
        "storage_key",
        "storageKey",
        "backend_locator",
        "dedup_domain_id",
        "retention_pin_id",
        "gc_candidate_id",
        "capture_operation_id",
    ];
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                assert!(!FORBIDDEN_KEYS.contains(&key.as_str()), "{key} leaked");
                assert_no_physical_identity(value);
            }
        }
        Value::Array(values) => values.iter().for_each(assert_no_physical_identity),
        _ => {}
    }
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

fn authenticated_get_without_csrf(uri: &str, session: &str) -> Request<Body> {
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

fn mutation_request(
    uri: &str,
    body: Option<Value>,
    session: &str,
    csrf: &str,
    idempotency_key: &str,
) -> Request<Body> {
    let has_body = body.is_some();
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
        .expect("test mutation must be valid");
    if has_body {
        request
            .headers_mut()
            .insert("content-type", HeaderValue::from_static("application/json"));
    }
    request.headers_mut().insert(
        "cookie",
        HeaderValue::from_str(&format!("synveil_session={session}; synveil_csrf={csrf}"))
            .expect("test cookies must be valid"),
    );
    request.headers_mut().insert(
        "x-csrf-token",
        HeaderValue::from_str(csrf).expect("test CSRF header must be valid"),
    );
    request.headers_mut().insert(
        "idempotency-key",
        HeaderValue::from_str(idempotency_key).expect("test idempotency key must be valid"),
    );
    request
        .headers_mut()
        .insert("sec-fetch-site", HeaderValue::from_static("same-origin"));
    request
        .headers_mut()
        .insert("origin", HeaderValue::from_static("https://app.example"));
    request
}

fn key() -> String {
    uuid::Uuid::now_v7().hyphenated().to_string()
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

async fn seed_backup(
    pool: &DatabasePool,
    audit_pool: &PgPool,
    owner_user_id: UserId,
) -> SeededBackup {
    let observed_at =
        Timestamp::parse("2026-08-31T00:00:00.123456Z").expect("fixture timestamp must be valid");
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
        LogicalName::new("Prompt 51 library").expect("library name must be valid"),
        &root,
        DedupDomainId::new(),
        observed_at,
    )
    .expect("library must be valid");
    repository
        .insert_library_with_root(&library, &root)
        .await
        .expect("library must persist");

    let target_library_id = LibraryId::new();
    let target_root = Node::new_root(
        NodeId::new(),
        target_library_id,
        LogicalName::new("root").expect("target root name must be valid"),
        observed_at,
    );
    let target_library = Library::new(
        target_library_id,
        owner_user_id,
        LogicalName::new("Prompt 55 restore target").expect("target library name must be valid"),
        &target_root,
        library.dedup_domain_id(),
        observed_at,
    )
    .expect("target library must be valid");
    repository
        .insert_library_with_root(&target_library, &target_root)
        .await
        .expect("target library must persist");

    let docs = Node::new_child(
        NodeId::new(),
        library_id,
        &root,
        NodeKind::Directory,
        LogicalName::new("docs").expect("directory name must be valid"),
        observed_at,
    )
    .expect("directory must be valid");
    repository
        .insert_node(&docs)
        .await
        .expect("directory must persist");
    let file = Node::new_child(
        NodeId::new(),
        library_id,
        &docs,
        NodeKind::File,
        LogicalName::new("report.pdf").expect("file name must be valid"),
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
        Sha256Digest::from_bytes([0x51; 32]),
        42,
    );
    repository
        .insert_object(object, observed_at)
        .await
        .expect("object must persist");
    sqlx::query(
        "INSERT INTO object_replicas
            (id, object_id, object_dedup_domain_id, backend_kind, storage_key,
             stored_length, stored_sha256, backend_version, state, created_at, verified_at)
         VALUES ($1, $2, $3, 'LOCAL_FILESYSTEM', $4, $5::NUMERIC, $6, 'v1',
                 'VERIFIED', $7, $7)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(object.object_id().into_uuid())
    .bind(object.dedup_domain_id().into_uuid())
    .bind(format!("prompt55-verified-replica-{}", object.object_id()))
    .bind(object.plaintext_length().to_string())
    .bind(object.canonical_hash().as_bytes().to_vec())
    .bind(observed_at.as_offset_datetime())
    .execute(audit_pool)
    .await
    .expect("verified replica must persist");
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
            LogicalName::new("prompt-51-backup").expect("backup set name must be valid"),
            library_id,
            Some(30),
            observed_at,
        )
        .await
        .expect("backup set must persist");
    backup
        .configure_snapshot_retention_policy(
            owner_user_id,
            "prompt51-policy-operation".to_owned(),
            backup_set.id(),
            3,
            86_400,
        )
        .await
        .expect("retention policy must persist");
    let snapshot = backup
        .capture_snapshot(
            owner_user_id,
            backup_set.id(),
            SnapshotId::new(),
            "prompt51-capture-operation".to_owned(),
        )
        .await
        .expect("snapshot must persist");
    let maintenance_run = backup
        .create_backup_maintenance_run(
            owner_user_id,
            "prompt51-maintenance-operation".to_owned(),
            backup_set.id(),
        )
        .await
        .expect("maintenance run must persist");

    // Seed the accepted historical EXPIRED shape before measuring the GET
    // side-effect boundary. This is test setup, not a public API operation.
    sqlx::query(
        "UPDATE backup_snapshots
         SET state = 'EXPIRED', expired_at = CURRENT_TIMESTAMP
         WHERE id = $1",
    )
    .bind(snapshot.id().into_uuid())
    .execute(audit_pool)
    .await
    .expect("expired snapshot fixture must persist");
    let snapshot = backup
        .get_backup_snapshot(owner_user_id, snapshot.id())
        .await
        .expect("expired snapshot must remain readable");

    SeededBackup {
        backup_set_id: backup_set.id(),
        snapshot,
        maintenance_run,
        target_library_id,
        target_root_node_id: target_root.id(),
        root_node_id: root.id(),
        docs_node_id: docs.id(),
    }
}

async fn counts(pool: &PgPool) -> ReadCounts {
    let (journal_head_sum, journal_epoch_sum): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(sync_head), 0)::BIGINT,
                COALESCE(SUM(journal_epoch), 0)::BIGINT
         FROM libraries",
    )
    .fetch_one(pool)
    .await
    .expect("library journal counters must be readable");
    let device_ack_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(acknowledged_sequence), 0)::BIGINT
         FROM device_sync_checkpoints",
    )
    .fetch_one(pool)
    .await
    .expect("device checkpoint counters must be readable");
    ReadCounts {
        snapshots: count(pool, "backup_snapshots").await,
        manifest_nodes: count(pool, "backup_snapshot_nodes").await,
        retention_pins: count(pool, "backup_snapshot_content_pins").await,
        restore_plans: count(pool, "backup_restore_plans").await,
        restore_executions: count(pool, "backup_restore_executions").await,
        prune_plans: count(pool, "backup_prune_plans").await,
        prune_executions: count(pool, "backup_prune_executions").await,
        expiry_plans: count(pool, "backup_snapshot_expiry_plans").await,
        expiry_executions: count(pool, "backup_snapshot_expiry_executions").await,
        maintenance_runs: count(pool, "backup_maintenance_runs").await,
        gc_candidates: count(pool, "object_gc_candidates").await,
        journal_entries: count(pool, "change_journal").await,
        journal_head_sum,
        journal_epoch_sum,
        device_checkpoints: count(pool, "device_sync_checkpoints").await,
        device_ack_sum,
    }
}

async fn count(pool: &PgPool, table: &str) -> i64 {
    // The table names are fixed by this test and never come from a request.
    let query = format!("SELECT count(*)::BIGINT FROM {table}");
    sqlx::query_scalar(&query)
        .fetch_one(pool)
        .await
        .expect("side-effect audit count must be readable")
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_backup_read_api_is_authenticated_owner_scoped_and_side_effect_free() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a fresh disposable PostgreSQL database");
    let config = DatabaseConfig::from_url(&url).expect("test URL must use PostgreSQL");
    let pool = Arc::new(
        DatabasePool::connect(&config)
            .await
            .expect("test PostgreSQL must accept a connection"),
    );
    let status = MigrationRunner::new()
        .run(pool.as_ref())
        .await
        .expect("all migrations must apply");
    assert!(status.is_current());
    assert_eq!(status.applied_versions().len(), 29);
    let audit_pool = PgPool::connect(&url)
        .await
        .expect("audit connection must succeed");

    let login_name = format!("prompt51-{}", uuid::Uuid::now_v7());
    let api_state = ApiState::from_current_platform()
        .with_postgres_auth(
            pool.clone(),
            PasswordHasherConfig::new(
                PasswordParameters::new(8 * 1024, 1, 1, 32)
                    .expect("password parameters must be valid"),
            )
            .expect("password config must be valid"),
            SessionConfig::default(),
            RebaselineTokenKey::from_bytes([0x51; 32]),
        )
        .with_readiness(Arc::new(StaticReadiness::new(true)))
        .with_cookie_config(CookieConfig::development());
    let (session, csrf, owner_user_id) = login(&api_state, &login_name).await;
    let seeded = seed_backup(pool.as_ref(), &audit_pool, owner_user_id).await;

    // Establish the accepted historical post-prune shape through the trusted
    // metadata service before the read-side effect boundary is measured. The
    // HTTP surface itself has no prune operation.
    let backup = BackupService::new(pool.as_ref().clone());
    let prune_plan = backup
        .create_prune_plan(
            owner_user_id,
            "prompt51-prune-operation".to_owned(),
            seeded.backup_set_id,
            seeded.snapshot.id(),
        )
        .await
        .expect("expired snapshot prune plan must persist");
    backup
        .execute_prune_plan(owner_user_id, prune_plan.id())
        .await
        .expect("expired snapshot prune execution must persist");
    assert_eq!(count(&audit_pool, "backup_snapshot_content_pins").await, 0);
    assert!(count(&audit_pool, "backup_snapshot_nodes").await > 0);
    let before = counts(&audit_pool).await;

    let paths = [
        "/api/v1/backups/sets".to_owned(),
        format!("/api/v1/backups/sets/{}", seeded.backup_set_id),
        format!(
            "/api/v1/backups/sets/{}/snapshots?limit=1",
            seeded.backup_set_id
        ),
        format!("/api/v1/backups/snapshots/{}", seeded.snapshot.id()),
        format!(
            "/api/v1/backups/snapshots/{}/nodes?parent_id={}",
            seeded.snapshot.id(),
            seeded.root_node_id
        ),
        format!(
            "/api/v1/backups/snapshots/{}/nodes?parent_id={}",
            seeded.snapshot.id(),
            seeded.docs_node_id
        ),
        format!(
            "/api/v1/backups/sets/{}/retention-policy",
            seeded.backup_set_id
        ),
        format!(
            "/api/v1/backups/sets/{}/maintenance-runs",
            seeded.backup_set_id
        ),
        format!(
            "/api/v1/backups/maintenance-runs/{}",
            seeded.maintenance_run.id()
        ),
    ];
    for path in paths {
        let response = router(api_state.clone())
            .oneshot(authenticated_get(&path, &session, &csrf))
            .await
            .expect("backup GET must not fail");
        assert_eq!(response.status(), StatusCode::OK, "path: {path}");
        let body = json_body(response).await;
        let serialized = body.to_string();
        for forbidden in [
            "ObjectId",
            "ObjectReplicaId",
            "object_id",
            "objectId",
            "object_replica_id",
            "objectReplicaId",
            "storage_key",
            "storageKey",
            "backend_locator",
            "dedup_domain_id",
            "retention_pin_id",
            "gc_candidate_id",
            "capture_operation_id",
        ] {
            assert!(
                !serialized.contains(forbidden),
                "{forbidden} leaked at {path}"
            );
        }
    }

    let expired = router(api_state.clone())
        .oneshot(authenticated_get(
            &format!("/api/v1/backups/snapshots/{}", seeded.snapshot.id()),
            &session,
            &csrf,
        ))
        .await
        .expect("expired snapshot GET must not fail");
    assert_eq!(expired.status(), StatusCode::OK);
    assert_eq!(json_body(expired).await["data"]["state"], "EXPIRED");

    let after = counts(&audit_pool).await;
    assert_eq!(after, before, "backup GETs must not mutate durable state");

    let unauthorized = router(api_state)
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/api/v1/backups/sets/{}", seeded.backup_set_id))
                .body(Body::empty())
                .expect("unauthenticated request must be valid"),
        )
        .await
        .expect("unauthenticated backup GET must not fail");
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL 17 database"]
async fn postgres_backup_operation_observability_is_stable_scoped_and_side_effect_free() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a fresh disposable PostgreSQL database");
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
    assert_eq!(status.applied_versions().len(), 29);
    let audit_pool = PgPool::connect(&url)
        .await
        .expect("audit connection must succeed");

    let api_state = ApiState::from_current_platform()
        .with_postgres_auth(
            pool.clone(),
            PasswordHasherConfig::new(
                PasswordParameters::new(8 * 1024, 1, 1, 32)
                    .expect("password parameters must be valid"),
            )
            .expect("password config must be valid"),
            SessionConfig::default(),
            RebaselineTokenKey::from_bytes([0x55; 32]),
        )
        .with_readiness(Arc::new(StaticReadiness::new(true)))
        .with_cookie_config(CookieConfig::development());
    let (session, csrf, owner_user_id) =
        login(&api_state, &format!("prompt55-{}", uuid::Uuid::now_v7())).await;
    let seeded = seed_backup(pool.as_ref(), &audit_pool, owner_user_id).await;
    let backup = BackupService::new(pool.as_ref().clone());

    // The maintenance item is created first, observed through HTTP, and then
    // explicitly advanced. No GET is allowed to perform this transition.
    let maintenance_path = format!(
        "/api/v1/backups/operations/maintenance/{}",
        seeded.maintenance_run.id()
    );
    let initial = router(api_state.clone())
        .oneshot(authenticated_get_without_csrf(&maintenance_path, &session))
        .await
        .expect("maintenance detail GET must not fail");
    assert_eq!(initial.status(), StatusCode::OK);
    let initial_body = json_body(initial).await;
    assert_eq!(initial_body["data"]["state"], "CREATED");
    assert_eq!(
        initial_body["data"]["progress"],
        json!({
            "completed_steps": 0,
            "total_steps": 3
        })
    );
    assert_eq!(initial_body["data"]["terminal"], false);
    assert_eq!(initial_body["data"]["next_action"], "ADVANCE");
    assert_no_physical_identity(&initial_body);

    let advanced = backup
        .advance_backup_maintenance_run(owner_user_id, seeded.maintenance_run.id())
        .await
        .expect("maintenance run must advance explicitly");
    assert_eq!(advanced.state().as_str(), "COMPLETED");
    let restore_snapshot_id = advanced
        .captured_snapshot_id()
        .expect("completed maintenance must expose its captured snapshot");

    // Restore is deliberately executed before the source snapshot is expired;
    // the receipt then remains historical after the later source/prune flow.
    let restore_created = router(api_state.clone())
        .oneshot(mutation_request(
            &format!("/api/v1/backups/snapshots/{restore_snapshot_id}/restore-plans"),
            Some(json!({
                "target_library_id": seeded.target_library_id.to_string(),
                "target_parent_node_id": seeded.target_root_node_id.to_string(),
                "destination_name": "Recovered"
            })),
            &session,
            &csrf,
            &key(),
        ))
        .await
        .expect("restore plan creation must not fail");
    assert_eq!(restore_created.status(), StatusCode::CREATED);
    let restore_created_body = json_body(restore_created).await;
    let restore_plan_id = restore_created_body["data"]["restore_plan_id"]
        .as_str()
        .expect("restore plan ID must be present")
        .to_owned();
    let planned_restore = router(api_state.clone())
        .oneshot(authenticated_get(
            &format!("/api/v1/backups/operations/restore/{restore_plan_id}"),
            &session,
            &csrf,
        ))
        .await
        .expect("planned restore detail GET must not fail");
    assert_eq!(planned_restore.status(), StatusCode::OK);
    let planned_restore_body = json_body(planned_restore).await;
    assert_no_physical_identity(&planned_restore_body);
    assert_eq!(planned_restore_body["data"]["state"], "PLANNED");
    assert_eq!(
        planned_restore_body["data"]["progress"],
        json!({
            "completed_steps": 1,
            "total_steps": 2
        })
    );
    assert_eq!(planned_restore_body["data"]["next_action"], "EXECUTE");
    assert_eq!(
        planned_restore_body["data"]["operation_id"],
        restore_plan_id
    );
    let restore_execution = router(api_state.clone())
        .oneshot(mutation_request(
            &format!("/api/v1/backups/restore-plans/{restore_plan_id}/execute"),
            None,
            &session,
            &csrf,
            &key(),
        ))
        .await
        .expect("restore execution must not fail");
    assert_eq!(restore_execution.status(), StatusCode::OK);
    let restore_execution_body = json_body(restore_execution).await;
    let executed_restore = router(api_state.clone())
        .oneshot(authenticated_get(
            &format!("/api/v1/backups/operations/restore/{restore_plan_id}"),
            &session,
            &csrf,
        ))
        .await
        .expect("executed restore detail GET must not fail");
    assert_eq!(executed_restore.status(), StatusCode::OK);
    let executed_restore_body = json_body(executed_restore).await;
    assert_no_physical_identity(&executed_restore_body);
    assert_eq!(executed_restore_body["data"]["state"], "EXECUTED");
    assert_eq!(
        executed_restore_body["data"]["progress"],
        json!({
            "completed_steps": 2,
            "total_steps": 2
        })
    );
    assert_eq!(executed_restore_body["data"]["terminal"], true);
    assert_eq!(executed_restore_body["data"]["next_action"], "NONE");
    assert_eq!(
        executed_restore_body["data"]["restore_execution_id"],
        restore_execution_body["data"]["restore_execution_id"]
    );

    // Establish the accepted expired snapshot and execute a real prune plan
    // before taking the immutable read-side baseline. The later GETs must not
    // reinterpret completion based on GC or current namespace contents.
    sqlx::query(
        "UPDATE backup_snapshots
         SET state = 'EXPIRED', expired_at = CURRENT_TIMESTAMP
         WHERE id = $1",
    )
    .bind(restore_snapshot_id.into_uuid())
    .execute(&audit_pool)
    .await
    .expect("expired snapshot fixture must persist");
    let prune_created = router(api_state.clone())
        .oneshot(mutation_request(
            &format!("/api/v1/backups/snapshots/{restore_snapshot_id}/prune-plans"),
            None,
            &session,
            &csrf,
            &key(),
        ))
        .await
        .expect("prune plan creation must not fail");
    assert_eq!(prune_created.status(), StatusCode::CREATED);
    let prune_created_body = json_body(prune_created).await;
    let prune_plan_id = prune_created_body["data"]["prune_plan_id"]
        .as_str()
        .expect("prune plan ID must be present")
        .to_owned();
    let planned_prune = router(api_state.clone())
        .oneshot(authenticated_get(
            &format!("/api/v1/backups/operations/prune/{prune_plan_id}"),
            &session,
            &csrf,
        ))
        .await
        .expect("planned prune detail GET must not fail");
    assert_eq!(planned_prune.status(), StatusCode::OK);
    let planned_prune_body = json_body(planned_prune).await;
    assert_no_physical_identity(&planned_prune_body);
    assert_eq!(planned_prune_body["data"]["state"], "PLANNED");
    assert_eq!(
        planned_prune_body["data"]["progress"],
        json!({
            "completed_steps": 1,
            "total_steps": 2
        })
    );
    assert_eq!(planned_prune_body["data"]["next_action"], "EXECUTE");
    let prune_execution = router(api_state.clone())
        .oneshot(mutation_request(
            &format!("/api/v1/backups/prune-plans/{prune_plan_id}/execute"),
            Some(json!({
                "confirm_snapshot_id": restore_snapshot_id.to_string()
            })),
            &session,
            &csrf,
            &key(),
        ))
        .await
        .expect("prune execution must not fail");
    assert_eq!(prune_execution.status(), StatusCode::OK);
    let prune_execution_body = json_body(prune_execution).await;
    let executed_prune = router(api_state.clone())
        .oneshot(authenticated_get(
            &format!("/api/v1/backups/operations/prune/{prune_plan_id}"),
            &session,
            &csrf,
        ))
        .await
        .expect("executed prune detail GET must not fail");
    assert_eq!(executed_prune.status(), StatusCode::OK);
    let executed_prune_body = json_body(executed_prune).await;
    assert_no_physical_identity(&executed_prune_body);
    assert_eq!(executed_prune_body["data"]["state"], "EXECUTED");
    assert_eq!(
        executed_prune_body["data"]["progress"],
        json!({
            "completed_steps": 2,
            "total_steps": 2
        })
    );
    assert_eq!(executed_prune_body["data"]["terminal"], true);
    assert_eq!(executed_prune_body["data"]["next_action"], "NONE");
    assert_eq!(
        executed_prune_body["data"]["prune_execution_id"],
        prune_execution_body["data"]["prune_execution_id"]
    );

    // The lifecycle tables intentionally protect created_at from normal
    // mutation. For this disposable equal-timestamp ordering fixture, disable
    // only the immutable-row triggers inside one setup transaction, write one
    // common durable timestamp, and restore the protections before the read.
    let restore_plan_uuid = Uuid::parse_str(&restore_plan_id).expect("restore plan ID is UUID");
    let prune_plan_uuid = Uuid::parse_str(&prune_plan_id).expect("prune plan ID is UUID");
    let equal_timestamp_targets = [
        (
            "backup_maintenance_runs",
            "backup_maintenance_runs_immutable",
            seeded.maintenance_run.id().into_uuid(),
        ),
        (
            "backup_restore_plans",
            "backup_restore_plans_immutable",
            restore_plan_uuid,
        ),
        (
            "backup_prune_plans",
            "backup_prune_plans_immutable",
            prune_plan_uuid,
        ),
    ];
    for (table, trigger, _) in &equal_timestamp_targets {
        sqlx::query(&format!("ALTER TABLE {table} DISABLE TRIGGER {trigger}"))
            .execute(&audit_pool)
            .await
            .expect("fixture trigger must be disabled");
    }
    let mut equal_timestamp_setup = audit_pool
        .begin()
        .await
        .expect("equal-timestamp setup transaction must begin");
    let equal_timestamp: String =
        sqlx::query_scalar("SELECT (clock_timestamp() - INTERVAL '1 second')::TEXT")
            .fetch_one(&mut *equal_timestamp_setup)
            .await
            .expect("equal timestamp must be available");
    for (table, _, id) in &equal_timestamp_targets {
        sqlx::query(&format!(
            "UPDATE {table} SET created_at = $1::TIMESTAMPTZ WHERE id = $2"
        ))
        .bind(&equal_timestamp)
        .bind(*id)
        .execute(&mut *equal_timestamp_setup)
        .await
        .expect("fixture created_at must be aligned");
    }
    equal_timestamp_setup
        .commit()
        .await
        .expect("equal-timestamp setup must commit");
    for (table, trigger, _) in &equal_timestamp_targets {
        sqlx::query(&format!("ALTER TABLE {table} ENABLE TRIGGER {trigger}"))
            .execute(&audit_pool)
            .await
            .expect("fixture trigger must be restored");
    }

    let other_set = backup
        .create_backup_set(
            owner_user_id,
            BackupSetId::new(),
            LogicalName::new("prompt55-other-set").expect("other set name must be valid"),
            seeded.target_library_id,
            None,
            Timestamp::parse("2026-08-31T00:00:01Z").expect("fixture timestamp must be valid"),
        )
        .await
        .expect("second backup set must persist");

    let before = counts(&audit_pool).await;
    let first_page = router(api_state.clone())
        .oneshot(authenticated_get(
            &format!(
                "/api/v1/backups/sets/{}/operations?limit=2",
                seeded.backup_set_id
            ),
            &session,
            &csrf,
        ))
        .await
        .expect("operation list GET must not fail");
    assert_eq!(first_page.status(), StatusCode::OK);
    assert_eq!(first_page.headers()["cache-control"], "private, no-store");
    let first_page = json_body(first_page).await;
    assert_eq!(first_page["data"].as_array().unwrap().len(), 2);
    assert_eq!(first_page["page"]["has_more"], true);
    assert!(
        first_page["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|value| { value.get("progress_percent").is_none() && value.get("eta").is_none() })
    );
    let first_cursor = first_page["page"]["next_cursor"]
        .as_str()
        .expect("operation list must return a cursor")
        .to_owned();
    let second_page = router(api_state.clone())
        .oneshot(authenticated_get(
            &format!(
                "/api/v1/backups/sets/{}/operations?limit=2&cursor={first_cursor}",
                seeded.backup_set_id
            ),
            &session,
            &csrf,
        ))
        .await
        .expect("second operation list GET must not fail");
    assert_eq!(second_page.status(), StatusCode::OK);
    let second_page = json_body(second_page).await;
    assert_eq!(second_page["data"].as_array().unwrap().len(), 1);
    assert_eq!(second_page["page"]["has_more"], false);
    let all_ids: Vec<_> = first_page["data"]
        .as_array()
        .unwrap()
        .iter()
        .chain(second_page["data"].as_array().unwrap())
        .map(|value| {
            (
                value["operation_kind"].as_str().unwrap().to_owned(),
                value["operation_id"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(all_ids.len(), 3);
    assert_eq!(
        all_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        all_ids.len()
    );
    assert_eq!(
        all_ids
            .iter()
            .map(|(kind, _)| kind.as_str())
            .collect::<std::collections::HashSet<_>>(),
        ["MAINTENANCE", "RESTORE", "PRUNE"]
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
    );
    assert_eq!(
        all_ids
            .iter()
            .map(|(kind, _)| kind.as_str())
            .collect::<Vec<_>>(),
        vec!["MAINTENANCE", "RESTORE", "PRUNE"]
    );
    assert_no_physical_identity(&first_page);
    assert_no_physical_identity(&second_page);

    for kind in ["MAINTENANCE", "RESTORE", "PRUNE"] {
        let response = router(api_state.clone())
            .oneshot(authenticated_get(
                &format!(
                    "/api/v1/backups/sets/{}/operations?kind={kind}",
                    seeded.backup_set_id
                ),
                &session,
                &csrf,
            ))
            .await
            .expect("kind-filter GET must not fail");
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert!(
            body["data"]
                .as_array()
                .unwrap()
                .iter()
                .all(|value| { value["operation_kind"] == kind })
        );
    }
    let invalid_kind = router(api_state.clone())
        .oneshot(authenticated_get(
            &format!(
                "/api/v1/backups/sets/{}/operations?kind=GC",
                seeded.backup_set_id
            ),
            &session,
            &csrf,
        ))
        .await
        .expect("invalid-kind GET must not fail");
    assert_eq!(invalid_kind.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(invalid_kind).await["error"]["code"],
        "invalid_request"
    );

    let cross_scope = router(api_state.clone())
        .oneshot(authenticated_get(
            &format!(
                "/api/v1/backups/sets/{}/operations?cursor={first_cursor}",
                other_set.id()
            ),
            &session,
            &csrf,
        ))
        .await
        .expect("cross-scope cursor GET must not fail");
    assert_eq!(cross_scope.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(cross_scope).await["error"]["code"],
        "invalid_cursor"
    );

    let wrong_kind = router(api_state.clone())
        .oneshot(authenticated_get(
            &format!(
                "/api/v1/backups/operations/prune/{}",
                seeded.maintenance_run.id()
            ),
            &session,
            &csrf,
        ))
        .await
        .expect("wrong-kind GET must not fail");
    assert_eq!(wrong_kind.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_body(wrong_kind).await["error"]["code"], "not_found");

    // A repeated poll of both collection and detail resources has no durable
    // write effect and returns the same data when no mutation is issued.
    for (path, expected_data) in [
        (
            format!(
                "/api/v1/backups/sets/{}/operations?limit=2",
                seeded.backup_set_id
            ),
            first_page["data"].clone(),
        ),
        (maintenance_path.clone(), initial_body["data"].clone()),
    ] {
        for _ in 0..100 {
            let response = router(api_state.clone())
                .oneshot(authenticated_get(&path, &session, &csrf))
                .await
                .expect("repeated observation GET must not fail");
            assert_eq!(response.status(), StatusCode::OK);
            let body = json_body(response).await;
            if path.contains("/operations/maintenance/") {
                // The operation was explicitly advanced during setup, so the
                // canonical expected state is the post-advance representation.
                assert_eq!(body["data"]["state"], "COMPLETED");
            } else {
                assert_eq!(body["data"], expected_data);
            }
        }
    }
    let after = counts(&audit_pool).await;
    assert_eq!(
        after, before,
        "operation GETs must not mutate durable state"
    );

    let unauthorized = router(api_state)
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!(
                    "/api/v1/backups/sets/{}/operations",
                    seeded.backup_set_id
                ))
                .body(Body::empty())
                .expect("unauthenticated request must be valid"),
        )
        .await
        .expect("unauthenticated operation GET must not fail");
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
}
