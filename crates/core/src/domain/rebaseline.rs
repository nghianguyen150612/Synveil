//! Platform-neutral contracts for server-side logical sync bootstrap.
//!
//! These values describe a current logical namespace cut. They deliberately
//! cannot represent object-store identities, replica locators, filesystem
//! paths, staging handles, credentials, or historical version manifests.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
};

use crate::{
    DeviceId, FileVersionId, LibraryId, LogicalName, NodeId, NodeKind, NodeState,
    RebaselineSnapshotId, Revision, Sequence, Sha256Digest, SyncBootstrapId, Timestamp, UserId,
};

/// Durable lifecycle for one bounded logical bootstrap session.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SyncBootstrapState {
    Open,
    Completed,
    Aborted,
    Expired,
}

impl SyncBootstrapState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::Completed => "COMPLETED",
            Self::Aborted => "ABORTED",
            Self::Expired => "EXPIRED",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncBootstrapStateParseError;

impl fmt::Display for SyncBootstrapStateParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("sync bootstrap state is unknown")
    }
}

impl std::error::Error for SyncBootstrapStateParseError {}

impl FromStr for SyncBootstrapState {
    type Err = SyncBootstrapStateParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "OPEN" => Ok(Self::Open),
            "COMPLETED" => Ok(Self::Completed),
            "ABORTED" => Ok(Self::Aborted),
            "EXPIRED" => Ok(Self::Expired),
            _ => Err(SyncBootstrapStateParseError),
        }
    }
}

/// Durable identity and journal cut for one retryable bootstrap manifest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncBootstrap {
    id: SyncBootstrapId,
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
    generation: Sequence,
    snapshot_epoch: Sequence,
    snapshot_resume_sequence: Sequence,
    manifest_item_count: u64,
    terminal_node_id: Option<NodeId>,
    state: SyncBootstrapState,
    created_at: Timestamp,
    expires_at: Timestamp,
    completed_at: Option<Timestamp>,
}

impl SyncBootstrap {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        id: SyncBootstrapId,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        generation: Sequence,
        snapshot_epoch: Sequence,
        snapshot_resume_sequence: Sequence,
        manifest_item_count: u64,
        terminal_node_id: Option<NodeId>,
        state: SyncBootstrapState,
        created_at: Timestamp,
        expires_at: Timestamp,
        completed_at: Option<Timestamp>,
    ) -> Self {
        Self {
            id,
            owner_user_id,
            device_id,
            library_id,
            generation,
            snapshot_epoch,
            snapshot_resume_sequence,
            manifest_item_count,
            terminal_node_id,
            state,
            created_at,
            expires_at,
            completed_at,
        }
    }

    #[must_use]
    pub const fn id(self) -> SyncBootstrapId {
        self.id
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

    #[must_use]
    pub const fn generation(self) -> Sequence {
        self.generation
    }

    #[must_use]
    pub const fn snapshot_epoch(self) -> Sequence {
        self.snapshot_epoch
    }

    #[must_use]
    pub const fn snapshot_resume_sequence(self) -> Sequence {
        self.snapshot_resume_sequence
    }

    #[must_use]
    pub const fn manifest_item_count(self) -> u64 {
        self.manifest_item_count
    }

    #[must_use]
    pub const fn terminal_node_id(self) -> Option<NodeId> {
        self.terminal_node_id
    }

    #[must_use]
    pub const fn state(self) -> SyncBootstrapState {
        self.state
    }

    #[must_use]
    pub const fn created_at(self) -> Timestamp {
        self.created_at
    }

    #[must_use]
    pub const fn expires_at(self) -> Timestamp {
        self.expires_at
    }

    #[must_use]
    pub const fn completed_at(self) -> Option<Timestamp> {
        self.completed_at
    }
}

/// A current logical Node captured in an immutable bootstrap manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogicalSnapshotNode {
    node_id: NodeId,
    parent_node_id: Option<NodeId>,
    name: LogicalName,
    kind: NodeKind,
    state: NodeState,
    revision: Revision,
    current_version_id: Option<FileVersionId>,
    content_length: Option<u64>,
    content_sha256: Option<Sha256Digest>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogicalSnapshotNodeError {
    InternalState,
    InvalidRootShape,
    DirectoryHasContent,
    IncompleteContentMetadata,
}

impl fmt::Display for LogicalSnapshotNodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InternalState => "logical snapshot node has an internal-only state",
            Self::InvalidRootShape => "logical snapshot root shape is invalid",
            Self::DirectoryHasContent => "logical snapshot directory has file content",
            Self::IncompleteContentMetadata => {
                "logical snapshot file content metadata is incomplete"
            }
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for LogicalSnapshotNodeError {}

