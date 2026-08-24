use super::{DomainError, LogicalName, LoginIdentifier};
use crate::{
    DedupDomainId, DeviceId, FileVersionId, LibraryId, NodeId, ObjectId, Revision, Sha256Digest,
    Timestamp, UserId,
};

const INITIAL_REVISION: Revision = Revision::new(0);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum UserStatus {
    Pending,
    Active,
    Locked,
    Disabled,
}

impl UserStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Active => "ACTIVE",
            Self::Locked => "LOCKED",
            Self::Disabled => "DISABLED",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct User {
    id: UserId,
    login: LoginIdentifier,
    status: UserStatus,
    is_instance_admin: bool,
    created_at: Timestamp,
    updated_at: Timestamp,
    revision: Revision,
}

impl User {
    #[must_use]
    pub fn new(
        id: UserId,
        login: LoginIdentifier,
        status: UserStatus,
        observed_at: Timestamp,
    ) -> Self {
        Self::rehydrate(
            id,
            login,
            status,
            observed_at,
            observed_at,
            INITIAL_REVISION,
        )
    }

    /// Reconstruct a user from canonical durable metadata without coupling
    /// the domain to a storage provider.
    #[must_use]
    pub fn rehydrate(
        id: UserId,
        login: LoginIdentifier,
        status: UserStatus,
        created_at: Timestamp,
        updated_at: Timestamp,
        revision: Revision,
    ) -> Self {
        Self::rehydrate_with_admin(id, login, status, false, created_at, updated_at, revision)
    }

    /// Reconstruct a user with the explicit instance-administrator privilege
    /// defined by the authentication domain contract.
    #[must_use]
    pub fn rehydrate_with_admin(
        id: UserId,
        login: LoginIdentifier,
        status: UserStatus,
        is_instance_admin: bool,
        created_at: Timestamp,
        updated_at: Timestamp,
        revision: Revision,
    ) -> Self {
        Self {
            id,
            login,
            status,
            is_instance_admin,
            created_at,
            updated_at,
            revision,
        }
    }

    #[must_use]
    pub const fn id(&self) -> UserId {
        self.id
    }

    #[must_use]
    pub const fn login(&self) -> &LoginIdentifier {
        &self.login
    }

    #[must_use]
    pub const fn status(&self) -> UserStatus {
        self.status
    }

