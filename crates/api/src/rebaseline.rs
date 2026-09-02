//! Authenticated transport for materialized logical sync bootstrap manifests.
//!
//! Cursor and terminal-page tokens are server-issued HMAC evidence, not
//! authorization capabilities. Every request independently checks the current
//! authenticated owner, registered ACTIVE device, owned library, and durable
//! bootstrap scope in PostgreSQL.

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
    DeviceId, LibraryId, LogicalSnapshotNode, Sequence, SyncBootstrap, SyncBootstrapId, UserId,
};
use synveil_metadata::{
    BootstrapCompletion, BootstrapCompletionEvidence, BootstrapPagePosition,
    BootstrapTerminalEvidence, DEFAULT_SYNC_BOOTSTRAP_PAGE_LIMIT, DatabasePool, RebaselineError,
    RebaselineReason, SnapshotNodePage, SyncBootstrapService,
};
use uuid::Uuid;

use crate::{
    ApiError, ApiState, RequestContext,
    auth::{AuthenticatedPrincipal, ResponseMeta},
};

type HmacSha256 = Hmac<Sha256>;

const TOKEN_VERSION: &str = "v1";
const CURSOR_KIND: &str = "rebaseline-cursor";
const COMPLETION_KIND: &str = "rebaseline-complete";
const CURSOR_PARTS: usize = 11;
const COMPLETION_PARTS: usize = 12;
const ID_HEX_BYTES: usize = 32;
const NUMBER_HEX_BYTES: usize = 16;
const TAG_BYTES: usize = 32;
const TAG_HEX_BYTES: usize = TAG_BYTES * 2;
const CURSOR_DOMAIN: &[u8] = b"synveil/rebaseline-cursor/v1\0";
const COMPLETION_DOMAIN: &[u8] = b"synveil/rebaseline-complete/v1\0";

pub const REBASELINE_BODY_LIMIT_BYTES: usize = 2 * 1024;
pub const MAX_REBASELINE_CURSOR_BYTES: usize = 320;
pub const MAX_REBASELINE_COMPLETION_TOKEN_BYTES: usize = 336;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RebaselineTokenError {
    Malformed,
    Oversized,
    InvalidSignature,
}

/// Safe configuration error for the persistent bootstrap HMAC key. The
/// candidate secret is deliberately never retained in or formatted by this
/// error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RebaselineTokenKeyParseError;

impl fmt::Display for RebaselineTokenKeyParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("rebaseline token key must be 64 lowercase hexadecimal characters")
    }
}

impl std::error::Error for RebaselineTokenKeyParseError {}

/// One per-application secret with explicit domain separation between page
/// cursors and terminal completion proof.
#[derive(Clone)]
pub struct RebaselineTokenKey([u8; 32]);

impl RebaselineTokenKey {
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
    pub(crate) const fn key_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Parse the deployment-stable 256-bit secret used across API restarts.
    /// No formatter or error path exposes the supplied candidate.
    pub fn from_hex(value: &str) -> Result<Self, RebaselineTokenKeyParseError> {
        let bytes = decode_hex(value, TAG_HEX_BYTES).ok_or(RebaselineTokenKeyParseError)?;
        let bytes: [u8; TAG_BYTES] = bytes.try_into().map_err(|_| RebaselineTokenKeyParseError)?;
        Ok(Self(bytes))
    }

    #[must_use]
    pub fn issue_cursor(&self, position: BootstrapPagePosition) -> String {
        let tag = self.sign_cursor(position);
        let token = format!(
            "{TOKEN_VERSION}.{CURSOR_KIND}.{}.{}.{}.{}.{}.{}.{}.{}.{}",
            encode_id(position.owner_user_id()),
            encode_id(position.device_id()),
            encode_id(position.library_id()),
            encode_id(position.bootstrap_id()),
            format_number(position.generation()),
            format_number(position.snapshot_epoch()),
            format_number(position.snapshot_resume_sequence()),
            encode_id(position.after_node_id()),
            encode_hex(&tag),
        );
        debug_assert!(token.len() <= MAX_REBASELINE_CURSOR_BYTES);
        token
    }

