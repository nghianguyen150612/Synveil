//! Authenticated one-way synchronization transport.
//!
//! This module exposes only bounded logical journal facts and a durable
//! per-device/per-library checkpoint. The acknowledgment token is integrity
//! evidence for a server-delivered page; it is not a device credential and it
//! never grants access outside the authenticated owner session.

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
use synveil_core::{ChangeEvent, DeviceId, DeviceSyncCheckpoint, LibraryId, Sequence, UserId};
use synveil_metadata::{
    DEFAULT_SYNC_FEED_LIMIT, DatabasePool, DeviceSyncService, RebaselineReason, SyncAckEvidence,
    SyncError, SyncFeedPage,
};

use crate::{
    ApiError, ApiState, RequestContext,
    auth::{AuthenticatedPrincipal, ResponseMeta},
};

type HmacSha256 = Hmac<Sha256>;

const SYNC_ACK_TOKEN_VERSION: &str = "v1";
const SYNC_ACK_TOKEN_KIND: &str = "sync-ack";
const SYNC_ACK_TOKEN_PARTS: usize = 10;
const SYNC_ACK_TAG_BYTES: usize = 32;
const SYNC_ACK_NUMBER_HEX_BYTES: usize = 16;
const SYNC_ACK_TAG_HEX_BYTES: usize = SYNC_ACK_TAG_BYTES * 2;
const SYNC_ACK_DOMAIN: &[u8] = b"synveil/sync-ack/v1\0";

/// Strictly bounded JSON body for the acknowledgment endpoint. The token
/// itself is 254 bytes at most; this leaves room for the JSON envelope while
/// keeping accidental large bodies out of the authenticated mutation path.
pub const SYNC_ACK_BODY_LIMIT_BYTES: usize = 2 * 1024;

/// The serialized signed evidence is fixed-width and intentionally below the
/// existing opaque-cursor boundary.
pub const MAX_SYNC_ACK_TOKEN_BYTES: usize = 256;

/// Safe token verification failures. No variant carries the candidate token,
/// its signature, or any other secret-bearing input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncAckTokenError {
    Malformed,
    Oversized,
    InvalidSignature,
}

/// Per-application HMAC key for bounded server-issued synchronization
/// acknowledgment evidence.
#[derive(Clone)]
pub struct SyncAckKey([u8; 32]);

impl SyncAckKey {
    #[must_use]
    pub fn generate() -> Self {
        let token = synveil_auth::SessionToken::generate();
        Self(*token.as_bytes())
    }

    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Issue a fixed-shape token binding the complete delivered-page
    /// evidence. The token contains no event payload.
    #[must_use]
    pub fn issue(&self, evidence: SyncAckEvidence) -> String {
        let owner = evidence.owner_user_id().to_string();
        let device = evidence.device_id().to_string();
        let library = evidence.library_id().to_string();
        let epoch = format_number(evidence.journal_epoch());
        let from = format_number(evidence.from_sequence());
        let through = format_number(evidence.through_sequence());
        let high_watermark = format_number(evidence.high_watermark());
        let tag = self.sign(evidence);
        let token = format!(
            "{SYNC_ACK_TOKEN_VERSION}.{SYNC_ACK_TOKEN_KIND}.{owner}.{device}.{library}.{epoch}.{from}.{through}.{high_watermark}.{}",
            encode_hex(&tag)
        );
        debug_assert!(token.len() <= MAX_SYNC_ACK_TOKEN_BYTES);
        token
    }

