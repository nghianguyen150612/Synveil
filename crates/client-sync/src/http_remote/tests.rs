use std::{
    collections::VecDeque,
    convert::Infallible,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::Response as HttpResponse,
};
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle};

use super::*;
use crate::CanonicalBaseUrl;

const OWNER: &str = "019a1234-0000-7000-8000-000000000001";
const DEVICE: &str = "019a1234-0000-7000-8000-000000000002";
const LIBRARY: &str = "019a1234-0000-7000-8000-000000000003";
const ROOT: &str = "019a1234-0000-7000-8000-000000000004";
const NODE: &str = "019a1234-0000-7000-8000-000000000005";
const VERSION: &str = "019a1234-0000-7000-8000-000000000006";
const BOOTSTRAP: &str = "019a1234-0000-7000-8000-000000000007";
const EVENT: &str = "019a1234-0000-7000-8000-000000000008";
const CREDENTIAL: &str = "019a1234-0000-7000-8000-000000000009";
const SNAPSHOT: &str = "019a1234-0000-7000-8000-00000000000a";
const CREATED: &str = "2026-08-28T00:00:00Z";

fn scope() -> ReplicaScope {
    ReplicaScope::new(
        OWNER.parse().unwrap(),
        DEVICE.parse().unwrap(),
        LIBRARY.parse().unwrap(),
    )
}

fn credential() -> DeviceCredentialSecret {
    DeviceCredentialSecret::from_bytes([0x6a; 32])
}

fn loaded(profile: &ServerProfile) -> LoadedDeviceCredential {
    LoadedDeviceCredential::for_test(
        profile,
        OWNER.parse().unwrap(),
        DEVICE.parse().unwrap(),
        CREDENTIAL.parse().unwrap(),
        credential(),
    )
}

fn envelope(data: Value) -> Value {
    json!({"data":data,"meta":{"request_id":"fixture-request-37"}})
}

fn checkpoint(sequence: u64) -> Value {
    envelope(
        json!({"device_id":DEVICE,"library_id":LIBRARY,"epoch":"1","acknowledged_sequence":sequence.to_string(),"created_at":CREATED,"updated_at":CREATED}),
    )
}

fn feed(sequence: u64) -> Value {
    envelope(json!({
        "device_id":DEVICE,"library_id":LIBRARY,"epoch":"1","from_sequence":"0",
        "through_sequence":sequence.to_string(),"high_watermark":sequence.to_string(),"has_more":false,
        "changes":[{"event_id":EVENT,"sequence":"1","schema_version":1,"resource_kind":"NODE",
            "resource_id":NODE,"change_kind":"FILE_CONTENT_COMMITTED","resource_revision":"2",
            "occurred_at":CREATED,"parent_node_id":ROOT,"node_kind":"FILE","node_state":"ACTIVE",
            "current_version_id":VERSION}],"ack_token":"isolated-test-ack"
    }))
}

fn node(revision: u64) -> Value {
    envelope(
        json!({"id":NODE,"type":"node","revision":revision.to_string(),"attributes":{
            "library_id":LIBRARY,"parent_id":ROOT,"name":"fixture.txt","kind":"FILE","state":"ACTIVE",
            "current_version_id":VERSION,"created_at":CREATED,"updated_at":CREATED,"purge_eligible":false
        }}),
    )
}

fn version(bytes: &[u8]) -> Value {
    envelope(json!({"id":VERSION,"type":"file_version","attributes":{
        "node_id":NODE,"created_at":CREATED,"byte_length":bytes.len().to_string(),
        "sha256":hash(bytes).to_string(),"is_current":true
    }}))
}

fn mutation_applied(mutation_id: synveil_core::ClientMutationId) -> Value {
    envelope(json!({
        "outcome":"APPLIED",
        "mutation_id": mutation_id.to_string(),
        "kind":"RENAME_NODE",
        "replayed":false,
        "node":{
            "id":NODE,
            "library_id":LIBRARY,
            "parent_node_id":ROOT,
            "kind":"FILE",
            "state":"ACTIVE",
            "name":"renamed.txt",
            "revision":"3",
            "current_version_id":VERSION,
            "trashed_at":null,
            "created_at":CREATED,
            "updated_at":CREATED
        },
        "journal_event_id":EVENT,
        "journal_sequence":"2"
    }))
}

fn upload_completion(session_id: &str, bytes: &[u8]) -> Value {
    envelope(json!({
        "id": session_id,
        "type":"upload_completion",
        "attributes":{
            "node_id":NODE,
            "file_version_id":VERSION,
            "node_revision":"3",
            "bytes":bytes.len().to_string(),
            "sha256":hash(bytes).to_string(),
            "committed_at":CREATED
        }
    }))
}

fn upload_session(session_id: &str, received: u64, bytes: &[u8], committed: bool) -> Value {
    let mut attributes = json!({
        "operation":"REPLACE_CONTENT",
        "state": if committed {"COMMITTED"} else {"OPEN"},
        "received_bytes": received.to_string(),
        "expected_bytes": bytes.len().to_string(),
        "expected_sha256": hash(bytes).to_string(),
        "created_at":CREATED,
        "updated_at":CREATED,
        "expires_at":"2026-08-28T00:15:00Z",
        "target":{
            "operation":"REPLACE_CONTENT",
            "library_id":LIBRARY,
            "node_id":NODE,
            "expected_revision":"2"
        }
    });
    if committed {
        attributes["completion"] = json!({
            "id": session_id,
            "type":"upload_completion",
            "attributes":{
                "node_id":NODE,
                "file_version_id":VERSION,
                "node_revision":"3",
                "bytes":bytes.len().to_string(),
                "sha256":hash(bytes).to_string(),
                "committed_at":CREATED
            }
        });
    }
    envelope(json!({"id":session_id,"type":"upload_session","attributes":attributes}))
}

fn bootstrap(completed: bool) -> Value {
    let mut value = json!({"bootstrap_id":BOOTSTRAP,"device_id":DEVICE,"library_id":LIBRARY,
        "state":if completed {"COMPLETED"} else {"OPEN"},"generation":"1","snapshot_epoch":"1",
        "snapshot_resume_sequence":"1","manifest_item_count":"1","created_at":CREATED,
        "expires_at":"2026-08-28T00:15:00Z"});
    if completed {
        value["completed_at"] = json!(CREATED);
    }
    value
}

fn snapshot_page() -> Value {
    envelope(json!({"bootstrap":bootstrap(false),"nodes":[{
        "node_id":ROOT,"name":"root","kind":"DIRECTORY","state":"ACTIVE","revision":"0"
    }],"has_more":false,"completion_token":"isolated-test-completion"}))
}

