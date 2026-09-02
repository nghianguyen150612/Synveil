use std::fmt;

use super::backup::{BackupMaintenanceRunState, BackupSetState, SnapshotState};
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
    InvalidTrashTimestamp,
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
    InvalidBackupSetStateTransition {
        from: BackupSetState,
        to: BackupSetState,
    },
    InvalidSnapshotStateTransition {
        from: SnapshotState,
        to: SnapshotState,
    },
    BackupManifestInvalidNodeState,
    BackupManifestInvalidRootShape,
    BackupManifestDirectoryHasContent,
    BackupManifestIncompleteContentMetadata,
    BackupRestoreInvalidPlanState,
    BackupRestoreInvalidPlanEntry,
    BackupPruneInvalidPlanState,
    BackupPruneInvalidPlanEntry,
    BackupRetentionPolicyInvalidConfig,
    BackupRetentionPolicyInvalidRevision,
    BackupSnapshotExpiryInvalidPlanState,
    BackupSnapshotExpiryInvalidPlanEntry,
    BackupSnapshotExpiryInvalidExecution,
    BackupMaintenanceRunInvalidState,
    InvalidBackupMaintenanceRunStateTransition {
        from: BackupMaintenanceRunState,
        to: BackupMaintenanceRunState,
    },
    BackupSetSourceMismatch,
    BackupSnapshotForeignKeyMismatch,
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
            Self::InvalidTrashTimestamp => "node trash timestamp does not match its state",
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
            Self::InvalidBackupSetStateTransition { .. } => {
                "backup set state transition is not allowed"
            }
            Self::InvalidSnapshotStateTransition { .. } => {
                "snapshot state transition is not allowed"
            }
            Self::BackupManifestInvalidNodeState => {
                "backup snapshot manifest contains a purging node"
            }
            Self::BackupManifestInvalidRootShape => {
                "backup snapshot manifest root shape is invalid"
            }
            Self::BackupManifestDirectoryHasContent => {
                "backup snapshot manifest directory has file content"
            }
            Self::BackupManifestIncompleteContentMetadata => {
                "backup snapshot manifest file content metadata is incomplete"
            }
            Self::BackupRestoreInvalidPlanState => {
                "backup restore plan state or counts are invalid"
            }
            Self::BackupRestoreInvalidPlanEntry => "backup restore plan entry shape is invalid",
            Self::BackupPruneInvalidPlanState => {
                "backup prune plan state or reference-accounting counts are invalid"
            }
            Self::BackupPruneInvalidPlanEntry => "backup prune plan entry shape is invalid",
            Self::BackupRetentionPolicyInvalidConfig => {
                "backup snapshot retention policy configuration is invalid"
            }
            Self::BackupRetentionPolicyInvalidRevision => {
                "backup snapshot retention policy revision is invalid"
            }
            Self::BackupSnapshotExpiryInvalidPlanState => {
                "backup snapshot expiry plan state or counts are invalid"
            }
            Self::BackupSnapshotExpiryInvalidPlanEntry => {
                "backup snapshot expiry plan entry shape is invalid"
            }
            Self::BackupSnapshotExpiryInvalidExecution => {
                "backup snapshot expiry execution evidence is invalid"
            }
            Self::BackupMaintenanceRunInvalidState => {
                "backup maintenance run state or references are invalid"
            }
            Self::InvalidBackupMaintenanceRunStateTransition { .. } => {
                "backup maintenance run state transition is not allowed"
            }
            Self::BackupSetSourceMismatch => {
                "backup set source does not match the requested library scope"
            }
            Self::BackupSnapshotForeignKeyMismatch => {
                "backup snapshot foreign key relationship is inconsistent"
            }
            Self::RevisionOverflow => "resource revision cannot be incremented",
        };

        formatter.write_str(message)
    }
}

impl std::error::Error for DomainError {}
