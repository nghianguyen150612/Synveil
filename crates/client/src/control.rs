//! Secure, machine-local control IPC for the production desktop process.
//!
//! The control plane is deliberately narrower than the synchronization host.
//! It translates requests into calls on one [`synveil_client_sync::DesktopSyncHostHandle`]
//! and exposes only bounded, redacted status snapshots. It does not open the
//! SQLite store, load credentials, construct another host/runtime, or invoke a
//! synchronization engine directly.

use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use synveil_client_sync::{
    DesktopAuthError, DesktopLibrarySetupError, DesktopProfileConfiguration,
    DesktopProfileConfigurationOutcome, DesktopSyncHostHandle, RootAvailability,
    SyncAttentionSnapshot, SyncConflictItem, SyncConflictResolution, SyncRuntimeControlState,
    SyncRuntimeEvent, SyncRuntimeLibraryPhase, SyncRuntimeLibraryStatus, SyncRuntimeOutcome,
    SyncRuntimeWakeResult,
};
use synveil_core::{
    DEVICE_SECRET_ENCODED_BYTES, EnrollmentSecret, LibraryId, OutboundIntentId, SyncConflictId,
};
use synveil_platform::{Platform, PlatformRuntime};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{Notify, broadcast},
    task::{JoinHandle, JoinSet},
    time,
};
use zeroize::Zeroizing;

use crate::{DesktopProcessStatus, DesktopSyncPauseStore};

#[cfg(unix)]
use std::path::Component;

#[cfg(windows)]
use windows_sys::Win32::Security::PSECURITY_DESCRIPTOR;

/// Readiness marker for the production desktop control-plane contract.
pub const DESKTOP_CONTROL_IPC_READINESS: &str = "SYNVEIL_DESKTOP_CONTROL_IPC_READY";

/// Initial wire-protocol version.
pub const DESKTOP_CONTROL_PROTOCOL_VERSION: u16 = 1;

/// Maximum encoded JSON payload in one length-prefixed frame.
pub const DESKTOP_CONTROL_MAX_FRAME_BYTES: usize = 64 * 1024;

/// Maximum number of concurrently active control connections.
pub const DESKTOP_CONTROL_MAX_CONNECTIONS: usize = 32;

/// Bounded best-effort event fanout capacity.
pub const DESKTOP_CONTROL_EVENT_CAPACITY: usize = 256;

/// Root/status polling interval used only to translate canonical host state
/// changes into invalidation events. It never performs synchronization work.
pub const DESKTOP_CONTROL_STATUS_POLL_INTERVAL: Duration = Duration::from_millis(250);

const CONTROL_ENDPOINT_DIRECTORY: &str = "synveil";
const CONTROL_SOCKET_SUFFIX: &str = ".sock";
#[cfg(any(windows, test))]
const CONTROL_PIPE_PREFIX: &str = "\\\\.\\pipe\\synveil-";
#[cfg(any(windows, test))]
const CONTROL_PIPE_SECURITY_DESCRIPTOR: &str = "D:P(A;;GA;;;OW)";
const CONTROL_CONNECTION_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const CONTROL_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const CONTROL_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(unix)]
const CONTROL_STALE_ENDPOINT_PROBE_TIMEOUT: Duration = Duration::from_millis(250);
// UUIDv7-only library IDs and the fixed category fields keep 128 statuses
// comfortably below the 64 KiB response-frame ceiling.
const CONTROL_MAX_LIBRARY_STATUS_ITEMS: usize = 128;
const CONTROL_MAX_FRAME_U32: u64 = u32::MAX as u64;

/// Capabilities advertised after a successful protocol handshake.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlCapability {
    Ping,
    ProcessStatus,
    LibraryStatus,
    SyncNow,
    SyncControl,
    Shutdown,
    Events,
    ProfileConfiguration,
    LibrarySetup,
    Attention,
}

/// Protocol-level error categories. They contain no OS paths, credentials,
/// remote diagnostics, or database details.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlErrorCode {
    ProtocolVersionUnsupported,
    FrameInvalid,
    FrameTooLarge,
    PayloadMalformed,
    RequestInvalid,
    UnknownCommand,
    UnknownLibrary,
    RuntimeStopped,
    ControlServerStopping,
    ConnectionLimit,
    EndpointAlreadyActive,
    EndpointUnsafe,
    Internal,
}

/// Client-to-server handshake message.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlClientHello {
    pub protocol_version: u16,
}

/// Server-to-client handshake message. A non-`None` error is terminal and is
/// followed by connection closure.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlServerHello {
    pub protocol_version: u16,
    pub capabilities: Vec<ControlCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ControlErrorCode>,
}

/// Wire command envelope. Library IDs remain strings on the wire and are
/// parsed through the canonical UUIDv7 domain boundary before dispatch.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRequest {
    pub request_id: u64,
    pub command: ControlCommand,
}

/// Deliberately small Prompt 96 command set.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControlCommand {
    Ping,
    GetProcessStatus,
    ListLibraries,
    GetAttentionSnapshot,
    ResolveConflict {
        library_id: String,
        conflict_id: String,
        intent_id: String,
        detected_at_ms: u64,
        action: ControlConflictAction,
    },
    GetLibraryStatus {
        library_id: String,
    },
    SyncNow {
        library_id: String,
    },
    GetSyncControlState,
    PauseSync,
    ResumeSync,
    SetupLibrary {
        name: String,
        root_path: String,
    },
    Shutdown,
    SubscribeEvents,
    Authenticate {
        enrollment_token: ControlAuthInput,
    },
    SignOut,
    /// Get the current profile configuration.
    GetProfileConfiguration,
    /// Validate a candidate server URL and display label without persisting.
    ValidateProfileConfiguration {
        base_url: String,
        display_label: String,
    },
    /// Create or update a profile configuration.
    CreateOrConfigureProfile {
        profile_id: String,
        base_url: String,
        display_label: String,
    },
    /// Update the connection configuration for an existing profile.
    UpdateProfileConfiguration {
        profile_id: String,
        base_url: String,
        display_label: String,
    },
    /// Unknown tagged commands deserialize here so the server can return a
    /// typed error without treating a future command as a process failure.
    #[serde(other)]
    Unsupported,
}

/// Bounded safe server identity presented to the bridge.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlProfileServerInfo {
    pub profile_id: String,
    pub base_url: String,
    pub display_label: String,
    pub created_at_ms: i64,
}

/// Bounded result of a profile configuration operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlProfileConfigurationOutcome {
    Validated,
    Created,
    Updated,
    NotFound,
    AlreadyConfigured,
    InvalidConfiguration,
    InvalidServerAddress,
    NetworkUnavailable,
    ConnectionRefused,
    Timeout,
    TlsFailure,
    IncompatibleServer,
    ServerFailure,
    PersistenceFailure,
    Busy,
    OutcomeUnknown,
}

/// Category-only result for the authenticated zero-library onboarding
/// operation. The selected local path is never echoed in this value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlLibrarySetupOutcome {
    Configured,
    AlreadyConfigured,
    InvalidName,
    InvalidRoot,
    AuthenticationRequired,
    NetworkUnavailable,
    ServerUnavailable,
    Timeout,
    TlsFailure,
    ServerIdentityConflict,
    PersistenceFailure,
    Busy,
    OutcomeUnknown,
    Unavailable,
    ProtocolError,
}

/// Current profile configuration state presented through the control plane.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlProfileConfiguration {
    pub configured: bool,
    pub authenticated: bool,
    pub server_info: Option<ControlProfileServerInfo>,
}

/// Bounded wire representation of a transient enrollment secret. It exists
/// only long enough to cross the local control connection and redacts its
/// value from `Debug`; the controller creates it from the validated domain
/// secret and the server immediately parses it into the same domain type.
#[derive(Clone, Eq, PartialEq)]
pub struct ControlAuthInput(Zeroizing<String>);

impl ControlAuthInput {
    pub(crate) fn from_secret(secret: &EnrollmentSecret) -> Self {
        Self(Zeroizing::new(secret.expose_secret().to_owned()))
    }

    fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for ControlAuthInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ControlAuthInput([REDACTED])")
    }
}

impl Serialize for ControlAuthInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ControlAuthInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value.len() > DEVICE_SECRET_ENCODED_BYTES {
            return Err(serde::de::Error::custom(
                "auth input exceeds bounded length",
            ));
        }
        Ok(Self(Zeroizing::new(value)))
    }
}

/// Category-only result for a local desktop authentication operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlAuthOutcome {
    Authenticated,
    SignedOut,
    InvalidCredentials,
    NetworkUnavailable,
    ServerUnavailable,
    RateLimited,
    SecureStoreUnavailable,
    Busy,
    ProtocolError,
    OutcomeUnknown,
}

/// Global user-controlled sync state. It is distinct from per-library auth,
/// root, and network categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlSyncControlState {
    Running,
    PausedByUser,
}

/// Category-only result for a durable pause/resume mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlSyncControlResult {
    Paused,
    Resumed,
    AlreadyPaused,
    AlreadyRunning,
    PersistenceFailure,
    Busy,
}

/// A safe, category-only process snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlProcessStatus {
    pub state: DesktopProcessStatus,
    pub control_ready: bool,
}

/// Runtime phase exposed by the control plane.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlRuntimeState {
    Idle,
    Scheduled,
    Running,
    BackingOff,
    AuthBlocked,
    RootBlocked,
    Faulted,
    Stopped,
}

/// Root state exposed without the configured filesystem path.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlRootState {
    Available,
    Unavailable,
    Recovering,
}

/// Authentication category. `Unknown` is used until the canonical runtime has
/// observed a result that proves readiness or a block; no credential detail is
/// inferred or exposed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlAuthState {
    Ready,
    Missing,
    Blocked,
    Revoked,
    Unknown,
}

/// Conflict category derived only from the existing runtime outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlConflictState {
    Clear,
    Required,
    Unknown,
}

/// The only two interactive actions supported by the canonical client-local
/// conflict policy. This is an enum, not a free-form QML command string.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlConflictAction {
    AcceptRemote,
    RetryLocalAgainstCurrentBase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlAttentionItemKind {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlConflictResolutionOutcome {
    Resolved,
    AlreadyResolved,
    Stale,
    NotFound,
    UnsupportedAction,
    PersistenceFailure,
    Busy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlAttentionSummary {
    pub total_count: u64,
    pub conflict_count: u64,
    pub other_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlAttentionLibrarySummary {
    pub library_id: String,
    pub conflict_count: u64,
    pub other_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlAttentionItem {
    pub attention_id: String,
    pub library_id: String,
    pub conflict_id: String,
    pub intent_id: String,
    pub node_id: Option<String>,
    pub category: String,
    pub relative_path: Option<String>,
    pub previous_relative_path: Option<String>,
    pub item_kind: Option<ControlAttentionItemKind>,
    pub local_length: Option<u64>,
    pub remote_length: Option<u64>,
    pub local_base_revision: Option<u64>,
    pub remote_observed_revision: Option<u64>,
    pub remote_observed_state: Option<String>,
    pub detected_at_ms: u64,
    pub supported_actions: Vec<ControlConflictAction>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlAttentionSnapshot {
    pub summary: ControlAttentionSummary,
    pub libraries: Vec<ControlAttentionLibrarySummary>,
    pub items: Vec<ControlAttentionItem>,
    pub truncated: bool,
}

/// Safe synchronization outcome category.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlSyncOutcome {
    Idle,
    Progress,
    ConflictBlocked,
    Offline,
    ServerTransient,
    RateLimited,
    AuthBlocked,
    RootUnavailable,
    RecoveryBlocked,
    FatalLocal,
    Panicked,
}

/// Bounded status for one registered library. It contains no root path,
/// server URL, credential identifier, token, cookie, authorization header, or
/// file content.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlLibraryStatus {
    pub library_id: String,
    pub runtime_state: ControlRuntimeState,
    pub root_state: ControlRootState,
    pub auth_state: ControlAuthState,
    pub conflict_state: ControlConflictState,
    pub next_due_ms: Option<u64>,
    pub last_outcome: Option<ControlSyncOutcome>,
    pub wake_pending: bool,
    pub transient_failures: u32,
}

/// Bounded library-list response. `truncated` makes the frame-size policy
/// explicit; callers can request an individual status for any known ID.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlLibraryList {
    pub libraries: Vec<ControlLibraryStatus>,
    pub truncated: bool,
}

/// Result of a scheduling-only Sync Now request.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlSyncScheduleResult {
    Queued,
    Coalesced,
    AlreadyRunningFollowupRecorded,
    Paused,
}

/// Safe event payload. Events are invalidation/best-effort notifications; the
/// canonical status commands remain authoritative after reconnect or lag.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControlEvent {
    ProcessStateChanged {
        state: DesktopProcessStatus,
    },
    LibraryStatusChanged {
        library_id: String,
    },
    RootAvailabilityChanged {
        library_id: String,
        state: ControlRootState,
    },
    SyncCycleCompleted {
        library_id: String,
        outcome: ControlSyncOutcome,
    },
    /// Durable attention changed. The payload is intentionally empty; the
    /// next bounded snapshot is authoritative after reconnect or event lag.
    AttentionStateChanged,
    ProfileConfigurationChanged,
    SyncControlStateChanged {
        state: ControlSyncControlState,
    },
    ControlServerStopping,
    Lagged {
        dropped_count: u64,
    },
}

/// Response envelope. Event frames use request ID zero and are sent only after
/// a successful Subscribe Events response.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlResponse {
    pub request_id: u64,
    pub body: ControlResponseBody,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ControlResponseBody {
    Pong {
        status: DesktopProcessStatus,
    },
    ProcessStatus {
        status: ControlProcessStatus,
    },
    Libraries {
        status: ControlLibraryList,
    },
    AttentionSnapshot {
        snapshot: ControlAttentionSnapshot,
    },
    ConflictResolution {
        result: ControlConflictResolutionOutcome,
    },
    LibraryStatus {
        status: ControlLibraryStatus,
    },
    SyncNow {
        result: ControlSyncScheduleResult,
    },
    SyncControlState {
        state: ControlSyncControlState,
    },
    SyncControl {
        result: ControlSyncControlResult,
    },
    Authenticate {
        result: ControlAuthOutcome,
    },
    SignOut {
        result: ControlAuthOutcome,
    },
    ShutdownAccepted,
    Subscribed {
        event_capacity: u32,
    },
    Event {
        event: ControlEvent,
    },
    Error {
        code: ControlErrorCode,
    },
    ProfileConfiguration {
        configuration: ControlProfileConfiguration,
    },
    ProfileConfigurationResult {
        outcome: ControlProfileConfigurationOutcome,
    },
    LibrarySetup {
        outcome: ControlLibrarySetupOutcome,
    },
}

impl ControlResponse {
    fn error(request_id: u64, code: ControlErrorCode) -> Self {
        Self {
            request_id,
            body: ControlResponseBody::Error { code },
        }
    }
}

/// Framing/serialization failures. No variant stores arbitrary input or an OS
/// error string, which keeps diagnostics safe by construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlFrameError {
    Closed,
    Io,
    ZeroLength,
    TooLarge,
    Truncated,
    PayloadMalformed,
}

