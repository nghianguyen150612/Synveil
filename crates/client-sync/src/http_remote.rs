//! Verified-TLS production transport for the inbound-only desktop engine.
//!
//! Each transport owns one immutable profile and one profile-bound credential.
//! There is no caller-supplied URL, cookie jar, proxy, redirect follower, TLS
//! override, or automatic retry. Error values contain only closed safe kinds.

use std::{fmt, pin::Pin, time::Duration};

use async_trait::async_trait;
use bytes::Bytes;
use futures_core::Stream;
use futures_util::{StreamExt, stream};
use reqwest::{
    Client, Method, RequestBuilder, Response, StatusCode,
    header::{self, HeaderValue},
};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use synveil_core::{
    ChangeEvent, ChangeKind, ClientMutation, ClientMutationId, ClientMutationRequest,
    DeviceCredentialId, DeviceCredentialSecret, DeviceId, EnrollmentSecret, FileVersionId,
    LogicalName, LogicalSnapshotNode, NodeId, NodeKind, NodeState, OutboundIntentId, Sequence,
    Sha256Digest, SyncBootstrap, SyncBootstrapId, SyncBootstrapState, Timestamp, UploadSessionId,
    UserId,
};
use url::Url;
use zeroize::Zeroizing;

use crate::{
    BootstrapCompletion, BootstrapPage, ContentByteStream, InboundChange, LoadedDeviceCredential,
    MAX_CONTENT_CHUNK_BYTES, MAX_PAGE_ITEMS, OpaqueEvidence, RemoteCheckpoint, RemoteContent,
    RemoteError, RemoteErrorKind, RemoteFeedPage, RemoteMutationApplied, RemoteMutationConflict,
    RemoteMutationOutcome, ReplicaScope, ServerProfile, ServerProfileId, SyncRemote,
    UploadCompletion, UploadSessionStatus, UploadTarget,
};

mod wire;

#[cfg(test)]
mod tests;

const MAX_JSON_BYTES: usize = 8 * 1024 * 1024;
const MAX_ERROR_BYTES: usize = 64 * 1024;
// Reviewed /api/v1 change-feed bound; snapshots independently allow 1,000.
const MAX_FEED_ITEMS: u32 = 500;
const ACK_TOKEN_BYTES: usize = 256;
const CURSOR_BYTES: usize = 320;
const COMPLETION_TOKEN_BYTES: usize = 336;
const USER_AGENT: &str = concat!("SynveilDesktop/", env!("CARGO_PKG_VERSION"));

/// Finite budgets, not security-policy escape hatches. Every value is checked
/// during construction; no public configuration can disable certificate
/// verification, enable redirects/cookies, or select an arbitrary base path.
#[derive(Clone, Copy, Debug)]
pub struct HttpClientConfig {
    pub connect_timeout: Duration,
    pub metadata_timeout: Duration,
    pub header_timeout: Duration,
    pub stream_idle_timeout: Duration,
    pub download_timeout: Duration,
    pub max_metadata_bytes: usize,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            metadata_timeout: Duration::from_secs(30),
            header_timeout: Duration::from_secs(20),
            stream_idle_timeout: Duration::from_secs(30),
            download_timeout: Duration::from_secs(60 * 60),
            max_metadata_bytes: MAX_JSON_BYTES,
        }
    }
}

impl HttpClientConfig {
    fn validate(self) -> Result<Self, RemoteError> {
        for duration in [
            self.connect_timeout,
            self.metadata_timeout,
            self.header_timeout,
            self.stream_idle_timeout,
            self.download_timeout,
        ] {
            if duration.is_zero() || duration > Duration::from_secs(24 * 60 * 60) {
                return Err(RemoteError::new(RemoteErrorKind::Rejected));
            }
        }
        if self.max_metadata_bytes == 0 || self.max_metadata_bytes > MAX_JSON_BYTES {
            return Err(RemoteError::new(RemoteErrorKind::Rejected));
        }
        Ok(self)
    }
}

/// Safe UI/runtime state. Auth and transport failures never delete or reset
/// local files, applied sequence, pending acknowledgement, or local issues.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionHealth {
    Online,
    AuthRequired,
    DeviceRevoked,
    ServerUnavailable,
    TlsError,
    ProtocolError,
}

impl ConnectionHealth {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Online => "ONLINE",
            Self::AuthRequired => "AUTH_REQUIRED",
            Self::DeviceRevoked => "DEVICE_REVOKED",
            Self::ServerUnavailable => "SERVER_UNAVAILABLE",
            Self::TlsError => "TLS_ERROR",
            Self::ProtocolError => "PROTOCOL_ERROR",
        }
    }

    fn from_error(error: RemoteError) -> Self {
        match error.kind() {
            RemoteErrorKind::AuthRequired
            | RemoteErrorKind::Forbidden
            | RemoteErrorKind::NotFound => Self::AuthRequired,
            RemoteErrorKind::DeviceRevoked => Self::DeviceRevoked,
            RemoteErrorKind::Offline
            | RemoteErrorKind::Unavailable
            | RemoteErrorKind::Internal
            | RemoteErrorKind::Timeout
            | RemoteErrorKind::RateLimited => Self::ServerUnavailable,
            RemoteErrorKind::Tls => Self::TlsError,
            // Rebaseline is an authenticated successful connection requiring
            // local synchronization work, not an unavailable connection.
            RemoteErrorKind::RebaselineRequired => Self::Online,
            _ => Self::ProtocolError,
        }
    }
}

struct Transport {
    profile: ServerProfile,
    config: HttpClientConfig,
    client: Client,
}