    /// Verify and recover the typed claims from a canonical token.
    pub fn verify(&self, candidate: &str) -> Result<SyncAckEvidence, SyncAckTokenError> {
        if candidate.len() > MAX_SYNC_ACK_TOKEN_BYTES {
            return Err(SyncAckTokenError::Oversized);
        }

        let parts: Vec<_> = candidate.split('.').collect();
        if parts.len() != SYNC_ACK_TOKEN_PARTS
            || parts[0] != SYNC_ACK_TOKEN_VERSION
            || parts[1] != SYNC_ACK_TOKEN_KIND
            || parts[2].len() != 36
            || parts[3].len() != 36
            || parts[4].len() != 36
            || parts[5].len() != SYNC_ACK_NUMBER_HEX_BYTES
            || parts[6].len() != SYNC_ACK_NUMBER_HEX_BYTES
            || parts[7].len() != SYNC_ACK_NUMBER_HEX_BYTES
            || parts[8].len() != SYNC_ACK_NUMBER_HEX_BYTES
            || parts[9].len() != SYNC_ACK_TAG_HEX_BYTES
        {
            return Err(SyncAckTokenError::Malformed);
        }

        let owner_user_id = UserId::from_str(parts[2]).map_err(|_| SyncAckTokenError::Malformed)?;
        let device_id = DeviceId::from_str(parts[3]).map_err(|_| SyncAckTokenError::Malformed)?;
        let library_id = LibraryId::from_str(parts[4]).map_err(|_| SyncAckTokenError::Malformed)?;
        let journal_epoch = parse_number(parts[5])?;
        let from_sequence = parse_number(parts[6])?;
        let through_sequence = parse_number(parts[7])?;
        let high_watermark = parse_number(parts[8])?;
        let tag = decode_hex(parts[9]).ok_or(SyncAckTokenError::Malformed)?;
        let evidence = SyncAckEvidence::new(
            owner_user_id,
            device_id,
            library_id,
            journal_epoch,
            from_sequence,
            through_sequence,
            high_watermark,
        );
        let expected = self.sign(evidence);
        if expected.as_slice().ct_eq(&tag).unwrap_u8() != 1 {
            return Err(SyncAckTokenError::InvalidSignature);
        }
        Ok(evidence)
    }

    fn sign(&self, evidence: SyncAckEvidence) -> [u8; SYNC_ACK_TAG_BYTES] {
        let mut mac = HmacSha256::new_from_slice(&self.0).expect("HMAC accepts a 256-bit key");
        mac.update(SYNC_ACK_DOMAIN);
        mac.update(evidence.owner_user_id().as_bytes());
        mac.update(evidence.device_id().as_bytes());
        mac.update(evidence.library_id().as_bytes());
        mac.update(&evidence.journal_epoch().get().to_be_bytes());
        mac.update(&evidence.from_sequence().get().to_be_bytes());
        mac.update(&evidence.through_sequence().get().to_be_bytes());
        mac.update(&evidence.high_watermark().get().to_be_bytes());
        let bytes = mac.finalize().into_bytes();
        let mut tag = [0_u8; SYNC_ACK_TAG_BYTES];
        tag.copy_from_slice(&bytes);
        tag
    }
}

impl Default for SyncAckKey {
    fn default() -> Self {
        Self::generate()
    }
}

impl fmt::Debug for SyncAckKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SyncAckKey([REDACTED])")
    }
}

fn format_number(value: Sequence) -> String {
    format!("{:016x}", value.get())
}

fn parse_number(value: &str) -> Result<Sequence, SyncAckTokenError> {
    if value.len() != SYNC_ACK_NUMBER_HEX_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(SyncAckTokenError::Malformed);
    }
    u64::from_str_radix(value, 16)
        .map(Sequence::new)
        .map_err(|_| SyncAckTokenError::Malformed)
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(hex_digit(byte >> 4));
        value.push(hex_digit(byte & 0x0f));
    }
    value
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.len() != SYNC_ACK_TAG_HEX_BYTES {
        return None;
    }
    let mut bytes = Vec::with_capacity(SYNC_ACK_TAG_BYTES);
    for pair in value.as_bytes().chunks(2) {
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

/// Application port for the authenticated one-way synchronization transport.
#[async_trait]
pub trait SyncFeedBackend: Send + Sync {
    async fn checkpoint(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
    ) -> Result<DeviceSyncCheckpoint, SyncError>;

    async fn fetch_feed(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        limit: u32,
    ) -> Result<SyncFeedPage, SyncError>;

    async fn acknowledge(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        evidence: SyncAckEvidence,
    ) -> Result<DeviceSyncCheckpoint, SyncError>;
}

/// PostgreSQL adapter for the transport port.
pub struct PostgresSyncFeedBackend {
    service: DeviceSyncService,
}

impl PostgresSyncFeedBackend {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self {
            service: DeviceSyncService::new(pool),
        }
    }
}

#[async_trait]
impl SyncFeedBackend for PostgresSyncFeedBackend {
    async fn checkpoint(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
    ) -> Result<DeviceSyncCheckpoint, SyncError> {
        self.service
            .ensure_checkpoint(owner_user_id, device_id, library_id)
            .await
    }

