//! Sanitized, ephemeral installer presentation. No endpoint, credential or effect API.
use super::{INSPECTION_TIMEOUT, LocalDiscovery};
use std::time::{Duration, Instant};
use synveil_server_network::transport::{CapabilitySummary, UnavailableReason};
use tokio::sync::watch;

/// A bounded local observation is never evidence for a later installation action.
pub const EVIDENCE_LIFETIME: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MethodStatus {
    NotInspected,
    LocallyAvailable,
    SetupRequired,
    Disconnected,
    Unavailable(UnavailableReason),
}

impl MethodStatus {
    pub fn guidance(self) -> &'static str {
        use UnavailableReason as R;
        match self {
            Self::NotInspected => "Run fresh preflight to inspect local capability.",
            Self::LocallyAvailable => {
                "Local capability observed. Approve the exact HTTPS endpoint and verify server identity in the existing connection flow; Synveil authentication is still required."
            }
            Self::SetupRequired => {
                "Tailscale is installed but not configured. An operator must separately approve and perform setup, then run preflight again."
            }
            Self::Disconnected => {
                "Tailscale is configured but disconnected. Restore the tailnet connection separately, then run preflight again."
            }
            Self::Unavailable(R::NotInstalled) => {
                "Required CLI is missing. An operator must separately approve installation; LAN-only operation does not require Tailscale."
            }
            Self::Unavailable(R::Unsupported) => {
                "Native inspection or this network schema is unsupported. Use the existing manual HTTPS flow or a qualified Linux environment."
            }
            Self::Unavailable(R::PermissionDenied) => {
                "Inspection is restricted. Ask the operator to review permissions; this preview never requests elevation."
            }
            Self::Unavailable(R::Cancelled) => "Inspection cancelled. Run fresh preflight.",
            Self::Unavailable(R::TimedOut) => {
                "Inspection timed out. Check local networking and run fresh preflight."
            }
            Self::Unavailable(R::Unreachable | R::Disconnected) => {
                "No usable local connectivity was observed. Restore the selected connection and run fresh preflight."
            }
            Self::Unavailable(R::NotConfigured) => {
                "Operator-approved configuration is required before fresh preflight."
            }
            Self::Unavailable(
                R::UntrustedEndpoint
                | R::IdentityConflict
                | R::AuthenticationRequired
                | R::AccessRevoked
                | R::EnrollmentExpired,
            ) => {
                "Endpoint approval, identity or authentication requires attention in the existing connection flow. Do not switch endpoints to bypass this failure."
            }
            Self::Unavailable(R::InconsistentEvidence | R::RouteChanged | R::LimitExceeded) => {
                "Network evidence is unknown, changed or exceeds inspection limits. Ask the operator to review it and run fresh preflight."
            }
        }
    }
}

/// Only typed statuses and tri-state installation/configuration facts cross this
/// boundary. Addresses, interface names, peers and raw CLI output are discarded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NetworkPreflight {
    pub lan: MethodStatus,
    pub tailscale: MethodStatus,
    pub tailscale_installed: Option<bool>,
    pub tailscale_configured: Option<bool>,
    observed_at: Option<Instant>,
}

impl Default for NetworkPreflight {
    fn default() -> Self {
        Self {
            lan: MethodStatus::NotInspected,
            tailscale: MethodStatus::NotInspected,
            tailscale_installed: None,
            tailscale_configured: None,
            observed_at: None,
        }
    }
}

impl NetworkPreflight {
    fn from_summary(summary: CapabilitySummary, now: Instant) -> Self {
        Self {
            lan: match (summary.lan_unavailable, summary.lan_configured) {
                (Some(reason), _) => MethodStatus::Unavailable(reason),
                (None, Some(true)) => MethodStatus::LocallyAvailable,
                _ => MethodStatus::Unavailable(UnavailableReason::Unreachable),
            },
            tailscale: match summary.tailscale_unavailable {
                Some(reason) => MethodStatus::Unavailable(reason),
                None if summary.tailscale_connected => MethodStatus::LocallyAvailable,
                None if summary.tailscale_configured == Some(true) => MethodStatus::Disconnected,
                None if summary.tailscale_installed == Some(true) => MethodStatus::SetupRequired,
                None => MethodStatus::Unavailable(UnavailableReason::InconsistentEvidence),
            },
            tailscale_installed: summary.tailscale_installed,
            tailscale_configured: summary.tailscale_configured,
            observed_at: Some(now),
        }
    }