impl Transport {
    fn new(profile: ServerProfile, config: HttpClientConfig) -> Result<Self, RemoteError> {
        let config = config.validate()?;
        // The typed base URL is the only HTTP exception boundary: its explicit
        // development constructor accepts literal loopback addresses only.
        let production = profile.base_url().as_url().scheme() == "https";
        let client = Client::builder()
            .https_only(production)
            .use_rustls_tls()
            .min_tls_version(reqwest::tls::Version::TLS_1_2)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .referer(false)
            .connection_verbose(false)
            .connect_timeout(config.connect_timeout)
            .read_timeout(config.stream_idle_timeout)
            .timeout(config.metadata_timeout)
            .pool_idle_timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(2)
            .user_agent(USER_AGENT)
            .build()
            .map_err(map_network_error)?;
        Ok(Self {
            profile,
            config,
            client,
        })
    }

    fn url(&self, segments: &[&str]) -> Result<Url, RemoteError> {
        let mut url = self.profile.base_url().as_url().clone();
        url.path_segments_mut()
            .map_err(|_| protocol_error())?
            .clear()
            .extend(segments);
        Ok(url)
    }

    fn request(&self, method: Method, url: Url) -> RequestBuilder {
        self.client
            .request(method, url)
            .header(header::ACCEPT, "application/json")
            .header(header::ACCEPT_ENCODING, "identity")
    }

    async fn send(&self, request: RequestBuilder) -> Result<Response, RemoteError> {
        let response = tokio::time::timeout(self.config.header_timeout, request.send())
            .await
            .map_err(|_| RemoteError::new(RemoteErrorKind::Timeout))?
            .map_err(map_network_error)?;
        if response.url().origin() != self.profile.base_url().as_url().origin() {
            return Err(protocol_error());
        }
        if response.status().is_redirection() {
            return Err(RemoteError::new(RemoteErrorKind::Redirect));
        }
        Ok(response)
    }

    async fn json<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
        expected: StatusCode,
    ) -> Result<T, RemoteError> {
        let response = self.send(request).await?;
        self.json_response(response, expected).await
    }

    async fn json_response<T: DeserializeOwned>(
        &self,
        response: Response,
        expected: StatusCode,
    ) -> Result<T, RemoteError> {
        let status = response.status();
        if status != expected {
            return Err(self.error_response(response).await);
        }
        validate_content_type(&response, "application/json")?;
        let bytes = bounded_body(response, self.config.max_metadata_bytes).await?;
        serde_json::from_slice(&bytes).map_err(|_| protocol_error())
    }

    async fn error_response(&self, response: Response) -> RemoteError {
        let status = response.status();
        if let Err(error) = validate_content_type(&response, "application/json") {
            return error;
        }
        let bytes = match bounded_body(
            response,
            self.config.max_metadata_bytes.min(MAX_ERROR_BYTES),
        )
        .await
        {
            Ok(bytes) => bytes,
            Err(error) => return error,
        };
        let envelope: wire::ErrorEnvelope = match serde_json::from_slice(&bytes) {
            Ok(envelope) => envelope,
            Err(_) => return protocol_error(),
        };
        let error = envelope.error;
        if wire::request_id(&error.request_id).is_err()
            || error.message.is_empty()
            || error.message.len() > 4096
            || error.code.len() > 128
        {
            return protocol_error();
        }
        // Never trust a remote retryable flag as permission to replay a
        // one-time request, and never surface server message/details strings.
        let _ = (error.retryable, error.details);
        map_api_error(status, &error.code)
    }
}

/// Production implementation of Prompt 36's exact transport-neutral port.
pub struct HttpSyncRemote {
    transport: Transport,
    credential: LoadedDeviceCredential,
    authorization: HeaderValue,
}

impl fmt::Debug for HttpSyncRemote {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpSyncRemote")
            .field("profile_id", &self.transport.profile.profile_id())
            .field("device_id", &self.credential.device_id())
            .finish_non_exhaustive()
    }
}

impl HttpSyncRemote {
    pub fn new(
        profile: ServerProfile,
        device_id: DeviceId,
        credential: LoadedDeviceCredential,
        config: HttpClientConfig,
    ) -> Result<Self, RemoteError> {
        if credential.profile_id() != profile.profile_id()
            || credential.base_url() != profile.base_url()
            || credential.device_id() != device_id
        {
            return Err(RemoteError::new(RemoteErrorKind::Rejected));
        }
        let bearer = Zeroizing::new(format!("Bearer {}", credential.secret().expose_secret()));
        let mut authorization = HeaderValue::from_str(&bearer).map_err(|_| protocol_error())?;
        authorization.set_sensitive(true);
        Ok(Self {
            transport: Transport::new(profile, config)?,
            credential,
            authorization,
        })
    }

    fn validate_scope(&self, scope: ReplicaScope) -> Result<(), RemoteError> {
        if scope.owner_user_id() != self.credential.owner_user_id()
            || scope.device_id() != self.credential.device_id()
        {
            return Err(RemoteError::new(RemoteErrorKind::Forbidden));
        }
        Ok(())
    }

    fn request(&self, method: Method, url: Url) -> RequestBuilder {
        // Authorization is per-request, never a client-global default. Only
        // URLs built from this transport's immutable profile enter here.
        self.transport
            .request(method, url)
            .header(header::AUTHORIZATION, self.authorization.clone())
    }

    fn scoped_url(&self, scope: ReplicaScope, tail: &[&str]) -> Result<Url, RemoteError> {
        self.validate_scope(scope)?;
        let device = scope.device_id().to_string();
        let library = scope.library_id().to_string();
        let mut path = vec!["api", "v1", "devices", &device, "libraries", &library];
        path.extend_from_slice(tail);
        self.transport.url(&path)
    }

