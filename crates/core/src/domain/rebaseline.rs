//! Platform-neutral contracts for server-side logical sync bootstrap.
//!
//! These values describe a current logical namespace cut. They deliberately
//! cannot represent object-store identities, replica locators, filesystem
//! paths, staging handles, credentials, or historical version manifests.

use std::{fmt, str::FromStr};

use crate::{
    DeviceId, FileVersionId, LibraryId, LogicalName, NodeId, NodeKind, NodeState, Revision,
    Sequence, Sha256Digest, SyncBootstrapId, Timestamp, UserId,
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

#[cfg(test)]
mod tests {
    use super::{LogicalSnapshotNode, LogicalSnapshotNodeError, SyncBootstrapState};
    use crate::{FileVersionId, LogicalName, NodeId, NodeKind, NodeState, Revision, Sha256Digest};
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
}
