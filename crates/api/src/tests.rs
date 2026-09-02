use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{
        HeaderMap, HeaderValue, Method, Request, StatusCode,
        header::{CONTENT_LENGTH, CONTENT_RANGE},
    },
    response::Response,
};
use bytes::Bytes;
use futures_util::{StreamExt, stream};
use http_body_util::BodyExt;
use serde_json::Value;
use sha2::{Digest, Sha256};
use synveil_auth::{
    AuthError, PasswordHasherConfig, PasswordParameters, PasswordVerification, PlaintextPassword,
    SessionExpiry, SessionPrincipal, SessionToken, StoredPasswordHash,
};
use synveil_core::{
    DomainError, ErrorCode, FileVersionId, Library, LibraryId, LogicalName, LoginIdentifier, Node,
    NodeId, NodeKind, NodeState, ObjectId, Revision, Sha256Digest, Timestamp, UploadSessionId,
    UploadSessionState, UserId,
};
use synveil_metadata::{
    AuthorizedContent, ContentReadMetadataBackend, ContentReadResolution, FileMetadataBackend,
    FileMetadataError, FileVersionMetadata, FileVersionPage, LibraryPage, MetadataError, NodePage,
    RestoredFileVersion, UploadCompletion, VersionHistoryBackend, VersionHistoryError,
    VersionRestoreBackend, VersionRestoreError,
};
use synveil_storage::{
    ByteRange, ContentReadApplicationService, ContentReadError, CreateUploadSessionRequest,
    IntegrityExpectation, ObjectKey, ObjectStore, PutRequest, UploadByteStream, UploadError,
    UploadProgress, UploadSessionView, UploadTargetRequest, UploadTargetView, boxed_content_stream,
    boxed_stream, open_local_object_store,
};
use tower::ServiceExt;

use super::{
    ApiError, ApiState, AuthenticationBackend, BOOTSTRAP_BODY_LIMIT_BYTES, BootstrapStatus,
    CookieConfig, DownloadBackend, DownloadMetadata, DownloadRead, EtagKey, IssuedSession,
    RequestId, StaticReadiness, SystemHealthAuthorizer, UPLOAD_JSON_BODY_LIMIT_BYTES,
    UploadBackend, map_core_error, router,
};

const TEST_LOGIN: &str = "alice";
const TEST_LOGIN_KEY: &str = "alice-key";
const TEST_PASSWORD: &str = "correct horse battery staple";

struct TestHealthAuthorizer;

impl SystemHealthAuthorizer for TestHealthAuthorizer {
    fn authorize(&self, headers: &HeaderMap) -> bool {
        headers
            .get("x-test-health-access")
            .and_then(|value| value.to_str().ok())
            == Some("allow")
    }
}

fn state(ready: bool) -> ApiState {
    ApiState::from_current_platform().with_readiness(Arc::new(StaticReadiness::new(ready)))
}

fn auth_state() -> ApiState {
    state(true)
        .with_auth_backend(Arc::new(TestAuthenticationBackend::new()))
        .with_cookie_config(CookieConfig::production())
        .with_allowed_origin("https://app.example")
}

fn request(method: Method, uri: &str, body: Body) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(body)
        .expect("test request must be valid")
}