    async fn node(&self, scope: ReplicaScope, node_id: NodeId) -> Result<wire::Node, RemoteError> {
        self.validate_scope(scope)?;
        let url = self
            .transport
            .url(&["api", "v1", "nodes", &node_id.to_string()])?;
        let result: wire::Envelope<wire::Node> = self
            .transport
            .json(self.request(Method::GET, url), StatusCode::OK)
            .await?;
        let node = result.data()?;
        node.validate(scope, node_id)?;
        Ok(node)
    }

    async fn version(
        &self,
        node_id: NodeId,
        version_id: FileVersionId,
    ) -> Result<(u64, Sha256Digest), RemoteError> {
        let url = self
            .transport
            .url(&["api", "v1", "versions", &version_id.to_string()])?;
        let result: wire::Envelope<wire::Version> = self
            .transport
            .json(self.request(Method::GET, url), StatusCode::OK)
            .await?;
        result.data()?.validate(node_id, version_id)
    }

    async fn desired_node(
        &self,
        scope: ReplicaScope,
        event: ChangeEvent,
    ) -> Result<Option<LogicalSnapshotNode>, RemoteError> {
        if event.change_kind() == ChangeKind::NodePurged {
            return Ok(None);
        }
        let node = match self.node(scope, event.resource_id()).await {
            Err(error) if error.kind() == RemoteErrorKind::NotFound => {
                return Err(RemoteError::new(RemoteErrorKind::RebaselineRequired));
            }
            result => result?,
        };
        let revision = wire::parse(&node.revision)?;
        // Journal events are compact facts, not historical full Node images.
        // If the current image moved past this event, guessing an old name or
        // replaying the latest image under an old revision is unsafe.
        if revision != event.resource_revision() {
            return Err(RemoteError::new(RemoteErrorKind::RebaselineRequired));
        }
        let attrs = node.attributes;
        let parent = wire::optional(attrs.parent_id.as_deref())?;
        let kind = wire::node_kind(&attrs.kind)?;
        let state = wire::node_state(&attrs.state)?;
        let version = wire::optional::<FileVersionId>(attrs.current_version_id.as_deref())?;
        if parent != event.parent_node_id()
            || event.node_kind() != Some(kind)
            || event.node_state() != Some(state)
            || event.current_version_id() != version
        {
            return Err(protocol_error());
        }
        let (length, hash) = match version {
            Some(version) => {
                let (length, hash) = self.version(event.resource_id(), version).await?;
                (Some(length), Some(hash))
            }
            None => (None, None),
        };
        let desired = LogicalSnapshotNode::new(
            event.resource_id(),
            parent,
            LogicalName::new(attrs.name).map_err(|_| protocol_error())?,
            kind,
            state,
            revision,
            version,
            length,
            hash,
        )
        .map_err(|_| protocol_error())?;
        match event.change_kind() {
            ChangeKind::NodeTrashed if state != NodeState::Trashed => return Err(protocol_error()),
            ChangeKind::NodeRestored if state != NodeState::Active => return Err(protocol_error()),
            ChangeKind::FileContentCommitted | ChangeKind::FileVersionRestored
                if kind != NodeKind::File || state != NodeState::Active || version.is_none() =>
            {
                return Err(protocol_error());
            }
            _ => {}
        }
        Ok(Some(desired))
    }

    /// Readiness is anonymous and strictly typed. A second, scoped checkpoint
    /// request establishes the credential's current owner/device authority.
    pub async fn probe_health(&self, scope: ReplicaScope) -> ConnectionHealth {
        match self.probe(scope).await {
            Ok(()) => ConnectionHealth::Online,
            Err(error) => ConnectionHealth::from_error(error),
        }
    }

    async fn probe(&self, scope: ReplicaScope) -> Result<(), RemoteError> {
        self.validate_scope(scope)?;
        let url = self.transport.url(&["health", "ready"])?;
        let ready: wire::Ready = self
            .transport
            .json(self.transport.request(Method::GET, url), StatusCode::OK)
            .await?;
        if ready.status != "ready" {
            return Err(protocol_error());
        }
        self.get_checkpoint(scope).await?;
        Ok(())
    }
}

#[async_trait]
impl SyncRemote for HttpSyncRemote {
    fn server_profile_id(&self) -> Option<ServerProfileId> {
        Some(self.transport.profile.profile_id())
    }

    fn device_credential_id(&self) -> Option<DeviceCredentialId> {
        Some(self.credential.credential_id())
    }

    async fn get_checkpoint(&self, scope: ReplicaScope) -> Result<RemoteCheckpoint, RemoteError> {
        let url = self.scoped_url(scope, &["checkpoint"])?;
        let result: wire::Envelope<wire::Checkpoint> = self
            .transport
            .json(self.request(Method::GET, url), StatusCode::OK)
            .await?;
        result.data()?.into_domain(scope)
    }