impl fmt::Display for ControlFrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Closed => "CONTROL_FRAME_CLOSED",
            Self::Io => "CONTROL_FRAME_IO",
            Self::ZeroLength => "CONTROL_FRAME_ZERO_LENGTH",
            Self::TooLarge => "CONTROL_FRAME_TOO_LARGE",
            Self::Truncated => "CONTROL_FRAME_TRUNCATED",
            Self::PayloadMalformed => "CONTROL_FRAME_PAYLOAD_MALFORMED",
        })
    }
}

impl std::error::Error for ControlFrameError {}

/// Encode one bounded big-endian u32 length-prefixed frame.
pub fn encode_frame(payload: &[u8]) -> Result<Vec<u8>, ControlFrameError> {
    if payload.is_empty() {
        return Err(ControlFrameError::ZeroLength);
    }
    if payload.len() > DESKTOP_CONTROL_MAX_FRAME_BYTES
        || (payload.len() as u64) > CONTROL_MAX_FRAME_U32
    {
        return Err(ControlFrameError::TooLarge);
    }
    let length = u32::try_from(payload.len()).map_err(|_| ControlFrameError::TooLarge)?;
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

/// Read one bounded frame without allocating according to an unchecked peer
/// length. The four-byte header is read before any body allocation.
pub async fn read_frame<R>(reader: &mut R) -> Result<Vec<u8>, ControlFrameError>
where
    R: AsyncRead + Unpin + ?Sized,
{
    let mut header = [0_u8; 4];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(ControlFrameError::Closed);
        }
        Err(_) => return Err(ControlFrameError::Io),
    }
    let declared = u32::from_be_bytes(header) as usize;
    if declared == 0 {
        return Err(ControlFrameError::ZeroLength);
    }
    if declared > DESKTOP_CONTROL_MAX_FRAME_BYTES {
        return Err(ControlFrameError::TooLarge);
    }
    let mut body = vec![0_u8; declared];
    match reader.read_exact(&mut body).await {
        Ok(_) => Ok(body),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
            Err(ControlFrameError::Truncated)
        }
        Err(_) => Err(ControlFrameError::Io),
    }
}

/// Serialize and write one bounded frame.
pub async fn write_frame<W, T>(writer: &mut W, value: &T) -> Result<(), ControlFrameError>
where
    W: AsyncWrite + Unpin + ?Sized,
    T: Serialize,
{
    let payload = serde_json::to_vec(value).map_err(|_| ControlFrameError::PayloadMalformed)?;
    let frame = encode_frame(&payload)?;
    writer
        .write_all(&frame)
        .await
        .map_err(|_| ControlFrameError::Io)
}

fn decode_payload<T>(payload: &[u8]) -> Result<T, ControlFrameError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_slice(payload).map_err(|_| ControlFrameError::PayloadMalformed)
}

/// Endpoint kind, kept separate from the endpoint value so logs need not print
/// a path or pipe name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlEndpointKind {
    UnixSocket,
    NamedPipe,
}

/// Deterministic profile-scoped local endpoint. The fields are private so a
/// caller cannot make the server unlink an arbitrary filesystem path.
#[derive(Clone, Eq, PartialEq)]
pub enum DesktopControlEndpoint {
    UnixSocket { path: PathBuf },
    NamedPipe { name: String },
}

impl fmt::Debug for DesktopControlEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopControlEndpoint")
            .field("kind", &self.kind())
            .finish_non_exhaustive()
    }
}

impl DesktopControlEndpoint {
    /// Resolve the endpoint from the platform's canonical runtime directory
    /// and an opaque profile ID. No secret material participates in naming.
    pub fn for_profile(
        platform: &dyn PlatformRuntime,
        profile_id: synveil_client_sync::ServerProfileId,
    ) -> Result<Self, DesktopControlServerError> {
        match platform.platform() {
            Platform::Linux => {
                let paths = platform
                    .resolve_paths()
                    .map_err(|_| DesktopControlServerError::RuntimeDirectoryUnavailable)?;
                let path = paths
                    .runtime_dir()
                    .as_path()
                    .join(CONTROL_ENDPOINT_DIRECTORY)
                    .join(format!("{profile_id}{CONTROL_SOCKET_SUFFIX}"));
                validate_endpoint_path_length(&path)?;
                Ok(Self::UnixSocket { path })
            }
            Platform::Windows => {
                #[cfg(windows)]
                {
                    let name = windows_pipe_name(profile_id);
                    validate_pipe_name(&name)?;
                    Ok(Self::NamedPipe { name })
                }
                #[cfg(not(windows))]
                {
                    let _ = profile_id;
                    Err(DesktopControlServerError::UnsupportedPlatform)
                }
            }
            Platform::Macos | Platform::Generic => {
                Err(DesktopControlServerError::UnsupportedPlatform)
            }
        }
    }

    #[must_use]
    pub const fn kind(&self) -> ControlEndpointKind {
        match self {
            Self::UnixSocket { .. } => ControlEndpointKind::UnixSocket,
            Self::NamedPipe { .. } => ControlEndpointKind::NamedPipe,
        }
    }

    /// Expose the generated path only to a caller that explicitly needs to
    /// connect; ordinary status/events and Debug never include it.
    #[must_use]
    pub fn unix_path(&self) -> Option<&Path> {
        match self {
            Self::UnixSocket { path } => Some(path),
            Self::NamedPipe { .. } => None,
        }
    }

    /// Expose the generated pipe name only to the transport client.
    #[must_use]
    pub fn named_pipe(&self) -> Option<&str> {
        match self {
            Self::UnixSocket { .. } => None,
            Self::NamedPipe { name } => Some(name),
        }
    }
}

fn validate_endpoint_path_length(path: &Path) -> Result<(), DesktopControlServerError> {
    if path.as_os_str().len() >= 108 {
        return Err(DesktopControlServerError::EndpointNameTooLong);
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn windows_pipe_name(profile_id: synveil_client_sync::ServerProfileId) -> String {
    format!("{CONTROL_PIPE_PREFIX}{profile_id}")
}

#[cfg(any(windows, test))]
fn validate_pipe_name(name: &str) -> Result<(), DesktopControlServerError> {
    if name.len() > 256 || !name.starts_with(CONTROL_PIPE_PREFIX) {
        return Err(DesktopControlServerError::EndpointNameTooLong);
    }
    Ok(())
}

/// Server bootstrap/transport failures. Error formatting is stable and does
/// not echo a path, pipe name, or underlying OS diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopControlServerError {
    UnsupportedPlatform,
    RuntimeDirectoryUnavailable,
    InsecureRuntimeDirectory,
    UnsafeEndpoint,
    EndpointAlreadyActive,
    EndpointStateUnknown,
    EndpointNameTooLong,
    BindFailed,
    SecurityDescriptorUnavailable,
    ListenerFailed,
    AlreadyStarted,
    NotStarted,
    TaskPanicked,
}

impl DesktopControlServerError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnsupportedPlatform => "CONTROL_PLATFORM_UNSUPPORTED",
            Self::RuntimeDirectoryUnavailable => "CONTROL_RUNTIME_DIRECTORY_UNAVAILABLE",
            Self::InsecureRuntimeDirectory => "CONTROL_RUNTIME_DIRECTORY_INSECURE",
            Self::UnsafeEndpoint => "CONTROL_ENDPOINT_UNSAFE",
            Self::EndpointAlreadyActive => "CONTROL_ENDPOINT_ALREADY_ACTIVE",
            Self::EndpointStateUnknown => "CONTROL_ENDPOINT_STATE_UNKNOWN",
            Self::EndpointNameTooLong => "CONTROL_ENDPOINT_NAME_TOO_LONG",
            Self::BindFailed => "CONTROL_ENDPOINT_BIND_FAILED",
            Self::SecurityDescriptorUnavailable => "CONTROL_SECURITY_DESCRIPTOR_UNAVAILABLE",
            Self::ListenerFailed => "CONTROL_LISTENER_FAILED",
            Self::AlreadyStarted => "CONTROL_SERVER_ALREADY_STARTED",
            Self::NotStarted => "CONTROL_SERVER_NOT_STARTED",
            Self::TaskPanicked => "CONTROL_SERVER_TASK_PANICKED",
        }
    }
}

impl fmt::Display for DesktopControlServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for DesktopControlServerError {}

/// Errors returned by the reusable local control client.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopControlClientError {
    UnsupportedPlatform,
    Endpoint(DesktopControlServerError),
    Connection,
    Handshake,
    ProtocolVersionUnsupported,
    Frame(ControlFrameError),
    Server(ControlErrorCode),
    ResponseMismatch,
    UnexpectedResponse,
    Closed,
}

impl fmt::Display for DesktopControlClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => formatter.write_str("CONTROL_CLIENT_PLATFORM_UNSUPPORTED"),
            Self::Endpoint(error) => formatter.write_str(error.code()),
            Self::Connection => formatter.write_str("CONTROL_CLIENT_CONNECTION_FAILED"),
            Self::Handshake => formatter.write_str("CONTROL_CLIENT_HANDSHAKE_FAILED"),
            Self::ProtocolVersionUnsupported => {
                formatter.write_str("CONTROL_PROTOCOL_VERSION_UNSUPPORTED")
            }
            Self::Frame(error) => formatter.write_str(&error.to_string()),
            Self::Server(code) => formatter.write_str(code.code()),
            Self::ResponseMismatch => formatter.write_str("CONTROL_CLIENT_RESPONSE_MISMATCH"),
            Self::UnexpectedResponse => formatter.write_str("CONTROL_CLIENT_RESPONSE_UNEXPECTED"),
            Self::Closed => formatter.write_str("CONTROL_CLIENT_CLOSED"),
        }
    }
}

impl std::error::Error for DesktopControlClientError {}

impl ControlErrorCode {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ProtocolVersionUnsupported => "CONTROL_PROTOCOL_VERSION_UNSUPPORTED",
            Self::FrameInvalid => "CONTROL_FRAME_INVALID",
            Self::FrameTooLarge => "CONTROL_FRAME_TOO_LARGE",
            Self::PayloadMalformed => "CONTROL_PAYLOAD_MALFORMED",
            Self::RequestInvalid => "CONTROL_REQUEST_INVALID",
            Self::UnknownCommand => "CONTROL_COMMAND_UNKNOWN",
            Self::UnknownLibrary => "CONTROL_LIBRARY_UNKNOWN",
            Self::RuntimeStopped => "CONTROL_RUNTIME_STOPPED",
            Self::ControlServerStopping => "CONTROL_SERVER_STOPPING",
            Self::ConnectionLimit => "CONTROL_CONNECTION_LIMIT",
            Self::EndpointAlreadyActive => "CONTROL_ENDPOINT_ALREADY_ACTIVE",
            Self::EndpointUnsafe => "CONTROL_ENDPOINT_UNSAFE",
            Self::Internal => "CONTROL_INTERNAL_FAILURE",
        }
    }
}

/// Process-owned control state. It contains only a host handle and ephemeral
/// coordination primitives; it owns no durable synchronization data.
#[derive(Clone)]
pub struct DesktopControlHandle {
    inner: Arc<DesktopControlShared>,
}

struct DesktopControlShared {
    host: DesktopSyncHostHandle,
    library_setup: Option<LibrarySetupContext>,
    sync_pause_store: Option<Arc<DesktopSyncPauseStore>>,
    process_status: AtomicU8,
    control_ready: AtomicBool,
    shutdown_requested: AtomicBool,
    shutdown_notify: Notify,
    events: broadcast::Sender<ControlEvent>,
    auth_in_flight: AtomicBool,
    profile_configuration_in_flight: AtomicBool,
    library_setup_in_flight: AtomicBool,
    sync_control_in_flight: AtomicBool,
    attention_in_flight: AtomicBool,
}

struct LibrarySetupContext {
    platform: Arc<dyn PlatformRuntime>,
    profile_id: synveil_client_sync::ServerProfileId,
}

impl fmt::Debug for DesktopControlHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopControlHandle")
            .field("process_status", &self.process_status())
            .field("runtime_identity", &self.inner.host.runtime_identity())
            .finish_non_exhaustive()
    }
}

impl DesktopControlHandle {
    pub fn new(host: DesktopSyncHostHandle, initial: DesktopProcessStatus) -> Self {
        Self::new_inner(host, initial, None, None)
    }

    pub fn new_with_library_setup(
        host: DesktopSyncHostHandle,
        initial: DesktopProcessStatus,
        platform: Arc<dyn PlatformRuntime>,
        profile_id: synveil_client_sync::ServerProfileId,
    ) -> Self {
        Self::new_inner(
            host,
            initial,
            Some(LibrarySetupContext {
                platform,
                profile_id,
            }),
            None,
        )
    }

    pub fn new_with_library_setup_and_sync_store(
        host: DesktopSyncHostHandle,
        initial: DesktopProcessStatus,
        platform: Arc<dyn PlatformRuntime>,
        profile_id: synveil_client_sync::ServerProfileId,
        sync_pause_store: DesktopSyncPauseStore,
    ) -> Self {
        Self::new_inner(
            host,
            initial,
            Some(LibrarySetupContext {
                platform,
                profile_id,
            }),
            Some(Arc::new(sync_pause_store)),
        )
    }

    fn new_inner(
        host: DesktopSyncHostHandle,
        initial: DesktopProcessStatus,
        library_setup: Option<LibrarySetupContext>,
        sync_pause_store: Option<Arc<DesktopSyncPauseStore>>,
    ) -> Self {
        let (events, _) = broadcast::channel(DESKTOP_CONTROL_EVENT_CAPACITY);
        Self {
            inner: Arc::new(DesktopControlShared {
                host,
                library_setup,
                sync_pause_store,
                process_status: AtomicU8::new(process_status_code(initial)),
                control_ready: AtomicBool::new(false),
                shutdown_requested: AtomicBool::new(false),
                shutdown_notify: Notify::new(),
                events,
                auth_in_flight: AtomicBool::new(false),
                profile_configuration_in_flight: AtomicBool::new(false),
                library_setup_in_flight: AtomicBool::new(false),
                sync_control_in_flight: AtomicBool::new(false),
                attention_in_flight: AtomicBool::new(false),
            }),
        }
    }

    #[must_use]
    pub fn process_status(&self) -> DesktopProcessStatus {
        process_status_from_code(self.inner.process_status.load(Ordering::Acquire))
    }

    /// Publish a process lifecycle transition to status readers and event
    /// subscribers. The process runner remains the sole lifecycle owner.
    pub(crate) fn set_process_status(&self, status: DesktopProcessStatus) {
        let previous = self
            .inner
            .process_status
            .swap(process_status_code(status), Ordering::AcqRel);
        if process_status_from_code(previous) != status {
            self.emit(ControlEvent::ProcessStateChanged { state: status });
        }
    }

    #[must_use]
    pub fn process_status_snapshot(&self) -> ControlProcessStatus {
        ControlProcessStatus {
            state: self.process_status(),
            control_ready: self.inner.control_ready.load(Ordering::Acquire),
        }
    }

