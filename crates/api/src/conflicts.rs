//! Authenticated, device-scoped durable conflict inspection and explicit
//! manual resolution transport.

use std::{fmt, str::FromStr};

use async_trait::async_trait;
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use synveil_core::{
    ClientMutation, ConflictResolutionAction, ConflictResolutionId, ConflictResolutionRequest,
    DeviceId, LibraryId, Revision, SyncConflictId, Timestamp, UserId,
};
use synveil_metadata::{
    ConflictManagementBackend, ConflictManagementError, ConflictManagementService, ConflictPage,
    ConflictPagePosition, ConflictResolutionResult, DEFAULT_CONFLICT_PAGE_LIMIT,
    MAX_CONFLICT_PAGE_LIMIT, MutationConflict, RebaselineReason, SyncConflictRecord,
};

use crate::{
    ApiError, ApiState, RequestContext,
    auth::{AuthContext, ResponseMeta},
};

type HmacSha256 = Hmac<Sha256>;

const CURSOR_VERSION: &str = "v1";
const CURSOR_KIND: &str = "conflict-open";
const CURSOR_PARTS: usize = 8;
const CURSOR_DOMAIN: &[u8] = b"synveil/conflict-open-cursor/v1\0";
const CURSOR_TAG_BYTES: usize = 32;
const MAX_TIMESTAMP_TEXT_BYTES: usize = 64;
pub const MAX_CONFLICT_CURSOR_BYTES: usize = 384;
pub const CONFLICT_RESOLUTION_BODY_LIMIT_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictCursorError {
    Malformed,
    Oversized,
    InvalidSignature,
}

#[derive(Clone)]
pub struct ConflictCursorKey([u8; 32]);

impl ConflictCursorKey {
    #[must_use]
    pub fn generate() -> Self {
        let token = synveil_auth::SessionToken::generate();
        Self(*token.as_bytes())
    }

    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub fn issue(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        position: ConflictPagePosition,
    ) -> String {
        let created_at = position.created_at().to_string();
        let tag = self.sign(
            owner_user_id,
            device_id,
            library_id,
            position.created_at(),
            position.conflict_id(),
        );
        let cursor = format!(
            "{CURSOR_VERSION}.{CURSOR_KIND}.{owner_user_id}.{device_id}.{library_id}.{}.{}.{}",
            encode_hex(created_at.as_bytes()),
            position.conflict_id(),
            encode_hex(&tag),
        );
        debug_assert!(cursor.len() <= MAX_CONFLICT_CURSOR_BYTES);
        cursor
    }

    pub fn verify(&self, candidate: &str) -> Result<ConflictCursorClaims, ConflictCursorError> {
        if candidate.len() > MAX_CONFLICT_CURSOR_BYTES {
            return Err(ConflictCursorError::Oversized);
        }
        let parts = candidate.split('.').collect::<Vec<_>>();
        if parts.len() != CURSOR_PARTS || parts[0] != CURSOR_VERSION || parts[1] != CURSOR_KIND {
            return Err(ConflictCursorError::Malformed);
        }
        let owner_user_id = parse_cursor_id(parts[2])?;
        let device_id = parse_cursor_id(parts[3])?;
        let library_id = parse_cursor_id(parts[4])?;
        let timestamp_bytes = decode_hex_bounded(parts[5], MAX_TIMESTAMP_TEXT_BYTES)
            .ok_or(ConflictCursorError::Malformed)?;
        let timestamp_text =
            std::str::from_utf8(&timestamp_bytes).map_err(|_| ConflictCursorError::Malformed)?;
        let created_at =
            Timestamp::parse(timestamp_text).map_err(|_| ConflictCursorError::Malformed)?;
        if created_at.to_string() != timestamp_text {
            return Err(ConflictCursorError::Malformed);
        }
        let conflict_id = parse_cursor_id(parts[6])?;
        let tag =
            decode_hex_exact(parts[7], CURSOR_TAG_BYTES).ok_or(ConflictCursorError::Malformed)?;
        let expected = self.sign(
            owner_user_id,
            device_id,
            library_id,
            created_at,
            conflict_id,
        );
        if expected.as_slice().ct_eq(&tag).unwrap_u8() != 1 {
            return Err(ConflictCursorError::InvalidSignature);
        }
        Ok(ConflictCursorClaims {
            owner_user_id,
            device_id,
            library_id,
            position: ConflictPagePosition::new(created_at, conflict_id),
        })
    }

