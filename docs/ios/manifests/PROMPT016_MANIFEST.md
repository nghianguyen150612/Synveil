# Synveil iOS v0.1 — Prompt 016 Manifest

## Overview & Goal
Prompt 016 establishes the **first real compiled and executed Swift → C ABI → Rust → Swift call** for the Synveil iOS client (`v0.1`).

The initial C ABI capability is:
```c
uint32_t synveil_ffi_abi_version(void);
```
returning `1` (`SYNVEIL_FFI_ABI_VERSION`).

## Baseline Metadata
- **Prompt**: 016
- **Integration Branch Target**: `ios-app`
- **Starting SHA**: `e1d6b5bb463adada37d4f0687581f918b336e7cb`
- **Work Branch**: `ios/p016-first-rust-call-jules-3637492448085105526`
- **Validated Implementation Head**: `fbaca544892275ed36742888584f47a166455e58`
- **PR**: #52 — https://github.com/nghianguyen150612/Synveil/pull/52
- **PR Base**: `ios-app`
- **Rust Toolchain**: `rustc 1.94.0` (Edition 2024)

## Part A — Rust ABI Implementation
- **Constant**: `pub const SYNVEIL_FFI_ABI_VERSION: u32 = 1;`
- **Exported Symbol**: `synveil_ffi_abi_version`
- **Signature**: `pub extern "C" fn synveil_ffi_abi_version() -> u32`
- **Panic Firewall**: `std::panic::catch_unwind(|| SYNVEIL_FFI_ABI_VERSION).unwrap_or(0)`
- **Unsafe Lint Policy**: `#![deny(unsafe_code)]` with `#[allow(unsafe_code)]` / `#[unsafe(no_mangle)]` narrowly scoped to the ABI export function.
- **Rust Unit Tests**: Verified `SYNVEIL_FFI_ABI_VERSION == 1` and `synveil_ffi_abi_version() == 1`.

## Part B — Header Generation & cbindgen
- **cbindgen Version**: `0.28.0` (pinned)
- **cbindgen Config**: `crates/ios-ffi/cbindgen.toml`
- **Generation Script**: `scripts/generate-ios-rust-header.sh`
- **Generated Header**: `clients/ios/Infrastructure/RustBridge/Generated/synveil_ios_ffi.h`
- **Drift Verification**: Supported via `./scripts/generate-ios-rust-header.sh --check` in CI.

## Part C — Raw Clang Module
- **Module Name**: `SynveilRustFFI`
- **Modulemap Path**: `clients/ios/Infrastructure/RustBridge/Raw/module.modulemap`
- **Visibility**: Restricted exclusively to `clients/ios/Infrastructure/RustBridge/`.

## Part D — Static Source Validator
- **Validator Script**: `clients/ios/Support/validate_ios_sources.py`
- **Updated `RAW_FFI_MODULES`**: Includes `"SynveilRustFFI"`.
- **Self-Tests**: `clients/ios/Support/tests/test_validate_ios_sources.py` (verified allowed in `RustBridge` and rejected in `Domain`, `Application`, `Features`).

## Part E — Artifact Pipeline & Symbol Policy
- **Artifact Script**: `scripts/build-ios-rust-artifacts.sh`
- **Symbol Check**: Verifies `synveil_ffi_abi_version` exists and no unexpected `synveil_ffi_*` exports exist.
- **Header Staging**: `target/ios-rust-artifacts/include/synveil_ios_ffi.h`
- **Manifest Metadata**:
  - `"c_abi_export_status": "ABI_VERSION_ONLY_P016"`
  - `"c_abi_exports": ["synveil_ffi_abi_version"]`
  - `"cbindgen_status": "ACTIVE_P016"`
  - `"header_status": "GENERATED_CBINDGEN_P016"`
  - `"xcframework_status": "XCFRAMEWORK_NOT_REQUIRED_P016"`
- **Validator**: `scripts/validate_ios_rust_artifact.py` (verified schema v1, closed file set, checksums).