    async fn fetch_changes(
        &self,
        scope: ReplicaScope,
        limit: u32,
    ) -> Result<RemoteFeedPage, RemoteError> {
        validate_limit(limit, MAX_FEED_ITEMS)?;
        let mut url = self.scoped_url(scope, &["changes"])?;
        url.query_pairs_mut()
            .append_pair("limit", &limit.to_string());
        let result: wire::Envelope<wire::Feed> = self
            .transport
            .json(self.request(Method::GET, url), StatusCode::OK)
            .await?;
        let page = result.data()?;
        wire::validate_scope(scope, &page.device_id, &page.library_id)?;
        let epoch = wire::parse::<Sequence>(&page.epoch)?;
        let from = wire::parse::<Sequence>(&page.from_sequence)?;
        let through = wire::parse::<Sequence>(&page.through_sequence)?;
        let high = wire::parse::<Sequence>(&page.high_watermark)?;
        let evidence = wire::evidence(page.ack_token, ACK_TOKEN_BYTES)?;
        if epoch.get() == 0
            || page.changes.len() > limit as usize
            || from > through
            || through > high
            || page.has_more != (through < high)
            || through.get() - from.get() != page.changes.len() as u64
            || page.changes.is_empty() == evidence.is_some()
        {
            return Err(protocol_error());
        }
        let mut changes = Vec::with_capacity(page.changes.len());
        for (index, change) in page.changes.into_iter().enumerate() {
            let event = change.into_domain(scope, epoch)?;
            if event.sequence().get() != from.get() + index as u64 + 1 {
                return Err(protocol_error());
            }
            let desired = self.desired_node(scope, event).await?;
            changes.push(InboundChange::new(event, desired));
        }
        RemoteFeedPage::new(
            scope,
            epoch,
            from,
            through,
            high,
            page.has_more,
            changes,
            evidence,
        )
        .map_err(|_| protocol_error())
    }