fn json_request(method: Method, uri: &str, body: &str) -> Request<Body> {
    let mut request = request(method, uri, Body::from(body.to_owned()));
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

fn cookie_header(session: &str, csrf: &str) -> HeaderValue {
    HeaderValue::from_str(&format!("synveil_session={session}; synveil_csrf={csrf}"))
        .expect("test cookie header must be valid")
}

fn authenticated_request(method: Method, uri: &str, session: &str, csrf: &str) -> Request<Body> {
    let mut request = request(method, uri, Body::empty());
    request
        .headers_mut()
        .insert("cookie", cookie_header(session, csrf));
    request
}

fn mark_same_origin(request: &mut Request<Body>) {
    request
        .headers_mut()
        .insert("origin", HeaderValue::from_static("https://app.example"));
    request
        .headers_mut()
        .insert("sec-fetch-site", HeaderValue::from_static("same-origin"));
}

struct TestAuthenticationBackend {
    password_config: PasswordHasherConfig,
    password_hash: StoredPasswordHash,
    user_id: UserId,
    bootstrap_state: Mutex<BootstrapStatus>,
    bootstrap_calls: AtomicUsize,
    sessions: Mutex<HashMap<[u8; 32], TestSession>>,
}

struct TestSession {
    principal: SessionPrincipal,
    expires_at: SessionExpiry,
    revoked: bool,
}

impl TestAuthenticationBackend {
    fn new() -> Self {
        let password_config = PasswordHasherConfig::new(
            PasswordParameters::new(8 * 1024, 1, 1, 32).expect("test password parameters"),
        )
        .expect("test password config");
        let password = PlaintextPassword::new(TEST_PASSWORD).expect("test password");
        let password_hash =
            StoredPasswordHash::hash(password_config, &password).expect("test password hash");
        Self {
            password_config,
            password_hash,
            user_id: UserId::new(),
            bootstrap_state: Mutex::new(BootstrapStatus::Open),
            bootstrap_calls: AtomicUsize::new(0),
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl AuthenticationBackend for TestAuthenticationBackend {
    async fn bootstrap_state(&self) -> Result<BootstrapStatus, AuthError> {
        Ok(*self.bootstrap_state.lock().expect("bootstrap state lock"))
    }

    async fn create_first_admin(
        &self,
        _login: LoginIdentifier,
        _password: PlaintextPassword,
    ) -> Result<(), AuthError> {
        self.bootstrap_calls.fetch_add(1, Ordering::Relaxed);
        let mut state = self.bootstrap_state.lock().expect("bootstrap state lock");
        if *state == BootstrapStatus::Closed {
            return Err(AuthError::BootstrapClosed);
        }
        *state = BootstrapStatus::Closed;
        Ok(())
    }

    async fn login(
        &self,
        login: LoginIdentifier,
        password: PlaintextPassword,
    ) -> Result<IssuedSession, AuthError> {
        let verified = self
            .password_hash
            .verify(self.password_config, &password)
            .map_err(AuthError::from)?;
        if !matches!(verified, PasswordVerification::Verified { .. })
            || login.value() != TEST_LOGIN
            || login.uniqueness_key() != TEST_LOGIN_KEY
        {
            return Err(AuthError::InvalidCredentials);
        }

        let token = SessionToken::generate();
        let session_id = synveil_auth::SessionId::new();
        let expiry = SessionExpiry::from_now(Timestamp::now(), Duration::from_secs(3_600))
            .expect("test session expiry");
        let principal = SessionPrincipal::new(self.user_id, true, session_id);
        self.sessions.lock().expect("test session lock").insert(
            token.digest(),
            TestSession {
                principal,
                expires_at: expiry,
                revoked: false,
            },
        );
        Ok(IssuedSession::new(token, principal, expiry))
    }

    async fn authenticate_session(
        &self,
        token: &SessionToken,
    ) -> Result<SessionPrincipal, AuthError> {
        let sessions = self.sessions.lock().expect("test session lock");
        let Some(session) = sessions.get(&token.digest()) else {
            return Err(AuthError::InvalidSession);
        };
        if session.revoked || session.expires_at.is_expired_at(Timestamp::now()) {
            return Err(AuthError::InvalidSession);
        }
        Ok(session.principal)
    }

    async fn revoke_current_session(&self, token: &SessionToken) -> Result<(), AuthError> {
        let mut sessions = self.sessions.lock().expect("test session lock");
        let Some(session) = sessions.get_mut(&token.digest()) else {
            return Err(AuthError::InvalidSession);
        };
        session.revoked = true;
        Ok(())
    }
}

struct TestFileMetadataBackend {
    state: Mutex<TestFileMetadataState>,
}

struct TestFileMetadataState {
    library: Library,
    nodes: HashMap<NodeId, synveil_core::Node>,
}

impl TestFileMetadataBackend {
    fn new(owner_user_id: UserId) -> Self {
        let observed_at = Timestamp::parse("2026-08-23T00:00:00Z").expect("valid test time");
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
            LogicalName::new("Primary").expect("valid library name"),
            &root,
            synveil_core::DedupDomainId::new(),
            observed_at,
        )
        .expect("test library must be valid");
        let mut nodes = HashMap::new();
        nodes.insert(root.id(), root);
        Self {
            state: Mutex::new(TestFileMetadataState { library, nodes }),
        }
    }

    fn library_id(&self) -> LibraryId {
        self.state
            .lock()
            .expect("file metadata state lock")
            .library
            .id()
    }

    fn root_id(&self) -> NodeId {
        self.state
            .lock()
            .expect("file metadata state lock")
            .library
            .root_node_id()
    }

    fn add_file(&self, node_id: NodeId, name: &str) {
        let mut state = self.state.lock().expect("file metadata state lock");
        let root = state
            .nodes
            .get(&state.library.root_node_id())
            .cloned()
            .expect("test root node");
        let file = Node::new_child(
            node_id,
            state.library.id(),
            &root,
            NodeKind::File,
            LogicalName::new(name).expect("test filename must be valid"),
            Timestamp::parse("2026-08-23T00:00:00Z").expect("valid test time"),
        )
        .expect("test file must be valid");
        state.nodes.insert(node_id, file);
    }
}

fn fake_file_error(error: DomainError) -> FileMetadataError {
    match error {
        DomainError::EmptyName | DomainError::NameTooLong => FileMetadataError::InvalidRequest,
        _ => FileMetadataError::InvalidState,
    }
}

#[async_trait]
impl FileMetadataBackend for TestFileMetadataBackend {
    async fn list_libraries(
        &self,
        user_id: UserId,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<LibraryPage, FileMetadataError> {
        if !(1..=100).contains(&limit) {
            return Err(FileMetadataError::InvalidRequest);
        }
        if cursor.as_deref() == Some("invalid") {
            return Err(FileMetadataError::InvalidCursor);
        }
        let state = self.state.lock().expect("file metadata state lock");
        if state.library.owner_user_id() != user_id {
            return Err(FileMetadataError::NotFound);
        }
        Ok(LibraryPage::new(vec![state.library.clone()], None, false))
    }

    async fn list_children(
        &self,
        user_id: UserId,
        library_id: LibraryId,
        parent_node_id: Option<NodeId>,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<NodePage, FileMetadataError> {
        if !(1..=100).contains(&limit) {
            return Err(FileMetadataError::InvalidRequest);
        }
        if cursor.as_deref() == Some("invalid") {
            return Err(FileMetadataError::InvalidCursor);
        }
        let state = self.state.lock().expect("file metadata state lock");
        if state.library.owner_user_id() != user_id || state.library.id() != library_id {
            return Err(FileMetadataError::NotFound);
        }
        let parent_node_id = parent_node_id.unwrap_or(state.library.root_node_id());
        let parent = state
            .nodes
            .get(&parent_node_id)
            .ok_or(FileMetadataError::NotFound)?;
        if parent.kind() != NodeKind::Directory || parent.state() != NodeState::Active {
            return Err(FileMetadataError::InvalidState);
        }
        let mut nodes = state
            .nodes
            .values()
            .filter(|node| {
                node.parent_node_id() == Some(parent_node_id) && node.state() == NodeState::Active
            })
            .cloned()
            .collect::<Vec<_>>();
        nodes.sort_by_key(Node::id);
        let has_more = nodes.len() > limit as usize;
        nodes.truncate(limit as usize);
        Ok(NodePage::new(nodes, None, has_more))
    }

    async fn get_node(&self, user_id: UserId, node_id: NodeId) -> Result<Node, FileMetadataError> {
        let state = self.state.lock().expect("file metadata state lock");
        if state.library.owner_user_id() != user_id {
            return Err(FileMetadataError::NotFound);
        }
        state
            .nodes
            .get(&node_id)
            .cloned()
            .ok_or(FileMetadataError::NotFound)
    }

    async fn create_directory(
        &self,
        user_id: UserId,
        library_id: LibraryId,
        parent_node_id: Option<NodeId>,
        name: LogicalName,
    ) -> Result<Node, FileMetadataError> {
        let mut state = self.state.lock().expect("file metadata state lock");
        if state.library.owner_user_id() != user_id || state.library.id() != library_id {
            return Err(FileMetadataError::NotFound);
        }
        let parent_node_id = parent_node_id.unwrap_or(state.library.root_node_id());
        let parent = state
            .nodes
            .get(&parent_node_id)
            .cloned()
            .ok_or(FileMetadataError::NotFound)?;
        let node = Node::new_child(
            NodeId::new(),
            library_id,
            &parent,
            NodeKind::Directory,
            name,
            Timestamp::now(),
        )
        .map_err(fake_file_error)?;
        state.nodes.insert(node.id(), node.clone());
        Ok(node)
    }

    async fn rename_node(
        &self,
        user_id: UserId,
        node_id: NodeId,
        name: LogicalName,
        expected_revision: Revision,
    ) -> Result<Node, FileMetadataError> {
        let mut state = self.state.lock().expect("file metadata state lock");
        if state.library.owner_user_id() != user_id {
            return Err(FileMetadataError::NotFound);
        }
        let node = state
            .nodes
            .get_mut(&node_id)
            .ok_or(FileMetadataError::NotFound)?;
        if node.revision() != expected_revision {
            return Err(FileMetadataError::VersionConflict {
                current_revision: node.revision(),
            });
        }
        node.rename(name, Timestamp::now())
            .map_err(fake_file_error)?;
        Ok(node.clone())
    }

    async fn move_node(
        &self,
        user_id: UserId,
        node_id: NodeId,
        destination_parent_id: NodeId,
        expected_revision: Revision,
    ) -> Result<Node, FileMetadataError> {
        let mut state = self.state.lock().expect("file metadata state lock");
        if state.library.owner_user_id() != user_id {
            return Err(FileMetadataError::NotFound);
        }
        let destination = state
            .nodes
            .get(&destination_parent_id)
            .cloned()
            .ok_or(FileMetadataError::NotFound)?;
        let mut ancestor = Some(destination.id());
        let mut seen = HashSet::new();
        while let Some(ancestor_id) = ancestor {
            if ancestor_id == node_id || !seen.insert(ancestor_id) {
                return Err(FileMetadataError::InvalidState);
            }
            ancestor = state.nodes.get(&ancestor_id).and_then(Node::parent_node_id);
        }
        let node = state
            .nodes
            .get_mut(&node_id)
            .ok_or(FileMetadataError::NotFound)?;
        if node.revision() != expected_revision {
            return Err(FileMetadataError::VersionConflict {
                current_revision: node.revision(),
            });
        }
        node.move_to(&destination, Timestamp::now())
            .map_err(fake_file_error)?;
        Ok(node.clone())
    }

    async fn delete_node(
        &self,
        user_id: UserId,
        node_id: NodeId,
        expected_revision: Revision,
    ) -> Result<Node, FileMetadataError> {
        let mut state = self.state.lock().expect("file metadata state lock");
        if state.library.owner_user_id() != user_id {
            return Err(FileMetadataError::NotFound);
        }
        if node_id == state.library.root_node_id() {
            return Err(FileMetadataError::InvalidState);
        }
        if state
            .nodes
            .values()
            .any(|node| node.parent_node_id() == Some(node_id))
        {
            return Err(FileMetadataError::InvalidState);
        }
        let node = state
            .nodes
            .get_mut(&node_id)
            .ok_or(FileMetadataError::NotFound)?;
        if node.revision() != expected_revision {
            return Err(FileMetadataError::VersionConflict {
                current_revision: node.revision(),
            });
        }
        node.transition_state(NodeState::Trashed, Timestamp::now())
            .map_err(fake_file_error)?;
        Ok(node.clone())
    }

    async fn restore_node(
        &self,
        user_id: UserId,
        node_id: NodeId,
        expected_revision: Revision,
    ) -> Result<Node, FileMetadataError> {
        let mut state = self.state.lock().expect("file metadata state lock");
        if state.library.owner_user_id() != user_id {
            return Err(FileMetadataError::NotFound);
        }
        let parent_id = state
            .nodes
            .get(&node_id)
            .and_then(Node::parent_node_id)
            .ok_or(FileMetadataError::InvalidState)?;
        let parent = state
            .nodes
            .get(&parent_id)
            .ok_or(FileMetadataError::InvalidState)?;
        if parent.kind() != NodeKind::Directory || parent.state() != NodeState::Active {
            return Err(FileMetadataError::InvalidState);
        }
        let node = state
            .nodes
            .get_mut(&node_id)
            .ok_or(FileMetadataError::NotFound)?;
        if node.revision() != expected_revision {
            return Err(FileMetadataError::VersionConflict {
                current_revision: node.revision(),
            });
        }
        node.transition_state(NodeState::Active, Timestamp::now())
            .map_err(fake_file_error)?;
        Ok(node.clone())
    }
}

struct TestVersionHistoryBackend {
    owner_user_id: UserId,
    file_node_id: NodeId,
    directory_node_id: NodeId,
    trashed_node_id: NodeId,
    current_version_id: FileVersionId,
    previous_version_id: FileVersionId,
}

impl TestVersionHistoryBackend {
    fn new(owner_user_id: UserId) -> Self {
        Self {
            owner_user_id,
            file_node_id: NodeId::new(),
            directory_node_id: NodeId::new(),
            trashed_node_id: NodeId::new(),
            current_version_id: FileVersionId::new(),
            previous_version_id: FileVersionId::new(),
        }
    }

    fn file_node_id(&self) -> NodeId {
        self.file_node_id
    }

    fn directory_node_id(&self) -> NodeId {
        self.directory_node_id
    }

    fn trashed_node_id(&self) -> NodeId {
        self.trashed_node_id
    }

    fn current_version_id(&self) -> FileVersionId {
        self.current_version_id
    }

    fn previous_version_id(&self) -> FileVersionId {
        self.previous_version_id
    }

    fn metadata(&self, version_id: FileVersionId) -> FileVersionMetadata {
        let is_current = version_id == self.current_version_id;
        FileVersionMetadata::new(
            version_id,
            self.file_node_id,
            Timestamp::parse("2026-08-24T00:00:00Z").expect("valid version timestamp"),
            if is_current { 42 } else { 24 },
            if is_current {
                Sha256Digest::from_bytes([0xab; 32])
            } else {
                Sha256Digest::from_bytes([0xcd; 32])
            },
            is_current,
        )
    }
}

#[async_trait]
impl VersionHistoryBackend for TestVersionHistoryBackend {
    async fn list_file_versions(
        &self,
        user_id: UserId,
        node_id: NodeId,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<FileVersionPage, VersionHistoryError> {
        if !(1..=100).contains(&limit) {
            return Err(VersionHistoryError::InvalidRequest);
        }
        if user_id != self.owner_user_id {
            return Err(VersionHistoryError::NotFound);
        }
        if cursor.as_deref() == Some("v1.version.opaque-page-cursor")
            && node_id != self.file_node_id
        {
            return Err(VersionHistoryError::InvalidCursor);
        }
        if node_id == self.directory_node_id {
            return Err(VersionHistoryError::InvalidState);
        }
        if node_id == self.trashed_node_id || node_id != self.file_node_id {
            return Err(VersionHistoryError::NotFound);
        }
        match cursor.as_deref() {
            None => Ok(FileVersionPage::new(
                vec![self.metadata(self.current_version_id)],
                Some("v1.version.opaque-page-cursor".to_owned()),
                true,
            )),
            Some("v1.version.opaque-page-cursor") => Ok(FileVersionPage::new(
                vec![self.metadata(self.previous_version_id)],
                None,
                false,
            )),
            Some("invalid" | "cross-node") => Err(VersionHistoryError::InvalidCursor),
            Some(_) => Err(VersionHistoryError::InvalidCursor),
        }
    }

    async fn get_file_version_metadata(
        &self,
        user_id: UserId,
        version_id: FileVersionId,
    ) -> Result<FileVersionMetadata, VersionHistoryError> {
        if user_id != self.owner_user_id
            || !matches!(version_id, id if id == self.current_version_id || id == self.previous_version_id)
        {
            return Err(VersionHistoryError::NotFound);
        }
        Ok(self.metadata(version_id))
    }
}

#[derive(Clone)]
struct StoredRestoreOutcome {
    node_id: NodeId,
    source_version_id: FileVersionId,
    expected_revision: Revision,
    result: RestoredFileVersion,
}

struct TestVersionRestoreBackend {
    owner_user_id: UserId,
    file_node_id: NodeId,
    directory_node_id: NodeId,
    trashed_node_id: NodeId,
    source_version_id: FileVersionId,
    source_byte_length: u64,
    source_sha256: Sha256Digest,
    current_version_id: Mutex<FileVersionId>,
    current_revision: Mutex<Revision>,
    outcomes: Mutex<HashMap<String, StoredRestoreOutcome>>,
    applied_calls: AtomicUsize,
}

impl TestVersionRestoreBackend {
    fn new(owner_user_id: UserId, file_node_id: NodeId, source_version_id: FileVersionId) -> Self {
        Self {
            owner_user_id,
            file_node_id,
            directory_node_id: NodeId::new(),
            trashed_node_id: NodeId::new(),
            source_version_id,
            source_byte_length: 24,
            source_sha256: Sha256Digest::from_bytes([0xcd; 32]),
            current_version_id: Mutex::new(FileVersionId::new()),
            current_revision: Mutex::new(Revision::new(0)),
            outcomes: Mutex::new(HashMap::new()),
            applied_calls: AtomicUsize::new(0),
        }
    }

    fn directory_node_id(&self) -> NodeId {
        self.directory_node_id
    }

    fn trashed_node_id(&self) -> NodeId {
        self.trashed_node_id
    }

    fn current_version_id(&self) -> FileVersionId {
        *self
            .current_version_id
            .lock()
            .expect("restore current version lock")
    }

    fn current_revision(&self) -> Revision {
        *self
            .current_revision
            .lock()
            .expect("restore current revision lock")
    }

    fn set_current_version_id(&self, version_id: FileVersionId) {
        *self
            .current_version_id
            .lock()
            .expect("restore current version lock") = version_id;
    }

    fn with_source_metadata(mut self, byte_length: u64, sha256: Sha256Digest) -> Self {
        self.source_byte_length = byte_length;
        self.source_sha256 = sha256;
        self
    }

    fn applied_calls(&self) -> usize {
        self.applied_calls.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl VersionRestoreBackend for TestVersionRestoreBackend {
    async fn restore_file_version(
        &self,
        user_id: UserId,
        node_id: NodeId,
        source_version_id: FileVersionId,
        expected_revision: Revision,
        idempotency_key: String,
    ) -> Result<RestoredFileVersion, VersionRestoreError> {
        if user_id != self.owner_user_id {
            return Err(VersionRestoreError::NotFound);
        }
        if node_id == self.trashed_node_id {
            return Err(VersionRestoreError::NotFound);
        }
        if node_id == self.directory_node_id {
            return Err(VersionRestoreError::InvalidState);
        }
        if node_id != self.file_node_id || source_version_id != self.source_version_id {
            return Err(VersionRestoreError::NotFound);
        }

        if let Some(outcome) = self
            .outcomes
            .lock()
            .expect("restore outcome lock")
            .get(&idempotency_key)
            .cloned()
        {
            if outcome.node_id != node_id
                || outcome.source_version_id != source_version_id
                || outcome.expected_revision != expected_revision
            {
                return Err(VersionRestoreError::IdempotencyConflict);
            }
            return Ok(outcome.result);
        }

        let current_revision = self.current_revision();
        if current_revision != expected_revision {
            return Err(VersionRestoreError::VersionConflict { current_revision });
        }
        if self.current_version_id() == source_version_id {
            return Err(VersionRestoreError::InvalidRequest);
        }

        let next_revision = Revision::new(
            current_revision
                .get()
                .checked_add(1)
                .expect("test revision must not overflow"),
        );
        let new_version_id = FileVersionId::new();
        let result = RestoredFileVersion::new(
            FileVersionMetadata::new(
                new_version_id,
                node_id,
                Timestamp::parse("2026-08-24T00:00:01Z").expect("valid restore timestamp"),
                self.source_byte_length,
                self.source_sha256,
                true,
            ),
            next_revision,
        );
        self.applied_calls.fetch_add(1, Ordering::Relaxed);
        *self
            .current_revision
            .lock()
            .expect("restore current revision lock") = next_revision;
        *self
            .current_version_id
            .lock()
            .expect("restore current version lock") = new_version_id;
        self.outcomes.lock().expect("restore outcome lock").insert(
            idempotency_key,
            StoredRestoreOutcome {
                node_id,
                source_version_id,
                expected_revision,
                result: result.clone(),
            },
        );
        Ok(result)
    }
}

struct TestDownloadBackend {
    owner_user_id: UserId,
    node_id: NodeId,
    file_version_id: FileVersionId,
    bytes: Vec<u8>,
    sha256: Sha256Digest,
    supports_range_reads: bool,
    current_error: Mutex<Option<ContentReadError>>,
    historical_error: Mutex<Option<ContentReadError>>,
    open_calls: AtomicUsize,
    range_calls: AtomicUsize,
}

impl TestDownloadBackend {
    fn new(owner_user_id: UserId, bytes: &[u8]) -> Self {
        Self::with_ids(owner_user_id, NodeId::new(), FileVersionId::new(), bytes)
    }

    fn with_ids(
        owner_user_id: UserId,
        node_id: NodeId,
        file_version_id: FileVersionId,
        bytes: &[u8],
    ) -> Self {
        let digest = Sha256::digest(bytes);
        Self {
            owner_user_id,
            node_id,
            file_version_id,
            bytes: bytes.to_vec(),
            sha256: Sha256Digest::try_from(digest.as_slice()).expect("SHA-256 is valid"),
            supports_range_reads: true,
            current_error: Mutex::new(None),
            historical_error: Mutex::new(None),
            open_calls: AtomicUsize::new(0),
            range_calls: AtomicUsize::new(0),
        }
    }

    fn node_id(&self) -> NodeId {
        self.node_id
    }

    fn file_version_id(&self) -> FileVersionId {
        self.file_version_id
    }

    fn set_current_error(&self, error: ContentReadError) {
        *self.current_error.lock().expect("download error lock") = Some(error);
    }

    fn open_calls(&self) -> usize {
        self.open_calls.load(Ordering::Relaxed)
    }

    fn range_calls(&self) -> usize {
        self.range_calls.load(Ordering::Relaxed)
    }

    fn metadata(&self) -> DownloadMetadata {
        DownloadMetadata::new(
            self.node_id,
            self.file_version_id,
            self.bytes.len() as u64,
            self.sha256,
        )
    }

    fn check_current(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
    ) -> Result<DownloadMetadata, ContentReadError> {
        if let Some(error) = *self.current_error.lock().expect("download error lock") {
            return Err(error);
        }
        if owner_user_id != self.owner_user_id || node_id != self.node_id {
            return Err(ContentReadError::ContentNotFound);
        }
        Ok(self.metadata())
    }

    fn check_historical(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
    ) -> Result<DownloadMetadata, ContentReadError> {
        if let Some(error) = *self
            .historical_error
            .lock()
            .expect("historical download error lock")
        {
            return Err(error);
        }
        if owner_user_id != self.owner_user_id || file_version_id != self.file_version_id {
            return Err(ContentReadError::VersionNotFound);
        }
        Ok(self.metadata())
    }

    fn read(&self, range: Option<ByteRange>) -> Result<DownloadRead, ContentReadError> {
        let bytes = match range {
            Some(range) => {
                if range.end_exclusive() > self.bytes.len() as u64 {
                    return Err(ContentReadError::InvalidRange);
                }
                self.range_calls.fetch_add(1, Ordering::Relaxed);
                self.bytes[range.start() as usize..range.end_exclusive() as usize].to_vec()
            }
            None => {
                self.open_calls.fetch_add(1, Ordering::Relaxed);
                self.bytes.clone()
            }
        };
        let frames = bytes
            .chunks(3)
            .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
            .collect::<Vec<_>>();
        Ok(DownloadRead::new(
            self.metadata(),
            boxed_content_stream(stream::iter(frames)),
        ))
    }
}

#[async_trait]
impl DownloadBackend for TestDownloadBackend {
    fn supports_range_reads(&self) -> bool {
        self.supports_range_reads
    }

    async fn current_content_metadata(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
    ) -> Result<DownloadMetadata, ContentReadError> {
        self.check_current(owner_user_id, node_id)
    }

    async fn historical_content_metadata(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
    ) -> Result<DownloadMetadata, ContentReadError> {
        self.check_historical(owner_user_id, file_version_id)
    }

    async fn open_current_content(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
    ) -> Result<DownloadRead, ContentReadError> {
        self.check_current(owner_user_id, node_id)?;
        self.read(None)
    }

    async fn open_current_content_range(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
        range: ByteRange,
    ) -> Result<DownloadRead, ContentReadError> {
        self.check_current(owner_user_id, node_id)?;
        self.read(Some(range))
    }

    async fn open_historical_content(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
    ) -> Result<DownloadRead, ContentReadError> {
        self.check_historical(owner_user_id, file_version_id)?;
        self.read(None)
    }

    async fn open_historical_content_range(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
        range: ByteRange,
    ) -> Result<DownloadRead, ContentReadError> {
        self.check_historical(owner_user_id, file_version_id)?;
        self.read(Some(range))
    }
}

struct TestRestoreDownloadBackend {
    current: Arc<TestDownloadBackend>,
    historical: Arc<TestDownloadBackend>,
}

#[async_trait]
impl DownloadBackend for TestRestoreDownloadBackend {
    fn supports_range_reads(&self) -> bool {
        self.current.supports_range_reads()
    }

    async fn current_content_metadata(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
    ) -> Result<DownloadMetadata, ContentReadError> {
        self.current
            .current_content_metadata(owner_user_id, node_id)
            .await
    }

    async fn historical_content_metadata(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
    ) -> Result<DownloadMetadata, ContentReadError> {
        self.historical
            .historical_content_metadata(owner_user_id, file_version_id)
            .await
    }

    async fn open_current_content(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
    ) -> Result<DownloadRead, ContentReadError> {
        self.current
            .open_current_content(owner_user_id, node_id)
            .await
    }

    async fn open_current_content_range(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
        range: ByteRange,
    ) -> Result<DownloadRead, ContentReadError> {
        self.current
            .open_current_content_range(owner_user_id, node_id, range)
            .await
    }

    async fn open_historical_content(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
    ) -> Result<DownloadRead, ContentReadError> {
        self.historical
            .open_historical_content(owner_user_id, file_version_id)
            .await
    }

    async fn open_historical_content_range(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
        range: ByteRange,
    ) -> Result<DownloadRead, ContentReadError> {
        self.historical
            .open_historical_content_range(owner_user_id, file_version_id, range)
            .await
    }
}

#[derive(Clone)]
struct TestContentReadMetadata {
    owner_user_id: UserId,
    node_id: NodeId,
    file_version_id: FileVersionId,
    content: AuthorizedContent,
}

#[async_trait]
impl ContentReadMetadataBackend for TestContentReadMetadata {
    async fn resolve_current_content(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
        _backend_kind: &str,
    ) -> Result<ContentReadResolution, MetadataError> {
        if owner_user_id != self.owner_user_id || node_id != self.node_id {
            return Ok(ContentReadResolution::NotFound);
        }
        Ok(ContentReadResolution::Found(self.content.clone()))
    }

    async fn resolve_file_version_content(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
        _backend_kind: &str,
    ) -> Result<ContentReadResolution, MetadataError> {
        if owner_user_id != self.owner_user_id || file_version_id != self.file_version_id {
            return Ok(ContentReadResolution::NotFound);
        }
        Ok(ContentReadResolution::Found(self.content.clone()))
    }
}

struct TestDownloadRoot(PathBuf);

impl Drop for TestDownloadRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct TestUploadBackend {
    state: Mutex<TestUploadState>,
    max_chunk_size: u64,
    frame_count: AtomicUsize,
    largest_frame: AtomicUsize,
}

#[derive(Default)]
struct TestUploadState {
    sessions: HashMap<UploadSessionId, TestUploadSession>,
}

struct TestUploadSession {
    view: UploadSessionView,
    bytes: Vec<u8>,
}

impl TestUploadBackend {
    fn new(max_chunk_size: u64) -> Self {
        Self {
            state: Mutex::new(TestUploadState::default()),
            max_chunk_size,
            frame_count: AtomicUsize::new(0),
            largest_frame: AtomicUsize::new(0),
        }
    }

    fn frame_count(&self) -> usize {
        self.frame_count.load(Ordering::Relaxed)
    }

    fn largest_frame(&self) -> usize {
        self.largest_frame.load(Ordering::Relaxed)
    }

    fn bytes(&self, session_id: UploadSessionId) -> Vec<u8> {
        self.state
            .lock()
            .expect("upload state lock")
            .sessions
            .get(&session_id)
            .expect("test upload session")
            .bytes
            .clone()
    }
}

#[async_trait]
impl UploadBackend for TestUploadBackend {
    fn max_chunk_size(&self) -> u64 {
        self.max_chunk_size
    }

    async fn create_upload_session(
        &self,
        request: CreateUploadSessionRequest,
    ) -> Result<UploadSessionView, UploadError> {
        let session_id = UploadSessionId::new();
        let target = match request.target {
            UploadTargetRequest::CreateFile {
                library_id,
                parent_node_id,
                name,
            } => UploadTargetView::CreateFile {
                library_id,
                parent_node_id,
                node_id: NodeId::new(),
                name,
            },
            UploadTargetRequest::ReplaceContent {
                library_id,
                node_id,
                expected_revision,
            } => UploadTargetView::ReplaceContent {
                library_id,
                node_id,
                expected_revision,
            },
        };
        let created_at =
            Timestamp::parse("2026-08-24T00:00:00Z").expect("test upload creation time");
        let view = UploadSessionView {
            id: session_id,
            owner_user_id: request.owner_user_id,
            target,
            expected_length: request.expected_length,
            expected_sha256: request.expected_sha256,
            received_bytes: 0,
            state: UploadSessionState::Open,
            created_at,
            updated_at: created_at,
            expires_at: Timestamp::parse("2099-08-24T00:00:00Z").expect("test upload expiry"),
            last_error_code: None,
            terminal_failure_code: None,
            completion: None,
        };
        self.state
            .lock()
            .expect("upload state lock")
            .sessions
            .insert(
                session_id,
                TestUploadSession {
                    view: view.clone(),
                    bytes: Vec::new(),
                },
            );
        Ok(view)
    }

    async fn get_upload_session(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
    ) -> Result<UploadSessionView, UploadError> {
        let state = self.state.lock().expect("upload state lock");
        let session = state
            .sessions
            .get(&session_id)
            .ok_or(UploadError::NotFound)?;
        if session.view.owner_user_id != owner_user_id {
            return Err(UploadError::NotFound);
        }
        Ok(session.view.clone())
    }

    async fn append_upload_stream(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        expected_offset: u64,
        mut stream: UploadByteStream,
    ) -> Result<UploadProgress, UploadError> {
        {
            let state = self.state.lock().expect("upload state lock");
            let session = state
                .sessions
                .get(&session_id)
                .ok_or(UploadError::NotFound)?;
            if session.view.owner_user_id != owner_user_id {
                return Err(UploadError::NotFound);
            }
            if session.view.state != UploadSessionState::Open {
                return Err(UploadError::InvalidState {
                    state: session.view.state,
                });
            }
            if session.view.received_bytes != expected_offset {
                return Err(UploadError::InvalidOffset {
                    current_offset: session.view.received_bytes,
                });
            }
        }

        let mut request_bytes = 0_u64;
        let mut current_offset = expected_offset;
        let mut appended = false;
        while let Some(frame) = stream.next().await {
            let frame = frame?;
            if frame.is_empty() {
                continue;
            }
            let frame_length =
                u64::try_from(frame.len()).map_err(|_| UploadError::ChunkTooLarge)?;
            request_bytes = request_bytes
                .checked_add(frame_length)
                .ok_or(UploadError::ChunkTooLarge)?;
            if request_bytes > self.max_chunk_size {
                return Err(UploadError::ChunkTooLarge);
            }
            self.frame_count.fetch_add(1, Ordering::Relaxed);
            self.largest_frame.fetch_max(frame.len(), Ordering::Relaxed);

            let mut state = self.state.lock().expect("upload state lock");
            let session = state
                .sessions
                .get_mut(&session_id)
                .ok_or(UploadError::NotFound)?;
            if session.view.received_bytes != current_offset {
                return Err(UploadError::InvalidOffset {
                    current_offset: session.view.received_bytes,
                });
            }
            let next_offset = current_offset
                .checked_add(frame_length)
                .ok_or(UploadError::SizeMismatch)?;
            if next_offset > session.view.expected_length {
                return Err(UploadError::SizeMismatch);
            }
            session.bytes.extend_from_slice(&frame);
            session.view.received_bytes = next_offset;
            current_offset = next_offset;
            appended = true;
        }
        if !appended {
            return Err(UploadError::InvalidRequest);
        }
        Ok(UploadProgress {
            session_id,
            received_bytes: current_offset,
        })
    }

    async fn complete_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
    ) -> Result<UploadCompletion, UploadError> {
        let mut state = self.state.lock().expect("upload state lock");
        let session = state
            .sessions
            .get_mut(&session_id)
            .ok_or(UploadError::NotFound)?;
        if session.view.owner_user_id != owner_user_id {
            return Err(UploadError::NotFound);
        }
        if let Some(completion) = &session.view.completion {
            return Ok(completion.clone());
        }
        if session.view.state != UploadSessionState::Open {
            return Err(UploadError::InvalidState {
                state: session.view.state,
            });
        }
        if session.view.received_bytes != session.view.expected_length {
            return Err(UploadError::SizeMismatch);
        }
        let node_id = match session.view.target {
            UploadTargetView::CreateFile { node_id, .. }
            | UploadTargetView::ReplaceContent { node_id, .. } => node_id,
        };
        let completion = UploadCompletion {
            session_id,
            node_id,
            file_version_id: FileVersionId::new(),
            object_id: ObjectId::new(),
            object_replica_id: synveil_core::ObjectReplicaId::new(),
            node_revision: Revision::new(1),
            length: session.view.expected_length,
            sha256: session
                .view
                .expected_sha256
                .unwrap_or_else(|| Sha256Digest::from_bytes([7_u8; 32])),
            committed_at: Timestamp::parse("2026-08-24T00:01:00Z").expect("test completion time"),
        };
        session.view.state = UploadSessionState::Committed;
        session.view.completion = Some(completion.clone());
        Ok(completion)
    }

    async fn abort_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
    ) -> Result<UploadSessionView, UploadError> {
        let mut state = self.state.lock().expect("upload state lock");
        let session = state
            .sessions
            .get_mut(&session_id)
            .ok_or(UploadError::NotFound)?;
        if session.view.owner_user_id != owner_user_id {
            return Err(UploadError::NotFound);
        }
        if session.view.state == UploadSessionState::Committed {
            return Ok(session.view.clone());
        }
        session.view.state = UploadSessionState::Aborted;
        Ok(session.view.clone())
    }
}

async fn json_body(response: Response) -> Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("response body must be readable")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response must be JSON")
}

async fn login_cookies(state: &ApiState) -> (String, String) {
    let response = router(state.clone())
        .oneshot(json_request(
            Method::POST,
            "/api/v1/auth/login",
            &format!(
                "{{\"login\":\"{TEST_LOGIN}\",\"login_key\":\"{TEST_LOGIN_KEY}\",\"password\":\"{TEST_PASSWORD}\"}}"
            ),
        ))
        .await
        .expect("login request must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    (
        cookie_value(&response, "synveil_session"),
        cookie_value(&response, "synveil_csrf"),
    )
}

fn add_csrf_header(request: &mut Request<Body>, token: &str) {
    request.headers_mut().insert(
        "x-csrf-token",
        HeaderValue::from_str(token).expect("test CSRF header must be valid"),
    );
}

fn upload_test_state() -> (
    ApiState,
    Arc<TestAuthenticationBackend>,
    Arc<TestUploadBackend>,
) {
    let auth_backend = Arc::new(TestAuthenticationBackend::new());
    let upload_backend = Arc::new(TestUploadBackend::new(8));
    let state = state(true)
        .with_auth_backend(auth_backend.clone())
        .with_upload_backend(upload_backend.clone())
        .with_cookie_config(CookieConfig::production())
        .with_allowed_origin("https://app.example");
    (state, auth_backend, upload_backend)
}

fn download_test_state() -> (
    ApiState,
    Arc<TestAuthenticationBackend>,
    Arc<TestDownloadBackend>,
) {
    let auth_backend = Arc::new(TestAuthenticationBackend::new());
    let download_backend = Arc::new(TestDownloadBackend::new(
        auth_backend.user_id,
        b"0123456789abcdef0123456789abcdef",
    ));
    let state = state(true)
        .with_auth_backend(auth_backend.clone())
        .with_download_backend(download_backend.clone())
        .with_cookie_config(CookieConfig::production())
        .with_allowed_origin("https://app.example");
    (state, auth_backend, download_backend)
}

fn version_test_state() -> (
    ApiState,
    Arc<TestAuthenticationBackend>,
    Arc<TestVersionHistoryBackend>,
    Arc<TestDownloadBackend>,
) {
    let auth_backend = Arc::new(TestAuthenticationBackend::new());
    let version_backend = Arc::new(TestVersionHistoryBackend::new(auth_backend.user_id));
    let download_backend = Arc::new(TestDownloadBackend::with_ids(
        auth_backend.user_id,
        version_backend.file_node_id(),
        version_backend.current_version_id(),
        b"version-history-compatible-content",
    ));
    let state = state(true)
        .with_auth_backend(auth_backend.clone())
        .with_version_history_backend(version_backend.clone())
        .with_download_backend(download_backend.clone())
        .with_cookie_config(CookieConfig::production())
        .with_allowed_origin("https://app.example");
    (state, auth_backend, version_backend, download_backend)
}

fn restore_test_state() -> (
    ApiState,
    Arc<TestAuthenticationBackend>,
    Arc<TestVersionHistoryBackend>,
    Arc<TestVersionRestoreBackend>,
) {
    restore_test_state_with_source_metadata(24, Sha256Digest::from_bytes([0xcd; 32]))
}

fn restore_test_state_with_source_metadata(
    source_byte_length: u64,
    source_sha256: Sha256Digest,
) -> (
    ApiState,
    Arc<TestAuthenticationBackend>,
    Arc<TestVersionHistoryBackend>,
    Arc<TestVersionRestoreBackend>,
) {
    let auth_backend = Arc::new(TestAuthenticationBackend::new());
    let version_backend = Arc::new(TestVersionHistoryBackend::new(auth_backend.user_id));
    let restore_backend = Arc::new(
        TestVersionRestoreBackend::new(
            auth_backend.user_id,
            version_backend.file_node_id(),
            version_backend.previous_version_id(),
        )
        .with_source_metadata(source_byte_length, source_sha256),
    );
    let state = state(true)
        .with_auth_backend(auth_backend.clone())
        .with_version_history_backend(version_backend.clone())
        .with_version_restore_backend(restore_backend.clone())
        .with_etag_key(EtagKey::from_bytes([0x42; 32]))
        .with_cookie_config(CookieConfig::production())
        .with_allowed_origin("https://app.example");
    (state, auth_backend, version_backend, restore_backend)
}

fn authenticated_body_request(
    method: Method,
    uri: &str,
    body: Body,
    session: &str,
    csrf: &str,
) -> Request<Body> {
    let mut request = request(method, uri, body);
    request
        .headers_mut()
        .insert("cookie", cookie_header(session, csrf));
    request
}

fn authenticated_restore_request(
    path: &str,
    session: &str,
    csrf: &str,
    if_match: Option<&str>,
    idempotency_key: Option<&str>,
) -> Request<Body> {
    let mut request = authenticated_request(Method::POST, path, session, csrf);
    mark_same_origin(&mut request);
    add_csrf_header(&mut request, csrf);
    if let Some(if_match) = if_match {
        request.headers_mut().insert(
            "if-match",
            HeaderValue::from_str(if_match).expect("test If-Match header must be valid"),
        );
    }
    if let Some(idempotency_key) = idempotency_key {
        request.headers_mut().insert(
            "idempotency-key",
            HeaderValue::from_str(idempotency_key)
                .expect("test idempotency key header must be valid"),
        );
    }
    request
}

async fn create_test_upload(
    state: &ApiState,
    session: &str,
    csrf: &str,
    expected_bytes: u64,
) -> UploadSessionId {
    let library_id = LibraryId::new();
    let parent_id = NodeId::new();
    let mut create = authenticated_body_request(
        Method::POST,
        "/api/v1/upload-sessions",
        Body::from(format!(
            r#"{{"operation":"CREATE_FILE","library_id":"{library_id}","parent_id":"{parent_id}","name":"report.bin","expected_bytes":"{expected_bytes}"}}"#
        )),
        session,
        csrf,
    );
    create
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    mark_same_origin(&mut create);
    add_csrf_header(&mut create, csrf);
    let response = router(state.clone())
        .oneshot(create)
        .await
        .expect("upload creation request must not fail");
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.headers().get("upload-offset").unwrap(), "0");
    let body = json_body(response).await;
    body["data"]["id"]
        .as_str()
        .expect("upload response must contain an ID")
        .parse()
        .expect("upload response ID must be canonical")
}

fn response_request_id(response: &Response) -> String {
    response
        .headers()
        .get("x-request-id")
        .expect("response must include a request ID")
        .to_str()
        .expect("request ID must be valid ASCII")
        .to_owned()
}

async fn response_bytes(response: Response) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .expect("response body must be readable")
        .to_bytes()
        .to_vec()
}

#[tokio::test]
async fn version_history_requires_authentication_and_does_not_require_csrf() {
    let (state, _auth_backend, version_backend, _download_backend) = version_test_state();
    for path in [
        format!("/api/v1/nodes/{}/versions", version_backend.file_node_id()),
        format!("/api/v1/versions/{}", version_backend.current_version_id()),
    ] {
        let response = router(state.clone())
            .oneshot(request(Method::GET, &path, Body::empty()))
            .await
            .expect("version metadata request must not fail");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            json_body(response).await["error"]["code"],
            "authentication_failed"
        );
    }
}

#[tokio::test]
async fn version_history_is_bounded_owner_scoped_and_download_compatible() {
    let (state, _auth_backend, version_backend, _download_backend) = version_test_state();
    let (session, csrf) = login_cookies(&state).await;
    let list_path = format!(
        "/api/v1/nodes/{}/versions?limit=1",
        version_backend.file_node_id()
    );
    let response = router(state.clone())
        .oneshot(authenticated_request(
            Method::GET,
            &list_path,
            &session,
            &csrf,
        ))
        .await
        .expect("version list request must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let body = json_body(response).await;
    assert_eq!(body["data"].as_array().expect("version page").len(), 1);
    assert_eq!(
        body["data"][0]["id"],
        version_backend.current_version_id().to_string()
    );
    assert_eq!(body["data"][0]["type"], "file_version");
    assert_eq!(
        body["data"][0]["attributes"]["node_id"],
        version_backend.file_node_id().to_string()
    );
    assert_eq!(body["data"][0]["attributes"]["byte_length"], "42");
    assert_eq!(
        body["data"][0]["attributes"]["sha256"],
        format!("sha256:{}", "ab".repeat(32))
    );
    assert_eq!(body["data"][0]["attributes"]["is_current"], true);
    let attributes = body["data"][0]["attributes"]
        .as_object()
        .expect("version attributes must be an object");
    for forbidden in [
        "object_id",
        "object_key",
        "storage_key",
        "replica_id",
        "backend_kind",
        "staging_handle",
        "physical_path",
    ] {
        assert!(
            !attributes.contains_key(forbidden),
            "field leaked: {forbidden}"
        );
    }
    let cursor = body["page"]["next_cursor"]
        .as_str()
        .expect("first page must have a cursor")
        .to_owned();
    assert!(!cursor.contains(&version_backend.file_node_id().to_string()));

    let continuation_path = format!(
        "/api/v1/nodes/{}/versions?cursor={cursor}",
        version_backend.file_node_id()
    );
    let response = router(state.clone())
        .oneshot(authenticated_request(
            Method::GET,
            &continuation_path,
            &session,
            &csrf,
        ))
        .await
        .expect("version continuation request must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["page"]["has_more"], false);
    assert_eq!(
        body["data"][0]["id"],
        version_backend.previous_version_id().to_string()
    );
    assert_eq!(body["data"][0]["attributes"]["is_current"], false);

    let direct_path = format!("/api/v1/versions/{}", version_backend.current_version_id());
    let response = router(state.clone())
        .oneshot(authenticated_request(
            Method::GET,
            &direct_path,
            &session,
            &csrf,
        ))
        .await
        .expect("direct version metadata request must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(
        json_body(response).await["data"]["id"],
        version_backend.current_version_id().to_string()
    );

    let historical_download_path = format!(
        "/api/v1/versions/{}/content",
        version_backend.current_version_id()
    );
    let response = router(state)
        .oneshot(authenticated_request(
            Method::GET,
            &historical_download_path,
            &session,
            &csrf,
        ))
        .await
        .expect("historical content compatibility request must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response_bytes(response).await,
        b"version-history-compatible-content"
    );
}

#[tokio::test]
async fn version_history_rejects_invalid_state_cursor_and_cross_owner_access() {
    let (state, _auth_backend, version_backend, _download_backend) = version_test_state();
    let (session, csrf) = login_cookies(&state).await;

    let invalid_limit = format!(
        "/api/v1/nodes/{}/versions?limit=101",
        version_backend.file_node_id()
    );
    let mut invalid_limit_request =
        authenticated_request(Method::GET, &invalid_limit, &session, &csrf);
    invalid_limit_request.headers_mut().insert(
        "x-request-id",
        HeaderValue::from_static("version-error-1234"),
    );
    let response = router(state.clone())
        .oneshot(invalid_limit_request)
        .await
        .expect("invalid version limit request must not fail");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response_request_id(&response), "version-error-1234");
    assert_eq!(
        json_body(response).await["error"]["code"],
        "invalid_request"
    );

    for (path, status, code) in [
        (
            format!(
                "/api/v1/nodes/{}/versions?cursor=invalid",
                version_backend.file_node_id()
            ),
            StatusCode::BAD_REQUEST,
            "invalid_cursor",
        ),
        (
            format!(
                "/api/v1/nodes/{}/versions",
                version_backend.directory_node_id()
            ),
            StatusCode::CONFLICT,
            "invalid_state",
        ),
        (
            format!(
                "/api/v1/nodes/{}/versions",
                version_backend.trashed_node_id()
            ),
            StatusCode::NOT_FOUND,
            "not_found",
        ),
    ] {
        let response = router(state.clone())
            .oneshot(authenticated_request(Method::GET, &path, &session, &csrf))
            .await
            .expect("version negative request must not fail");
        assert_eq!(response.status(), status);
        assert_eq!(json_body(response).await["error"]["code"], code);
    }

    let cross_node_cursor = format!(
        "/api/v1/nodes/{}/versions?cursor=v1.version.opaque-page-cursor",
        version_backend.directory_node_id()
    );
    let response = router(state.clone())
        .oneshot(authenticated_request(
            Method::GET,
            &cross_node_cursor,
            &session,
            &csrf,
        ))
        .await
        .expect("cross-node cursor request must not fail");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"]["code"], "invalid_cursor");

    let other_auth = Arc::new(TestAuthenticationBackend::new());
    let other_state = state.with_auth_backend(other_auth.clone());
    let (other_session, other_csrf) = login_cookies(&other_state).await;
    let response = router(other_state)
        .oneshot(authenticated_request(
            Method::GET,
            &format!("/api/v1/versions/{}", version_backend.current_version_id()),
            &other_session,
            &other_csrf,
        ))
        .await
        .expect("cross-owner version request must not fail");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_body(response).await["error"]["code"], "not_found");
}

#[tokio::test]
async fn version_restore_requires_authentication_csrf_if_match_and_idempotency_key() {
    let (state, _auth_backend, version_backend, _restore_backend) = restore_test_state();
    let path = format!(
        "/api/v1/nodes/{}/versions/{}/restore",
        version_backend.file_node_id(),
        version_backend.previous_version_id()
    );

    let response = router(state.clone())
        .oneshot(request(Method::POST, &path, Body::empty()))
        .await
        .expect("unauthenticated restore request must not fail");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        json_body(response).await["error"]["code"],
        "authentication_failed"
    );

    let (session, csrf) = login_cookies(&state).await;
    let response = router(state.clone())
        .oneshot(authenticated_request(Method::POST, &path, &session, &csrf))
        .await
        .expect("CSRF-protected restore request must not fail");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        json_body(response).await["error"]["code"],
        "permission_denied"
    );

    let response = router(state.clone())
        .oneshot(authenticated_restore_request(
            &path,
            &session,
            &csrf,
            None,
            Some("restore-key-001"),
        ))
        .await
        .expect("missing If-Match restore request must not fail");
    assert_eq!(response.status(), StatusCode::PRECONDITION_REQUIRED);
    assert_eq!(
        json_body(response).await["error"]["code"],
        "precondition_required"
    );

    let response = router(state.clone())
        .oneshot(authenticated_restore_request(
            &path,
            &session,
            &csrf,
            Some("\"not-an-etag\""),
            Some("restore-key-001"),
        ))
        .await
        .expect("invalid If-Match restore request must not fail");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(response).await["error"]["code"],
        "version_conflict"
    );

    let etag = state
        .etag_key()
        .issue(version_backend.file_node_id(), Revision::new(0));
    let response = router(state)
        .oneshot(authenticated_restore_request(
            &path,
            &session,
            &csrf,
            Some(&etag),
            None,
        ))
        .await
        .expect("missing idempotency-key restore request must not fail");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(response).await["error"]["code"],
        "invalid_request"
    );
}

