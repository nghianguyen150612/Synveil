#![forbid(unsafe_code)]

//! UI-neutral, side-effect-free planning for managed server reachability.
//! Platform adapters consume a confirmed plan only after re-inspection.

use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use synveil_server_config::{FirewallManager, NetworkIntegrationId, ReachabilityMode};

pub const MANAGED_BACKEND: &str = "127.0.0.1:3000";
pub const MANAGED_HTTPS_PORT: u16 = 443;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceKind {
    Ethernet,
    Wifi,
    ContainerBridge,
    Tunnel,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetworkCandidate {
    pub interface_id: String,
    pub address: IpAddr,
    pub prefix_length: u8,
    pub kind: InterfaceKind,
    pub operational: bool,
    pub default_route: bool,
}

impl NetworkCandidate {
    #[must_use]
    pub fn suitable_private_lan(&self) -> bool {
        self.operational
            && !matches!(
                self.kind,
                InterfaceKind::ContainerBridge | InterfaceKind::Tunnel
            )
            && matches!(self.address, IpAddr::V4(ip) if private_v4(ip))
            && self.prefix_length <= 32
            && !self.interface_id.is_empty()
    }
}

fn private_v4(ip: Ipv4Addr) -> bool {
    ip.is_private() && !ip.is_loopback() && !ip.is_link_local() && !ip.is_broadcast()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetworkObservation {
    pub candidates: Vec<NetworkCandidate>,
    pub occupied_listeners: Vec<(IpAddr, u16)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CandidateProposal<'a> {
    None,
    One(&'a NetworkCandidate),
    ChoiceRequired(Vec<&'a NetworkCandidate>),
}

#[must_use]
pub fn propose_private_lan(observed: &NetworkObservation) -> CandidateProposal<'_> {
    let candidates: Vec<_> = observed
        .candidates
        .iter()
        .filter(|c| c.suitable_private_lan())
        .collect();
    match candidates.as_slice() {
        [] => CandidateProposal::None,
        [one] => CandidateProposal::One(one),
        _ => CandidateProposal::ChoiceRequired(candidates),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustMode {
    ManagedPrivateCa,
    PublicWebPki,
    OperatorManagedHttps,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum FirewallPlan {
    None,
    CreateOwned {
        manager: FirewallManager,
        rule_id: String,
        interface_id: String,
        port: u16,
    },
    OperatorOwned,
    NeedsAttention,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetworkInstallPlan {
    pub network_integration_id: NetworkIntegrationId,
    pub server_installation_id: String,
    pub service_integration_id: String,
    pub config_generation: u64,
    pub config_fingerprint: String,
    pub mode: ReachabilityMode,
    pub backend_endpoint: String,
    pub listener: IpAddr,
    pub port: u16,
    pub interface_id: Option<String>,
    pub canonical_origin: String,
    pub trust_mode: TrustMode,
    pub firewall: FirewallPlan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanError {
    CandidateNotObserved,
    CandidateNotSuitable,
    PortInUse,
    InvalidOrigin,
    InconsistentMode,
}

pub fn plan_private_lan(
    observed: &NetworkObservation,
    selected: &NetworkCandidate,
    mut plan: NetworkInstallPlan,
) -> Result<NetworkInstallPlan, PlanError> {
    if !observed.candidates.contains(selected) {
        return Err(PlanError::CandidateNotObserved);
    }
    if !selected.suitable_private_lan() {
        return Err(PlanError::CandidateNotSuitable);
    }
    if observed
        .occupied_listeners
        .contains(&(selected.address, plan.port))
    {
        return Err(PlanError::PortInUse);
    }
    if plan.mode != ReachabilityMode::PrivateLan || plan.trust_mode != TrustMode::ManagedPrivateCa {
        return Err(PlanError::InconsistentMode);
    }
    validate_origin(&plan.canonical_origin, Some(selected.address), plan.port)?;
    plan.backend_endpoint = MANAGED_BACKEND.to_owned();
    plan.listener = selected.address;
    plan.interface_id = Some(selected.interface_id.clone());
    Ok(plan)
}

pub fn validate_origin(
    value: &str,
    expected_ip: Option<IpAddr>,
    port: u16,
) -> Result<(), PlanError> {
    let url = url::Url::parse(value).map_err(|_| PlanError::InvalidOrigin)?;
    if url.scheme() != "https"
        || url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || url.port_or_known_default() != Some(port)
        || expected_ip.is_some_and(|ip| url.host_str().and_then(|v| v.parse().ok()) != Some(ip))
    {
        return Err(PlanError::InvalidOrigin);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReachabilityStatus {
    OnlyThisDevice,
    AvailableOnLocalNetwork,
    RemoteHttpsReady,
    NetworkUnavailable,
    AddressChanged,
    PortInUse,
    CertificateNeedsAttention,
    FirewallNeedsAttention,
    EdgeNeedsRepair,
    ExternalProxyNeedsAttention,
    NotYetQualified,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn candidate(id: &str, ip: &str, kind: InterfaceKind) -> NetworkCandidate {
        NetworkCandidate {
            interface_id: id.into(),
            address: ip.parse().unwrap(),
            prefix_length: 24,
            kind,
            operational: true,
            default_route: true,
        }
    }
    #[test]
    fn candidate_selection_never_guesses() {
        let one = NetworkObservation {
            candidates: vec![candidate("eth0", "192.168.1.5", InterfaceKind::Ethernet)],
            occupied_listeners: vec![],
        };
        assert!(matches!(
            propose_private_lan(&one),
            CandidateProposal::One(_)
        ));
        let mut two = one.clone();
        two.candidates
            .push(candidate("wifi0", "10.0.0.8", InterfaceKind::Wifi));
        assert!(matches!(
            propose_private_lan(&two),
            CandidateProposal::ChoiceRequired(_)
        ));
    }
    #[test]
    fn rejects_public_loopback_container_and_tunnel() {
        for c in [
            candidate("eth", "8.8.8.8", InterfaceKind::Ethernet),
            candidate("lo", "127.0.0.1", InterfaceKind::Other),
            candidate("docker", "172.17.0.1", InterfaceKind::ContainerBridge),
            candidate("vpn", "10.2.0.1", InterfaceKind::Tunnel),
        ] {
            assert!(!c.suitable_private_lan());
        }
    }
    #[test]
    fn canonical_origin_is_exact_https_root() {
        let ip = Some("192.168.1.5".parse().unwrap());
        assert_eq!(validate_origin("https://192.168.1.5/", ip, 443), Ok(()));
        for bad in [
            "http://192.168.1.5/",
            "https://user@192.168.1.5/",
            "https://192.168.1.5/path",
            "https://192.168.1.5/?x=1",
            "https://192.168.1.6/",
        ] {
            assert_eq!(validate_origin(bad, ip, 443), Err(PlanError::InvalidOrigin));
        }
    }
}