    pub fn verify_cursor(
        &self,
        candidate: &str,
    ) -> Result<BootstrapPagePosition, RebaselineTokenError> {
        if candidate.len() > MAX_REBASELINE_CURSOR_BYTES {
            return Err(RebaselineTokenError::Oversized);
        }
        let parts = candidate.split('.').collect::<Vec<_>>();
        if parts.len() != CURSOR_PARTS || parts[0] != TOKEN_VERSION || parts[1] != CURSOR_KIND {
            return Err(RebaselineTokenError::Malformed);
        }
        let position = BootstrapPagePosition::new(
            parse_id(parts[2])?,
            parse_id(parts[3])?,
            parse_id(parts[4])?,
            parse_id(parts[5])?,
            parse_number(parts[6])?,
            parse_number(parts[7])?,
            parse_number(parts[8])?,
            parse_id(parts[9])?,
        );
        verify_tag(parts[10], &self.sign_cursor(position))?;
        Ok(position)
    }

    #[must_use]
    pub fn issue_completion(&self, evidence: BootstrapTerminalEvidence) -> String {
        let terminal = evidence
            .terminal_node_id()
            .map_or_else(|| "0".repeat(ID_HEX_BYTES), encode_id);
        let tag = self.sign_completion(evidence);
        let token = format!(
            "{TOKEN_VERSION}.{COMPLETION_KIND}.{}.{}.{}.{}.{}.{}.{}.{}.{}.{}",
            encode_id(evidence.owner_user_id()),
            encode_id(evidence.device_id()),
            encode_id(evidence.library_id()),
            encode_id(evidence.bootstrap_id()),
            format_number(evidence.generation()),
            format_number(evidence.snapshot_epoch()),
            format_number(evidence.snapshot_resume_sequence()),
            format_u64(evidence.manifest_item_count()),
            terminal,
            encode_hex(&tag),
        );
        debug_assert!(token.len() <= MAX_REBASELINE_COMPLETION_TOKEN_BYTES);
        token
    }

    pub fn verify_completion(
        &self,
        candidate: &str,
    ) -> Result<BootstrapCompletionEvidence, RebaselineTokenError> {
        if candidate.len() > MAX_REBASELINE_COMPLETION_TOKEN_BYTES {
            return Err(RebaselineTokenError::Oversized);
        }
        let parts = candidate.split('.').collect::<Vec<_>>();
        if parts.len() != COMPLETION_PARTS
            || parts[0] != TOKEN_VERSION
            || parts[1] != COMPLETION_KIND
        {
            return Err(RebaselineTokenError::Malformed);
        }
        let manifest_item_count = parse_u64(parts[9])?;
        let terminal_node_id = if parts[10].bytes().all(|byte| byte == b'0') {
            if manifest_item_count != 0 {
                return Err(RebaselineTokenError::Malformed);
            }
            None
        } else {
            if manifest_item_count == 0 {
                return Err(RebaselineTokenError::Malformed);
            }
            Some(parse_id(parts[10])?)
        };
        let terminal = BootstrapTerminalEvidence::new(
            parse_id(parts[2])?,
            parse_id(parts[3])?,
            parse_id(parts[4])?,
            parse_id(parts[5])?,
            parse_number(parts[6])?,
            parse_number(parts[7])?,
            parse_number(parts[8])?,
            manifest_item_count,
            terminal_node_id,
        );
        verify_tag(parts[11], &self.sign_completion(terminal))?;
        Ok(BootstrapCompletionEvidence::new(terminal))
    }

