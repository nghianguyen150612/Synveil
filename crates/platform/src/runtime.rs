use crate::{
    ComponentHealth, HealthComponent, HealthInfo, HealthState, HostInfo, PathResolver, Platform,
    ReadOnlyStorageDiscovery, SecretStore, SecretStoreState, ServiceLifecycle,
    StorageCapabilityDiscovery, UnsupportedSecureSecretStore, UnsupportedServiceLifecycle,
};

use std::path::Path;

/// Platform abstraction boundary consumed by application/runtime orchestration.
///
/// Application/runtime orchestration can depend on this contract directionally,
/// while concrete OS adapters remain outside the core crate. No method exposes
/// a service manager API, process exit code, secret value, or filesystem-specific
/// type.
pub trait PlatformRuntime: Send + Sync {
    fn platform(&self) -> Platform;
    fn host_info(&self) -> HostInfo;
    fn path_resolver(&self) -> &dyn PathResolver;
    fn storage_discovery(&self) -> &dyn StorageCapabilityDiscovery;
    fn secret_store(&self) -> &dyn SecretStore;
    fn service_lifecycle(&self) -> &dyn ServiceLifecycle;
    fn health(&self) -> HealthInfo;

    fn resolve_paths(&self) -> Result<crate::PlatformPaths, crate::PathResolutionError> {
        self.path_resolver().resolve_paths()
    }

    fn discover_storage(&self, path: &Path) -> crate::StorageCapabilities {
        self.storage_discovery().discover(path)
    }
}

/// Shared read-only runtime composition used by the minimal adapter namespaces.
pub struct MinimalPlatformRuntime {
    platform: Platform,
    host_info: HostInfo,
    path_resolver: Box<dyn PathResolver>,
    storage_discovery: ReadOnlyStorageDiscovery,
    secret_store: UnsupportedSecureSecretStore,
    service_lifecycle: UnsupportedServiceLifecycle,
}

impl MinimalPlatformRuntime {
    pub(crate) fn new(platform: Platform, path_resolver: Box<dyn PathResolver>) -> Self {
        Self {
            platform,
            host_info: HostInfo::for_platform(platform),
            path_resolver,
            storage_discovery: ReadOnlyStorageDiscovery,
            secret_store: UnsupportedSecureSecretStore::new(),
            service_lifecycle: UnsupportedServiceLifecycle::new(),
        }
    }

    #[must_use]
    pub const fn platform(&self) -> Platform {
        self.platform
    }

    #[must_use]
    pub const fn host_info(&self) -> &HostInfo {
        &self.host_info
    }

    #[must_use]
    pub fn path_resolver(&self) -> &dyn PathResolver {
        self.path_resolver.as_ref()
    }

    #[must_use]
    pub const fn storage_discovery(&self) -> &ReadOnlyStorageDiscovery {
        &self.storage_discovery
    }

    #[must_use]
    pub const fn secret_store(&self) -> &UnsupportedSecureSecretStore {
        &self.secret_store
    }

    #[must_use]
    pub const fn service_lifecycle(&self) -> &UnsupportedServiceLifecycle {
        &self.service_lifecycle
    }

    #[must_use]
    pub fn health(&self) -> HealthInfo {
        let paths_state = if self.path_resolver.resolve_paths().is_ok() {
            HealthState::Healthy
        } else {
            HealthState::Unknown
        };
        let secret_state = match self.secret_store.state() {
            SecretStoreState::Available => HealthState::Healthy,
            SecretStoreState::Unsupported | SecretStoreState::Unavailable => {
                HealthState::Unavailable
            }
        };
        let lifecycle_state = if self.service_lifecycle.status().is_supported() {
            HealthState::Healthy
        } else {
            HealthState::Unavailable
        };
        let runtime_state = if self.platform.is_primary_desktop() {
            HealthState::Healthy
        } else {
            HealthState::Degraded
        };
        let overall = if secret_state == HealthState::Unavailable
            || lifecycle_state == HealthState::Unavailable
        {
            HealthState::Degraded
        } else if paths_state == HealthState::Unknown {
            HealthState::Unknown
        } else {
            runtime_state
        };

        HealthInfo::new(
            overall,
            true,
            false,
            [
                ComponentHealth::new(HealthComponent::Runtime, runtime_state),
                ComponentHealth::new(HealthComponent::Paths, paths_state),
                ComponentHealth::new(HealthComponent::StorageDiscovery, HealthState::Unknown),
                ComponentHealth::new(HealthComponent::SecretStore, secret_state),
                ComponentHealth::new(HealthComponent::ServiceLifecycle, lifecycle_state),
            ],
        )
    }
}

impl PlatformRuntime for MinimalPlatformRuntime {
    fn platform(&self) -> Platform {
        self.platform()
    }

    fn host_info(&self) -> HostInfo {
        self.host_info.clone()
    }

    fn path_resolver(&self) -> &dyn PathResolver {
        self.path_resolver()
    }

    fn storage_discovery(&self) -> &dyn StorageCapabilityDiscovery {
        self.storage_discovery()
    }

    fn secret_store(&self) -> &dyn SecretStore {
        self.secret_store()
    }

    fn service_lifecycle(&self) -> &dyn ServiceLifecycle {
        self.service_lifecycle()
    }

    fn health(&self) -> HealthInfo {
        self.health()
    }
}
