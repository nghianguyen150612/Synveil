use crate::{
    MinimalPlatformRuntime, PathResolutionError, PathResolver, Platform, PlatformPaths, paths,
};

/// Linux/XDG path resolver. It only resolves paths and never creates them.
#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxPathResolver;

impl PathResolver for LinuxPathResolver {
    fn resolve_paths(&self) -> Result<PlatformPaths, PathResolutionError> {
        paths::resolve_linux_paths()
    }
}

/// Linux runtime with persistent native Secret Service storage on Linux.
/// Service lifecycle operations remain explicitly unsupported.
pub struct LinuxPlatformRuntime {
    inner: MinimalPlatformRuntime,
}

impl LinuxPlatformRuntime {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: MinimalPlatformRuntime::new(Platform::Linux, Box::new(LinuxPathResolver)),
        }
    }

    #[must_use]
    pub fn inner(&self) -> &MinimalPlatformRuntime {
        &self.inner
    }
}

impl Default for LinuxPlatformRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::PlatformRuntime for LinuxPlatformRuntime {
    fn platform(&self) -> Platform {
        self.inner.platform()
    }

    fn host_info(&self) -> crate::HostInfo {
        self.inner.host_info().clone()
    }

    fn path_resolver(&self) -> &dyn PathResolver {
        self.inner.path_resolver()
    }

    fn storage_discovery(&self) -> &dyn crate::StorageCapabilityDiscovery {
        self.inner.storage_discovery()
    }

    fn secret_store(&self) -> &dyn crate::SecretStore {
        self.inner.secret_store()
    }

    fn service_lifecycle(&self) -> &dyn crate::ServiceLifecycle {
        self.inner.service_lifecycle()
    }

    fn health(&self) -> crate::HealthInfo {
        self.inner.health()
    }
}