    fn sign_cursor(&self, position: BootstrapPagePosition) -> [u8; TAG_BYTES] {
        let mut mac = HmacSha256::new_from_slice(&self.0).expect("HMAC accepts a 256-bit key");
        mac.update(CURSOR_DOMAIN);
        mac.update(position.owner_user_id().as_bytes());
        mac.update(position.device_id().as_bytes());
        mac.update(position.library_id().as_bytes());
        mac.update(position.bootstrap_id().as_bytes());
        mac.update(&position.generation().get().to_be_bytes());
        mac.update(&position.snapshot_epoch().get().to_be_bytes());
        mac.update(&position.snapshot_resume_sequence().get().to_be_bytes());
        mac.update(position.after_node_id().as_bytes());
        finalize_tag(mac)
    }

    fn sign_completion(&self, evidence: BootstrapTerminalEvidence) -> [u8; TAG_BYTES] {
        let mut mac = HmacSha256::new_from_slice(&self.0).expect("HMAC accepts a 256-bit key");
        mac.update(COMPLETION_DOMAIN);
        mac.update(evidence.owner_user_id().as_bytes());
        mac.update(evidence.device_id().as_bytes());
        mac.update(evidence.library_id().as_bytes());
        mac.update(evidence.bootstrap_id().as_bytes());
        mac.update(&evidence.generation().get().to_be_bytes());
        mac.update(&evidence.snapshot_epoch().get().to_be_bytes());
        mac.update(&evidence.snapshot_resume_sequence().get().to_be_bytes());
        mac.update(&evidence.manifest_item_count().to_be_bytes());
        match evidence.terminal_node_id() {
            Some(node_id) => {
                mac.update(&[1]);
                mac.update(node_id.as_bytes());
            }
            None => mac.update(&[0]),
        }
        finalize_tag(mac)
    }
}

impl Default for RebaselineTokenKey {
    fn default() -> Self {
        Self::generate()
    }
}

impl fmt::Debug for RebaselineTokenKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RebaselineTokenKey([REDACTED])")
    }
}

fn finalize_tag(mac: HmacSha256) -> [u8; TAG_BYTES] {
    let bytes = mac.finalize().into_bytes();
    let mut tag = [0_u8; TAG_BYTES];
    tag.copy_from_slice(&bytes);
    tag
}

fn verify_tag(value: &str, expected: &[u8; TAG_BYTES]) -> Result<(), RebaselineTokenError> {
    let tag = decode_hex(value, TAG_HEX_BYTES).ok_or(RebaselineTokenError::Malformed)?;
    if expected.as_slice().ct_eq(&tag).unwrap_u8() != 1 {
        return Err(RebaselineTokenError::InvalidSignature);
    }
    Ok(())
}

fn encode_id<T>(id: T) -> String
where
    T: AsRef<Uuid>,
{
    encode_hex(id.as_ref().as_bytes())
}

fn parse_id<T>(value: &str) -> Result<T, RebaselineTokenError>
where
    T: TryFrom<Uuid, Error = synveil_core::IdParseError>,
{
    let bytes = decode_hex(value, ID_HEX_BYTES).ok_or(RebaselineTokenError::Malformed)?;
    let bytes: [u8; 16] = bytes
        .try_into()
        .map_err(|_| RebaselineTokenError::Malformed)?;
    T::try_from(Uuid::from_bytes(bytes)).map_err(|_| RebaselineTokenError::Malformed)
}

fn format_number(value: Sequence) -> String {
    format_u64(value.get())
}

fn format_u64(value: u64) -> String {
    format!("{value:016x}")
}

fn parse_number(value: &str) -> Result<Sequence, RebaselineTokenError> {
    parse_u64(value).map(Sequence::new)
}

fn parse_u64(value: &str) -> Result<u64, RebaselineTokenError> {
    if value.len() != NUMBER_HEX_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(RebaselineTokenError::Malformed);
    }
    u64::from_str_radix(value, 16).map_err(|_| RebaselineTokenError::Malformed)
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(hex_digit(byte >> 4));
        value.push(hex_digit(byte & 0x0f));
    }
    value
}

