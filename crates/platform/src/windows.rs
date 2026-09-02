use crate::{
    MinimalPlatformRuntime, PathResolutionError, PathResolver, Platform, PlatformPaths, paths,
};

/// Windows application-data path resolver. It only resolves paths and never
/// creates them or calls the Windows Service API.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsPathResolver;

impl PathResolver for WindowsPathResolver {
    fn resolve_paths(&self) -> Result<PlatformPaths, PathResolutionError> {
        paths::resolve_windows_paths()
    }
}

/// Windows runtime with native Credential Manager storage on Windows and an
/// explicitly unsupported service lifecycle.
pub struct WindowsPlatformRuntime {
    inner: MinimalPlatformRuntime,
}

impl WindowsPlatformRuntime {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: MinimalPlatformRuntime::new(Platform::Windows, Box::new(WindowsPathResolver)),
        }
    }

    #[must_use]
    pub fn inner(&self) -> &MinimalPlatformRuntime {
        &self.inner
    }
}

impl Default for WindowsPlatformRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::PlatformRuntime for WindowsPlatformRuntime {
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
