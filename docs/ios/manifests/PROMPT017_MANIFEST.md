# Synveil iOS v0.1 — Prompt017 Manifest

## 1. Prompt Metadata
- **Prompt Identifier**: `Prompt017`
- **Title**: FFI Error Model & Panic Firewall
- **Goal**: Implement stable FFI status/error model, reusable Rust panic firewall (`ffi_status_boundary`), Swift status decoder and error mapper, minimal fallible bridge C ABI call (`synveil_ffi_validate_abi_version`), real Simulator error tests, and updated artifact validation/headers.
- **Starting Integration Branch**: `ios-app`
- **Starting SHA**: `f9947a2491cad26bc1d3a5eb2fd3125d50cc672d`
- **Work Branch**: `ios/p017-ffi-error-model-6468962480144082762`
- **Validated Implementation Head**: `2c78df6ba55917bcf1ad92efdd26a1fd64d60acd`
- **PR**: #53 — https://github.com/nghianguyen150612/Synveil/pull/53
- **PR Base**: `ios-app`
- **Authoritative Files Inspected**:
  - `docs/ios/IOS_RUST_SWIFT_FFI_CONTRACT.md`
  - `docs/ios/manifests/PROMPT013_MANIFEST.md`
  - `docs/ios/manifests/PROMPT016_MANIFEST.md`
  - `crates/ios-ffi/src/lib.rs`
  - `crates/ios-ffi/cbindgen.toml`
  - `crates/ios-ffi/README.md`
  - `clients/ios/Infrastructure/RustBridge/RustBridgeAdapter.swift`
  - `clients/ios/Infrastructure/RustBridge/Generated/synveil_ios_ffi.h`
  - `clients/ios/Infrastructure/RustBridge/Raw/module.modulemap`
  - `clients/ios/Tests/SynveilTests/RustBridgeABITests.swift`
  - `scripts/generate-ios-rust-header.sh`
  - `scripts/build-ios-rust-artifacts.sh`
  - `scripts/validate_ios_rust_artifact.py`
  - `scripts/tests/test_validate_ios_rust_artifact.py`
  - `.github/workflows/ios-rust-apple-build.yml`
  - `.github/workflows/ios-build.yml`
  - `.github/workflows/ios-simulator-tests.yml`
  - `clients/ios/Support/validate_ios_sources.py`

## 2. ABI & Status Specification
- **ABI Version**: `1`
- **Status Table**:
  - `0` = `SUCCESS`
  - `1` = `INVALID_ARGUMENT`
  - `2` = `INVALID_UTF8`
  - `3` = `BUFFER_TOO_SMALL`
  - `4` = `DOMAIN_ERROR`
  - `5` = `INTERNAL_ERROR`
  - `6` = `PANIC_ENCOUNTERED`
  - `7` = `UNSUPPORTED_ABI_VERSION`
- **Additive Code 7 Justification**: `UNSUPPORTED_ABI_VERSION` added as an additive refinement to cleanly distinguish ABI incompatibility from domain/invalid argument errors without altering existing numeric meanings.
- **C Status Representation**: `uint32_t` functions and generated `#define SYNVEIL_FFI_STATUS_*` constants in `synveil_ios_ffi.h`.
- **Rust Status Representation**: `#[repr(u32)] pub enum SynveilFfiStatus`.
- **Panic Firewall**: `ffi_status_boundary<F>(operation: F) -> u32` using `std::panic::catch_unwind`.
- **Panic Payload Policy**: Panic payload never crosses ABI; converted strictly to `PANIC_ENCOUNTERED` (6).
- **Process-Global Panic Hook**: None introduced; process-global hooks prohibited to prevent interference with host app environment.

## 3. C ABI Export Set
- **Fallible Exported Function**: `synveil_ffi_validate_abi_version`
- **Function Signature**: `uint32_t synveil_ffi_validate_abi_version(uint32_t expected_version);`
- **Semantics**:
  - `expected_version == 1` -> `SUCCESS` (0)
  - `expected_version == 0` -> `INVALID_ARGUMENT` (1)
  - `expected_version != 0 && expected_version != 1` -> `UNSUPPORTED_ABI_VERSION` (7)
- **Current Exact C Export List**:
  - `synveil_ffi_abi_version`
  - `synveil_ffi_validate_abi_version`
- **Generated Header**: `clients/ios/Infrastructure/RustBridge/Generated/synveil_ios_ffi.h`
- **cbindgen Version & Check**: `cbindgen 0.28.0` verified via `./scripts/generate-ios-rust-header.sh --check`.

