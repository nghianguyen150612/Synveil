//! Pure presentation mapping for the redacted Prompt 97 controller model.
//!
//! The output is deliberately made from stable codes, generic labels, and
//! bounded values. It has no field for a root path, credential, cookie,
//! authorization header, file content, or raw transport diagnostic. The
//! canonical non-secret profile URL and label are exposed for onboarding.

use std::collections::BTreeMap;

use synveil_client::{
    DesktopControllerAttentionItem, DesktopControllerAttentionItemKind, DesktopControllerAuthState,
    DesktopControllerCommandResult, DesktopControllerConflictAction,
    DesktopControllerConflictState, DesktopControllerConnectionState, DesktopControllerErrorKind,
    DesktopControllerFreshness, DesktopControllerLibraryStatus, DesktopControllerRecoveryAction,
    DesktopControllerRecoveryItem, DesktopControllerRecoveryKind, DesktopControllerRootState,
    DesktopControllerRuntimeState, DesktopControllerSnapshot, DesktopControllerSyncControlState,
    DesktopControllerSyncOutcome, LibraryId,
};
use synveil_server_bootstrap::HostSetupState;

/// The single, ephemeral first-run destination rendered by QML.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WelcomeDestination {
    Initializing,
    Welcome,
    HostResume,
    HostReady,
    ExistingClient,
    HostNeedsAttention,
}

/// Small, Rust-owned product state used by the installation-to-sync surface.
/// QML renders these values and never derives meaning from runtime strings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirstRunProgressStageState {
    Pending,
    Active,
    Waiting,
    Complete,
    ActionRequired,
}

impl FirstRunProgressStageState {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Waiting => "waiting",
            Self::Complete => "complete",
            Self::ActionRequired => "action_required",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pending => "Not started",
            Self::Active => "Active",
            Self::Waiting => "Waiting",
            Self::Complete => "Complete",
            Self::ActionRequired => "Action required",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirstRunProgressStage {
    pub id: &'static str,
    pub title: &'static str,
    pub state: FirstRunProgressStageState,
    pub detail: &'static str,
    pub action: Option<&'static str>,
    pub action_label: Option<&'static str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FirstRunProgress {
    pub visible: bool,
    pub status: &'static str,
    pub stages: Vec<FirstRunProgressStage>,
}

impl FirstRunProgress {
    fn hidden(stages: Vec<FirstRunProgressStage>) -> Self {
        Self {
            visible: false,
            status: "Your files are up to date.",
            stages,
        }
    }
}

fn progress_stage(
    id: &'static str,
    title: &'static str,
    state: FirstRunProgressStageState,
    detail: &'static str,
    action: Option<&'static str>,
    action_label: Option<&'static str>,
) -> FirstRunProgressStage {
    FirstRunProgressStage {
        id,
        title,
        state,
        detail,
        action,
        action_label,
    }
}

impl WelcomeDestination {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Initializing => "initializing",
            Self::Welcome => "welcome",
            Self::HostResume => "host_resume",
            Self::HostReady => "host_ready",
            Self::ExistingClient => "existing_client",
            Self::HostNeedsAttention => "host_needs_attention",
        }
    }
}

/// Compose canonical client configuration and P036 inspection without adding
/// a durable "welcome seen" owner. Authentication and library count do not
/// affect this first-level decision once a profile is configured.
#[must_use]
pub const fn welcome_destination(
    client_initialized: bool,
    profile_configured: bool,
    host_state: HostSetupState,
) -> WelcomeDestination {
    if !client_initialized {
        return WelcomeDestination::Initializing;
    }
    if profile_configured {
        return WelcomeDestination::ExistingClient;
    }
    match host_state {
        HostSetupState::NotStarted | HostSetupState::Blocked => WelcomeDestination::Welcome,
        HostSetupState::Ready => WelcomeDestination::HostReady,
        HostSetupState::NeedsRepair => WelcomeDestination::HostNeedsAttention,
        HostSetupState::PreflightPassed
        | HostSetupState::DependenciesReady
        | HostSetupState::ConfigurationReady
        | HostSetupState::StorageReady
        | HostSetupState::ServicesReady
        | HostSetupState::AdminBootstrapRequired => WelcomeDestination::HostResume,
    }
}

/// First-library routing is derived only from one fresh, authenticated client
/// snapshot. It is never a persisted "first run completed" preference.
#[must_use]
pub const fn library_first_run_required(
    snapshot_fresh: bool,
    profile_configured: bool,
    profile_authenticated: bool,
    has_configured_library: bool,
) -> bool {
    snapshot_fresh && profile_configured && profile_authenticated && !has_configured_library
}

/// A setup command's success/unknown result is not enough to leave the wizard.
/// Only a newer fresh snapshot completes the confirmation interval.
#[must_use]
pub const fn library_setup_confirmation_finished(
    awaiting_confirmation: bool,
    snapshot_fresh: bool,
    snapshot_revision: u64,
    submitted_revision: u64,
) -> bool {
    awaiting_confirmation && snapshot_fresh && snapshot_revision > submitted_revision
}

/// Presentation guard independent of the controller's own protocol bound.
pub const MAX_PRESENTED_LIBRARY_ROWS: usize = 2_048;

/// Safe, Rust-owned authentication presentation. It deliberately carries no
/// secret or transport detail; QML only renders the code, message and action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthenticationPresentation {
    pub code: &'static str,
    pub message: &'static str,
    pub action: Option<&'static str>,
}

/// Bounded library-setup copy and its one safe next action. QML renders this
/// model and never interprets raw client or transport failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LibrarySetupPresentation {
    pub code: &'static str,
    pub message: &'static str,
    pub action: &'static str,
}