## Part F — Xcode & Swift Infrastructure
- **Xcode Project**: `clients/ios/Synveil.xcodeproj/project.pbxproj`
- **Linker Flags**: `OTHER_LDFLAGS = "-lsynveil_ios_ffi"`
- **Library Search Paths**:
  - `LIBRARY_SEARCH_PATHS[sdk=iphonesimulator*]` -> `$(SRCROOT)/../../target/ios-rust-artifacts/simulator/universal`, `$(SRCROOT)/../../target/ios-rust-artifacts/simulator/arm64`
  - `LIBRARY_SEARCH_PATHS[sdk=iphoneos*]` -> `$(SRCROOT)/../../target/ios-rust-artifacts/device/arm64`
- **Include Paths**: `SWIFT_INCLUDE_PATHS = "$(SRCROOT)/Infrastructure/RustBridge/Raw"`, `HEADER_SEARCH_PATHS = "$(SRCROOT)/Infrastructure/RustBridge/Generated"`
- **Swift Adapter**: `clients/ios/Infrastructure/RustBridge/RustBridgeAdapter.swift`
- **Error Type**: `RustBridgeCompatibilityError.unsupportedABIVersion`
- **RustBridgeProtocol Decision**: `RUST_BRIDGE_PROTOCOL_DEFERRED_TO_P020`
- **Preparation Script**: `scripts/prepare-ios-rust-xcode.sh`

## Part G — Simulator XCTest
- **XCTest File**: `clients/ios/Tests/SynveilTests/RustBridgeABITests.swift`
- **Expected Value**: `1`
- **Test Target**: `SynveilTests`

## Verification & Host Status
- `cargo check -p synveil-ios-ffi --locked`: PASSED
- `cargo test -p synveil-ios-ffi --locked`: PASSED
- `cargo clippy -p synveil-ios-ffi --all-targets --locked -- -D warnings`: PASSED
- `cargo fmt --all -- --check`: PASSED
- `python3 clients/ios/Support/validate_ios_sources.py`: PASSED
- `python3 -m unittest discover -s clients/ios/Support/tests`: PASSED
- `python3 -m unittest discover -s scripts/tests`: PASSED
- `./scripts/generate-ios-rust-header.sh --check`: PASSED
- `./scripts/build-ios-rust-artifacts.sh`: PASSED
- `python3 scripts/validate_ios_rust_artifact.py`: PASSED
- `git diff --check`: PASSED

## Required macOS CI Workflow Gates
Validated implementation head: `fbaca544892275ed36742888584f47a166455e58`

- `iOS Rust Apple Build`: SUCCESS — run `37169749423`, job `111340087850`
  - Uploaded artifact: `synveil-ios-rust-staticlibs`
  - Artifact ID: `11291515206`
  - Size: `29,525,819` bytes
  - Expired: false
  - Expires: `2026-10-18T02:08:30Z`
- `iOS Static Validation`: SUCCESS — run `37169749404`
- `iOS Build`: SUCCESS — run `37169749519`, Xcode Simulator Build job `111340088144`
- `iOS Simulator Tests`: SUCCESS — run `37169749433`, job `111340087716`
  - Simulator: iPhone 17 Pro, iOS Simulator 26.5, arm64
  - Full suite: 29 tests, 0 failures
  - `RustBridgeABITests`: 2 tests, 0 failures
  - `testRustBridgeABIVersionInitialization`: PASS; real Rust-backed adapter observed ABI v1

### Evidence-only correction rule
This manifest update changes documentation metadata only. Because the PR as a whole changes `clients/ios/**`, Rust bridge sources, and Apple-Rust workflow inputs, GitHub may rerun the required workflows for the new final PR head. If it does, all four required gates must pass again before merge. Final rerun evidence may be recorded in the PR discussion to avoid an infinite evidence-commit loop.

## Unrelated CI Classifications
- `Build, reproduce, inspect, and smoke AppImage`: Failed with `[synveil-artifact] ERROR: private or temporary build path found in synveil-desktop` (`matched path marker: /home/`). Pre-existing desktop build issue on `ios-app` branch unrelated to iOS Swift/Rust bridge work.


## PR / Merge State
- **PR**: #52 — https://github.com/nghianguyen150612/Synveil/pull/52
- **PR Base**: `ios-app`
- **Merge State at this manifest commit**: awaiting final-head verification
- **Final `ios-app` merge SHA**: assigned by GitHub at squash merge and recorded in PR merge metadata
