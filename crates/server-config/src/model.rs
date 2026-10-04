use std::{
    fmt,
    net::{IpAddr, SocketAddr},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use synveil_object_store::{
    CapabilityEvidence, CapabilitySupport, StorageAvailability, StorageBackendKind,
    StorageCapabilities, StorageCapability,
};

use crate::CredentialId;

pub const SERVER_CONFIG_SCHEMA_VERSION: u32 = 1;
pub const MAX_SERVER_CONFIG_BYTES: usize = 32 * 1024;
pub const SERVICE_TOPOLOGY_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub schema_version: u32,
    pub server_installation_id: String,
    pub generation: u64,
    pub deployment_profile: DeploymentProfile,
    pub database: DatabaseConfiguration,
    pub rebaseline_token_key_ref: CredentialId,
    pub storage: StorageConfiguration,
    pub network: NetworkConfiguration,
    pub service_topology_version: u32,
    pub configuration_state: ConfigurationState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentProfile {
    PersonalHomeManaged,
    AdvancedExternal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfiguration {
    pub mode: DatabaseMode,
    pub postgres_major: u16,
    pub ownership: DatabaseOwnership,
    pub endpoint: Option<DatabaseEndpoint>,
    pub database_name: Option<String>,
    pub runtime_role: Option<String>,
    pub credential_ref: CredentialId,
    pub managed_password_ref: Option<CredentialId>,
    pub dependency_runtime: Option<DependencyRuntimeIdentity>,
    pub managed_data_root: Option<ManagedDatabaseDataRoot>,
    pub credential_state: DatabaseCredentialState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseMode {
    ManagedPrivate,
    External,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseOwnership {
    SynveilManaged,
    OperatorOwned,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseEndpoint {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyRuntimeIdentity {
    SynveilPrivatePostgresql17,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedDatabaseDataRoot {
    VarLibSynveilPostgresql17,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseCredentialState {
    PendingManagedEndpoint,
    Materialized,
}

/// Stable identity for one managed server-object-storage installation.
/// This is independent from the server installation ID, object IDs, and any
/// filesystem or mount identity.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StorageId(String);

impl StorageId {
    #[must_use]
    pub fn new_v7() -> Self {
        Self(uuid::Uuid::now_v7().to_string())
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, ConfigValidationError> {
        let value = value.into();
        let parsed = uuid::Uuid::parse_str(&value)
            .map_err(|_| ConfigValidationError::InvalidStorageIdentity)?;
        if parsed.to_string() != value || parsed.get_version_num() != 7 {
            return Err(ConfigValidationError::InvalidStorageIdentity);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum StorageConfiguration {
    NotConfigured,
    PreparingLocal {
        root: String,
        storage_id: StorageId,
        root_identity: StorageRootIdentity,
    },
    ConfiguredLocal {
        root: String,
        storage_id: StorageId,
        root_identity: StorageRootIdentity,
        capabilities: StorageCapabilities,
    },
}

/// Filesystem object evidence bound into durable storage setup. A missing leaf
/// records the selected existing parent; a directory records its native
/// filesystem and inode identity. Values are diagnostic identity evidence,
/// never secrets or portable path strings.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StorageRootIdentity {
    MissingLeaf {
        parent_device: u64,
        parent_inode: u64,
    },
    Directory {
        device: u64,
        inode: u64,
    },
}

impl StorageRootIdentity {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        match self {
            Self::MissingLeaf {
                parent_device,
                parent_inode,
            } => *parent_device != 0 || *parent_inode != 0,
            Self::Directory { device, inode } => *device != 0 || *inode != 0,
        }
    }

    #[must_use]
    pub const fn is_directory(&self) -> bool {
        matches!(self, Self::Directory { .. })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkConfiguration {
    NotConfigured,
    Preparing(NetworkIntegration),
    Configured(NetworkIntegration),
}

/// Durable, non-secret identity and evidence for the client-facing HTTPS edge.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkIntegration {
    pub network_integration_id: NetworkIntegrationId,
    pub mode: ReachabilityMode,
    /// Always the private Axum endpoint; this is never the edge listener.
    pub backend_endpoint: String,
    pub listener: EdgeListener,
    pub canonical_origin: String,
    pub trust: NetworkTrust,
    pub firewall: FirewallState,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NetworkIntegrationId(String);

impl NetworkIntegrationId {
    #[must_use]
    pub fn new_v7() -> Self {
        Self(uuid::Uuid::now_v7().to_string())
    }
    pub fn parse(value: impl Into<String>) -> Result<Self, ConfigValidationError> {
        let value = value.into();
        let id = uuid::Uuid::parse_str(&value)
            .map_err(|_| ConfigValidationError::InvalidNetworkConfiguration)?;
        if id.to_string() != value || id.get_version_num() != 7 {
            return Err(ConfigValidationError::InvalidNetworkConfiguration);
        }
        Ok(Self(value))
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReachabilityMode {
    LocalOnly,
    PrivateLan,
    AdvancedExternalHttps,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeListener {
    pub address: IpAddr,
    pub port: u16,
    pub interface_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkTrust {
    ManagedPrivateCa { ca_certificate_sha256: String },
    PublicWebPki,
    OperatorManagedHttps,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum FirewallState {
    NotRequired,
    Planned {
        manager: FirewallManager,
        rule_id: String,
    },
    Applied {
        manager: FirewallManager,
        rule_id: String,
    },
    OperatorOwned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FirewallManager {
    Ufw,
    Firewalld,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigurationState {
    Prepared,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct ConfigFingerprint([u8; 32]);

impl ConfigFingerprint {
    #[must_use]
    pub fn from_canonical_bytes(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    #[must_use]
    pub fn to_hex(self) -> String {
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            use fmt::Write as _;
            let _ = write!(output, "{byte:02x}");
        }
        output
    }
}

impl fmt::Debug for ConfigFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ConfigFingerprint")
            .field(&self.to_hex())
            .finish()
    }
}

impl fmt::Display for ConfigFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigValidationError {
    UnsupportedSchema,
    InvalidInstallationId,
    InvalidGeneration,
    UnsupportedDeploymentMode,
    UnsupportedPostgresqlMajor,
    InconsistentDatabaseOwnership,
    InvalidDatabaseIdentity,
    InvalidDatabaseEndpoint,
    InvalidCredentialReference,
    InvalidStoragePath,
    InvalidStorageIdentity,
    InvalidStorageRootIdentity,
    InvalidStorageCapabilities,
    InvalidNetworkConfiguration,
    UnsupportedServiceTopology,
    InvalidConfigurationState,
    InvalidFormat,
    Empty,
    Oversized { limit: usize },
}

impl fmt::Display for ConfigValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedSchema => "configuration requires a newer Synveil version",
            Self::InvalidInstallationId => "server installation identity is invalid",
            Self::InvalidGeneration => "configuration generation is invalid",
            Self::UnsupportedDeploymentMode => "deployment mode is unsupported",
            Self::UnsupportedPostgresqlMajor => "PostgreSQL major version 17 is required",
            Self::InconsistentDatabaseOwnership => {
                "database ownership does not match deployment mode"
            }
            Self::InvalidDatabaseIdentity => "database logical identity is invalid",
            Self::InvalidDatabaseEndpoint => "database endpoint is invalid",
            Self::InvalidCredentialReference => "database credential reference is invalid",
            Self::InvalidStoragePath => "server storage path is structurally unsafe",
            Self::InvalidStorageIdentity => "server storage identity is invalid",
            Self::InvalidStorageRootIdentity => "server storage root identity is invalid",
            Self::InvalidStorageCapabilities => "server storage capability evidence is invalid",
            Self::InvalidNetworkConfiguration => "network configuration is invalid",
            Self::UnsupportedServiceTopology => "server service topology is unsupported",
            Self::InvalidConfigurationState => "configuration state is invalid",
            Self::InvalidFormat => "server configuration format is invalid",
            Self::Empty => "server configuration is empty",
            Self::Oversized { .. } => "server configuration exceeds its size limit",
        })
    }
}

impl std::error::Error for ConfigValidationError {}

impl ServerConfig {
    pub(crate) fn canonical_bytes(&self) -> Result<Vec<u8>, ConfigValidationError> {
        self.validate()?;
        let mut bytes =
            serde_json::to_vec_pretty(self).map_err(|_| ConfigValidationError::InvalidFormat)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_SERVER_CONFIG_BYTES {
            return Err(ConfigValidationError::Oversized {
                limit: MAX_SERVER_CONFIG_BYTES,
            });
        }
        Ok(bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, ConfigValidationError> {
        if bytes.is_empty() {
            return Err(ConfigValidationError::Empty);
        }
        if bytes.len() > MAX_SERVER_CONFIG_BYTES {
            return Err(ConfigValidationError::Oversized {
                limit: MAX_SERVER_CONFIG_BYTES,
            });
        }
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| ConfigValidationError::InvalidFormat)?;
        let version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or(ConfigValidationError::InvalidFormat)?;
        if version != SERVER_CONFIG_SCHEMA_VERSION {
            return Err(ConfigValidationError::UnsupportedSchema);
        }
        let config: Self =
            serde_json::from_value(value).map_err(|_| ConfigValidationError::InvalidFormat)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        if self.schema_version != SERVER_CONFIG_SCHEMA_VERSION {
            return Err(ConfigValidationError::UnsupportedSchema);
        }
        let parsed_id = uuid::Uuid::parse_str(&self.server_installation_id)
            .map_err(|_| ConfigValidationError::InvalidInstallationId)?;
        if parsed_id.to_string() != self.server_installation_id || parsed_id.get_version_num() != 7
        {
            return Err(ConfigValidationError::InvalidInstallationId);
        }
        if self.generation > i64::MAX as u64 {
            return Err(ConfigValidationError::InvalidGeneration);
        }
        if self.database.postgres_major != 17 {
            return Err(ConfigValidationError::UnsupportedPostgresqlMajor);
        }
        let (expected_mode, expected_ownership) = match self.deployment_profile {
            DeploymentProfile::PersonalHomeManaged => (
                DatabaseMode::ManagedPrivate,
                DatabaseOwnership::SynveilManaged,
            ),
            DeploymentProfile::AdvancedExternal => {
                (DatabaseMode::External, DatabaseOwnership::OperatorOwned)
            }
        };
        if self.database.mode != expected_mode || self.database.ownership != expected_ownership {
            return Err(ConfigValidationError::InconsistentDatabaseOwnership);
        }
        if self.database.credential_ref != CredentialId::DatabaseUrl
            || self.rebaseline_token_key_ref != CredentialId::RebaselineTokenKey
        {
            return Err(ConfigValidationError::InvalidCredentialReference);
        }
        match self.database.mode {
            DatabaseMode::ManagedPrivate => {
                if self.database.managed_password_ref != Some(CredentialId::DatabasePassword)
                    || self.database.dependency_runtime
                        != Some(DependencyRuntimeIdentity::SynveilPrivatePostgresql17)
                    || self.database.managed_data_root
                        != Some(ManagedDatabaseDataRoot::VarLibSynveilPostgresql17)
                {
                    return Err(ConfigValidationError::InconsistentDatabaseOwnership);
                }
                if self.database.credential_state == DatabaseCredentialState::Materialized
                    && self.database.endpoint.is_none()
                {
                    return Err(ConfigValidationError::InvalidDatabaseEndpoint);
                }
            }
            DatabaseMode::External => {
                if self.database.managed_password_ref.is_some()
                    || self.database.dependency_runtime.is_some()
                    || self.database.managed_data_root.is_some()
                {
                    return Err(ConfigValidationError::InconsistentDatabaseOwnership);
                }
                if self.database.credential_state != DatabaseCredentialState::Materialized {
                    return Err(ConfigValidationError::InvalidCredentialReference);
                }
            }
        }
        for value in [
            self.database.database_name.as_deref(),
            self.database.runtime_role.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if !valid_database_identifier(value) {
                return Err(ConfigValidationError::InvalidDatabaseIdentity);
            }
        }
        if self
            .database
            .endpoint
            .as_ref()
            .is_some_and(|endpoint| !valid_endpoint_host(&endpoint.host) || endpoint.port == 0)
        {
            return Err(ConfigValidationError::InvalidDatabaseEndpoint);
        }
        match &self.storage {
            StorageConfiguration::NotConfigured => {}
            StorageConfiguration::PreparingLocal {
                root,
                storage_id,
                root_identity,
            } => validate_storage_identity(root, storage_id, root_identity, false)?,
            StorageConfiguration::ConfiguredLocal {
                root,
                storage_id,
                root_identity,
                capabilities,
            } => {
                validate_storage_identity(root, storage_id, root_identity, true)?;
                if !valid_local_storage_capabilities(capabilities) {
                    return Err(ConfigValidationError::InvalidStorageCapabilities);
                }
            }
        }
        if let NetworkConfiguration::Preparing(network)
        | NetworkConfiguration::Configured(network) = &self.network
        {
            validate_network(network)?;
        }
        if self.service_topology_version != super::model::SERVICE_TOPOLOGY_VERSION {
            return Err(ConfigValidationError::UnsupportedServiceTopology);
        }
        if self.configuration_state != ConfigurationState::Prepared {
            return Err(ConfigValidationError::InvalidConfigurationState);
        }
        Ok(())
    }

    pub(crate) fn initial(profile: DeploymentProfile, installation_id: String) -> Self {
        let database = match profile {
            DeploymentProfile::PersonalHomeManaged => DatabaseConfiguration {
                mode: DatabaseMode::ManagedPrivate,
                postgres_major: 17,
                ownership: DatabaseOwnership::SynveilManaged,
                endpoint: None,
                database_name: Some("synveil".to_owned()),
                runtime_role: Some("synveil".to_owned()),
                credential_ref: CredentialId::DatabaseUrl,
                managed_password_ref: Some(CredentialId::DatabasePassword),
                dependency_runtime: Some(DependencyRuntimeIdentity::SynveilPrivatePostgresql17),
                managed_data_root: Some(ManagedDatabaseDataRoot::VarLibSynveilPostgresql17),
                credential_state: DatabaseCredentialState::PendingManagedEndpoint,
            },
            DeploymentProfile::AdvancedExternal => DatabaseConfiguration {
                mode: DatabaseMode::External,
                postgres_major: 17,
                ownership: DatabaseOwnership::OperatorOwned,
                endpoint: None,
                database_name: None,
                runtime_role: None,
                credential_ref: CredentialId::DatabaseUrl,
                managed_password_ref: None,
                dependency_runtime: None,
                managed_data_root: None,
                credential_state: DatabaseCredentialState::Materialized,
            },
        };
        Self {
            schema_version: SERVER_CONFIG_SCHEMA_VERSION,
            server_installation_id: installation_id,
            generation: 0,
            deployment_profile: profile,
            database,
            rebaseline_token_key_ref: CredentialId::RebaselineTokenKey,
            storage: StorageConfiguration::NotConfigured,
            network: NetworkConfiguration::NotConfigured,
            service_topology_version: SERVICE_TOPOLOGY_VERSION,
            configuration_state: ConfigurationState::Prepared,
        }
    }
}

fn validate_network(network: &NetworkIntegration) -> Result<(), ConfigValidationError> {
    NetworkIntegrationId::parse(network.network_integration_id.as_str().to_owned())?;
    let backend: SocketAddr = network
        .backend_endpoint
        .parse()
        .map_err(|_| ConfigValidationError::InvalidNetworkConfiguration)?;
    if backend != SocketAddr::from(([127, 0, 0, 1], 3000)) || network.listener.port == 0 {
        return Err(ConfigValidationError::InvalidNetworkConfiguration);
    }
    let origin = url::Url::parse(&network.canonical_origin)
        .map_err(|_| ConfigValidationError::InvalidNetworkConfiguration)?;
    if origin.scheme() != "https"
        || origin.username() != ""
        || origin.password().is_some()
        || origin.query().is_some()
        || origin.fragment().is_some()
        || origin.path() != "/"
        || origin.host_str().is_none()
        || origin.port() == Some(0)
    {
        return Err(ConfigValidationError::InvalidNetworkConfiguration);
    }
    let private_v4 =
        |ip: std::net::Ipv4Addr| ip.is_private() && !ip.is_loopback() && !ip.is_link_local();
    match network.mode {
        ReachabilityMode::LocalOnly => {
            if !network.listener.address.is_loopback()
                || network.listener.interface_id.is_some()
                || !matches!(network.firewall, FirewallState::NotRequired)
                || !matches!(network.trust, NetworkTrust::ManagedPrivateCa { .. })
            {
                return Err(ConfigValidationError::InvalidNetworkConfiguration);
            }
        }
        ReachabilityMode::PrivateLan => {
            if !matches!(network.listener.address, IpAddr::V4(ip) if private_v4(ip))
                || network
                    .listener
                    .interface_id
                    .as_deref()
                    .is_none_or(|v| v.is_empty() || v.len() > 128)
                || !matches!(network.trust, NetworkTrust::ManagedPrivateCa { .. })
            {
                return Err(ConfigValidationError::InvalidNetworkConfiguration);
            }
        }
        ReachabilityMode::AdvancedExternalHttps => {
            if !matches!(network.firewall, FirewallState::OperatorOwned)
                || matches!(network.trust, NetworkTrust::ManagedPrivateCa { .. })
            {
                return Err(ConfigValidationError::InvalidNetworkConfiguration);
            }
        }
    }
    if let NetworkTrust::ManagedPrivateCa {
        ca_certificate_sha256,
    } = &network.trust
    {
        if ca_certificate_sha256.len() != 64
            || !ca_certificate_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(ConfigValidationError::InvalidNetworkConfiguration);
        }
    }
    Ok(())
}

fn validate_storage_identity(
    root: &str,
    storage_id: &StorageId,
    root_identity: &StorageRootIdentity,
    require_directory: bool,
) -> Result<(), ConfigValidationError> {
    if !structurally_safe_absolute_path(root) {
        return Err(ConfigValidationError::InvalidStoragePath);
    }
    StorageId::parse(storage_id.as_str().to_owned())?;
    if !root_identity.is_valid() || (require_directory && !root_identity.is_directory()) {
        return Err(ConfigValidationError::InvalidStorageRootIdentity);
    }
    Ok(())
}

fn valid_local_storage_capabilities(capabilities: &StorageCapabilities) -> bool {
    capabilities.backend() == StorageBackendKind::LocalFilesystem
        && capabilities.availability() == StorageAvailability::Available
        && capabilities.evidence() == (CapabilityEvidence::AdapterProbe { version: 1 })
        && [
            StorageCapability::ExclusiveCreate,
            StorageCapability::DurableFsync,
            StorageCapability::AtomicRename,
            StorageCapability::AtomicPromotion,
            StorageCapability::Checksumming,
            StorageCapability::ReadAfterWrite,
            StorageCapability::DurableFlush,
        ]
        .into_iter()
        .all(|capability| capabilities.support(capability) == CapabilitySupport::Supported)
}

fn valid_database_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn valid_endpoint_host(value: &str) -> bool {
    if value.is_empty() || value.len() > 255 || value.as_bytes().contains(&0) {
        return false;
    }
    let value = value
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(value);
    if value.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    !value.is_empty()
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.as_bytes()[0].is_ascii_alphanumeric()
                && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

pub(crate) fn structurally_safe_absolute_path(value: &str) -> bool {
    if value.is_empty() || value.len() > 4096 || value.as_bytes().contains(&0) {
        return false;
    }
    let linux_absolute = value.starts_with('/') && value != "/";
    let windows_absolute = value.as_bytes().get(1) == Some(&b':')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
        && value
            .as_bytes()
            .get(2)
            .is_some_and(|b| *b == b'\\' || *b == b'/')
        && !matches!(value.len(), 3);
    if !linux_absolute && !windows_absolute {
        return false;
    }
    let normalized = value.replace('\\', "/").to_ascii_lowercase();
    if normalized
        .split('/')
        .any(|component| component == ".." || component == ".")
    {
        return false;
    }
    if [
        "/etc/synveil",
        "/var/lib/synveil/postgresql",
        "/etc/synveil/credentials",
    ]
    .iter()
    .any(|reserved| normalized == *reserved || normalized.starts_with(&format!("{reserved}/")))
    {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{
        CapabilityEvidence, CapabilitySupport, ConfigValidationError, DatabaseCredentialState,
        DeploymentProfile, EdgeListener, FirewallState, NetworkConfiguration, NetworkIntegration,
        NetworkIntegrationId, NetworkTrust, ReachabilityMode, ServerConfig, StorageAvailability,
        StorageBackendKind, StorageCapabilities, StorageCapability, StorageConfiguration,
        StorageId, StorageRootIdentity,
    };

    fn managed() -> ServerConfig {
        ServerConfig::initial(
            DeploymentProfile::PersonalHomeManaged,
            "018f2ed0-44c2-7c00-8000-000000000001".to_owned(),
        )
    }

    #[test]
    fn v1_round_trip_and_serialization_are_deterministic() {
        let config = managed();
        let first = config.canonical_bytes().unwrap();
        let second = config.canonical_bytes().unwrap();
        assert_eq!(first, second);
        assert_eq!(ServerConfig::parse(&first).unwrap(), config);
    }

    #[test]
    fn configured_storage_persists_strict_adapter_capability_evidence() {
        let mut config = managed();
        config.storage = StorageConfiguration::ConfiguredLocal {
            root: "/var/lib/synveil/storage".to_owned(),
            storage_id: StorageId::new_v7(),
            root_identity: StorageRootIdentity::Directory {
                device: 10,
                inode: 20,
            },
            capabilities: ready_storage_capabilities(),
        };
        let bytes = config.canonical_bytes().unwrap();
        assert_eq!(ServerConfig::parse(&bytes).unwrap(), config);
        assert_eq!(config.canonical_bytes().unwrap(), bytes);

        let mut incomplete = config.clone();
        if let StorageConfiguration::ConfiguredLocal { capabilities, .. } = &mut incomplete.storage
        {
            *capabilities = StorageCapabilities::for_location(
                StorageBackendKind::LocalFilesystem,
                StorageAvailability::Available,
                CapabilityEvidence::AdapterProbe { version: 1 },
            );
        }
        assert_eq!(
            incomplete.validate(),
            Err(ConfigValidationError::InvalidStorageCapabilities)
        );

        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["storage"]["capabilities"]["capabilities"]["future_capability"] =
            serde_json::json!("supported");
        assert_eq!(
            ServerConfig::parse(&serde_json::to_vec(&value).unwrap()),
            Err(ConfigValidationError::InvalidFormat)
        );
    }

    fn ready_storage_capabilities() -> StorageCapabilities {
        [
            StorageCapability::ExclusiveCreate,
            StorageCapability::DurableFsync,
            StorageCapability::AtomicRename,
            StorageCapability::AtomicPromotion,
            StorageCapability::Checksumming,
            StorageCapability::ReadAfterWrite,
            StorageCapability::DurableFlush,
        ]
        .into_iter()
        .fold(
            StorageCapabilities::for_location(
                StorageBackendKind::LocalFilesystem,
                StorageAvailability::Available,
                CapabilityEvidence::AdapterProbe { version: 1 },
            ),
            |capabilities, capability| {
                capabilities.with_support(capability, CapabilitySupport::Supported)
            },
        )
    }

    #[test]
    fn unknown_fields_and_future_schemas_fail_closed() {
        let bytes = managed().canonical_bytes().unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["unexpected"] = serde_json::json!("value");
        assert_eq!(
            ServerConfig::parse(&serde_json::to_vec(&value).unwrap()),
            Err(ConfigValidationError::InvalidFormat)
        );
        value.as_object_mut().unwrap().remove("unexpected");
        value["schema_version"] = serde_json::json!(2);
        assert_eq!(
            ServerConfig::parse(&serde_json::to_vec(&value).unwrap()),
            Err(ConfigValidationError::UnsupportedSchema)
        );
    }

    #[test]
    fn limits_unknown_modes_major_identity_paths_and_network() {
        let mut config = managed();
        config.database.postgres_major = 16;
        assert_eq!(
            config.validate(),
            Err(ConfigValidationError::UnsupportedPostgresqlMajor)
        );
        config = managed();
        config.server_installation_id = "not-a-uuid".to_owned();
        assert_eq!(
            config.validate(),
            Err(ConfigValidationError::InvalidInstallationId)
        );
        config = managed();
        config.server_installation_id = "00000000-0000-0000-0000-000000000000".to_owned();
        assert_eq!(
            config.validate(),
            Err(ConfigValidationError::InvalidInstallationId)
        );
        config = managed();
        config.server_installation_id = "018f2ed0-44c2-4c00-8000-000000000001".to_owned();
        assert_eq!(
            config.validate(),
            Err(ConfigValidationError::InvalidInstallationId)
        );
        config = managed();
        config.storage = StorageConfiguration::ConfiguredLocal {
            root: "/".to_owned(),
            storage_id: StorageId::new_v7(),
            root_identity: StorageRootIdentity::Directory {
                device: 1,
                inode: 1,
            },
            capabilities: StorageCapabilities::unknown(),
        };
        assert_eq!(
            config.validate(),
            Err(ConfigValidationError::InvalidStoragePath)
        );
        config = managed();
        config.network = NetworkConfiguration::Configured(NetworkIntegration {
            network_integration_id: NetworkIntegrationId::new_v7(),
            mode: ReachabilityMode::PrivateLan,
            backend_endpoint: "127.0.0.1:3000".to_owned(),
            listener: EdgeListener {
                address: "0.0.0.0".parse().unwrap(),
                port: 443,
                interface_id: Some("ethernet-1".to_owned()),
            },
            canonical_origin: "https://192.168.1.20/".to_owned(),
            trust: NetworkTrust::ManagedPrivateCa {
                ca_certificate_sha256: "a".repeat(64),
            },
            firewall: FirewallState::NotRequired,
        });
        assert_eq!(
            config.validate(),
            Err(ConfigValidationError::InvalidNetworkConfiguration)
        );
    }

    #[test]
    fn pending_storage_and_network_are_distinct_and_managed_external_modes_validate() {
        let managed_config = managed();
        assert_eq!(
            managed_config.database.credential_state,
            DatabaseCredentialState::PendingManagedEndpoint
        );
        assert_eq!(managed_config.storage, StorageConfiguration::NotConfigured);
        assert_eq!(managed_config.network, NetworkConfiguration::NotConfigured);
        let external = ServerConfig::initial(
            DeploymentProfile::AdvancedExternal,
            "018f2ed0-44c2-7c00-8000-000000000002".to_owned(),
        );
        assert!(external.validate().is_ok());
        assert_ne!(
            external.deployment_profile,
            managed_config.deployment_profile
        );
    }

    #[test]
    fn storage_identity_accepts_only_canonical_uuid_v7_values() {
        let valid = StorageId::new_v7();
        assert_eq!(StorageId::parse(valid.as_str()), Ok(valid));
        assert_eq!(
            StorageId::parse("018f2ed0-44c2-4c00-8000-000000000001"),
            Err(ConfigValidationError::InvalidStorageIdentity)
        );
        assert_eq!(
            StorageId::parse("not-a-uuid"),
            Err(ConfigValidationError::InvalidStorageIdentity)
        );
    }

    #[test]
    fn forbidden_secret_fields_are_rejected_and_debug_has_no_secret() {
        let config = managed();
        let debug = format!("{config:?}");
        assert!(!debug.contains("CANARY"));
        let mut value: serde_json::Value =
            serde_json::from_slice(&config.canonical_bytes().unwrap()).unwrap();
        value["database"]["password"] = serde_json::json!("canary");
        assert!(ServerConfig::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    }

    #[test]
    fn empty_and_oversized_documents_are_rejected() {
        assert_eq!(ServerConfig::parse(&[]), Err(ConfigValidationError::Empty));
        assert!(matches!(
            ServerConfig::parse(&vec![b' '; super::MAX_SERVER_CONFIG_BYTES + 1]),
            Err(ConfigValidationError::Oversized { .. })
        ));
    }
}
