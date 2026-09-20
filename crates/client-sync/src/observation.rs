//! Local filesystem observation is deliberately a control-plane boundary.
//!
//! Platform watchers can lose, duplicate, combine, or reorder events. This
//! module therefore exposes them as bounded hints only. [`OutboundObservationEngine`]
//! re-inspects a managed root and commits typed local intents to SQLite; it
//! never performs an HTTP mutation or upload.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt, fs,
    path::{Component, Path, PathBuf},
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, TryRecvError, sync_channel},
    },
    time::{Duration, Instant},
};

use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{ModifyKind, RenameMode},
};
use sha2::{Digest, Sha256};
use synveil_core::{
    FileVersionId, LibraryId, NodeId, NodeKind, OutboundIntentId, Revision, Sequence, Sha256Digest,
};
use tokio::sync::Mutex as AsyncMutex;

use crate::{
    ClientSyncError, DurableChangeNotification, LocalFingerprint, LocalNode, LocalObjectKind,
    LocalReplica, LocalStateStore, ManagedRelativePath, ReplicaScope, SyncRuntimeWakeReason,
    SyncWakeNotifier,
};

/// Version of the canonical outbound intent semantic-dedupe serialization.
pub const OUTBOUND_INTENT_DEDUPE_VERSION: u8 = 1;

/// Conservative cap for queued raw platform events per managed root.
pub const DEFAULT_RAW_WATCHER_QUEUE_CAPACITY: usize = 1_024;

/// Safe work bounds used by the correctness-first observer implementation.
pub const DEFAULT_MAX_HINTS_PER_POLL: usize = 256;
pub const DEFAULT_SCAN_BATCH_ENTRIES: usize = 128;

/// Typed local intent vocabulary. These names describe observed local facts;
/// they are not server mutation requests.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum OutboundIntentKind {
    CreateDirectory,
    CreateFile,
    RenameNode,
    MoveNode,
    DeleteOrTrashNode,
    ModifyFileContent,
}

impl OutboundIntentKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CreateDirectory => "CREATE_DIRECTORY",
            Self::CreateFile => "CREATE_FILE",
            Self::RenameNode => "RENAME_NODE",
            Self::MoveNode => "MOVE_NODE",
            Self::DeleteOrTrashNode => "DELETE_OR_TRASH_NODE",
            Self::ModifyFileContent => "MODIFY_FILE_CONTENT",
        }
    }
}

impl FromStr for OutboundIntentKind {
    type Err = ClientSyncError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "CREATE_DIRECTORY" => Ok(Self::CreateDirectory),
            "CREATE_FILE" => Ok(Self::CreateFile),
            "RENAME_NODE" => Ok(Self::RenameNode),
            "MOVE_NODE" => Ok(Self::MoveNode),
            "DELETE_OR_TRASH_NODE" => Ok(Self::DeleteOrTrashNode),
            "MODIFY_FILE_CONTENT" => Ok(Self::ModifyFileContent),
            _ => Err(ClientSyncError::InvalidState),
        }
    }
}

/// Closed local lifecycle for observation and controlled outbound submission.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum OutboundIntentState {
    Pending,
    Ready,
    Preparing,
    Uploading,
    Submitting,
    ServerApplied,
    Conflict,
    Blocked,
    Superseded,
    Cancelled,
    NeedsRebaseValidation,
    Reconciled,
}

impl OutboundIntentState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Ready => "READY",
            Self::Preparing => "PREPARING",
            Self::Uploading => "UPLOADING",
            Self::Submitting => "SUBMITTING",
            Self::ServerApplied => "SERVER_APPLIED",
            Self::Conflict => "CONFLICT",
            Self::Blocked => "BLOCKED",
            Self::Superseded => "SUPERSEDED",
            Self::Cancelled => "CANCELLED",
            Self::NeedsRebaseValidation => "NEEDS_REBASE_VALIDATION",
            Self::Reconciled => "RECONCILED",
        }
    }

    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(
            self,
            Self::Pending
                | Self::Ready
                | Self::Preparing
                | Self::Uploading
                | Self::Submitting
                | Self::Blocked
                | Self::NeedsRebaseValidation
        )
    }
}

impl FromStr for OutboundIntentState {
    type Err = ClientSyncError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "PENDING" => Ok(Self::Pending),
            "READY" => Ok(Self::Ready),
            "PREPARING" => Ok(Self::Preparing),
            "UPLOADING" => Ok(Self::Uploading),
            "SUBMITTING" => Ok(Self::Submitting),
            "SERVER_APPLIED" => Ok(Self::ServerApplied),
            "CONFLICT" => Ok(Self::Conflict),
            "BLOCKED" => Ok(Self::Blocked),
            "SUPERSEDED" => Ok(Self::Superseded),
            "CANCELLED" => Ok(Self::Cancelled),
            "NEEDS_REBASE_VALIDATION" => Ok(Self::NeedsRebaseValidation),
            "RECONCILED" => Ok(Self::Reconciled),
            _ => Err(ClientSyncError::InvalidState),
        }
    }
}

/// Crash-safe local control-plane record waiting for a future explicit
/// validation/submission phase.
#[derive(Clone, Eq, PartialEq)]
pub struct OutboundIntent {
    intent_id: OutboundIntentId,
    library_id: LibraryId,
    node_id: Option<NodeId>,
    parent_node_id: Option<NodeId>,
    kind: OutboundIntentKind,
    state: OutboundIntentState,
    observed_relative_path: ManagedRelativePath,
    old_relative_path: Option<ManagedRelativePath>,
    observed_fingerprint: Option<LocalFingerprint>,
    base_epoch: Sequence,
    base_applied_sequence: Sequence,
    base_revision: Option<Revision>,
    base_current_version_id: Option<FileVersionId>,
    base_parent_revision: Option<Revision>,
    dedupe_sha256: Sha256Digest,
}

impl fmt::Debug for OutboundIntent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OutboundIntent")
            .field("intent_id", &self.intent_id)
            .field("library_id", &self.library_id)
            .field("node_id", &self.node_id)
            .field("kind", &self.kind)
            .field("state", &self.state)
            .field("observed_relative_path", &"[REDACTED]")
            .field(
                "old_relative_path",
                &self.old_relative_path.as_ref().map(|_| "[REDACTED]"),
            )
            .finish_non_exhaustive()
    }
}

impl OutboundIntent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        library_id: LibraryId,
        node_id: Option<NodeId>,
        parent_node_id: Option<NodeId>,
        kind: OutboundIntentKind,
        observed_relative_path: ManagedRelativePath,
        old_relative_path: Option<ManagedRelativePath>,
        observed_fingerprint: Option<LocalFingerprint>,
        base_epoch: Sequence,
        base_applied_sequence: Sequence,
        base_revision: Option<Revision>,
        base_current_version_id: Option<FileVersionId>,
        base_parent_revision: Option<Revision>,
    ) -> Result<Self, ClientSyncError> {
        validate_intent_shape(
            node_id,
            kind,
            &observed_relative_path,
            old_relative_path.as_ref(),
            observed_fingerprint,
        )?;
        let dedupe_sha256 = semantic_dedupe_sha256(
            library_id,
            node_id,
            kind,
            &observed_relative_path,
            old_relative_path.as_ref(),
            observed_fingerprint,
            base_epoch,
            base_applied_sequence,
            base_revision,
            base_current_version_id,
            parent_node_id,
            base_parent_revision,
        );
        Ok(Self {
            intent_id: OutboundIntentId::new(),
            library_id,
            node_id,
            parent_node_id,
            kind,
            state: OutboundIntentState::Pending,
            observed_relative_path,
            old_relative_path,
            observed_fingerprint,
            base_epoch,
            base_applied_sequence,
            base_revision,
            base_current_version_id,
            base_parent_revision,
            dedupe_sha256,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rehydrate(
        intent_id: OutboundIntentId,
        library_id: LibraryId,
        node_id: Option<NodeId>,
        parent_node_id: Option<NodeId>,
        kind: OutboundIntentKind,
        state: OutboundIntentState,
        observed_relative_path: ManagedRelativePath,
        old_relative_path: Option<ManagedRelativePath>,
        observed_fingerprint: Option<LocalFingerprint>,
        base_epoch: Sequence,
        base_applied_sequence: Sequence,
        base_revision: Option<Revision>,
        base_current_version_id: Option<FileVersionId>,
        base_parent_revision: Option<Revision>,
        dedupe_sha256: Sha256Digest,
    ) -> Result<Self, ClientSyncError> {
        validate_intent_shape(
            node_id,
            kind,
            &observed_relative_path,
            old_relative_path.as_ref(),
            observed_fingerprint,
        )?;
        let expected = semantic_dedupe_sha256(
            library_id,
            node_id,
            kind,
            &observed_relative_path,
            old_relative_path.as_ref(),
            observed_fingerprint,
            base_epoch,
            base_applied_sequence,
            base_revision,
            base_current_version_id,
            parent_node_id,
            base_parent_revision,
        );
        if expected != dedupe_sha256 {
            return Err(ClientSyncError::InvalidState);
        }
        Ok(Self {
            intent_id,
            library_id,
            node_id,
            parent_node_id,
            kind,
            state,
            observed_relative_path,
            old_relative_path,
            observed_fingerprint,
            base_epoch,
            base_applied_sequence,
            base_revision,
            base_current_version_id,
            base_parent_revision,
            dedupe_sha256,
        })
    }

    #[must_use]
    pub const fn intent_id(&self) -> OutboundIntentId {
        self.intent_id
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
    pub const fn parent_node_id(&self) -> Option<NodeId> {
        self.parent_node_id
    }

    #[must_use]
    pub const fn kind(&self) -> OutboundIntentKind {
        self.kind
    }

    #[must_use]
    pub const fn state(&self) -> OutboundIntentState {
        self.state
    }

    #[must_use]
    pub const fn observed_relative_path(&self) -> &ManagedRelativePath {
        &self.observed_relative_path
    }

    #[must_use]
    pub const fn old_relative_path(&self) -> Option<&ManagedRelativePath> {
        self.old_relative_path.as_ref()
    }

    #[must_use]
    pub const fn observed_fingerprint(&self) -> Option<LocalFingerprint> {
        self.observed_fingerprint
    }

    #[must_use]
    pub const fn base_epoch(&self) -> Sequence {
        self.base_epoch
    }

    #[must_use]
    pub const fn base_applied_sequence(&self) -> Sequence {
        self.base_applied_sequence
    }

    #[must_use]
    pub const fn base_revision(&self) -> Option<Revision> {
        self.base_revision
    }

    #[must_use]
    pub const fn base_current_version_id(&self) -> Option<FileVersionId> {
        self.base_current_version_id
    }

    #[must_use]
    pub const fn base_parent_revision(&self) -> Option<Revision> {
        self.base_parent_revision
    }

    #[must_use]
    pub const fn dedupe_sha256(&self) -> Sha256Digest {
        self.dedupe_sha256
    }
}

fn validate_intent_shape(
    node_id: Option<NodeId>,
    kind: OutboundIntentKind,
    observed_path: &ManagedRelativePath,
    old_path: Option<&ManagedRelativePath>,
    fingerprint: Option<LocalFingerprint>,
) -> Result<(), ClientSyncError> {
    if observed_path.is_root() {
        return Err(ClientSyncError::InvalidRelativePath);
    }
    match kind {
        OutboundIntentKind::CreateDirectory => {
            if node_id.is_some()
                || fingerprint != Some(LocalFingerprint::directory())
                || old_path.is_some()
            {
                return Err(ClientSyncError::InvalidState);
            }
        }
        OutboundIntentKind::CreateFile | OutboundIntentKind::ModifyFileContent => {
            if fingerprint.is_none_or(|value| value.kind() != crate::LocalObjectKind::File)
                || (kind == OutboundIntentKind::CreateFile && node_id.is_some())
                || (kind == OutboundIntentKind::ModifyFileContent && node_id.is_none())
                || old_path.is_some()
            {
                return Err(ClientSyncError::InvalidState);
            }
        }
        OutboundIntentKind::RenameNode | OutboundIntentKind::MoveNode => {
            if node_id.is_none()
                || old_path.is_none()
                || old_path == Some(observed_path)
                || fingerprint.is_none()
            {
                return Err(ClientSyncError::InvalidState);
            }
        }
        OutboundIntentKind::DeleteOrTrashNode => {
            if node_id.is_none() || fingerprint.is_some() || old_path.is_some() {
                return Err(ClientSyncError::InvalidState);
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn semantic_dedupe_sha256(
    library_id: LibraryId,
    node_id: Option<NodeId>,
    kind: OutboundIntentKind,
    observed_path: &ManagedRelativePath,
    old_path: Option<&ManagedRelativePath>,
    fingerprint: Option<LocalFingerprint>,
    base_epoch: Sequence,
    base_sequence: Sequence,
    base_revision: Option<Revision>,
    base_version: Option<FileVersionId>,
    parent_node_id: Option<NodeId>,
    base_parent_revision: Option<Revision>,
) -> Sha256Digest {
    let mut digest = Sha256::new();
    digest.update(b"synveil-outbound-intent-dedupe");
    digest.update([OUTBOUND_INTENT_DEDUPE_VERSION]);
    write_dedupe_field(&mut digest, library_id.to_string().as_bytes());
    write_dedupe_field(
        &mut digest,
        node_id
            .map(|value| value.to_string())
            .as_deref()
            .unwrap_or("")
            .as_bytes(),
    );
    write_dedupe_field(&mut digest, kind.as_str().as_bytes());
    write_dedupe_field(&mut digest, observed_path.as_str().as_bytes());
    write_dedupe_field(
        &mut digest,
        old_path
            .map(ManagedRelativePath::as_str)
            .unwrap_or("")
            .as_bytes(),
    );
    match fingerprint {
        None => write_dedupe_field(&mut digest, b"ABSENT"),
        Some(value) => {
            write_dedupe_field(&mut digest, value.kind().as_str().as_bytes());
            write_dedupe_field(
                &mut digest,
                &value.length().unwrap_or_default().to_be_bytes(),
            );
            if let Some(sha256) = value.sha256() {
                write_dedupe_field(&mut digest, &sha256.into_bytes());
            } else {
                write_dedupe_field(&mut digest, &[]);
            }
        }
    }
    write_dedupe_field(&mut digest, &base_epoch.get().to_be_bytes());
    write_dedupe_field(&mut digest, &base_sequence.get().to_be_bytes());
    write_dedupe_field(
        &mut digest,
        &base_revision
            .map(Revision::get)
            .unwrap_or_default()
            .to_be_bytes(),
    );
    write_dedupe_field(
        &mut digest,
        base_version
            .map(|value| value.to_string())
            .as_deref()
            .unwrap_or("")
            .as_bytes(),
    );
    write_dedupe_field(
        &mut digest,
        parent_node_id
            .map(|value| value.to_string())
            .as_deref()
            .unwrap_or("")
            .as_bytes(),
    );
    write_dedupe_field(
        &mut digest,
        &base_parent_revision
            .map(Revision::get)
            .unwrap_or_default()
            .to_be_bytes(),
    );
    Sha256Digest::from_bytes(digest.finalize().into())
}

fn write_dedupe_field(digest: &mut Sha256, value: &[u8]) {
    digest.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    digest.update(value);
}

/// Durable, typed observation blockers. Their facts are intentionally concise
/// and paths are redacted in debug output.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ObservationIssueKind {
    WatcherOverflow,
    ObservationBusy,
    UnsupportedEntryType,
    UnrepresentableName,
    NameCollision,
    AmbiguousRename,
    RootInvalid,
    BaseStateChanged,
    HashUnstable,
}

impl ObservationIssueKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WatcherOverflow => "WATCHER_OVERFLOW",
            Self::ObservationBusy => "OBSERVATION_BUSY",
            Self::UnsupportedEntryType => "UNSUPPORTED_ENTRY_TYPE",
            Self::UnrepresentableName => "UNREPRESENTABLE_NAME",
            Self::NameCollision => "NAME_COLLISION",
            Self::AmbiguousRename => "AMBIGUOUS_RENAME",
            Self::RootInvalid => "ROOT_INVALID",
            Self::BaseStateChanged => "BASE_STATE_CHANGED",
            Self::HashUnstable => "HASH_UNSTABLE",
        }
    }
}

impl FromStr for ObservationIssueKind {
    type Err = ClientSyncError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "WATCHER_OVERFLOW" => Ok(Self::WatcherOverflow),
            "OBSERVATION_BUSY" => Ok(Self::ObservationBusy),
            "UNSUPPORTED_ENTRY_TYPE" => Ok(Self::UnsupportedEntryType),
            "UNREPRESENTABLE_NAME" => Ok(Self::UnrepresentableName),
            "NAME_COLLISION" => Ok(Self::NameCollision),
            "AMBIGUOUS_RENAME" => Ok(Self::AmbiguousRename),
            "ROOT_INVALID" => Ok(Self::RootInvalid),
            "BASE_STATE_CHANGED" => Ok(Self::BaseStateChanged),
            "HASH_UNSTABLE" => Ok(Self::HashUnstable),
            _ => Err(ClientSyncError::InvalidState),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ObservationIssue {
    issue_id: uuid::Uuid,
    library_id: LibraryId,
    node_id: Option<NodeId>,
    relative_path: Option<ManagedRelativePath>,
    kind: ObservationIssueKind,
}

impl fmt::Debug for ObservationIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ObservationIssue")
            .field("issue_id", &self.issue_id)
            .field("library_id", &self.library_id)
            .field("node_id", &self.node_id)
            .field(
                "relative_path",
                &self.relative_path.as_ref().map(|_| "[REDACTED]"),
            )
            .field("kind", &self.kind)
            .finish()
    }
}

impl ObservationIssue {
    pub(crate) fn new(
        issue_id: uuid::Uuid,
        library_id: LibraryId,
        node_id: Option<NodeId>,
        relative_path: Option<ManagedRelativePath>,
        kind: ObservationIssueKind,
    ) -> Self {
        Self {
            issue_id,
            library_id,
            node_id,
            relative_path,
            kind,
        }
    }

    #[must_use]
    pub const fn issue_id(&self) -> uuid::Uuid {
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
    pub const fn relative_path(&self) -> Option<&ManagedRelativePath> {
        self.relative_path.as_ref()
    }

    #[must_use]
    pub const fn kind(&self) -> ObservationIssueKind {
        self.kind
    }
}

/// Restart-safe reconciliation state for one managed root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationState {
    library_id: LibraryId,
    rescan_required: bool,
    scan_active: bool,
    scan_generation: Sequence,
}

impl ObservationState {
    pub(crate) const fn new(
        library_id: LibraryId,
        rescan_required: bool,
        scan_active: bool,
        scan_generation: Sequence,
    ) -> Self {
        Self {
            library_id,
            rescan_required,
            scan_active,
            scan_generation,
        }
    }

    #[must_use]
    pub const fn library_id(&self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn rescan_required(&self) -> bool {
        self.rescan_required
    }

    #[must_use]
    pub const fn scan_active(&self) -> bool {
        self.scan_active
    }

    #[must_use]
    pub const fn scan_generation(&self) -> Sequence {
        self.scan_generation
    }
}

/// Result of one bounded observer poll together with the optional post-commit
/// runtime wake. The wake is a scheduling hint; `inspected_hints` and the
/// durable result are independent of whether the runtime is still alive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObservationPollResult {
    inspected_hints: usize,
    notification: DurableChangeNotification,
}

impl ObservationPollResult {
    #[must_use]
    pub const fn inspected_hints(self) -> usize {
        self.inspected_hints
    }

    #[must_use]
    pub const fn notification(self) -> DurableChangeNotification {
        self.notification
    }
}

/// Result of one bounded reconciliation unit together with its optional
/// post-commit runtime wake.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationReconciliationResult {
    state: ObservationState,
    notification: DurableChangeNotification,
}

impl ObservationReconciliationResult {
    #[must_use]
    pub const fn state(&self) -> &ObservationState {
        &self.state
    }

    #[must_use]
    pub const fn notification(&self) -> DurableChangeNotification {
        self.notification
    }
}

/// Low-level watcher hint classes. They intentionally have no server semantic
/// interpretation and are always followed by local filesystem reinspection.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum WatchHintKind {
    Create,
    Modify,
    Remove,
    Rename,
    Metadata,
    RescanRequired,
}

