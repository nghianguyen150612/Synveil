# Synveil iOS — Rust Bridge Adapter (`clients/ios/Infrastructure/RustBridge/`)

## Purpose & Ownership
Implements `RustBridgeProtocol` by bridging Swift to the minimal C-static Rust library (`synveil_ios_core`).

### Responsibilities
- Type-safe Swift wrappers around C-FFI / UniFFI functions.
- UUIDv7 generation and SHA-256 cryptographic hashing.
- Token format parsing (`sve1_` enrollment, `svd1_` bearer).
- Sync change-feed evaluation, signed ACK computation, and rebaseline manifest tree hash verification.
- Memory ownership management (calling explicit Rust free functions for C-buffers returned by Rust).

## Future Artifacts Boundary
- Generated C headers and C-ABI export bindings will live here when introduced in P013–P016.
- Hand-editing generated bridge files is prohibited.
- Local Rust build outputs (`target/`) must remain untracked and outside source control.

## Prohibited
- Importing Tokio tasks, raw SQLx database handles, or desktop IPC sockets across FFI.
- Performing blocking FFI calls on the `@MainActor`.
