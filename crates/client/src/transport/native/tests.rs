use super::*;
use serde_json::json;

#[cfg(target_os = "linux")]
async fn test_child_result(child: tokio::process::Child) -> Result<Vec<u8>, UnavailableReason> {
    read_child(child, Query::Tailscale).await
}

fn status() -> serde_json::Value {
    json!({"BackendState":"Running", "TUN":true,
        "Self":{"ID":"self", "TailscaleIPs":["100.64.0.2"],"Online":true,"Expired":false},
        "Peer":{"key":{"ID":"peer", "TailscaleIPs":["100.64.0.3","fd7a:115c:a1e0::3"],"Online":true,"Expired":false}}})
}
fn parse(s: serde_json::Value) -> Result<TailscaleCapability, UnavailableReason> {
    parse_tailscale(&serde_json::to_vec(&s).unwrap())
}
#[test]
fn installation_is_never_reachability_and_unknown_states_fail_closed() {
    for (state, expected) in [
        ("NeedsLogin", Ok(TailscaleCapability::Installed)),
        ("Stopped", Ok(TailscaleCapability::ConfiguredDisconnected)),
        ("NeedsMachineAuth", Err(UnavailableReason::PermissionDenied)),
        ("FutureState", Err(UnavailableReason::Unsupported)),
    ] {
        assert_eq!(parse(json!({"BackendState":state})), expected);
    }
    assert!(matches!(
        parse(status()),
        Ok(TailscaleCapability::Connected { .. })
    ));
    let mut s = status();
    s["TUN"] = json!(false);
    assert_eq!(parse(s), Err(UnavailableReason::Unsupported));
    let mut s = status();
    s["Self"]["Online"] = json!(false);
    assert_eq!(parse(s), Ok(TailscaleCapability::ConfiguredDisconnected));
}
#[test]
fn omitted_false_expiry_matches_upstream_and_conflicts_are_rejected() {
    let mut s = status();
    s["Self"].as_object_mut().unwrap().remove("Expired");
    assert!(matches!(
        parse(s),
        Ok(TailscaleCapability::Connected { .. })
    ));
    let mut s = status();
    s["Peer"]["key"]["TailscaleIPs"] = json!(["100.64.0.2"]);
    assert_eq!(parse(s), Err(UnavailableReason::IdentityConflict));
    let mut s = status();
    s["Self"]["KeyExpiry"] = json!("2000-01-01T00:00:00Z");
    assert_eq!(parse(s), Err(UnavailableReason::EnrollmentExpired));
    let mut s = status();
    s["Self"]["Expired"] = json!(true);
    assert_eq!(parse(s), Err(UnavailableReason::EnrollmentExpired));
}
#[test]
fn route_evidence_is_exact_numeric_and_never_via_a_gateway() {
    let route = Query::Route("192.168.1.3".parse().unwrap());
    assert_eq!(
        command_failure(route, b"RTNETLINK answers: Network is unreachable\n"),
        UnavailableReason::Unreachable
    );
    assert_eq!(
        command_failure(route, b"RTNETLINK answers: Operation not permitted\n"),
        UnavailableReason::PermissionDenied
    );
    assert_eq!(
        command_failure(
            Query::Tailscale,
            b"RTNETLINK answers: Network is unreachable\n"
        ),
        UnavailableReason::PermissionDenied
    );
    assert_eq!(
        command_failure(route, b"unknown failure"),
        UnavailableReason::PermissionDenied
    );
    let ip = "192.168.1.3".parse().unwrap();
    assert!(
        parse_route(
            br#"[{"dst":"192.168.1.3","dev":"eth0","prefsrc":"192.168.1.2"}]"#,
            ip
        )
        .is_ok()
    );
    assert_eq!(parse_route(br#"[{"dst":"192.168.1.3","dev":"eth0","prefsrc":"192.168.1.2","gateway":"192.168.1.1"}]"#, ip), Err(UnavailableReason::Unreachable));
    for raw in [
        br#"[]"#.as_slice(),
        br#"[{"dst":"192.168.1.4","dev":"eth0","prefsrc":"192.168.1.2"}]"#,
    ] {
        assert_eq!(
            parse_route(raw, ip),
            Err(UnavailableReason::InconsistentEvidence)
        );
    }
}
#[test]
fn read_only_command_vocabulary_has_no_user_supplied_flags() {
    assert_eq!(
        Query::Tailscale.command(),
        (
            "/usr/bin/tailscale",
            vec!["status".into(), "--json".into(), "--peers=true".into()]
        )
    );
    assert_eq!(
        Query::Addresses.command(),
        (
            "/usr/bin/ip",
            vec!["-j".into(), "-d".into(), "address".into(), "show".into()]
        )
    );
    for ip in ["192.168.1.3", "fd12::3"] {
        let (_, args) = Query::Route(ip.parse().unwrap()).command();
        assert_eq!(&args[1..4], &["-j", "route", "get"]);
        assert_eq!(args[4], ip);
    }
}
#[cfg(target_os = "linux")]
#[tokio::test]
async fn supported_local_discovery_is_bounded_and_never_authenticated() {
    let result = tokio::time::timeout(Duration::from_secs(6), NativeDiscovery.inspect(&[]))
        .await
        .expect("local inspection bounded")
        .unwrap();
    if std::env::var("SYNVEIL_P017_REQUIRE_IP").as_deref() == Ok("1") {
        assert!(
            trusted_program("/usr/bin/ip").is_ok(),
            "CI must provide a trusted iproute2 executable"
        );
        assert!(
            result.lan.is_ok(),
            "supported local address query must parse successfully"
        );
    }
    assert!(!result.summary().authenticated);
    assert!(result.routes.is_empty());
    // This is local metadata evidence only, including a missing CLI. It does
    // not qualify LAN or Tailscale endpoint reachability.
}
#[test]
fn malformed_and_oversized_interface_metadata_fail_closed() {
    assert_eq!(
        parse_lan(b"invalid"),
        Err(UnavailableReason::InconsistentEvidence)
    );
    let entries = vec![json!({}); 65];
    assert_eq!(
        parse_lan(&serde_json::to_vec(&entries).unwrap()),
        Err(UnavailableReason::LimitExceeded)
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn child_output_deadline_and_cancellation_are_bounded() {
    fn child(script: &str) -> tokio::process::Child {
        tokio::process::Command::new("/bin/sh")
            .args(["-c", script])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap()
    }
    assert_eq!(test_child_result(child("while :; do printf '0123456789012345678901234567890123456789012345678901234567890123456789'; done")).await, Err(UnavailableReason::LimitExceeded));
    assert_eq!(test_child_result(child("while :; do printf '0123456789012345678901234567890123456789012345678901234567890123456789' >&2; done")).await, Err(UnavailableReason::LimitExceeded));
    // Shell builtins only: no descendant can hold stdout after child death.
    let process = child("while :; do :; done");
    let pid = process.id().unwrap();
    assert_eq!(
        test_child_result(process).await,
        Err(UnavailableReason::TimedOut)
    );
    for _ in 0..100 {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed-out child was not reaped");
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn dropping_inspection_kills_the_child_and_reaps_it() {
    let process = tokio::process::Command::new("/bin/sh")
        .args(["-c", "while :; do :; done"])
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let pid = process.id().unwrap();
    let future = test_child_result(process);
    let mut future = Box::pin(future);
    tokio::select! { biased; _ = &mut future => panic!("child should stall"), _ = tokio::task::yield_now() => {} }
    drop(future);
    for _ in 0..100 {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("cancelled child was not reaped");
}

#[cfg(not(target_os = "linux"))]
#[tokio::test]
async fn unsupported_native_host_requires_explicit_action() {
    assert_eq!(
        NativeDiscovery.inspect(&[]).await,
        Err(UnavailableReason::Unsupported)
    );
}
