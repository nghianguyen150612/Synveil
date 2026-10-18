# Synveil iOS — Infrastructure Module (`clients/ios/Infrastructure/`)

## Purpose & Ownership
The `Infrastructure` directory contains concrete platform and system framework implementations of the service protocols defined in `Domain/`.

### Sub-directories
- `Infrastructure/Network/`: Apple `URLSession` HTTP/TLS transport and authentication header injection.
- `Infrastructure/Persistence/`: Durable non-secret local SQLite database cache (`GRDB`).
- `Infrastructure/Security/`: `Security.framework` Keychain Vault for `svd1_` device bearer credentials.
- `Infrastructure/Files/`: `FileManager` local staging store and sandbox import/export handling.
- `Infrastructure/Transfers/`: Resumable 4 MiB upload and streaming download engine.
- `Infrastructure/RustBridge/`: Swift adapter wrapping the narrow C-FFI boundary (`synveil_ios_core`).

## Dependency Rules
- **Allowed Dependencies**: `Domain/` (entities and service protocols), Apple System Frameworks (`URLSession`, `Security`, `FileManager`, `Network`, `BackgroundTasks`), C-FFI Rust Bridge (`synveil_ios_core`).
- **Prohibited Dependencies**:
  - `SwiftUI` or `UIKit` presentation views.
  - Cross-layer leaking of raw system objects (e.g., raw Keychain C-pointers or SQLite handles) into `Domain/` or `Features/`.