impl WatchHintKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "CREATE_HINT",
            Self::Modify => "MODIFY_HINT",
            Self::Remove => "REMOVE_HINT",
            Self::Rename => "RENAME_HINT",
            Self::Metadata => "METADATA_HINT",
            Self::RescanRequired => "RESCAN_REQUIRED",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatchHint {
    kind: WatchHintKind,
    paths: Vec<ManagedRelativePath>,
}

impl WatchHint {
    pub fn new(
        kind: WatchHintKind,
        paths: impl Into<Vec<ManagedRelativePath>>,
    ) -> Result<Self, ClientSyncError> {
        let paths = paths.into();
        match kind {
            WatchHintKind::RescanRequired if !paths.is_empty() => {
                return Err(ClientSyncError::InvalidState);
            }
            WatchHintKind::Rename if paths.len() != 2 => return Err(ClientSyncError::InvalidState),
            WatchHintKind::RescanRequired => {}
            _ if paths.is_empty() || paths.len() > 2 => return Err(ClientSyncError::InvalidState),
            _ => {}
        }
        Ok(Self { kind, paths })
    }

    #[must_use]
    pub fn rescan_required() -> Self {
        Self {
            kind: WatchHintKind::RescanRequired,
            paths: Vec::new(),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> WatchHintKind {
        self.kind
    }

    #[must_use]
    pub fn paths(&self) -> &[ManagedRelativePath] {
        &self.paths
    }
}

/// Narrow, transport-neutral filesystem watcher boundary. Implementations only
/// emit bounded hints; callers must derive durable truth through reinspection.
pub trait LocalChangeWatcher: Send {
    fn start(&mut self, managed_root: &Path) -> Result<(), ClientSyncError>;
    fn poll(&mut self, maximum: usize) -> Result<Vec<WatchHint>, ClientSyncError>;
    fn stop(&mut self) -> Result<(), ClientSyncError>;
}

/// A deterministic in-memory watcher used by observer tests. It never derives
/// logical intent semantics and becomes `RESCAN_REQUIRED` if its own bounded
/// queue overflows.
pub struct ManualChangeWatcher {
    queue: Arc<Mutex<ManualWatcherQueue>>,
}

#[derive(Clone)]
pub struct ManualChangeSource {
    queue: Arc<Mutex<ManualWatcherQueue>>,
}

struct ManualWatcherQueue {
    capacity: usize,
    started: bool,
    hints: VecDeque<WatchHint>,
}

impl ManualChangeWatcher {
    #[must_use]
    pub fn pair() -> (Self, ManualChangeSource) {
        Self::with_capacity(DEFAULT_RAW_WATCHER_QUEUE_CAPACITY)
    }

    #[must_use]
    pub fn with_capacity(capacity: usize) -> (Self, ManualChangeSource) {
        let queue = Arc::new(Mutex::new(ManualWatcherQueue {
            capacity: capacity.max(1),
            started: false,
            hints: VecDeque::new(),
        }));
        (
            Self {
                queue: queue.clone(),
            },
            ManualChangeSource { queue },
        )
    }
}

impl ManualChangeSource {
    pub fn push(&self, hint: WatchHint) -> Result<(), ClientSyncError> {
        let mut queue = self
            .queue
            .lock()
            .map_err(|_| ClientSyncError::InvalidState)?;
        if queue.hints.len() >= queue.capacity {
            queue.hints.clear();
            queue.hints.push_back(WatchHint::rescan_required());
            return Ok(());
        }
        queue.hints.push_back(hint);
        Ok(())
    }

    pub fn overflow(&self) -> Result<(), ClientSyncError> {
        let mut queue = self
            .queue
            .lock()
            .map_err(|_| ClientSyncError::InvalidState)?;
        queue.hints.clear();
        queue.hints.push_back(WatchHint::rescan_required());
        Ok(())
    }
}

impl LocalChangeWatcher for ManualChangeWatcher {
    fn start(&mut self, managed_root: &Path) -> Result<(), ClientSyncError> {
        if !managed_root.is_absolute() || !managed_root.is_dir() {
            return Err(ClientSyncError::InvalidRoot);
        }
        self.queue
            .lock()
            .map_err(|_| ClientSyncError::InvalidState)?
            .started = true;
        Ok(())
    }

    fn poll(&mut self, maximum: usize) -> Result<Vec<WatchHint>, ClientSyncError> {
        if maximum == 0 {
            return Err(ClientSyncError::ResourceLimit);
        }
        let mut queue = self
            .queue
            .lock()
            .map_err(|_| ClientSyncError::InvalidState)?;
        if !queue.started {
            return Err(ClientSyncError::InvalidState);
        }
        Ok((0..maximum)
            .filter_map(|_| queue.hints.pop_front())
            .collect())
    }

    fn stop(&mut self) -> Result<(), ClientSyncError> {
        self.queue
            .lock()
            .map_err(|_| ClientSyncError::InvalidState)?
            .started = false;
        Ok(())
    }
}

/// Native Linux/Windows watcher backed by `notify` 8.2.0. On Linux the
/// recommended watcher selects inotify; on Windows it selects
/// ReadDirectoryChangesW. Both implementations feed one bounded channel.
pub struct NotifyLocalChangeWatcher {
    capacity: usize,
    root: Option<PathBuf>,
    receiver: Option<Receiver<notify::Result<Event>>>,
    watcher: Option<RecommendedWatcher>,
    overflowed: Arc<AtomicBool>,
}

impl fmt::Debug for NotifyLocalChangeWatcher {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NotifyLocalChangeWatcher")
            .field("capacity", &self.capacity)
            .field("started", &self.watcher.is_some())
            .finish_non_exhaustive()
    }
}

impl NotifyLocalChangeWatcher {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            root: None,
            receiver: None,
            watcher: None,
            overflowed: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Default for NotifyLocalChangeWatcher {
    fn default() -> Self {
        Self::new(DEFAULT_RAW_WATCHER_QUEUE_CAPACITY)
    }
}

impl LocalChangeWatcher for NotifyLocalChangeWatcher {
    fn start(&mut self, managed_root: &Path) -> Result<(), ClientSyncError> {
        if self.watcher.is_some() || !managed_root.is_absolute() || !managed_root.is_dir() {
            return Err(ClientSyncError::InvalidState);
        }
        let (sender, receiver) = sync_channel(self.capacity);
        let overflowed = self.overflowed.clone();
        let mut watcher = notify::recommended_watcher(move |event| {
            if sender.try_send(event).is_err() {
                overflowed.store(true, Ordering::Release);
            }
        })
        .map_err(|_| ClientSyncError::LocalIo)?;
        watcher
            .watch(managed_root, RecursiveMode::Recursive)
            .map_err(|_| ClientSyncError::LocalIo)?;
        self.root = Some(managed_root.to_path_buf());
        self.receiver = Some(receiver);
        self.watcher = Some(watcher);
        Ok(())
    }

