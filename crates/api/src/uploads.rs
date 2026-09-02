//! Authenticated HTTP transport for exact-offset resumable upload sessions.
//!
//! This module translates reviewed HTTP shapes into the transport-neutral
//! upload application service. It never opens a path, handles an object key,
//! promotes content, or writes upload metadata directly.

use std::str::FromStr;

use async_trait::async_trait;
use axum::{
    Json,
    body::Body,
    extract::{Extension, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use synveil_core::{
    LibraryId, LogicalName, NodeId, Revision, Sha256Digest, UploadSessionId, UserId,
};
use synveil_metadata::UploadCompletion;
use synveil_storage::{
    CreateUploadSessionRequest as ApplicationCreateRequest, UploadApplicationService,
    UploadByteStream, UploadError, UploadProgress, UploadSessionView, UploadTargetRequest,
    UploadTargetView, boxed_upload_stream,
};

use crate::{
    ApiError, ApiState, RequestContext,
    auth::{AuthContext, ResponseMeta},
};

/// Create-session commands are deliberately small JSON documents.
pub const UPLOAD_JSON_BODY_LIMIT_BYTES: usize = 16 * 1024;
pub const UPLOAD_OFFSET_HEADER_NAME: &str = "upload-offset";

/// Application-facing port used by the HTTP layer and its focused tests.
/// Implementations retain ownership of all upload state-machine decisions.
#[async_trait]
pub trait UploadBackend: Send + Sync {
    fn max_chunk_size(&self) -> u64;

    async fn create_upload_session(
        &self,
        request: ApplicationCreateRequest,
    ) -> Result<UploadSessionView, UploadError>;

    async fn get_upload_session(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
    ) -> Result<UploadSessionView, UploadError>;

    async fn append_upload_stream(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        expected_offset: u64,
        stream: UploadByteStream,
    ) -> Result<UploadProgress, UploadError>;

    async fn complete_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
    ) -> Result<UploadCompletion, UploadError>;

    async fn abort_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
    ) -> Result<UploadSessionView, UploadError>;
}

#[async_trait]
impl UploadBackend for UploadApplicationService {
    fn max_chunk_size(&self) -> u64 {
        self.limits().max_chunk_size
    }

    async fn create_upload_session(
        &self,
        request: ApplicationCreateRequest,
    ) -> Result<UploadSessionView, UploadError> {
        UploadApplicationService::create_upload_session(self, request).await
    }

    async fn get_upload_session(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
    ) -> Result<UploadSessionView, UploadError> {
        UploadApplicationService::get_upload_session(self, owner_user_id, session_id).await
    }

    async fn append_upload_stream(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
        expected_offset: u64,
        stream: UploadByteStream,
    ) -> Result<UploadProgress, UploadError> {
        UploadApplicationService::append_upload_stream(
            self,
            owner_user_id,
            session_id,
            expected_offset,
            stream,
        )
        .await
    }

    async fn complete_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
    ) -> Result<UploadCompletion, UploadError> {
        UploadApplicationService::complete_upload(self, owner_user_id, session_id).await
    }

    async fn abort_upload(
        &self,
        owner_user_id: UserId,
        session_id: UploadSessionId,
    ) -> Result<UploadSessionView, UploadError> {
        UploadApplicationService::abort_upload(self, owner_user_id, session_id).await
    }
}

/// Fail-closed backend for composition roots without upload persistence and
/// object storage. Routes remain authenticated and return a safe dependency
/// error rather than pretending that an upload succeeded.
pub(crate) struct UnavailableUploadBackend;

#[async_trait]
impl UploadBackend for UnavailableUploadBackend {
    fn max_chunk_size(&self) -> u64 {
        synveil_storage::UploadLimits::default().max_chunk_size
    }

    async fn create_upload_session(
        &self,
        _request: ApplicationCreateRequest,
    ) -> Result<UploadSessionView, UploadError> {
        Err(UploadError::DatabaseUnavailable)
    }

    async fn get_upload_session(
        &self,
        _owner_user_id: UserId,
        _session_id: UploadSessionId,
    ) -> Result<UploadSessionView, UploadError> {
        Err(UploadError::DatabaseUnavailable)
    }

    async fn append_upload_stream(
        &self,
        _owner_user_id: UserId,
        _session_id: UploadSessionId,
        _expected_offset: u64,
        _stream: UploadByteStream,
    ) -> Result<UploadProgress, UploadError> {
        Err(UploadError::DatabaseUnavailable)
    }

    async fn complete_upload(
        &self,
        _owner_user_id: UserId,
        _session_id: UploadSessionId,
    ) -> Result<UploadCompletion, UploadError> {
        Err(UploadError::DatabaseUnavailable)
    }