impl LogicalSnapshotNode {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        node_id: NodeId,
        parent_node_id: Option<NodeId>,
        name: LogicalName,
        kind: NodeKind,
        state: NodeState,
        revision: Revision,
        current_version_id: Option<FileVersionId>,
        content_length: Option<u64>,
        content_sha256: Option<Sha256Digest>,
    ) -> Result<Self, LogicalSnapshotNodeError> {
        if state == NodeState::Purging {
            return Err(LogicalSnapshotNodeError::InternalState);
        }
        if parent_node_id.is_none() && (kind != NodeKind::Directory || state != NodeState::Active) {
            return Err(LogicalSnapshotNodeError::InvalidRootShape);
        }
        if kind == NodeKind::Directory
            && (current_version_id.is_some()
                || content_length.is_some()
                || content_sha256.is_some())
        {
            return Err(LogicalSnapshotNodeError::DirectoryHasContent);
        }
        let has_complete_content =
            current_version_id.is_some() && content_length.is_some() && content_sha256.is_some();
        let has_no_content =
            current_version_id.is_none() && content_length.is_none() && content_sha256.is_none();
        if !has_complete_content && !has_no_content {
            return Err(LogicalSnapshotNodeError::IncompleteContentMetadata);
        }

        Ok(Self {
            node_id,
            parent_node_id,
            name,
            kind,
            state,
            revision,
            current_version_id,
            content_length,
            content_sha256,
        })
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
    pub const fn name(&self) -> &LogicalName {
        &self.name
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
}

/// A typed keyset position inside one durable rebaseline snapshot artifact.
///
/// This is intentionally distinct from `JournalCursor`: it identifies the
/// immutable `NodeId` immediately before the next snapshot page, while the
/// journal cursor identifies the incremental-change continuation boundary.
/// The artifact identity binds a continuation token to one snapshot and makes
/// cross-artifact page mixing a typed validation error at the metadata seam.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RebaselineSnapshotPageCursor {
    snapshot_id: RebaselineSnapshotId,
    after_node_id: NodeId,
}

impl RebaselineSnapshotPageCursor {
    #[must_use]
    pub const fn new(snapshot_id: RebaselineSnapshotId, after_node_id: NodeId) -> Self {
        Self {
            snapshot_id,
            after_node_id,
        }
    }

    #[must_use]
    pub const fn snapshot_id(self) -> RebaselineSnapshotId {
        self.snapshot_id
    }

    #[must_use]
    pub const fn after_node_id(self) -> NodeId {
        self.after_node_id
    }
}

/// A complete, owner-independent logical view of one library namespace.
///
/// The journal boundary is intentionally not part of this core value: the
/// metadata adapter pairs this library-scoped state with its canonical
/// `JournalHighWatermark`. Keeping the state aggregate independent of the
/// adapter prevents a device or transport identity from becoming part of the
/// logical snapshot content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogicalSnapshot {
    library_id: LibraryId,
    entries: Vec<LogicalSnapshotNode>,
}

/// Structural validation failures for a complete logical snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogicalSnapshotError {
    Empty,
    DuplicateNodeId,
    MissingRoot,
    MultipleRoots,
    RootNotDirectory,
    RootNotActive,
    MissingParent,
    ParentNotDirectory,
    ParentCycle,
}

impl fmt::Display for LogicalSnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Empty => "logical snapshot has no root or entries",
            Self::DuplicateNodeId => "logical snapshot contains a duplicate node identity",
            Self::MissingRoot => "logical snapshot has no root entry",
            Self::MultipleRoots => "logical snapshot contains multiple root entries",
            Self::RootNotDirectory => "logical snapshot root is not a directory",
            Self::RootNotActive => "logical snapshot root is not active",
            Self::MissingParent => "logical snapshot contains a missing parent entry",
            Self::ParentNotDirectory => "logical snapshot parent entry is not a directory",
            Self::ParentCycle => "logical snapshot parent topology contains a cycle",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for LogicalSnapshotError {}

