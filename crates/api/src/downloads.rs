//! Authenticated HTTP transport for immutable file-content reads.
//!
//! This module owns HTTP Range parsing, validators, response headers, and Axum
//! body framing. Authorization, immutable-version selection, replica
//! verification, and storage reads remain inside the transport-neutral content
//! application service.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    body::Body,
    extract::{Extension, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use synveil_core::{FileVersionId, NodeId, Sha256Digest, UserId};
use synveil_metadata::FileMetadataError;
use synveil_storage::{
    ByteRange, ContentByteStream, ContentDescriptor, ContentMetadata,
    ContentReadApplicationService, ContentReadError,
};

use crate::{
    ApiError, ApiState, RequestContext, auth::AuthenticatedPrincipal, error::map_content_read_error,
};

/// Application-facing port for authenticated content transport. The API never
/// receives a storage key or opens an object store directly.
#[async_trait]
pub trait DownloadBackend: Send + Sync {
    fn supports_range_reads(&self) -> bool;

    async fn current_content_metadata(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
    ) -> Result<DownloadMetadata, ContentReadError>;

    async fn historical_content_metadata(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
    ) -> Result<DownloadMetadata, ContentReadError>;

    async fn open_current_content(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
    ) -> Result<DownloadRead, ContentReadError>;

    async fn open_current_content_range(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
        range: ByteRange,
    ) -> Result<DownloadRead, ContentReadError>;

    async fn open_historical_content(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
    ) -> Result<DownloadRead, ContentReadError>;

    async fn open_historical_content_range(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
        range: ByteRange,
    ) -> Result<DownloadRead, ContentReadError>;
}

#[async_trait]
impl DownloadBackend for ContentReadApplicationService {
    fn supports_range_reads(&self) -> bool {
        ContentReadApplicationService::supports_range_reads(self)
    }

    async fn current_content_metadata(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
    ) -> Result<DownloadMetadata, ContentReadError> {
        ContentReadApplicationService::current_content_metadata(self, owner_user_id, node_id)
            .await
            .map(DownloadMetadata::from_content)
    }

    async fn historical_content_metadata(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
    ) -> Result<DownloadMetadata, ContentReadError> {
        ContentReadApplicationService::file_version_content_metadata(
            self,
            owner_user_id,
            file_version_id,
        )
        .await
        .map(DownloadMetadata::from_content)
    }

    async fn open_current_content(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
    ) -> Result<DownloadRead, ContentReadError> {
        ContentReadApplicationService::open_current_file_content(self, owner_user_id, node_id)
            .await
            .map(DownloadRead::from_content)
    }

    async fn open_current_content_range(
        &self,
        owner_user_id: UserId,
        node_id: NodeId,
        range: ByteRange,
    ) -> Result<DownloadRead, ContentReadError> {
        ContentReadApplicationService::open_file_content_range(self, owner_user_id, node_id, range)
            .await
            .map(DownloadRead::from_content)
    }

    async fn open_historical_content(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
    ) -> Result<DownloadRead, ContentReadError> {
        ContentReadApplicationService::open_file_version_content(
            self,
            owner_user_id,
            file_version_id,
        )
        .await
        .map(DownloadRead::from_content)
    }

    async fn open_historical_content_range(
        &self,
        owner_user_id: UserId,
        file_version_id: FileVersionId,
        range: ByteRange,
    ) -> Result<DownloadRead, ContentReadError> {
        ContentReadApplicationService::open_file_version_content_range(
            self,
            owner_user_id,
            file_version_id,
            range,
        )
        .await
        .map(DownloadRead::from_content)
    }
}

/// Fail-closed download backend for a composition root without a configured
/// PostgreSQL metadata pool and verified object store.
pub(crate) struct UnavailableDownloadBackend;

#[async_trait]
impl DownloadBackend for UnavailableDownloadBackend {
    fn supports_range_reads(&self) -> bool {
        false
    }

    async fn current_content_metadata(
        &self,
        _owner_user_id: UserId,
        _node_id: NodeId,
    ) -> Result<DownloadMetadata, ContentReadError> {
        Err(ContentReadError::StorageUnavailable)
    }

    async fn historical_content_metadata(
        &self,
        _owner_user_id: UserId,
        _file_version_id: FileVersionId,
    ) -> Result<DownloadMetadata, ContentReadError> {
        Err(ContentReadError::StorageUnavailable)
    }

    async fn open_current_content(
        &self,
        _owner_user_id: UserId,
        _node_id: NodeId,
    ) -> Result<DownloadRead, ContentReadError> {
        Err(ContentReadError::StorageUnavailable)
    }

    async fn open_current_content_range(
        &self,
        _owner_user_id: UserId,
        _node_id: NodeId,
        _range: ByteRange,
    ) -> Result<DownloadRead, ContentReadError> {
        Err(ContentReadError::StorageUnavailable)
    }

    async fn open_historical_content(
        &self,
        _owner_user_id: UserId,
        _file_version_id: FileVersionId,
    ) -> Result<DownloadRead, ContentReadError> {
        Err(ContentReadError::StorageUnavailable)
    }

    async fn open_historical_content_range(
        &self,
        _owner_user_id: UserId,
        _file_version_id: FileVersionId,
        _range: ByteRange,
    ) -> Result<DownloadRead, ContentReadError> {
        Err(ContentReadError::StorageUnavailable)
    }
}

/// Safe content metadata used by the HTTP transport and tests. The body is
/// intentionally kept separate so a preflight can finish without opening it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DownloadMetadata {
    node_id: NodeId,
    file_version_id: FileVersionId,
    length: u64,
    sha256: Sha256Digest,
}