#[tokio::test]
async fn version_restore_creates_one_new_version_and_replays_the_same_outcome() {
    let (state, auth_backend, version_backend, restore_backend) = restore_test_state();
    let (session, csrf) = login_cookies(&state).await;
    let node_id = version_backend.file_node_id();
    let source_version_id = version_backend.previous_version_id();
    let path = format!("/api/v1/nodes/{node_id}/versions/{source_version_id}/restore");
    let old_etag = state.etag_key().issue(node_id, Revision::new(0));
    let mut first_request = authenticated_restore_request(
        &path,
        &session,
        &csrf,
        Some(&old_etag),
        Some("restore-key-001"),
    );
    first_request.headers_mut().insert(
        "x-request-id",
        HeaderValue::from_static("restore-success-1"),
    );
    let response = router(state.clone())
        .oneshot(first_request)
        .await
        .expect("restore request must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let expected_etag = state.etag_key().issue(node_id, Revision::new(1));
    assert_eq!(response.headers()["etag"], expected_etag);
    assert_eq!(response_request_id(&response), "restore-success-1");
    let body = json_body(response).await;
    let restored_version_id = body["data"]["id"]
        .as_str()
        .expect("restore response must contain a version ID");
    assert_ne!(restored_version_id, source_version_id.to_string());
    assert_eq!(body["data"]["type"], "file_version");
    assert_eq!(body["data"]["attributes"]["node_id"], node_id.to_string());
    assert_eq!(body["data"]["attributes"]["byte_length"], "24");
    assert_eq!(
        body["data"]["attributes"]["sha256"],
        format!("sha256:{}", "cd".repeat(32))
    );
    assert_eq!(body["data"]["attributes"]["is_current"], true);
    assert_eq!(body["node"]["id"], node_id.to_string());
    assert_eq!(body["node"]["revision"], "1");
    assert_eq!(body["meta"]["request_id"], "restore-success-1");
    for forbidden in [
        "object_id",
        "object_key",
        "storage_key",
        "replica_id",
        "backend_kind",
        "staging_handle",
        "physical_path",
    ] {
        assert!(
            !body.to_string().contains(forbidden),
            "field leaked: {forbidden}"
        );
    }
    assert_eq!(restore_backend.applied_calls(), 1);
    assert_ne!(restore_backend.current_version_id(), source_version_id);
    assert_eq!(restore_backend.current_revision(), Revision::new(1));

    let response = router(state.clone())
        .oneshot(authenticated_restore_request(
            &path,
            &session,
            &csrf,
            Some(&old_etag),
            Some("restore-key-001"),
        ))
        .await
        .expect("replayed restore request must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["etag"], expected_etag);
    let replay_body = json_body(response).await;
    assert_eq!(replay_body["data"]["id"], restored_version_id);
    assert_eq!(replay_body["node"]["revision"], "1");
    assert_eq!(restore_backend.applied_calls(), 1);

    let download_state =
        state
            .clone()
            .with_download_backend(Arc::new(TestRestoreDownloadBackend {
                current: Arc::new(TestDownloadBackend::with_ids(
                    auth_backend.user_id,
                    node_id,
                    restore_backend.current_version_id(),
                    b"restored-content",
                )),
                historical: Arc::new(TestDownloadBackend::with_ids(
                    auth_backend.user_id,
                    node_id,
                    source_version_id,
                    b"historical-content",
                )),
            }));
    let response = router(download_state.clone())
        .oneshot(authenticated_request(
            Method::GET,
            &format!("/api/v1/versions/{source_version_id}/content"),
            &session,
            &csrf,
        ))
        .await
        .expect("historical restore source download must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_bytes(response).await, b"historical-content");
    let response = router(download_state)
        .oneshot(authenticated_request(
            Method::GET,
            &format!("/api/v1/nodes/{node_id}/content"),
            &session,
            &csrf,
        ))
        .await
        .expect("restored current download must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_bytes(response).await, b"restored-content");

    let mut stale_request = authenticated_restore_request(
        &path,
        &session,
        &csrf,
        Some(&old_etag),
        Some("restore-key-002"),
    );
    stale_request
        .headers_mut()
        .insert("x-request-id", HeaderValue::from_static("restore-stale-1"));
    let response = router(state.clone())
        .oneshot(stale_request)
        .await
        .expect("stale restore request must not fail");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(response_request_id(&response), "restore-stale-1");
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "version_conflict");
    assert_eq!(body["error"]["details"]["current_revision"], "1");
    assert_eq!(body["error"]["details"]["etag"], expected_etag);
    assert_eq!(restore_backend.applied_calls(), 1);

    let response = router(state)
        .oneshot(authenticated_restore_request(
            &path,
            &session,
            &csrf,
            Some(&expected_etag),
            Some("restore-key-001"),
        ))
        .await
        .expect("idempotency conflict request must not fail");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(response).await["error"]["code"],
        "idempotency_conflict"
    );
    assert_eq!(restore_backend.applied_calls(), 1);
}

#[tokio::test]
async fn version_restore_supports_zero_byte_historical_versions() {
    let zero_hash = Sha256Digest::from_bytes([0; 32]);
    let (state, _auth_backend, version_backend, restore_backend) =
        restore_test_state_with_source_metadata(0, zero_hash);
    let (session, csrf) = login_cookies(&state).await;
    let node_id = version_backend.file_node_id();
    let source_version_id = version_backend.previous_version_id();
    let path = format!("/api/v1/nodes/{node_id}/versions/{source_version_id}/restore");
    let etag = state.etag_key().issue(node_id, Revision::new(0));

    let response = router(state)
        .oneshot(authenticated_restore_request(
            &path,
            &session,
            &csrf,
            Some(&etag),
            Some("restore-zero-001"),
        ))
        .await
        .expect("zero-byte restore request must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["data"]["attributes"]["byte_length"], "0");
    assert_eq!(
        body["data"]["attributes"]["sha256"],
        format!("sha256:{}", "00".repeat(32))
    );
    assert_eq!(body["data"]["attributes"]["is_current"], true);
    assert_eq!(restore_backend.applied_calls(), 1);
}

#[tokio::test]
async fn version_restore_conceals_cross_owner_and_cross_node_access_and_rejects_invalid_states() {
    let (state, _auth_backend, version_backend, restore_backend) = restore_test_state();
    let (session, csrf) = login_cookies(&state).await;
    let node_id = version_backend.file_node_id();
    let source_version_id = version_backend.previous_version_id();

    for (node_id, source_version_id, status, code) in [
        (
            NodeId::new(),
            source_version_id,
            StatusCode::NOT_FOUND,
            "not_found",
        ),
        (
            restore_backend.directory_node_id(),
            source_version_id,
            StatusCode::CONFLICT,
            "invalid_state",
        ),
        (
            restore_backend.trashed_node_id(),
            source_version_id,
            StatusCode::NOT_FOUND,
            "not_found",
        ),
        (
            node_id,
            FileVersionId::new(),
            StatusCode::NOT_FOUND,
            "not_found",
        ),
    ] {
        let path = format!("/api/v1/nodes/{node_id}/versions/{source_version_id}/restore");
        let target_etag = state.etag_key().issue(node_id, Revision::new(0));
        let response = router(state.clone())
            .oneshot(authenticated_restore_request(
                &path,
                &session,
                &csrf,
                Some(&target_etag),
                Some("restore-negative-1"),
            ))
            .await
            .expect("restore negative request must not fail");
        assert_eq!(response.status(), status);
        assert_eq!(json_body(response).await["error"]["code"], code);
    }

    restore_backend.set_current_version_id(source_version_id);
    let path = format!("/api/v1/nodes/{node_id}/versions/{source_version_id}/restore");
    let etag = state.etag_key().issue(node_id, Revision::new(0));
    let response = router(state.clone())
        .oneshot(authenticated_restore_request(
            &path,
            &session,
            &csrf,
            Some(&etag),
            Some("restore-current-1"),
        ))
        .await
        .expect("current-version restore request must not fail");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(response).await["error"]["code"],
        "invalid_request"
    );

    let other_auth = Arc::new(TestAuthenticationBackend::new());
    let other_state = state.with_auth_backend(other_auth);
    let (other_session, other_csrf) = login_cookies(&other_state).await;
    let response = router(other_state)
        .oneshot(authenticated_restore_request(
            &path,
            &other_session,
            &other_csrf,
            Some(&etag),
            Some("restore-owner-1"),
        ))
        .await
        .expect("cross-owner restore request must not fail");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_body(response).await["error"]["code"], "not_found");
}

#[tokio::test]
async fn download_requires_authentication_and_does_not_require_csrf() {
    let (state, _auth_backend, backend) = download_test_state();
    let response = router(state)
        .oneshot(request(
            Method::GET,
            &format!("/api/v1/nodes/{}/content", backend.node_id()),
            Body::empty(),
        ))
        .await
        .expect("download request must not fail");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        json_body(response).await["error"]["code"],
        "authentication_failed"
    );
    assert_eq!(backend.open_calls(), 0);
    assert_eq!(backend.range_calls(), 0);

    let (state, _auth_backend, backend) = download_test_state();
    let (session, csrf) = login_cookies(&state).await;
    let response = router(state)
        .oneshot(authenticated_request(
            Method::GET,
            &format!("/api/v1/nodes/{}/content", backend.node_id()),
            &session,
            &csrf,
        ))
        .await
        .expect("authenticated download request must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response_bytes(response).await,
        b"0123456789abcdef0123456789abcdef"
    );
}

