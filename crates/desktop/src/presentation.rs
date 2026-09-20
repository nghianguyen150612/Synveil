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

/// Presentation guard independent of the controller's own protocol bound.
pub const MAX_PRESENTED_LIBRARY_ROWS: usize = 2_048;

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiRecoveryItem {
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
    pub recovery_items: Vec<UiRecoveryItem>,
    pub recovery_items_truncated: bool,
    pub recovery_action_required: usize,
    pub recovery_waiting_count: usize,
    pub client_recovery_code: &'static str,
    pub client_recovery_label: &'static str,
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
    let recovery_items = recovery
        .items
        .iter()
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
        recovery_items,
        recovery_items_truncated: recovery.truncated,
        recovery_action_required: bounded_usize(recovery.total_action_required),
        recovery_waiting_count: bounded_usize(recovery.total_waiting),
        client_recovery_code,
        client_recovery_label,
    }
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
    let (action_code, action_label) = item
        .action
        .map(recovery_action_presentation)
        .unwrap_or((None, ""));
    UiRecoveryItem {
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
            "The configured folder is unavailable. Synveil has not treated it as empty.",
        ),
        DesktopControllerRecoveryKind::ServerRetryable => (
            "server_retryable",
            "Waiting for server",
            "Synveil will retry automatically; a check now remains bounded.",
        ),
        DesktopControllerRecoveryKind::LocalFailure => (
            "local_failure",
            "Local sync needs attention",
            "A local sync condition needs a safe retry or authoritative recheck.",
        ),
        DesktopControllerRecoveryKind::LibrarySetupIncomplete => (
            "library_setup_incomplete",
            "Library setup incomplete",
            "Resume setup with the same folder so the existing identity can be reconciled safely.",
        ),
    }
}

fn recovery_action_presentation(
    action: DesktopControllerRecoveryAction,
) -> (Option<&'static str>, &'static str) {
    match action {
        DesktopControllerRecoveryAction::StartClient => (Some("start_client"), "Start client"),
        DesktopControllerRecoveryAction::ConfigureProfile => {
            (Some("configure_profile"), "Configure connection")
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
        DesktopControllerCommandResult::OutcomeUnknown => "Request status is unknown.",
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
        DesktopControllerCommandResult::ShutdownAccepted => "Request status is unknown.",
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
            "Sync setting status is unknown; refreshing status."
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

/// Map library onboarding results to bounded, generic UI copy. The selected
/// folder and remote diagnostics are intentionally not echoed here.
#[must_use]
pub const fn library_setup_feedback(result: DesktopControllerCommandResult) -> &'static str {
    match result {
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
        DesktopControllerCommandResult::PersistenceFailure => {
            "The library could not be saved locally."
        }
        DesktopControllerCommandResult::Busy => "Another request is already being processed.",
        DesktopControllerCommandResult::OutcomeUnknown => {
            "Request status is unknown; refresh status before trying again."
        }
        DesktopControllerCommandResult::Unavailable
        | DesktopControllerCommandResult::Disconnected
        | DesktopControllerCommandResult::AlreadyUnavailable => "Background service unavailable.",
        DesktopControllerCommandResult::Stopped
        | DesktopControllerCommandResult::RuntimeStopped => "Background service is stopped.",
        DesktopControllerCommandResult::AdmissionLimited => "Try again in a moment.",
        DesktopControllerCommandResult::ProtocolError => "Background service is incompatible.",
        _ => "Library setup could not be completed.",
    }
}

/// Map authentication outcomes to safe, generic UI copy. No server detail,
/// credential, token, path, or response body is included.
#[must_use]
pub const fn auth_feedback(result: DesktopControllerCommandResult) -> &'static str {
    match result {
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
        DesktopControllerCommandResult::LibraryConfigured
        | DesktopControllerCommandResult::LibraryAlreadyConfigured
        | DesktopControllerCommandResult::InvalidLibraryName
        | DesktopControllerCommandResult::InvalidLibraryRoot
        | DesktopControllerCommandResult::AuthenticationRequired
        | DesktopControllerCommandResult::ServerIdentityConflict => {
            "Library setup status is unavailable."
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
        DesktopControllerCommandResult::Busy => {
            "An authentication request is already being processed."
        }
        DesktopControllerCommandResult::OutcomeUnknown => {
            "Authentication result is unknown; refreshing status."
        }
        DesktopControllerCommandResult::ProtocolError => "Background service is incompatible.",
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
        | DesktopControllerCommandResult::ShutdownAccepted => "Request status is unknown.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use synveil_client::{DesktopControllerProcessStatus, DesktopProcessStatus, ServerProfileId};

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
        assert_eq!(ui.recovery_action_required, 1);
        assert_eq!(ui.recovery_items[0].kind_code, "library_setup_incomplete");
        assert_eq!(ui.recovery_items[0].action_code, Some("resume_setup"));
        assert!(ui.recovery_items[0].detail.contains("same folder"));
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
                "Signed in; status will update.",
            ),
            (
                DesktopControllerCommandResult::SignedOut,
                "Signed out; local credential removed.",
            ),
            (
                DesktopControllerCommandResult::InvalidCredentials,
                "The enrollment token was not accepted.",
            ),
            (
                DesktopControllerCommandResult::NetworkUnavailable,
                "Network unavailable. Try again later.",
            ),
            (
                DesktopControllerCommandResult::ServerUnavailable,
                "The server is unavailable. Try again later.",
            ),
            (
                DesktopControllerCommandResult::RateLimited,
                "Too many attempts. Try again later.",
            ),
            (
                DesktopControllerCommandResult::SecureStoreUnavailable,
                "Secure credential storage is unavailable.",
            ),
            (
                DesktopControllerCommandResult::Busy,
                "An authentication request is already being processed.",
            ),
            (
                DesktopControllerCommandResult::OutcomeUnknown,
                "Authentication result is unknown; refreshing status.",
            ),
            (
                DesktopControllerCommandResult::ProtocolError,
                "Background service is incompatible.",
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
    fn shutdown_result_is_not_a_desktop_shell_message() {
        assert_eq!(
            command_feedback(DesktopControllerCommandResult::ShutdownAccepted),
            "Request status is unknown."
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
}