impl DownloadMetadata {
    pub const fn new(
        node_id: NodeId,
        file_version_id: FileVersionId,
        length: u64,
        sha256: Sha256Digest,
    ) -> Self {
        Self {
            node_id,
            file_version_id,
            length,
            sha256,
        }
    }

    fn from_content(metadata: ContentMetadata) -> Self {
        Self::new(
            metadata.node_id(),
            metadata.file_version_id(),
            metadata.length(),
            metadata.sha256(),
        )
    }

    #[must_use]
    pub const fn node_id(self) -> NodeId {
        self.node_id
    }

    #[must_use]
    pub const fn file_version_id(self) -> FileVersionId {
        self.file_version_id
    }

    #[must_use]
    pub const fn length(self) -> u64 {
        self.length
    }

    #[must_use]
    pub const fn sha256(self) -> Sha256Digest {
        self.sha256
    }
}

pub struct DownloadRead {
    metadata: DownloadMetadata,
    body: ContentByteStream,
}

impl DownloadRead {
    pub fn new(metadata: DownloadMetadata, body: ContentByteStream) -> Self {
        Self { metadata, body }
    }

    fn from_content(descriptor: ContentDescriptor) -> Self {
        let metadata = DownloadMetadata::new(
            descriptor.node_id(),
            descriptor.file_version_id(),
            descriptor.length(),
            descriptor.sha256(),
        );
        Self::new(metadata, descriptor.into_stream())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ContentTarget {
    Current(NodeId),
    Historical(FileVersionId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RangeSpec {
    FromTo { start: u64, end_inclusive: u64 },
    From { start: u64 },
    Suffix { length: u64 },
}

pub(crate) async fn current_content(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(node_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let node_id = parse_id::<NodeId>(&node_id)?;
    serve_content(
        state,
        auth.owner_user_id(),
        ContentTarget::Current(node_id),
        &context,
        headers,
    )
    .await
}

pub(crate) async fn historical_content(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthenticatedPrincipal>,
    Extension(context): Extension<RequestContext>,
    Path(file_version_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let file_version_id = parse_id::<FileVersionId>(&file_version_id)?;
    serve_content(
        state,
        auth.owner_user_id(),
        ContentTarget::Historical(file_version_id),
        &context,
        headers,
    )
    .await
}

async fn serve_content(
    state: ApiState,
    owner_user_id: UserId,
    target: ContentTarget,
    _context: &RequestContext,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let backend = Arc::clone(state.download_backend());
    let metadata = match target {
        ContentTarget::Current(node_id) => {
            backend
                .current_content_metadata(owner_user_id, node_id)
                .await
        }
        ContentTarget::Historical(file_version_id) => {
            backend
                .historical_content_metadata(owner_user_id, file_version_id)
                .await
        }
    }
    .map_err(|error| map_content_read_error(error, None))?;

    let etag = content_etag(metadata.sha256());
    // If-None-Match takes precedence over Range. A matching validator is
    // answered from metadata only and never opens the object stream.
    if if_none_match_matches(&headers, &etag) {
        return Ok(not_modified(&etag, backend.supports_range_reads()));
    }

    let range_spec = parse_range_header(&headers).map_err(|_| ApiError::RangeNotSatisfiable {
        length: metadata.length(),
    })?;
    let range = match range_spec {
        Some(spec) => {
            if !backend.supports_range_reads() {
                return Err(ApiError::Core(synveil_core::ErrorCode::StorageUnavailable));
            }
            Some(resolve_range(spec, metadata.length()).map_err(|_| {
                ApiError::RangeNotSatisfiable {
                    length: metadata.length(),
                }
            })?)
        }
        None => None,
    };

    let read = match (target, range) {
        (ContentTarget::Current(node_id), None) => {
            backend.open_current_content(owner_user_id, node_id).await
        }
        (ContentTarget::Current(node_id), Some(range)) => {
            backend
                .open_current_content_range(owner_user_id, node_id, range)
                .await
        }
        (ContentTarget::Historical(file_version_id), None) => {
            backend
                .open_historical_content(owner_user_id, file_version_id)
                .await
        }
        (ContentTarget::Historical(file_version_id), Some(range)) => {
            backend
                .open_historical_content_range(owner_user_id, file_version_id, range)
                .await
        }
    }
    .map_err(|error| map_content_read_error(error, Some(metadata.length())))?;

    // The preflight and stream must refer to the same immutable version. A
    // current node can change between the two service calls; fail closed rather
    // than emitting a body with a validator or range for different bytes.
    if read.metadata != metadata {
        return Err(ApiError::Internal);
    }

    let filename = safe_filename(&state, owner_user_id, read.metadata.node_id()).await;
    Ok(content_response(
        read,
        range,
        backend.supports_range_reads(),
        filename.as_deref(),
    ))
}

async fn safe_filename(state: &ApiState, owner_user_id: UserId, node_id: NodeId) -> Option<String> {
    match state
        .file_metadata_backend()
        .get_node(owner_user_id, node_id)
        .await
    {
        Ok(node) => Some(node.name().as_str().to_owned()),
        Err(
            FileMetadataError::NotFound
            | FileMetadataError::PermissionDenied
            | FileMetadataError::InvalidRequest
            | FileMetadataError::InvalidCursor
            | FileMetadataError::VersionConflict { .. }
            | FileMetadataError::InvalidState
            | FileMetadataError::Database(_)
            | FileMetadataError::InvalidPersistedData,
        ) => None,
    }
}

fn content_response(
    read: DownloadRead,
    range: Option<ByteRange>,
    supports_range_reads: bool,
    filename: Option<&str>,
) -> Response {
    let metadata = read.metadata;
    let body = read.body;
    let status = range.map_or(StatusCode::OK, |_| StatusCode::PARTIAL_CONTENT);
    let stream_length = range.map_or(metadata.length(), ByteRange::length);
    let mut response = (status, Body::from_stream(body)).into_response();
    let response_headers = response.headers_mut();
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response_headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&stream_length.to_string())
            .expect("content length is valid HTTP header data"),
    );
    response_headers.insert(
        header::ETAG,
        HeaderValue::from_str(&content_etag(metadata.sha256()))
            .expect("content ETag is valid HTTP header data"),
    );
    response_headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response_headers.insert(header::CONTENT_DISPOSITION, content_disposition(filename));
    if supports_range_reads {
        response_headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    }
    if let Some(range) = range {
        response_headers.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!(
                "bytes {}-{}/{}",
                range.start(),
                range.end_exclusive() - 1,
                metadata.length()
            ))
            .expect("content range is valid HTTP header data"),
        );
    }
    response
}

fn not_modified(etag: &str, supports_range_reads: bool) -> Response {
    let mut response = StatusCode::NOT_MODIFIED.into_response();
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(etag).expect("content ETag is valid HTTP header data"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    if supports_range_reads {
        response
            .headers_mut()
            .insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    }
    response
}

fn content_etag(sha256: Sha256Digest) -> String {
    format!("\"{sha256}\"")
}

fn content_disposition(filename: Option<&str>) -> HeaderValue {
    let mut value = String::from("attachment; filename=\"download\"");
    if let Some(filename) = filename {
        value.push_str("; filename*=UTF-8''");
        append_rfc5987_filename(&mut value, filename);
    }
    HeaderValue::from_str(&value).expect("encoded content disposition is header-safe")
}

fn append_rfc5987_filename(output: &mut String, filename: &str) {
    for byte in filename.bytes() {
        if is_rfc5987_attr_char(byte) {
            output.push(byte as char);
        } else {
            output.push('%');
            output.push(hex_digit(byte >> 4));
            output.push(hex_digit(byte & 0x0f));
        }
    }
}

fn is_rfc5987_attr_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&byte)
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'A' + value - 10) as char,
        _ => unreachable!("hex nibble is bounded"),
    }
}

