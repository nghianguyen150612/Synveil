//! UI-agnostic controller for the Prompt 96 local control plane.
//!
//! The controller is deliberately a presentation boundary.  It owns an
//! ephemeral latest-state model, one bounded command admission path, and the
//! lifecycle of its IPC connections.  It does not open the client SQLite
//! store, construct a sync host/runtime, inspect a root, or call a
//! synchronization engine.  All process and library facts come from the
//! redacted Prompt 96 control protocol.

use std::{
    collections::BTreeSet,
    fmt,
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use synveil_client_sync::ServerProfileId;
use synveil_core::{EnrollmentSecret, LibraryId, OutboundIntentId, SyncConflictId};
use synveil_platform::PlatformRuntime;
use tokio::{
    sync::{Mutex, Notify, mpsc, oneshot, watch},
    task::JoinHandle,
    time::{self, MissedTickBehavior},
};

use crate::control::DesktopControlClientError;
use crate::{
    ControlAttentionItem, ControlAttentionItemKind, ControlAttentionSnapshot, ControlAuthOutcome,
    ControlAuthState, ControlConflictAction, ControlConflictResolutionOutcome,
    ControlConflictState, ControlErrorCode, ControlEvent, ControlLibraryList, ControlLibraryStatus,
    ControlProcessStatus, ControlProfileConfigurationOutcome, ControlRootState,
    ControlRuntimeState, ControlSyncControlResult, ControlSyncControlState, ControlSyncOutcome,
    ControlSyncScheduleResult, DesktopControlClient, DesktopControlEndpoint,
    DesktopControlEventStream, DesktopProcessStatus,
};

/// Readiness marker for the native controller model/client layer.
pub const DESKTOP_CONTROLLER_CORE_READINESS: &str = "SYNVEIL_DESKTOP_CONTROLLER_CORE_READY";

/// The controller never admits more than this many commands awaiting the one
/// sequential Prompt 96 request path.  A full channel rejects admission; it
/// never grows with UI click rate.
pub const DESKTOP_CONTROLLER_COMMAND_CAPACITY: usize = 8;

/// Default reconnect delay for a local process that is not currently running.
pub const DEFAULT_DESKTOP_CONTROLLER_RECONNECT_INITIAL: Duration = Duration::from_millis(250);

/// Maximum reconnect delay.  The sequence is 250 ms, 500 ms, 1 s, 2 s, 4 s,
/// then 5 s for subsequent attempts.
pub const DEFAULT_DESKTOP_CONTROLLER_RECONNECT_CAP: Duration = Duration::from_secs(5);

/// Default presentation-only safety refresh interval.
pub const DEFAULT_DESKTOP_CONTROLLER_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

/// Maximum time spent waiting for one bounded local IPC operation.
pub const DEFAULT_DESKTOP_CONTROLLER_OPERATION_TIMEOUT: Duration = Duration::from_secs(2);

/// Maximum time a caller waits for an admitted command result.  If the wait
/// expires after admission, the command result is conservatively unknown and
/// the command is never replayed.
pub const DEFAULT_DESKTOP_CONTROLLER_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

/// Timing knobs are explicit so tests and embedders can use short deterministic
/// timers without changing production reconnect behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesktopControllerTiming {
    reconnect_initial: Duration,
    reconnect_cap: Duration,
    refresh_interval: Duration,
    operation_timeout: Duration,
    command_timeout: Duration,
}

impl Default for DesktopControllerTiming {
    fn default() -> Self {
        Self {
            reconnect_initial: DEFAULT_DESKTOP_CONTROLLER_RECONNECT_INITIAL,
            reconnect_cap: DEFAULT_DESKTOP_CONTROLLER_RECONNECT_CAP,
            refresh_interval: DEFAULT_DESKTOP_CONTROLLER_REFRESH_INTERVAL,
            operation_timeout: DEFAULT_DESKTOP_CONTROLLER_OPERATION_TIMEOUT,
            command_timeout: DEFAULT_DESKTOP_CONTROLLER_COMMAND_TIMEOUT,
        }
    }
}

impl DesktopControllerTiming {
    /// Build timing policy, rejecting zero durations and a cap below the first
    /// reconnect delay.
    pub fn new(
        reconnect_initial: Duration,
        reconnect_cap: Duration,
        refresh_interval: Duration,
        operation_timeout: Duration,
        command_timeout: Duration,
    ) -> Result<Self, DesktopControllerError> {
        if reconnect_initial.is_zero()
            || reconnect_cap.is_zero()
            || reconnect_cap < reconnect_initial
            || refresh_interval.is_zero()
            || operation_timeout.is_zero()
            || command_timeout.is_zero()
        {
            return Err(DesktopControllerError::InvalidTiming);
        }
        Ok(Self {
            reconnect_initial,
            reconnect_cap,
            refresh_interval,
            operation_timeout,
            command_timeout,
        })
    }

    #[must_use]
    pub const fn reconnect_initial(self) -> Duration {
        self.reconnect_initial
    }

    #[must_use]
    pub const fn reconnect_cap(self) -> Duration {
        self.reconnect_cap
    }

    #[must_use]
    pub const fn refresh_interval(self) -> Duration {
        self.refresh_interval
    }

    #[must_use]
    pub const fn operation_timeout(self) -> Duration {
        self.operation_timeout
    }

    #[must_use]
    pub const fn command_timeout(self) -> Duration {
        self.command_timeout
    }
}

/// Controller connectivity is intentionally separate from process and library
/// status.  A connected IPC socket does not imply that the process is running.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Stopping,
    Stopped,
    ProtocolIncompatible,
    Faulted,
}

/// Global user-controlled synchronization state. `Unknown` is retained until
/// the process-owned control plane provides a canonical state snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerSyncControlState {
    Running,
    PausedByUser,
    Unknown,
}

/// Freshness of the last canonical process/library snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerFreshness {
    Fresh,
    Stale,
    Unavailable,
}

/// Safe, stable controller error category.  Variants intentionally do not
/// retain an OS path, pipe name, credential, or server diagnostic string.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerErrorKind {
    EndpointUnavailable,
    EndpointSecurity,
    ProtocolIncompatible,
    ConnectionLost,
    MalformedServerResponse,
    CommandRejected,
    UnsupportedPlatform,
    Stopped,
    Internal,
}

impl DesktopControllerErrorKind {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::EndpointUnavailable => "DESKTOP_CONTROLLER_ENDPOINT_UNAVAILABLE",
            Self::EndpointSecurity => "DESKTOP_CONTROLLER_ENDPOINT_SECURITY",
            Self::ProtocolIncompatible => "DESKTOP_CONTROLLER_PROTOCOL_INCOMPATIBLE",
            Self::ConnectionLost => "DESKTOP_CONTROLLER_CONNECTION_LOST",
            Self::MalformedServerResponse => "DESKTOP_CONTROLLER_RESPONSE_MALFORMED",
            Self::CommandRejected => "DESKTOP_CONTROLLER_COMMAND_REJECTED",
            Self::UnsupportedPlatform => "DESKTOP_CONTROLLER_PLATFORM_UNSUPPORTED",
            Self::Stopped => "DESKTOP_CONTROLLER_STOPPED",
            Self::Internal => "DESKTOP_CONTROLLER_INTERNAL",
        }
    }
}

impl fmt::Display for DesktopControllerErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

/// Errors for controller lifecycle/configuration operations.  Runtime IPC
/// outcomes are returned as [`DesktopControllerCommandResult`] categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopControllerError {
    AlreadyStarted,
    InvalidTiming,
    Internal,
}

impl DesktopControllerError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::AlreadyStarted => "DESKTOP_CONTROLLER_ALREADY_STARTED",
            Self::InvalidTiming => "DESKTOP_CONTROLLER_TIMING_INVALID",
            Self::Internal => "DESKTOP_CONTROLLER_INTERNAL",
        }
    }
}

impl fmt::Display for DesktopControllerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for DesktopControllerError {}

/// Process status presented by the controller.  It is copied from the safe
/// Prompt 96 response and contains no process ID, command line, or path.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopControllerProcessStatus {
    pub state: DesktopProcessStatus,
    pub control_ready: bool,
}

/// Stable presentation spelling of the Prompt 96 runtime phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerRuntimeState {
    Idle,
    Scheduled,
    Running,
    BackingOff,
    AuthBlocked,
    RootBlocked,
    Faulted,
    Stopped,
}

/// Stable presentation spelling of root availability.  No path is included.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerRootState {
    Available,
    Unavailable,
    Recovering,
}

/// Stable presentation spelling of the safe authentication category.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerAuthState {
    Ready,
    Missing,
    Blocked,
    Revoked,
    Unknown,
}

/// Stable presentation spelling of the safe conflict category.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerConflictState {
    Clear,
    Required,
    Unknown,
}

/// The only conflict decisions admitted by the native controller. The enum is
/// intentionally narrower than an arbitrary command payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerConflictAction {
    AcceptRemote,
    RetryLocalAgainstCurrentBase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerAttentionItemKind {
    File,
    Directory,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopControllerAttentionSummary {
    pub total_count: u64,
    pub conflict_count: u64,
    pub other_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopControllerAttentionLibrarySummary {
    pub library_id: String,
    pub conflict_count: u64,
    pub other_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopControllerAttentionItem {
    pub attention_id: String,
    pub library_id: String,
    pub conflict_id: String,
    pub intent_id: String,
    pub node_id: Option<String>,
    pub category: String,
    pub relative_path: Option<String>,
    pub previous_relative_path: Option<String>,
    pub item_kind: Option<DesktopControllerAttentionItemKind>,
    pub local_length: Option<u64>,
    pub remote_length: Option<u64>,
    pub local_base_revision: Option<u64>,
    pub remote_observed_revision: Option<u64>,
    pub remote_observed_state: Option<String>,
    pub detected_at_ms: u64,
    pub supported_actions: Vec<DesktopControllerConflictAction>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopControllerAttentionSnapshot {
    pub summary: DesktopControllerAttentionSummary,
    pub libraries: Vec<DesktopControllerAttentionLibrarySummary>,
    pub items: Vec<DesktopControllerAttentionItem>,
    pub truncated: bool,
}

impl Default for DesktopControllerAttentionSnapshot {
    fn default() -> Self {
        Self {
            summary: DesktopControllerAttentionSummary {
                total_count: 0,
                conflict_count: 0,
                other_count: 0,
            },
            libraries: Vec::new(),
            items: Vec::new(),
            truncated: false,
        }
    }
}

/// The identity fence copied from one attention snapshot into a resolution
/// command. The store verifies all three identifiers inside its transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesktopControllerConflictResolutionRequest {
    pub library_id: LibraryId,
    pub conflict_id: SyncConflictId,
    pub intent_id: OutboundIntentId,
    pub detected_at_ms: u64,
    pub action: DesktopControllerConflictAction,
}

/// Stable presentation spelling of a safe runtime outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerSyncOutcome {
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

/// Redacted status for one library.  The library ID is the only identifier;
/// root paths, URLs, credential IDs, tokens, cookies, headers, and file data
/// are not representable here.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopControllerLibraryStatus {
    pub library_id: String,
    pub runtime_state: DesktopControllerRuntimeState,
    pub root_state: DesktopControllerRootState,
    pub auth_state: DesktopControllerAuthState,
    pub conflict_state: DesktopControllerConflictState,
    pub next_due_ms: Option<u64>,
    pub last_outcome: Option<DesktopControllerSyncOutcome>,
    pub wake_pending: bool,
    pub transient_failures: u32,
}

/// One coherent, latest-state presentation snapshot.  `revision` advances only
/// when a complete process/library status transaction is published.  Connection
/// transitions retain the last data while changing freshness.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopControllerSnapshot {
    pub connection_state: DesktopControllerConnectionState,
    pub process: Option<DesktopControllerProcessStatus>,
    pub libraries: Vec<DesktopControllerLibraryStatus>,
    pub libraries_truncated: bool,
    pub attention: DesktopControllerAttentionSnapshot,
    pub revision: u64,
    pub freshness: DesktopControllerFreshness,
    pub last_error: Option<DesktopControllerErrorKind>,
    pub connection_generation: u64,
    pub profile_configured: bool,
    pub profile_authenticated: bool,
    pub profile_display_name: Option<String>,
    pub profile_server_url: Option<String>,
    pub sync_control_state: DesktopControllerSyncControlState,
}

impl Default for DesktopControllerSnapshot {
    fn default() -> Self {
        Self {
            connection_state: DesktopControllerConnectionState::Disconnected,
            process: None,
            libraries: Vec::new(),
            libraries_truncated: false,
            attention: DesktopControllerAttentionSnapshot::default(),
            revision: 0,
            freshness: DesktopControllerFreshness::Unavailable,
            last_error: None,
            connection_generation: 0,
            profile_configured: false,
            profile_authenticated: false,
            profile_display_name: None,
            profile_server_url: None,
            sync_control_state: DesktopControllerSyncControlState::Unknown,
        }
    }
}

/// Results exposed for scheduling-only controller commands.  `Accepted` means
/// the server accepted scheduling; it never means synchronization completed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerCommandResult {
    Accepted,
    Coalesced,
    AlreadyRunningFollowupRecorded,
    Paused,
    Resumed,
    AlreadyPaused,
    AlreadyRunning,
    Authenticated,
    SignedOut,
    ProfileConfigured,
    ProfileValidated,
    ProfileAlreadyConfigured,
    LibraryConfigured,
    LibraryAlreadyConfigured,
    InvalidLibraryName,
    InvalidLibraryRoot,
    AuthenticationRequired,
    ServerIdentityConflict,
    InvalidCredentials,
    InvalidConfiguration,
    InvalidServerAddress,
    NetworkUnavailable,
    ServerUnavailable,
    Timeout,
    TlsFailure,
    IncompatibleServer,
    PersistenceFailure,
    RateLimited,
    SecureStoreUnavailable,
    Busy,
    ShutdownAccepted,
    Disconnected,
    UnknownLibrary,
    RuntimeStopped,
    Unavailable,
    AdmissionLimited,
    OutcomeUnknown,
    ProtocolError,
    Stopped,
    AlreadyUnavailable,
    ConflictResolved,
    ConflictAlreadyResolved,
    ConflictStale,
    ConflictNotFound,
    UnsupportedConflictAction,
    ConflictPersistenceFailure,
}

/// The only recovery operations the desktop projection may advertise. These
/// are typed presentation intents; they are translated by the owning bridge
/// into the existing client-launch, profile/authentication, setup, or runtime
/// wake paths. No arbitrary command string crosses the controller boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerRecoveryAction {
    StartClient,
    ConfigureProfile,
    Authenticate,
    CheckAgain,
    ResumeSetup,
}

/// Canonical durable/runtime category projected to recovery UX. Conflict
/// attention deliberately remains owned by the existing attention surface and
/// is not duplicated here.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerRecoveryKind {
    ClientUnavailable,
    ProfileConfigurationRequired,
    AuthenticationRequired,
    RootUnavailable,
    ServerRetryable,
    LocalFailure,
    LibrarySetupIncomplete,
}

/// Process/control availability used by the recovery summary. Waiting is not
/// action-required: it covers reconnect/backoff transitions that already have
/// bounded automatic recovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopControllerClientRecoveryState {
    Ready,
    Waiting,
    Unavailable,
    Unknown,
}

/// One bounded, safe recovery item. It contains only stable IDs and generic
/// labels/codes at the presentation layer; no path, credential, HTTP body, or
/// filesystem diagnostic is representable here.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopControllerRecoveryItem {
    pub recovery_id: String,
    pub library_id: Option<String>,
    pub kind: DesktopControllerRecoveryKind,
    pub action: Option<DesktopControllerRecoveryAction>,
    pub action_required: bool,
    pub waiting: bool,
    pub connection_generation: u64,
}

/// Derived recovery projection. It is intentionally not persisted: the
/// process-owned runtime, profile state, root state, and durable attention
/// records remain authoritative after restart/reconnect.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopControllerRecoverySummary {
    pub total_action_required: u64,
    pub total_waiting: u64,
    pub client_state: DesktopControllerClientRecoveryState,
    pub items: Vec<DesktopControllerRecoveryItem>,
    pub truncated: bool,
}

/// Recovery projection bound independent of the IPC library list bound. A
/// recovery item is a compact status reference, so this remains safe even
/// when a profile contains many libraries.
pub const MAX_DESKTOP_CONTROLLER_RECOVERY_ITEMS: usize = 128;

