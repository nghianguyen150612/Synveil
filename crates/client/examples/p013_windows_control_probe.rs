//! P013 acceptance probe for an already running installed Windows client.
//!
//! This is a harness executable built from the exact source revision under
//! test. It never starts a server or substitutes a mock service. In `owner`
//! mode it reads only the installed user's non-secret profile ID and uses the
//! production client/protocol implementation to reach the installed client.
//! In `wrong-user` mode the caller supplies the profile ID discovered by the
//! owner-mode run; the probe expects the OS to deny opening that user's pipe.

use std::collections::BTreeMap;

#[cfg(windows)]
use std::time::Duration;

use serde::Serialize;
#[cfg(windows)]
use synveil_client::{
    ControlClientHello, ControlErrorCode, ControlServerHello, DESKTOP_CONTROL_MAX_FRAME_BYTES,
    DESKTOP_CONTROL_PROTOCOL_VERSION, DesktopClientConfig, DesktopControlClient,
    DesktopControlEndpoint, DesktopProcessStatus, ServerProfileId, read_frame, write_frame,
};
#[cfg(windows)]
use tokio::io::AsyncReadExt;
#[cfg(windows)]
use tokio::time;

#[derive(Serialize)]
struct ProbeEvidence {
    schema_version: u8,
    mode: &'static str,
    result: &'static str,
    checks: BTreeMap<&'static str, &'static str>,
}

#[cfg(windows)]
fn status(result: Result<(), &'static str>) -> &'static str {
    if result.is_ok() { "PASS" } else { "FAIL" }
}

fn emit(mode: &'static str, checks: BTreeMap<&'static str, &'static str>) -> i32 {
    let result = if checks.values().any(|value| *value == "FAIL") {
        "FAIL"
    } else if checks.values().all(|value| *value == "PASS") {
        "PASS"
    } else if checks
        .values()
        .any(|value| *value == "BLOCKED_BY_ENVIRONMENT")
    {
        "BLOCKED_BY_ENVIRONMENT"
    } else {
        "INCOMPLETE"
    };
    let evidence = ProbeEvidence {
        schema_version: 1,
        mode,
        result,
        checks,
    };
    match serde_json::to_string(&evidence) {
        Ok(json) => println!("{json}"),
        Err(_) => {
            println!(r#"{{"schema_version":1,"mode":"invalid","result":"FAIL","checks":{{}}}}"#)
        }
    }
    match result {
        "PASS" => 0,
        "BLOCKED_BY_ENVIRONMENT" => 2,
        _ => 1,
    }
}

#[cfg(windows)]
fn parse_profile_id(value: &str) -> Result<ServerProfileId, &'static str> {
    ServerProfileId::parse_str(value).map_err(|_| "profile_id_invalid")
}

#[cfg(windows)]
fn named_pipe(endpoint: &DesktopControlEndpoint) -> Result<&str, &'static str> {
    endpoint
        .named_pipe()
        .ok_or("named_pipe_endpoint_unavailable")
}

#[cfg(windows)]
async fn raw_pipe(
    name: &str,
) -> Result<tokio::net::windows::named_pipe::NamedPipeClient, &'static str> {
    tokio::net::windows::named_pipe::ClientOptions::new()
        .read(true)
        .write(true)
        .open(name)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::PermissionDenied => "pipe_access_denied",
            std::io::ErrorKind::NotFound => "pipe_unavailable",
            _ => "pipe_connection_failed",
        })
}

#[cfg(windows)]
async fn client_bounded(
    endpoint: DesktopControlEndpoint,
) -> Result<DesktopControlClient, &'static str> {
    time::timeout(
        Duration::from_secs(3),
        DesktopControlClient::connect(endpoint),
    )
    .await
    .map_err(|_| "client_connect_timeout")?
    .map_err(|_| "client_connect_failed")
}