    pub(crate) fn set_control_ready(&self, ready: bool) {
        self.inner.control_ready.store(ready, Ordering::Release);
    }

    #[must_use]
    pub fn library_status(&self, library_id: LibraryId) -> Option<ControlLibraryStatus> {
        self.inner
            .host
            .status(library_id)
            .and_then(|status| self.to_library_status(status))
    }

    #[must_use]
    pub fn library_list(&self) -> ControlLibraryList {
        let statuses = self.inner.host.statuses();
        let truncated = statuses.len() > CONTROL_MAX_LIBRARY_STATUS_ITEMS;
        let libraries = statuses
            .into_iter()
            .take(CONTROL_MAX_LIBRARY_STATUS_ITEMS)
            .filter_map(|status| self.to_library_status(status))
            .collect();
        ControlLibraryList {
            libraries,
            truncated,
        }
    }

    async fn attention_snapshot(&self) -> Result<ControlAttentionSnapshot, ControlErrorCode> {
        self.inner
            .host
            .attention_snapshot()
            .await
            .map(map_attention_snapshot)
            .map_err(|_| ControlErrorCode::Internal)
    }

    async fn resolve_conflict(
        &self,
        library_id: LibraryId,
        conflict_id: SyncConflictId,
        intent_id: OutboundIntentId,
        detected_at_ms: u64,
        action: ControlConflictAction,
    ) -> ControlConflictResolutionOutcome {
        let Some(_guard) = AttentionOperationGuard::try_acquire(&self.inner.attention_in_flight)
        else {
            return ControlConflictResolutionOutcome::Busy;
        };
        let resolution = match action {
            ControlConflictAction::AcceptRemote => SyncConflictResolution::AcceptRemote,
            ControlConflictAction::RetryLocalAgainstCurrentBase => {
                SyncConflictResolution::RetryLocalAgainstCurrentBase
            }
        };
        match self
            .inner
            .host
            .resolve_conflict(
                library_id,
                conflict_id,
                intent_id,
                detected_at_ms,
                resolution,
            )
            .await
        {
            Ok(_) => {
                self.emit(ControlEvent::AttentionStateChanged);
                ControlConflictResolutionOutcome::Resolved
            }
            Err(synveil_client_sync::ClientSyncError::ConflictAlreadyResolved) => {
                ControlConflictResolutionOutcome::AlreadyResolved
            }
            Err(synveil_client_sync::ClientSyncError::ConflictStale) => {
                ControlConflictResolutionOutcome::Stale
            }
            Err(synveil_client_sync::ClientSyncError::ConflictNotFound) => {
                ControlConflictResolutionOutcome::NotFound
            }
            Err(synveil_client_sync::ClientSyncError::ResolutionNotApplicable) => {
                ControlConflictResolutionOutcome::UnsupportedAction
            }
            Err(_) => ControlConflictResolutionOutcome::PersistenceFailure,
        }
    }

    #[must_use]
    pub fn sync_now(&self, library_id: LibraryId) -> SyncRuntimeWakeResult {
        self.inner.host.sync_now(library_id)
    }

    #[must_use]
    pub fn sync_control_state(&self) -> ControlSyncControlState {
        control_sync_control_state(self.inner.host.sync_control_state())
    }

    async fn set_sync_paused(&self, paused: bool) -> ControlSyncControlResult {
        let Some(_guard) =
            SyncControlOperationGuard::try_acquire(&self.inner.sync_control_in_flight)
        else {
            return ControlSyncControlResult::Busy;
        };

        let current = self.sync_control_state();
        if paused && current == ControlSyncControlState::PausedByUser {
            return ControlSyncControlResult::AlreadyPaused;
        }
        if !paused && current == ControlSyncControlState::Running {
            return ControlSyncControlResult::AlreadyRunning;
        }

        let Some(store) = self.inner.sync_pause_store.clone() else {
            return ControlSyncControlResult::PersistenceFailure;
        };

        let persisted = tokio::task::spawn_blocking(move || store.persist(paused)).await;
        if !matches!(persisted, Ok(Ok(()))) {
            return ControlSyncControlResult::PersistenceFailure;
        }

        let state = self.inner.host.set_user_paused(paused);
        let state = control_sync_control_state(state);
        match (paused, state) {
            (true, ControlSyncControlState::PausedByUser) => ControlSyncControlResult::Paused,
            (false, ControlSyncControlState::Running) => ControlSyncControlResult::Resumed,
            (true, ControlSyncControlState::Running) => ControlSyncControlResult::AlreadyRunning,
            (false, ControlSyncControlState::PausedByUser) => {
                ControlSyncControlResult::AlreadyPaused
            }
        }
    }

    async fn authenticate(&self, input: ControlAuthInput) -> ControlAuthOutcome {
        let Some(_guard) = AuthOperationGuard::try_acquire(&self.inner.auth_in_flight) else {
            return ControlAuthOutcome::Busy;
        };
        let Ok(secret) = EnrollmentSecret::parse(input.as_str()) else {
            return ControlAuthOutcome::InvalidCredentials;
        };
        match self.inner.host.authenticate(&secret).await {
            Ok(()) => ControlAuthOutcome::Authenticated,
            Err(error) => map_auth_outcome(error),
        }
    }

    async fn sign_out(&self) -> ControlAuthOutcome {
        let Some(_guard) = AuthOperationGuard::try_acquire(&self.inner.auth_in_flight) else {
            return ControlAuthOutcome::Busy;
        };
        match self.inner.host.sign_out().await {
            Ok(()) => ControlAuthOutcome::SignedOut,
            Err(error) => map_auth_outcome(error),
        }
    }

    async fn profile_configuration(&self) -> Result<ControlProfileConfiguration, ControlErrorCode> {
        let configuration = self
            .inner
            .host
            .profile_configuration()
            .await
            .map_err(|_| ControlErrorCode::Internal)?;
        Ok(map_profile_configuration(configuration))
    }

    async fn validate_profile_configuration(
        &self,
        base_url: String,
        display_label: String,
    ) -> ControlProfileConfigurationOutcome {
        let Some(_guard) =
            ProfileConfigurationGuard::try_acquire(&self.inner.profile_configuration_in_flight)
        else {
            return ControlProfileConfigurationOutcome::Busy;
        };
        map_profile_outcome(
            self.inner
                .host
                .validate_profile_configuration(base_url, display_label)
                .await,
        )
    }

    async fn configure_profile(
        &self,
        profile_id: String,
        base_url: String,
        display_label: String,
    ) -> ControlProfileConfigurationOutcome {
        let Some(_guard) =
            ProfileConfigurationGuard::try_acquire(&self.inner.profile_configuration_in_flight)
        else {
            return ControlProfileConfigurationOutcome::Busy;
        };
        let Ok(profile_id) = profile_id.parse() else {
            return ControlProfileConfigurationOutcome::InvalidConfiguration;
        };
        let outcome = self
            .inner
            .host
            .configure_profile(profile_id, base_url, display_label)
            .await;
        let wire_outcome = map_profile_outcome(outcome);
        if matches!(
            wire_outcome,
            ControlProfileConfigurationOutcome::Created
                | ControlProfileConfigurationOutcome::Updated
                | ControlProfileConfigurationOutcome::AlreadyConfigured
        ) {
            self.emit(ControlEvent::ProfileConfigurationChanged);
        }
        wire_outcome
    }

    async fn setup_library(&self, name: String, root_path: String) -> ControlLibrarySetupOutcome {
        let Some(_guard) = LibrarySetupGuard::try_acquire(&self.inner.library_setup_in_flight)
        else {
            return ControlLibrarySetupOutcome::Busy;
        };
        let Some(context) = &self.inner.library_setup else {
            return ControlLibrarySetupOutcome::Unavailable;
        };
        if name.trim().is_empty()
            || name.len() > synveil_core::MAX_LOGICAL_NAME_BYTES
            || name.chars().any(char::is_control)
        {
            return ControlLibrarySetupOutcome::InvalidName;
        }
        if root_path.is_empty()
            || root_path.len() > 16 * 1024
            || root_path.chars().any(char::is_control)
        {
            return ControlLibrarySetupOutcome::InvalidRoot;
        }
        let root = PathBuf::from(root_path);
        let canonical_root = match synveil_client_sync::validate_onboarding_root(&root) {
            Ok(root) => root,
            Err(_) => return ControlLibrarySetupOutcome::InvalidRoot,
        };
        let pending_id = match crate::DesktopClientConfig::pending_library_for_root(
            context.platform.as_ref(),
            context.profile_id,
            &canonical_root,
        ) {
            Ok(id) => id,
            Err(error) => return map_library_setup_config_error(error),
        };
        if pending_id.is_none() {
            match crate::DesktopClientConfig::root_overlaps_existing(
                context.platform.as_ref(),
                context.profile_id,
                &canonical_root,
            ) {
                Ok(true) => return ControlLibrarySetupOutcome::AlreadyConfigured,
                Ok(false) => {}
                Err(error) => return map_library_setup_config_error(error),
            }
        }
        let library_id = pending_id.unwrap_or_else(LibraryId::new);
        if let Err(error) = crate::DesktopClientConfig::append_pending_library_binding(
            context.platform.as_ref(),
            context.profile_id,
            library_id,
            &canonical_root,
        ) {
            return map_library_setup_config_error(error);
        }
        let setup = match self
            .inner
            .host
            .prepare_library(library_id, name, canonical_root)
            .await
        {
            Ok(setup) => setup,
            Err(error) => return map_library_setup_error(error),
        };
        if let Err(error) = crate::DesktopClientConfig::append_library_binding(
            context.platform.as_ref(),
            context.profile_id,
            setup.library_id(),
            setup.root(),
        ) {
            return map_library_setup_config_error(error);
        }
        match self.inner.host.register_prepared_library(setup).await {
            Ok(_) => {
                self.emit(ControlEvent::ProfileConfigurationChanged);
                ControlLibrarySetupOutcome::Configured
            }
            Err(error) => map_library_setup_error(error),
        }
    }

    /// Request the outer process lifecycle to perform graceful shutdown. This
    /// method never joins or aborts the host/runtime and is safe to call more
    /// than once.
    pub(crate) fn request_shutdown(&self) -> bool {
        match self.process_status() {
            DesktopProcessStatus::Stopped | DesktopProcessStatus::Faulted => false,
            DesktopProcessStatus::Stopping => true,
            DesktopProcessStatus::Starting | DesktopProcessStatus::Running => {
                self.set_process_status(DesktopProcessStatus::Stopping);
                self.inner.shutdown_requested.store(true, Ordering::Release);
                true
            }
        }
    }

    /// Wake the outer process only after the accepted response has been
    /// written to the requesting connection. This preserves the Shutdown
    /// acknowledgement ordering contract.
    pub(crate) fn notify_shutdown_request(&self) {
        self.inner.shutdown_notify.notify_waiters();
    }

    pub(crate) fn request_shutdown_after_server_failure(&self) {
        self.set_process_status(DesktopProcessStatus::Faulted);
        self.inner.shutdown_requested.store(true, Ordering::Release);
        self.inner.shutdown_notify.notify_waiters();
    }

    pub(crate) async fn wait_for_shutdown_request(&self) {
        loop {
            if self.inner.shutdown_requested.load(Ordering::Acquire) {
                return;
            }
            let notified = self.inner.shutdown_notify.notified();
            if self.inner.shutdown_requested.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }

    #[must_use]
    pub(crate) fn runtime_events(&self) -> broadcast::Receiver<SyncRuntimeEvent> {
        self.inner.host.events()
    }

    #[must_use]
    pub(crate) fn events(&self) -> broadcast::Receiver<ControlEvent> {
        self.inner.events.subscribe()
    }

    pub(crate) fn emit(&self, event: ControlEvent) {
        let _ = self.inner.events.send(event);
    }

    fn to_library_status(&self, status: SyncRuntimeLibraryStatus) -> Option<ControlLibraryStatus> {
        let library_id = status.library_id();
        let root_state = self
            .inner
            .host
            .root_status(library_id)
            .map(control_root_state)
            .unwrap_or(ControlRootState::Unavailable);
        let last_outcome = status.last_outcome().map(control_sync_outcome);
        Some(ControlLibraryStatus {
            library_id: library_id.to_string(),
            runtime_state: control_runtime_state(status.phase()),
            root_state,
            auth_state: control_auth_state(status),
            conflict_state: control_conflict_state(status),
            next_due_ms: status.next_due_in().map(duration_millis),
            last_outcome,
            wake_pending: status.wake_pending(),
            transient_failures: status.transient_failures(),
        })
    }
}

struct AuthOperationGuard<'a> {
    in_flight: &'a AtomicBool,
}

impl<'a> AuthOperationGuard<'a> {
    fn try_acquire(in_flight: &'a AtomicBool) -> Option<Self> {
        if in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            Some(Self { in_flight })
        } else {
            None
        }
    }
}

impl Drop for AuthOperationGuard<'_> {
    fn drop(&mut self) {
        self.in_flight.store(false, Ordering::Release);
    }
}

struct ProfileConfigurationGuard<'a> {
    in_flight: &'a AtomicBool,
}

impl<'a> ProfileConfigurationGuard<'a> {
    fn try_acquire(in_flight: &'a AtomicBool) -> Option<Self> {
        if in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            Some(Self { in_flight })
        } else {
            None
        }
    }
}

impl Drop for ProfileConfigurationGuard<'_> {
    fn drop(&mut self) {
        self.in_flight.store(false, Ordering::Release);
    }
}

struct LibrarySetupGuard<'a> {
    in_flight: &'a AtomicBool,
}

impl<'a> LibrarySetupGuard<'a> {
    fn try_acquire(in_flight: &'a AtomicBool) -> Option<Self> {
        if in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            Some(Self { in_flight })
        } else {
            None
        }
    }
}

impl Drop for LibrarySetupGuard<'_> {
    fn drop(&mut self) {
        self.in_flight.store(false, Ordering::Release);
    }
}

struct SyncControlOperationGuard<'a> {
    in_flight: &'a AtomicBool,
}

struct AttentionOperationGuard<'a> {
    in_flight: &'a AtomicBool,
}

impl<'a> AttentionOperationGuard<'a> {
    fn try_acquire(in_flight: &'a AtomicBool) -> Option<Self> {
        if in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            Some(Self { in_flight })
        } else {
            None
        }
    }
}

impl Drop for AttentionOperationGuard<'_> {
    fn drop(&mut self) {
        self.in_flight.store(false, Ordering::Release);
    }
}

impl<'a> SyncControlOperationGuard<'a> {
    fn try_acquire(in_flight: &'a AtomicBool) -> Option<Self> {
        if in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            Some(Self { in_flight })
        } else {
            None
        }
    }
}

impl Drop for SyncControlOperationGuard<'_> {
    fn drop(&mut self) {
        self.in_flight.store(false, Ordering::Release);
    }
}

fn map_profile_configuration(
    configuration: DesktopProfileConfiguration,
) -> ControlProfileConfiguration {
    let server_info = configuration
        .profile
        .map(|profile| ControlProfileServerInfo {
            profile_id: profile.profile_id().to_string(),
            base_url: profile.base_url().as_str().to_owned(),
            display_label: profile.display_label().to_owned(),
            created_at_ms: profile.created_at_ms(),
        });
    ControlProfileConfiguration {
        configured: server_info.is_some(),
        authenticated: configuration.authenticated,
        server_info,
    }
}

