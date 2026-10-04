# Synveil iOS — Rust Bridge Adapter (`clients/ios/Infrastructure/RustBridge/`)

## Purpose & Ownership
`clients/ios/Infrastructure/RustBridge/` is the sole authorized location for raw C ABI imports, C headers (`synveil_ios_ffi.h`), and low-level generated bridge bindings for the native Synveil iOS client (`v0.1`).

### Authoritative FFI Strategy Contract (P013)
The authoritative Rust↔Swift FFI strategy for Synveil iOS v0.1 is defined in:

`docs/ios/IOS_RUST_SWIFT_FFI_CONTRACT.md`

- **Bridge Technology**: Explicit stable C ABI implemented by a dedicated thin Rust bridge crate (`crates/ios-ffi/`, package `synveil-ios-ffi`, generating static library `libsynveil_ios_ffi.a`).
- **UniFFI Status**: UniFFI was evaluated and classified as `EVALUATED_NOT_SELECTED_FOR_IOS_V0_1`.
- **C++ Interop Prohibition**: Swift C++ Interop, Qt, and CXX-Qt are strictly prohibited for iOS v0.1.
- **FFI Boundary Ownership**: All raw C-FFI exports and generated binding modules are restricted strictly to `Infrastructure/RustBridge/`. No upper layer (`Domain`, `Application`, `Features`) may import or invoke raw C ABI symbols.
- **Callable Protocol Deferral**: To avoid inventing fake or synthetic Swift methods (or creating an empty marker protocol without runtime semantics), the callable `RustBridgeProtocol` Swift method contract remains deferred until P016/P020 when real compiled FFI signatures, ABI type mappings, memory ownership, and error encoding are available.

### P015 Status & CI Artifact Pipeline
- **Dedicated CI Artifact Workflow**: `.github/workflows/ios-rust-apple-build.yml`
- **Artifact Build Script**: `scripts/build-ios-rust-artifacts.sh`
- **Artifact Validator**: `scripts/validate_ios_rust_artifact.py`
- **Published GitHub Actions Artifact**: `synveil-ios-rust-staticlibs`
- **Supported Target Architectures**:
  - Device: `aarch64-apple-ios` (`device/arm64/libsynveil_ios_ffi.a`)
  - Simulator Apple Silicon: `aarch64-apple-ios-sim` (`simulator/arm64/libsynveil_ios_ffi.a`)
  - Simulator Intel: `x86_64-apple-ios` (`simulator/x86_64/libsynveil_ios_ffi.a`)
  - Simulator Universal: `lipo` combined `arm64` + `x86_64` (`simulator/universal/libsynveil_ios_ffi.a`)
- **Deterministic Staging Layout**:
  ```text
  target/ios-rust-artifacts/
  ├── device/
  │   └── arm64/
  │       └── libsynveil_ios_ffi.a
  ├── simulator/
  │   ├── arm64/
  │   │   └── libsynveil_ios_ffi.a
  │   ├── x86_64/
  │   │   └── libsynveil_ios_ffi.a
  │   └── universal/
  │       └── libsynveil_ios_ffi.a
  ├── manifest.json
  └── SHA256SUMS
  ```
- **Integrity & Schema**: `manifest.json` schema v1 + `SHA256SUMS` with verified SHA-256 digests and zero private path leakage.
- **C ABI & Header Status in P015**:
  - No C ABI functions exported yet (`synveil_ffi_*` symbols verified absent; deferred to P016).
  - No C headers (`synveil_ios_ffi.h`) or `cbindgen` invocation yet (deferred to P016).
  - No Swift or Xcode linker linking configured yet (P016 owns first Swift↔Rust call).

### P014–P020 Prompt Sequencing
1. **P014**: Minimal Rust bridge crate (`crates/ios-ffi/`, package `synveil-ios-ffi`, `staticlib`) & Apple compile proof. [COMPLETE]
2. **P015**: Rust Apple artifact CI workflow & packaging (`libsynveil_ios_ffi.a`). [COMPLETE]
3. **P016**: First trivial Swift↔Rust call (`synveil_ffi_abi_version()`).
4. **P017**: FFI error model & panic firewall (`std::panic::catch_unwind`).
5. **P018**: Memory ownership & buffer release safety (`synveil_ffi_buffer_release`).
6. **P019**: Async & concurrency boundary (background thread dispatch off MainActor).
7. **P020**: Shared model mapping & Swift `RustBridgeProtocol` introduction.

## Future Responsibilities (P016+)
- Type-safe Swift wrappers around C ABI functions.
- UUIDv7 generation and SHA-256 cryptographic hashing.
- Token format parsing (`sve1_` enrollment, `svd1_` bearer).
- Sync change-feed evaluation, signed ACK computation, and rebaseline manifest tree hash verification.
- Memory ownership management (calling explicit Rust release functions for C-buffers returned by Rust).

## Future Artifacts Boundary
- Generated C headers (`synveil_ios_ffi.h`) and C-ABI export bindings will live here when introduced in P016.
- Hand-editing generated bridge files is prohibited.
- Local Rust build outputs (`target/`) must remain untracked and outside source control.

## Prohibited
- Importing Tokio tasks, raw SQLx database handles, or desktop IPC sockets across FFI.
- Performing blocking FFI calls on the `@MainActor`.
- Exposing raw FFI symbols or C-pointers to `Domain`, `Application`, or `Features` layers.