#[cfg(windows)]
async fn owner_protocol(profile_id: ServerProfileId) -> BTreeMap<&'static str, &'static str> {
    let platform = synveil_platform::current();
    let endpoint = match DesktopControlEndpoint::for_profile(platform.as_ref(), profile_id) {
        Ok(endpoint) => endpoint,
        Err(_) => return BTreeMap::from([("pipe_endpoint_discovery", "FAIL")]),
    };
    let pipe_name = match named_pipe(&endpoint) {
        Ok(name) => name,
        Err(_) => return BTreeMap::from([("pipe_endpoint_discovery", "FAIL")]),
    };

    let mut checks = BTreeMap::new();
    checks.insert("pipe_endpoint_discovery", "PASS");

    let ping = async {
        let mut client = client_bounded(endpoint.clone()).await?;
        let status = time::timeout(Duration::from_secs(3), client.ping())
            .await
            .map_err(|_| "ping_timeout")?
            .map_err(|_| "ping")?;
        if status != DesktopProcessStatus::Running {
            return Err("server_not_running");
        }
        Ok::<(), &'static str>(())
    }
    .await;
    checks.insert("pipe_protocol_v1", status(ping));
    checks.insert("pipe_ping", status(ping));

    let unsupported = async {
        let mut pipe = raw_pipe(pipe_name).await?;
        write_frame(
            &mut pipe,
            &ControlClientHello {
                protocol_version: u16::MAX,
            },
        )
        .await
        .map_err(|_| "write_unsupported_version")?;
        let payload = time::timeout(Duration::from_secs(6), read_frame(&mut pipe))
            .await
            .map_err(|_| "unsupported_version_timeout")?
            .map_err(|_| "unsupported_version_closed")?;
        let hello: ControlServerHello =
            serde_json::from_slice(&payload).map_err(|_| "unsupported_version_malformed_reply")?;
        if hello.protocol_version != DESKTOP_CONTROL_PROTOCOL_VERSION
            || hello.error != Some(ControlErrorCode::ProtocolVersionUnsupported)
            || !hello.capabilities.is_empty()
        {
            return Err("unsupported_version_contract_mismatch");
        }
        Ok::<(), &'static str>(())
    }
    .await;
    checks.insert("pipe_unsupported_version", status(unsupported));

    let malformed = async {
        let mut pipe = raw_pipe(pipe_name).await?;
        let frame = synveil_client::encode_frame(b"{").map_err(|_| "malformed_frame_encode")?;
        tokio::io::AsyncWriteExt::write_all(&mut pipe, &frame)
            .await
            .map_err(|_| "malformed_write")?;
        expect_close(&mut pipe, Duration::from_secs(2)).await
    }
    .await;
    checks.insert("pipe_malformed_message", status(malformed));

    let oversized = async {
        let mut pipe = raw_pipe(pipe_name).await?;
        let oversized = u32::try_from(DESKTOP_CONTROL_MAX_FRAME_BYTES + 1)
            .map_err(|_| "oversized_length_invalid")?
            .to_be_bytes();
        tokio::io::AsyncWriteExt::write_all(&mut pipe, &oversized)
            .await
            .map_err(|_| "oversized_header_write")?;
        tokio::io::AsyncWriteExt::flush(&mut pipe)
            .await
            .map_err(|_| "oversized_flush")?;
        expect_close(&mut pipe, Duration::from_secs(2)).await
    }
    .await;
    checks.insert("pipe_oversized_payload", status(oversized));

    let server_timeout = async {
        let mut pipe = raw_pipe(pipe_name).await?;
        expect_close(&mut pipe, Duration::from_secs(7)).await
    }
    .await;
    checks.insert("pipe_server_handshake_timeout", status(server_timeout));

    let duplicate = async {
        let mut first = client_bounded(endpoint.clone()).await?;
        let mut second = client_bounded(endpoint.clone()).await?;
        for client in [&mut first, &mut second] {
            let response = time::timeout(Duration::from_secs(3), client.ping())
                .await
                .map_err(|_| "duplicate_ping_timeout")?
                .map_err(|_| "duplicate_ping")?;
            if response != DesktopProcessStatus::Running {
                return Err("duplicate_status_mismatch");
            }
        }
        Ok::<(), &'static str>(())
    }
    .await;
    checks.insert("pipe_duplicate_connection", status(duplicate));

    let cancellation = async {
        let mut pipe = raw_pipe(pipe_name).await?;
        match time::timeout(Duration::from_millis(150), read_frame(&mut pipe)).await {
            Err(_) => Ok(()),
            Ok(_) => Err("pending_handshake_did_not_cancel"),
        }
    }
    .await;
    checks.insert("pipe_timeout_cancellation", status(cancellation));
    checks
}