#[tokio::test]
async fn full_download_returns_safe_headers_and_a_bounded_multi_frame_body() {
    let (state, _auth_backend, backend) = download_test_state();
    let (session, csrf) = login_cookies(&state).await;
    let mut response = router(state)
        .oneshot(authenticated_request(
            Method::GET,
            &format!("/api/v1/nodes/{}/content", backend.node_id()),
            &session,
            &csrf,
        ))
        .await
        .expect("full download request must not fail");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[CONTENT_LENGTH], "32");
    assert_eq!(
        response.headers()["content-type"],
        "application/octet-stream"
    );
    assert_eq!(response.headers()["accept-ranges"], "bytes");
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert!(
        response.headers()["etag"]
            .to_str()
            .unwrap()
            .starts_with("\"sha256:")
    );
    assert!(
        response.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment; filename=\"download\"")
    );
    assert!(response.headers().contains_key("x-request-id"));

    let mut frames = 0;
    let mut body_bytes = Vec::new();
    while let Some(frame) = response.body_mut().frame().await {
        let frame = frame.expect("download body frame must be readable");
        if let Ok(data) = frame.into_data() {
            frames += 1;
            body_bytes.extend_from_slice(&data);
        }
    }
    assert!(
        frames > 1,
        "the response must preserve multiple stream frames"
    );
    assert_eq!(body_bytes, b"0123456789abcdef0123456789abcdef");
    assert_eq!(backend.open_calls(), 1);
}

