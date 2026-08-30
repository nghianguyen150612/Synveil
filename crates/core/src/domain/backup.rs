//! Platform-neutral contracts for the durable backup domain.
//!
//! Backup snapshots are point-in-time immutable captures of a library's logical
//! state. Once committed, a snapshot's manifest is immutable: changing or
//! deleting live files never mutates historical backup contents.
//!
//! Physical object references (`ObjectId`, `ObjectReplicaId`, storage keys)
//! remain server-internal and never cross this boundary.

use std::{fmt, str::FromStr};

use sha2::{Digest, Sha256};

use crate::{
    BackupMaintenanceRunId, BackupPruneExecutionId, BackupPrunePlanId, BackupRestoreExecutionId,
    BackupRestorePlanId, BackupSetId, BackupSnapshotExpiryExecutionId, BackupSnapshotExpiryPlanId,
    BackupSnapshotRetentionPolicyRevisionId, FileVersionId, LibraryId, LogicalName, NodeId,
    NodeKind, NodeState, Revision, Sequence, Sha256Digest, SnapshotId, Timestamp, UserId,
};

use super::errors::DomainError;

/// Identifies what is being backed up. The initial bounded implementation
/// supports only library snapshots. Future extensions may add external
/// directories, volumes, or device roots without changing the schema shape.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BackupSource {
    Library,
}

impl BackupSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Library => "LIBRARY",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupSourceParseError;

impl fmt::Display for BackupSourceParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("backup source kind is unknown")
    }
}

impl std::error::Error for BackupSourceParseError {}

impl FromStr for BackupSource {
    type Err = BackupSourceParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "LIBRARY" => Ok(Self::Library),
            _ => Err(BackupSourceParseError),
        }
    }
}

/// Durable lifecycle for one backup set.
///
/// Transitions:
///   `Created` -> `Active` -> `Disabled`
///   `Created` -> `Disabled`
///
/// Once disabled, a backup set cannot be re-enabled. Old snapshots remain
/// retained until an explicit expiry execution changes their lifecycle.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BackupSetState {
    Created,
    Active,
    Disabled,
}

impl BackupSetState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "CREATED",
            Self::Active => "ACTIVE",
            Self::Disabled => "DISABLED",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupSetStateParseError;

impl fmt::Display for BackupSetStateParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("backup set state is unknown")
    }
}

impl std::error::Error for BackupSetStateParseError {}

impl FromStr for BackupSetState {
    type Err = BackupSetStateParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "CREATED" => Ok(Self::Created),
            "ACTIVE" => Ok(Self::Active),
            "DISABLED" => Ok(Self::Disabled),
            _ => Err(BackupSetStateParseError),
        }
    }
}

/// Durable lifecycle for one backup snapshot.
///
/// Transitions:
///   `Building` -> `Completed`
///   `Building` -> `Failed`
///   `Completed` -> `Expired`
///
/// `Building` snapshots are not restorable. Only `Completed` snapshots are
/// eligible for future restore operations. `Expired` is a terminal retention
/// lifecycle state; a future pruning worker may later physically delete the row
/// and its manifest via `ON DELETE CASCADE`, but Prompt 41 does not perform that
/// deletion.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SnapshotState {
    Building,
    Completed,
    Failed,
    Expired,
}

impl SnapshotState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Building => "BUILDING",
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
            Self::Expired => "EXPIRED",
        }
    }

    /// Returns `true` when the snapshot is eligible for restore.
    #[must_use]
    pub const fn is_restorable(self) -> bool {
        matches!(self, Self::Completed)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotStateParseError;

impl fmt::Display for SnapshotStateParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("snapshot state is unknown")
    }
}

impl std::error::Error for SnapshotStateParseError {}

impl FromStr for SnapshotState {
    type Err = SnapshotStateParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "BUILDING" => Ok(Self::Building),
            "COMPLETED" => Ok(Self::Completed),
            "FAILED" => Ok(Self::Failed),
            "EXPIRED" => Ok(Self::Expired),
            _ => Err(SnapshotStateParseError),
        }
    }
}

/// A named, persistent backup configuration scoped to one owner and one
/// source library. The backup set owns the retention metadata that later
/// workers use to decide which snapshots to keep or expire.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupSet {
    id: BackupSetId,
    owner_user_id: UserId,
    name: LogicalName,
    source_library_id: LibraryId,
    source: BackupSource,
    retention_days: Option<u32>,
    state: BackupSetState,
    created_at: Timestamp,
    updated_at: Timestamp,
    revision: Revision,
}

impl BackupSet {
    pub fn new(
        id: BackupSetId,
        owner_user_id: UserId,
        name: LogicalName,
        source_library_id: LibraryId,
        retention_days: Option<u32>,
        observed_at: Timestamp,
    ) -> Result<Self, DomainError> {
        Self::rehydrate(
            id,
            owner_user_id,
            name,
            source_library_id,
            retention_days,
            BackupSetState::Created,
            observed_at,
            observed_at,
            Revision::new(0),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rehydrate(
        id: BackupSetId,
        owner_user_id: UserId,
        name: LogicalName,
        source_library_id: LibraryId,
        retention_days: Option<u32>,
        state: BackupSetState,
        created_at: Timestamp,
        updated_at: Timestamp,
        revision: Revision,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            id,
            owner_user_id,
            name,
            source_library_id,
            source: BackupSource::Library,
            retention_days,
            state,
            created_at,
            updated_at,
            revision,
        })
    }

    pub fn activate(&mut self, observed_at: Timestamp) -> Result<(), DomainError> {
        if self.state != BackupSetState::Created {
            return Err(DomainError::InvalidBackupSetStateTransition {
                from: self.state,
                to: BackupSetState::Active,
            });
        }
        self.state = BackupSetState::Active;
        self.bump_revision(observed_at);
        Ok(())
    }

    pub fn disable(&mut self, observed_at: Timestamp) -> Result<(), DomainError> {
        let allowed = matches!(self.state, BackupSetState::Created | BackupSetState::Active);
        if !allowed {
            return Err(DomainError::InvalidBackupSetStateTransition {
                from: self.state,
                to: BackupSetState::Disabled,
            });
        }
        self.state = BackupSetState::Disabled;
        self.bump_revision(observed_at);
        Ok(())
    }

    #[must_use]
    pub const fn id(&self) -> BackupSetId {
        self.id
    }

    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn name(&self) -> &LogicalName {
        &self.name
    }

    #[must_use]
    pub const fn source_library_id(&self) -> LibraryId {
        self.source_library_id
    }

    #[must_use]
    pub const fn source(&self) -> BackupSource {
        self.source
    }

    #[must_use]
    pub const fn retention_days(&self) -> Option<u32> {
        self.retention_days
    }

    #[must_use]
    pub const fn state(&self) -> BackupSetState {
        self.state
    }

    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    #[must_use]
    pub const fn updated_at(&self) -> Timestamp {
        self.updated_at
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    fn bump_revision(&mut self, observed_at: Timestamp) {
        self.revision = Revision::new(self.revision.get() + 1);
        self.updated_at = observed_at;
    }
}

/// An immutable point-in-time capture of a backup set's source library.
///
/// `snapshot_epoch` and `snapshot_resume_sequence` record the change-journal
/// position at which the manifest was cut. The manifest itself is represented
/// by [`BackupSnapshotNode`] rows and is immutable once the snapshot leaves the
/// `BUILDING` state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupSnapshot {
    id: SnapshotId,
    backup_set_id: BackupSetId,
    owner_user_id: UserId,
    source_library_id: LibraryId,
    snapshot_epoch: Sequence,
    snapshot_resume_sequence: Sequence,
    manifest_item_count: u64,
    terminal_node_id: Option<NodeId>,
    content_reference_count: u64,
    state: SnapshotState,
    created_at: Timestamp,
    committed_at: Option<Timestamp>,
    expired_at: Option<Timestamp>,
}

impl BackupSnapshot {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        id: SnapshotId,
        backup_set_id: BackupSetId,
        owner_user_id: UserId,
        source_library_id: LibraryId,
        snapshot_epoch: Sequence,
        snapshot_resume_sequence: Sequence,
        manifest_item_count: u64,
        terminal_node_id: Option<NodeId>,
        content_reference_count: u64,
        state: SnapshotState,
        created_at: Timestamp,
        committed_at: Option<Timestamp>,
        expired_at: Option<Timestamp>,
    ) -> Self {
        Self {
            id,
            backup_set_id,
            owner_user_id,
            source_library_id,
            snapshot_epoch,
            snapshot_resume_sequence,
            manifest_item_count,
            terminal_node_id,
            content_reference_count,
            state,
            created_at,
            committed_at,
            expired_at,
        }
    }

    /// Transition the snapshot lifecycle through the closed domain machine.
    ///
    /// Allowed:
    /// - `Building` -> `Completed` | `Failed`
    /// - `Completed` -> `Expired`
    ///
    /// A `Completed` snapshot is restorable. `Expired` and `Failed` are terminal.
    pub fn transition_state(
        &mut self,
        next: SnapshotState,
        observed_at: Timestamp,
    ) -> Result<(), DomainError> {
        let allowed = matches!(
            (self.state, next),
            (
                SnapshotState::Building,
                SnapshotState::Completed | SnapshotState::Failed
            ) | (SnapshotState::Completed, SnapshotState::Expired)
        );
        if !allowed {
            return Err(DomainError::InvalidSnapshotStateTransition {
                from: self.state,
                to: next,
            });
        }
        if self.state == next {
            return Ok(());
        }
        self.state = next;
        match next {
            SnapshotState::Completed => {
                self.committed_at = Some(observed_at);
            }
            SnapshotState::Failed => {}
            SnapshotState::Expired => {
                self.expired_at = Some(observed_at);
            }
            SnapshotState::Building => {}
        }
        Ok(())
    }

    #[must_use]
    pub const fn id(&self) -> SnapshotId {
        self.id
    }

    #[must_use]
    pub const fn backup_set_id(&self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn source_library_id(&self) -> LibraryId {
        self.source_library_id
    }

    #[must_use]
    pub const fn snapshot_epoch(&self) -> Sequence {
        self.snapshot_epoch
    }

    #[must_use]
    pub const fn snapshot_resume_sequence(&self) -> Sequence {
        self.snapshot_resume_sequence
    }

    #[must_use]
    pub const fn manifest_item_count(&self) -> u64 {
        self.manifest_item_count
    }

    #[must_use]
    pub const fn terminal_node_id(&self) -> Option<NodeId> {
        self.terminal_node_id
    }

    #[must_use]
    pub const fn content_reference_count(&self) -> u64 {
        self.content_reference_count
    }

    #[must_use]
    pub const fn state(&self) -> SnapshotState {
        self.state
    }

    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    #[must_use]
    pub const fn committed_at(&self) -> Option<Timestamp> {
        self.committed_at
    }

    #[must_use]
    pub const fn expired_at(&self) -> Option<Timestamp> {
        self.expired_at
    }

    #[must_use]
    pub const fn is_restorable(&self) -> bool {
        self.state.is_restorable()
    }
}

/// An immutable file content reference captured at snapshot commit time.
///
/// This is the logical content identity: length and SHA-256 hash. Physical
/// object references (`ObjectId`, `ObjectReplicaId`, storage keys) remain
/// server-internal and are never exposed through this boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupManifestContent {
    file_version_id: FileVersionId,
    byte_length: u64,
    sha256: Sha256Digest,
}

impl BackupManifestContent {
    #[must_use]
    pub const fn new(
        file_version_id: FileVersionId,
        byte_length: u64,
        sha256: Sha256Digest,
    ) -> Self {
        Self {
            file_version_id,
            byte_length,
            sha256,
        }
    }

    #[must_use]
    pub const fn file_version_id(&self) -> FileVersionId {
        self.file_version_id
    }

    #[must_use]
    pub const fn byte_length(&self) -> u64 {
        self.byte_length
    }

    #[must_use]
    pub const fn sha256(&self) -> Sha256Digest {
        self.sha256
    }
}

/// An immutable logical Node captured in a backup snapshot manifest.
///
/// Once the snapshot is committed, this row is never modified. Renames, moves,
/// trash/purge operations on the live library do not affect historical backup
/// manifest contents.
///
/// The type carries no `ObjectId`, `ObjectReplicaId`, storage key, backend
/// locator, staging handle, credential, or GC state. Content identity is
/// limited to the safe logical binding: `FileVersionId`, byte length, and
/// SHA-256 hash.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupSnapshotNode {
    node_id: NodeId,
    parent_node_id: Option<NodeId>,
    name: LogicalName,
    kind: NodeKind,
    state: NodeState,
    revision: Revision,
    content: Option<BackupManifestContent>,
    created_at: Timestamp,
    updated_at: Timestamp,
}

impl BackupSnapshotNode {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        node_id: NodeId,
        parent_node_id: Option<NodeId>,
        name: LogicalName,
        kind: NodeKind,
        state: NodeState,
        revision: Revision,
        content: Option<BackupManifestContent>,
        created_at: Timestamp,
        updated_at: Timestamp,
    ) -> Result<Self, DomainError> {
        if state == NodeState::Purging {
            return Err(DomainError::BackupManifestInvalidNodeState);
        }
        if parent_node_id.is_none() && (kind != NodeKind::Directory || state != NodeState::Active) {
            return Err(DomainError::BackupManifestInvalidRootShape);
        }
        if kind == NodeKind::Directory && content.is_some() {
            return Err(DomainError::BackupManifestDirectoryHasContent);
        }
        Ok(Self {
            node_id,
            parent_node_id,
            name,
            kind,
            state,
            revision,
            content,
            created_at,
            updated_at,
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
    pub const fn content(&self) -> Option<&BackupManifestContent> {
        self.content.as_ref()
    }

    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    #[must_use]
    pub const fn updated_at(&self) -> Timestamp {
        self.updated_at
    }
}

/// Version of the canonical semantic restore-plan request fingerprint.
pub const BACKUP_RESTORE_PLAN_FINGERPRINT_VERSION: u16 = 1;

/// Durable lifecycle for a restore plan. A plan is immutable provenance; only
/// the one-way stale or executed terminal transition is mutable.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BackupRestorePlanState {
    Planned,
    Stale,
    Executed,
}

impl BackupRestorePlanState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "PLANNED",
            Self::Stale => "STALE",
            Self::Executed => "EXECUTED",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupRestorePlanStateParseError;

impl fmt::Display for BackupRestorePlanStateParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("restore plan state is unknown")
    }
}

impl std::error::Error for BackupRestorePlanStateParseError {}

impl FromStr for BackupRestorePlanState {
    type Err = BackupRestorePlanStateParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "PLANNED" => Ok(Self::Planned),
            "STALE" => Ok(Self::Stale),
            "EXECUTED" => Ok(Self::Executed),
            _ => Err(BackupRestorePlanStateParseError),
        }
    }
}

/// The closed logical action vocabulary for a first restore plan. There is no
/// update, overwrite, merge, or delete action in this phase.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BackupRestoreAction {
    CreateDirectory,
    CreateFile,
}

impl BackupRestoreAction {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CreateDirectory => "CREATE_DIRECTORY",
            Self::CreateFile => "CREATE_FILE",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupRestoreActionParseError;

impl fmt::Display for BackupRestoreActionParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("restore plan action is unknown")
    }
}

impl std::error::Error for BackupRestoreActionParseError {}

impl FromStr for BackupRestoreAction {
    type Err = BackupRestoreActionParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "CREATE_DIRECTORY" => Ok(Self::CreateDirectory),
            "CREATE_FILE" => Ok(Self::CreateFile),
            _ => Err(BackupRestoreActionParseError),
        }
    }
}

/// Typed, owner-safe reasons why a restore preflight cannot proceed. These
/// values deliberately carry no physical Object or replica identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BackupRestorePreflightIssue {
    SnapshotNotRestorable,
    SnapshotCorrupt,
    InvalidTargetParent,
    DestinationNameConflict,
    TargetChanged,
    MissingRetentionReference,
    ContentUnavailable,
    TargetStorageIncompatible,
    IdempotencyConflict,
}