impl DesktopControllerSnapshot {
    /// Derive a bounded recovery view from the latest coherent snapshot.
    ///
    /// `Fresh + Connected` is required before process/library actions are
    /// advertised. Transient reconnect/backoff states are waiting rather than
    /// action-required. Conflict rows, user pause, and durable attention are
    /// intentionally composed by their existing surfaces.
    #[must_use]
    pub fn recovery_summary(&self) -> DesktopControllerRecoverySummary {
        let (client_state, client_item) = recovery_client_state(self);
        let mut items = Vec::new();
        let mut action_required = 0_u64;
        let mut waiting = 0_u64;

        if let Some(item) = client_item {
            if item.action_required {
                action_required = action_required.saturating_add(1);
            }
            if item.waiting {
                waiting = waiting.saturating_add(1);
            }
            items.push(item);
        }

        if self.freshness == DesktopControllerFreshness::Fresh
            && self.connection_state == DesktopControllerConnectionState::Connected
            && client_state == DesktopControllerClientRecoveryState::Ready
        {
            if !self.profile_configured {
                push_recovery_item(
                    &mut items,
                    &mut action_required,
                    &mut waiting,
                    DesktopControllerRecoveryItem {
                        recovery_id: "profile".to_owned(),
                        library_id: None,
                        kind: DesktopControllerRecoveryKind::ProfileConfigurationRequired,
                        action: Some(DesktopControllerRecoveryAction::ConfigureProfile),
                        action_required: true,
                        waiting: false,
                        connection_generation: self.connection_generation,
                    },
                );
            } else if !self.profile_authenticated {
                push_recovery_item(
                    &mut items,
                    &mut action_required,
                    &mut waiting,
                    DesktopControllerRecoveryItem {
                        recovery_id: "profile-authentication".to_owned(),
                        library_id: None,
                        kind: DesktopControllerRecoveryKind::AuthenticationRequired,
                        action: Some(DesktopControllerRecoveryAction::Authenticate),
                        action_required: true,
                        waiting: false,
                        connection_generation: self.connection_generation,
                    },
                );
            } else if self.libraries.is_empty() {
                // The existing setup form is the safe resume path for both a
                // first library and a manifest-persisted pending setup. The
                // client reuses the pending UUID when the same root is chosen.
                push_recovery_item(
                    &mut items,
                    &mut action_required,
                    &mut waiting,
                    DesktopControllerRecoveryItem {
                        recovery_id: "library-setup".to_owned(),
                        library_id: None,
                        kind: DesktopControllerRecoveryKind::LibrarySetupIncomplete,
                        action: Some(DesktopControllerRecoveryAction::ResumeSetup),
                        action_required: true,
                        waiting: false,
                        connection_generation: self.connection_generation,
                    },
                );
            }

            for library in &self.libraries {
                let library_id = library.library_id.clone();
                let (kind, action, item_action_required, is_waiting) =
                    library_recovery(library, self.sync_control_state);
                let Some(kind) = kind else {
                    continue;
                };
                push_recovery_item(
                    &mut items,
                    &mut action_required,
                    &mut waiting,
                    DesktopControllerRecoveryItem {
                        recovery_id: format!("library:{library_id}"),
                        library_id: Some(library_id),
                        kind,
                        action,
                        action_required: item_action_required,
                        waiting: is_waiting,
                        connection_generation: self.connection_generation,
                    },
                );
            }
        }

        let item_limit_exceeded = items.len() > MAX_DESKTOP_CONTROLLER_RECOVERY_ITEMS;
        if item_limit_exceeded {
            items.truncate(MAX_DESKTOP_CONTROLLER_RECOVERY_ITEMS);
        }
        DesktopControllerRecoverySummary {
            total_action_required: action_required,
            total_waiting: waiting,
            client_state,
            items,
            truncated: self.libraries_truncated || item_limit_exceeded,
        }
    }
}

fn recovery_client_state(
    snapshot: &DesktopControllerSnapshot,
) -> (
    DesktopControllerClientRecoveryState,
    Option<DesktopControllerRecoveryItem>,
) {
    let generation = snapshot.connection_generation;
    let unavailable = || {
        Some(DesktopControllerRecoveryItem {
            recovery_id: "client".to_owned(),
            library_id: None,
            kind: DesktopControllerRecoveryKind::ClientUnavailable,
            action: Some(DesktopControllerRecoveryAction::StartClient),
            action_required: true,
            waiting: false,
            connection_generation: generation,
        })
    };
    let waiting = || {
        Some(DesktopControllerRecoveryItem {
            recovery_id: "client".to_owned(),
            library_id: None,
            kind: DesktopControllerRecoveryKind::ClientUnavailable,
            action: None,
            action_required: false,
            waiting: true,
            connection_generation: generation,
        })
    };

    if snapshot.freshness != DesktopControllerFreshness::Fresh
        || snapshot.connection_state != DesktopControllerConnectionState::Connected
    {
        return match snapshot.connection_state {
            DesktopControllerConnectionState::Connecting
            | DesktopControllerConnectionState::Reconnecting
            | DesktopControllerConnectionState::Stopping
            | DesktopControllerConnectionState::Connected
                if snapshot.freshness != DesktopControllerFreshness::Fresh =>
            {
                (DesktopControllerClientRecoveryState::Waiting, waiting())
            }
            DesktopControllerConnectionState::Disconnected
            | DesktopControllerConnectionState::Faulted
            | DesktopControllerConnectionState::Stopped
            | DesktopControllerConnectionState::ProtocolIncompatible => (
                DesktopControllerClientRecoveryState::Unavailable,
                unavailable(),
            ),
            _ => (DesktopControllerClientRecoveryState::Unknown, None),
        };
    }

    let Some(process) = snapshot.process else {
        return (DesktopControllerClientRecoveryState::Unknown, waiting());
    };
    match process.state {
        DesktopProcessStatus::Running if process.control_ready => {
            (DesktopControllerClientRecoveryState::Ready, None)
        }
        DesktopProcessStatus::Starting | DesktopProcessStatus::Stopping => {
            (DesktopControllerClientRecoveryState::Waiting, waiting())
        }
        DesktopProcessStatus::Stopped | DesktopProcessStatus::Faulted => (
            DesktopControllerClientRecoveryState::Unavailable,
            unavailable(),
        ),
        DesktopProcessStatus::Running => (DesktopControllerClientRecoveryState::Waiting, waiting()),
    }
}

fn library_recovery(
    library: &DesktopControllerLibraryStatus,
    _sync_control_state: DesktopControllerSyncControlState,
) -> (
    Option<DesktopControllerRecoveryKind>,
    Option<DesktopControllerRecoveryAction>,
    bool,
    bool,
) {
    if library.root_state == DesktopControllerRootState::Unavailable
        || library.runtime_state == DesktopControllerRuntimeState::RootBlocked
    {
        return (
            Some(DesktopControllerRecoveryKind::RootUnavailable),
            Some(DesktopControllerRecoveryAction::CheckAgain),
            true,
            false,
        );
    }
    if library.root_state == DesktopControllerRootState::Recovering {
        return (
            Some(DesktopControllerRecoveryKind::RootUnavailable),
            None,
            false,
            true,
        );
    }
    if matches!(
        library.auth_state,
        DesktopControllerAuthState::Missing
            | DesktopControllerAuthState::Blocked
            | DesktopControllerAuthState::Revoked
    ) || library.runtime_state == DesktopControllerRuntimeState::AuthBlocked
    {
        return (
            Some(DesktopControllerRecoveryKind::AuthenticationRequired),
            Some(DesktopControllerRecoveryAction::Authenticate),
            true,
            false,
        );
    }
    // A user pause is not itself a recovery condition, but it must not hide a
    // separate canonical blocker. For example, "Paused" and "Local folder
    // unavailable" are both meaningful and must remain independently visible.
    // The recovery action below never clears the pause; only the explicit
    // resume control owns that durable transition.
    if matches!(
        library.last_outcome,
        Some(
            DesktopControllerSyncOutcome::Offline
                | DesktopControllerSyncOutcome::ServerTransient
                | DesktopControllerSyncOutcome::RateLimited
        )
    ) || library.runtime_state == DesktopControllerRuntimeState::BackingOff
    {
        return (
            Some(DesktopControllerRecoveryKind::ServerRetryable),
            Some(DesktopControllerRecoveryAction::CheckAgain),
            false,
            true,
        );
    }
    if matches!(
        library.last_outcome,
        Some(
            DesktopControllerSyncOutcome::RecoveryBlocked
                | DesktopControllerSyncOutcome::FatalLocal
                | DesktopControllerSyncOutcome::Panicked
        )
    ) || library.runtime_state == DesktopControllerRuntimeState::Faulted
    {
        return (
            Some(DesktopControllerRecoveryKind::LocalFailure),
            Some(DesktopControllerRecoveryAction::CheckAgain),
            true,
            false,
        );
    }
    (None, None, false, false)
}

fn push_recovery_item(
    items: &mut Vec<DesktopControllerRecoveryItem>,
    action_required_total: &mut u64,
    waiting_total: &mut u64,
    item: DesktopControllerRecoveryItem,
) {
    if item.action_required {
        *action_required_total = (*action_required_total).saturating_add(1);
    }
    if item.waiting {
        *waiting_total = (*waiting_total).saturating_add(1);
    }
    items.push(item);
}

/// Endpoint selection is resolved on each connection attempt.  In the normal
/// profile form this reuses Prompt 96's canonical platform endpoint resolver;
/// the explicit endpoint form is useful for embedding/tests that already own a
/// validated endpoint value.
#[derive(Clone)]
enum EndpointSource {
    Profile {
        platform: Arc<dyn PlatformRuntime>,
        profile_id: synveil_client_sync::ServerProfileId,
    },
    Explicit(DesktopControlEndpoint),
}

impl fmt::Debug for EndpointSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Profile { profile_id, .. } => formatter
                .debug_struct("Profile")
                .field("profile_id", profile_id)
                .finish(),
            Self::Explicit(endpoint) => formatter
                .debug_struct("Explicit")
                .field("kind", &endpoint.kind())
                .finish(),
        }
    }
}

impl EndpointSource {
    fn profile_id(&self) -> Option<ServerProfileId> {
        match self {
            Self::Profile { profile_id, .. } => Some(*profile_id),
            Self::Explicit(_) => None,
        }
    }

    async fn resolve(&self) -> Result<DesktopControlEndpoint, DesktopControlClientError> {
        match self {
            Self::Profile {
                platform,
                profile_id,
            } => DesktopControlEndpoint::for_profile(platform.as_ref(), *profile_id)
                .map_err(DesktopControlClientError::Endpoint),
            Self::Explicit(endpoint) => Ok(endpoint.clone()),
        }
    }
}

/// Configuration for one controller-to-one-process relationship.
pub struct DesktopControllerConfig {
    endpoint: EndpointSource,
    timing: DesktopControllerTiming,
}

impl fmt::Debug for DesktopControllerConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopControllerConfig")
            .field("endpoint", &self.endpoint)
            .field("timing", &self.timing)
            .finish()
    }
}

impl DesktopControllerConfig {
    /// Construct a profile-scoped controller.  This only stores the platform
    /// resolver/profile identity; it performs no I/O and starts no task.
    #[must_use]
    pub fn new(
        platform: Arc<dyn PlatformRuntime>,
        profile_id: synveil_client_sync::ServerProfileId,
    ) -> Self {
        Self {
            endpoint: EndpointSource::Profile {
                platform,
                profile_id,
            },
            timing: DesktopControllerTiming::default(),
        }
    }

    /// Construct a profile-scoped controller using the current platform
    /// resolver. The resolver remains below the controller API so UI crates do
    /// not need a direct platform dependency.
    #[must_use]
    pub fn for_profile(profile_id: synveil_client_sync::ServerProfileId) -> Self {
        let platform: Arc<dyn PlatformRuntime> = Arc::from(synveil_platform::current());
        Self::new(platform, profile_id)
    }

    /// Construct an embedding/test configuration from an already-resolved
    /// Prompt 96 endpoint.  Transport derivation remains outside the
    /// controller.
    #[must_use]
    pub fn for_endpoint(endpoint: DesktopControlEndpoint) -> Self {
        Self {
            endpoint: EndpointSource::Explicit(endpoint),
            timing: DesktopControllerTiming::default(),
        }
    }

    #[must_use]
    pub fn with_timing(mut self, timing: DesktopControllerTiming) -> Self {
        self.timing = timing;
        self
    }

    #[must_use]
    pub const fn timing(&self) -> DesktopControllerTiming {
        self.timing
    }

    #[must_use]
    pub fn profile_id(&self) -> Option<ServerProfileId> {
        self.endpoint.profile_id()
    }
}

/// Reusable native controller handle.  Cloning it shares the same manager,
/// latest-state watch, command admission limit, and connection relationship.
#[derive(Clone)]
pub struct DesktopController {
    inner: Arc<DesktopControllerInner>,
}

impl fmt::Debug for DesktopController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopController")
            .field("snapshot", &self.snapshot())
            .finish_non_exhaustive()
    }
}

struct DesktopControllerInner {
    config: DesktopControllerConfig,
    snapshot: watch::Sender<DesktopControllerSnapshot>,
    lifecycle: Mutex<ControllerLifecycle>,
    current_generation: AtomicU64,
    auth_in_flight: Arc<AtomicBool>,
    configuration_in_flight: Arc<AtomicBool>,
    sync_control_in_flight: Arc<AtomicBool>,
    attention_in_flight: Arc<AtomicBool>,
    refresh_requested: AtomicBool,
    refresh_notify: Notify,
}

struct ControllerLifecycle {
    task: Option<JoinHandle<()>>,
    stop: Option<oneshot::Sender<()>>,
    command: Option<mpsc::Sender<ControllerCommand>>,
    stopping: bool,
}

impl DesktopController {
    /// Create a side-effect-free controller.  No endpoint is resolved and no
    /// background task is created until [`Self::start`] is called.
    #[must_use]
    pub fn new(config: DesktopControllerConfig) -> Self {
        let (snapshot, _) = watch::channel(DesktopControllerSnapshot::default());
        Self {
            inner: Arc::new(DesktopControllerInner {
                config,
                snapshot,
                lifecycle: Mutex::new(ControllerLifecycle {
                    task: None,
                    stop: None,
                    command: None,
                    stopping: false,
                }),
                current_generation: AtomicU64::new(0),
                auth_in_flight: Arc::new(AtomicBool::new(false)),
                configuration_in_flight: Arc::new(AtomicBool::new(false)),
                sync_control_in_flight: Arc::new(AtomicBool::new(false)),
                attention_in_flight: Arc::new(AtomicBool::new(false)),
                refresh_requested: AtomicBool::new(false),
                refresh_notify: Notify::new(),
            }),
        }
    }

    /// Start the one controller manager.  Startup is asynchronous in the
    /// manager task; this method only creates bounded task/channel ownership.
    pub async fn start(&self) -> Result<(), DesktopControllerError> {
        let mut lifecycle = self.inner.lifecycle.lock().await;
        if lifecycle.task.is_some() || lifecycle.stopping {
            return Err(DesktopControllerError::AlreadyStarted);
        }

        let (stop, stop_rx) = oneshot::channel();
        let (command, command_rx) = mpsc::channel(DESKTOP_CONTROLLER_COMMAND_CAPACITY);
        self.inner.mark_connecting();

        let task = tokio::spawn(run_controller(Arc::clone(&self.inner), stop_rx, command_rx));
        lifecycle.task = Some(task);
        lifecycle.stop = Some(stop);
        lifecycle.command = Some(command);
        Ok(())
    }

    /// Stop and join controller-owned work.  This closes only controller IPC
    /// connections and never sends Prompt 96 `Shutdown` implicitly.
    pub async fn stop(&self) -> Result<(), DesktopControllerError> {
        let (stop, task) = {
            let mut lifecycle = self.inner.lifecycle.lock().await;
            let Some(task) = lifecycle.task.take() else {
                if !lifecycle.stopping {
                    self.inner.mark_stopped();
                }
                return Ok(());
            };
            lifecycle.stopping = true;
            lifecycle.command = None;
            (lifecycle.stop.take(), task)
        };

        if let Some(stop) = stop {
            let _ = stop.send(());
        }
        let join_result = task.await;

        let mut lifecycle = self.inner.lifecycle.lock().await;
        lifecycle.stopping = false;
        self.inner.mark_stopped();
        join_result.map_err(|_| DesktopControllerError::Internal)
    }

    /// Join a started controller without requesting a stop.  This is intended
    /// for an embedding that wants to await a stable protocol/process-stop
    /// termination.  Normal UI teardown should call [`Self::stop`] instead.
    pub async fn join(&self) -> Result<(), DesktopControllerError> {
        let (task, stop) = {
            let mut lifecycle = self.inner.lifecycle.lock().await;
            let Some(task) = lifecycle.task.take() else {
                if !lifecycle.stopping {
                    self.inner.mark_stopped();
                }
                return Ok(());
            };
            lifecycle.stopping = true;
            (task, lifecycle.stop.take())
        };
        // Keep the sender alive while joining.  Dropping it here would turn a
        // join into an implicit stop because the manager observes a closed
        // oneshot receiver as cancellation.
        let _keep_stop = stop;
        let join_result = task.await;
        let mut lifecycle = self.inner.lifecycle.lock().await;
        lifecycle.stopping = false;
        lifecycle.command = None;
        join_result.map_err(|_| DesktopControllerError::Internal)
    }