#[tokio::test]
async fn single_range_download_returns_exact_partial_semantics() {
    let (state, _auth_backend, backend) = download_test_state();
    let (session, csrf) = login_cookies(&state).await;
    let mut request = authenticated_request(
        Method::GET,
        &format!("/api/v1/nodes/{}/content", backend.node_id()),
        &session,
        &csrf,
    );
    request
        .headers_mut()
        .insert("range", HeaderValue::from_static("bytes=5-10"));
    let response = router(state)
        .oneshot(request)
        .await
        .expect("range download request must not fail");

    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()[CONTENT_RANGE], "bytes 5-10/32");
    assert_eq!(response.headers()[CONTENT_LENGTH], "6");
    assert_eq!(response_bytes(response).await, b"56789a");
    assert_eq!(backend.range_calls(), 1);
    assert_eq!(backend.open_calls(), 0);
}

#[tokio::test]
async fn open_ended_and_suffix_ranges_are_supported() {
    for (range, expected, content_range) in [
        ("bytes=27-", b"bcdef".as_slice(), "bytes 27-31/32"),
        ("bytes=-4", b"cdef".as_slice(), "bytes 28-31/32"),
    ] {
        let (state, _auth_backend, backend) = download_test_state();
        let (session, csrf) = login_cookies(&state).await;
        let mut request = authenticated_request(
            Method::GET,
            &format!("/api/v1/nodes/{}/content", backend.node_id()),
            &session,
            &csrf,
        );
        request
            .headers_mut()
            .insert("range", HeaderValue::from_str(range).unwrap());
        let response = router(state)
            .oneshot(request)
            .await
            .expect("range download request must not fail");
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[CONTENT_RANGE], content_range);
        assert_eq!(response_bytes(response).await, expected);
    }
}

#[tokio::test]
async fn invalid_and_multiple_ranges_return_416_without_opening_storage() {
    for range in ["bytes=999-1000", "bytes=0-1,2-3", "items=0-1", "bytes=-0"] {
        let (state, _auth_backend, backend) = download_test_state();
        let (session, csrf) = login_cookies(&state).await;
        let mut request = authenticated_request(
            Method::GET,
            &format!("/api/v1/nodes/{}/content", backend.node_id()),
            &session,
            &csrf,
        );
        request
            .headers_mut()
            .insert("range", HeaderValue::from_str(range).unwrap());
        let response = router(state)
            .oneshot(request)
            .await
            .expect("invalid range request must not fail");
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(response.headers()[CONTENT_RANGE], "bytes */32");
        let request_id = response_request_id(&response);
        let body = json_body(response).await;
        assert_eq!(body["error"]["code"], "invalid_range");
        assert_eq!(body["error"]["request_id"], request_id);
        assert_eq!(backend.open_calls(), 0);
        assert_eq!(backend.range_calls(), 0);
    }
}

#[tokio::test]
async fn if_none_match_returns_304_without_streaming_bytes() {
    let (state, _auth_backend, backend) = download_test_state();
    let (session, csrf) = login_cookies(&state).await;
    let path = format!("/api/v1/nodes/{}/content", backend.node_id());
    let response = router(state.clone())
        .oneshot(authenticated_request(Method::GET, &path, &session, &csrf))
        .await
        .expect("initial download request must not fail");
    let etag = response.headers()["etag"].clone();
    let _ = response_bytes(response).await;
    assert_eq!(backend.open_calls(), 1);

    let mut request = authenticated_request(Method::GET, &path, &session, &csrf);
    request.headers_mut().insert("if-none-match", etag);
    let response = router(state)
        .oneshot(request)
        .await
        .expect("conditional download request must not fail");
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(response_bytes(response).await, Vec::<u8>::new());
    assert_eq!(backend.open_calls(), 1);
}

#[tokio::test]
async fn historical_download_is_owner_scoped_and_supports_ranges() {
    let (state, _auth_backend, backend) = download_test_state();
    let (session, csrf) = login_cookies(&state).await;
    let path = format!("/api/v1/versions/{}/content", backend.file_version_id());
    let mut request = authenticated_request(Method::GET, &path, &session, &csrf);
    request
        .headers_mut()
        .insert("range", HeaderValue::from_static("bytes=16-"));
    let response = router(state)
        .oneshot(request)
        .await
        .expect("historical range request must not fail");
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()[CONTENT_RANGE], "bytes 16-31/32");
    assert_eq!(response_bytes(response).await, b"0123456789abcdef");

    let (state, _auth_backend, backend) = download_test_state();
    let (session, csrf) = login_cookies(&state).await;
    let missing_version = FileVersionId::new();
    let response = router(state)
        .oneshot(authenticated_request(
            Method::GET,
            &format!("/api/v1/versions/{missing_version}/content"),
            &session,
            &csrf,
        ))
        .await
        .expect("missing historical request must not fail");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_body(response).await["error"]["code"], "not_found");
    assert_eq!(backend.open_calls(), 0);
}

#[tokio::test]
async fn api_downloads_use_the_content_service_and_local_object_store() {
    let auth_backend = Arc::new(TestAuthenticationBackend::new());
    let owner_user_id = auth_backend.user_id;
    let node_id = NodeId::new();
    let file_version_id = FileVersionId::new();
    let object_id = ObjectId::new();
    let bytes = b"local-object-store-content".to_vec();
    let digest = Sha256::digest(&bytes);
    let sha256 = Sha256Digest::try_from(digest.as_slice()).expect("SHA-256 is valid");
    let key = ObjectKey::new(format!("objects/v1/{object_id}")).expect("object key is valid");
    let root =
        TestDownloadRoot(std::env::temp_dir().join(format!("synveil-api-download-{node_id}")));
    let object_store: Arc<dyn ObjectStore> =
        Arc::new(open_local_object_store(&root.0).expect("local object store must open"));
    object_store
        .put(
            PutRequest::new(
                key.clone(),
                boxed_stream(stream::iter([Ok(Bytes::from(bytes.clone()))])),
            )
            .with_integrity(
                IntegrityExpectation::none()
                    .with_length(bytes.len() as u64)
                    .with_sha256(sha256),
            ),
        )
        .await
        .expect("test object must be durable");
    let metadata = TestContentReadMetadata {
        owner_user_id,
        node_id,
        file_version_id,
        content: AuthorizedContent::new(
            node_id,
            file_version_id,
            object_id,
            bytes.len() as u64,
            sha256,
            Revision::new(4),
            Timestamp::parse("2026-08-24T00:00:00Z").expect("valid test time"),
            key.as_str(),
        ),
    };
    let content_service = ContentReadApplicationService::new(Arc::new(metadata), object_store);
    let state = state(true)
        .with_auth_backend(auth_backend)
        .with_download_backend(Arc::new(content_service))
        .with_cookie_config(CookieConfig::production())
        .with_allowed_origin("https://app.example");
    let (session, csrf) = login_cookies(&state).await;
    let path = format!("/api/v1/nodes/{node_id}/content");
    let response = router(state.clone())
        .oneshot(authenticated_request(Method::GET, &path, &session, &csrf))
        .await
        .expect("service-backed full download must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_bytes(response).await, bytes);

    let mut request = authenticated_request(Method::GET, &path, &session, &csrf);
    request
        .headers_mut()
        .insert("range", HeaderValue::from_static("bytes=6-11"));
    let response = router(state)
        .oneshot(request)
        .await
        .expect("service-backed range download must not fail");
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()[CONTENT_RANGE], "bytes 6-11/26");
    assert_eq!(response_bytes(response).await, b"object");
}

#[tokio::test]
async fn content_errors_are_safe_and_cross_owner_nodes_are_concealed() {
    let (state, _auth_backend, _backend) = download_test_state();
    let (session, csrf) = login_cookies(&state).await;
    let mut request = authenticated_request(
        Method::GET,
        &format!("/api/v1/nodes/{}/content", NodeId::new()),
        &session,
        &csrf,
    );
    request
        .headers_mut()
        .insert("range", HeaderValue::from_static("bytes=0-1"));
    let response = router(state)
        .oneshot(request)
        .await
        .expect("cross-owner request must not fail");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_body(response).await["error"]["code"], "not_found");

    let (state, _auth_backend, backend) = download_test_state();
    backend.set_current_error(ContentReadError::NotAFile);
    let (session, csrf) = login_cookies(&state).await;
    let response = router(state)
        .oneshot(authenticated_request(
            Method::GET,
            &format!("/api/v1/nodes/{}/content", backend.node_id()),
            &session,
            &csrf,
        ))
        .await
        .expect("directory request must not fail");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(response).await["error"]["code"], "invalid_state");

    let (state, _auth_backend, backend) = download_test_state();
    backend.set_current_error(ContentReadError::ContentUnavailable);
    let (session, csrf) = login_cookies(&state).await;
    let response = router(state)
        .oneshot(authenticated_request(
            Method::GET,
            &format!("/api/v1/nodes/{}/content", backend.node_id()),
            &session,
            &csrf,
        ))
        .await
        .expect("unavailable content request must not fail");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        json_body(response).await["error"]["code"],
        "storage_unavailable"
    );

    let (state, _auth_backend, backend) = download_test_state();
    backend.set_current_error(ContentReadError::IntegrityMismatch);
    let (session, csrf) = login_cookies(&state).await;
    let response = router(state)
        .oneshot(authenticated_request(
            Method::GET,
            &format!("/api/v1/nodes/{}/content", backend.node_id()),
            &session,
            &csrf,
        ))
        .await
        .expect("integrity failure request must not fail");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json_body(response).await["error"]["code"], "internal_error");
}

#[tokio::test]
async fn content_disposition_uses_encoded_logical_filename() {
    let (state, auth_backend, backend) = download_test_state();
    let file_metadata = Arc::new(TestFileMetadataBackend::new(auth_backend.user_id));
    file_metadata.add_file(backend.node_id(), "ảnh\r\nname/report.txt");
    let state = state.with_file_metadata_backend(file_metadata);
    let (session, csrf) = login_cookies(&state).await;
    let response = router(state)
        .oneshot(authenticated_request(
            Method::GET,
            &format!("/api/v1/nodes/{}/content", backend.node_id()),
            &session,
            &csrf,
        ))
        .await
        .expect("named download request must not fail");
    let disposition = response.headers()["content-disposition"].to_str().unwrap();
    assert_eq!(
        disposition,
        "attachment; filename=\"download\"; filename*=UTF-8''%E1%BA%A3nh%0D%0Aname%2Freport.txt"
    );
    assert!(!disposition.contains('\r'));
    assert!(!disposition.contains('\n'));
}

#[tokio::test]
async fn live_response_is_unconditional_and_correlated() {
    for path in ["/live", "/health/live"] {
        let response = router(state(false))
            .oneshot(request(Method::GET, path, Body::empty()))
            .await
            .expect("router must not fail");

        assert_eq!(response.status(), StatusCode::OK);
        let request_id = response_request_id(&response);
        assert!(
            RequestId::from_headers(&HeaderMap::from_iter([(
                "x-request-id".parse().unwrap(),
                HeaderValue::from_str(&request_id).unwrap(),
            )]))
            .is_some()
        );
        assert_eq!(json_body(response).await["status"], "live");
    }
}

#[tokio::test]
async fn readiness_is_distinct_from_liveness() {
    let response = router(state(false))
        .oneshot(request(Method::GET, "/ready", Body::empty()))
        .await
        .expect("router must not fail");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let request_id = response_request_id(&response);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "internal_dependency_unavailable");
    assert_eq!(body["error"]["request_id"], request_id);
    assert_eq!(body["error"]["retryable"], true);

    let response = router(state(true))
        .oneshot(request(Method::GET, "/ready", Body::empty()))
        .await
        .expect("router must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["status"], "ready");
}

#[tokio::test]
async fn request_id_is_propagated_only_when_safe() {
    let mut propagated = request(Method::GET, "/missing", Body::empty());
    propagated
        .headers_mut()
        .insert("x-request-id", HeaderValue::from_static("client.req-1234"));
    let response = router(state(true))
        .oneshot(propagated)
        .await
        .expect("router must not fail");
    assert_eq!(response_request_id(&response), "client.req-1234");
    assert_eq!(
        json_body(response).await["error"]["request_id"],
        "client.req-1234"
    );

    let mut unsafe_hint = request(Method::GET, "/missing", Body::empty());
    unsafe_hint
        .headers_mut()
        .insert("x-request-id", HeaderValue::from_static("short"));
    let response = router(state(true))
        .oneshot(unsafe_hint)
        .await
        .expect("router must not fail");
    let request_id = response_request_id(&response);
    assert_ne!(request_id, "short");
    assert_eq!(json_body(response).await["error"]["request_id"], request_id);
}

#[tokio::test]
async fn not_found_uses_the_safe_error_envelope() {
    let mut request = request(Method::GET, "/not-a-route", Body::empty());
    request
        .headers_mut()
        .insert("x-request-id", HeaderValue::from_static("not-found-1"));
    let response = router(state(true))
        .oneshot(request)
        .await
        .expect("router must not fail");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "not_found");
    assert_eq!(body["error"]["retryable"], false);
    assert_eq!(body["error"]["request_id"], "not-found-1");
    assert!(body["error"]["message"].as_str().unwrap().len() <= 512);
}

#[tokio::test]
async fn detailed_health_is_fail_closed_until_authorized() {
    let response = router(state(true))
        .oneshot(request(Method::GET, "/api/v1/system/health", Body::empty()))
        .await
        .expect("router must not fail");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        json_body(response).await["error"]["code"],
        "authentication_failed"
    );

    let authorized_state =
        state(true).with_system_health_authorizer(Arc::new(TestHealthAuthorizer));
    let mut request = request(Method::GET, "/api/v1/system/health", Body::empty());
    request
        .headers_mut()
        .insert("x-test-health-access", HeaderValue::from_static("allow"));
    request
        .headers_mut()
        .insert("x-request-id", HeaderValue::from_static("health-1234"));
    let response = router(authorized_state)
        .oneshot(request)
        .await
        .expect("router must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["data"]["liveness"], true);
    assert_eq!(body["data"]["readiness"], true);
    assert!(body["data"]["components"].is_array());
    assert_eq!(body["meta"]["request_id"], "health-1234");
    assert!(body.to_string().find("/").is_none());
}