    async fn acknowledge_changes(
        &self,
        scope: ReplicaScope,
        evidence: &OpaqueEvidence,
    ) -> Result<RemoteCheckpoint, RemoteError> {
        #[derive(Serialize)]
        struct Ack<'a> {
            ack_token: &'a str,
        }
        let url = self.scoped_url(scope, &["changes", "ack"])?;
        let payload = Ack {
            ack_token: evidence_text(evidence, ACK_TOKEN_BYTES)?,
        };
        let result: wire::Envelope<wire::Checkpoint> = self
            .transport
            .json(
                self.request(Method::POST, url).json(&payload),
                StatusCode::OK,
            )
            .await?;
        result.data()?.into_domain(scope)
    }

    async fn start_rebaseline(&self, scope: ReplicaScope) -> Result<SyncBootstrap, RemoteError> {
        let url = self.scoped_url(scope, &["rebaseline"])?;
        let result: wire::Envelope<wire::Bootstrap> = self
            .transport
            .json(
                self.request(Method::POST, url).json(&serde_json::json!({})),
                StatusCode::OK,
            )
            .await?;
        let bootstrap = result.data()?.into_domain(scope)?;
        if bootstrap.state() != SyncBootstrapState::Open {
            return Err(protocol_error());
        }
        Ok(bootstrap)
    }

    async fn fetch_rebaseline_page(
        &self,
        scope: ReplicaScope,
        bootstrap_id: SyncBootstrapId,
        cursor: Option<&OpaqueEvidence>,
        limit: u32,
    ) -> Result<BootstrapPage, RemoteError> {
        validate_limit(limit, MAX_PAGE_ITEMS as u32)?;
        let id = bootstrap_id.to_string();
        let mut url = self.scoped_url(scope, &["rebaseline", &id, "nodes"])?;
        url.query_pairs_mut()
            .append_pair("limit", &limit.to_string());
        if let Some(cursor) = cursor {
            url.query_pairs_mut()
                .append_pair("cursor", evidence_text(cursor, CURSOR_BYTES)?);
        }
        let result: wire::Envelope<wire::SnapshotPage> = self
            .transport
            .json(self.request(Method::GET, url), StatusCode::OK)
            .await?;
        let page = result.data()?;
        let bootstrap = page.bootstrap.into_domain(scope)?;
        let cursor = wire::evidence(page.next_cursor, CURSOR_BYTES)?;
        let completion = wire::evidence(page.completion_token, COMPLETION_TOKEN_BYTES)?;
        if bootstrap.id() != bootstrap_id
            || bootstrap.state() != SyncBootstrapState::Open
            || page.nodes.len() > limit as usize
            || page.nodes.len() as u64 > bootstrap.manifest_item_count()
            || page.has_more != cursor.is_some()
            || page.has_more == completion.is_some()
            || (page.has_more && page.nodes.is_empty())
        {
            return Err(protocol_error());
        }
        let nodes = page
            .nodes
            .into_iter()
            .map(wire::SnapshotNode::into_domain)
            .collect::<Result<Vec<_>, _>>()?;
        if nodes
            .windows(2)
            .any(|pair| pair[0].node_id() >= pair[1].node_id())
        {
            return Err(protocol_error());
        }
        BootstrapPage::new(bootstrap, nodes, page.has_more, cursor, completion)
            .map_err(|_| protocol_error())
    }

    async fn complete_rebaseline(
        &self,
        scope: ReplicaScope,
        bootstrap_id: SyncBootstrapId,
        evidence: &OpaqueEvidence,
    ) -> Result<BootstrapCompletion, RemoteError> {
        #[derive(Serialize)]
        struct Complete<'a> {
            completion_token: &'a str,
        }
        let id = bootstrap_id.to_string();
        let url = self.scoped_url(scope, &["rebaseline", &id, "complete"])?;
        let payload = Complete {
            completion_token: evidence_text(evidence, COMPLETION_TOKEN_BYTES)?,
        };
        let result: wire::Envelope<wire::Completion> = self
            .transport
            .json(
                self.request(Method::POST, url).json(&payload),
                StatusCode::OK,
            )
            .await?;
        let completion = result.data()?;
        let bootstrap = completion.bootstrap.into_domain(scope)?;
        let epoch = wire::parse(&completion.checkpoint.journal_epoch)?;
        let acknowledged = wire::parse(&completion.checkpoint.acknowledged_sequence)?;
        let updated = wire::parse::<Timestamp>(&completion.checkpoint.updated_at)?;
        let _ = completion.replayed;
        if bootstrap.id() != bootstrap_id
            || bootstrap.state() != SyncBootstrapState::Completed
            || epoch != bootstrap.snapshot_epoch()
            || acknowledged != bootstrap.snapshot_resume_sequence()
            || updated < bootstrap.created_at()
        {
            return Err(protocol_error());
        }
        Ok(BootstrapCompletion::new(
            bootstrap_id,
            RemoteCheckpoint::new(scope, epoch, acknowledged),
        ))
    }

    async fn download_current_content(
        &self,
        scope: ReplicaScope,
        node_id: NodeId,
        version_id: FileVersionId,
    ) -> Result<RemoteContent, RemoteError> {
        // Node lookup checks library scope; immutable version metadata checks
        // node/version identity. The existing version-content route reuses
        // the canonical logical download service (no ObjectStore duplication).
        let node = self.node(scope, node_id).await?;
        if wire::node_kind(&node.attributes.kind)? != NodeKind::File {
            return Err(protocol_error());
        }
        let (length, hash) = self.version(node_id, version_id).await?;
        let url =
            self.transport
                .url(&["api", "v1", "versions", &version_id.to_string(), "content"])?;
        let request = self
            .request(Method::GET, url)
            .header(header::ACCEPT, "application/octet-stream")
            .timeout(self.transport.config.download_timeout);
        let response = self.transport.send(request).await?;
        if response.status() != StatusCode::OK {
            return Err(self.transport.error_response(response).await);
        }
        validate_content_type(&response, "application/octet-stream")?;
        if response.headers().contains_key(header::CONTENT_RANGE)
            || response
                .content_length()
                .is_some_and(|actual| actual != length)
            || response
                .headers()
                .get(header::ETAG)
                .and_then(|header| header.to_str().ok())
                != Some(format!("\"{hash}\"").as_str())
        {
            return Err(RemoteError::new(RemoteErrorKind::Integrity));
        }
        Ok(RemoteContent::new(
            node_id,
            version_id,
            length,
            hash,
            bounded_stream(
                response,
                length,
                hash,
                self.transport.config.stream_idle_timeout,
            ),
        ))
    }

    async fn submit_client_mutation(
        &self,
        scope: ReplicaScope,
        request: &ClientMutationRequest,
    ) -> Result<RemoteMutationOutcome, RemoteError> {
        let url = self.scoped_url(scope, &["mutations"])?;
        let response = self
            .transport
            .send(
                self.request(Method::POST, url)
                    .json(&mutation_payload(request)),
            )
            .await?;
        if response.status() == StatusCode::CONFLICT {
            let envelope = conflict_error(&self.transport, response).await?;
            let details = envelope.error.details.ok_or_else(protocol_error)?;
            let conflict_id = details
                .get("conflict_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(protocol_error)
                .and_then(wire::parse)?;
            let reason = details
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(protocol_error)?;
            let replayed = details
                .get("replayed")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            return Ok(RemoteMutationOutcome::Conflict(
                RemoteMutationConflict::new(conflict_id, reason.to_owned(), replayed)
                    .map_err(|_| protocol_error())?,
            ));
        }
        if response.status() != StatusCode::OK {
            return Err(self.transport.error_response(response).await);
        }
        let payload: wire::Envelope<wire::MutationApplied> = self
            .transport
            .json_response(response, StatusCode::OK)
            .await?;
        let data = payload.data()?;
        if data.outcome != "APPLIED"
            || wire::parse::<ClientMutationId>(&data.mutation_id)? != request.mutation_id()
            || data.kind != request.kind().as_str()
            || wire::parse::<synveil_core::LibraryId>(&data.node.library_id)? != scope.library_id()
            || !data.node.validate_projection()
        {
            return Err(protocol_error());
        }
        Ok(RemoteMutationOutcome::Applied(RemoteMutationApplied::new(
            request.mutation_id(),
            wire::parse(&data.node.id)?,
            wire::parse(&data.node.revision)?,
            wire::parse(&data.journal_event_id)?,
            wire::parse(&data.journal_sequence)?,
            data.replayed,
        )))
    }

    async fn create_upload_session(
        &self,
        scope: ReplicaScope,
        idempotency_key: OutboundIntentId,
        target: &UploadTarget,
        expected_length: u64,
        expected_sha256: Sha256Digest,
    ) -> Result<UploadSessionStatus, RemoteError> {
        self.validate_scope(scope)?;
        let url = self.transport.url(&["api", "v1", "upload-sessions"])?;
        let response = self
            .transport
            .send(self.request(Method::POST, url).json(&upload_create_payload(
                idempotency_key,
                target,
                expected_length,
                expected_sha256,
            )))
            .await?;
        if response.status() != StatusCode::CREATED {
            return Err(self.transport.error_response(response).await);
        }
        let result: wire::Envelope<wire::UploadSession> = self
            .transport
            .json_response(response, StatusCode::CREATED)
            .await?;
        result.data()?.into_domain()
    }

    async fn get_upload_session(
        &self,
        scope: ReplicaScope,
        session_id: UploadSessionId,
    ) -> Result<UploadSessionStatus, RemoteError> {
        self.validate_scope(scope)?;
        let url = self
            .transport
            .url(&["api", "v1", "upload-sessions", &session_id.to_string()])?;
        let result: wire::Envelope<wire::UploadSession> = self
            .transport
            .json(self.request(Method::GET, url), StatusCode::OK)
            .await?;
        result.data()?.into_domain()
    }

    async fn append_upload_chunk(
        &self,
        scope: ReplicaScope,
        session_id: UploadSessionId,
        expected_offset: u64,
        chunk: Bytes,
    ) -> Result<u64, RemoteError> {
        self.validate_scope(scope)?;
        if chunk.is_empty() || chunk.len() > MAX_CONTENT_CHUNK_BYTES {
            return Err(RemoteError::new(RemoteErrorKind::BodyLimit));
        }
        let url = self
            .transport
            .url(&["api", "v1", "upload-sessions", &session_id.to_string()])?;
        let response = self
            .transport
            .send(
                self.request(Method::PATCH, url)
                    .header(header::CONTENT_TYPE, "application/octet-stream")
                    .header("upload-offset", expected_offset.to_string())
                    .body(chunk),
            )
            .await?;
        if response.status() != StatusCode::NO_CONTENT {
            return Err(self.transport.error_response(response).await);
        }
        upload_offset(&response)
    }

    async fn complete_upload(
        &self,
        scope: ReplicaScope,
        session_id: UploadSessionId,
    ) -> Result<UploadCompletion, RemoteError> {
        self.validate_scope(scope)?;
        let url = self.transport.url(&[
            "api",
            "v1",
            "upload-sessions",
            &session_id.to_string(),
            "complete",
        ])?;
        let result: wire::Envelope<wire::UploadCompletionResource> = self
            .transport
            .json(
                self.request(Method::POST, url).json(&serde_json::json!({})),
                StatusCode::OK,
            )
            .await?;
        result.data()?.into_domain(session_id)
    }

    async fn abort_upload(
        &self,
        scope: ReplicaScope,
        session_id: UploadSessionId,
    ) -> Result<UploadSessionStatus, RemoteError> {
        self.validate_scope(scope)?;
        let url = self.transport.url(&[
            "api",
            "v1",
            "upload-sessions",
            &session_id.to_string(),
            "abort",
        ])?;
        let result: wire::Envelope<wire::UploadSession> = self
            .transport
            .json(
                self.request(Method::POST, url).json(&serde_json::json!({})),
                StatusCode::OK,
            )
            .await?;
        result.data()?.into_domain()
    }
}

