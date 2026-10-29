use super::*;
use UnavailableReason as R;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use synveil_server_network::transport::{RouteEvidence, TailscaleCapability, TailscalePeer};
use synveil_server_network::{InterfaceKind, NetworkCandidate};

fn endpoint(t: Transport, ip: &str) -> Endpoint {
    let address: IpAddr = ip.parse().unwrap();
    Endpoint {
        profile_id: synveil_client_sync::ServerProfileId::new(),
        transport: t,
        origin: if address.is_ipv6() {
            format!("https://[{ip}]/")
        } else {
            format!("https://{ip}/")
        },
        server_id: "approved-server".into(),
        network_id: "approved-network".into(),
        trust_generation: 1,
        ca_sha256: None,
        address,
        peer_id: (t == Transport::Tailscale).then(|| "approved-peer".into()),
    }
}
fn snapshot() -> LocalCapabilities {
    LocalCapabilities {
        lan: Ok(vec![NetworkCandidate {
            interface_id: "eth0".into(),
            address: "192.168.1.2".parse().unwrap(),
            prefix_length: 24,
            kind: InterfaceKind::Ethernet,
            operational: true,
            default_route: false,
        }]),
        tailscale: TailscaleCapability::Connected {
            local_ips: vec!["100.64.0.2".parse().unwrap()],
            peers: vec![TailscalePeer {
                node_id: "approved-peer".into(),
                addresses: vec!["100.64.0.3".parse().unwrap()],
                online: true,
                expired: false,
            }],
        },
        routes: vec![
            RouteEvidence {
                destination: "192.168.1.3".parse().unwrap(),
                source: "192.168.1.2".parse().unwrap(),
                interface_id: "eth0".into(),
            },
            RouteEvidence {
                destination: "100.64.0.3".parse().unwrap(),
                source: "100.64.0.2".parse().unwrap(),
                interface_id: "tailscale0".into(),
            },
        ],
    }
}
struct FakeDiscovery {
    states: Mutex<Vec<LocalCapabilities>>,
    calls: AtomicUsize,
}
impl FakeDiscovery {
    fn new(states: Vec<LocalCapabilities>) -> Self {
        Self {
            states: Mutex::new(states),
            calls: AtomicUsize::new(0),
        }
    }
}
#[async_trait]
impl LocalDiscovery for FakeDiscovery {
    async fn inspect(&self, _: &[Endpoint]) -> Result<LocalCapabilities, R> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut s = self.states.lock().unwrap();
        Ok(if s.len() > 1 {
            s.remove(0)
        } else {
            s[0].clone()
        })
    }
}
struct FakeVerifier {
    calls: Mutex<Vec<Transport>>,
    results: Mutex<Vec<Result<(), R>>>,
}
impl FakeVerifier {
    fn new(results: Vec<Result<(), R>>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            results: Mutex::new(results),
        }
    }
}
#[async_trait]
impl EndpointVerifier for FakeVerifier {
    async fn verify(&self, e: &Endpoint) -> Result<(), R> {
        self.calls.lock().unwrap().push(e.transport);
        let mut r = self.results.lock().unwrap();
        if r.len() > 1 { r.remove(0) } else { r[0] }
    }
}
async fn select(
    d: &dyn LocalDiscovery,
    v: &dyn EndpointVerifier,
    endpoints: &[Endpoint],
    p: SelectionPolicy,
) -> Selection {
    let (_tx, rx) = watch::channel(false);
    discover_and_select(d, v, endpoints, p, rx).await
}
fn endpoints() -> Vec<Endpoint> {
    vec![
        endpoint(Transport::Tailscale, "100.64.0.3"),
        endpoint(Transport::Lan, "192.168.1.3"),
    ]
}