#[tokio::test]
async fn body_limit_returns_the_public_error_envelope() {
    let mut request = request(Method::POST, "/api/v1/system/health", Body::from("12345"));
    request
        .headers_mut()
        .insert(CONTENT_LENGTH, HeaderValue::from_static("5"));
    let response = router(state(true).with_body_limit(4))
        .oneshot(request)
        .await
        .expect("router must not fail");

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "payload_too_large");
    assert_eq!(body["error"]["retryable"], false);
}

#[tokio::test]
async fn bootstrap_status_is_safe_and_not_cacheable() {
    let response = router(auth_state())
        .oneshot(request(
            Method::GET,
            "/api/v1/system/bootstrap-status",
            Body::empty(),
        ))
        .await
        .expect("bootstrap status request must not fail");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let request_id = response_request_id(&response);
    let body = json_body(response).await;
    assert_eq!(body["data"]["setup_required"], true);
    assert_eq!(body["meta"]["request_id"], request_id);
    assert!(body.to_string().find("password").is_none());
    assert!(body.to_string().find("alice").is_none());
}

#[tokio::test]
async fn bootstrap_creates_first_admin_through_the_auth_boundary_and_closes() {
    let backend = Arc::new(TestAuthenticationBackend::new());
    let state = auth_state().with_auth_backend(backend.clone());
    let mut create = json_request(
        Method::POST,
        "/api/v1/bootstrap/admin",
        &format!(
            "{{\"login\":\"{TEST_LOGIN}\",\"login_key\":\"{TEST_LOGIN_KEY}\",\"password\":\"{TEST_PASSWORD}\"}}"
        ),
    );
    mark_same_origin(&mut create);
    create.headers_mut().insert(
        "x-request-id",
        HeaderValue::from_static("bootstrap-create-1"),
    );
    let response = router(state.clone())
        .oneshot(create)
        .await
        .expect("bootstrap create request must not fail");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    assert_eq!(response_request_id(&response), "bootstrap-create-1");
    assert!(
        response
            .headers()
            .get_all("set-cookie")
            .iter()
            .next()
            .is_none()
    );
    let body = json_body(response).await;
    assert_eq!(body["data"]["setup_required"], false);
    assert_eq!(body["meta"]["request_id"], "bootstrap-create-1");
    assert!(body.to_string().find(TEST_PASSWORD).is_none());
    assert!(body.to_string().find(TEST_LOGIN).is_none());
    assert_eq!(backend.bootstrap_calls.load(Ordering::Relaxed), 1);

    let status = router(state.clone())
        .oneshot(request(
            Method::GET,
            "/api/v1/system/bootstrap-status",
            Body::empty(),
        ))
        .await
        .expect("bootstrap status request must not fail");
    assert_eq!(status.status(), StatusCode::OK);
    assert_eq!(json_body(status).await["data"]["setup_required"], false);

    let mut repeated = json_request(
        Method::POST,
        "/api/v1/bootstrap/admin",
        &format!(
            "{{\"login\":\"{TEST_LOGIN}\",\"login_key\":\"{TEST_LOGIN_KEY}\",\"password\":\"{TEST_PASSWORD}\"}}"
        ),
    );
    mark_same_origin(&mut repeated);
    let repeated = router(state)
        .oneshot(repeated)
        .await
        .expect("repeated bootstrap request must not fail");
    assert_eq!(repeated.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(repeated).await["error"]["code"],
        "bootstrap_closed"
    );
    assert_eq!(backend.bootstrap_calls.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn bootstrap_rejects_cross_site_requests_unknown_fields_and_large_bodies() {
    let state = auth_state();
    let mut cross_site = json_request(
        Method::POST,
        "/api/v1/bootstrap/admin",
        "{\"login\":\"alice\",\"login_key\":\"alice-key\",\"password\":\"secret\"}",
    );
    cross_site
        .headers_mut()
        .insert("origin", HeaderValue::from_static("https://evil.example"));
    cross_site
        .headers_mut()
        .insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
    let cross_site = router(state.clone())
        .oneshot(cross_site)
        .await
        .expect("cross-site bootstrap request must not fail");
    assert_eq!(cross_site.status(), StatusCode::FORBIDDEN);

    let unknown = router(state.clone())
        .oneshot(json_request(
            Method::POST,
            "/api/v1/bootstrap/admin",
            "{\"login\":\"alice\",\"login_key\":\"alice-key\",\"password\":\"secret\",\"display_name\":\"Alice\"}",
        ))
        .await
        .expect("unknown-field bootstrap request must not fail");
    assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(unknown).await["error"]["code"], "invalid_request");

    let mut large = request(
        Method::POST,
        "/api/v1/bootstrap/admin",
        Body::from(vec![b'x'; BOOTSTRAP_BODY_LIMIT_BYTES + 1]),
    );
    large
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    large.headers_mut().insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&(BOOTSTRAP_BODY_LIMIT_BYTES + 1).to_string()).unwrap(),
    );
    let large = router(state)
        .oneshot(large)
        .await
        .expect("large bootstrap request must not fail");
    assert_eq!(large.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(json_body(large).await["error"]["code"], "payload_too_large");
}

