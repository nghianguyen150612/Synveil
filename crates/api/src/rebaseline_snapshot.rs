//! Authenticated transport for durable rebaseline snapshot artifacts.
//!
//! This module exposes three bounded operations over the Prompt 82
//! transport-neutral snapshot service: create a durable artifact, read its
//! immutable descriptor, and read its entries through stable bounded keyset
//! pages.
//!
//! The HTTP layer owns authentication, authorization context, path/query/body
//! parsing, DTO serialization, HTTP status mapping, and security/cache
//! headers. It does NOT own snapshot consistency, materialization, paging SQL,
//! expiry calculation, owner concealment rules, or journal semantics.

use std::str::FromStr;

use async_trait::async_trait;
use axum::{
    Json,
    body::Bytes,
    extract::{Extension, Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use synveil_core::{
    DeviceId, LibraryId, LogicalSnapshotNode, NodeId, RebaselineSnapshotId,
    RebaselineSnapshotPageCursor, UserId,
};
use synveil_metadata::{
    DatabasePool, DeviceSyncService, LogicalSnapshotService, RebaselineHandoffResult,
    RebaselineSnapshotDescriptor, RebaselineSnapshotPage, SnapshotError, SyncError,
};

use crate::{
    ApiError, ApiState, RequestContext,
    auth::{AuthenticatedPrincipal, ResponseMeta},
};

/// Wire version for the snapshot page cursor encoding.
const CURSOR_VERSION: u8 = 1;

/// Maximum encoded page cursor size. The canonical cursor encodes 97 JSON
/// bytes (194 hex characters); the bound leaves headroom for that shape while
/// remaining tightly bounded.
pub const MAX_SNAPSHOT_PAGE_CURSOR_BYTES: usize = 256;

/// Strictly bounded JSON body for the create endpoint. The create request is
/// intentionally empty; this keeps accidental large bodies out of the
/// snapshot-materialization path.
pub const SNAPSHOT_CREATE_BODY_LIMIT_BYTES: usize = 2 * 1024;

/// Application port for the durable rebaseline snapshot transport.
#[async_trait]
pub trait DurableSnapshotBackend: Send + Sync {
    async fn create_snapshot(
        &self,
        owner_user_id: UserId,
        library_id: LibraryId,
        observed_at: synveil_core::Timestamp,
    ) -> Result<RebaselineSnapshotDescriptor, SnapshotError>;

    async fn get_snapshot_descriptor(
        &self,
        owner_user_id: UserId,
        snapshot_id: RebaselineSnapshotId,
        observed_at: synveil_core::Timestamp,
    ) -> Result<RebaselineSnapshotDescriptor, SnapshotError>;

    #[allow(clippy::too_many_arguments)]
    async fn read_snapshot_page(
        &self,
        owner_user_id: UserId,
        snapshot_id: RebaselineSnapshotId,
        page_cursor: Option<RebaselineSnapshotPageCursor>,
        page_size: u32,
        observed_at: synveil_core::Timestamp,
    ) -> Result<RebaselineSnapshotPage, SnapshotError>;

    /// Complete the server-side checkpoint handoff from an immutable snapshot
    /// header. The default keeps alternate composition roots fail-closed while
    /// allowing the PostgreSQL adapter to opt into the Prompt 85 operation.
    async fn complete_rebaseline_handoff(
        &self,
        _owner_user_id: UserId,
        _device_id: DeviceId,
        _snapshot_id: RebaselineSnapshotId,
    ) -> Result<RebaselineHandoffResult, SyncError> {
        Err(SyncError::DependencyUnavailable)
    }
}

/// PostgreSQL adapter for the durable snapshot transport port.
pub struct PostgresDurableSnapshotBackend {
    service: LogicalSnapshotService,
}

impl PostgresDurableSnapshotBackend {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self {
            service: LogicalSnapshotService::new(pool),
        }
    }
}

#[async_trait]
impl DurableSnapshotBackend for PostgresDurableSnapshotBackend {
    async fn create_snapshot(
        &self,
        owner_user_id: UserId,
        library_id: LibraryId,
        observed_at: synveil_core::Timestamp,
    ) -> Result<RebaselineSnapshotDescriptor, SnapshotError> {
        self.service
            .create_rebaseline_snapshot(owner_user_id, library_id, observed_at)
            .await
    }

    async fn get_snapshot_descriptor(
        &self,
        owner_user_id: UserId,
        snapshot_id: RebaselineSnapshotId,
        observed_at: synveil_core::Timestamp,
    ) -> Result<RebaselineSnapshotDescriptor, SnapshotError> {
        self.service
            .get_rebaseline_snapshot(owner_user_id, snapshot_id, observed_at)
            .await
    }

    async fn read_snapshot_page(
        &self,
        owner_user_id: UserId,
        snapshot_id: RebaselineSnapshotId,
        page_cursor: Option<RebaselineSnapshotPageCursor>,
        page_size: u32,
        observed_at: synveil_core::Timestamp,
    ) -> Result<RebaselineSnapshotPage, SnapshotError> {
        self.service
            .read_rebaseline_snapshot_page(
                owner_user_id,
                snapshot_id,
                page_cursor,
                page_size,
                observed_at,
            )
            .await
    }

    async fn complete_rebaseline_handoff(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        snapshot_id: RebaselineSnapshotId,
    ) -> Result<RebaselineHandoffResult, SyncError> {
        DeviceSyncService::new(self.service.pool().clone())
            .complete_rebaseline_handoff(owner_user_id, device_id, snapshot_id)
            .await
    }
}

/// Fail-closed backend used by an unconfigured composition root.
pub(crate) struct UnavailableDurableSnapshotBackend;

#[async_trait]
impl DurableSnapshotBackend for UnavailableDurableSnapshotBackend {
    async fn create_snapshot(
        &self,
        _owner_user_id: UserId,
        _library_id: LibraryId,
        _observed_at: synveil_core::Timestamp,
    ) -> Result<RebaselineSnapshotDescriptor, SnapshotError> {
        Err(SnapshotError::DependencyUnavailable)
    }

    async fn get_snapshot_descriptor(
        &self,
        _owner_user_id: UserId,
        _snapshot_id: RebaselineSnapshotId,
        _observed_at: synveil_core::Timestamp,
    ) -> Result<RebaselineSnapshotDescriptor, SnapshotError> {
        Err(SnapshotError::DependencyUnavailable)
    }

    async fn read_snapshot_page(
        &self,
        _owner_user_id: UserId,
        _snapshot_id: RebaselineSnapshotId,
        _page_cursor: Option<RebaselineSnapshotPageCursor>,
        _page_size: u32,
        _observed_at: synveil_core::Timestamp,
    ) -> Result<RebaselineSnapshotPage, SnapshotError> {
        Err(SnapshotError::DependencyUnavailable)
    }
}

// ---------------------------------------------------------------------------
// Request / response DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateSnapshotRequest {}

/// The handoff request has no semantic input. An empty body is preferred, but
/// the canonical HTTP client may send `{}` when its JSON request builder needs
/// a content body. Denying all fields prevents a client from supplying an
/// owner, device, epoch, or sequence that could compete with server state.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HandoffRequest {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotPageQuery {
    cursor: Option<String>,
    limit: Option<u32>,
}

