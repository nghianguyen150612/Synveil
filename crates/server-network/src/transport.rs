//! Read-only SynveilNet evidence. These values never grant application access.
use crate::{InterfaceKind, NetworkCandidate};
use std::net::IpAddr;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Transport {
    Lan,
    Tailscale,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnavailableReason {
    NotInstalled,
    NotConfigured,
    Disconnected,
    PermissionDenied,
    Unsupported,
    InconsistentEvidence,
    UntrustedEndpoint,
    IdentityConflict,
    AuthenticationRequired,
    AccessRevoked,
    EnrollmentExpired,
    Unreachable,
    RouteChanged,
    TimedOut,
    Cancelled,
    LimitExceeded,
}

/// Installation, configuration and connection are separate from Synveil auth.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TailscaleCapability {
    Unavailable(UnavailableReason),
    Installed,
    ConfiguredDisconnected,
    Connected {
        local_ips: Vec<IpAddr>,
        peers: Vec<TailscalePeer>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TailscalePeer {
    pub node_id: String,
    pub addresses: Vec<IpAddr>,
    pub online: bool,
    pub expired: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteEvidence {
    pub destination: IpAddr,
    pub source: IpAddr,
    pub interface_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalCapabilities {
    pub lan: Result<Vec<NetworkCandidate>, UnavailableReason>,
    pub tailscale: TailscaleCapability,
    pub routes: Vec<RouteEvidence>,
}

/// Future Wizard boundary: no addresses, peer names, credentials or authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilitySummary {
    pub lan_configured: Option<bool>,
    pub tailscale_installed: Option<bool>,
    pub tailscale_configured: Option<bool>,
    pub tailscale_connected: bool,
    pub authenticated: bool,
    pub lan_unavailable: Option<UnavailableReason>,
    pub tailscale_unavailable: Option<UnavailableReason>,
}

impl LocalCapabilities {
    pub fn summary(&self) -> CapabilitySummary {
        CapabilitySummary {
            lan_configured: self.lan.as_ref().ok().map(|v| v.iter().any(lan_candidate)),
            tailscale_installed: match self.tailscale {
                TailscaleCapability::Unavailable(UnavailableReason::NotInstalled) => Some(false),
                TailscaleCapability::Unavailable(_) => None,
                _ => Some(true),
            },
            tailscale_configured: match self.tailscale {
                TailscaleCapability::Unavailable(_) => None,
                TailscaleCapability::Installed => Some(false),
                _ => Some(true),
            },
            tailscale_connected: matches!(self.tailscale, TailscaleCapability::Connected { .. }),
            authenticated: false,
            lan_unavailable: self.lan.as_ref().err().copied(),
            tailscale_unavailable: match self.tailscale {
                TailscaleCapability::Unavailable(e) => Some(e),
                _ => None,
            },
        }
    }

    pub fn path_available(
        &self,
        transport: Transport,
        address: IpAddr,
        peer_id: Option<&str>,
    ) -> Result<(), UnavailableReason> {
        match transport {
            Transport::Lan => {
                self.lan.as_ref().map_err(|e| *e)?;
            }
            Transport::Tailscale => match self.tailscale {
                TailscaleCapability::Unavailable(e) => return Err(e),
                TailscaleCapability::Installed => return Err(UnavailableReason::NotConfigured),
                TailscaleCapability::ConfiguredDisconnected => {
                    return Err(UnavailableReason::Disconnected);
                }
                TailscaleCapability::Connected { .. } => {}
            },
            Transport::Unavailable => return Err(UnavailableReason::Unsupported),
        }
        let routes: Vec<_> = self
            .routes
            .iter()
            .filter(|r| r.destination == address)
            .collect();
        let [route] = routes.as_slice() else {
            return Err(if routes.is_empty() {
                UnavailableReason::Unreachable
            } else {
                UnavailableReason::InconsistentEvidence
            });
        };
        match transport {
            Transport::Lan => {
                if !private_lan_address(address) || peer_id.is_some() {
                    return Err(UnavailableReason::UntrustedEndpoint);
                }
                let candidates = self.lan.as_ref().map_err(|e| *e)?;
                let matching: Vec<_> = candidates
                    .iter()
                    .filter(|c| c.address == route.source && c.interface_id == route.interface_id)
                    .collect();
                let [candidate] = matching.as_slice() else {
                    return Err(UnavailableReason::InconsistentEvidence);
                };
                let mut configured = (**candidate).clone();
                configured.operational = true;
                if !lan_candidate(&configured) || !same_subnet(&configured, address) {
                    return Err(UnavailableReason::InconsistentEvidence);
                }
                if !candidate.operational {
                    return Err(UnavailableReason::Unreachable);
                }
            }
            Transport::Tailscale => {
                let TailscaleCapability::Connected { local_ips, peers } = &self.tailscale else {
                    return Err(match self.tailscale {
                        TailscaleCapability::Unavailable(e) => e,
                        _ => UnavailableReason::Disconnected,
                    });
                };
                if !overlay_address(address)
                    || !local_ips.contains(&route.source)
                    || !route.interface_id.starts_with("tailscale")
                {
                    return Err(UnavailableReason::InconsistentEvidence);
                }
                let matching: Vec<_> = peers
                    .iter()
                    .filter(|p| p.addresses.contains(&address))
                    .collect();
                let [peer] = matching.as_slice() else {
                    return Err(UnavailableReason::IdentityConflict);
                };
                if peer_id != Some(peer.node_id.as_str())
                    || peers.iter().filter(|p| p.node_id == peer.node_id).count() != 1
                {
                    return Err(UnavailableReason::IdentityConflict);
                }
                if peer.expired {
                    return Err(UnavailableReason::EnrollmentExpired);
                }
                if !peer.online {
                    return Err(UnavailableReason::Disconnected);
                }
            }
            Transport::Unavailable => return Err(UnavailableReason::Unsupported),
        }
        Ok(())
    }
}

pub fn overlay_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => u32::from(v) & 0xffc0_0000 == 0x6440_0000,
        IpAddr::V6(v) => u128::from(v) >> 80 == 0xfd7a_115c_a1e0,
    }
}

pub fn private_lan_address(ip: IpAddr) -> bool {
    !overlay_address(ip)
        && match ip {
            IpAddr::V4(v) => {
                v.is_private() && !v.is_loopback() && !v.is_link_local() && !v.is_broadcast()
            }
            IpAddr::V6(v) => v.is_unique_local(),
        }
}

pub fn lan_candidate(c: &NetworkCandidate) -> bool {
    crate::classify_interface(&c.interface_id, "", false, false, false) == InterfaceKind::Other
        && c.operational
        && matches!(c.kind, InterfaceKind::Ethernet | InterfaceKind::Wifi)
        && private_lan_address(c.address)
        && !c.interface_id.is_empty()
        && c.prefix_length > 0
        && c.prefix_length <= if c.address.is_ipv4() { 32 } else { 128 }
}

fn same_subnet(c: &NetworkCandidate, address: IpAddr) -> bool {
    match (c.address, address) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            u32::from(a) >> (32 - c.prefix_length) == u32::from(b) >> (32 - c.prefix_length)
        }
        (IpAddr::V6(a), IpAddr::V6(b)) => {
            u128::from(a) >> (128 - c.prefix_length) == u128::from(b) >> (128 - c.prefix_length)
        }
        _ => false,
    }
}
