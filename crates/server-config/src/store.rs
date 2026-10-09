use fs2::FileExt as _;
use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
use synveil_object_store::StorageCapabilities;

use crate::{
    ConfigFingerprint, ConfigValidationError, CredentialId, DatabaseCredentialState,
    DatabaseEndpoint, DeploymentProfile, ExternalDatabaseCredential, MAX_SERVER_CONFIG_BYTES,
    NetworkConfiguration, ServerConfig, StorageConfiguration, StorageId, StorageRootIdentity,
};

pub const SERVER_CONFIG_FILE_NAME: &str = "server-config.json";
pub const CREDENTIAL_DIRECTORY_NAME: &str = "credentials";
pub const DEFAULT_MANAGED_CONFIG_ROOT: &str = "/etc/synveil";
pub const CONFIG_DIRECTORY_MODE: u32 = 0o750;
pub const CONFIG_FILE_MODE: u32 = 0o640;
pub const CREDENTIAL_DIRECTORY_MODE: u32 = 0o700;
pub const SECRET_FILE_MODE: u32 = 0o600;
const MAX_SECRET_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinuxConfigLayout {
    root: PathBuf,
    require_managed_ownership: bool,
}

impl LinuxConfigLayout {
    #[must_use]
    pub fn managed() -> Self {
        Self {
            root: PathBuf::from(DEFAULT_MANAGED_CONFIG_ROOT),
            require_managed_ownership: true,
        }
    }

    /// Explicit fixture/development root. Production callers use [`Self::managed`].
    pub fn at_root(root: impl Into<PathBuf>) -> Result<Self, ConfigStoreError> {
        let root = root.into();
        validate_absolute_directory_path(&root)?;
        Ok(Self {
            root,
            require_managed_ownership: false,
        })
    }