    fn poll(&mut self, maximum: usize) -> Result<Vec<WatchHint>, ClientSyncError> {
        if maximum == 0 {
            return Err(ClientSyncError::ResourceLimit);
        }
        let root = self.root.as_deref().ok_or(ClientSyncError::InvalidState)?;
        let receiver = self
            .receiver
            .as_ref()
            .ok_or(ClientSyncError::InvalidState)?;
        let mut hints = Vec::new();
        if self.overflowed.swap(false, Ordering::AcqRel) {
            hints.push(WatchHint::rescan_required());
        }
        while hints.len() < maximum {
            match receiver.try_recv() {
                Ok(Ok(event)) => match notify_event_to_hint(root, event) {
                    Ok(Some(hint)) => hints.push(hint),
                    Ok(None) => {}
                    Err(_) => hints.push(WatchHint::rescan_required()),
                },
                Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                    hints.push(WatchHint::rescan_required());
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        Ok(hints)
    }

    fn stop(&mut self) -> Result<(), ClientSyncError> {
        self.watcher.take();
        self.receiver.take();
        self.root.take();
        self.overflowed.store(false, Ordering::Release);
        Ok(())
    }
}

fn notify_event_to_hint(root: &Path, event: Event) -> Result<Option<WatchHint>, ClientSyncError> {
    let mut paths = Vec::with_capacity(event.paths.len());
    for native_path in event.paths {
        let Some(relative) = native_to_managed_relative(root, &native_path)? else {
            continue;
        };
        if is_control_path(&relative) {
            continue;
        }
        paths.push(relative);
    }
    if paths.is_empty() {
        return Ok(None);
    }
    let kind = match event.kind {
        EventKind::Create(_) => WatchHintKind::Create,
        EventKind::Remove(_) => WatchHintKind::Remove,
        EventKind::Modify(ModifyKind::Metadata(_)) => WatchHintKind::Metadata,
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if paths.len() == 2 => {
            WatchHintKind::Rename
        }
        // inotify can coalesce a completed move into one destination path.
        // Reinspect that path instead of manufacturing a durable overflow
        // blocker; a two-path event remains the only form interpreted as a
        // rename.
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if paths.len() == 1 => {
            WatchHintKind::Modify
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => WatchHintKind::Remove,
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => WatchHintKind::Create,
        EventKind::Modify(ModifyKind::Name(_)) if paths.len() == 1 => WatchHintKind::Modify,
        EventKind::Modify(ModifyKind::Data(_)) => WatchHintKind::Modify,
        EventKind::Access(_) => WatchHintKind::Metadata,
        _ => return Ok(Some(WatchHint::rescan_required())),
    };
    WatchHint::new(kind, paths).map(Some)
}

fn native_to_managed_relative(
    root: &Path,
    native_path: &Path,
) -> Result<Option<ManagedRelativePath>, ClientSyncError> {
    let relative = native_path
        .strip_prefix(root)
        .map_err(|_| ClientSyncError::InvalidRelativePath)?;
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
        && !relative.as_os_str().is_empty()
    {
        return Err(ClientSyncError::InvalidRelativePath);
    }
    if relative.as_os_str().is_empty() {
        return Ok(Some(ManagedRelativePath::root()));
    }
    let mut components = Vec::new();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(ClientSyncError::InvalidRelativePath);
        };
        components.push(
            component
                .to_str()
                .ok_or(ClientSyncError::InvalidRelativePath)?,
        );
    }
    // The control tree is deliberately not representable as an arbitrary
    // managed path. Native watchers still report its directory and file
    // events, so filter it only after the root-relative components have been
    // fully validated and before applying the managed-path policy.
    if components.first() == Some(&".synveil") {
        return Ok(None);
    }
    ManagedRelativePath::new(components.join("/")).map(Some)
}

pub(crate) fn is_control_path(path: &ManagedRelativePath) -> bool {
    path.as_str() == ".synveil" || path.as_str().starts_with(".synveil/")
}

/// Runtime bounds for a single managed-root observer. Debounce only reduces
/// transient editor noise; crash safety always comes from reconciliation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObservationConfig {
    raw_queue_capacity: usize,
    max_hints_per_poll: usize,
    scan_batch_entries: usize,
    debounce_window: Duration,
}

impl ObservationConfig {
    pub fn new(
        raw_queue_capacity: usize,
        max_hints_per_poll: usize,
        scan_batch_entries: usize,
        debounce_window: Duration,
    ) -> Result<Self, ClientSyncError> {
        if raw_queue_capacity == 0
            || max_hints_per_poll == 0
            || scan_batch_entries == 0
            || raw_queue_capacity > 8_192
            || max_hints_per_poll > 4_096
            || scan_batch_entries > 1_024
            || debounce_window > Duration::from_secs(5)
        {
            return Err(ClientSyncError::ResourceLimit);
        }
        Ok(Self {
            raw_queue_capacity,
            max_hints_per_poll,
            scan_batch_entries,
            debounce_window,
        })
    }

    #[must_use]
    pub const fn raw_queue_capacity(self) -> usize {
        self.raw_queue_capacity
    }

    #[must_use]
    pub const fn max_hints_per_poll(self) -> usize {
        self.max_hints_per_poll
    }

    #[must_use]
    pub const fn scan_batch_entries(self) -> usize {
        self.scan_batch_entries
    }