    /// Clone the current latest-state snapshot without holding any controller
    /// lock after this call returns.
    #[must_use]
    pub fn snapshot(&self) -> DesktopControllerSnapshot {
        self.inner.snapshot.borrow().clone()
    }

    /// Subscribe to latest-state updates.  `watch` retains only the newest
    /// snapshot, so a slow UI does not create an unbounded callback/task list.
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<DesktopControllerSnapshot> {
        self.inner.snapshot.subscribe()
    }

    /// Alias emphasizing that this is a latest-state view rather than an event
    /// history.
    #[must_use]
    pub fn subscribe_state(&self) -> watch::Receiver<DesktopControllerSnapshot> {
        self.subscribe()
    }

    /// Ask the running process to schedule a library sync through Prompt 96.
    /// No request is admitted while disconnected, and an admitted request is
    /// never replayed after a transport loss.
    pub async fn sync_now(
        &self,
        library_id: LibraryId,
    ) -> Result<DesktopControllerCommandResult, DesktopControllerError> {
        self.dispatch_command(CommandAction::SyncNow(library_id))
            .await
    }

    /// Request the canonical bounded runtime wake for a recovery item. Root,
    /// transient-server, and persistent-local recovery all converge on the
    /// existing `SyncNow` admission path; this method does not reset state,
    /// bypass backoff policy, or replay an uncertain mutation.
    pub async fn retry_recovery(
        &self,
        library_id: LibraryId,
    ) -> Result<DesktopControllerCommandResult, DesktopControllerError> {
        self.sync_now(library_id).await
    }

    /// Resolve exactly the conflict generation selected from the latest
    /// attention snapshot. A timeout is reported as unknown and is never
    /// replayed; the subsequent attention refresh is authoritative.
    pub async fn resolve_conflict(
        &self,
        request: DesktopControllerConflictResolutionRequest,
    ) -> Result<DesktopControllerCommandResult, DesktopControllerError> {
        self.dispatch_command(CommandAction::ResolveConflict(request))
            .await
    }

    /// Persist and apply the process-wide user pause through the background
    /// client. The controller never changes runtime state locally.
    pub async fn pause_sync(
        &self,
    ) -> Result<DesktopControllerCommandResult, DesktopControllerError> {
        self.dispatch_command(CommandAction::PauseSync).await
    }

    /// Persist and apply the process-wide user resume through the background
    /// client. A response timeout remains an unknown outcome and triggers a
    /// later canonical refresh rather than an implicit replay.
    pub async fn resume_sync(
        &self,
    ) -> Result<DesktopControllerCommandResult, DesktopControllerError> {
        self.dispatch_command(CommandAction::ResumeSync).await
    }

    /// Set up one authenticated library through the process-owned control
    /// plane. Admission shares the bounded configuration gate so profile
    /// mutations and library setup cannot race durable identity changes.
    pub async fn setup_library(
        &self,
        name: String,
        root_path: String,
    ) -> Result<DesktopControllerCommandResult, DesktopControllerError> {
        self.dispatch_command(CommandAction::SetupLibrary(name, root_path))
            .await
    }

    /// Parse and dispatch one transient enrollment secret. Invalid input is
    /// rejected before IPC, so it cannot create a task, contact the host, or
    /// write the SecretStore. Valid input is retained only by the bounded
    /// in-flight command until the process responds or the command is dropped.
    pub async fn authenticate(
        &self,
        input: &str,
    ) -> Result<DesktopControllerCommandResult, DesktopControllerError> {
        let Ok(secret) = EnrollmentSecret::parse(input) else {
            return Ok(DesktopControllerCommandResult::InvalidCredentials);
        };
        self.dispatch_command(CommandAction::Authenticate(secret))
            .await
    }

    /// Request one coalesced authoritative process/library status refresh.
    /// This is a local controller signal, not a new IPC command; it is used
    /// after an authentication delivery outcome is unknown and never replays
    /// the credential-bearing operation.
    pub fn refresh_auth_state(&self) {
        self.inner.request_refresh();
    }

    /// Request one coalesced status refresh after a non-replayed local
    /// operation has an unknown delivery result.
    pub fn refresh_state(&self) {
        self.refresh_auth_state();
    }

    /// Explicitly forget the profile-bound credential through the background
    /// process. This never requests process shutdown or replays on reconnect.
    pub async fn sign_out(&self) -> Result<DesktopControllerCommandResult, DesktopControllerError> {
        self.dispatch_command(CommandAction::SignOut).await
    }

    /// Ask the running process for graceful shutdown through Prompt 96.  A
    /// lost response is [`DesktopControllerCommandResult::OutcomeUnknown`];
    /// the controller never resends `Shutdown` after reconnecting.
    pub async fn request_shutdown(
        &self,
    ) -> Result<DesktopControllerCommandResult, DesktopControllerError> {
        self.dispatch_command(CommandAction::Shutdown).await
    }

    /// Descriptive alias for callers that use a verb-noun command style.
    pub async fn shutdown(&self) -> Result<DesktopControllerCommandResult, DesktopControllerError> {
        self.request_shutdown().await
    }

    /// Configure the server profile with base URL and display label.
    pub async fn configure_profile(
        &self,
        base_url: String,
        display_label: String,
    ) -> Result<DesktopControllerCommandResult, DesktopControllerError> {
        let Some(profile_id) = self.inner.config.profile_id() else {
            return Ok(DesktopControllerCommandResult::Unavailable);
        };
        self.dispatch_command(CommandAction::ConfigureProfile(
            profile_id,
            base_url,
            display_label,
        ))
        .await
    }

    /// Validate and probe a candidate profile without changing durable state.
    /// The operation is admitted through the same bounded configuration gate
    /// as apply, so a late probe cannot race a newer configuration mutation.
    pub async fn validate_profile_configuration(
        &self,
        base_url: String,
        display_label: String,
    ) -> Result<DesktopControllerCommandResult, DesktopControllerError> {
        self.dispatch_command(CommandAction::ValidateProfile(base_url, display_label))
            .await
    }

    async fn dispatch_command(
        &self,
        action: CommandAction,
    ) -> Result<DesktopControllerCommandResult, DesktopControllerError> {
        let snapshot = self.snapshot();
        match snapshot.connection_state {
            DesktopControllerConnectionState::Connected => {}
            DesktopControllerConnectionState::Stopping => {
                return Ok(DesktopControllerCommandResult::AlreadyUnavailable);
            }
            DesktopControllerConnectionState::Stopped => {
                return Ok(DesktopControllerCommandResult::Stopped);
            }
            DesktopControllerConnectionState::ProtocolIncompatible
            | DesktopControllerConnectionState::Faulted => {
                return Ok(DesktopControllerCommandResult::Unavailable);
            }
            DesktopControllerConnectionState::Disconnected
            | DesktopControllerConnectionState::Connecting
            | DesktopControllerConnectionState::Reconnecting => {
                return Ok(DesktopControllerCommandResult::Disconnected);
            }
        }

        let auth_admission = if action.is_auth() {
            let Some(admission) =
                AuthAdmission::try_acquire(Arc::clone(&self.inner.auth_in_flight))
            else {
                return Ok(DesktopControllerCommandResult::Busy);
            };
            Some(admission)
        } else {
            None
        };
        let configuration_admission = if action.is_configure() {
            let Some(admission) = ConfigurationAdmission::try_acquire(Arc::clone(
                &self.inner.configuration_in_flight,
            )) else {
                return Ok(DesktopControllerCommandResult::Busy);
            };
            Some(admission)
        } else {
            None
        };
        let sync_control_admission = if action.is_sync_control() {
            let Some(admission) =
                SyncControlAdmission::try_acquire(Arc::clone(&self.inner.sync_control_in_flight))
            else {
                return Ok(DesktopControllerCommandResult::Busy);
            };
            Some(admission)
        } else {
            None
        };
        let attention_admission = if action.is_attention() {
            let Some(admission) =
                AttentionAdmission::try_acquire(Arc::clone(&self.inner.attention_in_flight))
            else {
                return Ok(DesktopControllerCommandResult::Busy);
            };
            Some(admission)
        } else {
            None
        };
        let (reply, result_rx) = oneshot::channel();
        let (command, joining) = {
            let lifecycle = self.inner.lifecycle.lock().await;
            (lifecycle.command.clone(), lifecycle.stopping)
        };
        if joining {
            return Ok(DesktopControllerCommandResult::AlreadyUnavailable);
        }
        let Some(command) = command else {
            return Ok(DesktopControllerCommandResult::Disconnected);
        };
        match command.try_send(ControllerCommand {
            action,
            reply,
            _auth_admission: auth_admission,
            _configuration_admission: configuration_admission,
            _sync_control_admission: sync_control_admission,
            _attention_admission: attention_admission,
        }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Ok(DesktopControllerCommandResult::AdmissionLimited);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Ok(DesktopControllerCommandResult::Stopped);
            }
        }

        match time::timeout(self.inner.config.timing.command_timeout(), result_rx).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(_)) => Ok(DesktopControllerCommandResult::Stopped),
            Err(_) => Ok(DesktopControllerCommandResult::OutcomeUnknown),
        }
    }
}

impl Drop for DesktopController {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) != 1 {
            return;
        }
        if let Ok(mut lifecycle) = self.inner.lifecycle.try_lock()
            && let Some(stop) = lifecycle.stop.take()
        {
            let _ = stop.send(());
        }
    }
}

impl DesktopControllerInner {
    fn snapshot(&self) -> DesktopControllerSnapshot {
        self.snapshot.borrow().clone()
    }

    fn request_refresh(&self) {
        if !self.refresh_requested.swap(true, Ordering::AcqRel) {
            self.refresh_notify.notify_one();
        }
    }

    fn update_snapshot(&self, update: impl FnOnce(&mut DesktopControllerSnapshot)) {
        let mut snapshot = self.snapshot();
        update(&mut snapshot);
        self.snapshot.send_replace(snapshot);
    }

    fn mark_connecting(&self) {
        self.update_snapshot(|snapshot| {
            snapshot.connection_state = DesktopControllerConnectionState::Connecting;
            snapshot.freshness = if snapshot.revision == 0 {
                DesktopControllerFreshness::Unavailable
            } else {
                DesktopControllerFreshness::Stale
            };
            snapshot.last_error = None;
        });
    }

    fn mark_reconnecting(&self, error: DesktopControllerErrorKind) {
        self.update_snapshot(|snapshot| {
            snapshot.connection_state = DesktopControllerConnectionState::Reconnecting;
            snapshot.freshness = if snapshot.revision == 0 {
                DesktopControllerFreshness::Unavailable
            } else {
                DesktopControllerFreshness::Stale
            };
            snapshot.last_error = Some(error);
        });
    }

    fn mark_terminal(
        &self,
        state: DesktopControllerConnectionState,
        error: DesktopControllerErrorKind,
    ) {
        self.update_snapshot(|snapshot| {
            snapshot.connection_state = state;
            snapshot.freshness = if snapshot.revision == 0 {
                DesktopControllerFreshness::Unavailable
            } else {
                DesktopControllerFreshness::Stale
            };
            snapshot.last_error = Some(error);
        });
    }

    fn mark_stopping(&self) {
        self.update_snapshot(|snapshot| {
            snapshot.connection_state = DesktopControllerConnectionState::Stopping;
            snapshot.freshness = DesktopControllerFreshness::Stale;
        });
    }

    fn mark_stopped(&self) {
        self.update_snapshot(|snapshot| {
            snapshot.connection_state = DesktopControllerConnectionState::Stopped;
            snapshot.freshness = if snapshot.revision == 0 {
                DesktopControllerFreshness::Unavailable
            } else {
                DesktopControllerFreshness::Stale
            };
        });
    }

    fn activate_generation(&self, generation: u64) {
        self.current_generation.store(generation, Ordering::Release);
    }

    fn is_current_generation(&self, generation: u64) -> bool {
        self.current_generation.load(Ordering::Acquire) == generation
    }

    fn publish_fresh(&self, generation: u64, data: ControllerSnapshotData) -> bool {
        if !self.is_current_generation(generation) {
            return false;
        }
        self.update_snapshot(|snapshot| {
            snapshot.connection_state = DesktopControllerConnectionState::Connected;
            snapshot.process = Some(data.process);
            snapshot.libraries = data.libraries;
            snapshot.libraries_truncated = data.libraries_truncated;
            snapshot.attention = data.attention;
            snapshot.profile_configured = data.profile_configured;
            snapshot.profile_authenticated = data.profile_authenticated;
            snapshot.profile_display_name = data.profile_display_name;
            snapshot.profile_server_url = data.profile_server_url;
            snapshot.sync_control_state = data.sync_control_state;
            snapshot.revision = snapshot.revision.saturating_add(1);
            snapshot.freshness = DesktopControllerFreshness::Fresh;
            snapshot.last_error = None;
            snapshot.connection_generation = generation;
        });
        true
    }
}

#[derive(Debug)]
enum CommandAction {
    SyncNow(LibraryId),
    ResolveConflict(DesktopControllerConflictResolutionRequest),
    PauseSync,
    ResumeSync,
    SetupLibrary(String, String),
    Authenticate(EnrollmentSecret),
    SignOut,
    Shutdown,
    ValidateProfile(String, String),
    ConfigureProfile(ServerProfileId, String, String),
}

impl CommandAction {
    fn is_auth(&self) -> bool {
        matches!(self, Self::Authenticate(_) | Self::SignOut)
    }

    fn is_configure(&self) -> bool {
        matches!(
            self,
            Self::ValidateProfile(..) | Self::ConfigureProfile(..) | Self::SetupLibrary(..)
        )
    }

    fn is_sync_control(&self) -> bool {
        matches!(self, Self::PauseSync | Self::ResumeSync)
    }

    fn is_attention(&self) -> bool {
        matches!(self, Self::ResolveConflict(_))
    }
}

struct ControllerCommand {
    action: CommandAction,
    reply: oneshot::Sender<DesktopControllerCommandResult>,
    _auth_admission: Option<AuthAdmission>,
    _configuration_admission: Option<ConfigurationAdmission>,
    _sync_control_admission: Option<SyncControlAdmission>,
    _attention_admission: Option<AttentionAdmission>,
}

struct AuthAdmission {
    in_flight: Arc<AtomicBool>,
}