    #[must_use]
    pub const fn is_instance_admin(&self) -> bool {
        self.is_instance_admin
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
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DeviceStatus {
    Pending,
    Active,
    Paused,
    Revoked,
}

impl DeviceStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Active => "ACTIVE",
            Self::Paused => "PAUSED",
            Self::Revoked => "REVOKED",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Device {
    id: DeviceId,
    owner_user_id: UserId,
    display_name: LogicalName,
    status: DeviceStatus,
    created_at: Timestamp,
    updated_at: Timestamp,
    revision: Revision,
}

impl Device {
    #[must_use]
    pub fn new(
        id: DeviceId,
        owner_user_id: UserId,
        display_name: LogicalName,
        observed_at: Timestamp,
    ) -> Self {
        Self::rehydrate(
            id,
            owner_user_id,
            display_name,
            DeviceStatus::Pending,
            observed_at,
            observed_at,
            INITIAL_REVISION,
        )
    }

    /// Reconstruct a device from canonical durable metadata without coupling
    /// the domain to a storage provider.
    #[must_use]
    pub fn rehydrate(
        id: DeviceId,
        owner_user_id: UserId,
        display_name: LogicalName,
        status: DeviceStatus,
        created_at: Timestamp,
        updated_at: Timestamp,
        revision: Revision,
    ) -> Self {
        Self {
            id,
            owner_user_id,
            display_name,
            status,
            created_at,
            updated_at,
            revision,
        }
    }

    #[must_use]
    pub const fn id(&self) -> DeviceId {
        self.id
    }

    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn display_name(&self) -> &LogicalName {
        &self.display_name
    }

    #[must_use]
    pub const fn status(&self) -> DeviceStatus {
        self.status
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

    pub fn transition_status(
        &mut self,
        next: DeviceStatus,
        observed_at: Timestamp,
    ) -> Result<(), DomainError> {
        let allowed = matches!(
            (self.status, next),
            (
                DeviceStatus::Pending,
                DeviceStatus::Active | DeviceStatus::Revoked
            ) | (
                DeviceStatus::Active,
                DeviceStatus::Paused | DeviceStatus::Revoked
            ) | (
                DeviceStatus::Paused,
                DeviceStatus::Active | DeviceStatus::Revoked
            ) | (DeviceStatus::Revoked, DeviceStatus::Revoked)
        );

        if !allowed {
            return Err(DomainError::InvalidDeviceStateTransition {
                from: self.status,
                to: next,
            });
        }

        if self.status != next {
            self.status = next;
            self.bump_revision(observed_at)?;
        }
        Ok(())
    }

    fn bump_revision(&mut self, observed_at: Timestamp) -> Result<(), DomainError> {
        let next = self
            .revision
            .get()
            .checked_add(1)
            .ok_or(DomainError::RevisionOverflow)?;
        self.revision = Revision::new(next);
        self.updated_at = observed_at;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum LibraryStatus {
    Active,
    ReadOnly,
    Quarantined,
    Deleting,
}

impl LibraryStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::ReadOnly => "READ_ONLY",
            Self::Quarantined => "QUARANTINED",
            Self::Deleting => "DELETING",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Library {
    id: LibraryId,
    owner_user_id: UserId,
    name: LogicalName,
    root_node_id: NodeId,
    dedup_domain_id: DedupDomainId,
    status: LibraryStatus,
    created_at: Timestamp,
    updated_at: Timestamp,
    revision: Revision,
}

impl Library {
    pub fn new(
        id: LibraryId,
        owner_user_id: UserId,
        name: LogicalName,
        root: &Node,
        dedup_domain_id: DedupDomainId,
        observed_at: Timestamp,
    ) -> Result<Self, DomainError> {
        if root.library_id() != id {
            return Err(DomainError::NodeLibraryMismatch);
        }
        if root.kind() != NodeKind::Directory {
            return Err(DomainError::RootMustBeDirectory);
        }
        if root.state() != NodeState::Active {
            return Err(DomainError::RootMustBeActive);
        }
        if !root.is_root() {
            return Err(DomainError::RootCannotHaveParent);
        }

        Self::rehydrate(
            id,
            owner_user_id,
            name,
            LibraryStatus::Active,
            root,
            dedup_domain_id,
            observed_at,
            observed_at,
            INITIAL_REVISION,
        )
    }

    /// Reconstruct a library and validate its durable root relationship
    /// without coupling the domain to a storage provider.
    #[allow(clippy::too_many_arguments)]
    pub fn rehydrate(
        id: LibraryId,
        owner_user_id: UserId,
        name: LogicalName,
        status: LibraryStatus,
        root: &Node,
        dedup_domain_id: DedupDomainId,
        created_at: Timestamp,
        updated_at: Timestamp,
        revision: Revision,
    ) -> Result<Self, DomainError> {
        if root.library_id() != id
            || root.kind() != NodeKind::Directory
            || root.state() != NodeState::Active
            || !root.is_root()
            || root.current_version_id().is_some()
        {
            return Err(DomainError::RootNodeMismatch);
        }

        Ok(Self {
            id,
            owner_user_id,
            name,
            root_node_id: root.id(),
            dedup_domain_id,
            status,
            created_at,
            updated_at,
            revision,
        })
    }

    pub fn validate_root(&self, root: &Node) -> Result<(), DomainError> {
        if root.id() != self.root_node_id
            || root.library_id() != self.id
            || root.kind() != NodeKind::Directory
            || root.state() != NodeState::Active
            || !root.is_root()
            || root.current_version_id().is_some()
        {
            return Err(DomainError::RootNodeMismatch);
        }
        Ok(())
    }

    #[must_use]
    pub const fn id(&self) -> LibraryId {
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
    pub const fn root_node_id(&self) -> NodeId {
        self.root_node_id
    }

    #[must_use]
    pub const fn dedup_domain_id(&self) -> DedupDomainId {
        self.dedup_domain_id
    }

    #[must_use]
    pub const fn status(&self) -> LibraryStatus {
        self.status
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
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NodeKind {
    File,
    Directory,
}

impl NodeKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::File => "FILE",
            Self::Directory => "DIRECTORY",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NodeState {
    Active,
    Trashed,
    Purging,
}

impl NodeState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Trashed => "TRASHED",
            Self::Purging => "PURGING",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    id: NodeId,
    library_id: LibraryId,
    parent_node_id: Option<NodeId>,
    kind: NodeKind,
    name: LogicalName,
    current_version_id: Option<FileVersionId>,
    state: NodeState,
    created_at: Timestamp,
    updated_at: Timestamp,
    revision: Revision,
}

impl Node {
    #[must_use]
    pub fn root(
        id: NodeId,
        library_id: LibraryId,
        name: LogicalName,
        observed_at: Timestamp,
    ) -> Self {
        Self::new_root(id, library_id, name, observed_at)
    }

    pub fn new_root(
        id: NodeId,
        library_id: LibraryId,
        name: LogicalName,
        observed_at: Timestamp,
    ) -> Self {
        Self::rehydrate(
            id,
            library_id,
            None,
            NodeKind::Directory,
            name,
            None,
            NodeState::Active,
            observed_at,
            observed_at,
            INITIAL_REVISION,
        )
        .expect("canonical root node state is valid")
    }

    pub fn child(
        id: NodeId,
        parent: &Node,
        kind: NodeKind,
        name: LogicalName,
        observed_at: Timestamp,
    ) -> Result<Self, DomainError> {
        Self::new_child(id, parent.library_id(), parent, kind, name, observed_at)
    }

    pub fn new_child(
        id: NodeId,
        library_id: LibraryId,
        parent: &Node,
        kind: NodeKind,
        name: LogicalName,
        observed_at: Timestamp,
    ) -> Result<Self, DomainError> {
        if parent.library_id() != library_id {
            return Err(DomainError::ParentLibraryMismatch);
        }
        if parent.kind() != NodeKind::Directory {
            return Err(DomainError::ParentMustBeDirectory);
        }
        if parent.state() != NodeState::Active {
            return Err(DomainError::ParentMustBeActive);
        }
        if parent.id() == id {
            return Err(DomainError::ParentCannotBeSelf);
        }

        Self::rehydrate(
            id,
            library_id,
            Some(parent.id()),
            kind,
            name,
            None,
            NodeState::Active,
            observed_at,
            observed_at,
            INITIAL_REVISION,
        )
    }

    /// Reconstruct a node from canonical durable metadata. Relationship
    /// checks that need loaded ancestors remain application/persistence
    /// concerns; local shape invariants are enforced here.
    #[allow(clippy::too_many_arguments)]
    pub fn rehydrate(
        id: NodeId,
        library_id: LibraryId,
        parent_node_id: Option<NodeId>,
        kind: NodeKind,
        name: LogicalName,
        current_version_id: Option<FileVersionId>,
        state: NodeState,
        created_at: Timestamp,
        updated_at: Timestamp,
        revision: Revision,
    ) -> Result<Self, DomainError> {
        if parent_node_id.is_none() {
            if kind != NodeKind::Directory {
                return Err(DomainError::RootMustBeDirectory);
            }
            if state != NodeState::Active {
                return Err(DomainError::RootMustBeActive);
            }
        }
        if parent_node_id == Some(id) {
            return Err(DomainError::ParentCannotBeSelf);
        }
        if kind == NodeKind::Directory && current_version_id.is_some() {
            return Err(DomainError::DirectoryCannotReferenceFileVersion);
        }

        Ok(Self {
            id,
            library_id,
            parent_node_id,
            kind,
            name,
            current_version_id,
            state,
            created_at,
            updated_at,
            revision,
        })
    }

    pub fn validate_parent_relationship(&self, parent: &Node) -> Result<(), DomainError> {
        let Some(parent_node_id) = self.parent_node_id else {
            return Err(DomainError::RootCannotHaveParent);
        };
        if parent_node_id != parent.id() {
            return Err(DomainError::ParentReferenceMismatch);
        }
        if self.library_id != parent.library_id() {
            return Err(DomainError::ParentLibraryMismatch);
        }
        if parent.kind() != NodeKind::Directory {
            return Err(DomainError::ParentMustBeDirectory);
        }
        if parent.state() != NodeState::Active {
            return Err(DomainError::ParentMustBeActive);
        }
        if parent.id() == self.id {
            return Err(DomainError::ParentCannotBeSelf);
        }
        Ok(())
    }

    /// Validate an immediate-parent-first chain before a move is persisted.
    /// The chain may stop at any ancestor; every supplied link must be valid.
    pub fn validate_parent_chain(&self, ancestors: &[Node]) -> Result<(), DomainError> {
        let mut expected_parent = self.parent_node_id;
        let mut seen = Vec::with_capacity(ancestors.len());
        for ancestor in ancestors {
            let Some(expected_parent_id) = expected_parent else {
                return Err(DomainError::ParentChainMismatch);
            };
            if ancestor.id() != expected_parent_id {
                return Err(DomainError::ParentChainMismatch);
            }
            if ancestor.id() == self.id || seen.contains(&ancestor.id()) {
                return Err(DomainError::ParentCycle);
            }
            seen.push(ancestor.id());
            if ancestor.library_id() != self.library_id {
                return Err(DomainError::ParentLibraryMismatch);
            }
            if ancestor.state() != NodeState::Active {
                return Err(DomainError::ParentMustBeActive);
            }
            if ancestor.kind() != NodeKind::Directory {
                return Err(DomainError::ParentMustBeDirectory);
            }
            expected_parent = ancestor.parent_node_id;
        }
        Ok(())
    }

    /// Validate a candidate parent and its already-loaded ancestor chain
    /// before a metadata move is persisted. `ancestors` starts at the
    /// candidate parent's parent and does not include the candidate parent.
    pub fn validate_move_parent(
        &self,
        candidate_parent: &Node,
        ancestors: &[Node],
    ) -> Result<(), DomainError> {
        if candidate_parent.library_id() != self.library_id {
            return Err(DomainError::ParentLibraryMismatch);
        }
        if candidate_parent.kind() != NodeKind::Directory {
            return Err(DomainError::ParentMustBeDirectory);
        }
        if candidate_parent.state() != NodeState::Active {
            return Err(DomainError::ParentMustBeActive);
        }
        if candidate_parent.id() == self.id {
            return Err(DomainError::ParentCannotBeSelf);
        }
        if ancestors.iter().any(|ancestor| ancestor.id() == self.id) {
            return Err(DomainError::ParentCycle);
        }
        candidate_parent.validate_parent_chain(ancestors)
    }

    /// Rename this logical node without touching any content object or file
    /// version. The caller supplies a domain-valid logical name and a
    /// server-observed timestamp.
    pub fn rename(&mut self, name: LogicalName, observed_at: Timestamp) -> Result<(), DomainError> {
        if self.name != name {
            self.name = name;
            self.bump_revision(observed_at)?;
        }
        Ok(())
    }

    /// Move this logical node under an already validated directory parent.
    /// Physical object identity and immutable file history are deliberately
    /// outside this operation.
    pub fn move_to(&mut self, parent: &Node, observed_at: Timestamp) -> Result<(), DomainError> {
        if self.is_root() {
            return Err(DomainError::RootCannotHaveParent);
        }
        if self.library_id != parent.library_id() {
            return Err(DomainError::ParentLibraryMismatch);
        }
        if parent.kind() != NodeKind::Directory {
            return Err(DomainError::ParentMustBeDirectory);
        }
        if parent.state() != NodeState::Active {
            return Err(DomainError::ParentMustBeActive);
        }
        if parent.id() == self.id {
            return Err(DomainError::ParentCannotBeSelf);
        }
        if self.parent_node_id != Some(parent.id()) {
            self.parent_node_id = Some(parent.id());
            self.bump_revision(observed_at)?;
        }
        Ok(())
    }

    pub fn with_current_version(
        &self,
        version: &FileVersion,
        observed_at: Timestamp,
    ) -> Result<Self, DomainError> {
        if self.kind == NodeKind::Directory {
            return Err(DomainError::DirectoryCannotReferenceFileVersion);
        }
        if version.library_id() != self.library_id {
            return Err(DomainError::FileVersionLibraryMismatch);
        }
        if version.node_id() != self.id {
            return Err(DomainError::FileVersionNodeMismatch);
        }

        let mut next = self.clone();
        next.current_version_id = Some(version.id());
        next.bump_revision(observed_at)?;
        Ok(next)
    }

    pub fn transition_state(
        &mut self,
        next: NodeState,
        observed_at: Timestamp,
    ) -> Result<(), DomainError> {
        let allowed = matches!(
            (self.state, next),
            (NodeState::Active, NodeState::Trashed)
                | (NodeState::Trashed, NodeState::Active | NodeState::Purging)
                | (NodeState::Purging, NodeState::Purging)
        );

        if !allowed {
            return Err(DomainError::InvalidNodeStateTransition {
                from: self.state,
                to: next,
            });
        }

        if self.is_root() && matches!(next, NodeState::Trashed | NodeState::Purging) {
            return Err(DomainError::RootCannotBeDeleted);
        }

        if self.state != next {
            self.state = next;
            self.bump_revision(observed_at)?;
        }
        Ok(())
    }

    #[must_use]
    pub const fn id(&self) -> NodeId {
        self.id
    }

    #[must_use]
    pub const fn library_id(&self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn parent_node_id(&self) -> Option<NodeId> {
        self.parent_node_id
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
    pub const fn current_version_id(&self) -> Option<FileVersionId> {
        self.current_version_id
    }

    #[must_use]
    pub const fn state(&self) -> NodeState {
        self.state
    }

    #[must_use]
    pub const fn is_root(&self) -> bool {
        self.parent_node_id.is_none()
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

    fn bump_revision(&mut self, observed_at: Timestamp) -> Result<(), DomainError> {
        let next = self
            .revision
            .get()
            .checked_add(1)
            .ok_or(DomainError::RevisionOverflow)?;
        self.revision = Revision::new(next);
        self.updated_at = observed_at;
        Ok(())
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct ObjectReference {
    object_id: ObjectId,
    dedup_domain_id: DedupDomainId,
    canonical_hash: Sha256Digest,
    plaintext_length: u64,
}

impl ObjectReference {
    /// Create a reference to canonical, already verified object metadata.
    ///
    /// The type has no constructor for staging or quarantined objects; the
    /// persistence/application boundary must only call this after verification.
    #[must_use]
    pub const fn new(
        object_id: ObjectId,
        dedup_domain_id: DedupDomainId,
        canonical_hash: Sha256Digest,
        plaintext_length: u64,
    ) -> Self {
        Self {
            object_id,
            dedup_domain_id,
            canonical_hash,
            plaintext_length,
        }
    }

    #[must_use]
    pub const fn verified(
        object_id: ObjectId,
        dedup_domain_id: DedupDomainId,
        canonical_hash: Sha256Digest,
        plaintext_length: u64,
    ) -> Self {
        Self::new(object_id, dedup_domain_id, canonical_hash, plaintext_length)
    }

    #[must_use]
    pub const fn object_id(self) -> ObjectId {
        self.object_id
    }

    #[must_use]
    pub const fn dedup_domain_id(self) -> DedupDomainId {
        self.dedup_domain_id
    }

    #[must_use]
    pub const fn canonical_hash(self) -> Sha256Digest {
        self.canonical_hash
    }

    #[must_use]
    pub const fn plaintext_length(self) -> u64 {
        self.plaintext_length
    }

    #[must_use]
    pub const fn size(self) -> u64 {
        self.plaintext_length()
    }
}

impl std::fmt::Debug for ObjectReference {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ObjectReference")
            .field("object_id", &self.object_id)
            .field("dedup_domain_id", &self.dedup_domain_id)
            .field("canonical_hash", &"<redacted>")
            .field("plaintext_length", &self.plaintext_length)
            .finish()
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct FileVersion {
    id: FileVersionId,
    library_id: LibraryId,
    node_id: NodeId,
    object: ObjectReference,
    parent_version_id: Option<FileVersionId>,
    committed_at: Timestamp,
    revision: Revision,
}

impl FileVersion {
    pub fn new(
        id: FileVersionId,
        library: &Library,
        node: &Node,
        object: ObjectReference,
        parent_version_id: Option<FileVersionId>,
        committed_at: Timestamp,
    ) -> Result<Self, DomainError> {
        if node.kind() != NodeKind::File {
            return Err(DomainError::FileVersionRequiresFileNode);
        }
        if node.library_id() != library.id() {
            return Err(DomainError::FileVersionLibraryMismatch);
        }
        if object.dedup_domain_id() != library.dedup_domain_id() {
            return Err(DomainError::ObjectDedupDomainMismatch);
        }
        if parent_version_id == Some(id) {
            return Err(DomainError::FileVersionCannotParentItself);
        }

        Self::rehydrate(
            id,
            library,
            node,
            object,
            parent_version_id,
            committed_at,
            INITIAL_REVISION,
        )
    }

    /// Reconstruct an immutable file version with its persisted revision.
    pub fn rehydrate(
        id: FileVersionId,
        library: &Library,
        node: &Node,
        object: ObjectReference,
        parent_version_id: Option<FileVersionId>,
        committed_at: Timestamp,
        revision: Revision,
    ) -> Result<Self, DomainError> {
        if node.kind() != NodeKind::File {
            return Err(DomainError::FileVersionRequiresFileNode);
        }
        if node.library_id() != library.id() {
            return Err(DomainError::FileVersionLibraryMismatch);
        }
        if object.dedup_domain_id() != library.dedup_domain_id() {
            return Err(DomainError::ObjectDedupDomainMismatch);
        }
        if parent_version_id == Some(id) {
            return Err(DomainError::FileVersionCannotParentItself);
        }

        Ok(Self {
            id,
            library_id: library.id(),
            node_id: node.id(),
            object,
            parent_version_id,
            committed_at,
            revision,
        })
    }

    #[must_use]
    pub const fn id(self) -> FileVersionId {
        self.id
    }

    #[must_use]
    pub const fn library_id(self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn node_id(self) -> NodeId {
        self.node_id
    }

    #[must_use]
    pub const fn object_reference(self) -> ObjectReference {
        self.object
    }

    #[must_use]
    pub const fn parent_version_id(self) -> Option<FileVersionId> {
        self.parent_version_id
    }

    #[must_use]
    pub const fn committed_at(self) -> Timestamp {
        self.committed_at
    }

    #[must_use]
    pub const fn revision(self) -> Revision {
        self.revision
    }

    #[must_use]
    pub const fn canonical_hash(self) -> Sha256Digest {
        self.object.canonical_hash()
    }

    #[must_use]
    pub const fn plaintext_length(self) -> u64 {
        self.object.plaintext_length()
    }
}

impl std::fmt::Debug for FileVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FileVersion")
            .field("id", &self.id)
            .field("library_id", &self.library_id)
            .field("node_id", &self.node_id)
            .field("object", &self.object)
            .field("parent_version_id", &self.parent_version_id)
            .field("committed_at", &self.committed_at)
            .field("revision", &self.revision)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observed_at() -> Timestamp {
        Timestamp::parse("2026-08-22T00:00:00Z").expect("fixed timestamp is valid")
    }

    fn name(value: &str) -> LogicalName {
        LogicalName::new(value).expect("test logical name is valid")
    }

    fn login(value: &str) -> LoginIdentifier {
        LoginIdentifier::new(value, value).expect("test login is valid")
    }

    #[test]
    fn user_and_device_preserve_typed_identity_and_core_metadata() {
        let user_id = UserId::new();
        let device_id = DeviceId::new();
        let observed_at = observed_at();
        let user = User::new(user_id, login("alice"), UserStatus::Active, observed_at);
        let device = Device::new(device_id, user_id, name("Laptop"), observed_at);

        assert_eq!(user.id(), user_id);
        assert_eq!(user.status(), UserStatus::Active);
        assert_eq!(user.revision(), Revision::new(0));
        assert_eq!(user.created_at(), observed_at);
        assert_eq!(device.id(), device_id);
        assert_eq!(device.owner_user_id(), user_id);
        assert_eq!(device.status(), DeviceStatus::Pending);
        assert_eq!(device.revision(), Revision::new(0));
    }

    #[test]
    fn node_kind_is_explicit_and_root_has_no_parent_or_content() {
        let library_id = LibraryId::new();
        let root = Node::new_root(NodeId::new(), library_id, name("root"), observed_at());
        let library = Library::new(
            library_id,
            UserId::new(),
            name("Library"),
            &root,
            DedupDomainId::new(),
            observed_at(),
        )
        .expect("root belongs to library");
        let directory = Node::new_child(
            NodeId::new(),
            library_id,
            &root,
            NodeKind::Directory,
            name("folder"),
            observed_at(),
        )
        .expect("directory child is valid");
        let file = Node::new_child(
            NodeId::new(),
            library_id,
            &directory,
            NodeKind::File,
            name("file"),
            observed_at(),
        )
        .expect("file child is valid");

        assert_eq!(root.kind(), NodeKind::Directory);
        assert!(root.is_root());
        assert_eq!(root.parent_node_id(), None);
        assert_eq!(root.current_version_id(), None);
        assert_eq!(directory.kind(), NodeKind::Directory);
        assert_eq!(file.kind(), NodeKind::File);
        assert_eq!(file.parent_node_id(), Some(directory.id()));
        assert_eq!(library.root_node_id(), root.id());
        assert_eq!(library.dedup_domain_id().to_string().len(), 36);
    }

    #[test]
    fn parent_relationship_rejects_foreign_libraries_and_cycles() {
        let first_library_id = LibraryId::new();
        let second_library_id = LibraryId::new();
        let first_root = Node::new_root(
            NodeId::new(),
            first_library_id,
            name("first-root"),
            observed_at(),
        );
        let second_root = Node::new_root(
            NodeId::new(),
            second_library_id,
            name("second-root"),
            observed_at(),
        );

        assert_eq!(
            Node::new_child(
                NodeId::new(),
                second_library_id,
                &first_root,
                NodeKind::File,
                name("foreign"),
                observed_at(),
            ),
            Err(DomainError::ParentLibraryMismatch)
        );

        let parent = Node::new_child(
            NodeId::new(),
            first_library_id,
            &first_root,
            NodeKind::Directory,
            name("parent"),
            observed_at(),
        )
        .expect("parent is valid");
        let child = Node::new_child(
            NodeId::new(),
            first_library_id,
            &parent,
            NodeKind::Directory,
            name("child"),
            observed_at(),
        )
        .expect("child is valid");

        child
            .validate_parent_relationship(&parent)
            .expect("parent relationship is valid");
        assert_eq!(
            child.validate_parent_relationship(&second_root),
            Err(DomainError::ParentReferenceMismatch)
        );
        let grandchild = Node::new_child(
            NodeId::new(),
            first_library_id,
            &child,
            NodeKind::Directory,
            name("grandchild"),
            observed_at(),
        )
        .expect("grandchild is valid");
        assert_eq!(
            child.validate_move_parent(&grandchild, std::slice::from_ref(&child)),
            Err(DomainError::ParentCycle)
        );
        assert_eq!(
            first_root.validate_parent_relationship(&second_root),
            Err(DomainError::RootCannotHaveParent)
        );
    }

    #[test]
    fn logical_rename_and_move_bump_revision_without_touching_content() {
        let library_id = LibraryId::new();
        let first_time = observed_at();
        let second_time = Timestamp::parse("2026-08-22T00:00:01Z").expect("valid test time");
        let mut root = Node::new_root(NodeId::new(), library_id, name("root"), first_time);
        let first_parent = Node::new_child(
            NodeId::new(),
            library_id,
            &root,
            NodeKind::Directory,
            name("first"),
            first_time,
        )
        .expect("first parent is valid");
        let second_parent = Node::new_child(
            NodeId::new(),
            library_id,
            &root,
            NodeKind::Directory,
            name("second"),
            first_time,
        )
        .expect("second parent is valid");
        let mut file = Node::new_child(
            NodeId::new(),
            library_id,
            &first_parent,
            NodeKind::File,
            name("before"),
            first_time,
        )
        .expect("file is valid");

        file.rename(name("after"), second_time)
            .expect("rename is valid");
        assert_eq!(file.name().as_str(), "after");
        assert_eq!(file.revision(), Revision::new(1));
        assert_eq!(file.current_version_id(), None);
        assert_eq!(file.updated_at(), second_time);

        file.move_to(&second_parent, second_time)
            .expect("move is valid");
        assert_eq!(file.parent_node_id(), Some(second_parent.id()));
        assert_eq!(file.revision(), Revision::new(2));
        assert_eq!(file.current_version_id(), None);

        assert_eq!(
            root.move_to(&second_parent, second_time),
            Err(DomainError::RootCannotHaveParent)
        );
    }

    #[test]
    fn file_versions_are_the_only_node_content_binding() {
        let user_id = UserId::new();
        let library_id = LibraryId::new();
        let dedup_domain_id = DedupDomainId::new();
        let root = Node::new_root(NodeId::new(), library_id, name("root"), observed_at());
        let library = Library::new(
            library_id,
            user_id,
            name("Library"),
            &root,
            dedup_domain_id,
            observed_at(),
        )
        .expect("library root is valid");
        let directory = Node::new_child(
            NodeId::new(),
            library_id,
            &root,
            NodeKind::Directory,
            name("folder"),
            observed_at(),
        )
        .expect("directory is valid");
        let file = Node::new_child(
            NodeId::new(),
            library_id,
            &directory,
            NodeKind::File,
            name("file"),
            observed_at(),
        )
        .expect("file is valid");
        let object = ObjectReference::new(
            ObjectId::new(),
            dedup_domain_id,
            Sha256Digest::from_bytes([0xabu8; 32]),
            42,
        );
        let version = FileVersion::new(
            FileVersionId::new(),
            &library,
            &file,
            object,
            None,
            observed_at(),
        )
        .expect("file version is valid");

        assert_eq!(object.size(), 42);
        assert_eq!(
            object.canonical_hash(),
            Sha256Digest::from_bytes([0xabu8; 32])
        );
        assert_eq!(version.object_reference(), object);
        assert_eq!(version.revision(), Revision::new(0));
        assert_eq!(version.plaintext_length(), 42);
        assert_eq!(
            directory.with_current_version(&version, observed_at()),
            Err(DomainError::DirectoryCannotReferenceFileVersion)
        );
        assert_eq!(
            FileVersion::new(
                FileVersionId::new(),
                &library,
                &directory,
                object,
                None,
                observed_at(),
            ),
            Err(DomainError::FileVersionRequiresFileNode)
        );

        let updated_file = file
            .with_current_version(&version, observed_at())
            .expect("file head accepts its own immutable version");
        assert_eq!(updated_file.current_version_id(), Some(version.id()));
        assert_eq!(updated_file.revision(), Revision::new(1));
        assert_eq!(version.revision(), Revision::new(0));
    }

    #[test]
    fn object_reference_and_file_version_reject_cross_domain_bindings() {
        let library_id = LibraryId::new();
        let dedup_domain_id = DedupDomainId::new();
        let root = Node::new_root(NodeId::new(), library_id, name("root"), observed_at());
        let library = Library::new(
            library_id,
            UserId::new(),
            name("Library"),
            &root,
            dedup_domain_id,
            observed_at(),
        )
        .expect("library is valid");
        let file = Node::new_child(
            NodeId::new(),
            library_id,
            &root,
            NodeKind::File,
            name("file"),
            observed_at(),
        )
        .expect("file is valid");
        let foreign_object = ObjectReference::new(
            ObjectId::new(),
            DedupDomainId::new(),
            Sha256Digest::from_bytes([0; 32]),
            0,
        );

        assert_eq!(
            FileVersion::new(
                FileVersionId::new(),
                &library,
                &file,
                foreign_object,
                None,
                observed_at(),
            ),
            Err(DomainError::ObjectDedupDomainMismatch)
        );
        let version_with_parent = FileVersion::new(
            FileVersionId::new(),
            &library,
            &file,
            ObjectReference::new(
                ObjectId::new(),
                dedup_domain_id,
                Sha256Digest::from_bytes([0; 32]),
                0,
            ),
            Some(FileVersionId::new()),
            observed_at(),
        )
        .expect("version with a different parent is valid");
        assert!(version_with_parent.parent_version_id().is_some());
    }

    #[test]
    fn logical_deletion_is_metadata_only_and_revisioned() {
        let library_id = LibraryId::new();
        let root = Node::new_root(NodeId::new(), library_id, name("root"), observed_at());
        let file = Node::new_child(
            NodeId::new(),
            library_id,
            &root,
            NodeKind::File,
            name("file"),
            observed_at(),
        )
        .expect("file is valid");
        let mut file = file;

        file.transition_state(NodeState::Trashed, observed_at())
            .expect("active node can be trashed");
        assert_eq!(file.state(), NodeState::Trashed);
        assert_eq!(file.revision(), Revision::new(1));
        file.transition_state(NodeState::Active, observed_at())
            .expect("trashed node can be restored");
        file.transition_state(NodeState::Trashed, observed_at())
            .expect("active node can be trashed again");
        file.transition_state(NodeState::Purging, observed_at())
            .expect("trashed node can enter purge metadata state");
        assert_eq!(file.state(), NodeState::Purging);
        assert_eq!(file.current_version_id(), None);
        assert_eq!(
            file.transition_state(NodeState::Active, observed_at()),
            Err(DomainError::InvalidNodeStateTransition {
                from: NodeState::Purging,
                to: NodeState::Active,
            })
        );
    }
}
