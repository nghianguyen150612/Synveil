//! Authenticated HTTP transport for immutable file-version metadata.
//!
//! This boundary returns only logical, owner-authorized version history. It
//! never opens an object store and never serializes object, replica, backend,
//! staging, or filesystem identity.

use std::str::FromStr;

use async_trait::async_trait;
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use synveil_core::{FileVersionId, NodeId, Revision, UserId};
use synveil_metadata::{
    DEFAULT_PAGE_LIMIT, DatabaseError, DatabaseErrorKind, FileVersionMetadata, FileVersionPage,
    MAX_RESTORE_IDEMPOTENCY_KEY_BYTES, MIN_RESTORE_IDEMPOTENCY_KEY_BYTES, RestoredFileVersion,
    VersionHistoryBackend, VersionHistoryError, VersionRestoreBackend, VersionRestoreError,
};

use crate::{
    ApiError, ApiState, RequestContext,
    auth::{AuthContext, AuthenticatedPrincipal, ResponseMeta},
};

/// Fail-closed backend used until a PostgreSQL-backed version-history service
/// is wired into the composition root.
pub(crate) struct UnavailableVersionHistoryBackend;

#[async_trait]
impl VersionHistoryBackend for UnavailableVersionHistoryBackend {
    async fn list_file_versions(
        &self,
        _user_id: UserId,
        _node_id: NodeId,
        _cursor: Option<String>,
        _limit: u32,
    ) -> Result<FileVersionPage, VersionHistoryError> {
        Err(unavailable_error())
    }

    async fn get_file_version_metadata(
        &self,
        _user_id: UserId,
        _version_id: FileVersionId,
    ) -> Result<FileVersionMetadata, VersionHistoryError> {
        Err(unavailable_error())
    }
}

pub(crate) struct UnavailableVersionRestoreBackend;

#[async_trait]
impl VersionRestoreBackend for UnavailableVersionRestoreBackend {
    async fn restore_file_version(
        &self,
        _user_id: UserId,
        _node_id: NodeId,
        _source_version_id: FileVersionId,
        _expected_revision: Revision,
        _idempotency_key: String,
    ) -> Result<RestoredFileVersion, VersionRestoreError> {
        Err(VersionRestoreError::Database(DatabaseError::Failure(
            DatabaseErrorKind::ConnectionUnavailable,
        )))
    }
}