fn decode_hex(value: &str, expected_hex_bytes: usize) -> Option<Vec<u8>> {
    if value.len() != expected_hex_bytes {
        return None;
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().as_chunks::<2>().0 {
        bytes.push((decode_nibble(pair[0])? << 4) | decode_nibble(pair[1])?);
    }
    Some(bytes)
}

const fn decode_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

const fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'a' + value - 10) as char,
        _ => unreachable!(),
    }
}

#[async_trait]
pub trait RebaselineBackend: Send + Sync {
    async fn start(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
    ) -> Result<SyncBootstrap, RebaselineError>;

    #[allow(clippy::too_many_arguments)]
    async fn page_nodes(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        bootstrap_id: SyncBootstrapId,
        position: Option<BootstrapPagePosition>,
        limit: u32,
    ) -> Result<SnapshotNodePage, RebaselineError>;

    async fn complete(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        bootstrap_id: SyncBootstrapId,
        evidence: BootstrapCompletionEvidence,
    ) -> Result<BootstrapCompletion, RebaselineError>;
}

pub struct PostgresRebaselineBackend {
    service: SyncBootstrapService,
}

impl PostgresRebaselineBackend {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self {
            service: SyncBootstrapService::new(pool),
        }
    }
}

#[async_trait]
impl RebaselineBackend for PostgresRebaselineBackend {
    async fn start(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
    ) -> Result<SyncBootstrap, RebaselineError> {
        self.service
            .start(owner_user_id, device_id, library_id)
            .await
    }

    async fn page_nodes(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        bootstrap_id: SyncBootstrapId,
        position: Option<BootstrapPagePosition>,
        limit: u32,
    ) -> Result<SnapshotNodePage, RebaselineError> {
        self.service
            .page_nodes(
                owner_user_id,
                device_id,
                library_id,
                bootstrap_id,
                position,
                limit,
            )
            .await
    }

    async fn complete(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        bootstrap_id: SyncBootstrapId,
        evidence: BootstrapCompletionEvidence,
    ) -> Result<BootstrapCompletion, RebaselineError> {
        self.service
            .complete(owner_user_id, device_id, library_id, bootstrap_id, evidence)
            .await
    }
}

pub(crate) struct UnavailableRebaselineBackend;

#[async_trait]
impl RebaselineBackend for UnavailableRebaselineBackend {
    async fn start(
        &self,
        _owner_user_id: UserId,
        _device_id: DeviceId,
        _library_id: LibraryId,
    ) -> Result<SyncBootstrap, RebaselineError> {
        Err(RebaselineError::DependencyUnavailable)
    }

    async fn page_nodes(
        &self,
        _owner_user_id: UserId,
        _device_id: DeviceId,
        _library_id: LibraryId,
        _bootstrap_id: SyncBootstrapId,
        _position: Option<BootstrapPagePosition>,
        _limit: u32,
    ) -> Result<SnapshotNodePage, RebaselineError> {
        Err(RebaselineError::DependencyUnavailable)
    }