    async fn abort_upload(
        &self,
        _owner_user_id: UserId,
        _session_id: UploadSessionId,
    ) -> Result<UploadSessionView, UploadError> {
        Err(UploadError::DatabaseUnavailable)
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", deny_unknown_fields)]
pub(crate) enum CreateUploadSessionRequest {
    #[serde(rename = "CREATE_FILE")]
    CreateFile {
        library_id: String,
        parent_id: String,
        name: String,
        expected_bytes: String,
        expected_sha256: Option<String>,
    },
    #[serde(rename = "REPLACE_CONTENT")]
    ReplaceContent {
        library_id: String,
        node_id: String,
        expected_revision: String,
        expected_bytes: String,
        expected_sha256: Option<String>,
    },
}

#[derive(Debug, Serialize)]
struct ResourceResponse<T> {
    data: T,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
struct UploadSessionResource {
    id: String,
    #[serde(rename = "type")]
    resource_type: &'static str,
    attributes: UploadSessionAttributes,
}

#[derive(Debug, Serialize)]
struct UploadSessionAttributes {
    operation: &'static str,
    state: &'static str,
    received_bytes: String,
    expected_bytes: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_sha256: Option<String>,
    created_at: String,
    updated_at: String,
    expires_at: String,
    target: UploadTargetResource,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    terminal_failure_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completion: Option<UploadCompletionResource>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "operation")]
enum UploadTargetResource {
    #[serde(rename = "CREATE_FILE")]
    CreateFile {
        library_id: String,
        parent_id: String,
        node_id: String,
        name: String,
    },
    #[serde(rename = "REPLACE_CONTENT")]
    ReplaceContent {
        library_id: String,
        node_id: String,
        expected_revision: String,
    },
}

#[derive(Debug, Serialize)]
struct UploadCompletionResource {
    id: String,
    #[serde(rename = "type")]
    resource_type: &'static str,
    attributes: UploadCompletionAttributes,
}

#[derive(Debug, Serialize)]
struct UploadCompletionAttributes {
    node_id: String,
    file_version_id: String,
    object_id: String,
    node_revision: String,
    bytes: String,
    sha256: String,
    committed_at: String,
}

pub(crate) async fn create_upload_session(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Extension(context): Extension<RequestContext>,
    Json(payload): Json<CreateUploadSessionRequest>,
) -> Result<Response, ApiError> {
    let request = create_application_request(auth.principal().user_id(), payload)?;
    let session = state
        .upload_backend()
        .create_upload_session(request)
        .await
        .map_err(ApiError::from)?;
    Ok(session_response(
        StatusCode::CREATED,
        session,
        &context,
        true,
    ))
}

pub(crate) async fn get_upload_session(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Extension(context): Extension<RequestContext>,
    Path(session_id): Path<String>,
) -> Result<Response, ApiError> {
    let session_id = parse_id::<UploadSessionId>(&session_id)?;
    let session = state
        .upload_backend()
        .get_upload_session(auth.principal().user_id(), session_id)
        .await
        .map_err(ApiError::from)?;
    Ok(session_response(StatusCode::OK, session, &context, false))
}

pub(crate) async fn append_upload_chunk(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let session_id = parse_id::<UploadSessionId>(&session_id)?;
    let expected_offset = parse_upload_offset(&headers)?;
    require_octet_stream(&headers)?;
    validate_content_length(&headers, state.upload_backend().max_chunk_size())?;

    let stream = body
        .into_data_stream()
        .map(|frame| frame.map_err(|_| UploadError::ChunkTooLarge));
    let progress = state
        .upload_backend()
        .append_upload_stream(
            auth.principal().user_id(),
            session_id,
            expected_offset,
            boxed_upload_stream(stream),
        )
        .await
        .map_err(ApiError::from)?;

    let mut response = StatusCode::NO_CONTENT.into_response();
    set_upload_offset(response.headers_mut(), progress.received_bytes);
    set_no_store(response.headers_mut());
    Ok(response)
}

pub(crate) async fn complete_upload(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Extension(context): Extension<RequestContext>,
    Path(session_id): Path<String>,
) -> Result<Response, ApiError> {
    let session_id = parse_id::<UploadSessionId>(&session_id)?;
    let completion = state
        .upload_backend()
        .complete_upload(auth.principal().user_id(), session_id)
        .await
        .map_err(ApiError::from)?;
    let mut response = Json(ResourceResponse {
        data: completion_resource(&completion),
        meta: response_meta(&context),
    })
    .into_response();
    set_no_store(response.headers_mut());
    Ok(response)
}

pub(crate) async fn abort_upload(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Extension(context): Extension<RequestContext>,
    Path(session_id): Path<String>,
) -> Result<Response, ApiError> {
    let session_id = parse_id::<UploadSessionId>(&session_id)?;
    let session = state
        .upload_backend()
        .abort_upload(auth.principal().user_id(), session_id)
        .await
        .map_err(ApiError::from)?;
    Ok(session_response(StatusCode::OK, session, &context, false))
}

fn create_application_request(
    owner_user_id: UserId,
    payload: CreateUploadSessionRequest,
) -> Result<ApplicationCreateRequest, ApiError> {
    let (target, expected_bytes, expected_sha256) = match payload {
        CreateUploadSessionRequest::CreateFile {
            library_id,
            parent_id,
            name,
            expected_bytes,
            expected_sha256,
        } => (
            UploadTargetRequest::CreateFile {
                library_id: parse_id::<LibraryId>(&library_id)?,
                parent_node_id: parse_id::<NodeId>(&parent_id)?,
                name: LogicalName::new(name).map_err(|_| ApiError::InvalidRequest)?,
            },
            expected_bytes,
            expected_sha256,
        ),
        CreateUploadSessionRequest::ReplaceContent {
            library_id,
            node_id,
            expected_revision,
            expected_bytes,
            expected_sha256,
        } => (
            UploadTargetRequest::ReplaceContent {
                library_id: parse_id::<LibraryId>(&library_id)?,
                node_id: parse_id::<NodeId>(&node_id)?,
                expected_revision: Revision::from_str(&expected_revision)
                    .map_err(|_| ApiError::InvalidRequest)?,
            },
            expected_bytes,
            expected_sha256,
        ),
    };

    Ok(ApplicationCreateRequest {
        owner_user_id,
        target,
        expected_length: parse_decimal(&expected_bytes).map_err(|_| ApiError::InvalidRequest)?,
        expected_sha256: expected_sha256
            .as_deref()
            .map(Sha256Digest::from_str)
            .transpose()
            .map_err(|_| ApiError::InvalidRequest)?,
    })
}

fn parse_id<T>(value: &str) -> Result<T, ApiError>
where
    T: FromStr,
{
    value.parse().map_err(|_| ApiError::InvalidRequest)
}

fn parse_upload_offset(headers: &HeaderMap) -> Result<u64, ApiError> {
    let value = single_header(headers, UPLOAD_OFFSET_HEADER_NAME)
        .ok_or(ApiError::InvalidUploadOffset)?
        .to_str()
        .map_err(|_| ApiError::InvalidUploadOffset)?;
    parse_decimal(value).map_err(|_| ApiError::InvalidUploadOffset)
}

fn require_octet_stream(headers: &HeaderMap) -> Result<(), ApiError> {
    let value = single_header(headers, header::CONTENT_TYPE.as_str())
        .ok_or(ApiError::UnsupportedMediaType)?
        .to_str()
        .map_err(|_| ApiError::UnsupportedMediaType)?;
    if !value.eq_ignore_ascii_case("application/octet-stream") {
        return Err(ApiError::UnsupportedMediaType);
    }
    Ok(())
}

fn validate_content_length(headers: &HeaderMap, max_chunk_size: u64) -> Result<(), ApiError> {
    let values = headers
        .get_all(header::CONTENT_LENGTH)
        .iter()
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Ok(());
    }
    if values.len() != 1 {
        return Err(ApiError::InvalidRequest);
    }
    let value = values[0].to_str().map_err(|_| ApiError::InvalidRequest)?;
    let length = parse_decimal(value).map_err(|_| ApiError::InvalidRequest)?;
    if length == 0 {
        return Err(ApiError::Upload(UploadError::InvalidRequest));
    }
    if length > max_chunk_size {
        return Err(ApiError::PayloadTooLarge);
    }
    Ok(())
}

fn single_header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a HeaderValue> {
    let mut values = headers.get_all(name).iter();
    let first = values.next()?;
    values.next().is_none().then_some(first)
}

fn parse_decimal(value: &str) -> Result<u64, ()> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(());
    }
    value.parse().map_err(|_| ())
}