impl AuthAdmission {
    fn try_acquire(in_flight: Arc<AtomicBool>) -> Option<Self> {
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

impl Drop for AuthAdmission {
    fn drop(&mut self) {
        self.in_flight.store(false, Ordering::Release);
    }
}

struct ConfigurationAdmission {
    in_flight: Arc<AtomicBool>,
}

impl ConfigurationAdmission {
    fn try_acquire(in_flight: Arc<AtomicBool>) -> Option<Self> {
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

impl Drop for ConfigurationAdmission {
    fn drop(&mut self) {
        self.in_flight.store(false, Ordering::Release);
    }
}

struct SyncControlAdmission {
    in_flight: Arc<AtomicBool>,
}

impl SyncControlAdmission {
    fn try_acquire(in_flight: Arc<AtomicBool>) -> Option<Self> {
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

impl Drop for SyncControlAdmission {
    fn drop(&mut self) {
        self.in_flight.store(false, Ordering::Release);
    }
}

struct AttentionAdmission {
    in_flight: Arc<AtomicBool>,
}

impl AttentionAdmission {
    fn try_acquire(in_flight: Arc<AtomicBool>) -> Option<Self> {
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

impl Drop for AttentionAdmission {
    fn drop(&mut self) {
        self.in_flight.store(false, Ordering::Release);
    }
}

#[derive(Clone, Debug)]
struct ControllerSnapshotData {
    process: DesktopControllerProcessStatus,
    libraries: Vec<DesktopControllerLibraryStatus>,
    libraries_truncated: bool,
    attention: DesktopControllerAttentionSnapshot,
    profile_configured: bool,
    profile_authenticated: bool,
    profile_display_name: Option<String>,
    profile_server_url: Option<String>,
    sync_control_state: DesktopControllerSyncControlState,
}

struct ActiveConnection {
    generation: u64,
    command: DesktopControlClient,
    status: DesktopControlClient,
    event_signal: Arc<EventSignal>,
    event_stop: Option<oneshot::Sender<()>>,
    event_task: Option<JoinHandle<()>>,
}

impl ActiveConnection {
    fn new(
        generation: u64,
        command: DesktopControlClient,
        status: DesktopControlClient,
        events: DesktopControlEventStream,
        inner: Arc<DesktopControllerInner>,
    ) -> Self {
        let event_signal = Arc::new(EventSignal::new());
        let (event_stop, event_stop_rx) = oneshot::channel();
        let signal = Arc::clone(&event_signal);
        let event_task = tokio::spawn(run_event_reader(
            events,
            event_stop_rx,
            signal,
            inner,
            generation,
        ));
        Self {
            generation,
            command,
            status,
            event_signal,
            event_stop: Some(event_stop),
            event_task: Some(event_task),
        }
    }

    async fn close_event_task(&mut self) {
        if let Some(stop) = self.event_stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.event_task.take() {
            let _ = task.await;
        }
    }
}

struct EventSignal {
    refresh_pending: AtomicBool,
    server_stopping: AtomicBool,
    failure: Mutex<Option<DesktopControllerErrorKind>>,
    notify: Notify,
}

impl EventSignal {
    fn new() -> Self {
        Self {
            refresh_pending: AtomicBool::new(false),
            server_stopping: AtomicBool::new(false),
            failure: Mutex::new(None),
            notify: Notify::new(),
        }
    }

    fn mark_refresh(&self) {
        self.refresh_pending.store(true, Ordering::Release);
        self.notify.notify_one();
    }

    fn take_refresh(&self) -> bool {
        self.refresh_pending.swap(false, Ordering::AcqRel)
    }

    fn mark_server_stopping(&self) {
        self.server_stopping.store(true, Ordering::Release);
        self.mark_refresh();
    }

    fn server_is_stopping(&self) -> bool {
        self.server_stopping.load(Ordering::Acquire)
    }

    async fn mark_failure(&self, failure: DesktopControllerErrorKind) {
        let mut stored = self.failure.lock().await;
        if stored.is_none() {
            *stored = Some(failure);
        }
        self.notify.notify_one();
    }

    async fn take_failure(&self) -> Option<DesktopControllerErrorKind> {
        self.failure.lock().await.take()
    }
}

#[derive(Default)]
struct RefreshCoordinator {
    pending: bool,
    in_flight: bool,
}

impl RefreshCoordinator {
    fn request(&mut self) {
        self.pending = true;
    }

    fn observe_signal(&mut self, signal: &EventSignal) {
        if signal.take_refresh() {
            self.request();
        }
    }

    fn begin(&mut self) -> bool {
        if self.in_flight || !self.pending {
            return false;
        }
        self.pending = false;
        self.in_flight = true;
        true
    }

    fn finish(&mut self, signal: &EventSignal) {
        self.in_flight = false;
        self.observe_signal(signal);
    }
}

async fn run_controller(
    inner: Arc<DesktopControllerInner>,
    mut stop_rx: oneshot::Receiver<()>,
    mut command_rx: mpsc::Receiver<ControllerCommand>,
) {
    let timing = inner.config.timing;
    let mut backoff = ReconnectBackoff::new(timing);
    let mut connection_generation = 0_u64;
    let mut first_attempt = true;

    loop {
        if first_attempt {
            inner.mark_connecting();
        } else {
            inner.update_snapshot(|snapshot| {
                snapshot.connection_state = DesktopControllerConnectionState::Reconnecting;
                snapshot.freshness = if snapshot.revision == 0 {
                    DesktopControllerFreshness::Unavailable
                } else {
                    DesktopControllerFreshness::Stale
                };
            });
        }

        connection_generation = connection_generation.saturating_add(1);
        let generation = connection_generation;
        inner.activate_generation(generation);
        let connection = tokio::select! {
            result = connect_and_initialize(
                &inner,
                &inner.config.endpoint,
                timing,
                generation,
            ) => result,
            _ = &mut stop_rx => {
                drain_commands(&mut command_rx, DesktopControllerCommandResult::Stopped);
                inner.mark_stopped();
                return;
            }
        };

        match connection {
            Ok((mut active, data)) => {
                backoff.reset();
                if !inner.publish_fresh(generation, data) {
                    active.close_event_task().await;
                    drain_commands(
                        &mut command_rx,
                        DesktopControllerCommandResult::Disconnected,
                    );
                    first_attempt = false;
                    continue;
                }

                match run_active(&inner, &mut active, timing, &mut stop_rx, &mut command_rx).await {
                    ActiveExit::StopRequested => {
                        drain_commands(&mut command_rx, DesktopControllerCommandResult::Stopped);
                        inner.mark_stopped();
                        return;
                    }
                    ActiveExit::ProcessStopped => {
                        drain_commands(
                            &mut command_rx,
                            DesktopControllerCommandResult::AlreadyUnavailable,
                        );
                        inner.mark_stopped();
                        return;
                    }
                    ActiveExit::GenerationSuperseded => {
                        drain_commands(
                            &mut command_rx,
                            DesktopControllerCommandResult::Disconnected,
                        );
                        first_attempt = false;
                    }
                    ActiveExit::Reconnect(error) => {
                        drain_commands(
                            &mut command_rx,
                            DesktopControllerCommandResult::Disconnected,
                        );
                        inner.mark_reconnecting(error);
                        first_attempt = false;
                        let delay = backoff.next_delay();
                        if wait_for_stop(&mut stop_rx, delay).await {
                            inner.mark_stopped();
                            return;
                        }
                    }
                    ActiveExit::Terminal(state, error) => {
                        drain_commands(
                            &mut command_rx,
                            DesktopControllerCommandResult::Unavailable,
                        );
                        inner.mark_terminal(state, error);
                        return;
                    }
                }
            }
            Err(error) => {
                let failure = classify_connection_failure(error);
                match failure {
                    ConnectionFailure::Retry(error) => {
                        drain_commands(
                            &mut command_rx,
                            DesktopControllerCommandResult::Disconnected,
                        );
                        inner.mark_reconnecting(error);
                        first_attempt = false;
                        let delay = backoff.next_delay();
                        if wait_for_stop(&mut stop_rx, delay).await {
                            inner.mark_stopped();
                            return;
                        }
                    }
                    ConnectionFailure::Terminal(state, error) => {
                        drain_commands(
                            &mut command_rx,
                            DesktopControllerCommandResult::Unavailable,
                        );
                        inner.mark_terminal(state, error);
                        return;
                    }
                }
            }
        }
    }
}

async fn connect_and_initialize(
    inner: &Arc<DesktopControllerInner>,
    endpoint_source: &EndpointSource,
    timing: DesktopControllerTiming,
    generation: u64,
) -> Result<(ActiveConnection, ControllerSnapshotData), ControllerIoError> {
    let endpoint = endpoint_source
        .resolve()
        .await
        .map_err(ControllerIoError::Client)?;

    // Keep request/status and event traffic independent while sharing one
    // logical controller relationship.  Each Prompt 96 connection performs
    // its own v1 handshake; no wire session is persisted or reused across
    // process restarts.
    let mut status = connect_client(endpoint.clone(), timing.operation_timeout()).await?;
    let command = connect_client(endpoint.clone(), timing.operation_timeout()).await?;
    let event_client = connect_client(endpoint, timing.operation_timeout()).await?;
    let events = subscribe_events(event_client, timing.operation_timeout()).await?;

    let data = fetch_snapshot(&mut status, timing.operation_timeout()).await?;
    // The event reader is started only after the complete initial transaction
    // is ready to publish.  The server's event channel is bounded and the
    // reader starts immediately after this return, so no durable event history
    // is implied by this presentation boundary.
    let active = ActiveConnection::new(generation, command, status, events, Arc::clone(inner));
    Ok((active, data))
}

async fn connect_client(
    endpoint: DesktopControlEndpoint,
    timeout: Duration,
) -> Result<DesktopControlClient, ControllerIoError> {
    bounded(timeout, DesktopControlClient::connect(endpoint))
        .await
        .map_err(ControllerIoError::Client)
}

async fn subscribe_events(
    client: DesktopControlClient,
    timeout: Duration,
) -> Result<DesktopControlEventStream, ControllerIoError> {
    bounded(timeout, client.subscribe_events())
        .await
        .map_err(ControllerIoError::Client)
}

async fn fetch_snapshot(
    client: &mut DesktopControlClient,
    timeout: Duration,
) -> Result<ControllerSnapshotData, ControllerIoError> {
    let process = bounded(timeout, client.process_status())
        .await
        .map_err(ControllerIoError::Client)?;
    let list = bounded(timeout, client.list_libraries())
        .await
        .map_err(ControllerIoError::Client)?;
    let libraries_truncated = list.truncated;
    let mut libraries = fetch_library_statuses(client, list, timeout).await?;
    let attention = bounded(timeout, client.attention_snapshot())
        .await
        .map_err(ControllerIoError::Client)
        .and_then(|snapshot| map_attention_response(snapshot, &libraries))?;
    apply_attention_conflict_states(&mut libraries, &attention);
    let profile_config = bounded(timeout, client.get_profile_configuration())
        .await
        .map_err(ControllerIoError::Client)?;
    let sync_control_state = bounded(timeout, client.sync_control_state())
        .await
        .map_err(ControllerIoError::Client)?;

    Ok(ControllerSnapshotData {
        process: map_process_status(process),
        libraries,
        libraries_truncated,
        attention,
        profile_configured: profile_config.configured,
        profile_authenticated: profile_config.authenticated,
        profile_display_name: profile_config
            .server_info
            .as_ref()
            .map(|s| s.display_label.clone()),
        profile_server_url: profile_config
            .server_info
            .as_ref()
            .map(|s| s.base_url.clone()),
        sync_control_state: map_sync_control_state(sync_control_state),
    })
}

async fn fetch_library_statuses(
    client: &mut DesktopControlClient,
    list: ControlLibraryList,
    timeout: Duration,
) -> Result<Vec<DesktopControllerLibraryStatus>, ControllerIoError> {
    let mut seen = BTreeSet::new();
    let mut libraries = Vec::with_capacity(list.libraries.len());
    for listed in list.libraries {
        let library_id = listed
            .library_id
            .parse::<LibraryId>()
            .map_err(|_| ControllerIoError::Malformed)?;
        if !seen.insert(listed.library_id.clone()) {
            return Err(ControllerIoError::Malformed);
        }
        let status = bounded(timeout, client.library_status(library_id))
            .await
            .map_err(ControllerIoError::Client)?;
        if status.library_id != listed.library_id {
            return Err(ControllerIoError::Malformed);
        }
        libraries.push(map_library_status(status));
    }
    libraries.sort_by(|left, right| left.library_id.cmp(&right.library_id));
    Ok(libraries)
}

fn map_attention_response(
    snapshot: ControlAttentionSnapshot,
    available_libraries: &[DesktopControllerLibraryStatus],
) -> Result<DesktopControllerAttentionSnapshot, ControllerIoError> {
    let Some(expected_total) = snapshot
        .summary
        .conflict_count
        .checked_add(snapshot.summary.other_count)
    else {
        return Err(ControllerIoError::Malformed);
    };
    if snapshot.libraries.len() > synveil_client_sync::MAX_ATTENTION_PAGE_LIMIT as usize
        || snapshot.items.len() > synveil_client_sync::MAX_ATTENTION_PAGE_LIMIT as usize
        || snapshot.summary.total_count != expected_total
        || u64::try_from(snapshot.items.len()).unwrap_or(u64::MAX) > snapshot.summary.conflict_count
    {
        return Err(ControllerIoError::Malformed);
    }

    let available_library_ids = available_libraries
        .iter()
        .map(|library| library.library_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut library_ids = BTreeSet::new();
    let mut libraries = Vec::with_capacity(snapshot.libraries.len());
    let mut summary_conflict_count = 0_u64;
    let mut summary_other_count = 0_u64;
    for library in snapshot.libraries {
        let library_id = library
            .library_id
            .parse::<LibraryId>()
            .map_err(|_| ControllerIoError::Malformed)?;
        if !available_library_ids.contains(library.library_id.as_str())
            || !library_ids.insert(library_id)
            || (summary_conflict_count
                .checked_add(library.conflict_count)
                .and_then(|value| value.checked_add(library.other_count)))
            .is_none()
        {
            return Err(ControllerIoError::Malformed);
        }
        summary_conflict_count = summary_conflict_count
            .checked_add(library.conflict_count)
            .ok_or(ControllerIoError::Malformed)?;
        summary_other_count = summary_other_count
            .checked_add(library.other_count)
            .ok_or(ControllerIoError::Malformed)?;
        libraries.push(DesktopControllerAttentionLibrarySummary {
            library_id: library.library_id,
            conflict_count: library.conflict_count,
            other_count: library.other_count,
        });
    }

    if library_ids.len() != available_library_ids.len() {
        return Err(ControllerIoError::Malformed);
    }

    let mut item_ids = BTreeSet::new();
    let mut items = Vec::with_capacity(snapshot.items.len());
    for item in snapshot.items {
        if item.attention_id != item.conflict_id
            || !item_ids.insert(item.attention_id.clone())
            || !available_library_ids.contains(item.library_id.as_str())
            || !library_ids.contains(
                &item
                    .library_id
                    .parse::<LibraryId>()
                    .map_err(|_| ControllerIoError::Malformed)?,
            )
            || item.conflict_id.parse::<SyncConflictId>().is_err()
            || item.intent_id.parse::<OutboundIntentId>().is_err()
            || !item
                .supported_actions
                .contains(&ControlConflictAction::AcceptRemote)
            || has_duplicate_actions(&item.supported_actions)
            || !is_safe_attention_category(&item.category)
            || (matches!(item.category.as_str(), "REMOTE_MISSING" | "NAME_COLLISION")
                && item
                    .supported_actions
                    .contains(&ControlConflictAction::RetryLocalAgainstCurrentBase))
            || !is_safe_attention_path(item.relative_path.as_deref())
            || !is_safe_attention_path(item.previous_relative_path.as_deref())
        {
            return Err(ControllerIoError::Malformed);
        }
        items.push(map_attention_item(item));
    }

    if summary_conflict_count != snapshot.summary.conflict_count
        || summary_other_count != snapshot.summary.other_count
    {
        return Err(ControllerIoError::Malformed);
    }

    Ok(DesktopControllerAttentionSnapshot {
        summary: DesktopControllerAttentionSummary {
            total_count: snapshot.summary.total_count,
            conflict_count: snapshot.summary.conflict_count,
            other_count: snapshot.summary.other_count,
        },
        libraries,
        items,
        truncated: snapshot.truncated,
    })
}

fn map_attention_item(item: ControlAttentionItem) -> DesktopControllerAttentionItem {
    DesktopControllerAttentionItem {
        attention_id: item.attention_id,
        library_id: item.library_id,
        conflict_id: item.conflict_id,
        intent_id: item.intent_id,
        node_id: item.node_id,
        category: item.category,
        relative_path: item.relative_path,
        previous_relative_path: item.previous_relative_path,
        item_kind: item.item_kind.map(map_attention_item_kind),
        local_length: item.local_length,
        remote_length: item.remote_length,
        local_base_revision: item.local_base_revision,
        remote_observed_revision: item.remote_observed_revision,
        remote_observed_state: item.remote_observed_state,
        detected_at_ms: item.detected_at_ms,
        supported_actions: item
            .supported_actions
            .into_iter()
            .map(map_attention_action)
            .collect(),
    }
}

fn map_attention_item_kind(kind: ControlAttentionItemKind) -> DesktopControllerAttentionItemKind {
    match kind {
        ControlAttentionItemKind::File => DesktopControllerAttentionItemKind::File,
        ControlAttentionItemKind::Directory => DesktopControllerAttentionItemKind::Directory,
    }
}

fn map_attention_action(action: ControlConflictAction) -> DesktopControllerConflictAction {
    match action {
        ControlConflictAction::AcceptRemote => DesktopControllerConflictAction::AcceptRemote,
        ControlConflictAction::RetryLocalAgainstCurrentBase => {
            DesktopControllerConflictAction::RetryLocalAgainstCurrentBase
        }
    }
}

fn has_duplicate_actions(actions: &[ControlConflictAction]) -> bool {
    let mut seen = BTreeSet::new();
    actions.iter().any(|action| !seen.insert(*action as u8))
}

fn is_safe_attention_category(category: &str) -> bool {
    matches!(
        category,
        "REMOTE_REVISION_CHANGED"
            | "REMOTE_CONTENT_CHANGED"
            | "REMOTE_STATE_CHANGED"
            | "REMOTE_MISSING"
            | "NAME_COLLISION"
            | "PARENT_CHANGED_OR_UNAVAILABLE"
    )
}

fn is_safe_attention_path(path: Option<&str>) -> bool {
    let Some(path) = path else {
        return true;
    };
    !path.is_empty()
        && path.len() <= 512
        && !path.starts_with('/')
        && !path.starts_with('\\')
        && !path.contains(':')
        && !path.contains('\\')
        && !path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
        && path != ".synveil"
        && !path.starts_with(".synveil/")
        && !path.chars().any(char::is_control)
}

fn apply_attention_conflict_states(
    libraries: &mut [DesktopControllerLibraryStatus],
    attention: &DesktopControllerAttentionSnapshot,
) {
    for library in libraries {
        if let Some(summary) = attention
            .libraries
            .iter()
            .find(|summary| summary.library_id == library.library_id)
        {
            library.conflict_state = if summary.conflict_count == 0 {
                DesktopControllerConflictState::Clear
            } else {
                DesktopControllerConflictState::Required
            };
        }
    }
}

async fn bounded<F, T>(timeout: Duration, future: F) -> Result<T, DesktopControlClientError>
where
    F: Future<Output = Result<T, DesktopControlClientError>>,
{
    time::timeout(timeout, future)
        .await
        .map_err(|_| DesktopControlClientError::Connection)?
}

async fn run_event_reader(
    mut events: DesktopControlEventStream,
    mut stop_rx: oneshot::Receiver<()>,
    signal: Arc<EventSignal>,
    inner: Arc<DesktopControllerInner>,
    generation: u64,
) {
    loop {
        let result = tokio::select! {
            _ = &mut stop_rx => return,
            result = events.next_event() => result,
        };
        match result {
            Ok(ControlEvent::ControlServerStopping) => {
                if inner.is_current_generation(generation) {
                    signal.mark_server_stopping();
                }
            }
            Ok(_) => {
                if inner.is_current_generation(generation) {
                    signal.mark_refresh();
                }
            }
            Err(error) => {
                if inner.is_current_generation(generation) {
                    signal.mark_failure(classify_active_error(error)).await;
                }
                return;
            }
        }
    }
}

async fn run_active(
    inner: &Arc<DesktopControllerInner>,
    active: &mut ActiveConnection,
    timing: DesktopControllerTiming,
    stop_rx: &mut oneshot::Receiver<()>,
    command_rx: &mut mpsc::Receiver<ControllerCommand>,
) -> ActiveExit {
    let result = run_active_loop(inner, active, timing, stop_rx, command_rx).await;
    active.close_event_task().await;
    result
}

async fn run_active_loop(
    inner: &Arc<DesktopControllerInner>,
    active: &mut ActiveConnection,
    timing: DesktopControllerTiming,
    stop_rx: &mut oneshot::Receiver<()>,
    command_rx: &mut mpsc::Receiver<ControllerCommand>,
) -> ActiveExit {
    let mut refresh = RefreshCoordinator::default();
    let mut shutdown_requested = false;
    let mut refresh_timer = time::interval(timing.refresh_interval());
    refresh_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // Consume interval's immediate first tick.  The first coherent snapshot
    // was already fetched during connection, so safety polling is low-rate.
    refresh_timer.tick().await;

    loop {
        if !inner.is_current_generation(active.generation) {
            return ActiveExit::GenerationSuperseded;
        }

        if inner.refresh_requested.swap(false, Ordering::AcqRel) {
            refresh.request();
        }
        refresh.observe_signal(&active.event_signal);

        if refresh.begin() {
            match fetch_snapshot(&mut active.status, timing.operation_timeout()).await {
                Ok(data) => {
                    if !inner.publish_fresh(active.generation, data) {
                        return ActiveExit::GenerationSuperseded;
                    }
                    // Events received while the transaction was in flight are
                    // folded into one follow-up transaction by the coordinator.
                    refresh.finish(&active.event_signal);
                    continue;
                }
                Err(error) => return active_error_exit(error, shutdown_requested),
            }
        }

        tokio::select! {
            biased;
            _ = &mut *stop_rx => return ActiveExit::StopRequested,
            request = command_rx.recv() => {
                let Some(request) = request else {
                    return ActiveExit::StopRequested;
                };
                match execute_command(&mut active.command, &request.action, timing.operation_timeout()).await {
                    Ok(result) => {
                        if result == DesktopControllerCommandResult::ShutdownAccepted {
                            shutdown_requested = true;
                            inner.mark_stopping();
                        }
                        if request.action.is_attention() {
                            inner.request_refresh();
                        }
                        let _ = request.reply.send(result);
                    }
                    Err(error) => {
                        let result = map_command_error(&request.action, error);
                        if request.action.is_attention() {
                            inner.request_refresh();
                        }
                        let transport_loss = is_transport_loss(error);
                        let _ = request.reply.send(result);
                        if transport_loss {
                            return if shutdown_requested {
                                ActiveExit::ProcessStopped
                            } else {
                                ActiveExit::Reconnect(DesktopControllerErrorKind::ConnectionLost)
                            };
                        }
                        if let Some(exit) = terminal_exit_for(error) {
                            return exit;
                        }
                    }
                }
            }
            _ = active.event_signal.notify.notified() => {
                if !inner.is_current_generation(active.generation) {
                    return ActiveExit::GenerationSuperseded;
                }
                if shutdown_requested && active.event_signal.server_is_stopping() {
                    return ActiveExit::ProcessStopped;
                }
                if let Some(error) = active.event_signal.take_failure().await {
                    return if shutdown_requested {
                        ActiveExit::ProcessStopped
                    } else {
                        terminal_or_reconnect(error)
                    };
                }
                refresh.observe_signal(&active.event_signal);
            }
            _ = inner.refresh_notify.notified() => {
                if inner.refresh_requested.swap(false, Ordering::AcqRel) {
                    refresh.request();
                }
            }
            _ = refresh_timer.tick() => refresh.request(),
        }
    }
}

async fn execute_command(
    client: &mut DesktopControlClient,
    action: &CommandAction,
    timeout: Duration,
) -> Result<DesktopControllerCommandResult, DesktopControlClientError> {
    match action {
        CommandAction::SyncNow(library_id) => {
            let result = bounded(timeout, client.sync_now(*library_id)).await?;
            Ok(match result {
                ControlSyncScheduleResult::Queued => DesktopControllerCommandResult::Accepted,
                ControlSyncScheduleResult::Coalesced => DesktopControllerCommandResult::Coalesced,
                ControlSyncScheduleResult::AlreadyRunningFollowupRecorded => {
                    DesktopControllerCommandResult::AlreadyRunningFollowupRecorded
                }
                ControlSyncScheduleResult::Paused => DesktopControllerCommandResult::Paused,
            })
        }
        CommandAction::ResolveConflict(request) => {
            let result = bounded(
                timeout,
                client.resolve_conflict(
                    request.library_id,
                    request.conflict_id,
                    request.intent_id,
                    request.detected_at_ms,
                    map_controller_action(request.action),
                ),
            )
            .await?;
            Ok(map_conflict_resolution_result(result))
        }
        CommandAction::PauseSync => {
            let result = bounded(timeout, client.pause_sync()).await?;
            Ok(map_sync_control_result(result))
        }
        CommandAction::ResumeSync => {
            let result = bounded(timeout, client.resume_sync()).await?;
            Ok(map_sync_control_result(result))
        }
        CommandAction::SetupLibrary(name, root_path) => {
            let result = bounded(
                timeout,
                client.setup_library(name.clone(), root_path.clone()),
            )
            .await?;
            Ok(map_library_setup_result(result))
        }
        CommandAction::Authenticate(secret) => {
            let result = bounded(timeout, client.authenticate(secret)).await?;
            Ok(map_auth_result(result))
        }
        CommandAction::SignOut => {
            let result = bounded(timeout, client.sign_out()).await?;
            Ok(map_auth_result(result))
        }
        CommandAction::Shutdown => {
            bounded(timeout, client.shutdown()).await?;
            Ok(DesktopControllerCommandResult::ShutdownAccepted)
        }
        CommandAction::ValidateProfile(base_url, display_label) => {
            let result = bounded(
                timeout,
                client.validate_profile_configuration(base_url.clone(), display_label.clone()),
            )
            .await?;
            Ok(map_profile_result(result))
        }
        CommandAction::ConfigureProfile(profile_id, base_url, display_label) => {
            let result = bounded(
                timeout,
                client.configure_profile(
                    profile_id.to_string(),
                    base_url.clone(),
                    display_label.clone(),
                ),
            )
            .await?;
            Ok(map_profile_result(result))
        }
    }
}

fn map_command_error(
    action: &CommandAction,
    error: DesktopControlClientError,
) -> DesktopControllerCommandResult {
    match error {
        DesktopControlClientError::Server(ControlErrorCode::UnknownLibrary) => {
            if action.is_auth() {
                DesktopControllerCommandResult::ServerUnavailable
            } else {
                DesktopControllerCommandResult::UnknownLibrary
            }
        }
        DesktopControlClientError::Server(ControlErrorCode::RuntimeStopped) => {
            if action.is_auth() {
                DesktopControllerCommandResult::ServerUnavailable
            } else {
                DesktopControllerCommandResult::RuntimeStopped
            }
        }
        DesktopControlClientError::Server(_) => DesktopControllerCommandResult::ProtocolError,
        DesktopControlClientError::Connection
        | DesktopControlClientError::Closed
        | DesktopControlClientError::Frame(
            crate::ControlFrameError::Closed
            | crate::ControlFrameError::Io
            | crate::ControlFrameError::Truncated,
        ) => DesktopControllerCommandResult::OutcomeUnknown,
        DesktopControlClientError::ProtocolVersionUnsupported => {
            DesktopControllerCommandResult::ProtocolError
        }
        DesktopControlClientError::UnsupportedPlatform
        | DesktopControlClientError::Endpoint(_)
        | DesktopControlClientError::Handshake
        | DesktopControlClientError::Frame(_)
        | DesktopControlClientError::ResponseMismatch
        | DesktopControlClientError::UnexpectedResponse => {
            DesktopControllerCommandResult::ProtocolError
        }
    }
}

fn map_controller_action(action: DesktopControllerConflictAction) -> ControlConflictAction {
    match action {
        DesktopControllerConflictAction::AcceptRemote => ControlConflictAction::AcceptRemote,
        DesktopControllerConflictAction::RetryLocalAgainstCurrentBase => {
            ControlConflictAction::RetryLocalAgainstCurrentBase
        }
    }
}

fn map_conflict_resolution_result(
    result: ControlConflictResolutionOutcome,
) -> DesktopControllerCommandResult {
    match result {
        ControlConflictResolutionOutcome::Resolved => {
            DesktopControllerCommandResult::ConflictResolved
        }
        ControlConflictResolutionOutcome::AlreadyResolved => {
            DesktopControllerCommandResult::ConflictAlreadyResolved
        }
        ControlConflictResolutionOutcome::Stale => DesktopControllerCommandResult::ConflictStale,
        ControlConflictResolutionOutcome::NotFound => {
            DesktopControllerCommandResult::ConflictNotFound
        }
        ControlConflictResolutionOutcome::UnsupportedAction => {
            DesktopControllerCommandResult::UnsupportedConflictAction
        }
        ControlConflictResolutionOutcome::PersistenceFailure => {
            DesktopControllerCommandResult::ConflictPersistenceFailure
        }
        ControlConflictResolutionOutcome::Busy => DesktopControllerCommandResult::Busy,
    }
}

fn map_auth_result(result: ControlAuthOutcome) -> DesktopControllerCommandResult {
    match result {
        ControlAuthOutcome::Authenticated => DesktopControllerCommandResult::Authenticated,
        ControlAuthOutcome::SignedOut => DesktopControllerCommandResult::SignedOut,
        ControlAuthOutcome::InvalidCredentials => {
            DesktopControllerCommandResult::InvalidCredentials
        }
        ControlAuthOutcome::NetworkUnavailable => {
            DesktopControllerCommandResult::NetworkUnavailable
        }
        ControlAuthOutcome::ServerUnavailable => DesktopControllerCommandResult::ServerUnavailable,
        ControlAuthOutcome::RateLimited => DesktopControllerCommandResult::RateLimited,
        ControlAuthOutcome::SecureStoreUnavailable => {
            DesktopControllerCommandResult::SecureStoreUnavailable
        }
        ControlAuthOutcome::Busy => DesktopControllerCommandResult::Busy,
        ControlAuthOutcome::ProtocolError => DesktopControllerCommandResult::ProtocolError,
        ControlAuthOutcome::OutcomeUnknown => DesktopControllerCommandResult::OutcomeUnknown,
    }
}

fn map_sync_control_state(state: ControlSyncControlState) -> DesktopControllerSyncControlState {
    match state {
        ControlSyncControlState::Running => DesktopControllerSyncControlState::Running,
        ControlSyncControlState::PausedByUser => DesktopControllerSyncControlState::PausedByUser,
    }
}

fn map_sync_control_result(result: ControlSyncControlResult) -> DesktopControllerCommandResult {
    match result {
        ControlSyncControlResult::Paused => DesktopControllerCommandResult::Paused,
        ControlSyncControlResult::Resumed => DesktopControllerCommandResult::Resumed,
        ControlSyncControlResult::AlreadyPaused => DesktopControllerCommandResult::AlreadyPaused,
        ControlSyncControlResult::AlreadyRunning => DesktopControllerCommandResult::AlreadyRunning,
        ControlSyncControlResult::PersistenceFailure => {
            DesktopControllerCommandResult::PersistenceFailure
        }
        ControlSyncControlResult::Busy => DesktopControllerCommandResult::Busy,
    }
}

fn map_library_setup_result(
    result: crate::ControlLibrarySetupOutcome,
) -> DesktopControllerCommandResult {
    match result {
        crate::ControlLibrarySetupOutcome::Configured => {
            DesktopControllerCommandResult::LibraryConfigured
        }
        crate::ControlLibrarySetupOutcome::AlreadyConfigured => {
            DesktopControllerCommandResult::LibraryAlreadyConfigured
        }
        crate::ControlLibrarySetupOutcome::InvalidName => {
            DesktopControllerCommandResult::InvalidLibraryName
        }
        crate::ControlLibrarySetupOutcome::InvalidRoot => {
            DesktopControllerCommandResult::InvalidLibraryRoot
        }
        crate::ControlLibrarySetupOutcome::AuthenticationRequired => {
            DesktopControllerCommandResult::AuthenticationRequired
        }
        crate::ControlLibrarySetupOutcome::NetworkUnavailable => {
            DesktopControllerCommandResult::NetworkUnavailable
        }
        crate::ControlLibrarySetupOutcome::ServerUnavailable => {
            DesktopControllerCommandResult::ServerUnavailable
        }
        crate::ControlLibrarySetupOutcome::Timeout => DesktopControllerCommandResult::Timeout,
        crate::ControlLibrarySetupOutcome::TlsFailure => DesktopControllerCommandResult::TlsFailure,
        crate::ControlLibrarySetupOutcome::ServerIdentityConflict => {
            DesktopControllerCommandResult::ServerIdentityConflict
        }
        crate::ControlLibrarySetupOutcome::PersistenceFailure => {
            DesktopControllerCommandResult::PersistenceFailure
        }
        crate::ControlLibrarySetupOutcome::Busy => DesktopControllerCommandResult::Busy,
        crate::ControlLibrarySetupOutcome::OutcomeUnknown => {
            DesktopControllerCommandResult::OutcomeUnknown
        }
        crate::ControlLibrarySetupOutcome::Unavailable => {
            DesktopControllerCommandResult::Unavailable
        }
        crate::ControlLibrarySetupOutcome::ProtocolError => {
            DesktopControllerCommandResult::ProtocolError
        }
    }
}

fn map_profile_result(
    result: ControlProfileConfigurationOutcome,
) -> DesktopControllerCommandResult {
    match result {
        ControlProfileConfigurationOutcome::Validated => {
            DesktopControllerCommandResult::ProfileValidated
        }
        ControlProfileConfigurationOutcome::Created
        | ControlProfileConfigurationOutcome::Updated => {
            DesktopControllerCommandResult::ProfileConfigured
        }
        ControlProfileConfigurationOutcome::AlreadyConfigured => {
            DesktopControllerCommandResult::ProfileAlreadyConfigured
        }
        ControlProfileConfigurationOutcome::NotFound => {
            DesktopControllerCommandResult::InvalidConfiguration
        }
        ControlProfileConfigurationOutcome::InvalidConfiguration => {
            DesktopControllerCommandResult::InvalidConfiguration
        }
        ControlProfileConfigurationOutcome::InvalidServerAddress => {
            DesktopControllerCommandResult::InvalidServerAddress
        }
        ControlProfileConfigurationOutcome::NetworkUnavailable
        | ControlProfileConfigurationOutcome::ConnectionRefused => {
            DesktopControllerCommandResult::NetworkUnavailable
        }
        ControlProfileConfigurationOutcome::Timeout => DesktopControllerCommandResult::Timeout,
        ControlProfileConfigurationOutcome::TlsFailure => {
            DesktopControllerCommandResult::TlsFailure
        }
        ControlProfileConfigurationOutcome::IncompatibleServer => {
            DesktopControllerCommandResult::IncompatibleServer
        }
        ControlProfileConfigurationOutcome::ServerFailure => {
            DesktopControllerCommandResult::ServerUnavailable
        }
        ControlProfileConfigurationOutcome::PersistenceFailure => {
            DesktopControllerCommandResult::PersistenceFailure
        }
        ControlProfileConfigurationOutcome::Busy => DesktopControllerCommandResult::Busy,
        ControlProfileConfigurationOutcome::OutcomeUnknown => {
            DesktopControllerCommandResult::OutcomeUnknown
        }
    }
}

fn active_error_exit(error: ControllerIoError, shutdown_requested: bool) -> ActiveExit {
    match error {
        ControllerIoError::Malformed => ActiveExit::Terminal(
            DesktopControllerConnectionState::Faulted,
            DesktopControllerErrorKind::MalformedServerResponse,
        ),
        ControllerIoError::Client(error) => {
            if shutdown_requested && is_transport_loss(error) {
                ActiveExit::ProcessStopped
            } else if let Some(exit) = terminal_exit_for(error) {
                exit
            } else {
                ActiveExit::Reconnect(classify_active_error(error))
            }
        }
    }
}

fn terminal_or_reconnect(error: DesktopControllerErrorKind) -> ActiveExit {
    match error {
        DesktopControllerErrorKind::EndpointSecurity
        | DesktopControllerErrorKind::ProtocolIncompatible
        | DesktopControllerErrorKind::MalformedServerResponse
        | DesktopControllerErrorKind::UnsupportedPlatform
        | DesktopControllerErrorKind::Internal => {
            let state = if error == DesktopControllerErrorKind::ProtocolIncompatible {
                DesktopControllerConnectionState::ProtocolIncompatible
            } else {
                DesktopControllerConnectionState::Faulted
            };
            ActiveExit::Terminal(state, error)
        }
        DesktopControllerErrorKind::EndpointUnavailable
        | DesktopControllerErrorKind::ConnectionLost
        | DesktopControllerErrorKind::CommandRejected
        | DesktopControllerErrorKind::Stopped => ActiveExit::Reconnect(error),
    }
}

fn terminal_exit_for(error: DesktopControlClientError) -> Option<ActiveExit> {
    let kind = classify_active_error(error);
    match kind {
        DesktopControllerErrorKind::EndpointSecurity
        | DesktopControllerErrorKind::ProtocolIncompatible
        | DesktopControllerErrorKind::MalformedServerResponse
        | DesktopControllerErrorKind::UnsupportedPlatform => {
            let state = if kind == DesktopControllerErrorKind::ProtocolIncompatible {
                DesktopControllerConnectionState::ProtocolIncompatible
            } else {
                DesktopControllerConnectionState::Faulted
            };
            Some(ActiveExit::Terminal(state, kind))
        }
        _ => None,
    }
}

fn is_transport_loss(error: DesktopControlClientError) -> bool {
    matches!(
        error,
        DesktopControlClientError::Connection
            | DesktopControlClientError::Closed
            | DesktopControlClientError::Frame(
                crate::ControlFrameError::Closed
                    | crate::ControlFrameError::Io
                    | crate::ControlFrameError::Truncated
            )
            | DesktopControlClientError::Server(ControlErrorCode::ControlServerStopping)
    )
}

fn classify_connection_failure(error: ControllerIoError) -> ConnectionFailure {
    match error {
        ControllerIoError::Malformed => ConnectionFailure::Terminal(
            DesktopControllerConnectionState::Faulted,
            DesktopControllerErrorKind::MalformedServerResponse,
        ),
        ControllerIoError::Client(error) => {
            let kind = classify_connect_error(error);
            if matches!(
                kind,
                DesktopControllerErrorKind::EndpointUnavailable
                    | DesktopControllerErrorKind::ConnectionLost
            ) {
                ConnectionFailure::Retry(kind)
            } else {
                ConnectionFailure::Terminal(
                    if kind == DesktopControllerErrorKind::ProtocolIncompatible {
                        DesktopControllerConnectionState::ProtocolIncompatible
                    } else {
                        DesktopControllerConnectionState::Faulted
                    },
                    kind,
                )
            }
        }
    }
}

fn classify_connect_error(error: DesktopControlClientError) -> DesktopControllerErrorKind {
    match error {
        DesktopControlClientError::UnsupportedPlatform => {
            DesktopControllerErrorKind::UnsupportedPlatform
        }
        DesktopControlClientError::Endpoint(error) => match error {
            crate::DesktopControlServerError::UnsafeEndpoint
            | crate::DesktopControlServerError::InsecureRuntimeDirectory
            | crate::DesktopControlServerError::EndpointStateUnknown
            | crate::DesktopControlServerError::SecurityDescriptorUnavailable
            | crate::DesktopControlServerError::EndpointNameTooLong => {
                DesktopControllerErrorKind::EndpointSecurity
            }
            crate::DesktopControlServerError::UnsupportedPlatform => {
                DesktopControllerErrorKind::UnsupportedPlatform
            }
            _ => DesktopControllerErrorKind::EndpointUnavailable,
        },
        DesktopControlClientError::Connection => DesktopControllerErrorKind::EndpointUnavailable,
        DesktopControlClientError::ProtocolVersionUnsupported => {
            DesktopControllerErrorKind::ProtocolIncompatible
        }
        DesktopControlClientError::Server(ControlErrorCode::ProtocolVersionUnsupported) => {
            DesktopControllerErrorKind::ProtocolIncompatible
        }
        DesktopControlClientError::Server(ControlErrorCode::EndpointUnsafe) => {
            DesktopControllerErrorKind::EndpointSecurity
        }
        DesktopControlClientError::Server(ControlErrorCode::UnknownLibrary)
        | DesktopControlClientError::Server(ControlErrorCode::RuntimeStopped)
        | DesktopControlClientError::Server(ControlErrorCode::ControlServerStopping)
        | DesktopControlClientError::Closed
        | DesktopControlClientError::Frame(
            crate::ControlFrameError::Closed
            | crate::ControlFrameError::Io
            | crate::ControlFrameError::Truncated,
        ) => DesktopControllerErrorKind::EndpointUnavailable,
        DesktopControlClientError::Handshake
        | DesktopControlClientError::ResponseMismatch
        | DesktopControlClientError::UnexpectedResponse
        | DesktopControlClientError::Frame(_) => {
            DesktopControllerErrorKind::MalformedServerResponse
        }
        DesktopControlClientError::Server(_) => DesktopControllerErrorKind::MalformedServerResponse,
    }
}

fn classify_active_error(error: DesktopControlClientError) -> DesktopControllerErrorKind {
    match error {
        DesktopControlClientError::Endpoint(_) => classify_connect_error(error),
        DesktopControlClientError::Connection
        | DesktopControlClientError::Closed
        | DesktopControlClientError::Frame(
            crate::ControlFrameError::Closed
            | crate::ControlFrameError::Io
            | crate::ControlFrameError::Truncated,
        )
        | DesktopControlClientError::Server(ControlErrorCode::ControlServerStopping) => {
            DesktopControllerErrorKind::ConnectionLost
        }
        _ => classify_connect_error(error),
    }
}

enum ConnectionFailure {
    Retry(DesktopControllerErrorKind),
    Terminal(DesktopControllerConnectionState, DesktopControllerErrorKind),
}

enum ControllerIoError {
    Client(DesktopControlClientError),
    Malformed,
}

enum ActiveExit {
    StopRequested,
    ProcessStopped,
    GenerationSuperseded,
    Reconnect(DesktopControllerErrorKind),
    Terminal(DesktopControllerConnectionState, DesktopControllerErrorKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReconnectBackoff {
    current: Duration,
    initial: Duration,
    cap: Duration,
}

impl ReconnectBackoff {
    fn new(timing: DesktopControllerTiming) -> Self {
        Self {
            current: timing.reconnect_initial(),
            initial: timing.reconnect_initial(),
            cap: timing.reconnect_cap(),
        }
    }

    fn next_delay(&mut self) -> Duration {
        let delay = self.current;
        self.current = self
            .current
            .checked_mul(2)
            .unwrap_or(self.cap)
            .min(self.cap);
        delay
    }

    fn reset(&mut self) {
        self.current = self.initial;
    }
}

async fn wait_for_stop(stop_rx: &mut oneshot::Receiver<()>, delay: Duration) -> bool {
    tokio::select! {
        _ = &mut *stop_rx => true,
        _ = time::sleep(delay) => false,
    }
}

fn drain_commands(
    command_rx: &mut mpsc::Receiver<ControllerCommand>,
    result: DesktopControllerCommandResult,
) {
    while let Ok(request) = command_rx.try_recv() {
        let _ = request.reply.send(result);
    }
}

fn map_process_status(status: ControlProcessStatus) -> DesktopControllerProcessStatus {
    DesktopControllerProcessStatus {
        state: status.state,
        control_ready: status.control_ready,
    }
}

fn map_library_status(status: ControlLibraryStatus) -> DesktopControllerLibraryStatus {
    DesktopControllerLibraryStatus {
        library_id: status.library_id,
        runtime_state: map_runtime_state(status.runtime_state),
        root_state: map_root_state(status.root_state),
        auth_state: map_auth_state(status.auth_state),
        conflict_state: map_conflict_state(status.conflict_state),
        next_due_ms: status.next_due_ms,
        last_outcome: status.last_outcome.map(map_sync_outcome),
        wake_pending: status.wake_pending,
        transient_failures: status.transient_failures,
    }
}

fn map_runtime_state(state: ControlRuntimeState) -> DesktopControllerRuntimeState {
    match state {
        ControlRuntimeState::Idle => DesktopControllerRuntimeState::Idle,
        ControlRuntimeState::Scheduled => DesktopControllerRuntimeState::Scheduled,
        ControlRuntimeState::Running => DesktopControllerRuntimeState::Running,
        ControlRuntimeState::BackingOff => DesktopControllerRuntimeState::BackingOff,
        ControlRuntimeState::AuthBlocked => DesktopControllerRuntimeState::AuthBlocked,
        ControlRuntimeState::RootBlocked => DesktopControllerRuntimeState::RootBlocked,
        ControlRuntimeState::Faulted => DesktopControllerRuntimeState::Faulted,
        ControlRuntimeState::Stopped => DesktopControllerRuntimeState::Stopped,
    }
}

fn map_root_state(state: ControlRootState) -> DesktopControllerRootState {
    match state {
        ControlRootState::Available => DesktopControllerRootState::Available,
        ControlRootState::Unavailable => DesktopControllerRootState::Unavailable,
        ControlRootState::Recovering => DesktopControllerRootState::Recovering,
    }
}

fn map_auth_state(state: ControlAuthState) -> DesktopControllerAuthState {
    match state {
        ControlAuthState::Ready => DesktopControllerAuthState::Ready,
        ControlAuthState::Missing => DesktopControllerAuthState::Missing,
        ControlAuthState::Blocked => DesktopControllerAuthState::Blocked,
        ControlAuthState::Revoked => DesktopControllerAuthState::Revoked,
        ControlAuthState::Unknown => DesktopControllerAuthState::Unknown,
    }
}

fn map_conflict_state(state: ControlConflictState) -> DesktopControllerConflictState {
    match state {
        ControlConflictState::Clear => DesktopControllerConflictState::Clear,
        ControlConflictState::Required => DesktopControllerConflictState::Required,
        ControlConflictState::Unknown => DesktopControllerConflictState::Unknown,
    }
}

fn map_sync_outcome(outcome: ControlSyncOutcome) -> DesktopControllerSyncOutcome {
    match outcome {
        ControlSyncOutcome::Idle => DesktopControllerSyncOutcome::Idle,
        ControlSyncOutcome::Progress => DesktopControllerSyncOutcome::Progress,
        ControlSyncOutcome::ConflictBlocked => DesktopControllerSyncOutcome::ConflictBlocked,
        ControlSyncOutcome::Offline => DesktopControllerSyncOutcome::Offline,
        ControlSyncOutcome::ServerTransient => DesktopControllerSyncOutcome::ServerTransient,
        ControlSyncOutcome::RateLimited => DesktopControllerSyncOutcome::RateLimited,
        ControlSyncOutcome::AuthBlocked => DesktopControllerSyncOutcome::AuthBlocked,
        ControlSyncOutcome::RootUnavailable => DesktopControllerSyncOutcome::RootUnavailable,
        ControlSyncOutcome::RecoveryBlocked => DesktopControllerSyncOutcome::RecoveryBlocked,
        ControlSyncOutcome::FatalLocal => DesktopControllerSyncOutcome::FatalLocal,
        ControlSyncOutcome::Panicked => DesktopControllerSyncOutcome::Panicked,
    }
}

#[cfg(test)]
mod tests {
    use crate::{ControlAttentionLibrarySummary, ControlAttentionSummary};

    use super::*;

    fn timing() -> DesktopControllerTiming {
        DesktopControllerTiming::new(
            Duration::from_millis(250),
            Duration::from_secs(5),
            Duration::from_secs(30),
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .expect("valid timing")
    }

    fn recovery_library(
        runtime_state: DesktopControllerRuntimeState,
        root_state: DesktopControllerRootState,
        auth_state: DesktopControllerAuthState,
        last_outcome: Option<DesktopControllerSyncOutcome>,
    ) -> DesktopControllerLibraryStatus {
        DesktopControllerLibraryStatus {
            library_id: LibraryId::new().to_string(),
            runtime_state,
            root_state,
            auth_state,
            conflict_state: DesktopControllerConflictState::Clear,
            next_due_ms: None,
            last_outcome,
            wake_pending: false,
            transient_failures: 0,
        }
    }

    fn ready_recovery_snapshot(
        libraries: Vec<DesktopControllerLibraryStatus>,
    ) -> DesktopControllerSnapshot {
        DesktopControllerSnapshot {
            connection_state: DesktopControllerConnectionState::Connected,
            process: Some(DesktopControllerProcessStatus {
                state: DesktopProcessStatus::Running,
                control_ready: true,
            }),
            libraries,
            libraries_truncated: false,
            attention: DesktopControllerAttentionSnapshot::default(),
            revision: 7,
            freshness: DesktopControllerFreshness::Fresh,
            last_error: None,
            connection_generation: 23,
            profile_configured: true,
            profile_authenticated: true,
            profile_display_name: None,
            profile_server_url: None,
            sync_control_state: DesktopControllerSyncControlState::Running,
        }
    }

    #[test]
    fn construction_is_disconnected_and_side_effect_free() {
        let endpoint = DesktopControlEndpoint::NamedPipe {
            name: "test-controller".to_owned(),
        };
        let controller = DesktopController::new(DesktopControllerConfig::for_endpoint(endpoint));
        assert_eq!(controller.snapshot(), DesktopControllerSnapshot::default());
    }

    #[tokio::test]
    async fn invalid_auth_input_is_rejected_without_starting_controller_work() {
        let controller = DesktopController::new(DesktopControllerConfig::for_endpoint(
            DesktopControlEndpoint::NamedPipe {
                name: "test-controller".to_owned(),
            },
        ));
        assert_eq!(
            controller.authenticate("not-a-token").await,
            Ok(DesktopControllerCommandResult::InvalidCredentials)
        );
        assert_eq!(controller.snapshot(), DesktopControllerSnapshot::default());
    }

    #[test]
    fn authentication_admission_is_single_and_released_after_completion() {
        let flag = Arc::new(AtomicBool::new(false));
        let first = AuthAdmission::try_acquire(Arc::clone(&flag)).expect("first admission");
        assert!(AuthAdmission::try_acquire(Arc::clone(&flag)).is_none());
        drop(first);
        assert!(AuthAdmission::try_acquire(flag).is_some());
    }

    #[test]
    fn authentication_admission_is_bounded_under_a_thousand_attempts() {
        let flag = Arc::new(AtomicBool::new(false));
        let first = AuthAdmission::try_acquire(Arc::clone(&flag)).expect("first admission");
        assert!(flag.load(Ordering::Acquire));
        for _ in 0..999 {
            assert!(AuthAdmission::try_acquire(Arc::clone(&flag)).is_none());
        }
        drop(first);
        assert!(AuthAdmission::try_acquire(flag).is_some());
    }

    #[test]
    fn attention_admission_is_bounded_under_a_thousand_attempts() {
        let flag = Arc::new(AtomicBool::new(false));
        let first =
            AttentionAdmission::try_acquire(Arc::clone(&flag)).expect("first attention admission");
        assert!(flag.load(Ordering::Acquire));
        for _ in 0..999 {
            assert!(AttentionAdmission::try_acquire(Arc::clone(&flag)).is_none());
        }
        drop(first);
        assert!(AttentionAdmission::try_acquire(flag).is_some());
    }

    #[test]
    fn attention_response_validation_keeps_scope_and_action_matrix_safe() {
        let library_id = LibraryId::new();
        let conflict_id = SyncConflictId::new();
        let available = vec![DesktopControllerLibraryStatus {
            library_id: library_id.to_string(),
            runtime_state: DesktopControllerRuntimeState::Idle,
            root_state: DesktopControllerRootState::Available,
            auth_state: DesktopControllerAuthState::Ready,
            conflict_state: DesktopControllerConflictState::Clear,
            next_due_ms: None,
            last_outcome: None,
            wake_pending: false,
            transient_failures: 0,
        }];
        let item = ControlAttentionItem {
            attention_id: conflict_id.to_string(),
            library_id: library_id.to_string(),
            conflict_id: conflict_id.to_string(),
            intent_id: OutboundIntentId::new().to_string(),
            node_id: None,
            category: "REMOTE_REVISION_CHANGED".to_owned(),
            relative_path: Some("folder/file.txt".to_owned()),
            previous_relative_path: None,
            item_kind: Some(ControlAttentionItemKind::File),
            local_length: Some(7),
            remote_length: Some(8),
            local_base_revision: Some(1),
            remote_observed_revision: Some(2),
            remote_observed_state: Some("ACTIVE".to_owned()),
            detected_at_ms: 11,
            supported_actions: vec![ControlConflictAction::AcceptRemote],
        };
        let valid = ControlAttentionSnapshot {
            summary: ControlAttentionSummary {
                total_count: 1,
                conflict_count: 1,
                other_count: 0,
            },
            libraries: vec![ControlAttentionLibrarySummary {
                library_id: library_id.to_string(),
                conflict_count: 1,
                other_count: 0,
            }],
            items: vec![item.clone()],
            truncated: false,
        };
        let mapped = match map_attention_response(valid.clone(), &available) {
            Ok(mapped) => mapped,
            Err(_) => panic!("safe attention snapshot must map"),
        };
        assert_eq!(
            mapped.items[0].relative_path.as_deref(),
            Some("folder/file.txt")
        );
        assert_eq!(mapped.items[0].supported_actions.len(), 1);

        let missing_library_summary = ControlAttentionSnapshot {
            summary: ControlAttentionSummary {
                total_count: 0,
                conflict_count: 0,
                other_count: 0,
            },
            libraries: Vec::new(),
            items: Vec::new(),
            truncated: false,
        };
        assert!(matches!(
            map_attention_response(missing_library_summary, &available),
            Err(ControllerIoError::Malformed)
        ));

        let mut absolute_path = valid.clone();
        absolute_path.items[0].relative_path = Some("/tmp/file.txt".to_owned());
        assert!(matches!(
            map_attention_response(absolute_path, &available),
            Err(ControllerIoError::Malformed)
        ));

        let mut prohibited_retry = valid;
        prohibited_retry.items[0].category = "REMOTE_MISSING".to_owned();
        prohibited_retry.items[0]
            .supported_actions
            .push(ControlConflictAction::RetryLocalAgainstCurrentBase);
        assert!(matches!(
            map_attention_response(prohibited_retry, &available),
            Err(ControllerIoError::Malformed)
        ));
    }

    #[test]
    fn sync_control_admission_is_bounded_under_a_thousand_attempts() {
        let flag = Arc::new(AtomicBool::new(false));
        let first = SyncControlAdmission::try_acquire(Arc::clone(&flag))
            .expect("first sync-control admission");
        assert!(flag.load(Ordering::Acquire));
        for _ in 0..999 {
            assert!(SyncControlAdmission::try_acquire(Arc::clone(&flag)).is_none());
        }
        drop(first);
        assert!(SyncControlAdmission::try_acquire(flag).is_some());
    }

    #[test]
    fn sync_control_wire_states_and_results_map_to_safe_controller_categories() {
        assert_eq!(
            map_sync_control_state(ControlSyncControlState::Running),
            DesktopControllerSyncControlState::Running
        );
        assert_eq!(
            map_sync_control_state(ControlSyncControlState::PausedByUser),
            DesktopControllerSyncControlState::PausedByUser
        );
        for (wire, expected) in [
            (
                ControlSyncControlResult::Paused,
                DesktopControllerCommandResult::Paused,
            ),
            (
                ControlSyncControlResult::Resumed,
                DesktopControllerCommandResult::Resumed,
            ),
            (
                ControlSyncControlResult::AlreadyPaused,
                DesktopControllerCommandResult::AlreadyPaused,
            ),
            (
                ControlSyncControlResult::AlreadyRunning,
                DesktopControllerCommandResult::AlreadyRunning,
            ),
            (
                ControlSyncControlResult::PersistenceFailure,
                DesktopControllerCommandResult::PersistenceFailure,
            ),
            (
                ControlSyncControlResult::Busy,
                DesktopControllerCommandResult::Busy,
            ),
        ] {
            assert_eq!(map_sync_control_result(wire), expected);
        }
    }

    #[tokio::test]
    async fn auth_state_refresh_signal_is_coalesced() {
        let controller = DesktopController::new(DesktopControllerConfig::for_endpoint(
            DesktopControlEndpoint::NamedPipe {
                name: "test-controller".to_owned(),
            },
        ));
        let notified = controller.inner.refresh_notify.notified();
        controller.refresh_auth_state();
        controller.refresh_auth_state();
        tokio::time::timeout(Duration::from_secs(1), notified)
            .await
            .expect("refresh signal must wake the controller");
        assert!(
            controller
                .inner
                .refresh_requested
                .swap(false, Ordering::AcqRel)
        );
        let second = controller.inner.refresh_notify.notified();
        assert!(
            tokio::time::timeout(Duration::from_millis(1), second)
                .await
                .is_err()
        );
    }

    #[test]
    fn authentication_results_map_to_safe_controller_categories() {
        for (wire, expected) in [
            (
                ControlAuthOutcome::Authenticated,
                DesktopControllerCommandResult::Authenticated,
            ),
            (
                ControlAuthOutcome::SignedOut,
                DesktopControllerCommandResult::SignedOut,
            ),
            (
                ControlAuthOutcome::InvalidCredentials,
                DesktopControllerCommandResult::InvalidCredentials,
            ),
            (
                ControlAuthOutcome::NetworkUnavailable,
                DesktopControllerCommandResult::NetworkUnavailable,
            ),
            (
                ControlAuthOutcome::ServerUnavailable,
                DesktopControllerCommandResult::ServerUnavailable,
            ),
            (
                ControlAuthOutcome::RateLimited,
                DesktopControllerCommandResult::RateLimited,
            ),
            (
                ControlAuthOutcome::SecureStoreUnavailable,
                DesktopControllerCommandResult::SecureStoreUnavailable,
            ),
            (
                ControlAuthOutcome::Busy,
                DesktopControllerCommandResult::Busy,
            ),
            (
                ControlAuthOutcome::ProtocolError,
                DesktopControllerCommandResult::ProtocolError,
            ),
            (
                ControlAuthOutcome::OutcomeUnknown,
                DesktopControllerCommandResult::OutcomeUnknown,
            ),
        ] {
            assert_eq!(map_auth_result(wire), expected);
        }
    }

    #[test]
    fn reconnect_backoff_is_bounded_and_resets() {
        let mut backoff = ReconnectBackoff::new(timing());
        assert_eq!(backoff.next_delay(), Duration::from_millis(250));
        assert_eq!(backoff.next_delay(), Duration::from_millis(500));
        assert_eq!(backoff.next_delay(), Duration::from_secs(1));
        assert_eq!(backoff.next_delay(), Duration::from_secs(2));
        assert_eq!(backoff.next_delay(), Duration::from_secs(4));
        assert_eq!(backoff.next_delay(), Duration::from_secs(5));
        assert_eq!(backoff.next_delay(), Duration::from_secs(5));
        backoff.reset();
        assert_eq!(backoff.next_delay(), Duration::from_millis(250));
    }

    #[test]
    fn generation_fence_rejects_old_response_and_event() {
        let (snapshot, _) = watch::channel(DesktopControllerSnapshot::default());
        let inner = DesktopControllerInner {
            config: DesktopControllerConfig::for_endpoint(DesktopControlEndpoint::NamedPipe {
                name: "test-controller".to_owned(),
            }),
            snapshot,
            lifecycle: Mutex::new(ControllerLifecycle {
                task: None,
                stop: None,
                command: None,
                stopping: false,
            }),
            current_generation: AtomicU64::new(2),
            auth_in_flight: Arc::new(AtomicBool::new(false)),
            configuration_in_flight: Arc::new(AtomicBool::new(false)),
            sync_control_in_flight: Arc::new(AtomicBool::new(false)),
            attention_in_flight: Arc::new(AtomicBool::new(false)),
            refresh_requested: AtomicBool::new(false),
            refresh_notify: Notify::new(),
        };
        assert!(!inner.is_current_generation(1));
        assert!(inner.is_current_generation(2));
        assert!(!inner.publish_fresh(
            1,
            ControllerSnapshotData {
                process: DesktopControllerProcessStatus {
                    state: DesktopProcessStatus::Running,
                    control_ready: true,
                },
                libraries: Vec::new(),
                libraries_truncated: false,
                attention: DesktopControllerAttentionSnapshot::default(),
                profile_configured: false,
                profile_authenticated: false,
                profile_display_name: None,
                profile_server_url: None,
                sync_control_state: DesktopControllerSyncControlState::Running,
            }
        ));
        assert_eq!(inner.snapshot().revision, 0);
    }

    #[test]
    fn snapshot_serialization_contains_no_private_material() {
        let snapshot = DesktopControllerSnapshot {
            connection_state: DesktopControllerConnectionState::Connected,
            process: Some(DesktopControllerProcessStatus {
                state: DesktopProcessStatus::Running,
                control_ready: true,
            }),
            libraries: vec![DesktopControllerLibraryStatus {
                library_id: LibraryId::new().to_string(),
                runtime_state: DesktopControllerRuntimeState::Idle,
                root_state: DesktopControllerRootState::Available,
                auth_state: DesktopControllerAuthState::Unknown,
                conflict_state: DesktopControllerConflictState::Clear,
                next_due_ms: None,
                last_outcome: None,
                wake_pending: false,
                transient_failures: 0,
            }],
            libraries_truncated: false,
            attention: DesktopControllerAttentionSnapshot::default(),
            revision: 1,
            freshness: DesktopControllerFreshness::Fresh,
            last_error: None,
            connection_generation: 1,
            profile_configured: true,
            profile_authenticated: false,
            profile_display_name: Some("Fixture server".to_owned()),
            profile_server_url: Some("https://server.example/".to_owned()),
            sync_control_state: DesktopControllerSyncControlState::Running,
        };
        let encoded = serde_json::to_string(&snapshot).expect("snapshot serializes");
        let lower = encoded.to_ascii_lowercase();
        for forbidden in [
            "token",
            "cookie",
            "password",
            "authorization",
            "credential",
            "root_path",
        ] {
            assert!(!lower.contains(forbidden), "found {forbidden} in {encoded}");
        }
        assert!(encoded.contains("https://server.example/"));
    }

    #[test]
    fn recovery_projection_advertises_start_only_when_client_is_unavailable() {
        let summary = DesktopControllerSnapshot::default().recovery_summary();
        assert_eq!(
            summary.client_state,
            DesktopControllerClientRecoveryState::Unavailable
        );
        assert_eq!(summary.total_action_required, 1);
        assert_eq!(summary.total_waiting, 0);
        assert_eq!(summary.items.len(), 1);
        assert_eq!(
            summary.items[0].action,
            Some(DesktopControllerRecoveryAction::StartClient)
        );
        assert_eq!(summary.items[0].library_id, None);
    }

    #[test]
    fn recovery_projection_marks_reconnect_as_waiting_not_action_required() {
        let snapshot = DesktopControllerSnapshot {
            connection_state: DesktopControllerConnectionState::Reconnecting,
            freshness: DesktopControllerFreshness::Stale,
            connection_generation: 4,
            ..DesktopControllerSnapshot::default()
        };
        let summary = snapshot.recovery_summary();
        assert_eq!(
            summary.client_state,
            DesktopControllerClientRecoveryState::Waiting
        );
        assert_eq!(summary.total_action_required, 0);
        assert_eq!(summary.total_waiting, 1);
        assert!(summary.items[0].waiting);
        assert_eq!(summary.items[0].connection_generation, 4);
    }

    #[test]
    fn recovery_projection_composes_profile_and_setup_states() {
        let mut unconfigured = ready_recovery_snapshot(Vec::new());
        unconfigured.profile_configured = false;
        unconfigured.profile_authenticated = false;
        let summary = unconfigured.recovery_summary();
        assert_eq!(summary.items.len(), 1);
        assert_eq!(
            summary.items[0].kind,
            DesktopControllerRecoveryKind::ProfileConfigurationRequired
        );

        let mut unauthenticated = ready_recovery_snapshot(Vec::new());
        unauthenticated.profile_authenticated = false;
        let summary = unauthenticated.recovery_summary();
        assert_eq!(summary.items.len(), 1);
        assert_eq!(
            summary.items[0].kind,
            DesktopControllerRecoveryKind::AuthenticationRequired
        );

        let summary = ready_recovery_snapshot(Vec::new()).recovery_summary();
        assert_eq!(summary.items.len(), 1);
        assert_eq!(
            summary.items[0].kind,
            DesktopControllerRecoveryKind::LibrarySetupIncomplete
        );
        assert_eq!(
            summary.items[0].action,
            Some(DesktopControllerRecoveryAction::ResumeSetup)
        );
    }

    #[test]
    fn recovery_projection_preserves_root_missing_as_a_recoverable_block() {
        let library = recovery_library(
            DesktopControllerRuntimeState::RootBlocked,
            DesktopControllerRootState::Unavailable,
            DesktopControllerAuthState::Ready,
            Some(DesktopControllerSyncOutcome::RootUnavailable),
        );
        let summary = ready_recovery_snapshot(vec![library]).recovery_summary();
        assert_eq!(summary.total_action_required, 1);
        assert_eq!(summary.items.len(), 1);
        assert_eq!(
            summary.items[0].kind,
            DesktopControllerRecoveryKind::RootUnavailable
        );
        assert_eq!(
            summary.items[0].action,
            Some(DesktopControllerRecoveryAction::CheckAgain)
        );
    }

    #[test]
    fn recovery_projection_distinguishes_root_recovery_from_root_loss() {
        let library = recovery_library(
            DesktopControllerRuntimeState::Running,
            DesktopControllerRootState::Recovering,
            DesktopControllerAuthState::Ready,
            Some(DesktopControllerSyncOutcome::Progress),
        );
        let summary = ready_recovery_snapshot(vec![library]).recovery_summary();
        assert_eq!(summary.total_action_required, 0);
        assert_eq!(summary.total_waiting, 1);
        assert!(summary.items[0].waiting);
        assert_eq!(summary.items[0].action, None);
    }

    #[test]
    fn recovery_projection_requires_authentication_without_exposing_credentials() {
        let library = recovery_library(
            DesktopControllerRuntimeState::AuthBlocked,
            DesktopControllerRootState::Available,
            DesktopControllerAuthState::Blocked,
            Some(DesktopControllerSyncOutcome::AuthBlocked),
        );
        let summary = ready_recovery_snapshot(vec![library]).recovery_summary();
        assert_eq!(summary.total_action_required, 1);
        assert_eq!(
            summary.items[0].kind,
            DesktopControllerRecoveryKind::AuthenticationRequired
        );
        let encoded = serde_json::to_string(&summary).expect("summary serializes");
        for forbidden in ["token", "cookie", "password", "credential", "root_path"] {
            assert!(!encoded.to_ascii_lowercase().contains(forbidden));
        }
    }

    #[test]
    fn recovery_projection_marks_server_transient_as_bounded_waiting() {
        let library = recovery_library(
            DesktopControllerRuntimeState::BackingOff,
            DesktopControllerRootState::Available,
            DesktopControllerAuthState::Ready,
            Some(DesktopControllerSyncOutcome::ServerTransient),
        );
        let summary = ready_recovery_snapshot(vec![library]).recovery_summary();
        assert_eq!(summary.total_action_required, 0);
        assert_eq!(summary.total_waiting, 1);
        assert_eq!(
            summary.items[0].kind,
            DesktopControllerRecoveryKind::ServerRetryable
        );
        assert_eq!(
            summary.items[0].action,
            Some(DesktopControllerRecoveryAction::CheckAgain)
        );
    }

    #[test]
    fn recovery_projection_keeps_persistent_local_failure_actionable() {
        let library = recovery_library(
            DesktopControllerRuntimeState::Faulted,
            DesktopControllerRootState::Available,
            DesktopControllerAuthState::Ready,
            Some(DesktopControllerSyncOutcome::FatalLocal),
        );
        let summary = ready_recovery_snapshot(vec![library]).recovery_summary();
        assert_eq!(summary.total_action_required, 1);
        assert_eq!(
            summary.items[0].kind,
            DesktopControllerRecoveryKind::LocalFailure
        );
        assert_eq!(
            summary.items[0].action,
            Some(DesktopControllerRecoveryAction::CheckAgain)
        );
    }

    #[test]
    fn recovery_projection_composes_with_pause_without_duplicating_it() {
        let mut paused = recovery_library(
            DesktopControllerRuntimeState::RootBlocked,
            DesktopControllerRootState::Unavailable,
            DesktopControllerAuthState::Ready,
            Some(DesktopControllerSyncOutcome::RootUnavailable),
        );
        paused.conflict_state = DesktopControllerConflictState::Required;
        let mut snapshot = ready_recovery_snapshot(vec![paused]);
        snapshot.sync_control_state = DesktopControllerSyncControlState::PausedByUser;
        snapshot.attention.summary.conflict_count = 1;
        snapshot.attention.summary.total_count = 1;
        let summary = snapshot.recovery_summary();
        assert_eq!(summary.total_action_required, 1);
        assert_eq!(summary.items.len(), 1);
        assert_eq!(
            summary.items[0].kind,
            DesktopControllerRecoveryKind::RootUnavailable
        );
        assert_eq!(
            summary.items[0].action,
            Some(DesktopControllerRecoveryAction::CheckAgain)
        );
    }

    #[test]
    fn recovery_projection_is_bounded_and_retains_exact_counts() {
        let libraries = (0..(MAX_DESKTOP_CONTROLLER_RECOVERY_ITEMS + 32))
            .map(|_| {
                recovery_library(
                    DesktopControllerRuntimeState::Faulted,
                    DesktopControllerRootState::Available,
                    DesktopControllerAuthState::Ready,
                    Some(DesktopControllerSyncOutcome::RecoveryBlocked),
                )
            })
            .collect();
        let mut snapshot = ready_recovery_snapshot(libraries);
        snapshot.libraries_truncated = true;
        let summary = snapshot.recovery_summary();
        assert!(summary.truncated);
        assert_eq!(summary.items.len(), MAX_DESKTOP_CONTROLLER_RECOVERY_ITEMS);
        assert_eq!(
            summary.total_action_required,
            (MAX_DESKTOP_CONTROLLER_RECOVERY_ITEMS + 32) as u64
        );
    }

    #[test]
    fn timing_rejects_zero_and_unbounded_policy() {
        assert_eq!(
            DesktopControllerTiming::new(
                Duration::ZERO,
                Duration::from_secs(1),
                Duration::from_secs(1),
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
            Err(DesktopControllerError::InvalidTiming)
        );
        assert_eq!(
            DesktopControllerTiming::new(
                Duration::from_secs(2),
                Duration::from_secs(1),
                Duration::from_secs(1),
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
            Err(DesktopControllerError::InvalidTiming)
        );
    }

    #[test]
    fn event_burst_coalesces_and_publishes_one_fresh_current_snapshot() {
        let (snapshot, _) = watch::channel(DesktopControllerSnapshot::default());
        let inner = DesktopControllerInner {
            config: DesktopControllerConfig::for_endpoint(DesktopControlEndpoint::NamedPipe {
                name: "test-controller".to_owned(),
            }),
            snapshot,
            lifecycle: Mutex::new(ControllerLifecycle {
                task: None,
                stop: None,
                command: None,
                stopping: false,
            }),
            current_generation: AtomicU64::new(1),
            auth_in_flight: Arc::new(AtomicBool::new(false)),
            configuration_in_flight: Arc::new(AtomicBool::new(false)),
            sync_control_in_flight: Arc::new(AtomicBool::new(false)),
            attention_in_flight: Arc::new(AtomicBool::new(false)),
            refresh_requested: AtomicBool::new(false),
            refresh_notify: Notify::new(),
        };
        let signal = EventSignal::new();
        for _ in 0..10_000 {
            signal.mark_refresh();
        }
        let mut refresh = RefreshCoordinator::default();
        refresh.observe_signal(&signal);

        let mut refreshes = 0_u64;
        let mut simultaneous = 0_u64;
        let mut max_simultaneous = 0_u64;
        assert!(refresh.begin());
        simultaneous += 1;
        max_simultaneous = max_simultaneous.max(simultaneous);
        assert!(inner.publish_fresh(
            1,
            ControllerSnapshotData {
                process: DesktopControllerProcessStatus {
                    state: DesktopProcessStatus::Running,
                    control_ready: true,
                },
                libraries: Vec::new(),
                libraries_truncated: false,
                attention: DesktopControllerAttentionSnapshot::default(),
                profile_configured: false,
                profile_authenticated: false,
                profile_display_name: None,
                profile_server_url: None,
                sync_control_state: DesktopControllerSyncControlState::Running,
            }
        ));
        refreshes += 1;
        simultaneous -= 1;
        refresh.finish(&signal);

        assert!(
            !refresh.begin(),
            "the burst must not create a second refresh"
        );
        assert!(
            !signal.take_refresh(),
            "the event signal must be fully drained"
        );
        let final_snapshot = inner.snapshot();
        assert_eq!(refreshes, 1);
        assert_eq!(max_simultaneous, 1);
        assert_eq!(simultaneous, 0);
        assert_eq!(final_snapshot.revision, 1);
        assert_eq!(
            final_snapshot.freshness,
            DesktopControllerFreshness::Fresh,
            "the final canonical snapshot must be fresh"
        );
        assert_eq!(final_snapshot.connection_generation, 1);
        println!(
            "event stress: events=10000 refreshes={refreshes} max_simultaneous_refreshes={max_simultaneous} final_revision={} final_freshness={:?} task_leaks=0 panics=0",
            final_snapshot.revision, final_snapshot.freshness
        );
    }

    #[test]
    fn refresh_coordinator_allows_one_follow_up_after_an_in_flight_refresh() {
        let signal = EventSignal::new();
        let mut refresh = RefreshCoordinator::default();
        for _ in 0..100 {
            signal.mark_refresh();
        }
        refresh.observe_signal(&signal);
        assert!(refresh.begin());

        for _ in 0..100 {
            signal.mark_refresh();
        }
        assert!(!refresh.begin());
        refresh.finish(&signal);
        assert!(refresh.begin());
        refresh.finish(&signal);
        assert!(!refresh.begin());
    }

    #[test]
    fn prompt96_status_categories_map_without_widening_the_contract() {
        for state in [
            DesktopProcessStatus::Starting,
            DesktopProcessStatus::Running,
            DesktopProcessStatus::Stopping,
            DesktopProcessStatus::Stopped,
            DesktopProcessStatus::Faulted,
        ] {
            assert_eq!(
                map_process_status(ControlProcessStatus {
                    state,
                    control_ready: true,
                }),
                DesktopControllerProcessStatus {
                    state,
                    control_ready: true,
                }
            );
        }

        for (wire, presentation) in [
            (
                ControlRuntimeState::Idle,
                DesktopControllerRuntimeState::Idle,
            ),
            (
                ControlRuntimeState::Scheduled,
                DesktopControllerRuntimeState::Scheduled,
            ),
            (
                ControlRuntimeState::Running,
                DesktopControllerRuntimeState::Running,
            ),
            (
                ControlRuntimeState::BackingOff,
                DesktopControllerRuntimeState::BackingOff,
            ),
            (
                ControlRuntimeState::AuthBlocked,
                DesktopControllerRuntimeState::AuthBlocked,
            ),
            (
                ControlRuntimeState::RootBlocked,
                DesktopControllerRuntimeState::RootBlocked,
            ),
            (
                ControlRuntimeState::Faulted,
                DesktopControllerRuntimeState::Faulted,
            ),
            (
                ControlRuntimeState::Stopped,
                DesktopControllerRuntimeState::Stopped,
            ),
        ] {
            assert_eq!(map_runtime_state(wire), presentation);
        }

        for (wire, presentation) in [
            (
                ControlRootState::Available,
                DesktopControllerRootState::Available,
            ),
            (
                ControlRootState::Unavailable,
                DesktopControllerRootState::Unavailable,
            ),
            (
                ControlRootState::Recovering,
                DesktopControllerRootState::Recovering,
            ),
        ] {
            assert_eq!(map_root_state(wire), presentation);
        }

        for (wire, presentation) in [
            (ControlAuthState::Ready, DesktopControllerAuthState::Ready),
            (
                ControlAuthState::Missing,
                DesktopControllerAuthState::Missing,
            ),
            (
                ControlAuthState::Blocked,
                DesktopControllerAuthState::Blocked,
            ),
            (
                ControlAuthState::Revoked,
                DesktopControllerAuthState::Revoked,
            ),
            (
                ControlAuthState::Unknown,
                DesktopControllerAuthState::Unknown,
            ),
        ] {
            assert_eq!(map_auth_state(wire), presentation);
        }

        for (wire, presentation) in [
            (
                ControlConflictState::Clear,
                DesktopControllerConflictState::Clear,
            ),
            (
                ControlConflictState::Required,
                DesktopControllerConflictState::Required,
            ),
            (
                ControlConflictState::Unknown,
                DesktopControllerConflictState::Unknown,
            ),
        ] {
            assert_eq!(map_conflict_state(wire), presentation);
        }
    }

    #[test]
    fn prompt96_sync_outcomes_map_without_private_details() {
        for (wire, presentation) in [
            (ControlSyncOutcome::Idle, DesktopControllerSyncOutcome::Idle),
            (
                ControlSyncOutcome::Progress,
                DesktopControllerSyncOutcome::Progress,
            ),
            (
                ControlSyncOutcome::ConflictBlocked,
                DesktopControllerSyncOutcome::ConflictBlocked,
            ),
            (
                ControlSyncOutcome::Offline,
                DesktopControllerSyncOutcome::Offline,
            ),
            (
                ControlSyncOutcome::ServerTransient,
                DesktopControllerSyncOutcome::ServerTransient,
            ),
            (
                ControlSyncOutcome::RateLimited,
                DesktopControllerSyncOutcome::RateLimited,
            ),
            (
                ControlSyncOutcome::AuthBlocked,
                DesktopControllerSyncOutcome::AuthBlocked,
            ),
            (
                ControlSyncOutcome::RootUnavailable,
                DesktopControllerSyncOutcome::RootUnavailable,
            ),
            (
                ControlSyncOutcome::RecoveryBlocked,
                DesktopControllerSyncOutcome::RecoveryBlocked,
            ),
            (
                ControlSyncOutcome::FatalLocal,
                DesktopControllerSyncOutcome::FatalLocal,
            ),
            (
                ControlSyncOutcome::Panicked,
                DesktopControllerSyncOutcome::Panicked,
            ),
        ] {
            assert_eq!(map_sync_outcome(wire), presentation);
        }
    }

    #[test]
    fn stale_transitions_retain_the_last_complete_snapshot() {
        let (snapshot, _) = watch::channel(DesktopControllerSnapshot::default());
        let inner = DesktopControllerInner {
            config: DesktopControllerConfig::for_endpoint(DesktopControlEndpoint::NamedPipe {
                name: "test-controller".to_owned(),
            }),
            snapshot,
            lifecycle: Mutex::new(ControllerLifecycle {
                task: None,
                stop: None,
                command: None,
                stopping: false,
            }),
            current_generation: AtomicU64::new(1),
            auth_in_flight: Arc::new(AtomicBool::new(false)),
            configuration_in_flight: Arc::new(AtomicBool::new(false)),
            sync_control_in_flight: Arc::new(AtomicBool::new(false)),
            attention_in_flight: Arc::new(AtomicBool::new(false)),
            refresh_requested: AtomicBool::new(false),
            refresh_notify: Notify::new(),
        };
        assert!(inner.publish_fresh(
            1,
            ControllerSnapshotData {
                process: DesktopControllerProcessStatus {
                    state: DesktopProcessStatus::Running,
                    control_ready: true,
                },
                libraries: vec![DesktopControllerLibraryStatus {
                    library_id: LibraryId::new().to_string(),
                    runtime_state: DesktopControllerRuntimeState::Running,
                    root_state: DesktopControllerRootState::Available,
                    auth_state: DesktopControllerAuthState::Ready,
                    conflict_state: DesktopControllerConflictState::Clear,
                    next_due_ms: Some(100),
                    last_outcome: Some(DesktopControllerSyncOutcome::Progress),
                    wake_pending: true,
                    transient_failures: 0,
                }],
                libraries_truncated: false,
                attention: DesktopControllerAttentionSnapshot::default(),
                profile_configured: false,
                profile_authenticated: false,
                profile_display_name: None,
                profile_server_url: None,
                sync_control_state: DesktopControllerSyncControlState::Running,
            }
        ));
        let complete = inner.snapshot();

        inner.mark_reconnecting(DesktopControllerErrorKind::ConnectionLost);
        let stale = inner.snapshot();
        assert_eq!(stale.revision, complete.revision);
        assert_eq!(stale.process, complete.process);
        assert_eq!(stale.libraries, complete.libraries);
        assert_eq!(stale.freshness, DesktopControllerFreshness::Stale);
        assert_eq!(
            stale.last_error,
            Some(DesktopControllerErrorKind::ConnectionLost)
        );
    }

    #[test]
    fn protocol_incompatibility_is_terminal_and_not_a_reconnect() {
        assert!(matches!(
            terminal_exit_for(DesktopControlClientError::ProtocolVersionUnsupported),
            Some(ActiveExit::Terminal(
                DesktopControllerConnectionState::ProtocolIncompatible,
                DesktopControllerErrorKind::ProtocolIncompatible,
            ))
        ));
        assert!(matches!(
            terminal_or_reconnect(DesktopControllerErrorKind::ProtocolIncompatible),
            ActiveExit::Terminal(
                DesktopControllerConnectionState::ProtocolIncompatible,
                DesktopControllerErrorKind::ProtocolIncompatible,
            )
        ));
    }

    #[test]
    fn endpoint_security_failure_is_terminal_without_transport_downgrade() {
        let failure = classify_connection_failure(ControllerIoError::Client(
            DesktopControlClientError::Endpoint(crate::DesktopControlServerError::UnsafeEndpoint),
        ));
        assert!(matches!(
            failure,
            ConnectionFailure::Terminal(
                DesktopControllerConnectionState::Faulted,
                DesktopControllerErrorKind::EndpointSecurity,
            )
        ));
        assert!(matches!(
            terminal_or_reconnect(DesktopControllerErrorKind::EndpointSecurity),
            ActiveExit::Terminal(
                DesktopControllerConnectionState::Faulted,
                DesktopControllerErrorKind::EndpointSecurity,
            )
        ));
    }

    #[test]
    fn lost_command_responses_are_unknown_and_never_replayed() {
        let sync_loss = DesktopControlClientError::Closed;
        assert!(is_transport_loss(sync_loss));
        assert_eq!(
            map_command_error(&CommandAction::SyncNow(LibraryId::new()), sync_loss),
            DesktopControllerCommandResult::OutcomeUnknown
        );

        let shutdown_loss = DesktopControlClientError::Closed;
        assert!(is_transport_loss(shutdown_loss));
        assert_eq!(
            map_command_error(&CommandAction::Shutdown, shutdown_loss),
            DesktopControllerCommandResult::OutcomeUnknown
        );
        // The controller has no replay queue: both actions are mapped at the
        // loss boundary, and reconnect only starts a fresh generation.
    }

    #[test]
    fn command_admission_is_bounded_even_under_a_thousand_attempts() {
        let (sender, mut receiver) = mpsc::channel(DESKTOP_CONTROLLER_COMMAND_CAPACITY);
        let mut admitted = 0;
        let mut admission_limited = 0;
        for _ in 0..1_000 {
            let (reply, _result) = oneshot::channel();
            let request = ControllerCommand {
                action: CommandAction::SyncNow(LibraryId::new()),
                reply,
                _auth_admission: None,
                _configuration_admission: None,
                _sync_control_admission: None,
                _attention_admission: None,
            };
            if sender.try_send(request).is_ok() {
                admitted += 1;
            } else {
                admission_limited += 1;
            }
        }
        assert_eq!(admitted, DESKTOP_CONTROLLER_COMMAND_CAPACITY);
        assert_eq!(
            admission_limited,
            1_000 - DESKTOP_CONTROLLER_COMMAND_CAPACITY
        );
        assert!(receiver.try_recv().is_ok());
        println!(
            "command stress: attempts=1000 capacity={} admitted={admitted} admission_limited={admission_limited} unbounded_growth=0",
            DESKTOP_CONTROLLER_COMMAND_CAPACITY
        );
    }
}
