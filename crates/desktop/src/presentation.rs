//! Pure presentation mapping for the redacted Prompt 97 controller model.
//!
//! The output is deliberately made from stable codes, generic labels, and
//! bounded values.  It has no field for a root path, server URL, credential,
//! cookie, authorization header, file content, or raw transport diagnostic.

use std::collections::BTreeMap;

use synveil_client::{
    DesktopControllerAuthState, DesktopControllerCommandResult, DesktopControllerConflictState,
    DesktopControllerConnectionState, DesktopControllerErrorKind, DesktopControllerFreshness,
    DesktopControllerLibraryStatus, DesktopControllerRootState, DesktopControllerRuntimeState,
    DesktopControllerSnapshot, DesktopControllerSyncOutcome, LibraryId,
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
pub struct UiSnapshot {
    pub connection_code: &'static str,
    pub connection_label: &'static str,
    pub connection_detail: &'static str,
    pub freshness_code: &'static str,
    pub freshness_label: &'static str,
    pub process_code: &'static str,
    pub process_label: &'static str,
    pub process_control_ready: bool,
    pub libraries: Vec<UiLibrary>,
    pub libraries_truncated: bool,
    pub revision: u64,
    pub connection_generation: u64,
    pub last_error_code: Option<&'static str>,
    pub last_error_label: &'static str,
    pub attention_count: usize,
    pub root_unavailable_count: usize,
    pub can_sync_any: bool,
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

    let attention_count = libraries
        .iter()
        .filter(|library| library.needs_attention)
        .count();
    let root_unavailable_count = libraries
        .iter()
        .filter(|library| library.root_code != "available")
        .count();
    let can_sync_any = libraries.iter().any(|library| library.can_sync);

    let (connection_code, connection_label, connection_detail) = connection_presentation(
        snapshot.connection_state,
        snapshot.freshness,
        snapshot.last_error,
    );
    let (freshness_code, freshness_label) = freshness_presentation(snapshot.freshness);

    UiSnapshot {
        connection_code,
        connection_label,
        connection_detail,
        freshness_code,
        freshness_label,
        process_code,
        process_label,
        process_control_ready,
        libraries,
        libraries_truncated: snapshot.libraries_truncated || presented_truncated,
        revision: snapshot.revision,
        connection_generation: snapshot.connection_generation,
        last_error_code: snapshot.last_error.map(DesktopControllerErrorKind::code),
        last_error_label: snapshot
            .last_error
            .map_or("No connection error", |error| error_label(&error)),
        attention_count,
        root_unavailable_count,
        can_sync_any,
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
        DesktopControllerCommandResult::Disconnected
        | DesktopControllerCommandResult::Unavailable
        | DesktopControllerCommandResult::AlreadyUnavailable => "Background service unavailable.",
        DesktopControllerCommandResult::UnknownLibrary => "That library is no longer available.",
        DesktopControllerCommandResult::RuntimeStopped
        | DesktopControllerCommandResult::Stopped => "Background service is stopped.",
        DesktopControllerCommandResult::AdmissionLimited => "Try again in a moment.",
        DesktopControllerCommandResult::OutcomeUnknown => "Request status is unknown.",
        DesktopControllerCommandResult::ProtocolError => "Background service is incompatible.",
        // The desktop shell never exposes the Prompt 96 process-shutdown
        // command.  Keep this defensive mapping generic if a future caller
        // hands the presentation layer that result anyway.
        DesktopControllerCommandResult::ShutdownAccepted => "Request status is unknown.",
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
            revision: 4,
            freshness: DesktopControllerFreshness::Fresh,
            last_error: None,
            connection_generation: 2,
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
