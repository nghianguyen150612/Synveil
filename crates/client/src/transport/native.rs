//! Linux-only local metadata inspector. No packet probes or network mutations.
use super::*;
use serde::Deserialize;
use std::{collections::BTreeSet, process::Stdio};
use synveil_server_network::NetworkCandidate;
use synveil_server_network::transport::{
    RouteEvidence, TailscaleCapability, TailscalePeer, overlay_address,
};
use tokio::io::AsyncReadExt;

const OUTPUT_LIMIT: usize = 128 * 1024;
#[derive(Default)]
pub struct NativeDiscovery;

#[async_trait]
impl LocalDiscovery for NativeDiscovery {
    async fn inspect(
        &self,
        endpoints: &[Endpoint],
    ) -> Result<LocalCapabilities, UnavailableReason> {
        if endpoints.len() > MAX_ENDPOINTS {
            return Err(UnavailableReason::LimitExceeded);
        }
        if !cfg!(target_os = "linux") {
            return Err(UnavailableReason::Unsupported);
        }
        let lan = match run(Query::Addresses).await {
            Ok(bytes) => parse_lan(&bytes),
            Err(e) => Err(e),
        };
        let tailscale = match run(Query::Tailscale).await {
            Ok(bytes) => parse_tailscale(&bytes).unwrap_or_else(TailscaleCapability::Unavailable),
            Err(e) => TailscaleCapability::Unavailable(e),
        };
        let mut routes = Vec::new();
        let destinations: BTreeSet<_> = endpoints.iter().map(|e| e.address).collect();
        for destination in destinations {
            match run(Query::Route(destination)).await {
                Ok(bytes) => match parse_route(&bytes, destination) {
                    Ok(route) => routes.push(route),
                    Err(UnavailableReason::Unreachable) => {}
                    Err(reason) => return Err(reason),
                },
                Err(UnavailableReason::Unreachable) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(LocalCapabilities {
            lan,
            tailscale,
            routes,
        })
    }
}

/// Closed command vocabulary: callers cannot provide executables or flags.
#[derive(Clone, Copy)]
enum Query {
    Addresses,
    Route(IpAddr),
    Tailscale,
}
impl Query {
    fn command(self) -> (&'static str, Vec<String>) {
        match self {
            Self::Addresses => (
                "/usr/bin/ip",
                ["-j", "-d", "address", "show"].map(str::to_owned).to_vec(),
            ),
            Self::Route(ip) => (
                "/usr/bin/ip",
                vec![
                    if ip.is_ipv4() { "-4" } else { "-6" }.to_owned(),
                    "-j".into(),
                    "route".into(),
                    "get".into(),
                    ip.to_string(),
                ],
            ),
            Self::Tailscale => (
                "/usr/bin/tailscale",
                ["status", "--json", "--peers=true"]
                    .map(str::to_owned)
                    .to_vec(),
            ),
        }
    }
}
async fn run(query: Query) -> Result<Vec<u8>, UnavailableReason> {
    let (program, args) = query.command();
    trusted_program(program)?;
    let child = tokio::process::Command::new(program)
        .args(args)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(io_reason)?;
    read_child(child, query).await
}

async fn read_child(
    mut child: tokio::process::Child,
    query: Query,
) -> Result<Vec<u8>, UnavailableReason> {
    let operation = async {
        let stdout = child.stdout.take().ok_or(UnavailableReason::Unsupported)?;
        let stderr = child.stderr.take();
        let (bytes, errors) = tokio::try_join!(read_output(stdout), async {
            match stderr {
                Some(stream) => read_output(stream).await,
                None => Ok(Vec::new()),
            }
        })?;
        if bytes.len() > OUTPUT_LIMIT || errors.len() > OUTPUT_LIMIT {
            return Err(UnavailableReason::LimitExceeded);
        }
        let status = child.wait().await.map_err(io_reason)?;
        if !status.success() {
            return Err(command_failure(query, &errors));
        }

        Ok(bytes)
    };
    tokio::time::timeout(
        if matches!(query, Query::Tailscale) {
            Duration::from_secs(1)
        } else {
            INSPECTION_TIMEOUT
        },
        operation,
    )
    .await
    .map_err(|_| UnavailableReason::TimedOut)?
}

async fn read_output(
    stream: impl tokio::io::AsyncRead + Unpin,
) -> Result<Vec<u8>, UnavailableReason> {
    let mut output = Vec::new();
    stream
        .take((OUTPUT_LIMIT + 1) as u64)
        .read_to_end(&mut output)
        .await
        .map_err(io_reason)?;
    if output.len() > OUTPUT_LIMIT {
        return Err(UnavailableReason::LimitExceeded);
    }
    Ok(output)
}
fn command_failure(query: Query, stderr: &[u8]) -> UnavailableReason {
    // Only exact C-locale kernel outage messages can become an availability
    // failure. Permission, malformed and unknown diagnostics require action.
    if matches!(query, Query::Route(_))
        && [
            "RTNETLINK answers: Network is unreachable",
            "RTNETLINK answers: No route to host",
            "RTNETLINK answers: Network is down",
        ]
        .iter()
        .any(|known| stderr.trim_ascii() == known.as_bytes())
    {
        UnavailableReason::Unreachable
    } else {
        UnavailableReason::PermissionDenied
    }
}

fn io_reason(error: std::io::Error) -> UnavailableReason {
    match error.kind() {
        std::io::ErrorKind::NotFound => UnavailableReason::NotInstalled,
        std::io::ErrorKind::PermissionDenied => UnavailableReason::PermissionDenied,
        _ => UnavailableReason::Unsupported,
    }
}
fn trusted_program(program: &str) -> Result<(), UnavailableReason> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        let mut path = std::path::PathBuf::from("/");
        for part in std::path::Path::new(program).components().skip(1) {
            path.push(part);
            let metadata = std::fs::symlink_metadata(&path).map_err(io_reason)?;
            if metadata.file_type().is_symlink()
                || metadata.uid() != 0
                || metadata.mode() & 0o6022 != 0
            {
                return Err(UnavailableReason::PermissionDenied);
            }
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = program;
        Err(UnavailableReason::Unsupported)
    }
}

fn valid_interface(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn parse_lan(bytes: &[u8]) -> Result<Vec<NetworkCandidate>, UnavailableReason> {
    let entries: Vec<serde_json::Value> =
        serde_json::from_slice(bytes).map_err(|_| UnavailableReason::InconsistentEvidence)?;
    if entries.len() > 64 {
        return Err(UnavailableReason::LimitExceeded);
    }
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in entries {
        let name = entry["ifname"]
            .as_str()
            .filter(|v| valid_interface(v))
            .ok_or(UnavailableReason::InconsistentEvidence)?;
        if !seen.insert(name.to_owned()) {
            return Err(UnavailableReason::InconsistentEvidence);
        }
        let link = entry["linkinfo"]["info_kind"].as_str().unwrap_or("");
        // Match server-host's conservative physical-adapter boundary. Tunnels,
        // container bridges and unknown virtual adapters never become LAN.
        let base = std::path::Path::new("/sys/class/net").join(name);
        let kind = synveil_server_network::classify_interface(
            name,
            link,
            base.join("bridge").is_dir(),
            base.join("wireless").is_dir(),
            base.join("device").exists(),
        );
        let addresses = entry["addr_info"]
            .as_array()
            .ok_or(UnavailableReason::InconsistentEvidence)?;
        for addr in addresses {
            if candidates.len() >= 128 {
                return Err(UnavailableReason::LimitExceeded);
            }
            let address: IpAddr = addr["local"]
                .as_str()
                .and_then(|v| v.parse().ok())
                .ok_or(UnavailableReason::InconsistentEvidence)?;
            let prefix_length = addr["prefixlen"]
                .as_u64()
                .filter(|v| *v <= if address.is_ipv4() { 32 } else { 128 })
                .ok_or(UnavailableReason::InconsistentEvidence)?
                as u8;
            candidates.push(NetworkCandidate {
                interface_id: name.to_owned(),
                address,
                prefix_length,
                kind,
                operational: entry["operstate"].as_str() == Some("UP"),
                default_route: false,
            });
        }
    }
    Ok(candidates)
}
fn parse_route(bytes: &[u8], destination: IpAddr) -> Result<RouteEvidence, UnavailableReason> {
    let routes: Vec<serde_json::Value> =
        serde_json::from_slice(bytes).map_err(|_| UnavailableReason::InconsistentEvidence)?;
    let [r] = routes.as_slice() else {
        return Err(UnavailableReason::InconsistentEvidence);
    };
    if r.get("gateway").is_some() || r["type"].as_str().is_some_and(|s| s != "unicast") {
        return Err(UnavailableReason::Unreachable);
    }
    if r["dst"].as_str().and_then(|s| s.parse::<IpAddr>().ok()) != Some(destination) {
        return Err(UnavailableReason::InconsistentEvidence);
    }
    let source = r["prefsrc"]
        .as_str()
        .or_else(|| r["src"].as_str())
        .and_then(|v| v.parse().ok())
        .ok_or(UnavailableReason::InconsistentEvidence)?;
    let interface_id = r["dev"]
        .as_str()
        .filter(|v| valid_interface(v))
        .ok_or(UnavailableReason::InconsistentEvidence)?
        .to_owned();
    Ok(RouteEvidence {
        destination,
        source,
        interface_id,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Status {
    backend_state: String,
    #[serde(rename = "TUN")]
    tun: Option<bool>,
    #[serde(rename = "Self")]
    local: Option<Peer>,
    peer: Option<std::collections::BTreeMap<String, Peer>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Peer {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "TailscaleIPs")]
    ips: Vec<IpAddr>,
    online: bool,
    #[serde(default)]
    expired: bool,
    key_expiry: Option<String>,
}
fn expired(peer: &Peer) -> Result<bool, UnavailableReason> {
    if let Some(expiry) = &peer.key_expiry {
        let expiry =
            time::OffsetDateTime::parse(expiry, &time::format_description::well_known::Rfc3339)
                .map_err(|_| UnavailableReason::Unsupported)?;
        return Ok(peer.expired || expiry <= time::OffsetDateTime::now_utc());
    }
    Ok(peer.expired)
}
fn parse_tailscale(bytes: &[u8]) -> Result<TailscaleCapability, UnavailableReason> {
    let s: Status = serde_json::from_slice(bytes).map_err(|_| UnavailableReason::Unsupported)?;
    match s.backend_state.as_str() {
        "NeedsLogin" | "NoState" => return Ok(TailscaleCapability::Installed),
        "Stopped" => return Ok(TailscaleCapability::ConfiguredDisconnected),
        "NeedsMachineAuth" => return Err(UnavailableReason::PermissionDenied),
        "Running" => {}
        _ => return Err(UnavailableReason::Unsupported),
    }
    if s.tun != Some(true) {
        return Err(UnavailableReason::Unsupported);
    }
    let local = s.local.ok_or(UnavailableReason::InconsistentEvidence)?;
    if expired(&local)? {
        return Err(UnavailableReason::EnrollmentExpired);
    }
    if !local.online {
        return Ok(TailscaleCapability::ConfiguredDisconnected);
    }
    let mut ids = BTreeSet::new();
    let mut ips = BTreeSet::new();
    let mut peers = Vec::new();
    let all = s.peer.ok_or(UnavailableReason::Unsupported)?;
    if all.len() > 256 {
        return Err(UnavailableReason::LimitExceeded);
    }
    for peer in std::iter::once(&local).chain(all.values()) {
        if peer.id.is_empty()
            || peer.id.len() > 128
            || !ids.insert(peer.id.clone())
            || peer.ips.is_empty()
            || peer.ips.len() > 8
            || peer
                .ips
                .iter()
                .any(|ip| !overlay_address(*ip) || !ips.insert(*ip))
        {
            return Err(UnavailableReason::IdentityConflict);
        }
        let is_expired = expired(peer)?;
        if peer.id != local.id {
            peers.push(TailscalePeer {
                node_id: peer.id.clone(),
                addresses: peer.ips.clone(),
                online: peer.online,
                expired: is_expired,
            });
        }
    }
    Ok(TailscaleCapability::Connected {
        local_ips: local.ips,
        peers,
    })
}

#[cfg(test)]
mod tests;
