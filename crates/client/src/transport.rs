//! Opt-in, read-only transport discovery. Not wired into the production Wizard.
//! Adapter implementations are trusted ports; discovery metadata is never auth.
use async_trait::async_trait;
use std::{net::IpAddr, time::Duration};
use synveil_client_sync::{
    ConnectionHealth, HttpClientConfig, HttpSyncRemote, LoadedDeviceCredential, ReplicaScope,
    ServerProfile,
};
use synveil_server_network::transport::{LocalCapabilities, Transport, UnavailableReason};
use tokio::sync::watch;

pub mod native;

pub const MAX_ENDPOINTS: usize = 8;
pub const INSPECTION_TIMEOUT: Duration = Duration::from_secs(2);
pub const SELECTION_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SelectionPolicy {
    #[default]
    LanOnly,
    PreferLan,
    PreferTailscale,
    TailscaleOnly,
}

/// Explicitly approved binding, derived from an existing profile's HostTrust.
/// No discovery advertisement can construct this value by itself.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Endpoint {
    transport: Transport,
    origin: String,
    server_id: String,
    network_id: String,
    trust_generation: u64,
    ca_sha256: Option<String>,
    address: IpAddr,
    peer_id: Option<String>,
}
impl Endpoint {
    pub fn from_profile(
        profile: &ServerProfile,
        transport: Transport,
        peer_id: Option<String>,
    ) -> Result<Self, UnavailableReason> {
        let trust = profile
            .managed_host_trust()
            .ok_or(UnavailableReason::UntrustedEndpoint)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| UnavailableReason::UntrustedEndpoint)?
            .as_secs() as i64;
        trust
            .validate(profile.base_url().as_str(), now)
            .map_err(|_| UnavailableReason::UntrustedEndpoint)?;
        let url =
            url::Url::parse(trust.origin()).map_err(|_| UnavailableReason::UntrustedEndpoint)?;
        // DNS/route pinning is not yet qualified. Only explicit numeric origins.
        let address = url
            .host_str()
            .and_then(|s| s.trim_matches(['[', ']']).parse().ok())
            .ok_or(UnavailableReason::Unsupported)?;
        if !match transport {
            Transport::Lan => {
                synveil_server_network::transport::private_lan_address(address) && peer_id.is_none()
            }
            Transport::Tailscale => {
                synveil_server_network::transport::overlay_address(address)
                    && peer_id
                        .as_ref()
                        .is_some_and(|s| !s.is_empty() && s.len() <= 128)
            }
            Transport::Unavailable => false,
        } {
            return Err(UnavailableReason::UntrustedEndpoint);
        }
        Ok(Self {
            transport,
            origin: trust.origin().to_owned(),
            server_id: trust.server_id().to_owned(),
            network_id: trust.network_id().to_owned(),
            trust_generation: trust.generation(),
            ca_sha256: trust.ca_sha256().map(str::to_owned),
            address,
            peer_id,
        })
    }
    pub fn origin(&self) -> &str {
        &self.origin
    }
    pub fn transport(&self) -> Transport {
        self.transport
    }
    pub fn address(&self) -> IpAddr {
        self.address
    }
}

#[async_trait]
pub trait LocalDiscovery: Send + Sync {
    /// Must not enroll, ping, install, mutate network state or request elevation.
    async fn inspect(&self, endpoints: &[Endpoint])
    -> Result<LocalCapabilities, UnavailableReason>;
}
#[async_trait]
pub trait EndpointVerifier: Send + Sync {
    /// Trusted port: success requires current Synveil auth AND server identity.
    /// Must not exchange enrollment grants or mutate remote/application state.
    async fn verify(&self, endpoint: &Endpoint) -> Result<(), UnavailableReason>;
}

/// Reuses the existing canonical HTTPS transport, profile-bound credential,
/// anonymous readiness and authenticated owner/device checkpoint handshake.
pub struct SynveilVerifier {
    endpoints: Vec<VerifiedBinding>,
}
struct VerifiedBinding {
    endpoint: Endpoint,
    remote: HttpSyncRemote,
    scope: ReplicaScope,
    trust: synveil_core::host_trust::HostTrust,
}