#[cfg(windows)]
async fn owner_ping(profile_id: ServerProfileId) -> BTreeMap<&'static str, &'static str> {
    let platform = synveil_platform::current();
    let endpoint = match DesktopControlEndpoint::for_profile(platform.as_ref(), profile_id) {
        Ok(endpoint) => endpoint,
        Err(_) => return BTreeMap::from([("pipe_endpoint_discovery", "FAIL")]),
    };
    let result = async {
        let mut client = client_bounded(endpoint).await?;
        let response = time::timeout(Duration::from_secs(3), client.ping())
            .await
            .map_err(|_| "ping_timeout")?
            .map_err(|_| "ping")?;
        if response != DesktopProcessStatus::Running {
            return Err("server_not_running");
        }
        Ok::<(), &'static str>(())
    }
    .await;
    BTreeMap::from([
        ("pipe_endpoint_discovery", "PASS"),
        ("pipe_protocol_v1", status(result)),
        ("pipe_ping", status(result)),
    ])
}

#[cfg(windows)]
async fn expect_close(
    pipe: &mut tokio::net::windows::named_pipe::NamedPipeClient,
    timeout: Duration,
) -> Result<(), &'static str> {
    let mut byte = [0_u8; 1];
    match time::timeout(timeout, pipe.read(&mut byte)).await {
        Ok(Ok(0)) => Ok(()),
        Ok(Ok(_)) => Err("unexpected_pipe_response"),
        Ok(Err(error))
            if matches!(
                error.kind(),
                std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::ConnectionReset
            ) =>
        {
            Ok(())
        }
        Ok(Err(_)) => Err("pipe_read_failed"),
        Err(_) => Err("pipe_close_timeout"),
    }
}

#[cfg(windows)]
async fn wrong_user(profile_id: ServerProfileId) -> BTreeMap<&'static str, &'static str> {
    let platform = synveil_platform::current();
    let endpoint = match DesktopControlEndpoint::for_profile(platform.as_ref(), profile_id) {
        Ok(endpoint) => endpoint,
        Err(_) => return BTreeMap::from([("pipe_wrong_user", "FAIL")]),
    };
    let name = match named_pipe(&endpoint) {
        Ok(name) => name,
        Err(_) => return BTreeMap::from([("pipe_wrong_user", "FAIL")]),
    };
    let check = match raw_pipe(name).await {
        Err("pipe_access_denied") => Ok(()),
        Err("pipe_unavailable") => Err("endpoint_unavailable"),
        Err(_) => Err("connection_failure_not_authorization"),
        Ok(_) => Err("unrelated_user_connected"),
    };
    BTreeMap::from([("pipe_wrong_user", status(check))])
}

#[cfg(not(windows))]
fn unsupported_platform() -> i32 {
    emit(
        "unsupported_platform",
        BTreeMap::from([("windows_named_pipe", "BLOCKED_BY_ENVIRONMENT")]),
    )
}

