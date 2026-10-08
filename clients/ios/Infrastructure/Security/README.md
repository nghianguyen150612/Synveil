# Synveil iOS — Keychain Credential Infrastructure

## Ownership

`KeychainCredentialStore` implements the Application layer's
`SecureCredentialSinkProtocol` using Apple Keychain Services from `Security.framework`.
It is actor-isolated so active-session reads and writes are serialized.

## v0.1 storage policy

- One deterministic generic-password item stores the active device session.
- The versioned service is `com.synveil.ios.device-session.v1`; account is
  `active-device-session.v1`.
- Items use `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` and explicitly disable
  synchronizable storage.
- The primary app's default Keychain access is used. There is no custom access group or Keychain
  Sharing entitlement.
- The bounded, version-1 Codable envelope binds the credential to its canonical server endpoint,
  owner, device, credential ID, and server creation time. Transient request IDs are excluded.
- Store and replacement verify the exact session by reading it back. Replacement uses
  `SecItemUpdate` without deleting first.
- `load` validates the envelope, canonical endpoint, identifiers, timestamp, and `svd1_` credential.
  It is a primitive for Prompt026 and does not restore startup state.
- `delete` removes only the local active item and is idempotent. It does not perform server revocation.

## Security boundary

The credential exists in Keychain data and transient memory needed to validate or construct requests.
It is never used as an item service, account, label, or identifier and is never written to logs,
UserDefaults, SQLite, or files. The preflight probe uses a unique temporary account and attempts
cleanup on every outcome.
