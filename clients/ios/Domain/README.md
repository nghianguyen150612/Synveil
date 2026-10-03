# Synveil iOS — Domain Module (`clients/ios/Domain/`)

## Purpose & Ownership
The `Domain` directory contains platform-independent Swift domain entities, value objects, domain error classifications, and service protocol contracts. It represents the central core of the application architecture.

### Key Domain Concepts
- **Entities & Value Objects**: `ServerProfile`, `DeviceSession`, `Library`, `LogicalNode`, `MutationIntent`, `TransferJob`, `SyncCheckpoint`.
- **Domain Error Taxonomy**: `DomainError` and safe user-presentable error mappings.
- **Service Protocol Boundaries**:
  - `HTTPTransportProtocol` (`Domain/Services/Transport/`): Abstract HTTP networking transport contract (`HTTPMethod`, `HTTPTransportRequest`, `HTTPTransportResponse`).
  - `CredentialVaultProtocol` (reserved for auth prompts P024–P027).
  - `LocalCacheStoreProtocol` (reserved for local persistence P038+).
  - `TransferEngineProtocol` (reserved for transfer engine P028–P035).
  - `RustBridgeProtocol`: Intentionally deferred until P013/P016 when real Swift ↔ Rust FFI signatures and bindings are established.

## Dependency Rules
- **Allowed Dependencies**: Swift Standard Library & `Foundation` (`URL`, `UUID`, `Date`, `Data`).
- **Prohibited Dependencies**:
  - `SwiftUI`, `UIKit`, or any UI frameworks.
  - `URLSession` or HTTP networking frameworks.
  - `Security.framework` (Keychain Services).
  - SQLite, `GRDB`, or persistence engines.
  - Rust C-FFI header exports or raw C-pointers.
  - Concrete infrastructure implementations.

Domain entities must remain pure, testable, and completely decoupled from framework side-effects.