impl fmt::Display for BackupRestorePreflightIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::SnapshotNotRestorable => "backup snapshot is not restorable",
            Self::SnapshotCorrupt => "backup snapshot manifest is corrupt",
            Self::InvalidTargetParent => "restore target parent is invalid",
            Self::DestinationNameConflict => "restore destination name is already occupied",
            Self::TargetChanged => "restore target changed after planning",
            Self::MissingRetentionReference => "backup content retention reference is missing",
            Self::ContentUnavailable => "retained backup content is unavailable",
            Self::TargetStorageIncompatible => {
                "retained backup content is incompatible with target storage"
            }
            Self::IdempotencyConflict => {
                "restore operation identity conflicts with another request"
            }
        };
        formatter.write_str(message)
    }
}

/// The semantic request whose fingerprint is persisted with a restore plan.
/// The operation identity is deliberately separate and owner-scoped.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupRestorePlanRequest {
    backup_set_id: BackupSetId,
    snapshot_id: SnapshotId,
    target_library_id: LibraryId,
    target_parent_node_id: NodeId,
    destination_name: LogicalName,
}

impl BackupRestorePlanRequest {
    #[must_use]
    pub const fn new(
        backup_set_id: BackupSetId,
        snapshot_id: SnapshotId,
        target_library_id: LibraryId,
        target_parent_node_id: NodeId,
        destination_name: LogicalName,
    ) -> Self {
        Self {
            backup_set_id,
            snapshot_id,
            target_library_id,
            target_parent_node_id,
            destination_name,
        }
    }

    #[must_use]
    pub const fn backup_set_id(&self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn snapshot_id(&self) -> SnapshotId {
        self.snapshot_id
    }

    #[must_use]
    pub const fn target_library_id(&self) -> LibraryId {
        self.target_library_id
    }

    #[must_use]
    pub const fn target_parent_node_id(&self) -> NodeId {
        self.target_parent_node_id
    }

    #[must_use]
    pub const fn destination_name(&self) -> &LogicalName {
        &self.destination_name
    }

    /// Calculate a versioned SHA-256 digest over the semantic request fields.
    /// JSON/transport syntax and the retry operation identity are excluded.
    #[must_use]
    pub fn fingerprint(&self) -> BackupRestorePlanIdempotencyFingerprint {
        let mut canonical = Vec::with_capacity(180);
        canonical.extend_from_slice(b"synveil/backup-restore-plan\0");
        canonical.extend_from_slice(&BACKUP_RESTORE_PLAN_FINGERPRINT_VERSION.to_be_bytes());
        canonical.extend_from_slice(self.backup_set_id.as_bytes());
        canonical.extend_from_slice(self.snapshot_id.as_bytes());
        canonical.extend_from_slice(self.target_library_id.as_bytes());
        canonical.extend_from_slice(self.target_parent_node_id.as_bytes());
        let name = self.destination_name.as_str().as_bytes();
        canonical.extend_from_slice(&(name.len() as u32).to_be_bytes());
        canonical.extend_from_slice(name);

        let digest = Sha256::digest(canonical);
        let mut sha256 = [0_u8; 32];
        sha256.copy_from_slice(&digest);
        BackupRestorePlanIdempotencyFingerprint {
            version: BACKUP_RESTORE_PLAN_FINGERPRINT_VERSION,
            sha256,
        }
    }
}

/// The persisted semantic request fingerprint. It contains no physical
/// storage identity and is safe to compare at the metadata idempotency fence.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BackupRestorePlanIdempotencyFingerprint {
    version: u16,
    sha256: [u8; 32],
}

impl BackupRestorePlanIdempotencyFingerprint {
    #[must_use]
    pub const fn new(version: u16, sha256: [u8; 32]) -> Self {
        Self { version, sha256 }
    }

    #[must_use]
    pub const fn version(self) -> u16 {
        self.version
    }

    #[must_use]
    pub const fn sha256(self) -> [u8; 32] {
        self.sha256
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.sha256
    }
}

/// Durable provenance for one non-destructive restore plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupRestorePlan {
    id: BackupRestorePlanId,
    owner_user_id: UserId,
    operation_id: String,
    request_fingerprint: BackupRestorePlanIdempotencyFingerprint,
    backup_set_id: BackupSetId,
    snapshot_id: SnapshotId,
    target_library_id: LibraryId,
    target_parent_node_id: NodeId,
    destination_name: LogicalName,
    base_journal_epoch: Sequence,
    base_journal_head: Sequence,
    item_count: u64,
    content_item_count: u64,
    state: BackupRestorePlanState,
    created_at: Timestamp,
    stale_at: Option<Timestamp>,
}

impl BackupRestorePlan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: BackupRestorePlanId,
        owner_user_id: UserId,
        operation_id: String,
        request_fingerprint: BackupRestorePlanIdempotencyFingerprint,
        request: BackupRestorePlanRequest,
        base_journal_epoch: Sequence,
        base_journal_head: Sequence,
        item_count: u64,
        content_item_count: u64,
        state: BackupRestorePlanState,
        created_at: Timestamp,
        stale_at: Option<Timestamp>,
    ) -> Result<Self, DomainError> {
        if base_journal_epoch.get() == 0
            || item_count == 0
            || content_item_count > item_count
            || (matches!(
                state,
                BackupRestorePlanState::Planned | BackupRestorePlanState::Executed
            ) && stale_at.is_some())
            || (state == BackupRestorePlanState::Stale && stale_at.is_none())
        {
            return Err(DomainError::BackupRestoreInvalidPlanState);
        }
        Ok(Self {
            id,
            owner_user_id,
            operation_id,
            request_fingerprint,
            backup_set_id: request.backup_set_id,
            snapshot_id: request.snapshot_id,
            target_library_id: request.target_library_id,
            target_parent_node_id: request.target_parent_node_id,
            destination_name: request.destination_name,
            base_journal_epoch,
            base_journal_head,
            item_count,
            content_item_count,
            state,
            created_at,
            stale_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> BackupRestorePlanId {
        self.id
    }

    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    #[must_use]
    pub const fn request_fingerprint(&self) -> BackupRestorePlanIdempotencyFingerprint {
        self.request_fingerprint
    }

    #[must_use]
    pub const fn backup_set_id(&self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn snapshot_id(&self) -> SnapshotId {
        self.snapshot_id
    }

    #[must_use]
    pub const fn target_library_id(&self) -> LibraryId {
        self.target_library_id
    }

    #[must_use]
    pub const fn target_parent_node_id(&self) -> NodeId {
        self.target_parent_node_id
    }

    #[must_use]
    pub const fn destination_name(&self) -> &LogicalName {
        &self.destination_name
    }

    #[must_use]
    pub const fn base_journal_epoch(&self) -> Sequence {
        self.base_journal_epoch
    }

    #[must_use]
    pub const fn base_journal_head(&self) -> Sequence {
        self.base_journal_head
    }

    #[must_use]
    pub const fn item_count(&self) -> u64 {
        self.item_count
    }

    #[must_use]
    pub const fn content_item_count(&self) -> u64 {
        self.content_item_count
    }

    #[must_use]
    pub const fn state(&self) -> BackupRestorePlanState {
        self.state
    }

    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    #[must_use]
    pub const fn stale_at(&self) -> Option<Timestamp> {
        self.stale_at
    }

    /// Apply the only lifecycle transitions allowed after a durable plan is
    /// sealed. The planning provenance itself is never rewritten.
    pub fn transition_state(
        &mut self,
        next: BackupRestorePlanState,
        stale_at: Option<Timestamp>,
    ) -> Result<(), DomainError> {
        let allowed = matches!(
            (self.state, next),
            (
                BackupRestorePlanState::Planned,
                BackupRestorePlanState::Stale
            ) | (
                BackupRestorePlanState::Planned,
                BackupRestorePlanState::Executed
            ) | (BackupRestorePlanState::Stale, BackupRestorePlanState::Stale)
                | (
                    BackupRestorePlanState::Executed,
                    BackupRestorePlanState::Executed
                )
        );
        if !allowed
            || (next == BackupRestorePlanState::Stale) != stale_at.is_some()
            || (next != BackupRestorePlanState::Stale && stale_at.is_some())
        {
            return Err(DomainError::BackupRestoreInvalidPlanState);
        }
        self.state = next;
        if next == BackupRestorePlanState::Stale {
            self.stale_at = stale_at;
        }
        Ok(())
    }
}

/// One immutable logical action in a restore plan. The wrapper directory is
/// represented by ordinal zero and has no source snapshot node; all following
/// entries restore a non-root snapshot node beneath the wrapper/tree parent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupRestorePlanEntry {
    plan_id: BackupRestorePlanId,
    ordinal: u64,
    planned_node_id: NodeId,
    planned_parent_node_id: NodeId,
    source_snapshot_node_id: Option<NodeId>,
    source_parent_node_id: Option<NodeId>,
    source_state: NodeState,
    action: BackupRestoreAction,
    kind: NodeKind,
    name: LogicalName,
    source_revision: Option<Revision>,
    content: Option<BackupManifestContent>,
}

impl BackupRestorePlanEntry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        plan_id: BackupRestorePlanId,
        ordinal: u64,
        planned_node_id: NodeId,
        planned_parent_node_id: NodeId,
        source_snapshot_node_id: Option<NodeId>,
        source_parent_node_id: Option<NodeId>,
        source_state: NodeState,
        action: BackupRestoreAction,
        kind: NodeKind,
        name: LogicalName,
        source_revision: Option<Revision>,
        content: Option<BackupManifestContent>,
    ) -> Result<Self, DomainError> {
        let wrapper = source_snapshot_node_id.is_none();
        let valid = if wrapper {
            ordinal == 0
                && planned_node_id != planned_parent_node_id
                && source_parent_node_id.is_none()
                && source_revision.is_none()
                && source_state == NodeState::Active
                && action == BackupRestoreAction::CreateDirectory
                && kind == NodeKind::Directory
                && content.is_none()
        } else {
            ordinal > 0
                && planned_node_id != planned_parent_node_id
                && source_parent_node_id.is_some()
                && source_revision.is_some()
                && source_state != NodeState::Purging
                && ((kind == NodeKind::Directory
                    && action == BackupRestoreAction::CreateDirectory
                    && content.is_none())
                    || (kind == NodeKind::File
                        && action == BackupRestoreAction::CreateFile
                        && content.is_some()))
        };
        if !valid {
            return Err(DomainError::BackupRestoreInvalidPlanEntry);
        }
        Ok(Self {
            plan_id,
            ordinal,
            planned_node_id,
            planned_parent_node_id,
            source_snapshot_node_id,
            source_parent_node_id,
            source_state,
            action,
            kind,
            name,
            source_revision,
            content,
        })
    }

    #[must_use]
    pub const fn plan_id(&self) -> BackupRestorePlanId {
        self.plan_id
    }

    #[must_use]
    pub const fn ordinal(&self) -> u64 {
        self.ordinal
    }

    #[must_use]
    pub const fn planned_node_id(&self) -> NodeId {
        self.planned_node_id
    }

    #[must_use]
    pub const fn planned_parent_node_id(&self) -> NodeId {
        self.planned_parent_node_id
    }

    #[must_use]
    pub const fn source_snapshot_node_id(&self) -> Option<NodeId> {
        self.source_snapshot_node_id
    }

    #[must_use]
    pub const fn source_parent_node_id(&self) -> Option<NodeId> {
        self.source_parent_node_id
    }

    #[must_use]
    pub const fn source_state(&self) -> NodeState {
        self.source_state
    }

    #[must_use]
    pub const fn action(&self) -> BackupRestoreAction {
        self.action
    }

    #[must_use]
    pub const fn kind(&self) -> NodeKind {
        self.kind
    }

    #[must_use]
    pub const fn name(&self) -> &LogicalName {
        &self.name
    }

    #[must_use]
    pub const fn source_revision(&self) -> Option<Revision> {
        self.source_revision
    }

    #[must_use]
    pub const fn content(&self) -> Option<&BackupManifestContent> {
        self.content.as_ref()
    }

    #[must_use]
    pub const fn is_wrapper(&self) -> bool {
        self.source_snapshot_node_id.is_none()
    }
}

/// Canonical immutable receipt for one committed restore-plan execution.
/// Physical Object and replica identity deliberately does not cross this
/// boundary; only logical destination identities and journal evidence do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupRestoreExecution {
    id: BackupRestoreExecutionId,
    owner_user_id: UserId,
    plan_id: BackupRestorePlanId,
    target_library_id: LibraryId,
    journal_first_sequence: Sequence,
    journal_last_sequence: Sequence,
    created_node_count: u64,
    created_file_version_count: u64,
    executed_at: Timestamp,
}

impl BackupRestoreExecution {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: BackupRestoreExecutionId,
        owner_user_id: UserId,
        plan_id: BackupRestorePlanId,
        target_library_id: LibraryId,
        journal_first_sequence: Sequence,
        journal_last_sequence: Sequence,
        created_node_count: u64,
        created_file_version_count: u64,
        executed_at: Timestamp,
    ) -> Result<Self, DomainError> {
        if journal_first_sequence.get() == 0
            || journal_last_sequence < journal_first_sequence
            || created_node_count == 0
            || created_file_version_count > created_node_count
        {
            return Err(DomainError::BackupRestoreInvalidPlanState);
        }
        Ok(Self {
            id,
            owner_user_id,
            plan_id,
            target_library_id,
            journal_first_sequence,
            journal_last_sequence,
            created_node_count,
            created_file_version_count,
            executed_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> BackupRestoreExecutionId {
        self.id
    }

    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn plan_id(&self) -> BackupRestorePlanId {
        self.plan_id
    }

    #[must_use]
    pub const fn target_library_id(&self) -> LibraryId {
        self.target_library_id
    }

    #[must_use]
    pub const fn journal_first_sequence(&self) -> Sequence {
        self.journal_first_sequence
    }

    #[must_use]
    pub const fn journal_last_sequence(&self) -> Sequence {
        self.journal_last_sequence
    }

    #[must_use]
    pub const fn created_node_count(&self) -> u64 {
        self.created_node_count
    }

    #[must_use]
    pub const fn created_file_version_count(&self) -> u64 {
        self.created_file_version_count
    }

    #[must_use]
    pub const fn executed_at(&self) -> Timestamp {
        self.executed_at
    }
}

/// Immutable logical mapping from one plan entry to its committed destination
/// identity and canonical journal fact. Directories carry NodeCreated evidence;
/// files carry FileContentCommitted evidence and their new live FileVersionId.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupRestoreExecutionEntry {
    execution_id: BackupRestoreExecutionId,
    plan_id: BackupRestorePlanId,
    ordinal: u64,
    destination_node_id: NodeId,
    destination_file_version_id: Option<FileVersionId>,
    node_created_journal_sequence: Option<Sequence>,
    file_content_committed_journal_sequence: Option<Sequence>,
}

impl BackupRestoreExecutionEntry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        execution_id: BackupRestoreExecutionId,
        plan_id: BackupRestorePlanId,
        ordinal: u64,
        destination_node_id: NodeId,
        destination_file_version_id: Option<FileVersionId>,
        node_created_journal_sequence: Option<Sequence>,
        file_content_committed_journal_sequence: Option<Sequence>,
    ) -> Result<Self, DomainError> {
        let directory_shape = destination_file_version_id.is_none()
            && node_created_journal_sequence.is_some()
            && file_content_committed_journal_sequence.is_none();
        let file_shape = destination_file_version_id.is_some()
            && node_created_journal_sequence.is_none()
            && file_content_committed_journal_sequence.is_some();
        if (!directory_shape && !file_shape)
            || node_created_journal_sequence.is_some_and(|value| value.get() == 0)
            || file_content_committed_journal_sequence.is_some_and(|value| value.get() == 0)
        {
            return Err(DomainError::BackupRestoreInvalidPlanEntry);
        }
        Ok(Self {
            execution_id,
            plan_id,
            ordinal,
            destination_node_id,
            destination_file_version_id,
            node_created_journal_sequence,
            file_content_committed_journal_sequence,
        })
    }

    #[must_use]
    pub const fn execution_id(&self) -> BackupRestoreExecutionId {
        self.execution_id
    }

    #[must_use]
    pub const fn plan_id(&self) -> BackupRestorePlanId {
        self.plan_id
    }

    #[must_use]
    pub const fn ordinal(&self) -> u64 {
        self.ordinal
    }

    #[must_use]
    pub const fn destination_node_id(&self) -> NodeId {
        self.destination_node_id
    }

    #[must_use]
    pub const fn destination_file_version_id(&self) -> Option<FileVersionId> {
        self.destination_file_version_id
    }

    #[must_use]
    pub const fn node_created_journal_sequence(&self) -> Option<Sequence> {
        self.node_created_journal_sequence
    }

    #[must_use]
    pub const fn file_content_committed_journal_sequence(&self) -> Option<Sequence> {
        self.file_content_committed_journal_sequence
    }
}

