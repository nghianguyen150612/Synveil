# Prompt013 Manifest — Rust↔Swift FFI Strategy Contract

## 1. Metadata
- **Prompt**: `013`
- **Goal**: Define the authoritative Rust↔Swift FFI strategy contract for Synveil iOS v0.1 prior to building any Apple Rust binary artifacts.
- **Starting Integration Branch**: `ios-app`
- **Starting SHA**: `d29828af138cfb5e26c49444135f987e54623d2e`
- **Actual Work Branch**: `ios/p013-ffi-strategy-contract`
- **Validated Implementation Head**: `PENDING_COMMIT`
- **PR**: `PENDING_PR`
- **PR Base**: `ios-app`
- **Authoritative Files Inspected**:
  - `docs/ios/IOS_SHARED_CORE_REUSE_AUDIT.md`
  - `docs/ios/IOS_ARCHITECTURE.md`
  - `docs/ios/IOS_PLATFORM_MAPPING.md`
  - `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`
  - `docs/ios/IOS_V0_1_ROADMAP.md`
  - `docs/ios/IOS_VALIDATION_CI_ARCHITECTURE.md`
  - `docs/adr/ADR-058-ios-v0.1-client-architecture.md`
  - `docs/ios/manifests/PROMPT003_MANIFEST.md`
  - `docs/ios/manifests/PROMPT004_MANIFEST.md`
  - `docs/ios/manifests/PROMPT012_MANIFEST.md`
  - `clients/ios/README.md`
  - `clients/ios/Domain/README.md`
  - `clients/ios/Infrastructure/RustBridge/README.md`
  - `clients/ios/Support/validate_ios_sources.py`
  - `Cargo.toml`
  - `crates/core/Cargo.toml`

## 2. Repository Search Results
- Searched workspace for `extern "C"`, `#[no_mangle]`, `uniffi`, `cbindgen`, `crate-type`, `staticlib`, `cdylib`.
- **Finding**: Zero active production C-FFI, UniFFI, `extern "C"` exports, `staticlib`, or `cdylib` targets exist in iOS/core workspace crates. (Only desktop CXX-Qt vendor files contain C++ interop references, which are unrelated to iOS).

## 3. Selected Bridge Strategy & Contract Decisions
- **Selected Bridge Technology**: Explicit stable C ABI implemented by a dedicated thin Rust bridge crate.
- **Evaluated Alternatives**: UniFFI (`EVALUATED_NOT_SELECTED_FOR_IOS_V0_1`); Swift C++ Interop / CXX-Qt (`PROHIBITED`).
- **Bridge Crate Location & Package Name**: `crates/ios-ffi/` (`synveil-ios-ffi`).
- **Target Artifact Type**: `staticlib` (`libsynveil_ios_ffi.a`).
- **Apple Build Verification Status**: `APPLE_BUILD_NOT_YET_VERIFIED_P013` (compilation verified in P014/P015).
- **Symbol Namespace**: Mandatory `synveil_ffi_` prefix on all C exports.
- **ABI Versioning**: `synveil_ffi_abi_version() -> uint32_t` returning `1`.
- **Primitive Representation**: Explicit fixed-width C primitives (`uint8_t`, `uint32_t`, `uint64_t`, `int32_t`, `int64_t`, `size_t`).
- **Boolean Representation**: `uint8_t` (`0 = false`, `1 = true`).
- **String Contract**: Length-delimited UTF-8 (`const uint8_t* pointer + size_t length`). Swift owns input memory; Rust returns owned `SynveilFfiBuffer` released via Rust API.
- **Byte Buffer Contract**: Pointer + length (`const uint8_t* pointer + size_t length`).
- **Null Pointer Rules**: NULL + len 0 is valid empty buffer; NULL + len > 0 is invalid argument error (`SYNVEIL_FFI_STATUS_INVALID_ARGUMENT`).
- **Memory Ownership & Release**: Creator allocator owns release mechanism. Swift calls `synveil_ffi_buffer_release()`, never libc `free()`.
- **Error Transport**: Status code `uint32_t` return value + out-parameter data envelope. Zero secrets in error strings.
- **Panic Boundary Policy**: Mandatory `std::panic::catch_unwind` firewall on every entrypoint, converting uncaught panics to `SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED`.
- **Async Policy**: Synchronous, bounded, deterministic domain logic. Zero Rust `Future` or Tokio runtime across FFI.
- **Threading Policy**: Fully reentrant stateless calls. Offloaded off `@MainActor` by Swift adapter. Zero global mutable FFI state.
- **Opaque Handle Policy**: Preferred stateless calls in v0.1. Handles must be opaque pointers with explicit destroy API if introduced later.
- **Swift Adapter Boundary**: Raw C ABI imports restricted strictly to `clients/ios/Infrastructure/RustBridge/`. Upper layers never access raw C symbols.
- **Callable Protocol Deferral**: Swift `RustBridgeProtocol` remains deferred until P016/P020 when real callable bridge methods exist.
- **Header Generation Policy**: Recommended `CBINDGEN_RECOMMENDED_FOR_P014_P015`.
- **Security & Redaction**: Zero secret inputs logged or dumped in panic/error payloads.