    /// Explicit managed root override. It retains root-owned production checks;
    /// use [`Self::at_root`] only for disposable fixtures and development.
    pub fn at_managed_root(root: impl Into<PathBuf>) -> Result<Self, ConfigStoreError> {
        let root = root.into();
        validate_absolute_directory_path(&root)?;
        Ok(Self {
            root,
            require_managed_ownership: true,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn config_path(&self) -> PathBuf {
        self.root.join(SERVER_CONFIG_FILE_NAME)
    }

    #[must_use]
    pub fn credential_directory(&self) -> PathBuf {
        self.root.join(CREDENTIAL_DIRECTORY_NAME)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExistingServerEvidence {
    NoKnownServerState,
    ExistingOrAmbiguousState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewServerConfig {
    pub deployment_profile: DeploymentProfile,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InitializeResult {
    Created(ServerConfig),
    Reused(ServerConfig),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigInspection {
    Absent,
    ValidCurrent {
        config: Box<ServerConfig>,
        fingerprint: ConfigFingerprint,
    },
    UnsupportedSchema,
    Malformed,
    WrongType,
    SymlinkOrRedirected,
    PermissionMismatch,
    IdentityConflict,
    SecretMissing {
        id: CredentialId,
    },
    SecretInvalid {
        id: CredentialId,
    },
    PartialConfiguration,
}

impl ConfigInspection {
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        matches!(self, Self::ValidCurrent { .. })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteFailurePoint {
    BeforeRootDirectoryCreate,
    AfterRootDirectoryCreate,
    BeforeCredentialDirectoryCreate,
    AfterCredentialDirectoryCreate,
    BeforeTemporaryCreate,
    AfterTemporaryWrite,
    AfterFileSync,
    BeforeRename,
    AfterRename,
    BeforeParentSync,
    AfterParentSync,
    PostWriteVerification,
    BeforeSecretCreate,
    AfterSecretWrite,
    AfterSecretFileSync,
    AfterSecretParentSync,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigStoreError {
    NeedsRepair(ConfigInspection),
    Configuration(ConfigValidationError),
    InvalidPath,
    PermissionMismatch,
    SymlinkOrRedirected,
    WrongType,
    ConcurrentModification,
    GenerationExhausted,
    SecretAlreadyExists { id: CredentialId },
    ExternalCredentialRequired,
    StorageStateConflict,
    InvalidSecret { id: CredentialId },
    OutcomeUnknown,
    FailureInjected(WriteFailurePoint),
    Io,
}

impl fmt::Display for ConfigStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NeedsRepair(_) => {
                formatter.write_str("managed server configuration needs repair")
            }
            Self::Configuration(error) => error.fmt(formatter),
            Self::InvalidPath => {
                formatter.write_str("managed server configuration path is invalid")
            }
            Self::PermissionMismatch => {
                formatter.write_str("managed server configuration permissions do not match policy")
            }
            Self::SymlinkOrRedirected => {
                formatter.write_str("managed server configuration path is redirected")
            }
            Self::WrongType => {
                formatter.write_str("managed server configuration has an unexpected file type")
            }
            Self::ConcurrentModification => {
                formatter.write_str("managed server configuration changed during update")
            }
            Self::GenerationExhausted => {
                formatter.write_str("managed server configuration generation is exhausted")
            }
            Self::SecretAlreadyExists { .. } => {
                formatter.write_str("managed secret already exists and was preserved")
            }
            Self::ExternalCredentialRequired => {
                formatter.write_str("external PostgreSQL credential is required")
            }
            Self::StorageStateConflict => {
                formatter.write_str("managed storage state changed and requires review")
            }
            Self::InvalidSecret { .. } => formatter.write_str("managed secret is invalid"),
            Self::OutcomeUnknown => {
                formatter.write_str("managed write outcome is unknown; inspect current state")
            }
            Self::FailureInjected(_) => {
                formatter.write_str("managed write was interrupted by failure injection")
            }
            Self::Io => formatter.write_str("managed configuration filesystem operation failed"),
        }
    }
}

impl std::error::Error for ConfigStoreError {}

impl From<ConfigValidationError> for ConfigStoreError {
    fn from(error: ConfigValidationError) -> Self {
        Self::Configuration(error)
    }
}

#[derive(Clone, Debug)]
pub struct ServerConfigStore {
    layout: LinuxConfigLayout,
    #[cfg(test)]
    failure_point: Option<WriteFailurePoint>,
}

impl ServerConfigStore {
    #[must_use]
    pub fn new(layout: LinuxConfigLayout) -> Self {
        Self {
            layout,
            #[cfg(test)]
            failure_point: None,
        }
    }

    #[must_use]
    pub fn managed_linux() -> Self {
        Self::new(LinuxConfigLayout::managed())
    }

    #[must_use]
    pub fn layout(&self) -> &LinuxConfigLayout {
        &self.layout
    }

    /// Runtime-safe read of the non-secret authority. It does not inspect or
    /// open the root-only credential source directory.
    pub fn load_config_read_only(
        &self,
    ) -> Result<Option<(ServerConfig, ConfigFingerprint)>, ConfigInspection> {
        match self.read_config_only() {
            ConfigInspection::ValidCurrent {
                config,
                fingerprint,
            } => Ok(Some((*config, fingerprint))),
            ConfigInspection::Absent => Ok(None),
            problem => Err(problem),
        }
    }

    /// Read the generated managed PostgreSQL role password for the privileged
    /// P033 provisioning boundary. The returned wrapper is zeroized on drop
    /// and redacted by both `Debug` and `Display`.
    pub fn managed_database_password(&self) -> Result<crate::SecretMaterial, ConfigStoreError> {
        match self.inspect() {
            ConfigInspection::ValidCurrent { config, .. }
                if config.deployment_profile == DeploymentProfile::PersonalHomeManaged => {}
            problem => return Err(ConfigStoreError::NeedsRepair(problem)),
        }
        let bytes = read_regular_bounded(
            &self.secret_path(CredentialId::DatabasePassword),
            MAX_SECRET_BYTES,
            SECRET_FILE_MODE,
            self.layout.require_managed_ownership,
        )?
        .ok_or(ConfigStoreError::InvalidSecret {
            id: CredentialId::DatabasePassword,
        })?;
        let bytes = zeroize::Zeroizing::new(bytes);
        if !secret_bytes_valid(&bytes, SecretKind::HexKey) {
            return Err(ConfigStoreError::InvalidSecret {
                id: CredentialId::DatabasePassword,
            });
        }
        let value = std::str::from_utf8(&bytes)
            .map_err(|_| ConfigStoreError::InvalidSecret {
                id: CredentialId::DatabasePassword,
            })?
            .to_owned();
        Ok(crate::SecretMaterial::new(value))
    }

    pub fn inspect(&self) -> ConfigInspection {
        match self.check_layout(true) {
            Ok(()) => {}
            Err(ConfigStoreError::PermissionMismatch) => {
                return ConfigInspection::PermissionMismatch;
            }
            Err(ConfigStoreError::SymlinkOrRedirected) => {
                return ConfigInspection::SymlinkOrRedirected;
            }
            Err(ConfigStoreError::WrongType) => return ConfigInspection::WrongType,
            Err(_) => return ConfigInspection::PartialConfiguration,
        }

        let path = self.layout.config_path();
        let bytes = match read_regular_bounded(
            &path,
            MAX_SERVER_CONFIG_BYTES,
            CONFIG_FILE_MODE,
            self.layout.require_managed_ownership,
        ) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return self.classify_absent_config(),
            Err(ConfigStoreError::PermissionMismatch) => {
                return ConfigInspection::PermissionMismatch;
            }
            Err(ConfigStoreError::SymlinkOrRedirected) => {
                return ConfigInspection::SymlinkOrRedirected;
            }
            Err(ConfigStoreError::WrongType) => return ConfigInspection::WrongType,
            Err(_) => return ConfigInspection::Malformed,
        };
        let config = match ServerConfig::parse(&bytes) {
            Ok(config) => config,
            Err(ConfigValidationError::UnsupportedSchema) => {
                return ConfigInspection::UnsupportedSchema;
            }
            Err(ConfigValidationError::InvalidInstallationId) => {
                return ConfigInspection::IdentityConflict;
            }
            Err(_) => return ConfigInspection::Malformed,
        };
        let canonical = match config.canonical_bytes() {
            Ok(bytes) => bytes,
            Err(_) => return ConfigInspection::Malformed,
        };
        let fingerprint = ConfigFingerprint::from_canonical_bytes(&canonical);
        match self.validate_required_secrets(&config) {
            Ok(()) => ConfigInspection::ValidCurrent {
                config: Box::new(config),
                fingerprint,
            },
            Err(ConfigInspection::SecretMissing { id }) => ConfigInspection::SecretMissing { id },
            Err(ConfigInspection::SecretInvalid { id }) => ConfigInspection::SecretInvalid { id },
            Err(problem) => problem,
        }
    }

    /// Create a new authority only after preflight has proved that this is a
    /// fresh Host. Existing config or any known secret is inspected, never
    /// overwritten or regenerated.
    pub fn initialize_managed(
        &self,
        request: NewServerConfig,
        evidence: ExistingServerEvidence,
    ) -> Result<InitializeResult, ConfigStoreError> {
        self.initialize(request, evidence, None)
    }

    /// Create Advanced / Server Mode with an explicitly supplied external
    /// PostgreSQL URL. The URL is validated and persisted only as protected
    /// `database-url` secret material.
    pub fn initialize_external(
        &self,
        request: NewServerConfig,
        evidence: ExistingServerEvidence,
        credential: ExternalDatabaseCredential,
    ) -> Result<InitializeResult, ConfigStoreError> {
        self.initialize(request, evidence, Some(credential))
    }

    fn initialize(
        &self,
        request: NewServerConfig,
        evidence: ExistingServerEvidence,
        external_credential: Option<ExternalDatabaseCredential>,
    ) -> Result<InitializeResult, ConfigStoreError> {
        match self.inspect() {
            ConfigInspection::ValidCurrent { config, .. } => {
                if config.deployment_profile != request.deployment_profile {
                    return Err(ConfigStoreError::NeedsRepair(
                        ConfigInspection::IdentityConflict,
                    ));
                }
                return Ok(InitializeResult::Reused(*config));
            }
            ConfigInspection::Absent => {}
            other => return Err(ConfigStoreError::NeedsRepair(other)),
        }
        if evidence == ExistingServerEvidence::ExistingOrAmbiguousState {
            return Err(ConfigStoreError::NeedsRepair(
                ConfigInspection::IdentityConflict,
            ));
        }
        match request.deployment_profile {
            DeploymentProfile::PersonalHomeManaged if external_credential.is_some() => {
                return Err(ConfigStoreError::InvalidPath);
            }
            DeploymentProfile::AdvancedExternal if external_credential.is_none() => {
                return Err(ConfigStoreError::ExternalCredentialRequired);
            }
            _ => {}
        }
        self.ensure_layout()?;
        match self.inspect() {
            ConfigInspection::Absent => {}
            ConfigInspection::ValidCurrent { config, .. }
                if config.deployment_profile == request.deployment_profile =>
            {
                return Ok(InitializeResult::Reused(*config));
            }
            problem => return Err(ConfigStoreError::NeedsRepair(problem)),
        }
        let installation_id = uuid::Uuid::now_v7().to_string();
        let mut config = ServerConfig::initial(request.deployment_profile, installation_id);
        if let Some(credential) = external_credential.as_ref() {
            config.database.endpoint = credential.non_secret_endpoint();
        }
        config.validate()?;

        let rebaseline = generate_hex_secret()?;
        self.create_secret(
            CredentialId::RebaselineTokenKey,
            rebaseline.expose_for_storage(),
        )?;
        match request.deployment_profile {
            DeploymentProfile::PersonalHomeManaged => {
                let password = generate_hex_secret()?;
                self.create_secret(
                    CredentialId::DatabasePassword,
                    password.expose_for_storage(),
                )?;
            }
            DeploymentProfile::AdvancedExternal => {
                let credential = external_credential
                    .as_ref()
                    .ok_or(ConfigStoreError::ExternalCredentialRequired)?;
                self.create_secret(CredentialId::DatabaseUrl, credential.expose_for_storage())?;
            }
        }
        self.atomic_write_config(&config, None)?;
        Ok(InitializeResult::Created(config))
    }

    /// Durably record the confirmed storage intent before any filesystem
    /// initialization. Repeating the same pending operation is idempotent.
    pub fn begin_storage_preparation(
        &self,
        expected: ConfigFingerprint,
        root: String,
        storage_id: StorageId,
        root_identity: StorageRootIdentity,
    ) -> Result<ServerConfig, ConfigStoreError> {
        let mut config = self.current_for_update(expected)?;
        match &config.storage {
            StorageConfiguration::NotConfigured => {
                config.storage = StorageConfiguration::PreparingLocal {
                    root,
                    storage_id,
                    root_identity,
                };
            }
            StorageConfiguration::PreparingLocal {
                root: current_root,
                storage_id: current_id,
                root_identity: current_identity,
            } if current_root == &root
                && current_id == &storage_id
                && current_identity == &root_identity =>
            {
                return Ok(config);
            }
            StorageConfiguration::PreparingLocal { .. }
            | StorageConfiguration::ConfiguredLocal { .. } => {
                return Err(ConfigStoreError::StorageStateConflict);
            }
        }
        config.generation = config
            .generation
            .checked_add(1)
            .ok_or(ConfigStoreError::GenerationExhausted)?;
        self.atomic_write_config(&config, Some(expected))?;
        Ok(config)
    }

    /// Bind the directory inode after a missing selected leaf is created with
    /// its matching identity marker already present. Repeating the exact
    /// identity update is idempotent.
    pub fn record_storage_directory_identity(
        &self,
        expected: ConfigFingerprint,
        root: String,
        storage_id: StorageId,
        root_identity: StorageRootIdentity,
    ) -> Result<ServerConfig, ConfigStoreError> {
        if !root_identity.is_directory() || !root_identity.is_valid() {
            return Err(ConfigStoreError::StorageStateConflict);
        }
        let mut config = self.current_for_update(expected)?;
        match &config.storage {
            StorageConfiguration::PreparingLocal {
                root: current_root,
                storage_id: current_id,
                root_identity: current_identity,
            } if current_root == &root && current_id == &storage_id => match current_identity {
                StorageRootIdentity::MissingLeaf { .. } => {
                    config.storage = StorageConfiguration::PreparingLocal {
                        root,
                        storage_id,
                        root_identity,
                    };
                }
                StorageRootIdentity::Directory { .. } if current_identity == &root_identity => {
                    return Ok(config);
                }
                StorageRootIdentity::Directory { .. } => {
                    return Err(ConfigStoreError::StorageStateConflict);
                }
            },
            _ => return Err(ConfigStoreError::StorageStateConflict),
        }
        config.generation = config
            .generation
            .checked_add(1)
            .ok_or(ConfigStoreError::GenerationExhausted)?;
        self.atomic_write_config(&config, Some(expected))?;
        Ok(config)
    }

    /// Commit readiness only after the managed identity, ObjectStore layout,
    /// and required filesystem probes have been verified. Repeating the same
    /// completed transition is idempotent.
    pub fn complete_storage_preparation(
        &self,
        expected: ConfigFingerprint,
        root: String,
        storage_id: StorageId,
        root_identity: StorageRootIdentity,
        capabilities: StorageCapabilities,
    ) -> Result<ServerConfig, ConfigStoreError> {
        let mut config = self.current_for_update(expected)?;
        match &config.storage {
            StorageConfiguration::PreparingLocal {
                root: current_root,
                storage_id: current_id,
                root_identity: current_identity,
            } if current_root == &root
                && current_id == &storage_id
                && current_identity == &root_identity
                && root_identity.is_directory() =>
            {
                config.storage = StorageConfiguration::ConfiguredLocal {
                    root,
                    storage_id,
                    root_identity,
                    capabilities,
                };
            }
            StorageConfiguration::ConfiguredLocal {
                root: current_root,
                storage_id: current_id,
                root_identity: current_identity,
                capabilities: current_capabilities,
            } if current_root == &root
                && current_id == &storage_id
                && current_identity == &root_identity
                && current_capabilities == &capabilities =>
            {
                return Ok(config);
            }
            _ => return Err(ConfigStoreError::StorageStateConflict),
        }
        config.generation = config
            .generation
            .checked_add(1)
            .ok_or(ConfigStoreError::GenerationExhausted)?;
        self.atomic_write_config(&config, Some(expected))?;
        Ok(config)
    }

    /// Typed local-only network mutation boundary for P034. This foundation
    /// deliberately has no public/LAN state or listener side effect.
    pub fn update_network_local_private(
        &self,
        expected: ConfigFingerprint,
        network: NetworkConfiguration,
    ) -> Result<ServerConfig, ConfigStoreError> {
        let mut config = self.current_for_update(expected)?;
        config.network = network;
        config.generation = config
            .generation
            .checked_add(1)
            .ok_or(ConfigStoreError::GenerationExhausted)?;
        self.atomic_write_config(&config, Some(expected))?;
        Ok(config)
    }

    /// P033-facing endpoint handoff. It creates the final URL credential with
    /// create-new semantics, then atomically advances only the non-secret
    /// credential state in SERVER_CONFIG.
    pub fn materialize_managed_database_url(
        &self,
        expected: ConfigFingerprint,
        endpoint: DatabaseEndpoint,
    ) -> Result<ServerConfig, ConfigStoreError> {
        let mut config = self.current_for_update(expected)?;
        if config.deployment_profile != DeploymentProfile::PersonalHomeManaged
            || config.database.credential_state != DatabaseCredentialState::PendingManagedEndpoint
        {
            return Err(ConfigStoreError::NeedsRepair(
                ConfigInspection::IdentityConflict,
            ));
        }
        config.database.endpoint = Some(endpoint.clone());
        config.database.credential_state = DatabaseCredentialState::Materialized;
        config.generation = config
            .generation
            .checked_add(1)
            .ok_or(ConfigStoreError::GenerationExhausted)?;
        config.validate()?;
        let password = self.managed_database_password()?;
        let database_url =
            password.with_secret(|password| {
                let role = config.database.runtime_role.as_deref().ok_or(
                    ConfigStoreError::NeedsRepair(ConfigInspection::IdentityConflict),
                )?;
                let database = config.database.database_name.as_deref().ok_or(
                    ConfigStoreError::NeedsRepair(ConfigInspection::IdentityConflict),
                )?;
                let host = endpoint
                    .host
                    .strip_prefix('[')
                    .and_then(|host| host.strip_suffix(']'))
                    .unwrap_or(&endpoint.host);
                let host = host
                    .parse::<std::net::IpAddr>()
                    .map(|address| {
                        if address.is_ipv6() {
                            format!("[{address}]")
                        } else {
                            address.to_string()
                        }
                    })
                    .unwrap_or_else(|_| host.to_owned());
                let rendered = crate::SecretMaterial::new(format!(
                    "postgresql://{role}:{password}@{host}:{}/{database}",
                    endpoint.port
                ));
                rendered.with_secret(|value| {
                    synveil_metadata::DatabaseConfig::from_url(value.to_owned()).map_err(|_| {
                        ConfigStoreError::Configuration(
                            ConfigValidationError::InvalidDatabaseEndpoint,
                        )
                    })
                })?;
                Ok::<_, ConfigStoreError>(rendered)
            })?;
        self.create_secret(CredentialId::DatabaseUrl, database_url.expose_for_storage())?;
        self.atomic_write_config(&config, Some(expected))?;
        Ok(config)
    }

    fn current_for_update(
        &self,
        expected: ConfigFingerprint,
    ) -> Result<ServerConfig, ConfigStoreError> {
        match self.inspect() {
            ConfigInspection::ValidCurrent {
                config,
                fingerprint,
            } if fingerprint == expected => Ok(*config),
            ConfigInspection::ValidCurrent { .. } => Err(ConfigStoreError::ConcurrentModification),
            problem => Err(ConfigStoreError::NeedsRepair(problem)),
        }
    }

    fn validate_required_secrets(&self, config: &ServerConfig) -> Result<(), ConfigInspection> {
        self.validate_secret(CredentialId::RebaselineTokenKey, SecretKind::HexKey)?;
        match config.deployment_profile {
            DeploymentProfile::PersonalHomeManaged => {
                self.validate_secret(CredentialId::DatabasePassword, SecretKind::HexKey)?;
                match config.database.credential_state {
                    DatabaseCredentialState::PendingManagedEndpoint => {
                        if self.secret_exists(CredentialId::DatabaseUrl) {
                            return Err(ConfigInspection::PartialConfiguration);
                        }
                    }
                    DatabaseCredentialState::Materialized => {
                        self.validate_secret(CredentialId::DatabaseUrl, SecretKind::DatabaseUrl)?
                    }
                }
            }
            DeploymentProfile::AdvancedExternal => {
                self.validate_secret(CredentialId::DatabaseUrl, SecretKind::DatabaseUrl)?
            }
        }
        Ok(())
    }

    fn validate_secret(&self, id: CredentialId, kind: SecretKind) -> Result<(), ConfigInspection> {
        let path = self.secret_path(id);
        let bytes = read_regular_bounded(
            &path,
            MAX_SECRET_BYTES,
            SECRET_FILE_MODE,
            self.layout.require_managed_ownership,
        )
        .map_err(|error| match error {
            ConfigStoreError::PermissionMismatch => ConfigInspection::PermissionMismatch,
            ConfigStoreError::SymlinkOrRedirected => ConfigInspection::SymlinkOrRedirected,
            ConfigStoreError::WrongType => ConfigInspection::WrongType,
            _ if !path.exists() => ConfigInspection::SecretMissing { id },
            _ => ConfigInspection::SecretInvalid { id },
        })?
        .ok_or(ConfigInspection::SecretMissing { id })?;
        let bytes = zeroize::Zeroizing::new(bytes);
        if !secret_bytes_valid(&bytes, kind) {
            return Err(ConfigInspection::SecretInvalid { id });
        }
        Ok(())
    }

    fn classify_absent_config(&self) -> ConfigInspection {
        let mut found = false;
        for id in [
            CredentialId::DatabaseUrl,
            CredentialId::DatabasePassword,
            CredentialId::RebaselineTokenKey,
        ] {
            let path = self.secret_path(id);
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return ConfigInspection::PartialConfiguration,
            };
            if metadata.file_type().is_symlink() {
                return ConfigInspection::SymlinkOrRedirected;
            }
            if !metadata.is_file() {
                return ConfigInspection::WrongType;
            }
            if verify_metadata(
                &metadata,
                SECRET_FILE_MODE,
                self.layout.require_managed_ownership,
                true,
            )
            .is_err()
            {
                return ConfigInspection::PermissionMismatch;
            }
            found = true;
        }
        if found {
            ConfigInspection::PartialConfiguration
        } else {
            ConfigInspection::Absent
        }
    }

    fn secret_exists(&self, id: CredentialId) -> bool {
        fs::symlink_metadata(self.secret_path(id)).is_ok()
    }

    fn secret_path(&self, id: CredentialId) -> PathBuf {
        self.layout.credential_directory().join(id.as_str())
    }

    fn check_layout(&self, allow_absent: bool) -> Result<(), ConfigStoreError> {
        if !check_ancestors(&self.layout.root)? {
            return Ok(());
        }
        let root = check_directory_path(
            &self.layout.root,
            CONFIG_DIRECTORY_MODE,
            self.layout.require_managed_ownership,
            allow_absent,
        )?;
        if !root {
            return Ok(());
        }
        check_directory_path(
            &self.layout.credential_directory(),
            CREDENTIAL_DIRECTORY_MODE,
            self.layout.require_managed_ownership,
            allow_absent,
        )?;
        Ok(())
    }

    fn ensure_layout(&self) -> Result<(), ConfigStoreError> {
        validate_absolute_directory_path(&self.layout.root)?;
        self.inject(WriteFailurePoint::BeforeRootDirectoryCreate)?;
        if self.layout.require_managed_ownership {
            if !check_ancestors(&self.layout.root)? {
                return Err(ConfigStoreError::InvalidPath);
            }
            create_directory_safely(
                &self.layout.root,
                CONFIG_DIRECTORY_MODE,
                SecretOwner::RootSynveil,
                true,
            )?;
        } else {
            create_directory_chain_safely(&self.layout.root, CONFIG_DIRECTORY_MODE, false)?;
        }
        let parent = self
            .layout
            .root
            .parent()
            .ok_or(ConfigStoreError::InvalidPath)?;
        sync_directory(parent).map_err(|_| ConfigStoreError::OutcomeUnknown)?;
        self.inject(WriteFailurePoint::AfterRootDirectoryCreate)?;
        self.inject(WriteFailurePoint::BeforeCredentialDirectoryCreate)?;
        create_directory_safely(
            &self.layout.credential_directory(),
            CREDENTIAL_DIRECTORY_MODE,
            SecretOwner::RootRoot,
            self.layout.require_managed_ownership,
        )?;
        sync_directory(&self.layout.root).map_err(|_| ConfigStoreError::OutcomeUnknown)?;
        self.inject(WriteFailurePoint::AfterCredentialDirectoryCreate)?;
        self.check_layout(false)
    }

    fn create_secret(&self, id: CredentialId, contents: &[u8]) -> Result<(), ConfigStoreError> {
        if contents.is_empty() || contents.len() > MAX_SECRET_BYTES || contents.contains(&0) {
            return Err(ConfigStoreError::InvalidSecret { id });
        }
        let path = self.secret_path(id);
        self.inject(WriteFailurePoint::BeforeSecretCreate)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        set_create_mode(&mut options, SECRET_FILE_MODE);
        let mut file = options.open(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                ConfigStoreError::SecretAlreadyExists { id }
            } else {
                ConfigStoreError::Io
            }
        })?;
        set_open_file_permissions(&file, SECRET_FILE_MODE)?;
        set_expected_owner(
            &file,
            SecretOwner::RootRoot,
            self.layout.require_managed_ownership,
        )?;
        file.write_all(contents)
            .map_err(|_| ConfigStoreError::OutcomeUnknown)?;
        self.inject(WriteFailurePoint::AfterSecretWrite)
            .map_err(|_| ConfigStoreError::OutcomeUnknown)?;
        file.sync_all()
            .map_err(|_| ConfigStoreError::OutcomeUnknown)?;
        self.inject(WriteFailurePoint::AfterSecretFileSync)
            .map_err(|_| ConfigStoreError::OutcomeUnknown)?;
        verify_regular_file(
            &path,
            SECRET_FILE_MODE,
            self.layout.require_managed_ownership,
            true,
        )?;
        sync_directory(&self.layout.credential_directory())
            .map_err(|_| ConfigStoreError::OutcomeUnknown)?;
        self.inject(WriteFailurePoint::AfterSecretParentSync)
            .map_err(|_| ConfigStoreError::OutcomeUnknown)?;
        let reread = read_regular_bounded(
            &path,
            MAX_SECRET_BYTES,
            SECRET_FILE_MODE,
            self.layout.require_managed_ownership,
        )?
        .ok_or(ConfigStoreError::OutcomeUnknown)?;
        let reread = zeroize::Zeroizing::new(reread);
        if reread.as_slice() != contents {
            return Err(ConfigStoreError::OutcomeUnknown);
        }
        Ok(())
    }

    fn atomic_write_config(
        &self,
        config: &ServerConfig,
        expected: Option<ConfigFingerprint>,
    ) -> Result<(), ConfigStoreError> {
        if !check_ancestors(&self.layout.root)? {
            return Err(ConfigStoreError::NeedsRepair(ConfigInspection::Absent));
        }
        let directory_lock = open_directory_no_follow(&self.layout.root)?;
        directory_lock
            .lock_exclusive()
            .map_err(|_| ConfigStoreError::Io)?;
        config.validate()?;
        let bytes = config.canonical_bytes()?;
        let current = self.read_config_only();
        match (expected, current) {
            (None, ConfigInspection::Absent | ConfigInspection::PartialConfiguration) => {}
            (Some(expected), ConfigInspection::ValidCurrent { fingerprint, .. })
                if fingerprint == expected => {}
            (Some(_), ConfigInspection::ValidCurrent { .. }) => {
                return Err(ConfigStoreError::ConcurrentModification);
            }
            (_, problem) => return Err(ConfigStoreError::NeedsRepair(problem)),
        }
        self.inject(WriteFailurePoint::BeforeTemporaryCreate)?;
        let temp_path = self.layout.root.join(format!(
            ".{SERVER_CONFIG_FILE_NAME}.tmp.{}",
            uuid::Uuid::now_v7().simple()
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        set_create_mode(&mut options, CONFIG_FILE_MODE);
        let mut file = options.open(&temp_path).map_err(|_| ConfigStoreError::Io)?;
        set_open_file_permissions(&file, CONFIG_FILE_MODE)?;
        set_expected_owner(
            &file,
            SecretOwner::RootSynveil,
            self.layout.require_managed_ownership,
        )?;
        let write_result = (|| {
            file.write_all(&bytes)
                .map_err(|_| ConfigStoreError::OutcomeUnknown)?;
            self.inject(WriteFailurePoint::AfterTemporaryWrite)
                .map_err(|_| ConfigStoreError::OutcomeUnknown)?;
            file.sync_all()
                .map_err(|_| ConfigStoreError::OutcomeUnknown)?;
            self.inject(WriteFailurePoint::AfterFileSync)
                .map_err(|_| ConfigStoreError::OutcomeUnknown)?;
            verify_regular_file(
                &temp_path,
                CONFIG_FILE_MODE,
                self.layout.require_managed_ownership,
                false,
            )?;
            self.inject(WriteFailurePoint::BeforeRename)
                .map_err(|_| ConfigStoreError::OutcomeUnknown)?;
            fs::rename(&temp_path, self.layout.config_path())
                .map_err(|_| ConfigStoreError::OutcomeUnknown)?;
            self.inject(WriteFailurePoint::AfterRename)
                .map_err(|_| ConfigStoreError::OutcomeUnknown)?;
            self.inject(WriteFailurePoint::BeforeParentSync)
                .map_err(|_| ConfigStoreError::OutcomeUnknown)?;
            sync_directory(&self.layout.root).map_err(|_| ConfigStoreError::OutcomeUnknown)?;
            self.inject(WriteFailurePoint::AfterParentSync)
                .map_err(|_| ConfigStoreError::OutcomeUnknown)?;
            self.inject(WriteFailurePoint::PostWriteVerification)
                .map_err(|_| ConfigStoreError::OutcomeUnknown)?;
            let actual = read_regular_bounded(
                &self.layout.config_path(),
                MAX_SERVER_CONFIG_BYTES,
                CONFIG_FILE_MODE,
                self.layout.require_managed_ownership,
            )?
            .ok_or(ConfigStoreError::OutcomeUnknown)?;
            if actual != bytes || ServerConfig::parse(&actual).ok().as_ref() != Some(config) {
                return Err(ConfigStoreError::OutcomeUnknown);
            }
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        write_result
    }

    fn inject(&self, point: WriteFailurePoint) -> Result<(), ConfigStoreError> {
        #[cfg(test)]
        if self.failure_point == Some(point) {
            return Err(ConfigStoreError::FailureInjected(point));
        }
        let _ = point;
        Ok(())
    }

    fn read_config_only(&self) -> ConfigInspection {
        match check_ancestors(&self.layout.root) {
            Ok(true) => {}
            Ok(false) => return ConfigInspection::Absent,
            Err(ConfigStoreError::SymlinkOrRedirected) => {
                return ConfigInspection::SymlinkOrRedirected;
            }
            Err(ConfigStoreError::WrongType) => return ConfigInspection::WrongType,
            Err(_) => return ConfigInspection::Malformed,
        }
        match check_directory_path(
            &self.layout.root,
            CONFIG_DIRECTORY_MODE,
            self.layout.require_managed_ownership,
            true,
        ) {
            Ok(false) => return ConfigInspection::Absent,
            Ok(true) => {}
            Err(ConfigStoreError::PermissionMismatch) => {
                return ConfigInspection::PermissionMismatch;
            }
            Err(ConfigStoreError::SymlinkOrRedirected) => {
                return ConfigInspection::SymlinkOrRedirected;
            }
            Err(ConfigStoreError::WrongType) => return ConfigInspection::WrongType,
            Err(_) => return ConfigInspection::Malformed,
        }
        let path = self.layout.config_path();
        let bytes = match read_regular_bounded(
            &path,
            MAX_SERVER_CONFIG_BYTES,
            CONFIG_FILE_MODE,
            self.layout.require_managed_ownership,
        ) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return ConfigInspection::Absent,
            Err(ConfigStoreError::PermissionMismatch) => {
                return ConfigInspection::PermissionMismatch;
            }
            Err(ConfigStoreError::SymlinkOrRedirected) => {
                return ConfigInspection::SymlinkOrRedirected;
            }
            Err(ConfigStoreError::WrongType) => return ConfigInspection::WrongType,
            Err(_) => return ConfigInspection::Malformed,
        };
        let config = match ServerConfig::parse(&bytes) {
            Ok(config) => config,
            Err(ConfigValidationError::UnsupportedSchema) => {
                return ConfigInspection::UnsupportedSchema;
            }
            Err(ConfigValidationError::InvalidInstallationId) => {
                return ConfigInspection::IdentityConflict;
            }
            Err(_) => return ConfigInspection::Malformed,
        };
        let canonical = match config.canonical_bytes() {
            Ok(bytes) => bytes,
            Err(_) => return ConfigInspection::Malformed,
        };
        ConfigInspection::ValidCurrent {
            config: Box::new(config),
            fingerprint: ConfigFingerprint::from_canonical_bytes(&canonical),
        }
    }
}

#[derive(Clone, Copy)]
enum SecretKind {
    HexKey,
    DatabaseUrl,
}

fn generate_hex_secret() -> Result<crate::SecretMaterial, ConfigStoreError> {
    let mut random = zeroize::Zeroizing::new([0_u8; 32]);
    getrandom::fill(&mut *random).map_err(|_| ConfigStoreError::Io)?;
    let mut encoded = String::with_capacity(64);
    for byte in random.iter() {
        use fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    Ok(crate::SecretMaterial::new(encoded))
}

fn secret_bytes_valid(bytes: &[u8], kind: SecretKind) -> bool {
    if bytes.is_empty() || bytes.len() > MAX_SECRET_BYTES || bytes.contains(&0) {
        return false;
    }
    let normalized = strip_one_line_ending(bytes);
    match kind {
        SecretKind::HexKey => {
            normalized.len() == 64
                && normalized.iter().all(u8::is_ascii_hexdigit)
                && !normalized.iter().any(u8::is_ascii_uppercase)
        }
        SecretKind::DatabaseUrl => std::str::from_utf8(normalized)
            .ok()
            .and_then(|value| ExternalDatabaseCredential::new(value.to_owned()).ok())
            .is_some(),
    }
}

fn strip_one_line_ending(bytes: &[u8]) -> &[u8] {
    if let Some(without_lf) = bytes.strip_suffix(b"\n") {
        without_lf.strip_suffix(b"\r").unwrap_or(without_lf)
    } else {
        bytes
    }
}

fn read_regular_bounded(
    path: &Path,
    max: usize,
    mode: u32,
    require_managed_ownership: bool,
) -> Result<Option<Vec<u8>>, ConfigStoreError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ConfigStoreError::Io),
    };
    if metadata.file_type().is_symlink() {
        return Err(ConfigStoreError::SymlinkOrRedirected);
    }
    if !metadata.is_file() {
        return Err(ConfigStoreError::WrongType);
    }
    verify_metadata(&metadata, mode, require_managed_ownership, true)?;
    if metadata.len() > max as u64 {
        return Err(ConfigStoreError::Configuration(
            ConfigValidationError::Oversized { limit: max },
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    set_nofollow(&mut options);
    let file = options.open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::InvalidInput
            || error.kind() == std::io::ErrorKind::PermissionDenied
        {
            ConfigStoreError::SymlinkOrRedirected
        } else {
            ConfigStoreError::Io
        }
    })?;
    let opened = file.metadata().map_err(|_| ConfigStoreError::Io)?;
    if !opened.is_file() {
        return Err(ConfigStoreError::WrongType);
    }
    verify_metadata(&opened, mode, require_managed_ownership, true)?;
    if opened.len() > max as u64 {
        return Err(ConfigStoreError::Configuration(
            ConfigValidationError::Oversized { limit: max },
        ));
    }
    let mut bytes = zeroize::Zeroizing::new(Vec::with_capacity(opened.len() as usize));
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ConfigStoreError::Io)?;
    let after = file_metadata_by_path_safe(path)?;
    if bytes.len() > max
        || bytes.len() as u64 != opened.len()
        || after.len() != opened.len()
        || !same_file_identity(&opened, &after)
    {
        return Err(ConfigStoreError::Io);
    }
    Ok(Some(std::mem::take(&mut *bytes)))
}

#[cfg(unix)]
fn same_file_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_file_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.len() == right.len()
}

fn file_metadata_by_path_safe(path: &Path) -> Result<fs::Metadata, ConfigStoreError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ConfigStoreError::Io)?;
    if metadata.file_type().is_symlink() {
        return Err(ConfigStoreError::SymlinkOrRedirected);
    }
    if !metadata.is_file() {
        return Err(ConfigStoreError::WrongType);
    }
    Ok(metadata)
}

fn verify_regular_file(
    path: &Path,
    mode: u32,
    require_owner: bool,
    reject_hardlinks: bool,
) -> Result<(), ConfigStoreError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ConfigStoreError::Io)?;
    if metadata.file_type().is_symlink() {
        return Err(ConfigStoreError::SymlinkOrRedirected);
    }
    if !metadata.is_file() {
        return Err(ConfigStoreError::WrongType);
    }
    verify_metadata(&metadata, mode, require_owner, reject_hardlinks)
}

