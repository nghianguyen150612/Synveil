use crate::{
    MinimalPlatformRuntime, PathResolutionError, PathResolver, Platform, PlatformPaths, paths,
};

/// macOS application-support path resolver. It only resolves paths and never
/// creates them or calls launchd/Keychain APIs.
#[derive(Clone, Copy, Debug, Default)]
pub struct MacosPathResolver;

/// Compatibility spelling for callers that capitalize the OS name.
pub type MacOSPathResolver = MacosPathResolver;

impl PathResolver for MacosPathResolver {
    fn resolve_paths(&self) -> Result<PlatformPaths, PathResolutionError> {
        paths::resolve_macos_paths()
    }
}

/// Minimal macOS runtime with explicit unsupported lifecycle/secret states.
pub struct MacosPlatformRuntime {
    inner: MinimalPlatformRuntime,
}

/// Compatibility spelling for callers that capitalize the OS name.
pub type MacOSPlatformRuntime = MacosPlatformRuntime;

impl MacosPlatformRuntime {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: MinimalPlatformRuntime::new(Platform::Macos, Box::new(MacosPathResolver)),
        }
    }

    #[must_use]
    pub fn inner(&self) -> &MinimalPlatformRuntime {
        &self.inner
    }
}

impl Default for MacosPlatformRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::PlatformRuntime for MacosPlatformRuntime {
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
