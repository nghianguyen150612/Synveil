#![forbid(unsafe_code)]

//! Portable platform/runtime contracts for Synveil.
//!
//! The contracts in this crate are the boundary between the domain/application
//! core and host behavior. Adapters describe the current host, resolve paths,
//! and expose capability evidence. Linux/Windows also expose native persistent
//! secure-secret storage with no plaintext fallback. No adapter installs or
//! mutates service managers.

mod health;
mod host;
mod lifecycle;
mod native_secrets;
mod paths;
mod runtime;
mod secrets;
mod storage;

pub mod generic;
pub mod linux;
pub mod macos;
pub mod windows;

pub use health::{ComponentHealth, HealthComponent, HealthInfo, HealthState};
pub use host::{HostArchitecture, HostInfo, Platform};
pub use lifecycle::{
    LifecycleAction, LifecycleError, LifecycleReceipt, RestartRequest, ServiceLifecycle,
    ServiceLifecycleStatus, ServiceState, ShutdownRequest, UnsupportedServiceLifecycle,
};
pub use native_secrets::NativeSecureSecretStore;
pub use paths::{
    FixedPathResolver, PathKind, PathResolutionError, PathResolutionViolation, PathResolver,
    PlatformPathResolver, PlatformPaths,
};
pub use runtime::{MinimalPlatformRuntime, PlatformRuntime};
pub use secrets::{
    SecretName, SecretNameError, SecretStore, SecretStoreError, SecretStoreState, SecretValue,
    UnsupportedSecureSecretStore,
};
pub use storage::{
    CapabilityEvidence, CapabilitySupport, ReadOnlyStorageDiscovery, StorageAvailability,
    StorageBackendKind, StorageCapabilities, StorageCapability, StorageCapabilityDiscovery,
    StorageDiscovery,
};

/// Detect the host platform without invoking an operating-system API.
#[must_use]
pub fn detect_platform() -> Platform {
    Platform::detect()
}

/// Construct the adapter selected for the current compilation target.
///
/// This factory only composes an adapter. It does not install or start any
/// service and it never creates a directory or writes a secret. Linux/Windows
/// expose native secure persistence only through subsequent explicit calls.
#[must_use]
pub fn current() -> Box<dyn PlatformRuntime> {
    #[cfg(target_os = "linux")]
    {
        return Box::new(linux::LinuxPlatformRuntime::new());
    }

    #[cfg(target_os = "windows")]
    {
        return Box::new(windows::WindowsPlatformRuntime::new());
    }

    #[cfg(target_os = "macos")]
    {
        return Box::new(macos::MacosPlatformRuntime::new());
    }

    #[allow(unreachable_code)]
    Box::new(generic::GenericPlatformRuntime::new())
}

/// Compatibility spelling for callers that prefer an explicit factory name.
#[must_use]
pub fn current_runtime() -> Box<dyn PlatformRuntime> {
    current()
}