fn verify_metadata(
    metadata: &fs::Metadata,
    mode: u32,
    require_owner: bool,
    reject_hardlinks: bool,
) -> Result<(), ConfigStoreError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o777 != mode || (reject_hardlinks && metadata.nlink() != 1) {
            return Err(ConfigStoreError::PermissionMismatch);
        }
        if require_owner {
            let expected = match mode {
                CONFIG_DIRECTORY_MODE | CONFIG_FILE_MODE => expected_synveil_gid(),
                _ => Some(0),
            }
            .ok_or(ConfigStoreError::PermissionMismatch)?;
            let expected_uid = 0;
            if metadata.uid() != expected_uid || metadata.gid() != expected {
                return Err(ConfigStoreError::PermissionMismatch);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (metadata, mode, require_owner, reject_hardlinks);
    }
    Ok(())
}

fn check_directory_path(
    path: &Path,
    mode: u32,
    require_owner: bool,
    allow_absent: bool,
) -> Result<bool, ConfigStoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(ConfigStoreError::SymlinkOrRedirected);
            }
            if !metadata.is_dir() {
                return Err(ConfigStoreError::WrongType);
            }
            verify_metadata(&metadata, mode, require_owner, false)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && allow_absent => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(ConfigStoreError::Io),
        Err(_) => Err(ConfigStoreError::Io),
    }
}

