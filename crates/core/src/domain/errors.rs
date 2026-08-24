use std::fmt;

use super::models::{DeviceStatus, NodeState};

/// Validation failures for the persistence-independent domain model.
///
/// These errors intentionally do not carry rejected user values. Adapters may
/// map them to their own transport or persistence error contracts later.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainError {
    EmptyName,
    NameTooLong,
    EmptyLoginIdentifier,
    LoginIdentifierTooLong,
    EmptyLoginKey,
    LoginKeyTooLong,
    RootMustBeDirectory,
    RootMustBeActive,
    RootCannotHaveParent,
    RootCannotBeDeleted,
    RootNodeMismatch,
    ParentMustBeDirectory,
    ParentMustBeActive,
    ParentLibraryMismatch,
    ParentReferenceMismatch,
    ParentCannotBeSelf,
    ParentChainMismatch,
    ParentCycle,
    DirectoryNotEmpty,
    LibraryNotWritable,
    NodeLibraryMismatch,
    DirectoryCannotReferenceFileVersion,
    FileVersionRequiresFileNode,
    FileVersionLibraryMismatch,
    FileVersionNodeMismatch,
    ObjectDedupDomainMismatch,
    FileVersionCannotParentItself,
    InvalidNodeStateTransition {
        from: NodeState,
        to: NodeState,
    },
    InvalidDeviceStateTransition {
        from: DeviceStatus,
        to: DeviceStatus,
    },
    RevisionOverflow,
}

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyName => "logical name is empty",
            Self::NameTooLong => "logical name exceeds the domain length bound",
            Self::EmptyLoginIdentifier => "login identifier is empty",
            Self::LoginIdentifierTooLong => "login identifier exceeds the domain length bound",
            Self::EmptyLoginKey => "login uniqueness key is empty",
            Self::LoginKeyTooLong => "login uniqueness key exceeds the domain length bound",
            Self::RootMustBeDirectory => "library root must be a directory",
            Self::RootMustBeActive => "library root must be active",
            Self::RootCannotHaveParent => "library root cannot have a parent",
            Self::RootCannotBeDeleted => "library root cannot be logically deleted",
            Self::RootNodeMismatch => "library root does not match the supplied node",
            Self::ParentMustBeDirectory => "node parent must be a directory",
            Self::ParentMustBeActive => "node parent must be active",
            Self::ParentLibraryMismatch => "node and parent belong to different libraries",
            Self::ParentReferenceMismatch => {
                "node parent reference does not match the supplied parent"
            }
            Self::ParentCannotBeSelf => "node cannot be its own parent",
            Self::ParentChainMismatch => "node parent chain is inconsistent",
            Self::ParentCycle => "node parent chain contains a cycle",
            Self::DirectoryNotEmpty => "directory is not empty",
            Self::LibraryNotWritable => "library does not accept metadata writes",
            Self::NodeLibraryMismatch => "node belongs to a different library",
            Self::DirectoryCannotReferenceFileVersion => {
                "directory cannot reference a file version"
            }
            Self::FileVersionRequiresFileNode => "file version requires a file node",
            Self::FileVersionLibraryMismatch => "file version library does not match the node",
            Self::FileVersionNodeMismatch => "file version node does not match the node",
            Self::ObjectDedupDomainMismatch => {
                "object reference belongs to a different deduplication domain"
            }
            Self::FileVersionCannotParentItself => "file version cannot parent itself",
            Self::InvalidNodeStateTransition { .. } => "node state transition is not allowed",
            Self::InvalidDeviceStateTransition { .. } => "device state transition is not allowed",
            Self::RevisionOverflow => "resource revision cannot be incremented",
        };

        formatter.write_str(message)
    }
}

impl std::error::Error for DomainError {}