    #[must_use]
    pub const fn debounce_window(self) -> Duration {
        self.debounce_window
    }
}

impl Default for ObservationConfig {
    fn default() -> Self {
        Self {
            raw_queue_capacity: DEFAULT_RAW_WATCHER_QUEUE_CAPACITY,
            max_hints_per_poll: DEFAULT_MAX_HINTS_PER_POLL,
            scan_batch_entries: DEFAULT_SCAN_BATCH_ENTRIES,
            debounce_window: Duration::from_millis(200),
        }
    }
}

/// Durable observer for exactly one managed library root. It holds no remote
/// transport and therefore has no code path that can submit a server mutation.
pub struct OutboundObservationEngine {
    scope: ReplicaScope,
    replica: Arc<dyn LocalReplica>,
    state: Arc<LocalStateStore>,
    watcher: AsyncMutex<Box<dyn LocalChangeWatcher>>,
    config: ObservationConfig,
    buffered_hints: AsyncMutex<VecDeque<BufferedHint>>,
    wake_notifier: Option<Arc<dyn SyncWakeNotifier>>,
    durable_change_pending: AtomicBool,
    started: AtomicBool,
}

struct BufferedHint {
    hint: WatchHint,
    observed_at: Instant,
}

impl fmt::Debug for OutboundObservationEngine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OutboundObservationEngine")
            .field("scope", &self.scope)
            .field("started", &self.started.load(Ordering::Acquire))
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl OutboundObservationEngine {
    pub async fn new(
        scope: ReplicaScope,
        replica: Arc<dyn LocalReplica>,
        state: Arc<LocalStateStore>,
        watcher: Box<dyn LocalChangeWatcher>,
        config: ObservationConfig,
    ) -> Result<Self, ClientSyncError> {
        Self::new_with_optional_wake_notifier(scope, replica, state, watcher, config, None).await
    }

    /// Construct an observer with the shared runtime's post-commit wake
    /// boundary. The notifier is optional so existing embedders can retain
    /// startup/periodic-only operation while migrating to Prompt 93.
    pub async fn new_with_wake_notifier(
        scope: ReplicaScope,
        replica: Arc<dyn LocalReplica>,
        state: Arc<LocalStateStore>,
        watcher: Box<dyn LocalChangeWatcher>,
        config: ObservationConfig,
        wake_notifier: Arc<dyn SyncWakeNotifier>,
    ) -> Result<Self, ClientSyncError> {
        Self::new_with_optional_wake_notifier(
            scope,
            replica,
            state,
            watcher,
            config,
            Some(wake_notifier),
        )
        .await
    }

    async fn new_with_optional_wake_notifier(
        scope: ReplicaScope,
        replica: Arc<dyn LocalReplica>,
        state: Arc<LocalStateStore>,
        watcher: Box<dyn LocalChangeWatcher>,
        config: ObservationConfig,
        wake_notifier: Option<Arc<dyn SyncWakeNotifier>>,
    ) -> Result<Self, ClientSyncError> {
        if replica.scope() != scope {
            return Err(ClientSyncError::WrongScope);
        }
        let record = state
            .replica(scope.library_id())
            .await?
            .ok_or(ClientSyncError::InvalidState)?;
        if record.scope() != scope || record.root_binding_id() != replica.binding_id() {
            return Err(ClientSyncError::WrongRootBinding);
        }
        if record.server_profile_id() != replica.server_profile_id() {
            return Err(ClientSyncError::WrongServerProfile);
        }
        Ok(Self {
            scope,
            replica,
            state,
            watcher: AsyncMutex::new(watcher),
            config,
            buffered_hints: AsyncMutex::new(VecDeque::new()),
            wake_notifier,
            durable_change_pending: AtomicBool::new(false),
            started: AtomicBool::new(false),
        })
    }

    /// Construct the production Linux/Windows watcher. `notify` selects
    /// inotify on Linux and ReadDirectoryChangesW on Windows at compile time.
    pub async fn new_native(
        scope: ReplicaScope,
        replica: Arc<dyn LocalReplica>,
        state: Arc<LocalStateStore>,
        config: ObservationConfig,
    ) -> Result<Self, ClientSyncError> {
        Self::new_with_optional_wake_notifier(
            scope,
            replica,
            state,
            Box::new(NotifyLocalChangeWatcher::new(config.raw_queue_capacity())),
            config,
            None,
        )
        .await
    }

    /// Construct the native bounded watcher and connect its durable
    /// reconciliation result to a runtime wake notifier.
    pub async fn new_native_with_wake_notifier(
        scope: ReplicaScope,
        replica: Arc<dyn LocalReplica>,
        state: Arc<LocalStateStore>,
        config: ObservationConfig,
        wake_notifier: Arc<dyn SyncWakeNotifier>,
    ) -> Result<Self, ClientSyncError> {
        Self::new_with_optional_wake_notifier(
            scope,
            replica,
            state,
            Box::new(NotifyLocalChangeWatcher::new(config.raw_queue_capacity())),
            config,
            Some(wake_notifier),
        )
        .await
    }

    #[must_use]
    pub const fn scope(&self) -> ReplicaScope {
        self.scope
    }

    /// Identity of the runtime targeted by this observer's post-commit
    /// notifier, when one was attached by the application composition root.
    #[must_use]
    pub fn runtime_identity(&self) -> Option<crate::SyncRuntimeIdentity> {
        self.wake_notifier
            .as_ref()
            .and_then(|notifier| notifier.runtime_identity())
    }

    #[must_use]
    pub const fn config(&self) -> ObservationConfig {
        self.config
    }

    /// Validate the root, ensure any interrupted Prompt 36 operation has been
    /// recovered by its owner, start observation, then begin the durable scan
    /// that closes the watcher startup gap.
    pub async fn start(&self) -> Result<ObservationState, ClientSyncError> {
        self.start_with_notification()
            .await
            .map(|result| result.state)
    }

    /// Start observation and expose the durable-change/wake result from the
    /// first bounded reconciliation unit.
    pub async fn start_with_notification(
        &self,
    ) -> Result<ObservationReconciliationResult, ClientSyncError> {
        let _writer = self
            .state
            .lock_replica_writer(self.scope.library_id())
            .await;
        self.validate_root_or_stop().await?;
        if !self
            .state
            .unfinished_operations(self.scope.library_id())
            .await?
            .is_empty()
        {
            self.state
                .persist_observation_issue(
                    self.scope.library_id(),
                    None,
                    None,
                    ObservationIssueKind::ObservationBusy,
                )
                .await?;
            self.state
                .require_reconciliation(self.scope.library_id())
                .await?;
            return Err(ClientSyncError::ObservationIssue(
                ObservationIssueKind::ObservationBusy,
            ));
        }
        if !self.started.swap(true, Ordering::AcqRel) {
            let mut watcher = self.watcher.lock().await;
            if let Err(error) = watcher.start(self.replica.root_path()) {
                self.started.store(false, Ordering::Release);
                return Err(error);
            }
        }
        self.state
            .require_reconciliation(self.scope.library_id())
            .await?;
        drop(_writer);
        self.reconcile_once_with_notification().await
    }

    /// Poll the bounded raw queue, coalesce due hints, then advance one
    /// reconciliation batch if watcher completeness was lost. Return the count
    /// of hints that were actually re-inspected, not raw OS events received.
    pub async fn poll_once(&self) -> Result<usize, ClientSyncError> {
        self.poll_once_with_notification()
            .await
            .map(|result| result.inspected_hints)
    }

    /// Poll once and return both the bounded inspection count and the
    /// post-commit runtime wake status. A stopped runtime is reported in the
    /// notification while the durable observer operation remains successful.
    pub async fn poll_once_with_notification(
        &self,
    ) -> Result<ObservationPollResult, ClientSyncError> {
        let result = self.poll_once_inner().await;
        let notification = match &result {
            Err(_) => self.notify_pending_durable_change(),
            Ok((_, state)) => self.notify_if_reconciliation_complete(state),
        };
        result.map(|(inspected_hints, _)| ObservationPollResult {
            inspected_hints,
            notification,
        })
    }

    async fn poll_once_inner(&self) -> Result<(usize, ObservationState), ClientSyncError> {
        if !self.started.load(Ordering::Acquire) {
            return Err(ClientSyncError::InvalidState);
        }
        // Fence the watcher before draining any hint. A root can disappear
        // between the previous probe and this poll; no hint from that window
        // may be interpreted as a user deletion or other durable mutation.
        self.validate_root_or_stop().await?;
        let hints = {
            let mut watcher = self.watcher.lock().await;
            watcher.poll(self.config.max_hints_per_poll())?
        };
        let mut overflowed = false;
        {
            let mut buffered = self.buffered_hints.lock().await;
            for hint in hints {
                if hint.kind() == WatchHintKind::RescanRequired {
                    overflowed = true;
                    continue;
                }
                if buffered.len() >= self.config.raw_queue_capacity() {
                    buffered.clear();
                    overflowed = true;
                    continue;
                }
                buffered.push_back(BufferedHint {
                    hint,
                    observed_at: Instant::now(),
                });
            }
        }
        if overflowed {
            self.record_watcher_overflow().await?;
        }
        let inspected = self.flush_due_hints(false).await?;
        if self
            .state
            .observation_state(self.scope.library_id())
            .await?
            .is_some_and(|state| state.rescan_required() || state.scan_active())
        {
            let _ = self.reconcile_once_inner().await?;
        }
        let state = self
            .state
            .observation_state(self.scope.library_id())
            .await?
            .ok_or(ClientSyncError::InvalidState)?;
        Ok((inspected, state))
    }

    /// Deterministically flush all currently buffered hints. This is useful in
    /// tests and graceful shutdown; correctness still comes from the scan.
    pub async fn flush(&self) -> Result<usize, ClientSyncError> {
        self.flush_with_notification()
            .await
            .map(|result| result.inspected_hints)
    }

    /// Flush all currently buffered hints and return the durable-change/wake
    /// result. The notifier is called only after the observer writer guard has
    /// been released.
    pub async fn flush_with_notification(&self) -> Result<ObservationPollResult, ClientSyncError> {
        let result = self.flush_due_hints(true).await;
        let notification = self.notify_pending_durable_change();
        result.map(|inspected_hints| ObservationPollResult {
            inspected_hints,
            notification,
        })
    }

    /// Advance no more than one bounded reconciliation unit. Repeated calls
    /// converge after an overflow, a restart, or changes made while offline.
    pub async fn reconcile_once(&self) -> Result<ObservationState, ClientSyncError> {
        self.reconcile_once_with_notification()
            .await
            .map(|result| result.state)
    }

    /// Advance one bounded reconciliation unit and expose the optional
    /// post-commit wake result.
    pub async fn reconcile_once_with_notification(
        &self,
    ) -> Result<ObservationReconciliationResult, ClientSyncError> {
        let result = self.reconcile_once_inner().await;
        let notification = match &result {
            Err(_) => self.notify_pending_durable_change(),
            Ok(state) => self.notify_if_reconciliation_complete(state),
        };
        result.map(|state| ObservationReconciliationResult {
            state,
            notification,
        })
    }

    async fn reconcile_once_inner(&self) -> Result<ObservationState, ClientSyncError> {
        let _writer = self
            .state
            .lock_replica_writer(self.scope.library_id())
            .await;
        self.validate_root_or_stop().await?;
        if !self
            .state
            .unfinished_operations(self.scope.library_id())
            .await?
            .is_empty()
        {
            self.state
                .persist_observation_issue(
                    self.scope.library_id(),
                    None,
                    None,
                    ObservationIssueKind::ObservationBusy,
                )
                .await?;
            self.state
                .require_reconciliation(self.scope.library_id())
                .await?;
            return Err(ClientSyncError::ObservationIssue(
                ObservationIssueKind::ObservationBusy,
            ));
        }
        let state = self
            .state
            .begin_reconciliation(self.scope.library_id())
            .await?;
        if !state.scan_active() {
            return Ok(state);
        }
        if let Some(work) = self
            .state
            .next_observation_scan_work(self.scope.library_id(), state.scan_generation())
            .await?
        {
            self.reconcile_directory_work(state.scan_generation(), &work)
                .await?;
        } else {
            let unseen = self
                .state
                .unseen_local_nodes(
                    self.scope.library_id(),
                    state.scan_generation(),
                    self.config.scan_batch_entries(),
                )
                .await?;
            if !unseen.is_empty() {
                for node in unseen {
                    let observed = self
                        .state
                        .observed_node(self.scope.library_id(), node.node_id())
                        .await?
                        .ok_or(ClientSyncError::InvalidState)?;
                    self.classify_path(&observed.relative_path).await?;
                    self.state
                        .mark_scan_node_seen(
                            self.scope.library_id(),
                            state.scan_generation(),
                            node.node_id(),
                        )
                        .await?;
                }
            } else {
                let creates = self
                    .state
                    .unseen_active_create_intents(
                        self.scope.library_id(),
                        state.scan_generation(),
                        self.config.scan_batch_entries(),
                    )
                    .await?;
                if creates.is_empty() {
                    self.state
                        .finish_reconciliation(self.scope.library_id(), state.scan_generation())
                        .await?;
                    self.state
                        .retire_observation_suppressions(self.scope.library_id())
                        .await?;
                } else {
                    for intent in creates {
                        self.classify_path(intent.observed_relative_path()).await?;
                        self.state
                            .mark_scan_intent_seen(
                                self.scope.library_id(),
                                state.scan_generation(),
                                intent.intent_id(),
                            )
                            .await?;
                    }
                }
            }
        }
        self.state
            .observation_state(self.scope.library_id())
            .await?
            .ok_or(ClientSyncError::InvalidState)
    }

    /// Stop the watcher and conservatively mark a rescan requirement. Any raw
    /// event not drained before shutdown is recovered by next startup scan.
    pub async fn shutdown(&self) -> Result<(), ClientSyncError> {
        if self.started.load(Ordering::Acquire) {
            let flush_result = self.flush_due_hints(true).await;
            // Even if a later reconciliation step fails, an earlier intent
            // commit must still receive its best-effort wake before return.
            let _ = self.notify_pending_durable_change();
            if flush_result.is_err() {
                self.state
                    .require_reconciliation(self.scope.library_id())
                    .await?;
            }
            self.state
                .require_reconciliation(self.scope.library_id())
                .await?;
            let mut watcher = self.watcher.lock().await;
            watcher.stop()?;
            self.started.store(false, Ordering::Release);
        }
        Ok(())
    }

    pub async fn list_pending_intents(&self) -> Result<Vec<OutboundIntent>, ClientSyncError> {
        self.state
            .list_pending_intents(self.scope.library_id())
            .await
    }

    pub async fn get_intent(
        &self,
        intent_id: OutboundIntentId,
    ) -> Result<Option<OutboundIntent>, ClientSyncError> {
        self.state.outbound_intent(intent_id).await
    }

    pub async fn count_pending_intents(&self) -> Result<u64, ClientSyncError> {
        self.state
            .count_pending_intents(self.scope.library_id())
            .await
    }

    fn note_durable_change(&self) {
        self.durable_change_pending.store(true, Ordering::Release);
    }

    fn notify_pending_durable_change(&self) -> DurableChangeNotification {
        if !self.durable_change_pending.swap(false, Ordering::AcqRel) {
            return DurableChangeNotification::no_change();
        }
        let wake_result = self.wake_notifier.as_ref().map(|notifier| {
            notifier.wake_library(self.scope.library_id(), SyncRuntimeWakeReason::LocalChange)
        });
        DurableChangeNotification::committed(wake_result)
    }

    fn notify_if_reconciliation_complete(
        &self,
        state: &ObservationState,
    ) -> DurableChangeNotification {
        if state.scan_active() || state.rescan_required() {
            return self.pending_durable_change_notification();
        }
        self.notify_pending_durable_change()
    }

    fn pending_durable_change_notification(&self) -> DurableChangeNotification {
        if self.durable_change_pending.load(Ordering::Acquire) {
            DurableChangeNotification::committed(None)
        } else {
            DurableChangeNotification::no_change()
        }
    }

    async fn record_watcher_overflow(&self) -> Result<(), ClientSyncError> {
        self.state
            .persist_observation_issue(
                self.scope.library_id(),
                None,
                None,
                ObservationIssueKind::WatcherOverflow,
            )
            .await?;
        self.state
            .require_reconciliation(self.scope.library_id())
            .await?;
        Ok(())
    }

    async fn flush_due_hints(&self, force: bool) -> Result<usize, ClientSyncError> {
        let now = Instant::now();
        let due = {
            let mut buffered = self.buffered_hints.lock().await;
            let mut due = Vec::new();
            let mut retained = VecDeque::new();
            while let Some(buffered_hint) = buffered.pop_front() {
                if force
                    || now.duration_since(buffered_hint.observed_at)
                        >= self.config.debounce_window()
                {
                    due.push(buffered_hint.hint);
                } else {
                    retained.push_back(buffered_hint);
                }
            }
            *buffered = retained;
            due
        };
        if due.is_empty() {
            return Ok(0);
        }
        let _writer = self
            .state
            .lock_replica_writer(self.scope.library_id())
            .await;
        self.validate_root_or_stop().await?;
        let ambiguous_paths = self.detect_unpaired_rename(&due).await?;
        let (renames, mut paths) = coalesce_hints(due);
        for ambiguous_path in ambiguous_paths {
            paths.remove(&ambiguous_path);
        }
        for (source, destination) in renames {
            self.classify_rename(&source, &destination).await?;
        }
        let count = paths.len();
        for path in paths {
            self.classify_path(&path).await?;
        }
        Ok(count)
    }

    async fn validate_root_or_stop(&self) -> Result<(), ClientSyncError> {
        if self.replica.validate_root().is_ok() {
            self.state
                .resolve_recovered_root_observation_issues(self.scope.library_id())
                .await?;
            return Ok(());
        }
        self.state
            .persist_observation_issue(
                self.scope.library_id(),
                None,
                None,
                ObservationIssueKind::RootInvalid,
            )
            .await?;
        self.started.store(false, Ordering::Release);
        let mut watcher = self.watcher.lock().await;
        let _ = watcher.stop();
        Err(ClientSyncError::RootUnavailable)
    }

    async fn detect_unpaired_rename(
        &self,
        hints: &[WatchHint],
    ) -> Result<BTreeSet<ManagedRelativePath>, ClientSyncError> {
        let (_, paths) = coalesce_hints(hints.to_vec());
        let mut removed = Vec::new();
        let mut created = Vec::new();
        for path in paths {
            let known = self
                .state
                .observed_node_at_path(self.scope.library_id(), &path)
                .await?;
            match self.replica.inspect(&path) {
                Ok(None) if known.as_ref().is_some_and(|value| value.present) => {
                    removed.push((path, known))
                }
                Ok(Some(fingerprint)) if known.is_none() => created.push((path, fingerprint)),
                Ok(_) => {}
                Err(_) => {}
            }
        }
        let mut ambiguous = BTreeSet::new();
        for (source, source_node) in removed {
            let ambiguous_destinations = created
                .iter()
                .filter(|(_, fingerprint)| {
                    source_node
                        .as_ref()
                        .and_then(|value| value.fingerprint)
                        .is_some_and(|expected| expected.kind() == fingerprint.kind())
                })
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>();
            if !ambiguous_destinations.is_empty() {
                self.state
                    .persist_observation_issue(
                        self.scope.library_id(),
                        source_node.as_ref().map(|value| value.node.node_id()),
                        Some(&source),
                        ObservationIssueKind::AmbiguousRename,
                    )
                    .await?;
                for destination in &ambiguous_destinations {
                    self.state
                        .persist_observation_issue(
                            self.scope.library_id(),
                            None,
                            Some(destination),
                            ObservationIssueKind::AmbiguousRename,
                        )
                        .await?;
                }
                self.state
                    .require_reconciliation(self.scope.library_id())
                    .await?;
                ambiguous.insert(source);
                ambiguous.extend(ambiguous_destinations);
            }
        }
        Ok(ambiguous)
    }

    async fn classify_rename(
        &self,
        source: &ManagedRelativePath,
        destination: &ManagedRelativePath,
    ) -> Result<(), ClientSyncError> {
        if is_control_path(source) || is_control_path(destination) {
            return Ok(());
        }
        let source_actual = self.inspect_for_observation(source).await?;
        let destination_actual = self.inspect_for_observation(destination).await?;
        if self.suppression_matches(source, source_actual).await?
            && self
                .suppression_matches(destination, destination_actual)
                .await?
        {
            return Ok(());
        }
        let source_node = self
            .state
            .observed_node_at_path(self.scope.library_id(), source)
            .await?;
        let Some(source_node) = source_node.filter(|value| value.present) else {
            if source_actual.is_none()
                && let Some(destination_fingerprint) = destination_actual
                && self
                    .move_pending_create(source, destination, destination_fingerprint)
                    .await?
            {
                return Ok(());
            }
            return self.ambiguous_rename(source, None).await;
        };
        let Some(destination_fingerprint) = destination_actual else {
            return self
                .ambiguous_rename(source, Some(source_node.node.node_id()))
                .await;
        };
        if source_actual.is_some()
            || !node_kind_matches_fingerprint(source_node.node.kind(), destination_fingerprint)
        {
            return self
                .ambiguous_rename(source, Some(source_node.node.node_id()))
                .await;
        }
        if self
            .state
            .observed_node_at_path(self.scope.library_id(), destination)
            .await?
            .is_some_and(|value| value.node.node_id() != source_node.node.node_id())
            || self.replica.has_portable_name_collision(destination)?
        {
            self.state
                .persist_observation_issue(
                    self.scope.library_id(),
                    Some(source_node.node.node_id()),
                    Some(destination),
                    ObservationIssueKind::NameCollision,
                )
                .await?;
            self.state
                .require_reconciliation(self.scope.library_id())
                .await?;
            return Ok(());
        }
        let parent = self.required_parent(destination).await?;
        let source_parent = source
            .parent()
            .ok_or(ClientSyncError::InvalidRelativePath)?;
        let destination_parent = destination
            .parent()
            .ok_or(ClientSyncError::InvalidRelativePath)?;
        let kind = if source_parent == destination_parent {
            OutboundIntentKind::RenameNode
        } else {
            OutboundIntentKind::MoveNode
        };
        let intent = self
            .new_intent(
                Some(&source_node),
                Some(parent),
                kind,
                destination.clone(),
                Some(source.clone()),
                Some(destination_fingerprint),
            )
            .await?;
        let persisted = self
            .state
            .upsert_outbound_intent_with_result(&intent)
            .await?;
        if persisted.changed() {
            self.note_durable_change();
        }
        self.state
            .persist_observed_node(
                &source_node.node,
                destination,
                true,
                Some(destination_fingerprint),
            )
            .await?;
        if source_node.node.kind() == NodeKind::File
            && source_node.fingerprint != Some(destination_fingerprint)
        {
            self.record_file_modification(&source_node, destination, destination_fingerprint)
                .await?;
        }
        Ok(())
    }

    async fn ambiguous_rename(
        &self,
        path: &ManagedRelativePath,
        node_id: Option<NodeId>,
    ) -> Result<(), ClientSyncError> {
        self.state
            .persist_observation_issue(
                self.scope.library_id(),
                node_id,
                Some(path),
                ObservationIssueKind::AmbiguousRename,
            )
            .await?;
        self.state
            .require_reconciliation(self.scope.library_id())
            .await?;
        Ok(())
    }

    async fn classify_path(&self, path: &ManagedRelativePath) -> Result<(), ClientSyncError> {
        if is_control_path(path) {
            return Ok(());
        }
        if self
            .state
            .has_unresolved_observation_issue(
                self.scope.library_id(),
                path,
                ObservationIssueKind::AmbiguousRename,
            )
            .await?
        {
            return Ok(());
        }
        let actual = self.inspect_for_observation(path).await?;
        if self.suppression_matches(path, actual).await? {
            return Ok(());
        }
        let known = self
            .state
            .observed_node_at_path(self.scope.library_id(), path)
            .await?;
        match (known, actual) {
            (Some(known), Some(fingerprint)) => {
                self.classify_known_present(&known, path, fingerprint).await
            }
            (Some(known), None) if known.present => {
                let intent = self
                    .new_intent(
                        Some(&known),
                        None,
                        OutboundIntentKind::DeleteOrTrashNode,
                        path.clone(),
                        None,
                        None,
                    )
                    .await?;
                let persisted = self
                    .state
                    .upsert_outbound_intent_with_result(&intent)
                    .await?;
                if persisted.changed() {
                    self.note_durable_change();
                }
                self.state
                    .persist_observed_node(&known.node, path, false, None)
                    .await
            }
            (Some(_), None) => Ok(()),
            (None, Some(fingerprint)) => self.classify_unknown_present(path, fingerprint).await,
            (None, None) => {
                let _ = self
                    .state
                    .cancel_pending_create(self.scope.library_id(), path)
                    .await?;
                Ok(())
            }
        }
    }

    async fn classify_known_present(
        &self,
        known: &crate::state::ObservedLocalNode,
        path: &ManagedRelativePath,
        fingerprint: LocalFingerprint,
    ) -> Result<(), ClientSyncError> {
        if known.present && known.fingerprint == Some(fingerprint) {
            let _ = self
                .state
                .cancel_pending_delete(self.scope.library_id(), known.node.node_id())
                .await?;
            return Ok(());
        }
        if !known.present
            && node_kind_matches_fingerprint(known.node.kind(), fingerprint)
            && expected_node_fingerprint(&known.node) == Some(fingerprint)
            && self
                .state
                .cancel_pending_delete(self.scope.library_id(), known.node.node_id())
                .await?
        {
            self.state
                .persist_observed_node(&known.node, path, true, Some(fingerprint))
                .await?;
            return Ok(());
        }
        if !node_kind_matches_fingerprint(known.node.kind(), fingerprint) {
            self.state
                .persist_observation_issue(
                    self.scope.library_id(),
                    Some(known.node.node_id()),
                    Some(path),
                    ObservationIssueKind::UnsupportedEntryType,
                )
                .await?;
            self.state
                .require_reconciliation(self.scope.library_id())
                .await?;
            return Ok(());
        }
        match known.node.kind() {
            NodeKind::File => {
                self.record_file_modification(known, path, fingerprint)
                    .await
            }
            NodeKind::Directory => Ok(()),
        }
    }

    async fn record_file_modification(
        &self,
        known: &crate::state::ObservedLocalNode,
        path: &ManagedRelativePath,
        fingerprint: LocalFingerprint,
    ) -> Result<(), ClientSyncError> {
        if fingerprint.kind() != LocalObjectKind::File {
            return Err(ClientSyncError::InvalidState);
        }
        let parent = self.required_parent(path).await?;
        let intent = self
            .new_intent(
                Some(known),
                Some(parent),
                OutboundIntentKind::ModifyFileContent,
                path.clone(),
                None,
                Some(fingerprint),
            )
            .await?;
        let persisted = self
            .state
            .upsert_outbound_intent_with_result(&intent)
            .await?;
        if persisted.changed() {
            self.note_durable_change();
        }
        self.state
            .persist_observed_node(&known.node, path, true, Some(fingerprint))
            .await
    }

    async fn classify_unknown_present(
        &self,
        path: &ManagedRelativePath,
        fingerprint: LocalFingerprint,
    ) -> Result<(), ClientSyncError> {
        let parent = self.observed_parent(path).await?;
        if self.replica.has_portable_name_collision(path)? {
            self.state
                .persist_observation_issue(
                    self.scope.library_id(),
                    parent.as_ref().map(|value| value.node.node_id()),
                    Some(path),
                    ObservationIssueKind::NameCollision,
                )
                .await?;
            self.state
                .require_reconciliation(self.scope.library_id())
                .await?;
            return Ok(());
        }
        let kind = match fingerprint.kind() {
            LocalObjectKind::Directory => OutboundIntentKind::CreateDirectory,
            LocalObjectKind::File => OutboundIntentKind::CreateFile,
        };
        let intent = self
            .new_intent(None, parent, kind, path.clone(), None, Some(fingerprint))
            .await?;
        let persisted = self
            .state
            .upsert_outbound_intent_with_result(&intent)
            .await?;
        if persisted.changed() {
            self.note_durable_change();
        }
        Ok(())
    }

    async fn required_parent(
        &self,
        path: &ManagedRelativePath,
    ) -> Result<crate::state::ObservedLocalNode, ClientSyncError> {
        let parent = self.observed_parent(path).await?;
        if let Some(parent) = parent {
            return Ok(parent);
        }
        self.state
            .persist_observation_issue(
                self.scope.library_id(),
                None,
                Some(path),
                ObservationIssueKind::ObservationBusy,
            )
            .await?;
        self.state
            .require_reconciliation(self.scope.library_id())
            .await?;
        Err(ClientSyncError::ObservationIssue(
            ObservationIssueKind::ObservationBusy,
        ))
    }

    /// A user-created parent directory has no server NodeId until a future
    /// explicit submission phase. Preserve `None` rather than manufacture a
    /// typed server identity; a later phase must establish ordering.
    async fn observed_parent(
        &self,
        path: &ManagedRelativePath,
    ) -> Result<Option<crate::state::ObservedLocalNode>, ClientSyncError> {
        let parent_path = path.parent().ok_or(ClientSyncError::InvalidRelativePath)?;
        if self.inspect_for_observation(&parent_path).await? != Some(LocalFingerprint::directory())
        {
            self.state
                .persist_observation_issue(
                    self.scope.library_id(),
                    None,
                    Some(&parent_path),
                    ObservationIssueKind::ObservationBusy,
                )
                .await?;
            self.state
                .require_reconciliation(self.scope.library_id())
                .await?;
            return Err(ClientSyncError::ObservationIssue(
                ObservationIssueKind::ObservationBusy,
            ));
        }
        Ok(self
            .state
            .observed_node_at_path(self.scope.library_id(), &parent_path)
            .await?
            .filter(|value| value.present && value.node.kind() == NodeKind::Directory))
    }

    async fn move_pending_create(
        &self,
        source: &ManagedRelativePath,
        destination: &ManagedRelativePath,
        destination_fingerprint: LocalFingerprint,
    ) -> Result<bool, ClientSyncError> {
        let kind = match destination_fingerprint.kind() {
            LocalObjectKind::Directory => OutboundIntentKind::CreateDirectory,
            LocalObjectKind::File => OutboundIntentKind::CreateFile,
        };
        let parent = self.observed_parent(destination).await?;
        let replacement = self
            .new_intent(
                None,
                parent,
                kind,
                destination.clone(),
                None,
                Some(destination_fingerprint),
            )
            .await?;
        let moved = self.state.move_pending_create(source, &replacement).await?;
        if moved.is_some() {
            self.note_durable_change();
        }
        Ok(moved.is_some())
    }

    #[allow(clippy::too_many_arguments)]
    async fn new_intent(
        &self,
        observed_node: Option<&crate::state::ObservedLocalNode>,
        parent: Option<crate::state::ObservedLocalNode>,
        kind: OutboundIntentKind,
        observed_path: ManagedRelativePath,
        old_path: Option<ManagedRelativePath>,
        fingerprint: Option<LocalFingerprint>,
    ) -> Result<OutboundIntent, ClientSyncError> {
        // This is the last root fence before a classified filesystem fact is
        // turned into durable outbound work. If the root disappeared after
        // inspection, do not let an absent path become a delete intent.
        if self.replica.validate_root().is_err() {
            self.validate_root_or_stop().await?;
        }
        let replica = self
            .state
            .replica(self.scope.library_id())
            .await?
            .ok_or(ClientSyncError::InvalidState)?;
        OutboundIntent::new(
            self.scope.library_id(),
            observed_node.map(|value| value.node.node_id()),
            parent.as_ref().map(|value| value.node.node_id()),
            kind,
            observed_path,
            old_path,
            fingerprint,
            replica.journal_epoch(),
            replica.applied_sequence(),
            observed_node.map(|value| value.node.revision()),
            observed_node.and_then(|value| value.node.current_version_id()),
            parent.map(|value| value.node.revision()),
        )
    }

    async fn suppression_matches(
        &self,
        path: &ManagedRelativePath,
        actual: Option<LocalFingerprint>,
    ) -> Result<bool, ClientSyncError> {
        let suppressions = self
            .state
            .observation_suppressions_for_path(self.scope.library_id(), path)
            .await?;
        for suppression in suppressions {
            if !suppression.expected_present {
                if actual.is_none() && path == &suppression.expected_relative_path {
                    return Ok(true);
                }
                continue;
            }
            if actual != suppression.expected_fingerprint {
                continue;
            }
            let known = self
                .state
                .observed_node_at_path(self.scope.library_id(), path)
                .await?;
            if known.as_ref().is_some_and(|value| {
                value.present
                    && value.fingerprint == actual
                    && (value.node.node_id() == suppression.node_id
                        || path.as_str().starts_with(&format!(
                            "{}/",
                            suppression.expected_relative_path.as_str()
                        )))
            }) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn inspect_for_observation(
        &self,
        path: &ManagedRelativePath,
    ) -> Result<Option<LocalFingerprint>, ClientSyncError> {
        match self.replica.inspect(path) {
            Ok(value) => Ok(value),
            Err(ClientSyncError::ContentUnstable) => {
                if self.replica.validate_root().is_err() {
                    self.validate_root_or_stop().await?;
                }
                self.state
                    .persist_observation_issue(
                        self.scope.library_id(),
                        None,
                        Some(path),
                        ObservationIssueKind::HashUnstable,
                    )
                    .await?;
                self.state
                    .require_reconciliation(self.scope.library_id())
                    .await?;
                Err(ClientSyncError::ObservationIssue(
                    ObservationIssueKind::HashUnstable,
                ))
            }
            Err(ClientSyncError::InvalidRoot)
            | Err(ClientSyncError::RootUnavailable)
            | Err(ClientSyncError::WrongRootBinding) => {
                self.validate_root_or_stop().await?;
                Err(ClientSyncError::RootUnavailable)
            }
            Err(ClientSyncError::RootRedirected) | Err(ClientSyncError::InvalidState) => {
                if self.replica.validate_root().is_err() {
                    self.validate_root_or_stop().await?;
                }
                self.state
                    .persist_observation_issue(
                        self.scope.library_id(),
                        None,
                        Some(path),
                        ObservationIssueKind::UnsupportedEntryType,
                    )
                    .await?;
                self.state
                    .require_reconciliation(self.scope.library_id())
                    .await?;
                Err(ClientSyncError::ObservationIssue(
                    ObservationIssueKind::UnsupportedEntryType,
                ))
            }
            Err(ClientSyncError::LocalIo) => {
                if self.replica.validate_root().is_err() {
                    self.validate_root_or_stop().await?;
                    return Err(ClientSyncError::RootUnavailable);
                }
                self.state
                    .persist_observation_issue(
                        self.scope.library_id(),
                        None,
                        Some(path),
                        ObservationIssueKind::ObservationBusy,
                    )
                    .await?;
                self.state
                    .require_reconciliation(self.scope.library_id())
                    .await?;
                Err(ClientSyncError::ObservationIssue(
                    ObservationIssueKind::ObservationBusy,
                ))
            }
            Err(error) => Err(error),
        }
    }

    async fn reconcile_directory_work(
        &self,
        generation: Sequence,
        work: &crate::state::ObservationScanWork,
    ) -> Result<(), ClientSyncError> {
        if let Some(root_node) = self
            .state
            .observed_node_at_path(self.scope.library_id(), &work.relative_path)
            .await?
        {
            self.state
                .mark_scan_node_seen(
                    self.scope.library_id(),
                    generation,
                    root_node.node.node_id(),
                )
                .await?;
        }
        let directory = match self.inspect_for_observation(&work.relative_path).await? {
            Some(fingerprint) if fingerprint.kind() == LocalObjectKind::Directory => true,
            Some(_) | None => {
                self.classify_path(&work.relative_path).await?;
                false
            }
        };
        if !directory {
            return self
                .state
                .complete_observation_scan_work(
                    self.scope.library_id(),
                    generation,
                    work,
                    None,
                    &[],
                )
                .await;
        }
        let native_directory = self.replica.root_path().join(work.relative_path.as_path());
        if fs::symlink_metadata(&native_directory)
            .map_err(|_| ClientSyncError::LocalIo)?
            .file_type()
            .is_symlink()
        {
            self.state
                .persist_observation_issue(
                    self.scope.library_id(),
                    None,
                    Some(&work.relative_path),
                    ObservationIssueKind::UnsupportedEntryType,
                )
                .await?;
            self.state
                .require_reconciliation(self.scope.library_id())
                .await?;
            return Err(ClientSyncError::ObservationIssue(
                ObservationIssueKind::UnsupportedEntryType,
            ));
        }
        let mut candidates = BTreeMap::new();
        let maximum = self.config.scan_batch_entries();
        for entry in fs::read_dir(&native_directory)? {
            let entry = entry?;
            let name = match entry.file_name().into_string() {
                Ok(name) => name,
                Err(_) => {
                    self.state
                        .persist_observation_issue(
                            self.scope.library_id(),
                            None,
                            Some(&work.relative_path),
                            ObservationIssueKind::UnrepresentableName,
                        )
                        .await?;
                    self.state
                        .require_reconciliation(self.scope.library_id())
                        .await?;
                    return Err(ClientSyncError::ObservationIssue(
                        ObservationIssueKind::UnrepresentableName,
                    ));
                }
            };
            if work
                .cursor_name
                .as_ref()
                .is_some_and(|cursor| &name <= cursor)
            {
                continue;
            }
            if work.relative_path.is_root() && name == ".synveil" {
                continue;
            }
            let child = match work.relative_path.child(&name) {
                Ok(child) => child,
                Err(_) => {
                    self.state
                        .persist_observation_issue(
                            self.scope.library_id(),
                            None,
                            Some(&work.relative_path),
                            ObservationIssueKind::UnrepresentableName,
                        )
                        .await?;
                    continue;
                }
            };
            candidates.insert(name, child);
            if candidates.len() > maximum.saturating_add(1) {
                let last = candidates.keys().next_back().cloned();
                if let Some(last) = last {
                    candidates.remove(&last);
                }
            }
        }
        let has_more = candidates.len() > maximum;
        let selected: Vec<_> = candidates.into_iter().take(maximum).collect();
        let next_cursor = if has_more {
            Some(
                selected
                    .last()
                    .map(|(name, _)| name.as_str())
                    .ok_or(ClientSyncError::InvalidState)?,
            )
        } else {
            None
        };
        let mut child_directories = Vec::new();
        for (_, child) in &selected {
            let known = self
                .state
                .observed_node_at_path(self.scope.library_id(), child)
                .await?;
            self.classify_path(child).await?;
            if let Some(known) = known {
                self.state
                    .mark_scan_node_seen(self.scope.library_id(), generation, known.node.node_id())
                    .await?;
            }
            if matches!(
                self.replica.inspect(child),
                Ok(Some(fingerprint)) if fingerprint == LocalFingerprint::directory()
            ) {
                child_directories.push(child.clone());
            }
        }
        self.state
            .complete_observation_scan_work(
                self.scope.library_id(),
                generation,
                work,
                next_cursor,
                &child_directories,
            )
            .await
    }
}

fn node_kind_matches_fingerprint(kind: NodeKind, fingerprint: LocalFingerprint) -> bool {
    matches!(
        (kind, fingerprint.kind()),
        (NodeKind::File, LocalObjectKind::File) | (NodeKind::Directory, LocalObjectKind::Directory)
    )
}

fn expected_node_fingerprint(node: &LocalNode) -> Option<LocalFingerprint> {
    match node.kind() {
        NodeKind::Directory => Some(LocalFingerprint::directory()),
        NodeKind::File => node
            .local_length()
            .zip(node.local_sha256())
            .or_else(|| node.content_length().zip(node.content_sha256()))
            .map(|(length, sha256)| LocalFingerprint::file(length, sha256)),
    }
}

fn coalesce_hints(
    hints: Vec<WatchHint>,
) -> (
    Vec<(ManagedRelativePath, ManagedRelativePath)>,
    BTreeSet<ManagedRelativePath>,
) {
    let mut renames = Vec::new();
    let mut paths = BTreeSet::new();
    for hint in hints {
        match hint.kind() {
            WatchHintKind::Rename => {
                let source = hint.paths()[0].clone();
                let destination = hint.paths()[1].clone();
                if source == destination {
                    continue;
                }
                if let Some((_, existing_destination)) = renames
                    .iter_mut()
                    .find(|(_, existing_destination)| *existing_destination == source)
                {
                    *existing_destination = destination;
                } else {
                    renames.push((source, destination));
                }
            }
            WatchHintKind::RescanRequired => {}
            _ => paths.extend(hint.paths().iter().cloned()),
        }
    }
    (renames, paths)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Write,
        path::PathBuf,
        sync::{Arc, Mutex},
        time::Duration,
    };

    #[cfg(target_os = "linux")]
    use std::time::Instant;

    use synveil_core::{
        DeviceId, LibraryId, LogicalName, NodeId, NodeKind, NodeState, OutboundIntentId, Revision,
        Sequence, Sha256Digest, UserId,
    };

    use super::{
        LocalChangeWatcher, ManualChangeSource, ManualChangeWatcher, ObservationConfig,
        ObservationIssueKind, OutboundIntent, OutboundIntentKind, OutboundObservationEngine,
        WatchHint, WatchHintKind, coalesce_hints, native_to_managed_relative,
    };
    use crate::{
        FilesystemLocalReplica, LocalFingerprint, LocalNode, LocalReplica, LocalStateConfig,
        LocalStateStore, ManagedRelativePath, ReplicaScope, SyncRuntimeWakeReason,
        SyncRuntimeWakeResult, SyncWakeNotifier, test_support::remove_dir_all_bounded,
    };

    #[test]
    fn native_watcher_filters_validated_control_tree_before_path_policy() {
        let root = PathBuf::from("/tmp/synveil-observation-root");
        assert!(
            native_to_managed_relative(&root, &root.join(".synveil"))
                .unwrap()
                .is_none()
        );
        assert!(
            native_to_managed_relative(&root, &root.join(".synveil/staging/internal.part"),)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            native_to_managed_relative(&root, &root.join("visible/file.txt"))
                .unwrap()
                .unwrap()
                .as_str(),
            "visible/file.txt"
        );
        assert!(native_to_managed_relative(&root, &root.join("../outside"),).is_err());
    }

    #[cfg(target_os = "linux")]
    use crate::NotifyLocalChangeWatcher;

    struct Harness {
        directory: PathBuf,
        root: PathBuf,
        scope: ReplicaScope,
        root_id: NodeId,
        replica: Arc<FilesystemLocalReplica>,
        state: Arc<LocalStateStore>,
        engine: OutboundObservationEngine,
        source: Option<ManualChangeSource>,
    }

    struct RecordingNotifier {
        calls: Mutex<Vec<(LibraryId, SyncRuntimeWakeReason)>>,
    }

    impl RecordingNotifier {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
            })
        }