fn handoff(snapshot_id: &str, epoch: u64, sequence: u64) -> Value {
    envelope(json!({
        "snapshot_id": snapshot_id,
        "library_id": LIBRARY,
        "checkpoint": {"epoch": epoch.to_string(), "sequence": sequence.to_string()}
    }))
}

fn durable_snapshot_descriptor(snapshot_id: &str, epoch: u64, sequence: u64) -> Value {
    envelope(json!({
        "snapshot_id": snapshot_id,
        "library_id": LIBRARY,
        "journal_boundary": {
            "library_id": LIBRARY,
            "journal_epoch": epoch.to_string(),
            "resume_sequence": sequence.to_string()
        },
        "entry_count": "1",
        "created_at": CREATED,
        "expires_at": "2026-08-28T00:15:00Z"
    }))
}

fn scoped(tail: &str) -> String {
    format!("/api/v1/devices/{DEVICE}/libraries/{LIBRARY}/{tail}")
}
fn hash(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::from_bytes(Sha256::digest(bytes).into())
}

struct Step {
    method: &'static str,
    path: String,
    authenticated: bool,
    request_json: Option<Value>,
    status: StatusCode,
    content_type: &'static str,
    body: Vec<Bytes>,
    streamed: bool,
    delay: Duration,
    chunk_delay: Duration,
    etag: Option<String>,
    location: Option<String>,
    encoding: Option<&'static str>,
    extra_encoding: Option<&'static str>,
}

impl Step {
    fn json(method: &'static str, path: impl Into<String>, value: Value) -> Self {
        Self {
            method,
            path: path.into(),
            authenticated: true,
            request_json: None,
            status: StatusCode::OK,
            content_type: "application/json",
            body: vec![Bytes::from(serde_json::to_vec(&value).unwrap())],
            streamed: false,
            delay: Duration::ZERO,
            chunk_delay: Duration::ZERO,
            etag: None,
            location: None,
            encoding: None,
            extra_encoding: None,
        }
    }

    fn request(mut self, value: Value) -> Self {
        self.request_json = Some(value);
        self
    }

    fn octets(method: &'static str, path: impl Into<String>, body: Bytes) -> Self {
        let mut step = Self::json(method, path, serde_json::Value::Null);
        step.request_json = None;
        step.status = StatusCode::NO_CONTENT;
        step.content_type = "application/json";
        step.body = vec![body];
        step
    }

    fn created(mut self) -> Self {
        self.status = StatusCode::CREATED;
        self
    }

    fn offset(mut self, offset: usize) -> Self {
        self.location = Some(format!("offset:{offset}"));
        self
    }
    fn anonymous(mut self) -> Self {
        self.authenticated = false;
        self
    }
    fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }
    fn raw(mut self, body: impl Into<Bytes>, content_type: &'static str) -> Self {
        self.body = vec![body.into()];
        self.content_type = content_type;
        self
    }
    fn delayed(mut self) -> Self {
        self.delay = Duration::from_millis(150);
        self
    }

    fn content(bytes: Vec<Bytes>, expected: &[u8]) -> Self {
        let mut step = Self::json(
            "GET",
            format!("/api/v1/versions/{VERSION}/content"),
            json!({}),
        );
        step.content_type = "application/octet-stream";
        step.body = bytes;
        step.streamed = true;
        step.etag = Some(format!("\"{}\"", hash(expected)));
        step
    }
}

#[derive(Clone)]
struct ScriptState {
    steps: Arc<Mutex<VecDeque<Step>>>,
    received: Arc<AtomicUsize>,
}

struct Script {
    profile: ServerProfile,
    state: ScriptState,
    task: JoinHandle<()>,
}

impl Script {
    async fn new(steps: Vec<Step>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = CanonicalBaseUrl::parse_for_loopback_test(&format!(
            "http://{}",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let profile = ServerProfile::new(base_url, "HTTP test").unwrap();
        let state = ScriptState {
            steps: Arc::new(Mutex::new(steps.into())),
            received: Arc::new(AtomicUsize::new(0)),
        };
        let router = Router::new()
            .fallback(script_handler)
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            profile,
            state,
            task,
        }
    }

    fn remote(&self) -> HttpSyncRemote {
        self.remote_config(HttpClientConfig::default())
    }

    fn remote_config(&self, config: HttpClientConfig) -> HttpSyncRemote {
        HttpSyncRemote::new(
            self.profile.clone(),
            DEVICE.parse().unwrap(),
            loaded(&self.profile),
            config,
        )
        .unwrap()
    }

    fn assert_finished(&self) {
        assert!(
            self.state.steps.lock().unwrap().is_empty(),
            "expected requests were not issued"
        );
    }
    fn received(&self) -> usize {
        self.state.received.load(Ordering::SeqCst)
    }
}

impl Drop for Script {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn script_handler(State(state): State<ScriptState>, request: Request) -> HttpResponse<Body> {
    state.received.fetch_add(1, Ordering::SeqCst);
    let step = state
        .steps
        .lock()
        .unwrap()
        .pop_front()
        .expect("unexpected HTTP request (including forbidden redirect/retry)");
    assert_eq!(request.method().as_str(), step.method);
    assert_eq!(request.uri().path_and_query().unwrap().as_str(), step.path);
    assert!(
        request.headers().get(header::COOKIE).is_none(),
        "desktop must never send cookies"
    );
    assert_eq!(
        request.headers().get(header::USER_AGENT).unwrap(),
        USER_AGENT
    );
    assert_eq!(
        request.headers().get(header::ACCEPT_ENCODING).unwrap(),
        "identity"
    );
    if step.authenticated {
        let expected = format!("Bearer {}", credential().expose_secret());
        assert!(
            request
                .headers()
                .get(header::AUTHORIZATION)
                .is_some_and(|value| value.as_bytes() == expected.as_bytes()),
            "missing/wrong device authority"
        );
    } else {
        assert!(
            request.headers().get(header::AUTHORIZATION).is_none(),
            "anonymous exchange/probe must not carry device credential"
        );
    }
    let body = to_bytes(request.into_body(), 2048).await.unwrap();
    match step.request_json {
        Some(expected) => assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), expected),
        None if step.method == "PATCH" => assert_eq!(body, step.body[0]),
        None => assert!(body.is_empty()),
    }
    if !step.delay.is_zero() {
        tokio::time::sleep(step.delay).await;
    }
    let mut response = HttpResponse::builder()
        .status(step.status)
        .header(header::CONTENT_TYPE, step.content_type)
        .header(header::CACHE_CONTROL, "private, no-store")
        .header(
            header::SET_COOKIE,
            "browser-cookie=must-never-return; HttpOnly; Secure",
        );
    if let Some(etag) = step.etag {
        response = response.header(header::ETAG, etag);
    }
    if let Some(location) = step.location {
        if let Some(offset) = location.strip_prefix("offset:") {
            response = response.header("upload-offset", offset);
        } else {
            response = response.header(header::LOCATION, location);
        }
    }
    if let Some(encoding) = step.encoding {
        response = response.header(header::CONTENT_ENCODING, encoding);
    }
    if let Some(encoding) = step.extra_encoding {
        response = response.header(header::CONTENT_ENCODING, encoding);
    }
    let body = if step.streamed {
        let chunks: VecDeque<_> = step.body.into();
        Body::from_stream(stream::unfold(
            (chunks, step.chunk_delay),
            |(mut chunks, delay)| async move {
                let chunk = chunks.pop_front()?;
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                Some((Ok::<Bytes, Infallible>(chunk), (chunks, delay)))
            },
        ))
    } else {
        Body::from(step.body.into_iter().next().unwrap_or_default())
    };
    response.body(body).unwrap()
}