fn session_response(
    status: StatusCode,
    session: UploadSessionView,
    context: &RequestContext,
    include_location: bool,
) -> Response {
    let location = include_location.then(|| {
        format!(
            "{}/upload-sessions/{}",
            crate::API_VERSION_PREFIX,
            session.id
        )
    });
    let offset = session.received_bytes;
    let mut response = (
        status,
        Json(ResourceResponse {
            data: session_resource(session),
            meta: response_meta(context),
        }),
    )
        .into_response();
    set_upload_offset(response.headers_mut(), offset);
    if let Some(location) = location {
        response.headers_mut().insert(
            header::LOCATION,
            HeaderValue::from_str(&location).expect("generated upload location is header-safe"),
        );
    }
    set_no_store(response.headers_mut());
    response
}

fn session_resource(session: UploadSessionView) -> UploadSessionResource {
    let operation = match &session.target {
        UploadTargetView::CreateFile { .. } => "CREATE_FILE",
        UploadTargetView::ReplaceContent { .. } => "REPLACE_CONTENT",
    };
    let target = match session.target {
        UploadTargetView::CreateFile {
            library_id,
            parent_node_id,
            node_id,
            name,
        } => UploadTargetResource::CreateFile {
            library_id: library_id.to_string(),
            parent_id: parent_node_id.to_string(),
            node_id: node_id.to_string(),
            name: name.into_string(),
        },
        UploadTargetView::ReplaceContent {
            library_id,
            node_id,
            expected_revision,
        } => UploadTargetResource::ReplaceContent {
            library_id: library_id.to_string(),
            node_id: node_id.to_string(),
            expected_revision: expected_revision.to_string(),
        },
    };
    UploadSessionResource {
        id: session.id.to_string(),
        resource_type: "upload_session",
        attributes: UploadSessionAttributes {
            operation,
            state: session.state.as_str(),
            received_bytes: session.received_bytes.to_string(),
            expected_bytes: session.expected_length.to_string(),
            expected_sha256: session.expected_sha256.map(|digest| digest.to_string()),
            created_at: session.created_at.to_string(),
            updated_at: session.updated_at.to_string(),
            expires_at: session.expires_at.to_string(),
            target,
            last_error_code: safe_upload_error_code(session.last_error_code),
            terminal_failure_code: safe_upload_error_code(session.terminal_failure_code),
            completion: session.completion.as_ref().map(completion_resource),
        },
    }
}