## 4. Swift Bridge Layer
- **Swift Status Type**: `RustBridgeStatus` (decodes raw `UInt32` status codes including `.unknown(UInt32)`).
- **Swift Error Type**: `RustBridgeError` (`invalidArgument`, `invalidUtf8`, `bufferTooSmall`, `domainError`, `internalError`, `panicEncountered`, `unknownStatus(UInt32)`).
- **Unknown Status Handling**: Maps safely to `.unknown(code)` / `RustBridgeError.unknownStatus(code)` without trapping or default success.
- **RustBridgeAdapter Behavior**: `init()` invokes `synveil_ffi_validate_abi_version` and throws `RustBridgeCompatibilityError` or `RustBridgeError` on non-success status.
- **RustBridgeProtocol Status**: `DEFERRED_TO_P020`.

## 5. Testing & Verification
- **Rust Unit Tests**:
  - Verified numeric status values and constants.
  - Verified `ffi_status_boundary` success, error return, and deliberate panic containment (`PANIC_ENCOUNTERED`).
  - Verified `synveil_ffi_validate_abi_version` with `1`, `0`, and `2`.
- **Swift XCTest Files**:
  - `RustBridgeABITests.swift`: Real C ABI calls proving adapter init success (v1), `invalidArgument` (v0), and `unsupportedABIVersion` (v2).
  - `RustBridgeErrorTests.swift`: Swift status decoder unit tests for all codes and unknown status `0xFFFF_FFFE`.
- **Simulator Real Rust Error Proof**: Proved via `RustBridgeAdapter(expectedABIVersion: 0)` and `RustBridgeAdapter(expectedABIVersion: 2)` calling real Rust static library FFI export `synveil_ffi_validate_abi_version`.

## 6. Artifact & Safety Boundaries
- **Artifact Symbol Policy**: Exact two approved symbols (`synveil_ffi_abi_version`, `synveil_ffi_validate_abi_version`).
- **Artifact Manifest Status**: `"c_abi_export_status": "STATUS_MODEL_P017"`, `"ffi_status_model": "P017_STABLE_UINT32"`.
- **Artifact Validator Tests**: Updated and passing in `scripts/tests/test_validate_ios_rust_artifact.py`.
- **Memory/Buffer Ownership Status**: `DEFERRED_P018`.
- **Async Boundary Status**: `DEFERRED_P019`.
- **Shared Model Mapping Status**: `DEFERRED_P020`.

## 7. Deferred Items & Limitations
- Buffer allocation/release (`SynveilFfiBuffer`, `synveil_ffi_buffer_release`) remains strictly deferred to P018.
- Concurrency scheduling / async Task offloading remains deferred to P019.
- Domain model mapping (hashes, tokens, sync feeds) remains deferred to P020.


## 8. Implementation-Head CI & Artifact Evidence
Validated implementation head: `2c78df6ba55917bcf1ad92efdd26a1fd64d60acd`.

- **iOS Rust Apple Build**: run `37174680907` — SUCCESS
  - Job: `111354736022` — `Rust Apple Target Build`
  - Artifact: `synveil-ios-rust-staticlibs`
  - Artifact ID: `11293175492`
  - Size: `29,526,689` bytes
  - Expired: false
  - Expires: `2026-10-18T03:45:21Z`
- **iOS Static Validation**: run `37174680889` — SUCCESS
- **iOS Build**: run `37174680882` — SUCCESS
- **iOS Simulator Tests**: run `37174680879` — SUCCESS
  - Simulator: iPhone 17 Pro
  - Runtime: iOS Simulator 26.5
  - Architecture: arm64
  - Full test suite: 33 tests, 0 failures
  - P017 tests compile and execute as part of `SynveilTests`, including real Rust-backed ABI compatibility error paths described in Section 5.

### Evidence discipline
This is the final manifest metadata correction. If this documentation-only commit retriggers PR workflows, final-head workflow evidence will be recorded in the PR discussion rather than by another manifest commit, avoiding an evidence-commit/concurrency loop.

## 9. PR / Merge State
- **PR**: #53 — https://github.com/nghianguyen150612/Synveil/pull/53
- **PR Base**: `ios-app`
- **Merge State at this manifest commit**: awaiting final-head verification
- **Final `ios-app` SHA**: assigned by GitHub at squash merge and recorded in PR merge metadata