#[tokio::test]
async fn every_sync_operation_matches_real_dto_method_path_headers_and_evidence() {
    let bytes = b"bounded logical content";
    let server = Script::new(vec![
        Step::json("GET", scoped("checkpoint"), checkpoint(0)),
        Step::json("GET", scoped("changes?limit=2"), feed(1)),
        Step::json("GET", format!("/api/v1/nodes/{NODE}"), node(2)),
        Step::json("GET", format!("/api/v1/versions/{VERSION}"), version(bytes)),
        Step::json("POST", scoped("changes/ack"), checkpoint(1)).request(json!({"ack_token":"isolated-test-ack"})),
        Step::json("POST", scoped("rebaseline"), envelope(bootstrap(false))).request(json!({})),
        Step::json("GET", scoped(&format!("rebaseline/{BOOTSTRAP}/nodes?limit=2")), snapshot_page()),
        Step::json("POST", scoped(&format!("rebaseline/{BOOTSTRAP}/complete")), envelope(json!({
            "bootstrap":bootstrap(true),"checkpoint":{"journal_epoch":"1","acknowledged_sequence":"1","updated_at":CREATED},"replayed":false
        }))).request(json!({"completion_token":"isolated-test-completion"})),
        Step::json("GET", format!("/api/v1/nodes/{NODE}"), node(2)),
        Step::json("GET", format!("/api/v1/versions/{VERSION}"), version(bytes)),
        Step::content(vec![Bytes::from_static(&bytes[..4]),Bytes::from_static(&bytes[4..])], bytes),
        Step::json("GET", "/health/ready", json!({"status":"ready"})).anonymous(),
        Step::json("GET", scoped("checkpoint"), checkpoint(1)),
    ]).await;
    let remote = server.remote();
    assert_eq!(
        remote
            .get_checkpoint(scope())
            .await
            .unwrap()
            .acknowledged_sequence(),
        Sequence::new(0)
    );
    let page = remote.fetch_changes(scope(), 2).await.unwrap();
    assert_eq!(page.changes().len(), 1);
    assert_eq!(
        page.changes()[0].desired_node().unwrap().name().as_str(),
        "fixture.txt"
    );
    assert_eq!(
        remote
            .acknowledge_changes(scope(), page.ack_evidence().unwrap())
            .await
            .unwrap()
            .acknowledged_sequence(),
        Sequence::new(1)
    );
    let bootstrap = remote.start_rebaseline(scope()).await.unwrap();
    let page = remote
        .fetch_rebaseline_page(scope(), bootstrap.id(), None, 2)
        .await
        .unwrap();
    assert_eq!(page.nodes().len(), 1);
    assert_eq!(
        remote
            .complete_rebaseline(scope(), bootstrap.id(), page.completion_evidence().unwrap())
            .await
            .unwrap()
            .bootstrap_id(),
        bootstrap.id()
    );
    let content = remote
        .download_current_content(scope(), NODE.parse().unwrap(), VERSION.parse().unwrap())
        .await
        .unwrap();
    assert_eq!(content.length(), bytes.len() as u64);
    let received = collect(content).await.unwrap();
    assert_eq!(received, bytes);
    assert_eq!(remote.probe_health(scope()).await, ConnectionHealth::Online);
    assert_eq!(
        remote.server_profile_id(),
        Some(server.profile.profile_id())
    );
    assert!(!format!("{remote:?}").contains(credential().expose_secret()));
    server.assert_finished();
}

#[tokio::test]
async fn durable_rebaseline_handoff_uses_snapshot_path_and_strict_confirmation() {
    let snapshot_id: synveil_core::RebaselineSnapshotId = SNAPSHOT.parse().unwrap();
    let server = Script::new(vec![
        Step::json(
            "POST",
            format!("/api/v1/rebaseline-snapshots/{SNAPSHOT}/handoff"),
            handoff(SNAPSHOT, 3, 27),
        )
        .request(json!({})),
    ])
    .await;
    let confirmation = server
        .remote()
        .complete_rebaseline_handoff(scope(), snapshot_id)
        .await
        .unwrap();
    assert_eq!(confirmation.snapshot_id(), snapshot_id);
    assert_eq!(confirmation.library_id(), scope().library_id());
    assert_eq!(confirmation.checkpoint().scope(), scope());
    assert_eq!(confirmation.checkpoint().epoch(), Sequence::new(3));
    assert_eq!(
        confirmation.checkpoint().acknowledged_sequence(),
        Sequence::new(27)
    );
    server.assert_finished();
}

#[tokio::test]
async fn durable_rebaseline_snapshot_create_uses_scoped_existing_route_and_server_boundary() {
    let server = Script::new(vec![
        Step::json(
            "POST",
            format!("/api/v1/libraries/{LIBRARY}/rebaseline-snapshots"),
            durable_snapshot_descriptor(SNAPSHOT, 3, 27),
        )
        .created()
        .request(json!({})),
    ])
    .await;
    let descriptor = server.remote().create_snapshot(scope()).await.unwrap();
    assert_eq!(descriptor.snapshot_id().to_string(), SNAPSHOT);
    assert_eq!(descriptor.library_id(), scope().library_id());
    assert_eq!(descriptor.boundary().journal_epoch(), Sequence::new(3));
    assert_eq!(descriptor.boundary().resume_sequence(), Sequence::new(27));
    assert_eq!(descriptor.entry_count(), 1);
    server.assert_finished();
}