    fn unavailable(reason: UnavailableReason) -> Self {
        Self {
            lan: MethodStatus::Unavailable(reason),
            tailscale: MethodStatus::Unavailable(reason),
            observed_at: Some(Instant::now()),
            ..Self::default()
        }
    }

    pub fn is_current_at(&self, now: Instant) -> bool {
        self.observed_at
            .and_then(|observed| now.checked_duration_since(observed))
            .is_some_and(|age| age < EVIDENCE_LIFETIME)
    }

    pub fn summary(&self) -> String {
        if !self.is_current_at(Instant::now()) {
            return "Network evidence is missing or stale. Run fresh preflight; no approval can be derived from this preview.".into();
        }
        fn fact(value: Option<bool>) -> &'static str {
            match value {
                Some(true) => "yes",
                Some(false) => "no",
                None => "unknown",
            }
        }
        format!(
            "LAN: {}\nTailscale (installed: {}; configured: {}): {}\nDiscovery does not establish authenticated connectivity or authorize installation. Any network setup requires separate user approval.",
            self.lan.guidance(),
            fact(self.tailscale_installed),
            fact(self.tailscale_configured),
            self.tailscale.guidance(),
        )
    }
}

/// One local snapshot, no approved endpoints, no route queries or handshakes.
/// Dropping/cancelling this future drops NativeDiscovery's kill-on-drop children.
pub async fn inspect(
    discovery: &dyn LocalDiscovery,
    cancel: watch::Receiver<bool>,
) -> NetworkPreflight {
    if *cancel.borrow() {
        return NetworkPreflight::unavailable(UnavailableReason::Cancelled);
    }
    tokio::select! {
        biased;
        _ = super::cancelled(cancel) => NetworkPreflight::unavailable(UnavailableReason::Cancelled),
        result = tokio::time::timeout(INSPECTION_TIMEOUT, discovery.inspect(&[])) => {
            match result {
                Ok(Ok(local)) => NetworkPreflight::from_summary(local.summary(), Instant::now()),
                Ok(Err(reason)) => NetworkPreflight::unavailable(reason),
                Err(_) => NetworkPreflight::unavailable(UnavailableReason::TimedOut),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{Endpoint, native::NativeDiscovery, tests::snapshot};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use synveil_server_network::transport::{LocalCapabilities, TailscaleCapability};

    struct Discovery {
        local: LocalCapabilities,
        calls: AtomicUsize,
    }
    #[async_trait]
    impl LocalDiscovery for Discovery {
        async fn inspect(
            &self,
            endpoints: &[Endpoint],
        ) -> Result<LocalCapabilities, UnavailableReason> {
            // The presentation must never request endpoint routes or qualification.
            assert!(endpoints.is_empty());
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.local.clone())
        }
    }

    #[tokio::test]
    async fn lan_only_survives_missing_disconnected_or_unknown_overlay() {
        for overlay in [
            TailscaleCapability::Unavailable(UnavailableReason::NotInstalled),
            TailscaleCapability::ConfiguredDisconnected,
            TailscaleCapability::Unavailable(UnavailableReason::Unsupported),
            TailscaleCapability::Unavailable(UnavailableReason::PermissionDenied),
        ] {
            let mut local = snapshot();
            local.tailscale = overlay;
            let discovery = Discovery {
                local,
                calls: AtomicUsize::new(0),
            };
            let (_tx, rx) = watch::channel(false);
            let view = inspect(&discovery, rx).await;
            assert_eq!(view.lan, MethodStatus::LocallyAvailable);
            assert_ne!(view.tailscale, MethodStatus::LocallyAvailable);
            assert_eq!(discovery.calls.load(Ordering::SeqCst), 1);
            assert!(
                view.summary()
                    .contains("does not establish authenticated connectivity")
            );
        }
    }

    #[test]
    fn connected_overlay_and_peer_never_establish_authenticated_connectivity() {
        let local = snapshot();
        let view = NetworkPreflight::from_summary(local.summary(), Instant::now());
        assert_eq!(view.tailscale, MethodStatus::LocallyAvailable);
        assert_eq!(view.tailscale_installed, Some(true));
        assert_eq!(view.tailscale_configured, Some(true));
        let text = format!("{:?} {}", view, view.summary());
        for raw in ["192.168.1.2", "100.64.0.2", "approved-peer", "eth0"] {
            assert!(!text.contains(raw));
        }
        assert!(text.contains("separate user approval"));
    }

    #[test]
    fn installation_configuration_and_unavailable_connectivity_remain_distinct() {
        let mut local = snapshot();
        local.lan = Ok(vec![]);
        for (overlay, installed, configured, status) in [
            (
                TailscaleCapability::Installed,
                Some(true),
                Some(false),
                MethodStatus::SetupRequired,
            ),
            (
                TailscaleCapability::ConfiguredDisconnected,
                Some(true),
                Some(true),
                MethodStatus::Disconnected,
            ),
            (
                TailscaleCapability::Unavailable(UnavailableReason::NotInstalled),
                Some(false),
                None,
                MethodStatus::Unavailable(UnavailableReason::NotInstalled),
            ),
        ] {
            local.tailscale = overlay;
            let view = NetworkPreflight::from_summary(local.summary(), Instant::now());
            assert_eq!(
                view.lan,
                MethodStatus::Unavailable(UnavailableReason::Unreachable)
            );
            assert_eq!(view.tailscale, status);
            assert_eq!(view.tailscale_installed, installed);
            assert_eq!(view.tailscale_configured, configured);
        }
    }

    #[test]
    fn unsupported_windows_macos_unknown_schema_and_restricted_privilege_are_actionable() {
        for reason in [
            UnavailableReason::Unsupported,
            UnavailableReason::PermissionDenied,
            UnavailableReason::InconsistentEvidence,
            UnavailableReason::IdentityConflict,
            UnavailableReason::UntrustedEndpoint,
        ] {
            let view = NetworkPreflight::unavailable(reason);
            assert_eq!(view.tailscale_installed, None);
            assert_eq!(view.tailscale_configured, None);
            assert_ne!(view.lan, MethodStatus::LocallyAvailable);
            assert!(!view.lan.guidance().is_empty());
        }
        assert!(
            MethodStatus::Unavailable(UnavailableReason::IdentityConflict)
                .guidance()
                .contains("Do not switch endpoints")
        );
    }

    #[tokio::test]
    async fn actual_native_windows_macos_boundary_is_unsupported() {
        if !cfg!(target_os = "linux") {
            let (_tx, rx) = watch::channel(false);
            assert_eq!(
                inspect(&NativeDiscovery, rx).await.lan,
                MethodStatus::Unavailable(UnavailableReason::Unsupported)
            );
        }
    }

    #[test]
    fn stale_evidence_requires_fresh_preflight_including_exact_expiry() {
        let now = Instant::now();
        let view = NetworkPreflight::from_summary(snapshot().summary(), now);
        assert!(view.is_current_at(now));
        assert!(!view.is_current_at(now + EVIDENCE_LIFETIME));
        assert!(!view.is_current_at(now - Duration::from_secs(1)));
        assert!(!NetworkPreflight::default().is_current_at(now));
    }

    struct Stalled(AtomicUsize);
    #[async_trait]
    impl LocalDiscovery for Stalled {
        async fn inspect(&self, _: &[Endpoint]) -> Result<LocalCapabilities, UnavailableReason> {
            struct Dropped<'a>(&'a AtomicUsize);
            impl Drop for Dropped<'_> {
                fn drop(&mut self) {
                    self.0.fetch_add(1, Ordering::SeqCst);
                }
            }
            let _guard = Dropped(&self.0);
            std::future::pending().await
        }
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_and_cancellation_drop_inflight_discovery_without_retries() {
        let stalled = Stalled(AtomicUsize::new(0));
        let (_tx, rx) = watch::channel(false);
        let view = inspect(&stalled, rx).await;
        assert_eq!(
            view.lan,
            MethodStatus::Unavailable(UnavailableReason::TimedOut)
        );
        assert_eq!(stalled.0.load(Ordering::SeqCst), 1);
        let (tx, rx) = watch::channel(false);
        let probe = inspect(&stalled, rx);
        tokio::pin!(probe);
        tokio::select! {
            _ = &mut probe => panic!("must remain in flight"),
            _ = tokio::task::yield_now() => {},
        }
        tx.send_replace(true);
        assert_eq!(
            probe.await.lan,
            MethodStatus::Unavailable(UnavailableReason::Cancelled)
        );
        assert_eq!(stalled.0.load(Ordering::SeqCst), 2);
        let (tx, rx) = watch::channel(true);
        let discovery = Discovery {
            local: snapshot(),
            calls: AtomicUsize::new(0),
        };
        assert_eq!(
            inspect(&discovery, rx).await.lan,
            MethodStatus::Unavailable(UnavailableReason::Cancelled)
        );
        assert_eq!(discovery.calls.load(Ordering::SeqCst), 0);
        drop(tx);
    }
}