const CONTROL_MAX_ATTENTION_PATH_BYTES: usize = 512;

fn bounded_attention_path(
    path: Option<&synveil_client_sync::ManagedRelativePath>,
) -> Option<String> {
    let value = path?.as_str();
    if value.len() <= CONTROL_MAX_ATTENTION_PATH_BYTES {
        return Some(value.to_owned());
    }
    let mut end = CONTROL_MAX_ATTENTION_PATH_BYTES - '…'.len_utf8();
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut bounded = value[..end].to_owned();
    bounded.push('…');
    Some(bounded)
}

fn map_attention_item(item: &SyncConflictItem) -> ControlAttentionItem {
    let mut supported_actions = vec![ControlConflictAction::AcceptRemote];
    if item.supports_resolution(SyncConflictResolution::RetryLocalAgainstCurrentBase) {
        supported_actions.push(ControlConflictAction::RetryLocalAgainstCurrentBase);
    }
    ControlAttentionItem {
        attention_id: item.conflict_id().to_string(),
        library_id: item.library_id().to_string(),
        conflict_id: item.conflict_id().to_string(),
        intent_id: item.intent_id().to_string(),
        node_id: item.node_id().map(|value| value.to_string()),
        category: item.kind().as_str().to_owned(),
        relative_path: bounded_attention_path(item.relative_path()),
        previous_relative_path: bounded_attention_path(item.previous_relative_path()),
        item_kind: item.item_kind().map(|kind| match kind {
            synveil_client_sync::LocalObjectKind::File => ControlAttentionItemKind::File,
            synveil_client_sync::LocalObjectKind::Directory => ControlAttentionItemKind::Directory,
        }),
        local_length: item.local_length(),
        remote_length: item.remote_length(),
        local_base_revision: item.local_base_revision().map(|value| value.get()),
        remote_observed_revision: item.remote_observed_revision().map(|value| value.get()),
        remote_observed_state: item
            .remote_observed_state()
            .map(|value| value.as_str().to_owned()),
        detected_at_ms: item.detected_at_ms(),
        supported_actions,
    }
}

fn map_attention_snapshot(snapshot: SyncAttentionSnapshot) -> ControlAttentionSnapshot {
    let summary = snapshot.summary();
    ControlAttentionSnapshot {
        summary: ControlAttentionSummary {
            total_count: summary.total_count(),
            conflict_count: summary.conflict_count(),
            other_count: summary.other_count(),
        },
        libraries: snapshot
            .libraries()
            .iter()
            .map(|library| ControlAttentionLibrarySummary {
                library_id: library.library_id().to_string(),
                conflict_count: library.conflict_count(),
                other_count: library.other_count(),
            })
            .collect(),
        items: snapshot
            .conflict_items()
            .iter()
            .map(map_attention_item)
            .collect(),
        truncated: snapshot.truncated(),
    }
}

fn map_profile_outcome(
    outcome: DesktopProfileConfigurationOutcome,
) -> ControlProfileConfigurationOutcome {
    match outcome {
        DesktopProfileConfigurationOutcome::Validated => {
            ControlProfileConfigurationOutcome::Validated
        }
        DesktopProfileConfigurationOutcome::Created => ControlProfileConfigurationOutcome::Created,
        DesktopProfileConfigurationOutcome::Updated => ControlProfileConfigurationOutcome::Updated,
        DesktopProfileConfigurationOutcome::AlreadyConfigured => {
            ControlProfileConfigurationOutcome::AlreadyConfigured
        }
        DesktopProfileConfigurationOutcome::InvalidConfiguration => {
            ControlProfileConfigurationOutcome::InvalidConfiguration
        }
        DesktopProfileConfigurationOutcome::InvalidServerAddress => {
            ControlProfileConfigurationOutcome::InvalidServerAddress
        }
        DesktopProfileConfigurationOutcome::NetworkUnavailable => {
            ControlProfileConfigurationOutcome::NetworkUnavailable
        }
        DesktopProfileConfigurationOutcome::Timeout => ControlProfileConfigurationOutcome::Timeout,
        DesktopProfileConfigurationOutcome::TlsFailure => {
            ControlProfileConfigurationOutcome::TlsFailure
        }
        DesktopProfileConfigurationOutcome::IncompatibleServer => {
            ControlProfileConfigurationOutcome::IncompatibleServer
        }
        DesktopProfileConfigurationOutcome::ServerFailure => {
            ControlProfileConfigurationOutcome::ServerFailure
        }
        DesktopProfileConfigurationOutcome::PersistenceFailure => {
            ControlProfileConfigurationOutcome::PersistenceFailure
        }
    }
}

fn map_library_setup_config_error(
    error: crate::DesktopClientConfigError,
) -> ControlLibrarySetupOutcome {
    match error {
        crate::DesktopClientConfigError::Client(
            synveil_client_sync::ClientSyncError::InvalidRoot
            | synveil_client_sync::ClientSyncError::RootUnavailable
            | synveil_client_sync::ClientSyncError::RootRedirected,
        )
        | crate::DesktopClientConfigError::InvalidRootPath => {
            ControlLibrarySetupOutcome::InvalidRoot
        }
        _ => ControlLibrarySetupOutcome::PersistenceFailure,
    }
}

fn map_library_setup_error(error: DesktopLibrarySetupError) -> ControlLibrarySetupOutcome {
    match error {
        DesktopLibrarySetupError::HostUnavailable
        | DesktopLibrarySetupError::ProfileUnavailable => ControlLibrarySetupOutcome::Unavailable,
        DesktopLibrarySetupError::AuthenticationRequired => {
            ControlLibrarySetupOutcome::AuthenticationRequired
        }
        DesktopLibrarySetupError::InvalidName => ControlLibrarySetupOutcome::InvalidName,
        DesktopLibrarySetupError::InvalidRoot => ControlLibrarySetupOutcome::InvalidRoot,
        DesktopLibrarySetupError::AlreadyRegistered => {
            ControlLibrarySetupOutcome::AlreadyConfigured
        }
        DesktopLibrarySetupError::ServerIdentityConflict => {
            ControlLibrarySetupOutcome::ServerIdentityConflict
        }
        DesktopLibrarySetupError::ResponseUnknown => ControlLibrarySetupOutcome::OutcomeUnknown,
        DesktopLibrarySetupError::Remote(error) => map_library_setup_remote_error(error),
        DesktopLibrarySetupError::Client(error) => match error {
            synveil_client_sync::ClientSyncError::AuthenticationRequired => {
                ControlLibrarySetupOutcome::AuthenticationRequired
            }
            synveil_client_sync::ClientSyncError::InvalidRoot
            | synveil_client_sync::ClientSyncError::RootUnavailable
            | synveil_client_sync::ClientSyncError::RootRedirected => {
                ControlLibrarySetupOutcome::InvalidRoot
            }
            synveil_client_sync::ClientSyncError::Remote(error) => {
                map_library_setup_remote_error(error)
            }
            _ => ControlLibrarySetupOutcome::PersistenceFailure,
        },
    }
}

fn map_library_setup_remote_error(
    error: synveil_client_sync::RemoteError,
) -> ControlLibrarySetupOutcome {
    use synveil_client_sync::RemoteErrorKind;
    match error.kind() {
        RemoteErrorKind::Offline => ControlLibrarySetupOutcome::NetworkUnavailable,
        RemoteErrorKind::Timeout => ControlLibrarySetupOutcome::Timeout,
        RemoteErrorKind::Tls => ControlLibrarySetupOutcome::TlsFailure,
        RemoteErrorKind::Unavailable | RemoteErrorKind::Internal | RemoteErrorKind::RateLimited => {
            ControlLibrarySetupOutcome::ServerUnavailable
        }
        RemoteErrorKind::AuthRequired
        | RemoteErrorKind::Forbidden
        | RemoteErrorKind::DeviceRevoked => ControlLibrarySetupOutcome::AuthenticationRequired,
        RemoteErrorKind::Protocol
        | RemoteErrorKind::Rejected
        | RemoteErrorKind::NotFound
        | RemoteErrorKind::Integrity
        | RemoteErrorKind::Conflict
        | RemoteErrorKind::CheckpointConflict
        | RemoteErrorKind::InvalidEvidence
        | RemoteErrorKind::RebaselineRequired
        | RemoteErrorKind::BodyLimit
        | RemoteErrorKind::Redirect => ControlLibrarySetupOutcome::ProtocolError,
    }
}

fn map_auth_outcome(error: DesktopAuthError) -> ControlAuthOutcome {
    match error {
        DesktopAuthError::InvalidCredentials => ControlAuthOutcome::InvalidCredentials,
        DesktopAuthError::NetworkUnavailable => ControlAuthOutcome::NetworkUnavailable,
        DesktopAuthError::RateLimited => ControlAuthOutcome::RateLimited,
        DesktopAuthError::SecureStoreUnavailable => ControlAuthOutcome::SecureStoreUnavailable,
        DesktopAuthError::HostUnavailable
        | DesktopAuthError::ProfileUnavailable
        | DesktopAuthError::ProfileMismatch
        | DesktopAuthError::ServerUnavailable => ControlAuthOutcome::ServerUnavailable,
        DesktopAuthError::Protocol => ControlAuthOutcome::ProtocolError,
    }
}

fn process_status_code(status: DesktopProcessStatus) -> u8 {
    match status {
        DesktopProcessStatus::Starting => 0,
        DesktopProcessStatus::Running => 1,
        DesktopProcessStatus::Stopping => 2,
        DesktopProcessStatus::Stopped => 3,
        DesktopProcessStatus::Faulted => 4,
    }
}

fn process_status_from_code(code: u8) -> DesktopProcessStatus {
    match code {
        0 => DesktopProcessStatus::Starting,
        1 => DesktopProcessStatus::Running,
        2 => DesktopProcessStatus::Stopping,
        3 => DesktopProcessStatus::Stopped,
        _ => DesktopProcessStatus::Faulted,
    }
}

fn control_runtime_state(phase: SyncRuntimeLibraryPhase) -> ControlRuntimeState {
    match phase {
        SyncRuntimeLibraryPhase::Idle => ControlRuntimeState::Idle,
        SyncRuntimeLibraryPhase::Scheduled => ControlRuntimeState::Scheduled,
        SyncRuntimeLibraryPhase::Running => ControlRuntimeState::Running,
        SyncRuntimeLibraryPhase::BackingOff => ControlRuntimeState::BackingOff,
        SyncRuntimeLibraryPhase::AuthBlocked => ControlRuntimeState::AuthBlocked,
        SyncRuntimeLibraryPhase::RootBlocked => ControlRuntimeState::RootBlocked,
        SyncRuntimeLibraryPhase::Faulted => ControlRuntimeState::Faulted,
        SyncRuntimeLibraryPhase::Stopped => ControlRuntimeState::Stopped,
    }
}

fn control_root_state(state: RootAvailability) -> ControlRootState {
    match state {
        RootAvailability::Available => ControlRootState::Available,
        RootAvailability::Unavailable => ControlRootState::Unavailable,
        RootAvailability::Recovering => ControlRootState::Recovering,
    }
}

fn control_sync_control_state(state: SyncRuntimeControlState) -> ControlSyncControlState {
    match state {
        SyncRuntimeControlState::Running => ControlSyncControlState::Running,
        SyncRuntimeControlState::PausedByUser => ControlSyncControlState::PausedByUser,
    }
}

fn control_sync_outcome(outcome: SyncRuntimeOutcome) -> ControlSyncOutcome {
    match outcome {
        SyncRuntimeOutcome::Idle => ControlSyncOutcome::Idle,
        SyncRuntimeOutcome::Progress => ControlSyncOutcome::Progress,
        SyncRuntimeOutcome::ConflictBlocked => ControlSyncOutcome::ConflictBlocked,
        SyncRuntimeOutcome::Offline => ControlSyncOutcome::Offline,
        SyncRuntimeOutcome::ServerTransient => ControlSyncOutcome::ServerTransient,
        SyncRuntimeOutcome::RateLimited => ControlSyncOutcome::RateLimited,
        SyncRuntimeOutcome::AuthBlocked => ControlSyncOutcome::AuthBlocked,
        SyncRuntimeOutcome::RootUnavailable => ControlSyncOutcome::RootUnavailable,
        SyncRuntimeOutcome::RecoveryBlocked => ControlSyncOutcome::RecoveryBlocked,
        SyncRuntimeOutcome::FatalLocal => ControlSyncOutcome::FatalLocal,
        SyncRuntimeOutcome::Panicked => ControlSyncOutcome::Panicked,
    }
}

fn control_auth_state(status: SyncRuntimeLibraryStatus) -> ControlAuthState {
    if status.phase() == SyncRuntimeLibraryPhase::AuthBlocked
        || status.last_outcome() == Some(SyncRuntimeOutcome::AuthBlocked)
    {
        ControlAuthState::Blocked
    } else if matches!(
        status.last_outcome(),
        Some(
            SyncRuntimeOutcome::Idle
                | SyncRuntimeOutcome::Progress
                | SyncRuntimeOutcome::ConflictBlocked,
        )
    ) {
        // Idle is a completed authenticated cycle with no durable work. It is
        // distinct from the initial None state, which remains unknown until
        // the runtime has actually reached the server.
        ControlAuthState::Ready
    } else {
        ControlAuthState::Unknown
    }
}

fn control_conflict_state(status: SyncRuntimeLibraryStatus) -> ControlConflictState {
    match status.last_outcome() {
        Some(SyncRuntimeOutcome::ConflictBlocked) => ControlConflictState::Required,
        Some(_) => ControlConflictState::Clear,
        None => ControlConflictState::Unknown,
    }
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// A bounded local server owned by one production process.
pub struct DesktopControlServer {
    endpoint: DesktopControlEndpoint,
    transport: Option<BoundControlTransport>,
    stop: Arc<ControlServerStop>,
    task: Option<JoinHandle<Result<(), DesktopControlServerError>>>,
}

impl fmt::Debug for DesktopControlServer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopControlServer")
            .field("kind", &self.endpoint.kind())
            .field("started", &self.task.is_some())
            .finish_non_exhaustive()
    }
}

struct ControlServerStop {
    requested: AtomicBool,
    notify: Notify,
}

struct ActiveConnectionGuard {
    count: Arc<AtomicUsize>,
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::AcqRel);
    }
}

impl ControlServerStop {
    fn new() -> Self {
        Self {
            requested: AtomicBool::new(false),
            notify: Notify::new(),
        }
    }

    fn request(&self) {
        self.requested.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
}

impl DesktopControlServer {
    /// Bind the secure endpoint. On Linux this may remove only a verified
    /// stale socket at the exact generated path. No listener task is spawned
    /// until [`Self::start`] succeeds.
    pub async fn bind(endpoint: DesktopControlEndpoint) -> Result<Self, DesktopControlServerError> {
        let transport = BoundControlTransport::bind(&endpoint).await?;
        Ok(Self {
            endpoint,
            transport: Some(transport),
            stop: Arc::new(ControlServerStop::new()),
            task: None,
        })
    }