fn completion_resource(completion: &UploadCompletion) -> UploadCompletionResource {
    UploadCompletionResource {
        id: completion.session_id.to_string(),
        resource_type: "upload_completion",
        attributes: UploadCompletionAttributes {
            node_id: completion.node_id.to_string(),
            file_version_id: completion.file_version_id.to_string(),
            object_id: completion.object_id.to_string(),
            node_revision: completion.node_revision.to_string(),
            bytes: completion.length.to_string(),
            sha256: completion.sha256.to_string(),
            committed_at: completion.committed_at.to_string(),
        },
    }
}

fn safe_upload_error_code(value: Option<String>) -> Option<String> {
    value.filter(|value| {
        matches!(
            value.as_str(),
            "upload_expired"
                | "invalid_offset"
                | "size_mismatch"
                | "hash_mismatch"
                | "version_conflict"
                | "object_corrupt"
                | "object_key_mismatch"
                | "concurrent_append"
                | "storage_unavailable"
                | "database_unavailable"
                | "capacity_unavailable"
                | "completion_conflict"
        )
    })
}

fn response_meta(context: &RequestContext) -> ResponseMeta {
    ResponseMeta {
        request_id: context.request_id().to_string(),
    }
}

fn set_upload_offset(headers: &mut HeaderMap, offset: u64) {
    headers.insert(
        UPLOAD_OFFSET_HEADER_NAME,
        HeaderValue::from_str(&offset.to_string())
            .expect("a decimal upload offset is valid HTTP header data"),
    );
}

fn set_no_store(headers: &mut HeaderMap) {
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, header};

    use super::{parse_decimal, parse_upload_offset, require_octet_stream};

    #[test]
    fn canonical_decimal_parser_rejects_signs_leading_zeroes_and_overflow() {
        assert_eq!(parse_decimal("0"), Ok(0));
        assert_eq!(parse_decimal(u64::MAX.to_string().as_str()), Ok(u64::MAX));
        for invalid in ["", "01", "+1", "-1", "1.0", "18446744073709551616"] {
            assert_eq!(parse_decimal(invalid), Err(()));
        }
    }

    #[test]
    fn offset_and_content_type_must_each_be_single_canonical_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("upload-offset", HeaderValue::from_static("42"));
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
        assert_eq!(parse_upload_offset(&headers), Ok(42));
        assert!(require_octet_stream(&headers).is_ok());

        headers.append("upload-offset", HeaderValue::from_static("42"));
        assert!(parse_upload_offset(&headers).is_err());
        headers.remove("upload-offset");
        headers.insert("upload-offset", HeaderValue::from_static("042"));
        assert!(parse_upload_offset(&headers).is_err());

        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream; charset=binary"),
        );
        assert!(require_octet_stream(&headers).is_err());
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
        headers.append(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
        assert!(require_octet_stream(&headers).is_err());
    }
}