/// Once-only enrollment handoff. Store the secret through LocalStateStore's
/// profile-bound SecretStore lifecycle; never serialize this value to disk.
pub struct EnrollmentCredentials {
    profile: ServerProfile,
    owner_user_id: UserId,
    device_id: DeviceId,
    credential_id: DeviceCredentialId,
    secret: DeviceCredentialSecret,
    created_at: Timestamp,
}

impl EnrollmentCredentials {
    #[must_use]
    pub const fn profile_id(&self) -> ServerProfileId {
        self.profile.profile_id()
    }
    #[must_use]
    pub const fn base_url(&self) -> &crate::CanonicalBaseUrl {
        self.profile.base_url()
    }
    #[must_use]
    pub const fn owner_user_id(&self) -> UserId {
        self.owner_user_id
    }
    #[must_use]
    pub const fn device_id(&self) -> DeviceId {
        self.device_id
    }
    #[must_use]
    pub const fn credential_id(&self) -> DeviceCredentialId {
        self.credential_id
    }
    #[must_use]
    pub const fn secret(&self) -> &DeviceCredentialSecret {
        &self.secret
    }
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        profile: ServerProfile,
        owner_user_id: UserId,
        device_id: DeviceId,
        credential_id: DeviceCredentialId,
        secret: DeviceCredentialSecret,
        created_at: Timestamp,
    ) -> Self {
        Self {
            profile,
            owner_user_id,
            device_id,
            credential_id,
            secret,
            created_at,
        }
    }
}

impl fmt::Debug for EnrollmentCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EnrollmentCredentials")
            .field("profile_id", &self.profile.profile_id())
            .field("owner_user_id", &self.owner_user_id)
            .field("device_id", &self.device_id)
            .field("credential_id", &self.credential_id)
            .field("secret", &"[REDACTED]")
            .finish()
    }
}

/// Anonymous, one-time exchange over the same verified-origin transport.
/// A timeout or lost response MUST NOT be retried: the owner must revoke the
/// uncertain credential/device, issue a fresh grant, and explicitly re-enroll.
pub struct HttpEnrollmentClient {
    transport: Transport,
}

impl fmt::Debug for HttpEnrollmentClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpEnrollmentClient")
            .field("profile_id", &self.transport.profile.profile_id())
            .finish_non_exhaustive()
    }
}

impl HttpEnrollmentClient {
    pub fn new(profile: ServerProfile, config: HttpClientConfig) -> Result<Self, RemoteError> {
        Ok(Self {
            transport: Transport::new(profile, config)?,
        })
    }

    pub async fn exchange(
        &self,
        secret: &EnrollmentSecret,
    ) -> Result<EnrollmentCredentials, RemoteError> {
        #[derive(Serialize)]
        struct Exchange<'a> {
            enrollment_token: &'a str,
        }
        let url = self
            .transport
            .url(&["api", "v1", "device-enrollment", "exchange"])?;
        let payload = Exchange {
            enrollment_token: secret.expose_secret(),
        };
        let response: wire::Envelope<wire::Enrollment> = self
            .transport
            .json(
                self.transport.request(Method::POST, url).json(&payload),
                StatusCode::CREATED,
            )
            .await?;
        response.data()?.into_domain(self.transport.profile.clone())
    }
}

fn validate_limit(limit: u32, maximum: u32) -> Result<(), RemoteError> {
    if limit == 0 || limit > maximum {
        return Err(RemoteError::new(RemoteErrorKind::Rejected));
    }
    Ok(())
}

