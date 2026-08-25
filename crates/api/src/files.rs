//! HTTP DTOs and handlers for authenticated logical file/folder metadata.
//!
//! The transport contract intentionally exposes only names, hierarchy,
//! logical state, revisions, and timestamps. No handler in this module reads
//! bytes, resolves an operating-system path, touches an object store, or
//! creates a sync/backup/sharing side effect.

use std::str::FromStr;

use async_trait::async_trait;
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use synveil_core::{
    Library, LibraryId, LogicalName, Node, NodeId, Revision, Timestamp, TrashRetentionPolicy,
    UserId,
};
use synveil_metadata::{
    DEFAULT_PAGE_LIMIT, DatabaseError, DatabaseErrorKind, FileMetadataBackend, FileMetadataError,
    LibraryPage, NodePage,
};

use crate::{
    ApiError, ApiState, EtagKey, RequestContext,
    auth::{AuthContext, ResponseMeta},
};

/// The metadata routes accept only small JSON command documents. This is an
/// application-level guard in addition to the global transport limit.
pub const FILE_METADATA_BODY_LIMIT_BYTES: usize = 16 * 1024;

/// Fail-closed backend used by an unconfigured composition root. It prevents
/// a route from accidentally claiming that metadata writes succeeded before
/// PostgreSQL has been installed in the application state.
pub(crate) struct UnavailableFileMetadataBackend;

#[async_trait]
impl FileMetadataBackend for UnavailableFileMetadataBackend {
    async fn list_libraries(
        &self,
        _user_id: UserId,
        _cursor: Option<String>,
        _limit: u32,
    ) -> Result<LibraryPage, FileMetadataError> {
        Err(unavailable_error())
    }

    async fn list_children(
        &self,
        _user_id: UserId,
        _library_id: LibraryId,
        _parent_node_id: Option<NodeId>,
        _cursor: Option<String>,
        _limit: u32,
    ) -> Result<NodePage, FileMetadataError> {
        Err(unavailable_error())
    }

    async fn get_node(
        &self,
        _user_id: UserId,
        _node_id: NodeId,
    ) -> Result<Node, FileMetadataError> {
        Err(unavailable_error())
    }

    async fn create_directory(
        &self,
        _user_id: UserId,
        _library_id: LibraryId,
        _parent_node_id: Option<NodeId>,
        _name: LogicalName,
    ) -> Result<Node, FileMetadataError> {
        Err(unavailable_error())
    }

    async fn rename_node(
        &self,
        _user_id: UserId,
        _node_id: NodeId,
        _name: LogicalName,
        _expected_revision: Revision,
    ) -> Result<Node, FileMetadataError> {
        Err(unavailable_error())
    }

    async fn move_node(
        &self,
        _user_id: UserId,
        _node_id: NodeId,
        _destination_parent_id: NodeId,
        _expected_revision: Revision,
    ) -> Result<Node, FileMetadataError> {
        Err(unavailable_error())
    }

    async fn delete_node(
        &self,
        _user_id: UserId,
        _node_id: NodeId,
        _expected_revision: Revision,
    ) -> Result<Node, FileMetadataError> {
        Err(unavailable_error())
    }

    async fn restore_node(
        &self,
        _user_id: UserId,
        _node_id: NodeId,
        _expected_revision: Revision,
    ) -> Result<Node, FileMetadataError> {
        Err(unavailable_error())
    }
}