#[tokio::main]
async fn main() {
    #[cfg(windows)]
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    #[cfg(windows)]
    {
        let code = match args.as_slice() {
            [mode] if mode == "owner" => {
                let platform = synveil_platform::current();
                match DesktopClientConfig::existing_profile_id(platform.as_ref()) {
                    Ok(Some(profile_id)) => emit("owner", owner_protocol(profile_id).await),
                    _ => emit(
                        "owner",
                        BTreeMap::from([("pipe_endpoint_discovery", "FAIL")]),
                    ),
                }
            }
            [mode] if mode == "ping" => {
                let platform = synveil_platform::current();
                match DesktopClientConfig::existing_profile_id(platform.as_ref()) {
                    Ok(Some(profile_id)) => emit("ping", owner_ping(profile_id).await),
                    _ => emit(
                        "ping",
                        BTreeMap::from([("pipe_endpoint_discovery", "FAIL")]),
                    ),
                }
            }
            [mode, profile] if mode == "wrong-user" => match parse_profile_id(profile) {
                Ok(profile_id) => emit("wrong-user", wrong_user(profile_id).await),
                Err(_) => emit("wrong-user", BTreeMap::from([("pipe_wrong_user", "FAIL")])),
            },
            [mode] if mode == "shutdown" => {
                let approved = std::env::var("P013_OPERATOR_APPROVAL").ok().as_deref()
                    == Some("I_APPROVE_DISPOSABLE_SYNVEIL_WINDOWS_ACCEPTANCE");
                if !approved {
                    emit("shutdown", BTreeMap::from([("operator_approval", "FAIL")]));
                    std::process::exit(1);
                }
                let platform = synveil_platform::current();
                match DesktopClientConfig::existing_profile_id(platform.as_ref()) {
                    Ok(Some(profile_id)) => emit("shutdown", shutdown_server(profile_id).await),
                    _ => emit(
                        "shutdown",
                        BTreeMap::from([("pipe_server_termination", "FAIL")]),
                    ),
                }
            }
            [mode] if mode == "absent" => {
                let platform = synveil_platform::current();
                match DesktopClientConfig::existing_profile_id(platform.as_ref()) {
                    Ok(Some(profile_id)) => emit("absent", absent_endpoint(profile_id).await),
                    _ => emit(
                        "absent",
                        BTreeMap::from([("pipe_expired_endpoint", "FAIL")]),
                    ),
                }
            }
            [mode] if mode == "stale" => {
                let profile_id = ServerProfileId::new();
                emit("stale", stale_profile(profile_id).await)
            }
            _ => emit("invalid_arguments", BTreeMap::from([("arguments", "FAIL")])),
        };
        std::process::exit(code);
    }
    #[cfg(not(windows))]
    {
        std::process::exit(unsupported_platform());
    }
}

#[cfg(windows)]
async fn shutdown_server(profile_id: ServerProfileId) -> BTreeMap<&'static str, &'static str> {
    let platform = synveil_platform::current();
    let endpoint = match DesktopControlEndpoint::for_profile(platform.as_ref(), profile_id) {
        Ok(endpoint) => endpoint,
        Err(_) => return BTreeMap::from([("pipe_server_termination", "FAIL")]),
    };
    let result = async {
        let mut client = client_bounded(endpoint).await?;
        client.shutdown().await.map_err(|_| "shutdown_not_accepted")
    }
    .await;
    BTreeMap::from([("pipe_server_termination", status(result))])
}

#[cfg(windows)]
async fn absent_endpoint(profile_id: ServerProfileId) -> BTreeMap<&'static str, &'static str> {
    let platform = synveil_platform::current();
    let endpoint = match DesktopControlEndpoint::for_profile(platform.as_ref(), profile_id) {
        Ok(endpoint) => endpoint,
        Err(_) => return BTreeMap::from([("pipe_expired_endpoint", "FAIL")]),
    };
    let result = match named_pipe(&endpoint) {
        Ok(name) => match raw_pipe(name).await {
            Err("pipe_unavailable") => Ok(()),
            Err(_) => Err("unexpected_pipe_open_error"),
            Ok(_) => Err("endpoint_still_available"),
        },
        Err(_) => Err("named_pipe_endpoint_unavailable"),
    };
    BTreeMap::from([("pipe_expired_endpoint", status(result))])
}

#[cfg(windows)]
async fn stale_profile(profile_id: ServerProfileId) -> BTreeMap<&'static str, &'static str> {
    let platform = synveil_platform::current();
    let endpoint = match DesktopControlEndpoint::for_profile(platform.as_ref(), profile_id) {
        Ok(endpoint) => endpoint,
        Err(_) => return BTreeMap::from([("pipe_stale_profile", "FAIL")]),
    };
    let check = match time::timeout(
        Duration::from_secs(3),
        DesktopControlClient::connect(endpoint),
    )
    .await
    {
        Err(_) => Err("stale_endpoint_timeout"),
        Ok(Err(_)) => Ok(()),
        Ok(Ok(_)) => Err("stale_endpoint_connected"),
    };
    BTreeMap::from([("pipe_stale_profile", status(check))])
}