    async fn complete(
        &self,
        _owner_user_id: UserId,
        _device_id: DeviceId,
        _library_id: LibraryId,
        _bootstrap_id: SyncBootstrapId,
        _evidence: BootstrapCompletionEvidence,
    ) -> Result<BootstrapCompletion, RebaselineError> {
        Err(RebaselineError::DependencyUnavailable)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StartRequest {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PageQuery {
    cursor: Option<String>,
    limit: Option<u32>,
}

impl PageQuery {
    fn limit(&self) -> u32 {
        self.limit.unwrap_or(DEFAULT_SYNC_BOOTSTRAP_PAGE_LIMIT)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompleteRequest {
    completion_token: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct BootstrapResponse {
    data: BootstrapData,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
struct BootstrapData {
    bootstrap_id: String,
    device_id: String,
    library_id: String,
    state: &'static str,
    generation: String,
    snapshot_epoch: String,
    snapshot_resume_sequence: String,
    manifest_item_count: String,
    created_at: String,
    expires_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    completed_at: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct SnapshotPageResponse {
    data: SnapshotPageData,
    meta: ResponseMeta,
}

#[derive(Serialize)]
struct SnapshotPageData {
    bootstrap: BootstrapData,
    nodes: Vec<SnapshotNodeData>,
    has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completion_token: Option<String>,
}

#[derive(Debug, Serialize)]
struct SnapshotNodeData {
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
    current_content: Option<SnapshotContentData>,
}

#[derive(Debug, Serialize)]
struct SnapshotContentData {
    byte_length: String,
    sha256: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct CompletionResponse {
    data: CompletionData,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
struct CompletionData {
    bootstrap: BootstrapData,
    checkpoint: CompletionCheckpointData,
    replayed: bool,
}

#[derive(Debug, Serialize)]
struct CompletionCheckpointData {
    journal_epoch: String,
    acknowledged_sequence: String,
    updated_at: String,
}

pub(crate) async fn start(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path((device_id, library_id)): Path<(String, String)>,
    Json(_payload): Json<StartRequest>,
) -> Result<Response, ApiError> {
    let (device_id, library_id) = parse_scope(&device_id, &library_id)?;
    auth.require_device(device_id)?;
    let bootstrap = state
        .rebaseline_backend()
        .start(auth.owner_user_id(), device_id, library_id)
        .await
        .map_err(map_rebaseline_error)?;
    tracing::info!(
        bootstrap_id = %bootstrap.id(),
        device_id = %device_id,
        library_id = %library_id,
        resume_sequence = %bootstrap.snapshot_resume_sequence(),
        item_count = bootstrap.manifest_item_count(),
        "sync rebaseline bootstrap started"
    );
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(BootstrapResponse {
                data: bootstrap_data(bootstrap),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn get_nodes(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path((device_id, library_id, bootstrap_id)): Path<(String, String, String)>,
    Query(query): Query<PageQuery>,
) -> Result<Response, ApiError> {
    let (device_id, library_id, bootstrap_id) =
        parse_bootstrap_scope(&device_id, &library_id, &bootstrap_id)?;
    auth.require_device(device_id)?;
    let position = query
        .cursor
        .as_deref()
        .map(|cursor| state.rebaseline_token_key().verify_cursor(cursor))
        .transpose()
        .map_err(|_| ApiError::RebaselineInvalidCursor)?;
    if let Some(position) = position {
        if position.owner_user_id() != auth.owner_user_id() {
            return Err(ApiError::Core(synveil_core::ErrorCode::NotFound));
        }
        if position.device_id() != device_id
            || position.library_id() != library_id
            || position.bootstrap_id() != bootstrap_id
        {
            return Err(ApiError::RebaselineInvalidCursor);
        }
    }
    let page = state
        .rebaseline_backend()
        .page_nodes(
            auth.owner_user_id(),
            device_id,
            library_id,
            bootstrap_id,
            position,
            query.limit(),
        )
        .await
        .map_err(map_rebaseline_error)?;
    let next_cursor = page
        .next_position()
        .map(|position| state.rebaseline_token_key().issue_cursor(position));
    let completion_token = page
        .terminal_evidence()
        .map(|evidence| state.rebaseline_token_key().issue_completion(evidence));
    let data = snapshot_page_data(page, next_cursor, completion_token);
    tracing::info!(
        bootstrap_id = %bootstrap_id,
        device_id = %device_id,
        library_id = %library_id,
        page_count = data.nodes.len(),
        terminal = !data.has_more,
        "sync rebaseline manifest page delivered"
    );
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(SnapshotPageResponse {
                data,
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn complete(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path((device_id, library_id, bootstrap_id)): Path<(String, String, String)>,
    Json(payload): Json<CompleteRequest>,
) -> Result<Response, ApiError> {
    let (device_id, library_id, bootstrap_id) =
        parse_bootstrap_scope(&device_id, &library_id, &bootstrap_id)?;
    auth.require_device(device_id)?;
    let evidence = state
        .rebaseline_token_key()
        .verify_completion(&payload.completion_token)
        .map_err(|_| ApiError::RebaselineInvalidBootstrapToken)?;
    let terminal = evidence.terminal();
    if terminal.owner_user_id() != auth.owner_user_id() {
        return Err(ApiError::Core(synveil_core::ErrorCode::NotFound));
    }
    if terminal.device_id() != device_id
        || terminal.library_id() != library_id
        || terminal.bootstrap_id() != bootstrap_id
    {
        return Err(ApiError::RebaselineInvalidBootstrapToken);
    }
    let completion = state
        .rebaseline_backend()
        .complete(
            auth.owner_user_id(),
            device_id,
            library_id,
            bootstrap_id,
            evidence,
        )
        .await
        .map_err(map_rebaseline_error)?;
    tracing::info!(
        bootstrap_id = %bootstrap_id,
        device_id = %device_id,
        library_id = %library_id,
        acknowledged_sequence = %completion.checkpoint().acknowledged_sequence(),
        replayed = completion.replayed(),
        "sync rebaseline bootstrap completed"
    );
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(CompletionResponse {
                data: completion_data(completion),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

fn parse_scope(device_id: &str, library_id: &str) -> Result<(DeviceId, LibraryId), ApiError> {
    Ok((
        DeviceId::from_str(device_id).map_err(|_| ApiError::InvalidRequest)?,
        LibraryId::from_str(library_id).map_err(|_| ApiError::InvalidRequest)?,
    ))
}

fn parse_bootstrap_scope(
    device_id: &str,
    library_id: &str,
    bootstrap_id: &str,
) -> Result<(DeviceId, LibraryId, SyncBootstrapId), ApiError> {
    let (device_id, library_id) = parse_scope(device_id, library_id)?;
    let bootstrap_id =
        SyncBootstrapId::from_str(bootstrap_id).map_err(|_| ApiError::InvalidRequest)?;
    Ok((device_id, library_id, bootstrap_id))
}

fn bootstrap_data(bootstrap: SyncBootstrap) -> BootstrapData {
    BootstrapData {
        bootstrap_id: bootstrap.id().to_string(),
        device_id: bootstrap.device_id().to_string(),
        library_id: bootstrap.library_id().to_string(),
        state: bootstrap.state().as_str(),
        generation: bootstrap.generation().to_string(),
        snapshot_epoch: bootstrap.snapshot_epoch().to_string(),
        snapshot_resume_sequence: bootstrap.snapshot_resume_sequence().to_string(),
        manifest_item_count: bootstrap.manifest_item_count().to_string(),
        created_at: bootstrap.created_at().to_string(),
        expires_at: bootstrap.expires_at().to_string(),
        completed_at: bootstrap.completed_at().map(|value| value.to_string()),
    }
}

fn snapshot_page_data(
    page: SnapshotNodePage,
    next_cursor: Option<String>,
    completion_token: Option<String>,
) -> SnapshotPageData {
    SnapshotPageData {
        bootstrap: bootstrap_data(page.bootstrap()),
        nodes: page.nodes().iter().map(snapshot_node_data).collect(),
        has_more: page.has_more(),
        next_cursor,
        completion_token,
    }
}

fn snapshot_node_data(node: &LogicalSnapshotNode) -> SnapshotNodeData {
    let current_content = match (node.content_length(), node.content_sha256()) {
        (Some(byte_length), Some(sha256)) => Some(SnapshotContentData {
            byte_length: byte_length.to_string(),
            sha256: sha256.to_string(),
        }),
        _ => None,
    };
    SnapshotNodeData {
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

fn completion_data(completion: BootstrapCompletion) -> CompletionData {
    let checkpoint = completion.checkpoint();
    CompletionData {
        bootstrap: bootstrap_data(completion.bootstrap()),
        checkpoint: CompletionCheckpointData {
            journal_epoch: checkpoint.journal_epoch().to_string(),
            acknowledged_sequence: checkpoint.acknowledged_sequence().to_string(),
            updated_at: checkpoint.updated_at().to_string(),
        },
        replayed: completion.replayed(),
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

fn map_rebaseline_error(error: RebaselineError) -> ApiError {
    match error {
        RebaselineError::NotFound => ApiError::Core(synveil_core::ErrorCode::NotFound),
        RebaselineError::InvalidLimit => ApiError::RebaselineInvalidLimit,
        RebaselineError::InvalidCursor => ApiError::RebaselineInvalidCursor,
        RebaselineError::InvalidBootstrapToken => ApiError::RebaselineInvalidBootstrapToken,
        RebaselineError::BootstrapExpired => ApiError::RebaselineBootstrapExpired,
        RebaselineError::BootstrapConflict => ApiError::RebaselineBootstrapConflict,
        RebaselineError::RebaselineRequired {
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
        RebaselineError::DependencyUnavailable | RebaselineError::Database(_) => {
            ApiError::RebaselineDependencyUnavailable
        }
        RebaselineError::InvalidPersistedData => ApiError::RebaselineInvalidPersistedData,
        RebaselineError::InternalError => ApiError::Internal,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_REBASELINE_COMPLETION_TOKEN_BYTES, MAX_REBASELINE_CURSOR_BYTES, RebaselineTokenError,
        RebaselineTokenKey, RebaselineTokenKeyParseError,
    };
    use synveil_core::{DeviceId, LibraryId, NodeId, Sequence, SyncBootstrapId, UserId};
    use synveil_metadata::{BootstrapPagePosition, BootstrapTerminalEvidence};

    fn position() -> BootstrapPagePosition {
        BootstrapPagePosition::new(
            UserId::new(),
            DeviceId::new(),
            LibraryId::new(),
            SyncBootstrapId::new(),
            Sequence::new(4),
            Sequence::new(2),
            Sequence::new(19),
            NodeId::new(),
        )
    }

    fn mutate_token_part(token: &str, part_index: usize) -> String {
        let mut parts = token.split('.').map(str::to_owned).collect::<Vec<_>>();
        let part = parts
            .get_mut(part_index)
            .expect("test token part index must be present");
        let replacement = if part.ends_with('0') { '1' } else { '0' };
        part.pop();
        part.push(replacement);
        parts.join(".")
    }

    #[test]
    fn snapshot_cursor_round_trips_and_is_scope_bound() {
        let key = RebaselineTokenKey::from_bytes([0x33; 32]);
        let position = position();
        let cursor = key.issue_cursor(position);
        assert!(cursor.len() <= MAX_REBASELINE_CURSOR_BYTES);
        assert_eq!(key.verify_cursor(&cursor), Ok(position));

        let other = BootstrapPagePosition::new(
            position.owner_user_id(),
            position.device_id(),
            LibraryId::new(),
            position.bootstrap_id(),
            position.generation(),
            position.snapshot_epoch(),
            position.snapshot_resume_sequence(),
            position.after_node_id(),
        );
        assert_ne!(key.issue_cursor(other), cursor);

        for scoped_part in 2..=9 {
            assert_eq!(
                key.verify_cursor(&mutate_token_part(&cursor, scoped_part)),
                Err(RebaselineTokenError::InvalidSignature),
                "cursor claim part {scoped_part} must be integrity-bound"
            );
        }
    }

    #[test]
    fn snapshot_cursor_rejects_tamper_and_oversize() {
        let key = RebaselineTokenKey::from_bytes([0x33; 32]);
        let mut cursor = key.issue_cursor(position());
        let replacement = if cursor.ends_with('0') { '1' } else { '0' };
        cursor.pop();
        cursor.push(replacement);
        assert_eq!(
            key.verify_cursor(&cursor),
            Err(RebaselineTokenError::InvalidSignature)
        );
        assert_eq!(
            key.verify_cursor(&"x".repeat(MAX_REBASELINE_CURSOR_BYTES + 1)),
            Err(RebaselineTokenError::Oversized)
        );
    }

    #[test]
    fn completion_token_round_trips_and_binds_terminal_claims() {
        let key = RebaselineTokenKey::from_bytes([0x44; 32]);
        let position = position();
        let terminal = BootstrapTerminalEvidence::new(
            position.owner_user_id(),
            position.device_id(),
            position.library_id(),
            position.bootstrap_id(),
            position.generation(),
            position.snapshot_epoch(),
            position.snapshot_resume_sequence(),
            9,
            Some(position.after_node_id()),
        );
        let token = key.issue_completion(terminal);
        assert!(token.len() <= MAX_REBASELINE_COMPLETION_TOKEN_BYTES);
        assert_eq!(key.verify_completion(&token).unwrap().terminal(), terminal);

        let empty = BootstrapTerminalEvidence::new(
            terminal.owner_user_id(),
            terminal.device_id(),
            terminal.library_id(),
            SyncBootstrapId::new(),
            Sequence::new(5),
            terminal.snapshot_epoch(),
            terminal.snapshot_resume_sequence(),
            0,
            None,
        );
        let empty_token = key.issue_completion(empty);
        assert_eq!(
            key.verify_completion(&empty_token).unwrap().terminal(),
            empty
        );
    }

    #[test]
    fn completion_token_rejects_wrong_key_tamper_and_oversize() {
        let key = RebaselineTokenKey::from_bytes([0x44; 32]);
        let other_key = RebaselineTokenKey::from_bytes([0x45; 32]);
        let position = position();
        let terminal = BootstrapTerminalEvidence::new(
            position.owner_user_id(),
            position.device_id(),
            position.library_id(),
            position.bootstrap_id(),
            position.generation(),
            position.snapshot_epoch(),
            position.snapshot_resume_sequence(),
            1,
            Some(position.after_node_id()),
        );
        let token = key.issue_completion(terminal);
        assert_eq!(
            other_key.verify_completion(&token),
            Err(RebaselineTokenError::InvalidSignature)
        );
        for scoped_part in 2..=6 {
            assert_eq!(
                key.verify_completion(&mutate_token_part(&token, scoped_part)),
                Err(RebaselineTokenError::InvalidSignature),
                "owner/device/library/bootstrap/generation claim part {scoped_part} must be integrity-bound"
            );
        }
        assert_eq!(
            key.verify_completion(&"x".repeat(MAX_REBASELINE_COMPLETION_TOKEN_BYTES + 1)),
            Err(RebaselineTokenError::Oversized)
        );
    }

    #[test]
    fn token_key_debug_is_redacted() {
        let debug = format!("{:?}", RebaselineTokenKey::from_bytes([0x77; 32]));
        assert_eq!(debug, "RebaselineTokenKey([REDACTED])");
        assert!(!debug.contains("119"));
    }

    #[test]
    fn deployment_key_is_canonical_and_stable_across_restart() {
        let encoded = "33".repeat(32);
        let first = RebaselineTokenKey::from_hex(&encoded).unwrap();
        let restarted = RebaselineTokenKey::from_hex(&encoded).unwrap();
        let position = position();
        assert_eq!(
            restarted.verify_cursor(&first.issue_cursor(position)),
            Ok(position)
        );
        let terminal = BootstrapTerminalEvidence::new(
            position.owner_user_id(),
            position.device_id(),
            position.library_id(),
            position.bootstrap_id(),
            position.generation(),
            position.snapshot_epoch(),
            position.snapshot_resume_sequence(),
            1,
            Some(position.after_node_id()),
        );
        assert_eq!(
            restarted
                .verify_completion(&first.issue_completion(terminal))
                .unwrap()
                .terminal(),
            terminal
        );
        assert!(matches!(
            RebaselineTokenKey::from_hex(&"3".repeat(63)),
            Err(RebaselineTokenKeyParseError)
        ));
        assert!(matches!(
            RebaselineTokenKey::from_hex(&"GG".repeat(32)),
            Err(RebaselineTokenKeyParseError)
        ));
    }
}