    async fn fetch_feed(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        limit: u32,
    ) -> Result<SyncFeedPage, SyncError> {
        self.service
            .fetch_feed(owner_user_id, device_id, library_id, limit)
            .await
    }

    async fn acknowledge(
        &self,
        owner_user_id: UserId,
        device_id: DeviceId,
        library_id: LibraryId,
        evidence: SyncAckEvidence,
    ) -> Result<DeviceSyncCheckpoint, SyncError> {
        self.service
            .acknowledge(owner_user_id, device_id, library_id, evidence)
            .await
    }
}

/// Fail-closed backend used by an unconfigured composition root.
pub(crate) struct UnavailableSyncFeedBackend;

#[async_trait]
impl SyncFeedBackend for UnavailableSyncFeedBackend {
    async fn checkpoint(
        &self,
        _owner_user_id: UserId,
        _device_id: DeviceId,
        _library_id: LibraryId,
    ) -> Result<DeviceSyncCheckpoint, SyncError> {
        Err(SyncError::DependencyUnavailable)
    }

    async fn fetch_feed(
        &self,
        _owner_user_id: UserId,
        _device_id: DeviceId,
        _library_id: LibraryId,
        _limit: u32,
    ) -> Result<SyncFeedPage, SyncError> {
        Err(SyncError::DependencyUnavailable)
    }