        fn calls(&self) -> Vec<(LibraryId, SyncRuntimeWakeReason)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl SyncWakeNotifier for RecordingNotifier {
        fn wake_library(
            &self,
            library_id: LibraryId,
            reason: SyncRuntimeWakeReason,
        ) -> SyncRuntimeWakeResult {
            self.calls.lock().unwrap().push((library_id, reason));
            SyncRuntimeWakeResult::Queued
        }
    }

    impl Harness {
        async fn manual() -> Self {
            let (watcher, source) = ManualChangeWatcher::with_capacity(8);
            Self::new(Box::new(watcher), Some(source)).await
        }

        #[cfg(target_os = "linux")]
        async fn native() -> Self {
            Self::new(Box::new(NotifyLocalChangeWatcher::new(32)), None).await
        }

        async fn new(
            watcher: Box<dyn LocalChangeWatcher>,
            source: Option<ManualChangeSource>,
        ) -> Self {
            Self::new_with_notifier(watcher, source, None).await
        }

        async fn new_with_notifier(
            watcher: Box<dyn LocalChangeWatcher>,
            source: Option<ManualChangeSource>,
            notifier: Option<Arc<dyn SyncWakeNotifier>>,
        ) -> Self {
            let debounce = if source.is_some() {
                Duration::ZERO
            } else {
                Duration::from_millis(200)
            };
            Self::new_with_notifier_and_config(
                watcher,
                source,
                notifier,
                ObservationConfig::new(8, 8, 2, debounce).unwrap(),
            )
            .await
        }

