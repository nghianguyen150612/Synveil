use std::{
    fmt,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use fs2::FileExt;
use sqlx::{
    Row, Sqlite, SqlitePool, Transaction,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use synveil_core::{
    ChangeKind, ClientMutationId, ClientMutationRequest, FileVersionId, LibraryId, LogicalName,
    LogicalSnapshotNode, NodeId, NodeKind, NodeState, OutboundIntentId, Revision, Sequence,
    Sha256Digest, SyncBootstrap, SyncBootstrapId, UploadSessionId,
};
use synveil_platform::PlatformRuntime;
use uuid::Uuid;

use crate::{
    ClientSyncError, EngineStatus, InboundChange, LocalFingerprint, ManagedRelativePath,
    ObservationIssue, ObservationIssueKind, ObservationState, OpaqueEvidence, OutboundIntent,
    OutboundIntentKind, OutboundIntentState, RemoteFeedPage, RemoteMutationApplied,
    RemoteMutationConflict, ReplicaScope, RootBindingId, ServerProfileId, UploadCompletion,
    local_collision_key,
};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

const MAX_EVENT_EVIDENCE_ROWS: i64 = 4_096;
const MAX_LOCAL_NODE_QUERY_ROWS: usize = 100_000;
const MAX_RECOVERY_ROWS: usize = 4_096;
const MAX_OBSERVATION_QUERY_ROWS: usize = 4_096;

/// The locally observed projection of a tracked server Node. It overlays the
/// last server-applied `local_nodes` row without rewriting that immutable base
/// while an outbound intent remains unsent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ObservedLocalNode {
    pub node: LocalNode,
    pub relative_path: ManagedRelativePath,
    pub present: bool,
    pub fingerprint: Option<LocalFingerprint>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ObservationScanWork {
    pub generation: Sequence,
    pub relative_path: ManagedRelativePath,
    pub cursor_name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ObservationSuppression {
    pub operation_id: Uuid,
    pub node_id: NodeId,
    pub expected_relative_path: ManagedRelativePath,
    pub expected_present: bool,
    pub expected_fingerprint: Option<LocalFingerprint>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboundMutationRecord {
    intent_id: OutboundIntentId,
    mutation_id: ClientMutationId,
    request: ClientMutationRequest,
    request_json: String,
}

impl OutboundMutationRecord {
    #[must_use]
    pub const fn intent_id(&self) -> OutboundIntentId {
        self.intent_id
    }

    #[must_use]
    pub const fn mutation_id(&self) -> ClientMutationId {
        self.mutation_id
    }

    #[must_use]
    pub const fn request(&self) -> &ClientMutationRequest {
        &self.request
    }

    #[must_use]
    pub fn request_json(&self) -> &str {
        &self.request_json
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboundUploadRecord {
    intent_id: OutboundIntentId,
    upload_session_id: Option<UploadSessionId>,
    staging_relative_path: ManagedRelativePath,
    expected_length: u64,
    expected_sha256: Sha256Digest,
    acknowledged_offset: u64,
    state: String,
}

impl OutboundUploadRecord {
    #[must_use]
    pub const fn intent_id(&self) -> OutboundIntentId {
        self.intent_id
    }

    #[must_use]
    pub const fn upload_session_id(&self) -> Option<UploadSessionId> {
        self.upload_session_id
    }

    #[must_use]
    pub const fn staging_relative_path(&self) -> &ManagedRelativePath {
        &self.staging_relative_path
    }

    #[must_use]
    pub const fn expected_length(&self) -> u64 {
        self.expected_length
    }

    #[must_use]
    pub const fn expected_sha256(&self) -> Sha256Digest {
        self.expected_sha256
    }

    #[must_use]
    pub const fn acknowledged_offset(&self) -> u64 {
        self.acknowledged_offset
    }

    #[must_use]
    pub fn state(&self) -> &str {
        &self.state
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct LocalStateConfig {
    database_path: PathBuf,
}

impl fmt::Debug for LocalStateConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalStateConfig")
            .field("database_path", &"[REDACTED]")
            .finish()
    }
}

impl LocalStateConfig {
    #[must_use]
    pub fn new(database_path: impl Into<PathBuf>) -> Self {
        Self {
            database_path: database_path.into(),
        }
    }

    pub fn from_platform(runtime: &dyn PlatformRuntime) -> Result<Self, ClientSyncError> {
        let paths = runtime
            .resolve_paths()
            .map_err(|_| ClientSyncError::InvalidState)?;
        Ok(Self::new(
            paths.data_dir().as_path().join("client-sync/state.sqlite3"),
        ))
    }

    #[must_use]
    pub fn database_path(&self) -> &Path {
        &self.database_path
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplicaRecord {
    scope: ReplicaScope,
    server_profile_id: Option<ServerProfileId>,
    root_binding_id: RootBindingId,
    root_node_id: Option<NodeId>,
    journal_epoch: Sequence,
    applied_sequence: Sequence,
    acknowledged_sequence: Sequence,
    status: EngineStatus,
}

impl ReplicaRecord {
    #[must_use]
    pub const fn server_profile_id(&self) -> Option<ServerProfileId> {
        self.server_profile_id
    }

    #[must_use]
    pub const fn scope(&self) -> ReplicaScope {
        self.scope
    }

    #[must_use]
    pub const fn root_binding_id(&self) -> RootBindingId {
        self.root_binding_id
    }

    #[must_use]
    pub const fn root_node_id(&self) -> Option<NodeId> {
        self.root_node_id
    }

    #[must_use]
    pub const fn journal_epoch(&self) -> Sequence {
        self.journal_epoch
    }

    #[must_use]
    pub const fn applied_sequence(&self) -> Sequence {
        self.applied_sequence
    }

    #[must_use]
    pub const fn acknowledged_sequence(&self) -> Sequence {
        self.acknowledged_sequence
    }

    #[must_use]
    pub const fn status(&self) -> EngineStatus {
        self.status
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalNode {
    library_id: LibraryId,
    node_id: NodeId,
    parent_node_id: Option<NodeId>,
    relative_path: ManagedRelativePath,
    logical_name: LogicalName,
    kind: NodeKind,
    state: NodeState,
    revision: Revision,
    current_version_id: Option<FileVersionId>,
    content_length: Option<u64>,
    content_sha256: Option<Sha256Digest>,
    local_length: Option<u64>,
    local_sha256: Option<Sha256Digest>,
    bootstrap_generation: Sequence,
    present: bool,
    quarantine_relative_path: Option<ManagedRelativePath>,
}

impl LocalNode {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        library_id: LibraryId,
        node_id: NodeId,
        parent_node_id: Option<NodeId>,
        relative_path: ManagedRelativePath,
        logical_name: LogicalName,
        kind: NodeKind,
        state: NodeState,
        revision: Revision,
        current_version_id: Option<FileVersionId>,
        content_length: Option<u64>,
        content_sha256: Option<Sha256Digest>,
        local_length: Option<u64>,
        local_sha256: Option<Sha256Digest>,
        bootstrap_generation: Sequence,
        present: bool,
        quarantine_relative_path: Option<ManagedRelativePath>,
    ) -> Self {
        Self {
            library_id,
            node_id,
            parent_node_id,
            relative_path,
            logical_name,
            kind,
            state,
            revision,
            current_version_id,
            content_length,
            content_sha256,
            local_length,
            local_sha256,
            bootstrap_generation,
            present,
            quarantine_relative_path,
        }
    }

    #[must_use]
    pub const fn library_id(&self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn node_id(&self) -> NodeId {
        self.node_id
    }

    #[must_use]
    pub const fn parent_node_id(&self) -> Option<NodeId> {
        self.parent_node_id
    }

    #[must_use]
    pub const fn relative_path(&self) -> &ManagedRelativePath {
        &self.relative_path
    }

    #[must_use]
    pub const fn logical_name(&self) -> &LogicalName {
        &self.logical_name
    }

    #[must_use]
    pub const fn kind(&self) -> NodeKind {
        self.kind
    }

    #[must_use]
    pub const fn state(&self) -> NodeState {
        self.state
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    #[must_use]
    pub const fn current_version_id(&self) -> Option<FileVersionId> {
        self.current_version_id
    }

    #[must_use]
    pub const fn content_length(&self) -> Option<u64> {
        self.content_length
    }

    #[must_use]
    pub const fn content_sha256(&self) -> Option<Sha256Digest> {
        self.content_sha256
    }

    #[must_use]
    pub const fn local_length(&self) -> Option<u64> {
        self.local_length
    }

    #[must_use]
    pub const fn local_sha256(&self) -> Option<Sha256Digest> {
        self.local_sha256
    }

    #[must_use]
    pub const fn bootstrap_generation(&self) -> Sequence {
        self.bootstrap_generation
    }

    #[must_use]
    pub const fn present(&self) -> bool {
        self.present
    }

    #[must_use]
    pub const fn quarantine_relative_path(&self) -> Option<&ManagedRelativePath> {
        self.quarantine_relative_path.as_ref()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum LocalIssueKind {
    LocalDivergence,
    LocalPathOccupied,
    LocalNameUnrepresentable,
    LocalNameCollision,
    LocalParentMissing,
    LocalTypeMismatch,
    LocalIoUnavailable,
    ContentIntegrityMismatch,
    LocalRecoveryAmbiguous,
}

impl LocalIssueKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LocalDivergence => "LOCAL_DIVERGENCE",
            Self::LocalPathOccupied => "LOCAL_PATH_OCCUPIED",
            Self::LocalNameUnrepresentable => "LOCAL_NAME_UNREPRESENTABLE",
            Self::LocalNameCollision => "LOCAL_NAME_COLLISION",
            Self::LocalParentMissing => "LOCAL_PARENT_MISSING",
            Self::LocalTypeMismatch => "LOCAL_TYPE_MISMATCH",
            Self::LocalIoUnavailable => "LOCAL_IO_UNAVAILABLE",
            Self::ContentIntegrityMismatch => "CONTENT_INTEGRITY_MISMATCH",
            Self::LocalRecoveryAmbiguous => "LOCAL_RECOVERY_AMBIGUOUS",
        }
    }

    fn parse(value: &str) -> Result<Self, ClientSyncError> {
        match value {
            "LOCAL_DIVERGENCE" => Ok(Self::LocalDivergence),
            "LOCAL_PATH_OCCUPIED" => Ok(Self::LocalPathOccupied),
            "LOCAL_NAME_UNREPRESENTABLE" => Ok(Self::LocalNameUnrepresentable),
            "LOCAL_NAME_COLLISION" => Ok(Self::LocalNameCollision),
            "LOCAL_PARENT_MISSING" => Ok(Self::LocalParentMissing),
            "LOCAL_TYPE_MISMATCH" => Ok(Self::LocalTypeMismatch),
            "LOCAL_IO_UNAVAILABLE" => Ok(Self::LocalIoUnavailable),
            "CONTENT_INTEGRITY_MISMATCH" => Ok(Self::ContentIntegrityMismatch),
            "LOCAL_RECOVERY_AMBIGUOUS" => Ok(Self::LocalRecoveryAmbiguous),
            _ => Err(ClientSyncError::InvalidState),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalApplyIssue {
    issue_id: Uuid,
    library_id: LibraryId,
    node_id: Option<NodeId>,
    server_sequence: Option<Sequence>,
    bootstrap_generation: Option<Sequence>,
    kind: LocalIssueKind,
    expected_state: String,
}

impl LocalApplyIssue {
    #[must_use]
    pub const fn issue_id(&self) -> Uuid {
        self.issue_id
    }

    #[must_use]
    pub const fn library_id(&self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn node_id(&self) -> Option<NodeId> {
        self.node_id
    }

    #[must_use]
    pub const fn server_sequence(&self) -> Option<Sequence> {
        self.server_sequence
    }

    #[must_use]
    pub const fn bootstrap_generation(&self) -> Option<Sequence> {
        self.bootstrap_generation
    }

    #[must_use]
    pub const fn kind(&self) -> LocalIssueKind {
        self.kind
    }

    #[must_use]
    pub fn expected_state(&self) -> &str {
        &self.expected_state
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalOperationKind {
    CreateDirectory,
    ReplaceFile,
    Rename,
    Move,
    Trash,
    Restore,
    Purge,
}

impl LocalOperationKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CreateDirectory => "CREATE_DIRECTORY",
            Self::ReplaceFile => "REPLACE_FILE",
            Self::Rename => "RENAME",
            Self::Move => "MOVE",
            Self::Trash => "TRASH",
            Self::Restore => "RESTORE",
            Self::Purge => "PURGE",
        }
    }

    fn parse(value: &str) -> Result<Self, ClientSyncError> {
        match value {
            "CREATE_DIRECTORY" => Ok(Self::CreateDirectory),
            "REPLACE_FILE" => Ok(Self::ReplaceFile),
            "RENAME" => Ok(Self::Rename),
            "MOVE" => Ok(Self::Move),
            "TRASH" => Ok(Self::Trash),
            "RESTORE" => Ok(Self::Restore),
            "PURGE" => Ok(Self::Purge),
            _ => Err(ClientSyncError::InvalidState),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalOperationState {
    Prepared,
    FilesystemApplied,
    DatabaseCommitted,
    NeedsAttention,
}

impl LocalOperationState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "PREPARED",
            Self::FilesystemApplied => "FILESYSTEM_APPLIED",
            Self::DatabaseCommitted => "DATABASE_COMMITTED",
            Self::NeedsAttention => "NEEDS_ATTENTION",
        }
    }

    fn parse(value: &str) -> Result<Self, ClientSyncError> {
        match value {
            "PREPARED" => Ok(Self::Prepared),
            "FILESYSTEM_APPLIED" => Ok(Self::FilesystemApplied),
            "DATABASE_COMMITTED" => Ok(Self::DatabaseCommitted),
            "NEEDS_ATTENTION" => Ok(Self::NeedsAttention),
            _ => Err(ClientSyncError::InvalidState),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct LocalOperation {
    operation_id: Uuid,
    library_id: LibraryId,
    node_id: NodeId,
    server_sequence: Option<Sequence>,
    bootstrap_generation: Option<Sequence>,
    kind: LocalOperationKind,
    source: Option<ManagedRelativePath>,
    destination: Option<ManagedRelativePath>,
    staging: Option<ManagedRelativePath>,
    expected_kind: Option<NodeKind>,
    expected_length: Option<u64>,
    expected_sha256: Option<Sha256Digest>,
    desired_revision: Revision,
    desired_version_id: Option<FileVersionId>,
    desired_length: Option<u64>,
    desired_sha256: Option<Sha256Digest>,
    state: LocalOperationState,
}

impl fmt::Debug for LocalOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalOperation")
            .field("operation_id", &self.operation_id)
            .field("library_id", &self.library_id)
            .field("node_id", &self.node_id)
            .field("server_sequence", &self.server_sequence)
            .field("bootstrap_generation", &self.bootstrap_generation)
            .field("kind", &self.kind)
            .field("source", &self.source.as_ref().map(|_| "[REDACTED]"))
            .field(
                "destination",
                &self.destination.as_ref().map(|_| "[REDACTED]"),
            )
            .field("staging", &self.staging.as_ref().map(|_| "[REDACTED]"))
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl LocalOperation {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        library_id: LibraryId,
        node_id: NodeId,
        server_sequence: Option<Sequence>,
        bootstrap_generation: Option<Sequence>,
        kind: LocalOperationKind,
        source: Option<ManagedRelativePath>,
        destination: Option<ManagedRelativePath>,
        expected_kind: Option<NodeKind>,
        expected_length: Option<u64>,
        expected_sha256: Option<Sha256Digest>,
        desired_revision: Revision,
        desired_version_id: Option<FileVersionId>,
        desired_length: Option<u64>,
        desired_sha256: Option<Sha256Digest>,
    ) -> Self {
        Self {
            operation_id: Uuid::now_v7(),
            library_id,
            node_id,
            server_sequence,
            bootstrap_generation,
            kind,
            source,
            destination,
            staging: None,
            expected_kind,
            expected_length,
            expected_sha256,
            desired_revision,
            desired_version_id,
            desired_length,
            desired_sha256,
            state: LocalOperationState::Prepared,
        }
    }

    #[must_use]
    pub const fn operation_id(&self) -> Uuid {
        self.operation_id
    }

    #[must_use]
    pub const fn library_id(&self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn node_id(&self) -> NodeId {
        self.node_id
    }

    #[must_use]
    pub const fn server_sequence(&self) -> Option<Sequence> {
        self.server_sequence
    }

    #[must_use]
    pub const fn bootstrap_generation(&self) -> Option<Sequence> {
        self.bootstrap_generation
    }

    #[must_use]
    pub const fn kind(&self) -> LocalOperationKind {
        self.kind
    }

    #[must_use]
    pub const fn source(&self) -> Option<&ManagedRelativePath> {
        self.source.as_ref()
    }

    #[must_use]
    pub const fn destination(&self) -> Option<&ManagedRelativePath> {
        self.destination.as_ref()
    }

    #[must_use]
    pub const fn staging(&self) -> Option<&ManagedRelativePath> {
        self.staging.as_ref()
    }

    #[must_use]
    pub const fn expected_kind(&self) -> Option<NodeKind> {
        self.expected_kind
    }

    #[must_use]
    pub const fn expected_length(&self) -> Option<u64> {
        self.expected_length
    }

    #[must_use]
    pub const fn expected_sha256(&self) -> Option<Sha256Digest> {
        self.expected_sha256
    }

    #[must_use]
    pub const fn desired_revision(&self) -> Revision {
        self.desired_revision
    }

    #[must_use]
    pub const fn desired_version_id(&self) -> Option<FileVersionId> {
        self.desired_version_id
    }

    #[must_use]
    pub const fn desired_length(&self) -> Option<u64> {
        self.desired_length
    }

    #[must_use]
    pub const fn desired_sha256(&self) -> Option<Sha256Digest> {
        self.desired_sha256
    }

    #[must_use]
    pub const fn state(&self) -> LocalOperationState {
        self.state
    }

    pub fn set_staging(&mut self, staging: ManagedRelativePath) {
        self.staging = Some(staging);
    }

    pub(crate) fn set_state(&mut self, state: LocalOperationState) {
        self.state = state;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingAck {
    library_id: LibraryId,
    epoch: Sequence,
    from_sequence: Sequence,
    through_sequence: Sequence,
    high_watermark: Sequence,
    evidence: OpaqueEvidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OutboundSubmissionResult {
    pub resource_id: NodeId,
    pub revision: Revision,
    pub file_version_id: Option<FileVersionId>,
    pub length: Option<u64>,
    pub sha256: Option<Sha256Digest>,
}

impl OutboundSubmissionResult {
    pub(crate) fn matches_desired(self, desired: &LogicalSnapshotNode) -> bool {
        if self.resource_id != desired.node_id() || self.revision != desired.revision() {
            return false;
        }
        if self.file_version_id.is_none() && self.length.is_none() && self.sha256.is_none() {
            return true;
        }
        self.file_version_id == desired.current_version_id()
            && self.length == desired.content_length()
            && self.sha256 == desired.content_sha256()
    }
}

impl PendingAck {
    #[must_use]
    pub const fn library_id(&self) -> LibraryId {
        self.library_id
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
    pub const fn evidence(&self) -> &OpaqueEvidence {
        &self.evidence
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootstrapRecord {
    library_id: LibraryId,
    bootstrap_id: SyncBootstrapId,
    generation: Sequence,
    snapshot_epoch: Sequence,
    resume_sequence: Sequence,
    manifest_item_count: u64,
    state: String,
    next_cursor: Option<OpaqueEvidence>,
    completion_evidence: Option<OpaqueEvidence>,
    terminal_fetched: bool,
}

impl BootstrapRecord {
    #[must_use]
    pub const fn library_id(&self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn bootstrap_id(&self) -> SyncBootstrapId {
        self.bootstrap_id
    }

    #[must_use]
    pub const fn generation(&self) -> Sequence {
        self.generation
    }

    #[must_use]
    pub const fn snapshot_epoch(&self) -> Sequence {
        self.snapshot_epoch
    }

    #[must_use]
    pub const fn resume_sequence(&self) -> Sequence {
        self.resume_sequence
    }

    #[must_use]
    pub const fn manifest_item_count(&self) -> u64 {
        self.manifest_item_count
    }

    #[must_use]
    pub fn state(&self) -> &str {
        &self.state
    }

    #[must_use]
    pub const fn next_cursor(&self) -> Option<&OpaqueEvidence> {
        self.next_cursor.as_ref()
    }

    #[must_use]
    pub const fn completion_evidence(&self) -> Option<&OpaqueEvidence> {
        self.completion_evidence.as_ref()
    }

    #[must_use]
    pub const fn terminal_fetched(&self) -> bool {
        self.terminal_fetched
    }
}

#[derive(Clone, Debug)]
pub(crate) struct StoredPage {
    pub epoch: Sequence,
    pub from_sequence: Sequence,
    pub through_sequence: Sequence,
    pub high_watermark: Sequence,
    pub has_more: bool,
    pub ack_evidence: Option<OpaqueEvidence>,
    pub changes: Vec<StoredChange>,
}

#[derive(Clone, Debug)]
pub(crate) struct StoredChange {
    pub sequence: Sequence,
    pub event_id: synveil_core::ChangeEventId,
    pub change_kind: ChangeKind,
    pub resource_id: NodeId,
    pub resource_revision: Revision,
    pub desired_node: Option<LogicalSnapshotNode>,
}

pub struct LocalStateStore {
    pub(crate) pool: SqlitePool,
    pub(crate) credential_lifecycle_lock: tokio::sync::Mutex<()>,
    replica_writer_guard: tokio::sync::Mutex<()>,
    writer_lock: File,
    database_path: PathBuf,
}

impl fmt::Debug for LocalStateStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalStateStore")
            .field("database_path", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl LocalStateStore {
    pub async fn open(config: &LocalStateConfig) -> Result<Self, ClientSyncError> {
        let database_path = config.database_path();
        if !database_path.is_absolute() {
            return Err(ClientSyncError::InvalidState);
        }
        let parent = database_path
            .parent()
            .ok_or(ClientSyncError::InvalidState)?;
        fs::create_dir_all(parent)?;
        let lock_path = database_path.with_extension("sqlite3.writer.lock");
        let writer_lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        FileExt::try_lock_exclusive(&writer_lock).map_err(|_| ClientSyncError::ConcurrentWriter)?;

        let options = SqliteConnectOptions::new()
            .filename(database_path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .busy_timeout(std::time::Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        MIGRATOR
            .run(&pool)
            .await
            .map_err(|_| ClientSyncError::Database)?;
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await?;

        Ok(Self {
            pool,
            credential_lifecycle_lock: tokio::sync::Mutex::new(()),
            replica_writer_guard: tokio::sync::Mutex::new(()),
            writer_lock,
            database_path: database_path.to_path_buf(),
        })
    }

    #[must_use]
    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    /// Coordinates inbound filesystem application and outbound reinspection
    /// inside this process. The adjacent `fs2` lock still prevents a second
    /// process from opening the same local state database.
    pub(crate) async fn lock_replica_writer(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.replica_writer_guard.lock().await
    }

    pub async fn schema_version(&self) -> Result<i64, ClientSyncError> {
        sqlx::query_scalar(
            "SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success = 1",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn bind_replica(
        &self,
        scope: ReplicaScope,
        binding_id: RootBindingId,
    ) -> Result<ReplicaRecord, ClientSyncError> {
        self.bind_replica_inner(scope, binding_id, None).await
    }

    /// Bind a fresh replica to an explicit enrolled server profile. Legacy
    /// unbound replicas cannot be silently migrated or rebound.
    pub async fn bind_replica_to_profile(
        &self,
        scope: ReplicaScope,
        binding_id: RootBindingId,
        profile_id: ServerProfileId,
    ) -> Result<ReplicaRecord, ClientSyncError> {
        if self.server_profile(profile_id).await?.is_none() {
            return Err(ClientSyncError::InvalidServerProfile);
        }
        let enrollment = self
            .profile_enrollment(profile_id)
            .await?
            .ok_or(ClientSyncError::AuthenticationRequired)?;
        if enrollment.owner_user_id() != scope.owner_user_id()
            || enrollment.device_id() != scope.device_id()
        {
            return Err(ClientSyncError::WrongScope);
        }
        if enrollment.forgotten_at_ms().is_some() {
            return Err(ClientSyncError::AuthenticationRequired);
        }
        self.bind_replica_inner(scope, binding_id, Some(profile_id))
            .await
    }

    async fn bind_replica_inner(
        &self,
        scope: ReplicaScope,
        binding_id: RootBindingId,
        profile_id: Option<ServerProfileId>,
    ) -> Result<ReplicaRecord, ClientSyncError> {
        let now = now_ms()?;
        sqlx::query(
            "INSERT INTO replicas (
                 library_id, owner_user_id, device_id, root_binding_id,
                 created_at_ms, updated_at_ms, server_profile_id
             ) VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(library_id) DO NOTHING",
        )
        .bind(scope.library_id().to_string())
        .bind(scope.owner_user_id().to_string())
        .bind(scope.device_id().to_string())
        .bind(binding_id.to_string())
        .bind(now)
        .bind(now)
        .bind(profile_id.map(|value| value.to_string()))
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO observation_state (library_id, updated_at_ms)
             VALUES (?, ?) ON CONFLICT(library_id) DO NOTHING",
        )
        .bind(scope.library_id().to_string())
        .bind(now)
        .execute(&self.pool)
        .await?;
        let record = self
            .replica(scope.library_id())
            .await?
            .ok_or(ClientSyncError::InvalidState)?;
        if record.scope() != scope || record.root_binding_id() != binding_id {
            return Err(ClientSyncError::WrongRootBinding);
        }
        if record.server_profile_id() != profile_id {
            return Err(ClientSyncError::WrongServerProfile);
        }
        Ok(record)
    }

    pub async fn replica(
        &self,
        library_id: LibraryId,
    ) -> Result<Option<ReplicaRecord>, ClientSyncError> {
        let row = sqlx::query(
            "SELECT owner_user_id, device_id, library_id, root_binding_id,
                    root_node_id, journal_epoch, applied_sequence,
                    acknowledged_sequence, status, server_profile_id
             FROM replicas WHERE library_id = ?",
        )
        .bind(library_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(decode_replica).transpose()
    }

    pub async fn set_status(
        &self,
        library_id: LibraryId,
        status: EngineStatus,
    ) -> Result<(), ClientSyncError> {
        let changed =
            sqlx::query("UPDATE replicas SET status = ?, updated_at_ms = ? WHERE library_id = ?")
                .bind(status_as_str(status))
                .bind(now_ms()?)
                .bind(library_id.to_string())
                .execute(&self.pool)
                .await?
                .rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        Ok(())
    }

    pub async fn local_node(
        &self,
        library_id: LibraryId,
        node_id: NodeId,
    ) -> Result<Option<LocalNode>, ClientSyncError> {
        let row = sqlx::query("SELECT * FROM local_nodes WHERE library_id = ? AND node_id = ?")
            .bind(library_id.to_string())
            .bind(node_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(decode_local_node).transpose()
    }

    pub async fn local_node_at_collision_key(
        &self,
        library_id: LibraryId,
        collision_key: &str,
    ) -> Result<Option<LocalNode>, ClientSyncError> {
        let row =
            sqlx::query("SELECT * FROM local_nodes WHERE library_id = ? AND collision_key = ?")
                .bind(library_id.to_string())
                .bind(collision_key)
                .fetch_optional(&self.pool)
                .await?;
        row.map(decode_local_node).transpose()
    }

    pub async fn local_nodes(
        &self,
        library_id: LibraryId,
    ) -> Result<Vec<LocalNode>, ClientSyncError> {
        let rows = sqlx::query("SELECT * FROM local_nodes WHERE library_id = ? ORDER BY length(relative_path), relative_path LIMIT 100001")
            .bind(library_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        if rows.len() > MAX_LOCAL_NODE_QUERY_ROWS {
            return Err(ClientSyncError::ResourceLimit);
        }
        rows.into_iter().map(decode_local_node).collect()
    }

    pub async fn upsert_local_node(&self, node: &LocalNode) -> Result<(), ClientSyncError> {
        let mut transaction = self.pool.begin().await?;
        upsert_local_node_tx(&mut transaction, node).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn remove_local_node(
        &self,
        library_id: LibraryId,
        node_id: NodeId,
    ) -> Result<(), ClientSyncError> {
        sqlx::query("DELETE FROM local_nodes WHERE library_id = ? AND node_id = ?")
            .bind(library_id.to_string())
            .bind(node_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn persist_page(&self, page: &RemoteFeedPage) -> Result<(), ClientSyncError> {
        validate_page(page)?;
        let library_id = page.scope().library_id();
        let mut transaction = self.pool.begin().await?;
        let existing: Option<(i64, i64, i64)> = sqlx::query_as(
            "SELECT epoch, from_sequence, through_sequence FROM pending_pages WHERE library_id = ?",
        )
        .bind(library_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some((epoch, from, through)) = existing {
            if epoch != sequence_i64(page.epoch())?
                || from != sequence_i64(page.from_sequence())?
                || through != sequence_i64(page.through_sequence())?
            {
                return Err(ClientSyncError::InvalidState);
            }
            transaction.commit().await?;
            return Ok(());
        }
        let now = now_ms()?;
        sqlx::query(
            "INSERT INTO pending_pages (
                 library_id, epoch, from_sequence, through_sequence,
                 high_watermark, has_more, ack_evidence, state,
                 created_at_ms, updated_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?, 'RECEIVED', ?, ?)",
        )
        .bind(library_id.to_string())
        .bind(sequence_i64(page.epoch())?)
        .bind(sequence_i64(page.from_sequence())?)
        .bind(sequence_i64(page.through_sequence())?)
        .bind(sequence_i64(page.high_watermark())?)
        .bind(page.has_more())
        .bind(page.ack_evidence().map(OpaqueEvidence::as_bytes))
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        for change in page.changes() {
            insert_pending_change(&mut transaction, library_id, change).await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub(crate) async fn pending_page(
        &self,
        library_id: LibraryId,
    ) -> Result<Option<StoredPage>, ClientSyncError> {
        let Some(row) = sqlx::query("SELECT * FROM pending_pages WHERE library_id = ?")
            .bind(library_id.to_string())
            .fetch_optional(&self.pool)
            .await?
        else {
            return Ok(None);
        };
        let changes = self.pending_changes(library_id).await?;
        Ok(Some(StoredPage {
            epoch: sequence_from_i64(row.try_get("epoch")?)?,
            from_sequence: sequence_from_i64(row.try_get("from_sequence")?)?,
            through_sequence: sequence_from_i64(row.try_get("through_sequence")?)?,
            high_watermark: sequence_from_i64(row.try_get("high_watermark")?)?,
            has_more: row.try_get("has_more")?,
            ack_evidence: optional_evidence(row.try_get("ack_evidence")?)?,
            changes,
        }))
    }

    async fn pending_changes(
        &self,
        library_id: LibraryId,
    ) -> Result<Vec<StoredChange>, ClientSyncError> {
        let rows =
            sqlx::query("SELECT * FROM pending_events WHERE library_id = ? ORDER BY sequence")
                .bind(library_id.to_string())
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter().map(decode_stored_change).collect()
    }

    pub(crate) async fn mark_page_applying(
        &self,
        library_id: LibraryId,
    ) -> Result<(), ClientSyncError> {
        let changed = sqlx::query("UPDATE pending_pages SET state = 'APPLYING', updated_at_ms = ? WHERE library_id = ? AND state IN ('RECEIVED','APPLYING')")
            .bind(now_ms()?)
            .bind(library_id.to_string())
            .execute(&self.pool)
            .await?
            .rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        self.set_status(library_id, EngineStatus::Applying).await
    }

    pub(crate) async fn record_event_applied(
        &self,
        scope: ReplicaScope,
        epoch: Sequence,
        change: &StoredChange,
        node: Option<&LocalNode>,
        operation_id: Option<Uuid>,
    ) -> Result<(), ClientSyncError> {
        let mut transaction = self.pool.begin().await?;
        let applied: i64 =
            sqlx::query_scalar("SELECT applied_sequence FROM replicas WHERE library_id = ?")
                .bind(scope.library_id().to_string())
                .fetch_one(&mut *transaction)
                .await?;
        let expected = applied
            .checked_add(1)
            .ok_or(ClientSyncError::InvalidState)?;
        let sequence = sequence_i64(change.sequence)?;
        if sequence < expected {
            let evidence: Option<(String, String, i64)> = sqlx::query_as(
                "SELECT event_id, resource_id, resource_revision
                 FROM applied_events
                 WHERE library_id = ? AND epoch = ? AND sequence = ?",
            )
            .bind(scope.library_id().to_string())
            .bind(sequence_i64(epoch)?)
            .bind(sequence)
            .fetch_optional(&mut *transaction)
            .await?;
            let expected_evidence = (
                change.event_id.to_string(),
                change.resource_id.to_string(),
                revision_i64(change.resource_revision)?,
            );
            if evidence.as_ref() != Some(&expected_evidence) {
                return Err(ClientSyncError::SequenceRegression);
            }
            transaction.commit().await?;
            return Ok(());
        }
        if sequence != expected {
            return Err(ClientSyncError::SequenceGap);
        }

        if let Some(node) = node {
            upsert_local_node_tx(&mut transaction, node).await?;
        } else {
            sqlx::query("DELETE FROM local_nodes WHERE library_id = ? AND node_id = ?")
                .bind(scope.library_id().to_string())
                .bind(change.resource_id.to_string())
                .execute(&mut *transaction)
                .await?;
        }
        sqlx::query(
            "INSERT INTO applied_events (
                 library_id, epoch, sequence, event_id, resource_id, resource_revision
             ) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(scope.library_id().to_string())
        .bind(sequence_i64(epoch)?)
        .bind(sequence)
        .bind(change.event_id.to_string())
        .bind(change.resource_id.to_string())
        .bind(revision_i64(change.resource_revision)?)
        .execute(&mut *transaction)
        .await?;
        let changed = sqlx::query(
            "UPDATE replicas SET applied_sequence = ?, updated_at_ms = ?
             WHERE library_id = ? AND applied_sequence = ? AND acknowledged_sequence <= ?",
        )
        .bind(sequence)
        .bind(now_ms()?)
        .bind(scope.library_id().to_string())
        .bind(applied)
        .bind(sequence)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        if let Some(operation_id) = operation_id {
            insert_observation_suppression_tx(&mut transaction, operation_id, node).await?;
            let changed = sqlx::query(
                "UPDATE local_operations SET state = 'DATABASE_COMMITTED', updated_at_ms = ?
                 WHERE operation_id = ? AND state = 'FILESYSTEM_APPLIED'",
            )
            .bind(now_ms()?)
            .bind(operation_id.to_string())
            .execute(&mut *transaction)
            .await?
            .rows_affected();
            if changed != 1 {
                return Err(ClientSyncError::InvalidState);
            }
        }
        trim_event_evidence(&mut transaction, scope.library_id(), epoch).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub(crate) async fn mark_page_locally_committed(
        &self,
        library_id: LibraryId,
    ) -> Result<Option<PendingAck>, ClientSyncError> {
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT epoch, from_sequence, through_sequence, high_watermark, ack_evidence
             FROM pending_pages WHERE library_id = ?",
        )
        .bind(library_id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        let applied: i64 =
            sqlx::query_scalar("SELECT applied_sequence FROM replicas WHERE library_id = ?")
                .bind(library_id.to_string())
                .fetch_one(&mut *transaction)
                .await?;
        let through: i64 = row.try_get("through_sequence")?;
        if applied != through {
            return Err(ClientSyncError::InvalidState);
        }
        let evidence: Option<Vec<u8>> = row.try_get("ack_evidence")?;
        if through == row.try_get::<i64, _>("from_sequence")? {
            sqlx::query("DELETE FROM pending_pages WHERE library_id = ?")
                .bind(library_id.to_string())
                .execute(&mut *transaction)
                .await?;
            transaction.commit().await?;
            return Ok(None);
        }
        let evidence = evidence.ok_or(ClientSyncError::InvalidState)?;
        sqlx::query(
            "INSERT INTO pending_acknowledgements (
                 library_id, epoch, from_sequence, through_sequence,
                 high_watermark, evidence, created_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(library_id) DO UPDATE SET
                 epoch = excluded.epoch,
                 from_sequence = excluded.from_sequence,
                 through_sequence = excluded.through_sequence,
                 high_watermark = excluded.high_watermark,
                 evidence = excluded.evidence",
        )
        .bind(library_id.to_string())
        .bind(row.try_get::<i64, _>("epoch")?)
        .bind(row.try_get::<i64, _>("from_sequence")?)
        .bind(through)
        .bind(row.try_get::<i64, _>("high_watermark")?)
        .bind(&evidence)
        .bind(now_ms()?)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("UPDATE pending_pages SET state = 'ACK_PENDING', updated_at_ms = ? WHERE library_id = ?")
            .bind(now_ms()?)
            .bind(library_id.to_string())
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "UPDATE replicas SET status = 'ACK_PENDING', updated_at_ms = ? WHERE library_id = ?",
        )
        .bind(now_ms()?)
        .bind(library_id.to_string())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        self.pending_ack(library_id).await
    }

    pub async fn pending_ack(
        &self,
        library_id: LibraryId,
    ) -> Result<Option<PendingAck>, ClientSyncError> {
        let row = sqlx::query("SELECT * FROM pending_acknowledgements WHERE library_id = ?")
            .bind(library_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| {
            Ok(PendingAck {
                library_id,
                epoch: sequence_from_i64(row.try_get("epoch")?)?,
                from_sequence: sequence_from_i64(row.try_get("from_sequence")?)?,
                through_sequence: sequence_from_i64(row.try_get("through_sequence")?)?,
                high_watermark: sequence_from_i64(row.try_get("high_watermark")?)?,
                evidence: OpaqueEvidence::new(row.try_get::<Vec<u8>, _>("evidence")?)?,
            })
        })
        .transpose()
    }

    pub(crate) async fn confirm_ack(
        &self,
        scope: ReplicaScope,
        epoch: Sequence,
        acknowledged: Sequence,
    ) -> Result<(), ClientSyncError> {
        let mut transaction = self.pool.begin().await?;
        let pending: (i64, i64) = sqlx::query_as(
            "SELECT epoch, through_sequence FROM pending_acknowledgements WHERE library_id = ?",
        )
        .bind(scope.library_id().to_string())
        .fetch_one(&mut *transaction)
        .await?;
        let applied: i64 =
            sqlx::query_scalar("SELECT applied_sequence FROM replicas WHERE library_id = ?")
                .bind(scope.library_id().to_string())
                .fetch_one(&mut *transaction)
                .await?;
        let acknowledged = sequence_i64(acknowledged)?;
        if pending.0 != sequence_i64(epoch)? || acknowledged < pending.1 || acknowledged > applied {
            return Err(ClientSyncError::InvalidRemoteResponse);
        }
        let changed = sqlx::query(
            "UPDATE replicas SET journal_epoch = ?, acknowledged_sequence = ?,
                    status = 'IDLE', updated_at_ms = ?
             WHERE library_id = ? AND acknowledged_sequence <= ? AND ? <= applied_sequence",
        )
        .bind(sequence_i64(epoch)?)
        .bind(acknowledged)
        .bind(now_ms()?)
        .bind(scope.library_id().to_string())
        .bind(acknowledged)
        .bind(acknowledged)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        let deleted = sqlx::query("DELETE FROM pending_acknowledgements WHERE library_id = ?")
            .bind(scope.library_id().to_string())
            .execute(&mut *transaction)
            .await?
            .rows_affected();
        if deleted != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        let deleted = sqlx::query("DELETE FROM pending_pages WHERE library_id = ?")
            .bind(scope.library_id().to_string())
            .execute(&mut *transaction)
            .await?
            .rows_affected();
        if deleted != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn prepare_operation(
        &self,
        operation: &LocalOperation,
    ) -> Result<(), ClientSyncError> {
        sqlx::query(
            "INSERT INTO local_operations (
                 operation_id, library_id, node_id, server_sequence,
                 bootstrap_generation, operation_kind, source_relative_path,
                 destination_relative_path, staging_relative_path, expected_kind,
                 expected_length, expected_sha256, desired_revision,
                 desired_version_id, desired_length, desired_sha256, state,
                 created_at_ms, updated_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(operation.operation_id().to_string())
        .bind(operation.library_id().to_string())
        .bind(operation.node_id().to_string())
        .bind(optional_sequence_i64(operation.server_sequence())?)
        .bind(optional_sequence_i64(operation.bootstrap_generation())?)
        .bind(operation.kind().as_str())
        .bind(operation.source().map(ManagedRelativePath::as_str))
        .bind(operation.destination().map(ManagedRelativePath::as_str))
        .bind(operation.staging().map(ManagedRelativePath::as_str))
        .bind(operation.expected_kind().map(node_kind_as_str))
        .bind(optional_u64_i64(operation.expected_length())?)
        .bind(
            operation
                .expected_sha256()
                .map(|hash| hash.into_bytes().to_vec()),
        )
        .bind(revision_i64(operation.desired_revision())?)
        .bind(operation.desired_version_id().map(|id| id.to_string()))
        .bind(optional_u64_i64(operation.desired_length())?)
        .bind(
            operation
                .desired_sha256()
                .map(|hash| hash.into_bytes().to_vec()),
        )
        .bind(operation.state().as_str())
        .bind(now_ms()?)
        .bind(now_ms()?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_operation_staging(
        &self,
        operation_id: Uuid,
        staging: &ManagedRelativePath,
    ) -> Result<(), ClientSyncError> {
        sqlx::query(
            "UPDATE local_operations SET staging_relative_path = ?, updated_at_ms = ?
             WHERE operation_id = ? AND state = 'PREPARED'",
        )
        .bind(staging.as_str())
        .bind(now_ms()?)
        .bind(operation_id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_operation_state(
        &self,
        operation_id: Uuid,
        state: LocalOperationState,
    ) -> Result<(), ClientSyncError> {
        let changed = sqlx::query(
            "UPDATE local_operations SET state = ?, updated_at_ms = ? WHERE operation_id = ?",
        )
        .bind(state.as_str())
        .bind(now_ms()?)
        .bind(operation_id.to_string())
        .execute(&self.pool)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        Ok(())
    }

    pub async fn unfinished_operations(
        &self,
        library_id: LibraryId,
    ) -> Result<Vec<LocalOperation>, ClientSyncError> {
        let rows = sqlx::query(
            "SELECT * FROM local_operations
             WHERE library_id = ? AND state != 'DATABASE_COMMITTED'
             ORDER BY created_at_ms, operation_id LIMIT 4097",
        )
        .bind(library_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        if rows.len() > MAX_RECOVERY_ROWS {
            return Err(ClientSyncError::ResourceLimit);
        }
        rows.into_iter().map(decode_operation).collect()
    }

    pub(crate) async fn operation_for_fact(
        &self,
        library_id: LibraryId,
        node_id: NodeId,
        server_sequence: Option<Sequence>,
        bootstrap_generation: Option<Sequence>,
    ) -> Result<Option<LocalOperation>, ClientSyncError> {
        let row = sqlx::query(
            "SELECT * FROM local_operations
             WHERE library_id = ? AND node_id = ?
               AND server_sequence IS ? AND bootstrap_generation IS ?
               AND state != 'DATABASE_COMMITTED'
             ORDER BY created_at_ms LIMIT 1",
        )
        .bind(library_id.to_string())
        .bind(node_id.to_string())
        .bind(optional_sequence_i64(server_sequence)?)
        .bind(optional_sequence_i64(bootstrap_generation)?)
        .fetch_optional(&self.pool)
        .await?;
        row.map(decode_operation).transpose()
    }

    pub(crate) async fn commit_removed_node_operation(
        &self,
        library_id: LibraryId,
        node_id: NodeId,
        operation_id: Uuid,
    ) -> Result<(), ClientSyncError> {
        let mut transaction = self.pool.begin().await?;
        insert_observation_suppression_tx(&mut transaction, operation_id, None).await?;
        let deleted = sqlx::query("DELETE FROM local_nodes WHERE library_id = ? AND node_id = ?")
            .bind(library_id.to_string())
            .bind(node_id.to_string())
            .execute(&mut *transaction)
            .await?
            .rows_affected();
        if deleted != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        let changed = sqlx::query(
            "UPDATE local_operations SET state = 'DATABASE_COMMITTED', updated_at_ms = ?
             WHERE operation_id = ? AND state = 'FILESYSTEM_APPLIED'",
        )
        .bind(now_ms()?)
        .bind(operation_id.to_string())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        transaction.commit().await?;
        Ok(())
    }

    pub(crate) async fn completed_operations(
        &self,
        library_id: LibraryId,
    ) -> Result<Vec<LocalOperation>, ClientSyncError> {
        let rows = sqlx::query(
            "SELECT * FROM local_operations
             WHERE library_id = ? AND state = 'DATABASE_COMMITTED'
             ORDER BY updated_at_ms, operation_id LIMIT 4097",
        )
        .bind(library_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        if rows.len() > MAX_RECOVERY_ROWS {
            return Err(ClientSyncError::ResourceLimit);
        }
        rows.into_iter().map(decode_operation).collect()
    }

    pub(crate) async fn delete_completed_operation(
        &self,
        operation_id: Uuid,
    ) -> Result<(), ClientSyncError> {
        let deleted = sqlx::query(
            "DELETE FROM local_operations
             WHERE operation_id = ? AND state = 'DATABASE_COMMITTED'",
        )
        .bind(operation_id.to_string())
        .execute(&self.pool)
        .await?
        .rows_affected();
        if deleted != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        Ok(())
    }

    pub async fn persist_issue(
        &self,
        library_id: LibraryId,
        node_id: Option<NodeId>,
        server_sequence: Option<Sequence>,
        bootstrap_generation: Option<Sequence>,
        kind: LocalIssueKind,
        expected_state: &str,
    ) -> Result<LocalApplyIssue, ClientSyncError> {
        if expected_state.is_empty() || expected_state.len() > 1_024 {
            return Err(ClientSyncError::ResourceLimit);
        }
        let issue_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO local_apply_issues (
                 issue_id, library_id, node_id, server_sequence,
                 bootstrap_generation, issue_kind, expected_state, created_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT DO NOTHING",
        )
        .bind(issue_id.to_string())
        .bind(library_id.to_string())
        .bind(node_id.map(|id| id.to_string()))
        .bind(optional_sequence_i64(server_sequence)?)
        .bind(optional_sequence_i64(bootstrap_generation)?)
        .bind(kind.as_str())
        .bind(expected_state)
        .bind(now_ms()?)
        .execute(&self.pool)
        .await?;
        self.set_status(library_id, EngineStatus::BlockedLocalIssue)
            .await?;
        let row = sqlx::query(
            "SELECT * FROM local_apply_issues
             WHERE library_id = ? AND COALESCE(node_id, '') = COALESCE(?, '')
               AND COALESCE(server_sequence, -1) = COALESCE(?, -1)
               AND COALESCE(bootstrap_generation, -1) = COALESCE(?, -1)
               AND issue_kind = ? AND resolved_at_ms IS NULL",
        )
        .bind(library_id.to_string())
        .bind(node_id.map(|id| id.to_string()))
        .bind(optional_sequence_i64(server_sequence)?)
        .bind(optional_sequence_i64(bootstrap_generation)?)
        .bind(kind.as_str())
        .fetch_one(&self.pool)
        .await?;
        decode_issue(row)
    }

    pub async fn unresolved_issues(
        &self,
        library_id: LibraryId,
    ) -> Result<Vec<LocalApplyIssue>, ClientSyncError> {
        let rows = sqlx::query(
            "SELECT * FROM local_apply_issues
             WHERE library_id = ? AND resolved_at_ms IS NULL
             ORDER BY created_at_ms, issue_id LIMIT 4097",
        )
        .bind(library_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        if rows.len() > MAX_RECOVERY_ROWS {
            return Err(ClientSyncError::ResourceLimit);
        }
        rows.into_iter().map(decode_issue).collect()
    }

    /// Explicitly clear one local blocker after a caller has inspected and
    /// corrected the local condition. The engine never resolves issues by
    /// policy or by observing a later server event.
    pub async fn resolve_issue(
        &self,
        library_id: LibraryId,
        issue_id: Uuid,
    ) -> Result<(), ClientSyncError> {
        let mut transaction = self.pool.begin().await?;
        let changed = sqlx::query(
            "UPDATE local_apply_issues SET resolved_at_ms = ?
             WHERE library_id = ? AND issue_id = ? AND resolved_at_ms IS NULL",
        )
        .bind(now_ms()?)
        .bind(library_id.to_string())
        .bind(issue_id.to_string())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        let remaining: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM local_apply_issues
             WHERE library_id = ? AND resolved_at_ms IS NULL",
        )
        .bind(library_id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        if remaining == 0 {
            sqlx::query(
                "UPDATE replicas SET status = 'IDLE', updated_at_ms = ?
                 WHERE library_id = ? AND status = 'BLOCKED_LOCAL_ISSUE'",
            )
            .bind(now_ms()?)
            .bind(library_id.to_string())
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Return the durable state that makes a watcher/reconciliation restart
    /// safe. Opening a legacy database is never enough; it must first be bound
    /// to this exact managed replica.
    pub async fn observation_state(
        &self,
        library_id: LibraryId,
    ) -> Result<Option<ObservationState>, ClientSyncError> {
        let row = sqlx::query("SELECT * FROM observation_state WHERE library_id = ?")
            .bind(library_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(decode_observation_state).transpose()
    }

    /// Persist loss of watcher completeness before callers attempt any further
    /// classification. This changes no inbound checkpoint.
    pub async fn require_reconciliation(
        &self,
        library_id: LibraryId,
    ) -> Result<ObservationState, ClientSyncError> {
        let changed = sqlx::query(
            "UPDATE observation_state
             SET rescan_required = 1, updated_at_ms = ? WHERE library_id = ?",
        )
        .bind(now_ms()?)
        .bind(library_id.to_string())
        .execute(&self.pool)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        self.observation_state(library_id)
            .await?
            .ok_or(ClientSyncError::InvalidState)
    }

    /// Persist one safe, typed observation issue. These records are separate
    /// from inbound apply blockers so an overflow does not pretend that the
    /// server checkpoint advanced or that an inbound conflict was resolved.
    pub async fn persist_observation_issue(
        &self,
        library_id: LibraryId,
        node_id: Option<NodeId>,
        relative_path: Option<&ManagedRelativePath>,
        kind: ObservationIssueKind,
    ) -> Result<ObservationIssue, ClientSyncError> {
        let issue_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO observation_issues (
                 issue_id, library_id, node_id, relative_path, issue_kind, created_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT DO NOTHING",
        )
        .bind(issue_id.to_string())
        .bind(library_id.to_string())
        .bind(node_id.map(|value| value.to_string()))
        .bind(relative_path.map(ManagedRelativePath::as_str))
        .bind(kind.as_str())
        .bind(now_ms()?)
        .execute(&self.pool)
        .await?;
        let row = sqlx::query(
            "SELECT * FROM observation_issues
             WHERE library_id = ? AND COALESCE(node_id, '') = COALESCE(?, '')
               AND COALESCE(relative_path, '') = COALESCE(?, '')
               AND issue_kind = ? AND resolved_at_ms IS NULL",
        )
        .bind(library_id.to_string())
        .bind(node_id.map(|value| value.to_string()))
        .bind(relative_path.map(ManagedRelativePath::as_str))
        .bind(kind.as_str())
        .fetch_one(&self.pool)
        .await?;
        decode_observation_issue(row)
    }

    pub async fn observation_issues(
        &self,
        library_id: LibraryId,
    ) -> Result<Vec<ObservationIssue>, ClientSyncError> {
        let rows = sqlx::query(
            "SELECT * FROM observation_issues WHERE library_id = ? AND resolved_at_ms IS NULL
             ORDER BY created_at_ms, issue_id LIMIT 4097",
        )
        .bind(library_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        if rows.len() > MAX_OBSERVATION_QUERY_ROWS {
            return Err(ClientSyncError::ResourceLimit);
        }
        rows.into_iter().map(decode_observation_issue).collect()
    }

    pub(crate) async fn has_unresolved_observation_issue(
        &self,
        library_id: LibraryId,
        relative_path: &ManagedRelativePath,
        kind: ObservationIssueKind,
    ) -> Result<bool, ClientSyncError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM observation_issues
             WHERE library_id = ? AND relative_path = ? AND issue_kind = ?
               AND resolved_at_ms IS NULL",
        )
        .bind(library_id.to_string())
        .bind(relative_path.as_str())
        .bind(kind.as_str())
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    pub async fn resolve_observation_issue(
        &self,
        library_id: LibraryId,
        issue_id: Uuid,
    ) -> Result<(), ClientSyncError> {
        let changed = sqlx::query(
            "UPDATE observation_issues SET resolved_at_ms = ?
             WHERE library_id = ? AND issue_id = ? AND resolved_at_ms IS NULL",
        )
        .bind(now_ms()?)
        .bind(library_id.to_string())
        .bind(issue_id.to_string())
        .execute(&self.pool)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        Ok(())
    }

    /// Insert an intent idempotently. Exact semantic duplicates return the
    /// original durable row. The small set of proven local coalescing rules
    /// updates only a still-active row and never crosses identity boundaries.
    pub async fn upsert_outbound_intent(
        &self,
        intent: &OutboundIntent,
    ) -> Result<OutboundIntent, ClientSyncError> {
        let exact = sqlx::query(
            "SELECT * FROM outbound_intents
             WHERE library_id = ? AND dedupe_version = 1 AND dedupe_sha256 = ?
               AND state IN ('PENDING','BLOCKED','NEEDS_REBASE_VALIDATION') LIMIT 1",
        )
        .bind(intent.library_id().to_string())
        .bind(intent.dedupe_sha256().into_bytes().to_vec())
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = exact {
            return decode_outbound_intent(row);
        }

        let coalesced = match intent.kind() {
            OutboundIntentKind::CreateDirectory | OutboundIntentKind::CreateFile => {
                sqlx::query(
                    "SELECT * FROM outbound_intents
                     WHERE library_id = ? AND node_id IS NULL
                       AND observed_relative_path = ? AND intent_kind = ?
                       AND state IN ('PENDING','BLOCKED','NEEDS_REBASE_VALIDATION')
                     ORDER BY created_at_ms LIMIT 1",
                )
                .bind(intent.library_id().to_string())
                .bind(intent.observed_relative_path().as_str())
                .bind(intent.kind().as_str())
                .fetch_optional(&self.pool)
                .await?
            }
            OutboundIntentKind::ModifyFileContent => {
                sqlx::query(
                    "SELECT * FROM outbound_intents
                     WHERE library_id = ? AND node_id = ? AND intent_kind = 'MODIFY_FILE_CONTENT'
                       AND state IN ('PENDING','BLOCKED','NEEDS_REBASE_VALIDATION')
                     ORDER BY created_at_ms LIMIT 1",
                )
                .bind(intent.library_id().to_string())
                .bind(
                    intent
                        .node_id()
                        .ok_or(ClientSyncError::InvalidState)?
                        .to_string(),
                )
                .fetch_optional(&self.pool)
                .await?
            }
            OutboundIntentKind::RenameNode | OutboundIntentKind::MoveNode => {
                sqlx::query(
                    "SELECT * FROM outbound_intents
                     WHERE library_id = ? AND node_id = ?
                       AND intent_kind IN ('RENAME_NODE','MOVE_NODE')
                       AND observed_relative_path = ?
                       AND state IN ('PENDING','BLOCKED','NEEDS_REBASE_VALIDATION')
                     ORDER BY created_at_ms LIMIT 1",
                )
                .bind(intent.library_id().to_string())
                .bind(
                    intent
                        .node_id()
                        .ok_or(ClientSyncError::InvalidState)?
                        .to_string(),
                )
                .bind(
                    intent
                        .old_relative_path()
                        .ok_or(ClientSyncError::InvalidState)?
                        .as_str(),
                )
                .fetch_optional(&self.pool)
                .await?
            }
            OutboundIntentKind::DeleteOrTrashNode => {
                sqlx::query(
                    "SELECT * FROM outbound_intents
                     WHERE library_id = ? AND node_id = ? AND intent_kind = 'DELETE_OR_TRASH_NODE'
                       AND state IN ('PENDING','BLOCKED','NEEDS_REBASE_VALIDATION')
                     ORDER BY created_at_ms LIMIT 1",
                )
                .bind(intent.library_id().to_string())
                .bind(
                    intent
                        .node_id()
                        .ok_or(ClientSyncError::InvalidState)?
                        .to_string(),
                )
                .fetch_optional(&self.pool)
                .await?
            }
        };
        if let Some(row) = coalesced {
            let existing = decode_outbound_intent(row)?;
            let changed = sqlx::query(
                "UPDATE outbound_intents SET node_id = ?, parent_node_id = ?, intent_kind = ?,
                        observed_relative_path = ?, old_relative_path = ?, observed_kind = ?,
                        observed_length = ?, observed_sha256 = ?, base_epoch = ?,
                        base_applied_sequence = ?, base_revision = ?, base_current_version_id = ?,
                        base_parent_revision = ?, dedupe_version = 1, dedupe_sha256 = ?,
                        updated_at_ms = ? WHERE intent_id = ?",
            )
            .bind(intent.node_id().map(|value| value.to_string()))
            .bind(intent.parent_node_id().map(|value| value.to_string()))
            .bind(intent.kind().as_str())
            .bind(intent.observed_relative_path().as_str())
            .bind(intent.old_relative_path().map(ManagedRelativePath::as_str))
            .bind(intent.observed_fingerprint().map(fingerprint_kind_as_str))
            .bind(optional_fingerprint_length(intent.observed_fingerprint())?)
            .bind(
                intent
                    .observed_fingerprint()
                    .and_then(LocalFingerprint::sha256)
                    .map(|value| value.into_bytes().to_vec()),
            )
            .bind(sequence_i64(intent.base_epoch())?)
            .bind(sequence_i64(intent.base_applied_sequence())?)
            .bind(intent.base_revision().map(revision_i64).transpose()?)
            .bind(
                intent
                    .base_current_version_id()
                    .map(|value| value.to_string()),
            )
            .bind(
                intent
                    .base_parent_revision()
                    .map(revision_i64)
                    .transpose()?,
            )
            .bind(intent.dedupe_sha256().into_bytes().to_vec())
            .bind(now_ms()?)
            .bind(existing.intent_id().to_string())
            .execute(&self.pool)
            .await?
            .rows_affected();
            if changed != 1 {
                return Err(ClientSyncError::InvalidState);
            }
            return self
                .outbound_intent(existing.intent_id())
                .await?
                .ok_or(ClientSyncError::InvalidState);
        }

        let now = now_ms()?;
        sqlx::query(
            "INSERT INTO outbound_intents (
                 intent_id, library_id, node_id, parent_node_id, intent_kind, state,
                 observed_relative_path, old_relative_path, observed_kind, observed_length,
                 observed_sha256, base_epoch, base_applied_sequence, base_revision,
                 base_current_version_id, base_parent_revision, dedupe_version, dedupe_sha256,
                 created_at_ms, updated_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?)",
        )
        .bind(intent.intent_id().to_string())
        .bind(intent.library_id().to_string())
        .bind(intent.node_id().map(|value| value.to_string()))
        .bind(intent.parent_node_id().map(|value| value.to_string()))
        .bind(intent.kind().as_str())
        .bind(intent.state().as_str())
        .bind(intent.observed_relative_path().as_str())
        .bind(intent.old_relative_path().map(ManagedRelativePath::as_str))
        .bind(intent.observed_fingerprint().map(fingerprint_kind_as_str))
        .bind(optional_fingerprint_length(intent.observed_fingerprint())?)
        .bind(
            intent
                .observed_fingerprint()
                .and_then(LocalFingerprint::sha256)
                .map(|value| value.into_bytes().to_vec()),
        )
        .bind(sequence_i64(intent.base_epoch())?)
        .bind(sequence_i64(intent.base_applied_sequence())?)
        .bind(intent.base_revision().map(revision_i64).transpose()?)
        .bind(
            intent
                .base_current_version_id()
                .map(|value| value.to_string()),
        )
        .bind(
            intent
                .base_parent_revision()
                .map(revision_i64)
                .transpose()?,
        )
        .bind(intent.dedupe_sha256().into_bytes().to_vec())
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(intent.clone())
    }

    /// Preserve a pre-submission create intent's ID when a paired native
    /// rename changes its observed path. The replacement must remain a create
    /// with no fabricated server NodeId.
    pub(crate) async fn move_pending_create(
        &self,
        source: &ManagedRelativePath,
        replacement: &OutboundIntent,
    ) -> Result<Option<OutboundIntent>, ClientSyncError> {
        if replacement.node_id().is_some()
            || !matches!(
                replacement.kind(),
                OutboundIntentKind::CreateDirectory | OutboundIntentKind::CreateFile
            )
        {
            return Err(ClientSyncError::InvalidState);
        }
        let row = sqlx::query(
            "SELECT * FROM outbound_intents
             WHERE library_id = ? AND node_id IS NULL AND observed_relative_path = ?
               AND intent_kind = ?
               AND state IN ('PENDING','BLOCKED','NEEDS_REBASE_VALIDATION')
             ORDER BY created_at_ms LIMIT 1",
        )
        .bind(replacement.library_id().to_string())
        .bind(source.as_str())
        .bind(replacement.kind().as_str())
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let existing = decode_outbound_intent(row)?;

        let exact = sqlx::query(
            "SELECT * FROM outbound_intents
             WHERE library_id = ? AND dedupe_version = 1 AND dedupe_sha256 = ?
               AND state IN ('PENDING','BLOCKED','NEEDS_REBASE_VALIDATION') LIMIT 1",
        )
        .bind(replacement.library_id().to_string())
        .bind(replacement.dedupe_sha256().into_bytes().to_vec())
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = exact {
            let exact = decode_outbound_intent(row)?;
            let changed = sqlx::query(
                "UPDATE outbound_intents SET state = 'SUPERSEDED', updated_at_ms = ?
                 WHERE intent_id = ? AND state IN ('PENDING','BLOCKED','NEEDS_REBASE_VALIDATION')",
            )
            .bind(now_ms()?)
            .bind(existing.intent_id().to_string())
            .execute(&self.pool)
            .await?
            .rows_affected();
            if changed != 1 {
                return Err(ClientSyncError::InvalidState);
            }
            return Ok(Some(exact));
        }

        let changed = sqlx::query(
            "UPDATE outbound_intents SET parent_node_id = ?, intent_kind = ?,
                    observed_relative_path = ?, old_relative_path = ?, observed_kind = ?,
                    observed_length = ?, observed_sha256 = ?, base_epoch = ?,
                    base_applied_sequence = ?, base_revision = ?, base_current_version_id = ?,
                    base_parent_revision = ?, dedupe_version = 1, dedupe_sha256 = ?,
                    updated_at_ms = ?
             WHERE intent_id = ? AND state IN ('PENDING','BLOCKED','NEEDS_REBASE_VALIDATION')",
        )
        .bind(replacement.parent_node_id().map(|value| value.to_string()))
        .bind(replacement.kind().as_str())
        .bind(replacement.observed_relative_path().as_str())
        .bind(
            replacement
                .old_relative_path()
                .map(ManagedRelativePath::as_str),
        )
        .bind(
            replacement
                .observed_fingerprint()
                .map(fingerprint_kind_as_str),
        )
        .bind(optional_fingerprint_length(
            replacement.observed_fingerprint(),
        )?)
        .bind(
            replacement
                .observed_fingerprint()
                .and_then(LocalFingerprint::sha256)
                .map(|value| value.into_bytes().to_vec()),
        )
        .bind(sequence_i64(replacement.base_epoch())?)
        .bind(sequence_i64(replacement.base_applied_sequence())?)
        .bind(replacement.base_revision().map(revision_i64).transpose()?)
        .bind(
            replacement
                .base_current_version_id()
                .map(|value| value.to_string()),
        )
        .bind(
            replacement
                .base_parent_revision()
                .map(revision_i64)
                .transpose()?,
        )
        .bind(replacement.dedupe_sha256().into_bytes().to_vec())
        .bind(now_ms()?)
        .bind(existing.intent_id().to_string())
        .execute(&self.pool)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        Ok(Some(
            self.outbound_intent(existing.intent_id())
                .await?
                .ok_or(ClientSyncError::InvalidState)?,
        ))
    }

    pub async fn outbound_intent(
        &self,
        intent_id: OutboundIntentId,
    ) -> Result<Option<OutboundIntent>, ClientSyncError> {
        let row = sqlx::query("SELECT * FROM outbound_intents WHERE intent_id = ?")
            .bind(intent_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(decode_outbound_intent).transpose()
    }

    pub async fn next_submittable_intent(
        &self,
        library_id: LibraryId,
    ) -> Result<Option<OutboundIntent>, ClientSyncError> {
        let row = sqlx::query(
            "SELECT * FROM outbound_intents
             WHERE library_id = ? AND state IN ('PENDING','READY','PREPARING','UPLOADING','SUBMITTING')
             ORDER BY created_at_ms, intent_id LIMIT 1",
        )
        .bind(library_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(decode_outbound_intent).transpose()
    }

    pub async fn transition_outbound_intent(
        &self,
        intent_id: OutboundIntentId,
        from: &[OutboundIntentState],
        to: OutboundIntentState,
    ) -> Result<bool, ClientSyncError> {
        if from.is_empty() {
            return Err(ClientSyncError::InvalidState);
        }
        let placeholders = std::iter::repeat_n("?", from.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "UPDATE outbound_intents SET state = ?, updated_at_ms = ?
             WHERE intent_id = ? AND state IN ({placeholders})",
        );
        let mut query = sqlx::query(&sql)
            .bind(to.as_str())
            .bind(now_ms()?)
            .bind(intent_id.to_string());
        for state in from {
            query = query.bind(state.as_str());
        }
        let changed = query.execute(&self.pool).await?.rows_affected();
        Ok(changed == 1)
    }

    pub async fn durable_mutation_request(
        &self,
        intent_id: OutboundIntentId,
    ) -> Result<Option<OutboundMutationRecord>, ClientSyncError> {
        let row = sqlx::query("SELECT * FROM outbound_mutation_requests WHERE intent_id = ?")
            .bind(intent_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(decode_mutation_record).transpose()
    }

    pub async fn persist_mutation_request(
        &self,
        intent_id: OutboundIntentId,
        request: &ClientMutationRequest,
        request_json: &str,
    ) -> Result<OutboundMutationRecord, ClientSyncError> {
        if request_json.is_empty() || request_json.len() > 16 * 1024 {
            return Err(ClientSyncError::ResourceLimit);
        }
        let now = now_ms()?;
        sqlx::query(
            "INSERT INTO outbound_mutation_requests (
                 intent_id, mutation_id, mutation_kind, base_epoch, base_sequence,
                 request_json, fingerprint_version, fingerprint_sha256,
                 created_at_ms, updated_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(intent_id) DO NOTHING",
        )
        .bind(intent_id.to_string())
        .bind(request.mutation_id().to_string())
        .bind(request.kind().as_str())
        .bind(sequence_i64(request.base_epoch())?)
        .bind(sequence_i64(request.base_sequence())?)
        .bind(request_json)
        .bind(i64::from(request.fingerprint().version()))
        .bind(request.fingerprint().sha256().to_vec())
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        let persisted = self
            .durable_mutation_request(intent_id)
            .await?
            .ok_or(ClientSyncError::InvalidState)?;
        if persisted.request() != request || persisted.request_json() != request_json {
            return Err(ClientSyncError::InvalidState);
        }
        Ok(persisted)
    }

    pub async fn durable_upload_session(
        &self,
        intent_id: OutboundIntentId,
    ) -> Result<Option<OutboundUploadRecord>, ClientSyncError> {
        let row = sqlx::query("SELECT * FROM outbound_upload_sessions WHERE intent_id = ?")
            .bind(intent_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(decode_upload_record).transpose()
    }

    pub async fn persist_staged_upload(
        &self,
        intent_id: OutboundIntentId,
        operation: &str,
        staging_relative_path: &ManagedRelativePath,
        expected_length: u64,
        expected_sha256: Sha256Digest,
    ) -> Result<OutboundUploadRecord, ClientSyncError> {
        if !matches!(operation, "CREATE_FILE" | "REPLACE_CONTENT") {
            return Err(ClientSyncError::InvalidState);
        }
        let now = now_ms()?;
        sqlx::query(
            "INSERT INTO outbound_upload_sessions (
                 intent_id, operation, staging_relative_path, expected_length,
                 expected_sha256, state, created_at_ms, updated_at_ms
             ) VALUES (?, ?, ?, ?, ?, 'STAGED', ?, ?)
             ON CONFLICT(intent_id) DO NOTHING",
        )
        .bind(intent_id.to_string())
        .bind(operation)
        .bind(staging_relative_path.as_str())
        .bind(u64_i64(expected_length)?)
        .bind(expected_sha256.into_bytes().to_vec())
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        let persisted = self
            .durable_upload_session(intent_id)
            .await?
            .ok_or(ClientSyncError::InvalidState)?;
        if persisted.staging_relative_path() != staging_relative_path
            || persisted.expected_length() != expected_length
            || persisted.expected_sha256() != expected_sha256
        {
            return Err(ClientSyncError::InvalidState);
        }
        Ok(persisted)
    }

    pub async fn persist_upload_session_id(
        &self,
        intent_id: OutboundIntentId,
        upload_session_id: UploadSessionId,
        acknowledged_offset: u64,
    ) -> Result<(), ClientSyncError> {
        let changed = sqlx::query(
            "UPDATE outbound_upload_sessions
             SET upload_session_id = COALESCE(upload_session_id, ?),
                 acknowledged_offset = ?, state = 'SESSION_CREATED', updated_at_ms = ?
             WHERE intent_id = ? AND (upload_session_id IS NULL OR upload_session_id = ?)",
        )
        .bind(upload_session_id.to_string())
        .bind(u64_i64(acknowledged_offset)?)
        .bind(now_ms()?)
        .bind(intent_id.to_string())
        .bind(upload_session_id.to_string())
        .execute(&self.pool)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        Ok(())
    }

    pub async fn update_upload_offset(
        &self,
        intent_id: OutboundIntentId,
        acknowledged_offset: u64,
        state: &str,
    ) -> Result<(), ClientSyncError> {
        if !matches!(state, "UPLOADING" | "COMPLETING" | "COMPLETED" | "FAILED") {
            return Err(ClientSyncError::InvalidState);
        }
        let changed = sqlx::query(
            "UPDATE outbound_upload_sessions
             SET acknowledged_offset = ?, state = ?, updated_at_ms = ?
             WHERE intent_id = ? AND acknowledged_offset <= ?",
        )
        .bind(u64_i64(acknowledged_offset)?)
        .bind(state)
        .bind(now_ms()?)
        .bind(intent_id.to_string())
        .bind(u64_i64(acknowledged_offset)?)
        .execute(&self.pool)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        Ok(())
    }

    pub async fn record_mutation_applied(
        &self,
        intent_id: OutboundIntentId,
        result: RemoteMutationApplied,
    ) -> Result<(), ClientSyncError> {
        let now = now_ms()?;
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO outbound_submission_results (
                 intent_id, mutation_id, outcome, resource_id, result_revision,
                 journal_event_id, journal_sequence, replayed, created_at_ms, updated_at_ms
             ) VALUES (?, ?, 'SERVER_APPLIED', ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(intent_id) DO UPDATE SET
                 outcome = excluded.outcome,
                 resource_id = excluded.resource_id,
                 result_revision = excluded.result_revision,
                 journal_event_id = excluded.journal_event_id,
                 journal_sequence = excluded.journal_sequence,
                 replayed = excluded.replayed,
                 updated_at_ms = excluded.updated_at_ms",
        )
        .bind(intent_id.to_string())
        .bind(result.mutation_id().to_string())
        .bind(result.node_id().to_string())
        .bind(revision_i64(result.revision())?)
        .bind(result.journal_event_id().to_string())
        .bind(sequence_i64(result.journal_sequence())?)
        .bind(if result.replayed() { 1_i64 } else { 0_i64 })
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE outbound_intents SET state = 'SERVER_APPLIED', updated_at_ms = ? WHERE intent_id = ?",
        )
        .bind(now)
        .bind(intent_id.to_string())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn record_upload_completed(
        &self,
        intent_id: OutboundIntentId,
        completion: UploadCompletion,
    ) -> Result<(), ClientSyncError> {
        let now = now_ms()?;
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO outbound_submission_results (
                 intent_id, upload_session_id, outcome, resource_id, result_revision,
                 file_version_id, result_length, result_sha256,
                 replayed, created_at_ms, updated_at_ms
             ) VALUES (?, ?, 'SERVER_APPLIED', ?, ?, ?, ?, ?, 0, ?, ?)
             ON CONFLICT(intent_id) DO UPDATE SET
                 outcome = excluded.outcome,
                 upload_session_id = excluded.upload_session_id,
                 resource_id = excluded.resource_id,
                 result_revision = excluded.result_revision,
                 file_version_id = excluded.file_version_id,
                 result_length = excluded.result_length,
                 result_sha256 = excluded.result_sha256,
                 updated_at_ms = excluded.updated_at_ms",
        )
        .bind(intent_id.to_string())
        .bind(completion.session_id().to_string())
        .bind(completion.node_id().to_string())
        .bind(revision_i64(completion.node_revision())?)
        .bind(completion.file_version_id().to_string())
        .bind(u64_i64(completion.length())?)
        .bind(completion.sha256().into_bytes().to_vec())
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("UPDATE outbound_upload_sessions SET state = 'COMPLETED', acknowledged_offset = ?, updated_at_ms = ? WHERE intent_id = ?")
            .bind(u64_i64(completion.length())?)
            .bind(now)
            .bind(intent_id.to_string())
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "UPDATE outbound_intents SET state = 'SERVER_APPLIED', updated_at_ms = ? WHERE intent_id = ?",
        )
        .bind(now)
        .bind(intent_id.to_string())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn record_mutation_conflict(
        &self,
        intent_id: OutboundIntentId,
        mutation_id: ClientMutationId,
        conflict: &RemoteMutationConflict,
    ) -> Result<(), ClientSyncError> {
        let now = now_ms()?;
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO outbound_submission_results (
                 intent_id, mutation_id, outcome, conflict_id, conflict_reason,
                 replayed, created_at_ms, updated_at_ms
             ) VALUES (?, ?, 'CONFLICT', ?, ?, ?, ?, ?)
             ON CONFLICT(intent_id) DO UPDATE SET
                 outcome = excluded.outcome,
                 conflict_id = excluded.conflict_id,
                 conflict_reason = excluded.conflict_reason,
                 replayed = excluded.replayed,
                 updated_at_ms = excluded.updated_at_ms",
        )
        .bind(intent_id.to_string())
        .bind(mutation_id.to_string())
        .bind(conflict.conflict_id().to_string())
        .bind(conflict.reason())
        .bind(if conflict.replayed() { 1_i64 } else { 0_i64 })
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE outbound_intents SET state = 'CONFLICT', updated_at_ms = ? WHERE intent_id = ?",
        )
        .bind(now)
        .bind(intent_id.to_string())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn has_pending_file_content_submission(
        &self,
        _library_id: LibraryId,
        node_id: NodeId,
    ) -> Result<bool, ClientSyncError> {
        let row = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM outbound_submission_results
             WHERE resource_id = ? AND outcome = 'SERVER_APPLIED' AND file_version_id IS NOT NULL",
        )
        .bind(node_id.to_string())
        .fetch_one(&self.pool)
        .await?;
        Ok(row > 0)
    }

    pub async fn mark_intent_blocked(
        &self,
        intent_id: OutboundIntentId,
    ) -> Result<(), ClientSyncError> {
        let changed = sqlx::query(
            "UPDATE outbound_intents SET state = 'BLOCKED', updated_at_ms = ? WHERE intent_id = ?",
        )
        .bind(now_ms()?)
        .bind(intent_id.to_string())
        .execute(&self.pool)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        Ok(())
    }

    pub async fn reconcile_server_applied_intents(
        &self,
        library_id: LibraryId,
    ) -> Result<u64, ClientSyncError> {
        let changed = sqlx::query(
            "UPDATE outbound_intents
             SET state = 'RECONCILED', updated_at_ms = ?
             WHERE library_id = ? AND state = 'SERVER_APPLIED'
               AND EXISTS (
                   SELECT 1 FROM outbound_submission_results r
                   WHERE r.intent_id = outbound_intents.intent_id
                     AND r.resource_id IS NOT NULL
                     AND r.result_revision IS NOT NULL
                     AND EXISTS (
                         SELECT 1 FROM local_nodes n
                         WHERE n.library_id = outbound_intents.library_id
                           AND n.node_id = r.resource_id
                           AND n.revision >= r.result_revision
                     )
               )",
        )
        .bind(now_ms()?)
        .bind(library_id.to_string())
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(changed)
    }

    pub(crate) async fn outbound_result_for_sequence(
        &self,
        library_id: LibraryId,
        sequence: Sequence,
    ) -> Result<Option<OutboundSubmissionResult>, ClientSyncError> {
        let row = sqlx::query(
            "SELECT resource_id, result_revision, file_version_id, result_length, result_sha256
             FROM outbound_submission_results r
             JOIN outbound_intents i ON i.intent_id = r.intent_id
             WHERE i.library_id = ? AND r.outcome = 'SERVER_APPLIED' AND r.journal_sequence = ?",
        )
        .bind(library_id.to_string())
        .bind(sequence_i64(sequence)?)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            let resource_id = NodeId::try_from_uuid(
                Uuid::parse_str(row.try_get::<String, _>("resource_id")?.as_str())
                    .map_err(|_| ClientSyncError::InvalidState)?,
            )
            .map_err(|_| ClientSyncError::InvalidState)?;
            let revision = Revision::new(
                u64::try_from(row.try_get::<i64, _>("result_revision")?)
                    .map_err(|_| ClientSyncError::InvalidState)?,
            );
            let file_version_id = optional_id(row.try_get("file_version_id")?)?;
            let length = optional_u64(row.try_get("result_length")?)?;
            let sha256 = optional_hash(row.try_get("result_sha256")?)?;
            Ok(OutboundSubmissionResult {
                resource_id,
                revision,
                file_version_id,
                length,
                sha256,
            })
        })
        .transpose()
    }

    pub async fn list_pending_intents(
        &self,
        library_id: LibraryId,
    ) -> Result<Vec<OutboundIntent>, ClientSyncError> {
        let rows = sqlx::query(
            "SELECT * FROM outbound_intents WHERE library_id = ? AND state = 'PENDING'
             ORDER BY created_at_ms, intent_id LIMIT 4097",
        )
        .bind(library_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        if rows.len() > MAX_OBSERVATION_QUERY_ROWS {
            return Err(ClientSyncError::ResourceLimit);
        }
        rows.into_iter().map(decode_outbound_intent).collect()
    }

    pub async fn count_pending_intents(
        &self,
        library_id: LibraryId,
    ) -> Result<u64, ClientSyncError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM outbound_intents WHERE library_id = ? AND state = 'PENDING'",
        )
        .bind(library_id.to_string())
        .fetch_one(&self.pool)
        .await?;
        u64_from_i64(count)
    }

    pub(crate) async fn cancel_pending_delete(
        &self,
        library_id: LibraryId,
        node_id: NodeId,
    ) -> Result<bool, ClientSyncError> {
        let changed = sqlx::query(
            "UPDATE outbound_intents SET state = 'CANCELLED', updated_at_ms = ?
             WHERE library_id = ? AND node_id = ? AND intent_kind = 'DELETE_OR_TRASH_NODE'
               AND state IN ('PENDING','BLOCKED','NEEDS_REBASE_VALIDATION')",
        )
        .bind(now_ms()?)
        .bind(library_id.to_string())
        .bind(node_id.to_string())
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(changed > 0)
    }

    pub(crate) async fn cancel_pending_create(
        &self,
        library_id: LibraryId,
        relative_path: &ManagedRelativePath,
    ) -> Result<bool, ClientSyncError> {
        let changed = sqlx::query(
            "UPDATE outbound_intents SET state = 'CANCELLED', updated_at_ms = ?
             WHERE library_id = ? AND node_id IS NULL
               AND intent_kind IN ('CREATE_DIRECTORY','CREATE_FILE')
               AND observed_relative_path = ?
               AND state IN ('PENDING','BLOCKED','NEEDS_REBASE_VALIDATION')",
        )
        .bind(now_ms()?)
        .bind(library_id.to_string())
        .bind(relative_path.as_str())
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(changed > 0)
    }

    /// Preserve queued work when an inbound event or rebaseline invalidates its
    /// server base. The observer never silently drops or auto-submits it.
    pub(crate) async fn mark_outbound_intents_needing_rebase(
        &self,
        library_id: LibraryId,
        node_id: Option<NodeId>,
    ) -> Result<bool, ClientSyncError> {
        let affected: i64 = match node_id {
            Some(node_id) => {
                let target = self.local_node(library_id, node_id).await?;
                if let Some(target) = target {
                    let prefix = format!("{}/", target.relative_path().as_str());
                    sqlx::query_scalar(
                        "SELECT COUNT(*) FROM outbound_intents
                         WHERE library_id = ? AND state IN ('PENDING','READY','PREPARING','UPLOADING','SUBMITTING','BLOCKED','NEEDS_REBASE_VALIDATION')
                           AND (node_id = ? OR node_id IN (
                               SELECT node_id FROM local_nodes
                               WHERE library_id = ? AND relative_path LIKE ? ESCAPE '\\'
                           ) OR observed_relative_path = ?
                              OR observed_relative_path LIKE ? ESCAPE '\\')",
                    )
                    .bind(library_id.to_string())
                    .bind(node_id.to_string())
                    .bind(library_id.to_string())
                    .bind(format!("{}%", escape_like(&prefix)))
                    .bind(target.relative_path().as_str())
                    .bind(format!("{}%", escape_like(&prefix)))
                    .fetch_one(&self.pool)
                    .await?
                } else {
                    sqlx::query_scalar(
                        "SELECT COUNT(*) FROM outbound_intents
                         WHERE library_id = ? AND node_id = ?
                           AND state IN ('PENDING','READY','PREPARING','UPLOADING','SUBMITTING','BLOCKED','NEEDS_REBASE_VALIDATION')",
                    )
                    .bind(library_id.to_string())
                    .bind(node_id.to_string())
                    .fetch_one(&self.pool)
                    .await?
                }
            }
            None => {
                sqlx::query_scalar(
                    "SELECT COUNT(*) FROM outbound_intents
                 WHERE library_id = ? AND state IN ('PENDING','READY','PREPARING','UPLOADING','SUBMITTING','BLOCKED','NEEDS_REBASE_VALIDATION')",
                )
                .bind(library_id.to_string())
                .fetch_one(&self.pool)
                .await?
            }
        };
        if affected == 0 {
            return Ok(false);
        }

        match node_id {
            Some(node_id) => {
                if let Some(target) = self.local_node(library_id, node_id).await? {
                    let prefix = format!("{}/", target.relative_path().as_str());
                    sqlx::query(
                        "UPDATE outbound_intents SET state = 'NEEDS_REBASE_VALIDATION', updated_at_ms = ?
                         WHERE library_id = ? AND state IN ('PENDING','READY','PREPARING','UPLOADING','SUBMITTING','BLOCKED')
                           AND (node_id = ? OR node_id IN (
                               SELECT node_id FROM local_nodes
                               WHERE library_id = ? AND relative_path LIKE ? ESCAPE '\\'
                           ) OR observed_relative_path = ?
                              OR observed_relative_path LIKE ? ESCAPE '\\')",
                    )
                    .bind(now_ms()?)
                    .bind(library_id.to_string())
                    .bind(node_id.to_string())
                    .bind(library_id.to_string())
                    .bind(format!("{}%", escape_like(&prefix)))
                    .bind(target.relative_path().as_str())
                    .bind(format!("{}%", escape_like(&prefix)))
                    .execute(&self.pool)
                    .await?;
                } else {
                    sqlx::query(
                        "UPDATE outbound_intents SET state = 'NEEDS_REBASE_VALIDATION', updated_at_ms = ?
                         WHERE library_id = ? AND node_id = ? AND state IN ('PENDING','READY','PREPARING','UPLOADING','SUBMITTING','BLOCKED')",
                    )
                    .bind(now_ms()?)
                    .bind(library_id.to_string())
                    .bind(node_id.to_string())
                    .execute(&self.pool)
                    .await?;
                }
            }
            None => {
                sqlx::query(
                    "UPDATE outbound_intents SET state = 'NEEDS_REBASE_VALIDATION', updated_at_ms = ?
                     WHERE library_id = ? AND state IN ('PENDING','READY','PREPARING','UPLOADING','SUBMITTING','BLOCKED')",
                )
                .bind(now_ms()?)
                .bind(library_id.to_string())
                .execute(&self.pool)
                .await?;
            }
        }

        if affected > 0 {
            self.persist_observation_issue(
                library_id,
                node_id,
                None,
                ObservationIssueKind::BaseStateChanged,
            )
            .await?;
            self.set_status(library_id, EngineStatus::BlockedLocalIssue)
                .await?;
        }
        Ok(true)
    }

    pub(crate) async fn observed_node_at_path(
        &self,
        library_id: LibraryId,
        relative_path: &ManagedRelativePath,
    ) -> Result<Option<ObservedLocalNode>, ClientSyncError> {
        let row = sqlx::query(
            "SELECT n.*, o.relative_path AS observed_relative_path,
                    o.present AS observed_present, o.observed_kind,
                    o.observed_length, o.observed_sha256
             FROM local_nodes n
             LEFT JOIN observation_nodes o
               ON o.library_id = n.library_id AND o.node_id = n.node_id
             WHERE n.library_id = ?
               AND COALESCE(o.relative_path, n.relative_path) = ? LIMIT 1",
        )
        .bind(library_id.to_string())
        .bind(relative_path.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.map(decode_observed_node).transpose()
    }

    pub(crate) async fn observed_node(
        &self,
        library_id: LibraryId,
        node_id: NodeId,
    ) -> Result<Option<ObservedLocalNode>, ClientSyncError> {
        let row = sqlx::query(
            "SELECT n.*, o.relative_path AS observed_relative_path,
                    o.present AS observed_present, o.observed_kind,
                    o.observed_length, o.observed_sha256
             FROM local_nodes n
             LEFT JOIN observation_nodes o
               ON o.library_id = n.library_id AND o.node_id = n.node_id
             WHERE n.library_id = ? AND n.node_id = ?",
        )
        .bind(library_id.to_string())
        .bind(node_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(decode_observed_node).transpose()
    }

    pub(crate) async fn persist_observed_node(
        &self,
        node: &LocalNode,
        relative_path: &ManagedRelativePath,
        present: bool,
        fingerprint: Option<LocalFingerprint>,
    ) -> Result<(), ClientSyncError> {
        validate_observed_fingerprint(present, fingerprint)?;
        let changed = sqlx::query(
            "INSERT INTO observation_nodes (
                 library_id, node_id, relative_path, present, observed_kind,
                 observed_length, observed_sha256, updated_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(library_id, node_id) DO UPDATE SET
                 relative_path = excluded.relative_path, present = excluded.present,
                 observed_kind = excluded.observed_kind,
                 observed_length = excluded.observed_length,
                 observed_sha256 = excluded.observed_sha256,
                 updated_at_ms = excluded.updated_at_ms",
        )
        .bind(node.library_id().to_string())
        .bind(node.node_id().to_string())
        .bind(relative_path.as_str())
        .bind(present)
        .bind(fingerprint.map(fingerprint_kind_as_str))
        .bind(optional_fingerprint_length(fingerprint)?)
        .bind(
            fingerprint
                .and_then(LocalFingerprint::sha256)
                .map(|value| value.into_bytes().to_vec()),
        )
        .bind(now_ms()?)
        .execute(&self.pool)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        Ok(())
    }

    pub(crate) async fn begin_reconciliation(
        &self,
        library_id: LibraryId,
    ) -> Result<ObservationState, ClientSyncError> {
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query("SELECT * FROM observation_state WHERE library_id = ?")
            .bind(library_id.to_string())
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(ClientSyncError::InvalidState)?;
        let current = decode_observation_state(row)?;
        if !current.scan_active() && current.rescan_required() {
            let next_generation = current
                .scan_generation()
                .get()
                .checked_add(1)
                .ok_or(ClientSyncError::ResourceLimit)?;
            let next_generation = Sequence::new(next_generation);
            sqlx::query("DELETE FROM observation_scan_work WHERE library_id = ?")
                .bind(library_id.to_string())
                .execute(&mut *transaction)
                .await?;
            sqlx::query("DELETE FROM observation_scan_seen WHERE library_id = ?")
                .bind(library_id.to_string())
                .execute(&mut *transaction)
                .await?;
            sqlx::query("DELETE FROM observation_scan_seen_intents WHERE library_id = ?")
                .bind(library_id.to_string())
                .execute(&mut *transaction)
                .await?;
            sqlx::query(
                "INSERT INTO observation_scan_work (library_id, generation, relative_path)
                 VALUES (?, ?, '')",
            )
            .bind(library_id.to_string())
            .bind(sequence_i64(next_generation)?)
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                "UPDATE observation_state SET scan_active = 1, rescan_required = 0,
                        scan_generation = ?, updated_at_ms = ? WHERE library_id = ?",
            )
            .bind(sequence_i64(next_generation)?)
            .bind(now_ms()?)
            .bind(library_id.to_string())
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        self.observation_state(library_id)
            .await?
            .ok_or(ClientSyncError::InvalidState)
    }

    pub(crate) async fn next_observation_scan_work(
        &self,
        library_id: LibraryId,
        generation: Sequence,
    ) -> Result<Option<ObservationScanWork>, ClientSyncError> {
        let row = sqlx::query(
            "SELECT generation, relative_path, cursor_name FROM observation_scan_work
             WHERE library_id = ? AND generation = ? ORDER BY relative_path LIMIT 1",
        )
        .bind(library_id.to_string())
        .bind(sequence_i64(generation)?)
        .fetch_optional(&self.pool)
        .await?;
        row.map(decode_scan_work).transpose()
    }

    pub(crate) async fn mark_scan_node_seen(
        &self,
        library_id: LibraryId,
        generation: Sequence,
        node_id: NodeId,
    ) -> Result<(), ClientSyncError> {
        sqlx::query(
            "INSERT INTO observation_scan_seen (library_id, generation, node_id)
             VALUES (?, ?, ?) ON CONFLICT DO NOTHING",
        )
        .bind(library_id.to_string())
        .bind(sequence_i64(generation)?)
        .bind(node_id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn unseen_active_create_intents(
        &self,
        library_id: LibraryId,
        generation: Sequence,
        maximum: usize,
    ) -> Result<Vec<OutboundIntent>, ClientSyncError> {
        let limit =
            i64::try_from(maximum.saturating_add(1)).map_err(|_| ClientSyncError::ResourceLimit)?;
        let rows = sqlx::query(
            "SELECT i.* FROM outbound_intents i
             LEFT JOIN observation_scan_seen_intents s
               ON s.library_id = i.library_id AND s.generation = ? AND s.intent_id = i.intent_id
             WHERE i.library_id = ?
               AND i.intent_kind IN ('CREATE_DIRECTORY','CREATE_FILE')
               AND i.state IN ('PENDING','BLOCKED','NEEDS_REBASE_VALIDATION')
               AND s.intent_id IS NULL
             ORDER BY i.created_at_ms, i.intent_id LIMIT ?",
        )
        .bind(sequence_i64(generation)?)
        .bind(library_id.to_string())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .take(maximum)
            .map(decode_outbound_intent)
            .collect()
    }

    pub(crate) async fn mark_scan_intent_seen(
        &self,
        library_id: LibraryId,
        generation: Sequence,
        intent_id: OutboundIntentId,
    ) -> Result<(), ClientSyncError> {
        sqlx::query(
            "INSERT INTO observation_scan_seen_intents (library_id, generation, intent_id)
             VALUES (?, ?, ?) ON CONFLICT DO NOTHING",
        )
        .bind(library_id.to_string())
        .bind(sequence_i64(generation)?)
        .bind(intent_id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn complete_observation_scan_work(
        &self,
        library_id: LibraryId,
        generation: Sequence,
        work: &ObservationScanWork,
        cursor_name: Option<&str>,
        child_directories: &[ManagedRelativePath],
    ) -> Result<(), ClientSyncError> {
        let mut transaction = self.pool.begin().await?;
        if let Some(cursor_name) = cursor_name {
            let changed = sqlx::query(
                "UPDATE observation_scan_work SET cursor_name = ?
                 WHERE library_id = ? AND generation = ? AND relative_path = ?",
            )
            .bind(cursor_name)
            .bind(library_id.to_string())
            .bind(sequence_i64(generation)?)
            .bind(work.relative_path.as_str())
            .execute(&mut *transaction)
            .await?
            .rows_affected();
            if changed != 1 {
                return Err(ClientSyncError::InvalidState);
            }
        } else {
            let deleted = sqlx::query(
                "DELETE FROM observation_scan_work
                 WHERE library_id = ? AND generation = ? AND relative_path = ?",
            )
            .bind(library_id.to_string())
            .bind(sequence_i64(generation)?)
            .bind(work.relative_path.as_str())
            .execute(&mut *transaction)
            .await?
            .rows_affected();
            if deleted != 1 {
                return Err(ClientSyncError::InvalidState);
            }
            for child in child_directories {
                sqlx::query(
                    "INSERT INTO observation_scan_work (library_id, generation, relative_path)
                     VALUES (?, ?, ?) ON CONFLICT DO NOTHING",
                )
                .bind(library_id.to_string())
                .bind(sequence_i64(generation)?)
                .bind(child.as_str())
                .execute(&mut *transaction)
                .await?;
            }
        }
        transaction.commit().await?;
        Ok(())
    }

    pub(crate) async fn unseen_local_nodes(
        &self,
        library_id: LibraryId,
        generation: Sequence,
        maximum: usize,
    ) -> Result<Vec<LocalNode>, ClientSyncError> {
        let limit =
            i64::try_from(maximum.saturating_add(1)).map_err(|_| ClientSyncError::ResourceLimit)?;
        let rows = sqlx::query(
            "SELECT n.* FROM local_nodes n
             LEFT JOIN observation_scan_seen s
               ON s.library_id = n.library_id AND s.generation = ? AND s.node_id = n.node_id
             WHERE n.library_id = ? AND n.present = 1 AND s.node_id IS NULL
             ORDER BY length(n.relative_path) DESC, n.relative_path LIMIT ?",
        )
        .bind(sequence_i64(generation)?)
        .bind(library_id.to_string())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .take(maximum)
            .map(decode_local_node)
            .collect()
    }

    pub(crate) async fn finish_reconciliation(
        &self,
        library_id: LibraryId,
        generation: Sequence,
    ) -> Result<(), ClientSyncError> {
        let mut transaction = self.pool.begin().await?;
        let work_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM observation_scan_work WHERE library_id = ? AND generation = ?",
        )
        .bind(library_id.to_string())
        .bind(sequence_i64(generation)?)
        .fetch_one(&mut *transaction)
        .await?;
        if work_count != 0 {
            return Err(ClientSyncError::InvalidState);
        }
        let missing_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM local_nodes n LEFT JOIN observation_scan_seen s
                 ON s.library_id = n.library_id AND s.generation = ? AND s.node_id = n.node_id
             WHERE n.library_id = ? AND n.present = 1 AND s.node_id IS NULL",
        )
        .bind(sequence_i64(generation)?)
        .bind(library_id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        if missing_count != 0 {
            return Err(ClientSyncError::InvalidState);
        }
        let missing_creates: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM outbound_intents i
             LEFT JOIN observation_scan_seen_intents s
               ON s.library_id = i.library_id AND s.generation = ? AND s.intent_id = i.intent_id
             WHERE i.library_id = ?
               AND i.intent_kind IN ('CREATE_DIRECTORY','CREATE_FILE')
               AND i.state IN ('PENDING','BLOCKED','NEEDS_REBASE_VALIDATION')
               AND s.intent_id IS NULL",
        )
        .bind(sequence_i64(generation)?)
        .bind(library_id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        if missing_creates != 0 {
            return Err(ClientSyncError::InvalidState);
        }
        sqlx::query("DELETE FROM observation_scan_seen WHERE library_id = ? AND generation = ?")
            .bind(library_id.to_string())
            .bind(sequence_i64(generation)?)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "DELETE FROM observation_scan_seen_intents WHERE library_id = ? AND generation = ?",
        )
        .bind(library_id.to_string())
        .bind(sequence_i64(generation)?)
        .execute(&mut *transaction)
        .await?;
        let changed = sqlx::query(
            "UPDATE observation_state SET scan_active = 0, last_reconciled_at_ms = ?,
                    updated_at_ms = ? WHERE library_id = ? AND scan_generation = ?",
        )
        .bind(now_ms()?)
        .bind(now_ms()?)
        .bind(library_id.to_string())
        .bind(sequence_i64(generation)?)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        transaction.commit().await?;
        Ok(())
    }

    pub(crate) async fn observation_suppressions_for_path(
        &self,
        library_id: LibraryId,
        relative_path: &ManagedRelativePath,
    ) -> Result<Vec<ObservationSuppression>, ClientSyncError> {
        let rows = sqlx::query(
            "SELECT * FROM observation_suppressions WHERE library_id = ?
             ORDER BY created_at_ms, operation_id LIMIT 4097",
        )
        .bind(library_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        if rows.len() > MAX_OBSERVATION_QUERY_ROWS {
            return Err(ClientSyncError::ResourceLimit);
        }
        let suppressions: Vec<_> = rows
            .into_iter()
            .map(decode_observation_suppression)
            .collect::<Result<_, _>>()?;
        Ok(suppressions
            .into_iter()
            .filter(|suppression| {
                relative_path.as_str() == suppression.expected_relative_path.as_str()
                    || relative_path
                        .as_str()
                        .starts_with(&format!("{}/", suppression.expected_relative_path.as_str()))
            })
            .collect())
    }

    pub(crate) async fn retire_observation_suppressions(
        &self,
        library_id: LibraryId,
    ) -> Result<(), ClientSyncError> {
        sqlx::query("DELETE FROM observation_suppressions WHERE library_id = ?")
            .bind(library_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn begin_bootstrap(
        &self,
        scope: ReplicaScope,
        bootstrap: SyncBootstrap,
    ) -> Result<(), ClientSyncError> {
        if bootstrap.owner_user_id() != scope.owner_user_id()
            || bootstrap.device_id() != scope.device_id()
            || bootstrap.library_id() != scope.library_id()
        {
            return Err(ClientSyncError::WrongScope);
        }
        // A rebaseline changes the server epoch/base even before the new
        // manifest is applied. Preserve every local intent and require a
        // future explicit validation instead of silently reusing stale bases.
        let _ = self
            .mark_outbound_intents_needing_rebase(scope.library_id(), None)
            .await?;
        let existing = self.bootstrap(scope.library_id()).await?;
        if let Some(existing) = existing {
            if existing.bootstrap_id() != bootstrap.id()
                || existing.generation() != bootstrap.generation()
            {
                return Err(ClientSyncError::InvalidState);
            }
            return Ok(());
        }
        let now = now_ms()?;
        sqlx::query(
            "INSERT INTO bootstrap_sessions (
                 library_id, bootstrap_id, generation, snapshot_epoch,
                 resume_sequence, manifest_item_count, state,
                 created_at_ms, updated_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, 'FETCHING', ?, ?)",
        )
        .bind(scope.library_id().to_string())
        .bind(bootstrap.id().to_string())
        .bind(sequence_i64(bootstrap.generation())?)
        .bind(sequence_i64(bootstrap.snapshot_epoch())?)
        .bind(sequence_i64(bootstrap.snapshot_resume_sequence())?)
        .bind(u64_i64(bootstrap.manifest_item_count())?)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.set_status(scope.library_id(), EngineStatus::Bootstrapping)
            .await
    }

    pub async fn bootstrap(
        &self,
        library_id: LibraryId,
    ) -> Result<Option<BootstrapRecord>, ClientSyncError> {
        let row = sqlx::query("SELECT * FROM bootstrap_sessions WHERE library_id = ?")
            .bind(library_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(decode_bootstrap).transpose()
    }

    pub(crate) async fn persist_bootstrap_page(
        &self,
        scope: ReplicaScope,
        page: &crate::BootstrapPage,
    ) -> Result<(), ClientSyncError> {
        let bootstrap = page.bootstrap();
        if bootstrap.owner_user_id() != scope.owner_user_id()
            || bootstrap.device_id() != scope.device_id()
            || bootstrap.library_id() != scope.library_id()
        {
            return Err(ClientSyncError::WrongScope);
        }
        if page.has_more() == page.next_cursor().is_none()
            || page.has_more() == page.completion_evidence().is_some()
        {
            return Err(ClientSyncError::InvalidRemoteResponse);
        }
        let mut transaction = self.pool.begin().await?;
        let expected: (String, i64, i64) = sqlx::query_as(
            "SELECT bootstrap_id, generation, manifest_item_count
             FROM bootstrap_sessions WHERE library_id = ? AND state = 'FETCHING'",
        )
        .bind(scope.library_id().to_string())
        .fetch_one(&mut *transaction)
        .await?;
        if expected.0 != bootstrap.id().to_string()
            || expected.1 != sequence_i64(bootstrap.generation())?
        {
            return Err(ClientSyncError::InvalidState);
        }
        for node in page.nodes() {
            insert_bootstrap_node(&mut transaction, scope.library_id(), bootstrap.id(), node)
                .await?;
        }
        let durable_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM bootstrap_nodes WHERE library_id = ? AND bootstrap_id = ?",
        )
        .bind(scope.library_id().to_string())
        .bind(bootstrap.id().to_string())
        .fetch_one(&mut *transaction)
        .await?;
        if durable_count > expected.2 {
            return Err(ClientSyncError::InvalidRemoteResponse);
        }
        if !page.has_more() {
            if durable_count != expected.2 {
                return Err(ClientSyncError::InvalidRemoteResponse);
            }
            validate_terminal_manifest(
                &mut transaction,
                scope.library_id(),
                bootstrap.id(),
                durable_count,
            )
            .await?;
        }
        let changed = sqlx::query(
            "UPDATE bootstrap_sessions SET next_cursor = ?, completion_evidence = ?,
                    terminal_fetched = ?, state = ?, updated_at_ms = ?
             WHERE library_id = ? AND bootstrap_id = ?",
        )
        .bind(page.next_cursor().map(OpaqueEvidence::as_bytes))
        .bind(page.completion_evidence().map(OpaqueEvidence::as_bytes))
        .bind(!page.has_more())
        .bind(if page.has_more() {
            "FETCHING"
        } else {
            "MANIFEST_DURABLE"
        })
        .bind(now_ms()?)
        .bind(scope.library_id().to_string())
        .bind(bootstrap.id().to_string())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        transaction.commit().await?;
        Ok(())
    }

    pub(crate) async fn unapplied_bootstrap_nodes(
        &self,
        library_id: LibraryId,
        bootstrap_id: SyncBootstrapId,
    ) -> Result<Vec<LogicalSnapshotNode>, ClientSyncError> {
        let rows = sqlx::query(
            "WITH RECURSIVE desired_tree(node_id, depth) AS (
                 SELECT node_id, 0 FROM bootstrap_nodes
                 WHERE library_id = ? AND bootstrap_id = ? AND parent_node_id IS NULL
                 UNION ALL
                 SELECT child.node_id, desired_tree.depth + 1
                 FROM bootstrap_nodes child
                 JOIN desired_tree ON child.parent_node_id = desired_tree.node_id
                 WHERE child.library_id = ? AND child.bootstrap_id = ?
             )
             SELECT b.* FROM bootstrap_nodes b
             JOIN desired_tree t ON t.node_id = b.node_id
             WHERE b.library_id = ? AND b.bootstrap_id = ? AND b.applied = 0
             ORDER BY t.depth, b.node_id LIMIT 1000",
        )
        .bind(library_id.to_string())
        .bind(bootstrap_id.to_string())
        .bind(library_id.to_string())
        .bind(bootstrap_id.to_string())
        .bind(library_id.to_string())
        .bind(bootstrap_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(decode_snapshot_node).collect()
    }

    pub(crate) async fn record_bootstrap_node_applied(
        &self,
        node: &LocalNode,
        bootstrap_id: SyncBootstrapId,
        operation_id: Option<Uuid>,
    ) -> Result<(), ClientSyncError> {
        let mut transaction = self.pool.begin().await?;
        upsert_local_node_tx(&mut transaction, node).await?;
        let changed = sqlx::query(
            "UPDATE bootstrap_nodes SET applied = 1
             WHERE library_id = ? AND bootstrap_id = ? AND node_id = ?",
        )
        .bind(node.library_id().to_string())
        .bind(bootstrap_id.to_string())
        .bind(node.node_id().to_string())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        if let Some(operation_id) = operation_id {
            insert_observation_suppression_tx(&mut transaction, operation_id, Some(node)).await?;
            let changed = sqlx::query(
                "UPDATE local_operations SET state = 'DATABASE_COMMITTED', updated_at_ms = ?
                 WHERE operation_id = ? AND state = 'FILESYSTEM_APPLIED'",
            )
            .bind(now_ms()?)
            .bind(operation_id.to_string())
            .execute(&mut *transaction)
            .await?
            .rows_affected();
            if changed != 1 {
                return Err(ClientSyncError::InvalidState);
            }
        }
        transaction.commit().await?;
        Ok(())
    }

    pub(crate) async fn bootstrap_sweep_candidates(
        &self,
        library_id: LibraryId,
        generation: Sequence,
    ) -> Result<Vec<LocalNode>, ClientSyncError> {
        let rows = sqlx::query(
            "SELECT * FROM local_nodes
             WHERE library_id = ? AND bootstrap_generation != ?
             ORDER BY length(relative_path) DESC LIMIT 1000",
        )
        .bind(library_id.to_string())
        .bind(sequence_i64(generation)?)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(decode_local_node).collect()
    }

    pub(crate) async fn mark_bootstrap_local_complete(
        &self,
        library_id: LibraryId,
        bootstrap_id: SyncBootstrapId,
    ) -> Result<(), ClientSyncError> {
        let mut transaction = self.pool.begin().await?;
        let row: (i64, i64, i64) = sqlx::query_as(
            "SELECT manifest_item_count,
                    (SELECT COUNT(*) FROM bootstrap_nodes b WHERE b.library_id = bootstrap_sessions.library_id AND b.bootstrap_id = bootstrap_sessions.bootstrap_id),
                    (SELECT COUNT(*) FROM bootstrap_nodes b WHERE b.library_id = bootstrap_sessions.library_id AND b.bootstrap_id = bootstrap_sessions.bootstrap_id AND b.applied = 1)
             FROM bootstrap_sessions
             WHERE library_id = ? AND bootstrap_id = ? AND terminal_fetched = 1",
        )
        .bind(library_id.to_string())
        .bind(bootstrap_id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        if row.0 != row.1 || row.1 != row.2 {
            return Err(ClientSyncError::InvalidState);
        }
        let changed = sqlx::query(
            "UPDATE bootstrap_sessions SET state = 'COMPLETION_PENDING', updated_at_ms = ?
             WHERE library_id = ? AND bootstrap_id = ? AND completion_evidence IS NOT NULL",
        )
        .bind(now_ms()?)
        .bind(library_id.to_string())
        .bind(bootstrap_id.to_string())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        transaction.commit().await?;
        Ok(())
    }

    pub(crate) async fn complete_bootstrap(
        &self,
        scope: ReplicaScope,
        bootstrap_id: SyncBootstrapId,
        epoch: Sequence,
        acknowledged: Sequence,
    ) -> Result<(), ClientSyncError> {
        let mut transaction = self.pool.begin().await?;
        let record: (i64, i64) = sqlx::query_as(
            "SELECT snapshot_epoch, resume_sequence FROM bootstrap_sessions
             WHERE library_id = ? AND bootstrap_id = ? AND state = 'COMPLETION_PENDING'",
        )
        .bind(scope.library_id().to_string())
        .bind(bootstrap_id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        let acknowledged = sequence_i64(acknowledged)?;
        if record.0 != sequence_i64(epoch)? || record.1 != acknowledged {
            return Err(ClientSyncError::InvalidRemoteResponse);
        }
        let root_node_id: Option<String> = sqlx::query_scalar(
            "SELECT node_id FROM local_nodes WHERE library_id = ? AND parent_node_id IS NULL",
        )
        .bind(scope.library_id().to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        let root_node_id = root_node_id.ok_or(ClientSyncError::InvalidState)?;
        let changed = sqlx::query(
            "UPDATE replicas SET root_node_id = ?, journal_epoch = ?,
                    applied_sequence = ?, acknowledged_sequence = ?, status = 'IDLE',
                    updated_at_ms = ?
             WHERE library_id = ? AND acknowledged_sequence <= ?",
        )
        .bind(root_node_id)
        .bind(sequence_i64(epoch)?)
        .bind(acknowledged)
        .bind(acknowledged)
        .bind(now_ms()?)
        .bind(scope.library_id().to_string())
        .bind(acknowledged)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        let deleted = sqlx::query("DELETE FROM bootstrap_sessions WHERE library_id = ?")
            .bind(scope.library_id().to_string())
            .execute(&mut *transaction)
            .await?
            .rows_affected();
        if deleted != 1 {
            return Err(ClientSyncError::InvalidState);
        }
        transaction.commit().await?;
        Ok(())
    }
}

async fn validate_terminal_manifest(
    transaction: &mut Transaction<'_, Sqlite>,
    library_id: LibraryId,
    bootstrap_id: SyncBootstrapId,
    expected_count: i64,
) -> Result<(), ClientSyncError> {
    let root_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM bootstrap_nodes
         WHERE library_id = ? AND bootstrap_id = ? AND parent_node_id IS NULL",
    )
    .bind(library_id.to_string())
    .bind(bootstrap_id.to_string())
    .fetch_one(&mut **transaction)
    .await?;
    if root_count != 1 {
        return Err(ClientSyncError::InvalidRemoteResponse);
    }

    let invalid_parent_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM bootstrap_nodes child
         LEFT JOIN bootstrap_nodes parent
           ON parent.library_id = child.library_id
          AND parent.bootstrap_id = child.bootstrap_id
          AND parent.node_id = child.parent_node_id
         WHERE child.library_id = ? AND child.bootstrap_id = ?
           AND child.parent_node_id IS NOT NULL
           AND (parent.node_id IS NULL OR parent.node_kind != 'DIRECTORY')",
    )
    .bind(library_id.to_string())
    .bind(bootstrap_id.to_string())
    .fetch_one(&mut **transaction)
    .await?;
    if invalid_parent_count != 0 {
        return Err(ClientSyncError::InvalidRemoteResponse);
    }

    let reachable_count: i64 = sqlx::query_scalar(
        "WITH RECURSIVE reachable(node_id) AS (
             SELECT node_id FROM bootstrap_nodes
             WHERE library_id = ? AND bootstrap_id = ? AND parent_node_id IS NULL
             UNION
             SELECT child.node_id
             FROM bootstrap_nodes child
             JOIN reachable parent ON child.parent_node_id = parent.node_id
             WHERE child.library_id = ? AND child.bootstrap_id = ?
         )
         SELECT COUNT(*) FROM reachable",
    )
    .bind(library_id.to_string())
    .bind(bootstrap_id.to_string())
    .bind(library_id.to_string())
    .bind(bootstrap_id.to_string())
    .fetch_one(&mut **transaction)
    .await?;
    if reachable_count != expected_count {
        return Err(ClientSyncError::InvalidRemoteResponse);
    }
    Ok(())
}

impl Drop for LocalStateStore {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.writer_lock);
    }
}

fn validate_page(page: &RemoteFeedPage) -> Result<(), ClientSyncError> {
    if page.high_watermark() < page.through_sequence()
        || page.from_sequence() > page.through_sequence()
        || page.has_more() != (page.through_sequence() < page.high_watermark())
    {
        return Err(ClientSyncError::InvalidRemoteResponse);
    }
    if page.changes().is_empty() {
        if page.from_sequence() != page.through_sequence() || page.ack_evidence().is_some() {
            return Err(ClientSyncError::InvalidRemoteResponse);
        }
        return Ok(());
    }
    if page.ack_evidence().is_none() {
        return Err(ClientSyncError::InvalidRemoteResponse);
    }
    let mut expected = page
        .from_sequence()
        .get()
        .checked_add(1)
        .ok_or(ClientSyncError::InvalidRemoteResponse)?;
    for change in page.changes() {
        let event = change.event();
        if event.owner_user_id() != page.scope().owner_user_id()
            || event.library_id() != page.scope().library_id()
        {
            return Err(ClientSyncError::WrongScope);
        }
        if event.journal_epoch() != page.epoch() {
            return Err(ClientSyncError::WrongEpoch);
        }
        if event.sequence().get() < expected {
            return Err(ClientSyncError::SequenceRegression);
        }
        if event.sequence().get() > expected {
            return Err(ClientSyncError::SequenceGap);
        }
        if event.schema_version() != 1 {
            return Err(ClientSyncError::UnsupportedSchemaVersion);
        }
        if event.resource_kind() != synveil_core::ChangeResourceKind::Node {
            return Err(ClientSyncError::InvalidRemoteResponse);
        }
        match (event.change_kind(), change.desired_node()) {
            (ChangeKind::NodePurged, None) => {}
            (ChangeKind::NodePurged, Some(_)) | (_, None) => {
                return Err(ClientSyncError::InvalidRemoteResponse);
            }
            (_, Some(node)) => {
                if node.node_id() != event.resource_id()
                    || node.revision() != event.resource_revision()
                    || event
                        .parent_node_id()
                        .is_some_and(|id| Some(id) != node.parent_node_id())
                    || event.node_kind().is_some_and(|kind| kind != node.kind())
                    || event
                        .node_state()
                        .is_some_and(|state| state != node.state())
                    || event
                        .current_version_id()
                        .is_some_and(|version| Some(version) != node.current_version_id())
                {
                    return Err(ClientSyncError::InvalidRemoteResponse);
                }
                match event.change_kind() {
                    ChangeKind::NodeTrashed if node.state() != NodeState::Trashed => {
                        return Err(ClientSyncError::InvalidRemoteResponse);
                    }
                    ChangeKind::NodeRestored if node.state() != NodeState::Active => {
                        return Err(ClientSyncError::InvalidRemoteResponse);
                    }
                    ChangeKind::FileContentCommitted | ChangeKind::FileVersionRestored
                        if node.kind() != NodeKind::File
                            || node.state() != NodeState::Active
                            || node.current_version_id().is_none()
                            || node.content_length().is_none()
                            || node.content_sha256().is_none() =>
                    {
                        return Err(ClientSyncError::InvalidRemoteResponse);
                    }
                    _ => {}
                }
            }
        }
        expected = expected
            .checked_add(1)
            .ok_or(ClientSyncError::InvalidRemoteResponse)?;
    }
    if expected - 1 != page.through_sequence().get() {
        return Err(ClientSyncError::SequenceGap);
    }
    Ok(())
}

async fn insert_pending_change(
    transaction: &mut Transaction<'_, Sqlite>,
    library_id: LibraryId,
    change: &InboundChange,
) -> Result<(), ClientSyncError> {
    let event = change.event();
    let desired = change.desired_node();
    let changed = sqlx::query(
        "INSERT INTO pending_events (
             library_id, sequence, event_id, schema_version, resource_id,
             change_kind, resource_revision, parent_node_id, logical_name,
             node_kind, node_state, current_version_id, content_length,
             content_sha256
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(library_id.to_string())
    .bind(sequence_i64(event.sequence())?)
    .bind(event.id().to_string())
    .bind(i64::from(event.schema_version()))
    .bind(event.resource_id().to_string())
    .bind(event.change_kind().as_str())
    .bind(revision_i64(event.resource_revision())?)
    .bind(
        desired
            .and_then(LogicalSnapshotNode::parent_node_id)
            .map(|id| id.to_string()),
    )
    .bind(desired.map(|node| node.name().as_str()))
    .bind(desired.map(|node| node_kind_as_str(node.kind())))
    .bind(desired.map(|node| node_state_as_str(node.state())))
    .bind(
        desired
            .and_then(LogicalSnapshotNode::current_version_id)
            .map(|id| id.to_string()),
    )
    .bind(optional_u64_i64(
        desired.and_then(LogicalSnapshotNode::content_length),
    )?)
    .bind(
        desired
            .and_then(LogicalSnapshotNode::content_sha256)
            .map(|hash| hash.into_bytes().to_vec()),
    )
    .execute(&mut **transaction)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(ClientSyncError::InvalidRemoteResponse);
    }
    Ok(())
}

async fn insert_bootstrap_node(
    transaction: &mut Transaction<'_, Sqlite>,
    library_id: LibraryId,
    bootstrap_id: SyncBootstrapId,
    node: &LogicalSnapshotNode,
) -> Result<(), ClientSyncError> {
    let changed = sqlx::query(
        "INSERT INTO bootstrap_nodes (
             library_id, bootstrap_id, node_id, parent_node_id, logical_name,
             node_kind, node_state, revision, current_version_id,
             content_length, content_sha256
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(library_id, bootstrap_id, node_id) DO UPDATE SET
             parent_node_id = excluded.parent_node_id,
             logical_name = excluded.logical_name,
             node_kind = excluded.node_kind,
             node_state = excluded.node_state,
             revision = excluded.revision,
             current_version_id = excluded.current_version_id,
             content_length = excluded.content_length,
             content_sha256 = excluded.content_sha256
         WHERE bootstrap_nodes.parent_node_id IS excluded.parent_node_id
           AND bootstrap_nodes.logical_name = excluded.logical_name
           AND bootstrap_nodes.node_kind = excluded.node_kind
           AND bootstrap_nodes.node_state = excluded.node_state
           AND bootstrap_nodes.revision = excluded.revision
           AND bootstrap_nodes.current_version_id IS excluded.current_version_id
           AND bootstrap_nodes.content_length IS excluded.content_length
           AND bootstrap_nodes.content_sha256 IS excluded.content_sha256",
    )
    .bind(library_id.to_string())
    .bind(bootstrap_id.to_string())
    .bind(node.node_id().to_string())
    .bind(node.parent_node_id().map(|id| id.to_string()))
    .bind(node.name().as_str())
    .bind(node_kind_as_str(node.kind()))
    .bind(node_state_as_str(node.state()))
    .bind(revision_i64(node.revision())?)
    .bind(node.current_version_id().map(|id| id.to_string()))
    .bind(optional_u64_i64(node.content_length())?)
    .bind(node.content_sha256().map(|hash| hash.into_bytes().to_vec()))
    .execute(&mut **transaction)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(ClientSyncError::InvalidRemoteResponse);
    }
    Ok(())
}

/// Persist time-independent proof of one completed inbound filesystem action
/// before its local operation row can be cleaned up. A watcher may suppress a
/// change only by later matching this operation, the expected path, NodeId,
/// kind, and file fingerprint where relevant.
async fn insert_observation_suppression_tx(
    transaction: &mut Transaction<'_, Sqlite>,
    operation_id: Uuid,
    node: Option<&LocalNode>,
) -> Result<(), ClientSyncError> {
    let row = sqlx::query("SELECT * FROM local_operations WHERE operation_id = ?")
        .bind(operation_id.to_string())
        .fetch_one(&mut **transaction)
        .await?;
    let operation = decode_operation(row)?;
    if operation.state() != LocalOperationState::FilesystemApplied {
        return Err(ClientSyncError::InvalidState);
    }
    let mut proofs = Vec::new();
    match node {
        Some(node) if node.present() => {
            proofs.push((
                node.relative_path().clone(),
                true,
                Some(local_node_fingerprint(node).ok_or(ClientSyncError::InvalidState)?),
            ));
            if matches!(
                operation.kind(),
                LocalOperationKind::Rename | LocalOperationKind::Move
            ) && let Some(source) = operation.source()
                && source != node.relative_path()
            {
                proofs.push((source.clone(), false, None));
            }
        }
        _ => proofs.push((
            operation
                .source()
                .cloned()
                .ok_or(ClientSyncError::InvalidState)?,
            false,
            None,
        )),
    }
    for (expected_relative_path, expected_present, expected_fingerprint) in proofs {
        sqlx::query(
            "INSERT INTO observation_suppressions (
                 operation_id, library_id, node_id, expected_relative_path,
                 expected_present, expected_kind, expected_length, expected_sha256,
                 created_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(operation_id, expected_relative_path) DO NOTHING",
        )
        .bind(operation.operation_id().to_string())
        .bind(operation.library_id().to_string())
        .bind(operation.node_id().to_string())
        .bind(expected_relative_path.as_str())
        .bind(expected_present)
        .bind(expected_fingerprint.map(fingerprint_kind_as_str))
        .bind(optional_fingerprint_length(expected_fingerprint)?)
        .bind(
            expected_fingerprint
                .and_then(LocalFingerprint::sha256)
                .map(|value| value.into_bytes().to_vec()),
        )
        .bind(now_ms()?)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn upsert_local_node_tx(
    transaction: &mut Transaction<'_, Sqlite>,
    node: &LocalNode,
) -> Result<(), ClientSyncError> {
    let old: Option<(String, String)> = sqlx::query_as(
        "SELECT relative_path, collision_key FROM local_nodes
         WHERE library_id = ? AND node_id = ?",
    )
    .bind(node.library_id().to_string())
    .bind(node.node_id().to_string())
    .fetch_optional(&mut **transaction)
    .await?;
    let collision_key = collision_path_key(node.relative_path());
    if let Some((old_path, old_key)) = old.as_ref()
        && old_path != node.relative_path().as_str()
    {
        let old_prefix = format!("{old_path}/");
        let new_prefix = format!("{}/", node.relative_path().as_str());
        let old_key_prefix = format!("{old_key}/");
        let new_key_prefix = format!("{collision_key}/");
        sqlx::query(
            "UPDATE local_nodes
             SET relative_path = ? || substr(relative_path, length(?) + 1),
                 collision_key = ? || substr(collision_key, length(?) + 1)
             WHERE library_id = ? AND relative_path LIKE ? ESCAPE '\\'",
        )
        .bind(&new_prefix)
        .bind(&old_prefix)
        .bind(&new_key_prefix)
        .bind(&old_key_prefix)
        .bind(node.library_id().to_string())
        .bind(format!("{}%", escape_like(&old_prefix)))
        .execute(&mut **transaction)
        .await?;
    }
    sqlx::query(
        "INSERT INTO local_nodes (
             library_id, node_id, parent_node_id, relative_path, collision_key,
             logical_name, node_kind, node_state, revision, current_version_id,
             content_length, content_sha256, local_length, local_sha256,
             bootstrap_generation, present, quarantine_relative_path
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(library_id, node_id) DO UPDATE SET
             parent_node_id = excluded.parent_node_id,
             relative_path = excluded.relative_path,
             collision_key = excluded.collision_key,
             logical_name = excluded.logical_name,
             node_kind = excluded.node_kind,
             node_state = excluded.node_state,
             revision = excluded.revision,
             current_version_id = excluded.current_version_id,
             content_length = excluded.content_length,
             content_sha256 = excluded.content_sha256,
             local_length = excluded.local_length,
             local_sha256 = excluded.local_sha256,
             bootstrap_generation = excluded.bootstrap_generation,
             present = excluded.present,
             quarantine_relative_path = excluded.quarantine_relative_path",
    )
    .bind(node.library_id().to_string())
    .bind(node.node_id().to_string())
    .bind(node.parent_node_id().map(|id| id.to_string()))
    .bind(node.relative_path().as_str())
    .bind(collision_key)
    .bind(node.logical_name().as_str())
    .bind(node_kind_as_str(node.kind()))
    .bind(node_state_as_str(node.state()))
    .bind(revision_i64(node.revision())?)
    .bind(node.current_version_id().map(|id| id.to_string()))
    .bind(optional_u64_i64(node.content_length())?)
    .bind(node.content_sha256().map(|hash| hash.into_bytes().to_vec()))
    .bind(optional_u64_i64(node.local_length())?)
    .bind(node.local_sha256().map(|hash| hash.into_bytes().to_vec()))
    .bind(sequence_i64(node.bootstrap_generation())?)
    .bind(node.present())
    .bind(
        node.quarantine_relative_path()
            .map(ManagedRelativePath::as_str),
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn trim_event_evidence(
    transaction: &mut Transaction<'_, Sqlite>,
    library_id: LibraryId,
    epoch: Sequence,
) -> Result<(), ClientSyncError> {
    sqlx::query(
        "DELETE FROM applied_events WHERE library_id = ? AND epoch = ?
         AND sequence <= COALESCE((
             SELECT sequence FROM applied_events
             WHERE library_id = ? AND epoch = ?
             ORDER BY sequence DESC LIMIT 1 OFFSET ?
         ), -1)",
    )
    .bind(library_id.to_string())
    .bind(sequence_i64(epoch)?)
    .bind(library_id.to_string())
    .bind(sequence_i64(epoch)?)
    .bind(MAX_EVENT_EVIDENCE_ROWS)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn decode_replica(row: sqlx::sqlite::SqliteRow) -> Result<ReplicaRecord, ClientSyncError> {
    let library_id = parse_id(row.try_get("library_id")?)?;
    Ok(ReplicaRecord {
        server_profile_id: row
            .try_get::<Option<String>, _>("server_profile_id")?
            .map(|value| value.parse())
            .transpose()?,
        scope: ReplicaScope::new(
            parse_id(row.try_get("owner_user_id")?)?,
            parse_id(row.try_get("device_id")?)?,
            library_id,
        ),
        root_binding_id: RootBindingId::parse(row.try_get("root_binding_id")?)?,
        root_node_id: optional_id(row.try_get("root_node_id")?)?,
        journal_epoch: sequence_from_i64(row.try_get("journal_epoch")?)?,
        applied_sequence: sequence_from_i64(row.try_get("applied_sequence")?)?,
        acknowledged_sequence: sequence_from_i64(row.try_get("acknowledged_sequence")?)?,
        status: parse_status(row.try_get("status")?)?,
    })
}

fn decode_local_node(row: sqlx::sqlite::SqliteRow) -> Result<LocalNode, ClientSyncError> {
    Ok(LocalNode::new(
        parse_id(row.try_get("library_id")?)?,
        parse_id(row.try_get("node_id")?)?,
        optional_id(row.try_get("parent_node_id")?)?,
        ManagedRelativePath::new(row.try_get::<String, _>("relative_path")?)?,
        LogicalName::new(row.try_get::<String, _>("logical_name")?)
            .map_err(|_| ClientSyncError::InvalidState)?,
        parse_node_kind(row.try_get("node_kind")?)?,
        parse_node_state(row.try_get("node_state")?)?,
        Revision::new(u64_from_i64(row.try_get("revision")?)?),
        optional_id(row.try_get("current_version_id")?)?,
        optional_u64(row.try_get("content_length")?)?,
        optional_hash(row.try_get("content_sha256")?)?,
        optional_u64(row.try_get("local_length")?)?,
        optional_hash(row.try_get("local_sha256")?)?,
        sequence_from_i64(row.try_get("bootstrap_generation")?)?,
        row.try_get("present")?,
        row.try_get::<Option<String>, _>("quarantine_relative_path")?
            .map(ManagedRelativePath::new)
            .transpose()?,
    ))
}

fn decode_stored_change(row: sqlx::sqlite::SqliteRow) -> Result<StoredChange, ClientSyncError> {
    let name: Option<String> = row.try_get("logical_name")?;
    let desired_node = name
        .map(|name| {
            LogicalSnapshotNode::new(
                parse_id(row.try_get("resource_id")?)?,
                optional_id(row.try_get("parent_node_id")?)?,
                LogicalName::new(name).map_err(|_| ClientSyncError::InvalidState)?,
                parse_node_kind(row.try_get("node_kind")?)?,
                parse_node_state(row.try_get("node_state")?)?,
                Revision::new(u64_from_i64(row.try_get("resource_revision")?)?),
                optional_id(row.try_get("current_version_id")?)?,
                optional_u64(row.try_get("content_length")?)?,
                optional_hash(row.try_get("content_sha256")?)?,
            )
            .map_err(|_| ClientSyncError::InvalidState)
        })
        .transpose()?;
    Ok(StoredChange {
        sequence: sequence_from_i64(row.try_get("sequence")?)?,
        event_id: parse_id(row.try_get("event_id")?)?,
        change_kind: ChangeKind::from_str(row.try_get("change_kind")?)
            .map_err(|_| ClientSyncError::InvalidState)?,
        resource_id: parse_id(row.try_get("resource_id")?)?,
        resource_revision: Revision::new(u64_from_i64(row.try_get("resource_revision")?)?),
        desired_node,
    })
}

fn decode_bootstrap(row: sqlx::sqlite::SqliteRow) -> Result<BootstrapRecord, ClientSyncError> {
    Ok(BootstrapRecord {
        library_id: parse_id(row.try_get("library_id")?)?,
        bootstrap_id: parse_id(row.try_get("bootstrap_id")?)?,
        generation: sequence_from_i64(row.try_get("generation")?)?,
        snapshot_epoch: sequence_from_i64(row.try_get("snapshot_epoch")?)?,
        resume_sequence: sequence_from_i64(row.try_get("resume_sequence")?)?,
        manifest_item_count: u64_from_i64(row.try_get("manifest_item_count")?)?,
        state: row.try_get("state")?,
        next_cursor: optional_evidence(row.try_get("next_cursor")?)?,
        completion_evidence: optional_evidence(row.try_get("completion_evidence")?)?,
        terminal_fetched: row.try_get("terminal_fetched")?,
    })
}

fn decode_snapshot_node(
    row: sqlx::sqlite::SqliteRow,
) -> Result<LogicalSnapshotNode, ClientSyncError> {
    LogicalSnapshotNode::new(
        parse_id(row.try_get("node_id")?)?,
        optional_id(row.try_get("parent_node_id")?)?,
        LogicalName::new(row.try_get::<String, _>("logical_name")?)
            .map_err(|_| ClientSyncError::InvalidState)?,
        parse_node_kind(row.try_get("node_kind")?)?,
        parse_node_state(row.try_get("node_state")?)?,
        Revision::new(u64_from_i64(row.try_get("revision")?)?),
        optional_id(row.try_get("current_version_id")?)?,
        optional_u64(row.try_get("content_length")?)?,
        optional_hash(row.try_get("content_sha256")?)?,
    )
    .map_err(|_| ClientSyncError::InvalidState)
}

fn decode_operation(row: sqlx::sqlite::SqliteRow) -> Result<LocalOperation, ClientSyncError> {
    Ok(LocalOperation {
        operation_id: Uuid::parse_str(row.try_get("operation_id")?)
            .map_err(|_| ClientSyncError::InvalidState)?,
        library_id: parse_id(row.try_get("library_id")?)?,
        node_id: parse_id(row.try_get("node_id")?)?,
        server_sequence: optional_sequence(row.try_get("server_sequence")?)?,
        bootstrap_generation: optional_sequence(row.try_get("bootstrap_generation")?)?,
        kind: LocalOperationKind::parse(row.try_get("operation_kind")?)?,
        source: optional_path(row.try_get("source_relative_path")?)?,
        destination: optional_path(row.try_get("destination_relative_path")?)?,
        staging: optional_path(row.try_get("staging_relative_path")?)?,
        expected_kind: row
            .try_get::<Option<String>, _>("expected_kind")?
            .as_deref()
            .map(parse_node_kind)
            .transpose()?,
        expected_length: optional_u64(row.try_get("expected_length")?)?,
        expected_sha256: optional_hash(row.try_get("expected_sha256")?)?,
        desired_revision: Revision::new(u64_from_i64(row.try_get("desired_revision")?)?),
        desired_version_id: optional_id(row.try_get("desired_version_id")?)?,
        desired_length: optional_u64(row.try_get("desired_length")?)?,
        desired_sha256: optional_hash(row.try_get("desired_sha256")?)?,
        state: LocalOperationState::parse(row.try_get("state")?)?,
    })
}

fn decode_issue(row: sqlx::sqlite::SqliteRow) -> Result<LocalApplyIssue, ClientSyncError> {
    Ok(LocalApplyIssue {
        issue_id: Uuid::parse_str(row.try_get("issue_id")?)
            .map_err(|_| ClientSyncError::InvalidState)?,
        library_id: parse_id(row.try_get("library_id")?)?,
        node_id: optional_id(row.try_get("node_id")?)?,
        server_sequence: optional_sequence(row.try_get("server_sequence")?)?,
        bootstrap_generation: optional_sequence(row.try_get("bootstrap_generation")?)?,
        kind: LocalIssueKind::parse(row.try_get("issue_kind")?)?,
        expected_state: row.try_get("expected_state")?,
    })
}

fn decode_observation_state(
    row: sqlx::sqlite::SqliteRow,
) -> Result<ObservationState, ClientSyncError> {
    Ok(ObservationState::new(
        parse_id(row.try_get("library_id")?)?,
        row.try_get("rescan_required")?,
        row.try_get("scan_active")?,
        sequence_from_i64(row.try_get("scan_generation")?)?,
    ))
}

fn decode_observation_issue(
    row: sqlx::sqlite::SqliteRow,
) -> Result<ObservationIssue, ClientSyncError> {
    Ok(ObservationIssue::new(
        Uuid::parse_str(row.try_get("issue_id")?).map_err(|_| ClientSyncError::InvalidState)?,
        parse_id(row.try_get("library_id")?)?,
        optional_id(row.try_get("node_id")?)?,
        row.try_get::<Option<String>, _>("relative_path")?
            .map(ManagedRelativePath::new)
            .transpose()?,
        ObservationIssueKind::from_str(row.try_get("issue_kind")?)?,
    ))
}

fn decode_scan_work(row: sqlx::sqlite::SqliteRow) -> Result<ObservationScanWork, ClientSyncError> {
    let cursor_name: Option<String> = row.try_get("cursor_name")?;
    if cursor_name
        .as_ref()
        .is_some_and(|value| value.is_empty() || value.len() > 1_024)
    {
        return Err(ClientSyncError::InvalidState);
    }
    Ok(ObservationScanWork {
        generation: sequence_from_i64(row.try_get("generation")?)?,
        relative_path: ManagedRelativePath::new(row.try_get::<String, _>("relative_path")?)?,
        cursor_name,
    })
}

fn decode_observed_node(
    row: sqlx::sqlite::SqliteRow,
) -> Result<ObservedLocalNode, ClientSyncError> {
    let observed_path: Option<String> = row.try_get("observed_relative_path")?;
    let observed_present: Option<bool> = row.try_get("observed_present")?;
    let observed_kind: Option<String> = row.try_get("observed_kind")?;
    let observed_length: Option<i64> = row.try_get("observed_length")?;
    let observed_sha256: Option<Vec<u8>> = row.try_get("observed_sha256")?;
    let node = decode_local_node(row)?;
    let relative_path = observed_path
        .map(ManagedRelativePath::new)
        .transpose()?
        .unwrap_or_else(|| node.relative_path().clone());
    let present = observed_present.unwrap_or(node.present());
    let fingerprint = if observed_present.is_some() {
        fingerprint_from_columns(observed_kind.as_deref(), observed_length, observed_sha256)?
    } else {
        local_node_fingerprint(&node)
    };
    validate_observed_fingerprint(present, fingerprint)?;
    Ok(ObservedLocalNode {
        node,
        relative_path,
        present,
        fingerprint,
    })
}

fn decode_observation_suppression(
    row: sqlx::sqlite::SqliteRow,
) -> Result<ObservationSuppression, ClientSyncError> {
    let expected_present: bool = row.try_get("expected_present")?;
    let fingerprint = fingerprint_from_columns(
        row.try_get::<Option<String>, _>("expected_kind")?
            .as_deref(),
        row.try_get("expected_length")?,
        row.try_get("expected_sha256")?,
    )?;
    validate_observed_fingerprint(expected_present, fingerprint)?;
    Ok(ObservationSuppression {
        operation_id: Uuid::parse_str(row.try_get("operation_id")?)
            .map_err(|_| ClientSyncError::InvalidState)?,
        node_id: parse_id(row.try_get("node_id")?)?,
        expected_relative_path: ManagedRelativePath::new(
            row.try_get::<String, _>("expected_relative_path")?,
        )?,
        expected_present,
        expected_fingerprint: fingerprint,
    })
}

fn decode_outbound_intent(row: sqlx::sqlite::SqliteRow) -> Result<OutboundIntent, ClientSyncError> {
    let observed_fingerprint = fingerprint_from_columns(
        row.try_get::<Option<String>, _>("observed_kind")?
            .as_deref(),
        row.try_get("observed_length")?,
        row.try_get("observed_sha256")?,
    )?;
    let dedupe = optional_hash(Some(row.try_get::<Vec<u8>, _>("dedupe_sha256")?))?
        .ok_or(ClientSyncError::InvalidState)?;
    OutboundIntent::rehydrate(
        parse_id(row.try_get("intent_id")?)?,
        parse_id(row.try_get("library_id")?)?,
        optional_id(row.try_get("node_id")?)?,
        optional_id(row.try_get("parent_node_id")?)?,
        OutboundIntentKind::from_str(row.try_get("intent_kind")?)?,
        OutboundIntentState::from_str(row.try_get("state")?)?,
        ManagedRelativePath::new(row.try_get::<String, _>("observed_relative_path")?)?,
        optional_path(row.try_get("old_relative_path")?)?,
        observed_fingerprint,
        sequence_from_i64(row.try_get("base_epoch")?)?,
        sequence_from_i64(row.try_get("base_applied_sequence")?)?,
        optional_u64(row.try_get("base_revision")?)?.map(Revision::new),
        optional_id(row.try_get("base_current_version_id")?)?,
        optional_u64(row.try_get("base_parent_revision")?)?.map(Revision::new),
        dedupe,
    )
}

fn decode_mutation_record(
    row: sqlx::sqlite::SqliteRow,
) -> Result<OutboundMutationRecord, ClientSyncError> {
    let intent_id = parse_id(row.try_get("intent_id")?)?;
    let mutation_id = parse_id(row.try_get("mutation_id")?)?;
    let request_json: String = row.try_get("request_json")?;
    let request = crate::outbound::mutation_request_from_json(&request_json)?;
    if request.mutation_id() != mutation_id
        || request.kind().as_str() != row.try_get::<&str, _>("mutation_kind")?
        || sequence_i64(request.base_epoch())? != row.try_get::<i64, _>("base_epoch")?
        || sequence_i64(request.base_sequence())? != row.try_get::<i64, _>("base_sequence")?
        || i64::from(request.fingerprint().version())
            != row.try_get::<i64, _>("fingerprint_version")?
        || request.fingerprint().sha256().to_vec()
            != row.try_get::<Vec<u8>, _>("fingerprint_sha256")?
    {
        return Err(ClientSyncError::InvalidState);
    }
    Ok(OutboundMutationRecord {
        intent_id,
        mutation_id,
        request,
        request_json,
    })
}

fn decode_upload_record(
    row: sqlx::sqlite::SqliteRow,
) -> Result<OutboundUploadRecord, ClientSyncError> {
    let expected_sha256 = optional_hash(Some(row.try_get::<Vec<u8>, _>("expected_sha256")?))?
        .ok_or(ClientSyncError::InvalidState)?;
    Ok(OutboundUploadRecord {
        intent_id: parse_id(row.try_get("intent_id")?)?,
        upload_session_id: optional_id(row.try_get("upload_session_id")?)?,
        staging_relative_path: ManagedRelativePath::new(
            row.try_get::<String, _>("staging_relative_path")?,
        )?,
        expected_length: u64_from_i64(row.try_get("expected_length")?)?,
        expected_sha256,
        acknowledged_offset: u64_from_i64(row.try_get("acknowledged_offset")?)?,
        state: row.try_get("state")?,
    })
}

fn fingerprint_kind_as_str(fingerprint: LocalFingerprint) -> &'static str {
    fingerprint.kind().as_str()
}

fn optional_fingerprint_length(
    fingerprint: Option<LocalFingerprint>,
) -> Result<Option<i64>, ClientSyncError> {
    fingerprint
        .and_then(LocalFingerprint::length)
        .map(u64_i64)
        .transpose()
}

fn fingerprint_from_columns(
    kind: Option<&str>,
    length: Option<i64>,
    sha256: Option<Vec<u8>>,
) -> Result<Option<LocalFingerprint>, ClientSyncError> {
    match kind {
        None => {
            if length.is_some() || sha256.is_some() {
                return Err(ClientSyncError::InvalidState);
            }
            Ok(None)
        }
        Some("DIRECTORY") => {
            if length.is_some() || sha256.is_some() {
                return Err(ClientSyncError::InvalidState);
            }
            Ok(Some(LocalFingerprint::directory()))
        }
        Some("FILE") => {
            let length = optional_u64(length)?.ok_or(ClientSyncError::InvalidState)?;
            let sha256 = optional_hash(sha256)?.ok_or(ClientSyncError::InvalidState)?;
            Ok(Some(LocalFingerprint::file(length, sha256)))
        }
        Some(_) => Err(ClientSyncError::InvalidState),
    }
}

fn validate_observed_fingerprint(
    present: bool,
    fingerprint: Option<LocalFingerprint>,
) -> Result<(), ClientSyncError> {
    if !present && fingerprint.is_some() {
        return Err(ClientSyncError::InvalidState);
    }
    if present && fingerprint.is_none() {
        return Err(ClientSyncError::InvalidState);
    }
    Ok(())
}

fn local_node_fingerprint(node: &LocalNode) -> Option<LocalFingerprint> {
    if !node.present() {
        return None;
    }
    match node.kind() {
        NodeKind::Directory => Some(LocalFingerprint::directory()),
        NodeKind::File => node
            .local_length()
            .zip(node.local_sha256())
            .or_else(|| node.content_length().zip(node.content_sha256()))
            .map(|(length, sha256)| LocalFingerprint::file(length, sha256)),
    }
}

fn parse_id<T>(value: &str) -> Result<T, ClientSyncError>
where
    T: FromStr,
{
    T::from_str(value).map_err(|_| ClientSyncError::InvalidState)
}

fn optional_id<T>(value: Option<String>) -> Result<Option<T>, ClientSyncError>
where
    T: FromStr,
{
    value.as_deref().map(parse_id).transpose()
}

fn parse_node_kind(value: &str) -> Result<NodeKind, ClientSyncError> {
    match value {
        "FILE" => Ok(NodeKind::File),
        "DIRECTORY" => Ok(NodeKind::Directory),
        _ => Err(ClientSyncError::InvalidState),
    }
}

fn parse_node_state(value: &str) -> Result<NodeState, ClientSyncError> {
    match value {
        "ACTIVE" => Ok(NodeState::Active),
        "TRASHED" => Ok(NodeState::Trashed),
        _ => Err(ClientSyncError::InvalidState),
    }
}

const fn node_kind_as_str(value: NodeKind) -> &'static str {
    match value {
        NodeKind::File => "FILE",
        NodeKind::Directory => "DIRECTORY",
    }
}

const fn node_state_as_str(value: NodeState) -> &'static str {
    match value {
        NodeState::Active => "ACTIVE",
        NodeState::Trashed => "TRASHED",
        NodeState::Purging => "PURGING",
    }
}

const fn status_as_str(value: EngineStatus) -> &'static str {
    match value {
        EngineStatus::Idle => "IDLE",
        EngineStatus::Bootstrapping => "BOOTSTRAPPING",
        EngineStatus::Applying => "APPLYING",
        EngineStatus::AckPending => "ACK_PENDING",
        EngineStatus::BlockedLocalIssue => "BLOCKED_LOCAL_ISSUE",
        EngineStatus::Offline => "OFFLINE",
        EngineStatus::Error => "ERROR",
    }
}

fn parse_status(value: &str) -> Result<EngineStatus, ClientSyncError> {
    match value {
        "IDLE" => Ok(EngineStatus::Idle),
        "BOOTSTRAPPING" => Ok(EngineStatus::Bootstrapping),
        "APPLYING" => Ok(EngineStatus::Applying),
        "ACK_PENDING" => Ok(EngineStatus::AckPending),
        "BLOCKED_LOCAL_ISSUE" => Ok(EngineStatus::BlockedLocalIssue),
        "OFFLINE" => Ok(EngineStatus::Offline),
        "ERROR" => Ok(EngineStatus::Error),
        _ => Err(ClientSyncError::InvalidState),
    }
}

fn sequence_i64(value: Sequence) -> Result<i64, ClientSyncError> {
    u64_i64(value.get())
}

fn revision_i64(value: Revision) -> Result<i64, ClientSyncError> {
    u64_i64(value.get())
}

fn u64_i64(value: u64) -> Result<i64, ClientSyncError> {
    i64::try_from(value).map_err(|_| ClientSyncError::ResourceLimit)
}

fn optional_u64_i64(value: Option<u64>) -> Result<Option<i64>, ClientSyncError> {
    value.map(u64_i64).transpose()
}

fn optional_sequence_i64(value: Option<Sequence>) -> Result<Option<i64>, ClientSyncError> {
    value.map(sequence_i64).transpose()
}

fn sequence_from_i64(value: i64) -> Result<Sequence, ClientSyncError> {
    Ok(Sequence::new(u64_from_i64(value)?))
}

fn u64_from_i64(value: i64) -> Result<u64, ClientSyncError> {
    u64::try_from(value).map_err(|_| ClientSyncError::InvalidState)
}

fn optional_u64(value: Option<i64>) -> Result<Option<u64>, ClientSyncError> {
    value.map(u64_from_i64).transpose()
}

fn optional_sequence(value: Option<i64>) -> Result<Option<Sequence>, ClientSyncError> {
    value.map(sequence_from_i64).transpose()
}

fn optional_hash(value: Option<Vec<u8>>) -> Result<Option<Sha256Digest>, ClientSyncError> {
    value
        .map(|bytes| {
            Sha256Digest::try_from(bytes.as_slice()).map_err(|_| ClientSyncError::InvalidState)
        })
        .transpose()
}

fn optional_evidence(value: Option<Vec<u8>>) -> Result<Option<OpaqueEvidence>, ClientSyncError> {
    value.map(OpaqueEvidence::new).transpose()
}

fn optional_path(value: Option<String>) -> Result<Option<ManagedRelativePath>, ClientSyncError> {
    value.map(ManagedRelativePath::new).transpose()
}

fn collision_path_key(path: &ManagedRelativePath) -> String {
    path.as_str()
        .split('/')
        .map(local_collision_key)
        .collect::<Vec<_>>()
        .join("/")
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub(crate) fn now_ms() -> Result<i64, ClientSyncError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ClientSyncError::InvalidState)?;
    i64::try_from(duration.as_millis()).map_err(|_| ClientSyncError::InvalidState)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, str::FromStr};

    use sqlx::Row;
    use synveil_core::{
        DeviceId, LibraryId, LogicalName, NodeId, NodeKind, NodeState, Revision, Sequence,
        SyncBootstrap, SyncBootstrapId, SyncBootstrapState, Timestamp, UserId,
    };

    use super::{LocalNode, LocalOperation, LocalOperationKind, LocalStateConfig, LocalStateStore};
    use crate::{
        LOCAL_SCHEMA_VERSION, ManagedRelativePath, OutboundIntentState, ReplicaScope, RootBindingId,
    };

    fn temporary_database(label: &str) -> (PathBuf, PathBuf) {
        let directory = std::env::temp_dir().join(format!(
            "synveil-client-state-{label}-{}",
            uuid::Uuid::now_v7()
        ));
        fs::create_dir(&directory).unwrap();
        (directory.join("state.sqlite3"), directory)
    }

    fn scope() -> ReplicaScope {
        ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new())
    }

    fn timestamp(value: &str) -> Timestamp {
        Timestamp::from_str(value).unwrap()
    }

    #[tokio::test]
    async fn fresh_migration_reopen_and_single_writer_lock_are_enforced() {
        let (path, directory) = temporary_database("migration");
        let config = LocalStateConfig::new(&path);
        let store = LocalStateStore::open(&config).await.unwrap();
        assert_eq!(store.schema_version().await.unwrap(), LOCAL_SCHEMA_VERSION);
        assert!(matches!(
            LocalStateStore::open(&config).await,
            Err(crate::ClientSyncError::ConcurrentWriter)
        ));
        drop(store);
        let reopened = LocalStateStore::open(&config).await.unwrap();
        assert_eq!(
            reopened.schema_version().await.unwrap(),
            LOCAL_SCHEMA_VERSION
        );
        drop(reopened);
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn replica_binding_survives_restart_and_is_scope_isolated() {
        let (path, directory) = temporary_database("binding");
        let config = LocalStateConfig::new(&path);
        let replica_scope = scope();
        let binding = RootBindingId::new();
        let store = LocalStateStore::open(&config).await.unwrap();
        let replica = store.bind_replica(replica_scope, binding).await.unwrap();
        assert_eq!(replica.scope(), replica_scope);
        drop(store);
        let reopened = LocalStateStore::open(&config).await.unwrap();
        assert_eq!(
            reopened
                .replica(replica_scope.library_id())
                .await
                .unwrap()
                .unwrap(),
            replica
        );
        let other = scope();
        assert!(
            reopened
                .bind_replica(other, RootBindingId::new())
                .await
                .is_ok()
        );
        assert_ne!(other.library_id(), replica_scope.library_id());
        drop(reopened);
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn durability_pragmas_checks_and_foreign_keys_are_enforced() {
        let (path, directory) = temporary_database("pragmas");
        let store = LocalStateStore::open(&LocalStateConfig::new(&path))
            .await
            .unwrap();
        let replica_scope = scope();
        store
            .bind_replica(replica_scope, RootBindingId::new())
            .await
            .unwrap();
        let journal_mode: String = sqlx::query("PRAGMA journal_mode")
            .fetch_one(&store.pool)
            .await
            .unwrap()
            .try_get(0)
            .unwrap();
        let synchronous: i64 = sqlx::query("PRAGMA synchronous")
            .fetch_one(&store.pool)
            .await
            .unwrap()
            .try_get(0)
            .unwrap();
        let foreign_keys: i64 = sqlx::query("PRAGMA foreign_keys")
            .fetch_one(&store.pool)
            .await
            .unwrap()
            .try_get(0)
            .unwrap();
        let busy_timeout: i64 = sqlx::query("PRAGMA busy_timeout")
            .fetch_one(&store.pool)
            .await
            .unwrap()
            .try_get(0)
            .unwrap();
        assert_eq!(journal_mode, "wal");
        assert_eq!(synchronous, 2);
        assert_eq!(foreign_keys, 1);
        assert_eq!(busy_timeout, 5_000);

        assert!(
            sqlx::query(
                "UPDATE replicas SET applied_sequence = 1, acknowledged_sequence = 2
                 WHERE library_id = ?",
            )
            .bind(replica_scope.library_id().to_string())
            .execute(&store.pool)
            .await
            .is_err()
        );
        assert!(
            sqlx::query(
                "INSERT INTO local_nodes (
                     library_id, node_id, parent_node_id, relative_path,
                     collision_key, logical_name, node_kind, node_state,
                     revision, bootstrap_generation, present
                 ) VALUES (?, ?, ?, 'orphan', 'orphan', 'orphan',
                           'DIRECTORY', 'ACTIVE', 1, 0, 1)",
            )
            .bind(replica_scope.library_id().to_string())
            .bind(NodeId::new().to_string())
            .bind(NodeId::new().to_string())
            .execute(&store.pool)
            .await
            .is_err()
        );
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn outbound_submission_migration_persists_mutation_identity_and_results() {
        let (path, directory) = temporary_database("outbound-submission");
        let store = LocalStateStore::open(&LocalStateConfig::new(&path))
            .await
            .unwrap();
        let replica_scope = scope();
        store
            .bind_replica(replica_scope, RootBindingId::new())
            .await
            .unwrap();
        let root_id = NodeId::new();
        store
            .upsert_local_node(&LocalNode::new(
                replica_scope.library_id(),
                root_id,
                None,
                ManagedRelativePath::root(),
                LogicalName::new("root").unwrap(),
                NodeKind::Directory,
                NodeState::Active,
                Revision::new(7),
                None,
                None,
                None,
                None,
                None,
                Sequence::new(1),
                true,
                None,
            ))
            .await
            .unwrap();
        let intent = crate::OutboundIntent::new(
            replica_scope.library_id(),
            None,
            Some(root_id),
            crate::OutboundIntentKind::CreateDirectory,
            ManagedRelativePath::new("created").unwrap(),
            None,
            Some(crate::LocalFingerprint::directory()),
            Sequence::new(1),
            Sequence::new(0),
            None,
            None,
            Some(Revision::new(7)),
        )
        .unwrap();
        let intent = store.upsert_outbound_intent(&intent).await.unwrap();
        let request = synveil_core::ClientMutationRequest::new(
            synveil_core::ClientMutationId::new(),
            Sequence::new(1),
            Sequence::new(0),
            synveil_core::ClientMutation::create_directory(
                root_id,
                Revision::new(7),
                LogicalName::new("created").unwrap(),
            ),
        );
        let json = crate::outbound::mutation_request_to_json(&request).unwrap();
        let first = store
            .persist_mutation_request(intent.intent_id(), &request, &json)
            .await
            .unwrap();
        let second = store
            .persist_mutation_request(intent.intent_id(), &request, &json)
            .await
            .unwrap();
        assert_eq!(first.mutation_id(), second.mutation_id());
        let event_id = synveil_core::ChangeEventId::new();
        store
            .record_mutation_applied(
                intent.intent_id(),
                crate::RemoteMutationApplied::new(
                    request.mutation_id(),
                    root_id,
                    Revision::new(8),
                    event_id,
                    Sequence::new(2),
                    false,
                ),
            )
            .await
            .unwrap();
        assert_eq!(
            store
                .durable_mutation_request(intent.intent_id())
                .await
                .unwrap()
                .unwrap()
                .mutation_id(),
            request.mutation_id()
        );
        assert_eq!(
            store
                .outbound_intent(intent.intent_id())
                .await
                .unwrap()
                .unwrap()
                .state(),
            OutboundIntentState::ServerApplied
        );
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn node_identity_mapping_updates_unicode_descendants_setwise() {
        let (path, directory) = temporary_database("unicode-descendants");
        let store = LocalStateStore::open(&LocalStateConfig::new(&path))
            .await
            .unwrap();
        let replica_scope = scope();
        store
            .bind_replica(replica_scope, RootBindingId::new())
            .await
            .unwrap();
        let root_id = NodeId::new();
        let parent_id = NodeId::new();
        let child_id = NodeId::new();
        let directory_node = |node_id, parent_node_id, relative_path: &str, logical_name: &str| {
            LocalNode::new(
                replica_scope.library_id(),
                node_id,
                parent_node_id,
                ManagedRelativePath::new(relative_path).unwrap(),
                LogicalName::new(logical_name).unwrap(),
                NodeKind::Directory,
                NodeState::Active,
                Revision::new(1),
                None,
                None,
                None,
                None,
                None,
                Sequence::new(1),
                true,
                None,
            )
        };
        store
            .upsert_local_node(&directory_node(root_id, None, "", "root"))
            .await
            .unwrap();
        store
            .upsert_local_node(&directory_node(
                parent_id,
                Some(root_id),
                "dữ-liệu",
                "dữ-liệu",
            ))
            .await
            .unwrap();
        store
            .upsert_local_node(&directory_node(
                child_id,
                Some(parent_id),
                "dữ-liệu/child",
                "child",
            ))
            .await
            .unwrap();

        store
            .upsert_local_node(&directory_node(parent_id, Some(root_id), "mới", "mới"))
            .await
            .unwrap();
        assert_eq!(
            store
                .local_node(replica_scope.library_id(), child_id)
                .await
                .unwrap()
                .unwrap()
                .relative_path()
                .as_str(),
            "mới/child"
        );
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn pending_ack_operation_and_bootstrap_generation_survive_reopen() {
        let (path, directory) = temporary_database("recovery-state");
        let config = LocalStateConfig::new(&path);
        let replica_scope = scope();
        let store = LocalStateStore::open(&config).await.unwrap();
        store
            .bind_replica(replica_scope, RootBindingId::new())
            .await
            .unwrap();
        sqlx::query("UPDATE replicas SET applied_sequence = 1 WHERE library_id = ?")
            .bind(replica_scope.library_id().to_string())
            .execute(&store.pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO pending_acknowledgements (
                 library_id, epoch, from_sequence, through_sequence,
                 high_watermark, evidence, created_at_ms
             ) VALUES (?, 7, 0, 1, 1, ?, 1)",
        )
        .bind(replica_scope.library_id().to_string())
        .bind(b"opaque-ack".as_slice())
        .execute(&store.pool)
        .await
        .unwrap();
        let operation = LocalOperation::new(
            replica_scope.library_id(),
            NodeId::new(),
            Some(Sequence::new(1)),
            None,
            LocalOperationKind::CreateDirectory,
            None,
            Some(ManagedRelativePath::new("folder").unwrap()),
            None,
            None,
            None,
            Revision::new(1),
            None,
            None,
            None,
        );
        store.prepare_operation(&operation).await.unwrap();
        let bootstrap = SyncBootstrap::new(
            SyncBootstrapId::new(),
            replica_scope.owner_user_id(),
            replica_scope.device_id(),
            replica_scope.library_id(),
            Sequence::new(9),
            Sequence::new(7),
            Sequence::new(3),
            1,
            Some(NodeId::new()),
            SyncBootstrapState::Open,
            timestamp("2026-08-27T01:00:00.123456Z"),
            timestamp("2026-08-28T01:00:00.123456Z"),
            None,
        );
        store
            .begin_bootstrap(replica_scope, bootstrap)
            .await
            .unwrap();
        drop(store);

        let reopened = LocalStateStore::open(&config).await.unwrap();
        assert_eq!(
            reopened
                .pending_ack(replica_scope.library_id())
                .await
                .unwrap()
                .unwrap()
                .evidence()
                .as_bytes(),
            b"opaque-ack"
        );
        assert_eq!(
            reopened
                .unfinished_operations(replica_scope.library_id())
                .await
                .unwrap()[0]
                .operation_id(),
            operation.operation_id()
        );
        assert_eq!(
            reopened
                .bootstrap(replica_scope.library_id())
                .await
                .unwrap()
                .unwrap()
                .generation(),
            Sequence::new(9)
        );
        drop(reopened);
        fs::remove_dir_all(directory).unwrap();
    }
}