    #[must_use]
    pub const fn endpoint(&self) -> &DesktopControlEndpoint {
        &self.endpoint
    }

    /// Start the one accept/event supervisor for this bound endpoint.
    pub fn start(
        &mut self,
        control: DesktopControlHandle,
    ) -> Result<(), DesktopControlServerError> {
        if self.task.is_some() {
            return Err(DesktopControlServerError::AlreadyStarted);
        }
        let transport = self
            .transport
            .take()
            .ok_or(DesktopControlServerError::NotStarted)?;
        let stop = Arc::clone(&self.stop);
        control.set_control_ready(true);
        self.task = Some(tokio::spawn(async move {
            run_server(transport, control, stop).await
        }));
        Ok(())
    }

    /// Stop accepting connections and join only control/event tasks. The
    /// synchronization host is stopped by the outer process lifecycle, never
    /// from this method.
    pub async fn stop(&mut self) -> Result<(), DesktopControlServerError> {
        self.stop.request();
        if let Some(task) = self.task.take() {
            match task.await {
                Ok(result) => result,
                Err(_) => Err(DesktopControlServerError::TaskPanicked),
            }
        } else if let Some(transport) = self.transport.take() {
            cleanup_bound_transport(transport);
            Ok(())
        } else {
            Ok(())
        }
    }
}

impl Drop for DesktopControlServer {
    fn drop(&mut self) {
        self.stop.request();
        if let Some(transport) = self.transport.take() {
            cleanup_bound_transport(transport);
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

struct AcceptedControlConnection {
    io: BoxedControlIo,
}

trait ControlIo: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T> ControlIo for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

type BoxedControlIo = Box<dyn ControlIo>;

enum BoundControlTransport {
    #[cfg(unix)]
    Unix {
        listener: tokio::net::UnixListener,
        path: PathBuf,
        owner_uid: u32,
    },
    #[cfg(windows)]
    Windows {
        server: Option<tokio::net::windows::named_pipe::NamedPipeServer>,
        name: String,
    },
}

impl Drop for BoundControlTransport {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let Self::Unix { path, .. } = self;
            cleanup_unix_socket(path);
        }
    }
}

impl BoundControlTransport {
    async fn bind(endpoint: &DesktopControlEndpoint) -> Result<Self, DesktopControlServerError> {
        match endpoint {
            DesktopControlEndpoint::UnixSocket { path } => {
                #[cfg(unix)]
                {
                    return bind_unix_transport(path).await;
                }
                #[cfg(not(unix))]
                {
                    let _ = path;
                    Err(DesktopControlServerError::UnsupportedPlatform)
                }
            }
            DesktopControlEndpoint::NamedPipe { name } => {
                #[cfg(windows)]
                {
                    return bind_windows_transport(name);
                }
                #[cfg(not(windows))]
                {
                    let _ = name;
                    Err(DesktopControlServerError::UnsupportedPlatform)
                }
            }
        }
    }

    async fn accept(&mut self) -> Result<AcceptedControlConnection, DesktopControlServerError> {
        match self {
            #[cfg(unix)]
            Self::Unix {
                listener,
                owner_uid,
                ..
            } => {
                let (stream, _) = listener
                    .accept()
                    .await
                    .map_err(|_| DesktopControlServerError::ListenerFailed)?;
                let peer = stream
                    .peer_cred()
                    .map_err(|_| DesktopControlServerError::ListenerFailed)?;
                if peer.uid() != *owner_uid {
                    // Authentication is completed before the first frame is
                    // read. Dropping the stream intentionally leaks no status.
                    return Err(DesktopControlServerError::UnsafeEndpoint);
                }
                Ok(AcceptedControlConnection {
                    io: Box::new(stream),
                })
            }
            #[cfg(windows)]
            Self::Windows { server, name } => {
                let current = server
                    .take()
                    .ok_or(DesktopControlServerError::ListenerFailed)?;
                current
                    .connect()
                    .await
                    .map_err(|_| DesktopControlServerError::ListenerFailed)?;
                let connected = current;
                *server = Some(create_windows_pipe(name, false)?);
                Ok(AcceptedControlConnection {
                    io: Box::new(connected),
                })
            }
            #[cfg(not(any(unix, windows)))]
            _ => Err(DesktopControlServerError::UnsupportedPlatform),
        }
    }
}

fn cleanup_bound_transport(transport: BoundControlTransport) {
    drop(transport);
}

async fn run_server(
    mut transport: BoundControlTransport,
    control: DesktopControlHandle,
    stop: Arc<ControlServerStop>,
) -> Result<(), DesktopControlServerError> {
    let mut connections = JoinSet::new();
    let active_connections = Arc::new(AtomicUsize::new(0));
    let mut runtime_events = Some(control.runtime_events());
    let mut root_timer = time::interval(DESKTOP_CONTROL_STATUS_POLL_INTERVAL);
    let mut previous_roots = root_state_map(&control);

    let result = loop {
        if stop.is_requested() {
            break Ok(());
        }
        tokio::select! {
            biased;
            _ = stop.notify.notified() => break Ok(()),
            joined = connections.join_next(), if !connections.is_empty() => {
                if let Some(Err(_)) = joined {
                    // A malformed or disconnected client is isolated to its
                    // task. The runtime and other clients remain unaffected.
                }
            }
            accepted = transport.accept() => {
                match accepted {
                    Ok(connection) => {
                        if active_connections.load(Ordering::Acquire) >= DESKTOP_CONTROL_MAX_CONNECTIONS {
                            // Immediate close is bounded backpressure. A slow
                            // over-limit peer cannot pin an additional task.
                            drop(connection);
                            continue;
                        }
                        let active = Arc::clone(&active_connections);
                        active.fetch_add(1, Ordering::AcqRel);
                        let control = control.clone();
                        connections.spawn(async move {
                            let _active = ActiveConnectionGuard { count: active };
                            serve_connection(connection, control).await;
                        });
                    }
                    Err(DesktopControlServerError::UnsafeEndpoint) => {
                        // On Unix this is a wrong-user peer. It must be
                        // rejected without turning one client into process
                        // failure; accept the next connection.
                        continue;
                    }
                    Err(error) => {
                        control.request_shutdown_after_server_failure();
                        break Err(error);
                    }
                }
            }
            runtime_event = async {
                match runtime_events.as_mut() {
                    Some(receiver) => receiver.recv().await,
                    None => std::future::pending().await,
                }
            }, if runtime_events.is_some() => {
                match runtime_event {
                    Ok(event) => translate_runtime_event(&control, event),
                    Err(broadcast::error::RecvError::Lagged(dropped)) => {
                        control.emit(ControlEvent::Lagged { dropped_count: dropped });
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        // A closed source must disable this select branch;
                        // repeatedly receiving Closed would otherwise create a
                        // busy loop during a host/runtime teardown.
                        runtime_events = None;
                    }
                }
            }
            _ = root_timer.tick() => {
                let current = root_state_map(&control);
                for (library_id, state) in &current {
                    if previous_roots.get(library_id) != Some(state) {
                        control.emit(ControlEvent::RootAvailabilityChanged {
                            library_id: library_id.to_string(),
                            state: *state,
                        });
                        control.emit(ControlEvent::LibraryStatusChanged {
                            library_id: library_id.to_string(),
                        });
                    }
                }
                previous_roots = current;
            }
        }
    };

    control.emit(ControlEvent::ControlServerStopping);
    control.set_control_ready(false);
    connections.shutdown().await;
    drop(transport);
    result
}

async fn serve_connection(
    mut connection: AcceptedControlConnection,
    control: DesktopControlHandle,
) {
    let result = serve_connection_inner(&mut connection.io, &control).await;
    if let Err(error) = result
        && !matches!(error, DesktopControlClientError::Closed)
    {
        tracing::debug!(error = %error, "desktop control connection closed");
    }
}

async fn serve_connection_inner(
    io: &mut BoxedControlIo,
    control: &DesktopControlHandle,
) -> Result<(), DesktopControlClientError> {
    let hello_payload = time::timeout(CONTROL_HANDSHAKE_TIMEOUT, read_frame(io))
        .await
        .map_err(|_| DesktopControlClientError::Closed)?
        .map_err(DesktopControlClientError::Frame)?;
    let hello: ControlClientHello =
        decode_payload(&hello_payload).map_err(|_| DesktopControlClientError::Handshake)?;
    if hello.protocol_version != DESKTOP_CONTROL_PROTOCOL_VERSION {
        write_control_frame(
            io,
            &ControlServerHello {
                protocol_version: DESKTOP_CONTROL_PROTOCOL_VERSION,
                capabilities: Vec::new(),
                error: Some(ControlErrorCode::ProtocolVersionUnsupported),
            },
        )
        .await?;
        return Err(DesktopControlClientError::ProtocolVersionUnsupported);
    }
    write_control_frame(
        io,
        &ControlServerHello {
            protocol_version: DESKTOP_CONTROL_PROTOCOL_VERSION,
            capabilities: vec![
                ControlCapability::Ping,
                ControlCapability::ProcessStatus,
                ControlCapability::LibraryStatus,
                ControlCapability::SyncNow,
                ControlCapability::SyncControl,
                ControlCapability::Shutdown,
                ControlCapability::Events,
                ControlCapability::ProfileConfiguration,
                ControlCapability::LibrarySetup,
                ControlCapability::Attention,
            ],
            error: None,
        },
    )
    .await?;

    loop {
        let payload = match time::timeout(CONTROL_CONNECTION_IDLE_TIMEOUT, read_frame(io)).await {
            Ok(result) => result.map_err(DesktopControlClientError::Frame)?,
            Err(_) => return Err(DesktopControlClientError::Closed),
        };
        let request: ControlRequest = decode_payload(&payload)
            .map_err(|_| DesktopControlClientError::Frame(ControlFrameError::PayloadMalformed))?;
        if request.request_id == 0 {
            write_control_frame(
                io,
                &ControlResponse::error(0, ControlErrorCode::RequestInvalid),
            )
            .await?;
            continue;
        }

        if let ControlCommand::SubscribeEvents = request.command {
            let mut events = control.events();
            write_control_frame(
                io,
                &ControlResponse {
                    request_id: request.request_id,
                    body: ControlResponseBody::Subscribed {
                        event_capacity: DESKTOP_CONTROL_EVENT_CAPACITY as u32,
                    },
                },
            )
            .await?;
            stream_events(io, &mut events).await?;
            return Ok(());
        }

        let response = dispatch_request(control, request).await;
        let should_close = matches!(&response.body, ControlResponseBody::ShutdownAccepted);
        let write_result = write_control_frame(io, &response).await;
        if should_close {
            control.notify_shutdown_request();
        }
        write_result?;
        if should_close {
            return Ok(());
        }
    }
}

async fn stream_events(
    io: &mut BoxedControlIo,
    events: &mut broadcast::Receiver<ControlEvent>,
) -> Result<(), DesktopControlClientError> {
    loop {
        let event = match events.recv().await {
            Ok(event) => event,
            Err(broadcast::error::RecvError::Lagged(dropped)) => ControlEvent::Lagged {
                dropped_count: dropped,
            },
            Err(broadcast::error::RecvError::Closed) => {
                return Err(DesktopControlClientError::Closed);
            }
        };
        write_control_frame(
            io,
            &ControlResponse {
                request_id: 0,
                body: ControlResponseBody::Event { event },
            },
        )
        .await?;
    }
}

async fn write_control_frame<T>(
    io: &mut BoxedControlIo,
    value: &T,
) -> Result<(), DesktopControlClientError>
where
    T: Serialize,
{
    time::timeout(CONTROL_WRITE_TIMEOUT, write_frame(io, value))
        .await
        .map_err(|_| DesktopControlClientError::Closed)?
        .map_err(DesktopControlClientError::Frame)
}

async fn dispatch_request(
    control: &DesktopControlHandle,
    request: ControlRequest,
) -> ControlResponse {
    let request_id = request.request_id;
    let body = match request.command {
        ControlCommand::Ping => ControlResponseBody::Pong {
            status: control.process_status(),
        },
        ControlCommand::GetProcessStatus => ControlResponseBody::ProcessStatus {
            status: control.process_status_snapshot(),
        },
        ControlCommand::ListLibraries => ControlResponseBody::Libraries {
            status: control.library_list(),
        },
        ControlCommand::GetAttentionSnapshot => match control.attention_snapshot().await {
            Ok(snapshot) => ControlResponseBody::AttentionSnapshot { snapshot },
            Err(error) => return ControlResponse::error(request_id, error),
        },
        ControlCommand::ResolveConflict {
            library_id,
            conflict_id,
            intent_id,
            detected_at_ms,
            action,
        } => {
            let Ok(library_id) = library_id.parse::<LibraryId>() else {
                return ControlResponse::error(request_id, ControlErrorCode::RequestInvalid);
            };
            let Ok(conflict_id) = conflict_id.parse::<SyncConflictId>() else {
                return ControlResponse::error(request_id, ControlErrorCode::RequestInvalid);
            };
            let Ok(intent_id) = intent_id.parse::<OutboundIntentId>() else {
                return ControlResponse::error(request_id, ControlErrorCode::RequestInvalid);
            };
            ControlResponseBody::ConflictResolution {
                result: control
                    .resolve_conflict(library_id, conflict_id, intent_id, detected_at_ms, action)
                    .await,
            }
        }
        ControlCommand::GetLibraryStatus { library_id } => {
            let Ok(library_id) = library_id.parse::<LibraryId>() else {
                return ControlResponse::error(request_id, ControlErrorCode::RequestInvalid);
            };
            match control.library_status(library_id) {
                Some(status) => ControlResponseBody::LibraryStatus { status },
                None => {
                    return ControlResponse::error(request_id, ControlErrorCode::UnknownLibrary);
                }
            }
        }
        ControlCommand::SyncNow { library_id } => {
            let Ok(library_id) = library_id.parse::<LibraryId>() else {
                return ControlResponse::error(request_id, ControlErrorCode::RequestInvalid);
            };
            match control.sync_now(library_id) {
                SyncRuntimeWakeResult::Queued => ControlResponseBody::SyncNow {
                    result: ControlSyncScheduleResult::Queued,
                },
                SyncRuntimeWakeResult::Coalesced => ControlResponseBody::SyncNow {
                    result: ControlSyncScheduleResult::Coalesced,
                },
                SyncRuntimeWakeResult::AlreadyRunningFollowupRecorded => {
                    ControlResponseBody::SyncNow {
                        result: ControlSyncScheduleResult::AlreadyRunningFollowupRecorded,
                    }
                }
                SyncRuntimeWakeResult::PausedByUser => ControlResponseBody::SyncNow {
                    result: ControlSyncScheduleResult::Paused,
                },
                SyncRuntimeWakeResult::RuntimeStopped => {
                    return ControlResponse::error(request_id, ControlErrorCode::RuntimeStopped);
                }
                SyncRuntimeWakeResult::UnknownLibrary => {
                    return ControlResponse::error(request_id, ControlErrorCode::UnknownLibrary);
                }
            }
        }
        ControlCommand::GetSyncControlState => ControlResponseBody::SyncControlState {
            state: control.sync_control_state(),
        },
        ControlCommand::PauseSync => ControlResponseBody::SyncControl {
            result: control.set_sync_paused(true).await,
        },
        ControlCommand::ResumeSync => ControlResponseBody::SyncControl {
            result: control.set_sync_paused(false).await,
        },
        ControlCommand::SetupLibrary { name, root_path } => ControlResponseBody::LibrarySetup {
            outcome: control.setup_library(name, root_path).await,
        },
        ControlCommand::Authenticate { enrollment_token } => ControlResponseBody::Authenticate {
            result: control.authenticate(enrollment_token).await,
        },
        ControlCommand::SignOut => ControlResponseBody::SignOut {
            result: control.sign_out().await,
        },
        ControlCommand::Shutdown => {
            if control.request_shutdown() {
                ControlResponseBody::ShutdownAccepted
            } else {
                return ControlResponse::error(request_id, ControlErrorCode::ControlServerStopping);
            }
        }
        ControlCommand::SubscribeEvents => {
            // Handled before dispatch to preserve the receiver created before
            // the acknowledgement is written.
            return ControlResponse::error(request_id, ControlErrorCode::RequestInvalid);
        }
        ControlCommand::Unsupported => {
            return ControlResponse::error(request_id, ControlErrorCode::UnknownCommand);
        }
        ControlCommand::GetProfileConfiguration => match control.profile_configuration().await {
            Ok(configuration) => ControlResponseBody::ProfileConfiguration { configuration },
            Err(error) => return ControlResponse::error(request_id, error),
        },
        ControlCommand::ValidateProfileConfiguration {
            base_url,
            display_label,
        } => {
            let outcome = control
                .validate_profile_configuration(base_url, display_label)
                .await;
            ControlResponseBody::ProfileConfigurationResult { outcome }
        }
        ControlCommand::CreateOrConfigureProfile {
            profile_id,
            base_url,
            display_label,
        } => {
            let outcome = control
                .configure_profile(profile_id, base_url, display_label)
                .await;
            ControlResponseBody::ProfileConfigurationResult { outcome }
        }
        ControlCommand::UpdateProfileConfiguration {
            profile_id,
            base_url,
            display_label,
        } => {
            let outcome = control
                .configure_profile(profile_id, base_url, display_label)
                .await;
            ControlResponseBody::ProfileConfigurationResult { outcome }
        }
    };
    ControlResponse { request_id, body }
}

fn translate_runtime_event(control: &DesktopControlHandle, event: SyncRuntimeEvent) {
    match event {
        SyncRuntimeEvent::RuntimeStarted => {
            control.emit(ControlEvent::ProcessStateChanged {
                state: control.process_status(),
            });
        }
        SyncRuntimeEvent::RuntimeStopping | SyncRuntimeEvent::RuntimeStopped => {
            control.emit(ControlEvent::ProcessStateChanged {
                state: control.process_status(),
            });
        }
        SyncRuntimeEvent::RuntimeTaskFaulted => {
            control.emit(ControlEvent::ProcessStateChanged {
                // The runtime event is an invalidation signal. The outer
                // process lifecycle remains the sole owner of the process
                // status cell, so do not publish a Faulted event that would
                // disagree with GetProcessStatus until that owner changes it.
                state: control.process_status(),
            });
        }
        SyncRuntimeEvent::SyncControlStateChanged { state } => {
            control.emit(ControlEvent::SyncControlStateChanged {
                state: control_sync_control_state(state),
            });
        }
        SyncRuntimeEvent::CycleStarted { library_id }
        | SyncRuntimeEvent::BackoffScheduled { library_id, .. }
        | SyncRuntimeEvent::AuthBlocked { library_id }
        | SyncRuntimeEvent::LibraryFaulted { library_id } => {
            control.emit(ControlEvent::LibraryStatusChanged {
                library_id: library_id.to_string(),
            });
        }
        SyncRuntimeEvent::CycleFinished {
            library_id,
            outcome,
        } => {
            control.emit(ControlEvent::SyncCycleCompleted {
                library_id: library_id.to_string(),
                outcome: control_sync_outcome(outcome),
            });
            control.emit(ControlEvent::LibraryStatusChanged {
                library_id: library_id.to_string(),
            });
            control.emit(ControlEvent::AttentionStateChanged);
        }
    }
}

fn root_state_map(control: &DesktopControlHandle) -> BTreeMap<LibraryId, ControlRootState> {
    control
        .inner
        .host
        .root_statuses()
        .into_iter()
        .map(|(library_id, state)| (library_id, control_root_state(state)))
        .collect()
}

/// Reusable local control client. It holds one sequential connection and no
/// persistent identity or IPC session state.
pub struct DesktopControlClient {
    endpoint: DesktopControlEndpoint,
    io: BoxedControlIo,
    next_request_id: u64,
}

impl fmt::Debug for DesktopControlClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopControlClient")
            .field("kind", &self.endpoint.kind())
            .finish_non_exhaustive()
    }
}

