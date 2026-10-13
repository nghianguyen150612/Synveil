use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use synveil_server_bootstrap::{OwnerObservation, PrivilegedEffect, ReviewedHostPlan};
use synveil_server_config::{DeploymentProfile, ReachabilityMode};

pub const FRAME_LIMIT: usize = 128 * 1024;
pub const HELPER: &str = "/usr/libexec/synveil-host-helper";
pub const INSPECT_SOCKET: &str = "/run/synveil-host/inspect.sock";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostPurpose {
    #[default]
    Setup,
    Stop,
    RemoveSoftware,
    Upgrade,
    ReconfigureNetwork,
    RepairHost,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostSelection {
    pub profile: DeploymentProfile,
    pub storage_root: String,
    pub mode: ReachabilityMode,
    pub interface_id: Option<String>,
    pub listener_address: String,
    pub canonical_origin: String,
    #[serde(default)]
    pub external_database_target: Option<ExternalDatabaseTarget>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalDatabaseTarget {
    pub host: String,
    pub port: u16,
    pub role: String,
    pub database: String,
    pub transport: String,
}
impl Default for HostSelection {
    fn default() -> Self {
        Self {
            profile: DeploymentProfile::PersonalHomeManaged,
            storage_root: "/var/lib/synveil/storage".to_owned(),
            mode: ReachabilityMode::LocalOnly,
            interface_id: None,
            listener_address: "127.0.0.1".to_owned(),
            canonical_origin: "https://localhost/".to_owned(),
            external_database_target: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedContext {
    pub network_integration_id: String,
    #[serde(default)]
    pub purpose: HostPurpose,
    pub plan: ReviewedHostPlan,
    pub selection: HostSelection,
    pub nonce: String,
    pub binding: String,
    pub summary: Vec<String>,
}

// Secret-bearing requests deliberately have no Debug implementation.
#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Inspect,
    Review {
        selection: HostSelection,
        external_database_url: Option<String>,
    },
    ReviewLifecycle {
        purpose: HostPurpose,
    },
    ReviewNetwork {
        selection: HostSelection,
    },
    Confirm {
        context: Box<ReviewedContext>,
        external_database_url: Option<String>,
    },
    Apply {
        binding: String,
        sequence: usize,
        effect: PrivilegedEffect,
    },
    Account {
        login: String,
        password: String,
        create: bool,
    },
    EnrollDesktop {
        login: String,
        password: String,
    },
    Verify,
    Cancel,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum Response {
    Inspection {
        observation: OwnerObservation,
        generation: u64,
        detail: String,
        candidates: Vec<synveil_server_network::NetworkCandidate>,
        selection: Option<HostSelection>,
    },
    Review {
        context: ReviewedContext,
    },
    Complete,
    Ready {
        descriptor: String,
        library_id: String,
    },
    Enrollment {
        token: String,
    },
    Error {
        code: String,
    },
}
pub fn write_frame<W: Write, T: Serialize>(stream: &mut W, value: &T) -> Result<(), &'static str> {
    let bytes =
        zeroize::Zeroizing::new(serde_json::to_vec(value).map_err(|_| "invalid IPC encoding")?);
    if bytes.len() > FRAME_LIMIT {
        return Err("IPC frame too large");
    }
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .and_then(|()| stream.write_all(&bytes))
        .and_then(|()| stream.flush())
        .map_err(|_| "IPC disconnected")
}
pub fn read_frame<R: Read, T: for<'de> Deserialize<'de>>(
    stream: &mut R,
) -> Result<T, &'static str> {
    let mut header = [0; 4];
    stream
        .read_exact(&mut header)
        .map_err(|_| "IPC disconnected")?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > FRAME_LIMIT {
        return Err("invalid IPC frame bounds");
    }
    let mut bytes = zeroize::Zeroizing::new(vec![0; length]);
    stream
        .read_exact(&mut bytes)
        .map_err(|_| "IPC disconnected")?;
    serde_json::from_slice(&bytes).map_err(|_| "invalid IPC request")
}