fn evidence_text(evidence: &OpaqueEvidence, maximum: usize) -> Result<&str, RemoteError> {
    let text = std::str::from_utf8(evidence.as_bytes())
        .map_err(|_| RemoteError::new(RemoteErrorKind::InvalidEvidence))?;
    if text.is_empty() || text.len() > maximum || !text.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(RemoteError::new(RemoteErrorKind::InvalidEvidence));
    }
    Ok(text)
}

#[derive(Serialize)]
struct MutationPayload {
    mutation_id: String,
    base_epoch: String,
    base_sequence: String,
    kind: String,
    payload: serde_json::Value,
}

fn mutation_payload(request: &ClientMutationRequest) -> MutationPayload {
    let payload = match request.mutation() {
        ClientMutation::CreateDirectory {
            parent_node_id,
            expected_parent_revision,
            name,
        } => serde_json::json!({
            "parent_node_id": parent_node_id.to_string(),
            "expected_parent_revision": expected_parent_revision.to_string(),
            "name": name.as_str(),
        }),
        ClientMutation::RenameNode {
            node_id,
            expected_revision,
            new_name,
        } => serde_json::json!({
            "node_id": node_id.to_string(),
            "expected_revision": expected_revision.to_string(),
            "new_name": new_name.as_str(),
        }),
        ClientMutation::MoveNode {
            node_id,
            expected_revision,
            new_parent_node_id,
            expected_new_parent_revision,
        } => serde_json::json!({
            "node_id": node_id.to_string(),
            "expected_revision": expected_revision.to_string(),
            "new_parent_node_id": new_parent_node_id.to_string(),
            "expected_new_parent_revision": expected_new_parent_revision.to_string(),
        }),
        ClientMutation::TrashNode {
            node_id,
            expected_revision,
        } => serde_json::json!({
            "node_id": node_id.to_string(),
            "expected_revision": expected_revision.to_string(),
        }),
        ClientMutation::RestoreNode {
            node_id,
            expected_revision,
            expected_parent_node_id,
            expected_parent_revision,
        } => serde_json::json!({
            "node_id": node_id.to_string(),
            "expected_revision": expected_revision.to_string(),
            "expected_parent_node_id": expected_parent_node_id.to_string(),
            "expected_parent_revision": expected_parent_revision.to_string(),
        }),
    };
    MutationPayload {
        mutation_id: request.mutation_id().to_string(),
        base_epoch: request.base_epoch().to_string(),
        base_sequence: request.base_sequence().to_string(),
        kind: request.kind().as_str().to_owned(),
        payload,
    }
}

#[derive(Serialize)]
#[serde(tag = "operation")]
enum UploadCreatePayload {
    #[serde(rename = "CREATE_FILE")]
    CreateFile {
        idempotency_key: String,
        library_id: String,
        parent_id: String,
        name: String,
        expected_bytes: String,
        expected_sha256: Option<String>,
    },
    #[serde(rename = "REPLACE_CONTENT")]
    ReplaceContent {
        idempotency_key: String,
        library_id: String,
        node_id: String,
        expected_revision: String,
        expected_bytes: String,
        expected_sha256: Option<String>,
    },
}

fn upload_create_payload(
    idempotency_key: OutboundIntentId,
    target: &UploadTarget,
    expected_length: u64,
    expected_sha256: Sha256Digest,
) -> UploadCreatePayload {
    match target {
        UploadTarget::CreateFile {
            library_id,
            parent_node_id,
            name,
        } => UploadCreatePayload::CreateFile {
            idempotency_key: idempotency_key.to_string(),
            library_id: library_id.to_string(),
            parent_id: parent_node_id.to_string(),
            name: name.as_str().to_owned(),
            expected_bytes: expected_length.to_string(),
            expected_sha256: Some(expected_sha256.to_string()),
        },
        UploadTarget::ReplaceContent {
            library_id,
            node_id,
            expected_revision,
        } => UploadCreatePayload::ReplaceContent {
            idempotency_key: idempotency_key.to_string(),
            library_id: library_id.to_string(),
            node_id: node_id.to_string(),
            expected_revision: expected_revision.to_string(),
            expected_bytes: expected_length.to_string(),
            expected_sha256: Some(expected_sha256.to_string()),
        },
    }
}

async fn conflict_error(
    transport: &Transport,
    response: Response,
) -> Result<wire::ErrorEnvelope, RemoteError> {
    validate_content_type(&response, "application/json")?;
    let bytes = bounded_body(
        response,
        transport.config.max_metadata_bytes.min(MAX_ERROR_BYTES),
    )
    .await?;
    serde_json::from_slice(&bytes).map_err(|_| protocol_error())
}

fn upload_offset(response: &Response) -> Result<u64, RemoteError> {
    if response.headers().get_all("upload-offset").iter().count() != 1 {
        return Err(protocol_error());
    }
    let value = response
        .headers()
        .get("upload-offset")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(protocol_error)?;
    wire::decimal(value)
}

fn validate_content_type(response: &Response, expected: &str) -> Result<(), RemoteError> {
    if response
        .headers()
        .get_all(header::CONTENT_TYPE)
        .iter()
        .count()
        != 1
        || response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_none_or(|value| !value.trim().eq_ignore_ascii_case(expected))
        || response
            .headers()
            .get_all(header::CONTENT_ENCODING)
            .iter()
            .count()
            > 1
        || response
            .headers()
            .get(header::CONTENT_ENCODING)
            .is_some_and(|value| value != "identity")
    {
        return Err(protocol_error());
    }
    Ok(())
}