impl DesktopControlClient {
    /// Connect and complete the version-1 handshake.
    pub async fn connect(
        endpoint: DesktopControlEndpoint,
    ) -> Result<Self, DesktopControlClientError> {
        #[cfg(unix)]
        if let DesktopControlEndpoint::UnixSocket { path } = &endpoint {
            validate_unix_client_endpoint(path).map_err(DesktopControlClientError::Endpoint)?;
        }
        let mut io = open_control_connection(&endpoint).await?;
        write_frame(
            &mut io,
            &ControlClientHello {
                protocol_version: DESKTOP_CONTROL_PROTOCOL_VERSION,
            },
        )
        .await
        .map_err(DesktopControlClientError::Frame)?;
        let payload = read_frame(&mut io)
            .await
            .map_err(DesktopControlClientError::Frame)?;
        let hello: ControlServerHello =
            decode_payload(&payload).map_err(|_| DesktopControlClientError::Handshake)?;
        if let Some(error) = hello.error {
            return Err(if error == ControlErrorCode::ProtocolVersionUnsupported {
                DesktopControlClientError::ProtocolVersionUnsupported
            } else {
                DesktopControlClientError::Server(error)
            });
        }
        if hello.protocol_version != DESKTOP_CONTROL_PROTOCOL_VERSION {
            return Err(DesktopControlClientError::ProtocolVersionUnsupported);
        }
        Ok(Self {
            endpoint,
            io,
            next_request_id: 1,
        })
    }

    /// Resolve the profile endpoint through the platform contract and connect.
    pub async fn connect_for_profile(
        platform: &dyn PlatformRuntime,
        profile_id: synveil_client_sync::ServerProfileId,
    ) -> Result<Self, DesktopControlClientError> {
        let endpoint = DesktopControlEndpoint::for_profile(platform, profile_id)
            .map_err(DesktopControlClientError::Endpoint)?;
        Self::connect(endpoint).await
    }

    #[must_use]
    pub const fn endpoint(&self) -> &DesktopControlEndpoint {
        &self.endpoint
    }

    pub async fn ping(&mut self) -> Result<DesktopProcessStatus, DesktopControlClientError> {
        match self.exchange(ControlCommand::Ping).await? {
            ControlResponseBody::Pong { status } => Ok(status),
            _ => Err(DesktopControlClientError::UnexpectedResponse),
        }
    }

    pub async fn process_status(
        &mut self,
    ) -> Result<ControlProcessStatus, DesktopControlClientError> {
        match self.exchange(ControlCommand::GetProcessStatus).await? {
            ControlResponseBody::ProcessStatus { status } => Ok(status),
            _ => Err(DesktopControlClientError::UnexpectedResponse),
        }
    }

    pub async fn list_libraries(
        &mut self,
    ) -> Result<ControlLibraryList, DesktopControlClientError> {
        match self.exchange(ControlCommand::ListLibraries).await? {
            ControlResponseBody::Libraries { status } => Ok(status),
            _ => Err(DesktopControlClientError::UnexpectedResponse),
        }
    }

    pub async fn attention_snapshot(
        &mut self,
    ) -> Result<ControlAttentionSnapshot, DesktopControlClientError> {
        match self.exchange(ControlCommand::GetAttentionSnapshot).await? {
            ControlResponseBody::AttentionSnapshot { snapshot } => Ok(snapshot),
            _ => Err(DesktopControlClientError::UnexpectedResponse),
        }
    }

    pub async fn resolve_conflict(
        &mut self,
        library_id: LibraryId,
        conflict_id: SyncConflictId,
        intent_id: OutboundIntentId,
        detected_at_ms: u64,
        action: ControlConflictAction,
    ) -> Result<ControlConflictResolutionOutcome, DesktopControlClientError> {
        match self
            .exchange(ControlCommand::ResolveConflict {
                library_id: library_id.to_string(),
                conflict_id: conflict_id.to_string(),
                intent_id: intent_id.to_string(),
                detected_at_ms,
                action,
            })
            .await?
        {
            ControlResponseBody::ConflictResolution { result } => Ok(result),
            _ => Err(DesktopControlClientError::UnexpectedResponse),
        }
    }

    pub async fn library_status(
        &mut self,
        library_id: LibraryId,
    ) -> Result<ControlLibraryStatus, DesktopControlClientError> {
        match self
            .exchange(ControlCommand::GetLibraryStatus {
                library_id: library_id.to_string(),
            })
            .await?
        {
            ControlResponseBody::LibraryStatus { status } => Ok(status),
            _ => Err(DesktopControlClientError::UnexpectedResponse),
        }
    }

    /// Schedule a canonical runtime wake. Success means accepted/coalesced,
    /// never that a synchronization cycle has completed.
    pub async fn sync_now(
        &mut self,
        library_id: LibraryId,
    ) -> Result<ControlSyncScheduleResult, DesktopControlClientError> {
        match self
            .exchange(ControlCommand::SyncNow {
                library_id: library_id.to_string(),
            })
            .await?
        {
            ControlResponseBody::SyncNow { result } => Ok(result),
            _ => Err(DesktopControlClientError::UnexpectedResponse),
        }
    }

    pub async fn sync_control_state(
        &mut self,
    ) -> Result<ControlSyncControlState, DesktopControlClientError> {
        match self.exchange(ControlCommand::GetSyncControlState).await? {
            ControlResponseBody::SyncControlState { state } => Ok(state),
            _ => Err(DesktopControlClientError::UnexpectedResponse),
        }
    }

    pub async fn pause_sync(
        &mut self,
    ) -> Result<ControlSyncControlResult, DesktopControlClientError> {
        match self.exchange(ControlCommand::PauseSync).await? {
            ControlResponseBody::SyncControl { result } => Ok(result),
            _ => Err(DesktopControlClientError::UnexpectedResponse),
        }
    }

    pub async fn resume_sync(
        &mut self,
    ) -> Result<ControlSyncControlResult, DesktopControlClientError> {
        match self.exchange(ControlCommand::ResumeSync).await? {
            ControlResponseBody::SyncControl { result } => Ok(result),
            _ => Err(DesktopControlClientError::UnexpectedResponse),
        }
    }

    pub async fn setup_library(
        &mut self,
        name: String,
        root_path: String,
    ) -> Result<ControlLibrarySetupOutcome, DesktopControlClientError> {
        match self
            .exchange(ControlCommand::SetupLibrary { name, root_path })
            .await?
        {
            ControlResponseBody::LibrarySetup { outcome } => Ok(outcome),
            _ => Err(DesktopControlClientError::UnexpectedResponse),
        }
    }

    /// Exchange one bounded enrollment secret through the process-owned host.
    /// The secret is copied into a zeroizing wire wrapper only for this
    /// request and is never retained by the client after the exchange.
    pub async fn authenticate(
        &mut self,
        enrollment_secret: &EnrollmentSecret,
    ) -> Result<ControlAuthOutcome, DesktopControlClientError> {
        match self
            .exchange(ControlCommand::Authenticate {
                enrollment_token: ControlAuthInput::from_secret(enrollment_secret),
            })
            .await?
        {
            ControlResponseBody::Authenticate { result } => Ok(result),
            _ => Err(DesktopControlClientError::UnexpectedResponse),
        }
    }

    /// Explicitly forget the profile-bound local credential through the
    /// process-owned lifecycle. It is not coupled to connection/process quit.
    pub async fn sign_out(&mut self) -> Result<ControlAuthOutcome, DesktopControlClientError> {
        match self.exchange(ControlCommand::SignOut).await? {
            ControlResponseBody::SignOut { result } => Ok(result),
            _ => Err(DesktopControlClientError::UnexpectedResponse),
        }
    }

    pub async fn shutdown(&mut self) -> Result<(), DesktopControlClientError> {
        match self.exchange(ControlCommand::Shutdown).await? {
            ControlResponseBody::ShutdownAccepted => Ok(()),
            _ => Err(DesktopControlClientError::UnexpectedResponse),
        }
    }

    pub async fn configure_profile(
        &mut self,
        profile_id: String,
        base_url: String,
        display_label: String,
    ) -> Result<ControlProfileConfigurationOutcome, DesktopControlClientError> {
        match self
            .exchange(ControlCommand::CreateOrConfigureProfile {
                profile_id,
                base_url,
                display_label,
            })
            .await?
        {
            ControlResponseBody::ProfileConfigurationResult { outcome } => Ok(outcome),
            _ => Err(DesktopControlClientError::UnexpectedResponse),
        }
    }

    pub async fn validate_profile_configuration(
        &mut self,
        base_url: String,
        display_label: String,
    ) -> Result<ControlProfileConfigurationOutcome, DesktopControlClientError> {
        match self
            .exchange(ControlCommand::ValidateProfileConfiguration {
                base_url,
                display_label,
            })
            .await?
        {
            ControlResponseBody::ProfileConfigurationResult { outcome } => Ok(outcome),
            _ => Err(DesktopControlClientError::UnexpectedResponse),
        }
    }

    pub async fn get_profile_configuration(
        &mut self,
    ) -> Result<ControlProfileConfiguration, DesktopControlClientError> {
        match self
            .exchange(ControlCommand::GetProfileConfiguration)
            .await?
        {
            ControlResponseBody::ProfileConfiguration { configuration } => Ok(configuration),
            _ => Err(DesktopControlClientError::UnexpectedResponse),
        }
    }

    /// Consume the request client and turn the connection into a bounded event
    /// stream. Status can be re-fetched with a newly connected client after a
    /// lag or process restart.
    pub async fn subscribe_events(
        mut self,
    ) -> Result<DesktopControlEventStream, DesktopControlClientError> {
        let response = self.exchange(ControlCommand::SubscribeEvents).await?;
        if !matches!(response, ControlResponseBody::Subscribed { .. }) {
            return Err(DesktopControlClientError::UnexpectedResponse);
        }
        Ok(DesktopControlEventStream { io: self.io })
    }