impl SnapshotPageQuery {
    fn limit(&self) -> u32 {
        self.limit
            .unwrap_or(synveil_metadata::DEFAULT_REBASELINE_SNAPSHOT_PAGE_SIZE)
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct SnapshotDescriptorResponse {
    data: SnapshotDescriptorData,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
struct SnapshotDescriptorData {
    snapshot_id: String,
    library_id: String,
    journal_boundary: JournalBoundaryData,
    entry_count: String,
    created_at: String,
    expires_at: String,
}

#[derive(Debug, Serialize)]
struct JournalBoundaryData {
    library_id: String,
    journal_epoch: String,
    resume_sequence: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct RebaselineHandoffResponse {
    data: RebaselineHandoffData,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
struct RebaselineHandoffData {
    snapshot_id: String,
    library_id: String,
    checkpoint: RebaselineHandoffCheckpointData,
}

#[derive(Debug, Serialize)]
struct RebaselineHandoffCheckpointData {
    epoch: String,
    sequence: String,
}

#[derive(Serialize)]
pub(crate) struct SnapshotPageResponse {
    data: SnapshotPageData,
    meta: ResponseMeta,
}

#[derive(Serialize)]
struct SnapshotPageData {
    snapshot_id: String,
    library_id: String,
    journal_boundary: JournalBoundaryData,
    entry_count: String,
    entries: Vec<SnapshotEntryData>,
    has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
struct SnapshotEntryData {
    node_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_node_id: Option<String>,
    name: String,
    kind: &'static str,
    state: &'static str,
    revision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_version_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_content: Option<SnapshotEntryContentData>,
}

#[derive(Debug, Serialize)]
struct SnapshotEntryContentData {
    byte_length: String,
    sha256: String,
}

// ---------------------------------------------------------------------------
// Page cursor wire encoding
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
struct SnapshotPageCursorWire {
    v: u8,
    sid: String,
    nid: String,
}

fn encode_page_cursor(cursor: &RebaselineSnapshotPageCursor) -> String {
    let wire = SnapshotPageCursorWire {
        v: CURSOR_VERSION,
        sid: cursor.snapshot_id().to_string(),
        nid: cursor.after_node_id().to_string(),
    };
    let json = serde_json::to_vec(&wire).expect("cursor serialization is infallible");
    hex_encode(&json)
}

fn decode_page_cursor(encoded: &str) -> Result<RebaselineSnapshotPageCursor, ApiError> {
    if encoded.len() > MAX_SNAPSHOT_PAGE_CURSOR_BYTES {
        return Err(ApiError::SnapshotInvalidCursor);
    }
    let bytes = hex_decode(encoded).map_err(|_| ApiError::SnapshotInvalidCursor)?;
    let wire: SnapshotPageCursorWire =
        serde_json::from_slice(&bytes).map_err(|_| ApiError::SnapshotInvalidCursor)?;
    if wire.v != CURSOR_VERSION {
        return Err(ApiError::SnapshotInvalidCursor);
    }
    let snapshot_id =
        RebaselineSnapshotId::from_str(&wire.sid).map_err(|_| ApiError::SnapshotInvalidCursor)?;
    let after_node_id = NodeId::from_str(&wire.nid).map_err(|_| ApiError::SnapshotInvalidCursor)?;
    Ok(RebaselineSnapshotPageCursor::new(
        snapshot_id,
        after_node_id,
    ))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(hex_digit(byte >> 4));
        value.push(hex_digit(byte & 0x0f));
    }
    value
}

fn hex_decode(value: &str) -> Result<Vec<u8>, ()> {
    if !value.len().is_multiple_of(2) {
        return Err(());
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks(2) {
        let high = decode_hex_nibble(pair[0])?;
        let low = decode_hex_nibble(pair[1])?;
        bytes.push((high << 4) | low);
    }
    Ok(bytes)
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'a' + value - 10) as char,
        _ => unreachable!("hex nibble is bounded"),
    }
}

const fn decode_hex_nibble(value: u8) -> Result<u8, ()> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(()),
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/v1/libraries/{library_id}/rebaseline-snapshots
pub(crate) async fn create_snapshot(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(library_id): Path<String>,
    Json(_payload): Json<CreateSnapshotRequest>,
) -> Result<Response, ApiError> {
    let library_id = LibraryId::from_str(&library_id).map_err(|_| ApiError::InvalidRequest)?;
    let observed_at = synveil_core::Timestamp::now();
    let descriptor = state
        .durable_snapshot_backend()
        .create_snapshot(auth.owner_user_id(), library_id, observed_at)
        .await
        .map_err(map_snapshot_error)?;
    let location = format!("/api/v1/rebaseline-snapshots/{}", descriptor.snapshot_id());
    tracing::info!(
        snapshot_id = %descriptor.snapshot_id(),
        library_id = %library_id,
        entry_count = descriptor.entry_count(),
        "durable rebaseline snapshot created"
    );
    let mut response = (
        StatusCode::CREATED,
        Json(SnapshotDescriptorResponse {
            data: descriptor_data(descriptor),
            meta: response_meta(&context),
        }),
    )
        .into_response();
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(&location).expect("Location header value is valid ASCII"),
    );
    Ok(private_no_store(response))
}

/// GET /api/v1/rebaseline-snapshots/{snapshot_id}
pub(crate) async fn get_descriptor(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(snapshot_id): Path<String>,
) -> Result<Response, ApiError> {
    let snapshot_id =
        RebaselineSnapshotId::from_str(&snapshot_id).map_err(|_| ApiError::InvalidRequest)?;
    let observed_at = synveil_core::Timestamp::now();
    let descriptor = state
        .durable_snapshot_backend()
        .get_snapshot_descriptor(auth.owner_user_id(), snapshot_id, observed_at)
        .await
        .map_err(map_snapshot_error)?;
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(SnapshotDescriptorResponse {
                data: descriptor_data(descriptor),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

/// GET /api/v1/rebaseline-snapshots/{snapshot_id}/entries
pub(crate) async fn read_entries(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(snapshot_id): Path<String>,
    Query(query): Query<SnapshotPageQuery>,
) -> Result<Response, ApiError> {
    let snapshot_id =
        RebaselineSnapshotId::from_str(&snapshot_id).map_err(|_| ApiError::InvalidRequest)?;
    let page_size = query.limit();
    if page_size == 0 || page_size > synveil_metadata::MAX_REBASELINE_SNAPSHOT_PAGE_SIZE {
        return Err(ApiError::SnapshotInvalidPageSize);
    }
    let page_cursor = match &query.cursor {
        Some(encoded) => {
            let cursor = decode_page_cursor(encoded)?;
            if cursor.snapshot_id() != snapshot_id {
                return Err(ApiError::SnapshotInvalidCursor);
            }
            Some(cursor)
        }
        None => None,
    };
    let observed_at = synveil_core::Timestamp::now();
    let page = state
        .durable_snapshot_backend()
        .read_snapshot_page(
            auth.owner_user_id(),
            snapshot_id,
            page_cursor,
            page_size,
            observed_at,
        )
        .await
        .map_err(map_snapshot_error)?;
    let next_cursor = page.next_cursor().map(|c| encode_page_cursor(&c));
    let descriptor = page.descriptor();
    let entries: Vec<SnapshotEntryData> = page.entries().iter().map(snapshot_entry_data).collect();
    tracing::info!(
        snapshot_id = %snapshot_id,
        library_id = %descriptor.library_id(),
        page_count = entries.len(),
        has_more = page.has_more(),
        "durable rebaseline snapshot page delivered"
    );
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(SnapshotPageResponse {
                data: SnapshotPageData {
                    snapshot_id: descriptor.snapshot_id().to_string(),
                    library_id: descriptor.library_id().to_string(),
                    journal_boundary: journal_boundary_data(descriptor),
                    entry_count: descriptor.entry_count().to_string(),
                    entries,
                    has_more: page.has_more(),
                    next_cursor,
                },
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

/// POST /api/v1/rebaseline-snapshots/{snapshot_id}/handoff
///
/// This route is deliberately device-credential-only. A browser session has
/// an owner but no authenticated device identity, and the request has no
/// device path parameter to manufacture one. The canonical sync device bearer
/// therefore supplies both owner and device authority without allowing the
/// caller to select another device's checkpoint.
pub(crate) async fn complete_handoff(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(snapshot_id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let snapshot_id =
        RebaselineSnapshotId::from_str(&snapshot_id).map_err(|_| ApiError::InvalidRequest)?;
    if !body.is_empty() {
        serde_json::from_slice::<HandoffRequest>(&body).map_err(|_| ApiError::InvalidRequest)?;
    }
    let device_id = match auth {
        AuthenticatedPrincipal::DeviceCredential { device_id, .. } => device_id,
        AuthenticatedPrincipal::BrowserSession { .. } => return Err(ApiError::Unauthorized),
    };
    let result = state
        .durable_snapshot_backend()
        .complete_rebaseline_handoff(auth.owner_user_id(), device_id, snapshot_id)
        .await
        .map_err(map_handoff_error)?;
    let checkpoint = result.installed_checkpoint();
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(RebaselineHandoffResponse {
                data: RebaselineHandoffData {
                    snapshot_id: result.snapshot_id().to_string(),
                    library_id: result.library_id().to_string(),
                    checkpoint: RebaselineHandoffCheckpointData {
                        epoch: checkpoint.journal_epoch().to_string(),
                        sequence: checkpoint.acknowledged_sequence().to_string(),
                    },
                },
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn descriptor_data(descriptor: RebaselineSnapshotDescriptor) -> SnapshotDescriptorData {
    SnapshotDescriptorData {
        snapshot_id: descriptor.snapshot_id().to_string(),
        library_id: descriptor.library_id().to_string(),
        journal_boundary: journal_boundary_data(descriptor),
        entry_count: descriptor.entry_count().to_string(),
        created_at: descriptor.created_at().to_string(),
        expires_at: descriptor.expires_at().to_string(),
    }
}

fn journal_boundary_data(descriptor: RebaselineSnapshotDescriptor) -> JournalBoundaryData {
    let boundary = descriptor.boundary();
    JournalBoundaryData {
        library_id: boundary.library_id().to_string(),
        journal_epoch: boundary.journal_epoch().to_string(),
        resume_sequence: boundary.sequence().to_string(),
    }
}

fn snapshot_entry_data(node: &LogicalSnapshotNode) -> SnapshotEntryData {
    let current_content = match (node.content_length(), node.content_sha256()) {
        (Some(byte_length), Some(sha256)) => Some(SnapshotEntryContentData {
            byte_length: byte_length.to_string(),
            sha256: sha256.to_string(),
        }),
        _ => None,
    };
    SnapshotEntryData {
        node_id: node.node_id().to_string(),
        parent_node_id: node.parent_node_id().map(|value| value.to_string()),
        name: node.name().as_str().to_owned(),
        kind: node.kind().as_str(),
        state: node.state().as_str(),
        revision: node.revision().to_string(),
        current_version_id: node.current_version_id().map(|value| value.to_string()),
        current_content,
    }
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

fn map_snapshot_error(error: SnapshotError) -> ApiError {
    match error {
        SnapshotError::NotFound => ApiError::Core(synveil_core::ErrorCode::NotFound),
        SnapshotError::InvalidPageSize => ApiError::SnapshotInvalidPageSize,
        SnapshotError::InvalidPageCursor => ApiError::SnapshotInvalidCursor,
        SnapshotError::Expired => ApiError::SnapshotExpired,
        SnapshotError::ActiveArtifactLimitReached => {
            ApiError::Core(synveil_core::ErrorCode::RateLimited)
        }
        SnapshotError::InvalidObservedTime => ApiError::Internal,
        SnapshotError::DependencyUnavailable | SnapshotError::Database(_) => {
            ApiError::ReadinessUnavailable
        }
        SnapshotError::InvalidPersistedData => ApiError::Internal,
        SnapshotError::InvalidSnapshot(_) => ApiError::Internal,
    }
}

fn map_handoff_error(error: SyncError) -> ApiError {
    match error {
        SyncError::NotFound => ApiError::Core(synveil_core::ErrorCode::NotFound),
        SyncError::CheckpointConflict
        | SyncError::CheckpointAheadOfSnapshot
        | SyncError::CheckpointEpochConflict => ApiError::SyncCheckpointConflict,
        SyncError::DependencyUnavailable | SyncError::Database(_) => ApiError::ReadinessUnavailable,
        SyncError::InvalidPersistedData | SyncError::InternalError => ApiError::Internal,
        SyncError::InvalidLimit
        | SyncError::InvalidAckToken
        | SyncError::RebaselineRequired { .. } => ApiError::Internal,
    }
}