async fn bounded_body(
    response: Response,
    maximum: usize,
) -> Result<Zeroizing<Vec<u8>>, RemoteError> {
    if response
        .content_length()
        .is_some_and(|length| length > maximum as u64)
    {
        return Err(RemoteError::new(RemoteErrorKind::BodyLimit));
    }
    let mut body = response.bytes_stream();
    let mut bytes = Zeroizing::new(Vec::new());
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(map_network_error)?;
        if chunk.len() > maximum.saturating_sub(bytes.len()) {
            return Err(RemoteError::new(RemoteErrorKind::BodyLimit));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

type HttpByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>;

struct DownloadStreamState {
    source: HttpByteStream,
    pending: Bytes,
    length: u64,
    seen: u64,
    expected_hash: Sha256Digest,
    hasher: Sha256,
    idle_timeout: Duration,
    done: bool,
}

fn bounded_stream(
    response: Response,
    length: u64,
    hash: Sha256Digest,
    idle_timeout: Duration,
) -> ContentByteStream {
    let state = DownloadStreamState {
        source: Box::pin(response.bytes_stream()),
        pending: Bytes::new(),
        length,
        seen: 0,
        expected_hash: hash,
        hasher: Sha256::new(),
        idle_timeout,
        done: false,
    };
    Box::pin(stream::unfold(state, |mut state| async move {
        if state.done {
            return None;
        }
        loop {
            if !state.pending.is_empty() {
                let chunk = state
                    .pending
                    .split_to(state.pending.len().min(MAX_CONTENT_CHUNK_BYTES));
                state.seen += chunk.len() as u64;
                state.hasher.update(&chunk);
                return Some((Ok(chunk), state));
            }
            let next = tokio::time::timeout(state.idle_timeout, state.source.next()).await;
            match next {
                Err(_) => {
                    state.done = true;
                    return Some((Err(RemoteError::new(RemoteErrorKind::Timeout)), state));
                }
                Ok(Some(Err(error))) => {
                    state.done = true;
                    return Some((Err(map_network_error(error)), state));
                }
                Ok(Some(Ok(bytes))) => {
                    if bytes.len() as u64 > state.length.saturating_sub(state.seen) {
                        state.done = true;
                        return Some((Err(RemoteError::new(RemoteErrorKind::BodyLimit)), state));
                    }
                    state.pending = bytes;
                }
                Ok(None) => {
                    if state.seen != state.length
                        || Sha256Digest::from_bytes(state.hasher.clone().finalize().into())
                            != state.expected_hash
                    {
                        state.done = true;
                        return Some((Err(RemoteError::new(RemoteErrorKind::Integrity)), state));
                    }
                    return None;
                }
            }
        }
    }))
}

fn protocol_error() -> RemoteError {
    RemoteError::new(RemoteErrorKind::Protocol)
}

fn map_network_error(error: reqwest::Error) -> RemoteError {
    // Inspect typed causes, never match or expose a URL/error/body string.
    if is_tls_failure(&error, 16) {
        return RemoteError::new(RemoteErrorKind::Tls);
    }
    let kind = if error.is_timeout() {
        RemoteErrorKind::Timeout
    } else if error.is_redirect() {
        RemoteErrorKind::Redirect
    } else if error.is_connect() || error.is_body() {
        RemoteErrorKind::Offline
    } else {
        RemoteErrorKind::Protocol
    };
    RemoteError::new(kind)
}

fn is_tls_failure(error: &(dyn std::error::Error + 'static), remaining: u8) -> bool {
    if remaining == 0 {
        return false;
    }
    if error.downcast_ref::<rustls::Error>().is_some() {
        return true;
    }
    // std::io::Error::source can skip the inner error itself and return its
    // source. tokio-rustls stores its typed certificate error in get_ref().
    if error
        .downcast_ref::<std::io::Error>()
        .and_then(std::io::Error::get_ref)
        .is_some_and(|inner| is_tls_failure(inner, remaining - 1))
    {
        return true;
    }
    error
        .source()
        .is_some_and(|source| is_tls_failure(source, remaining - 1))
}

fn map_api_error(status: StatusCode, code: &str) -> RemoteError {
    use RemoteErrorKind as Kind;
    let kind = match (status.as_u16(), code) {
        (401, "authentication_failed" | "device_authentication_failed" | "invalid_enrollment") => {
            Kind::AuthRequired
        }
        (401 | 403 | 404, "device_revoked") => Kind::DeviceRevoked,
        (403, "permission_denied") => Kind::Forbidden,
        (404, "not_found") => Kind::NotFound,
        (409, "sync_rebaseline_required") | (410, "bootstrap_expired") => Kind::RebaselineRequired,
        (409, "checkpoint_conflict" | "bootstrap_conflict") => Kind::CheckpointConflict,
        (400, "invalid_ack_token" | "invalid_bootstrap_token" | "invalid_cursor") => {
            Kind::InvalidEvidence
        }
        (429, "rate_limited") => Kind::RateLimited,
        (
            503,
            "internal_dependency_unavailable" | "dependency_unavailable" | "storage_unavailable",
        ) => Kind::Unavailable,
        (500, "internal_error" | "invalid_persisted_data") => Kind::Internal,
        (413, "payload_too_large" | "upload_payload_too_large") => Kind::BodyLimit,
        (400, "invalid_request" | "invalid_limit" | "invalid_upload_request")
        | (415, "unsupported_media_type" | "unsupported_upload_media_type") => Kind::Rejected,
        (409, "version_conflict" | "upload_conflict" | "upload_precondition_failed") => {
            Kind::RebaselineRequired
        }
        (409, "invalid_state" | "invalid_upload_offset") => Kind::Rejected,
        (410, "upload_expired") => Kind::Integrity,
        (507, "capacity_unavailable") => Kind::Unavailable,
        _ => Kind::Protocol,
    };
    RemoteError::new(kind)
}