#[tokio::test]
async fn durable_rebaseline_handoff_rejects_unknown_or_numeric_confirmation_fields() {
    let snapshot_id: synveil_core::RebaselineSnapshotId = SNAPSHOT.parse().unwrap();
    let mut unknown = handoff(SNAPSHOT, 3, 27);
    unknown["data"]["future_authority"] = json!(true);
    let mut numeric = handoff(SNAPSHOT, 3, 27);
    numeric["data"]["checkpoint"]["sequence"] = json!(27);
    let server = Script::new(vec![
        Step::json(
            "POST",
            format!("/api/v1/rebaseline-snapshots/{SNAPSHOT}/handoff"),
            unknown,
        )
        .request(json!({})),
        Step::json(
            "POST",
            format!("/api/v1/rebaseline-snapshots/{SNAPSHOT}/handoff"),
            numeric,
        )
        .request(json!({})),
    ])
    .await;
    for _ in 0..2 {
        assert_eq!(
            server
                .remote()
                .complete_rebaseline_handoff(scope(), snapshot_id)
                .await
                .unwrap_err()
                .kind(),
            RemoteErrorKind::Protocol
        );
    }
    server.assert_finished();
}

#[tokio::test]
async fn outbound_mutation_and_upload_operations_match_reviewed_http_contract() {
    let mutation_id = synveil_core::ClientMutationId::new();
    let upload_idempotency_key = synveil_core::OutboundIntentId::new();
    let upload_id = synveil_core::UploadSessionId::try_from_uuid(*upload_idempotency_key.as_uuid())
        .unwrap()
        .to_string();
    let bytes = b"replacement bytes";
    let server = Script::new(vec![
        Step::json("POST", scoped("mutations"), mutation_applied(mutation_id)).request(json!({
            "mutation_id": mutation_id.to_string(),
            "base_epoch":"1",
            "base_sequence":"1",
            "kind":"RENAME_NODE",
            "payload":{"node_id":NODE,"expected_revision":"2","new_name":"renamed.txt"}
        })),
        Step::json(
            "POST",
            "/api/v1/upload-sessions",
            upload_session(&upload_id, 0, bytes, false),
        )
        .created()
        .request(json!({
            "operation":"REPLACE_CONTENT",
            "idempotency_key": upload_idempotency_key.to_string(),
            "library_id":LIBRARY,
            "node_id":NODE,
            "expected_revision":"2",
            "expected_bytes":bytes.len().to_string(),
            "expected_sha256":hash(bytes).to_string()
        })),
        Step::json(
            "GET",
            format!("/api/v1/upload-sessions/{upload_id}"),
            upload_session(&upload_id, 0, bytes, false),
        ),
        Step::octets(
            "PATCH",
            format!("/api/v1/upload-sessions/{upload_id}"),
            Bytes::from_static(bytes),
        )
        .offset(bytes.len()),
        Step::json(
            "POST",
            format!("/api/v1/upload-sessions/{upload_id}/complete"),
            upload_completion(&upload_id, bytes),
        )
        .request(json!({})),
        Step::json(
            "POST",
            format!("/api/v1/upload-sessions/{upload_id}/abort"),
            upload_session(&upload_id, bytes.len() as u64, bytes, false),
        )
        .request(json!({})),
    ])
    .await;
    let remote = server.remote();
    let request = synveil_core::ClientMutationRequest::new(
        mutation_id,
        Sequence::new(1),
        Sequence::new(1),
        synveil_core::ClientMutation::rename_node(
            NODE.parse().unwrap(),
            synveil_core::Revision::new(2),
            synveil_core::LogicalName::new("renamed.txt").unwrap(),
        ),
    );
    assert!(matches!(
        remote
            .submit_client_mutation(scope(), &request)
            .await
            .unwrap(),
        crate::RemoteMutationOutcome::Applied(_)
    ));
    let target = crate::UploadTarget::ReplaceContent {
        library_id: LIBRARY.parse().unwrap(),
        node_id: NODE.parse().unwrap(),
        expected_revision: synveil_core::Revision::new(2),
    };
    let status = remote
        .create_upload_session(
            scope(),
            upload_idempotency_key,
            &target,
            bytes.len() as u64,
            hash(bytes),
        )
        .await
        .unwrap();
    let session_id = status.session_id();
    assert_eq!(
        remote
            .get_upload_session(scope(), session_id)
            .await
            .unwrap()
            .received_bytes(),
        0
    );
    assert_eq!(
        remote
            .append_upload_chunk(scope(), session_id, 0, Bytes::from_static(bytes))
            .await
            .unwrap(),
        bytes.len() as u64
    );
    assert_eq!(
        remote
            .complete_upload(scope(), session_id)
            .await
            .unwrap()
            .length(),
        bytes.len() as u64
    );
    assert!(remote.abort_upload(scope(), session_id).await.is_ok());
    server.assert_finished();
}

