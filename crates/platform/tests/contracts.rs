use std::path::{Path, PathBuf};

use synveil_core::{CacheDir, ConfigDir, DataDir, RuntimeDir};
use synveil_platform::{
    CapabilitySupport, HealthComponent, HealthState, PathKind, PathResolutionError,
    PathResolutionViolation, Platform, PlatformPaths, PlatformRuntime, ReadOnlyStorageDiscovery,
    SecretName, SecretStore, SecretStoreError, SecretStoreState, ServiceLifecycle,
    StorageAvailability, StorageBackendKind, StorageCapabilities, StorageCapability,
    StorageCapabilityDiscovery, UnsupportedSecureSecretStore, UnsupportedServiceLifecycle, current,
    detect_platform,
};

#[test]
fn platform_detection_matches_the_compiled_target() {
    let detected = detect_platform();

    #[cfg(target_os = "linux")]
    assert_eq!(detected, Platform::Linux);
    #[cfg(target_os = "windows")]
    assert_eq!(detected, Platform::Windows);
    #[cfg(target_os = "macos")]
    assert_eq!(detected, Platform::Macos);
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    assert_eq!(detected, Platform::Generic);

    assert_eq!(detected.as_str(), detected.to_string());
}

#[test]
fn platform_paths_preserve_typed_roots_and_reject_escape_paths() {
    let paths = PlatformPaths::new(
        DataDir::new("data"),
        ConfigDir::new("config"),
        CacheDir::new("cache"),
        RuntimeDir::new("runtime"),
    );

    assert_eq!(paths.root(PathKind::Data), Path::new("data"));
    assert_eq!(paths.root(PathKind::Config), Path::new("config"));
    assert_eq!(paths.root(PathKind::Cache), Path::new("cache"));
    assert_eq!(paths.root(PathKind::Runtime), Path::new("runtime"));
    assert_eq!(
        paths.resolve(PathKind::Data, Path::new("objects/one")),
        Ok(PathBuf::from("data").join("objects/one"))
    );
    assert_eq!(
        paths.resolve(PathKind::Data, Path::new("../outside")),
        Err(PathResolutionError::InvalidRelativePath {
            kind: PathKind::Data,
            violation: PathResolutionViolation::ParentTraversal,
        })
    );

    let absolute = std::env::current_dir().expect("the test process has a current directory");
    assert!(absolute.is_absolute());
    assert!(matches!(
        paths.resolve(PathKind::Data, absolute),
        Err(PathResolutionError::InvalidRelativePath {
            kind: PathKind::Data,
            violation: PathResolutionViolation::Absolute,
        })
    ));
}

#[test]
fn storage_capabilities_do_not_infer_acceleration_from_the_host() {
    let unknown = StorageCapabilities::unknown();
    assert_eq!(unknown.backend(), StorageBackendKind::Unknown);
    assert_eq!(unknown.availability(), StorageAvailability::NotProbed);
    assert_eq!(
        unknown.support(StorageCapability::Reflink),
        CapabilitySupport::Unknown
    );
    assert!(!unknown.supports(StorageCapability::Reflink));

    let discovery = ReadOnlyStorageDiscovery;
    let inspected = discovery.discover(Path::new("."));
    assert_eq!(inspected.backend(), StorageBackendKind::LocalFilesystem);
    assert_eq!(inspected.availability(), StorageAvailability::Available);
    assert_eq!(
        inspected.support(StorageCapability::AtomicRename),
        CapabilitySupport::Unknown
    );
}

#[test]
fn unsupported_secret_store_never_accepts_or_returns_plaintext() {
    let store = UnsupportedSecureSecretStore::new();
    let name = SecretName::new("database-password").unwrap();

    assert_eq!(store.state(), SecretStoreState::Unsupported);
    assert_eq!(
        store.put_secret(&name, b"test-only-secret"),
        Err(SecretStoreError::Unsupported)
    );
    assert_eq!(store.get_secret(&name), Err(SecretStoreError::Unsupported));
    assert_eq!(
        store.delete_secret(&name),
        Err(SecretStoreError::Unsupported)
    );
}

#[test]
fn unsupported_lifecycle_exposes_stable_state_and_no_mutation() {
    let lifecycle = UnsupportedServiceLifecycle::new();

    assert!(!lifecycle.status().is_supported());
    assert_eq!(lifecycle.status().state().as_str(), "UNSUPPORTED");
    assert!(matches!(
        lifecycle.start(),
        Err(synveil_platform::LifecycleError::Unsupported)
    ));
    assert!(matches!(
        lifecycle.graceful_shutdown(),
        Err(synveil_platform::LifecycleError::Unsupported)
    ));
    assert!(matches!(
        lifecycle.restart(Default::default()),
        Err(synveil_platform::LifecycleError::Unsupported)
    ));
}

#[test]
fn current_runtime_has_a_clear_boundary_and_restricted_health() {
    let runtime = current();

    assert_eq!(runtime.platform(), detect_platform());
    assert_eq!(runtime.host_info().platform(), detect_platform());
    assert_eq!(
        runtime.secret_store().state(),
        SecretStoreState::Unsupported
    );
    assert_eq!(
        runtime.health().component(HealthComponent::SecretStore),
        Some(HealthState::Unavailable)
    );
    assert!(!runtime.health().readiness());
}

#[test]
fn all_adapter_namespaces_expose_the_same_portable_runtime_boundary() {
    let generic = synveil_platform::generic::GenericPlatformRuntime::new();
    let linux = synveil_platform::linux::LinuxPlatformRuntime::new();
    let windows = synveil_platform::windows::WindowsPlatformRuntime::new();
    let macos = synveil_platform::macos::MacosPlatformRuntime::new();

    let runtimes: [(&dyn PlatformRuntime, Platform); 4] = [
        (&generic, Platform::Generic),
        (&linux, Platform::Linux),
        (&windows, Platform::Windows),
        (&macos, Platform::Macos),
    ];

    for (runtime, expected_platform) in runtimes {
        assert_eq!(runtime.platform(), expected_platform);
        assert_eq!(runtime.host_info().platform(), expected_platform);
        assert_eq!(
            runtime.secret_store().state(),
            SecretStoreState::Unsupported
        );
        assert!(!runtime.service_lifecycle().status().is_supported());
    }
}
