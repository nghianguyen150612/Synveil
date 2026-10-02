# Synveil iOS — Security & Keychain Infrastructure (`clients/ios/Infrastructure/Security/`)

## Purpose & Ownership
Implements `CredentialVaultProtocol` using Apple Keychain Services (`Security.framework`).

### Responsibilities
- Storing encrypted `svd1_` device bearer tokens scoped by profile ID.
- Applying `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` protection class (Secure Enclave protected, non-migratable).
- Providing in-memory bearer token retrieval for HTTP request header composition.
- Safe local credential deletion ("Forget on this device") via `SecItemDelete`.

## Prohibited
- Exposing plaintext secret strings in `UserDefaults`, SQLite database tables, logs, or UI view models.
- Storing credentials without profile and device scope checks.
