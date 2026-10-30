use synveil_server_network::transport::*;
use synveil_server_network::{InterfaceKind, NetworkCandidate, classify_interface};

#[test]
fn overlay_and_private_address_families_are_disjoint() {
    for ip in ["100.64.0.0", "100.127.255.255", "fd7a:115c:a1e0::1"] {
        let ip = ip.parse().unwrap();
        assert!(overlay_address(ip));
        assert!(!private_lan_address(ip));
    }
    for ip in ["192.168.1.3", "10.0.0.1", "172.16.0.1", "fd12::1"] {
        let ip = ip.parse().unwrap();
        assert!(private_lan_address(ip));
        assert!(!overlay_address(ip));
    }
    for ip in [
        "127.0.0.1",
        "::1",
        "::",
        "0.0.0.0",
        "169.254.1.1",
        "fe80::1",
        "8.8.8.8",
        "2001:4860:4860::8888",
        "224.0.0.1",
        "100.128.0.1",
    ] {
        let ip = ip.parse().unwrap();
        assert!(!private_lan_address(ip));
        assert!(!overlay_address(ip));
    }
}
#[test]
fn shared_interface_classifier_keeps_tunnels_and_bridges_out_of_lan() {
    for (name, link, expected) in [
        ("tailscale0", "", InterfaceKind::Tunnel),
        ("enp0s1", "wireguard", InterfaceKind::Tunnel),
        ("docker0", "", InterfaceKind::ContainerBridge),
        ("eth0", "veth", InterfaceKind::ContainerBridge),
    ] {
        let kind = classify_interface(name, link, false, false, true);
        assert_eq!(kind, expected);
        let candidate = NetworkCandidate {
            interface_id: name.into(),
            address: "192.168.1.2".parse().unwrap(),
            prefix_length: 24,
            kind,
            operational: true,
            default_route: true,
        };
        assert!(!lan_candidate(&candidate));
    }
    assert_eq!(
        classify_interface("eth0", "", false, false, false),
        InterfaceKind::Other
    );
    assert_eq!(
        classify_interface("eth0", "", false, false, true),
        InterfaceKind::Ethernet
    );
}
#[test]
fn uncertain_inspection_is_not_reported_as_missing_installation() {
    let s = LocalCapabilities {
        lan: Err(UnavailableReason::PermissionDenied),
        tailscale: TailscaleCapability::Unavailable(UnavailableReason::PermissionDenied),
        routes: vec![],
    };
    let summary = s.summary();
    assert_eq!(summary.lan_configured, None);
    assert_eq!(summary.tailscale_installed, None);
    assert_eq!(summary.tailscale_configured, None);
    assert!(!summary.authenticated);
    assert_eq!(
        summary.tailscale_unavailable,
        Some(UnavailableReason::PermissionDenied)
    );
}
