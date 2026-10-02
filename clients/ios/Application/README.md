# Synveil iOS — Application Orchestration Module (`clients/ios/Application/`)

## Purpose & Ownership
The `Application` directory contains application-level orchestration, use-case coordinators, and process lifecycle managers. It drives workflows without tying logic to specific SwiftUI views.

### Responsibilities
- **`AppOrchestrator`**: Coordinates startup initialization, health check preflights, and global state transitions.
- **`SessionController`**: Manages active `ServerProfile` state, credential loading, unauthenticated vs authenticated navigation boundaries, and profile switching.
- **`SyncCoordinator`**: Orchestrates inbound change-feed ingestion, outbound mutation replay queues, and rebaseline recovery flows.
- **`TransferCoordinator`**: Bridge between UI actions and the background `TransferEngine`.
- **`BackgroundSyncScheduler`**: Registers and handles iOS system `BGAppRefreshTask` triggers.

## Dependency Rules
- **Allowed Dependencies**: `Domain/` (models, errors), Service Interfaces (`HTTPTransportProtocol`, `CredentialVaultProtocol`, `LocalCacheStoreProtocol`, `TransferEngineProtocol`, `RustBridgeProtocol`).
- **Prohibited Dependencies**:
  - `SwiftUI` or `UIKit` framework imports.
  - Concrete `URLSession`, SQLite SQL queries, or `Security.framework` Keychain calls (must depend strictly on Service protocols).
  - Direct C-FFI Rust invocations.