/// Convert every canonical authentication result into bounded product copy.
#[must_use]
pub const fn authentication_presentation(
    result: DesktopControllerCommandResult,
) -> AuthenticationPresentation {
    match result {
        DesktopControllerCommandResult::Authenticated => AuthenticationPresentation {
            code: "authenticated",
            message: "This device is signed in.",
            action: Some("sign_out"),
        },
        DesktopControllerCommandResult::SignedOut => AuthenticationPresentation {
            code: "sign_in_required",
            message: "This device is signed out.",
            action: Some("sign_in"),
        },
        DesktopControllerCommandResult::InvalidCredentials => AuthenticationPresentation {
            code: "invalid_code",
            message: "That device code wasn't accepted. Check the code and try again.",
            action: Some("sign_in"),
        },
        DesktopControllerCommandResult::NetworkUnavailable => AuthenticationPresentation {
            code: "network_unavailable",
            message: "You're offline. Check your network and try again.",
            action: Some("sign_in"),
        },
        DesktopControllerCommandResult::ServerUnavailable => AuthenticationPresentation {
            code: "server_unavailable",
            message: "Synveil couldn't reach the server right now.",
            action: Some("sign_in"),
        },
        DesktopControllerCommandResult::RateLimited => AuthenticationPresentation {
            code: "rate_limited",
            message: "Too many attempts. Wait a moment and try again.",
            action: Some("sign_in"),
        },
        DesktopControllerCommandResult::SecureStoreUnavailable => AuthenticationPresentation {
            code: "secure_storage_unavailable",
            message: "Secure credential storage isn't available right now.",
            action: Some("sign_in"),
        },
        DesktopControllerCommandResult::Busy => AuthenticationPresentation {
            code: "busy",
            message: "Sign-in is already in progress.",
            action: None,
        },
        DesktopControllerCommandResult::OutcomeUnknown => AuthenticationPresentation {
            code: "reconciling",
            message: "Synveil is checking whether this device was signed in.",
            action: None,
        },
        DesktopControllerCommandResult::ProtocolError => AuthenticationPresentation {
            code: "protocol_problem",
            message: "This Synveil installation couldn't complete sign-in.",
            action: Some("sign_in"),
        },
        _ => AuthenticationPresentation {
            code: "protocol_problem",
            message: "Synveil couldn't complete this authentication action.",
            action: None,
        },
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiLibrary {
    pub library_id: String,
    pub label: String,
    pub runtime_code: &'static str,
    pub runtime_label: &'static str,
    pub root_code: &'static str,
    pub root_label: &'static str,
    pub auth_code: &'static str,
    pub auth_label: &'static str,
    pub conflict_code: &'static str,
    pub conflict_label: &'static str,
    pub outcome_code: Option<&'static str>,
    pub outcome_label: &'static str,
    pub next_due_ms: Option<u64>,
    pub wake_pending: bool,
    pub transient_failures: u32,
    pub first_sync_completed: bool,
    pub needs_attention: bool,
    pub can_sync: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiAttentionItem {
    pub attention_id: String,
    pub library_id: String,
    pub intent_id: String,
    pub library_label: String,
    pub category_code: String,
    pub category_label: String,
    pub relative_path: Option<String>,
    pub previous_relative_path: Option<String>,
    pub path_label: String,
    pub item_kind_label: String,
    pub local_length: Option<u64>,
    pub remote_length: Option<u64>,
    pub local_base_revision: Option<u64>,
    pub remote_observed_revision: Option<u64>,
    pub remote_observed_state: Option<String>,
    pub detected_at_ms: u64,
    pub can_accept_remote: bool,
    pub can_retry_local: bool,
}

/// Presentation only: canonical controller/process owners retain all truth.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryState {
    Waiting,
    ActionRequired,
    Recovered,
}
impl RecoveryState {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::ActionRequired => "action_required",
            Self::Recovered => "recovered",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryCapability {
    Supported,
    GuidanceOnly,
    Unavailable,
}
impl RecoveryCapability {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::GuidanceOnly => "guidance_only",
            Self::Unavailable => "unavailable",
        }
    }
}

pub fn repair_presentation() -> (RecoveryCapability, &'static str) {
    if cfg!(windows) {
        (RecoveryCapability::GuidanceOnly,
         "Use the official Synveil installer for the same installed version and choose Repair Synveil. Your files and settings are preserved. The desktop cannot verify or launch a suitable installer here.")
    } else if cfg!(target_os = "linux") {
        (RecoveryCapability::GuidanceOnly,
         "For a system package, repair Synveil using your system’s package manager. For AppImage, use its existing integration repair. Use the same installed version. Your files and settings are preserved; this app has not performed a repair.")
    } else {
        (
            RecoveryCapability::Unavailable,
            "Installation repair is not available on this device.",
        )
    }
}

/// An action rendered from older status cannot mutate a newer profile/root.
pub fn recovery_mutation_allowed(
    presented: &UiSnapshot,
    current: &DesktopControllerSnapshot,
) -> bool {
    current.freshness == DesktopControllerFreshness::Fresh
        && current.connection_state == DesktopControllerConnectionState::Connected
        && current.process.as_ref().is_some_and(|process| {
            process.state == synveil_client::DesktopProcessStatus::Running && process.control_ready
        })
        && current.connection_generation == presented.connection_generation
        && current.revision == presented.revision
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiRecoveryItem {
    pub state: RecoveryState,
    pub capability: RecoveryCapability,
    pub recovery_id: String,
    pub library_id: Option<String>,
    pub library_label: String,
    pub kind_code: &'static str,
    pub kind_label: &'static str,
    pub detail: &'static str,
    pub action_code: Option<&'static str>,
    pub action_label: &'static str,
    pub action_required: bool,
    pub waiting: bool,
    pub connection_generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiSnapshot {
    pub connection_code: &'static str,
    pub connection_label: &'static str,
    pub connection_detail: &'static str,
    pub freshness_code: &'static str,
    pub freshness_label: &'static str,
    pub process_code: &'static str,
    pub process_label: &'static str,
    pub process_control_ready: bool,
    pub sync_control_code: &'static str,
    pub sync_control_label: &'static str,
    pub libraries: Vec<UiLibrary>,
    pub libraries_truncated: bool,
    pub empty_library_message: &'static str,
    pub attention_items: Vec<UiAttentionItem>,
    pub attention_items_truncated: bool,
    pub revision: u64,
    pub connection_generation: u64,
    pub last_error_code: Option<&'static str>,
    pub last_error_label: &'static str,
    pub attention_count: usize,
    pub conflict_attention_count: usize,
    pub other_attention_count: usize,
    pub root_unavailable_count: usize,
    pub can_sync_any: bool,
    pub profile_configured: bool,
    pub profile_authenticated: bool,
    pub profile_display_name: Option<String>,
    pub profile_server_url: Option<String>,
    pub can_reconnect_server: bool,
    pub recovery_items: Vec<UiRecoveryItem>,
    pub recovery_items_truncated: bool,
    pub recovery_action_required: usize,
    pub recovery_waiting_count: usize,
    pub client_recovery_code: &'static str,
    pub client_recovery_label: &'static str,
    pub first_run_progress: FirstRunProgress,
}

impl Default for UiSnapshot {
    fn default() -> Self {
        map_snapshot(&DesktopControllerSnapshot::default())
    }
}

/// Convert one controller snapshot into the complete safe UI snapshot.
#[must_use]
pub fn map_snapshot(snapshot: &DesktopControllerSnapshot) -> UiSnapshot {
    let process_control_ready = snapshot
        .process
        .is_some_and(|process| process.control_ready);
    let process_code = snapshot
        .process
        .map_or("unknown", |process| process.state.as_str());
    let process_label =
        snapshot.process.map_or(
            "Background service status unknown",
            |process| match process.state {
                synveil_client::DesktopProcessStatus::Starting => "Background service starting",
                synveil_client::DesktopProcessStatus::Running if process.control_ready => {
                    "Background service running"
                }
                synveil_client::DesktopProcessStatus::Running => "Background service not ready",
                synveil_client::DesktopProcessStatus::Stopping => "Background service stopping",
                synveil_client::DesktopProcessStatus::Stopped => "Background service stopped",
                synveil_client::DesktopProcessStatus::Faulted => "Background service unavailable",
            },
        );

    let mut libraries_by_id = BTreeMap::new();
    for status in &snapshot.libraries {
        let Ok(_) = status.library_id.parse::<LibraryId>() else {
            continue;
        };
        libraries_by_id
            .entry(status.library_id.clone())
            .or_insert_with(|| map_library(status, snapshot, process_control_ready));
    }
    let mut libraries = libraries_by_id.into_values().collect::<Vec<_>>();
    let presented_truncated = libraries.len() > MAX_PRESENTED_LIBRARY_ROWS;
    if presented_truncated {
        libraries.truncate(MAX_PRESENTED_LIBRARY_ROWS);
    }

    let attention_items = snapshot
        .attention
        .items
        .iter()
        .map(map_attention_item)
        .collect::<Vec<_>>();
    let durable_conflict_count = bounded_usize(snapshot.attention.summary.conflict_count);
    let durable_other_count = bounded_usize(snapshot.attention.summary.other_count);
    // Prompt 106 attention remains the durable conflict/issue projection. The
    // derived Prompt 107 recovery projection below owns root/auth/runtime
    // blockers, so the two surfaces can coexist without double-counting.
    let attention_count = durable_conflict_count.saturating_add(durable_other_count);
    let other_attention_count = durable_other_count;
    let root_unavailable_count = libraries
        .iter()
        .filter(|library| library.root_code != "available")
        .count();
    let can_sync_any = libraries.iter().any(|library| library.can_sync);
    let recovery = snapshot.recovery_summary();
    // Setup, sign-in, and first-library actions already have dedicated forms.
    // Keep those same actions out of the recovery card so the user sees one
    // clear next step. Per-library sign-in recovery remains listed so its
    // action can select the affected library.
    let dedicated_recovery_actions = recovery
        .items
        .iter()
        .filter(|item| recovery_action_has_dedicated_surface(snapshot, item))
        .collect::<Vec<_>>();
    let hidden_recovery_action_count = dedicated_recovery_actions
        .iter()
        .filter(|item| item.action_required)
        .count();
    let recovery_items = recovery
        .items
        .iter()
        .filter(|item| !recovery_action_has_dedicated_surface(snapshot, item))
        .map(map_recovery_item)
        .collect::<Vec<_>>();
    let (client_recovery_code, client_recovery_label) =
        client_recovery_presentation(recovery.client_state);

    let (connection_code, connection_label, connection_detail) = connection_presentation(
        snapshot.connection_state,
        snapshot.freshness,
        snapshot.last_error,
    );
    let (freshness_code, freshness_label) = freshness_presentation(snapshot.freshness);
    let (sync_control_code, sync_control_label) =
        sync_control_presentation(snapshot.sync_control_state);
    let first_run_progress = first_run_progress(snapshot);

    UiSnapshot {
        connection_code,
        connection_label,
        connection_detail,
        freshness_code,
        freshness_label,
        process_code,
        process_label,
        process_control_ready,
        sync_control_code,
        sync_control_label,
        libraries,
        libraries_truncated: snapshot.libraries_truncated || presented_truncated,
        empty_library_message: empty_library_presentation(snapshot),
        attention_items,
        attention_items_truncated: snapshot.attention.truncated,
        revision: snapshot.revision,
        connection_generation: snapshot.connection_generation,
        last_error_code: snapshot.last_error.map(DesktopControllerErrorKind::code),
        last_error_label: snapshot
            .last_error
            .map_or("No connection error", |error| error_label(&error)),
        attention_count,
        conflict_attention_count: durable_conflict_count,
        other_attention_count,
        root_unavailable_count,
        can_sync_any,
        profile_configured: snapshot.profile_configured,
        profile_authenticated: snapshot.profile_authenticated,
        profile_display_name: snapshot.profile_display_name.clone(),
        profile_server_url: snapshot.profile_server_url.clone(),
        can_reconnect_server: recovery.items.iter().any(|item| {
            item.kind == DesktopControllerRecoveryKind::ProfileConfigurationRequired
                && item.action_required
        }) && snapshot.freshness == DesktopControllerFreshness::Fresh,
        recovery_items,
        recovery_items_truncated: recovery.truncated,
        recovery_action_required: bounded_usize(
            recovery
                .total_action_required
                .saturating_sub(u64::try_from(hidden_recovery_action_count).unwrap_or(u64::MAX)),
        ),
        recovery_waiting_count: bounded_usize(recovery.total_waiting),
        client_recovery_code,
        client_recovery_label,
        first_run_progress,
    }
}

/// Project the authoritative setup and first-sync owners into five compact
/// product stages. Installation itself is intentionally absent: the desktop
/// has no native installer telemetry to prove a package-manager milestone.
#[must_use]
pub fn first_run_progress(snapshot: &DesktopControllerSnapshot) -> FirstRunProgress {
    let app_ready = snapshot.process.is_some_and(|process| {
        matches!(process.state, synveil_client::DesktopProcessStatus::Running)
            && process.control_ready
    });
    let app_stage = progress_stage(
        "app_ready",
        "Synveil ready",
        if app_ready {
            FirstRunProgressStageState::Complete
        } else if matches!(
            snapshot.connection_state,
            DesktopControllerConnectionState::Connecting
                | DesktopControllerConnectionState::Reconnecting
        ) || snapshot.freshness == DesktopControllerFreshness::Unavailable
        {
            FirstRunProgressStageState::Active
        } else {
            FirstRunProgressStageState::Waiting
        },
        if app_ready {
            "Synveil is ready."
        } else {
            "Starting Synveil…"
        },
        None,
        None,
    );

    let server_ready = snapshot.profile_configured;
    let server_stage = progress_stage(
        "server_ready",
        "Server ready",
        if server_ready {
            FirstRunProgressStageState::Complete
        } else if app_ready
            && snapshot.connection_state == DesktopControllerConnectionState::Connected
            && snapshot.freshness == DesktopControllerFreshness::Fresh
        {
            FirstRunProgressStageState::ActionRequired
        } else {
            FirstRunProgressStageState::Pending
        },
        if server_ready {
            "Server setup completed."
        } else {
            "Connect to a Synveil server to continue."
        },
        (!server_ready).then_some("configure_connection"),
        (!server_ready).then_some("Open connection"),
    );

    let signed_in = snapshot.profile_authenticated;
    let signed_in_stage = progress_stage(
        "signed_in",
        "Signed in",
        if !server_ready {
            FirstRunProgressStageState::Pending
        } else if signed_in {
            FirstRunProgressStageState::Complete
        } else if snapshot.connection_state == DesktopControllerConnectionState::Connected
            && snapshot.freshness == DesktopControllerFreshness::Fresh
        {
            FirstRunProgressStageState::ActionRequired
        } else {
            FirstRunProgressStageState::Waiting
        },
        if signed_in {
            "This device is signed in."
        } else {
            "Sign in to continue."
        },
        (!signed_in && server_ready).then_some("sign_in"),
        (!signed_in && server_ready).then_some("Sign in to continue"),
    );

    let library_ready = !snapshot.libraries.is_empty();
    let library_stage = progress_stage(
        "library_ready",
        "Library ready",
        if !signed_in {
            FirstRunProgressStageState::Pending
        } else if library_ready {
            FirstRunProgressStageState::Complete
        } else if snapshot.connection_state == DesktopControllerConnectionState::Connected
            && snapshot.freshness == DesktopControllerFreshness::Fresh
        {
            FirstRunProgressStageState::ActionRequired
        } else {
            FirstRunProgressStageState::Waiting
        },
        if library_ready {
            "Your library is configured."
        } else {
            "Set up a library to start synchronizing."
        },
        (!library_ready && signed_in).then_some("resume_setup"),
        (!library_ready && signed_in).then_some("Set up a library"),
    );

    let first_sync_stage = first_sync_stage(snapshot, library_ready, signed_in);
    let stages = vec![
        app_stage,
        server_stage,
        signed_in_stage,
        library_stage,
        first_sync_stage,
    ];
    if !snapshot
        .libraries
        .iter()
        .any(|library| !library.first_sync_completed)
    {
        return FirstRunProgress::hidden(stages);
    }

    let status = stages
        .iter()
        .find(|stage| stage.id == "first_sync")
        .map_or("Checking sync status…", |stage| stage.detail);
    FirstRunProgress {
        visible: library_ready && !snapshot.libraries.is_empty(),
        status,
        stages,
    }
}

fn first_sync_stage(
    snapshot: &DesktopControllerSnapshot,
    library_ready: bool,
    signed_in: bool,
) -> FirstRunProgressStage {
    if !library_ready || !signed_in {
        return progress_stage(
            "first_sync",
            "First sync",
            FirstRunProgressStageState::Pending,
            "Waiting for your library.",
            None,
            None,
        );
    }

    let pending = snapshot
        .libraries
        .iter()
        .filter(|library| !library.first_sync_completed)
        .collect::<Vec<_>>();
    if pending.is_empty() && first_sync_completion_proven(snapshot) {
        return progress_stage(
            "first_sync",
            "Up to date",
            FirstRunProgressStageState::Complete,
            "Your files are up to date.",
            None,
            None,
        );
    }
    // A durable completion marker records first-run history, but the current
    // snapshot still has to be quiescent before this projection may call the
    // stage complete. This also prevents a newer running/follow-up cycle from
    // being hidden behind an old idle result.
    let pending = if pending.is_empty() {
        snapshot.libraries.iter().collect::<Vec<_>>()
    } else {
        pending
    };
    if snapshot.connection_state != DesktopControllerConnectionState::Connected
        || snapshot.freshness != DesktopControllerFreshness::Fresh
    {
        return progress_stage(
            "first_sync",
            "First sync",
            FirstRunProgressStageState::Waiting,
            "Waiting for connection…",
            None,
            None,
        );
    }
    if snapshot.sync_control_state == DesktopControllerSyncControlState::PausedByUser {
        return progress_stage(
            "first_sync",
            "First sync",
            FirstRunProgressStageState::ActionRequired,
            "Synchronization is paused.",
            Some("resume_sync"),
            Some("Resume synchronization"),
        );
    }
    if pending.iter().any(|library| {
        matches!(
            library.auth_state,
            DesktopControllerAuthState::Missing
                | DesktopControllerAuthState::Blocked
                | DesktopControllerAuthState::Revoked
        ) || library.runtime_state == DesktopControllerRuntimeState::AuthBlocked
    }) {
        return progress_stage(
            "first_sync",
            "First sync",
            FirstRunProgressStageState::ActionRequired,
            "Sign in again to continue.",
            Some("sign_in"),
            Some("Sign in to continue"),
        );
    }
    if pending
        .iter()
        .any(|library| library.root_state == DesktopControllerRootState::Unavailable)
    {
        return progress_stage(
            "first_sync",
            "First sync",
            FirstRunProgressStageState::ActionRequired,
            "The local folder is unavailable.",
            Some("check_again"),
            Some("Check folder"),
        );
    }
    if pending
        .iter()
        .any(|library| library.root_state == DesktopControllerRootState::Recovering)
    {
        return progress_stage(
            "first_sync",
            "First sync",
            FirstRunProgressStageState::Waiting,
            "Checking the local folder…",
            None,
            None,
        );
    }
    if pending
        .iter()
        .any(|library| library.conflict_state == DesktopControllerConflictState::Required)
    {
        return progress_stage(
            "first_sync",
            "First sync",
            FirstRunProgressStageState::ActionRequired,
            "Resolve the conflict to continue.",
            Some("resolve_conflict"),
            Some("Resolve the conflict"),
        );
    }
    if pending.iter().any(|library| {
        matches!(
            library.runtime_state,
            DesktopControllerRuntimeState::Faulted | DesktopControllerRuntimeState::Stopped
        ) || matches!(
            library.last_outcome,
            Some(
                DesktopControllerSyncOutcome::RecoveryBlocked
                    | DesktopControllerSyncOutcome::FatalLocal
                    | DesktopControllerSyncOutcome::Panicked
            )
        )
    }) {
        return progress_stage(
            "first_sync",
            "First sync",
            FirstRunProgressStageState::ActionRequired,
            "Synchronization needs attention.",
            Some("check_again"),
            Some("Check sync status"),
        );
    }
    if pending.iter().any(|library| {
        library.runtime_state == DesktopControllerRuntimeState::Running
            || library.last_outcome == Some(DesktopControllerSyncOutcome::Progress)
    }) {
        return progress_stage(
            "first_sync",
            "Synchronizing",
            FirstRunProgressStageState::Active,
            "Syncing your files…",
            None,
            None,
        );
    }
    if pending.iter().any(|library| {
        matches!(
            library.runtime_state,
            DesktopControllerRuntimeState::Scheduled | DesktopControllerRuntimeState::BackingOff
        ) || matches!(
            library.last_outcome,
            Some(
                DesktopControllerSyncOutcome::Offline
                    | DesktopControllerSyncOutcome::ServerTransient
                    | DesktopControllerSyncOutcome::RateLimited
            )
        )
    }) {
        return progress_stage(
            "first_sync",
            "First sync",
            FirstRunProgressStageState::Waiting,
            "Waiting to synchronize…",
            None,
            None,
        );
    }
    progress_stage(
        "first_sync",
        "First sync",
        FirstRunProgressStageState::Waiting,
        "Checking sync status…",
        None,
        None,
    )
}

fn first_sync_completion_proven(snapshot: &DesktopControllerSnapshot) -> bool {
    snapshot.connection_state == DesktopControllerConnectionState::Connected
        && snapshot.freshness == DesktopControllerFreshness::Fresh
        && snapshot.profile_authenticated
        && snapshot.process.is_some_and(|process| {
            matches!(process.state, synveil_client::DesktopProcessStatus::Running)
                && process.control_ready
        })
        && snapshot.sync_control_state == DesktopControllerSyncControlState::Running
        && !snapshot.libraries.is_empty()
        && snapshot.libraries.iter().all(|library| {
            library.first_sync_completed
                && library.runtime_state == DesktopControllerRuntimeState::Idle
                && library.last_outcome == Some(DesktopControllerSyncOutcome::Idle)
                && !library.wake_pending
                && library.root_state == DesktopControllerRootState::Available
                && library.auth_state == DesktopControllerAuthState::Ready
                && library.conflict_state == DesktopControllerConflictState::Clear
        })
}

/// Defensive controller-side ordering guard for GUI updates. A newer
/// connection generation always wins; within one generation revisions cannot
/// move backwards. Freshness changes at the same revision are allowed because
/// they represent a current connection transition rather than library data.
#[must_use]
pub fn snapshot_is_acceptable(current: &UiSnapshot, incoming: &UiSnapshot) -> bool {
    incoming.connection_generation > current.connection_generation
        || (incoming.connection_generation == current.connection_generation
            && incoming.revision >= current.revision)
}

/// Preserve a stable selection across an atomic snapshot replacement, or
/// clear it when the selected library is no longer present. A first snapshot
/// selects the first stable library only; no path or server value is inferred.
#[must_use]
pub fn selected_library_id(previous: Option<&str>, snapshot: &UiSnapshot) -> Option<String> {
    match previous {
        Some(previous)
            if snapshot
                .libraries
                .iter()
                .any(|library| library.library_id == previous) =>
        {
            Some(previous.to_owned())
        }
        Some(_) => None,
        None => snapshot
            .libraries
            .first()
            .map(|library| library.library_id.clone()),
    }
}

fn bounded_usize(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

fn empty_library_presentation(snapshot: &DesktopControllerSnapshot) -> &'static str {
    if !snapshot.profile_configured {
        "Connect to a Synveil server to get started."
    } else if snapshot.connection_state != DesktopControllerConnectionState::Connected
        || snapshot.freshness != DesktopControllerFreshness::Fresh
    {
        "Library status will appear here when Synveil reconnects."
    } else if !snapshot.profile_authenticated {
        "Sign in to this device to continue."
    } else {
        "Set up a library to start syncing."
    }
}

fn map_attention_item(item: &DesktopControllerAttentionItem) -> UiAttentionItem {
    let (category_code, category_label) = attention_category_presentation(&item.category);
    let item_kind_label = match item.item_kind {
        Some(DesktopControllerAttentionItemKind::File) => "File",
        Some(DesktopControllerAttentionItemKind::Directory) => "Folder",
        None => "Item",
    };
    let path_label = item
        .relative_path
        .clone()
        .unwrap_or_else(|| "Item details unavailable".to_owned());
    UiAttentionItem {
        attention_id: item.attention_id.clone(),
        library_id: item.library_id.clone(),
        intent_id: item.intent_id.clone(),
        library_label: library_label(&item.library_id),
        category_code: category_code.to_owned(),
        category_label: category_label.to_owned(),
        relative_path: item.relative_path.clone(),
        previous_relative_path: item.previous_relative_path.clone(),
        path_label,
        item_kind_label: item_kind_label.to_owned(),
        local_length: item.local_length,
        remote_length: item.remote_length,
        local_base_revision: item.local_base_revision,
        remote_observed_revision: item.remote_observed_revision,
        remote_observed_state: item.remote_observed_state.clone(),
        detected_at_ms: item.detected_at_ms,
        can_accept_remote: item
            .supported_actions
            .contains(&DesktopControllerConflictAction::AcceptRemote),
        can_retry_local: item
            .supported_actions
            .contains(&DesktopControllerConflictAction::RetryLocalAgainstCurrentBase),
    }
}

fn map_recovery_item(item: &DesktopControllerRecoveryItem) -> UiRecoveryItem {
    let (kind_code, kind_label, detail) = recovery_kind_presentation(item.kind);
    let detail = if item.kind == DesktopControllerRecoveryKind::RootUnavailable && item.waiting {
        "Synveil is checking the original folder. Your files and library are preserved."
    } else {
        detail
    };
    let (action_code, action_label) = item
        .action
        .map(recovery_action_presentation)
        .unwrap_or((None, ""));
    let (action_code, action_label) = if item.kind == DesktopControllerRecoveryKind::RootUnavailable
    {
        if item.waiting {
            (None, "")
        } else {
            (Some("restore_missing_folder"), "Restore missing folder")
        }
    } else {
        (action_code, action_label)
    };
    UiRecoveryItem {
        state: if item.waiting {
            RecoveryState::Waiting
        } else if item.action_required {
            RecoveryState::ActionRequired
        } else {
            RecoveryState::Recovered
        },
        capability: if action_code.is_some() {
            RecoveryCapability::Supported
        } else {
            RecoveryCapability::Unavailable
        },
        recovery_id: item.recovery_id.clone(),
        library_label: item
            .library_id
            .as_deref()
            .map(library_label)
            .unwrap_or_else(|| "Account".to_owned()),
        library_id: item.library_id.clone(),
        kind_code,
        kind_label,
        detail,
        action_code,
        action_label,
        action_required: item.action_required,
        waiting: item.waiting,
        connection_generation: item.connection_generation,
    }
}

fn recovery_action_has_dedicated_surface(
    snapshot: &DesktopControllerSnapshot,
    item: &DesktopControllerRecoveryItem,
) -> bool {
    match item.kind {
        DesktopControllerRecoveryKind::ProfileConfigurationRequired => !snapshot.profile_configured,
        DesktopControllerRecoveryKind::AuthenticationRequired => {
            item.library_id.is_none()
                && snapshot.profile_configured
                && !snapshot.profile_authenticated
        }
        DesktopControllerRecoveryKind::LibrarySetupIncomplete => {
            snapshot.profile_configured
                && snapshot.profile_authenticated
                && snapshot.libraries.is_empty()
        }
        DesktopControllerRecoveryKind::ClientUnavailable
        | DesktopControllerRecoveryKind::RootUnavailable
        | DesktopControllerRecoveryKind::ServerRetryable
        | DesktopControllerRecoveryKind::LocalFailure => false,
    }
}

fn client_recovery_presentation(
    state: synveil_client::DesktopControllerClientRecoveryState,
) -> (&'static str, &'static str) {
    match state {
        synveil_client::DesktopControllerClientRecoveryState::Ready => ("ready", "Ready"),
        synveil_client::DesktopControllerClientRecoveryState::Waiting => {
            ("waiting", "Waiting for background client")
        }
        synveil_client::DesktopControllerClientRecoveryState::Unavailable => {
            ("unavailable", "Background client unavailable")
        }
        synveil_client::DesktopControllerClientRecoveryState::Unknown => {
            ("unknown", "Background client status unknown")
        }
    }
}

fn recovery_kind_presentation(
    kind: DesktopControllerRecoveryKind,
) -> (&'static str, &'static str, &'static str) {
    match kind {
        DesktopControllerRecoveryKind::ClientUnavailable => (
            "client_unavailable",
            "Background client unavailable",
            "The existing background client is not reachable. Your recovery state is preserved.",
        ),
        DesktopControllerRecoveryKind::ProfileConfigurationRequired => (
            "profile_configuration_required",
            "Server connection required",
            "Configure the server connection before synchronization can continue.",
        ),
        DesktopControllerRecoveryKind::AuthenticationRequired => (
            "authentication_required",
            "Sign-in required",
            "Sign in again to resume synchronization for this profile.",
        ),
        DesktopControllerRecoveryKind::RootUnavailable => (
            "root_unavailable",
            "Local folder unavailable",
            "Reconnect the drive or restore access to the original folder, then select Restore missing folder to check it. Synveil has not treated it as empty. Your files and library are preserved.",
        ),
        DesktopControllerRecoveryKind::ServerRetryable => (
            "server_retryable",
            "Waiting for server",
            "The server is temporarily unavailable. Synveil will retry automatically; select Check again to refresh its status.",
        ),
        DesktopControllerRecoveryKind::LocalFailure => (
            "local_failure",
            "Local sync needs attention",
            "Synveil needs to check the local sync status before it can continue.",
        ),
        DesktopControllerRecoveryKind::LibrarySetupIncomplete => (
            "library_setup_incomplete",
            "Library setup incomplete",
            "Choose the same local folder to continue setup. Synveil keeps existing files.",
        ),
    }
}

fn recovery_action_presentation(
    action: DesktopControllerRecoveryAction,
) -> (Option<&'static str>, &'static str) {
    match action {
        DesktopControllerRecoveryAction::StartClient => (Some("start_client"), "Start client"),
        DesktopControllerRecoveryAction::ConfigureProfile => {
            (Some("configure_profile"), "Reconnect server")
        }
        DesktopControllerRecoveryAction::Authenticate => (Some("authenticate"), "Sign in"),
        DesktopControllerRecoveryAction::CheckAgain => (Some("check_again"), "Check again"),
        DesktopControllerRecoveryAction::ResumeSetup => (Some("resume_setup"), "Resume setup"),
    }
}

fn attention_category_presentation(category: &str) -> (&'static str, &'static str) {
    match category {
        "REMOTE_REVISION_CHANGED" => ("REMOTE_REVISION_CHANGED", "Remote revision changed"),
        "REMOTE_CONTENT_CHANGED" => ("REMOTE_CONTENT_CHANGED", "Remote content changed"),
        "REMOTE_STATE_CHANGED" => ("REMOTE_STATE_CHANGED", "Remote state changed"),
        "REMOTE_MISSING" => ("REMOTE_MISSING", "Remote item is missing"),
        "NAME_COLLISION" => ("NAME_COLLISION", "Name collision"),
        "PARENT_CHANGED_OR_UNAVAILABLE" => (
            "PARENT_CHANGED_OR_UNAVAILABLE",
            "Parent changed or unavailable",
        ),
        _ => ("UNKNOWN_CONFLICT", "Conflict details unavailable"),
    }
}

fn map_library(
    status: &DesktopControllerLibraryStatus,
    snapshot: &DesktopControllerSnapshot,
    process_control_ready: bool,
) -> UiLibrary {
    let (runtime_code, runtime_label) = runtime_presentation(status.runtime_state);
    let (root_code, root_label) = root_presentation(status.root_state);
    let (auth_code, auth_label) = auth_presentation(status.auth_state);
    let (conflict_code, conflict_label) = conflict_presentation(status.conflict_state);
    let (outcome_code, outcome_label) = status
        .last_outcome
        .map_or((None, "No recent sync result"), outcome_presentation);
    let can_sync = snapshot.connection_state == DesktopControllerConnectionState::Connected
        && snapshot.freshness == DesktopControllerFreshness::Fresh
        && process_control_ready
        && snapshot.sync_control_state != DesktopControllerSyncControlState::PausedByUser
        && root_code == "available"
        && auth_code == "ready"
        && conflict_code == "clear"
        && !matches!(
            status.runtime_state,
            DesktopControllerRuntimeState::AuthBlocked
                | DesktopControllerRuntimeState::RootBlocked
                | DesktopControllerRuntimeState::Faulted
                | DesktopControllerRuntimeState::Stopped
        );
    let needs_attention = conflict_code != "clear"
        || root_code != "available"
        || matches!(
            status.auth_state,
            DesktopControllerAuthState::Missing
                | DesktopControllerAuthState::Blocked
                | DesktopControllerAuthState::Revoked
        )
        || matches!(
            status.runtime_state,
            DesktopControllerRuntimeState::AuthBlocked
                | DesktopControllerRuntimeState::RootBlocked
                | DesktopControllerRuntimeState::Faulted
        );

    UiLibrary {
        library_id: status.library_id.clone(),
        label: library_label(&status.library_id),
        runtime_code,
        runtime_label,
        root_code,
        root_label,
        auth_code,
        auth_label,
        conflict_code,
        conflict_label,
        outcome_code,
        outcome_label,
        next_due_ms: status.next_due_ms,
        wake_pending: status.wake_pending,
        transient_failures: status.transient_failures,
        first_sync_completed: status.first_sync_completed,
        needs_attention,
        can_sync,
    }
}

fn library_label(library_id: &str) -> String {
    let prefix = library_id.get(..8).unwrap_or("unknown");
    format!("Library {prefix}")
}

fn connection_presentation(
    state: DesktopControllerConnectionState,
    freshness: DesktopControllerFreshness,
    error: Option<DesktopControllerErrorKind>,
) -> (&'static str, &'static str, &'static str) {
    match state {
        DesktopControllerConnectionState::Connected
            if freshness == DesktopControllerFreshness::Fresh =>
        {
            ("connected", "Connected", "Status is current.")
        }
        DesktopControllerConnectionState::Connected => (
            "stale",
            "Reconnecting",
            "Showing the last safe status while reconnecting.",
        ),
        DesktopControllerConnectionState::Connecting => (
            "connecting",
            "Connecting",
            "Waiting for the existing background service.",
        ),
        DesktopControllerConnectionState::Reconnecting => (
            "reconnecting",
            "Reconnecting",
            "Waiting for the existing background service.",
        ),
        DesktopControllerConnectionState::ProtocolIncompatible => (
            "protocol_incompatible",
            "Incompatible background service",
            "This desktop version cannot use the background service.",
        ),
        DesktopControllerConnectionState::Faulted => (
            "faulted",
            "Background service unavailable",
            "The background service is unavailable.",
        ),
        DesktopControllerConnectionState::Stopping => (
            "stopping",
            "Stopping",
            "The desktop shell is closing its local connection.",
        ),
        DesktopControllerConnectionState::Stopped => (
            "stopped",
            "Not connected",
            "The background service is not running.",
        ),
        DesktopControllerConnectionState::Disconnected => match error {
            Some(DesktopControllerErrorKind::EndpointSecurity) => (
                "security_error",
                "Connection unavailable",
                "The local control connection is unavailable.",
            ),
            _ => (
                "disconnected",
                "Not connected",
                "Waiting for the existing background service.",
            ),
        },
    }
}

fn freshness_presentation(freshness: DesktopControllerFreshness) -> (&'static str, &'static str) {
    match freshness {
        DesktopControllerFreshness::Fresh => ("fresh", "Current"),
        DesktopControllerFreshness::Stale => ("stale", "Last known status"),
        DesktopControllerFreshness::Unavailable => ("unavailable", "Status unavailable"),
    }
}

fn sync_control_presentation(
    state: DesktopControllerSyncControlState,
) -> (&'static str, &'static str) {
    match state {
        DesktopControllerSyncControlState::Running => ("running", "Running"),
        DesktopControllerSyncControlState::PausedByUser => ("paused", "Paused by user"),
        DesktopControllerSyncControlState::Unknown => ("unknown", "Sync status unavailable"),
    }
}

fn runtime_presentation(state: DesktopControllerRuntimeState) -> (&'static str, &'static str) {
    match state {
        DesktopControllerRuntimeState::Idle => ("idle", "Idle"),
        DesktopControllerRuntimeState::Scheduled => ("scheduled", "Scheduled"),
        DesktopControllerRuntimeState::Running => ("running", "Sync in progress"),
        DesktopControllerRuntimeState::BackingOff => ("backing_off", "Retrying later"),
        DesktopControllerRuntimeState::AuthBlocked => ("auth_blocked", "Authentication blocked"),
        DesktopControllerRuntimeState::RootBlocked => ("root_blocked", "Folder unavailable"),
        DesktopControllerRuntimeState::Faulted => ("faulted", "Needs attention"),
        DesktopControllerRuntimeState::Stopped => ("stopped", "Stopped"),
    }
}

fn root_presentation(state: DesktopControllerRootState) -> (&'static str, &'static str) {
    match state {
        DesktopControllerRootState::Available => ("available", "Folder available"),
        DesktopControllerRootState::Unavailable => ("unavailable", "Folder unavailable"),
        DesktopControllerRootState::Recovering => ("recovering", "Checking folder changes"),
    }
}

fn auth_presentation(state: DesktopControllerAuthState) -> (&'static str, &'static str) {
    match state {
        DesktopControllerAuthState::Ready => ("ready", "Authentication ready"),
        DesktopControllerAuthState::Missing => ("missing", "Authentication required"),
        DesktopControllerAuthState::Blocked => ("blocked", "Authentication blocked"),
        DesktopControllerAuthState::Revoked => ("revoked", "Authentication revoked"),
        DesktopControllerAuthState::Unknown => ("unknown", "Authentication status unknown"),
    }
}

fn conflict_presentation(state: DesktopControllerConflictState) -> (&'static str, &'static str) {
    match state {
        DesktopControllerConflictState::Clear => ("clear", "No conflict reported"),
        DesktopControllerConflictState::Required => ("required", "Needs attention"),
        DesktopControllerConflictState::Unknown => ("unknown", "Conflict status unknown"),
    }
}

fn outcome_presentation(
    outcome: DesktopControllerSyncOutcome,
) -> (Option<&'static str>, &'static str) {
    let (code, label) = match outcome {
        DesktopControllerSyncOutcome::Idle => ("idle", "No recent sync result"),
        DesktopControllerSyncOutcome::Progress => ("progress", "Sync in progress"),
        DesktopControllerSyncOutcome::ConflictBlocked => ("conflict_blocked", "Needs attention"),
        DesktopControllerSyncOutcome::Offline => ("offline", "Waiting for connection"),
        DesktopControllerSyncOutcome::ServerTransient => ("server_transient", "Retrying later"),
        DesktopControllerSyncOutcome::RateLimited => ("rate_limited", "Retrying later"),
        DesktopControllerSyncOutcome::AuthBlocked => ("auth_blocked", "Authentication blocked"),
        DesktopControllerSyncOutcome::RootUnavailable => ("root_unavailable", "Folder unavailable"),
        DesktopControllerSyncOutcome::RecoveryBlocked => {
            ("recovery_blocked", "Recovery needs attention")
        }
        DesktopControllerSyncOutcome::FatalLocal => ("fatal_local", "Needs attention"),
        DesktopControllerSyncOutcome::Panicked => ("panicked", "Needs attention"),
    };
    (Some(code), label)
}

fn error_label(error: &DesktopControllerErrorKind) -> &'static str {
    match error {
        DesktopControllerErrorKind::EndpointUnavailable
        | DesktopControllerErrorKind::ConnectionLost => {
            "The local control connection is unavailable."
        }
        DesktopControllerErrorKind::EndpointSecurity => {
            "The local control connection is unavailable."
        }
        DesktopControllerErrorKind::ProtocolIncompatible => {
            "This desktop version cannot use the background service."
        }
        DesktopControllerErrorKind::MalformedServerResponse => {
            "The background service returned an unusable status."
        }
        DesktopControllerErrorKind::CommandRejected => {
            "The background service rejected the request."
        }
        DesktopControllerErrorKind::UnsupportedPlatform => {
            "This platform does not support the local desktop connection."
        }
        DesktopControllerErrorKind::Stopped => "The background service is not running.",
        DesktopControllerErrorKind::Internal => "The background service is unavailable.",
    }
}

/// Map command results without implying that synchronization completed.
#[must_use]
pub const fn command_feedback(result: DesktopControllerCommandResult) -> &'static str {
    match result {
        DesktopControllerCommandResult::Accepted => {
            "Sync requested; completion will appear in status."
        }
        DesktopControllerCommandResult::Coalesced
        | DesktopControllerCommandResult::AlreadyRunningFollowupRecorded => {
            "Sync already running; request recorded."
        }
        DesktopControllerCommandResult::Paused => "Sync is paused by user.",
        DesktopControllerCommandResult::Resumed => "Sync resumed; status will update.",
        DesktopControllerCommandResult::AlreadyPaused => "Sync is already paused.",
        DesktopControllerCommandResult::AlreadyRunning => "Sync is already running.",
        DesktopControllerCommandResult::Disconnected
        | DesktopControllerCommandResult::Unavailable
        | DesktopControllerCommandResult::AlreadyUnavailable => "Background service unavailable.",
        DesktopControllerCommandResult::UnknownLibrary => "That library is no longer available.",
        DesktopControllerCommandResult::RuntimeStopped
        | DesktopControllerCommandResult::Stopped => "Background service is stopped.",
        DesktopControllerCommandResult::AdmissionLimited => "Try again in a moment.",
        DesktopControllerCommandResult::OutcomeUnknown => {
            "Synveil cannot confirm whether that completed. It is checking the current status; wait for the update."
        }
        DesktopControllerCommandResult::ProtocolError => "Background service is incompatible.",
        DesktopControllerCommandResult::Authenticated => "Signed in; status will update.",
        DesktopControllerCommandResult::SignedOut => "Signed out; local credential removed.",
        DesktopControllerCommandResult::ProfileConfigured => {
            "Server profile saved; continue with device authentication."
        }
        DesktopControllerCommandResult::ProfileValidated => {
            "Server address verified; ready to save."
        }
        DesktopControllerCommandResult::ProfileAlreadyConfigured => {
            "Server profile is already configured."
        }
        DesktopControllerCommandResult::LibraryConfigured => "Library created; status will update.",
        DesktopControllerCommandResult::LibraryAlreadyConfigured => {
            "That folder is already configured."
        }
        DesktopControllerCommandResult::InvalidLibraryName => "Enter a valid library name.",
        DesktopControllerCommandResult::InvalidLibraryRoot => {
            "Choose a writable local folder with no conflicting Synveil control tree."
        }
        DesktopControllerCommandResult::AuthenticationRequired => {
            "Authenticate this device before creating a library."
        }
        DesktopControllerCommandResult::ServerIdentityConflict => {
            "The server returned a conflicting library identity."
        }
        DesktopControllerCommandResult::InvalidCredentials => {
            "The enrollment token was not accepted."
        }
        DesktopControllerCommandResult::InvalidConfiguration => "The profile details are invalid.",
        DesktopControllerCommandResult::InvalidServerAddress => {
            "Enter a valid HTTPS server address."
        }
        DesktopControllerCommandResult::NetworkUnavailable => {
            "Network unavailable. Try again later."
        }
        DesktopControllerCommandResult::ServerUnavailable => {
            "The server is unavailable. Try again later."
        }
        DesktopControllerCommandResult::Timeout => "The server did not respond in time.",
        DesktopControllerCommandResult::TlsFailure => {
            "A secure connection to the server could not be established."
        }
        DesktopControllerCommandResult::IncompatibleServer => {
            "The address is not a compatible Synveil server."
        }
        DesktopControllerCommandResult::PersistenceFailure => {
            "The profile could not be saved locally."
        }
        DesktopControllerCommandResult::RateLimited => "Too many attempts. Try again later.",
        DesktopControllerCommandResult::SecureStoreUnavailable => {
            "Secure credential storage is unavailable."
        }
        DesktopControllerCommandResult::Busy => "Another request is already being processed.",
        DesktopControllerCommandResult::ConflictResolved => {
            "Conflict decision saved; sync status will update."
        }
        DesktopControllerCommandResult::ConflictAlreadyResolved => {
            "That conflict was already resolved; status will update."
        }
        DesktopControllerCommandResult::ConflictStale => {
            "That conflict changed; refreshed attention details."
        }
        DesktopControllerCommandResult::ConflictNotFound => {
            "That conflict is no longer available; refreshed attention details."
        }
        DesktopControllerCommandResult::UnsupportedConflictAction => {
            "That action is not available for this conflict."
        }
        DesktopControllerCommandResult::ConflictPersistenceFailure => {
            "The conflict decision could not be saved locally."
        }
        // The desktop shell never exposes the Prompt 96 process-shutdown
        // command.  Keep this defensive mapping generic if a future caller
        // hands the presentation layer that result anyway.
        DesktopControllerCommandResult::ShutdownAccepted => {
            "Synveil cannot confirm whether that completed. It is checking the current status; wait for the update."
        }
    }
}

/// Map global pause/resume results to bounded, generic settings copy.
#[must_use]
pub const fn sync_control_feedback(result: DesktopControllerCommandResult) -> &'static str {
    match result {
        DesktopControllerCommandResult::Paused => "Sync paused. Local status remains available.",
        DesktopControllerCommandResult::Resumed => "Sync resumed; queued work will be considered.",
        DesktopControllerCommandResult::AlreadyPaused => "Sync is already paused.",
        DesktopControllerCommandResult::AlreadyRunning => "Sync is already running.",
        DesktopControllerCommandResult::PersistenceFailure => {
            "Sync setting could not be saved; runtime state was not changed."
        }
        DesktopControllerCommandResult::Busy => "Another sync setting request is in progress.",
        DesktopControllerCommandResult::OutcomeUnknown => {
            "Synveil cannot confirm whether the sync setting changed. It is checking the current status."
        }
        DesktopControllerCommandResult::Disconnected
        | DesktopControllerCommandResult::Unavailable
        | DesktopControllerCommandResult::AlreadyUnavailable => "Background service unavailable.",
        DesktopControllerCommandResult::Stopped
        | DesktopControllerCommandResult::RuntimeStopped => "Background service is stopped.",
        DesktopControllerCommandResult::ProtocolError => "Background service is incompatible.",
        DesktopControllerCommandResult::AdmissionLimited => "Try again in a moment.",
        _ => "Sync setting request was not applied.",
    }
}

