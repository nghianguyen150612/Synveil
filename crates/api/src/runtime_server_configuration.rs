//! Read-only selection of managed SERVER_CONFIG versus legacy operator mode.

use std::{env, fmt, path::PathBuf};

use synveil_server_config::{
    ConfigInspection, DatabaseCredentialState, LinuxConfigLayout, SERVER_CONFIG_FILE_NAME,
    ServerConfig, ServerConfigStore, StorageConfiguration,
};

pub const SERVER_CONFIG_FILE_ENV: &str = "SYNVEIL_SERVER_CONFIG_FILE";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeServerConfiguration {
    LegacyOperator,
    Managed(Box<ServerConfig>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeServerConfigurationError {
    InvalidConfigurationPath,
    ConfigurationRequiresRepair(ConfigInspection),
    AmbiguousAuthority,
    ConfigurationNotReady,
}

impl fmt::Display for RuntimeServerConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfigurationPath => {
                formatter.write_str("managed server configuration path is invalid")
            }
            Self::ConfigurationRequiresRepair(_) => formatter.write_str(
                "managed server configuration requires repair or a newer Synveil version",
            ),
            Self::AmbiguousAuthority => formatter.write_str(
                "managed server configuration conflicts with legacy operator environment inputs",
            ),
            Self::ConfigurationNotReady => formatter
                .write_str("managed server configuration is not ready for runtime services"),
        }
    }
}

impl std::error::Error for RuntimeServerConfigurationError {}

/// Select one configuration authority. Linux production defaults to the fixed
/// `/etc/synveil/server-config.json` path. Other platforms remain in explicit
/// legacy operator mode unless an absolute override is supplied. No
/// current-directory or directory-scanning discovery is performed.
pub fn server_configuration_from_runtime()
-> Result<RuntimeServerConfiguration, RuntimeServerConfigurationError> {
    let override_path = env::var_os(SERVER_CONFIG_FILE_ENV);
    let (store, required) = if let Some(path) = override_path {
        let path = PathBuf::from(path);
        if !path.is_absolute()
            || path
                .file_name()
                .is_none_or(|name| name != SERVER_CONFIG_FILE_NAME)
        {
            return Err(RuntimeServerConfigurationError::InvalidConfigurationPath);
        }
        let root = path
            .parent()
            .ok_or(RuntimeServerConfigurationError::InvalidConfigurationPath)?;
        let layout = explicit_runtime_layout(root.to_path_buf())
            .map_err(|_| RuntimeServerConfigurationError::InvalidConfigurationPath)?;
        (ServerConfigStore::new(layout), true)
    } else {
        #[cfg(target_os = "linux")]
        {
            (ServerConfigStore::managed_linux(), false)
        }
        #[cfg(not(target_os = "linux"))]
        {
            return Ok(RuntimeServerConfiguration::LegacyOperator);
        }
    };

    select_server_configuration(&store, required)
}

fn explicit_runtime_layout(
    root: PathBuf,
) -> Result<LinuxConfigLayout, synveil_server_config::ConfigStoreError> {
    #[cfg(target_os = "linux")]
    {
        LinuxConfigLayout::at_managed_root(root)
    }
    #[cfg(not(target_os = "linux"))]
    {
        LinuxConfigLayout::at_root(root)
    }
}

fn select_server_configuration(
    store: &ServerConfigStore,
    required: bool,
) -> Result<RuntimeServerConfiguration, RuntimeServerConfigurationError> {
    let loaded = store
        .load_config_read_only()
        .map_err(RuntimeServerConfigurationError::ConfigurationRequiresRepair)?;
    let Some((config, _fingerprint)) = loaded else {
        if required {
            return Err(
                RuntimeServerConfigurationError::ConfigurationRequiresRepair(
                    ConfigInspection::Absent,
                ),
            );
        }
        return Ok(RuntimeServerConfiguration::LegacyOperator);
    };

    if has_legacy_configuration_inputs() {
        return Err(RuntimeServerConfigurationError::AmbiguousAuthority);
    }
    if config.database.credential_state != DatabaseCredentialState::Materialized
        || !matches!(config.storage, StorageConfiguration::ConfiguredLocal { .. })
    {
        return Err(RuntimeServerConfigurationError::ConfigurationNotReady);
    }
    Ok(RuntimeServerConfiguration::Managed(Box::new(config)))
}

