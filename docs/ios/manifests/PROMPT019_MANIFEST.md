# Prompt019 Manifest — Synveil iOS v0.1 — FFI Async & Concurrency Boundary

## 1. Prompt Overview & Starting State
- **Prompt Number**: P019
- **Goal**: Establish the authoritative async and concurrency boundary between Swift and Rust for Synveil iOS v0.1.
- **Starting Branch**: `ios-app`
- **Starting SHA**: `993426a3d37b809751d415037628e568917c27ea`
- **Actual Work Branch**: `ios/p019-ffi-concurrency`
- **Authoritative Inputs Inspected**:
  - `docs/ios/IOS_RUST_SWIFT_FFI_CONTRACT.md`
  - `docs/ios/manifests/PROMPT018_MANIFEST.md`
  - `crates/ios-ffi/src/lib.rs`
  - `crates/ios-ffi/Cargo.toml`
  - `crates/ios-ffi/README.md`
  - `clients/ios/Infrastructure/RustBridge/RustBridgeAdapter.swift`
  - `clients/ios/Infrastructure/RustBridge/RustBridgeStatus.swift`
  - `clients/ios/Infrastructure/RustBridge/RustBridgeError.swift`
  - `clients/ios/Infrastructure/RustBridge/README.md`
  - `clients/ios/Tests/SynveilTests/RustBridgeMemoryTests.swift`
  - `clients/ios/Infrastructure/RustBridge/Generated/synveil_ios_ffi.h`
  - `clients/ios/Synveil.xcodeproj/project.pbxproj`
  - `clients/ios/Support/validate_ios_sources.py`
  - `clients/ios/Support/tests/test_validate_ios_sources.py`
  - `scripts/build-ios-rust-artifacts.sh`
  - `scripts/validate_ios_rust_artifact.py`
  - `scripts/tests/test_validate_ios_rust_artifact.py`
  - `.github/workflows/ios-simulator-tests.yml`
  - `.github/workflows/ios-rust-apple-build.yml`
  - `.github/workflows/ios-static-validation.yml`

---

## 2. C ABI & Export Set Invariants
- **ABI Version**: `1`
- **Exact C Export Set**:
  1. `synveil_ffi_abi_version`
  2. `synveil_ffi_buffer_release`
  3. `synveil_ffi_sha256_format`
  4. `synveil_ffi_sha256_parse`
  5. `synveil_ffi_validate_abi_version`
- **Export Set Change Status**: `UNCHANGED` (0 new C ABI exports added).

---

## 3. Concurrency Architecture Decisions
- **Rust Async Runtime Status**: `NONE` (No Tokio, async-std, smol, Reqwest, or futures crossing FFI).
- **Callback ABI Status**: `CALLBACK_ABI_NOT_SELECTED_FOR_IOS_V0_1` (No function pointers, continuations, or task handles in Rust).
- **Opaque Async-Handle Status**: `NONE` (No task handle allocation, polling, or cancellation handle APIs).
- **Swift Concurrency Owner**: `SWIFT_OWNS_CONCURRENCY = TRUE` (Swift manages all asynchronous scheduling).

---

## 4. Swift Concurrency Boundary Implementation
- **Executor Type & Path**: `enum RustBridgeExecutor` in `clients/ios/Infrastructure/RustBridge/RustBridgeExecutor.swift`.
- **Executor Implementation Strategy**: `Task.detached` encapsulated worker task to escape caller actor context (`@MainActor`).
- **Detached Task Containment**: Detached task handle remains strictly private to `RustBridgeExecutor.run` and is immediately awaited; never stored or exposed to upper layers.
- **Sendable Constraints**: Generic operation `T` is `Sendable`, closure is `@Sendable`.
- **Sendable Audit**: 0 new `@unchecked Sendable` or `@preconcurrency` declarations added.
- **MainActor Policy**: Upper layers invoke `RustBridgeAsyncAdapter` asynchronously from `@MainActor` without executing synchronous C FFI inline on the main thread.
- **Async Adapter Type & Path**: `struct RustBridgeAsyncAdapter: Sendable` in `clients/ios/Infrastructure/RustBridge/RustBridgeAsyncAdapter.swift`.
- **Async Initialization Semantics**: `public init() async throws` executes low-level ABI probing and compatibility checks on `RustBridgeExecutor`.
- **Async Parse Semantics**: `public func parseSHA256(_ canonical: String) async throws -> Data` runs via `RustBridgeExecutor.run`.
- **Async Format Semantics**: `public func formatSHA256(_ digest: Data) async throws -> String` runs via `RustBridgeExecutor.run`.

