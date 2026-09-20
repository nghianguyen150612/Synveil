//! Authenticated logical file/folder metadata application boundary.
//!
//! This module deliberately stops at PostgreSQL metadata. It does not create
//! file bytes, object-store replicas, sync events, backup records, or
//! filesystem paths. The repository methods it calls keep authorization and
//! conditional mutations inside the same PostgreSQL transaction as the
//! corresponding metadata update.

use std::{fmt, str::FromStr};

use async_trait::async_trait;
use synveil_core::{
    DomainError, Library, LibraryId, LogicalName, Node, NodeId, Revision, Timestamp,
    TrashRetentionPolicy, UserId,
};

use crate::{DatabaseError, DatabasePool, DomainRepository, MappingError, MetadataError};

pub const DEFAULT_PAGE_LIMIT: u32 = 50;
pub const MAX_PAGE_LIMIT: u32 = 100;
const MAX_CURSOR_PARTS: usize = 4;
const CURSOR_VERSION: &str = "v1";
pub(crate) const MAX_ANCESTOR_DEPTH: usize = 4_096;

/// Safe application failures for logical file/folder metadata operations.
///
/// `NotFound` is also used for an inaccessible resource so callers cannot use
/// a random typed ID to distinguish another owner's records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileMetadataError {
    NotFound,
    PermissionDenied,
    InvalidRequest,
    InvalidCursor,
    VersionConflict { current_revision: Revision },
    InvalidState,
    Database(DatabaseError),
    InvalidPersistedData,
}

impl fmt::Display for FileMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotFound => "file metadata resource was not found",
            Self::PermissionDenied => "file metadata operation is not permitted",
            Self::InvalidRequest => "file metadata request is invalid",
            Self::InvalidCursor => "file metadata cursor is invalid",
            Self::VersionConflict { .. } => "file metadata revision conflicts",
            Self::InvalidState => "file metadata resource is in an invalid state",
            Self::Database(error) => return error.fmt(formatter),
            Self::InvalidPersistedData => "file metadata persisted data is invalid",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for FileMetadataError {}

/// A bounded, stable child listing page. The cursor is an opaque string to
/// callers; its value is scoped to the library and parent by the service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodePage {
    nodes: Vec<Node>,
    next_cursor: Option<String>,
    has_more: bool,
}

impl NodePage {
    #[must_use]
    pub fn new(nodes: Vec<Node>, next_cursor: Option<String>, has_more: bool) -> Self {
        Self {
            nodes,
            next_cursor,
            has_more,
        }
    }

    #[must_use]
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    #[must_use]
    pub fn into_nodes(self) -> Vec<Node> {
        self.nodes
    }

    #[must_use]
    pub fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_deref()
    }

    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }
}

/// A bounded page of owner-visible libraries used to resolve the user's
/// authoritative library/root without inventing a second ownership model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LibraryPage {
    libraries: Vec<Library>,
    next_cursor: Option<String>,
    has_more: bool,
}

impl LibraryPage {
    #[must_use]
    pub fn new(libraries: Vec<Library>, next_cursor: Option<String>, has_more: bool) -> Self {
        Self {
            libraries,
            next_cursor,
            has_more,
        }
    }

    #[must_use]
    pub fn libraries(&self) -> &[Library] {
        &self.libraries
    }

    #[must_use]
    pub fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_deref()
    }

    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }
}