fn parse_range_header(headers: &HeaderMap) -> Result<Option<RangeSpec>, ()> {
    let mut values = headers.get_all(header::RANGE).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(());
    }
    let value = value.to_str().map_err(|_| ())?;
    let (unit, ranges) = value.split_once('=').ok_or(())?;
    if !unit.eq_ignore_ascii_case("bytes") {
        return Err(());
    }
    let mut ranges = ranges.split(',');
    let range = ranges.next().ok_or(())?.trim();
    if range.is_empty() || ranges.next().is_some() {
        return Err(());
    }
    let (start, end) = range.split_once('-').ok_or(())?;
    if start.is_empty() {
        let length = parse_unsigned_decimal(end)?;
        if length == 0 {
            return Err(());
        }
        return Ok(Some(RangeSpec::Suffix { length }));
    }
    let start = parse_unsigned_decimal(start)?;
    if end.is_empty() {
        return Ok(Some(RangeSpec::From { start }));
    }
    let end_inclusive = parse_unsigned_decimal(end)?;
    if start > end_inclusive {
        return Err(());
    }
    Ok(Some(RangeSpec::FromTo {
        start,
        end_inclusive,
    }))
}

fn resolve_range(spec: RangeSpec, length: u64) -> Result<ByteRange, ()> {
    if length == 0 {
        return Err(());
    }
    match spec {
        RangeSpec::FromTo {
            start,
            end_inclusive,
        } => {
            if start >= length {
                return Err(());
            }
            let end_inclusive = end_inclusive.min(length - 1);
            ByteRange::new(start, end_inclusive.checked_add(1).ok_or(())?).map_err(|_| ())
        }
        RangeSpec::From { start } => {
            if start >= length {
                return Err(());
            }
            ByteRange::new(start, length).map_err(|_| ())
        }
        RangeSpec::Suffix { length: suffix } => {
            ByteRange::new(length.saturating_sub(suffix), length).map_err(|_| ())
        }
    }
}

