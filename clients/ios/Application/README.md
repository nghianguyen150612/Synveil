# Synveil iOS — Application Orchestration Module (`clients/ios/Application/`)

## Purpose & Ownership
The `Application` directory contains application-level orchestration, use-case coordinators, and process lifecycle managers. It drives workflows without tying logic to specific SwiftUI views.

### Responsibilities
- **`SessionController`**: `@Observable` `@MainActor` manager of top-level application startup state (`AppStartupState`). Coalesces startup restoration, discards stale results, and exposes an explicit retry for transient remote verification failures.
- **`SessionRestorationService`**: Loads the active session through `SecureCredentialSinkProtocol`, retains the Keychain-validated endpoint scope, and coordinates read-only DeviceBearer authorization verification.
- **`AppStartupState`**: Pure Swift (`Sendable`, `Equatable`) enum representing startup and ownership boundaries, including `.restorationVerificationPending` while a locally valid session awaits remote authorization.
- **`AppRecoveryReason`**: Typed classification of recovery boundaries (`.configuration`, `.authentication`, `.deviceRevoked`, `.secureStore`, `.credential`, `.scopeMismatch`, `.tls`, `.protocolFailure`, `.enrollmentAmbiguous`, `.transport`).
- **`AppOrchestrator`**: Coordinates startup initialization, health check preflights, and global state transitions.
- **`SyncCoordinator`**: Orchestrates inbound change-feed ingestion, outbound mutation replay queues, and rebaseline recovery flows.
- **`TransferCoordinator`**: Bridge between UI actions and the background `TransferEngine`.
- **`BackgroundSyncScheduler`**: Registers and handles iOS system `BGAppRefreshTask` triggers.

## Dependency Rules
- **Allowed Dependencies**: `Domain/` (models, errors), Service Interfaces (`HTTPTransportProtocol`, `CredentialVaultProtocol`, `LocalCacheStoreProtocol`, `TransferEngineProtocol`, `RustBridgeProtocol`).
- **Prohibited Dependencies**:
  - `SwiftUI` or `UIKit` framework imports.
  - Concrete `URLSession`, SQLite SQL queries, or `Security.framework` Keychain calls (must depend strictly on Service protocols).
  - Direct C-FFI Rust invocations or concrete `RustBridgeAsyncAdapter`/`RustBridgeAdapter` dependencies (must depend on `RustBridgeProtocol` abstraction defined in `Application/Services/RustBridgeProtocol.swift`).