        async fn new_with_notifier_and_config(
            watcher: Box<dyn LocalChangeWatcher>,
            source: Option<ManualChangeSource>,
            notifier: Option<Arc<dyn SyncWakeNotifier>>,
            config: ObservationConfig,
        ) -> Self {
            let directory = std::env::temp_dir().join(format!(
                "synveil-outbound-observation-{}",
                uuid::Uuid::now_v7()
            ));
            let root = directory.join("root");
            fs::create_dir(&directory).unwrap();
            fs::create_dir(&root).unwrap();
            let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
            let replica = Arc::new(FilesystemLocalReplica::initialize(&root, scope).unwrap());
            let state = Arc::new(
                LocalStateStore::open(&LocalStateConfig::new(directory.join("state.sqlite3")))
                    .await
                    .unwrap(),
            );
            state
                .bind_replica(scope, replica.binding_id())
                .await
                .unwrap();
            let root_id = NodeId::new();
            state
                .upsert_local_node(&local_node(
                    scope,
                    root_id,
                    None,
                    ManagedRelativePath::root(),
                    "root",
                    NodeKind::Directory,
                    LocalFingerprint::directory(),
                ))
                .await
                .unwrap();
            let engine = match notifier {
                Some(notifier) => OutboundObservationEngine::new_with_wake_notifier(
                    scope,
                    replica.clone(),
                    state.clone(),
                    watcher,
                    config,
                    notifier,
                )
                .await
                .unwrap(),
                None => OutboundObservationEngine::new(
                    scope,
                    replica.clone(),
                    state.clone(),
                    watcher,
                    config,
                )
                .await
                .unwrap(),
            };
            Self {
                directory,
                root,
                scope,
                root_id,
                replica,
                state,
                engine,
                source,
            }
        }

        async fn start(&self) {
            self.engine.start().await.unwrap();
            self.settle().await;
        }

        async fn settle(&self) {
            for _ in 0..128 {
                let state = self.engine.reconcile_once().await.unwrap();
                if !state.scan_active() && !state.rescan_required() {
                    return;
                }
            }
            panic!("bounded reconciliation did not converge");
        }

        async fn seed_directory(&self, path: &str, parent_node_id: NodeId) -> NodeId {
            fs::create_dir_all(self.root.join(path)).unwrap();
            let node_id = NodeId::new();
            self.state
                .upsert_local_node(&local_node(
                    self.scope,
                    node_id,
                    Some(parent_node_id),
                    ManagedRelativePath::new(path).unwrap(),
                    path.rsplit('/').next().unwrap(),
                    NodeKind::Directory,
                    LocalFingerprint::directory(),
                ))
                .await
                .unwrap();
            node_id
        }