async fn collect(content: RemoteContent) -> Result<Vec<u8>, RemoteError> {
    let mut body = content.into_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        assert!(chunk.len() <= MAX_CONTENT_CHUNK_BYTES);
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[tokio::test]
async fn typed_status_mapping_does_not_collapse_errors_or_echo_remote_messages() {
    let cases = [
        (401, "authentication_failed", RemoteErrorKind::AuthRequired),
        (401, "device_revoked", RemoteErrorKind::DeviceRevoked),
        (403, "permission_denied", RemoteErrorKind::Forbidden),
        (404, "not_found", RemoteErrorKind::NotFound),
        (404, "device_revoked", RemoteErrorKind::DeviceRevoked),
        (
            409,
            "sync_rebaseline_required",
            RemoteErrorKind::RebaselineRequired,
        ),
        (
            409,
            "checkpoint_conflict",
            RemoteErrorKind::CheckpointConflict,
        ),
        (400, "invalid_ack_token", RemoteErrorKind::InvalidEvidence),
        (
            400,
            "invalid_bootstrap_token",
            RemoteErrorKind::InvalidEvidence,
        ),
        (429, "rate_limited", RemoteErrorKind::RateLimited),
        (
            503,
            "internal_dependency_unavailable",
            RemoteErrorKind::Unavailable,
        ),
        (503, "dependency_unavailable", RemoteErrorKind::Unavailable),
        (500, "internal_error", RemoteErrorKind::Internal),
        (
            410,
            "bootstrap_expired",
            RemoteErrorKind::RebaselineRequired,
        ),
        (401, "unknown_future_error", RemoteErrorKind::Protocol),
        (200, "authentication_failed", RemoteErrorKind::Protocol),
    ];
    let steps = cases.iter().map(|(status,code,_)| Step::json("GET",scoped("checkpoint"),json!({"error":{
        "code":code,"message":credential().expose_secret(),"request_id":"fixture-request-37","retryable":true,
        "details":{"nested_secret":credential().expose_secret()}
    }})).status(StatusCode::from_u16(*status).unwrap())).collect();
    let server = Script::new(steps).await;
    let remote = server.remote();
    for (_, _, kind) in cases {
        let error = remote.get_checkpoint(scope()).await.unwrap_err();
        assert_eq!(error.kind(), kind);
        assert!(!format!("{error:?} {error}").contains(credential().expose_secret()));
    }
    server.assert_finished();
}

#[tokio::test]
async fn malformed_json_unknown_required_shape_scope_epoch_and_content_type_fail_closed() {
    let mut wrong_scope = checkpoint(0);
    wrong_scope["data"]["device_id"] = json!(OWNER);
    let mut wrong_epoch = checkpoint(0);
    wrong_epoch["data"]["epoch"] = json!("0");
    let mut unknown_field = checkpoint(0);
    unknown_field["data"]["future_authority"] = json!(true);
    let mut numeric_sequence = checkpoint(0);
    numeric_sequence["data"]["acknowledged_sequence"] = json!(0);
    let server = Script::new(vec![
        Step::json("GET", scoped("checkpoint"), json!({})).raw("{", "application/json"),
        Step::json("GET", scoped("checkpoint"), json!({}))
            .raw("<html>Proxy login</html>", "text/html"),
        Step::json("GET", scoped("checkpoint"), wrong_scope),
        Step::json("GET", scoped("checkpoint"), wrong_epoch),
        Step::json("GET", scoped("checkpoint"), unknown_field),
        Step::json("GET", scoped("checkpoint"), numeric_sequence),
    ])
    .await;
    let remote = server.remote();
    for _ in 0..6 {
        assert_eq!(
            remote.get_checkpoint(scope()).await.unwrap_err().kind(),
            RemoteErrorKind::Protocol
        );
    }
    server.assert_finished();
}

#[tokio::test]
async fn metadata_bound_enforced_with_and_without_content_length() {
    let mut streamed = Step::json("GET", scoped("checkpoint"), json!({}))
        .raw(Bytes::from(vec![b'x'; 513]), "application/json");
    streamed.streamed = true;
    let server = Script::new(vec![
        Step::json("GET", scoped("checkpoint"), json!({}))
            .raw(Bytes::from(vec![b'x'; 513]), "application/json"),
        streamed,
    ])
    .await;
    let remote = server.remote_config(HttpClientConfig {
        max_metadata_bytes: 512,
        ..HttpClientConfig::default()
    });
    for _ in 0..2 {
        assert_eq!(
            remote.get_checkpoint(scope()).await.unwrap_err().kind(),
            RemoteErrorKind::BodyLimit
        );
    }
    server.assert_finished();
}

#[tokio::test]
async fn redirect_never_forwards_cross_origin_bearer_or_cookies() {
    let other = Script::new(vec![]).await;
    let mut redirect = Step::json("GET", scoped("checkpoint"), json!({})).status(StatusCode::FOUND);
    redirect.location = Some(format!("{}stolen", other.profile.base_url().as_str()));
    let first = Script::new(vec![redirect]).await;
    let error = first.remote().get_checkpoint(scope()).await.unwrap_err();
    assert_eq!(error.kind(), RemoteErrorKind::Redirect);
    assert_eq!(other.received(), 0);
    first.assert_finished();
}

#[tokio::test]
async fn profile_owner_and_device_binding_reject_before_network() {
    let first = Script::new(vec![]).await;
    let other = Script::new(vec![]).await;
    assert_eq!(
        HttpSyncRemote::new(
            other.profile.clone(),
            DEVICE.parse().unwrap(),
            loaded(&first.profile),
            HttpClientConfig::default()
        )
        .unwrap_err()
        .kind(),
        RemoteErrorKind::Rejected
    );
    assert_eq!(
        HttpSyncRemote::new(
            first.profile.clone(),
            OWNER.parse().unwrap(),
            loaded(&first.profile),
            HttpClientConfig::default()
        )
        .unwrap_err()
        .kind(),
        RemoteErrorKind::Rejected
    );
    let remote = first.remote();
    let wrong_owner = ReplicaScope::new(
        DEVICE.parse().unwrap(),
        DEVICE.parse().unwrap(),
        LIBRARY.parse().unwrap(),
    );
    let wrong_device = ReplicaScope::new(
        OWNER.parse().unwrap(),
        OWNER.parse().unwrap(),
        LIBRARY.parse().unwrap(),
    );
    for bad in [wrong_owner, wrong_device] {
        assert_eq!(
            remote.get_checkpoint(bad).await.unwrap_err().kind(),
            RemoteErrorKind::Forbidden
        );
    }
    // The server's feed contract is 1..=500; snapshot pages allow 1..=1000.
    // Invalid limits must fail locally without transmitting the credential.
    for limit in [0, 501, 1_000, 1_001] {
        assert_eq!(
            remote
                .fetch_changes(scope(), limit)
                .await
                .unwrap_err()
                .kind(),
            RemoteErrorKind::Rejected
        );
    }
    for limit in [0, 1_001] {
        assert_eq!(
            remote
                .fetch_rebaseline_page(scope(), BOOTSTRAP.parse().unwrap(), None, limit)
                .await
                .unwrap_err()
                .kind(),
            RemoteErrorKind::Rejected
        );
    }
    assert_eq!(first.received(), 0);
    assert_eq!(other.received(), 0);
}

#[tokio::test]
async fn headers_and_metadata_read_deadlines_are_typed_and_never_retried() {
    let server = Script::new(vec![
        Step::json("GET", scoped("checkpoint"), checkpoint(0)).delayed(),
    ])
    .await;
    let remote = server.remote_config(HttpClientConfig {
        header_timeout: Duration::from_millis(30),
        ..HttpClientConfig::default()
    });
    assert_eq!(
        remote.get_checkpoint(scope()).await.unwrap_err().kind(),
        RemoteErrorKind::Timeout
    );
    assert_eq!(server.received(), 1);
    let mut step = Step::json("GET", scoped("checkpoint"), checkpoint(0));
    step.streamed = true;
    step.chunk_delay = Duration::from_millis(150);
    let server = Script::new(vec![step]).await;
    let remote = server.remote_config(HttpClientConfig {
        metadata_timeout: Duration::from_millis(30),
        ..HttpClientConfig::default()
    });
    assert_eq!(
        remote.get_checkpoint(scope()).await.unwrap_err().kind(),
        RemoteErrorKind::Timeout
    );
    assert_eq!(server.received(), 1);
}

#[tokio::test]
async fn content_stream_is_chunk_bounded_and_hash_checked_without_whole_file_buffering() {
    let bytes = vec![0x42; MAX_CONTENT_CHUNK_BYTES * 2 + 71];
    let server = Script::new(vec![
        Step::json("GET", format!("/api/v1/nodes/{NODE}"), node(2)),
        Step::json(
            "GET",
            format!("/api/v1/versions/{VERSION}"),
            version(&bytes),
        ),
        Step::content(vec![Bytes::from(bytes.clone())], &bytes),
    ])
    .await;
    let content = server
        .remote()
        .download_current_content(scope(), NODE.parse().unwrap(), VERSION.parse().unwrap())
        .await
        .unwrap();
    assert_eq!(collect(content).await.unwrap(), bytes);
    server.assert_finished();
}

#[tokio::test]
async fn content_stream_rejects_extra_missing_corrupt_bytes_and_idle_timeout() {
    for (actual, expected_error) in [
        (&b"hello!"[..], RemoteErrorKind::BodyLimit),
        (&b"hell"[..], RemoteErrorKind::Integrity),
        (&b"jello"[..], RemoteErrorKind::Integrity),
    ] {
        let server = Script::new(vec![
            Step::json("GET", format!("/api/v1/nodes/{NODE}"), node(2)),
            Step::json(
                "GET",
                format!("/api/v1/versions/{VERSION}"),
                version(b"hello"),
            ),
            Step::content(vec![Bytes::copy_from_slice(actual)], b"hello"),
        ])
        .await;
        let content = server
            .remote()
            .download_current_content(scope(), NODE.parse().unwrap(), VERSION.parse().unwrap())
            .await
            .unwrap();
        assert_eq!(collect(content).await.unwrap_err().kind(), expected_error);
        server.assert_finished();
    }
    let mut content = Step::content(vec![Bytes::from_static(b"hello")], b"hello");
    content.chunk_delay = Duration::from_millis(150);
    let server = Script::new(vec![
        Step::json("GET", format!("/api/v1/nodes/{NODE}"), node(2)),
        Step::json(
            "GET",
            format!("/api/v1/versions/{VERSION}"),
            version(b"hello"),
        ),
        content,
    ])
    .await;
    let remote = server.remote_config(HttpClientConfig {
        stream_idle_timeout: Duration::from_millis(30),
        ..HttpClientConfig::default()
    });
    let content = remote
        .download_current_content(scope(), NODE.parse().unwrap(), VERSION.parse().unwrap())
        .await
        .unwrap();
    assert_eq!(
        collect(content).await.unwrap_err().kind(),
        RemoteErrorKind::Timeout
    );
}

#[tokio::test]
async fn content_compression_version_and_validator_mismatch_fail_closed() {
    let mut wrong_version = version(b"hello");
    wrong_version["data"]["id"] = json!(EVENT);
    let mut wrong_etag = Step::content(vec![Bytes::from_static(b"hello")], b"hello");
    wrong_etag.etag = Some("\"different\"".to_owned());
    let mut compressed = Step::content(vec![Bytes::from_static(b"hello")], b"hello");
    compressed.encoding = Some("gzip");
    let server = Script::new(vec![
        Step::json("GET", format!("/api/v1/nodes/{NODE}"), node(2)),
        Step::json("GET", format!("/api/v1/versions/{VERSION}"), wrong_version),
        Step::json("GET", format!("/api/v1/nodes/{NODE}"), node(2)),
        Step::json(
            "GET",
            format!("/api/v1/versions/{VERSION}"),
            version(b"hello"),
        ),
        wrong_etag,
        Step::json("GET", format!("/api/v1/nodes/{NODE}"), node(2)),
        Step::json(
            "GET",
            format!("/api/v1/versions/{VERSION}"),
            version(b"hello"),
        ),
        compressed,
    ])
    .await;
    let remote = server.remote();
    for expected in [
        RemoteErrorKind::Protocol,
        RemoteErrorKind::Integrity,
        RemoteErrorKind::Protocol,
    ] {
        assert_eq!(
            remote
                .download_current_content(scope(), NODE.parse().unwrap(), VERSION.parse().unwrap())
                .await
                .unwrap_err()
                .kind(),
            expected
        );
    }
    server.assert_finished();
}

#[tokio::test]
async fn duplicate_or_combined_content_encoding_is_rejected_for_json_and_files() {
    for (encoding, extra_encoding) in [
        ("identity", Some("gzip")),
        ("identity", Some("identity")),
        ("identity, gzip", None),
    ] {
        let mut metadata = Step::json("GET", scoped("checkpoint"), checkpoint(0));
        metadata.encoding = Some(encoding);
        metadata.extra_encoding = extra_encoding;
        let mut content = Step::content(vec![Bytes::from_static(b"hello")], b"hello");
        content.encoding = Some(encoding);
        content.extra_encoding = extra_encoding;
        let server = Script::new(vec![
            metadata,
            Step::json("GET", format!("/api/v1/nodes/{NODE}"), node(2)),
            Step::json(
                "GET",
                format!("/api/v1/versions/{VERSION}"),
                version(b"hello"),
            ),
            content,
        ])
        .await;
        let remote = server.remote();
        assert_eq!(
            remote.get_checkpoint(scope()).await.unwrap_err().kind(),
            RemoteErrorKind::Protocol
        );
        assert_eq!(
            remote
                .download_current_content(scope(), NODE.parse().unwrap(), VERSION.parse().unwrap())
                .await
                .unwrap_err()
                .kind(),
            RemoteErrorKind::Protocol
        );
        server.assert_finished();
    }
}

#[tokio::test]
async fn feed_revision_drift_requires_rebaseline_and_never_fabricates_old_names() {
    let server = Script::new(vec![
        Step::json("GET", scoped("changes?limit=1"), feed(1)),
        Step::json("GET", format!("/api/v1/nodes/{NODE}"), node(3)),
    ])
    .await;
    assert_eq!(
        server
            .remote()
            .fetch_changes(scope(), 1)
            .await
            .unwrap_err()
            .kind(),
        RemoteErrorKind::RebaselineRequired
    );
    server.assert_finished();
}

#[tokio::test]
async fn initial_zero_revision_directory_event_matches_the_canonical_domain() {
    // Synveil INITIAL_REVISION is zero, including roots and freshly created
    // directories. Canonical zero is valid; numeric JSON and leading zeroes
    // remain invalid protocol representations.
    let mut page = feed(1);
    let event = page["data"]["changes"][0].as_object_mut().unwrap();
    event.insert("change_kind".into(), json!("NODE_CREATED"));
    event.insert("resource_revision".into(), json!("0"));
    event.insert("node_kind".into(), json!("DIRECTORY"));
    event.remove("current_version_id");
    let mut desired = node(0);
    desired["data"]["attributes"]["kind"] = json!("DIRECTORY");
    desired["data"]["attributes"]
        .as_object_mut()
        .unwrap()
        .remove("current_version_id");
    let server = Script::new(vec![
        Step::json("GET", scoped("changes?limit=1"), page),
        Step::json("GET", format!("/api/v1/nodes/{NODE}"), desired),
    ])
    .await;
    let page = server.remote().fetch_changes(scope(), 1).await.unwrap();
    assert_eq!(page.changes()[0].event().resource_revision().get(), 0);
    assert_eq!(
        page.changes()[0].desired_node().unwrap().revision().get(),
        0
    );
    assert_eq!(
        page.changes()[0].desired_node().unwrap().kind(),
        NodeKind::Directory
    );
    assert_eq!(server.received(), 2);
    server.assert_finished();
}

#[tokio::test]
async fn feed_schema_sequence_evidence_and_page_scope_are_checked_before_resolution() {
    let mut future = feed(1);
    future["data"]["changes"][0]["schema_version"] = json!(2);
    let mut gap = feed(1);
    gap["data"]["changes"][0]["sequence"] = json!("2");
    let mut missing_evidence = feed(1);
    missing_evidence["data"]["ack_token"] = Value::Null;
    let mut mismatch = feed(1);
    mismatch["data"]["library_id"] = json!(OWNER);
    let mut bad_bounds = feed(1);
    bad_bounds["data"]["high_watermark"] = json!("0");
    let server = Script::new(
        [future, gap, missing_evidence, mismatch, bad_bounds]
            .into_iter()
            .map(|value| Step::json("GET", scoped("changes?limit=1"), value))
            .collect(),
    )
    .await;
    let remote = server.remote();
    for _ in 0..5 {
        assert_eq!(
            remote.fetch_changes(scope(), 1).await.unwrap_err().kind(),
            RemoteErrorKind::Protocol
        );
    }
    server.assert_finished();
}

#[tokio::test]
async fn rebaseline_terminal_evidence_and_identity_are_required() {
    let mut no_proof = snapshot_page();
    no_proof["data"]["completion_token"] = Value::Null;
    let mut wrong_id = snapshot_page();
    wrong_id["data"]["bootstrap"]["bootstrap_id"] = json!(EVENT);
    let mut inconsistent = snapshot_page();
    inconsistent["data"]["has_more"] = json!(true);
    let path = scoped(&format!("rebaseline/{BOOTSTRAP}/nodes?limit=2"));
    let server = Script::new(
        [no_proof, wrong_id, inconsistent]
            .into_iter()
            .map(|value| Step::json("GET", path.clone(), value))
            .collect(),
    )
    .await;
    let remote = server.remote();
    for _ in 0..3 {
        assert_eq!(
            remote
                .fetch_rebaseline_page(scope(), BOOTSTRAP.parse().unwrap(), None, 2)
                .await
                .unwrap_err()
                .kind(),
            RemoteErrorKind::Protocol
        );
    }
    server.assert_finished();
}

#[tokio::test]
async fn rebaseline_cursor_is_encoded_as_data_and_terminal_page_is_exact() {
    let mut cut = bootstrap(false);
    cut["manifest_item_count"] = json!("2");
    let mut first = snapshot_page();
    first["data"]["bootstrap"] = cut.clone();
    first["data"]["has_more"] = json!(true);
    first["data"]
        .as_object_mut()
        .unwrap()
        .remove("completion_token");
    first["data"]["next_cursor"] = json!("isolated/token?part=2&proof=x");
    let second = envelope(json!({"bootstrap":cut,"nodes":[{
        "node_id":NODE,"parent_node_id":ROOT,"name":"fixture.txt","kind":"FILE","state":"ACTIVE",
        "revision":"2","current_version_id":VERSION,
        "current_content":{"byte_length":"5","sha256":hash(b"hello").to_string()}
    }],"has_more":false,"completion_token":"isolated-test-completion"}));
    let server = Script::new(vec![
        Step::json("GET",scoped(&format!("rebaseline/{BOOTSTRAP}/nodes?limit=1")),first),
        Step::json("GET",scoped(&format!("rebaseline/{BOOTSTRAP}/nodes?limit=1&cursor=isolated%2Ftoken%3Fpart%3D2%26proof%3Dx")),second),
    ]).await;
    let remote = server.remote();
    let first = remote
        .fetch_rebaseline_page(scope(), BOOTSTRAP.parse().unwrap(), None, 1)
        .await
        .unwrap();
    assert!(first.has_more());
    assert!(first.completion_evidence().is_none());
    let second = remote
        .fetch_rebaseline_page(scope(), BOOTSTRAP.parse().unwrap(), first.next_cursor(), 1)
        .await
        .unwrap();
    assert!(!second.has_more());
    assert_eq!(second.nodes()[0].content_length(), Some(5));
    assert!(second.completion_evidence().is_some());
    server.assert_finished();
}

#[tokio::test]
async fn purge_feed_uses_tombstone_without_a_current_node_request() {
    let mut page = feed(1);
    let event = page["data"]["changes"][0].as_object_mut().unwrap();
    event.insert("change_kind".into(), json!("NODE_PURGED"));
    event.remove("parent_node_id");
    event.remove("node_state");
    event.remove("current_version_id");
    let server = Script::new(vec![Step::json("GET", scoped("changes?limit=1"), page)]).await;
    let page = server.remote().fetch_changes(scope(), 1).await.unwrap();
    assert!(page.changes()[0].desired_node().is_none());
    assert_eq!(
        page.changes()[0].event().change_kind(),
        ChangeKind::NodePurged
    );
    assert_eq!(server.received(), 1);
    server.assert_finished();
}

#[tokio::test]
async fn download_total_deadline_is_independent_of_longer_idle_budget() {
    let mut body = Step::content(vec![Bytes::from_static(b"hello")], b"hello");
    body.chunk_delay = Duration::from_millis(150);
    let server = Script::new(vec![
        Step::json("GET", format!("/api/v1/nodes/{NODE}"), node(2)),
        Step::json(
            "GET",
            format!("/api/v1/versions/{VERSION}"),
            version(b"hello"),
        ),
        body,
    ])
    .await;
    let remote = server.remote_config(HttpClientConfig {
        download_timeout: Duration::from_millis(30),
        stream_idle_timeout: Duration::from_secs(5),
        ..HttpClientConfig::default()
    });
    let content = remote
        .download_current_content(scope(), NODE.parse().unwrap(), VERSION.parse().unwrap())
        .await
        .unwrap();
    assert_eq!(
        collect(content).await.unwrap_err().kind(),
        RemoteErrorKind::Timeout
    );
    server.assert_finished();
}

#[tokio::test]
async fn enrollment_exact_201_once_only_redacted_handoff_without_bearer_or_cookie() {
    let enrollment = EnrollmentSecret::from_bytes([0x4f; 32]);
    let server = Script::new(vec![
        Step::json(
            "POST",
            "/api/v1/device-enrollment/exchange",
            envelope(json!({
                "owner_user_id":OWNER,"device_id":DEVICE,"credential_id":CREDENTIAL,
                "device_credential":credential().expose_secret(),"created_at":CREATED
            })),
        )
        .anonymous()
        .status(StatusCode::CREATED)
        .request(json!({"enrollment_token":enrollment.expose_secret()})),
    ])
    .await;
    let client =
        HttpEnrollmentClient::new(server.profile.clone(), HttpClientConfig::default()).unwrap();
    let result = client.exchange(&enrollment).await.unwrap();
    assert_eq!(result.profile_id(), server.profile.profile_id());
    assert_eq!(result.base_url(), server.profile.base_url());
    assert_eq!(result.owner_user_id(), OWNER.parse().unwrap());
    assert_eq!(result.device_id(), DEVICE.parse().unwrap());
    assert_eq!(result.credential_id(), CREDENTIAL.parse().unwrap());
    assert_eq!(result.secret(), &credential());
    assert!(!format!("{result:?} {client:?}").contains(credential().expose_secret()));
    assert_eq!(server.received(), 1);
    server.assert_finished();
}

#[tokio::test]
async fn enrollment_lost_response_is_not_retried() {
    let enrollment = EnrollmentSecret::from_bytes([0x4f; 32]);
    let server = Script::new(vec![
        Step::json("POST", "/api/v1/device-enrollment/exchange", json!({}))
            .anonymous()
            .request(json!({"enrollment_token":enrollment.expose_secret()}))
            .delayed(),
    ])
    .await;
    let client = HttpEnrollmentClient::new(
        server.profile.clone(),
        HttpClientConfig {
            header_timeout: Duration::from_millis(30),
            ..HttpClientConfig::default()
        },
    )
    .unwrap();
    let error = client.exchange(&enrollment).await.unwrap_err();
    assert_eq!(error.kind(), RemoteErrorKind::Timeout);
    assert_eq!(server.received(), 1);
    assert!(!format!("{error:?}").contains(enrollment.expose_secret()));
}

#[tokio::test]
async fn health_rejects_html_proxy_and_distinguishes_revocation_and_authentication() {
    let server=Script::new(vec![
        Step::json("GET","/health/ready",json!({})).anonymous().raw("<html>not Synveil</html>","text/html"),
        Step::json("GET","/health/ready",json!({"status":"ready"})).anonymous(),
        Step::json("GET",scoped("checkpoint"),json!({"error":{"code":"device_revoked","message":"revoked","request_id":"fixture-request-37","retryable":false}})).status(StatusCode::UNAUTHORIZED),
        Step::json("GET","/health/ready",json!({"status":"ready"})).anonymous(),
        Step::json("GET",scoped("checkpoint"),json!({"error":{"code":"authentication_failed","message":"authentication required","request_id":"fixture-request-37","retryable":false}})).status(StatusCode::UNAUTHORIZED),
    ]).await;
    let remote = server.remote();
    assert_eq!(
        remote.probe_health(scope()).await,
        ConnectionHealth::ProtocolError
    );
    assert_eq!(
        remote.probe_health(scope()).await,
        ConnectionHealth::DeviceRevoked
    );
    assert_eq!(
        remote.probe_health(scope()).await,
        ConnectionHealth::AuthRequired
    );
    server.assert_finished();
}

#[tokio::test]
async fn connection_refusal_is_offline_and_configuration_cannot_disable_bounds() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let profile = ServerProfile::new(
        CanonicalBaseUrl::parse_for_loopback_test(&format!("http://{address}")).unwrap(),
        "offline",
    )
    .unwrap();
    let remote = HttpSyncRemote::new(
        profile.clone(),
        DEVICE.parse().unwrap(),
        loaded(&profile),
        HttpClientConfig::default(),
    )
    .unwrap();
    assert_eq!(
        remote.get_checkpoint(scope()).await.unwrap_err().kind(),
        RemoteErrorKind::Offline
    );
    for bad in [
        HttpClientConfig {
            connect_timeout: Duration::ZERO,
            ..HttpClientConfig::default()
        },
        HttpClientConfig {
            max_metadata_bytes: 0,
            ..HttpClientConfig::default()
        },
        HttpClientConfig {
            max_metadata_bytes: MAX_JSON_BYTES + 1,
            ..HttpClientConfig::default()
        },
    ] {
        assert_eq!(
            HttpSyncRemote::new(
                profile.clone(),
                DEVICE.parse().unwrap(),
                loaded(&profile),
                bad
            )
            .unwrap_err()
            .kind(),
            RemoteErrorKind::Rejected
        );
    }
}

#[tokio::test]
async fn production_tls_rejects_ephemeral_self_signed_certificate_before_http_authority() {
    // Generated in memory for this one test: no key fixture, client root
    // override, certificate bypass, network dependency, or plaintext probe.
    let certified = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_owned()]).unwrap();
    let key = rustls::pki_types::PrivatePkcs8KeyDer::from(certified.key_pair.serialize_der());
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certified.cert.der().clone()], key.into())
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        // A rejected TLS handshake cannot receive HTTP Authorization.
        tokio_rustls::TlsAcceptor::from(Arc::new(config))
            .accept(stream)
            .await
            .is_err()
    });
    let profile = ServerProfile::new(
        CanonicalBaseUrl::parse(&format!("https://{address}")).unwrap(),
        "TLS rejection test",
    )
    .unwrap();
    let remote = HttpSyncRemote::new(
        profile.clone(),
        DEVICE.parse().unwrap(),
        loaded(&profile),
        HttpClientConfig::default(),
    )
    .unwrap();
    let error = remote.get_checkpoint(scope()).await.unwrap_err();
    assert_eq!(error.kind(), RemoteErrorKind::Tls);
    assert!(!format!("{error:?} {remote:?}").contains(credential().expose_secret()));
    assert!(
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
    );
}