impl LogicalSnapshot {
    /// Validate and canonicalize a complete logical namespace.
    ///
    /// Entries are ordered by immutable `NodeId`; mutable names, paths, and
    /// timestamps never determine snapshot pagination or equality. A complete
    /// snapshot contains exactly one active directory root, and every other
    /// entry must resolve through directory parents to that root. Trashed
    /// entries remain valid current logical state; purged/internal `PURGING`
    /// entries cannot be constructed as `LogicalSnapshotNode` values.
    pub fn new(
        library_id: LibraryId,
        mut entries: Vec<LogicalSnapshotNode>,
    ) -> Result<Self, LogicalSnapshotError> {
        if entries.is_empty() {
            return Err(LogicalSnapshotError::Empty);
        }

        entries.sort_unstable_by_key(LogicalSnapshotNode::node_id);
        let mut topology = BTreeMap::new();
        for entry in &entries {
            if topology
                .insert(entry.node_id(), (entry.kind(), entry.parent_node_id()))
                .is_some()
            {
                return Err(LogicalSnapshotError::DuplicateNodeId);
            }
        }

        let mut roots = entries
            .iter()
            .filter(|entry| entry.parent_node_id().is_none());
        let Some(root) = roots.next() else {
            return Err(LogicalSnapshotError::MissingRoot);
        };
        if roots.next().is_some() {
            return Err(LogicalSnapshotError::MultipleRoots);
        }
        if root.kind() != NodeKind::Directory {
            return Err(LogicalSnapshotError::RootNotDirectory);
        }
        if root.state() != NodeState::Active {
            return Err(LogicalSnapshotError::RootNotActive);
        }

        for entry in &entries {
            let mut parent_id = entry.parent_node_id();
            let mut seen = BTreeSet::new();
            while let Some(current_id) = parent_id {
                if !seen.insert(current_id) {
                    return Err(LogicalSnapshotError::ParentCycle);
                }
                let Some((kind, next_parent_id)) = topology.get(&current_id) else {
                    return Err(LogicalSnapshotError::MissingParent);
                };
                if *kind != NodeKind::Directory {
                    return Err(LogicalSnapshotError::ParentNotDirectory);
                }
                parent_id = *next_parent_id;
            }
        }

        Ok(Self {
            library_id,
            entries,
        })
    }