        async fn seed_file(&self, path: &str, parent_node_id: NodeId, contents: &[u8]) -> NodeId {
            fs::write(self.root.join(path), contents).unwrap();
            let relative = ManagedRelativePath::new(path).unwrap();
            let fingerprint = self.replica.inspect(&relative).unwrap().unwrap();
            let node_id = NodeId::new();
            self.state
                .upsert_local_node(&local_node(
                    self.scope,
                    node_id,
                    Some(parent_node_id),
                    relative,
                    path.rsplit('/').next().unwrap(),
                    NodeKind::File,
                    fingerprint,
                ))
                .await
                .unwrap();
            node_id
        }

        async fn observe(&self, kind: WatchHintKind, paths: &[&str]) {
            let paths = paths
                .iter()
                .map(|path| ManagedRelativePath::new(*path).unwrap())
                .collect::<Vec<_>>();
            self.source
                .as_ref()
                .unwrap()
                .push(WatchHint::new(kind, paths).unwrap())
                .unwrap();
            self.engine.poll_once().await.unwrap();
        }

        async fn close(self) {
            self.engine.shutdown().await.unwrap();
            self.state.close_pool().await;
            let directory = self.directory.clone();
            drop(self);
            remove_dir_all_bounded(&directory).unwrap();
        }
    }

    fn local_node(
        scope: ReplicaScope,
        node_id: NodeId,
        parent_node_id: Option<NodeId>,
        relative_path: ManagedRelativePath,
        name: &str,
        kind: NodeKind,
        fingerprint: LocalFingerprint,
    ) -> LocalNode {
        let (length, sha256) = match fingerprint.kind() {
            crate::LocalObjectKind::Directory => (None, None),
            crate::LocalObjectKind::File => (fingerprint.length(), fingerprint.sha256()),
        };
        LocalNode::new(
            scope.library_id(),
            node_id,
            parent_node_id,
            relative_path,
            LogicalName::new(name).unwrap(),
            kind,
            NodeState::Active,
            Revision::new(1),
            None,
            None,
            None,
            length,
            sha256,
            Sequence::new(0),
            true,
            None,
        )
    }

    #[test]
    fn intent_shapes_hints_and_rename_coalescing_are_closed() {
        let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), LibraryId::new());
        let path = ManagedRelativePath::new("draft.txt").unwrap();
        let fingerprint = LocalFingerprint::file(1, Sha256Digest::from_bytes([7; 32]));
        assert!(
            OutboundIntent::new(
                scope.library_id(),
                Some(NodeId::new()),
                Some(NodeId::new()),
                OutboundIntentKind::CreateFile,
                path.clone(),
                None,
                Some(fingerprint),
                Sequence::new(0),
                Sequence::new(0),
                None,
                None,
                None,
            )
            .is_err()
        );
        assert!(
            OutboundIntent::new(
                scope.library_id(),
                Some(NodeId::new()),
                Some(NodeId::new()),
                OutboundIntentKind::RenameNode,
                path.clone(),
                Some(path.clone()),
                Some(fingerprint),
                Sequence::new(0),
                Sequence::new(0),
                Some(Revision::new(1)),
                None,
                Some(Revision::new(1)),
            )
            .is_err()
        );
        assert!(WatchHint::new(WatchHintKind::Rename, vec![path.clone()]).is_err());
        assert!(WatchHint::new(WatchHintKind::RescanRequired, vec![path.clone()]).is_err());