#[test]
fn core_errors_map_to_http_only_at_the_api_boundary() {
    assert_eq!(
        map_core_error(ErrorCode::NotFound).status_code(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        map_core_error(ErrorCode::VersionConflict).status_code(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        map_core_error(ErrorCode::StorageUnavailable).status_code(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        ApiError::PayloadTooLarge.status_code(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[test]
fn upload_errors_map_to_stable_transport_codes_without_backend_details() {
    let cases = [
        (
            UploadError::NotFound,
            StatusCode::NOT_FOUND,
            "upload_not_found",
        ),
        (
            UploadError::InvalidOffset { current_offset: 7 },
            StatusCode::CONFLICT,
            "invalid_offset",
        ),
        (
            UploadError::InvalidState {
                state: UploadSessionState::Aborted,
            },
            StatusCode::CONFLICT,
            "invalid_state",
        ),
        (
            UploadError::UploadExpired,
            StatusCode::GONE,
            "upload_expired",
        ),
        (
            UploadError::SizeMismatch,
            StatusCode::CONFLICT,
            "size_mismatch",
        ),
        (
            UploadError::HashMismatch,
            StatusCode::PRECONDITION_FAILED,
            "hash_mismatch",
        ),
        (
            UploadError::VersionConflict {
                current_revision: Some(Revision::new(3)),
            },
            StatusCode::CONFLICT,
            "version_conflict",
        ),
        (
            UploadError::CapacityUnavailable,
            StatusCode::INSUFFICIENT_STORAGE,
            "capacity_unavailable",
        ),
        (
            UploadError::StorageUnavailable,
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
        ),
        (
            UploadError::ChunkTooLarge,
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
        ),
    ];

    for (error, status, code) in cases {
        let error = ApiError::from(error);
        assert_eq!(error.status_code(), status);
        assert_eq!(error.code(), code);
    }
    assert_eq!(
        ApiError::UnsupportedMediaType.status_code(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert_eq!(
        ApiError::UnsupportedMediaType.code(),
        "unsupported_media_type"
    );
}

#[tokio::test]
async fn login_sets_secure_host_only_cookies_and_never_returns_the_session_token() {
    let response = router(auth_state())
        .oneshot(json_request(
            Method::POST,
            "/api/v1/auth/login",
            &format!(
                "{{\"login\":\"{TEST_LOGIN}\",\"login_key\":\"{TEST_LOGIN_KEY}\",\"password\":\"{TEST_PASSWORD}\"}}"
            ),
        ))
        .await
        .expect("login request must not fail");

    assert_eq!(response.status(), StatusCode::OK);
    let session = cookie_value(&response, "synveil_session");
    let csrf = cookie_value(&response, "synveil_csrf");
    assert_eq!(session.len(), 64);
    assert_eq!(csrf.len(), 128);
    for header in response.headers().get_all("set-cookie") {
        let value = header.to_str().expect("cookie header must be ASCII");
        assert!(value.contains("Path=/"));
        assert!(value.contains("SameSite=Lax"));
        assert!(value.contains("Secure"));
        assert!(!value.contains("Domain="));
    }
    let session_cookie = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .find(|value| value.to_str().unwrap().starts_with("synveil_session="))
        .expect("session cookie must be present")
        .to_str()
        .unwrap();
    assert!(session_cookie.contains("HttpOnly"));
    assert!(!session_cookie.contains(&csrf));
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    assert_eq!(response.headers().get("vary").unwrap(), "Cookie");

    let body = json_body(response).await;
    assert_eq!(body["data"]["authenticated"], true);
    assert_eq!(body["data"]["user_id"].as_str().unwrap().len(), 36);
    assert!(body.to_string().find("token").is_none());
    assert!(body.to_string().find(&session).is_none());
}

#[tokio::test]
async fn invalid_login_credentials_use_one_generic_public_failure() {
    let wrong_password = router(auth_state())
        .oneshot(json_request(
            Method::POST,
            "/api/v1/auth/login",
            &format!(
                "{{\"login\":\"{TEST_LOGIN}\",\"login_key\":\"{TEST_LOGIN_KEY}\",\"password\":\"wrong\"}}"
            ),
        ))
        .await
        .expect("login request must not fail");
    let wrong_password_body = json_body(wrong_password).await;

    let wrong_identifier = router(auth_state())
        .oneshot(json_request(
            Method::POST,
            "/api/v1/auth/login",
            &format!(
                "{{\"login\":\"nobody\",\"login_key\":\"unknown\",\"password\":\"{TEST_PASSWORD}\"}}"
            ),
        ))
        .await
        .expect("login request must not fail");
    let wrong_identifier_body = json_body(wrong_identifier).await;

    assert_eq!(
        wrong_password_body["error"]["code"],
        "authentication_failed"
    );
    assert_eq!(
        wrong_identifier_body["error"]["code"],
        "authentication_failed"
    );
    assert_eq!(
        wrong_password_body["error"]["message"],
        wrong_identifier_body["error"]["message"]
    );
    assert!(
        wrong_password_body
            .to_string()
            .find(TEST_PASSWORD)
            .is_none()
    );
}

#[tokio::test]
async fn session_endpoint_requires_a_valid_cookie_but_not_a_csrf_token() {
    let state = auth_state();
    let (session, csrf) = login_cookies(&state).await;

    let missing = router(state.clone())
        .oneshot(request(Method::GET, "/api/v1/auth/session", Body::empty()))
        .await
        .expect("session request must not fail");
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

    let mut malformed = request(Method::GET, "/api/v1/auth/session", Body::empty());
    malformed.headers_mut().insert(
        "cookie",
        HeaderValue::from_static("synveil_session=not-a-session-token"),
    );
    let malformed = router(state.clone())
        .oneshot(malformed)
        .await
        .expect("session request must not fail");
    assert_eq!(malformed.status(), StatusCode::UNAUTHORIZED);

    let response = router(state)
        .oneshot(authenticated_request(
            Method::GET,
            "/api/v1/auth/session",
            &session,
            &csrf,
        ))
        .await
        .expect("session request must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let body = json_body(response).await;
    assert_eq!(body["data"]["authenticated"], true);
    assert!(body.to_string().find(&session).is_none());
    assert!(body.to_string().find("token").is_none());
}

#[tokio::test]
async fn csrf_endpoint_rotates_a_session_bound_proof_and_logout_requires_it() {
    let state = auth_state();
    let (session, login_csrf) = login_cookies(&state).await;

    let csrf_response = router(state.clone())
        .oneshot(authenticated_request(
            Method::GET,
            "/api/v1/auth/csrf",
            &session,
            &login_csrf,
        ))
        .await
        .expect("CSRF request must not fail");
    assert_eq!(csrf_response.status(), StatusCode::OK);
    let csrf_body = json_body(csrf_response).await;
    let rotated_csrf = csrf_body["data"]["csrf_token"]
        .as_str()
        .expect("CSRF response must contain a token")
        .to_owned();
    assert_eq!(rotated_csrf.len(), 128);

    let mut missing_header =
        authenticated_request(Method::POST, "/api/v1/auth/logout", &session, &rotated_csrf);
    mark_same_origin(&mut missing_header);
    let missing_header = router(state.clone())
        .oneshot(missing_header)
        .await
        .expect("logout request must not fail");
    assert_eq!(missing_header.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        json_body(missing_header).await["error"]["code"],
        "permission_denied"
    );

    let mut wrong_header =
        authenticated_request(Method::POST, "/api/v1/auth/logout", &session, &rotated_csrf);
    add_csrf_header(&mut wrong_header, &"0".repeat(128));
    mark_same_origin(&mut wrong_header);
    let wrong_header = router(state.clone())
        .oneshot(wrong_header)
        .await
        .expect("logout request must not fail");
    assert_eq!(wrong_header.status(), StatusCode::FORBIDDEN);

    let mut cross_site =
        authenticated_request(Method::POST, "/api/v1/auth/logout", &session, &rotated_csrf);
    add_csrf_header(&mut cross_site, &rotated_csrf);
    cross_site
        .headers_mut()
        .insert("origin", HeaderValue::from_static("https://evil.example"));
    cross_site
        .headers_mut()
        .insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
    let cross_site = router(state.clone())
        .oneshot(cross_site)
        .await
        .expect("logout request must not fail");
    assert_eq!(cross_site.status(), StatusCode::FORBIDDEN);

    let mut valid =
        authenticated_request(Method::POST, "/api/v1/auth/logout", &session, &rotated_csrf);
    add_csrf_header(&mut valid, &rotated_csrf);
    mark_same_origin(&mut valid);
    let valid = router(state.clone())
        .oneshot(valid)
        .await
        .expect("logout request must not fail");
    assert_eq!(valid.status(), StatusCode::NO_CONTENT);
    assert_eq!(valid.headers().get_all("set-cookie").iter().count(), 2);
    for header in valid.headers().get_all("set-cookie") {
        assert!(header.to_str().unwrap().contains("Max-Age=0"));
        assert!(header.to_str().unwrap().contains("Path=/"));
    }

    let mut repeated =
        authenticated_request(Method::POST, "/api/v1/auth/logout", &session, &rotated_csrf);
    add_csrf_header(&mut repeated, &rotated_csrf);
    mark_same_origin(&mut repeated);
    let repeated = router(state)
        .oneshot(repeated)
        .await
        .expect("repeated logout request must not fail");
    assert_eq!(repeated.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn csrf_proof_from_another_session_is_rejected() {
    let backend = Arc::new(TestAuthenticationBackend::new());
    let state = state(true)
        .with_auth_backend(backend)
        .with_cookie_config(CookieConfig::production())
        .with_allowed_origin("https://app.example");
    let (_first_session, first_csrf) = login_cookies(&state).await;
    let (second_session, _) = login_cookies(&state).await;

    let mut request = authenticated_request(
        Method::POST,
        "/api/v1/auth/logout",
        &second_session,
        &first_csrf,
    );
    add_csrf_header(&mut request, &first_csrf);
    mark_same_origin(&mut request);
    let response = router(state)
        .oneshot(request)
        .await
        .expect("logout request must not fail");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn logical_metadata_routes_are_authenticated_csrf_protected_and_conditional() {
    let auth_backend = Arc::new(TestAuthenticationBackend::new());
    let file_backend = Arc::new(TestFileMetadataBackend::new(auth_backend.user_id));
    let library_id = file_backend.library_id();
    let root_id = file_backend.root_id();
    let state = state(true)
        .with_auth_backend(auth_backend)
        .with_file_metadata_backend(file_backend)
        .with_cookie_config(CookieConfig::production())
        .with_allowed_origin("https://app.example");
    let (session, csrf) = login_cookies(&state).await;

    let list = router(state.clone())
        .oneshot(authenticated_request(
            Method::GET,
            "/api/v1/libraries",
            &session,
            &csrf,
        ))
        .await
        .expect("library list request must not fail");
    assert_eq!(list.status(), StatusCode::OK);
    assert_eq!(json_body(list).await["data"][0]["type"], "library");

    let invalid_cursor_path =
        format!("/api/v1/libraries/{library_id}/nodes?cursor=invalid&limit=1");
    let invalid_cursor = router(state.clone())
        .oneshot(authenticated_request(
            Method::GET,
            &invalid_cursor_path,
            &session,
            &csrf,
        ))
        .await
        .expect("invalid cursor request must not fail");
    assert_eq!(invalid_cursor.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(invalid_cursor).await["error"]["code"],
        "invalid_cursor"
    );

    let root_path = format!("/api/v1/nodes/{root_id}");
    let root = router(state.clone())
        .oneshot(authenticated_request(
            Method::GET,
            &root_path,
            &session,
            &csrf,
        ))
        .await
        .expect("node read request must not fail");
    assert_eq!(root.status(), StatusCode::OK);
    let root_etag = root
        .headers()
        .get("etag")
        .expect("node reads must issue an ETag")
        .to_str()
        .expect("ETag must be valid ASCII")
        .to_owned();
    assert!(root_etag.starts_with("\"v1.node."));
    assert_eq!(json_body(root).await["data"]["id"], root_id.to_string());

    let create_path = format!("/api/v1/libraries/{library_id}/nodes");
    let mut missing_csrf = json_request(Method::POST, &create_path, r#"{"name":"Reports"}"#);
    missing_csrf
        .headers_mut()
        .insert("cookie", cookie_header(&session, &csrf));
    mark_same_origin(&mut missing_csrf);
    let missing_csrf = router(state.clone())
        .oneshot(missing_csrf)
        .await
        .expect("metadata request must not fail");
    assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);

    let mut oversized = authenticated_body_request(
        Method::POST,
        "/api/v1/upload-sessions",
        Body::from(vec![b'x'; UPLOAD_JSON_BODY_LIMIT_BYTES + 1]),
        &session,
        &csrf,
    );
    oversized
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    oversized.headers_mut().insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&(UPLOAD_JSON_BODY_LIMIT_BYTES + 1).to_string()).unwrap(),
    );
    mark_same_origin(&mut oversized);
    add_csrf_header(&mut oversized, &csrf);
    let oversized = router(state.clone())
        .oneshot(oversized)
        .await
        .expect("oversized upload creation must not fail");
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        json_body(oversized).await["error"]["code"],
        "payload_too_large"
    );
    assert_eq!(
        json_body(missing_csrf).await["error"]["code"],
        "permission_denied"
    );

    let mut create = json_request(Method::POST, &create_path, r#"{"name":"Reports"}"#);
    create
        .headers_mut()
        .insert("cookie", cookie_header(&session, &csrf));
    mark_same_origin(&mut create);
    add_csrf_header(&mut create, &csrf);
    let created = router(state.clone())
        .oneshot(create)
        .await
        .expect("directory creation request must not fail");
    assert_eq!(created.status(), StatusCode::CREATED);
    let created_etag = created
        .headers()
        .get("etag")
        .expect("created nodes must issue an ETag")
        .to_str()
        .expect("ETag must be valid ASCII")
        .to_owned();
    let created_body = json_body(created).await;
    let node_id = created_body["data"]["id"]
        .as_str()
        .expect("created node must have an ID")
        .to_owned();
    assert_eq!(created_body["data"]["attributes"]["kind"], "DIRECTORY");
    assert_eq!(created_body["data"]["revision"], "0");
    assert_eq!(created_body["data"]["attributes"]["purge_eligible"], false);
    assert!(created_body["data"]["attributes"]["trashed_at"].is_null());
    assert!(created_body["data"]["attributes"]["restore_deadline"].is_null());

    let node_path = format!("/api/v1/nodes/{node_id}");
    let mut rename = json_request(Method::PATCH, &node_path, r#"{"name":"Renamed"}"#);
    rename
        .headers_mut()
        .insert("cookie", cookie_header(&session, &csrf));
    rename
        .headers_mut()
        .insert("if-match", HeaderValue::from_str(&created_etag).unwrap());
    mark_same_origin(&mut rename);
    add_csrf_header(&mut rename, &csrf);
    let renamed = router(state.clone())
        .oneshot(rename)
        .await
        .expect("rename request must not fail");
    assert_eq!(renamed.status(), StatusCode::OK);
    let renamed_etag = renamed
        .headers()
        .get("etag")
        .expect("renamed nodes must issue an ETag")
        .to_str()
        .expect("ETag must be valid ASCII")
        .to_owned();
    assert_ne!(renamed_etag, created_etag);
    assert_eq!(json_body(renamed).await["data"]["revision"], "1");

    let mut stale = json_request(Method::PATCH, &node_path, r#"{"name":"Stale"}"#);
    stale
        .headers_mut()
        .insert("cookie", cookie_header(&session, &csrf));
    stale
        .headers_mut()
        .insert("if-match", HeaderValue::from_str(&created_etag).unwrap());
    mark_same_origin(&mut stale);
    add_csrf_header(&mut stale, &csrf);
    let stale = router(state.clone())
        .oneshot(stale)
        .await
        .expect("stale request must not fail");
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let stale_body = json_body(stale).await;
    assert_eq!(stale_body["error"]["code"], "version_conflict");
    assert_eq!(stale_body["error"]["details"]["resource_type"], "node");
    assert_eq!(stale_body["error"]["details"]["current_revision"], "1");
    assert!(stale_body["error"]["details"]["etag"].is_string());

    let mut create_archive = json_request(Method::POST, &create_path, r#"{"name":"Archive"}"#);
    create_archive
        .headers_mut()
        .insert("cookie", cookie_header(&session, &csrf));
    mark_same_origin(&mut create_archive);
    add_csrf_header(&mut create_archive, &csrf);
    let archive = router(state.clone())
        .oneshot(create_archive)
        .await
        .expect("archive creation request must not fail");
    assert_eq!(archive.status(), StatusCode::CREATED);
    let archive_id = json_body(archive).await["data"]["id"]
        .as_str()
        .expect("archive must have an ID")
        .to_owned();

    let mut move_request = json_request(
        Method::PATCH,
        &node_path,
        &format!(r#"{{"parent_id":"{archive_id}"}}"#),
    );
    move_request
        .headers_mut()
        .insert("cookie", cookie_header(&session, &csrf));
    move_request
        .headers_mut()
        .insert("if-match", HeaderValue::from_str(&renamed_etag).unwrap());
    mark_same_origin(&mut move_request);
    add_csrf_header(&mut move_request, &csrf);
    let moved = router(state.clone())
        .oneshot(move_request)
        .await
        .expect("move request must not fail");
    assert_eq!(moved.status(), StatusCode::OK);
    let moved_etag = moved
        .headers()
        .get("etag")
        .expect("moved nodes must issue an ETag")
        .to_str()
        .expect("ETag must be valid ASCII")
        .to_owned();
    let moved_body = json_body(moved).await;
    assert_eq!(moved_body["data"]["attributes"]["parent_id"], archive_id);
    assert_eq!(moved_body["data"]["revision"], "2");

    let mut missing_precondition = authenticated_request(
        Method::POST,
        &format!("/api/v1/nodes/{node_id}/trash"),
        &session,
        &csrf,
    );
    mark_same_origin(&mut missing_precondition);
    add_csrf_header(&mut missing_precondition, &csrf);
    let missing_precondition = router(state.clone())
        .oneshot(missing_precondition)
        .await
        .expect("missing precondition request must not fail");
    assert_eq!(
        missing_precondition.status(),
        StatusCode::PRECONDITION_REQUIRED
    );
    assert_eq!(
        json_body(missing_precondition).await["error"]["code"],
        "precondition_required"
    );

    let mut trash = authenticated_request(
        Method::POST,
        &format!("/api/v1/nodes/{node_id}/trash"),
        &session,
        &csrf,
    );
    trash
        .headers_mut()
        .insert("if-match", HeaderValue::from_str(&moved_etag).unwrap());
    mark_same_origin(&mut trash);
    add_csrf_header(&mut trash, &csrf);
    let trash = router(state.clone())
        .oneshot(trash)
        .await
        .expect("logical delete request must not fail");
    assert_eq!(trash.status(), StatusCode::OK);
    let trashed_etag = trash
        .headers()
        .get("etag")
        .expect("trashed nodes must issue an ETag")
        .to_str()
        .expect("ETag must be valid ASCII")
        .to_owned();
    let trashed_body = json_body(trash).await;
    assert_eq!(trashed_body["data"]["attributes"]["state"], "TRASHED");
    assert!(trashed_body["data"]["attributes"]["trashed_at"].is_string());
    assert!(trashed_body["data"]["attributes"]["restore_deadline"].is_string());
    assert_eq!(trashed_body["data"]["attributes"]["purge_eligible"], false);

    let mut restore = authenticated_request(
        Method::POST,
        &format!("/api/v1/nodes/{node_id}/restore"),
        &session,
        &csrf,
    );
    restore
        .headers_mut()
        .insert("if-match", HeaderValue::from_str(&trashed_etag).unwrap());
    mark_same_origin(&mut restore);
    add_csrf_header(&mut restore, &csrf);
    let restore = router(state.clone())
        .oneshot(restore)
        .await
        .expect("restore request must not fail");
    assert_eq!(restore.status(), StatusCode::OK);
    let restored_body = json_body(restore).await;
    assert_eq!(restored_body["data"]["attributes"]["state"], "ACTIVE");
    assert!(restored_body["data"]["attributes"]["trashed_at"].is_null());
    assert!(restored_body["data"]["attributes"]["restore_deadline"].is_null());
    assert_eq!(restored_body["data"]["attributes"]["purge_eligible"], false);
}

#[tokio::test]
async fn upload_create_requires_authentication_and_csrf_and_uses_tagged_targets() {
    let (state, auth_backend, upload_backend) = upload_test_state();
    let library_id = LibraryId::new();
    let parent_id = NodeId::new();
    let create_body = format!(
        r#"{{"operation":"CREATE_FILE","library_id":"{library_id}","parent_id":"{parent_id}","name":"report.bin","expected_bytes":"8"}}"#
    );

    let unauthenticated = router(state.clone())
        .oneshot(json_request(
            Method::POST,
            "/api/v1/upload-sessions",
            &create_body,
        ))
        .await
        .expect("unauthenticated upload request must not fail");
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let (session, csrf) = login_cookies(&state).await;
    let mut missing_csrf = authenticated_body_request(
        Method::POST,
        "/api/v1/upload-sessions",
        Body::from(create_body.clone()),
        &session,
        &csrf,
    );
    missing_csrf
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    mark_same_origin(&mut missing_csrf);
    let missing_csrf = router(state.clone())
        .oneshot(missing_csrf)
        .await
        .expect("CSRF rejection must not fail");
    assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);

    let mut create = authenticated_body_request(
        Method::POST,
        "/api/v1/upload-sessions",
        Body::from(create_body),
        &session,
        &csrf,
    );
    create
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    create.headers_mut().insert(
        "x-request-id",
        HeaderValue::from_static("upload-create-1234"),
    );
    mark_same_origin(&mut create);
    add_csrf_header(&mut create, &csrf);
    let created = router(state.clone())
        .oneshot(create)
        .await
        .expect("upload creation must not fail");
    assert_eq!(created.status(), StatusCode::CREATED);
    assert_eq!(created.headers().get("upload-offset").unwrap(), "0");
    assert!(
        created
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("/api/v1/upload-sessions/")
    );
    assert_eq!(response_request_id(&created), "upload-create-1234");
    let created_body = json_body(created).await;
    assert_eq!(created_body["data"]["type"], "upload_session");
    assert_eq!(
        created_body["data"]["attributes"]["operation"],
        "CREATE_FILE"
    );
    assert_eq!(created_body["data"]["attributes"]["received_bytes"], "0");
    assert_eq!(created_body["meta"]["request_id"], "upload-create-1234");
    let serialized = created_body.to_string();
    for forbidden in ["object_key", "staging", "object_replica", "user_id"] {
        assert!(!serialized.contains(forbidden));
    }

    let replace_node_id = NodeId::new();
    let digest = Sha256Digest::from_bytes([3_u8; 32]);
    let mut replace = authenticated_body_request(
        Method::POST,
        "/api/v1/upload-sessions",
        Body::from(format!(
            r#"{{"operation":"REPLACE_CONTENT","library_id":"{library_id}","node_id":"{replace_node_id}","expected_revision":"7","expected_bytes":"8","expected_sha256":"{digest}"}}"#
        )),
        &session,
        &csrf,
    );
    replace
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    mark_same_origin(&mut replace);
    add_csrf_header(&mut replace, &csrf);
    let replaced = router(state.clone())
        .oneshot(replace)
        .await
        .expect("replace upload creation must not fail");
    assert_eq!(replaced.status(), StatusCode::CREATED);
    let replaced_body = json_body(replaced).await;
    assert_eq!(
        replaced_body["data"]["attributes"]["target"]["operation"],
        "REPLACE_CONTENT"
    );
    assert_eq!(
        replaced_body["data"]["attributes"]["target"]["expected_revision"],
        "7"
    );
    assert_eq!(
        replaced_body["data"]["attributes"]["expected_sha256"],
        digest.to_string()
    );

    let state_guard = upload_backend.state.lock().expect("upload state lock");
    assert!(
        state_guard
            .sessions
            .values()
            .all(|upload| upload.view.owner_user_id == auth_backend.user_id)
    );
}

#[tokio::test]
async fn upload_status_requires_auth_without_csrf_and_conceals_cross_owner_sessions() {
    let (state, auth_backend, upload_backend) = upload_test_state();
    let (session, csrf) = login_cookies(&state).await;
    let session_id = create_test_upload(&state, &session, &csrf, 8).await;
    let path = format!("/api/v1/upload-sessions/{session_id}");

    let missing_auth = router(state.clone())
        .oneshot(request(Method::GET, &path, Body::empty()))
        .await
        .expect("unauthenticated status request must not fail");
    assert_eq!(missing_auth.status(), StatusCode::UNAUTHORIZED);

    let status = router(state.clone())
        .oneshot(authenticated_request(Method::GET, &path, &session, &csrf))
        .await
        .expect("authenticated status request must not fail");
    assert_eq!(status.status(), StatusCode::OK);
    assert_eq!(status.headers().get("upload-offset").unwrap(), "0");
    assert_eq!(
        json_body(status).await["data"]["attributes"]["received_bytes"],
        "0"
    );

    let foreign = upload_backend
        .create_upload_session(CreateUploadSessionRequest {
            owner_user_id: UserId::new(),
            target: UploadTargetRequest::CreateFile {
                library_id: LibraryId::new(),
                parent_node_id: NodeId::new(),
                name: LogicalName::new("foreign.bin").expect("test name"),
            },
            expected_length: 8,
            expected_sha256: None,
        })
        .await
        .expect("foreign test upload must be created");
    assert_ne!(foreign.owner_user_id, auth_backend.user_id);
    let concealed = router(state)
        .oneshot(authenticated_request(
            Method::GET,
            &format!("/api/v1/upload-sessions/{}", foreign.id),
            &session,
            &csrf,
        ))
        .await
        .expect("cross-owner status request must not fail");
    assert_eq!(concealed.status(), StatusCode::NOT_FOUND);
    let concealed_body = json_body(concealed).await;
    assert_eq!(concealed_body["error"]["code"], "upload_not_found");
    assert!(!concealed_body.to_string().contains("foreign.bin"));
}

#[tokio::test]
async fn upload_append_streams_exact_offsets_enforces_limits_and_recovers_ambiguity() {
    let (state, _auth_backend, upload_backend) = upload_test_state();
    let (session, csrf) = login_cookies(&state).await;
    let session_id = create_test_upload(&state, &session, &csrf, 8).await;
    // The raw upload route bypasses the unrelated global JSON/body ceiling and
    // remains bounded by the upload backend's limit instead.
    let state = state.with_body_limit(1);
    let path = format!("/api/v1/upload-sessions/{session_id}");

    let mut missing_auth = request(Method::PATCH, &path, Body::from("a"));
    missing_auth.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/octet-stream"),
    );
    missing_auth
        .headers_mut()
        .insert("upload-offset", HeaderValue::from_static("0"));
    let missing_auth = router(state.clone())
        .oneshot(missing_auth)
        .await
        .expect("unauthenticated append must not fail");
    assert_eq!(missing_auth.status(), StatusCode::UNAUTHORIZED);

    let mut missing_csrf =
        authenticated_body_request(Method::PATCH, &path, Body::from("a"), &session, &csrf);
    missing_csrf.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/octet-stream"),
    );
    missing_csrf
        .headers_mut()
        .insert("upload-offset", HeaderValue::from_static("0"));
    mark_same_origin(&mut missing_csrf);
    let missing_csrf = router(state.clone())
        .oneshot(missing_csrf)
        .await
        .expect("CSRF append rejection must not fail");
    assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);

    let mut missing_offset =
        authenticated_body_request(Method::PATCH, &path, Body::from("a"), &session, &csrf);
    missing_offset.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/octet-stream"),
    );
    mark_same_origin(&mut missing_offset);
    add_csrf_header(&mut missing_offset, &csrf);
    let missing_offset = router(state.clone())
        .oneshot(missing_offset)
        .await
        .expect("missing offset rejection must not fail");
    assert_eq!(missing_offset.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_offset).await["error"]["code"],
        "invalid_offset"
    );

    let mut malformed_offset =
        authenticated_body_request(Method::PATCH, &path, Body::from("a"), &session, &csrf);
    malformed_offset.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/octet-stream"),
    );
    malformed_offset
        .headers_mut()
        .insert("upload-offset", HeaderValue::from_static("00"));
    mark_same_origin(&mut malformed_offset);
    add_csrf_header(&mut malformed_offset, &csrf);
    let malformed_offset = router(state.clone())
        .oneshot(malformed_offset)
        .await
        .expect("malformed offset rejection must not fail");
    assert_eq!(malformed_offset.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(malformed_offset).await["error"]["code"],
        "invalid_offset"
    );

    let mut wrong_type =
        authenticated_body_request(Method::PATCH, &path, Body::from("a"), &session, &csrf);
    wrong_type
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    wrong_type
        .headers_mut()
        .insert("upload-offset", HeaderValue::from_static("0"));
    mark_same_origin(&mut wrong_type);
    add_csrf_header(&mut wrong_type, &csrf);
    let wrong_type = router(state.clone())
        .oneshot(wrong_type)
        .await
        .expect("content-type rejection must not fail");
    assert_eq!(wrong_type.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(
        json_body(wrong_type).await["error"]["code"],
        "unsupported_media_type"
    );

    let mut oversized = authenticated_body_request(
        Method::PATCH,
        &path,
        Body::from(vec![b'x'; 9]),
        &session,
        &csrf,
    );
    oversized.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/octet-stream"),
    );
    oversized
        .headers_mut()
        .insert("upload-offset", HeaderValue::from_static("0"));
    oversized
        .headers_mut()
        .insert(CONTENT_LENGTH, HeaderValue::from_static("9"));
    mark_same_origin(&mut oversized);
    add_csrf_header(&mut oversized, &csrf);
    let oversized = router(state.clone())
        .oneshot(oversized)
        .await
        .expect("oversized append rejection must not fail");
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        json_body(oversized).await["error"]["code"],
        "payload_too_large"
    );
    assert!(upload_backend.bytes(session_id).is_empty());

    let mut empty =
        authenticated_body_request(Method::PATCH, &path, Body::empty(), &session, &csrf);
    empty.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/octet-stream"),
    );
    empty
        .headers_mut()
        .insert("upload-offset", HeaderValue::from_static("0"));
    mark_same_origin(&mut empty);
    add_csrf_header(&mut empty, &csrf);
    let empty = router(state.clone())
        .oneshot(empty)
        .await
        .expect("empty append rejection must not fail");
    assert_eq!(empty.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(empty).await["error"]["code"],
        "invalid_upload_request"
    );
    assert!(upload_backend.bytes(session_id).is_empty());

    let frames = futures_util::stream::iter([
        Ok::<Bytes, std::convert::Infallible>(Bytes::from_static(b"abc")),
        Ok(Bytes::from_static(b"def")),
    ]);
    let mut append = authenticated_body_request(
        Method::PATCH,
        &path,
        Body::from_stream(frames),
        &session,
        &csrf,
    );
    append.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/octet-stream"),
    );
    append
        .headers_mut()
        .insert("upload-offset", HeaderValue::from_static("0"));
    mark_same_origin(&mut append);
    add_csrf_header(&mut append, &csrf);
    let append_response = router(state.clone())
        .oneshot(append)
        .await
        .expect("streaming append must not fail");
    assert_eq!(append_response.status(), StatusCode::NO_CONTENT);
    assert_eq!(append_response.headers().get("upload-offset").unwrap(), "6");
    assert_eq!(upload_backend.frame_count(), 2);
    assert_eq!(upload_backend.largest_frame(), 3);

    // Treat the successful response above as lost. Recovery reads the
    // authoritative persisted offset instead of replaying offset zero.
    let status = router(state.clone())
        .oneshot(authenticated_request(Method::GET, &path, &session, &csrf))
        .await
        .expect("recovery status must not fail");
    assert_eq!(status.headers().get("upload-offset").unwrap(), "6");
    assert_eq!(
        json_body(status).await["data"]["attributes"]["received_bytes"],
        "6"
    );

    for wrong_offset in ["0", "7"] {
        let mut wrong =
            authenticated_body_request(Method::PATCH, &path, Body::from("x"), &session, &csrf);
        wrong.headers_mut().insert(
            "content-type",
            HeaderValue::from_static("application/octet-stream"),
        );
        wrong.headers_mut().insert(
            "upload-offset",
            HeaderValue::from_str(wrong_offset).unwrap(),
        );
        wrong.headers_mut().insert(
            "x-request-id",
            HeaderValue::from_static("upload-offset-1234"),
        );
        mark_same_origin(&mut wrong);
        add_csrf_header(&mut wrong, &csrf);
        let wrong = router(state.clone())
            .oneshot(wrong)
            .await
            .expect("offset conflict must not fail");
        assert_eq!(wrong.status(), StatusCode::CONFLICT);
        assert_eq!(wrong.headers().get("upload-offset").unwrap(), "6");
        let body = json_body(wrong).await;
        assert_eq!(body["error"]["code"], "invalid_offset");
        assert_eq!(body["error"]["details"]["current_offset"], "6");
        assert_eq!(body["error"]["request_id"], "upload-offset-1234");
        let serialized = body.to_string();
        for forbidden in ["object_key", "staging", "filesystem", "sql"] {
            assert!(!serialized.contains(forbidden));
        }
    }
    assert_eq!(upload_backend.bytes(session_id), b"abcdef");

    let mut resume =
        authenticated_body_request(Method::PATCH, &path, Body::from("gh"), &session, &csrf);
    resume.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/octet-stream"),
    );
    resume
        .headers_mut()
        .insert("upload-offset", HeaderValue::from_static("6"));
    mark_same_origin(&mut resume);
    add_csrf_header(&mut resume, &csrf);
    let resumed = router(state.clone())
        .oneshot(resume)
        .await
        .expect("resumed append must not fail");
    assert_eq!(resumed.status(), StatusCode::NO_CONTENT);
    assert_eq!(resumed.headers().get("upload-offset").unwrap(), "8");
    assert_eq!(upload_backend.bytes(session_id), b"abcdefgh");

    let status = router(state.clone())
        .oneshot(authenticated_request(Method::GET, &path, &session, &csrf))
        .await
        .expect("final status must not fail");
    assert_eq!(status.headers().get("upload-offset").unwrap(), "8");

    let aggregate_session_id = create_test_upload(&state, &session, &csrf, 9).await;
    let aggregate_path = format!("/api/v1/upload-sessions/{aggregate_session_id}");
    let aggregate_frames = futures_util::stream::iter([
        Ok::<Bytes, std::convert::Infallible>(Bytes::from_static(b"1234")),
        Ok(Bytes::from_static(b"5678")),
        Ok(Bytes::from_static(b"9")),
    ]);
    let mut aggregate = authenticated_body_request(
        Method::PATCH,
        &aggregate_path,
        Body::from_stream(aggregate_frames),
        &session,
        &csrf,
    );
    aggregate.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/octet-stream"),
    );
    aggregate
        .headers_mut()
        .insert("upload-offset", HeaderValue::from_static("0"));
    mark_same_origin(&mut aggregate);
    add_csrf_header(&mut aggregate, &csrf);
    let aggregate = router(state.clone())
        .oneshot(aggregate)
        .await
        .expect("aggregate-limit append must not fail");
    assert_eq!(aggregate.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        json_body(aggregate).await["error"]["code"],
        "payload_too_large"
    );

    let recovered = router(state)
        .oneshot(authenticated_request(
            Method::GET,
            &aggregate_path,
            &session,
            &csrf,
        ))
        .await
        .expect("aggregate-limit recovery status must not fail");
    assert_eq!(recovered.headers().get("upload-offset").unwrap(), "8");
    assert_eq!(
        json_body(recovered).await["data"]["attributes"]["received_bytes"],
        "8"
    );
}

#[tokio::test]
async fn upload_append_streams_a_body_larger_than_one_transport_frame_without_aggregation() {
    const TRANSPORT_FRAME_BYTES: usize = 16 * 1024;
    const FRAME_COUNT: usize = 9;
    const TOTAL_BYTES: usize = TRANSPORT_FRAME_BYTES * FRAME_COUNT;

    let auth_backend = Arc::new(TestAuthenticationBackend::new());
    let upload_backend = Arc::new(TestUploadBackend::new(
        u64::try_from(TOTAL_BYTES).expect("test body length fits u64"),
    ));
    let state = state(true)
        .with_auth_backend(auth_backend)
        .with_upload_backend(upload_backend.clone())
        .with_cookie_config(CookieConfig::production())
        .with_allowed_origin("https://app.example");
    let (session, csrf) = login_cookies(&state).await;
    let session_id = create_test_upload(
        &state,
        &session,
        &csrf,
        u64::try_from(TOTAL_BYTES).expect("test body length fits u64"),
    )
    .await;
    let path = format!("/api/v1/upload-sessions/{session_id}");
    let frames = (0..FRAME_COUNT).map(|frame_index| {
        Ok::<Bytes, std::convert::Infallible>(Bytes::from(vec![
            u8::try_from(frame_index).expect(
                "test frame index fits u8"
            );
            TRANSPORT_FRAME_BYTES
        ]))
    });
    let mut append = authenticated_body_request(
        Method::PATCH,
        &path,
        Body::from_stream(futures_util::stream::iter(frames)),
        &session,
        &csrf,
    );
    append.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/octet-stream"),
    );
    append
        .headers_mut()
        .insert("upload-offset", HeaderValue::from_static("0"));
    mark_same_origin(&mut append);
    add_csrf_header(&mut append, &csrf);

    let response = router(state)
        .oneshot(append)
        .await
        .expect("large framed append must not fail");

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        response.headers().get("upload-offset").unwrap(),
        TOTAL_BYTES.to_string().as_str()
    );
    assert_eq!(upload_backend.frame_count(), FRAME_COUNT);
    assert_eq!(upload_backend.largest_frame(), TRANSPORT_FRAME_BYTES);
    assert_eq!(upload_backend.bytes(session_id).len(), TOTAL_BYTES);
}