fn create_directory_chain_safely(
    path: &Path,
    mode: u32,
    require_owner: bool,
) -> Result<(), ConfigStoreError> {
    validate_absolute_directory_path(path)?;
    let mut current = PathBuf::from("/");
    let parts = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(part) => Some(part.to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    for (index, part) in parts.iter().enumerate() {
        current.push(part);
        let is_target = index + 1 == parts.len();
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(ConfigStoreError::SymlinkOrRedirected);
                }
                if !metadata.is_dir() {
                    return Err(ConfigStoreError::WrongType);
                }
                if is_target {
                    verify_metadata(&metadata, mode, require_owner, false)?;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|_| ConfigStoreError::Io)?;
                if is_target {
                    let directory = open_new_directory(&current)?;
                    set_open_file_permissions(&directory, mode)?;
                    set_expected_owner(&directory, SecretOwner::RootSynveil, require_owner)?;
                    verify_metadata(
                        &directory.metadata().map_err(|_| ConfigStoreError::Io)?,
                        mode,
                        require_owner,
                        false,
                    )?;
                }
                let parent = current.parent().ok_or(ConfigStoreError::InvalidPath)?;
                sync_directory(parent).map_err(|_| ConfigStoreError::OutcomeUnknown)?;
            }
            Err(_) => return Err(ConfigStoreError::Io),
        }
    }
    Ok(())
}