## 4. Prompt Responsibility Matrix (P014–P020)
- **P014**: Minimal Rust library crate (`crates/ios-ffi/`, package `synveil-ios-ffi`, `staticlib`) + Apple target check (`cargo check`).
- **P015**: Rust Apple artifact CI workflow (`libsynveil_ios_ffi.a`).
- **P016**: First trivial Swift↔Rust call (`synveil_ffi_abi_version()`).
- **P017**: FFI error model & panic firewall (`std::panic::catch_unwind`).
- **P018**: Memory ownership & safety implementation (`synveil_ffi_buffer_release`, leak/double-free tests).
- **P019**: Async & concurrency boundary (background thread dispatch off `@MainActor`).
- **P020**: Shared model mapping & Swift `RustBridgeProtocol` introduction.

## 5. Linux Verification & Static Validation
- **Static Validator**: `python3 clients/ios/Support/validate_ios_sources.py` -> SUCCESS.
- **Validator Unit Self-Tests**: `python3 -m unittest discover -s clients/ios/Support/tests` -> SUCCESS (16 self-tests).
- **Git Diff Check**: `git diff --check` -> Clean (no whitespace errors or conflict markers).
- **FFI Artifact Absence Check**: Confirmed zero `.a`, `.dylib`, `.framework`, `.xcframework`, `.h`, generated bindings, or Rust FFI code added in P013.

## 6. GitHub Actions CI Evidence
- **`iOS Static Validation`**: `PENDING_PR_RUN`
- **`iOS Build`**: `PENDING_PR_RUN`
- **`iOS Simulator Tests`**: `PENDING_PR_RUN`
- **Unrelated Subsystem Failures Classified**:
  - `Build, reproduce, inspect, and smoke AppImage`: `FAILURE` — classified `UNRELATED_SUBSYSTEM_FAILURE` (Linux desktop CXX-Qt app build path check failure in `synveil-desktop`, completely outside native iOS v0.1 client scope).
  - `Native desktop UI (Windows Qt 6)`: `FAILURE` — classified `UNRELATED_SUBSYSTEM_FAILURE` (Windows Qt 6 CXX-Qt desktop app build failure in `crates/install-engine/src/appimage.rs`, completely outside native iOS v0.1 client scope).
  - `Native desktop UI (Linux Qt 6)`: `FAILURE` — classified `UNRELATED_SUBSYSTEM_FAILURE` (Linux Qt 6 CXX-Qt desktop app job failure, completely outside native iOS v0.1 client scope).
  - `Check (windows-latest)`: `FAILURE` — classified `UNRELATED_SUBSYSTEM_FAILURE` (Windows Rust workspace build failure in `crates/install-engine/src/appimage.rs`, completely outside native iOS v0.1 client scope).
  - `Test (macos-latest)`: `FAILURE` — classified `UNRELATED_SUBSYSTEM_FAILURE` (macOS Rust desktop client test failure in `synveil-client::control::tests`, completely outside native iOS v0.1 client scope).
  - `Test (windows-latest)`: `FAILURE` — classified `UNRELATED_SUBSYSTEM_FAILURE` (Windows Rust installer engine test failure in `synveil-install-engine`, completely outside native iOS v0.1 client scope).
  - `Test (ubuntu-latest)`: `FAILURE` — classified `UNRELATED_SUBSYSTEM_FAILURE` (Linux systemd tmpfiles test failure in `synveil-metadata`, completely outside native iOS v0.1 client scope).
  - `Web quality`: `FAILURE` — classified `UNRELATED_SUBSYSTEM_FAILURE` (React web frontend Vitest test failure in `RestoreWorkflowPage.test.tsx`, completely outside native iOS v0.1 client scope).
  - `PG17 live suites (scheduling, worker, cycle, lifecycle, stress)`: `FAILURE` — classified `UNRELATED_SUBSYSTEM_FAILURE` (PostgreSQL 17 server maintenance integration suite failure, completely outside native iOS v0.1 client scope).
  - `Build and validate DEB + RPM (unsigned CI artifacts)`: `FAILURE` — classified `UNRELATED_SUBSYSTEM_FAILURE` (Linux desktop DEB/RPM packaging reproducibility failure in `synveil-desktop`, completely outside native iOS v0.1 client scope).

## 7. Files Summary
- **Files Created**:
  - `docs/ios/IOS_RUST_SWIFT_FFI_CONTRACT.md`
  - `docs/ios/manifests/PROMPT013_MANIFEST.md`
- **Files Modified**:
  - `clients/ios/README.md`
  - `clients/ios/Infrastructure/RustBridge/README.md`
  - `docs/ios/IOS_ARCHITECTURE.md`
  - `docs/ios/IOS_VALIDATION_CI_ARCHITECTURE.md`

## 8. Limitations & Exclusions
- P013 is strictly a contract prompt. Zero C code, Rust code, headers, XCFrameworks, or Swift call wrappers were implemented.
- Rust Apple compilation, static library creation, cbindgen, and FFI bridging will be implemented in P014–P020.

## 9. Merge Status
- **PR**: `PENDING_PR`
- **Merge Status**: PENDING_CI_AND_MERGE
- **Final `ios-app` SHA**: PENDING_MERGE
