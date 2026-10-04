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
    Managed(ServerConfig),
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

/// Select one configuration authority. The production default is fixed at
/// `/etc/synveil/server-config.json`; an override is explicit and absolute.
/// No current-directory or directory-scanning discovery is performed.
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
        let layout = LinuxConfigLayout::at_managed_root(root.to_path_buf())
            .map_err(|_| RuntimeServerConfigurationError::InvalidConfigurationPath)?;
        (ServerConfigStore::new(layout), true)
    } else {
        (ServerConfigStore::managed_linux(), false)
    };

    select_server_configuration(&store, required)
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
        || config.storage == StorageConfiguration::NotConfigured
    {
        return Err(RuntimeServerConfigurationError::ConfigurationNotReady);
    }
    Ok(RuntimeServerConfiguration::Managed(config))
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

    use synveil_server_config::{
        DeploymentProfile, ExistingServerEvidence, ExternalDatabaseCredential, NewServerConfig,
    };

    use super::{
        RuntimeServerConfiguration, RuntimeServerConfigurationError, SERVER_CONFIG_FILE_ENV,
        select_server_configuration, server_configuration_from_runtime,
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
    fn explicit_managed_authority_rejects_mixed_operator_environment() {
        clear_env(|| {
            let temp = tempfile::tempdir().unwrap();
            let layout =
                synveil_server_config::LinuxConfigLayout::at_root(temp.path().join("etc/synveil"))
                    .unwrap();
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
    fn advanced_external_config_uses_read_only_shared_authority() {
        clear_env(|| {
            let temp = tempfile::tempdir().unwrap();
            let layout =
                synveil_server_config::LinuxConfigLayout::at_root(temp.path().join("etc/synveil"))
                    .unwrap();
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
            store
                .update_storage(
                    fingerprint,
                    temp.path().join("objects").to_string_lossy().into_owned(),
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
}