    fn sign(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        created_at: Timestamp,
        conflict_id: SyncConflictId,
    ) -> [u8; CURSOR_TAG_BYTES] {
        let mut mac = HmacSha256::new_from_slice(&self.0).expect("HMAC accepts a 256-bit key");
        mac.update(CURSOR_DOMAIN);
        mac.update(owner_user_id.as_bytes());
        mac.update(device_id.as_bytes());
        mac.update(library_id.as_bytes());
        mac.update(created_at.to_string().as_bytes());
        mac.update(conflict_id.as_bytes());
        let bytes = mac.finalize().into_bytes();
        let mut tag = [0_u8; CURSOR_TAG_BYTES];
        tag.copy_from_slice(&bytes);
        tag
    }
}

impl Default for ConflictCursorKey {
    fn default() -> Self {
        Self::generate()
    }
}

impl fmt::Debug for ConflictCursorKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ConflictCursorKey([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConflictCursorClaims {
    owner_user_id: UserId,
    device_id: DeviceId,
    library_id: LibraryId,
    position: ConflictPagePosition,
}

impl ConflictCursorClaims {
    #[must_use]
    pub const fn owner_user_id(self) -> UserId {
        self.owner_user_id
    }

    #[must_use]
    pub const fn device_id(self) -> DeviceId {
        self.device_id
    }

    #[must_use]
    pub const fn library_id(self) -> LibraryId {
        self.library_id
    }

    #[must_use]
    pub const fn position(self) -> ConflictPagePosition {
        self.position
    }
}

pub struct PostgresConflictManagementBackend {
    service: ConflictManagementService,
}

impl PostgresConflictManagementBackend {
    #[must_use]
    pub fn new(pool: synveil_metadata::DatabasePool) -> Self {
        Self {
            service: ConflictManagementService::new(pool),
        }
    }
}

#[async_trait]
impl ConflictManagementBackend for PostgresConflictManagementBackend {
    async fn list_open(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        position: Option<ConflictPagePosition>,
        limit: u32,
    ) -> Result<ConflictPage, ConflictManagementError> {
        self.service
            .list_open(owner_user_id, device_id, library_id, position, limit)
            .await
    }

    async fn detail(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        conflict_id: SyncConflictId,
    ) -> Result<SyncConflictRecord, ConflictManagementError> {
        self.service
            .detail(owner_user_id, device_id, library_id, conflict_id)
            .await
    }

    async fn resolve(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        conflict_id: SyncConflictId,
        request: ConflictResolutionRequest,
    ) -> Result<ConflictResolutionResult, ConflictManagementError> {
        self.service
            .resolve(owner_user_id, device_id, library_id, conflict_id, request)
            .await
    }
}

pub(crate) struct UnavailableConflictManagementBackend;

#[async_trait]
impl ConflictManagementBackend for UnavailableConflictManagementBackend {
    async fn list_open(
        &self,
        _owner_user_id: UserId,
        _device_id: DeviceId,
        _library_id: LibraryId,
        _position: Option<ConflictPagePosition>,
        _limit: u32,
    ) -> Result<ConflictPage, ConflictManagementError> {
        Err(ConflictManagementError::DependencyUnavailable)
    }

    async fn detail(
        &self,
        _owner_user_id: UserId,
        _device_id: DeviceId,
        _library_id: LibraryId,
        _conflict_id: SyncConflictId,
    ) -> Result<SyncConflictRecord, ConflictManagementError> {
        Err(ConflictManagementError::DependencyUnavailable)
    }

    async fn resolve(
        &self,
        _owner_user_id: UserId,
        _device_id: DeviceId,
        _library_id: LibraryId,
        _conflict_id: SyncConflictId,
        _request: ConflictResolutionRequest,
    ) -> Result<ConflictResolutionResult, ConflictManagementError> {
        Err(ConflictManagementError::DependencyUnavailable)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConflictListQuery {
    limit: Option<String>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResolveConflictRequest {
    resolution_id: String,
    action: String,
    expected_current_revision: Option<String>,
    expected_current_parent_revision: Option<String>,
}

#[derive(Debug, Serialize)]
struct ConflictListResponse {
    data: ConflictListData,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
struct ConflictListData {
    conflicts: Vec<ConflictSummaryData>,
    page: ConflictPageData,
}

#[derive(Debug, Serialize)]
struct ConflictPageData {
    has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
struct ConflictSummaryData {
    conflict_id: String,
    original_client_mutation_id: String,
    mutation_kind: &'static str,
    resource_id: String,
    reason: &'static str,
    lifecycle: &'static str,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct ConflictDetailResponse {
    data: ConflictDetailData,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
struct ConflictDetailData {
    conflict_id: String,
    original_client_mutation_id: String,
    mutation_kind: &'static str,
    resource_id: String,
    reason: &'static str,
    lifecycle: &'static str,
    created_at: String,
    original_intent: OriginalIntentData,
    historical_server_observation: HistoricalServerObservationData,
    #[serde(skip_serializing_if = "Option::is_none")]
    terminal_resolution: Option<TerminalResolutionData>,
}

#[derive(Debug, Serialize)]
struct OriginalIntentData {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    requested_parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_parent_revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    requested_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct HistoricalServerObservationData {
    resource_id: String,
    reason: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    original_expected_revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    server_revision_at_conflict: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    server_state_at_conflict: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    server_parent_id_at_conflict: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    server_name_at_conflict: Option<String>,
    server_epoch_at_conflict: String,
    server_sequence_at_conflict: String,
}

#[derive(Debug, Serialize)]
struct TerminalResolutionData {
    resolution_id: String,
    action: &'static str,
    completed_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    journal_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    journal_sequence: Option<String>,
}

#[derive(Debug, Serialize)]
struct ResolutionResponse {
    data: ResolutionData,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
struct ResolutionData {
    outcome: &'static str,
    conflict_id: String,
    resolution_id: String,
    action: &'static str,
    lifecycle: &'static str,
    replayed: bool,
    completed_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    journal_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    journal_sequence: Option<String>,
}

pub(crate) async fn list(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Extension(context): Extension<RequestContext>,
    Path((device_id, library_id)): Path<(String, String)>,
    Query(query): Query<ConflictListQuery>,
) -> Result<Response, ApiError> {
    let device_id = parse_path_id::<DeviceId>(&device_id)?;
    let library_id = parse_path_id::<LibraryId>(&library_id)?;
    let owner_user_id = auth.principal().user_id();
    let limit = parse_limit(query.limit.as_deref())?;
    let position = query
        .cursor
        .as_deref()
        .map(|cursor| state.conflict_cursor_key().verify(cursor))
        .transpose()
        .map_err(|_| ApiError::Core(synveil_core::ErrorCode::InvalidCursor))?
        .map(|claims| {
            if claims.owner_user_id() != owner_user_id
                || claims.device_id() != device_id
                || claims.library_id() != library_id
            {
                return Err(ApiError::Core(synveil_core::ErrorCode::InvalidCursor));
            }
            Ok(claims.position())
        })
        .transpose()?;
    let page = state
        .conflict_management_backend()
        .list_open(owner_user_id, device_id, library_id, position, limit)
        .await
        .map_err(map_conflict_error)?;
    let next_cursor = page.next_position().map(|position| {
        state
            .conflict_cursor_key()
            .issue(owner_user_id, device_id, library_id, position)
    });
    let conflicts = page.conflicts().iter().map(conflict_summary_data).collect();
    let mut response = (
        StatusCode::OK,
        Json(ConflictListResponse {
            data: ConflictListData {
                conflicts,
                page: ConflictPageData {
                    has_more: page.has_more(),
                    next_cursor,
                },
            },
            meta: response_meta(&context),
        }),
    )
        .into_response();
    no_store(&mut response);
    Ok(response)
}

pub(crate) async fn detail(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Extension(context): Extension<RequestContext>,
    Path((device_id, library_id, conflict_id)): Path<(String, String, String)>,
) -> Result<Response, ApiError> {
    let device_id = parse_path_id::<DeviceId>(&device_id)?;
    let library_id = parse_path_id::<LibraryId>(&library_id)?;
    let conflict_id = parse_path_id::<SyncConflictId>(&conflict_id)?;
    let conflict = state
        .conflict_management_backend()
        .detail(
            auth.principal().user_id(),
            device_id,
            library_id,
            conflict_id,
        )
        .await
        .map_err(map_conflict_error)?;
    let mut response = (
        StatusCode::OK,
        Json(ConflictDetailResponse {
            data: conflict_detail_data(&conflict),
            meta: response_meta(&context),
        }),
    )
        .into_response();
    no_store(&mut response);
    Ok(response)
}

pub(crate) async fn resolve(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Extension(context): Extension<RequestContext>,
    Path((device_id, library_id, conflict_id)): Path<(String, String, String)>,
    Json(payload): Json<ResolveConflictRequest>,
) -> Result<Response, ApiError> {
    let device_id = parse_path_id::<DeviceId>(&device_id)?;
    let library_id = parse_path_id::<LibraryId>(&library_id)?;
    let conflict_id = parse_path_id::<SyncConflictId>(&conflict_id)?;
    let request = parse_resolution_request(payload)?;
    let resolution_id = request.resolution_id();
    let action = request.action();
    let result = state
        .conflict_management_backend()
        .resolve(
            auth.principal().user_id(),
            device_id,
            library_id,
            conflict_id,
            request,
        )
        .await
        .map_err(map_conflict_error)?;

    let data = match result {
        ConflictResolutionResult::AcceptedServer {
            completed_at,
            replayed,
            ..
        } => ResolutionData {
            outcome: "ACCEPTED_SERVER",
            conflict_id: conflict_id.to_string(),
            resolution_id: resolution_id.to_string(),
            action: action.as_str(),
            lifecycle: "DISMISSED",
            replayed,
            completed_at: completed_at.to_string(),
            journal_event_id: None,
            journal_sequence: None,
        },
        ConflictResolutionResult::AppliedClientIntent {
            journal_event_id,
            journal_sequence,
            completed_at,
            replayed,
            ..
        } => ResolutionData {
            outcome: "APPLIED_CLIENT_INTENT",
            conflict_id: conflict_id.to_string(),
            resolution_id: resolution_id.to_string(),
            action: action.as_str(),
            lifecycle: "RESOLVED",
            replayed,
            completed_at: completed_at.to_string(),
            journal_event_id: Some(journal_event_id.to_string()),
            journal_sequence: Some(journal_sequence.to_string()),
        },
    };
    tracing::info!(
        device_id = %device_id,
        library_id = %library_id,
        conflict_id = %conflict_id,
        resolution_id = %resolution_id,
        resolution_action = action.as_str(),
        resolution_outcome = data.outcome,
        replayed = data.replayed,
        journal_sequence = data.journal_sequence.as_deref().unwrap_or("none"),
        "manual conflict resolution completed"
    );
    let mut response = (
        StatusCode::OK,
        Json(ResolutionResponse {
            data,
            meta: response_meta(&context),
        }),
    )
        .into_response();
    no_store(&mut response);
    Ok(response)
}

fn parse_resolution_request(
    payload: ResolveConflictRequest,
) -> Result<ConflictResolutionRequest, ApiError> {
    let resolution_id = ConflictResolutionId::from_str(&payload.resolution_id)
        .map_err(|_| ApiError::InvalidConflictResolution)?;
    let action = ConflictResolutionAction::from_str(&payload.action)
        .map_err(|_| ApiError::InvalidConflictResolution)?;
    let expected_current_revision = payload
        .expected_current_revision
        .as_deref()
        .map(Revision::from_str)
        .transpose()
        .map_err(|_| ApiError::InvalidConflictResolution)?;
    let expected_current_parent_revision = payload
        .expected_current_parent_revision
        .as_deref()
        .map(Revision::from_str)
        .transpose()
        .map_err(|_| ApiError::InvalidConflictResolution)?;
    Ok(ConflictResolutionRequest::new(
        resolution_id,
        action,
        expected_current_revision,
        expected_current_parent_revision,
    ))
}

fn parse_limit(value: Option<&str>) -> Result<u32, ApiError> {
    let Some(value) = value else {
        return Ok(DEFAULT_CONFLICT_PAGE_LIMIT);
    };
    let limit = value
        .parse::<u32>()
        .map_err(|_| ApiError::SyncInvalidLimit)?;
    if limit.to_string() != value || !(1..=MAX_CONFLICT_PAGE_LIMIT).contains(&limit) {
        return Err(ApiError::SyncInvalidLimit);
    }
    Ok(limit)
}

fn parse_path_id<T>(value: &str) -> Result<T, ApiError>
where
    T: FromStr,
{
    value
        .parse()
        .map_err(|_| ApiError::Core(synveil_core::ErrorCode::NotFound))
}

fn conflict_summary_data(conflict: &SyncConflictRecord) -> ConflictSummaryData {
    ConflictSummaryData {
        conflict_id: conflict.conflict_id().to_string(),
        original_client_mutation_id: conflict.original_client_mutation_id().to_string(),
        mutation_kind: conflict.mutation_kind().as_str(),
        resource_id: conflict.resource_id().to_string(),
        reason: conflict.historical_observation().reason().as_str(),
        lifecycle: conflict.lifecycle().as_str(),
        created_at: conflict.created_at().to_string(),
    }
}

fn conflict_detail_data(conflict: &SyncConflictRecord) -> ConflictDetailData {
    let observation = conflict.historical_observation();
    ConflictDetailData {
        conflict_id: conflict.conflict_id().to_string(),
        original_client_mutation_id: conflict.original_client_mutation_id().to_string(),
        mutation_kind: conflict.mutation_kind().as_str(),
        resource_id: conflict.resource_id().to_string(),
        reason: observation.reason().as_str(),
        lifecycle: conflict.lifecycle().as_str(),
        created_at: conflict.created_at().to_string(),
        original_intent: original_intent_data(conflict.original_intent()),
        historical_server_observation: historical_observation_data(observation),
        terminal_resolution: conflict.terminal_resolution().map(|resolution| {
            TerminalResolutionData {
                resolution_id: resolution.resolution_id().to_string(),
                action: resolution.action().as_str(),
                completed_at: resolution.completed_at().to_string(),
                journal_event_id: resolution.journal_event_id().map(|id| id.to_string()),
                journal_sequence: resolution
                    .journal_sequence()
                    .map(|sequence| sequence.to_string()),
            }
        }),
    }
}

fn original_intent_data(intent: &ClientMutation) -> OriginalIntentData {
    match intent {
        ClientMutation::CreateDirectory {
            parent_node_id,
            expected_parent_revision,
            name,
        } => OriginalIntentData {
            kind: intent.kind().as_str(),
            node_id: None,
            parent_node_id: Some(parent_node_id.to_string()),
            requested_parent_id: None,
            expected_revision: None,
            expected_parent_revision: Some(expected_parent_revision.to_string()),
            requested_name: Some(name.as_str().to_owned()),
        },
        ClientMutation::RenameNode {
            node_id,
            expected_revision,
            new_name,
        } => OriginalIntentData {
            kind: intent.kind().as_str(),
            node_id: Some(node_id.to_string()),
            parent_node_id: None,
            requested_parent_id: None,
            expected_revision: Some(expected_revision.to_string()),
            expected_parent_revision: None,
            requested_name: Some(new_name.as_str().to_owned()),
        },
        ClientMutation::MoveNode {
            node_id,
            expected_revision,
            new_parent_node_id,
            expected_new_parent_revision,
        } => OriginalIntentData {
            kind: intent.kind().as_str(),
            node_id: Some(node_id.to_string()),
            parent_node_id: None,
            requested_parent_id: Some(new_parent_node_id.to_string()),
            expected_revision: Some(expected_revision.to_string()),
            expected_parent_revision: Some(expected_new_parent_revision.to_string()),
            requested_name: None,
        },
        ClientMutation::TrashNode {
            node_id,
            expected_revision,
        } => OriginalIntentData {
            kind: intent.kind().as_str(),
            node_id: Some(node_id.to_string()),
            parent_node_id: None,
            requested_parent_id: None,
            expected_revision: Some(expected_revision.to_string()),
            expected_parent_revision: None,
            requested_name: None,
        },
        ClientMutation::RestoreNode {
            node_id,
            expected_revision,
            expected_parent_node_id,
            expected_parent_revision,
        } => OriginalIntentData {
            kind: intent.kind().as_str(),
            node_id: Some(node_id.to_string()),
            parent_node_id: Some(expected_parent_node_id.to_string()),
            requested_parent_id: None,
            expected_revision: Some(expected_revision.to_string()),
            expected_parent_revision: Some(expected_parent_revision.to_string()),
            requested_name: None,
        },
    }
}

fn historical_observation_data(observation: &MutationConflict) -> HistoricalServerObservationData {
    HistoricalServerObservationData {
        resource_id: observation.resource_id().to_string(),
        reason: observation.reason().as_str(),
        original_expected_revision: observation
            .expected_revision()
            .map(|revision| revision.to_string()),
        server_revision_at_conflict: observation
            .current_revision()
            .map(|revision| revision.to_string()),
        server_state_at_conflict: observation.current_state().map(|state| state.as_str()),
        server_parent_id_at_conflict: observation.current_parent_id().map(|id| id.to_string()),
        server_name_at_conflict: observation
            .current_name()
            .map(|name| name.as_str().to_owned()),
        server_epoch_at_conflict: observation.server_epoch().to_string(),
        server_sequence_at_conflict: observation.server_sequence().to_string(),
    }
}

fn map_conflict_error(error: ConflictManagementError) -> ApiError {
    match error {
        ConflictManagementError::NotFound => ApiError::Core(synveil_core::ErrorCode::NotFound),
        ConflictManagementError::InvalidListRequest => ApiError::InvalidRequest,
        ConflictManagementError::InvalidConflictResolution => ApiError::InvalidConflictResolution,
        ConflictManagementError::ResolutionIdConflict => ApiError::ResolutionIdConflict,
        ConflictManagementError::ConflictNotOpen { lifecycle } => {
            ApiError::ConflictNotOpen { lifecycle }
        }
        ConflictManagementError::ResolutionConflict {
            resolution_id,
            conflict,
            completed_at,
            replayed,
        } => ApiError::ResolutionConflict {
            resolution_id,
            conflict,
            completed_at,
            replayed,
        },
        ConflictManagementError::RebaselineRequired {
            reason,
            current_epoch,
            minimum_retained_sequence,
        } => ApiError::SyncRebaselineRequired {
            reason: match reason {
                RebaselineReason::EpochMismatch => "epoch_mismatch",
                RebaselineReason::HistoryUnavailable => "history_unavailable",
            },
            current_epoch: current_epoch.to_string(),
            minimum_retained_sequence: minimum_retained_sequence.to_string(),
        },
        ConflictManagementError::DependencyUnavailable | ConflictManagementError::Database(_) => {
            ApiError::ConflictDependencyUnavailable
        }
        ConflictManagementError::InvalidPersistedData => ApiError::ConflictInvalidPersistedData,
        ConflictManagementError::InternalError => ApiError::Internal,
    }
}

fn response_meta(context: &RequestContext) -> ResponseMeta {
    ResponseMeta {
        request_id: context.request_id().to_string(),
    }
}

fn no_store(response: &mut Response) {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
}

fn parse_cursor_id<T>(value: &str) -> Result<T, ConflictCursorError>
where
    T: FromStr,
{
    value.parse().map_err(|_| ConflictCursorError::Malformed)
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(hex_digit(byte >> 4));
        encoded.push(hex_digit(byte & 0x0f));
    }
    encoded
}

fn decode_hex_exact(value: &str, expected_bytes: usize) -> Option<Vec<u8>> {
    if value.len() != expected_bytes.checked_mul(2)? {
        return None;
    }
    decode_hex_bounded(value, expected_bytes)
}

fn decode_hex_bounded(value: &str, max_bytes: usize) -> Option<Vec<u8>> {
    if value.is_empty()
        || !value.len().is_multiple_of(2)
        || value.len() > max_bytes.checked_mul(2)?
    {
        return None;
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
    debug_assert!(remainder.is_empty());
    for pair in pairs {
        let high = decode_hex_nibble(pair[0])?;
        let low = decode_hex_nibble(pair[1])?;
        bytes.push((high << 4) | low);
    }
    Some(bytes)
}

fn decode_hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'a' + value - 10) as char,
        _ => unreachable!("hex nibble is bounded"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConflictCursorKey, MAX_CONFLICT_CURSOR_BYTES, ResolveConflictRequest, map_conflict_error,
        parse_resolution_request,
    };
    use axum::http::StatusCode;
    use synveil_core::{
        ConflictResolutionAction, ConflictResolutionId, DeviceId, LibraryId, Revision, Sequence,
        SyncConflictId, Timestamp, UserId,
    };
    use synveil_metadata::{ConflictManagementError, RebaselineReason};

    #[test]
    fn signed_conflict_cursor_is_scope_bound_bounded_and_tamper_safe() {
        let key = ConflictCursorKey::from_bytes([0x35; 32]);
        let owner = UserId::new();
        let device = DeviceId::new();
        let library = LibraryId::new();
        let position = synveil_metadata::ConflictPagePosition::new(
            Timestamp::parse("2026-08-27T12:34:56.123456Z").expect("valid timestamp"),
            SyncConflictId::new(),
        );
        let cursor = key.issue(owner, device, library, position);
        assert!(cursor.len() <= MAX_CONFLICT_CURSOR_BYTES);
        let claims = key.verify(&cursor).expect("cursor must verify");
        assert_eq!(claims.owner_user_id(), owner);
        assert_eq!(claims.device_id(), device);
        assert_eq!(claims.library_id(), library);
        assert_eq!(claims.position(), position);

        let mut tampered = cursor.into_bytes();
        let last = tampered.last_mut().expect("cursor is nonempty");
        *last = if *last == b'a' { b'b' } else { b'a' };
        assert!(key.verify(std::str::from_utf8(&tampered).unwrap()).is_err());
        assert!(
            key.verify(&"a".repeat(MAX_CONFLICT_CURSOR_BYTES + 1))
                .is_err()
        );
    }

    #[test]
    fn strict_resolution_dto_maps_only_typed_fields() {
        let resolution_id = ConflictResolutionId::new();
        let request = parse_resolution_request(ResolveConflictRequest {
            resolution_id: resolution_id.to_string(),
            action: "APPLY_CLIENT_INTENT".to_owned(),
            expected_current_revision: Some("7".to_owned()),
            expected_current_parent_revision: Some("4".to_owned()),
        })
        .expect("typed request must parse");
        assert_eq!(request.resolution_id(), resolution_id);
        assert_eq!(
            request.action(),
            ConflictResolutionAction::ApplyClientIntent
        );
        assert_eq!(request.expected_current_revision(), Some(Revision::new(7)));

        let invalid = parse_resolution_request(ResolveConflictRequest {
            resolution_id: resolution_id.to_string(),
            action: "AUTO_MERGE".to_owned(),
            expected_current_revision: None,
            expected_current_parent_revision: None,
        });
        assert!(invalid.is_err());
    }

    #[test]
    fn rebaseline_safety_fence_keeps_the_stable_public_error() {
        let error = map_conflict_error(ConflictManagementError::RebaselineRequired {
            reason: RebaselineReason::HistoryUnavailable,
            current_epoch: Sequence::new(7),
            minimum_retained_sequence: Sequence::new(11),
        });

        assert_eq!(error.status_code(), StatusCode::CONFLICT);
        assert_eq!(error.code(), "sync_rebaseline_required");
        assert!(!error.retryable());
    }
}