/// Map library onboarding results to bounded product copy and a safe action.
/// The selected folder and remote diagnostics are intentionally not echoed.
#[must_use]
pub const fn library_setup_presentation(
    result: DesktopControllerCommandResult,
) -> LibrarySetupPresentation {
    match result {
        DesktopControllerCommandResult::LibraryConfigured
        | DesktopControllerCommandResult::LibraryAlreadyConfigured => LibrarySetupPresentation {
            code: "checking",
            message: "Checking whether your library is ready…",
            action: "wait",
        },
        DesktopControllerCommandResult::InvalidLibraryName => LibrarySetupPresentation {
            code: "invalid_name",
            message: "Enter a library name to continue.",
            action: "edit_name",
        },
        DesktopControllerCommandResult::InvalidLibraryRoot => LibrarySetupPresentation {
            code: "invalid_folder",
            message: "Synveil can’t use that folder. Choose another folder or check that it is available.",
            action: "choose_folder",
        },
        DesktopControllerCommandResult::AuthenticationRequired => LibrarySetupPresentation {
            code: "sign_in_required",
            message: "Sign in to this device to continue.",
            action: "sign_in",
        },
        DesktopControllerCommandResult::ServerIdentityConflict => LibrarySetupPresentation {
            code: "setup_needs_review",
            message: "Synveil couldn’t safely confirm this setup. Check your server connection before continuing.",
            action: "check_connection",
        },
        DesktopControllerCommandResult::NetworkUnavailable
        | DesktopControllerCommandResult::ServerUnavailable
        | DesktopControllerCommandResult::Timeout
        | DesktopControllerCommandResult::TlsFailure => LibrarySetupPresentation {
            code: "connection_problem",
            message: "Synveil couldn’t reach your server securely. Check your connection and try again.",
            action: "retry_setup",
        },
        DesktopControllerCommandResult::IncompatibleServer => LibrarySetupPresentation {
            code: "server_incompatible",
            message: "This server isn’t compatible with this version of Synveil.",
            action: "check_server",
        },
        DesktopControllerCommandResult::PersistenceFailure => LibrarySetupPresentation {
            code: "setup_incomplete",
            message: "Setup is incomplete on this device. Choose the same folder to continue safely.",
            action: "resume_setup",
        },
        DesktopControllerCommandResult::Busy | DesktopControllerCommandResult::AdmissionLimited => {
            LibrarySetupPresentation {
                code: "busy",
                message: "Setup is already being processed. Please wait.",
                action: "wait",
            }
        }
        DesktopControllerCommandResult::OutcomeUnknown => LibrarySetupPresentation {
            code: "checking",
            message: "Checking whether your library was created…",
            action: "wait",
        },
        DesktopControllerCommandResult::Stopped
        | DesktopControllerCommandResult::RuntimeStopped => LibrarySetupPresentation {
            code: "client_unavailable",
            message: "Synveil isn’t ready yet. Start the app again to continue setup.",
            action: "start_client",
        },
        DesktopControllerCommandResult::ProtocolError
        | DesktopControllerCommandResult::Unavailable
        | DesktopControllerCommandResult::Disconnected
        | DesktopControllerCommandResult::AlreadyUnavailable => LibrarySetupPresentation {
            code: "client_unavailable",
            message: "Synveil isn’t ready yet. Reopen the app to continue setup.",
            action: "start_client",
        },
        _ => LibrarySetupPresentation {
            code: "setup_problem",
            message: "Synveil couldn’t finish setting up your library. Choose the same folder to continue safely.",
            action: "resume_setup",
        },
    }
}