/// Version of the canonical semantic prune-plan request fingerprint.
///
/// Reference counts deliberately do not participate: they are an immutable
/// observation made by planning, not operator intent.
pub const BACKUP_PRUNE_PLAN_FINGERPRINT_VERSION: u16 = 1;

/// Durable lifecycle for a retention-release preflight plan. `PLANNED` is
/// evidence only; it grants no deletion or pin-release authority. A later
/// accounting change may only make it `STALE`, and an explicit authorized
/// atomic prune execution makes it terminal `EXECUTED`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BackupPrunePlanState {
    Planned,
    Stale,
    Executed,
}

impl BackupPrunePlanState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "PLANNED",
            Self::Stale => "STALE",
            Self::Executed => "EXECUTED",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupPrunePlanStateParseError;

impl fmt::Display for BackupPrunePlanStateParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("backup prune plan state is unknown")
    }
}

impl std::error::Error for BackupPrunePlanStateParseError {}

impl FromStr for BackupPrunePlanState {
    type Err = BackupPrunePlanStateParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "PLANNED" => Ok(Self::Planned),
            "STALE" => Ok(Self::Stale),
            "EXECUTED" => Ok(Self::Executed),
            _ => Err(BackupPrunePlanStateParseError),
        }
    }
}

/// The logical outcome for one distinct retained content item after every pin
/// owned by the source snapshot is hypothetically excluded. This is never a
/// deletion authorization.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BackupPruneImpact {
    RetainedByOtherReference,
    WouldBecomeUnreferenced,
}

impl BackupPruneImpact {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RetainedByOtherReference => "RETAINED_BY_OTHER_REFERENCE",
            Self::WouldBecomeUnreferenced => "WOULD_BECOME_UNREFERENCED",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupPruneImpactParseError;

impl fmt::Display for BackupPruneImpactParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("backup prune impact is unknown")
    }
}

impl std::error::Error for BackupPruneImpactParseError {}

impl FromStr for BackupPruneImpact {
    type Err = BackupPruneImpactParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "RETAINED_BY_OTHER_REFERENCE" => Ok(Self::RetainedByOtherReference),
            "WOULD_BECOME_UNREFERENCED" => Ok(Self::WouldBecomeUnreferenced),
            _ => Err(BackupPruneImpactParseError),
        }
    }
}

/// Typed, owner-safe reasons why a retention-release preflight cannot make a
/// plan. These values never disclose canonical Object or replica identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BackupPrunePreflightIssue {
    SnapshotNotExpired,
    SnapshotCorrupt,
    RetentionCorrupt,
}

impl fmt::Display for BackupPrunePreflightIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::SnapshotNotExpired => "backup snapshot retention is not eligible for pruning",
            Self::SnapshotCorrupt => "backup snapshot manifest is corrupt",
            Self::RetentionCorrupt => "backup snapshot retention evidence is corrupt",
        };
        formatter.write_str(message)
    }
}

/// The semantic request for one owner-scoped, single-snapshot prune plan.
/// The retry operation identity is deliberately separate from this value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupPrunePlanRequest {
    backup_set_id: BackupSetId,
    snapshot_id: SnapshotId,
}

impl BackupPrunePlanRequest {
    #[must_use]
    pub const fn new(backup_set_id: BackupSetId, snapshot_id: SnapshotId) -> Self {
        Self {
            backup_set_id,
            snapshot_id,
        }
    }

    #[must_use]
    pub const fn backup_set_id(&self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn snapshot_id(&self) -> SnapshotId {
        self.snapshot_id
    }

    /// Calculate a versioned SHA-256 digest over semantic operator intent.
    /// Observed reference counts and other mutable state are excluded.
    #[must_use]
    pub fn fingerprint(&self) -> BackupPrunePlanIdempotencyFingerprint {
        let mut canonical = Vec::with_capacity(96);
        canonical.extend_from_slice(b"synveil/backup-prune-plan\0");
        canonical.extend_from_slice(&BACKUP_PRUNE_PLAN_FINGERPRINT_VERSION.to_be_bytes());
        canonical.extend_from_slice(self.backup_set_id.as_bytes());
        canonical.extend_from_slice(self.snapshot_id.as_bytes());

        let digest = Sha256::digest(canonical);
        let mut sha256 = [0_u8; 32];
        sha256.copy_from_slice(&digest);
        BackupPrunePlanIdempotencyFingerprint {
            version: BACKUP_PRUNE_PLAN_FINGERPRINT_VERSION,
            sha256,
        }
    }
}

/// The persisted semantic prune-request fingerprint. It contains no physical
/// storage identity and is safe to compare at the retry fence.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BackupPrunePlanIdempotencyFingerprint {
    version: u16,
    sha256: [u8; 32],
}

impl BackupPrunePlanIdempotencyFingerprint {
    #[must_use]
    pub const fn new(version: u16, sha256: [u8; 32]) -> Self {
        Self { version, sha256 }
    }

    #[must_use]
    pub const fn version(self) -> u16 {
        self.version
    }

    #[must_use]
    pub const fn sha256(self) -> [u8; 32] {
        self.sha256
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.sha256
    }
}

/// Immutable provenance for one retention-release preflight. Aggregate impact
/// counts are public-safe; per-object accounting remains server-internal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupPrunePlan {
    id: BackupPrunePlanId,
    owner_user_id: UserId,
    operation_id: String,
    request_fingerprint: BackupPrunePlanIdempotencyFingerprint,
    backup_set_id: BackupSetId,
    snapshot_id: SnapshotId,
    snapshot_manifest_item_count: u64,
    snapshot_content_reference_count: u64,
    planned_pin_release_count: u64,
    distinct_retained_content_count: u64,
    retained_after_release_count: u64,
    would_become_unreferenced_count: u64,
    state: BackupPrunePlanState,
    created_at: Timestamp,
    stale_at: Option<Timestamp>,
}

impl BackupPrunePlan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: BackupPrunePlanId,
        owner_user_id: UserId,
        operation_id: String,
        request_fingerprint: BackupPrunePlanIdempotencyFingerprint,
        request: BackupPrunePlanRequest,
        snapshot_manifest_item_count: u64,
        snapshot_content_reference_count: u64,
        planned_pin_release_count: u64,
        distinct_retained_content_count: u64,
        retained_after_release_count: u64,
        would_become_unreferenced_count: u64,
        state: BackupPrunePlanState,
        created_at: Timestamp,
        stale_at: Option<Timestamp>,
    ) -> Result<Self, DomainError> {
        let counts_are_valid = planned_pin_release_count == snapshot_content_reference_count
            && distinct_retained_content_count <= planned_pin_release_count
            && retained_after_release_count + would_become_unreferenced_count
                == distinct_retained_content_count;
        let stale_shape_is_valid = (state == BackupPrunePlanState::Stale) == stale_at.is_some();
        if !counts_are_valid || !stale_shape_is_valid {
            return Err(DomainError::BackupPruneInvalidPlanState);
        }
        Ok(Self {
            id,
            owner_user_id,
            operation_id,
            request_fingerprint,
            backup_set_id: request.backup_set_id,
            snapshot_id: request.snapshot_id,
            snapshot_manifest_item_count,
            snapshot_content_reference_count,
            planned_pin_release_count,
            distinct_retained_content_count,
            retained_after_release_count,
            would_become_unreferenced_count,
            state,
            created_at,
            stale_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> BackupPrunePlanId {
        self.id
    }

    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    #[must_use]
    pub const fn request_fingerprint(&self) -> BackupPrunePlanIdempotencyFingerprint {
        self.request_fingerprint
    }

    #[must_use]
    pub const fn backup_set_id(&self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn snapshot_id(&self) -> SnapshotId {
        self.snapshot_id
    }

    #[must_use]
    pub const fn snapshot_manifest_item_count(&self) -> u64 {
        self.snapshot_manifest_item_count
    }

    #[must_use]
    pub const fn snapshot_content_reference_count(&self) -> u64 {
        self.snapshot_content_reference_count
    }

    #[must_use]
    pub const fn planned_pin_release_count(&self) -> u64 {
        self.planned_pin_release_count
    }

    #[must_use]
    pub const fn distinct_retained_content_count(&self) -> u64 {
        self.distinct_retained_content_count
    }

    #[must_use]
    pub const fn retained_after_release_count(&self) -> u64 {
        self.retained_after_release_count
    }

    #[must_use]
    pub const fn would_become_unreferenced_count(&self) -> u64 {
        self.would_become_unreferenced_count
    }

    #[must_use]
    pub const fn state(&self) -> BackupPrunePlanState {
        self.state
    }

    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    #[must_use]
    pub const fn stale_at(&self) -> Option<Timestamp> {
        self.stale_at
    }

    /// The reference basis is immutable. Validation can only mark a planned
    /// observation stale, and an explicit prune execution can only make it
    /// terminal `EXECUTED`; it must never be overwritten with later counts.
    pub fn transition_state(
        &mut self,
        next: BackupPrunePlanState,
        stale_at: Option<Timestamp>,
    ) -> Result<(), DomainError> {
        let allowed = matches!(
            (self.state, next),
            (BackupPrunePlanState::Planned, BackupPrunePlanState::Stale)
                | (BackupPrunePlanState::Stale, BackupPrunePlanState::Stale)
                | (
                    BackupPrunePlanState::Planned,
                    BackupPrunePlanState::Executed
                )
        );
        if !allowed
            || (next == BackupPrunePlanState::Stale) != stale_at.is_some()
            || (next != BackupPrunePlanState::Stale && stale_at.is_some())
            || (self.state == BackupPrunePlanState::Stale && stale_at != self.stale_at)
        {
            return Err(DomainError::BackupPruneInvalidPlanState);
        }
        self.state = next;
        self.stale_at = stale_at;
        Ok(())
    }
}

/// One immutable logical retention pin release entry. It deliberately carries
/// only source manifest identity and logical content provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupPrunePlanEntry {
    plan_id: BackupPrunePlanId,
    ordinal: u64,
    source_snapshot_node_id: NodeId,
    content: BackupManifestContent,
}

impl BackupPrunePlanEntry {
    #[must_use]
    pub const fn new(
        plan_id: BackupPrunePlanId,
        ordinal: u64,
        source_snapshot_node_id: NodeId,
        content: BackupManifestContent,
    ) -> Self {
        Self {
            plan_id,
            ordinal,
            source_snapshot_node_id,
            content,
        }
    }

    #[must_use]
    pub const fn plan_id(&self) -> BackupPrunePlanId {
        self.plan_id
    }

    #[must_use]
    pub const fn ordinal(&self) -> u64 {
        self.ordinal
    }

    #[must_use]
    pub const fn source_snapshot_node_id(&self) -> NodeId {
        self.source_snapshot_node_id
    }

    #[must_use]
    pub const fn content(&self) -> &BackupManifestContent {
        &self.content
    }
}

/// Typed, owner-safe reasons why a prune execution cannot proceed or is
/// rejected. These values never disclose canonical Object or replica identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BackupPruneExecutionPreflightIssue {
    SnapshotNotExpired,
    PlanStale,
    PlanAlreadyExecuted,
    SnapshotAlreadyPruned,
    PlanReferenceDrift,
    PlanCorruption,
}

impl fmt::Display for BackupPruneExecutionPreflightIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::SnapshotNotExpired => "source snapshot is not eligible for pruning",
            Self::PlanStale => "prune plan reference basis has drifted and must be replanned",
            Self::PlanAlreadyExecuted => "prune plan has already been executed",
            Self::SnapshotAlreadyPruned => "snapshot retention has already been released",
            Self::PlanReferenceDrift => "reference accounting has changed since planning",
            Self::PlanCorruption => "prune plan internal evidence is corrupt",
        };
        formatter.write_str(message)
    }
}

/// Immutable receipt for one committed prune-plan execution. The receipt is
/// the authoritative evidence that a snapshot's dedicated retention was
/// released. Physical Object and replica identity deliberately does not cross
/// this boundary; only logical counts and identifiers are exposed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupPruneExecution {
    id: BackupPruneExecutionId,
    owner_user_id: UserId,
    prune_plan_id: BackupPrunePlanId,
    backup_set_id: BackupSetId,
    snapshot_id: SnapshotId,
    released_pin_count: u64,
    distinct_object_count: u64,
    retained_by_other_reference_count: u64,
    gc_handoff_object_count: u64,
    executed_at: Timestamp,
}