---

## 5. Cancellation & Error Policy
- **Cancellation-Before-Call Behavior**: Evaluates `Task.checkCancellation()` before scheduling worker and inside detached task before calling FFI, throwing Swift `CancellationError`.
- **Cancellation-In-Flight Behavior**: In-flight synchronous Rust FFI call runs to completion to guarantee memory safety. No mid-FFI interrupt.
- **Cancellation-After-Call Behavior**: After awaiting FFI worker, `Task.checkCancellation()` checks parent task cancellation. If cancelled, worker result is discarded safely and `CancellationError` is thrown.
- **No-Mid-FFI-Interrupt Rule**: In-flight Rust call completes normally; Rust-owned buffers are freed normally before cancellation error is surfaced.
- **Error Propagation Behavior**: Existing `RustBridgeError` cases (`domainError`, `invalidUtf8`, etc.) propagate unchanged. `CancellationError` is thrown on cancellation. No generic error wrapping.

---

## 6. Validation & Static Analysis
- **Synchronous Adapter Isolation**: `RustBridgeAdapter` reference is restricted to `Infrastructure/RustBridge` and `Tests`. Direct usage in `App`, `Domain`, `Application`, or `Features` is rejected by static validator.
- **Validator Changes**: Updated `clients/ios/Support/validate_ios_sources.py` to enforce `RustBridgeAdapter` prohibition in production code outside `Infrastructure/RustBridge`.
- **Validator Self-Tests**: Updated `clients/ios/Support/tests/test_validate_ios_sources.py` (17/17 tests passing).
- **Header Drift Result**: Header check `./scripts/generate-ios-rust-header.sh --check` passed (`SUCCESS`).
- **Artifact Concurrency Metadata**: Extended `manifest.json` with:
  ```json
  "ffi_concurrency_model": "P019_SWIFT_MANAGED_SYNC_RUST",
  "rust_async_runtime": "NONE",
  "callback_abi": "NONE",
  "cancellation_model": "P019_SWIFT_COOPERATIVE_NO_MID_FFI_INTERRUPT"
  ```
- **Artifact Validator Tests**: Updated `scripts/validate_ios_rust_artifact.py` and `scripts/tests/test_validate_ios_rust_artifact.py` (4/4 tests passing).

---

## 7. Deferred Components & Next Steps
- **RustBridgeProtocol Status**: `RUST_BRIDGE_PROTOCOL_DEFERRED_TO_P020`
- **P020 Status**: P020 will introduce shared model mapping and `RustBridgeProtocol`.

---

## 8. Host Validation Executed
- `cargo fmt --all -- --check`
- `cargo check -p synveil-ios-ffi --locked`
- `cargo test -p synveil-ios-ffi --locked` (12/12 Rust tests passed)
- `cargo clippy -p synveil-ios-ffi --all-targets --locked -- -D warnings`
- `python3 clients/ios/Support/validate_ios_sources.py` (SUCCESS)
- `python3 -m unittest discover -s clients/ios/Support/tests` (17/17 passed)
- `python3 -m unittest discover -s scripts/tests` (4/4 passed)
- `python3 -m py_compile scripts/validate_ios_rust_artifact.py`
- `./scripts/generate-ios-rust-header.sh --check` (SUCCESS)
- `bash -n scripts/*.sh`
- `git diff --check`