fn has_legacy_configuration_inputs() -> bool {
    [
        "DATABASE_URL",
        "SYNVEIL_OBJECT_ROOT",
        "SYNVEIL_BIND_ADDR",
        "SYNVEIL_PUBLIC_ORIGIN",
        "SYNVEIL_REBASELINE_TOKEN_KEY",
    ]
    .into_iter()
    .any(|key| env::var_os(key).is_some())
}

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use std::env;

    #[cfg(unix)]
    use synveil_server_config::{
        ConfigInspection, DeploymentProfile, ExistingServerEvidence, ExternalDatabaseCredential,
        NewServerConfig, StorageId, StorageRootIdentity,
    };
    #[cfg(unix)]
    use synveil_storage::{
        CapabilityEvidence, CapabilitySupport, StorageAvailability, StorageBackendKind,
        StorageCapabilities, StorageCapability,
    };

    #[cfg(unix)]
    use super::select_server_configuration;
    use super::{
        RuntimeServerConfiguration, RuntimeServerConfigurationError, SERVER_CONFIG_FILE_ENV,
        server_configuration_from_runtime,
    };

    fn clear_env(f: impl FnOnce()) {
        let _guard = crate::runtime_test_support::environment_lock();
        let names = [
            SERVER_CONFIG_FILE_ENV,
            "DATABASE_URL",
            "SYNVEIL_OBJECT_ROOT",
            "SYNVEIL_BIND_ADDR",
            "SYNVEIL_PUBLIC_ORIGIN",
            "SYNVEIL_REBASELINE_TOKEN_KEY",
        ];
        let original = names.map(env::var_os);
        unsafe {
            for name in names {
                env::remove_var(name);
            }
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        unsafe {
            for (name, value) in names.into_iter().zip(original) {
                if let Some(value) = value {
                    env::set_var(name, value);
                } else {
                    env::remove_var(name);
                }
            }
        }
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }

    #[cfg(unix)]
    fn fixture_layout(temp: &tempfile::TempDir) -> synveil_server_config::LinuxConfigLayout {
        // macOS exposes temporary directories through `/var`, which is an
        // intentional system symlink to `/private/var`. Resolve that fixture
        // alias before exercising the config store's strict no-symlink path
        // checks; the test should validate the selected config root itself.
        synveil_server_config::LinuxConfigLayout::at_root(
            temp.path().canonicalize().unwrap().join("etc/synveil"),
        )
        .unwrap()
    }

    #[cfg(unix)]
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
    fn absence_uses_legacy_operator_mode_without_cwd_search() {
        clear_env(|| {
            assert_eq!(
                server_configuration_from_runtime().unwrap(),
                RuntimeServerConfiguration::LegacyOperator
            );
        });
    }

    #[test]
    #[cfg(unix)]
    fn explicit_managed_authority_rejects_mixed_operator_environment() {
        clear_env(|| {
            let temp = tempfile::tempdir().unwrap();
            let layout = fixture_layout(&temp);
            let store = synveil_server_config::ServerConfigStore::new(layout.clone());
            store
                .initialize_managed(
                    NewServerConfig {
                        deployment_profile: DeploymentProfile::PersonalHomeManaged,
                    },
                    ExistingServerEvidence::NoKnownServerState,
                )
                .unwrap();
            // P033 owns endpoint materialization; a pending V1 managed config is
            // rejected before a runtime could start with guessed DB settings.
            unsafe {
                env::set_var(
                    "DATABASE_URL",
                    "postgresql://canary:secret@example.invalid/db",
                );
            }
            assert_eq!(
                select_server_configuration(&store, true).unwrap_err(),
                RuntimeServerConfigurationError::AmbiguousAuthority
            );
            unsafe {
                env::remove_var("DATABASE_URL");
            }
            assert_eq!(
                select_server_configuration(&store, true).unwrap_err(),
                RuntimeServerConfigurationError::ConfigurationNotReady
            );
            assert!(
                !format!(
                    "{:?}",
                    select_server_configuration(&store, true).unwrap_err()
                )
                .contains("canary")
            );
        });
    }

    #[test]
    fn relative_override_is_rejected() {
        clear_env(|| {
            unsafe {
                env::set_var(SERVER_CONFIG_FILE_ENV, "relative/server-config.json");
            }
            assert_eq!(
                server_configuration_from_runtime().unwrap_err(),
                RuntimeServerConfigurationError::InvalidConfigurationPath
            );
        });
    }

    #[test]
    #[cfg(unix)]
    fn advanced_external_config_uses_read_only_shared_authority() {
        clear_env(|| {
            let temp = tempfile::tempdir().unwrap();
            let layout = fixture_layout(&temp);
            let store = synveil_server_config::ServerConfigStore::new(layout.clone());
            store
                .initialize_external(
                    NewServerConfig {
                        deployment_profile: DeploymentProfile::AdvancedExternal,
                    },
                    ExistingServerEvidence::NoKnownServerState,
                    ExternalDatabaseCredential::new(
                        "postgresql://operator:CANARY-password@db.example.invalid:5544/synveil"
                            .to_owned(),
                    )
                    .unwrap(),
                )
                .unwrap();
            unsafe {
                env::set_var(SERVER_CONFIG_FILE_ENV, layout.config_path());
            }
            let fingerprint = match store.inspect() {
                synveil_server_config::ConfigInspection::ValidCurrent { fingerprint, .. } => {
                    fingerprint
                }
                other => panic!("unexpected inspection: {other:?}"),
            };
            let root = temp
                .path()
                .canonicalize()
                .unwrap()
                .join("objects")
                .to_string_lossy()
                .into_owned();
            let storage_id = StorageId::new_v7();
            store
                .begin_storage_preparation(
                    fingerprint,
                    root.clone(),
                    storage_id.clone(),
                    StorageRootIdentity::MissingLeaf {
                        parent_device: 1,
                        parent_inode: 1,
                    },
                )
                .unwrap();
            let preparing_fingerprint = match store.inspect() {
                ConfigInspection::ValidCurrent { fingerprint, .. } => fingerprint,
                other => panic!("unexpected inspection: {other:?}"),
            };
            let directory_identity = StorageRootIdentity::Directory {
                device: 1,
                inode: 2,
            };
            store
                .record_storage_directory_identity(
                    preparing_fingerprint,
                    root.clone(),
                    storage_id.clone(),
                    directory_identity.clone(),
                )
                .unwrap();
            let ready_fingerprint = match store.inspect() {
                ConfigInspection::ValidCurrent { fingerprint, .. } => fingerprint,
                other => panic!("unexpected inspection: {other:?}"),
            };
            store
                .complete_storage_preparation(
                    ready_fingerprint,
                    root,
                    storage_id,
                    directory_identity,
                    ready_storage_capabilities(),
                )
                .unwrap();
            let selected = select_server_configuration(&store, true).unwrap();
            assert!(matches!(
                &selected,
                RuntimeServerConfiguration::Managed(config)
                    if config.deployment_profile == DeploymentProfile::AdvancedExternal
                        && config.database.endpoint.as_ref().is_some_and(|endpoint| endpoint.port == 5544)
            ));
            assert!(!format!("{selected:?}").contains("CANARY-password"));
        });
    }

    #[test]
    #[cfg(unix)]
    fn preparing_storage_is_not_runtime_ready() {
        clear_env(|| {
            let temp = tempfile::tempdir().unwrap();
            let layout = fixture_layout(&temp);
            let store = synveil_server_config::ServerConfigStore::new(layout);
            store
                .initialize_external(
                    NewServerConfig {
                        deployment_profile: DeploymentProfile::AdvancedExternal,
                    },
                    ExistingServerEvidence::NoKnownServerState,
                    ExternalDatabaseCredential::new(
                        "postgresql://operator:CANARY-password@db.example.invalid/synveil"
                            .to_owned(),
                    )
                    .unwrap(),
                )
                .unwrap();
            let fingerprint = match store.inspect() {
                ConfigInspection::ValidCurrent { fingerprint, .. } => fingerprint,
                other => panic!("unexpected inspection: {other:?}"),
            };
            store
                .begin_storage_preparation(
                    fingerprint,
                    temp.path()
                        .canonicalize()
                        .unwrap()
                        .join("object-data")
                        .to_string_lossy()
                        .into_owned(),
                    StorageId::new_v7(),
                    StorageRootIdentity::MissingLeaf {
                        parent_device: 1,
                        parent_inode: 1,
                    },
                )
                .unwrap();
            assert_eq!(
                select_server_configuration(&store, true),
                Err(RuntimeServerConfigurationError::ConfigurationNotReady)
            );
        });
    }
}
