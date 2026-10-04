#![cfg_attr(not(test), forbid(unsafe_code))]

//! Durable managed server configuration primitives.
//!
//! The schema contains non-secret state only. Secret source files are owned by
//! [`ServerConfigStore`] and use fixed credential identifiers.

mod model;
mod secret;
mod store;

pub use synveil_object_store::{
    CapabilityEvidence, CapabilitySupport, StorageAvailability, StorageBackendKind,
    StorageCapabilities, StorageCapability,
};

pub use model::{
    ConfigFingerprint, ConfigValidationError, DatabaseConfiguration, DatabaseCredentialState,
    DatabaseEndpoint, DatabaseMode, DatabaseOwnership, DependencyRuntimeIdentity,
    DeploymentProfile, EdgeListener, FirewallManager, FirewallState, MAX_SERVER_CONFIG_BYTES,
    ManagedDatabaseDataRoot, NetworkConfiguration, NetworkIntegration, NetworkIntegrationId,
    NetworkTrust, ReachabilityMode, SERVER_CONFIG_SCHEMA_VERSION, ServerConfig,
    StorageConfiguration, StorageId, StorageRootIdentity,
};
pub use secret::{
    CredentialId, ExternalDatabaseCredential, ExternalDatabaseCredentialError,
    MAX_EXTERNAL_DATABASE_URL_BYTES, SecretMaterial, SecretReference,
};
pub use store::{
    ConfigInspection, ConfigStoreError, ExistingServerEvidence, InitializeResult,
    LinuxConfigLayout, NewServerConfig, SERVER_CONFIG_FILE_NAME, ServerConfigStore,
    WriteFailurePoint,
};
