//! Real router + PostgreSQL + production HTTP transport + durable desktop.
//! All secrets are freshly generated or isolated test values; no internet or
//! production database/managed root/native credential store is used here.

use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use synveil_api::{ApiState, CookieConfig, RebaselineTokenKey, StaticReadiness, router};
use synveil_auth::{PasswordHasherConfig, PasswordParameters, SessionConfig};
use synveil_client_sync::{
    CanonicalBaseUrl, ClientSyncError, ConnectionHealth, EngineConfig, FailureInjector,
    FailurePoint, FilesystemLocalReplica, HttpClientConfig, HttpEnrollmentClient, HttpSyncRemote,
    InboundSyncEngine, LocalStateConfig, LocalStateStore, RemoteErrorKind, ReplicaScope,
    ServerProfile, SyncOutcome, SyncRemote,
};
use synveil_core::{
    DedupDomainId, Device, DeviceId, EnrollmentSecret, Library, LibraryId, LogicalName,
    LoginIdentifier, Node, NodeId, OutboundIntentId, Sha256Digest, SyncConflictId, Timestamp, User,
    UserId, UserStatus,
};
use synveil_metadata::{
    DatabaseConfig, DatabasePool, DomainRepository, MigrationRunner, PostgresContentReadRepository,
    PostgresUploadRepository, UploadCompletion,
};
use synveil_platform::{SecretName, SecretStore, SecretStoreError, SecretStoreState, SecretValue};
use synveil_storage::{
    ContentReadApplicationService, CreateUploadSessionRequest, ObjectStore,
    UploadApplicationService, UploadLimits, UploadTargetRequest, open_local_object_store,
};

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("synveil-p37-http-e2e-{}", uuid::Uuid::now_v7()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct TestSecretStore(Mutex<BTreeMap<SecretName, SecretValue>>);

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

struct FailBeforeAck(AtomicBool);

impl FailureInjector for FailBeforeAck {
    fn check(&self, point: FailurePoint) -> Result<(), ClientSyncError> {
        if point == FailurePoint::AfterLocalCommitBeforeAck && !self.0.swap(true, Ordering::SeqCst)
        {
            return Err(ClientSyncError::InjectedFailure);
        }
        Ok(())
    }
}

struct ServerTask(tokio::task::JoinHandle<()>);
impl Drop for ServerTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
impl ServerTask {
    async fn stop(&mut self) {
        self.0.abort();
        let _ = (&mut self.0).await;
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
        let body = json!({"login":"desktop-test-owner", "login_key":"desktop-test-owner", "password":"isolated-test-password-not-a-production-secret"});
        let setup = client
            .post(format!("{origin}/api/v1/bootstrap/admin"))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(setup.status(), StatusCode::OK);
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
        let owner = body["data"]["user_id"].as_str().unwrap().parse().unwrap();
        Self {
            client,
            origin,
            cookie,
            csrf,
            owner,
        }
    }

    fn post(&self, path: &str, body: Value) -> reqwest::RequestBuilder {
        self.client
            .post(format!("{}{path}", self.origin))
            .header("cookie", &self.cookie)
            .header("origin", &self.origin)
            .header("x-csrf-token", &self.csrf)
            .json(&body)
    }

    async fn grant(&self, device: Option<DeviceId>) -> (DeviceId, EnrollmentSecret) {
        let target = match device {
            Some(device) => json!({"kind":"existing","device_id":device.to_string()}),
            None => json!({"kind":"new","display_name":"Isolated desktop"}),
        };
        let response = self
            .post(
                "/api/v1/devices/enrollment-grants",
                json!({"target":target}),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["data"]["owner_user_id"], self.owner.to_string());
        (
            body["data"]["device_id"].as_str().unwrap().parse().unwrap(),
            EnrollmentSecret::parse(body["data"]["enrollment_token"].as_str().unwrap()).unwrap(),
        )
    }
}

async fn upload(
    service: &UploadApplicationService,
    owner: UserId,
    target: UploadTargetRequest,
    bytes: &[u8],
) -> UploadCompletion {
    let session = service
        .create_upload_session(CreateUploadSessionRequest {
            idempotency_key: OutboundIntentId::new(),
            owner_user_id: owner,
            target,
            expected_length: bytes.len() as u64,
            expected_sha256: Some(Sha256Digest::from_bytes(Sha256::digest(bytes).into())),
        })
        .await
        .unwrap();
    for (index, chunk) in bytes.chunks(32 * 1024).enumerate() {
        service
            .append_upload_chunk(
                owner,
                session.id,
                (index * 32 * 1024) as u64,
                Bytes::copy_from_slice(chunk),
            )
            .await
            .unwrap();
    }
    service.complete_upload(owner, session.id).await.unwrap()
}

async fn converge(engine: &InboundSyncEngine) {
    for _ in 0..64 {
        match engine.synchronize_once().await.unwrap() {
            SyncOutcome::Idle => return,
            SyncOutcome::Progressed
            | SyncOutcome::MoreAvailable
            | SyncOutcome::BootstrapRequired => {}
            outcome => panic!("unexpected bounded sync outcome: {outcome:?}"),
        }
    }
    panic!("bounded sync did not converge");
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn postgres_http_enrollment_bootstrap_feed_ack_download_and_revocation_preserve_local_state()
{
    let database_url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("fresh disposable PostgreSQL is required");
    let config = DatabaseConfig::from_url(database_url.clone()).unwrap();
    let audit_pool = sqlx::PgPool::connect(&database_url).await.unwrap();
    let pool = Arc::new(DatabasePool::connect(&config).await.unwrap());
    assert!(
        MigrationRunner::new()
            .run(&pool)
            .await
            .unwrap()
            .is_current()
    );
    let temp = TempRoot::new();
    let objects = temp.0.join("objects");
    fs::create_dir(&objects).unwrap();
    let object_store: Arc<dyn ObjectStore> = Arc::new(open_local_object_store(&objects).unwrap());
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
            PasswordHasherConfig::new(PasswordParameters::new(16 * 1024, 2, 1, 32).unwrap())
                .unwrap(),
            SessionConfig::default(),
            RebaselineTokenKey::from_bytes([0x61; 32]),
        )
        .with_readiness(Arc::new(StaticReadiness::new(true)))
        .with_cookie_config(CookieConfig::development())
        .with_allowed_origin(&origin)
        .with_download_backend(Arc::new(downloads))
        .with_upload_backend(Arc::new(uploads.clone()));
    let mut server = ServerTask(tokio::spawn(async move {
        axum::serve(listener, router(api_state)).await.unwrap();
    }));
    let browser_client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();
    let browser = Browser::login(browser_client, origin.clone()).await;
    let library_id = LibraryId::new();
    let fixture_time = Timestamp::parse("2026-08-28T00:00:00Z").unwrap();
    let root = Node::root(
        NodeId::new(),
        library_id,
        LogicalName::new("root").unwrap(),
        fixture_time,
    );
    let library = Library::new(
        library_id,
        browser.owner,
        LogicalName::new("Desktop test library").unwrap(),
        &root,
        DedupDomainId::new(),
        fixture_time,
    )
    .unwrap();
    DomainRepository::new(&pool)
        .insert_library_with_root(&library, &root)
        .await
        .unwrap();
    let initial_bytes = vec![0x2a; 192 * 1024 + 17];
    let initial = upload(
        &uploads,
        browser.owner,
        UploadTargetRequest::CreateFile {
            library_id,
            parent_node_id: root.id(),
            name: LogicalName::new("file.bin").unwrap(),
        },
        &initial_bytes,
    )
    .await;

    let profile = ServerProfile::new(
        CanonicalBaseUrl::parse_for_loopback_test(&origin).unwrap(),
        "Disposable test server",
    )
    .unwrap();
    let profile_id = profile.profile_id();
    let (device_id, grant) = browser.grant(None).await;
    let enrollment =
        HttpEnrollmentClient::new(profile.clone(), HttpClientConfig::default()).unwrap();
    let credential = enrollment.exchange(&grant).await.unwrap();
    assert_eq!(credential.device_id(), device_id);
    assert_eq!(credential.owner_user_id(), browser.owner);
    assert!(
        enrollment.exchange(&grant).await.is_err(),
        "consumed grant cannot mint again"
    );
    let credentials: i64 =
        sqlx::query_scalar("SELECT count(*) FROM device_credentials WHERE device_id = $1")
            .bind(device_id.into_uuid())
            .fetch_one(&audit_pool)
            .await
            .unwrap();
    assert_eq!(credentials, 1);
    let persisted: String = sqlx::query_scalar(
        "SELECT row_to_json(c)::text FROM device_credentials c WHERE credential_id = $1",
    )
    .bind(credential.credential_id().into_uuid())
    .fetch_one(&audit_pool)
    .await
    .unwrap();
    assert!(!persisted.contains(credential.secret().expose_secret()));
    let persisted_grant: String = sqlx::query_scalar(
        "SELECT row_to_json(g)::text FROM device_enrollment_grants g WHERE device_id = $1",
    )
    .bind(device_id.into_uuid())
    .fetch_one(&audit_pool)
    .await
    .unwrap();
    assert!(!persisted_grant.contains(grant.expose_secret()));

    // Fresh PostgreSQL authority checks through the real middleware and
    // application services, not only the deterministic auth test backend.
    let device_request = |method, path: &str| {
        browser
            .client
            .request(method, format!("{origin}{path}"))
            .bearer_auth(credential.secret().expose_secret())
    };
    let foreign_owner = UserId::new();
    DomainRepository::new(&pool)
        .insert_user(&User::new(
            foreign_owner,
            LoginIdentifier::new("foreign-desktop-owner", "foreign-desktop-owner").unwrap(),
            UserStatus::Active,
            fixture_time,
        ))
        .await
        .unwrap();
    let foreign_device = Device::new(
        DeviceId::new(),
        foreign_owner,
        LogicalName::new("foreign desktop").unwrap(),
        fixture_time,
    );
    DomainRepository::new(&pool)
        .insert_device(&foreign_device)
        .await
        .unwrap();
    let foreign_library_id = LibraryId::new();
    let foreign_root = Node::root(
        NodeId::new(),
        foreign_library_id,
        LogicalName::new("root").unwrap(),
        fixture_time,
    );
    let foreign_library = Library::new(
        foreign_library_id,
        foreign_owner,
        LogicalName::new("Foreign library").unwrap(),
        &foreign_root,
        DedupDomainId::new(),
        fixture_time,
    )
    .unwrap();
    DomainRepository::new(&pool)
        .insert_library_with_root(&foreign_library, &foreign_root)
        .await
        .unwrap();
    let foreign_file = upload(
        &uploads,
        foreign_owner,
        UploadTargetRequest::CreateFile {
            library_id: foreign_library_id,
            parent_node_id: foreign_root.id(),
            name: LogicalName::new("foreign.bin").unwrap(),
        },
        b"foreign owner content",
    )
    .await;
    let device_scope = format!("/api/v1/devices/{device_id}/libraries/{library_id}");
    for path in [
        format!(
            "/api/v1/devices/{}/libraries/{library_id}/checkpoint",
            foreign_device.id()
        ),
        format!("/api/v1/devices/{device_id}/libraries/{foreign_library_id}/checkpoint"),
        format!("/api/v1/nodes/{}", foreign_file.node_id),
        format!("/api/v1/nodes/{}/content", foreign_file.node_id),
        format!("/api/v1/versions/{}", foreign_file.file_version_id),
        format!("/api/v1/versions/{}/content", foreign_file.file_version_id),
    ] {
        let response = device_request(reqwest::Method::GET, &path)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        assert_eq!(
            response.json::<Value>().await.unwrap()["error"]["code"],
            "not_found"
        );
    }
    for (method, path) in [
        (reqwest::Method::GET, "/api/v1/system/health".to_owned()),
        (reqwest::Method::GET, "/api/v1/auth/session".to_owned()),
        (reqwest::Method::GET, "/api/v1/libraries".to_owned()),
        (
            reqwest::Method::POST,
            "/api/v1/devices/enrollment-grants".to_owned(),
        ),
        (
            reqwest::Method::POST,
            format!("/api/v1/devices/{device_id}/credentials/revoke-all"),
        ),
        (reqwest::Method::GET, format!("{device_scope}/conflicts")),
        (
            reqwest::Method::POST,
            format!("{device_scope}/conflicts/{}/resolve", SyncConflictId::new()),
        ),
    ] {
        assert_eq!(
            device_request(method, &path)
                .json(&json!({}))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }

    let device_mutation =
        device_request(reqwest::Method::POST, &format!("{device_scope}/mutations"))
            .json(&json!({
                "mutation_id": synveil_core::ClientMutationId::new().to_string(),
                "base_epoch": "1",
                "base_sequence": "0",
                "kind": "TRASH_NODE",
                "payload": {"node_id": initial.node_id.to_string(), "expected_revision": "999"}
            }))
            .send()
            .await
            .unwrap();
    assert_ne!(device_mutation.status(), StatusCode::UNAUTHORIZED);
    assert_ne!(device_mutation.status(), StatusCode::FORBIDDEN);

    let device_upload = device_request(reqwest::Method::POST, "/api/v1/upload-sessions")
        .json(&json!({
            "operation":"REPLACE_CONTENT",
            "idempotency_key": OutboundIntentId::new().to_string(),
            "library_id": library_id.to_string(),
            "node_id": initial.node_id.to_string(),
            "expected_revision": initial.node_revision.to_string(),
            "expected_bytes":"0",
            "expected_sha256": synveil_core::Sha256Digest::from_bytes([0_u8; 32]).to_string()
        }))
        .send()
        .await
        .unwrap();
    assert_ne!(device_upload.status(), StatusCode::UNAUTHORIZED);
    assert_ne!(device_upload.status(), StatusCode::FORBIDDEN);

    let foreign_grant = browser
        .post(
            "/api/v1/devices/enrollment-grants",
            json!({"target":{
                "kind":"existing", "device_id":foreign_device.id().to_string(),
            }}),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(foreign_grant.status(), StatusCode::NOT_FOUND);
    let no_csrf = browser
        .client
        .post(format!("{origin}{device_scope}/changes/ack"))
        .header("cookie", &browser.cookie)
        .header("origin", &origin)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(no_csrf.status(), StatusCode::FORBIDDEN);
    let invalid_mixed = browser
        .client
        .post(format!("{origin}{device_scope}/changes/ack"))
        .header("cookie", &browser.cookie)
        .header("authorization", "Bearer invalid")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid_mixed.status(), StatusCode::UNAUTHORIZED);

    let secrets = TestSecretStore::default();
    let local_config = LocalStateConfig::new(temp.0.join("state.sqlite3"));
    let local = LocalStateStore::open(&local_config).await.unwrap();
    local.save_server_profile(&profile).await.unwrap();
    local.store_enrollment(&credential, &secrets).await.unwrap();
    assert_eq!(secrets.0.lock().unwrap().len(), 1);
    drop(local);
    let local = Arc::new(LocalStateStore::open(&local_config).await.unwrap());
    let reopened_profile = local.server_profile(profile_id).await.unwrap().unwrap();
    assert_eq!(reopened_profile, profile);
    let loaded = local
        .load_device_credential(profile_id, &secrets)
        .await
        .unwrap()
        .unwrap();
    assert!(!format!("{loaded:?}").contains(credential.secret().expose_secret()));
    let remote = Arc::new(
        HttpSyncRemote::new(
            reopened_profile,
            device_id,
            loaded,
            HttpClientConfig::default(),
        )
        .unwrap(),
    );
    let scope = ReplicaScope::new(browser.owner, device_id, library_id);
    assert_eq!(remote.probe_health(scope).await, ConnectionHealth::Online);
    let managed = temp.0.join("managed");
    fs::create_dir(&managed).unwrap();
    let replica = Arc::new(
        FilesystemLocalReplica::initialize_for_profile(&managed, scope, profile_id).unwrap(),
    );
    let engine = InboundSyncEngine::new(
        scope,
        remote.clone(),
        replica.clone(),
        local.clone(),
        EngineConfig::new(2, 1).unwrap(),
    )
    .await
    .unwrap();
    converge(&engine).await;
    assert_eq!(fs::read(managed.join("file.bin")).unwrap(), initial_bytes);
    assert!(local.bootstrap(library_id).await.unwrap().is_none());

    // Real browser-originated mutation feeds the desktop. The desktop itself
    // never produces outbound mutations and never sends a browser cookie.
    let created = browser
        .post(
            &format!("/api/v1/libraries/{library_id}/nodes"),
            json!({"name":"from-server", "parent_id":root.id().to_string()}),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    converge(&engine).await;
    assert!(managed.join("from-server").is_dir());
    let checkpoint = remote.get_checkpoint(scope).await.unwrap();
    let record = local.replica(library_id).await.unwrap().unwrap();
    assert_eq!(
        record.applied_sequence(),
        checkpoint.acknowledged_sequence()
    );
    assert_eq!(
        record.acknowledged_sequence(),
        checkpoint.acknowledged_sequence()
    );

    let mut stream = remote
        .download_current_content(scope, initial.node_id, initial.file_version_id)
        .await
        .unwrap()
        .into_stream();
    let mut actual = Vec::new();
    while let Some(bytes) = stream.next().await {
        actual.extend_from_slice(&bytes.unwrap());
    }
    assert_eq!(actual, initial_bytes);

    // Crash after visible+SQLite apply, but before ack. Then revoke. Neither
    // authentication failure nor an offline server may erase this pending work.
    let replacement_bytes = vec![0x53; 128 * 1024 + 9];
    let replacement = upload(
        &uploads,
        browser.owner,
        UploadTargetRequest::ReplaceContent {
            library_id,
            node_id: initial.node_id,
            expected_revision: initial.node_revision,
        },
        &replacement_bytes,
    )
    .await;
    let failing_engine = InboundSyncEngine::with_failure_injector(
        scope,
        remote.clone(),
        replica.clone(),
        local.clone(),
        EngineConfig::default(),
        Arc::new(FailBeforeAck(AtomicBool::new(false))),
    )
    .await
    .unwrap();
    assert!(matches!(
        failing_engine.synchronize_once().await,
        Err(ClientSyncError::InjectedFailure)
    ));
    assert_eq!(
        fs::read(managed.join("file.bin")).unwrap(),
        replacement_bytes
    );
    let before = local.replica(library_id).await.unwrap().unwrap();
    // This crash point follows event commits but precedes the durable page-to-
    // ACK_PENDING transition. Recovery must complete that transition locally,
    // then retain its evidence when the remote rejects the revoked credential.
    assert!(local.pending_ack(library_id).await.unwrap().is_none());
    let revoke = browser
        .post(
            &format!(
                "/api/v1/devices/{device_id}/credentials/{}/revoke",
                credential.credential_id()
            ),
            json!({}),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(revoke.status(), StatusCode::NO_CONTENT);
    for (method, path) in [
        (reqwest::Method::GET, format!("{device_scope}/checkpoint")),
        (reqwest::Method::GET, format!("{device_scope}/changes")),
        (reqwest::Method::POST, format!("{device_scope}/changes/ack")),
        (reqwest::Method::POST, format!("{device_scope}/rebaseline")),
        (
            reqwest::Method::GET,
            format!(
                "{device_scope}/rebaseline/{}/nodes",
                synveil_core::SyncBootstrapId::new()
            ),
        ),
        (
            reqwest::Method::POST,
            format!(
                "{device_scope}/rebaseline/{}/complete",
                synveil_core::SyncBootstrapId::new()
            ),
        ),
        (
            reqwest::Method::GET,
            format!("/api/v1/nodes/{}/content", initial.node_id),
        ),
        (
            reqwest::Method::GET,
            format!("/api/v1/versions/{}/content", initial.file_version_id),
        ),
    ] {
        let response = device_request(method, &path)
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.json::<Value>().await.unwrap()["error"]["code"],
            "device_revoked"
        );
    }
    assert_eq!(
        remote.probe_health(scope).await,
        ConnectionHealth::DeviceRevoked
    );
    match engine.synchronize_once().await {
        Err(ClientSyncError::Remote(error)) => {
            assert_eq!(error.kind(), RemoteErrorKind::DeviceRevoked)
        }
        _ => panic!("revoked credential must stop before ack"),
    }
    let after = local.replica(library_id).await.unwrap().unwrap();
    assert_eq!(after.applied_sequence(), before.applied_sequence());
    assert_eq!(
        after.acknowledged_sequence(),
        before.acknowledged_sequence()
    );
    let pending = local.pending_ack(library_id).await.unwrap().unwrap();
    assert_eq!(pending.through_sequence(), before.applied_sequence());
    match engine.synchronize_once().await {
        Err(ClientSyncError::Remote(error)) => {
            assert_eq!(error.kind(), RemoteErrorKind::DeviceRevoked)
        }
        _ => panic!("repeated revoked authentication must retain pending acknowledgement"),
    }
    assert_eq!(
        local.pending_ack(library_id).await.unwrap().unwrap(),
        pending
    );
    assert_eq!(
        fs::read(managed.join("file.bin")).unwrap(),
        replacement_bytes
    );

    let (_, fresh_grant) = browser.grant(Some(device_id)).await;
    let fresh = enrollment.exchange(&fresh_grant).await.unwrap();
    local.replace_enrollment(&fresh, &secrets).await.unwrap();
    let loaded = local
        .load_device_credential(profile_id, &secrets)
        .await
        .unwrap()
        .unwrap();
    let fresh_remote = Arc::new(
        HttpSyncRemote::new(
            profile.clone(),
            device_id,
            loaded,
            HttpClientConfig::default(),
        )
        .unwrap(),
    );
    let fresh_engine = InboundSyncEngine::new(
        scope,
        fresh_remote.clone(),
        replica.clone(),
        local.clone(),
        EngineConfig::default(),
    )
    .await
    .unwrap();
    converge(&fresh_engine).await;
    assert!(local.pending_ack(library_id).await.unwrap().is_none());
    assert_eq!(
        fresh_remote
            .get_checkpoint(scope)
            .await
            .unwrap()
            .acknowledged_sequence(),
        before.applied_sequence()
    );
    assert_eq!(
        fs::read(managed.join("file.bin")).unwrap(),
        replacement_bytes
    );
    assert_eq!(
        secrets.0.lock().unwrap().len(),
        1,
        "replaced secret is deleted"
    );

    // Deliberately stop only our disposable listener. A new transport cannot
    // reuse already accepted keep-alive connections from the stopped listener.
    // Offline state must preserve all logical progress and the visible file.
    server.stop().await;
    let loaded = local
        .load_device_credential(profile_id, &secrets)
        .await
        .unwrap()
        .unwrap();
    let offline_remote = Arc::new(
        HttpSyncRemote::new(
            profile.clone(),
            device_id,
            loaded,
            HttpClientConfig::default(),
        )
        .unwrap(),
    );
    let offline_engine = InboundSyncEngine::new(
        scope,
        offline_remote,
        replica,
        local.clone(),
        EngineConfig::default(),
    )
    .await
    .unwrap();
    let before_offline = local.replica(library_id).await.unwrap().unwrap();
    assert_eq!(
        offline_engine.synchronize_once().await.unwrap(),
        SyncOutcome::Offline
    );
    let after_offline = local.replica(library_id).await.unwrap().unwrap();
    assert_eq!(
        before_offline.applied_sequence(),
        after_offline.applied_sequence()
    );
    assert_eq!(
        before_offline.acknowledged_sequence(),
        after_offline.acknowledged_sequence()
    );
    assert_eq!(
        fs::read(managed.join("file.bin")).unwrap(),
        replacement_bytes
    );

    local
        .forget_device_credential(profile_id, &secrets)
        .await
        .unwrap();
    assert!(secrets.0.lock().unwrap().is_empty());
    assert!(
        local
            .load_device_credential(profile_id, &secrets)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fs::read(managed.join("file.bin")).unwrap(),
        replacement_bytes
    );
    for entry in fs::read_dir(&temp.0).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            let bytes = fs::read(path).unwrap();
            for secret in [
                credential.secret().expose_secret(),
                fresh.secret().expose_secret(),
                grant.expose_secret(),
                fresh_grant.expose_secret(),
            ] {
                assert!(
                    !bytes
                        .windows(secret.len())
                        .any(|window| window == secret.as_bytes()),
                    "SQLite/sidecars must never contain credentials or grant tokens"
                );
            }
        }
    }
    assert_eq!(replacement.length, replacement_bytes.len() as u64);
    drop(offline_engine);
    drop(fresh_engine);
    drop(failing_engine);
    drop(engine);
    drop(local);
    drop(server);
    audit_pool.close().await;
}

#[tokio::test]
#[ignore = "set SYNVEIL_TEST_DATABASE_URL to a fresh disposable PostgreSQL database"]
async fn outbound_rename_and_content_round_trip_converges_without_watcher_bounce() {
    let url = std::env::var("SYNVEIL_TEST_DATABASE_URL")
        .expect("SYNVEIL_TEST_DATABASE_URL must identify a disposable test database");
    let config = DatabaseConfig::from_url(&url).expect("valid disposable database URL");
    let audit_pool = sqlx::PgPool::connect(&url)
        .await
        .expect("connect postgres audit pool");
    let pool = Arc::new(
        DatabasePool::connect(&config)
            .await
            .expect("connect postgres"),
    );
    MigrationRunner::new()
        .run(&pool)
        .await
        .expect("run migrations");
    let temp = TempRoot::new();
    let object_store: Arc<dyn ObjectStore> =
        Arc::new(open_local_object_store(temp.0.join("objects")).unwrap());
    let uploads = UploadApplicationService::new(
        Arc::new(PostgresUploadRepository::new(pool.as_ref().clone())),
        object_store.clone(),
        UploadLimits {
            max_chunk_size: 16 * 1024,
            ..UploadLimits::default()
        },
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
            PasswordHasherConfig::new(PasswordParameters::new(16 * 1024, 2, 1, 32).unwrap())
                .unwrap(),
            SessionConfig::default(),
            RebaselineTokenKey::from_bytes([0x62; 32]),
        )
        .with_readiness(Arc::new(StaticReadiness::new(true)))
        .with_cookie_config(CookieConfig::development())
        .with_allowed_origin(&origin)
        .with_download_backend(Arc::new(downloads))
        .with_upload_backend(Arc::new(uploads.clone()));
    let server = ServerTask(tokio::spawn(async move {
        axum::serve(listener, router(api_state)).await.unwrap();
    }));
    let browser_client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();
    let browser = Browser::login(browser_client, origin.clone()).await;
    let library_id = LibraryId::new();
    let fixture_time = Timestamp::parse("2026-08-28T00:00:00Z").unwrap();
    let root = Node::root(
        NodeId::new(),
        library_id,
        LogicalName::new("root").unwrap(),
        fixture_time,
    );
    let library = Library::new(
        library_id,
        browser.owner,
        LogicalName::new("Outbound roundtrip").unwrap(),
        &root,
        DedupDomainId::new(),
        fixture_time,
    )
    .unwrap();
    DomainRepository::new(&pool)
        .insert_library_with_root(&library, &root)
        .await
        .unwrap();
    let initial_bytes = b"server version one".to_vec();
    let initial = upload(
        &uploads,
        browser.owner,
        UploadTargetRequest::CreateFile {
            library_id,
            parent_node_id: root.id(),
            name: LogicalName::new("node-a.txt").unwrap(),
        },
        &initial_bytes,
    )
    .await;

    let profile = ServerProfile::new(
        CanonicalBaseUrl::parse_for_loopback_test(&origin).unwrap(),
        "Outbound server",
    )
    .unwrap();
    let profile_id = profile.profile_id();
    let secrets_a = TestSecretStore::default();
    let secrets_b = TestSecretStore::default();
    let enrollment =
        HttpEnrollmentClient::new(profile.clone(), HttpClientConfig::default()).unwrap();
    let (device_a, grant_a) = browser.grant(None).await;
    let (device_b, grant_b) = browser.grant(None).await;
    let credential_a = enrollment.exchange(&grant_a).await.unwrap();
    let credential_b = enrollment.exchange(&grant_b).await.unwrap();
    let state_a = Arc::new(
        LocalStateStore::open(&LocalStateConfig::new(temp.0.join("state-a.sqlite3")))
            .await
            .unwrap(),
    );
    let state_b = Arc::new(
        LocalStateStore::open(&LocalStateConfig::new(temp.0.join("state-b.sqlite3")))
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
    let loaded_a = state_a
        .load_device_credential(profile_id, &secrets_a)
        .await
        .unwrap()
        .unwrap();
    let loaded_b = state_b
        .load_device_credential(profile_id, &secrets_b)
        .await
        .unwrap()
        .unwrap();
    let remote_a = Arc::new(
        HttpSyncRemote::new(
            profile.clone(),
            device_a,
            loaded_a,
            HttpClientConfig::default(),
        )
        .unwrap(),
    );
    let remote_b = Arc::new(
        HttpSyncRemote::new(
            profile.clone(),
            device_b,
            loaded_b,
            HttpClientConfig::default(),
        )
        .unwrap(),
    );
    let scope_a = ReplicaScope::new(browser.owner, device_a, library_id);
    let scope_b = ReplicaScope::new(browser.owner, device_b, library_id);
    let root_a = temp.0.join("managed-a");
    let root_b = temp.0.join("managed-b");
    fs::create_dir(&root_a).unwrap();
    fs::create_dir(&root_b).unwrap();
    let replica_a = Arc::new(
        FilesystemLocalReplica::initialize_for_profile(&root_a, scope_a, profile_id).unwrap(),
    );
    let replica_b = Arc::new(
        FilesystemLocalReplica::initialize_for_profile(&root_b, scope_b, profile_id).unwrap(),
    );
    let local_a_pool =
        sqlx::SqlitePool::connect(&format!("sqlite://{}", state_a.database_path().display()))
            .await
            .unwrap();
    let inbound_a = InboundSyncEngine::new(
        scope_a,
        remote_a.clone(),
        replica_a.clone(),
        state_a.clone(),
        EngineConfig::new(1, 1).unwrap(),
    )
    .await
    .unwrap();
    let inbound_b = InboundSyncEngine::new(
        scope_b,
        remote_b.clone(),
        replica_b.clone(),
        state_b.clone(),
        EngineConfig::new(1, 1).unwrap(),
    )
    .await
    .unwrap();
    converge(&inbound_a).await;
    converge(&inbound_b).await;
    assert_eq!(fs::read(root_a.join("node-a.txt")).unwrap(), initial_bytes);
    assert_eq!(fs::read(root_b.join("node-a.txt")).unwrap(), initial_bytes);

    let (watcher_a, source_a) = synveil_client_sync::ManualChangeWatcher::with_capacity(32);
    let observer_a = synveil_client_sync::OutboundObservationEngine::new(
        scope_a,
        replica_a.clone(),
        state_a.clone(),
        Box::new(watcher_a),
        synveil_client_sync::ObservationConfig::new(32, 32, 8, Duration::ZERO).unwrap(),
    )
    .await
    .unwrap();
    observer_a.start().await.unwrap();
    fs::rename(root_a.join("node-a.txt"), root_a.join("renamed-a.txt")).unwrap();
    source_a
        .push(
            synveil_client_sync::WatchHint::new(
                synveil_client_sync::WatchHintKind::Rename,
                vec![
                    synveil_client_sync::ManagedRelativePath::new("node-a.txt").unwrap(),
                    synveil_client_sync::ManagedRelativePath::new("renamed-a.txt").unwrap(),
                ],
            )
            .unwrap(),
        )
        .unwrap();
    observer_a.poll_once().await.unwrap();
    let rename_intents = observer_a.list_pending_intents().await.unwrap();
    assert_eq!(rename_intents.len(), 1);
    assert_eq!(
        rename_intents[0].kind(),
        synveil_client_sync::OutboundIntentKind::RenameNode
    );

    let submit_a = synveil_client_sync::OutboundSubmissionEngine::new(
        scope_a,
        remote_a.clone(),
        replica_a.clone(),
        state_a.clone(),
    )
    .await
    .unwrap();
    let before_submit_ack = remote_a
        .get_checkpoint(scope_a)
        .await
        .unwrap()
        .acknowledged_sequence();
    assert_eq!(
        submit_a.process_next_ready_intent().await.unwrap(),
        synveil_client_sync::OutboundSubmissionOutcome::Submitted(rename_intents[0].intent_id())
    );
    let rename_row: (i64, i64, i64) = sqlx::query_as(
        "SELECT
            (SELECT COUNT(*) FROM device_mutation_operations WHERE client_mutation_id = $1),
            (SELECT COUNT(*) FROM change_journal WHERE library_id = $2 AND change_kind = 'NODE_RENAMED'),
            (SELECT acknowledged_sequence FROM device_sync_checkpoints WHERE owner_user_id = $3 AND device_id = $4 AND library_id = $2)"
    )
    .bind(state_a.durable_mutation_request(rename_intents[0].intent_id()).await.unwrap().unwrap().mutation_id().into_uuid())
    .bind(library_id.into_uuid())
    .bind(browser.owner.into_uuid())
    .bind(device_a.into_uuid())
    .fetch_one(&audit_pool)
    .await
    .unwrap();
    assert_eq!(rename_row.0, 1);
    assert_eq!(rename_row.1, 1);
    assert_eq!(
        synveil_core::Sequence::new(u64::try_from(rename_row.2).unwrap()),
        before_submit_ack
    );
    assert_eq!(
        state_a
            .replica(library_id)
            .await
            .unwrap()
            .unwrap()
            .applied_sequence(),
        synveil_core::Sequence::new(1)
    );
    assert_eq!(
        state_a
            .outbound_intent(rename_intents[0].intent_id())
            .await
            .unwrap()
            .unwrap()
            .state(),
        synveil_client_sync::OutboundIntentState::ServerApplied
    );
    // A self-generated rename already changed the local filesystem, so
    // apply only the feed metadata before observer suppression checks.
    let content_sync_outcome = inbound_a.synchronize_once().await.unwrap();
    if content_sync_outcome != SyncOutcome::Progressed {
        let issue: Option<(String, Option<i64>, String)> = sqlx::query_as(
            "SELECT issue_kind, server_sequence, expected_state FROM local_apply_issues LIMIT 1",
        )
        .fetch_optional(&local_a_pool)
        .await
        .unwrap();
        panic!("content self-originated sync outcome {content_sync_outcome:?}, issue {issue:?}");
    }
    observer_a.poll_once().await.unwrap();
    assert_eq!(observer_a.count_pending_intents().await.unwrap(), 0);
    assert_eq!(
        state_a
            .outbound_intent(rename_intents[0].intent_id())
            .await
            .unwrap()
            .unwrap()
            .state(),
        synveil_client_sync::OutboundIntentState::Reconciled
    );
    assert_eq!(state_b.count_pending_intents(library_id).await.unwrap(), 0);
    converge(&inbound_b).await;
    assert!(root_b.join("renamed-a.txt").exists());
    assert!(!root_b.join("node-a.txt").exists());

    let replacement_bytes = b"server version two from device a".to_vec();
    fs::write(root_a.join("renamed-a.txt"), &replacement_bytes).unwrap();
    source_a
        .push(
            synveil_client_sync::WatchHint::new(
                synveil_client_sync::WatchHintKind::Modify,
                vec![synveil_client_sync::ManagedRelativePath::new("renamed-a.txt").unwrap()],
            )
            .unwrap(),
        )
        .unwrap();
    observer_a.poll_once().await.unwrap();
    let content_intents = observer_a.list_pending_intents().await.unwrap();
    assert_eq!(content_intents.len(), 1);
    assert_eq!(
        content_intents[0].kind(),
        synveil_client_sync::OutboundIntentKind::ModifyFileContent
    );
    assert_eq!(
        submit_a.process_next_ready_intent().await.unwrap(),
        synveil_client_sync::OutboundSubmissionOutcome::Submitted(content_intents[0].intent_id())
    );
    let stored_result: (Option<String>, Option<i64>, Vec<u8>, i64, i64) = sqlx::query_as(
        "SELECT file_version_id, result_length, result_sha256,
            (SELECT COUNT(*) FROM outbound_submission_results WHERE intent_id = ? AND outcome = 'SERVER_APPLIED'),
            (SELECT COUNT(*) FROM outbound_submission_results WHERE intent_id = ? AND upload_session_id IS NOT NULL)
         FROM outbound_submission_results WHERE intent_id = ?"
    )
    .bind(content_intents[0].intent_id().to_string())
    .bind(content_intents[0].intent_id().to_string())
    .bind(content_intents[0].intent_id().to_string())
    .fetch_one(&local_a_pool)
    .await
    .unwrap();
    assert!(stored_result.0.is_some());
    assert_eq!(stored_result.1, Some(replacement_bytes.len() as i64));
    assert_eq!(
        stored_result.2,
        Sha256::digest(&replacement_bytes).as_slice()
    );
    assert_eq!(stored_result.3, 1);
    assert_eq!(stored_result.4, 1);
    let content_row: (i64, i64, i64) = sqlx::query_as(
        "SELECT
            (SELECT COUNT(*) FROM upload_sessions WHERE id = $1),
            (SELECT COUNT(*) FROM file_versions WHERE node_id = $2),
            (SELECT COUNT(*) FROM change_journal WHERE library_id = $3 AND change_kind = 'FILE_CONTENT_COMMITTED')"
    )
    .bind(synveil_core::UploadSessionId::try_from_uuid(*content_intents[0].intent_id().as_uuid()).unwrap().into_uuid())
    .bind(initial.node_id.into_uuid())
    .bind(library_id.into_uuid())
    .fetch_one(&audit_pool)
    .await
    .unwrap();
    assert_eq!(content_row.0, 1);
    assert_eq!(content_row.1, 2);
    assert_eq!(content_row.2, 2);
    assert_eq!(
        state_a
            .replica(library_id)
            .await
            .unwrap()
            .unwrap()
            .applied_sequence(),
        synveil_core::Sequence::new(2)
    );
    let content_sync_outcome = inbound_a.synchronize_once().await.unwrap();
    if content_sync_outcome != SyncOutcome::Progressed {
        let issue: Option<(String, Option<i64>, String)> = sqlx::query_as(
            "SELECT issue_kind, server_sequence, expected_state FROM local_apply_issues LIMIT 1",
        )
        .fetch_optional(&local_a_pool)
        .await
        .unwrap();
        panic!("content self-originated sync outcome {content_sync_outcome:?}, issue {issue:?}");
    }
    observer_a.poll_once().await.unwrap();
    assert_eq!(observer_a.count_pending_intents().await.unwrap(), 0);
    assert_eq!(
        state_a
            .outbound_intent(content_intents[0].intent_id())
            .await
            .unwrap()
            .unwrap()
            .state(),
        synveil_client_sync::OutboundIntentState::Reconciled
    );
    converge(&inbound_b).await;
    assert_eq!(
        fs::read(root_b.join("renamed-a.txt")).unwrap(),
        replacement_bytes
    );
    assert_eq!(state_b.count_pending_intents(library_id).await.unwrap(), 0);
    assert_eq!(
        remote_a
            .get_checkpoint(scope_a)
            .await
            .unwrap()
            .acknowledged_sequence(),
        synveil_core::Sequence::new(3)
    );
    assert_eq!(
        remote_b
            .get_checkpoint(scope_b)
            .await
            .unwrap()
            .acknowledged_sequence(),
        synveil_core::Sequence::new(3)
    );
    let serialized_result: String = sqlx::query_scalar("SELECT json_group_array(json_object('file_version_id', file_version_id, 'result_length', result_length, 'journal_sequence', journal_sequence)) FROM outbound_submission_results")
        .fetch_one(&local_a_pool)
        .await
        .unwrap();
    assert!(!serialized_result.contains("object_id"));
    drop(server);
    audit_pool.close().await;
}
