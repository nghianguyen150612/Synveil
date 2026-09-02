use crate::{
    FixedPathResolver, MinimalPlatformRuntime, PathResolutionError, PathResolver, Platform,
    PlatformPaths, paths,
};

/// Generic path resolver requiring explicit Synveil path environment variables.
#[derive(Clone, Copy, Debug, Default)]
pub struct GenericPathResolver;

impl PathResolver for GenericPathResolver {
    fn resolve_paths(&self) -> Result<PlatformPaths, PathResolutionError> {
        paths::resolve_generic_paths()
    }
}

/// Read-only fallback runtime for an unrecognised host.
pub struct GenericPlatformRuntime {
    inner: MinimalPlatformRuntime,
}

impl GenericPlatformRuntime {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: MinimalPlatformRuntime::new(Platform::Generic, Box::new(GenericPathResolver)),
        }
    }

    #[must_use]
    pub fn inner(&self) -> &MinimalPlatformRuntime {
        &self.inner
    }
}

impl Default for GenericPlatformRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::PlatformRuntime for GenericPlatformRuntime {
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

/// Construct a deterministic generic resolver for adapter tests.
#[must_use]
pub fn fixed_path_resolver(paths: PlatformPaths) -> FixedPathResolver {
    FixedPathResolver::new(paths)
}