#[tokio::test]
async fn dual_network_selection_is_explicit_deterministic_and_authenticated() {
    for (policy, expected) in [
        (SelectionPolicy::LanOnly, Transport::Lan),
        (SelectionPolicy::PreferLan, Transport::Lan),
        (SelectionPolicy::PreferTailscale, Transport::Tailscale),
        (SelectionPolicy::TailscaleOnly, Transport::Tailscale),
    ] {
        for reverse in [false, true] {
            let mut endpoints = endpoints();
            if reverse {
                endpoints.reverse();
            }
            let d = FakeDiscovery::new(vec![snapshot()]);
            let v = FakeVerifier::new(vec![Ok(())]);
            assert_eq!(
                select(&d, &v, &endpoints, policy).await.transport(),
                expected
            );
            assert_eq!(*v.calls.lock().unwrap(), vec![expected]);
            assert_eq!(d.calls.load(Ordering::SeqCst), 2);
        }
    }
}
#[tokio::test]
async fn partial_installation_disconnected_tailnet_and_lan_only_need_no_dependency() {
    for tailscale in [
        TailscaleCapability::Unavailable(R::NotInstalled),
        TailscaleCapability::Installed,
        TailscaleCapability::ConfiguredDisconnected,
    ] {
        let mut s = snapshot();
        s.tailscale = tailscale;
        assert!(!s.summary().authenticated);
        assert!(!s.summary().tailscale_connected);
        let d = FakeDiscovery::new(vec![s]);
        let v = FakeVerifier::new(vec![Ok(())]);
        assert_eq!(
            select(&d, &v, &endpoints(), SelectionPolicy::LanOnly)
                .await
                .transport(),
            Transport::Lan
        );
        assert_eq!(
            select(&d, &v, &endpoints(), SelectionPolicy::TailscaleOnly)
                .await
                .transport(),
            Transport::Unavailable
        );
    }
}
#[tokio::test]
async fn unavailable_lan_can_fall_back_only_under_explicit_policy() {
    let mut missing_route = snapshot();
    missing_route.routes.remove(0);
    let mut link_down = snapshot();
    link_down.lan.as_mut().unwrap()[0].operational = false;
    for s in [missing_route, link_down] {
        let d = FakeDiscovery::new(vec![s]);
        let v = FakeVerifier::new(vec![Ok(())]);
        assert_eq!(
            select(&d, &v, &endpoints(), SelectionPolicy::PreferLan)
                .await
                .transport(),
            Transport::Tailscale
        );
        assert_eq!(
            select(&d, &v, &endpoints(), SelectionPolicy::LanOnly).await,
            Selection::Unavailable(R::Unreachable)
        );
    }
}
#[tokio::test]
async fn security_failures_never_retry_or_downgrade() {
    for reason in [
        R::UntrustedEndpoint,
        R::IdentityConflict,
        R::AuthenticationRequired,
        R::AccessRevoked,
        R::EnrollmentExpired,
        R::PermissionDenied,
        R::Unsupported,
    ] {
        let d = FakeDiscovery::new(vec![snapshot()]);
        let v = FakeVerifier::new(vec![Err(reason)]);
        assert_eq!(
            select(&d, &v, &endpoints(), SelectionPolicy::PreferLan).await,
            Selection::Unavailable(reason)
        );
        assert_eq!(v.calls.lock().unwrap().len(), 1);
    }
}
#[tokio::test]
async fn retry_bounds_apply_only_to_read_only_unreachable_handshakes() {
    let d = FakeDiscovery::new(vec![snapshot()]);
    let v = FakeVerifier::new(vec![Err(R::Unreachable)]);
    assert_eq!(
        select(&d, &v, &endpoints(), SelectionPolicy::PreferLan).await,
        Selection::Unavailable(R::Unreachable)
    );
    assert_eq!(
        *v.calls.lock().unwrap(),
        vec![
            Transport::Lan,
            Transport::Lan,
            Transport::Tailscale,
            Transport::Tailscale
        ]
    );
    let v = FakeVerifier::new(vec![Err(R::Unreachable), Ok(())]);
    assert_eq!(
        select(&d, &v, &endpoints(), SelectionPolicy::PreferLan)
            .await
            .transport(),
        Transport::Lan
    );
}
#[tokio::test]
async fn spoofing_conflicts_and_expired_peers_fail_closed() {
    for reason in [
        R::IdentityConflict,
        R::EnrollmentExpired,
        R::InconsistentEvidence,
    ] {
        let mut s = snapshot();
        match reason {
            R::IdentityConflict => {
                if let TailscaleCapability::Connected { peers, .. } = &mut s.tailscale {
                    peers[0].node_id = "spoof".into();
                }
            }
            R::EnrollmentExpired => {
                if let TailscaleCapability::Connected { peers, .. } = &mut s.tailscale {
                    peers[0].expired = true;
                }
            }
            _ => s.routes.push(s.routes[1].clone()),
        }
        let d = FakeDiscovery::new(vec![s]);
        let v = FakeVerifier::new(vec![Ok(())]);
        assert_eq!(
            select(&d, &v, &endpoints(), SelectionPolicy::PreferTailscale).await,
            Selection::Unavailable(reason)
        );
        assert!(v.calls.lock().unwrap().is_empty());
    }
    let mut endpoints = endpoints();
    endpoints[0].server_id = "another-server".into();
    let d = FakeDiscovery::new(vec![snapshot()]);
    let v = FakeVerifier::new(vec![Ok(())]);
    assert_eq!(
        select(&d, &v, &endpoints, SelectionPolicy::PreferLan).await,
        Selection::Unavailable(R::IdentityConflict)
    );
    assert_eq!(d.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn route_changes_invalidate_previously_authenticated_selection() {
    let a = snapshot();
    let mut b = a.clone();
    b.routes[0].interface_id = "wifi0".into();
    let d = FakeDiscovery::new(vec![a, b]);
    let v = FakeVerifier::new(vec![Ok(())]);
    assert_eq!(
        select(&d, &v, &endpoints(), SelectionPolicy::PreferLan).await,
        Selection::Unavailable(R::RouteChanged)
    );
}
#[tokio::test]
async fn ipv6_lan_and_overlay_keep_separate_routes() {
    for (transport, address, source, interface) in [
        (Transport::Lan, "fd12::3", "fd12::2", "eth0"),
        (
            Transport::Tailscale,
            "fd7a:115c:a1e0::3",
            "fd7a:115c:a1e0::2",
            "tailscale0",
        ),
    ] {
        let mut s = snapshot();
        s.routes = vec![RouteEvidence {
            destination: address.parse().unwrap(),
            source: source.parse().unwrap(),
            interface_id: interface.into(),
        }];
        if transport == Transport::Lan {
            let c = &mut s.lan.as_mut().unwrap()[0];
            c.address = source.parse().unwrap();
            c.prefix_length = 64;
        } else if let TailscaleCapability::Connected { local_ips, peers } = &mut s.tailscale {
            *local_ips = vec![source.parse().unwrap()];
            peers[0].addresses = vec![address.parse().unwrap()];
        }
        let d = FakeDiscovery::new(vec![s]);
        let v = FakeVerifier::new(vec![Ok(())]);
        assert_eq!(
            select(
                &d,
                &v,
                &[endpoint(transport, address)],
                SelectionPolicy::PreferLan
            )
            .await
            .transport(),
            transport
        );
    }
}
#[tokio::test(start_paused = true)]
async fn cancellation_and_deadlines_bound_stalled_adapters() {
    struct Stalled;
    #[async_trait]
    impl LocalDiscovery for Stalled {
        async fn inspect(&self, _: &[Endpoint]) -> Result<LocalCapabilities, R> {
            std::future::pending().await
        }
    }
    #[async_trait]
    impl EndpointVerifier for Stalled {
        async fn verify(&self, _: &Endpoint) -> Result<(), R> {
            std::future::pending().await
        }
    }
    let v = FakeVerifier::new(vec![Ok(())]);
    assert_eq!(
        select(&Stalled, &v, &endpoints(), SelectionPolicy::PreferLan).await,
        Selection::Unavailable(R::TimedOut)
    );
    let d = FakeDiscovery::new(vec![snapshot()]);
    assert_eq!(
        select(&d, &Stalled, &endpoints(), SelectionPolicy::PreferLan).await,
        Selection::Unavailable(R::Unreachable)
    );
    let many: Vec<_> = (3..11)
        .map(|n| endpoint(Transport::Lan, &format!("192.168.1.{n}")))
        .collect();
    let mut s = snapshot();
    s.routes = many
        .iter()
        .map(|e| RouteEvidence {
            destination: e.address,
            source: "192.168.1.2".parse().unwrap(),
            interface_id: "eth0".into(),
        })
        .collect();
    let d = FakeDiscovery::new(vec![s]);
    let start = tokio::time::Instant::now();
    assert_eq!(
        select(&d, &Stalled, &many, SelectionPolicy::LanOnly).await,
        Selection::Unavailable(R::TimedOut)
    );
    assert_eq!(start.elapsed(), SELECTION_TIMEOUT);
    let (tx, rx) = watch::channel(true);
    assert_eq!(
        discover_and_select(&Stalled, &v, &endpoints(), SelectionPolicy::PreferLan, rx).await,
        Selection::Unavailable(R::Cancelled)
    );
    tx.send_replace(false);
    let rx = tx.subscribe();
    let endpoints = endpoints();
    let future = discover_and_select(&Stalled, &v, &endpoints, SelectionPolicy::PreferLan, rx);
    tokio::pin!(future);
    tokio::select! { biased; _ = &mut future => panic!("inspection should stall"), _ = tokio::task::yield_now() => {} }
    tx.send(true).unwrap();
    assert_eq!(future.await, Selection::Unavailable(R::Cancelled));
}
#[tokio::test]
async fn endpoint_limits_and_empty_input_do_not_start_discovery() {
    let d = FakeDiscovery::new(vec![snapshot()]);
    let v = FakeVerifier::new(vec![Ok(())]);
    assert_eq!(
        select(&d, &v, &[], SelectionPolicy::LanOnly).await,
        Selection::Unavailable(R::NotConfigured)
    );
    assert_eq!(
        select(
            &d,
            &v,
            &vec![endpoints()[0].clone(); MAX_ENDPOINTS + 1],
            SelectionPolicy::LanOnly
        )
        .await,
        Selection::Unavailable(R::LimitExceeded)
    );
    assert_eq!(d.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn only_explicit_profile_trust_can_create_numeric_transport_endpoints() {
    use synveil_client_sync::CanonicalBaseUrl;
    use synveil_core::host_trust::{HostTrust, PublicHostTrust};
    for (origin, transport, peer, expected) in [
        ("https://192.168.1.3/", Transport::Lan, None, Ok(())),
        ("https://[fd12::3]/", Transport::Lan, None, Ok(())),
        (
            "https://100.64.0.3/",
            Transport::Lan,
            None,
            Err(R::UntrustedEndpoint),
        ),
        (
            "https://192.168.1.3/",
            Transport::Tailscale,
            Some("peer".into()),
            Err(R::UntrustedEndpoint),
        ),
        (
            "https://100.64.0.3/",
            Transport::Tailscale,
            Some("peer".into()),
            Ok(()),
        ),
        (
            "https://synveil.example/",
            Transport::Lan,
            None,
            Err(R::Unsupported),
        ),
        (
            "https://1.1.1.1/",
            Transport::Lan,
            None,
            Err(R::UntrustedEndpoint),
        ),
    ] {
        let profile =
            ServerProfile::new(CanonicalBaseUrl::parse(origin).unwrap(), "approved").unwrap();
        assert_eq!(
            Endpoint::from_profile(&profile, transport, peer.clone()),
            Err(R::UntrustedEndpoint)
        );
        let trust = HostTrust::Public(PublicHostTrust {
            schema_version: 1,
            kind: "public_web_pki".into(),
            server_installation_id: uuid::Uuid::now_v7().to_string(),
            network_integration_id: uuid::Uuid::now_v7().to_string(),
            canonical_origin: origin.into(),
            config_generation: 1,
        });
        let profile = profile.with_managed_host_trust(trust).unwrap();
        assert_eq!(
            Endpoint::from_profile(&profile, transport, peer).map(|_| ()),
            expected
        );
        let other_profile = ServerProfile::new(profile.base_url().clone(), "another profile")
            .unwrap()
            .with_managed_host_trust(profile.managed_host_trust().unwrap().clone())
            .unwrap();
        if let (Ok(a), Ok(b)) = (
            Endpoint::from_profile(&profile, transport, None),
            Endpoint::from_profile(&other_profile, transport, None),
        ) {
            assert_ne!(
                a, b,
                "same origin and trust must not alias another credential profile"
            );
        }
        let changed = profile
            .reconfigured(
                CanonicalBaseUrl::parse("https://192.168.1.99/").unwrap(),
                "changed",
            )
            .unwrap();
        assert_eq!(
            Endpoint::from_profile(&changed, Transport::Lan, None),
            Err(R::UntrustedEndpoint)
        );
    }
}

#[tokio::test]
async fn unsupported_hosts_permissions_and_inconsistent_adapters_never_become_usable() {
    struct Unsupported;
    #[async_trait]
    impl LocalDiscovery for Unsupported {
        async fn inspect(&self, _: &[Endpoint]) -> Result<LocalCapabilities, R> {
            Err(R::Unsupported)
        }
    }
    let v = FakeVerifier::new(vec![Ok(())]);
    assert_eq!(
        select(&Unsupported, &v, &endpoints(), SelectionPolicy::PreferLan).await,
        Selection::Unavailable(R::Unsupported)
    );
    for reason in [R::PermissionDenied, R::Unsupported, R::InconsistentEvidence] {
        let mut s = snapshot();
        s.lan = Err(reason);
        s.routes.clear();
        assert_eq!(
            select(
                &FakeDiscovery::new(vec![s]),
                &v,
                &endpoints(),
                SelectionPolicy::PreferLan
            )
            .await,
            Selection::Unavailable(reason)
        );
    }
    assert!(v.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn same_transport_ties_use_canonical_origin_and_overlay_only_never_downgrades() {
    let a = endpoint(Transport::Lan, "192.168.1.3");
    let b = endpoint(Transport::Lan, "192.168.1.4");
    let mut s = snapshot();
    s.routes.push(RouteEvidence {
        destination: b.address,
        ..s.routes[0].clone()
    });
    let d = FakeDiscovery::new(vec![s]);
    let v = FakeVerifier::new(vec![Ok(())]);
    for endpoints in [vec![a.clone(), b.clone()], vec![b.clone(), a.clone()]] {
        assert_eq!(
            select(&d, &v, &endpoints, SelectionPolicy::LanOnly).await,
            Selection::Selected {
                transport: Transport::Lan,
                origin: a.origin.clone()
            }
        );
    }
    let mut s = snapshot();
    s.tailscale = TailscaleCapability::ConfiguredDisconnected;
    assert_eq!(
        select(
            &FakeDiscovery::new(vec![s]),
            &v,
            &endpoints(),
            SelectionPolicy::TailscaleOnly
        )
        .await,
        Selection::Unavailable(R::Disconnected)
    );
    assert_eq!(
        select(&d, &v, &[a], SelectionPolicy::TailscaleOnly).await,
        Selection::Unavailable(R::NotConfigured)
    );
}

#[tokio::test(start_paused = true)]
async fn stalled_lan_handshakes_leave_time_for_explicit_overlay_fallback() {
    struct StalledLan;
    #[async_trait]
    impl EndpointVerifier for StalledLan {
        async fn verify(&self, endpoint: &Endpoint) -> Result<(), R> {
            if endpoint.transport == Transport::Lan {
                std::future::pending().await
            } else {
                Ok(())
            }
        }
    }
    let d = FakeDiscovery::new(vec![snapshot()]);
    let start = tokio::time::Instant::now();
    assert_eq!(
        select(&d, &StalledLan, &endpoints(), SelectionPolicy::PreferLan)
            .await
            .transport(),
        Transport::Tailscale
    );
    assert_eq!(start.elapsed(), HANDSHAKE_TIMEOUT * 2);
}