    async fn exchange(
        &mut self,
        command: ControlCommand,
    ) -> Result<ControlResponseBody, DesktopControlClientError> {
        let request_id = self.next_request_id;
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .ok_or(DesktopControlClientError::ResponseMismatch)?;
        write_frame(
            &mut self.io,
            &ControlRequest {
                request_id,
                command,
            },
        )
        .await
        .map_err(DesktopControlClientError::Frame)?;
        let payload = read_frame(&mut self.io)
            .await
            .map_err(DesktopControlClientError::Frame)?;
        let response: ControlResponse = decode_payload(&payload)
            .map_err(|_| DesktopControlClientError::Frame(ControlFrameError::PayloadMalformed))?;
        if response.request_id != request_id {
            return Err(DesktopControlClientError::ResponseMismatch);
        }
        match response.body {
            ControlResponseBody::Error { code } => Err(DesktopControlClientError::Server(code)),
            body => Ok(body),
        }
    }
}

/// Long-lived event connection returned by Subscribe Events.
pub struct DesktopControlEventStream {
    io: BoxedControlIo,
}

impl fmt::Debug for DesktopControlEventStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopControlEventStream")
            .finish_non_exhaustive()
    }
}

impl DesktopControlEventStream {
    pub async fn next_event(&mut self) -> Result<ControlEvent, DesktopControlClientError> {
        let payload = read_frame(&mut self.io)
            .await
            .map_err(DesktopControlClientError::Frame)?;
        let response: ControlResponse = decode_payload(&payload)
            .map_err(|_| DesktopControlClientError::Frame(ControlFrameError::PayloadMalformed))?;
        if response.request_id != 0 {
            return Err(DesktopControlClientError::ResponseMismatch);
        }
        match response.body {
            ControlResponseBody::Event { event } => Ok(event),
            ControlResponseBody::Error { code } => Err(DesktopControlClientError::Server(code)),
            _ => Err(DesktopControlClientError::UnexpectedResponse),
        }
    }
}

async fn open_control_connection(
    endpoint: &DesktopControlEndpoint,
) -> Result<BoxedControlIo, DesktopControlClientError> {
    match endpoint {
        DesktopControlEndpoint::UnixSocket { path } => {
            #[cfg(unix)]
            {
                let stream = time::timeout(
                    CONTROL_STALE_ENDPOINT_PROBE_TIMEOUT,
                    tokio::net::UnixStream::connect(path),
                )
                .await
                .map_err(|_| DesktopControlClientError::Connection)?
                .map_err(|_| DesktopControlClientError::Connection)?;
                Ok(Box::new(stream))
            }
            #[cfg(not(unix))]
            {
                let _ = path;
                Err(DesktopControlClientError::UnsupportedPlatform)
            }
        }
        DesktopControlEndpoint::NamedPipe { name } => {
            #[cfg(windows)]
            {
                use tokio::net::windows::named_pipe::ClientOptions;
                let client = ClientOptions::new()
                    .read(true)
                    .write(true)
                    .open(name)
                    .map_err(|_| DesktopControlClientError::Connection)?;
                return Ok(Box::new(client));
            }
            #[cfg(not(windows))]
            {
                let _ = name;
                Err(DesktopControlClientError::UnsupportedPlatform)
            }
        }
    }
}

#[cfg(unix)]
async fn bind_unix_transport(
    path: &Path,
) -> Result<BoundControlTransport, DesktopControlServerError> {
    let owner_uid = nix::unistd::geteuid().as_raw();
    let Some(control_dir) = path.parent() else {
        return Err(DesktopControlServerError::UnsafeEndpoint);
    };
    let Some(runtime_dir) = control_dir.parent() else {
        return Err(DesktopControlServerError::UnsafeEndpoint);
    };
    validate_runtime_directory(runtime_dir, owner_uid)?;
    ensure_secure_control_directory(control_dir, owner_uid)?;
    prepare_stale_socket(path, owner_uid).await?;

    let std_listener = match std::os::unix::net::UnixListener::bind(path) {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            // Re-inspect the exact path. A live listener is never unlinked.
            prepare_stale_socket(path, owner_uid).await?;
            std::os::unix::net::UnixListener::bind(path)
                .map_err(|_| DesktopControlServerError::EndpointAlreadyActive)?
        }
        Err(_) => return Err(DesktopControlServerError::BindFailed),
    };
    let cleanup = UnixSocketCleanupGuard::new(path);
    if std_listener.set_nonblocking(true).is_err() {
        drop(std_listener);
        drop(cleanup);
        return Err(DesktopControlServerError::BindFailed);
    }
    // Use the no-following pathname operation so a same-UID path replacement
    // cannot turn the permissions step into a chmod of an attacker-selected
    // target. The symlink check is repeated by verify_socket below.
    if nix::sys::stat::fchmodat(
        None,
        path,
        nix::sys::stat::Mode::from_bits_truncate(0o600),
        nix::sys::stat::FchmodatFlags::NoFollowSymlink,
    )
    .is_err()
    {
        drop(std_listener);
        drop(cleanup);
        return Err(DesktopControlServerError::BindFailed);
    }
    if let Err(error) = verify_socket(path, owner_uid) {
        drop(std_listener);
        drop(cleanup);
        return Err(error);
    }
    let listener = match tokio::net::UnixListener::from_std(std_listener) {
        Ok(listener) => listener,
        Err(_) => {
            drop(cleanup);
            cleanup_unix_socket(path);
            return Err(DesktopControlServerError::BindFailed);
        }
    };
    cleanup.disarm();
    Ok(BoundControlTransport::Unix {
        listener,
        path: path.to_path_buf(),
        owner_uid,
    })
}

#[cfg(unix)]
struct UnixSocketCleanupGuard<'a> {
    path: &'a Path,
    armed: bool,
}

#[cfg(unix)]
impl<'a> UnixSocketCleanupGuard<'a> {
    fn new(path: &'a Path) -> Self {
        Self { path, armed: true }
    }

    fn disarm(mut self) {
        self.armed = false;
    }
}

#[cfg(unix)]
impl Drop for UnixSocketCleanupGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            cleanup_unix_socket(self.path);
        }
    }
}

#[cfg(unix)]
fn validate_runtime_directory(
    path: &Path,
    owner_uid: u32,
) -> Result<(), DesktopControlServerError> {
    use std::{
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt},
    };

    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(DesktopControlServerError::UnsafeEndpoint);
    }
    validate_no_symlink_ancestors(path)?;
    if fs::symlink_metadata(path).is_err() {
        fs::create_dir_all(path)
            .map_err(|_| DesktopControlServerError::RuntimeDirectoryUnavailable)?;
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| DesktopControlServerError::RuntimeDirectoryUnavailable)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != owner_uid
        || metadata.permissions().mode() & 0o022 != 0
    {
        return Err(DesktopControlServerError::InsecureRuntimeDirectory);
    }
    Ok(())
}

#[cfg(unix)]
fn validate_no_symlink_ancestors(path: &Path) -> Result<(), DesktopControlServerError> {
    use std::fs;

    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => current.push(Path::new("/")),
            Component::Normal(value) => current.push(value),
            Component::ParentDir | Component::CurDir | Component::Prefix(_) => {
                return Err(DesktopControlServerError::UnsafeEndpoint);
            }
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(DesktopControlServerError::UnsafeEndpoint);
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(DesktopControlServerError::UnsafeEndpoint);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return Err(DesktopControlServerError::RuntimeDirectoryUnavailable),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn validate_unix_client_endpoint(path: &Path) -> Result<(), DesktopControlServerError> {
    use std::{
        fs,
        os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    };

    if !path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::CurDir | Component::Prefix(_)
            )
        })
    {
        return Err(DesktopControlServerError::UnsafeEndpoint);
    }
    let Some(control_dir) = path.parent() else {
        return Err(DesktopControlServerError::UnsafeEndpoint);
    };
    let Some(runtime_dir) = control_dir.parent() else {
        return Err(DesktopControlServerError::UnsafeEndpoint);
    };
    let owner_uid = nix::unistd::geteuid().as_raw();

    // Missing parents describe a process that is not currently running and
    // remain a normal bounded connection failure. Existing ancestors must be
    // canonical directories before the client considers the endpoint.
    validate_no_symlink_ancestors(runtime_dir)?;
    let runtime_metadata = match fs::symlink_metadata(runtime_dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(DesktopControlServerError::RuntimeDirectoryUnavailable),
    };
    if runtime_metadata.file_type().is_symlink() || !runtime_metadata.is_dir() {
        return Err(DesktopControlServerError::UnsafeEndpoint);
    }
    if runtime_metadata.uid() != owner_uid || runtime_metadata.permissions().mode() & 0o022 != 0 {
        return Err(DesktopControlServerError::InsecureRuntimeDirectory);
    }

    validate_no_symlink_ancestors(control_dir)?;
    let control_metadata = match fs::symlink_metadata(control_dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(DesktopControlServerError::RuntimeDirectoryUnavailable),
    };
    if control_metadata.file_type().is_symlink()
        || !control_metadata.is_dir()
        || control_metadata.uid() != owner_uid
    {
        return Err(DesktopControlServerError::UnsafeEndpoint);
    }
    if control_metadata.permissions().mode() & 0o077 != 0 {
        return Err(DesktopControlServerError::InsecureRuntimeDirectory);
    }

    let endpoint_metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(DesktopControlServerError::EndpointStateUnknown),
    };
    if endpoint_metadata.file_type().is_symlink()
        || !endpoint_metadata.file_type().is_socket()
        || endpoint_metadata.uid() != owner_uid
    {
        return Err(DesktopControlServerError::UnsafeEndpoint);
    }
    Ok(())
}

#[cfg(unix)]
fn ensure_secure_control_directory(
    path: &Path,
    owner_uid: u32,
) -> Result<(), DesktopControlServerError> {
    use std::{
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt},
    };

    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() || metadata.uid() != owner_uid {
            return Err(DesktopControlServerError::UnsafeEndpoint);
        }
    } else {
        fs::create_dir(path).map_err(|_| DesktopControlServerError::RuntimeDirectoryUnavailable)?;
    }
    nix::sys::stat::fchmodat(
        None,
        path,
        nix::sys::stat::Mode::from_bits_truncate(0o700),
        nix::sys::stat::FchmodatFlags::NoFollowSymlink,
    )
    .map_err(|_| DesktopControlServerError::InsecureRuntimeDirectory)?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| DesktopControlServerError::InsecureRuntimeDirectory)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != owner_uid
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.permissions().mode() & 0o700 != 0o700
    {
        return Err(DesktopControlServerError::InsecureRuntimeDirectory);
    }
    Ok(())
}

#[cfg(unix)]
async fn prepare_stale_socket(
    path: &Path,
    owner_uid: u32,
) -> Result<(), DesktopControlServerError> {
    use std::{
        fs,
        os::unix::fs::{FileTypeExt, MetadataExt},
    };

    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(DesktopControlServerError::UnsafeEndpoint),
    };
    if metadata.file_type().is_symlink() || !metadata.file_type().is_socket() {
        return Err(DesktopControlServerError::UnsafeEndpoint);
    }
    if metadata.uid() != owner_uid {
        return Err(DesktopControlServerError::UnsafeEndpoint);
    }

    match time::timeout(
        CONTROL_STALE_ENDPOINT_PROBE_TIMEOUT,
        tokio::net::UnixStream::connect(path),
    )
    .await
    {
        Ok(Ok(stream)) => {
            drop(stream);
            Err(DesktopControlServerError::EndpointAlreadyActive)
        }
        Ok(Err(error))
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
            ) =>
        {
            let latest = fs::symlink_metadata(path)
                .map_err(|_| DesktopControlServerError::EndpointStateUnknown)?;
            if latest.file_type().is_symlink()
                || !latest.file_type().is_socket()
                || latest.uid() != owner_uid
            {
                return Err(DesktopControlServerError::UnsafeEndpoint);
            }
            fs::remove_file(path).map_err(|_| DesktopControlServerError::EndpointStateUnknown)
        }
        Ok(Err(_)) | Err(_) => Err(DesktopControlServerError::EndpointStateUnknown),
    }
}

#[cfg(unix)]
fn verify_socket(path: &Path, owner_uid: u32) -> Result<(), DesktopControlServerError> {
    use std::{
        fs,
        os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    };

    let metadata = fs::symlink_metadata(path).map_err(|_| DesktopControlServerError::BindFailed)?;
    if metadata.file_type().is_symlink()
        || !metadata.file_type().is_socket()
        || metadata.uid() != owner_uid
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.permissions().mode() & 0o600 != 0o600
    {
        return Err(DesktopControlServerError::UnsafeEndpoint);
    }
    Ok(())
}

#[cfg(unix)]
fn cleanup_unix_socket(path: &Path) {
    use std::{
        fs,
        os::unix::fs::{FileTypeExt, MetadataExt},
    };

    let owner_uid = nix::unistd::geteuid().as_raw();
    if let Ok(metadata) = fs::symlink_metadata(path)
        && !metadata.file_type().is_symlink()
        && metadata.file_type().is_socket()
        && metadata.uid() == owner_uid
    {
        let _ = fs::remove_file(path);
    }
}

#[cfg(windows)]
fn bind_windows_transport(name: &str) -> Result<BoundControlTransport, DesktopControlServerError> {
    validate_pipe_name(name)?;
    Ok(BoundControlTransport::Windows {
        server: Some(create_windows_pipe(name, true)?),
        name: name.to_owned(),
    })
}

#[cfg(windows)]
fn create_windows_pipe(
    name: &str,
    first_instance: bool,
) -> Result<tokio::net::windows::named_pipe::NamedPipeServer, DesktopControlServerError> {
    use std::ffi::c_void;
    use tokio::net::windows::named_pipe::ServerOptions;
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;

    let descriptor = WindowsCurrentUserSecurityDescriptor::new()?;
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.raw as *mut c_void,
        bInheritHandle: 0,
    };
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first_instance)
        .reject_remote_clients(true)
        .max_instances(DESKTOP_CONTROL_MAX_CONNECTIONS)
        .in_buffer_size(DESKTOP_CONTROL_MAX_FRAME_BYTES as u32)
        .out_buffer_size(DESKTOP_CONTROL_MAX_FRAME_BYTES as u32);
    // SAFETY: `attributes` points to a valid SECURITY_ATTRIBUTES whose
    // descriptor remains alive for the synchronous CreateNamedPipeW call. The
    // returned Tokio server takes ownership only of the returned pipe handle.
    unsafe {
        options
            .create_with_security_attributes_raw(
                name,
                &mut attributes as *mut SECURITY_ATTRIBUTES as *mut c_void,
            )
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::PermissionDenied && first_instance {
                    DesktopControlServerError::EndpointAlreadyActive
                } else {
                    DesktopControlServerError::BindFailed
                }
            })
    }
}

#[cfg(windows)]
struct WindowsCurrentUserSecurityDescriptor {
    raw: PSECURITY_DESCRIPTOR,
}

