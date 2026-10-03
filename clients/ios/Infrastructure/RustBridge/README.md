# Synveil iOS — Rust Bridge Adapter (`clients/ios/Infrastructure/RustBridge/`)

## Purpose & Ownership
`clients/ios/Infrastructure/RustBridge/` is the sole authorized location for raw Rust C-FFI / UniFFI imports, C headers, and low-level generated bridge bindings.

### Protocol Strategy & Deferral Decision (P012)
- **FFI Boundary Ownership**: All raw Rust C-FFI exports and generated binding modules are restricted strictly to `Infrastructure/RustBridge/`. No upper layer (`Domain`, `Application`, `Features`) may import or invoke raw FFI modules.
- **Callable Protocol Deferral**: To avoid inventing fake or synthetic Swift methods (or creating an empty marker protocol without runtime semantics), the callable `RustBridgeProtocol` Swift method contract is explicitly deferred to P013–P016, where real FFI signatures, ABI type mappings, memory ownership, and error encoding will be established.

### Future Responsibilities (P013+)
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
- Exposing raw FFI symbols or C-pointers to `Domain`, `Application`, or `Features` layers.