        let first = ManagedRelativePath::new("a").unwrap();
        let middle = ManagedRelativePath::new("b").unwrap();
        let last = ManagedRelativePath::new("c").unwrap();
        let (renames, paths) = coalesce_hints(vec![
            WatchHint::new(WatchHintKind::Rename, vec![first.clone(), middle.clone()]).unwrap(),
            WatchHint::new(WatchHintKind::Rename, vec![middle.clone(), last.clone()]).unwrap(),
            WatchHint::new(WatchHintKind::Modify, vec![last.clone()]).unwrap(),
        ]);
        assert_eq!(renames, vec![(first, last.clone())]);
        assert_eq!(paths.into_iter().collect::<Vec<_>>(), vec![last]);
        let intent_id = OutboundIntentId::new();
        assert_eq!(
            OutboundIntentId::parse_str(&intent_id.to_string()).unwrap(),
            intent_id
        );
    }

    #[tokio::test]
    async fn manual_watcher_captures_modify_rename_move_and_delete() {
        let harness = Harness::manual().await;
        let left = harness.seed_directory("left", harness.root_id).await;
        let right = harness.seed_directory("right", harness.root_id).await;
        let file_id = harness.seed_file("left/report.txt", left, b"old").await;
        harness.start().await;

        fs::write(harness.root.join("left/report.txt"), b"changed").unwrap();
        harness
            .observe(WatchHintKind::Modify, &["left/report.txt"])
            .await;
        assert!(
            harness
                .engine
                .list_pending_intents()
                .await
                .unwrap()
                .iter()
                .any(
                    |intent| intent.kind() == OutboundIntentKind::ModifyFileContent
                        && intent.node_id() == Some(file_id)
                )
        );

        fs::rename(
            harness.root.join("left/report.txt"),
            harness.root.join("right/final.txt"),
        )
        .unwrap();
        harness
            .observe(
                WatchHintKind::Rename,
                &["left/report.txt", "right/final.txt"],
            )
            .await;
        assert!(
            harness
                .engine
                .list_pending_intents()
                .await
                .unwrap()
                .iter()
                .any(|intent| intent.kind() == OutboundIntentKind::MoveNode
                    && intent.node_id() == Some(file_id)
                    && intent
                        .old_relative_path()
                        .is_some_and(|path| path.as_str() == "left/report.txt")
                    && intent.observed_relative_path().as_str() == "right/final.txt")
        );

        fs::remove_file(harness.root.join("right/final.txt")).unwrap();
        harness
            .observe(WatchHintKind::Remove, &["right/final.txt"])
            .await;
        assert!(
            harness
                .engine
                .list_pending_intents()
                .await
                .unwrap()
                .iter()
                .any(
                    |intent| intent.kind() == OutboundIntentKind::DeleteOrTrashNode
                        && intent.node_id() == Some(file_id)
                )
        );
        let _ = right;
        harness.close().await;
    }

    #[tokio::test]
    async fn durable_observation_batch_wakes_once_and_noop_does_not_wake() {
        let (watcher, source) = ManualChangeWatcher::with_capacity(8);
        let notifier = RecordingNotifier::new();
        let harness = Harness::new_with_notifier(
            Box::new(watcher),
            Some(source.clone()),
            Some(notifier.clone() as Arc<dyn SyncWakeNotifier>),
        )
        .await;
        harness.start().await;
        assert!(notifier.calls().is_empty());

        fs::create_dir(harness.root.join("first")).unwrap();
        fs::create_dir(harness.root.join("second")).unwrap();
        source
            .push(
                WatchHint::new(
                    WatchHintKind::Create,
                    vec![ManagedRelativePath::new("first").unwrap()],
                )
                .unwrap(),
            )
            .unwrap();
        source
            .push(
                WatchHint::new(
                    WatchHintKind::Create,
                    vec![ManagedRelativePath::new("second").unwrap()],
                )
                .unwrap(),
            )
            .unwrap();
        let result = harness.engine.poll_once_with_notification().await.unwrap();
        assert_eq!(result.inspected_hints(), 2);
        assert_eq!(
            result.notification().durable_result(),
            crate::DurableChangeResult::Committed
        );
        assert_eq!(
            result.notification().wake_result(),
            Some(SyncRuntimeWakeResult::Queued)
        );
        assert_eq!(notifier.calls().len(), 1);
        assert_eq!(
            notifier.calls()[0],
            (
                harness.scope.library_id(),
                SyncRuntimeWakeReason::LocalChange
            )
        );
        assert_eq!(harness.engine.count_pending_intents().await.unwrap(), 2);

        let noop = harness.engine.poll_once_with_notification().await.unwrap();
        assert_eq!(
            noop.notification().durable_result(),
            crate::DurableChangeResult::NoChange
        );
        assert_eq!(notifier.calls().len(), 1);
        harness.close().await;
    }

    #[tokio::test]
    async fn existing_tree_scan_creates_content_intents_without_deletes() {
        let harness = Harness::manual().await;
        fs::create_dir_all(harness.root.join("nested")).unwrap();
        fs::write(harness.root.join("alpha.txt"), b"alpha").unwrap();
        fs::write(harness.root.join("nested/beta.txt"), b"beta").unwrap();

        harness.start().await;
        let intents = harness.engine.list_pending_intents().await.unwrap();
        assert!(intents.iter().any(|intent| {
            intent.kind() == OutboundIntentKind::CreateFile
                && intent.observed_relative_path().as_str() == "alpha.txt"
                && intent.parent_node_id() == Some(harness.root_id)
        }));
        assert!(intents.iter().any(|intent| {
            intent.kind() == OutboundIntentKind::CreateDirectory
                && intent.observed_relative_path().as_str() == "nested"
                && intent.parent_node_id() == Some(harness.root_id)
        }));
        assert!(intents.iter().any(|intent| {
            intent.kind() == OutboundIntentKind::CreateFile
                && intent.observed_relative_path().as_str() == "nested/beta.txt"
                && intent.parent_node_id().is_none()
        }));
        assert!(
            !intents
                .iter()
                .any(|intent| intent.kind() == OutboundIntentKind::DeleteOrTrashNode)
        );

        harness.close().await;
    }

    #[tokio::test]
    async fn thousand_observation_events_emit_one_wake_and_preserve_intents() {
        let (watcher, source) = ManualChangeWatcher::with_capacity(1_024);
        let notifier = RecordingNotifier::new();
        let harness = Harness::new_with_notifier_and_config(
            Box::new(watcher),
            Some(source.clone()),
            Some(notifier.clone() as Arc<dyn SyncWakeNotifier>),
            ObservationConfig::new(1_024, 1_024, 128, Duration::ZERO).unwrap(),
        )
        .await;
        harness.start().await;

        for index in 0..1_000 {
            let name = format!("burst-{index}");
            fs::create_dir(harness.root.join(&name)).unwrap();
            source
                .push(
                    WatchHint::new(
                        WatchHintKind::Create,
                        vec![ManagedRelativePath::new(&name).unwrap()],
                    )
                    .unwrap(),
                )
                .unwrap();
        }

        let result = harness.engine.poll_once_with_notification().await.unwrap();
        assert_eq!(result.inspected_hints(), 1_000);
        assert_eq!(
            result.notification().durable_result(),
            crate::DurableChangeResult::Committed
        );
        assert_eq!(
            result.notification().wake_result(),
            Some(SyncRuntimeWakeResult::Queued)
        );
        assert_eq!(notifier.calls().len(), 1);
        assert_eq!(harness.engine.count_pending_intents().await.unwrap(), 1_000);
        harness.close().await;
    }

    #[tokio::test]
    async fn validated_root_recovery_resolves_only_root_observation_issues() {
        let harness = Harness::manual().await;
        harness
            .state
            .persist_observation_issue(
                harness.scope.library_id(),
                None,
                None,
                ObservationIssueKind::RootInvalid,
            )
            .await
            .unwrap();
        harness
            .state
            .persist_observation_issue(
                harness.scope.library_id(),
                None,
                None,
                ObservationIssueKind::WatcherOverflow,
            )
            .await
            .unwrap();

        harness.engine.start().await.unwrap();

        let issues = harness
            .state
            .observation_issues(harness.scope.library_id())
            .await
            .unwrap();
        assert!(
            !issues
                .iter()
                .any(|issue| issue.kind() == ObservationIssueKind::RootInvalid)
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.kind() == ObservationIssueKind::WatcherOverflow)
        );
        harness.close().await;
    }

    #[tokio::test]
    async fn rescan_durable_work_emits_one_wake_after_bounded_reconciliation() {
        let (watcher, source) = ManualChangeWatcher::with_capacity(8);
        let notifier = RecordingNotifier::new();
        let harness = Harness::new_with_notifier(
            Box::new(watcher),
            Some(source.clone()),
            Some(notifier.clone() as Arc<dyn SyncWakeNotifier>),
        )
        .await;
        harness.start().await;

        fs::create_dir(harness.root.join("rescan-directory")).unwrap();
        fs::write(
            harness.root.join("rescan-directory/rescan-file.txt"),
            b"rescan",
        )
        .unwrap();
        source.overflow().unwrap();

        let first = harness.engine.poll_once_with_notification().await.unwrap();
        assert_eq!(
            first.notification().durable_result(),
            crate::DurableChangeResult::Committed
        );
        assert_eq!(first.notification().wake_result(), None);
        harness.settle().await;

        assert_eq!(harness.engine.count_pending_intents().await.unwrap(), 2);
        assert_eq!(notifier.calls().len(), 1);
        assert_eq!(
            notifier.calls()[0],
            (
                harness.scope.library_id(),
                SyncRuntimeWakeReason::LocalChange
            )
        );
        harness.close().await;
    }

    #[tokio::test]
    async fn pending_create_rename_preserves_its_local_intent_id() {
        let harness = Harness::manual().await;
        harness.start().await;
        fs::write(harness.root.join("draft.txt"), b"draft").unwrap();
        harness.observe(WatchHintKind::Create, &["draft.txt"]).await;
        let created = harness.engine.list_pending_intents().await.unwrap();
        assert_eq!(created.len(), 1);
        let intent_id = created[0].intent_id();

        fs::rename(
            harness.root.join("draft.txt"),
            harness.root.join("final.txt"),
        )
        .unwrap();
        harness
            .observe(WatchHintKind::Rename, &["draft.txt", "final.txt"])
            .await;
        let intents = harness.engine.list_pending_intents().await.unwrap();
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].intent_id(), intent_id);
        assert_eq!(intents[0].kind(), OutboundIntentKind::CreateFile);
        assert_eq!(intents[0].observed_relative_path().as_str(), "final.txt");
        harness.close().await;
    }

    #[tokio::test]
    async fn overflow_rescan_captures_nested_creates_and_excludes_control_paths() {
        let harness = Harness::manual().await;
        harness.start().await;
        fs::create_dir(harness.root.join("new-directory")).unwrap();
        fs::write(harness.root.join("new-directory/child.txt"), b"child").unwrap();
        fs::write(
            harness.root.join(".synveil/staging/ignored-by-observer"),
            b"internal",
        )
        .unwrap();
        harness.source.as_ref().unwrap().overflow().unwrap();
        harness.engine.poll_once().await.unwrap();
        harness.settle().await;

        let intents = harness.engine.list_pending_intents().await.unwrap();
        assert!(intents.iter().any(|intent| {
            intent.kind() == OutboundIntentKind::CreateDirectory
                && intent.observed_relative_path().as_str() == "new-directory"
        }));
        assert!(intents.iter().any(|intent| {
            intent.kind() == OutboundIntentKind::CreateFile
                && intent.observed_relative_path().as_str() == "new-directory/child.txt"
                && intent.parent_node_id().is_none()
        }));
        assert!(intents.iter().all(|intent| {
            !intent
                .observed_relative_path()
                .as_str()
                .starts_with(".synveil/")
        }));
        assert!(
            harness
                .state
                .observation_issues(harness.scope.library_id())
                .await
                .unwrap()
                .iter()
                .any(|issue| issue.kind() == ObservationIssueKind::WatcherOverflow)
        );
        harness.close().await;
    }

    #[tokio::test]
    async fn outbound_staging_writes_are_excluded_from_observer_intents() {
        let harness = Harness::manual().await;
        harness.start().await;
        let staging_dir = harness.root.join(".synveil/staging/outbound-upload");
        fs::create_dir_all(&staging_dir).unwrap();
        let staging_file = staging_dir.join("chunk.tmp");
        let mut file = fs::File::create(&staging_file).unwrap();
        file.write_all(b"internal staged bytes").unwrap();
        file.sync_all().unwrap();
        drop(file);
        fs::remove_file(&staging_file).unwrap();
        harness.source.as_ref().unwrap().overflow().unwrap();
        harness.engine.poll_once().await.unwrap();
        harness.settle().await;

        assert_eq!(harness.engine.list_pending_intents().await.unwrap(), []);
        harness.close().await;
    }

    #[tokio::test]
    async fn restart_scan_cancels_a_vanished_unsubmitted_create() {
        let harness = Harness::manual().await;
        harness.start().await;
        fs::write(harness.root.join("temporary.txt"), b"temporary").unwrap();
        harness
            .observe(WatchHintKind::Create, &["temporary.txt"])
            .await;
        assert_eq!(harness.engine.count_pending_intents().await.unwrap(), 1);

        let Harness {
            directory,
            root,
            scope,
            root_id: _,
            replica,
            state,
            engine,
            source,
        } = harness;
        engine.shutdown().await.unwrap();
        drop(engine);
        drop(source);
        drop(state);
        fs::remove_file(root.join("temporary.txt")).unwrap();

        let state = Arc::new(
            LocalStateStore::open(&LocalStateConfig::new(directory.join("state.sqlite3")))
                .await
                .unwrap(),
        );
        let (watcher, _source) = ManualChangeWatcher::with_capacity(8);
        let restarted = OutboundObservationEngine::new(
            scope,
            replica,
            state.clone(),
            Box::new(watcher),
            ObservationConfig::new(8, 8, 2, Duration::ZERO).unwrap(),
        )
        .await
        .unwrap();
        restarted.start().await.unwrap();
        for _ in 0..128 {
            let current = restarted.reconcile_once().await.unwrap();
            if !current.scan_active() && !current.rescan_required() {
                break;
            }
        }
        assert_eq!(restarted.count_pending_intents().await.unwrap(), 0);
        restarted.shutdown().await.unwrap();
        state.close_pool().await;
        drop(restarted);
        drop(state);
        remove_dir_all_bounded(&directory).unwrap();
    }

    #[tokio::test]
    async fn unpaired_remove_create_is_ambiguous_and_does_not_guess_rename_or_split_intents() {
        let harness = Harness::manual().await;
        let file_id = harness
            .seed_file("old.txt", harness.root_id, b"same-kind")
            .await;
        harness.start().await;

        fs::remove_file(harness.root.join("old.txt")).unwrap();
        fs::write(harness.root.join("new.txt"), b"same-kind").unwrap();
        harness
            .source
            .as_ref()
            .unwrap()
            .push(
                WatchHint::new(
                    WatchHintKind::Remove,
                    vec![ManagedRelativePath::new("old.txt").unwrap()],
                )
                .unwrap(),
            )
            .unwrap();
        harness
            .source
            .as_ref()
            .unwrap()
            .push(
                WatchHint::new(
                    WatchHintKind::Create,
                    vec![ManagedRelativePath::new("new.txt").unwrap()],
                )
                .unwrap(),
            )
            .unwrap();
        harness.engine.poll_once().await.unwrap();

        assert_eq!(harness.engine.count_pending_intents().await.unwrap(), 0);
        assert!(
            harness
                .state
                .observation_issues(harness.scope.library_id())
                .await
                .unwrap()
                .iter()
                .any(|issue| {
                    issue.kind() == ObservationIssueKind::AmbiguousRename
                        && (issue.node_id() == Some(file_id)
                            || issue
                                .relative_path()
                                .is_some_and(|path| path.as_str() == "new.txt"))
                })
        );
        harness.close().await;
    }

    #[tokio::test]
    async fn durable_suppression_does_not_hide_immediate_user_edit() {
        let harness = Harness::manual().await;
        let file_id = harness
            .seed_file("server.txt", harness.root_id, b"server")
            .await;
        harness.start().await;
        let path = ManagedRelativePath::new("server.txt").unwrap();
        let fingerprint = harness.replica.inspect(&path).unwrap().unwrap();
        sqlx::query(
            "INSERT INTO observation_suppressions (
                 operation_id, library_id, node_id, expected_relative_path,
                 expected_present, expected_kind, expected_length, expected_sha256,
                 created_at_ms
             ) VALUES (?, ?, ?, ?, 1, 'FILE', ?, ?, 1)",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(harness.scope.library_id().to_string())
        .bind(file_id.to_string())
        .bind(path.as_str())
        .bind(i64::try_from(fingerprint.length().unwrap()).unwrap())
        .bind(fingerprint.sha256().unwrap().into_bytes().to_vec())
        .execute(&harness.state.pool)
        .await
        .unwrap();

        harness
            .observe(WatchHintKind::Modify, &["server.txt"])
            .await;
        assert_eq!(harness.engine.count_pending_intents().await.unwrap(), 0);

        fs::write(harness.root.join("server.txt"), b"user").unwrap();
        harness
            .observe(WatchHintKind::Modify, &["server.txt"])
            .await;
        let intents = harness.engine.list_pending_intents().await.unwrap();
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].kind(), OutboundIntentKind::ModifyFileContent);
        assert_eq!(intents[0].node_id(), Some(file_id));
        harness.close().await;
    }

    #[tokio::test]
    async fn inbound_directory_and_absent_suppressions_create_no_outbound_intents() {
        let (watcher, source) = ManualChangeWatcher::with_capacity(8);
        let notifier = RecordingNotifier::new();
        let harness = Harness::new_with_notifier(
            Box::new(watcher),
            Some(source),
            Some(notifier.clone() as Arc<dyn SyncWakeNotifier>),
        )
        .await;
        let dir_id = harness.seed_directory("from-server", harness.root_id).await;
        let file_id = harness
            .seed_file("deleted-by-server.txt", harness.root_id, b"delete")
            .await;
        harness.start().await;
        fs::remove_file(harness.root.join("deleted-by-server.txt")).unwrap();
        sqlx::query(
            "INSERT INTO observation_suppressions (
                 operation_id, library_id, node_id, expected_relative_path,
                 expected_present, expected_kind, expected_length, expected_sha256,
                 created_at_ms
             ) VALUES (?, ?, ?, 'from-server', 1, 'DIRECTORY', NULL, NULL, 1),
                      (?, ?, ?, 'deleted-by-server.txt', 0, NULL, NULL, NULL, 1)",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(harness.scope.library_id().to_string())
        .bind(dir_id.to_string())
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(harness.scope.library_id().to_string())
        .bind(file_id.to_string())
        .execute(&harness.state.pool)
        .await
        .unwrap();

        harness
            .observe(WatchHintKind::Create, &["from-server"])
            .await;
        harness
            .observe(WatchHintKind::Remove, &["deleted-by-server.txt"])
            .await;
        assert_eq!(harness.engine.count_pending_intents().await.unwrap(), 0);
        assert!(notifier.calls().is_empty());

        fs::write(harness.root.join("user.txt"), b"user").unwrap();
        harness.observe(WatchHintKind::Create, &["user.txt"]).await;
        assert_eq!(harness.engine.count_pending_intents().await.unwrap(), 1);
        assert_eq!(notifier.calls().len(), 1);
        harness.close().await;
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn linux_notify_backend_delivers_a_live_filesystem_create() {
        let harness = Harness::native().await;
        harness.start().await;
        fs::write(harness.root.join("native.txt"), b"native").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            harness.engine.poll_once().await.unwrap();
            if harness.engine.count_pending_intents().await.unwrap() == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let intents = harness.engine.list_pending_intents().await.unwrap();
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].kind(), OutboundIntentKind::CreateFile);
        assert_eq!(intents[0].observed_relative_path().as_str(), "native.txt");
        harness.close().await;
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn linux_notify_backend_handles_live_rename_move_delete_symlink_and_editor_save() {
        use std::os::unix::fs::symlink;

        let harness = Harness::native().await;
        let left = harness.seed_directory("left", harness.root_id).await;
        let right = harness.seed_directory("right", harness.root_id).await;
        let file_id = harness.seed_file("left/live.txt", left, b"old").await;
        harness.start().await;

        fs::rename(
            harness.root.join("left/live.txt"),
            harness.root.join("right/live-renamed.txt"),
        )
        .unwrap();
        eventually(&harness, |intents| {
            intents.iter().any(|intent| {
                intent.node_id() == Some(file_id)
                    && matches!(
                        intent.kind(),
                        OutboundIntentKind::MoveNode | OutboundIntentKind::DeleteOrTrashNode
                    )
            })
        })
        .await;
        let native_move_supported = harness
            .engine
            .list_pending_intents()
            .await
            .unwrap()
            .iter()
            .any(|intent| {
                intent.node_id() == Some(file_id)
                    && intent.kind() == OutboundIntentKind::MoveNode
                    && intent.observed_relative_path().as_str() == "right/live-renamed.txt"
            });
        if !native_move_supported {
            assert!(
                harness
                    .engine
                    .list_pending_intents()
                    .await
                    .unwrap()
                    .iter()
                    .any(|intent| intent.node_id() == Some(file_id)
                        && intent.kind() == OutboundIntentKind::DeleteOrTrashNode)
            );
        }

        if native_move_supported {
            fs::write(harness.root.join(".editor.tmp"), b"new").unwrap();
            fs::rename(
                harness.root.join(".editor.tmp"),
                harness.root.join("right/live-renamed.txt"),
            )
            .unwrap();
            eventually(&harness, |intents| {
                intents.iter().any(|intent| {
                    intent.node_id() == Some(file_id)
                        && intent.kind() == OutboundIntentKind::ModifyFileContent
                        && intent.observed_relative_path().as_str() == "right/live-renamed.txt"
                })
            })
            .await;

            fs::remove_file(harness.root.join("right/live-renamed.txt")).unwrap();
            eventually(&harness, |intents| {
                intents.iter().any(|intent| {
                    intent.node_id() == Some(file_id)
                        && intent.kind() == OutboundIntentKind::DeleteOrTrashNode
                })
            })
            .await;
        }

        symlink("/tmp", harness.root.join("unsupported-link")).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let _ = harness.engine.poll_once().await;
            if harness
                .state
                .observation_issues(harness.scope.library_id())
                .await
                .unwrap()
                .iter()
                .any(|issue| issue.kind() == ObservationIssueKind::UnsupportedEntryType)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            harness
                .state
                .observation_issues(harness.scope.library_id())
                .await
                .unwrap()
                .iter()
                .any(|issue| issue.kind() == ObservationIssueKind::UnsupportedEntryType)
        );
        let _ = right;
        harness.close().await;
    }

    #[cfg(target_os = "linux")]
    async fn eventually(harness: &Harness, predicate: impl Fn(&[OutboundIntent]) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let _ = harness.engine.poll_once().await;
            let intents = harness.engine.list_pending_intents().await.unwrap();
            if predicate(&intents) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let intents = harness.engine.list_pending_intents().await.unwrap();
        panic!("condition not met; intents: {intents:?}");
    }
}
