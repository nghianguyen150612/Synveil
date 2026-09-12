use std::{fmt, pin::Pin};

use async_trait::async_trait;
use futures_core::Stream;
use synveil_core::{
    ChangeEvent, ChangeEventId, ClientMutationId, ClientMutationRequest, DeviceId, FileVersionId,
    LibraryId, LogicalSnapshotNode, NodeId, NodeState, OutboundIntentId, RebaselineSnapshotId,
    Revision, Sequence, Sha256Digest, SyncBootstrap, SyncBootstrapId, SyncConflictId,
    UploadSessionId, UploadSessionState, UserId,
};

use crate::{ClientSyncError, MAX_OPAQUE_EVIDENCE_BYTES, MAX_PAGE_ITEMS};

/// Authenticated logical scope expected on every remote response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplicaScope {
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
}

impl ReplicaScope {
    #[must_use]
    pub const fn new(owner_user_id: UserId, device_id: DeviceId, library_id: LibraryId) -> Self {
        Self {
            owner_user_id,
            device_id,
            library_id,
        }
    }

    #[must_use]
    pub const fn owner_user_id(self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn device_id(self) -> DeviceId {
        self.device_id
    }

    #[must_use]
    pub const fn library_id(self) -> LibraryId {
        self.library_id
    }
}

/// Opaque server-issued cursor, acknowledgement, or completion evidence.
///
/// It is persistable but deliberately redacted from `Debug` and never
/// implements `Display`.
#[derive(Clone, Eq, PartialEq)]
pub struct OpaqueEvidence(Vec<u8>);

impl OpaqueEvidence {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self, ClientSyncError> {
        let bytes = bytes.into();
        if bytes.is_empty() || bytes.len() > MAX_OPAQUE_EVIDENCE_BYTES {
            return Err(ClientSyncError::ResourceLimit);
        }
        Ok(Self(bytes))
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for OpaqueEvidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OpaqueEvidence([REDACTED])")
    }
}

/// Transport-neutral checkpoint confirmation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoteCheckpoint {
    scope: ReplicaScope,
    epoch: Sequence,
    acknowledged_sequence: Sequence,
}

impl RemoteCheckpoint {
    #[must_use]
    pub const fn new(
        scope: ReplicaScope,
        epoch: Sequence,
        acknowledged_sequence: Sequence,
    ) -> Self {
        Self {
            scope,
            epoch,
            acknowledged_sequence,
        }
    }

    #[must_use]
    pub const fn scope(self) -> ReplicaScope {
        self.scope
    }

    #[must_use]
    pub const fn epoch(self) -> Sequence {
        self.epoch
    }

    #[must_use]
    pub const fn acknowledged_sequence(self) -> Sequence {
        self.acknowledged_sequence
    }
}

/// Transport-neutral confirmation returned by the durable snapshot handoff.
/// The HTTP adapter validates the response's snapshot/library identity before
/// constructing this value; the engine then compares the checkpoint against
/// its locally persisted `AppliedPendingHandoff` marker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RebaselineHandoffConfirmation {
    snapshot_id: RebaselineSnapshotId,
    library_id: LibraryId,
    checkpoint: RemoteCheckpoint,
}

impl RebaselineHandoffConfirmation {
    #[must_use]
    pub const fn new(
        snapshot_id: RebaselineSnapshotId,
        library_id: LibraryId,
        checkpoint: RemoteCheckpoint,
    ) -> Self {
        Self {
            snapshot_id,
            library_id,
            checkpoint,
        }
    }

    #[must_use]
    pub const fn snapshot_id(self) -> RebaselineSnapshotId {
        self.snapshot_id
    }

    #[must_use]
    pub const fn library_id(self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn checkpoint(self) -> RemoteCheckpoint {
        self.checkpoint
    }
}

/// One journal event plus the logical desired state resolved by the remote
/// adapter. Purge is the only event allowed to carry no desired Node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboundChange {
    event: ChangeEvent,
    desired_node: Option<LogicalSnapshotNode>,
}

impl InboundChange {
    #[must_use]
    pub const fn new(event: ChangeEvent, desired_node: Option<LogicalSnapshotNode>) -> Self {
        Self {
            event,
            desired_node,
        }
    }

