# Synveil iOS — Rust Bridge Adapter (`clients/ios/Infrastructure/RustBridge/`)

## Purpose & Ownership
`clients/ios/Infrastructure/RustBridge/` is the sole authorized location for raw C ABI imports (`SynveilRustFFI`), C headers (`synveil_ios_ffi.h`), module maps (`module.modulemap`), and low-level generated bridge bindings for the native Synveil iOS client (`v0.1`).

### Authoritative FFI Strategy Contract
The authoritative Rust↔Swift FFI strategy for Synveil iOS v0.1 is defined in:

`docs/ios/IOS_RUST_SWIFT_FFI_CONTRACT.md`

- **Bridge Technology**: Explicit stable C ABI implemented by a dedicated thin Rust bridge crate (`crates/ios-ffi/`, package `synveil-ios-ffi`, generating static library `libsynveil_ios_ffi.a`).
- **UniFFI Status**: UniFFI was evaluated and classified as `EVALUATED_NOT_SELECTED_FOR_IOS_V0_1`.
- **C++ Interop Prohibition**: Swift C++ Interop, Qt, and CXX-Qt are strictly prohibited for iOS v0.1.
- **FFI Boundary Ownership**: All raw C-FFI exports (`SynveilRustFFI`) are restricted strictly to `Infrastructure/RustBridge/`. No upper layer (`Domain`, `Application`, `Features`) may import or invoke raw C ABI symbols.
- **Callable Protocol Deferral**: To avoid inventing fake or synthetic Swift methods, `RustBridgeProtocol` remains deferred to P020 (`RUST_BRIDGE_PROTOCOL_DEFERRED_TO_P020`).

### P016 Status & First Compiled Swift ↔ Rust Call
- **First Exported C Symbol**: `uint32_t synveil_ffi_abi_version(void)`
- **ABI Version**: `1`
- **Header Generation Tooling**: `cbindgen 0.28.0` (pinned) via `scripts/generate-ios-rust-header.sh`
- **Generated Header Path**: `clients/ios/Infrastructure/RustBridge/Generated/synveil_ios_ffi.h`
- **Raw Clang Module**: `SynveilRustFFI` defined in `clients/ios/Infrastructure/RustBridge/Raw/module.modulemap`
- **Swift Infrastructure Adapter**: `RustBridgeAdapter.swift` (validates `synveil_ffi_abi_version() == 1`)
- **Simulator XCTest Verification**: `RustBridgeABITests.swift` calling `RustBridgeAdapter` and verifying real linkage.
- **Xcode Linker Strategy**: `OTHER_LDFLAGS = "-lsynveil_ios_ffi"`, platform-conditional `LIBRARY_SEARCH_PATHS`, and `SWIFT_INCLUDE_PATHS` referencing raw module map.

### Staging Layout (P016)
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
├── include/
│   └── synveil_ios_ffi.h
├── manifest.json
└── SHA256SUMS
```

### Prompt Sequencing (P014–P020)
1. **P014**: Minimal Rust bridge crate (`crates/ios-ffi/`, `synveil-ios-ffi`, `staticlib`) & Apple compile proof. [COMPLETE]
2. **P015**: Rust Apple artifact CI workflow & packaging (`libsynveil_ios_ffi.a`). [COMPLETE]
3. **P016**: First real Swift↔Rust call (`synveil_ffi_abi_version()`). [COMPLETE]
4. **P017**: FFI error model & panic firewall (`std::panic::catch_unwind`).
5. **P018**: Memory ownership & buffer release safety (`synveil_ffi_buffer_release`).
6. **P019**: Async & concurrency boundary (background thread dispatch off MainActor).
7. **P020**: Shared model mapping & Swift `RustBridgeProtocol` introduction.

## Prohibited
- Importing Tokio tasks, raw SQLx database handles, or desktop IPC sockets across FFI.
- Performing blocking FFI calls on `@MainActor`.
- Exposing raw FFI symbols or C-pointers to `Domain`, `Application`, or `Features` layers.