impl BackupPruneExecution {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: BackupPruneExecutionId,
        owner_user_id: UserId,
        prune_plan_id: BackupPrunePlanId,
        backup_set_id: BackupSetId,
        snapshot_id: SnapshotId,
        released_pin_count: u64,
        distinct_object_count: u64,
        retained_by_other_reference_count: u64,
        gc_handoff_object_count: u64,
        executed_at: Timestamp,
    ) -> Result<Self, DomainError> {
        if retained_by_other_reference_count + gc_handoff_object_count != distinct_object_count {
            return Err(DomainError::BackupPruneInvalidPlanState);
        }
        Ok(Self {
            id,
            owner_user_id,
            prune_plan_id,
            backup_set_id,
            snapshot_id,
            released_pin_count,
            distinct_object_count,
            retained_by_other_reference_count,
            gc_handoff_object_count,
            executed_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> BackupPruneExecutionId {
        self.id
    }

    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn prune_plan_id(&self) -> BackupPrunePlanId {
        self.prune_plan_id
    }

    #[must_use]
    pub const fn backup_set_id(&self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn snapshot_id(&self) -> SnapshotId {
        self.snapshot_id
    }

    #[must_use]
    pub const fn released_pin_count(&self) -> u64 {
        self.released_pin_count
    }

    #[must_use]
    pub const fn distinct_object_count(&self) -> u64 {
        self.distinct_object_count
    }

    #[must_use]
    pub const fn retained_by_other_reference_count(&self) -> u64 {
        self.retained_by_other_reference_count
    }

    #[must_use]
    pub const fn gc_handoff_object_count(&self) -> u64 {
        self.gc_handoff_object_count
    }

    #[must_use]
    pub const fn executed_at(&self) -> Timestamp {
        self.executed_at
    }
}

/// Version of the semantic retention-policy configuration fingerprint.
pub const BACKUP_SNAPSHOT_RETENTION_POLICY_FINGERPRINT_VERSION: u16 = 1;

/// Version of the semantic snapshot-expiry planning request fingerprint.
pub const BACKUP_SNAPSHOT_EXPIRY_PLAN_FINGERPRINT_VERSION: u16 = 1;

/// Version of the coherent snapshot-cohort observation fingerprint.
pub const BACKUP_SNAPSHOT_EXPIRY_BASIS_FINGERPRINT_VERSION: u16 = 1;

/// Maximum policy duration accepted by the portable timestamp domain. Ten
/// thousand Julian years is far beyond product retention horizons while still
/// permitting checked subtraction from contemporary server timestamps.
pub const MAX_BACKUP_SNAPSHOT_RETENTION_SECONDS: u64 = 315_576_000_000;

/// The bounded first retention policy. Both values are strict positive
/// integers and the duration must fit checked `Timestamp` arithmetic.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BackupSnapshotRetentionPolicyConfig {
    keep_latest_completed: u64,
    expire_after_seconds: u64,
}

impl BackupSnapshotRetentionPolicyConfig {
    pub fn new(keep_latest_completed: u64, expire_after_seconds: u64) -> Result<Self, DomainError> {
        if keep_latest_completed == 0
            || expire_after_seconds == 0
            || expire_after_seconds > MAX_BACKUP_SNAPSHOT_RETENTION_SECONDS
            || i64::try_from(keep_latest_completed).is_err()
        {
            return Err(DomainError::BackupRetentionPolicyInvalidConfig);
        }
        Ok(Self {
            keep_latest_completed,
            expire_after_seconds,
        })
    }

    #[must_use]
    pub const fn keep_latest_completed(self) -> u64 {
        self.keep_latest_completed
    }

    #[must_use]
    pub const fn expire_after_seconds(self) -> u64 {
        self.expire_after_seconds
    }

    #[must_use]
    pub const fn expire_after(self) -> std::time::Duration {
        std::time::Duration::from_secs(self.expire_after_seconds)
    }
}

/// A positive, monotonically increasing revision number within one backup
/// set. Allocation is serialized at the PostgreSQL boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BackupSnapshotRetentionPolicyRevisionNumber(u64);

impl BackupSnapshotRetentionPolicyRevisionNumber {
    pub fn new(value: u64) -> Result<Self, DomainError> {
        if value == 0 || i64::try_from(value).is_err() {
            return Err(DomainError::BackupRetentionPolicyInvalidRevision);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn checked_next(self) -> Result<Self, DomainError> {
        self.0
            .checked_add(1)
            .ok_or(DomainError::BackupRetentionPolicyInvalidRevision)
            .and_then(Self::new)
    }
}

impl FromStr for BackupSnapshotRetentionPolicyRevisionNumber {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<u64>()
            .map_err(|_| DomainError::BackupRetentionPolicyInvalidRevision)
            .and_then(Self::new)
    }
}

impl fmt::Display for BackupSnapshotRetentionPolicyRevisionNumber {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// The caller's semantic request to append one immutable policy revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupSnapshotRetentionPolicyRequest {
    backup_set_id: BackupSetId,
    config: BackupSnapshotRetentionPolicyConfig,
}

impl BackupSnapshotRetentionPolicyRequest {
    #[must_use]
    pub const fn new(
        backup_set_id: BackupSetId,
        config: BackupSnapshotRetentionPolicyConfig,
    ) -> Self {
        Self {
            backup_set_id,
            config,
        }
    }

    #[must_use]
    pub const fn backup_set_id(self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn config(self) -> BackupSnapshotRetentionPolicyConfig {
        self.config
    }

    #[must_use]
    pub fn fingerprint(self) -> BackupSnapshotRetentionPolicyIdempotencyFingerprint {
        let mut canonical = Vec::with_capacity(66);
        canonical.extend_from_slice(b"synveil/backup-snapshot-retention-policy\0");
        canonical
            .extend_from_slice(&BACKUP_SNAPSHOT_RETENTION_POLICY_FINGERPRINT_VERSION.to_be_bytes());
        canonical.extend_from_slice(self.backup_set_id.as_bytes());
        canonical.extend_from_slice(&self.config.keep_latest_completed.to_be_bytes());
        canonical.extend_from_slice(&self.config.expire_after_seconds.to_be_bytes());
        let digest = Sha256::digest(canonical);
        let mut sha256 = [0_u8; 32];
        sha256.copy_from_slice(&digest);
        BackupSnapshotRetentionPolicyIdempotencyFingerprint::new(
            BACKUP_SNAPSHOT_RETENTION_POLICY_FINGERPRINT_VERSION,
            sha256,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BackupSnapshotRetentionPolicyIdempotencyFingerprint {
    version: u16,
    sha256: [u8; 32],
}

impl BackupSnapshotRetentionPolicyIdempotencyFingerprint {
    #[must_use]
    pub const fn new(version: u16, sha256: [u8; 32]) -> Self {
        Self { version, sha256 }
    }

    #[must_use]
    pub const fn version(self) -> u16 {
        self.version
    }

    #[must_use]
    pub const fn sha256(self) -> [u8; 32] {
        self.sha256
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.sha256
    }
}

/// One append-only policy revision. Changing policy always creates another
/// value; no field on an existing revision is mutable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupSnapshotRetentionPolicyRevision {
    id: BackupSnapshotRetentionPolicyRevisionId,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
    revision_number: BackupSnapshotRetentionPolicyRevisionNumber,
    operation_id: String,
    request_fingerprint: BackupSnapshotRetentionPolicyIdempotencyFingerprint,
    config: BackupSnapshotRetentionPolicyConfig,
    created_at: Timestamp,
}

impl BackupSnapshotRetentionPolicyRevision {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: BackupSnapshotRetentionPolicyRevisionId,
        owner_user_id: UserId,
        backup_set_id: BackupSetId,
        revision_number: BackupSnapshotRetentionPolicyRevisionNumber,
        operation_id: String,
        request_fingerprint: BackupSnapshotRetentionPolicyIdempotencyFingerprint,
        config: BackupSnapshotRetentionPolicyConfig,
        created_at: Timestamp,
    ) -> Result<Self, DomainError> {
        let request = BackupSnapshotRetentionPolicyRequest::new(backup_set_id, config);
        if request_fingerprint.version() != BACKUP_SNAPSHOT_RETENTION_POLICY_FINGERPRINT_VERSION
            || request.fingerprint() != request_fingerprint
        {
            return Err(DomainError::BackupRetentionPolicyInvalidRevision);
        }
        Ok(Self {
            id,
            owner_user_id,
            backup_set_id,
            revision_number,
            operation_id,
            request_fingerprint,
            config,
            created_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> BackupSnapshotRetentionPolicyRevisionId {
        self.id
    }

    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn backup_set_id(&self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn revision_number(&self) -> BackupSnapshotRetentionPolicyRevisionNumber {
        self.revision_number
    }

    #[must_use]
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    #[must_use]
    pub const fn request_fingerprint(&self) -> BackupSnapshotRetentionPolicyIdempotencyFingerprint {
        self.request_fingerprint
    }

    #[must_use]
    pub const fn config(&self) -> BackupSnapshotRetentionPolicyConfig {
        self.config
    }

    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }
}

/// Public lifecycle for a snapshot-expiry plan. Planning is evidence only;
/// Prompt 48 allows `PLANNED -> EXECUTED` and `PLANNED -> STALE`. Once executed
/// or stale, the plan is terminal and its provenance is never rewritten.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BackupSnapshotExpiryPlanState {
    Planned,
    Stale,
    Executed,
}

impl BackupSnapshotExpiryPlanState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "PLANNED",
            Self::Stale => "STALE",
            Self::Executed => "EXECUTED",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupSnapshotExpiryPlanStateParseError;

impl fmt::Display for BackupSnapshotExpiryPlanStateParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("backup snapshot expiry plan state is unknown")
    }
}

impl std::error::Error for BackupSnapshotExpiryPlanStateParseError {}

impl FromStr for BackupSnapshotExpiryPlanState {
    type Err = BackupSnapshotExpiryPlanStateParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "PLANNED" => Ok(Self::Planned),
            "STALE" => Ok(Self::Stale),
            "EXECUTED" => Ok(Self::Executed),
            _ => Err(BackupSnapshotExpiryPlanStateParseError),
        }
    }
}

/// Exactly one policy decision for every completed snapshot in the observed
/// cohort, in mandatory precedence order at planning time.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BackupSnapshotExpiryDecision {
    KeepLatest,
    KeepRecent,
    BlockedActiveRestorePlan,
    Expire,
}

impl BackupSnapshotExpiryDecision {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::KeepLatest => "KEEP_LATEST",
            Self::KeepRecent => "KEEP_RECENT",
            Self::BlockedActiveRestorePlan => "BLOCKED_ACTIVE_RESTORE_PLAN",
            Self::Expire => "EXPIRE",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupSnapshotExpiryDecisionParseError;

impl fmt::Display for BackupSnapshotExpiryDecisionParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("backup snapshot expiry decision is unknown")
    }
}

impl std::error::Error for BackupSnapshotExpiryDecisionParseError {}

impl FromStr for BackupSnapshotExpiryDecision {
    type Err = BackupSnapshotExpiryDecisionParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "KEEP_LATEST" => Ok(Self::KeepLatest),
            "KEEP_RECENT" => Ok(Self::KeepRecent),
            "BLOCKED_ACTIVE_RESTORE_PLAN" => Ok(Self::BlockedActiveRestorePlan),
            "EXPIRE" => Ok(Self::Expire),
            _ => Err(BackupSnapshotExpiryDecisionParseError),
        }
    }
}

/// Typed, owner-safe planning failures. They disclose no snapshot count,
/// blocker identity, or physical storage identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BackupSnapshotExpiryPreflightIssue {
    RetentionPolicyNotConfigured,
    ExpiryAlreadyPlanned,
    DurationUnrepresentable,
}

impl fmt::Display for BackupSnapshotExpiryPreflightIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::RetentionPolicyNotConfigured => {
                "backup snapshot retention policy is not configured"
            }
            Self::ExpiryAlreadyPlanned => "backup set already has an active snapshot expiry plan",
            Self::DurationUnrepresentable => {
                "backup snapshot retention duration is not representable"
            }
        };
        formatter.write_str(message)
    }
}

/// Typed, owner-safe execution failures. They disclose a plan's immutable basis
/// is no longer the authoritative current observation; no snapshot count or
/// physical storage identity is exposed.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BackupSnapshotExpiryExecutionPreflightIssue {
    PlanStale,
    PlanAlreadyExecuted,
    PlanCorruption,
}

impl fmt::Display for BackupSnapshotExpiryExecutionPreflightIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::PlanStale => "snapshot expiry plan basis has drifted and must be replanned",
            Self::PlanAlreadyExecuted => "snapshot expiry plan has already been executed",
            Self::PlanCorruption => "snapshot expiry plan evidence is corrupt",
        };
        formatter.write_str(message)
    }
}

/// Version of the manual maintenance-run request fingerprint.
pub const BACKUP_MAINTENANCE_RUN_FINGERPRINT_VERSION: u16 = 1;

/// Durable lifecycle for one explicit manual backup maintenance run.
///
/// Forward transitions are `CREATED -> SNAPSHOT_CAPTURED -> EXPIRY_PLANNED ->
/// COMPLETED`. Any active state may terminate as `STALE`; valid child work is
/// never compensated by this lifecycle.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BackupMaintenanceRunState {
    Created,
    SnapshotCaptured,
    ExpiryPlanned,
    Completed,
    Stale,
}

impl BackupMaintenanceRunState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "CREATED",
            Self::SnapshotCaptured => "SNAPSHOT_CAPTURED",
            Self::ExpiryPlanned => "EXPIRY_PLANNED",
            Self::Completed => "COMPLETED",
            Self::Stale => "STALE",
        }
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Stale)
    }

    #[must_use]
    pub const fn can_transition_to(self, to: Self) -> bool {
        matches!(
            (self, to),
            (Self::Created, Self::SnapshotCaptured)
                | (Self::SnapshotCaptured, Self::ExpiryPlanned)
                | (Self::ExpiryPlanned, Self::Completed)
                | (Self::Created, Self::Stale)
                | (Self::SnapshotCaptured, Self::Stale)
                | (Self::ExpiryPlanned, Self::Stale)
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupMaintenanceRunStateParseError;

impl fmt::Display for BackupMaintenanceRunStateParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("backup maintenance run state is unknown")
    }
}

impl std::error::Error for BackupMaintenanceRunStateParseError {}

impl FromStr for BackupMaintenanceRunState {
    type Err = BackupMaintenanceRunStateParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "CREATED" => Ok(Self::Created),
            "SNAPSHOT_CAPTURED" => Ok(Self::SnapshotCaptured),
            "EXPIRY_PLANNED" => Ok(Self::ExpiryPlanned),
            "COMPLETED" => Ok(Self::Completed),
            "STALE" => Ok(Self::Stale),
            _ => Err(BackupMaintenanceRunStateParseError),
        }
    }
}

/// Owner-safe maintenance preflight reasons. These values never expose physical
/// storage identity or foreign resource details.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BackupMaintenanceRunPreflightIssue {
    RetentionPolicyNotConfigured,
    MaintenanceAlreadyRunning,
    OperationConflict,
    PolicyChanged,
    ExpiryPlanStale,
    InvalidRunState,
    RunCorruption,
}

impl fmt::Display for BackupMaintenanceRunPreflightIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::RetentionPolicyNotConfigured => {
                "backup snapshot retention policy is not configured"
            }
            Self::MaintenanceAlreadyRunning => "backup set already has an active maintenance run",
            Self::OperationConflict => {
                "backup maintenance operation identity conflicts with the request"
            }
            Self::PolicyChanged => {
                "backup snapshot retention policy changed before maintenance planning"
            }
            Self::ExpiryPlanStale => "snapshot expiry plan became stale during maintenance",
            Self::InvalidRunState => "backup maintenance run cannot be advanced from this state",
            Self::RunCorruption => "backup maintenance run evidence is corrupt",
        };
        formatter.write_str(message)
    }
}

/// The semantic request for one explicit maintenance run. Child operation IDs
/// are durable orchestration provenance, not part of caller-controlled intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupMaintenanceRunRequest {
    backup_set_id: BackupSetId,
}

impl BackupMaintenanceRunRequest {
    #[must_use]
    pub const fn new(backup_set_id: BackupSetId) -> Self {
        Self { backup_set_id }
    }