    #[must_use]
    pub const fn event(&self) -> ChangeEvent {
        self.event
    }

    #[must_use]
    pub const fn desired_node(&self) -> Option<&LogicalSnapshotNode> {
        self.desired_node.as_ref()
    }
}

/// One bounded incremental feed page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteFeedPage {
    scope: ReplicaScope,
    epoch: Sequence,
    from_sequence: Sequence,
    through_sequence: Sequence,
    high_watermark: Sequence,
    has_more: bool,
    changes: Vec<InboundChange>,
    ack_evidence: Option<OpaqueEvidence>,
}

impl RemoteFeedPage {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        scope: ReplicaScope,
        epoch: Sequence,
        from_sequence: Sequence,
        through_sequence: Sequence,
        high_watermark: Sequence,
        has_more: bool,
        changes: Vec<InboundChange>,
        ack_evidence: Option<OpaqueEvidence>,
    ) -> Result<Self, ClientSyncError> {
        if changes.len() > MAX_PAGE_ITEMS {
            return Err(ClientSyncError::ResourceLimit);
        }
        Ok(Self {
            scope,
            epoch,
            from_sequence,
            through_sequence,
            high_watermark,
            has_more,
            changes,
            ack_evidence,
        })
    }

    #[must_use]
    pub const fn scope(&self) -> ReplicaScope {
        self.scope
    }

    #[must_use]
    pub const fn epoch(&self) -> Sequence {
        self.epoch
    }

    #[must_use]
    pub const fn from_sequence(&self) -> Sequence {
        self.from_sequence
    }

    #[must_use]
    pub const fn through_sequence(&self) -> Sequence {
        self.through_sequence
    }

    #[must_use]
    pub const fn high_watermark(&self) -> Sequence {
        self.high_watermark
    }

    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }

    #[must_use]
    pub fn changes(&self) -> &[InboundChange] {
        &self.changes
    }

    #[must_use]
    pub const fn ack_evidence(&self) -> Option<&OpaqueEvidence> {
        self.ack_evidence.as_ref()
    }
}

/// One bounded immutable snapshot page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootstrapPage {
    bootstrap: SyncBootstrap,
    nodes: Vec<LogicalSnapshotNode>,
    has_more: bool,
    next_cursor: Option<OpaqueEvidence>,
    completion_evidence: Option<OpaqueEvidence>,
}

impl BootstrapPage {
    pub fn new(
        bootstrap: SyncBootstrap,
        nodes: Vec<LogicalSnapshotNode>,
        has_more: bool,
        next_cursor: Option<OpaqueEvidence>,
        completion_evidence: Option<OpaqueEvidence>,
    ) -> Result<Self, ClientSyncError> {
        if nodes.len() > MAX_PAGE_ITEMS {
            return Err(ClientSyncError::ResourceLimit);
        }
        Ok(Self {
            bootstrap,
            nodes,
            has_more,
            next_cursor,
            completion_evidence,
        })
    }

    #[must_use]
    pub const fn bootstrap(&self) -> SyncBootstrap {
        self.bootstrap
    }