fn unavailable_error() -> FileMetadataError {
    FileMetadataError::Database(DatabaseError::Failure(
        DatabaseErrorKind::ConnectionUnavailable,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PageQuery {
    cursor: Option<String>,
    limit: Option<u32>,
}

impl PageQuery {
    fn limit(&self) -> u32 {
        self.limit.unwrap_or(DEFAULT_PAGE_LIMIT)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChildrenQuery {
    parent_id: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
}

impl ChildrenQuery {
    fn limit(&self) -> u32 {
        self.limit.unwrap_or(DEFAULT_PAGE_LIMIT)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateDirectoryRequest {
    name: String,
    parent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateNodeRequest {
    name: Option<String>,
    parent_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CollectionResponse<T> {
    data: Vec<T>,
    page: PageResponse,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct PageResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
    has_more: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ResourceResponse<T> {
    data: T,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct NodeResource {
    id: String,
    #[serde(rename = "type")]
    resource_type: &'static str,
    revision: String,
    attributes: NodeAttributes,
}

#[derive(Debug, Serialize)]
pub(crate) struct NodeAttributes {
    library_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_id: Option<String>,
    name: String,
    kind: &'static str,
    state: &'static str,
    created_at: String,
    updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    trashed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    restore_deadline: Option<String>,
    purge_eligible: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct LibraryResource {
    id: String,
    #[serde(rename = "type")]
    resource_type: &'static str,
    revision: String,
    attributes: LibraryAttributes,
}

#[derive(Debug, Serialize)]
pub(crate) struct LibraryAttributes {
    name: String,
    root_node_id: String,
    status: &'static str,
    created_at: String,
    updated_at: String,
}

pub(crate) async fn list_libraries(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Extension(context): Extension<RequestContext>,
    Query(query): Query<PageQuery>,
) -> Result<Json<CollectionResponse<LibraryResource>>, ApiError> {
    let limit = query.limit();
    let page = state
        .file_metadata_backend()
        .list_libraries(auth.principal().user_id(), query.cursor, limit)
        .await
        .map_err(|error| map_file_error(error, None, state.etag_key()))?;
    Ok(Json(library_page_response(page, &context)))
}

pub(crate) async fn list_children(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Extension(context): Extension<RequestContext>,
    Path(library_id): Path<String>,
    Query(query): Query<ChildrenQuery>,
) -> Result<Json<CollectionResponse<NodeResource>>, ApiError> {
    let limit = query.limit();
    let cursor = query.cursor;
    let library_id = parse_id::<LibraryId>(&library_id)?;
    let parent_id = query
        .parent_id
        .as_deref()
        .map(parse_id::<NodeId>)
        .transpose()?;
    let page = state
        .file_metadata_backend()
        .list_children(
            auth.principal().user_id(),
            library_id,
            parent_id,
            cursor,
            limit,
        )
        .await
        .map_err(|error| map_file_error(error, None, state.etag_key()))?;
    Ok(Json(node_page_response(
        page,
        &context,
        state.trash_retention_policy(),
    )))
}

pub(crate) async fn create_directory(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Extension(context): Extension<RequestContext>,
    Path(library_id): Path<String>,
    Json(payload): Json<CreateDirectoryRequest>,
) -> Result<Response, ApiError> {
    let library_id = parse_id::<LibraryId>(&library_id)?;
    let parent_id = payload
        .parent_id
        .as_deref()
        .map(parse_id::<NodeId>)
        .transpose()?;
    let name = LogicalName::new(payload.name).map_err(|_| ApiError::InvalidRequest)?;
    let node = state
        .file_metadata_backend()
        .create_directory(auth.principal().user_id(), library_id, parent_id, name)
        .await
        .map_err(|error| map_file_error(error, None, state.etag_key()))?;
    Ok(node_response(
        StatusCode::CREATED,
        node,
        &context,
        state.etag_key(),
        state.trash_retention_policy(),
    ))
}

pub(crate) async fn get_node(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Extension(context): Extension<RequestContext>,
    Path(node_id): Path<String>,
) -> Result<Response, ApiError> {
    let node_id = parse_id::<NodeId>(&node_id)?;
    let node = state
        .file_metadata_backend()
        .get_node(auth.principal().user_id(), node_id)
        .await
        .map_err(|error| map_file_error(error, Some(node_id), state.etag_key()))?;
    Ok(node_response(
        StatusCode::OK,
        node,
        &context,
        state.etag_key(),
        state.trash_retention_policy(),
    ))
}

pub(crate) async fn update_node(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Extension(context): Extension<RequestContext>,
    Path(node_id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<UpdateNodeRequest>,
) -> Result<Response, ApiError> {
    let node_id = parse_id::<NodeId>(&node_id)?;
    let expected_revision = expected_revision(&headers, node_id, state.etag_key())?;
    let (name, parent_id) = match (payload.name, payload.parent_id) {
        (Some(name), None) => (
            Some(LogicalName::new(name).map_err(|_| ApiError::InvalidRequest)?),
            None,
        ),
        (None, Some(parent_id)) => (None, Some(parse_id::<NodeId>(&parent_id)?)),
        _ => return Err(ApiError::InvalidRequest),
    };
    let node = if let Some(name) = name {
        state
            .file_metadata_backend()
            .rename_node(auth.principal().user_id(), node_id, name, expected_revision)
            .await
    } else {
        state
            .file_metadata_backend()
            .move_node(
                auth.principal().user_id(),
                node_id,
                parent_id.expect("validated move request has a parent"),
                expected_revision,
            )
            .await
    }
    .map_err(|error| map_file_error(error, Some(node_id), state.etag_key()))?;
    Ok(node_response(
        StatusCode::OK,
        node,
        &context,
        state.etag_key(),
        state.trash_retention_policy(),
    ))
}

pub(crate) async fn delete_node(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Extension(context): Extension<RequestContext>,
    Path(node_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let node_id = parse_id::<NodeId>(&node_id)?;
    let expected_revision = expected_revision(&headers, node_id, state.etag_key())?;
    let node = state
        .file_metadata_backend()
        .delete_node(auth.principal().user_id(), node_id, expected_revision)
        .await
        .map_err(|error| map_file_error(error, Some(node_id), state.etag_key()))?;
    Ok(node_response(
        StatusCode::OK,
        node,
        &context,
        state.etag_key(),
        state.trash_retention_policy(),
    ))
}

pub(crate) async fn restore_node(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Extension(context): Extension<RequestContext>,
    Path(node_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let node_id = parse_id::<NodeId>(&node_id)?;
    let expected_revision = expected_revision(&headers, node_id, state.etag_key())?;
    let node = state
        .file_metadata_backend()
        .restore_node(auth.principal().user_id(), node_id, expected_revision)
        .await
        .map_err(|error| map_file_error(error, Some(node_id), state.etag_key()))?;
    Ok(node_response(
        StatusCode::OK,
        node,
        &context,
        state.etag_key(),
        state.trash_retention_policy(),
    ))
}

fn parse_id<T>(value: &str) -> Result<T, ApiError>
where
    T: FromStr,
{
    value.parse().map_err(|_| ApiError::InvalidRequest)
}

fn expected_revision(
    headers: &HeaderMap,
    node_id: NodeId,
    etag_key: &EtagKey,
) -> Result<Revision, ApiError> {
    let value = headers
        .get(header::IF_MATCH)
        .ok_or(ApiError::PreconditionRequired)?
        .to_str()
        .map_err(|_| ApiError::Core(synveil_core::ErrorCode::VersionConflict))?;
    etag_key
        .verify(value, node_id)
        .ok_or(ApiError::Core(synveil_core::ErrorCode::VersionConflict))
}

fn map_file_error(
    error: FileMetadataError,
    node_id: Option<NodeId>,
    etag_key: &EtagKey,
) -> ApiError {
    match error {
        FileMetadataError::NotFound => ApiError::Core(synveil_core::ErrorCode::NotFound),
        FileMetadataError::PermissionDenied => {
            ApiError::Core(synveil_core::ErrorCode::PermissionDenied)
        }
        FileMetadataError::InvalidRequest => ApiError::InvalidRequest,
        FileMetadataError::InvalidCursor => ApiError::Core(synveil_core::ErrorCode::InvalidCursor),
        FileMetadataError::VersionConflict { current_revision } => ApiError::VersionConflict {
            current_revision: Some(current_revision.to_string()),
            current_etag: node_id.map(|id| etag_key.issue(id, current_revision)),
        },
        FileMetadataError::InvalidState => ApiError::Core(synveil_core::ErrorCode::InvalidState),
        FileMetadataError::Database(_) => ApiError::ReadinessUnavailable,
        FileMetadataError::InvalidPersistedData => ApiError::Internal,
    }
}

fn library_page_response(
    page: LibraryPage,
    context: &RequestContext,
) -> CollectionResponse<LibraryResource> {
    CollectionResponse {
        data: page.libraries().iter().map(library_resource).collect(),
        page: PageResponse {
            next_cursor: page.next_cursor().map(str::to_owned),
            has_more: page.has_more(),
        },
        meta: response_meta(context),
    }
}

fn node_page_response(
    page: NodePage,
    context: &RequestContext,
    policy: TrashRetentionPolicy,
) -> CollectionResponse<NodeResource> {
    CollectionResponse {
        data: page
            .nodes()
            .iter()
            .map(|node| node_resource(node, policy))
            .collect(),
        page: PageResponse {
            next_cursor: page.next_cursor().map(str::to_owned),
            has_more: page.has_more(),
        },
        meta: response_meta(context),
    }
}

fn node_response(
    status: StatusCode,
    node: Node,
    context: &RequestContext,
    etag_key: &EtagKey,
    policy: TrashRetentionPolicy,
) -> Response {
    let etag = etag_key.issue(node.id(), node.revision());
    let mut response = (
        status,
        Json(ResourceResponse {
            data: node_resource(&node, policy),
            meta: response_meta(context),
        }),
    )
        .into_response();
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(&etag).expect("generated ETag is valid HTTP header data"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-cache"),
    );
    response
}

fn node_resource(node: &Node, policy: TrashRetentionPolicy) -> NodeResource {
    let trashed_at = node.trashed_at();
    NodeResource {
        id: node.id().to_string(),
        resource_type: "node",
        revision: node.revision().to_string(),
        attributes: NodeAttributes {
            library_id: node.library_id().to_string(),
            parent_id: node.parent_node_id().map(|id| id.to_string()),
            name: node.name().as_str().to_owned(),
            kind: node.kind().as_str(),
            state: node.state().as_str(),
            created_at: node.created_at().to_string(),
            updated_at: node.updated_at().to_string(),
            trashed_at: trashed_at.map(|timestamp| timestamp.to_string()),
            restore_deadline: trashed_at
                .and_then(|timestamp| policy.restore_deadline(timestamp))
                .map(|timestamp| timestamp.to_string()),
            purge_eligible: policy.is_purge_eligible(
                node.state(),
                node.is_root(),
                trashed_at,
                Timestamp::now(),
            ),
        },
    }
}

fn library_resource(library: &Library) -> LibraryResource {
    LibraryResource {
        id: library.id().to_string(),
        resource_type: "library",
        revision: library.revision().to_string(),
        attributes: LibraryAttributes {
            name: library.name().as_str().to_owned(),
            root_node_id: library.root_node_id().to_string(),
            status: library.status().as_str(),
            created_at: library.created_at().to_string(),
            updated_at: library.updated_at().to_string(),
        },
    }
}

fn response_meta(context: &RequestContext) -> ResponseMeta {
    ResponseMeta {
        request_id: context.request_id().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::FILE_METADATA_BODY_LIMIT_BYTES;
    use synveil_metadata::MAX_PAGE_LIMIT;

    #[test]
    fn metadata_transport_limits_are_bounded() {
        assert_eq!(FILE_METADATA_BODY_LIMIT_BYTES, 16 * 1024);
        assert_eq!(MAX_PAGE_LIMIT, 100);
    }
}
