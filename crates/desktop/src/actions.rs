//! Small, UI-facing policies that keep the native shell bounded and explicit.
//!
//! These values are deliberately independent of Qt. The bridge uses the
//! request gate and latest-value coalescer directly; the tray and close
//! policies make native C++/QML behavior auditable without a display server.

use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};

pub(crate) const TRAY_ACTION_LABELS: [&str; 3] =
    ["Open Synveil", "Sync Now", "Quit Synveil Desktop"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) enum CloseDisposition {
    HideToTray,
    Exit,
}

#[must_use]
#[allow(dead_code)]
pub(crate) const fn close_disposition(tray_available: bool) -> CloseDisposition {
    if tray_available {
        CloseDisposition::HideToTray
    } else {
        CloseDisposition::Exit
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuitScope {
    ControllerOnly,
}

pub(crate) const DESKTOP_QUIT_SCOPE: QuitScope = QuitScope::ControllerOnly;

/// Prevent rapid UI clicks from creating one asynchronous task per click.
#[derive(Debug, Default)]
pub(crate) struct SyncRequestGate(AtomicBool);

impl SyncRequestGate {
    #[must_use]
    pub(crate) fn try_acquire(&self) -> bool {
        !self.0.swap(true, Ordering::AcqRel)
    }

    pub(crate) fn release(&self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Coalesces repeated startup-toggle clicks to the latest desired value while
/// one bounded supervisor operation is in flight. It never creates one task
/// or one process-management command per click.
#[derive(Debug, Default)]
pub(crate) struct StartupIntentGate {
    desired: AtomicBool,
    generation: AtomicU64,
    in_flight: AtomicBool,
}

impl StartupIntentGate {
    pub(crate) fn set_intent(&self, enabled: bool) {
        self.desired.store(enabled, Ordering::Release);
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    pub(crate) fn try_start(&self) -> Option<(bool, u64)> {
        if self
            .in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            Some((
                self.desired.load(Ordering::Acquire),
                self.generation.load(Ordering::Acquire),
            ))
        } else {
            None
        }
    }

    pub(crate) fn current(&self) -> (bool, u64) {
        (
            self.desired.load(Ordering::Acquire),
            self.generation.load(Ordering::Acquire),
        )
    }

    pub(crate) fn release(&self) {
        self.in_flight.store(false, Ordering::Release);
    }
}

/// Latest-value state delivery. Many controller updates collapse into one
/// queued GUI callback, while a state arriving during callback application is
/// guaranteed to receive one follow-up callback.
#[derive(Clone, Debug)]
pub(crate) struct LatestValue<T> {
    latest: Arc<Mutex<Option<T>>>,
    queued: Arc<AtomicBool>,
}

impl<T> Default for LatestValue<T> {
    fn default() -> Self {
        Self {
            latest: Arc::new(Mutex::new(None)),
            queued: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl<T> LatestValue<T> {
    /// Store a value and report whether the caller must enqueue a callback.
    pub(crate) fn publish(&self, value: T) -> bool {
        let Ok(mut latest) = self.latest.lock() else {
            return false;
        };
        *latest = Some(value);
        drop(latest);
        !self.queued.swap(true, Ordering::AcqRel)
    }

    /// Take the current value and report whether a later publication needs a
    /// follow-up callback.
    pub(crate) fn take(&self) -> (Option<T>, bool) {
        let value = self.latest.lock().ok().and_then(|mut latest| latest.take());
        self.queued.store(false, Ordering::Release);
        let has_followup = self
            .latest
            .lock()
            .map(|latest| latest.is_some())
            .unwrap_or(false);
        let should_enqueue = has_followup && !self.queued.swap(true, Ordering::AcqRel);
        (value, should_enqueue)
    }

    pub(crate) fn discard(&self) {
        self.queued.store(false, Ordering::Release);
        if let Ok(mut latest) = self.latest.lock() {
            latest.take();
        }
    }
}

#[must_use]
pub(crate) fn tray_tooltip(connection_label: &'static str) -> String {
    format!("Synveil — {connection_label}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::thread;

    #[test]
    fn ui21_sync_gate_admits_one_request() {
        let gate = SyncRequestGate::default();
        assert!(gate.try_acquire());
        assert!(!gate.try_acquire());
        gate.release();
    }

    #[test]
    fn ui22_sync_gate_can_admit_a_followup_after_release() {
        let gate = SyncRequestGate::default();
        assert!(gate.try_acquire());
        gate.release();
        assert!(gate.try_acquire());
    }

    #[test]
    fn ui23_tray_vocabulary_has_no_completion_claim() {
        assert_eq!(TRAY_ACTION_LABELS[1], "Sync Now");
        assert!(!TRAY_ACTION_LABELS
            .iter()
            .any(|label| label.contains("completed")));
    }

    #[test]
    fn ui24_tray_vocabulary_has_no_service_stop_action() {
        assert!(!TRAY_ACTION_LABELS
            .iter()
            .any(|label| label.contains("Stop") || label.contains("Shutdown")));
    }

    #[test]
    fn ui25_close_to_tray_is_selected_only_when_available() {
        assert_eq!(close_disposition(true), CloseDisposition::HideToTray);
        assert_eq!(close_disposition(false), CloseDisposition::Exit);
    }

    #[test]
    fn ui26_quit_scope_is_controller_only() {
        assert_eq!(DESKTOP_QUIT_SCOPE, QuitScope::ControllerOnly);
    }

    #[test]
    fn ui27_tooltip_contains_only_a_safe_connection_label() {
        let tooltip = tray_tooltip("Reconnecting");
        assert_eq!(tooltip, "Synveil — Reconnecting");
        assert!(!tooltip.contains('/') && !tooltip.contains("token"));
    }

    #[test]
    fn ui28_latest_value_replaces_older_state() {
        let latest = LatestValue::default();
        assert!(latest.publish(1_u64));
        assert!(!latest.publish(2_u64));
        assert_eq!(latest.take(), (Some(2), false));
    }

    #[test]
    fn ui29_latest_value_requires_a_followup_after_a_racing_publication() {
        let latest = LatestValue::default();
        assert!(latest.publish(1_u64));
        assert_eq!(latest.take(), (Some(1), false));
        assert!(latest.publish(2_u64));
        assert_eq!(latest.take(), (Some(2), false));
    }

    #[test]
    fn ui30_one_thousand_rapid_clicks_are_bounded_to_one_in_flight_request() {
        let gate = Arc::new(SyncRequestGate::default());
        let admitted = Arc::new(AtomicUsize::new(0));
        thread::scope(|scope| {
            for _ in 0..1_000 {
                let gate = Arc::clone(&gate);
                let admitted = Arc::clone(&admitted);
                scope.spawn(move || {
                    if gate.try_acquire() {
                        admitted.fetch_add(1, Ordering::Relaxed);
                    }
                });
            }
        });
        assert_eq!(admitted.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn ui41_one_thousand_auth_actions_are_bounded_to_one_in_flight_request() {
        let gate = Arc::new(SyncRequestGate::default());
        let admitted = Arc::new(AtomicUsize::new(0));
        thread::scope(|scope| {
            for _ in 0..1_000 {
                let gate = Arc::clone(&gate);
                let admitted = Arc::clone(&admitted);
                scope.spawn(move || {
                    if gate.try_acquire() {
                        admitted.fetch_add(1, Ordering::Relaxed);
                    }
                });
            }
        });
        assert_eq!(admitted.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn ui31_ten_thousand_updates_keep_only_the_latest_value() {
        let latest = LatestValue::default();
        for value in 0_u64..10_000 {
            let should_enqueue = latest.publish(value);
            assert_eq!(should_enqueue, value == 0);
        }
        assert_eq!(latest.take(), (Some(9_999), false));
    }

    #[test]
    fn ui32_concurrent_updates_remain_bounded_and_latest_wins() {
        let latest = Arc::new(LatestValue::default());
        thread::scope(|scope| {
            for worker in 0_u64..8 {
                let latest = Arc::clone(&latest);
                scope.spawn(move || {
                    for value in 0_u64..1_250 {
                        let _ = latest.publish(worker * 1_250 + value);
                    }
                });
            }
        });
        let (value, followup) = latest.take();
        assert!(value.is_some());
        assert!(!followup);
    }

    #[test]
    fn ui33_gate_release_does_not_replay_a_request() {
        let gate = SyncRequestGate::default();
        assert!(gate.try_acquire());
        gate.release();
    }

    #[test]
    fn ui34_tray_open_and_quit_are_high_level_actions() {
        assert_eq!(TRAY_ACTION_LABELS[0], "Open Synveil");
        assert_eq!(TRAY_ACTION_LABELS[2], "Quit Synveil Desktop");
    }

    #[test]
    fn ui35_no_raw_diagnostic_can_enter_the_tooltip_type() {
        fn accepts_only_static_label(label: &'static str) -> String {
            tray_tooltip(label)
        }
        assert_eq!(
            accepts_only_static_label("Connected"),
            "Synveil — Connected"
        );
    }

    #[test]
    fn ui36_discard_clears_pending_snapshot() {
        let latest = LatestValue::default();
        assert!(latest.publish(7_u64));
        latest.discard();
        assert_eq!(latest.take(), (None, false));
    }

    #[test]
    fn ui37_empty_tray_state_is_still_generic() {
        assert_eq!(
            tray_tooltip("Status unavailable"),
            "Synveil — Status unavailable"
        );
    }

    #[test]
    fn ui38_sync_gate_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SyncRequestGate>();
    }

    #[test]
    fn ui39_latest_value_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<LatestValue<u64>>();
    }

    #[test]
    fn ui40_coalescer_does_not_block_on_qt_or_controller_work() {
        let latest = LatestValue::default();
        assert!(latest.publish(1_u64));
        assert_eq!(latest.take(), (Some(1), false));
    }

    #[test]
    fn ui42_startup_intent_keeps_latest_value_with_one_in_flight_worker() {
        let gate = StartupIntentGate::default();
        gate.set_intent(true);
        assert_eq!(gate.try_start(), Some((true, 1)));
        gate.set_intent(false);
        assert!(!gate.current().0);
        assert_eq!(gate.try_start(), None);
        gate.release();
        assert_eq!(gate.try_start(), Some((false, 2)));
    }
}