fn create_directory_safely(
    path: &Path,
    mode: u32,
    owner: SecretOwner,
    require_owner: bool,
) -> Result<(), ConfigStoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(ConfigStoreError::SymlinkOrRedirected);
            }
            if !metadata.is_dir() {
                return Err(ConfigStoreError::WrongType);
            }
            verify_metadata(&metadata, mode, require_owner, false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|_| ConfigStoreError::Io)?;
            let directory = open_new_directory(path)?;
            set_open_file_permissions(&directory, mode)?;
            set_expected_owner(&directory, owner, require_owner)?;
            verify_metadata(
                &directory.metadata().map_err(|_| ConfigStoreError::Io)?,
                mode,
                require_owner,
                false,
            )
        }
        Err(_) => Err(ConfigStoreError::Io),
    }
}

fn validate_absolute_directory_path(path: &Path) -> Result<(), ConfigStoreError> {
    if !path.is_absolute() || path == Path::new("/") || path.as_os_str().is_empty() {
        return Err(ConfigStoreError::InvalidPath);
    }
    if path
        .components()
        .any(|component| !matches!(component, Component::RootDir | Component::Normal(_)))
    {
        return Err(ConfigStoreError::InvalidPath);
    }
    Ok(())
}

fn check_ancestors(path: &Path) -> Result<bool, ConfigStoreError> {
    let mut current = PathBuf::from("/");
    let mut parts = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(part) => Some(part.to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    parts.pop();
    for part in parts {
        current.push(part);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(ConfigStoreError::Io),
        };
        if metadata.file_type().is_symlink() {
            return Err(ConfigStoreError::SymlinkOrRedirected);
        }
        if !metadata.is_dir() {
            return Err(ConfigStoreError::WrongType);
        }
    }
    Ok(true)
}

fn set_create_mode(options: &mut OpenOptions, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(mode)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(not(unix))]
    {
        let _ = (options, mode);
    }
}

fn set_nofollow(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(not(unix))]
    let _ = options;
}

fn open_new_directory(path: &Path) -> Result<File, ConfigStoreError> {
    let before = fs::symlink_metadata(path).map_err(|_| ConfigStoreError::Io)?;
    if before.file_type().is_symlink() {
        return Err(ConfigStoreError::SymlinkOrRedirected);
    }
    if !before.is_dir() {
        return Err(ConfigStoreError::WrongType);
    }
    let directory = open_directory_no_follow(path)?;
    let opened = directory.metadata().map_err(|_| ConfigStoreError::Io)?;
    if !same_file_identity(&before, &opened) {
        return Err(ConfigStoreError::SymlinkOrRedirected);
    }
    Ok(directory)
}

fn set_open_file_permissions(file: &File, mode: u32) -> Result<(), ConfigStoreError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(mode))
            .map_err(|_| ConfigStoreError::Io)
    }
    #[cfg(not(unix))]
    {
        let _ = (file, mode);
        Ok(())
    }
}

fn sync_directory(path: &Path) -> Result<(), ConfigStoreError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| ConfigStoreError::Io)
}

fn open_directory_no_follow(path: &Path) -> Result<File, ConfigStoreError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = options
        .open(path)
        .map_err(|_| ConfigStoreError::SymlinkOrRedirected)?;
    if !file.metadata().map_err(|_| ConfigStoreError::Io)?.is_dir() {
        return Err(ConfigStoreError::WrongType);
    }
    Ok(file)
}

#[derive(Clone, Copy)]
enum SecretOwner {
    RootRoot,
    RootSynveil,
}

fn set_expected_owner(
    file: &File,
    owner: SecretOwner,
    enforce: bool,
) -> Result<(), ConfigStoreError> {
    if !enforce {
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        use nix::unistd::{Gid, Uid, fchown};
        use std::os::fd::AsRawFd;
        let gid = match owner {
            SecretOwner::RootRoot => 0,
            SecretOwner::RootSynveil => {
                expected_synveil_gid().ok_or(ConfigStoreError::PermissionMismatch)?
            }
        };
        fchown(
            file.as_raw_fd(),
            Some(Uid::from_raw(0)),
            Some(Gid::from_raw(gid)),
        )
        .map_err(|_| ConfigStoreError::PermissionMismatch)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (file, owner);
        Err(ConfigStoreError::PermissionMismatch)
    }
}

#[cfg(target_os = "linux")]
fn expected_synveil_gid() -> Option<u32> {
    nix::unistd::Group::from_name("synveil")
        .ok()
        .flatten()
        .map(|group| group.gid.as_raw())
}

#[cfg(all(not(target_os = "linux"), unix))]
fn expected_synveil_gid() -> Option<u32> {
    None
}