    async fn acknowledge(
        &self,
        _owner_user_id: UserId,
        _device_id: DeviceId,
        _library_id: LibraryId,
        _evidence: SyncAckEvidence,
    ) -> Result<DeviceSyncCheckpoint, SyncError> {
        Err(SyncError::DependencyUnavailable)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SyncFeedQuery {
    limit: Option<u32>,
}

impl SyncFeedQuery {
    fn limit(&self) -> u32 {
        self.limit.unwrap_or(DEFAULT_SYNC_FEED_LIMIT)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SyncAckRequest {
    ack_token: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SyncCheckpointResponse {
    data: SyncCheckpointData,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
struct SyncCheckpointData {
    device_id: String,
    library_id: String,
    epoch: String,
    acknowledged_sequence: String,
    created_at: String,
    updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_seen_high_watermark: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct SyncFeedResponse {
    data: SyncFeedData,
    meta: ResponseMeta,
}

#[derive(Serialize)]
struct SyncFeedData {
    device_id: String,
    library_id: String,
    epoch: String,
    from_sequence: String,
    through_sequence: String,
    high_watermark: String,
    has_more: bool,
    changes: Vec<SyncChangeData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ack_token: Option<String>,
}

#[derive(Debug, Serialize)]
struct SyncChangeData {
    event_id: String,
    schema_version: u16,
    sequence: String,
    resource_kind: &'static str,
    resource_id: String,
    change_kind: &'static str,
    resource_revision: String,
    occurred_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    node_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    node_state: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_version_id: Option<String>,
}

pub(crate) async fn get_checkpoint(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path((device_id, library_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let (device_id, library_id) = parse_scope(&device_id, &library_id)?;
    auth.require_device(device_id)?;
    let checkpoint = state
        .sync_backend()
        .checkpoint(auth.owner_user_id(), device_id, library_id)
        .await
        .map_err(map_sync_error)?;
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(SyncCheckpointResponse {
                data: checkpoint_data(checkpoint),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn get_changes(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path((device_id, library_id)): Path<(String, String)>,
    Query(query): Query<SyncFeedQuery>,
) -> Result<Response, ApiError> {
    let (device_id, library_id) = parse_scope(&device_id, &library_id)?;
    auth.require_device(device_id)?;
    let limit = query.limit();
    let page = state
        .sync_backend()
        .fetch_feed(auth.owner_user_id(), device_id, library_id, limit)
        .await
        .map_err(map_sync_error)?;
    let ack_token = if page.changes().is_empty() {
        None
    } else {
        Some(state.sync_ack_key().issue(SyncAckEvidence::new(
            auth.owner_user_id(),
            device_id,
            library_id,
            page.checkpoint().journal_epoch(),
            page.from_sequence(),
            page.through_sequence(),
            page.high_watermark().sequence(),
        )))
    };
    let data = feed_data(page, ack_token);
    tracing::info!(
        device_id = %device_id,
        library_id = %library_id,
        page_count = data.changes.len(),
        from_sequence = %data.from_sequence,
        through_sequence = %data.through_sequence,
        high_watermark = %data.high_watermark,
        "sync feed page delivered"
    );
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(SyncFeedResponse {
                data,
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

pub(crate) async fn acknowledge(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path((device_id, library_id)): Path<(String, String)>,
    Json(payload): Json<SyncAckRequest>,
) -> Result<Response, ApiError> {
    let (device_id, library_id) = parse_scope(&device_id, &library_id)?;
    auth.require_device(device_id)?;
    let evidence = state
        .sync_ack_key()
        .verify(&payload.ack_token)
        .map_err(|_| ApiError::SyncInvalidAckToken)?;
    if evidence.owner_user_id() != auth.owner_user_id()
        || evidence.device_id() != device_id
        || evidence.library_id() != library_id
    {
        return Err(ApiError::Core(synveil_core::ErrorCode::NotFound));
    }
    let checkpoint = state
        .sync_backend()
        .acknowledge(auth.owner_user_id(), device_id, library_id, evidence)
        .await
        .map_err(map_sync_error)?;
    tracing::info!(
        device_id = %device_id,
        library_id = %library_id,
        acknowledged_sequence = %checkpoint.acknowledged_sequence(),
        "sync checkpoint acknowledgment applied"
    );
    Ok(private_no_store(
        (
            StatusCode::OK,
            Json(SyncCheckpointResponse {
                data: checkpoint_data(checkpoint),
                meta: response_meta(&context),
            }),
        )
            .into_response(),
    ))
}

fn parse_scope(device_id: &str, library_id: &str) -> Result<(DeviceId, LibraryId), ApiError> {
    let device_id = DeviceId::from_str(device_id).map_err(|_| ApiError::InvalidRequest)?;
    let library_id = LibraryId::from_str(library_id).map_err(|_| ApiError::InvalidRequest)?;
    Ok((device_id, library_id))
}

fn checkpoint_data(checkpoint: DeviceSyncCheckpoint) -> SyncCheckpointData {
    SyncCheckpointData {
        device_id: checkpoint.device_id().to_string(),
        library_id: checkpoint.library_id().to_string(),
        epoch: checkpoint.journal_epoch().to_string(),
        acknowledged_sequence: checkpoint.acknowledged_sequence().to_string(),
        created_at: checkpoint.created_at().to_string(),
        updated_at: checkpoint.updated_at().to_string(),
        last_seen_high_watermark: checkpoint
            .last_seen_high_watermark()
            .map(|sequence| sequence.to_string()),
    }
}

fn feed_data(page: SyncFeedPage, ack_token: Option<String>) -> SyncFeedData {
    let checkpoint = page.checkpoint();
    let device_id = checkpoint.device_id();
    let library_id = checkpoint.library_id();
    let epoch = checkpoint.journal_epoch();
    SyncFeedData {
        device_id: device_id.to_string(),
        library_id: library_id.to_string(),
        epoch: epoch.to_string(),
        from_sequence: page.from_sequence().to_string(),
        through_sequence: page.through_sequence().to_string(),
        high_watermark: page.high_watermark().sequence().to_string(),
        has_more: page.has_more(),
        changes: page.changes().iter().copied().map(change_data).collect(),
        ack_token,
    }
}

fn change_data(change: ChangeEvent) -> SyncChangeData {
    SyncChangeData {
        event_id: change.id().to_string(),
        schema_version: change.schema_version(),
        sequence: change.sequence().to_string(),
        resource_kind: change.resource_kind().as_str(),
        resource_id: change.resource_id().to_string(),
        change_kind: change.change_kind().as_str(),
        resource_revision: change.resource_revision().to_string(),
        occurred_at: change.occurred_at().to_string(),
        parent_node_id: change.parent_node_id().map(|id| id.to_string()),
        node_kind: change.node_kind().map(|kind| kind.as_str()),
        node_state: change.node_state().map(|state| state.as_str()),
        current_version_id: change.current_version_id().map(|id| id.to_string()),
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

fn map_sync_error(error: SyncError) -> ApiError {
    match error {
        SyncError::NotFound => ApiError::Core(synveil_core::ErrorCode::NotFound),
        SyncError::InvalidLimit => ApiError::SyncInvalidLimit,
        SyncError::InvalidAckToken => ApiError::SyncInvalidAckToken,
        SyncError::CheckpointConflict => ApiError::SyncCheckpointConflict,
        SyncError::RebaselineRequired {
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
        SyncError::DependencyUnavailable | SyncError::Database(_) => ApiError::ReadinessUnavailable,
        SyncError::InvalidPersistedData | SyncError::InternalError => ApiError::Internal,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_SYNC_ACK_TOKEN_BYTES, SyncAckKey, SyncAckTokenError, format_number, parse_number,
    };
    use synveil_core::{DeviceId, LibraryId, Sequence, UserId};
    use synveil_metadata::SyncAckEvidence;

    #[test]
    fn signed_ack_tokens_round_trip_with_fixed_width_bounded_claims() {
        let key = SyncAckKey::from_bytes([0x42; 32]);
        let evidence = SyncAckEvidence::new(
            UserId::new(),
            DeviceId::new(),
            LibraryId::new(),
            Sequence::new(7),
            Sequence::new(100),
            Sequence::new(200),
            Sequence::new(250),
        );
        let token = key.issue(evidence);

        assert!(token.len() <= MAX_SYNC_ACK_TOKEN_BYTES);
        assert_eq!(key.verify(&token), Ok(evidence));
        assert_ne!(
            token,
            key.issue(SyncAckEvidence::new(
                evidence.owner_user_id(),
                evidence.device_id(),
                evidence.library_id(),
                evidence.journal_epoch(),
                evidence.from_sequence(),
                evidence.through_sequence(),
                Sequence::new(251),
            ))
        );
    }

    #[test]
    fn token_verification_rejects_malformed_oversized_and_tampered_values() {
        let key = SyncAckKey::from_bytes([0x42; 32]);
        assert_eq!(key.verify("not-a-token"), Err(SyncAckTokenError::Malformed));
        assert_eq!(
            key.verify(&"x".repeat(MAX_SYNC_ACK_TOKEN_BYTES + 1)),
            Err(SyncAckTokenError::Oversized)
        );
        let evidence = SyncAckEvidence::new(
            UserId::new(),
            DeviceId::new(),
            LibraryId::new(),
            Sequence::new(1),
            Sequence::new(0),
            Sequence::new(1),
            Sequence::new(1),
        );
        let mut token = key.issue(evidence);
        let replacement = if token.ends_with('0') { '1' } else { '0' };
        token.pop();
        token.push(replacement);
        assert_eq!(key.verify(&token), Err(SyncAckTokenError::InvalidSignature));
    }

    #[test]
    fn signed_ack_tokens_bind_owner_device_library_and_epoch_claims() {
        let key = SyncAckKey::from_bytes([0x42; 32]);
        let evidence = SyncAckEvidence::new(
            UserId::new(),
            DeviceId::new(),
            LibraryId::new(),
            Sequence::new(3),
            Sequence::new(10),
            Sequence::new(20),
            Sequence::new(25),
        );
        let token = key.issue(evidence);

        let mut parts = token.split('.').map(str::to_owned).collect::<Vec<_>>();
        for (index, replacement) in [
            (2, UserId::new().to_string()),
            (3, DeviceId::new().to_string()),
            (4, LibraryId::new().to_string()),
            (5, format_number(Sequence::new(4))),
        ] {
            parts[index] = replacement;
            assert_eq!(
                key.verify(&parts.join(".")),
                Err(SyncAckTokenError::InvalidSignature),
                "claim at token part {index} must remain signature-bound"
            );
            parts = key.issue(evidence).split('.').map(str::to_owned).collect();
        }
    }

    #[test]
    fn numeric_claims_are_canonical_lowercase_fixed_width_hex() {
        assert_eq!(format_number(Sequence::new(15)), "000000000000000f");
        assert_eq!(parse_number("000000000000000f"), Ok(Sequence::new(15)));
        assert_eq!(
            parse_number("000000000000000F"),
            Err(SyncAckTokenError::Malformed)
        );
        assert_eq!(parse_number("f"), Err(SyncAckTokenError::Malformed));
    }
}