#[cfg(windows)]
impl WindowsCurrentUserSecurityDescriptor {
    fn new() -> Result<Self, DesktopControlServerError> {
        use windows_sys::Win32::{
            Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
            System::SystemServices::SECURITY_DESCRIPTOR_REVISION,
        };

        // OW is the Windows Owner Rights SID. The protected DACL grants full
        // control only to the object owner (the creating process's user) and
        // has no Everyone/Anonymous ACE.
        let sddl: Vec<u16> = format!("{CONTROL_PIPE_SECURITY_DESCRIPTOR}\0")
            .encode_utf16()
            .collect();
        let mut raw: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let mut size = 0_u32;
        // SAFETY: Windows receives a terminated UTF-16 SDDL string and valid
        // output pointers. It allocates the descriptor with LocalAlloc; Drop
        // releases it with LocalFree.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SECURITY_DESCRIPTOR_REVISION,
                &mut raw,
                &mut size,
            )
        };
        if ok == 0 || raw.is_null() || size == 0 {
            return Err(DesktopControlServerError::SecurityDescriptorUnavailable);
        }
        Ok(Self { raw })
    }
}

#[cfg(windows)]
impl Drop for WindowsCurrentUserSecurityDescriptor {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            // SAFETY: `raw` is exactly the LocalAlloc-owned descriptor returned
            // by ConvertStringSecurityDescriptorToSecurityDescriptorW.
            unsafe {
                let _ = windows_sys::Win32::Foundation::LocalFree(self.raw);
            }
        }
    }
}

/// Convenience aliases for callers that prefer the shorter names.
pub type ControlClient = DesktopControlClient;
pub type ControlClientError = DesktopControlClientError;
pub type ControlEventStream = DesktopControlEventStream;
pub type ControlServer = DesktopControlServer;

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::{
        fs,
        os::unix::fs::{FileTypeExt, PermissionsExt},
        sync::Arc,
    };

    #[cfg(unix)]
    use synveil_client_sync::{
        DesktopSyncHost, DesktopSyncHostConfig, LocalStateConfig, LocalStateStore,
    };
    #[cfg(unix)]
    use synveil_platform::UnsupportedSecureSecretStore;

    use super::*;

    fn test_profile() -> synveil_client_sync::ServerProfileId {
        synveil_client_sync::ServerProfileId::new()
    }

    #[test]
    fn framing_rejects_zero_and_oversized_payloads_before_body_allocation() {
        assert_eq!(encode_frame(&[]), Err(ControlFrameError::ZeroLength));
        assert_eq!(
            encode_frame(&vec![0_u8; DESKTOP_CONTROL_MAX_FRAME_BYTES + 1]),
            Err(ControlFrameError::TooLarge)
        );
    }

    #[test]
    fn event_fanout_is_bounded_and_reports_lag_without_blocking_producers() {
        let (sender, mut slow) = broadcast::channel(DESKTOP_CONTROL_EVENT_CAPACITY);
        let mut fast = sender.subscribe();

        for _ in 0..10_000 {
            sender
                .send(ControlEvent::ControlServerStopping)
                .expect("both bounded receivers remain subscribed");
            assert!(matches!(
                fast.try_recv(),
                Ok(ControlEvent::ControlServerStopping)
            ));
        }

        let dropped = match slow.try_recv() {
            Err(broadcast::error::TryRecvError::Lagged(dropped)) => dropped,
            other => panic!("expected bounded lag notification, got {other:?}"),
        };
        assert!(dropped >= (10_000 - DESKTOP_CONTROL_EVENT_CAPACITY) as u64);
    }

    #[test]
    fn endpoint_names_are_profile_scoped_and_do_not_contain_secrets() {
        let id = test_profile();
        let endpoint = DesktopControlEndpoint::NamedPipe {
            name: windows_pipe_name(id),
        };
        let debug = format!("{endpoint:?}");
        assert_eq!(endpoint.kind(), ControlEndpointKind::NamedPipe);
        assert!(
            endpoint
                .named_pipe()
                .is_some_and(|name| name.contains(&id.to_string()))
        );
        assert!(!debug.contains("\\\\.\\pipe"));
        assert!(!debug.contains("secret"));
    }

    #[test]
    fn windows_pipe_security_policy_is_owner_only_without_everyone_or_anonymous() {
        assert!(CONTROL_PIPE_SECURITY_DESCRIPTOR.contains("OW"));
        assert!(!CONTROL_PIPE_SECURITY_DESCRIPTOR.contains("WD"));
        assert!(!CONTROL_PIPE_SECURITY_DESCRIPTOR.contains("AN"));
        assert!(validate_pipe_name(&windows_pipe_name(test_profile())).is_ok());
    }

    #[test]
    fn protocol_status_and_events_serialize_without_private_fields() {
        let library = ControlLibraryStatus {
            library_id: LibraryId::new().to_string(),
            runtime_state: ControlRuntimeState::Idle,
            root_state: ControlRootState::Available,
            auth_state: ControlAuthState::Ready,
            conflict_state: ControlConflictState::Clear,
            next_due_ms: Some(1),
            last_outcome: Some(ControlSyncOutcome::Idle),
            wake_pending: false,
            transient_failures: 0,
        };
        let response = ControlResponse {
            request_id: 7,
            body: ControlResponseBody::Libraries {
                status: ControlLibraryList {
                    libraries: vec![library],
                    truncated: false,
                },
            },
        };
        let event = ControlEvent::RootAvailabilityChanged {
            library_id: LibraryId::new().to_string(),
            state: ControlRootState::Unavailable,
        };
        let serialized = serde_json::to_string(&(response, event)).expect("wire status");
        for forbidden in [
            "credential",
            "token",
            "cookie",
            "authorization",
            "root-path",
            "server-url",
        ] {
            assert!(!serialized.to_ascii_lowercase().contains(forbidden));
        }
    }

    #[test]
    fn authentication_wire_input_is_bounded_and_debug_redacted() {
        let secret = EnrollmentSecret::from_bytes([0x5a; 32]);
        let input = ControlAuthInput::from_secret(&secret);
        let request = ControlRequest {
            request_id: 9,
            command: ControlCommand::Authenticate {
                enrollment_token: input.clone(),
            },
        };
        let debug = format!("{request:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains(secret.expose_secret()));

        let encoded = serde_json::to_string(&request).expect("auth request serializes");
        assert!(encoded.contains(secret.expose_secret()));
        let decoded: ControlRequest = serde_json::from_str(&encoded).expect("auth request decodes");
        assert_eq!(decoded, request);

        let oversized = format!(
            r#"{{"request_id":1,"command":{{"kind":"authenticate","enrollment_token":"{}"}}}}"#,
            "x".repeat(DEVICE_SECRET_ENCODED_BYTES + 1)
        );
        assert!(serde_json::from_str::<ControlRequest>(&oversized).is_err());
    }

    #[test]
    fn authentication_operation_admission_stays_held_until_guard_drops() {
        let in_flight = AtomicBool::new(false);
        let guard = AuthOperationGuard::try_acquire(&in_flight).expect("first admission");
        assert!(in_flight.load(Ordering::Acquire));
        assert!(AuthOperationGuard::try_acquire(&in_flight).is_none());
        assert!(in_flight.load(Ordering::Acquire));
        drop(guard);
        assert!(!in_flight.load(Ordering::Acquire));
        assert!(AuthOperationGuard::try_acquire(&in_flight).is_some());
    }

    #[test]
    fn attention_operation_admission_stays_held_until_guard_drops() {
        let in_flight = AtomicBool::new(false);
        let guard = AttentionOperationGuard::try_acquire(&in_flight).expect("first admission");
        assert!(in_flight.load(Ordering::Acquire));
        assert!(AttentionOperationGuard::try_acquire(&in_flight).is_none());
        drop(guard);
        assert!(!in_flight.load(Ordering::Acquire));
        assert!(AttentionOperationGuard::try_acquire(&in_flight).is_some());
    }

    #[test]
    fn attention_path_projection_is_utf8_safe_and_bounded() {
        let path = synveil_client_sync::ManagedRelativePath::new(format!(
            "folder/{}file.txt",
            "x/".repeat(CONTROL_MAX_ATTENTION_PATH_BYTES)
        ))
        .expect("fixture path");
        let projected = bounded_attention_path(Some(&path)).expect("bounded path");
        assert!(projected.len() <= CONTROL_MAX_ATTENTION_PATH_BYTES);
        assert!(projected.ends_with('…'));
        assert!(projected.is_char_boundary(projected.len()));
    }

    #[tokio::test]
    async fn invalid_auth_input_is_rejected_before_host_access() {
        let root = std::env::temp_dir().join(format!("sv96-auth-invalid-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).expect("fixture root");
        let state = Arc::new(
            LocalStateStore::open(&LocalStateConfig::new(root.join("state.sqlite3")))
                .await
                .expect("fixture state"),
        );
        let host = DesktopSyncHost::new(
            state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            DesktopSyncHostConfig::default(),
            Vec::<synveil_client_sync::DesktopSyncLibraryConfig>::new(),
        )
        .await
        .expect("empty host");
        host.start().await.expect("host start");
        let control = DesktopControlHandle::new(host.handle(), DesktopProcessStatus::Running);
        let response = dispatch_request(
            &control,
            ControlRequest {
                request_id: 1,
                command: ControlCommand::Authenticate {
                    enrollment_token: ControlAuthInput(Zeroizing::new("not-a-token".to_owned())),
                },
            },
        )
        .await;
        assert_eq!(
            response.body,
            ControlResponseBody::Authenticate {
                result: ControlAuthOutcome::InvalidCredentials,
            }
        );
        assert!(
            state
                .profile_enrollment(test_profile())
                .await
                .expect("state read")
                .is_none()
        );
        host.shutdown().await.expect("host stop");
        state.close_pool().await;
        fs::remove_dir_all(&root).expect("fixture cleanup");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn active_socket_is_never_replaced_and_stale_socket_is_recovered() {
        let root = std::env::temp_dir().join(format!("sv96-active-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).expect("fixture root");
        let path = root.join(CONTROL_ENDPOINT_DIRECTORY).join("control.sock");
        let endpoint = DesktopControlEndpoint::UnixSocket { path: path.clone() };

        let first = DesktopControlServer::bind(endpoint.clone())
            .await
            .expect("first endpoint bind");
        let second = DesktopControlServer::bind(endpoint.clone()).await;
        assert!(matches!(
            second,
            Err(DesktopControlServerError::EndpointAlreadyActive)
        ));
        assert!(
            fs::symlink_metadata(&path)
                .expect("active socket remains")
                .file_type()
                .is_socket()
        );
        drop(first);
        assert!(fs::symlink_metadata(&path).is_err());

        let stale_listener = std::os::unix::net::UnixListener::bind(&path).expect("stale bind");
        drop(stale_listener);
        assert!(
            fs::symlink_metadata(&path)
                .expect("stale socket remains")
                .file_type()
                .is_socket()
        );
        let mut replacement = DesktopControlServer::bind(endpoint)
            .await
            .expect("stale endpoint recovery");
        assert!(
            fs::symlink_metadata(&path)
                .expect("replacement socket")
                .file_type()
                .is_socket()
        );
        replacement.stop().await.expect("replacement stop");
        fs::remove_dir_all(&root).expect("fixture cleanup");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlink_regular_file_and_directory_endpoints_are_refused() {
        let root = std::env::temp_dir().join(format!("sv96-unsafe-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(root.join(CONTROL_ENDPOINT_DIRECTORY)).expect("fixture directory");
        let path = root.join(CONTROL_ENDPOINT_DIRECTORY).join("control.sock");
        let target = root.join("target");

        fs::write(&target, b"not a socket").expect("regular target");
        std::os::unix::fs::symlink(&target, &path).expect("symlink endpoint");
        let endpoint = DesktopControlEndpoint::UnixSocket { path: path.clone() };
        assert!(matches!(
            DesktopControlServer::bind(endpoint.clone()).await,
            Err(DesktopControlServerError::UnsafeEndpoint)
        ));
        assert!(
            fs::symlink_metadata(&path)
                .expect("symlink remains")
                .file_type()
                .is_symlink()
        );

        fs::remove_file(&path).expect("remove test symlink");
        fs::write(&path, b"regular endpoint").expect("regular endpoint");
        assert!(matches!(
            DesktopControlServer::bind(endpoint.clone()).await,
            Err(DesktopControlServerError::UnsafeEndpoint)
        ));
        fs::remove_file(&path).expect("remove regular endpoint");

        fs::create_dir(&path).expect("directory endpoint");
        assert!(matches!(
            DesktopControlServer::bind(endpoint).await,
            Err(DesktopControlServerError::UnsafeEndpoint)
        ));
        fs::remove_dir(&path).expect("remove directory endpoint");
        fs::remove_dir_all(&root).expect("fixture cleanup");
    }

    #[test]
    #[cfg(unix)]
    fn secure_directory_and_socket_policy_is_explicit() {
        let root =
            std::env::temp_dir().join(format!("synveil-control-security-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).expect("fixture root");
        let control_dir = root.join(CONTROL_ENDPOINT_DIRECTORY);
        ensure_secure_control_directory(&control_dir, nix::unistd::geteuid().as_raw())
            .expect("secure control directory");
        let metadata = fs::symlink_metadata(&control_dir).expect("control metadata");
        assert!(metadata.is_dir());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        fs::remove_dir(&control_dir).expect("remove fixture control directory");
        fs::remove_dir(&root).expect("remove fixture root");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_server_and_client_share_one_host_control_surface() {
        let root =
            std::env::temp_dir().join(format!("synveil-control-local-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).expect("fixture root");
        let state = Arc::new(
            LocalStateStore::open(&LocalStateConfig::new(root.join("state.sqlite3")))
                .await
                .expect("fixture state"),
        );
        let host = DesktopSyncHost::new(
            state.clone(),
            Arc::new(UnsupportedSecureSecretStore::new()),
            DesktopSyncHostConfig::default(),
            Vec::<synveil_client_sync::DesktopSyncLibraryConfig>::new(),
        )
        .await
        .expect("empty host");
        host.start().await.expect("host start");
        let control = DesktopControlHandle::new(host.handle(), DesktopProcessStatus::Running);
        let endpoint = DesktopControlEndpoint::UnixSocket {
            path: root.join(CONTROL_ENDPOINT_DIRECTORY).join("test.sock"),
        };
        let mut server = DesktopControlServer::bind(endpoint.clone())
            .await
            .expect("control bind");
        server.start(control).expect("control start");
        let mut client = DesktopControlClient::connect(endpoint)
            .await
            .expect("control connect");
        assert_eq!(
            client.ping().await.expect("ping"),
            DesktopProcessStatus::Running
        );
        assert_eq!(
            client.process_status().await.expect("process status").state,
            DesktopProcessStatus::Running
        );
        assert!(
            client
                .list_libraries()
                .await
                .expect("library list")
                .libraries
                .is_empty()
        );
        server.stop().await.expect("control stop");
        host.shutdown().await.expect("host stop");
        state.close_pool().await;
        fs::remove_dir_all(&root).expect("fixture cleanup");
    }

    #[tokio::test]
    async fn unknown_commands_return_typed_errors() {
        let request = ControlRequest {
            request_id: 1,
            command: ControlCommand::Unsupported,
        };
        let json = serde_json::to_vec(&request).expect("request");
        let decoded: ControlRequest = serde_json::from_slice(&json).expect("decode");
        assert_eq!(decoded, request);
    }
}
