#![forbid(unsafe_code)]
//! Production composition only; resource ownership remains with P031--P035.
// The bounded pkexec/Unix IPC client is a Linux implementation detail. Keep a
// typed unsupported adapter available to the desktop crate on other targets so
// its Connect code remains buildable without pretending that Host is supported.
#[cfg(target_os = "linux")]
pub mod client;
#[cfg(not(target_os = "linux"))]
#[path = "unsupported_client.rs"]
pub mod client;
#[cfg(target_os = "linux")]
pub mod native;
pub mod protocol;

pub fn supported_platform() -> Option<String> {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return None;
    }
    let bytes = std::fs::read_to_string("/etc/os-release").ok()?;
    let value = |key: &str| {
        bytes.lines().find_map(|line| {
            line.strip_prefix(&format!("{key}="))
                .map(|s| s.trim_matches('"'))
        })
    };
    match (value("ID"), value("VERSION_ID")) {
        (Some("ubuntu"), Some("24.04")) => Some("ubuntu-24.04".to_owned()),
        (Some("fedora"), Some("42")) => Some("fedora-42".to_owned()),
        _ => None,
    }
}

/// A redacted result from the read-only Private LAN capability probe.
///
/// This deliberately exposes only a bounded candidate count. Interface names,
/// addresses, routes, and command output remain inside the Host owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivateLanCapability {
    Available { candidate_count: u8 },
    NoCandidate,
    InspectionUnavailable,
    Unsupported,
}

/// Inspect the host's existing network interfaces without changing listeners
/// or network configuration. No candidate identity crosses this boundary.
#[must_use]
pub fn inspect_private_lan_capability() -> PrivateLanCapability {
    #[cfg(target_os = "linux")]
    {
        return match native::inspect_private_lan_candidate_count() {
            Ok(0) => PrivateLanCapability::NoCandidate,
            Ok(candidate_count) => PrivateLanCapability::Available { candidate_count },
            Err(()) => PrivateLanCapability::InspectionUnavailable,
        };
    }

    #[cfg(not(target_os = "linux"))]
    PrivateLanCapability::Unsupported
}