fn unavailable_error() -> VersionHistoryError {
    VersionHistoryError::Database(DatabaseError::Failure(
        DatabaseErrorKind::ConnectionUnavailable,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VersionPageQuery {
    cursor: Option<String>,
    limit: Option<u32>,
}

impl VersionPageQuery {
    fn limit(&self) -> u32 {
        self.limit.unwrap_or(DEFAULT_PAGE_LIMIT)
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct VersionCollectionResponse {
    data: Vec<VersionResource>,
    page: VersionPageResponse,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct VersionPageResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
    has_more: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct VersionResourceResponse {
    data: VersionResource,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct RestoreVersionResponse {
    data: VersionResource,
    node: RestoreNodeConcurrency,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct RestoreNodeConcurrency {
    id: String,
    revision: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct VersionResource {
    id: String,
    #[serde(rename = "type")]
    resource_type: &'static str,
    attributes: VersionAttributes,
}

#[derive(Debug, Serialize)]
pub(crate) struct VersionAttributes {
    node_id: String,
    created_at: String,
    byte_length: String,
    sha256: String,
    is_current: bool,
}

pub(crate) async fn list_versions(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Extension(context): Extension<RequestContext>,
    Path(node_id): Path<String>,
    Query(query): Query<VersionPageQuery>,
) -> Result<Response, ApiError> {
    let node_id = parse_id::<NodeId>(&node_id)?;
    let limit = query.limit();
    let page = state
        .version_history_backend()
        .list_file_versions(auth.principal().user_id(), node_id, query.cursor, limit)
        .await
        .map_err(map_version_error)?;
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(VersionCollectionResponse {
                data: page
                    .versions()
                    .iter()
                    .copied()
                    .map(version_resource)
                    .collect(),
                page: VersionPageResponse {
                    next_cursor: page.next_cursor().map(str::to_owned),
                    has_more: page.has_more(),
                },
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn get_version(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(version_id): Path<String>,
) -> Result<Response, ApiError> {
    let version_id = parse_id::<FileVersionId>(&version_id)?;
    let version = state
        .version_history_backend()
        .get_file_version_metadata(auth.owner_user_id(), version_id)
        .await
        .map_err(map_version_error)?;
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(VersionResourceResponse {
                data: version_resource(version),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn restore_version(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Extension(context): Extension<RequestContext>,
    Path((node_id, source_version_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let node_id = parse_id::<NodeId>(&node_id)?;
    let source_version_id = parse_id::<FileVersionId>(&source_version_id)?;
    let expected_revision = expected_revision(&headers, node_id, state.etag_key())?;
    let idempotency_key = idempotency_key(&headers)?;
    let restored = state
        .version_restore_backend()
        .restore_file_version(
            auth.principal().user_id(),
            node_id,
            source_version_id,
            expected_revision,
            idempotency_key,
        )
        .await
        .map_err(|error| map_restore_error(error, node_id, state.etag_key()))?;
    Ok(restore_response(restored, &context, state.etag_key()))
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
    etag_key: &crate::EtagKey,
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

fn idempotency_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let value = headers
        .get(header::HeaderName::from_static("idempotency-key"))
        .ok_or(ApiError::InvalidRequest)?
        .to_str()
        .map_err(|_| ApiError::InvalidRequest)?;
    if !(MIN_RESTORE_IDEMPOTENCY_KEY_BYTES..=MAX_RESTORE_IDEMPOTENCY_KEY_BYTES)
        .contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'~' | b'-'))
    {
        return Err(ApiError::InvalidRequest);
    }
    Ok(value.to_owned())
}

fn map_version_error(error: VersionHistoryError) -> ApiError {
    match error {
        VersionHistoryError::NotFound => ApiError::Core(synveil_core::ErrorCode::NotFound),
        VersionHistoryError::InvalidRequest => ApiError::InvalidRequest,
        VersionHistoryError::InvalidCursor => {
            ApiError::Core(synveil_core::ErrorCode::InvalidCursor)
        }
        VersionHistoryError::InvalidState => ApiError::Core(synveil_core::ErrorCode::InvalidState),
        VersionHistoryError::Database(_) => ApiError::ReadinessUnavailable,
        VersionHistoryError::InvalidPersistedData => ApiError::Internal,
    }
}

fn map_restore_error(
    error: VersionRestoreError,
    node_id: NodeId,
    etag_key: &crate::EtagKey,
) -> ApiError {
    match error {
        VersionRestoreError::NotFound => ApiError::Core(synveil_core::ErrorCode::NotFound),
        VersionRestoreError::InvalidRequest => ApiError::InvalidRequest,
        VersionRestoreError::InvalidState => ApiError::Core(synveil_core::ErrorCode::InvalidState),
        VersionRestoreError::VersionConflict { current_revision } => ApiError::VersionConflict {
            current_revision: Some(current_revision.to_string()),
            current_etag: Some(etag_key.issue(node_id, current_revision)),
        },
        VersionRestoreError::ContentUnavailable => {
            ApiError::Core(synveil_core::ErrorCode::StorageUnavailable)
        }
        VersionRestoreError::IdempotencyConflict => ApiError::IdempotencyConflict,
        VersionRestoreError::Database(_) => ApiError::ReadinessUnavailable,
        VersionRestoreError::InvalidPersistedData => ApiError::Internal,
    }
}

fn version_resource(version: FileVersionMetadata) -> VersionResource {
    VersionResource {
        id: version.id().to_string(),
        resource_type: "file_version",
        attributes: VersionAttributes {
            node_id: version.node_id().to_string(),
            created_at: version.committed_at().to_string(),
            byte_length: version.byte_length().to_string(),
            sha256: version.sha256().to_string(),
            is_current: version.is_current(),
        },
    }
}

fn restore_response(
    restored: RestoredFileVersion,
    context: &RequestContext,
    etag_key: &crate::EtagKey,
) -> Response {
    let version = restored.version();
    let etag = etag_key.issue(version.node_id(), restored.node_revision());
    let mut response = (
        StatusCode::OK,
        Json(RestoreVersionResponse {
            data: version_resource(version),
            node: RestoreNodeConcurrency {
                id: version.node_id().to_string(),
                revision: restored.node_revision().to_string(),
            },
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
        HeaderValue::from_static("private, no-store"),
    );
    response
}

fn response_meta(context: &RequestContext) -> ResponseMeta {
    ResponseMeta {
        request_id: context.request_id().to_string(),
    }
}

fn private_no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}