    #[must_use]
    pub fn nodes(&self) -> &[LogicalSnapshotNode] {
        &self.nodes
    }

    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }

    #[must_use]
    pub const fn next_cursor(&self) -> Option<&OpaqueEvidence> {
        self.next_cursor.as_ref()
    }

    #[must_use]
    pub const fn completion_evidence(&self) -> Option<&OpaqueEvidence> {
        self.completion_evidence.as_ref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootstrapCompletion {
    bootstrap_id: SyncBootstrapId,
    checkpoint: RemoteCheckpoint,
}

impl BootstrapCompletion {
    #[must_use]
    pub const fn new(bootstrap_id: SyncBootstrapId, checkpoint: RemoteCheckpoint) -> Self {
        Self {
            bootstrap_id,
            checkpoint,
        }
    }

    #[must_use]
    pub const fn bootstrap_id(self) -> SyncBootstrapId {
        self.bootstrap_id
    }

    #[must_use]
    pub const fn checkpoint(self) -> RemoteCheckpoint {
        self.checkpoint
    }
}

pub type ContentByteStream =
    Pin<Box<dyn Stream<Item = Result<bytes::Bytes, RemoteError>> + Send + 'static>>;

pub fn boxed_content_stream<S>(stream: S) -> ContentByteStream
where
    S: Stream<Item = Result<bytes::Bytes, RemoteError>> + Send + 'static,
{
    Box::pin(stream)
}

/// Logical identity/integrity envelope for one current-content stream.
pub struct RemoteContent {
    node_id: NodeId,
    version_id: FileVersionId,
    length: u64,
    sha256: Sha256Digest,
    body: ContentByteStream,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoteMutationApplied {
    mutation_id: ClientMutationId,
    node_id: NodeId,
    revision: Revision,
    journal_event_id: ChangeEventId,
    journal_sequence: Sequence,
    replayed: bool,
}

impl RemoteMutationApplied {
    #[must_use]
    pub const fn new(
        mutation_id: ClientMutationId,
        node_id: NodeId,
        revision: Revision,
        journal_event_id: ChangeEventId,
        journal_sequence: Sequence,
        replayed: bool,
    ) -> Self {
        Self {
            mutation_id,
            node_id,
            revision,
            journal_event_id,
            journal_sequence,
            replayed,
        }
    }

    #[must_use]
    pub const fn mutation_id(self) -> ClientMutationId {
        self.mutation_id
    }

    #[must_use]
    pub const fn node_id(self) -> NodeId {
        self.node_id
    }

    #[must_use]
    pub const fn revision(self) -> Revision {
        self.revision
    }

    #[must_use]
    pub const fn journal_event_id(self) -> ChangeEventId {
        self.journal_event_id
    }

    #[must_use]
    pub const fn journal_sequence(self) -> Sequence {
        self.journal_sequence
    }

    #[must_use]
    pub const fn replayed(self) -> bool {
        self.replayed
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteMutationConflict {
    conflict_id: SyncConflictId,
    reason: String,
    replayed: bool,
    resource_id: Option<NodeId>,
    expected_revision: Option<Revision>,
    current_revision: Option<Revision>,
    current_state: Option<NodeState>,
    current_parent_id: Option<NodeId>,
    server_epoch: Option<Sequence>,
    server_sequence: Option<Sequence>,
}

impl RemoteMutationConflict {
    pub fn new(
        conflict_id: SyncConflictId,
        reason: impl Into<String>,
        replayed: bool,
    ) -> Result<Self, ClientSyncError> {
        let reason = reason.into();
        if reason.is_empty()
            || reason.len() > 128
            || !reason
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte == b'_')
        {
            return Err(ClientSyncError::InvalidRemoteResponse);
        }
        Ok(Self {
            conflict_id,
            reason,
            replayed,
            resource_id: None,
            expected_revision: None,
            current_revision: None,
            current_state: None,
            current_parent_id: None,
            server_epoch: None,
            server_sequence: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_evidence(
        conflict_id: SyncConflictId,
        reason: impl Into<String>,
        replayed: bool,
        resource_id: NodeId,
        expected_revision: Option<Revision>,
        current_revision: Option<Revision>,
        current_state: Option<NodeState>,
        current_parent_id: Option<NodeId>,
        server_epoch: Sequence,
        server_sequence: Sequence,
    ) -> Result<Self, ClientSyncError> {
        let mut conflict = Self::new(conflict_id, reason, replayed)?;
        conflict.resource_id = Some(resource_id);
        conflict.expected_revision = expected_revision;
        conflict.current_revision = current_revision;
        conflict.current_state = current_state;
        conflict.current_parent_id = current_parent_id;
        conflict.server_epoch = Some(server_epoch);
        conflict.server_sequence = Some(server_sequence);
        Ok(conflict)
    }

    #[must_use]
    pub const fn conflict_id(&self) -> SyncConflictId {
        self.conflict_id
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    #[must_use]
    pub const fn replayed(&self) -> bool {
        self.replayed
    }

    #[must_use]
    pub const fn resource_id(&self) -> Option<NodeId> {
        self.resource_id
    }

    #[must_use]
    pub const fn expected_revision(&self) -> Option<Revision> {
        self.expected_revision
    }

    #[must_use]
    pub const fn current_revision(&self) -> Option<Revision> {
        self.current_revision
    }

    #[must_use]
    pub const fn current_state(&self) -> Option<NodeState> {
        self.current_state
    }

    #[must_use]
    pub const fn current_parent_id(&self) -> Option<NodeId> {
        self.current_parent_id
    }

    #[must_use]
    pub const fn server_epoch(&self) -> Option<Sequence> {
        self.server_epoch
    }

    #[must_use]
    pub const fn server_sequence(&self) -> Option<Sequence> {
        self.server_sequence
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RemoteMutationOutcome {
    Applied(RemoteMutationApplied),
    Conflict(RemoteMutationConflict),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UploadTarget {
    CreateFile {
        library_id: LibraryId,
        parent_node_id: NodeId,
        name: synveil_core::LogicalName,
    },
    ReplaceContent {
        library_id: LibraryId,
        node_id: NodeId,
        expected_revision: Revision,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadSessionStatus {
    session_id: UploadSessionId,
    target: UploadTarget,
    state: UploadSessionState,
    expected_length: u64,
    expected_sha256: Option<Sha256Digest>,
    received_bytes: u64,
    completion: Option<UploadCompletion>,
    version_conflict: bool,
}

impl UploadSessionStatus {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        session_id: UploadSessionId,
        target: UploadTarget,
        state: UploadSessionState,
        expected_length: u64,
        expected_sha256: Option<Sha256Digest>,
        received_bytes: u64,
        completion: Option<UploadCompletion>,
    ) -> Self {
        Self {
            session_id,
            target,
            state,
            expected_length,
            expected_sha256,
            received_bytes,
            completion,
            version_conflict: false,
        }
    }

    #[must_use]
    pub const fn with_version_conflict(mut self, version_conflict: bool) -> Self {
        self.version_conflict = version_conflict;
        self
    }

    #[must_use]
    pub const fn session_id(&self) -> UploadSessionId {
        self.session_id
    }

    #[must_use]
    pub const fn target(&self) -> &UploadTarget {
        &self.target
    }

    #[must_use]
    pub const fn state(&self) -> UploadSessionState {
        self.state
    }

    #[must_use]
    pub const fn expected_length(&self) -> u64 {
        self.expected_length
    }

    #[must_use]
    pub const fn expected_sha256(&self) -> Option<Sha256Digest> {
        self.expected_sha256
    }

    #[must_use]
    pub const fn received_bytes(&self) -> u64 {
        self.received_bytes
    }

    #[must_use]
    pub const fn completion(&self) -> Option<&UploadCompletion> {
        self.completion.as_ref()
    }

    #[must_use]
    pub const fn is_version_conflict(&self) -> bool {
        self.version_conflict
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadCompletion {
    session_id: UploadSessionId,
    node_id: NodeId,
    file_version_id: FileVersionId,
    node_revision: Revision,
    length: u64,
    sha256: Sha256Digest,
}

impl UploadCompletion {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        session_id: UploadSessionId,
        node_id: NodeId,
        file_version_id: FileVersionId,
        node_revision: Revision,
        length: u64,
        sha256: Sha256Digest,
    ) -> Self {
        Self {
            session_id,
            node_id,
            file_version_id,
            node_revision,
            length,
            sha256,
        }
    }

    #[must_use]
    pub const fn session_id(self) -> UploadSessionId {
        self.session_id
    }

    #[must_use]
    pub const fn node_id(self) -> NodeId {
        self.node_id
    }

    #[must_use]
    pub const fn file_version_id(self) -> FileVersionId {
        self.file_version_id
    }

    #[must_use]
    pub const fn node_revision(self) -> Revision {
        self.node_revision
    }

    #[must_use]
    pub const fn length(self) -> u64 {
        self.length
    }

    #[must_use]
    pub const fn sha256(self) -> Sha256Digest {
        self.sha256
    }
}

impl RemoteContent {
    #[must_use]
    pub fn new(
        node_id: NodeId,
        version_id: FileVersionId,
        length: u64,
        sha256: Sha256Digest,
        body: ContentByteStream,
    ) -> Self {
        Self {
            node_id,
            version_id,
            length,
            sha256,
            body,
        }
    }

    #[must_use]
    pub const fn node_id(&self) -> NodeId {
        self.node_id
    }

    #[must_use]
    pub const fn version_id(&self) -> FileVersionId {
        self.version_id
    }

    #[must_use]
    pub const fn length(&self) -> u64 {
        self.length
    }

    #[must_use]
    pub const fn sha256(&self) -> Sha256Digest {
        self.sha256
    }

    #[must_use]
    pub fn into_stream(self) -> ContentByteStream {
        self.body
    }
}

impl fmt::Debug for RemoteContent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteContent")
            .field("node_id", &self.node_id)
            .field("version_id", &self.version_id)
            .field("length", &self.length)
            .field("sha256", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteErrorKind {
    Offline,
    RebaselineRequired,
    NotFound,
    Integrity,
    Unavailable,
    Rejected,
    AuthRequired,
    DeviceRevoked,
    Forbidden,
    CheckpointConflict,
    Conflict,
    InvalidEvidence,
    RateLimited,
    Internal,
    Protocol,
    Tls,
    Timeout,
    BodyLimit,
    Redirect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoteError {
    kind: RemoteErrorKind,
    current_revision: Option<Revision>,
}

impl RemoteError {
    #[must_use]
    pub const fn new(kind: RemoteErrorKind) -> Self {
        Self {
            kind,
            current_revision: None,
        }
    }

    #[must_use]
    pub const fn with_current_revision(kind: RemoteErrorKind, current_revision: Revision) -> Self {
        Self {
            kind,
            current_revision: Some(current_revision),
        }
    }

    #[must_use]
    pub const fn kind(self) -> RemoteErrorKind {
        self.kind
    }

    #[must_use]
    pub const fn current_revision(self) -> Option<Revision> {
        self.current_revision
    }

    #[must_use]
    pub const fn code(self) -> &'static str {
        match self.kind {
            RemoteErrorKind::Offline => "REMOTE_OFFLINE",
            RemoteErrorKind::RebaselineRequired => "REMOTE_REBASELINE_REQUIRED",
            RemoteErrorKind::NotFound => "REMOTE_RESOURCE_NOT_FOUND",
            RemoteErrorKind::Integrity => "REMOTE_CONTENT_INTEGRITY_ERROR",
            RemoteErrorKind::Unavailable => "REMOTE_UNAVAILABLE",
            RemoteErrorKind::Rejected => "REMOTE_REQUEST_REJECTED",
            RemoteErrorKind::AuthRequired => "REMOTE_AUTH_REQUIRED",
            RemoteErrorKind::DeviceRevoked => "REMOTE_DEVICE_REVOKED",
            RemoteErrorKind::Forbidden => "REMOTE_PERMISSION_DENIED",
            RemoteErrorKind::CheckpointConflict => "REMOTE_CHECKPOINT_CONFLICT",
            RemoteErrorKind::Conflict => "REMOTE_STATE_CONFLICT",
            RemoteErrorKind::InvalidEvidence => "REMOTE_EVIDENCE_INVALID",
            RemoteErrorKind::RateLimited => "REMOTE_RATE_LIMITED",
            RemoteErrorKind::Internal => "REMOTE_INTERNAL_ERROR",
            RemoteErrorKind::Protocol => "REMOTE_PROTOCOL_ERROR",
            RemoteErrorKind::Tls => "REMOTE_TLS_ERROR",
            RemoteErrorKind::Timeout => "REMOTE_TIMEOUT",
            RemoteErrorKind::BodyLimit => "REMOTE_BODY_LIMIT",
            RemoteErrorKind::Redirect => "REMOTE_REDIRECT_REJECTED",
        }
    }

    /// A later runtime may retry safe operations after backoff. This is not
    /// permission to replay one-time enrollment or other non-idempotent work.
    #[must_use]
    pub const fn retryable(self) -> bool {
        matches!(
            self.kind,
            RemoteErrorKind::Offline
                | RemoteErrorKind::Unavailable
                | RemoteErrorKind::RateLimited
                | RemoteErrorKind::Timeout
                | RemoteErrorKind::Internal
        )
    }
}

impl fmt::Display for RemoteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for RemoteError {}

/// Narrow transport-neutral port implemented by a fake/in-process adapter in
/// this phase and by the production HTTP/auth adapter in Prompt 37.
#[async_trait]
pub trait SyncRemote: Send + Sync {
    /// Production transports identify the explicit server profile to which
    /// their credential and every request are bound. Transport-neutral test
    /// adapters can continue to use an unbound Prompt 36 replica.
    fn server_profile_id(&self) -> Option<crate::ServerProfileId> {
        None
    }

    /// Identifies the loaded credential generation so local forget/re-enroll
    /// can reject an already-constructed stale transport before networking.
    fn device_credential_id(&self) -> Option<synveil_core::DeviceCredentialId> {
        None
    }

    async fn get_checkpoint(&self, scope: ReplicaScope) -> Result<RemoteCheckpoint, RemoteError>;

    async fn fetch_changes(
        &self,
        scope: ReplicaScope,
        limit: u32,
    ) -> Result<RemoteFeedPage, RemoteError>;

    async fn acknowledge_changes(
        &self,
        scope: ReplicaScope,
        evidence: &OpaqueEvidence,
    ) -> Result<RemoteCheckpoint, RemoteError>;

    /// Complete the durable snapshot-to-checkpoint handoff. The boundary is
    /// intentionally absent from this port: the server derives it from its
    /// authenticated owner-scoped snapshot header.
    async fn complete_rebaseline_handoff(
        &self,
        _scope: ReplicaScope,
        _snapshot_id: RebaselineSnapshotId,
    ) -> Result<RebaselineHandoffConfirmation, RemoteError> {
        Err(RemoteError::new(RemoteErrorKind::Forbidden))
    }

    async fn start_rebaseline(&self, scope: ReplicaScope) -> Result<SyncBootstrap, RemoteError>;

    async fn fetch_rebaseline_page(
        &self,
        scope: ReplicaScope,
        bootstrap_id: SyncBootstrapId,
        cursor: Option<&OpaqueEvidence>,
        limit: u32,
    ) -> Result<BootstrapPage, RemoteError>;

    async fn complete_rebaseline(
        &self,
        scope: ReplicaScope,
        bootstrap_id: SyncBootstrapId,
        evidence: &OpaqueEvidence,
    ) -> Result<BootstrapCompletion, RemoteError>;

    async fn download_current_content(
        &self,
        scope: ReplicaScope,
        node_id: NodeId,
        version_id: FileVersionId,
    ) -> Result<RemoteContent, RemoteError>;

    async fn submit_client_mutation(
        &self,
        _scope: ReplicaScope,
        _request: &ClientMutationRequest,
    ) -> Result<RemoteMutationOutcome, RemoteError> {
        Err(RemoteError::new(RemoteErrorKind::Forbidden))
    }

    async fn create_upload_session(
        &self,
        _scope: ReplicaScope,
        _idempotency_key: OutboundIntentId,
        _target: &UploadTarget,
        _expected_length: u64,
        _expected_sha256: Sha256Digest,
    ) -> Result<UploadSessionStatus, RemoteError> {
        Err(RemoteError::new(RemoteErrorKind::Forbidden))
    }

    async fn get_upload_session(
        &self,
        _scope: ReplicaScope,
        _session_id: UploadSessionId,
    ) -> Result<UploadSessionStatus, RemoteError> {
        Err(RemoteError::new(RemoteErrorKind::Forbidden))
    }

    async fn append_upload_chunk(
        &self,
        _scope: ReplicaScope,
        _session_id: UploadSessionId,
        _expected_offset: u64,
        _chunk: bytes::Bytes,
    ) -> Result<u64, RemoteError> {
        Err(RemoteError::new(RemoteErrorKind::Forbidden))
    }

    async fn complete_upload(
        &self,
        _scope: ReplicaScope,
        _session_id: UploadSessionId,
    ) -> Result<UploadCompletion, RemoteError> {
        Err(RemoteError::new(RemoteErrorKind::Forbidden))
    }

    async fn abort_upload(
        &self,
        _scope: ReplicaScope,
        _session_id: UploadSessionId,
    ) -> Result<UploadSessionStatus, RemoteError> {
        Err(RemoteError::new(RemoteErrorKind::Forbidden))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngineStatus {
    Idle,
    Bootstrapping,
    Applying,
    AckPending,
    BlockedLocalIssue,
    Offline,
    Error,
}