/// Map authentication outcomes to safe, generic UI copy. No server detail,
/// credential, token, path, or response body is included.
#[must_use]
pub const fn auth_feedback(result: DesktopControllerCommandResult) -> &'static str {
    if matches!(
        result,
        DesktopControllerCommandResult::Authenticated
            | DesktopControllerCommandResult::SignedOut
            | DesktopControllerCommandResult::InvalidCredentials
            | DesktopControllerCommandResult::NetworkUnavailable
            | DesktopControllerCommandResult::ServerUnavailable
            | DesktopControllerCommandResult::RateLimited
            | DesktopControllerCommandResult::SecureStoreUnavailable
            | DesktopControllerCommandResult::Busy
            | DesktopControllerCommandResult::OutcomeUnknown
            | DesktopControllerCommandResult::ProtocolError
    ) {
        return authentication_presentation(result).message;
    }
    match result {
        DesktopControllerCommandResult::Authenticated
        | DesktopControllerCommandResult::SignedOut
        | DesktopControllerCommandResult::InvalidCredentials
        | DesktopControllerCommandResult::NetworkUnavailable
        | DesktopControllerCommandResult::ServerUnavailable
        | DesktopControllerCommandResult::RateLimited
        | DesktopControllerCommandResult::SecureStoreUnavailable
        | DesktopControllerCommandResult::Busy
        | DesktopControllerCommandResult::OutcomeUnknown
        | DesktopControllerCommandResult::ProtocolError => authentication_presentation(result).message,
        DesktopControllerCommandResult::ProfileConfigured => {
            "Server profile saved; continue with device authentication."
        }
        DesktopControllerCommandResult::ProfileValidated => {
            "Server address verified; ready to save."
        }
        DesktopControllerCommandResult::ProfileAlreadyConfigured => {
            "Server profile is already configured."
        }
        DesktopControllerCommandResult::LibraryConfigured
        | DesktopControllerCommandResult::LibraryAlreadyConfigured
        | DesktopControllerCommandResult::InvalidLibraryName
        | DesktopControllerCommandResult::InvalidLibraryRoot
        | DesktopControllerCommandResult::AuthenticationRequired
        | DesktopControllerCommandResult::ServerIdentityConflict => {
            "Library setup status is unavailable."
        }
        DesktopControllerCommandResult::InvalidConfiguration => "The profile details are invalid.",
        DesktopControllerCommandResult::InvalidServerAddress => {
            "Enter a valid HTTPS server address."
        }
        DesktopControllerCommandResult::Timeout => "The server did not respond in time.",
        DesktopControllerCommandResult::TlsFailure => {
            "A secure connection to the server could not be established."
        }
        DesktopControllerCommandResult::IncompatibleServer => {
            "The address is not a compatible Synveil server."
        }
        DesktopControllerCommandResult::PersistenceFailure => {
            "The profile could not be saved locally."
        }
        DesktopControllerCommandResult::Disconnected
        | DesktopControllerCommandResult::Unavailable
        | DesktopControllerCommandResult::AlreadyUnavailable => "Background service unavailable.",
        DesktopControllerCommandResult::Stopped
        | DesktopControllerCommandResult::RuntimeStopped => "Background service is stopped.",
        DesktopControllerCommandResult::AdmissionLimited => "Try again in a moment.",
        DesktopControllerCommandResult::UnknownLibrary => "That library is no longer available.",
        DesktopControllerCommandResult::Accepted
        | DesktopControllerCommandResult::Coalesced
        | DesktopControllerCommandResult::AlreadyRunningFollowupRecorded
        | DesktopControllerCommandResult::Paused
        | DesktopControllerCommandResult::Resumed
        | DesktopControllerCommandResult::AlreadyPaused
        | DesktopControllerCommandResult::AlreadyRunning
        | DesktopControllerCommandResult::ConflictResolved
        | DesktopControllerCommandResult::ConflictAlreadyResolved
        | DesktopControllerCommandResult::ConflictStale
        | DesktopControllerCommandResult::ConflictNotFound
        | DesktopControllerCommandResult::UnsupportedConflictAction
        | DesktopControllerCommandResult::ConflictPersistenceFailure
        | DesktopControllerCommandResult::ShutdownAccepted => {
            "Synveil cannot confirm whether that completed. It is checking the current status; wait for the update."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use synveil_client::{DesktopControllerProcessStatus, DesktopProcessStatus, ServerProfileId};

    #[test]
    fn p042_same_root_restore_is_a_typed_preserving_recheck() {
        let mut library = status(library_id(42));
        library.root_state = DesktopControllerRootState::Unavailable;
        let snapshot = authenticated_snapshot(vec![library]);
        let ui = map_snapshot(&snapshot);
        let item = &ui.recovery_items[0];
        assert_eq!(item.action_code, Some("restore_missing_folder"));
        assert_eq!(item.state, RecoveryState::ActionRequired);
        assert_eq!(item.capability, RecoveryCapability::Supported);
        assert!(item.detail.contains("original folder"));
        assert!(item.detail.contains("not treated it as empty"));
        assert_eq!(ui.libraries.len(), 1);
        assert!(snapshot.libraries[0].first_sync_completed);
        assert!(recovery_mutation_allowed(&ui, &snapshot));
        let mut changed = snapshot.clone();
        changed.process.as_mut().unwrap().control_ready = false;
        assert!(!recovery_mutation_allowed(&ui, &changed));
        changed = snapshot.clone();
        changed.revision += 1;
        assert!(!recovery_mutation_allowed(&ui, &changed));
        changed = snapshot.clone();
        changed.connection_generation += 1;
        assert!(!recovery_mutation_allowed(&ui, &changed));
        changed = snapshot;
        changed.freshness = DesktopControllerFreshness::Stale;
        assert!(!recovery_mutation_allowed(&ui, &changed));
    }
    #[test]
    fn p042_recovering_folder_waits_and_transient_server_does_not_edit_profile() {
        let mut library = status(library_id(42));
        library.root_state = DesktopControllerRootState::Recovering;
        let ui = map_snapshot(&authenticated_snapshot(vec![library.clone()]));
        assert_eq!(ui.recovery_items[0].state, RecoveryState::Waiting);
        assert_ne!(
            ui.recovery_items[0].action_code,
            Some("restore_missing_folder")
        );
        library.root_state = DesktopControllerRootState::Available;
        library.last_outcome = Some(DesktopControllerSyncOutcome::ServerTransient);
        let ui = map_snapshot(&authenticated_snapshot(vec![library]));
        assert_eq!(ui.recovery_items[0].state, RecoveryState::Waiting);
        assert_eq!(ui.recovery_items[0].action_code, Some("check_again"));
    }
    #[test]
    fn p042_repair_is_guidance_without_a_desktop_lifecycle_authority() {
        let (capability, detail) = repair_presentation();
        assert_ne!(capability, RecoveryCapability::Supported);
        assert!(!detail.is_empty());
        assert_eq!(
            recovery_action_presentation(DesktopControllerRecoveryAction::ConfigureProfile).1,
            "Reconnect server"
        );
    }
    #[test]
    fn p042_product_surface_has_stable_accessibility_and_no_secrets() {
        let qml = include_str!("../qml/Main.qml");
        for name in [
            "recoveryPage",
            "recoverySummary",
            "repairSynveilButton",
            "reconnectServerButton",
            "restoreMissingFolderButton",
            "restartBackgroundServiceButton",
            "recoveryBusyIndicator",
            "recoveryFeedbackLabel",
        ] {
            assert!(qml.contains(name), "{name}");
        }
        for kind in [
            DesktopControllerRecoveryKind::ClientUnavailable,
            DesktopControllerRecoveryKind::RootUnavailable,
            DesktopControllerRecoveryKind::ServerRetryable,
        ] {
            let copy = recovery_kind_presentation(kind).2.to_lowercase();
            for term in [
                "sqlite",
                "ipc",
                "secretstore",
                "uuid",
                "root binding",
                ".synveil",
                "postgresql",
                "manifest",
                "journal",
            ] {
                assert!(!copy.contains(term));
            }
        }
    }

    fn library_id(seed: u128) -> String {
        let mut bytes = seed.to_be_bytes();
        bytes[6] = (bytes[6] & 0x0f) | 0x70;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        uuid::Uuid::from_bytes(bytes).hyphenated().to_string()
    }

    fn status(id: String) -> DesktopControllerLibraryStatus {
        DesktopControllerLibraryStatus {
            library_id: id,
            runtime_state: DesktopControllerRuntimeState::Idle,
            root_state: DesktopControllerRootState::Available,
            auth_state: DesktopControllerAuthState::Ready,
            conflict_state: DesktopControllerConflictState::Clear,
            next_due_ms: None,
            last_outcome: Some(DesktopControllerSyncOutcome::Idle),
            wake_pending: false,
            transient_failures: 0,
            first_sync_completed: true,
        }
    }

    fn fresh_snapshot(statuses: Vec<DesktopControllerLibraryStatus>) -> DesktopControllerSnapshot {
        DesktopControllerSnapshot {
            connection_state: DesktopControllerConnectionState::Connected,
            process: Some(DesktopControllerProcessStatus {
                state: DesktopProcessStatus::Running,
                control_ready: true,
            }),
            libraries: statuses,
            libraries_truncated: false,
            attention: synveil_client::DesktopControllerAttentionSnapshot::default(),
            revision: 4,
            freshness: DesktopControllerFreshness::Fresh,
            last_error: None,
            connection_generation: 2,
            profile_configured: false,
            profile_authenticated: false,
            profile_display_name: None,
            profile_server_url: None,
            sync_control_state: DesktopControllerSyncControlState::Running,
        }
    }

    fn authenticated_snapshot(
        statuses: Vec<DesktopControllerLibraryStatus>,
    ) -> DesktopControllerSnapshot {
        let mut snapshot = fresh_snapshot(statuses);
        snapshot.profile_configured = true;
        snapshot.profile_authenticated = true;
        snapshot
    }

    fn progress_stage_for<'a>(
        progress: &'a FirstRunProgress,
        id: &str,
    ) -> &'a FirstRunProgressStage {
        progress
            .stages
            .iter()
            .find(|stage| stage.id == id)
            .expect("progress stage")
    }

    fn first_sync_stage_for(snapshot: &DesktopControllerSnapshot) -> FirstRunProgressStage {
        first_sync_stage(
            snapshot,
            !snapshot.libraries.is_empty(),
            snapshot.profile_authenticated,
        )
    }

    #[test]
    fn ux_unit_1_configuration_required_has_a_clear_next_step() {
        let ui = map_snapshot(&fresh_snapshot(Vec::new()));
        assert_eq!(
            ui.empty_library_message,
            "Connect to a Synveil server to get started."
        );
        assert!(ui.recovery_items.is_empty());
        assert_eq!(ui.recovery_action_required, 0);
    }

    #[test]
    fn ux_unit_2_unauthenticated_empty_state_prompts_sign_in() {
        let mut snapshot = fresh_snapshot(Vec::new());
        snapshot.profile_configured = true;
        snapshot.profile_authenticated = false;
        let ui = map_snapshot(&snapshot);

        assert_eq!(
            ui.empty_library_message,
            "Sign in to this device to continue."
        );
        assert!(ui.recovery_items.is_empty());
        assert_eq!(ui.recovery_action_required, 0);
    }

    #[test]
    fn ux_unit_3_authenticated_empty_state_uses_one_library_setup_action() {
        let ui = map_snapshot(&authenticated_snapshot(Vec::new()));

        assert_eq!(
            ui.empty_library_message,
            "Set up a library to start syncing."
        );
        assert!(ui.recovery_items.is_empty());
        assert_eq!(ui.recovery_action_required, 0);
        assert!(library_first_run_required(true, true, true, false));
        assert!(!library_first_run_required(false, true, true, false));
        assert!(!library_first_run_required(true, false, true, false));
        assert!(!library_first_run_required(true, true, false, false));
        assert!(!library_first_run_required(true, true, true, true));
        assert!(!library_setup_confirmation_finished(true, true, 8, 8));
        assert!(!library_setup_confirmation_finished(true, false, 9, 8));
        assert!(!library_setup_confirmation_finished(false, true, 9, 8));
        assert!(library_setup_confirmation_finished(true, true, 9, 8));
    }

    #[test]
    fn ux_unit_4_pause_and_recovery_are_shown_as_separate_states() {
        let mut unavailable = status(library_id(1144));
        unavailable.root_state = DesktopControllerRootState::Unavailable;
        unavailable.runtime_state = DesktopControllerRuntimeState::RootBlocked;
        let mut snapshot = authenticated_snapshot(vec![unavailable]);
        snapshot.sync_control_state = DesktopControllerSyncControlState::PausedByUser;

        let ui = map_snapshot(&snapshot);
        assert_eq!(ui.sync_control_label, "Paused by user");
        assert!(!ui.can_sync_any);
        assert_eq!(ui.recovery_action_required, 1);
        assert!(ui
            .recovery_items
            .iter()
            .any(|item| item.kind_code == "root_unavailable" && item.action_required));
    }

    #[test]
    fn ux_unit_5_root_unavailable_never_looks_like_an_empty_folder() {
        let mut unavailable = status(library_id(1145));
        unavailable.root_state = DesktopControllerRootState::Unavailable;
        unavailable.runtime_state = DesktopControllerRuntimeState::RootBlocked;

        let ui = map_snapshot(&authenticated_snapshot(vec![unavailable]));
        let recovery = ui
            .recovery_items
            .iter()
            .find(|item| item.kind_code == "root_unavailable")
            .expect("root-unavailable recovery item");

        assert_eq!(ui.libraries[0].root_label, "Folder unavailable");
        assert!(recovery.detail.contains("not treated it as empty"));
        assert!(!recovery.detail.to_ascii_lowercase().contains("deleted"));
    }

    #[test]
    fn ux_unit_6_conflict_actions_match_only_the_supported_choices() {
        let library = library_id(1146);
        let mut snapshot = authenticated_snapshot(vec![status(library.clone())]);
        snapshot.attention = synveil_client::DesktopControllerAttentionSnapshot {
            summary: synveil_client::DesktopControllerAttentionSummary {
                total_count: 2,
                conflict_count: 2,
                other_count: 0,
            },
            libraries: Vec::new(),
            items: vec![
                synveil_client::DesktopControllerAttentionItem {
                    attention_id: "attention-accept".to_owned(),
                    library_id: library.clone(),
                    conflict_id: "conflict-accept".to_owned(),
                    intent_id: "intent-accept".to_owned(),
                    node_id: None,
                    category: "REMOTE_CONTENT_CHANGED".to_owned(),
                    relative_path: Some("notes/today.txt".to_owned()),
                    previous_relative_path: None,
                    item_kind: Some(synveil_client::DesktopControllerAttentionItemKind::File),
                    local_length: None,
                    remote_length: None,
                    local_base_revision: None,
                    remote_observed_revision: None,
                    remote_observed_state: None,
                    detected_at_ms: 1,
                    supported_actions: vec![synveil_client::DesktopControllerConflictAction::AcceptRemote],
                },
                synveil_client::DesktopControllerAttentionItem {
                    attention_id: "attention-retry".to_owned(),
                    library_id: library.clone(),
                    conflict_id: "conflict-retry".to_owned(),
                    intent_id: "intent-retry".to_owned(),
                    node_id: None,
                    category: "REMOTE_REVISION_CHANGED".to_owned(),
                    relative_path: Some("notes/later.txt".to_owned()),
                    previous_relative_path: None,
                    item_kind: Some(synveil_client::DesktopControllerAttentionItemKind::File),
                    local_length: None,
                    remote_length: None,
                    local_base_revision: None,
                    remote_observed_revision: None,
                    remote_observed_state: None,
                    detected_at_ms: 2,
                    supported_actions: vec![
                        synveil_client::DesktopControllerConflictAction::RetryLocalAgainstCurrentBase,
                    ],
                },
            ],
            truncated: false,
        };

        let ui = map_snapshot(&snapshot);
        assert_eq!(ui.attention_items.len(), 2);
        assert!(ui.attention_items[0].can_accept_remote);
        assert!(!ui.attention_items[0].can_retry_local);
        assert!(!ui.attention_items[1].can_accept_remote);
        assert!(ui.attention_items[1].can_retry_local);
    }

    #[test]
    fn ux_unit_7_errors_are_short_and_never_expose_internal_details() {
        let errors = [
            DesktopControllerErrorKind::EndpointUnavailable,
            DesktopControllerErrorKind::EndpointSecurity,
            DesktopControllerErrorKind::ProtocolIncompatible,
            DesktopControllerErrorKind::ConnectionLost,
            DesktopControllerErrorKind::MalformedServerResponse,
            DesktopControllerErrorKind::CommandRejected,
            DesktopControllerErrorKind::UnsupportedPlatform,
            DesktopControllerErrorKind::Stopped,
            DesktopControllerErrorKind::Internal,
        ];

        for error in errors {
            let label = error_label(&error);
            let lower = label.to_ascii_lowercase();
            assert!(!label.is_empty());
            assert!(!lower.contains("debug"));
            assert!(!lower.contains("sqlite"));
            assert!(!lower.contains("token"));
            assert!(!label.contains("/"));
            assert!(!label.contains("::"));
        }
    }

    #[test]
    fn library_setup_results_have_bounded_copy_and_one_safe_action() {
        let cases = [
            (
                DesktopControllerCommandResult::LibraryConfigured,
                "checking",
                "wait",
            ),
            (
                DesktopControllerCommandResult::LibraryAlreadyConfigured,
                "checking",
                "wait",
            ),
            (
                DesktopControllerCommandResult::InvalidLibraryName,
                "invalid_name",
                "edit_name",
            ),
            (
                DesktopControllerCommandResult::InvalidLibraryRoot,
                "invalid_folder",
                "choose_folder",
            ),
            (
                DesktopControllerCommandResult::AuthenticationRequired,
                "sign_in_required",
                "sign_in",
            ),
            (
                DesktopControllerCommandResult::ServerIdentityConflict,
                "setup_needs_review",
                "check_connection",
            ),
            (
                DesktopControllerCommandResult::NetworkUnavailable,
                "connection_problem",
                "retry_setup",
            ),
            (
                DesktopControllerCommandResult::ServerUnavailable,
                "connection_problem",
                "retry_setup",
            ),
            (
                DesktopControllerCommandResult::Timeout,
                "connection_problem",
                "retry_setup",
            ),
            (
                DesktopControllerCommandResult::TlsFailure,
                "connection_problem",
                "retry_setup",
            ),
            (
                DesktopControllerCommandResult::IncompatibleServer,
                "server_incompatible",
                "check_server",
            ),
            (
                DesktopControllerCommandResult::PersistenceFailure,
                "setup_incomplete",
                "resume_setup",
            ),
            (DesktopControllerCommandResult::Busy, "busy", "wait"),
            (
                DesktopControllerCommandResult::OutcomeUnknown,
                "checking",
                "wait",
            ),
            (
                DesktopControllerCommandResult::Unavailable,
                "client_unavailable",
                "start_client",
            ),
            (
                DesktopControllerCommandResult::ProtocolError,
                "client_unavailable",
                "start_client",
            ),
        ];
        for (result, code, action) in cases {
            let presented = library_setup_presentation(result);
            assert_eq!(presented.code, code);
            assert_eq!(presented.action, action);
            assert!(!presented.message.is_empty());
            for forbidden in [
                "replica",
                "root node",
                "UUID",
                "manifest",
                "IPC",
                ".synveil",
            ] {
                assert!(
                    !presented
                        .message
                        .to_ascii_lowercase()
                        .contains(&forbidden.to_ascii_lowercase()),
                    "{} exposed {forbidden}",
                    presented.message
                );
            }
            assert!(!presented.message.contains('/'));
        }
        let unknown = library_setup_presentation(DesktopControllerCommandResult::OutcomeUnknown);
        assert!(unknown.message.contains("Checking"));
        assert_eq!(unknown.action, "wait");
    }

    #[test]
    fn ux_unit_8_unknown_outcomes_wait_for_reconciliation_without_retry_advice() {
        let messages = [
            command_feedback(DesktopControllerCommandResult::OutcomeUnknown),
            sync_control_feedback(DesktopControllerCommandResult::OutcomeUnknown),
            library_setup_presentation(DesktopControllerCommandResult::OutcomeUnknown).message,
            auth_feedback(DesktopControllerCommandResult::OutcomeUnknown),
        ];

        for message in messages {
            let lower = message.to_ascii_lowercase();
            assert!(lower.contains("checking"), "{message}");
            assert!(!lower.contains("retry"), "{message}");
            assert!(!lower.contains("try again"), "{message}");
            assert!(!lower.contains("outcomeunknown"), "{message}");
        }

        let bridge = include_str!("bridge.rs");
        assert!(bridge.contains("controller.refresh_state();"));
        assert!(bridge.contains("never replay Sync Now"));
    }

    #[test]
    fn ux_unit_10_critical_controls_have_accessible_names_and_keyboard_submission() {
        let qml = include_str!("../qml/Main.qml");
        for accessible_name in [
            "Open desktop settings",
            "Close desktop settings",
            "Pause synchronization",
            "Resume synchronization",
            "Close the desktop window to the system tray",
            "Server address",
            "Sign in this device",
            "Library name",
            "Choose local library folder",
            "Create library",
            "Accept the remote conflict state",
            "Retry the local change against the current remote state",
        ] {
            assert!(
                qml.contains(accessible_name),
                "missing accessible name: {accessible_name}"
            );
        }
        assert!(qml.contains("onAccepted: root.submitProfileConfiguration()"));
        assert!(qml.contains("onAccepted: root.submitLibrarySetup()"));
        assert!(qml.contains("function submitToken()"));
        assert!(qml.contains("ScrollView {"));
    }

    #[test]
    fn ux_unit_11_idle_empty_attention_state_is_neutral() {
        let ui = map_snapshot(&authenticated_snapshot(vec![status(library_id(1147))]));
        assert_eq!(ui.libraries[0].runtime_label, "Idle");
        assert_eq!(ui.libraries[0].outcome_label, "No recent sync result");
        assert_eq!(ui.libraries[0].conflict_label, "No conflict reported");
        assert_eq!(ui.attention_count, 0);
        assert_eq!(ui.recovery_action_required, 0);
        assert_eq!(ui.recovery_waiting_count, 0);
    }

    #[test]
    fn ux_unit_12_recovery_copy_uses_plain_language_and_safe_next_steps() {
        let server = recovery_kind_presentation(DesktopControllerRecoveryKind::ServerRetryable).2;
        let local = recovery_kind_presentation(DesktopControllerRecoveryKind::LocalFailure).2;
        let setup =
            recovery_kind_presentation(DesktopControllerRecoveryKind::LibrarySetupIncomplete).2;

        assert!(server.contains("retry automatically"));
        assert!(server.contains("Check again"));
        assert!(local.contains("check the local sync status"));
        assert!(setup.contains("same local folder"));
        for message in [server, local, setup] {
            let lower = message.to_ascii_lowercase();
            for term in ["bounded", "authoritative", "identity", "reconcile"] {
                assert!(!lower.contains(term), "{message}");
            }
        }
    }

    #[test]
    fn ui1_empty_initial_state_does_not_claim_synced() {
        let ui = map_snapshot(&DesktopControllerSnapshot::default());
        assert!(ui.libraries.is_empty());
        assert!(!ui.can_sync_any);
        assert_ne!(ui.connection_label, "Everything synced");
    }

    #[test]
    fn ui2_available_root_is_display_only() {
        let ui = map_snapshot(&fresh_snapshot(vec![status(library_id(1))]));
        assert_eq!(ui.libraries[0].root_label, "Folder available");
        assert!(!ui.libraries[0].label.contains('/'));
    }

    #[test]
    fn ui3_unavailable_root_is_degraded_without_deletion_language() {
        let mut item = status(library_id(2));
        item.root_state = DesktopControllerRootState::Unavailable;
        let ui = map_snapshot(&fresh_snapshot(vec![item]));
        assert_eq!(ui.libraries[0].root_label, "Folder unavailable");
        assert!(!ui.libraries[0].can_sync);
        assert!(!ui.libraries[0].root_label.contains("delete"));
    }

    #[test]
    fn ui4_recovering_root_is_explicitly_checking() {
        let mut item = status(library_id(3));
        item.root_state = DesktopControllerRootState::Recovering;
        let ui = map_snapshot(&fresh_snapshot(vec![item]));
        assert_eq!(ui.libraries[0].root_label, "Checking folder changes");
    }

    #[test]
    fn recovery_ui_separates_action_required_from_waiting() {
        let mut root = status(library_id(31));
        root.root_state = DesktopControllerRootState::Unavailable;
        root.runtime_state = DesktopControllerRuntimeState::RootBlocked;
        let mut server = status(library_id(32));
        server.runtime_state = DesktopControllerRuntimeState::BackingOff;
        server.last_outcome = Some(DesktopControllerSyncOutcome::ServerTransient);
        let ui = map_snapshot(&authenticated_snapshot(vec![root, server]));
        assert_eq!(ui.recovery_action_required, 1);
        assert_eq!(ui.recovery_waiting_count, 1);
        assert_eq!(ui.recovery_items.len(), 2);
        assert!(ui
            .recovery_items
            .iter()
            .any(|item| item.kind_code == "root_unavailable" && item.action_required));
        assert!(ui
            .recovery_items
            .iter()
            .any(|item| item.kind_code == "server_retryable" && item.waiting));
    }

    #[test]
    fn recovery_ui_composes_with_attention_without_duplicate_conflict_controls() {
        let mut root = status(library_id(33));
        root.root_state = DesktopControllerRootState::Unavailable;
        root.runtime_state = DesktopControllerRuntimeState::RootBlocked;
        let mut snapshot = authenticated_snapshot(vec![root]);
        snapshot.attention.summary = synveil_client::DesktopControllerAttentionSummary {
            total_count: 2,
            conflict_count: 2,
            other_count: 0,
        };
        let ui = map_snapshot(&snapshot);
        assert_eq!(ui.attention_count, 2);
        assert_eq!(ui.conflict_attention_count, 2);
        assert_eq!(ui.recovery_action_required, 1);
        assert!(ui
            .recovery_items
            .iter()
            .all(|item| item.kind_code != "conflict"));
    }

    #[test]
    fn recovery_ui_preserves_generation_and_exposes_only_fixed_actions() {
        let mut item = status(library_id(34));
        item.auth_state = DesktopControllerAuthState::Blocked;
        let ui = map_snapshot(&authenticated_snapshot(vec![item]));
        assert_eq!(ui.recovery_items[0].connection_generation, 2);
        assert_eq!(ui.recovery_items[0].action_code, Some("authenticate"));
        let rendered = format!("{ui:?}");
        assert!(!rendered.contains("/private"));
        assert!(!rendered.contains("root_path"));
        assert!(!rendered.contains("credential"));
    }

    #[test]
    fn recovery_ui_uses_same_setup_surface_for_an_empty_authenticated_profile() {
        let ui = map_snapshot(&authenticated_snapshot(Vec::new()));
        assert_eq!(ui.recovery_action_required, 0);
        assert!(ui.recovery_items.is_empty());
        assert_eq!(
            ui.empty_library_message,
            "Set up a library to start syncing."
        );
    }

    #[test]
    fn ui5_auth_blocked_has_no_credential_surface() {
        let mut item = status(library_id(4));
        item.auth_state = DesktopControllerAuthState::Blocked;
        let ui = map_snapshot(&fresh_snapshot(vec![item]));
        assert_eq!(ui.libraries[0].auth_label, "Authentication blocked");
        assert!(!ui.libraries[0].auth_label.contains("credential"));
    }

    #[test]
    fn ui6_missing_auth_requires_attention() {
        let mut item = status(library_id(5));
        item.auth_state = DesktopControllerAuthState::Missing;
        let ui = map_snapshot(&fresh_snapshot(vec![item]));
        assert!(ui.libraries[0].needs_attention);
        assert!(!ui.libraries[0].can_sync);
    }

    #[test]
    fn ui7_ready_auth_can_enable_sync() {
        let ui = map_snapshot(&fresh_snapshot(vec![status(library_id(6))]));
        assert!(ui.libraries[0].can_sync);
    }

    #[test]
    fn ui7a_user_pause_is_global_and_disables_library_sync_actions() {
        let mut snapshot = fresh_snapshot(vec![status(library_id(60))]);
        snapshot.sync_control_state = DesktopControllerSyncControlState::PausedByUser;
        let ui = map_snapshot(&snapshot);
        assert_eq!(ui.sync_control_code, "paused");
        assert_eq!(ui.sync_control_label, "Paused by user");
        assert!(!ui.can_sync_any);
        assert!(!ui.libraries[0].can_sync);
    }

    #[test]
    fn ui8_required_conflict_is_needs_attention() {
        let mut item = status(library_id(7));
        item.conflict_state = DesktopControllerConflictState::Required;
        let ui = map_snapshot(&fresh_snapshot(vec![item]));
        assert_eq!(ui.libraries[0].conflict_label, "Needs attention");
        assert!(!ui.libraries[0].can_sync);
    }

    #[test]
    fn ui9_clear_conflict_is_not_a_completion_claim() {
        let ui = map_snapshot(&fresh_snapshot(vec![status(library_id(8))]));
        assert_eq!(ui.libraries[0].conflict_label, "No conflict reported");
        assert_eq!(ui.libraries[0].outcome_label, "No recent sync result");
    }

    #[test]
    fn ui10_stale_snapshot_retains_safe_library_data() {
        let mut snapshot = fresh_snapshot(vec![status(library_id(9))]);
        snapshot.connection_state = DesktopControllerConnectionState::Reconnecting;
        snapshot.freshness = DesktopControllerFreshness::Stale;
        let ui = map_snapshot(&snapshot);
        assert_eq!(ui.libraries.len(), 1);
        assert_eq!(ui.freshness_code, "stale");
        assert!(!ui.can_sync_any);
    }

    #[test]
    fn ui11_fresh_revision_and_generation_are_preserved() {
        let ui = map_snapshot(&fresh_snapshot(vec![]));
        assert_eq!(ui.revision, 4);
        assert_eq!(ui.connection_generation, 2);
    }

    #[test]
    fn ui12_disconnected_disables_sync() {
        let mut snapshot = fresh_snapshot(vec![status(library_id(10))]);
        snapshot.connection_state = DesktopControllerConnectionState::Disconnected;
        snapshot.freshness = DesktopControllerFreshness::Unavailable;
        let ui = map_snapshot(&snapshot);
        assert!(!ui.libraries[0].can_sync);
    }

    #[test]
    fn ui13_current_connected_status_enables_only_eligible_library() {
        let mut blocked = status(library_id(12));
        blocked.runtime_state = DesktopControllerRuntimeState::RootBlocked;
        let ui = map_snapshot(&fresh_snapshot(vec![status(library_id(11)), blocked]));
        assert!(ui.libraries[0].can_sync);
        assert!(!ui.libraries[1].can_sync);
    }

    #[test]
    fn ui14_idle_is_narrow_status_not_everything_synced() {
        let ui = map_snapshot(&fresh_snapshot(vec![status(library_id(13))]));
        assert_eq!(ui.libraries[0].runtime_label, "Idle");
        assert!(!ui.libraries[0].runtime_label.contains("synced"));
    }

    #[test]
    fn ui15_progress_is_not_completion() {
        let mut item = status(library_id(14));
        item.last_outcome = Some(DesktopControllerSyncOutcome::Progress);
        let ui = map_snapshot(&fresh_snapshot(vec![item]));
        assert_eq!(ui.libraries[0].outcome_label, "Sync in progress");
        assert_ne!(ui.libraries[0].outcome_label, "Sync completed");
    }

    #[test]
    fn ui16_library_order_is_stable_by_id() {
        let first = library_id(16);
        let second = library_id(15);
        let ui = map_snapshot(&fresh_snapshot(vec![
            status(first.clone()),
            status(second.clone()),
        ]));
        assert_eq!(ui.libraries[0].library_id, second);
        assert_eq!(ui.libraries[1].library_id, first);
    }

    #[test]
    fn ui17_duplicate_library_ids_are_not_duplicated_in_model() {
        let id = library_id(17);
        let mut second = status(id.clone());
        second.wake_pending = true;
        let ui = map_snapshot(&fresh_snapshot(vec![status(id), second]));
        assert_eq!(ui.libraries.len(), 1);
        assert!(!ui.libraries[0].wake_pending);
    }

    #[test]
    fn ui18_invalid_library_ids_are_not_exposed() {
        let ui = map_snapshot(&fresh_snapshot(vec![status("/private/root".to_owned())]));
        assert!(ui.libraries.is_empty());
    }

    #[test]
    fn ui19_controller_truncation_is_preserved() {
        let mut snapshot = fresh_snapshot(vec![status(library_id(19))]);
        snapshot.libraries_truncated = true;
        let ui = map_snapshot(&snapshot);
        assert!(ui.libraries_truncated);
    }

    #[test]
    fn ui20_errors_are_generic_and_stable() {
        let snapshot = DesktopControllerSnapshot {
            last_error: Some(DesktopControllerErrorKind::MalformedServerResponse),
            ..DesktopControllerSnapshot::default()
        };
        let ui = map_snapshot(&snapshot);
        assert_eq!(
            ui.last_error_label,
            "The background service returned an unusable status."
        );
        assert!(!ui.last_error_label.contains("Debug"));
        assert!(!ui.last_error_label.contains("/"));
    }

    #[test]
    fn command_feedback_never_claims_completion() {
        assert_eq!(
            command_feedback(DesktopControllerCommandResult::Accepted),
            "Sync requested; completion will appear in status."
        );
        assert!(!command_feedback(DesktopControllerCommandResult::Accepted).contains("completed"));
    }

    #[test]
    fn authentication_feedback_is_typed_generic_and_non_echoing() {
        let cases = [
            (
                DesktopControllerCommandResult::Authenticated,
                "This device is signed in.",
            ),
            (
                DesktopControllerCommandResult::SignedOut,
                "This device is signed out.",
            ),
            (
                DesktopControllerCommandResult::InvalidCredentials,
                "That device code wasn't accepted. Check the code and try again.",
            ),
            (
                DesktopControllerCommandResult::NetworkUnavailable,
                "You're offline. Check your network and try again.",
            ),
            (
                DesktopControllerCommandResult::ServerUnavailable,
                "Synveil couldn't reach the server right now.",
            ),
            (
                DesktopControllerCommandResult::RateLimited,
                "Too many attempts. Wait a moment and try again.",
            ),
            (
                DesktopControllerCommandResult::SecureStoreUnavailable,
                "Secure credential storage isn't available right now.",
            ),
            (
                DesktopControllerCommandResult::Busy,
                "Sign-in is already in progress.",
            ),
            (
                DesktopControllerCommandResult::OutcomeUnknown,
                "Synveil is checking whether this device was signed in.",
            ),
            (
                DesktopControllerCommandResult::ProtocolError,
                "This Synveil installation couldn't complete sign-in.",
            ),
        ];

        for (result, expected) in cases {
            let feedback = auth_feedback(result);
            assert_eq!(feedback, expected);
            assert!(!feedback.contains("synthetic-enrollment-token"));
            assert!(!feedback.contains("/private/"));
        }
    }

    #[test]
    fn authentication_results_have_stable_state_and_allowed_action() {
        use DesktopControllerCommandResult as Result;
        let cases = [
            (Result::Authenticated, "authenticated", Some("sign_out")),
            (Result::SignedOut, "sign_in_required", Some("sign_in")),
            (Result::InvalidCredentials, "invalid_code", Some("sign_in")),
            (
                Result::NetworkUnavailable,
                "network_unavailable",
                Some("sign_in"),
            ),
            (
                Result::ServerUnavailable,
                "server_unavailable",
                Some("sign_in"),
            ),
            (Result::RateLimited, "rate_limited", Some("sign_in")),
            (
                Result::SecureStoreUnavailable,
                "secure_storage_unavailable",
                Some("sign_in"),
            ),
            (Result::Busy, "busy", None),
            (Result::ProtocolError, "protocol_problem", Some("sign_in")),
            (Result::OutcomeUnknown, "reconciling", None),
        ];
        for (result, code, action) in cases {
            let presented = authentication_presentation(result);
            assert_eq!(presented.code, code);
            assert_eq!(presented.action, action);
            assert!(!presented
                .message
                .to_ascii_lowercase()
                .contains("enrollment"));
            assert!(!presented.message.contains("synthetic-device-code-canary"));
        }
    }

    #[test]
    fn shutdown_result_is_not_a_desktop_shell_message() {
        assert_eq!(
            command_feedback(DesktopControllerCommandResult::ShutdownAccepted),
            "Synveil cannot confirm whether that completed. It is checking the current status; wait for the update."
        );
    }

    #[test]
    fn profile_id_test_dependency_is_used_only_for_id_generation() {
        let _ = ServerProfileId::new();
    }

    #[test]
    fn selection_is_retained_for_a_present_library_and_cleared_when_removed() {
        let first_id = library_id(21);
        let second_id = library_id(22);
        let initial = map_snapshot(&fresh_snapshot(vec![
            status(first_id.clone()),
            status(second_id.clone()),
        ]));
        assert_eq!(
            selected_library_id(Some(&first_id), &initial),
            Some(first_id.clone())
        );

        let removed = map_snapshot(&fresh_snapshot(vec![status(second_id.clone())]));
        assert_eq!(selected_library_id(Some(&first_id), &removed), None);
    }

    #[test]
    fn snapshot_mapping_handles_ten_thousand_fresh_revisions() {
        let id = library_id(23);
        let mut snapshot = fresh_snapshot(vec![status(id)]);
        for revision in 1..=10_000 {
            snapshot.revision = revision;
            let ui = map_snapshot(&snapshot);
            assert_eq!(ui.revision, revision);
            assert_eq!(ui.libraries.len(), 1);
            assert!(!ui.libraries[0].label.contains('/'));
        }
    }

    #[test]
    fn snapshot_churn_never_exposes_an_unbounded_model() {
        for revision in 0..10_000_u64 {
            let statuses = (0..u128::from(revision % 64))
                .map(|offset| status(library_id(24 + offset)))
                .collect();
            let mut snapshot = fresh_snapshot(statuses);
            snapshot.revision = revision;
            let ui = map_snapshot(&snapshot);
            assert!(ui.libraries.len() <= MAX_PRESENTED_LIBRARY_ROWS);
        }
    }

    #[test]
    fn thousand_library_model_has_stable_order_and_safe_labels() {
        let statuses = (0..1_000_u128)
            .map(|seed| status(library_id(100_000 + seed)))
            .collect();
        let ui = map_snapshot(&fresh_snapshot(statuses));
        assert_eq!(ui.libraries.len(), 1_000);
        assert!(ui
            .libraries
            .windows(2)
            .all(|pair| pair[0].library_id < pair[1].library_id));
        assert!(ui
            .libraries
            .iter()
            .all(|library| !library.label.contains('/') && !library.label.contains("\\")));
    }

    #[test]
    fn safe_projection_contains_no_raw_error_or_root_value() {
        let mut snapshot = DesktopControllerSnapshot {
            last_error: Some(DesktopControllerErrorKind::MalformedServerResponse),
            ..DesktopControllerSnapshot::default()
        };
        snapshot.libraries.push(status("/private/root".to_owned()));
        let ui = map_snapshot(&snapshot);
        let rendered = format!("{ui:?}");
        assert!(!rendered.contains("/private/root"));
        assert!(!rendered.contains("Debug"));
    }

    #[test]
    fn welcome_routing_uses_authoritative_client_and_host_state() {
        use super::WelcomeDestination as Destination;

        assert_eq!(
            welcome_destination(false, false, HostSetupState::NotStarted),
            Destination::Initializing
        );
        assert_eq!(
            welcome_destination(true, false, HostSetupState::NotStarted),
            Destination::Welcome
        );
        assert_eq!(
            welcome_destination(true, true, HostSetupState::NotStarted),
            Destination::ExistingClient
        );
        assert_eq!(
            welcome_destination(true, true, HostSetupState::NeedsRepair),
            Destination::ExistingClient
        );
        assert_eq!(
            welcome_destination(true, false, HostSetupState::StorageReady),
            Destination::HostResume
        );
        assert_eq!(
            welcome_destination(true, false, HostSetupState::Ready),
            Destination::HostReady
        );
        assert_eq!(
            welcome_destination(true, false, HostSetupState::NeedsRepair),
            Destination::HostNeedsAttention
        );
        assert_eq!(
            welcome_destination(true, false, HostSetupState::Blocked),
            Destination::Welcome
        );
    }

    #[test]
    fn configured_signed_out_and_zero_library_clients_bypass_welcome() {
        // Authentication and library state intentionally are not routing inputs.
        assert_eq!(
            welcome_destination(true, true, HostSetupState::NotStarted),
            WelcomeDestination::ExistingClient
        );
    }

    #[test]
    fn p041_fresh_setup_evidence_completes_only_setup_stages() {
        let ui = map_snapshot(&authenticated_snapshot(vec![status(library_id(41))]));
        for id in ["app_ready", "server_ready", "signed_in", "library_ready"] {
            assert_eq!(
                progress_stage_for(&ui.first_run_progress, id).state,
                FirstRunProgressStageState::Complete,
                "{id}"
            );
        }
        assert!(!ui.first_run_progress.visible);
    }

    #[test]
    fn p041_welcome_and_connection_states_do_not_complete_later_stages() {
        let welcome = map_snapshot(&fresh_snapshot(Vec::new()));
        assert_eq!(
            progress_stage_for(&welcome.first_run_progress, "server_ready").state,
            FirstRunProgressStageState::ActionRequired
        );
        assert_eq!(
            progress_stage_for(&welcome.first_run_progress, "signed_in").state,
            FirstRunProgressStageState::Pending
        );
        assert_eq!(
            progress_stage_for(&welcome.first_run_progress, "library_ready").state,
            FirstRunProgressStageState::Pending
        );

        let mut connecting = DesktopControllerSnapshot::default();
        connecting.connection_state = DesktopControllerConnectionState::Connecting;
        connecting.freshness = DesktopControllerFreshness::Unavailable;
        let progress = first_run_progress(&connecting);
        assert_eq!(
            progress_stage_for(&progress, "server_ready").state,
            FirstRunProgressStageState::Pending
        );
    }

    #[test]
    fn p041_authentication_and_library_setup_are_action_required() {
        let mut unauthenticated = fresh_snapshot(Vec::new());
        unauthenticated.profile_configured = true;
        let progress = first_run_progress(&unauthenticated);
        assert_eq!(
            progress_stage_for(&progress, "signed_in").state,
            FirstRunProgressStageState::ActionRequired
        );
        assert_eq!(
            progress_stage_for(&progress, "signed_in").action,
            Some("sign_in")
        );

        let progress = first_run_progress(&authenticated_snapshot(Vec::new()));
        assert_eq!(
            progress_stage_for(&progress, "library_ready").state,
            FirstRunProgressStageState::ActionRequired
        );
        assert_eq!(
            progress_stage_for(&progress, "library_ready").action,
            Some("resume_setup")
        );
    }

    #[test]
    fn p041_library_confirmation_does_not_complete_first_sync() {
        let mut item = status(library_id(42));
        item.first_sync_completed = false;
        let progress = first_run_progress(&authenticated_snapshot(vec![item]));
        assert_eq!(
            progress_stage_for(&progress, "library_ready").state,
            FirstRunProgressStageState::Complete
        );
        assert_ne!(
            progress_stage_for(&progress, "first_sync").state,
            FirstRunProgressStageState::Complete
        );
        assert!(!progress.status.contains('%'));
    }

    #[test]
    fn p041_running_scheduled_and_progress_states_are_not_completion() {
        let mut scheduled = status(library_id(43));
        scheduled.first_sync_completed = false;
        scheduled.runtime_state = DesktopControllerRuntimeState::Scheduled;
        assert_eq!(
            first_sync_stage_for(&authenticated_snapshot(vec![scheduled])).state,
            FirstRunProgressStageState::Waiting
        );

        let mut running = status(library_id(44));
        running.first_sync_completed = false;
        running.runtime_state = DesktopControllerRuntimeState::Running;
        let stage = first_sync_stage_for(&authenticated_snapshot(vec![running]));
        assert_eq!(stage.state, FirstRunProgressStageState::Active);
        assert_eq!(stage.detail, "Syncing your files…");

        let mut progress = status(library_id(45));
        progress.first_sync_completed = false;
        progress.last_outcome = Some(DesktopControllerSyncOutcome::Progress);
        let stage = first_sync_stage_for(&authenticated_snapshot(vec![progress]));
        assert_eq!(stage.state, FirstRunProgressStageState::Active);
        assert_ne!(stage.state, FirstRunProgressStageState::Complete);
    }

    #[test]
    fn p041_idle_requires_quiescent_evidence_and_no_follow_up_wake() {
        let complete = authenticated_snapshot(vec![status(library_id(46))]);
        assert!(first_sync_completion_proven(&complete));
        assert_eq!(
            first_sync_stage_for(&complete).state,
            FirstRunProgressStageState::Complete
        );

        let mut follow_up = complete.clone();
        follow_up.libraries[0].wake_pending = true;
        assert!(!first_sync_completion_proven(&follow_up));
        assert_ne!(
            first_sync_stage_for(&follow_up).state,
            FirstRunProgressStageState::Complete
        );

        let mut more_work = complete.clone();
        more_work.libraries[0].runtime_state = DesktopControllerRuntimeState::Scheduled;
        more_work.libraries[0].last_outcome = Some(DesktopControllerSyncOutcome::Progress);
        assert!(!first_sync_completion_proven(&more_work));
    }

    #[test]
    fn p041_stale_or_disconnected_state_waits_without_advancing_sync() {
        let mut stale = authenticated_snapshot(vec![status(library_id(47))]);
        stale.libraries[0].first_sync_completed = false;
        stale.connection_state = DesktopControllerConnectionState::Reconnecting;
        stale.freshness = DesktopControllerFreshness::Stale;
        assert_eq!(
            first_sync_stage_for(&stale).state,
            FirstRunProgressStageState::Waiting
        );
        assert!(!first_sync_completion_proven(&stale));

        stale.connection_state = DesktopControllerConnectionState::Disconnected;
        stale.freshness = DesktopControllerFreshness::Unavailable;
        assert_eq!(
            first_sync_stage_for(&stale).detail,
            "Waiting for connection…"
        );
    }

    #[test]
    fn p041_waiting_and_action_required_sync_states_remain_distinct() {
        let mut backoff = status(library_id(48));
        backoff.first_sync_completed = false;
        backoff.runtime_state = DesktopControllerRuntimeState::BackingOff;
        backoff.last_outcome = Some(DesktopControllerSyncOutcome::ServerTransient);
        assert_eq!(
            first_sync_stage_for(&authenticated_snapshot(vec![backoff])).state,
            FirstRunProgressStageState::Waiting
        );

        let mut auth = status(library_id(49));
        auth.first_sync_completed = false;
        auth.auth_state = DesktopControllerAuthState::Missing;
        auth.runtime_state = DesktopControllerRuntimeState::AuthBlocked;
        let auth_stage = first_sync_stage_for(&authenticated_snapshot(vec![auth]));
        assert_eq!(auth_stage.state, FirstRunProgressStageState::ActionRequired);
        assert_eq!(auth_stage.action, Some("sign_in"));

        let mut conflict = status(library_id(50));
        conflict.first_sync_completed = false;
        conflict.conflict_state = DesktopControllerConflictState::Required;
        let conflict_stage = first_sync_stage_for(&authenticated_snapshot(vec![conflict]));
        assert_eq!(
            conflict_stage.state,
            FirstRunProgressStageState::ActionRequired
        );
        assert_eq!(conflict_stage.action, Some("resolve_conflict"));

        let mut paused = authenticated_snapshot(vec![status(library_id(51))]);
        paused.libraries[0].first_sync_completed = false;
        paused.sync_control_state = DesktopControllerSyncControlState::PausedByUser;
        let paused_stage = first_sync_stage_for(&paused);
        assert_eq!(
            paused_stage.state,
            FirstRunProgressStageState::ActionRequired
        );
        assert_eq!(paused_stage.action, Some("resume_sync"));
    }

    #[test]
    fn p041_root_and_fatal_states_never_look_synced() {
        let mut root = status(library_id(52));
        root.first_sync_completed = false;
        root.root_state = DesktopControllerRootState::Unavailable;
        root.runtime_state = DesktopControllerRuntimeState::RootBlocked;
        let root_stage = first_sync_stage_for(&authenticated_snapshot(vec![root]));
        assert_eq!(root_stage.state, FirstRunProgressStageState::ActionRequired);
        assert_eq!(root_stage.action, Some("check_again"));

        let mut fatal = status(library_id(53));
        fatal.first_sync_completed = false;
        fatal.runtime_state = DesktopControllerRuntimeState::Faulted;
        fatal.last_outcome = Some(DesktopControllerSyncOutcome::FatalLocal);
        let fatal_stage = first_sync_stage_for(&authenticated_snapshot(vec![fatal]));
        assert_eq!(
            fatal_stage.state,
            FirstRunProgressStageState::ActionRequired
        );
        assert_ne!(fatal_stage.state, FirstRunProgressStageState::Complete);
    }

    #[test]
    fn p041_ordering_guard_rejects_old_revision_and_generation() {
        let current = map_snapshot(&authenticated_snapshot(vec![status(library_id(54))]));
        let mut older_revision = current.clone();
        older_revision.revision = current.revision.saturating_sub(1);
        assert!(!snapshot_is_acceptable(&current, &older_revision));

        let mut older_generation = current.clone();
        older_generation.connection_generation = current.connection_generation.saturating_sub(1);
        older_generation.revision = current.revision.saturating_add(100);
        assert!(!snapshot_is_acceptable(&current, &older_generation));
        assert!(snapshot_is_acceptable(&current, &current));
    }

    #[test]
    fn p041_stage_codes_and_copy_are_bounded_and_accessible() {
        let progress = first_run_progress(&authenticated_snapshot(vec![status(library_id(55))]));
        let ids = progress
            .stages
            .iter()
            .map(|stage| stage.id)
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "app_ready",
                "server_ready",
                "signed_in",
                "library_ready",
                "first_sync"
            ]
        );
        assert!(progress.stages.iter().all(|stage| {
            !stage.title.contains("rebaseline")
                && !stage.detail.contains("checkpoint")
                && !stage.detail.contains("runtime")
                && !stage.detail.contains("SQLite")
        }));
    }
}