#[cfg(all(test, unix))]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt};
    use synveil_object_store::{
        CapabilityEvidence, CapabilitySupport, StorageAvailability, StorageBackendKind,
        StorageCapabilities, StorageCapability,
    };

    use super::{
        CONFIG_DIRECTORY_MODE, CONFIG_FILE_MODE, CREDENTIAL_DIRECTORY_MODE, ConfigInspection,
        ConfigStoreError, ExistingServerEvidence, InitializeResult, LinuxConfigLayout,
        NewServerConfig, SECRET_FILE_MODE, ServerConfigStore, WriteFailurePoint,
    };
    use crate::{
        ConfigFingerprint, CredentialId, DatabaseCredentialState, DeploymentProfile,
        ExternalDatabaseCredential, MAX_EXTERNAL_DATABASE_URL_BYTES, StorageConfiguration,
        StorageId, StorageRootIdentity,
    };

    fn fixture_store() -> (tempfile::TempDir, ServerConfigStore) {
        let temp = tempfile::tempdir().unwrap();
        let layout =
            LinuxConfigLayout::at_root(canonical_fixture_root(&temp).join("etc/synveil")).unwrap();
        (temp, ServerConfigStore::new(layout))
    }

    fn canonical_fixture_root(temp: &tempfile::TempDir) -> std::path::PathBuf {
        fs::canonicalize(temp.path()).expect("canonicalize temporary fixture root")
    }

    fn initialize_managed(store: &ServerConfigStore) -> crate::ServerConfig {
        match store
            .initialize_managed(
                NewServerConfig {
                    deployment_profile: DeploymentProfile::PersonalHomeManaged,
                },
                ExistingServerEvidence::NoKnownServerState,
            )
            .unwrap()
        {
            InitializeResult::Created(config) | InitializeResult::Reused(config) => config,
        }
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
    fn creates_layout_modes_config_and_stable_identity() {
        let (_temp, store) = fixture_store();
        let first = initialize_managed(&store);
        assert!(matches!(
            store.inspect(),
            ConfigInspection::ValidCurrent { .. }
        ));
        let second = initialize_managed(&store);
        assert_eq!(first.server_installation_id, second.server_installation_id);
        for (path, mode) in [
            (store.layout().root(), CONFIG_DIRECTORY_MODE),
            (store.layout().config_path().as_path(), CONFIG_FILE_MODE),
            (
                store.layout().credential_directory().as_path(),
                CREDENTIAL_DIRECTORY_MODE,
            ),
            (
                store
                    .layout()
                    .credential_directory()
                    .join("rebaseline-token-key")
                    .as_path(),
                SECRET_FILE_MODE,
            ),
            (
                store
                    .layout()
                    .credential_directory()
                    .join("database-password")
                    .as_path(),
                SECRET_FILE_MODE,
            ),
        ] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                mode
            );
        }
    }

    #[test]
    fn managed_override_does_not_create_unowned_missing_parent_directories() {
        let temp = tempfile::tempdir().unwrap();
        let root = canonical_fixture_root(&temp).join("missing-parent/etc/synveil");
        let layout = LinuxConfigLayout::at_managed_root(&root).unwrap();
        let store = ServerConfigStore::new(layout);
        let result = store.initialize_managed(
            NewServerConfig {
                deployment_profile: DeploymentProfile::PersonalHomeManaged,
            },
            ExistingServerEvidence::NoKnownServerState,
        );
        assert!(matches!(result, Err(ConfigStoreError::InvalidPath)));
        assert!(!temp.path().join("missing-parent").exists());
    }

    #[test]
    fn generated_secrets_are_high_entropy_canonical_and_preserved() {
        let (_temp1, store1) = fixture_store();
        let config1 = initialize_managed(&store1);
        let key_path1 = store1
            .layout()
            .credential_directory()
            .join("rebaseline-token-key");
        let password_path1 = store1
            .layout()
            .credential_directory()
            .join("database-password");
        let key1 = fs::read(&key_path1).unwrap();
        let password1 = fs::read(&password_path1).unwrap();
        for value in [&key1, &password1] {
            assert_eq!(value.len(), 64);
            assert!(
                value
                    .iter()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
            );
        }
        initialize_managed(&store1);
        assert_eq!(fs::read(&key_path1).unwrap(), key1);
        assert_eq!(fs::read(&password_path1).unwrap(), password1);

        let (_temp2, store2) = fixture_store();
        let config2 = initialize_managed(&store2);
        let key2 = fs::read(
            store2
                .layout()
                .credential_directory()
                .join("rebaseline-token-key"),
        )
        .unwrap();
        assert_ne!(
            config1.server_installation_id,
            config2.server_installation_id
        );
        assert_ne!(key1, key2);
    }

    #[test]
    fn unknown_adjacent_files_are_left_untouched() {
        let (_temp, store) = fixture_store();
        fs::create_dir_all(store.layout().root().parent().unwrap()).unwrap();
        fs::create_dir(store.layout().root()).unwrap();
        fs::set_permissions(
            store.layout().root(),
            fs::Permissions::from_mode(CONFIG_DIRECTORY_MODE),
        )
        .unwrap();
        let marker = store.layout().root().join("operator-note.txt");
        fs::write(&marker, b"leave me").unwrap();
        initialize_managed(&store);
        assert_eq!(fs::read(marker).unwrap(), b"leave me");
    }

    #[test]
    fn unknown_future_configuration_is_not_rewritten() {
        let (_temp, store) = fixture_store();
        initialize_managed(&store);
        let path = store.layout().config_path();
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["schema_version"] = serde_json::json!(99);
        let future = serde_json::to_vec_pretty(&value).unwrap();
        fs::write(&path, &future).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(CONFIG_FILE_MODE)).unwrap();
        assert_eq!(store.inspect(), ConfigInspection::UnsupportedSchema);
        assert!(matches!(
            store.initialize_managed(
                NewServerConfig {
                    deployment_profile: DeploymentProfile::PersonalHomeManaged,
                },
                ExistingServerEvidence::NoKnownServerState,
            ),
            Err(ConfigStoreError::NeedsRepair(
                ConfigInspection::UnsupportedSchema
            ))
        ));
        assert_eq!(fs::read(path).unwrap(), future);
    }

    #[test]
    fn directory_flock_serializes_concurrent_compare_and_swap_updates() {
        let (_temp, store) = fixture_store();
        initialize_managed(&store);
        let fingerprint = match store.inspect() {
            ConfigInspection::ValidCurrent { fingerprint, .. } => fingerprint,
            other => panic!("unexpected inspection: {other:?}"),
        };
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let mut workers = Vec::new();
        for (root, storage_id) in [
            ("/srv/synveil/one", StorageId::new_v7()),
            ("/srv/synveil/two", StorageId::new_v7()),
        ] {
            let store = store.clone();
            let barrier = std::sync::Arc::clone(&barrier);
            let root = root.to_owned();
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                store.begin_storage_preparation(
                    fingerprint,
                    root,
                    storage_id,
                    StorageRootIdentity::MissingLeaf {
                        parent_device: 1,
                        parent_inode: 1,
                    },
                )
            }));
        }
        barrier.wait();
        let outcomes = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert!(
            outcomes.iter().filter(|result| result.is_ok()).count() <= 1,
            "both compare-and-swap updates reported success: {outcomes:?}"
        );
        assert!(
            outcomes.iter().any(|result| {
                result.is_ok() || matches!(result, Err(ConfigStoreError::OutcomeUnknown))
            }),
            "no update was committed or reported outcome-unknown: {outcomes:?}"
        );
        let persisted = match store.inspect() {
            ConfigInspection::ValidCurrent { config, .. } => config,
            other => panic!("concurrent config update left unsafe state: {other:?}"),
        };
        assert_eq!(persisted.generation, 1);
        assert!(matches!(
            persisted.storage,
            StorageConfiguration::PreparingLocal { root, .. }
                if root == "/srv/synveil/one" || root == "/srv/synveil/two"
        ));
    }

    #[test]
    fn configured_transition_outcome_unknown_reconciles_the_same_storage_identity() {
        for point in [
            WriteFailurePoint::BeforeTemporaryCreate,
            WriteFailurePoint::AfterTemporaryWrite,
            WriteFailurePoint::AfterFileSync,
            WriteFailurePoint::BeforeRename,
            WriteFailurePoint::AfterRename,
            WriteFailurePoint::BeforeParentSync,
            WriteFailurePoint::AfterParentSync,
            WriteFailurePoint::PostWriteVerification,
        ] {
            let (_temp, store) = fixture_store();
            initialize_managed(&store);
            let fingerprint = match store.inspect() {
                ConfigInspection::ValidCurrent { fingerprint, .. } => fingerprint,
                other => panic!("unexpected inspection: {other:?}"),
            };
            let root = "/srv/synveil/reconcile-final-commit".to_owned();
            let storage_id = StorageId::new_v7();
            store
                .begin_storage_preparation(
                    fingerprint,
                    root.clone(),
                    storage_id.clone(),
                    StorageRootIdentity::MissingLeaf {
                        parent_device: 10,
                        parent_inode: 20,
                    },
                )
                .unwrap();
            let preparing_fingerprint = match store.inspect() {
                ConfigInspection::ValidCurrent { fingerprint, .. } => fingerprint,
                other => panic!("unexpected inspection: {other:?}"),
            };
            let root_identity = StorageRootIdentity::Directory {
                device: 10,
                inode: 30,
            };
            store
                .record_storage_directory_identity(
                    preparing_fingerprint,
                    root.clone(),
                    storage_id.clone(),
                    root_identity.clone(),
                )
                .unwrap();
            let configured_fingerprint = match store.inspect() {
                ConfigInspection::ValidCurrent { fingerprint, .. } => fingerprint,
                other => panic!("unexpected inspection: {other:?}"),
            };
            let failing = ServerConfigStore {
                layout: store.layout().clone(),
                failure_point: Some(point),
            };
            assert!(
                failing
                    .complete_storage_preparation(
                        configured_fingerprint,
                        root.clone(),
                        storage_id.clone(),
                        root_identity.clone(),
                        ready_storage_capabilities(),
                    )
                    .is_err()
            );

            let reconciler = ServerConfigStore::new(store.layout().clone());
            let (after, after_fingerprint) = match reconciler.inspect() {
                ConfigInspection::ValidCurrent {
                    config,
                    fingerprint,
                } => (*config, fingerprint),
                other => panic!("fault left config unreconcilable at {point:?}: {other:?}"),
            };
            let commit_may_have_landed = matches!(
                point,
                WriteFailurePoint::AfterRename
                    | WriteFailurePoint::BeforeParentSync
                    | WriteFailurePoint::AfterParentSync
                    | WriteFailurePoint::PostWriteVerification
            );
            if commit_may_have_landed {
                assert!(matches!(
                    &after.storage,
                    StorageConfiguration::ConfiguredLocal {
                        root: actual_root,
                        storage_id: actual_id,
                        root_identity: actual_identity,
                        ..
                    } if actual_root == &root
                        && actual_id == &storage_id
                        && actual_identity == &root_identity
                ));
            } else {
                assert!(matches!(
                    &after.storage,
                    StorageConfiguration::PreparingLocal {
                        root: actual_root,
                        storage_id: actual_id,
                        root_identity: actual_identity,
                    } if actual_root == &root
                        && actual_id == &storage_id
                        && actual_identity == &root_identity
                ));
                reconciler
                    .complete_storage_preparation(
                        after_fingerprint,
                        root.clone(),
                        storage_id.clone(),
                        root_identity.clone(),
                        ready_storage_capabilities(),
                    )
                    .unwrap();
            }
            let final_config = match reconciler.inspect() {
                ConfigInspection::ValidCurrent { config, .. } => config,
                other => panic!("reconciliation failed at {point:?}: {other:?}"),
            };
            assert!(matches!(
                &final_config.storage,
                StorageConfiguration::ConfiguredLocal {
                    root: actual_root,
                    storage_id: actual_id,
                    root_identity: actual_identity,
                    ..
                } if actual_root == &root
                    && actual_id == &storage_id
                    && actual_identity == &root_identity
            ));
        }
    }

    #[test]
    fn external_config_uses_same_schema_and_protected_database_url() {
        let (_temp, store) = fixture_store();
        let credential = ExternalDatabaseCredential::new(
            "postgresql://operator:CANARY-credential@db.example.invalid/synveil".to_owned(),
        )
        .unwrap();
        let config = match store
            .initialize_external(
                NewServerConfig {
                    deployment_profile: DeploymentProfile::AdvancedExternal,
                },
                ExistingServerEvidence::NoKnownServerState,
                credential,
            )
            .unwrap()
        {
            InitializeResult::Created(config) => config,
            InitializeResult::Reused(_) => panic!("fresh fixture must create"),
        };
        assert_eq!(config.database.postgres_major, 17);
        assert!(matches!(
            store.inspect(),
            ConfigInspection::ValidCurrent { .. }
        ));
        let diagnostics = format!("{:?} {:?}", store.inspect(), config);
        assert!(!diagnostics.contains("CANARY-credential"));
        assert!(
            !String::from_utf8(fs::read(store.layout().config_path()).unwrap())
                .unwrap()
                .contains("CANARY-credential")
        );
        assert_eq!(
            config.database.endpoint.as_ref().unwrap().host,
            "db.example.invalid"
        );
        assert_eq!(config.database.endpoint.as_ref().unwrap().port, 5432);
    }

    #[test]
    fn managed_endpoint_materialization_uses_generated_secret_and_never_serializes_it() {
        let (_temp, store) = fixture_store();
        initialize_managed(&store);
        let fingerprint = match store.inspect() {
            ConfigInspection::ValidCurrent { fingerprint, .. } => fingerprint,
            other => panic!("unexpected inspection: {other:?}"),
        };
        let updated = store
            .materialize_managed_database_url(
                fingerprint,
                crate::DatabaseEndpoint {
                    host: "127.0.0.1".to_owned(),
                    port: 55432,
                },
            )
            .unwrap();
        assert_eq!(
            updated.database.credential_state,
            DatabaseCredentialState::Materialized
        );
        assert!(matches!(
            store.inspect(),
            ConfigInspection::ValidCurrent { .. }
        ));
        let config_text = fs::read_to_string(store.layout().config_path()).unwrap();
        let secret_text =
            fs::read_to_string(store.layout().credential_directory().join("database-url")).unwrap();
        let generated_password = store.managed_database_password().unwrap();
        generated_password.with_secret(|password| {
            assert!(secret_text.contains(password));
        });
        assert!(!config_text.contains(&secret_text));
        assert_eq!(
            fs::metadata(store.layout().credential_directory().join("database-url"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            SECRET_FILE_MODE
        );
    }

    #[test]
    fn update_is_atomic_preserves_identity_and_uses_compare_and_swap() {
        let (_temp, store) = fixture_store();
        let config = initialize_managed(&store);
        let fingerprint = match store.inspect() {
            ConfigInspection::ValidCurrent { fingerprint, .. } => fingerprint,
            other => panic!("unexpected inspection: {other:?}"),
        };
        let storage_id = StorageId::new_v7();
        let parent_identity = StorageRootIdentity::MissingLeaf {
            parent_device: 1,
            parent_inode: 2,
        };
        let directory_identity = StorageRootIdentity::Directory {
            device: 1,
            inode: 3,
        };
        let preparing = store
            .begin_storage_preparation(
                fingerprint,
                "/srv/synveil/server-data".to_owned(),
                storage_id.clone(),
                parent_identity.clone(),
            )
            .unwrap();
        assert_eq!(
            preparing.server_installation_id,
            config.server_installation_id
        );
        assert_eq!(preparing.generation, 1);
        assert_eq!(
            preparing.storage,
            StorageConfiguration::PreparingLocal {
                root: "/srv/synveil/server-data".to_owned(),
                storage_id: storage_id.clone(),
                root_identity: parent_identity.clone(),
            }
        );
        assert_eq!(preparing.database, config.database);
        assert_eq!(
            preparing.rebaseline_token_key_ref,
            config.rebaseline_token_key_ref
        );
        assert_eq!(preparing.network, config.network);
        let prepared_fingerprint = match store.inspect() {
            ConfigInspection::ValidCurrent { fingerprint, .. } => fingerprint,
            other => panic!("unexpected inspection: {other:?}"),
        };
        let retried = store
            .begin_storage_preparation(
                prepared_fingerprint,
                "/srv/synveil/server-data".to_owned(),
                storage_id.clone(),
                parent_identity.clone(),
            )
            .unwrap();
        assert_eq!(retried.generation, preparing.generation);
        assert_eq!(retried.storage, preparing.storage);
        let directory_bound = store
            .record_storage_directory_identity(
                prepared_fingerprint,
                "/srv/synveil/server-data".to_owned(),
                storage_id.clone(),
                directory_identity.clone(),
            )
            .unwrap();
        let directory_bound_fingerprint = match store.inspect() {
            ConfigInspection::ValidCurrent { fingerprint, .. } => fingerprint,
            other => panic!("unexpected inspection: {other:?}"),
        };
        assert_eq!(directory_bound.generation, 2);
        let configured = store
            .complete_storage_preparation(
                directory_bound_fingerprint,
                "/srv/synveil/server-data".to_owned(),
                storage_id.clone(),
                directory_identity.clone(),
                ready_storage_capabilities(),
            )
            .unwrap();
        assert_eq!(
            configured.server_installation_id,
            config.server_installation_id
        );
        assert_eq!(configured.generation, 3);
        assert_eq!(configured.database, config.database);
        assert_eq!(
            configured.rebaseline_token_key_ref,
            config.rebaseline_token_key_ref
        );
        assert_eq!(configured.network, config.network);
        assert_eq!(
            configured.storage,
            StorageConfiguration::ConfiguredLocal {
                root: "/srv/synveil/server-data".to_owned(),
                storage_id: storage_id.clone(),
                root_identity: directory_identity,
                capabilities: ready_storage_capabilities(),
            }
        );
        assert!(matches!(
            store.begin_storage_preparation(
                fingerprint,
                "/srv/other".to_owned(),
                storage_id,
                parent_identity,
            ),
            Err(ConfigStoreError::ConcurrentModification)
        ));
    }

    #[test]
    fn missing_or_malformed_expected_secret_is_never_replaced() {
        let (_temp, store) = fixture_store();
        initialize_managed(&store);
        let key = store
            .layout()
            .credential_directory()
            .join("rebaseline-token-key");
        let original = fs::read(&key).unwrap();
        fs::remove_file(&key).unwrap();
        assert!(matches!(
            store.inspect(),
            ConfigInspection::SecretMissing {
                id: CredentialId::RebaselineTokenKey
            }
        ));
        assert!(matches!(
            store.initialize_managed(
                NewServerConfig {
                    deployment_profile: DeploymentProfile::PersonalHomeManaged
                },
                ExistingServerEvidence::NoKnownServerState
            ),
            Err(ConfigStoreError::NeedsRepair(
                ConfigInspection::SecretMissing { .. }
            ))
        ));
        assert!(!key.exists());
        fs::write(&key, b"malformed").unwrap();
        fs::set_permissions(&key, fs::Permissions::from_mode(SECRET_FILE_MODE)).unwrap();
        assert!(matches!(
            store.inspect(),
            ConfigInspection::SecretInvalid {
                id: CredentialId::RebaselineTokenKey
            }
        ));
        assert_ne!(fs::read(&key).unwrap(), original);
    }

    #[test]
    fn absent_config_with_partial_secret_or_existing_state_requires_repair() {
        let (_temp, store) = fixture_store();
        store.ensure_layout().unwrap();
        fs::write(
            store
                .layout()
                .credential_directory()
                .join("rebaseline-token-key"),
            "x".repeat(64),
        )
        .unwrap();
        fs::set_permissions(
            store
                .layout()
                .credential_directory()
                .join("rebaseline-token-key"),
            fs::Permissions::from_mode(SECRET_FILE_MODE),
        )
        .unwrap();
        assert!(matches!(
            store.inspect(),
            ConfigInspection::PartialConfiguration
        ));
        assert!(
            store
                .initialize_managed(
                    NewServerConfig {
                        deployment_profile: DeploymentProfile::PersonalHomeManaged
                    },
                    ExistingServerEvidence::NoKnownServerState,
                )
                .is_err()
        );
        let (_temp2, store2) = fixture_store();
        assert!(matches!(
            store2.initialize_managed(
                NewServerConfig {
                    deployment_profile: DeploymentProfile::PersonalHomeManaged
                },
                ExistingServerEvidence::ExistingOrAmbiguousState,
            ),
            Err(ConfigStoreError::NeedsRepair(
                ConfigInspection::IdentityConflict
            ))
        ));
    }

    #[test]
    fn symlink_directory_config_secret_and_hardlink_are_rejected() {
        let (_temp, store) = fixture_store();
        fs::create_dir_all(store.layout().root().parent().unwrap()).unwrap();
        let outside = store.layout().root().with_extension("outside");
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, store.layout().root()).unwrap();
        assert_eq!(store.inspect(), ConfigInspection::SymlinkOrRedirected);
        assert!(store.ensure_layout().is_err());

        let (_temp2, store2) = fixture_store();
        initialize_managed(&store2);
        let config_path = store2.layout().config_path();
        let saved = store2.layout().root().join("saved-config");
        fs::rename(&config_path, &saved).unwrap();
        std::os::unix::fs::symlink(&saved, &config_path).unwrap();
        assert_eq!(store2.inspect(), ConfigInspection::SymlinkOrRedirected);

        let (_temp3, store3) = fixture_store();
        initialize_managed(&store3);
        let key = store3
            .layout()
            .credential_directory()
            .join("rebaseline-token-key");
        let moved = store3.layout().root().join("outside-secret");
        fs::rename(&key, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &key).unwrap();
        assert_eq!(store3.inspect(), ConfigInspection::SymlinkOrRedirected);

        let (_temp4, store4) = fixture_store();
        initialize_managed(&store4);
        let db_password = store4
            .layout()
            .credential_directory()
            .join("database-password");
        let alias = store4.layout().root().join("secret-alias");
        fs::hard_link(&db_password, &alias).unwrap();
        assert!(matches!(
            store4.inspect(),
            ConfigInspection::PermissionMismatch
        ));
    }

    #[test]
    fn wrong_type_and_permission_mismatch_are_classified() {
        let (_temp, store) = fixture_store();
        fs::create_dir_all(store.layout().root().parent().unwrap()).unwrap();
        fs::write(store.layout().root(), b"not a directory").unwrap();
        assert_eq!(store.inspect(), ConfigInspection::WrongType);

        let (_temp2, store2) = fixture_store();
        initialize_managed(&store2);
        fs::set_permissions(
            store2.layout().config_path(),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert_eq!(store2.inspect(), ConfigInspection::PermissionMismatch);
    }

    #[test]
    fn fault_injection_leaves_reconcilable_state_and_never_replaces_secret() {
        for point in [
            WriteFailurePoint::BeforeRootDirectoryCreate,
            WriteFailurePoint::AfterRootDirectoryCreate,
            WriteFailurePoint::BeforeCredentialDirectoryCreate,
            WriteFailurePoint::AfterCredentialDirectoryCreate,
        ] {
            let temp = tempfile::tempdir().unwrap();
            let layout =
                LinuxConfigLayout::at_root(canonical_fixture_root(&temp).join("etc/synveil"))
                    .unwrap();
            let failing = ServerConfigStore {
                layout: layout.clone(),
                failure_point: Some(point),
            };
            assert!(
                failing
                    .initialize_managed(
                        NewServerConfig {
                            deployment_profile: DeploymentProfile::PersonalHomeManaged
                        },
                        ExistingServerEvidence::NoKnownServerState,
                    )
                    .is_err()
            );
            let retry = ServerConfigStore::new(layout);
            initialize_managed(&retry);
            assert!(matches!(
                retry.inspect(),
                ConfigInspection::ValidCurrent { .. }
            ));
        }

        let temp = tempfile::tempdir().unwrap();
        let layout =
            LinuxConfigLayout::at_root(canonical_fixture_root(&temp).join("etc/synveil")).unwrap();
        let store = ServerConfigStore {
            layout,
            failure_point: Some(WriteFailurePoint::AfterSecretFileSync),
        };
        assert!(
            store
                .initialize_managed(
                    NewServerConfig {
                        deployment_profile: DeploymentProfile::PersonalHomeManaged
                    },
                    ExistingServerEvidence::NoKnownServerState,
                )
                .is_err()
        );
        let key_path = store
            .layout()
            .credential_directory()
            .join("rebaseline-token-key");
        let durable_key = fs::read(&key_path).unwrap();
        assert_eq!(durable_key.len(), 64);
        let retry = ServerConfigStore::new(store.layout.clone());
        assert!(matches!(
            retry.inspect(),
            ConfigInspection::PartialConfiguration
        ));
        assert!(
            retry
                .initialize_managed(
                    NewServerConfig {
                        deployment_profile: DeploymentProfile::PersonalHomeManaged
                    },
                    ExistingServerEvidence::NoKnownServerState,
                )
                .is_err()
        );
        assert_eq!(fs::read(key_path).unwrap(), durable_key);

        for point in [
            WriteFailurePoint::BeforeTemporaryCreate,
            WriteFailurePoint::AfterTemporaryWrite,
            WriteFailurePoint::AfterFileSync,
            WriteFailurePoint::BeforeRename,
            WriteFailurePoint::AfterRename,
            WriteFailurePoint::BeforeParentSync,
            WriteFailurePoint::AfterParentSync,
            WriteFailurePoint::PostWriteVerification,
        ] {
            let temp = tempfile::tempdir().unwrap();
            let layout =
                LinuxConfigLayout::at_root(canonical_fixture_root(&temp).join("etc/synveil"))
                    .unwrap();
            let store = ServerConfigStore {
                layout,
                failure_point: Some(WriteFailurePoint::BeforeTemporaryCreate),
            };
            store.ensure_layout().unwrap();
            let config = ServerConfigStore::new(store.layout.clone());
            let initialized = initialize_managed(&config);
            let expected = match config.inspect() {
                ConfigInspection::ValidCurrent { fingerprint, .. } => fingerprint,
                _ => unreachable!(),
            };
            let failing = ServerConfigStore {
                layout: store.layout.clone(),
                failure_point: Some(point),
            };
            let result = failing.begin_storage_preparation(
                expected,
                "/srv/synveil/test".to_owned(),
                StorageId::new_v7(),
                StorageRootIdentity::MissingLeaf {
                    parent_device: 1,
                    parent_inode: 1,
                },
            );
            assert!(result.is_err(), "fault point {point:?}");
            let after = ServerConfigStore::new(store.layout.clone()).inspect();
            match point {
                WriteFailurePoint::AfterRename
                | WriteFailurePoint::BeforeParentSync
                | WriteFailurePoint::AfterParentSync
                | WriteFailurePoint::PostWriteVerification => {
                    assert!(
                        matches!(after, ConfigInspection::ValidCurrent { config, .. } if config.generation == 1 && matches!(config.storage, StorageConfiguration::PreparingLocal { .. }))
                    );
                }
                _ => assert!(
                    matches!(after, ConfigInspection::ValidCurrent { config, .. } if config.generation == initialized.generation)
                ),
            }
        }
    }

    #[test]
    fn malformed_config_and_oversized_secret_are_safe_errors() {
        let (_temp, store) = fixture_store();
        initialize_managed(&store);
        fs::write(store.layout().config_path(), b"not json").unwrap();
        fs::set_permissions(
            store.layout().config_path(),
            fs::Permissions::from_mode(CONFIG_FILE_MODE),
        )
        .unwrap();
        assert_eq!(store.inspect(), ConfigInspection::Malformed);

        let (_temp2, store2) = fixture_store();
        initialize_managed(&store2);
        let url = store2
            .layout()
            .credential_directory()
            .join("database-password");
        fs::write(&url, vec![b'x'; MAX_EXTERNAL_DATABASE_URL_BYTES + 1]).unwrap();
        fs::set_permissions(&url, fs::Permissions::from_mode(SECRET_FILE_MODE)).unwrap();
        assert_eq!(
            store2.inspect(),
            ConfigInspection::SecretInvalid {
                id: CredentialId::DatabasePassword
            }
        );
    }

    #[test]
    fn config_fingerprint_is_non_secret_and_revisions_are_explicit() {
        let (_temp, store) = fixture_store();
        let config = initialize_managed(&store);
        let fingerprint = match store.inspect() {
            ConfigInspection::ValidCurrent { fingerprint, .. } => fingerprint,
            _ => unreachable!(),
        };
        assert_eq!(
            ConfigFingerprint::from_canonical_bytes(
                &fs::read(store.layout().config_path()).unwrap()
            ),
            fingerprint
        );
        assert_eq!(config.generation, 0);
        assert_eq!(fingerprint.to_hex().len(), 64);
    }
}