fn parse_unsigned_decimal(value: &str) -> Result<u64, ()> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(());
    }
    value.parse().map_err(|_| ())
}

fn if_none_match_matches(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .any(|candidate| {
            candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate).trim() == etag
        })
}

fn parse_id<T>(value: &str) -> Result<T, ApiError>
where
    T: std::str::FromStr,
{
    value.parse().map_err(|_| ApiError::InvalidRequest)
}

#[cfg(test)]
mod tests {
    use super::{RangeSpec, append_rfc5987_filename, parse_range_header, resolve_range};
    use axum::http::{HeaderMap, HeaderValue, header};
    use synveil_storage::ByteRange;

    fn headers(value: &str) -> HeaderMap {
        HeaderMap::from_iter([(
            header::RANGE,
            HeaderValue::from_str(value).expect("test range header is valid"),
        )])
    }

    #[test]
    fn parses_the_three_supported_single_range_forms() {
        assert_eq!(
            parse_range_header(&headers("bytes=2-5")),
            Ok(Some(RangeSpec::FromTo {
                start: 2,
                end_inclusive: 5,
            }))
        );
        assert_eq!(
            parse_range_header(&headers("bytes=2-")),
            Ok(Some(RangeSpec::From { start: 2 }))
        );
        assert_eq!(
            parse_range_header(&headers("bytes=-5")),
            Ok(Some(RangeSpec::Suffix { length: 5 }))
        );
    }

    #[test]
    fn rejects_ambiguous_or_invalid_ranges() {
        for value in [
            "items=0-1",
            "bytes=",
            "bytes=0-1,2-3",
            "bytes=0-1,",
            "bytes=1-0",
            "bytes=-0",
            "bytes=--1",
            "bytes=0--1",
            "bytes=abc-1",
            "bytes=0-18446744073709551616",
        ] {
            assert_eq!(parse_range_header(&headers(value)), Err(()), "{value}");
        }
    }

    #[test]
    fn resolves_and_clamps_ranges_without_floating_point_math() {
        assert_eq!(
            resolve_range(
                RangeSpec::FromTo {
                    start: 2,
                    end_inclusive: 99,
                },
                10,
            ),
            ByteRange::new(2, 10).map_err(|_| ())
        );
        assert_eq!(
            resolve_range(RangeSpec::Suffix { length: 4 }, 10),
            ByteRange::new(6, 10).map_err(|_| ())
        );
        assert_eq!(resolve_range(RangeSpec::From { start: 10 }, 10), Err(()));
    }

    #[test]
    fn filename_encoding_cannot_inject_header_bytes() {
        let mut encoded = String::from("UTF-8''");
        append_rfc5987_filename(&mut encoded, "ảnh\r\nname/report.txt");
        assert_eq!(encoded, "UTF-8''%E1%BA%A3nh%0D%0Aname%2Freport.txt");
        assert!(!encoded.contains('\r'));
        assert!(!encoded.contains('\n'));
    }
}