    #[must_use]
    pub const fn backup_set_id(self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub fn fingerprint(self) -> BackupMaintenanceRunIdempotencyFingerprint {
        let mut canonical = Vec::with_capacity(61);
        canonical.extend_from_slice(b"synveil/backup-maintenance-run\0");
        canonical.extend_from_slice(&BACKUP_MAINTENANCE_RUN_FINGERPRINT_VERSION.to_be_bytes());
        canonical.extend_from_slice(self.backup_set_id.as_bytes());
        let digest = Sha256::digest(canonical);
        let mut sha256 = [0_u8; 32];
        sha256.copy_from_slice(&digest);
        BackupMaintenanceRunIdempotencyFingerprint::new(
            BACKUP_MAINTENANCE_RUN_FINGERPRINT_VERSION,
            sha256,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BackupMaintenanceRunIdempotencyFingerprint {
    version: u16,
    sha256: [u8; 32],
}

impl BackupMaintenanceRunIdempotencyFingerprint {
    #[must_use]
    pub const fn new(version: u16, sha256: [u8; 32]) -> Self {
        Self { version, sha256 }
    }

    #[must_use]
    pub const fn version(self) -> u16 {
        self.version
    }

    #[must_use]
    pub const fn sha256(self) -> [u8; 32] {
        self.sha256
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.sha256
    }
}

/// Durable orchestration evidence for one explicit manual maintenance run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupMaintenanceRun {
    id: BackupMaintenanceRunId,
    owner_user_id: UserId,
    operation_id: String,
    request_fingerprint: BackupMaintenanceRunIdempotencyFingerprint,
    backup_set_id: BackupSetId,
    policy_revision_id: BackupSnapshotRetentionPolicyRevisionId,
    policy_revision_number: BackupSnapshotRetentionPolicyRevisionNumber,
    capture_operation_id: String,
    expiry_plan_operation_id: String,
    state: BackupMaintenanceRunState,
    captured_snapshot_id: Option<SnapshotId>,
    expiry_plan_id: Option<BackupSnapshotExpiryPlanId>,
    expiry_execution_id: Option<BackupSnapshotExpiryExecutionId>,
    snapshot_captured_at: Option<Timestamp>,
    expiry_planned_at: Option<Timestamp>,
    maintenance_completed_at: Option<Timestamp>,
    stale_at: Option<Timestamp>,
    created_at: Timestamp,
}

impl BackupMaintenanceRun {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: BackupMaintenanceRunId,
        owner_user_id: UserId,
        operation_id: String,
        request_fingerprint: BackupMaintenanceRunIdempotencyFingerprint,
        backup_set_id: BackupSetId,
        policy_revision_id: BackupSnapshotRetentionPolicyRevisionId,
        policy_revision_number: BackupSnapshotRetentionPolicyRevisionNumber,
        capture_operation_id: String,
        expiry_plan_operation_id: String,
        state: BackupMaintenanceRunState,
        captured_snapshot_id: Option<SnapshotId>,
        expiry_plan_id: Option<BackupSnapshotExpiryPlanId>,
        expiry_execution_id: Option<BackupSnapshotExpiryExecutionId>,
        snapshot_captured_at: Option<Timestamp>,
        expiry_planned_at: Option<Timestamp>,
        maintenance_completed_at: Option<Timestamp>,
        stale_at: Option<Timestamp>,
        created_at: Timestamp,
    ) -> Result<Self, DomainError> {
        let expected = BackupMaintenanceRunRequest::new(backup_set_id).fingerprint();
        if request_fingerprint.version() != BACKUP_MAINTENANCE_RUN_FINGERPRINT_VERSION
            || request_fingerprint != expected
            || !Self::reference_shape_is_valid(
                state,
                captured_snapshot_id,
                expiry_plan_id,
                expiry_execution_id,
                snapshot_captured_at,
                expiry_planned_at,
                maintenance_completed_at,
                stale_at,
                created_at,
            )
        {
            return Err(DomainError::BackupMaintenanceRunInvalidState);
        }

        Ok(Self {
            id,
            owner_user_id,
            operation_id,
            request_fingerprint,
            backup_set_id,
            policy_revision_id,
            policy_revision_number,
            capture_operation_id,
            expiry_plan_operation_id,
            state,
            captured_snapshot_id,
            expiry_plan_id,
            expiry_execution_id,
            snapshot_captured_at,
            expiry_planned_at,
            maintenance_completed_at,
            stale_at,
            created_at,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn reference_shape_is_valid(
        state: BackupMaintenanceRunState,
        captured_snapshot_id: Option<SnapshotId>,
        expiry_plan_id: Option<BackupSnapshotExpiryPlanId>,
        expiry_execution_id: Option<BackupSnapshotExpiryExecutionId>,
        snapshot_captured_at: Option<Timestamp>,
        expiry_planned_at: Option<Timestamp>,
        maintenance_completed_at: Option<Timestamp>,
        stale_at: Option<Timestamp>,
        created_at: Timestamp,
    ) -> bool {
        let timestamps_are_ordered = snapshot_captured_at.is_none_or(|value| value >= created_at)
            && expiry_planned_at.is_none_or(|planned| {
                snapshot_captured_at.is_some_and(|captured| planned >= captured)
            })
            && maintenance_completed_at.is_none_or(|completed| {
                expiry_planned_at.is_some_and(|planned| completed >= planned)
            })
            && stale_at.is_none_or(|value| value >= created_at);
        let terminal_shape = match state {
            BackupMaintenanceRunState::Completed => {
                maintenance_completed_at.is_some() && stale_at.is_none()
            }
            BackupMaintenanceRunState::Stale => {
                maintenance_completed_at.is_none() && stale_at.is_some()
            }
            _ => maintenance_completed_at.is_none() && stale_at.is_none(),
        };
        timestamps_are_ordered
            && terminal_shape
            && match state {
                BackupMaintenanceRunState::Created => {
                    captured_snapshot_id.is_none()
                        && expiry_plan_id.is_none()
                        && expiry_execution_id.is_none()
                        && snapshot_captured_at.is_none()
                        && expiry_planned_at.is_none()
                }
                BackupMaintenanceRunState::SnapshotCaptured => {
                    captured_snapshot_id.is_some()
                        && expiry_plan_id.is_none()
                        && expiry_execution_id.is_none()
                        && snapshot_captured_at.is_some()
                        && expiry_planned_at.is_none()
                }
                BackupMaintenanceRunState::ExpiryPlanned => {
                    captured_snapshot_id.is_some()
                        && expiry_plan_id.is_some()
                        && expiry_execution_id.is_none()
                        && snapshot_captured_at.is_some()
                        && expiry_planned_at.is_some()
                }
                BackupMaintenanceRunState::Completed => {
                    captured_snapshot_id.is_some()
                        && expiry_plan_id.is_some()
                        && expiry_execution_id.is_some()
                        && snapshot_captured_at.is_some()
                        && expiry_planned_at.is_some()
                }
                BackupMaintenanceRunState::Stale => expiry_execution_id.is_none(),
            }
    }

    #[must_use]
    pub const fn id(&self) -> BackupMaintenanceRunId {
        self.id
    }

    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    #[must_use]
    pub const fn request_fingerprint(&self) -> BackupMaintenanceRunIdempotencyFingerprint {
        self.request_fingerprint
    }

    #[must_use]
    pub const fn backup_set_id(&self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn policy_revision_id(&self) -> BackupSnapshotRetentionPolicyRevisionId {
        self.policy_revision_id
    }

    #[must_use]
    pub const fn policy_revision_number(&self) -> BackupSnapshotRetentionPolicyRevisionNumber {
        self.policy_revision_number
    }

    #[must_use]
    pub fn capture_operation_id(&self) -> &str {
        &self.capture_operation_id
    }

    #[must_use]
    pub fn expiry_plan_operation_id(&self) -> &str {
        &self.expiry_plan_operation_id
    }

    #[must_use]
    pub const fn state(&self) -> BackupMaintenanceRunState {
        self.state
    }

    #[must_use]
    pub const fn captured_snapshot_id(&self) -> Option<SnapshotId> {
        self.captured_snapshot_id
    }

    #[must_use]
    pub const fn expiry_plan_id(&self) -> Option<BackupSnapshotExpiryPlanId> {
        self.expiry_plan_id
    }

    #[must_use]
    pub const fn expiry_execution_id(&self) -> Option<BackupSnapshotExpiryExecutionId> {
        self.expiry_execution_id
    }

    #[must_use]
    pub const fn snapshot_captured_at(&self) -> Option<Timestamp> {
        self.snapshot_captured_at
    }

    #[must_use]
    pub const fn expiry_planned_at(&self) -> Option<Timestamp> {
        self.expiry_planned_at
    }

    #[must_use]
    pub const fn maintenance_completed_at(&self) -> Option<Timestamp> {
        self.maintenance_completed_at
    }

    #[must_use]
    pub const fn stale_at(&self) -> Option<Timestamp> {
        self.stale_at
    }

    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }
}

/// The semantic request for an explicit plan. The policy revision and server
/// time are authoritative observations, not caller-controlled intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupSnapshotExpiryPlanRequest {
    backup_set_id: BackupSetId,
}

impl BackupSnapshotExpiryPlanRequest {
    #[must_use]
    pub const fn new(backup_set_id: BackupSetId) -> Self {
        Self { backup_set_id }
    }

    #[must_use]
    pub const fn backup_set_id(self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub fn fingerprint(self) -> BackupSnapshotExpiryPlanIdempotencyFingerprint {
        let mut canonical = Vec::with_capacity(62);
        canonical.extend_from_slice(b"synveil/backup-snapshot-expiry-plan\0");
        canonical.extend_from_slice(&BACKUP_SNAPSHOT_EXPIRY_PLAN_FINGERPRINT_VERSION.to_be_bytes());
        canonical.extend_from_slice(self.backup_set_id.as_bytes());
        let digest = Sha256::digest(canonical);
        let mut sha256 = [0_u8; 32];
        sha256.copy_from_slice(&digest);
        BackupSnapshotExpiryPlanIdempotencyFingerprint::new(
            BACKUP_SNAPSHOT_EXPIRY_PLAN_FINGERPRINT_VERSION,
            sha256,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BackupSnapshotExpiryPlanIdempotencyFingerprint {
    version: u16,
    sha256: [u8; 32],
}

impl BackupSnapshotExpiryPlanIdempotencyFingerprint {
    #[must_use]
    pub const fn new(version: u16, sha256: [u8; 32]) -> Self {
        Self { version, sha256 }
    }

    #[must_use]
    pub const fn version(self) -> u16 {
        self.version
    }

    #[must_use]
    pub const fn sha256(self) -> [u8; 32] {
        self.sha256
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.sha256
    }
}

/// One logical row in the ordered cohort basis used to hash a coherent
/// planning observation. The lifecycle evidence is implicitly COMPLETED.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupSnapshotExpiryBasisEntry {
    snapshot_id: SnapshotId,
    committed_at: Timestamp,
    blocked_by_active_restore_plan: bool,
}

impl BackupSnapshotExpiryBasisEntry {
    #[must_use]
    pub const fn new(
        snapshot_id: SnapshotId,
        committed_at: Timestamp,
        blocked_by_active_restore_plan: bool,
    ) -> Self {
        Self {
            snapshot_id,
            committed_at,
            blocked_by_active_restore_plan,
        }
    }

    #[must_use]
    pub const fn snapshot_id(self) -> SnapshotId {
        self.snapshot_id
    }

    #[must_use]
    pub const fn committed_at(self) -> Timestamp {
        self.committed_at
    }

    #[must_use]
    pub const fn blocked_by_active_restore_plan(self) -> bool {
        self.blocked_by_active_restore_plan
    }
}

/// Versioned SHA-256 seal over the ordered COMPLETED cohort, blocker booleans,
/// backup set, immutable policy revision, and original evaluation time.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BackupSnapshotExpiryBasisFingerprint {
    version: u16,
    sha256: [u8; 32],
}

impl BackupSnapshotExpiryBasisFingerprint {
    #[must_use]
    pub const fn new(version: u16, sha256: [u8; 32]) -> Self {
        Self { version, sha256 }
    }

    #[must_use]
    pub fn calculate(
        backup_set_id: BackupSetId,
        policy_revision_id: BackupSnapshotRetentionPolicyRevisionId,
        evaluated_at: Timestamp,
        ordered_cohort: &[BackupSnapshotExpiryBasisEntry],
    ) -> Self {
        let mut canonical = Vec::with_capacity(96 + ordered_cohort.len().saturating_mul(50));
        canonical.extend_from_slice(b"synveil/backup-snapshot-expiry-basis\0");
        canonical
            .extend_from_slice(&BACKUP_SNAPSHOT_EXPIRY_BASIS_FINGERPRINT_VERSION.to_be_bytes());
        canonical.extend_from_slice(backup_set_id.as_bytes());
        canonical.extend_from_slice(policy_revision_id.as_bytes());
        canonical.extend_from_slice(
            &evaluated_at
                .as_offset_datetime()
                .unix_timestamp_nanos()
                .to_be_bytes(),
        );
        canonical.extend_from_slice(&(ordered_cohort.len() as u64).to_be_bytes());
        for entry in ordered_cohort {
            canonical.extend_from_slice(entry.snapshot_id.as_bytes());
            canonical.extend_from_slice(
                &entry
                    .committed_at
                    .as_offset_datetime()
                    .unix_timestamp_nanos()
                    .to_be_bytes(),
            );
            canonical.extend_from_slice(b"COMPLETED\0");
            canonical.push(u8::from(entry.blocked_by_active_restore_plan));
        }
        let digest = Sha256::digest(canonical);
        let mut sha256 = [0_u8; 32];
        sha256.copy_from_slice(&digest);
        Self::new(BACKUP_SNAPSHOT_EXPIRY_BASIS_FINGERPRINT_VERSION, sha256)
    }

    #[must_use]
    pub const fn version(self) -> u16 {
        self.version
    }

    #[must_use]
    pub const fn sha256(self) -> [u8; 32] {
        self.sha256
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.sha256
    }
}

/// Immutable provenance and aggregate evidence for one non-destructive
/// snapshot-expiry plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupSnapshotExpiryPlan {
    id: BackupSnapshotExpiryPlanId,
    owner_user_id: UserId,
    operation_id: String,
    request_fingerprint: BackupSnapshotExpiryPlanIdempotencyFingerprint,
    backup_set_id: BackupSetId,
    policy_revision_id: BackupSnapshotRetentionPolicyRevisionId,
    policy_revision_number: BackupSnapshotRetentionPolicyRevisionNumber,
    evaluated_at: Timestamp,
    cutoff_at: Timestamp,
    snapshot_basis_fingerprint: BackupSnapshotExpiryBasisFingerprint,
    evaluated_completed_snapshot_count: u64,
    expire_candidate_count: u64,
    keep_latest_count: u64,
    keep_recent_count: u64,
    blocked_active_restore_count: u64,
    state: BackupSnapshotExpiryPlanState,
    created_at: Timestamp,
    stale_at: Option<Timestamp>,
}

impl BackupSnapshotExpiryPlan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: BackupSnapshotExpiryPlanId,
        owner_user_id: UserId,
        operation_id: String,
        request_fingerprint: BackupSnapshotExpiryPlanIdempotencyFingerprint,
        backup_set_id: BackupSetId,
        policy_revision_id: BackupSnapshotRetentionPolicyRevisionId,
        policy_revision_number: BackupSnapshotRetentionPolicyRevisionNumber,
        evaluated_at: Timestamp,
        cutoff_at: Timestamp,
        snapshot_basis_fingerprint: BackupSnapshotExpiryBasisFingerprint,
        evaluated_completed_snapshot_count: u64,
        expire_candidate_count: u64,
        keep_latest_count: u64,
        keep_recent_count: u64,
        blocked_active_restore_count: u64,
        state: BackupSnapshotExpiryPlanState,
        created_at: Timestamp,
        stale_at: Option<Timestamp>,
    ) -> Result<Self, DomainError> {
        let classified_count = keep_latest_count
            .checked_add(keep_recent_count)
            .and_then(|value| value.checked_add(blocked_active_restore_count))
            .and_then(|value| value.checked_add(expire_candidate_count));
        let stale_shape_is_valid =
            (state == BackupSnapshotExpiryPlanState::Stale) == stale_at.is_some();
        let stale_time_is_valid = stale_at.is_none_or(|value| value >= created_at);
        let expected_request_fingerprint =
            BackupSnapshotExpiryPlanRequest::new(backup_set_id).fingerprint();
        if classified_count != Some(evaluated_completed_snapshot_count)
            || !stale_shape_is_valid
            || !stale_time_is_valid
            || cutoff_at >= evaluated_at
            || request_fingerprint.version() != BACKUP_SNAPSHOT_EXPIRY_PLAN_FINGERPRINT_VERSION
            || request_fingerprint != expected_request_fingerprint
            || snapshot_basis_fingerprint.version()
                != BACKUP_SNAPSHOT_EXPIRY_BASIS_FINGERPRINT_VERSION
        {
            return Err(DomainError::BackupSnapshotExpiryInvalidPlanState);
        }
        Ok(Self {
            id,
            owner_user_id,
            operation_id,
            request_fingerprint,
            backup_set_id,
            policy_revision_id,
            policy_revision_number,
            evaluated_at,
            cutoff_at,
            snapshot_basis_fingerprint,
            evaluated_completed_snapshot_count,
            expire_candidate_count,
            keep_latest_count,
            keep_recent_count,
            blocked_active_restore_count,
            state,
            created_at,
            stale_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> BackupSnapshotExpiryPlanId {
        self.id
    }

    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    #[must_use]
    pub const fn request_fingerprint(&self) -> BackupSnapshotExpiryPlanIdempotencyFingerprint {
        self.request_fingerprint
    }

    #[must_use]
    pub const fn backup_set_id(&self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn policy_revision_id(&self) -> BackupSnapshotRetentionPolicyRevisionId {
        self.policy_revision_id
    }

    #[must_use]
    pub const fn policy_revision_number(&self) -> BackupSnapshotRetentionPolicyRevisionNumber {
        self.policy_revision_number
    }

    #[must_use]
    pub const fn evaluated_at(&self) -> Timestamp {
        self.evaluated_at
    }

    #[must_use]
    pub const fn cutoff_at(&self) -> Timestamp {
        self.cutoff_at
    }

    #[must_use]
    pub const fn snapshot_basis_fingerprint(&self) -> BackupSnapshotExpiryBasisFingerprint {
        self.snapshot_basis_fingerprint
    }

    #[must_use]
    pub const fn evaluated_completed_snapshot_count(&self) -> u64 {
        self.evaluated_completed_snapshot_count
    }

    #[must_use]
    pub const fn expire_candidate_count(&self) -> u64 {
        self.expire_candidate_count
    }

    #[must_use]
    pub const fn keep_latest_count(&self) -> u64 {
        self.keep_latest_count
    }

    #[must_use]
    pub const fn keep_recent_count(&self) -> u64 {
        self.keep_recent_count
    }

    #[must_use]
    pub const fn blocked_active_restore_count(&self) -> u64 {
        self.blocked_active_restore_count
    }

    #[must_use]
    pub const fn state(&self) -> BackupSnapshotExpiryPlanState {
        self.state
    }

    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    #[must_use]
    pub const fn stale_at(&self) -> Option<Timestamp> {
        self.stale_at
    }

    pub fn transition_state(
        &mut self,
        next: BackupSnapshotExpiryPlanState,
        stale_at: Option<Timestamp>,
    ) -> Result<(), DomainError> {
        let allowed = matches!(
            (self.state, next),
            (
                BackupSnapshotExpiryPlanState::Planned,
                BackupSnapshotExpiryPlanState::Stale
            ) | (
                BackupSnapshotExpiryPlanState::Stale,
                BackupSnapshotExpiryPlanState::Stale
            ) | (
                BackupSnapshotExpiryPlanState::Planned,
                BackupSnapshotExpiryPlanState::Executed
            ) | (
                BackupSnapshotExpiryPlanState::Executed,
                BackupSnapshotExpiryPlanState::Executed
            )
        );
        if !allowed
            || (next == BackupSnapshotExpiryPlanState::Stale) != stale_at.is_some()
            || stale_at.is_some_and(|value| value < self.created_at)
            || (self.state == BackupSnapshotExpiryPlanState::Stale && stale_at != self.stale_at)
            || (next == BackupSnapshotExpiryPlanState::Executed && stale_at.is_some())
            || (self.state == BackupSnapshotExpiryPlanState::Executed
                && next == BackupSnapshotExpiryPlanState::Executed
                && stale_at.is_some())
        {
            return Err(DomainError::BackupSnapshotExpiryInvalidPlanState);
        }
        self.state = next;
        self.stale_at = stale_at;
        Ok(())
    }
}

/// One immutable decision entry for one COMPLETED snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupSnapshotExpiryPlanEntry {
    plan_id: BackupSnapshotExpiryPlanId,
    snapshot_id: SnapshotId,
    committed_at: Timestamp,
    recency_rank: u64,
    decision: BackupSnapshotExpiryDecision,
}

impl BackupSnapshotExpiryPlanEntry {
    pub fn new(
        plan_id: BackupSnapshotExpiryPlanId,
        snapshot_id: SnapshotId,
        committed_at: Timestamp,
        recency_rank: u64,
        decision: BackupSnapshotExpiryDecision,
    ) -> Result<Self, DomainError> {
        if recency_rank == 0 || i64::try_from(recency_rank).is_err() {
            return Err(DomainError::BackupSnapshotExpiryInvalidPlanEntry);
        }
        Ok(Self {
            plan_id,
            snapshot_id,
            committed_at,
            recency_rank,
            decision,
        })
    }

    #[must_use]
    pub const fn plan_id(self) -> BackupSnapshotExpiryPlanId {
        self.plan_id
    }

    #[must_use]
    pub const fn snapshot_id(self) -> SnapshotId {
        self.snapshot_id
    }

    #[must_use]
    pub const fn committed_at(self) -> Timestamp {
        self.committed_at
    }

    #[must_use]
    pub const fn recency_rank(self) -> u64 {
        self.recency_rank
    }

    #[must_use]
    pub const fn decision(self) -> BackupSnapshotExpiryDecision {
        self.decision
    }
}

/// Version of the canonical snapshot-expiry execution receipt semantics.
pub const BACKUP_SNAPSHOT_EXPIRY_EXECUTION_FINGERPRINT_VERSION: u16 = 1;

/// Canonical immutable receipt for one atomic snapshot-expiry plan execution.
/// The receipt is the authoritative evidence that a PLANNED expiry decision set
/// was applied exactly once. Physical Object and replica identity deliberately
/// does not cross this boundary; only logical counts, identifiers, policies,
/// and timestamps are exposed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupSnapshotExpiryExecution {
    id: BackupSnapshotExpiryExecutionId,
    owner_user_id: UserId,
    expiry_plan_id: BackupSnapshotExpiryPlanId,
    backup_set_id: BackupSetId,
    policy_revision_id: BackupSnapshotRetentionPolicyRevisionId,
    evaluated_at: Timestamp,
    evaluated_snapshot_count: u64,
    expired_snapshot_count: u64,
    unchanged_snapshot_count: u64,
    executed_at: Timestamp,
}

impl BackupSnapshotExpiryExecution {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: BackupSnapshotExpiryExecutionId,
        owner_user_id: UserId,
        expiry_plan_id: BackupSnapshotExpiryPlanId,
        backup_set_id: BackupSetId,
        policy_revision_id: BackupSnapshotRetentionPolicyRevisionId,
        evaluated_at: Timestamp,
        evaluated_snapshot_count: u64,
        expired_snapshot_count: u64,
        unchanged_snapshot_count: u64,
        executed_at: Timestamp,
    ) -> Result<Self, DomainError> {
        let unchanged = evaluated_snapshot_count
            .checked_sub(expired_snapshot_count)
            .ok_or(DomainError::BackupSnapshotExpiryInvalidExecution)?;
        if unchanged != unchanged_snapshot_count || executed_at < evaluated_at {
            return Err(DomainError::BackupSnapshotExpiryInvalidExecution);
        }
        Ok(Self {
            id,
            owner_user_id,
            expiry_plan_id,
            backup_set_id,
            policy_revision_id,
            evaluated_at,
            evaluated_snapshot_count,
            expired_snapshot_count,
            unchanged_snapshot_count,
            executed_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> BackupSnapshotExpiryExecutionId {
        self.id
    }

    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn expiry_plan_id(&self) -> BackupSnapshotExpiryPlanId {
        self.expiry_plan_id
    }

    #[must_use]
    pub const fn backup_set_id(&self) -> BackupSetId {
        self.backup_set_id
    }

    #[must_use]
    pub const fn policy_revision_id(&self) -> BackupSnapshotRetentionPolicyRevisionId {
        self.policy_revision_id
    }

    #[must_use]
    pub const fn evaluated_at(&self) -> Timestamp {
        self.evaluated_at
    }

    #[must_use]
    pub const fn evaluated_snapshot_count(&self) -> u64 {
        self.evaluated_snapshot_count
    }

    #[must_use]
    pub const fn expired_snapshot_count(&self) -> u64 {
        self.expired_snapshot_count
    }

    #[must_use]
    pub const fn unchanged_snapshot_count(&self) -> u64 {
        self.unchanged_snapshot_count
    }

    #[must_use]
    pub const fn executed_at(&self) -> Timestamp {
        self.executed_at
    }
}

/// Immutable per-plan-entry execution evidence. Every persisted Prompt 47
/// decision entry maps to exactly one evidence row recording whether its
/// snapshot lifecycle transitioned (`COMPLETED` -> `EXPIRED`) this execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupSnapshotExpiryExecutionEntry {
    execution_id: BackupSnapshotExpiryExecutionId,
    expiry_plan_entry_plan_id: BackupSnapshotExpiryPlanId,
    snapshot_id: SnapshotId,
    original_decision: BackupSnapshotExpiryDecision,
    transitioned: bool,
}

impl BackupSnapshotExpiryExecutionEntry {
    #[must_use]
    pub const fn new(
        execution_id: BackupSnapshotExpiryExecutionId,
        expiry_plan_entry_plan_id: BackupSnapshotExpiryPlanId,
        snapshot_id: SnapshotId,
        original_decision: BackupSnapshotExpiryDecision,
        transitioned: bool,
    ) -> Self {
        Self {
            execution_id,
            expiry_plan_entry_plan_id,
            snapshot_id,
            original_decision,
            transitioned,
        }
    }

    #[must_use]
    pub const fn execution_id(&self) -> BackupSnapshotExpiryExecutionId {
        self.execution_id
    }

    #[must_use]
    pub const fn expiry_plan_entry_plan_id(&self) -> BackupSnapshotExpiryPlanId {
        self.expiry_plan_entry_plan_id
    }

    #[must_use]
    pub const fn snapshot_id(&self) -> SnapshotId {
        self.snapshot_id
    }

    #[must_use]
    pub const fn original_decision(&self) -> BackupSnapshotExpiryDecision {
        self.original_decision
    }

    #[must_use]
    pub const fn transitioned(&self) -> bool {
        self.transitioned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observed_at() -> Timestamp {
        Timestamp::parse("2026-08-29T00:00:00Z").expect("fixed timestamp is valid")
    }

    fn name(value: &str) -> LogicalName {
        LogicalName::new(value).expect("test logical name is valid")
    }

    #[test]
    fn backup_source_round_trips() {
        assert_eq!(BackupSource::Library.as_str(), "LIBRARY");
        assert_eq!("LIBRARY".parse::<BackupSource>(), Ok(BackupSource::Library));
        assert!("UNKNOWN".parse::<BackupSource>().is_err());
    }

    #[test]
    fn backup_set_states_are_closed_and_canonical() {
        for state in [
            BackupSetState::Created,
            BackupSetState::Active,
            BackupSetState::Disabled,
        ] {
            assert_eq!(BackupSetState::from_str(state.as_str()), Ok(state));
        }
        assert!("DELETED".parse::<BackupSetState>().is_err());
    }

    #[test]
    fn snapshot_states_are_closed_and_canonical() {
        for state in [
            SnapshotState::Building,
            SnapshotState::Completed,
            SnapshotState::Failed,
            SnapshotState::Expired,
        ] {
            assert_eq!(SnapshotState::from_str(state.as_str()), Ok(state));
        }
        assert!("ARCHIVED".parse::<SnapshotState>().is_err());
        assert!("RETAINED".parse::<SnapshotState>().is_err());
    }

    #[test]
    fn snapshot_restorable_only_when_completed() {
        assert!(!SnapshotState::Building.is_restorable());
        assert!(SnapshotState::Completed.is_restorable());
        assert!(!SnapshotState::Failed.is_restorable());
        assert!(!SnapshotState::Expired.is_restorable());
    }

    #[test]
    fn backup_set_created_with_correct_initial_state() {
        let set = BackupSet::new(
            BackupSetId::new(),
            UserId::new(),
            name("daily-backup"),
            LibraryId::new(),
            Some(30),
            observed_at(),
        )
        .expect("backup set is valid");

        assert_eq!(set.state(), BackupSetState::Created);
        assert_eq!(set.name().as_str(), "daily-backup");
        assert_eq!(set.retention_days(), Some(30));
        assert_eq!(set.source(), BackupSource::Library);
        assert_eq!(set.revision(), Revision::new(0));
    }

    #[test]
    fn backup_set_activation_transitions_from_created() {
        let mut set = BackupSet::new(
            BackupSetId::new(),
            UserId::new(),
            name("set"),
            LibraryId::new(),
            None,
            observed_at(),
        )
        .unwrap();

        set.activate(observed_at()).unwrap();
        assert_eq!(set.state(), BackupSetState::Active);
        assert_eq!(set.revision(), Revision::new(1));

        assert_eq!(
            set.activate(observed_at()),
            Err(DomainError::InvalidBackupSetStateTransition {
                from: BackupSetState::Active,
                to: BackupSetState::Active,
            })
        );
    }

    #[test]
    fn backup_set_disable_from_created_or_active() {
        let mut set = BackupSet::new(
            BackupSetId::new(),
            UserId::new(),
            name("set"),
            LibraryId::new(),
            None,
            observed_at(),
        )
        .unwrap();

        set.disable(observed_at()).unwrap();
        assert_eq!(set.state(), BackupSetState::Disabled);

        assert_eq!(
            set.disable(observed_at()),
            Err(DomainError::InvalidBackupSetStateTransition {
                from: BackupSetState::Disabled,
                to: BackupSetState::Disabled,
            })
        );
    }

    #[test]
    fn backup_manifest_content_carries_only_logical_reference() {
        let fv_id = FileVersionId::new();
        let digest = Sha256Digest::from_bytes([0xabu8; 32]);
        let content = BackupManifestContent::new(fv_id, 42, digest);

        assert_eq!(content.file_version_id(), fv_id);
        assert_eq!(content.byte_length(), 42);
        assert_eq!(content.sha256(), digest);
    }

    #[test]
    fn backup_snapshot_node_rejects_purging_state() {
        assert_eq!(
            BackupSnapshotNode::new(
                NodeId::new(),
                Some(NodeId::new()),
                name("file.txt"),
                NodeKind::File,
                NodeState::Purging,
                Revision::new(1),
                None,
                observed_at(),
                observed_at(),
            ),
            Err(DomainError::BackupManifestInvalidNodeState)
        );
    }

    #[test]
    fn backup_snapshot_node_rejects_directory_with_content() {
        assert_eq!(
            BackupSnapshotNode::new(
                NodeId::new(),
                Some(NodeId::new()),
                name("dir"),
                NodeKind::Directory,
                NodeState::Active,
                Revision::new(1),
                Some(BackupManifestContent::new(
                    FileVersionId::new(),
                    10,
                    Sha256Digest::from_bytes([0; 32]),
                )),
                observed_at(),
                observed_at(),
            ),
            Err(DomainError::BackupManifestDirectoryHasContent)
        );
    }

    #[test]
    fn backup_snapshot_node_rejects_non_active_root() {
        assert_eq!(
            BackupSnapshotNode::new(
                NodeId::new(),
                None,
                name("root"),
                NodeKind::File,
                NodeState::Active,
                Revision::new(1),
                None,
                observed_at(),
                observed_at(),
            ),
            Err(DomainError::BackupManifestInvalidRootShape)
        );
    }

    #[test]
    fn backup_snapshot_node_root_must_be_directory() {
        let node = BackupSnapshotNode::new(
            NodeId::new(),
            None,
            name("root"),
            NodeKind::Directory,
            NodeState::Active,
            Revision::new(1),
            None,
            observed_at(),
            observed_at(),
        )
        .unwrap();

        assert_eq!(node.kind(), NodeKind::Directory);
        assert!(node.parent_node_id().is_none());
        assert!(node.content().is_none());
    }

    #[test]
    fn backup_snapshot_node_file_with_content_is_valid() {
        let digest = Sha256Digest::from_bytes([0x5a; 32]);
        let fv_id = FileVersionId::new();
        let node = BackupSnapshotNode::new(
            NodeId::new(),
            Some(NodeId::new()),
            name("document.pdf"),
            NodeKind::File,
            NodeState::Active,
            Revision::new(5),
            Some(BackupManifestContent::new(fv_id, 1024, digest)),
            observed_at(),
            observed_at(),
        )
        .unwrap();

        let content = node.content().expect("file has content");
        assert_eq!(content.file_version_id(), fv_id);
        assert_eq!(content.byte_length(), 1024);
        assert_eq!(content.sha256(), digest);
    }

    #[test]
    fn backup_set_id_and_snapshot_id_are_distinct() {
        use std::any::TypeId;

        assert_ne!(TypeId::of::<BackupSetId>(), TypeId::of::<SnapshotId>());
        assert_ne!(
            TypeId::of::<BackupSnapshotRetentionPolicyRevisionId>(),
            TypeId::of::<BackupSnapshotExpiryPlanId>()
        );
        assert_ne!(
            TypeId::of::<BackupSnapshotExpiryPlanId>(),
            TypeId::of::<crate::ObjectId>()
        );
    }

    #[test]
    fn backup_snapshot_state_machine_allows_valid_transitions() {
        let now = observed_at();
        let mut snapshot = BackupSnapshot::new(
            SnapshotId::new(),
            BackupSetId::new(),
            UserId::new(),
            LibraryId::new(),
            Sequence::new(1),
            Sequence::new(0),
            0,
            None,
            0,
            SnapshotState::Building,
            now,
            None,
            None,
        );

        snapshot
            .transition_state(SnapshotState::Completed, now)
            .unwrap();
        assert_eq!(snapshot.state(), SnapshotState::Completed);
        assert!(snapshot.committed_at().is_some());

        snapshot
            .transition_state(SnapshotState::Expired, now)
            .unwrap();
        assert_eq!(snapshot.state(), SnapshotState::Expired);
        assert!(snapshot.expired_at().is_some());
    }

    #[test]
    fn backup_snapshot_rejects_invalid_transitions() {
        let now = observed_at();
        let mut snapshot = BackupSnapshot::new(
            SnapshotId::new(),
            BackupSetId::new(),
            UserId::new(),
            LibraryId::new(),
            Sequence::new(1),
            Sequence::new(0),
            0,
            None,
            0,
            SnapshotState::Completed,
            now,
            Some(now),
            None,
        );

        assert_eq!(
            snapshot.transition_state(SnapshotState::Building, now),
            Err(DomainError::InvalidSnapshotStateTransition {
                from: SnapshotState::Completed,
                to: SnapshotState::Building,
            })
        );

        let mut expired = snapshot.clone();
        expired
            .transition_state(SnapshotState::Expired, now)
            .expect("completed snapshot can expire");
        assert_eq!(expired.state(), SnapshotState::Expired);
        assert_eq!(
            expired.transition_state(SnapshotState::Completed, now),
            Err(DomainError::InvalidSnapshotStateTransition {
                from: SnapshotState::Expired,
                to: SnapshotState::Completed,
            })
        );
    }

    #[test]
    fn backup_snapshot_building_is_not_restorable() {
        let snapshot = BackupSnapshot::new(
            SnapshotId::new(),
            BackupSetId::new(),
            UserId::new(),
            LibraryId::new(),
            Sequence::new(1),
            Sequence::new(0),
            0,
            None,
            0,
            SnapshotState::Building,
            observed_at(),
            None,
            None,
        );

        assert!(!snapshot.is_restorable());
    }

    #[test]
    fn backup_snapshot_completed_is_restorable() {
        let now = observed_at();
        let mut snapshot = BackupSnapshot::new(
            SnapshotId::new(),
            BackupSetId::new(),
            UserId::new(),
            LibraryId::new(),
            Sequence::new(1),
            Sequence::new(0),
            0,
            None,
            0,
            SnapshotState::Building,
            now,
            None,
            None,
        );
        snapshot
            .transition_state(SnapshotState::Completed, now)
            .unwrap();
        assert!(snapshot.is_restorable());
    }

    #[test]
    fn restore_plan_states_and_actions_are_closed_and_canonical() {
        for state in [
            BackupRestorePlanState::Planned,
            BackupRestorePlanState::Stale,
            BackupRestorePlanState::Executed,
        ] {
            assert_eq!(state.as_str().parse::<BackupRestorePlanState>(), Ok(state));
        }
        for action in [
            BackupRestoreAction::CreateDirectory,
            BackupRestoreAction::CreateFile,
        ] {
            assert_eq!(action.as_str().parse::<BackupRestoreAction>(), Ok(action));
        }
        assert!("EXECUTING".parse::<BackupRestorePlanState>().is_err());
        assert!("OVERWRITE".parse::<BackupRestoreAction>().is_err());
    }

    #[test]
    fn restore_plan_execution_lifecycle_is_one_way() {
        let now = observed_at();
        let request = BackupRestorePlanRequest::new(
            BackupSetId::new(),
            SnapshotId::new(),
            LibraryId::new(),
            NodeId::new(),
            name("Recovered"),
        );
        let mut executed = BackupRestorePlan::new(
            BackupRestorePlanId::new(),
            UserId::new(),
            "restore-plan-execution".to_owned(),
            request.fingerprint(),
            request.clone(),
            Sequence::new(1),
            Sequence::new(0),
            1,
            0,
            BackupRestorePlanState::Planned,
            now,
            None,
        )
        .expect("planned plan is valid");
        executed
            .transition_state(BackupRestorePlanState::Executed, None)
            .expect("planned plan can execute");
        assert_eq!(executed.state(), BackupRestorePlanState::Executed);
        assert_eq!(executed.stale_at(), None);
        assert!(
            executed
                .transition_state(BackupRestorePlanState::Planned, None)
                .is_err()
        );

        let mut stale = BackupRestorePlan::new(
            BackupRestorePlanId::new(),
            UserId::new(),
            "restore-plan-stale".to_owned(),
            request.fingerprint(),
            request,
            Sequence::new(1),
            Sequence::new(0),
            1,
            0,
            BackupRestorePlanState::Planned,
            now,
            None,
        )
        .expect("planned plan is valid");
        stale
            .transition_state(BackupRestorePlanState::Stale, Some(now))
            .expect("planned plan can stale");
        assert_eq!(stale.state(), BackupRestorePlanState::Stale);
        assert_eq!(stale.stale_at(), Some(now));
        assert!(
            stale
                .transition_state(BackupRestorePlanState::Executed, None)
                .is_err()
        );
    }

    #[test]
    fn restore_execution_receipt_and_entries_reject_invalid_shapes() {
        let now = observed_at();
        assert!(
            BackupRestoreExecution::new(
                BackupRestoreExecutionId::new(),
                UserId::new(),
                BackupRestorePlanId::new(),
                LibraryId::new(),
                Sequence::new(0),
                Sequence::new(1),
                1,
                0,
                now,
            )
            .is_err()
        );

        let execution_id = BackupRestoreExecutionId::new();
        let plan_id = BackupRestorePlanId::new();
        let node_id = NodeId::new();
        assert!(
            BackupRestoreExecutionEntry::new(
                execution_id,
                plan_id,
                0,
                node_id,
                Some(FileVersionId::new()),
                Some(Sequence::new(1)),
                Some(Sequence::new(2)),
            )
            .is_err()
        );
        assert!(
            BackupRestoreExecutionEntry::new(
                execution_id,
                plan_id,
                0,
                node_id,
                None,
                Some(Sequence::new(1)),
                None,
            )
            .is_ok()
        );
        assert!(
            BackupRestoreExecutionEntry::new(
                execution_id,
                plan_id,
                1,
                NodeId::new(),
                Some(FileVersionId::new()),
                None,
                Some(Sequence::new(2)),
            )
            .is_ok()
        );
    }

    #[test]
    fn restore_plan_entry_enforces_wrapper_and_content_shapes() {
        let plan_id = BackupRestorePlanId::new();
        let wrapper = BackupRestorePlanEntry::new(
            plan_id,
            0,
            NodeId::new(),
            NodeId::new(),
            None,
            None,
            NodeState::Active,
            BackupRestoreAction::CreateDirectory,
            NodeKind::Directory,
            name("Recovered"),
            None,
            None,
        )
        .expect("wrapper shape is valid");
        assert!(wrapper.is_wrapper());

        let content = BackupManifestContent::new(
            FileVersionId::new(),
            42,
            Sha256Digest::from_bytes([0x5a; 32]),
        );
        let file = BackupRestorePlanEntry::new(
            plan_id,
            1,
            NodeId::new(),
            wrapper.planned_node_id(),
            Some(NodeId::new()),
            Some(NodeId::new()),
            NodeState::Active,
            BackupRestoreAction::CreateFile,
            NodeKind::File,
            name("file.txt"),
            Some(Revision::new(2)),
            Some(content),
        )
        .expect("file content shape is valid");
        assert_eq!(file.content(), Some(&content));
        assert_eq!(
            BackupRestorePlanEntry::new(
                plan_id,
                1,
                NodeId::new(),
                wrapper.planned_node_id(),
                Some(NodeId::new()),
                Some(NodeId::new()),
                NodeState::Active,
                BackupRestoreAction::CreateDirectory,
                NodeKind::Directory,
                name("wrong"),
                Some(Revision::new(1)),
                Some(content),
            ),
            Err(DomainError::BackupRestoreInvalidPlanEntry)
        );
    }

    #[test]
    fn restore_plan_request_fingerprint_is_stable_and_semantic() {
        let request = BackupRestorePlanRequest::new(
            BackupSetId::new(),
            SnapshotId::new(),
            LibraryId::new(),
            NodeId::new(),
            name("Recovered"),
        );
        assert_eq!(request.fingerprint(), request.fingerprint());
        let changed = BackupRestorePlanRequest::new(
            request.backup_set_id(),
            request.snapshot_id(),
            request.target_library_id(),
            request.target_parent_node_id(),
            name("Recovered-2"),
        );
        assert_ne!(request.fingerprint(), changed.fingerprint());
        assert_eq!(
            request.fingerprint().version(),
            BACKUP_RESTORE_PLAN_FINGERPRINT_VERSION
        );
    }

    #[test]
    fn prune_plan_states_and_impacts_are_closed_and_canonical() {
        for state in [
            BackupPrunePlanState::Planned,
            BackupPrunePlanState::Stale,
            BackupPrunePlanState::Executed,
        ] {
            assert_eq!(state.as_str().parse::<BackupPrunePlanState>(), Ok(state));
        }
        for impact in [
            BackupPruneImpact::RetainedByOtherReference,
            BackupPruneImpact::WouldBecomeUnreferenced,
        ] {
            assert_eq!(impact.as_str().parse::<BackupPruneImpact>(), Ok(impact));
        }
        assert!("EXECUTING".parse::<BackupPrunePlanState>().is_err());
        assert!("SAFE_TO_DELETE".parse::<BackupPruneImpact>().is_err());
    }

    #[test]
    fn prune_plan_request_fingerprint_is_stable_and_snapshot_scoped() {
        let request = BackupPrunePlanRequest::new(BackupSetId::new(), SnapshotId::new());
        assert_eq!(request.fingerprint(), request.fingerprint());
        assert_eq!(
            request.fingerprint().version(),
            BACKUP_PRUNE_PLAN_FINGERPRINT_VERSION
        );
        assert_ne!(
            request.fingerprint(),
            BackupPrunePlanRequest::new(request.backup_set_id(), SnapshotId::new()).fingerprint()
        );
    }

    #[test]
    fn prune_plan_enforces_reference_accounting_aggregates_and_one_way_staleness() {
        let now = observed_at();
        let request = BackupPrunePlanRequest::new(BackupSetId::new(), SnapshotId::new());
        assert!(
            BackupPrunePlan::new(
                BackupPrunePlanId::new(),
                UserId::new(),
                "prune-aggregate-check".to_owned(),
                request.fingerprint(),
                request,
                3,
                2,
                1,
                1,
                1,
                0,
                BackupPrunePlanState::Planned,
                now,
                None,
            )
            .is_err()
        );

        let mut plan = BackupPrunePlan::new(
            BackupPrunePlanId::new(),
            UserId::new(),
            "prune-stale-check".to_owned(),
            request.fingerprint(),
            request,
            3,
            2,
            2,
            1,
            0,
            1,
            BackupPrunePlanState::Planned,
            now,
            None,
        )
        .expect("coherent plan aggregates are valid");
        plan.transition_state(BackupPrunePlanState::Stale, Some(now))
            .expect("planned plan can become stale");
        assert!(
            plan.transition_state(BackupPrunePlanState::Planned, None)
                .is_err()
        );
        assert!(
            plan.transition_state(BackupPrunePlanState::Executed, None)
                .is_err()
        );
        assert!(
            plan.transition_state(
                BackupPrunePlanState::Stale,
                Some(
                    Timestamp::parse("2026-08-29T00:00:01Z")
                        .expect("fixed stale timestamp is valid"),
                ),
            )
            .is_err()
        );
    }

    #[test]
    fn prune_plan_execution_lifecycle_is_one_way_and_terminal() {
        let now = observed_at();
        let request = BackupPrunePlanRequest::new(BackupSetId::new(), SnapshotId::new());
        let mut executed = BackupPrunePlan::new(
            BackupPrunePlanId::new(),
            UserId::new(),
            "prune-execution-check".to_owned(),
            request.fingerprint(),
            request,
            3,
            2,
            2,
            1,
            0,
            1,
            BackupPrunePlanState::Planned,
            now,
            None,
        )
        .expect("planned plan is valid");
        executed
            .transition_state(BackupPrunePlanState::Executed, None)
            .expect("planned plan can execute");
        assert_eq!(executed.state(), BackupPrunePlanState::Executed);
        assert_eq!(executed.stale_at(), None);
        assert!(
            executed
                .transition_state(BackupPrunePlanState::Planned, None)
                .is_err()
        );
        assert!(
            executed
                .transition_state(BackupPrunePlanState::Stale, Some(now))
                .is_err()
        );
    }

    #[test]
    fn prune_execution_receipt_validates_aggregate_shapes() {
        let now = observed_at();
        assert!(
            BackupPruneExecution::new(
                BackupPruneExecutionId::new(),
                UserId::new(),
                BackupPrunePlanId::new(),
                BackupSetId::new(),
                SnapshotId::new(),
                2,
                2,
                1,
                0,
                now,
            )
            .is_err(),
            "retained + gc_handoff must equal distinct object count"
        );
        let receipt = BackupPruneExecution::new(
            BackupPruneExecutionId::new(),
            UserId::new(),
            BackupPrunePlanId::new(),
            BackupSetId::new(),
            SnapshotId::new(),
            2,
            1,
            0,
            1,
            now,
        )
        .expect("coherent receipt is valid");
        assert_eq!(receipt.released_pin_count(), 2);
        assert_eq!(receipt.distinct_object_count(), 1);
        assert_eq!(receipt.retained_by_other_reference_count(), 0);
        assert_eq!(receipt.gc_handoff_object_count(), 1);
        assert_eq!(receipt.executed_at(), now);

        let zero = BackupPruneExecution::new(
            BackupPruneExecutionId::new(),
            UserId::new(),
            BackupPrunePlanId::new(),
            BackupSetId::new(),
            SnapshotId::new(),
            0,
            0,
            0,
            0,
            now,
        )
        .expect("zero-content receipt is valid");
        assert_eq!(zero.distinct_object_count(), 0);
    }

    #[test]
    fn prune_logical_entry_carries_only_manifest_content() {
        let content = BackupManifestContent::new(
            FileVersionId::new(),
            512,
            Sha256Digest::from_bytes([0x7c; 32]),
        );
        let entry = BackupPrunePlanEntry::new(BackupPrunePlanId::new(), 0, NodeId::new(), content);
        assert_eq!(entry.content(), &content);
        assert_eq!(entry.ordinal(), 0);
    }

    #[test]
    fn retention_policy_config_rejects_zero_and_unrepresentable_values() {
        assert_eq!(
            BackupSnapshotRetentionPolicyConfig::new(0, 1),
            Err(DomainError::BackupRetentionPolicyInvalidConfig)
        );
        assert_eq!(
            BackupSnapshotRetentionPolicyConfig::new(1, 0),
            Err(DomainError::BackupRetentionPolicyInvalidConfig)
        );
        assert_eq!(
            BackupSnapshotRetentionPolicyConfig::new(1, MAX_BACKUP_SNAPSHOT_RETENTION_SECONDS + 1,),
            Err(DomainError::BackupRetentionPolicyInvalidConfig)
        );
        assert!(
            BackupSnapshotRetentionPolicyConfig::new(1, MAX_BACKUP_SNAPSHOT_RETENTION_SECONDS,)
                .is_ok()
        );
        let config = BackupSnapshotRetentionPolicyConfig::new(3, 86_400)
            .expect("positive bounded policy is valid");
        assert_eq!(config.keep_latest_completed(), 3);
        assert_eq!(config.expire_after_seconds(), 86_400);
        assert_eq!(config.expire_after().as_secs(), 86_400);
    }

    #[test]
    fn retention_policy_fingerprint_is_stable_versioned_and_semantic() {
        let backup_set_id = BackupSetId::new();
        let config = BackupSnapshotRetentionPolicyConfig::new(2, 2_592_000).unwrap();
        let request = BackupSnapshotRetentionPolicyRequest::new(backup_set_id, config);
        assert_eq!(request.fingerprint(), request.fingerprint());
        assert_eq!(
            request.fingerprint().version(),
            BACKUP_SNAPSHOT_RETENTION_POLICY_FINGERPRINT_VERSION
        );
        assert_ne!(
            request.fingerprint(),
            BackupSnapshotRetentionPolicyRequest::new(
                backup_set_id,
                BackupSnapshotRetentionPolicyConfig::new(3, 2_592_000).unwrap(),
            )
            .fingerprint()
        );
        assert_ne!(
            request.fingerprint(),
            BackupSnapshotRetentionPolicyRequest::new(BackupSetId::new(), config).fingerprint()
        );
        assert_eq!(
            BackupSnapshotRetentionPolicyRevision::new(
                BackupSnapshotRetentionPolicyRevisionId::new(),
                UserId::new(),
                backup_set_id,
                BackupSnapshotRetentionPolicyRevisionNumber::new(1).unwrap(),
                "invalid-policy-fingerprint".to_owned(),
                BackupSnapshotRetentionPolicyIdempotencyFingerprint::new(1, [0; 32]),
                config,
                observed_at(),
            ),
            Err(DomainError::BackupRetentionPolicyInvalidRevision)
        );
    }

    #[test]
    fn retention_policy_revision_numbers_parse_order_and_overflow_safely() {
        let one = "1"
            .parse::<BackupSnapshotRetentionPolicyRevisionNumber>()
            .expect("revision one is valid");
        let two = one.checked_next().expect("revision two is representable");
        assert_eq!(one.get(), 1);
        assert_eq!(two.to_string(), "2");
        assert!(one < two);
        assert!(
            "0".parse::<BackupSnapshotRetentionPolicyRevisionNumber>()
                .is_err()
        );
        assert!(
            BackupSnapshotRetentionPolicyRevisionNumber::new(i64::MAX as u64)
                .unwrap()
                .checked_next()
                .is_err()
        );
    }

    #[test]
    fn expiry_states_and_decisions_are_closed_and_canonical() {
        for state in [
            BackupSnapshotExpiryPlanState::Planned,
            BackupSnapshotExpiryPlanState::Stale,
            BackupSnapshotExpiryPlanState::Executed,
        ] {
            assert_eq!(
                state.as_str().parse::<BackupSnapshotExpiryPlanState>(),
                Ok(state)
            );
        }
        for decision in [
            BackupSnapshotExpiryDecision::KeepLatest,
            BackupSnapshotExpiryDecision::KeepRecent,
            BackupSnapshotExpiryDecision::BlockedActiveRestorePlan,
            BackupSnapshotExpiryDecision::Expire,
        ] {
            assert_eq!(
                decision.as_str().parse::<BackupSnapshotExpiryDecision>(),
                Ok(decision)
            );
        }
        assert!(
            "EXECUTING"
                .parse::<BackupSnapshotExpiryPlanState>()
                .is_err()
        );
        assert!("DELETED".parse::<BackupSnapshotExpiryPlanState>().is_err());
        assert!("DELETE".parse::<BackupSnapshotExpiryDecision>().is_err());
    }

    #[test]
    fn expiry_basis_fingerprint_is_ordered_and_restore_blocker_sensitive() {
        let backup_set_id = BackupSetId::new();
        let policy_revision_id = BackupSnapshotRetentionPolicyRevisionId::new();
        let evaluated_at = Timestamp::parse("2026-08-30T00:00:00Z").unwrap();
        let committed_at = Timestamp::parse("2026-08-01T00:00:00Z").unwrap();
        let first = BackupSnapshotExpiryBasisEntry::new(SnapshotId::new(), committed_at, false);
        let second = BackupSnapshotExpiryBasisEntry::new(SnapshotId::new(), committed_at, false);
        let original = BackupSnapshotExpiryBasisFingerprint::calculate(
            backup_set_id,
            policy_revision_id,
            evaluated_at,
            &[first, second],
        );
        assert_eq!(
            original,
            BackupSnapshotExpiryBasisFingerprint::calculate(
                backup_set_id,
                policy_revision_id,
                evaluated_at,
                &[first, second],
            )
        );
        assert_ne!(
            original,
            BackupSnapshotExpiryBasisFingerprint::calculate(
                backup_set_id,
                policy_revision_id,
                evaluated_at,
                &[second, first],
            )
        );
        assert_ne!(
            original,
            BackupSnapshotExpiryBasisFingerprint::calculate(
                backup_set_id,
                policy_revision_id,
                evaluated_at,
                &[
                    BackupSnapshotExpiryBasisEntry::new(
                        first.snapshot_id(),
                        first.committed_at(),
                        true,
                    ),
                    second,
                ],
            )
        );
    }

    #[test]
    fn expiry_plan_entry_and_one_way_staleness_validate_shapes() {
        let plan_id = BackupSnapshotExpiryPlanId::new();
        let committed_at = Timestamp::parse("2026-08-01T00:00:00Z").unwrap();
        assert_eq!(
            BackupSnapshotExpiryPlanEntry::new(
                plan_id,
                SnapshotId::new(),
                committed_at,
                0,
                BackupSnapshotExpiryDecision::Expire,
            ),
            Err(DomainError::BackupSnapshotExpiryInvalidPlanEntry)
        );
        assert!(
            BackupSnapshotExpiryPlanEntry::new(
                plan_id,
                SnapshotId::new(),
                committed_at,
                1,
                BackupSnapshotExpiryDecision::KeepLatest,
            )
            .is_ok()
        );

        let backup_set_id = BackupSetId::new();
        let request = BackupSnapshotExpiryPlanRequest::new(backup_set_id);
        let evaluated_at = Timestamp::parse("2026-08-30T00:00:00Z").unwrap();
        let cutoff_at = Timestamp::parse("2026-07-31T00:00:00Z").unwrap();
        let mut plan = BackupSnapshotExpiryPlan::new(
            plan_id,
            UserId::new(),
            "expiry-domain-plan".to_owned(),
            request.fingerprint(),
            backup_set_id,
            BackupSnapshotRetentionPolicyRevisionId::new(),
            BackupSnapshotRetentionPolicyRevisionNumber::new(1).unwrap(),
            evaluated_at,
            cutoff_at,
            BackupSnapshotExpiryBasisFingerprint::new(
                BACKUP_SNAPSHOT_EXPIRY_BASIS_FINGERPRINT_VERSION,
                [7; 32],
            ),
            4,
            1,
            1,
            1,
            1,
            BackupSnapshotExpiryPlanState::Planned,
            evaluated_at,
            None,
        )
        .expect("coherent aggregate plan is valid");
        let stale_at = Timestamp::parse("2026-08-30T00:00:01Z").unwrap();
        plan.transition_state(BackupSnapshotExpiryPlanState::Stale, Some(stale_at))
            .expect("planned can become stale");
        assert_eq!(plan.state(), BackupSnapshotExpiryPlanState::Stale);
        assert!(
            plan.transition_state(BackupSnapshotExpiryPlanState::Planned, None)
                .is_err()
        );
        assert!(
            plan.transition_state(
                BackupSnapshotExpiryPlanState::Stale,
                Some(Timestamp::parse("2026-08-30T00:00:02Z").unwrap()),
            )
            .is_err()
        );
    }

    #[test]
    fn expiry_plan_rejects_inconsistent_counts_or_time_basis() {
        let backup_set_id = BackupSetId::new();
        let request = BackupSnapshotExpiryPlanRequest::new(backup_set_id);
        let evaluated_at = Timestamp::parse("2026-08-30T00:00:00Z").unwrap();
        let basis = BackupSnapshotExpiryBasisFingerprint::new(
            BACKUP_SNAPSHOT_EXPIRY_BASIS_FINGERPRINT_VERSION,
            [8; 32],
        );
        let result = BackupSnapshotExpiryPlan::new(
            BackupSnapshotExpiryPlanId::new(),
            UserId::new(),
            "expiry-invalid-counts".to_owned(),
            request.fingerprint(),
            backup_set_id,
            BackupSnapshotRetentionPolicyRevisionId::new(),
            BackupSnapshotRetentionPolicyRevisionNumber::new(1).unwrap(),
            evaluated_at,
            Timestamp::parse("2026-07-31T00:00:00Z").unwrap(),
            basis,
            2,
            1,
            1,
            1,
            0,
            BackupSnapshotExpiryPlanState::Planned,
            evaluated_at,
            None,
        );
        assert_eq!(
            result,
            Err(DomainError::BackupSnapshotExpiryInvalidPlanState)
        );
    }

    #[test]
    fn expiry_plan_executed_state_is_terminal_and_one_way() {
        let now = observed_at();
        let backup_set_id = BackupSetId::new();
        let request = BackupSnapshotExpiryPlanRequest::new(backup_set_id);
        let mut plan = BackupSnapshotExpiryPlan::new(
            BackupSnapshotExpiryPlanId::new(),
            UserId::new(),
            "expiry-executed-state".to_owned(),
            request.fingerprint(),
            backup_set_id,
            BackupSnapshotRetentionPolicyRevisionId::new(),
            BackupSnapshotRetentionPolicyRevisionNumber::new(1).unwrap(),
            now,
            Timestamp::parse("2026-07-31T00:00:00Z").unwrap(),
            BackupSnapshotExpiryBasisFingerprint::new(
                BACKUP_SNAPSHOT_EXPIRY_BASIS_FINGERPRINT_VERSION,
                [9; 32],
            ),
            1,
            1,
            0,
            0,
            0,
            BackupSnapshotExpiryPlanState::Planned,
            now,
            None,
        )
        .expect("planned plan is valid");
        plan.transition_state(BackupSnapshotExpiryPlanState::Executed, None)
            .expect("planned can execute");
        assert_eq!(plan.state(), BackupSnapshotExpiryPlanState::Executed);
        assert_eq!(plan.stale_at(), None);
        assert!(
            plan.transition_state(BackupSnapshotExpiryPlanState::Stale, Some(now))
                .is_err()
        );
        assert!(
            plan.transition_state(BackupSnapshotExpiryPlanState::Planned, None)
                .is_err()
        );
        assert!(
            plan.transition_state(BackupSnapshotExpiryPlanState::Executed, None)
                .is_ok()
        );
    }

    #[test]
    fn expiry_execution_receipt_validates_count_arithmetic_and_order() {
        let now = observed_at();
        let evaluated_at = Timestamp::parse("2026-08-28T00:00:00Z").unwrap();
        let receipt = BackupSnapshotExpiryExecution::new(
            BackupSnapshotExpiryExecutionId::new(),
            UserId::new(),
            BackupSnapshotExpiryPlanId::new(),
            BackupSetId::new(),
            BackupSnapshotRetentionPolicyRevisionId::new(),
            evaluated_at,
            5,
            2,
            3,
            now,
        )
        .expect("coherent receipt is valid");
        assert_eq!(receipt.expired_snapshot_count(), 2);
        assert_eq!(receipt.unchanged_snapshot_count(), 3);
        assert!(
            BackupSnapshotExpiryExecution::new(
                BackupSnapshotExpiryExecutionId::new(),
                UserId::new(),
                BackupSnapshotExpiryPlanId::new(),
                BackupSetId::new(),
                BackupSnapshotRetentionPolicyRevisionId::new(),
                now,
                5,
                2,
                3,
                evaluated_at,
            )
            .is_err(),
            "executed_at must not precede evaluated_at"
        );
        assert!(
            BackupSnapshotExpiryExecution::new(
                BackupSnapshotExpiryExecutionId::new(),
                UserId::new(),
                BackupSnapshotExpiryPlanId::new(),
                BackupSetId::new(),
                BackupSnapshotRetentionPolicyRevisionId::new(),
                now,
                5,
                2,
                3,
                evaluated_at,
            )
            .is_err(),
            "executed_at must not precede evaluated_at"
        );
    }

    #[test]
    fn expiry_execution_entry_maps_decision_and_transition_flag() {
        let plan_id = BackupSnapshotExpiryPlanId::new();
        let entry = BackupSnapshotExpiryExecutionEntry::new(
            BackupSnapshotExpiryExecutionId::new(),
            plan_id,
            SnapshotId::new(),
            BackupSnapshotExpiryDecision::Expire,
            true,
        );
        assert_eq!(
            entry.original_decision(),
            BackupSnapshotExpiryDecision::Expire
        );
        assert!(entry.transitioned());
        assert_ne!(
            entry,
            BackupSnapshotExpiryExecutionEntry::new(
                entry.execution_id(),
                plan_id,
                entry.snapshot_id(),
                BackupSnapshotExpiryDecision::KeepLatest,
                false,
            )
        );
    }

    #[test]
    fn maintenance_run_states_are_closed_and_monotonic() {
        for state in [
            BackupMaintenanceRunState::Created,
            BackupMaintenanceRunState::SnapshotCaptured,
            BackupMaintenanceRunState::ExpiryPlanned,
            BackupMaintenanceRunState::Completed,
            BackupMaintenanceRunState::Stale,
        ] {
            assert_eq!(
                state.as_str().parse::<BackupMaintenanceRunState>(),
                Ok(state)
            );
        }
        assert!("RUNNING".parse::<BackupMaintenanceRunState>().is_err());
        assert!(
            BackupMaintenanceRunState::Created
                .can_transition_to(BackupMaintenanceRunState::SnapshotCaptured)
        );
        assert!(
            BackupMaintenanceRunState::SnapshotCaptured
                .can_transition_to(BackupMaintenanceRunState::ExpiryPlanned)
        );
        assert!(
            BackupMaintenanceRunState::ExpiryPlanned
                .can_transition_to(BackupMaintenanceRunState::Completed)
        );
        assert!(
            BackupMaintenanceRunState::SnapshotCaptured
                .can_transition_to(BackupMaintenanceRunState::Stale)
        );
        assert!(BackupMaintenanceRunState::Completed.is_terminal());
        assert!(BackupMaintenanceRunState::Stale.is_terminal());
        assert!(
            !BackupMaintenanceRunState::Completed
                .can_transition_to(BackupMaintenanceRunState::Stale)
        );
        assert!(
            !BackupMaintenanceRunState::ExpiryPlanned
                .can_transition_to(BackupMaintenanceRunState::SnapshotCaptured)
        );
    }

    #[test]
    fn maintenance_run_request_fingerprint_is_stable_and_set_scoped() {
        let request = BackupMaintenanceRunRequest::new(BackupSetId::new());
        assert_eq!(request.fingerprint(), request.fingerprint());
        assert_eq!(
            request.fingerprint().version(),
            BACKUP_MAINTENANCE_RUN_FINGERPRINT_VERSION
        );
        assert_ne!(
            request.fingerprint(),
            BackupMaintenanceRunRequest::new(BackupSetId::new()).fingerprint()
        );
    }

    #[test]
    fn maintenance_run_validates_state_reference_shapes() {
        let now = observed_at();
        let request = BackupMaintenanceRunRequest::new(BackupSetId::new());
        let valid = BackupMaintenanceRun::new(
            BackupMaintenanceRunId::new(),
            UserId::new(),
            "maintenance-domain".to_owned(),
            request.fingerprint(),
            request.backup_set_id(),
            BackupSnapshotRetentionPolicyRevisionId::new(),
            BackupSnapshotRetentionPolicyRevisionNumber::new(1).unwrap(),
            "maintenance-domain-capture".to_owned(),
            "maintenance-domain-expiry".to_owned(),
            BackupMaintenanceRunState::Created,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            now,
        );
        assert!(valid.is_ok());

        let invalid = BackupMaintenanceRun::new(
            BackupMaintenanceRunId::new(),
            UserId::new(),
            "maintenance-invalid".to_owned(),
            request.fingerprint(),
            request.backup_set_id(),
            BackupSnapshotRetentionPolicyRevisionId::new(),
            BackupSnapshotRetentionPolicyRevisionNumber::new(1).unwrap(),
            "maintenance-invalid-capture".to_owned(),
            "maintenance-invalid-expiry".to_owned(),
            BackupMaintenanceRunState::Completed,
            Some(SnapshotId::new()),
            Some(BackupSnapshotExpiryPlanId::new()),
            None,
            Some(now),
            Some(now),
            Some(now),
            None,
            now,
        );
        assert_eq!(invalid, Err(DomainError::BackupMaintenanceRunInvalidState));
    }
}