impl SynveilVerifier {
    pub fn new() -> Self {
        Self {
            endpoints: Vec::new(),
        }
    }
    pub fn add(
        &mut self,
        profile: ServerProfile,
        transport: Transport,
        peer_id: Option<String>,
        credential: LoadedDeviceCredential,
        scope: ReplicaScope,
    ) -> Result<Endpoint, UnavailableReason> {
        if self.endpoints.len() >= MAX_ENDPOINTS {
            return Err(UnavailableReason::LimitExceeded);
        }
        let endpoint = Endpoint::from_profile(&profile, transport, peer_id)?;
        if credential.owner_user_id() != scope.owner_user_id()
            || credential.device_id() != scope.device_id()
        {
            return Err(UnavailableReason::AuthenticationRequired);
        }
        let trust = profile
            .managed_host_trust()
            .ok_or(UnavailableReason::UntrustedEndpoint)?
            .clone();
        let remote = HttpSyncRemote::new(
            profile,
            scope.device_id(),
            credential,
            HttpClientConfig::default(),
        )
        .map_err(|_| UnavailableReason::UntrustedEndpoint)?;
        if self
            .endpoints
            .iter()
            .any(|b| b.endpoint.origin == endpoint.origin)
        {
            return Err(UnavailableReason::IdentityConflict);
        }
        self.endpoints.push(VerifiedBinding {
            endpoint: endpoint.clone(),
            remote,
            scope,
            trust,
        });
        Ok(endpoint)
    }
}
impl Default for SynveilVerifier {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl EndpointVerifier for SynveilVerifier {
    async fn verify(&self, endpoint: &Endpoint) -> Result<(), UnavailableReason> {
        let binding = self
            .endpoints
            .iter()
            .find(|b| &b.endpoint == endpoint)
            .ok_or(UnavailableReason::UntrustedEndpoint)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| UnavailableReason::UntrustedEndpoint)?
            .as_secs() as i64;
        binding
            .trust
            .validate(endpoint.origin(), now)
            .map_err(|_| UnavailableReason::UntrustedEndpoint)?;
        match binding.remote.probe_health(binding.scope).await {
            ConnectionHealth::Online => Ok(()),
            ConnectionHealth::AuthRequired => Err(UnavailableReason::AuthenticationRequired),
            ConnectionHealth::DeviceRevoked => Err(UnavailableReason::AccessRevoked),
            ConnectionHealth::ServerUnavailable => Err(UnavailableReason::Unreachable),
            ConnectionHealth::TlsError | ConnectionHealth::ProtocolError => {
                Err(UnavailableReason::UntrustedEndpoint)
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Selection {
    /// Ephemeral advisory result only. Consumers must use normal authenticated
    /// operations; this result is neither a reusable session nor a permission.
    Selected {
        transport: Transport,
        origin: String,
    },
    Unavailable(UnavailableReason),
}
impl Selection {
    pub fn transport(&self) -> Transport {
        match self {
            Self::Selected { transport, .. } => *transport,
            Self::Unavailable(_) => Transport::Unavailable,
        }
    }
}

/// At most two GET-only handshakes per endpoint, eight endpoints, two local
/// snapshots and fifteen seconds total. Dropping the future cancels the work.
pub async fn discover_and_select(
    discovery: &dyn LocalDiscovery,
    verifier: &dyn EndpointVerifier,
    endpoints: &[Endpoint],
    policy: SelectionPolicy,
    cancel: watch::Receiver<bool>,
) -> Selection {
    if *cancel.borrow() {
        return Selection::Unavailable(UnavailableReason::Cancelled);
    }
    tokio::select! {
        biased;
        _ = cancelled(cancel) => Selection::Unavailable(UnavailableReason::Cancelled),
        result = tokio::time::timeout(SELECTION_TIMEOUT, select_inner(discovery, verifier, endpoints, policy)) => result.unwrap_or(Selection::Unavailable(UnavailableReason::TimedOut)),
    }
}
async fn cancelled(mut cancel: watch::Receiver<bool>) {
    loop {
        let requested = *cancel.borrow_and_update();
        if requested {
            return;
        }
        if cancel.changed().await.is_err() {
            return;
        }
    }
}
async fn inspect(
    discovery: &dyn LocalDiscovery,
    endpoints: &[Endpoint],
) -> Result<LocalCapabilities, UnavailableReason> {
    tokio::time::timeout(INSPECTION_TIMEOUT, discovery.inspect(endpoints))
        .await
        .map_err(|_| UnavailableReason::TimedOut)?
}
async fn select_inner(
    discovery: &dyn LocalDiscovery,
    verifier: &dyn EndpointVerifier,
    endpoints: &[Endpoint],
    policy: SelectionPolicy,
) -> Selection {
    use UnavailableReason as R;
    let result = async {
        if endpoints.is_empty() {
            return Err(R::NotConfigured);
        }
        if endpoints.len() > MAX_ENDPOINTS {
            return Err(R::LimitExceeded);
        }
        // No implicit identity migration or conflicting origin binding.
        if endpoints
            .iter()
            .any(|e| e.server_id != endpoints[0].server_id)
        {
            return Err(R::IdentityConflict);
        }
        let mut ordered: Vec<_> = endpoints
            .iter()
            .filter(|e| match policy {
                SelectionPolicy::LanOnly => e.transport == Transport::Lan,
                SelectionPolicy::TailscaleOnly => e.transport == Transport::Tailscale,
                _ => true,
            })
            .collect();
        if ordered.is_empty() {
            return Err(R::NotConfigured);
        }
        ordered.sort_by_key(|e| {
            (
                match policy {
                    SelectionPolicy::PreferTailscale => e.transport != Transport::Tailscale,
                    _ => e.transport != Transport::Lan,
                },
                &e.origin,
            )
        });
        if ordered.windows(2).any(|e| e[0].origin == e[1].origin) {
            return Err(R::IdentityConflict);
        }
        let inspected_endpoints: Vec<_> = ordered.iter().map(|e| (*e).clone()).collect();
        let before = inspect(discovery, &inspected_endpoints).await?;
        let mut last = R::Unreachable;
        for e in ordered {
            if let Err(reason) = before.path_available(e.transport, e.address, e.peer_id.as_deref())
            {
                // Bad identity/evidence is never an outage-driven fallback.
                if !matches!(
                    reason,
                    R::NotInstalled | R::NotConfigured | R::Disconnected | R::Unreachable
                ) {
                    return Err(reason);
                }
                last = reason;
                continue;
            }
            let mut verified = false;
            for _ in 0..2 {
                match verifier.verify(e).await {
                    Ok(()) => {
                        verified = true;
                        break;
                    }
                    Err(R::Unreachable) => last = R::Unreachable,
                    Err(reason) => return Err(reason),
                }
            }
            if !verified {
                continue;
            }
            let after = inspect(discovery, &inspected_endpoints).await?;
            if before != after {
                return Err(R::RouteChanged);
            }
            after.path_available(e.transport, e.address, e.peer_id.as_deref())?;
            return Ok(Selection::Selected {
                transport: e.transport,
                origin: e.origin.clone(),
            });
        }
        Err(last)
    }
    .await;
    result.unwrap_or_else(Selection::Unavailable)
}

#[cfg(test)]
mod tests;
