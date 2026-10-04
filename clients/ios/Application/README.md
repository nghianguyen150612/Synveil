# Synveil iOS — Application Orchestration Module (`clients/ios/Application/`)

## Purpose & Ownership
The `Application` directory contains application-level orchestration, use-case coordinators, and process lifecycle managers. It drives workflows without tying logic to specific SwiftUI views.

### Responsibilities
- **`SessionController`**: `@Observable` `@MainActor` manager of top-level application startup state (`AppStartupState`). Idempotently resolves initial startup state against `AppConfiguration` and drives root navigation state transitions.
- **`AppStartupState`**: Pure Swift (`Sendable`, `Equatable`) enum representing major application ownership boundaries: `.initializing`, `.needsServerProfile`, `.readyForServerValidation`, `.needsEnrollment`, `.authenticated`, and `.recoveryRequired(AppRecoveryReason)`.
- **`AppRecoveryReason`**: Typed classification of recovery boundaries (`.configuration`, `.authentication`, `.deviceRevoked`, `.secureStore`, `.enrollmentAmbiguous`, `.transport`).
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