#[tokio::test]
async fn upload_complete_and_abort_are_csrf_protected_and_retry_safe() {
    let (state, _auth_backend, _upload_backend) = upload_test_state();
    let (session, csrf) = login_cookies(&state).await;
    let session_id = create_test_upload(&state, &session, &csrf, 2).await;
    let upload_path = format!("/api/v1/upload-sessions/{session_id}");

    let mut append = authenticated_body_request(
        Method::PATCH,
        &upload_path,
        Body::from("ok"),
        &session,
        &csrf,
    );
    append.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/octet-stream"),
    );
    append
        .headers_mut()
        .insert("upload-offset", HeaderValue::from_static("0"));
    mark_same_origin(&mut append);
    add_csrf_header(&mut append, &csrf);
    let appended = router(state.clone())
        .oneshot(append)
        .await
        .expect("completion fixture append must not fail");
    assert_eq!(appended.status(), StatusCode::NO_CONTENT);

    let complete_path = format!("{upload_path}/complete");
    let mut missing_complete_csrf =
        authenticated_request(Method::POST, &complete_path, &session, &csrf);
    mark_same_origin(&mut missing_complete_csrf);
    let missing_complete_csrf = router(state.clone())
        .oneshot(missing_complete_csrf)
        .await
        .expect("completion CSRF rejection must not fail");
    assert_eq!(missing_complete_csrf.status(), StatusCode::FORBIDDEN);

    let mut complete = authenticated_request(Method::POST, &complete_path, &session, &csrf);
    mark_same_origin(&mut complete);
    add_csrf_header(&mut complete, &csrf);
    let completed = router(state.clone())
        .oneshot(complete)
        .await
        .expect("upload completion must not fail");
    assert_eq!(completed.status(), StatusCode::OK);
    let completed_body = json_body(completed).await;
    assert_eq!(completed_body["data"]["type"], "upload_completion");
    assert_eq!(completed_body["data"]["id"], session_id.to_string());
    assert_eq!(completed_body["data"]["attributes"]["bytes"], "2");
    assert_eq!(completed_body["data"]["attributes"]["node_revision"], "1");
    assert!(completed_body["data"]["attributes"]["file_version_id"].is_string());
    assert!(completed_body["data"]["attributes"]["sha256"].is_string());
    assert!(completed_body.to_string().find("object_replica").is_none());

    let mut repeat_complete = authenticated_request(Method::POST, &complete_path, &session, &csrf);
    mark_same_origin(&mut repeat_complete);
    add_csrf_header(&mut repeat_complete, &csrf);
    let repeat_complete = router(state.clone())
        .oneshot(repeat_complete)
        .await
        .expect("repeated upload completion must not fail");
    assert_eq!(repeat_complete.status(), StatusCode::OK);
    assert_eq!(
        json_body(repeat_complete).await["data"],
        completed_body["data"]
    );

    let abort_session_id = create_test_upload(&state, &session, &csrf, 4).await;
    let abort_path = format!("/api/v1/upload-sessions/{abort_session_id}/abort");
    let mut missing_abort_csrf = authenticated_request(Method::POST, &abort_path, &session, &csrf);
    mark_same_origin(&mut missing_abort_csrf);
    let missing_abort_csrf = router(state.clone())
        .oneshot(missing_abort_csrf)
        .await
        .expect("abort CSRF rejection must not fail");
    assert_eq!(missing_abort_csrf.status(), StatusCode::FORBIDDEN);

    for _ in 0..2 {
        let mut abort = authenticated_request(Method::POST, &abort_path, &session, &csrf);
        mark_same_origin(&mut abort);
        add_csrf_header(&mut abort, &csrf);
        let aborted = router(state.clone())
            .oneshot(abort)
            .await
            .expect("upload abort must not fail");
        assert_eq!(aborted.status(), StatusCode::OK);
        assert_eq!(
            json_body(aborted).await["data"]["attributes"]["state"],
            "ABORTED"
        );
    }
}
