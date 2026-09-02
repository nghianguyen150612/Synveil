//! Platform-neutral contracts for the durable metadata change journal.
//!
//! These are resulting facts about committed logical state. They intentionally
//! contain no object-store key, filesystem path, replica locator, or arbitrary
//! payload. The PostgreSQL adapter owns persistence and cursor transport.

use std::{fmt, str::FromStr};

use crate::{
    ChangeEventId, FileVersionId, LibraryId, NodeId, NodeKind, NodeState, Revision, Sequence,
    Timestamp, UserId,
};

/// The logical resource families that may appear in the initial journal.
///
/// The vocabulary is deliberately closed so callers cannot turn the journal
/// into an arbitrary string event bus.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ChangeResourceKind {
    Node,
}

impl ChangeResourceKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Node => "NODE",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChangeResourceKindParseError;

impl fmt::Display for ChangeResourceKindParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("change resource kind is unknown")
    }
}

impl std::error::Error for ChangeResourceKindParseError {}

impl FromStr for ChangeResourceKind {
    type Err = ChangeResourceKindParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "NODE" => Ok(Self::Node),
            _ => Err(ChangeResourceKindParseError),
        }
    }
}

/// User-visible logical mutations represented by Prompt 31.
///
/// Maintenance transitions such as GC leases and replica deletion attempts
/// are intentionally absent. They are not synchronization facts.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ChangeKind {
    NodeCreated,
    NodeRenamed,
    NodeMoved,
    NodeTrashed,
    NodeRestored,
    FileContentCommitted,
    FileVersionRestored,
    NodePurged,
}

impl ChangeKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NodeCreated => "NODE_CREATED",
            Self::NodeRenamed => "NODE_RENAMED",
            Self::NodeMoved => "NODE_MOVED",
            Self::NodeTrashed => "NODE_TRASHED",
            Self::NodeRestored => "NODE_RESTORED",
            Self::FileContentCommitted => "FILE_CONTENT_COMMITTED",
            Self::FileVersionRestored => "FILE_VERSION_RESTORED",
            Self::NodePurged => "NODE_PURGED",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChangeKindParseError;

impl fmt::Display for ChangeKindParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("change kind is unknown")
    }
}

impl std::error::Error for ChangeKindParseError {}

impl FromStr for ChangeKind {
    type Err = ChangeKindParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "NODE_CREATED" => Ok(Self::NodeCreated),
            "NODE_RENAMED" => Ok(Self::NodeRenamed),
            "NODE_MOVED" => Ok(Self::NodeMoved),
            "NODE_TRASHED" => Ok(Self::NodeTrashed),
            "NODE_RESTORED" => Ok(Self::NodeRestored),
            "FILE_CONTENT_COMMITTED" => Ok(Self::FileContentCommitted),
            "FILE_VERSION_RESTORED" => Ok(Self::FileVersionRestored),
            "NODE_PURGED" => Ok(Self::NodePurged),
            _ => Err(ChangeKindParseError),
        }
    }
}

/// A durable resulting fact about one committed logical Node mutation.
///
/// `sequence` is meaningful only together with `library_id` and
/// `journal_epoch`. The optional projection fields are compact and bounded;
/// consumers can refetch canonical metadata by `resource_id` when they need
/// more detail. No physical storage identity is represented here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChangeEvent {
    id: ChangeEventId,
    owner_user_id: UserId,
    library_id: LibraryId,
    journal_epoch: Sequence,
    sequence: Sequence,
    schema_version: u16,
    resource_kind: ChangeResourceKind,
    resource_id: NodeId,
    change_kind: ChangeKind,
    occurred_at: Timestamp,
    resource_revision: Revision,
    parent_node_id: Option<NodeId>,
    node_kind: Option<NodeKind>,
    node_state: Option<NodeState>,
    current_version_id: Option<FileVersionId>,
}

impl ChangeEvent {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        id: ChangeEventId,
        owner_user_id: UserId,
        library_id: LibraryId,
        journal_epoch: Sequence,
        sequence: Sequence,
        schema_version: u16,
        resource_kind: ChangeResourceKind,
        resource_id: NodeId,
        change_kind: ChangeKind,
        occurred_at: Timestamp,
        resource_revision: Revision,
        parent_node_id: Option<NodeId>,
        node_kind: Option<NodeKind>,
        node_state: Option<NodeState>,
        current_version_id: Option<FileVersionId>,
    ) -> Self {
        Self {
            id,
            owner_user_id,
            library_id,
            journal_epoch,
            sequence,
            schema_version,
            resource_kind,
            resource_id,
            change_kind,
            occurred_at,
            resource_revision,
            parent_node_id,
            node_kind,
            node_state,
            current_version_id,
        }
    }

    #[must_use]
    pub const fn id(self) -> ChangeEventId {
        self.id
    }

    #[must_use]
    pub const fn owner_user_id(self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn library_id(self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn journal_epoch(self) -> Sequence {
        self.journal_epoch
    }

    #[must_use]
    pub const fn sequence(self) -> Sequence {
        self.sequence
    }

    #[must_use]
    pub const fn schema_version(self) -> u16 {
        self.schema_version
    }

    #[must_use]
    pub const fn resource_kind(self) -> ChangeResourceKind {
        self.resource_kind
    }

    #[must_use]
    pub const fn resource_id(self) -> NodeId {
        self.resource_id
    }

    #[must_use]
    pub const fn change_kind(self) -> ChangeKind {
        self.change_kind
    }

    #[must_use]
    pub const fn occurred_at(self) -> Timestamp {
        self.occurred_at
    }

    #[must_use]
    pub const fn resource_revision(self) -> Revision {
        self.resource_revision
    }

    #[must_use]
    pub const fn parent_node_id(self) -> Option<NodeId> {
        self.parent_node_id
    }

    #[must_use]
    pub const fn node_kind(self) -> Option<NodeKind> {
        self.node_kind
    }

    #[must_use]
    pub const fn node_state(self) -> Option<NodeState> {
        self.node_state
    }

    #[must_use]
    pub const fn current_version_id(self) -> Option<FileVersionId> {
        self.current_version_id
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{ChangeKind, ChangeResourceKind};

    #[test]
    fn change_kinds_are_typed_and_canonical() {
        for (value, expected) in [
            ("NODE_CREATED", ChangeKind::NodeCreated),
            ("NODE_RENAMED", ChangeKind::NodeRenamed),
            ("NODE_MOVED", ChangeKind::NodeMoved),
            ("NODE_TRASHED", ChangeKind::NodeTrashed),
            ("NODE_RESTORED", ChangeKind::NodeRestored),
            ("FILE_CONTENT_COMMITTED", ChangeKind::FileContentCommitted),
            ("FILE_VERSION_RESTORED", ChangeKind::FileVersionRestored),
            ("NODE_PURGED", ChangeKind::NodePurged),
        ] {
            assert_eq!(ChangeKind::from_str(value), Ok(expected));
            assert_eq!(expected.as_str(), value);
        }
        assert!(ChangeKind::from_str("GC_RETRY").is_err());
        assert_eq!(ChangeResourceKind::Node.as_str(), "NODE");
    }
}