    #[must_use]
    pub const fn library_id(&self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub fn entries(&self) -> &[LogicalSnapshotNode] {
        &self.entries
    }

    #[must_use]
    pub fn root(&self) -> &LogicalSnapshotNode {
        self.entries
            .iter()
            .find(|entry| entry.parent_node_id().is_none())
            .expect("LogicalSnapshot always contains exactly one root")
    }

    #[must_use]
    pub fn root_node_id(&self) -> NodeId {
        self.root().node_id()
    }

    #[must_use]
    pub fn into_entries(self) -> Vec<LogicalSnapshotNode> {
        self.entries
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LogicalSnapshot, LogicalSnapshotError, LogicalSnapshotNode, LogicalSnapshotNodeError,
        RebaselineSnapshotPageCursor, SyncBootstrapState,
    };
    use crate::{
        FileVersionId, LogicalName, NodeId, NodeKind, NodeState, RebaselineSnapshotId, Revision,
        Sha256Digest,
    };
    use std::str::FromStr;

    #[test]
    fn bootstrap_states_are_closed_and_canonical() {
        for state in [
            SyncBootstrapState::Open,
            SyncBootstrapState::Completed,
            SyncBootstrapState::Aborted,
            SyncBootstrapState::Expired,
        ] {
            assert_eq!(SyncBootstrapState::from_str(state.as_str()), Ok(state));
        }
        assert!(SyncBootstrapState::from_str("BUILDING").is_err());
    }

    #[test]
    fn logical_snapshot_projection_rejects_internal_or_incomplete_shapes() {
        let name = LogicalName::new("file.txt").unwrap();
        assert_eq!(
            LogicalSnapshotNode::new(
                NodeId::new(),
                Some(NodeId::new()),
                name.clone(),
                NodeKind::File,
                NodeState::Purging,
                Revision::new(1),
                None,
                None,
                None,
            ),
            Err(LogicalSnapshotNodeError::InternalState)
        );
        assert_eq!(
            LogicalSnapshotNode::new(
                NodeId::new(),
                Some(NodeId::new()),
                name,
                NodeKind::File,
                NodeState::Active,
                Revision::new(1),
                Some(FileVersionId::new()),
                Some(4),
                None,
            ),
            Err(LogicalSnapshotNodeError::IncompleteContentMetadata)
        );
    }

    #[test]
    fn logical_snapshot_file_carries_only_safe_current_content_metadata() {
        let version_id = FileVersionId::new();
        let digest = Sha256Digest::from_bytes([0x5a; 32]);
        let node = LogicalSnapshotNode::new(
            NodeId::new(),
            Some(NodeId::new()),
            LogicalName::new("file.txt").unwrap(),
            NodeKind::File,
            NodeState::Trashed,
            Revision::new(9),
            Some(version_id),
            Some(42),
            Some(digest),
        )
        .unwrap();

        assert_eq!(node.current_version_id(), Some(version_id));
        assert_eq!(node.content_length(), Some(42));
        assert_eq!(node.content_sha256(), Some(digest));
    }

    #[test]
    fn durable_page_cursor_is_scoped_to_one_snapshot_and_node_position() {
        let snapshot_id = RebaselineSnapshotId::new();
        let node_id = NodeId::new();
        let cursor = RebaselineSnapshotPageCursor::new(snapshot_id, node_id);

        assert_eq!(cursor.snapshot_id(), snapshot_id);
        assert_eq!(cursor.after_node_id(), node_id);
    }

    fn snapshot_node(
        node_id: NodeId,
        parent_node_id: Option<NodeId>,
        kind: NodeKind,
        state: NodeState,
        name: &str,
    ) -> LogicalSnapshotNode {
        LogicalSnapshotNode::new(
            node_id,
            parent_node_id,
            LogicalName::new(name).unwrap(),
            kind,
            state,
            Revision::new(0),
            None,
            None,
            None,
        )
        .unwrap()
    }

    #[test]
    fn logical_snapshot_canonicalizes_node_id_order_and_keeps_trashed_state() {
        let library_id = crate::LibraryId::new();
        let root_id = NodeId::new();
        let directory_id = NodeId::new();
        let file_id = NodeId::new();
        let root = snapshot_node(
            root_id,
            None,
            NodeKind::Directory,
            NodeState::Active,
            "root",
        );
        let directory = snapshot_node(
            directory_id,
            Some(root_id),
            NodeKind::Directory,
            NodeState::Active,
            "directory",
        );
        let file = snapshot_node(
            file_id,
            Some(directory_id),
            NodeKind::File,
            NodeState::Trashed,
            "file",
        );

        let snapshot = LogicalSnapshot::new(library_id, vec![file.clone(), root, directory])
            .expect("complete topology must be accepted");

        assert_eq!(snapshot.library_id(), library_id);
        assert_eq!(snapshot.len(), 3);
        assert!(!snapshot.is_empty());
        assert!(
            snapshot
                .entries()
                .windows(2)
                .all(|window| window[0].node_id() < window[1].node_id())
        );
        assert_eq!(
            snapshot
                .entries()
                .iter()
                .find(|entry| entry.node_id() == file_id)
                .map(LogicalSnapshotNode::state),
            Some(NodeState::Trashed)
        );
    }

    #[test]
    fn logical_snapshot_rejects_incomplete_or_cyclic_topology() {
        let library_id = crate::LibraryId::new();
        let root_id = NodeId::new();
        let root = snapshot_node(
            root_id,
            None,
            NodeKind::Directory,
            NodeState::Active,
            "root",
        );

        assert_eq!(
            LogicalSnapshot::new(library_id, Vec::new()),
            Err(LogicalSnapshotError::Empty)
        );
        assert_eq!(
            LogicalSnapshot::new(
                library_id,
                vec![snapshot_node(
                    NodeId::new(),
                    Some(NodeId::new()),
                    NodeKind::File,
                    NodeState::Active,
                    "orphan",
                )],
            ),
            Err(LogicalSnapshotError::MissingRoot)
        );

        let duplicate = snapshot_node(
            NodeId::new(),
            Some(root_id),
            NodeKind::File,
            NodeState::Active,
            "duplicate",
        );
        assert_eq!(
            LogicalSnapshot::new(library_id, vec![root.clone(), duplicate.clone(), duplicate]),
            Err(LogicalSnapshotError::DuplicateNodeId)
        );

        let second_root = snapshot_node(
            NodeId::new(),
            None,
            NodeKind::Directory,
            NodeState::Active,
            "second-root",
        );
        assert_eq!(
            LogicalSnapshot::new(library_id, vec![root.clone(), second_root]),
            Err(LogicalSnapshotError::MultipleRoots)
        );

        let file_parent_id = NodeId::new();
        let child_id = NodeId::new();
        let file_parent = snapshot_node(
            file_parent_id,
            Some(root_id),
            NodeKind::File,
            NodeState::Active,
            "file-parent",
        );
        let child_of_file = snapshot_node(
            child_id,
            Some(file_parent_id),
            NodeKind::File,
            NodeState::Active,
            "child-of-file",
        );
        assert_eq!(
            LogicalSnapshot::new(library_id, vec![root.clone(), file_parent, child_of_file]),
            Err(LogicalSnapshotError::ParentNotDirectory)
        );

        let cycle_a_id = NodeId::new();
        let cycle_b_id = NodeId::new();
        let cycle_a = snapshot_node(
            cycle_a_id,
            Some(cycle_b_id),
            NodeKind::Directory,
            NodeState::Active,
            "cycle-a",
        );
        let cycle_b = snapshot_node(
            cycle_b_id,
            Some(cycle_a_id),
            NodeKind::Directory,
            NodeState::Active,
            "cycle-b",
        );
        assert_eq!(
            LogicalSnapshot::new(library_id, vec![root, cycle_a, cycle_b]),
            Err(LogicalSnapshotError::ParentCycle)
        );
    }
}
