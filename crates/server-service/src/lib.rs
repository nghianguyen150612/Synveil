#![forbid(unsafe_code)]

//! UI-neutral planning and inspection for the privileged Linux server-service
//! coordinator.  This crate deliberately exposes no arbitrary command runner.

use std::{fmt, path::Path};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use synveil_server_config::{DeploymentProfile, ServerConfig, StorageConfiguration};

pub const POSTGRES_MAJOR: u16 = 17;
pub const CLUSTER_MARKER_NAME: &str = ".synveil-managed-postgresql.json";
pub const MANAGED_DATA_DIRECTORY: &str = "/var/lib/synveil/postgresql/17/data";
pub const MANAGED_PORT_FIRST: u16 = 55432;
pub const MANAGED_PORT_LAST: u16 = 55463;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeArtifactIdentity {
    pub artifact_id: String,
    pub version: String,
    pub source_revision: String,
    pub platform: String,
    pub architecture: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedClusterIdentity {
    pub schema_version: u32,
    pub kind: String,
    pub server_installation_id: String,
    pub managed_cluster_id: String,
    pub postgres_major: u16,
    pub runtime_artifact_id: String,
    pub data_directory: String,
    pub postgres_system_identifier: Option<String>,
}

impl ManagedClusterIdentity {
    pub fn new(server_installation_id: String, runtime_artifact_id: String) -> Self {
        Self {
            schema_version: 1,
            kind: "synveil_managed_postgresql".into(),
            server_installation_id,
            managed_cluster_id: uuid::Uuid::now_v7().to_string(),
            postgres_major: POSTGRES_MAJOR,
            runtime_artifact_id,
            data_directory: MANAGED_DATA_DIRECTORY.into(),
            postgres_system_identifier: None,
        }
    }

    pub fn validate(&self) -> Result<(), ServiceError> {
        if self.schema_version != 1 || self.kind != "synveil_managed_postgresql" {
            return Err(ServiceError::IdentityConflict);
        }
        for value in [&self.server_installation_id, &self.managed_cluster_id] {
            let id = uuid::Uuid::parse_str(value).map_err(|_| ServiceError::IdentityConflict)?;
            if id.get_version_num() != 7 || id.to_string() != *value {
                return Err(ServiceError::IdentityConflict);
            }
        }
        if self.postgres_major != POSTGRES_MAJOR {
            return Err(ServiceError::UnsupportedVersion);
        }
        if self.runtime_artifact_id.is_empty() || self.data_directory != MANAGED_DATA_DIRECTORY {
            return Err(ServiceError::IdentityConflict);
        }
        if self
            .postgres_system_identifier
            .as_ref()
            .is_some_and(|id| id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err(ServiceError::IdentityConflict);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClusterState {
    Absent,
    PreparedEmpty,
    Initialized,
    NeedsRepair,
    UnsupportedVersion,
    Foreign,
}

/// Classifies a cluster without deleting, rewriting, or running `initdb`.
pub fn classify_cluster(
    marker: Option<&ManagedClusterIdentity>,
    data_is_empty: bool,
    pg_version: Option<&str>,
    observed_system_identifier: Option<&str>,
) -> ClusterState {
    let Some(marker) = marker else {
        return if data_is_empty {
            ClusterState::Absent
        } else {
            ClusterState::Foreign
        };
    };
    if marker.validate().is_err() {
        return if marker.postgres_major != POSTGRES_MAJOR {
            ClusterState::UnsupportedVersion
        } else {
            ClusterState::NeedsRepair
        };
    }
    if data_is_empty && marker.postgres_system_identifier.is_none() {
        return ClusterState::PreparedEmpty;
    }
    if pg_version != Some("17") {
        return pg_version.map_or(ClusterState::NeedsRepair, |_| {
            ClusterState::UnsupportedVersion
        });
    }
    match (
        &marker.postgres_system_identifier,
        observed_system_identifier,
    ) {
        (Some(expected), Some(actual)) if expected == actual => ClusterState::Initialized,
        _ => ClusterState::NeedsRepair,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerInstallPlan {
    pub server_installation_id: String,
    pub config_generation: u64,
    pub config_fingerprint: String,
    pub storage_id: String,
    pub storage_root: String,
    pub profile: DeploymentProfile,
    pub server_runtime: RuntimeArtifactIdentity,
    pub postgresql_runtime: Option<RuntimeArtifactIdentity>,
}

pub fn prepare_plan(
    config: &ServerConfig,
    canonical_config: &[u8],
    server_runtime: RuntimeArtifactIdentity,
    postgresql_runtime: Option<RuntimeArtifactIdentity>,
) -> Result<ServerInstallPlan, ServiceError> {
    config
        .validate()
        .map_err(|_| ServiceError::InvalidConfiguration)?;
    let StorageConfiguration::ConfiguredLocal {
        root, storage_id, ..
    } = &config.storage
    else {
        return Err(ServiceError::StorageNotConfigured);
    };
    if config.deployment_profile == DeploymentProfile::PersonalHomeManaged
        && postgresql_runtime.is_none()
    {
        return Err(ServiceError::RuntimeMissing);
    }
    if config.deployment_profile == DeploymentProfile::AdvancedExternal
        && postgresql_runtime.is_some()
    {
        return Err(ServiceError::ExternalDatabaseOwnedByOperator);
    }
    validate_runtime(&server_runtime)?;
    if let Some(runtime) = &postgresql_runtime {
        validate_runtime(runtime)?;
        if !runtime.version.starts_with("17.") {
            return Err(ServiceError::UnsupportedVersion);
        }
    }
    Ok(ServerInstallPlan {
        server_installation_id: config.server_installation_id.clone(),
        config_generation: config.generation,
        config_fingerprint: hex(Sha256::digest(canonical_config).as_slice()),
        storage_id: storage_id.as_str().to_owned(),
        storage_root: root.clone(),
        profile: config.deployment_profile,
        server_runtime,
        postgresql_runtime,
    })
}

pub fn verify_plan_is_current(
    plan: &ServerInstallPlan,
    config: &ServerConfig,
    bytes: &[u8],
) -> Result<(), ServiceError> {
    let fingerprint = hex(Sha256::digest(bytes).as_slice());
    if plan.server_installation_id != config.server_installation_id
        || plan.config_generation != config.generation
        || plan.config_fingerprint != fingerprint
    {
        return Err(ServiceError::StalePlan);
    }
    Ok(())
}

/// Selects the first inspected free loopback port. A persisted port is never
/// silently replaced; callers must report its conflict as repair-required.
pub fn select_managed_port(persisted: Option<u16>, occupied: &[u16]) -> Result<u16, ServiceError> {
    if let Some(port) = persisted {
        if !(MANAGED_PORT_FIRST..=MANAGED_PORT_LAST).contains(&port) || occupied.contains(&port) {
            return Err(ServiceError::EndpointConflict);
        }
        return Ok(port);
    }
    (MANAGED_PORT_FIRST..=MANAGED_PORT_LAST)
        .find(|candidate| !occupied.contains(candidate))
        .ok_or(ServiceError::NoPrivatePortAvailable)
}

/// Escapes one verified absolute path for a systemd double-quoted value.
pub fn systemd_path_value(path: &Path) -> Result<String, ServiceError> {
    let value = path.to_str().ok_or(ServiceError::InvalidStoragePath)?;
    if !path.is_absolute() || value.contains('\0') || value.contains('\n') || value.contains('\r') {
        return Err(ServiceError::InvalidStoragePath);
    }
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

fn validate_runtime(runtime: &RuntimeArtifactIdentity) -> Result<(), ServiceError> {
    let hash_ok = runtime.sha256.len() == 64
        && runtime
            .sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
    if runtime.artifact_id.is_empty()
        || runtime.version.is_empty()
        || runtime.source_revision.len() != 40
        || runtime.platform != "linux"
        || runtime.architecture != "x86_64"
        || !hash_ok
    {
        return Err(ServiceError::UnverifiedRuntime);
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceError {
    InvalidConfiguration,
    StorageNotConfigured,
    InvalidStoragePath,
    RuntimeMissing,
    UnverifiedRuntime,
    UnsupportedVersion,
    IdentityConflict,
    ExternalDatabaseOwnedByOperator,
    EndpointConflict,
    NoPrivatePortAvailable,
    StalePlan,
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ServiceError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_and_wrong_major_clusters_are_never_adopted() {
        assert_eq!(
            classify_cluster(None, false, Some("17"), Some("1")),
            ClusterState::Foreign
        );
        let mut marker = ManagedClusterIdentity::new(
            "018f2ed0-44c2-7c00-8000-000000000001".into(),
            "pg17-r1".into(),
        );
        marker.postgres_major = 16;
        assert_eq!(
            classify_cluster(Some(&marker), false, Some("16"), Some("1")),
            ClusterState::UnsupportedVersion
        );
    }

    #[test]
    fn system_identifier_is_required_and_must_match() {
        let mut marker = ManagedClusterIdentity::new(
            "018f2ed0-44c2-7c00-8000-000000000001".into(),
            "pg17-r1".into(),
        );
        marker.postgres_system_identifier = Some("7250123456789012345".into());
        assert_eq!(
            classify_cluster(
                Some(&marker),
                false,
                Some("17"),
                Some("7250123456789012345")
            ),
            ClusterState::Initialized
        );
        assert_eq!(
            classify_cluster(Some(&marker), false, Some("17"), Some("9")),
            ClusterState::NeedsRepair
        );
    }

    #[test]
    fn port_selection_is_bounded_and_persisted_conflicts_fail_closed() {
        assert_eq!(select_managed_port(None, &[55432, 55433]), Ok(55434));
        assert_eq!(
            select_managed_port(Some(55432), &[55432]),
            Err(ServiceError::EndpointConflict)
        );
        assert_eq!(
            select_managed_port(None, &(55432..=55463).collect::<Vec<_>>()),
            Err(ServiceError::NoPrivatePortAvailable)
        );
    }

    #[test]
    fn systemd_path_escaping_rejects_directive_injection() {
        assert_eq!(
            systemd_path_value(Path::new("/srv/Synveil Objects")).unwrap(),
            "\"/srv/Synveil Objects\""
        );
        assert_eq!(
            systemd_path_value(Path::new("/srv/x\nReadWritePaths=/etc")),
            Err(ServiceError::InvalidStoragePath)
        );
        assert_eq!(
            systemd_path_value(Path::new("relative")),
            Err(ServiceError::InvalidStoragePath)
        );
    }
}
