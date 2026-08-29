//! OS-backed persistent secret storage. No global keyring default is used:
//! selecting a mock default elsewhere cannot turn this into volatile storage.

use crate::{SecretName, SecretStore, SecretStoreError, SecretStoreState, SecretValue};

#[cfg(any(target_os = "linux", target_os = "windows"))]
use std::sync::Mutex;

#[cfg(any(target_os = "linux", target_os = "windows"))]
static NATIVE_SECRET_ACCESS: Mutex<()> = Mutex::new(());

const MAX_NATIVE_SECRET_BYTES: usize = 4_096;

/// Linux: persistent Secret Service over encrypted D-Bus sessions. Windows:
/// Credential Manager. Other targets fail explicitly as Unsupported.
///
/// Construction is side-effect-free; availability probes are read-only, and
/// credential operations occur only when callers explicitly use this port.
/// Backend diagnostics are never returned because they may contain secrets.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeSecureSecretStore;

impl NativeSecureSecretStore {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn entry(name: &SecretName) -> Result<keyring::Entry, SecretStoreError> {
        const SERVICE: &str = "org.synveil.desktop.credentials.v1";
        #[cfg(target_os = "linux")]
        let builder = keyring::secret_service::default_credential_builder();
        #[cfg(target_os = "windows")]
        let builder = keyring::windows::default_credential_builder();
        let credential = builder
            .build(None, SERVICE, name.as_str())
            .map_err(|_| SecretStoreError::Unavailable)?;
        Ok(keyring::Entry::new_with_credential(credential))
    }
}

impl SecretStore for NativeSecureSecretStore {
    fn state(&self) -> SecretStoreState {
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            let Ok(_guard) = NATIVE_SECRET_ACCESS.lock() else {
                return SecretStoreState::Unavailable;
            };
            let Ok(name) = SecretName::new("availability-probe-v1") else {
                return SecretStoreState::Unavailable;
            };
            let Ok(entry) = Self::entry(&name) else {
                return SecretStoreState::Unavailable;
            };
            // Only attributes are requested. We neither create a sentinel nor
            // read a user's credential during capability probing.
            match entry.get_attributes() {
                Ok(_) | Err(keyring::Error::NoEntry) => SecretStoreState::Available,
                Err(_) => SecretStoreState::Unavailable,
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        SecretStoreState::Unsupported
    }

    fn put_secret(&self, name: &SecretName, value: &[u8]) -> Result<(), SecretStoreError> {
        if value.is_empty() {
            return Err(SecretStoreError::EmptySecret);
        }
        if value.len() > MAX_NATIVE_SECRET_BYTES {
            return Err(SecretStoreError::SecretTooLarge);
        }
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            let _guard = NATIVE_SECRET_ACCESS
                .lock()
                .map_err(|_| SecretStoreError::Unavailable)?;
            Self::entry(name)?
                .set_secret(value)
                .map_err(|_| SecretStoreError::Unavailable)
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            let _ = name;
            Err(SecretStoreError::Unsupported)
        }
    }

    fn get_secret(&self, name: &SecretName) -> Result<Option<SecretValue>, SecretStoreError> {
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            let _guard = NATIVE_SECRET_ACCESS
                .lock()
                .map_err(|_| SecretStoreError::Unavailable)?;
            match Self::entry(name)?.get_secret() {
                Ok(value) => {
                    let value = SecretValue::new(value);
                    if value.as_bytes().is_empty()
                        || value.as_bytes().len() > MAX_NATIVE_SECRET_BYTES
                    {
                        return Err(SecretStoreError::Unavailable);
                    }
                    Ok(Some(value))
                }
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(_) => Err(SecretStoreError::Unavailable),
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            let _ = name;
            Err(SecretStoreError::Unsupported)
        }
    }

    fn delete_secret(&self, name: &SecretName) -> Result<bool, SecretStoreError> {
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            let _guard = NATIVE_SECRET_ACCESS
                .lock()
                .map_err(|_| SecretStoreError::Unavailable)?;
            match Self::entry(name)?.delete_credential() {
                Ok(()) => Ok(true),
                Err(keyring::Error::NoEntry) => Ok(false),
                Err(_) => Err(SecretStoreError::Unavailable),
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            let _ = name;
            Err(SecretStoreError::Unsupported)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_secret_bounds_fail_before_any_backend_operation() {
        let store = NativeSecureSecretStore::new();
        let name = SecretName::new("test-bounds").unwrap();
        assert_eq!(
            store.put_secret(&name, b""),
            Err(SecretStoreError::EmptySecret)
        );
        assert_eq!(
            store.put_secret(&name, &[0; MAX_NATIVE_SECRET_BYTES + 1]),
            Err(SecretStoreError::SecretTooLarge)
        );
        assert!(!format!("{store:?}").contains("test-bounds"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn native_builder_is_persistent_secret_service_not_the_global_mock() {
        let name = SecretName::new("test-native-builder-selection").unwrap();
        let entry = NativeSecureSecretStore::entry(&name).unwrap();
        assert!(
            entry
                .get_credential()
                .is::<keyring::secret_service::SsCredential>()
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_builder_is_windows_credential_manager_not_the_global_mock() {
        let name = SecretName::new("test-native-builder-selection").unwrap();
        let entry = NativeSecureSecretStore::entry(&name).unwrap();
        assert!(
            entry
                .get_credential()
                .is::<keyring::windows::WinCredential>()
        );
    }

    #[test]
    #[ignore = "requires an unlocked native OS credential store; writes only a fresh synthetic test key"]
    fn native_secret_survives_backend_recreation_and_is_deleted() {
        const CHILD_KEY: &str = "SYNVEIL_NATIVE_SECRET_TEST_CHILD_KEY";
        const TEST_VALUE: &[u8] = b"isolated-synthetic-native-test-value";
        if let Some(name) = std::env::var_os(CHILD_KEY) {
            let name = SecretName::new(name.into_string().unwrap()).unwrap();
            let restarted = NativeSecureSecretStore::new();
            assert_eq!(
                restarted.get_secret(&name).unwrap().unwrap().as_bytes(),
                TEST_VALUE
            );
            return;
        }
        let key = synveil_core::DeviceId::new();
        let name = SecretName::new(format!("synthetic-native-persistence-test/{key}")).unwrap();
        let first = NativeSecureSecretStore::new();
        assert_eq!(first.state(), SecretStoreState::Available);
        first.put_secret(&name, TEST_VALUE).unwrap();
        // A new process has no shared Rust memory or global keyring default.
        // Only the non-secret lookup name crosses the process boundary.
        let restarted = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "native_secrets::tests::native_secret_survives_backend_recreation_and_is_deleted",
            ])
            .env(CHILD_KEY, name.as_str())
            .output()
            .unwrap();
        let second = NativeSecureSecretStore::new();
        assert_eq!(
            second.get_secret(&name).unwrap().unwrap().as_bytes(),
            TEST_VALUE
        );
        assert!(second.delete_secret(&name).unwrap());
        assert!(first.get_secret(&name).unwrap().is_none());
        assert!(
            restarted.status.success(),
            "native secret must survive process restart"
        );
    }
}