/// Application-facing metadata port. The API supplies only the authenticated
/// user's typed identity; ownership and library/node relationships are checked
/// again by the PostgreSQL implementation.
#[async_trait]
pub trait FileMetadataBackend: Send + Sync {
    async fn list_libraries(
        &self,
        user_id: UserId,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<LibraryPage, FileMetadataError>;

    /// Create one owner-scoped library and its canonical logical root.
    /// Implementations may use the supplied UUID to make a retried client
    /// request idempotent. The default keeps older composition fixtures
    /// fail-closed until they explicitly opt into library creation.
    async fn create_library(
        &self,
        _user_id: UserId,
        _library_id: LibraryId,
        _name: LogicalName,
    ) -> Result<Library, FileMetadataError> {
        Err(FileMetadataError::InvalidRequest)
    }

    async fn list_children(
        &self,
        user_id: UserId,
        library_id: LibraryId,
        parent_node_id: Option<NodeId>,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<NodePage, FileMetadataError>;

    async fn get_node(&self, user_id: UserId, node_id: NodeId) -> Result<Node, FileMetadataError>;

    async fn create_directory(
        &self,
        user_id: UserId,
        library_id: LibraryId,
        parent_node_id: Option<NodeId>,
        name: LogicalName,
    ) -> Result<Node, FileMetadataError>;

    async fn rename_node(
        &self,
        user_id: UserId,
        node_id: NodeId,
        name: LogicalName,
        expected_revision: Revision,
    ) -> Result<Node, FileMetadataError>;

    async fn move_node(
        &self,
        user_id: UserId,
        node_id: NodeId,
        destination_parent_id: NodeId,
        expected_revision: Revision,
    ) -> Result<Node, FileMetadataError>;

    async fn delete_node(
        &self,
        user_id: UserId,
        node_id: NodeId,
        expected_revision: Revision,
    ) -> Result<Node, FileMetadataError>;

    async fn restore_node(
        &self,
        user_id: UserId,
        node_id: NodeId,
        expected_revision: Revision,
    ) -> Result<Node, FileMetadataError>;
}

/// PostgreSQL-backed logical metadata application service.
#[derive(Clone)]
pub struct FileMetadataService {
    pool: DatabasePool,
    trash_retention_policy: TrashRetentionPolicy,
}

impl FileMetadataService {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self::new_with_policy(pool, TrashRetentionPolicy::default())
    }

    #[must_use]
    pub fn new_with_policy(
        pool: DatabasePool,
        trash_retention_policy: TrashRetentionPolicy,
    ) -> Self {
        Self {
            pool,
            trash_retention_policy,
        }
    }

    #[must_use]
    pub fn pool(&self) -> &DatabasePool {
        &self.pool
    }

    #[must_use]
    pub const fn trash_retention_policy(&self) -> TrashRetentionPolicy {
        self.trash_retention_policy
    }

    pub async fn list_libraries(
        &self,
        user_id: UserId,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<LibraryPage, FileMetadataError> {
        let limit = validate_limit(limit)?;
        let after = decode_library_cursor(cursor.as_deref())?;
        let rows = DomainRepository::new(&self.pool)
            .list_owned_libraries(user_id, after, limit)
            .await
            .map_err(map_metadata_error)?;
        let next_cursor = rows
            .has_more
            .then(|| {
                rows.libraries
                    .last()
                    .map(|library| encode_library_cursor(library.id()))
            })
            .flatten();
        Ok(LibraryPage::new(rows.libraries, next_cursor, rows.has_more))
    }

    pub async fn create_library(
        &self,
        user_id: UserId,
        library_id: LibraryId,
        name: LogicalName,
    ) -> Result<Library, FileMetadataError> {
        let library = DomainRepository::new(&self.pool)
            .create_library_owned(user_id, library_id, name.clone(), Timestamp::now())
            .await
            .map_err(map_metadata_error)?;
        if library.owner_user_id() != user_id {
            // Do not reveal whether a caller-supplied UUID belongs to another
            // owner. The API maps this to the same inaccessible-resource
            // boundary used by ordinary metadata reads.
            return Err(FileMetadataError::NotFound);
        }
        if library.name() != &name {
            return Err(FileMetadataError::InvalidRequest);
        }
        Ok(library)
    }

    pub async fn list_children(
        &self,
        user_id: UserId,
        library_id: LibraryId,
        parent_node_id: Option<NodeId>,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<NodePage, FileMetadataError> {
        let limit = validate_limit(limit)?;
        let parent_node_id = DomainRepository::new(&self.pool)
            .resolve_owned_parent(user_id, library_id, parent_node_id)
            .await
            .map_err(map_metadata_error)?
            .ok_or(FileMetadataError::NotFound)?;
        let after = decode_node_cursor(cursor.as_deref(), library_id, parent_node_id)?;
        let rows = DomainRepository::new(&self.pool)
            .list_owned_children(user_id, library_id, parent_node_id, after, limit)
            .await
            .map_err(map_metadata_error)?;
        let next_cursor = rows
            .has_more
            .then(|| {
                rows.nodes
                    .last()
                    .map(|node| encode_node_cursor(library_id, parent_node_id, node.id()))
            })
            .flatten();
        Ok(NodePage::new(rows.nodes, next_cursor, rows.has_more))
    }

    pub async fn get_node(
        &self,
        user_id: UserId,
        node_id: NodeId,
    ) -> Result<Node, FileMetadataError> {
        DomainRepository::new(&self.pool)
            .find_owned_node(user_id, node_id)
            .await
            .map_err(map_metadata_error)?
            .ok_or(FileMetadataError::NotFound)
    }

    pub async fn create_directory(
        &self,
        user_id: UserId,
        library_id: LibraryId,
        parent_node_id: Option<NodeId>,
        name: LogicalName,
    ) -> Result<Node, FileMetadataError> {
        DomainRepository::new(&self.pool)
            .create_directory_owned(user_id, library_id, parent_node_id, name, now())
            .await
            .map_err(map_metadata_error)?
            .ok_or(FileMetadataError::NotFound)
    }

    pub async fn rename_node(
        &self,
        user_id: UserId,
        node_id: NodeId,
        name: LogicalName,
        expected_revision: Revision,
    ) -> Result<Node, FileMetadataError> {
        match DomainRepository::new(&self.pool)
            .rename_node_owned(user_id, node_id, name, expected_revision, now())
            .await
            .map_err(map_metadata_error)?
        {
            NodeMutation::Applied(node) => Ok(node),
            NodeMutation::NotFound => Err(FileMetadataError::NotFound),
            NodeMutation::VersionConflict(node) => Err(FileMetadataError::VersionConflict {
                current_revision: node.revision(),
            }),
        }
    }

    pub async fn move_node(
        &self,
        user_id: UserId,
        node_id: NodeId,
        destination_parent_id: NodeId,
        expected_revision: Revision,
    ) -> Result<Node, FileMetadataError> {
        match DomainRepository::new(&self.pool)
            .move_node_owned(
                user_id,
                node_id,
                destination_parent_id,
                expected_revision,
                now(),
            )
            .await
            .map_err(map_metadata_error)?
        {
            NodeMutation::Applied(node) => Ok(node),
            NodeMutation::NotFound => Err(FileMetadataError::NotFound),
            NodeMutation::VersionConflict(node) => Err(FileMetadataError::VersionConflict {
                current_revision: node.revision(),
            }),
        }
    }

    pub async fn delete_node(
        &self,
        user_id: UserId,
        node_id: NodeId,
        expected_revision: Revision,
    ) -> Result<Node, FileMetadataError> {
        match DomainRepository::new(&self.pool)
            .delete_node_owned(user_id, node_id, expected_revision, now())
            .await
            .map_err(map_metadata_error)?
        {
            NodeMutation::Applied(node) => Ok(node),
            NodeMutation::NotFound => Err(FileMetadataError::NotFound),
            NodeMutation::VersionConflict(node) => Err(FileMetadataError::VersionConflict {
                current_revision: node.revision(),
            }),
        }
    }

    pub async fn restore_node(
        &self,
        user_id: UserId,
        node_id: NodeId,
        expected_revision: Revision,
    ) -> Result<Node, FileMetadataError> {
        match DomainRepository::new(&self.pool)
            .restore_node_owned(user_id, node_id, expected_revision, now())
            .await
            .map_err(map_metadata_error)?
        {
            NodeMutation::Applied(node) => Ok(node),
            NodeMutation::NotFound => Err(FileMetadataError::NotFound),
            NodeMutation::VersionConflict(node) => Err(FileMetadataError::VersionConflict {
                current_revision: node.revision(),
            }),
        }
    }
}

#[async_trait]
impl FileMetadataBackend for FileMetadataService {
    async fn list_libraries(
        &self,
        user_id: UserId,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<LibraryPage, FileMetadataError> {
        self.list_libraries(user_id, cursor, limit).await
    }

    async fn create_library(
        &self,
        user_id: UserId,
        library_id: LibraryId,
        name: LogicalName,
    ) -> Result<Library, FileMetadataError> {
        self.create_library(user_id, library_id, name).await
    }

    async fn list_children(
        &self,
        user_id: UserId,
        library_id: LibraryId,
        parent_node_id: Option<NodeId>,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<NodePage, FileMetadataError> {
        self.list_children(user_id, library_id, parent_node_id, cursor, limit)
            .await
    }

    async fn get_node(&self, user_id: UserId, node_id: NodeId) -> Result<Node, FileMetadataError> {
        self.get_node(user_id, node_id).await
    }

    async fn create_directory(
        &self,
        user_id: UserId,
        library_id: LibraryId,
        parent_node_id: Option<NodeId>,
        name: LogicalName,
    ) -> Result<Node, FileMetadataError> {
        self.create_directory(user_id, library_id, parent_node_id, name)
            .await
    }

    async fn rename_node(
        &self,
        user_id: UserId,
        node_id: NodeId,
        name: LogicalName,
        expected_revision: Revision,
    ) -> Result<Node, FileMetadataError> {
        self.rename_node(user_id, node_id, name, expected_revision)
            .await
    }

    async fn move_node(
        &self,
        user_id: UserId,
        node_id: NodeId,
        destination_parent_id: NodeId,
        expected_revision: Revision,
    ) -> Result<Node, FileMetadataError> {
        self.move_node(user_id, node_id, destination_parent_id, expected_revision)
            .await
    }

    async fn delete_node(
        &self,
        user_id: UserId,
        node_id: NodeId,
        expected_revision: Revision,
    ) -> Result<Node, FileMetadataError> {
        self.delete_node(user_id, node_id, expected_revision).await
    }

    async fn restore_node(
        &self,
        user_id: UserId,
        node_id: NodeId,
        expected_revision: Revision,
    ) -> Result<Node, FileMetadataError> {
        self.restore_node(user_id, node_id, expected_revision).await
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum NodeMutation {
    Applied(Node),
    NotFound,
    VersionConflict(Node),
}

fn validate_limit(limit: u32) -> Result<u32, FileMetadataError> {
    if (1..=MAX_PAGE_LIMIT).contains(&limit) {
        Ok(limit)
    } else {
        Err(FileMetadataError::InvalidRequest)
    }
}

fn now() -> Timestamp {
    let value = Timestamp::now().as_offset_datetime();
    let nanos = value.nanosecond();
    Timestamp::from_offset_datetime(
        value
            .replace_nanosecond(nanos / 1_000 * 1_000)
            .expect("microsecond truncation remains a valid timestamp"),
    )
}

fn map_metadata_error(error: MetadataError) -> FileMetadataError {
    match error {
        MetadataError::Database(error) => FileMetadataError::Database(error),
        MetadataError::Mapping(MappingError::Domain(error)) => map_domain_error(error),
        MetadataError::Mapping(_) => FileMetadataError::InvalidPersistedData,
        MetadataError::CapacityUnavailable => FileMetadataError::InvalidState,
    }
}

fn map_domain_error(error: DomainError) -> FileMetadataError {
    match error {
        DomainError::ParentLibraryMismatch | DomainError::NodeLibraryMismatch => {
            FileMetadataError::PermissionDenied
        }
        DomainError::InvalidTrashTimestamp => FileMetadataError::InvalidPersistedData,
        DomainError::DirectoryNotEmpty
        | DomainError::LibraryNotWritable
        | DomainError::RootCannotBeDeleted
        | DomainError::RootCannotHaveParent
        | DomainError::ParentMustBeDirectory
        | DomainError::ParentCannotBeSelf
        | DomainError::ParentCycle
        | DomainError::ParentChainMismatch
        | DomainError::ParentReferenceMismatch
        | DomainError::InvalidNodeStateTransition { .. }
        | DomainError::ParentMustBeActive => FileMetadataError::InvalidState,
        DomainError::RevisionOverflow => FileMetadataError::InvalidPersistedData,
        DomainError::EmptyName | DomainError::NameTooLong => FileMetadataError::InvalidRequest,
        _ => FileMetadataError::InvalidRequest,
    }
}

fn encode_library_cursor(library_id: LibraryId) -> String {
    format!("{CURSOR_VERSION}.library.{library_id}")
}

fn decode_library_cursor(value: Option<&str>) -> Result<Option<LibraryId>, FileMetadataError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != 3 || parts[0] != CURSOR_VERSION || parts[1] != "library" {
        return Err(FileMetadataError::InvalidCursor);
    }
    LibraryId::from_str(parts[2])
        .map(Some)
        .map_err(|_| FileMetadataError::InvalidCursor)
}

fn encode_node_cursor(library_id: LibraryId, parent_node_id: NodeId, node_id: NodeId) -> String {
    format!("{CURSOR_VERSION}.node.{library_id}.{parent_node_id}.{node_id}")
}

fn decode_node_cursor(
    value: Option<&str>,
    library_id: LibraryId,
    parent_node_id: NodeId,
) -> Result<Option<NodeId>, FileMetadataError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != MAX_CURSOR_PARTS + 1 || parts[0] != CURSOR_VERSION || parts[1] != "node" {
        return Err(FileMetadataError::InvalidCursor);
    }
    let cursor_library =
        LibraryId::from_str(parts[2]).map_err(|_| FileMetadataError::InvalidCursor)?;
    let cursor_parent = NodeId::from_str(parts[3]).map_err(|_| FileMetadataError::InvalidCursor)?;
    let cursor_node = NodeId::from_str(parts[4]).map_err(|_| FileMetadataError::InvalidCursor)?;
    if cursor_library != library_id || cursor_parent != parent_node_id {
        return Err(FileMetadataError::InvalidCursor);
    }
    Ok(Some(cursor_node))
}

#[cfg(test)]
mod tests {
    use super::{
        CURSOR_VERSION, decode_library_cursor, decode_node_cursor, encode_library_cursor,
        encode_node_cursor, validate_limit,
    };
    use synveil_core::{LibraryId, NodeId};

    #[test]
    fn cursors_are_scoped_and_reject_malformed_values() {
        let library = LibraryId::new();
        let parent = NodeId::new();
        let node = NodeId::new();
        let library_cursor = encode_library_cursor(library);
        assert_eq!(
            decode_library_cursor(Some(&library_cursor)).unwrap(),
            Some(library)
        );
        assert!(decode_library_cursor(Some("v0.library.bad")).is_err());

        let cursor = encode_node_cursor(library, parent, node);
        assert_eq!(
            decode_node_cursor(Some(&cursor), library, parent).unwrap(),
            Some(node)
        );
        assert!(decode_node_cursor(Some(&cursor), LibraryId::new(), parent).is_err());
        assert!(decode_node_cursor(Some("v1.node"), library, parent).is_err());
        assert_eq!(CURSOR_VERSION, "v1");
    }

    #[test]
    fn page_limits_are_bounded() {
        assert!(validate_limit(1).is_ok());
        assert!(validate_limit(super::MAX_PAGE_LIMIT).is_ok());
        assert!(validate_limit(0).is_err());
        assert!(validate_limit(super::MAX_PAGE_LIMIT + 1).is_err());
    }
}
