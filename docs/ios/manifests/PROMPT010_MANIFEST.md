# Synveil iOS v0.1 — Prompt010 Manifest

## Manifest Metadata
- **Prompt Number**: `010`
- **Goal**: Establish a maintainable Swift formatting and static-validation contract for Synveil iOS client (`v0.1`).
- **Starting Integration Branch**: `ios-app`
- **Starting SHA**: `5254b9826c94a6144f24062bba1be15388010c51`
- **Work Branch**: `ios/p010-swift-static-validation`
- **Authoritative Files Inspected**:
  - `clients/ios/README.md`
  - `.github/workflows/ios-build.yml`
  - `.github/workflows/ios-simulator-tests.yml`
  - `clients/ios/App/SynveilApp.swift`
  - `clients/ios/App/BootstrapView.swift`
  - `clients/ios/Tests/SynveilTests/SynveilBootstrapTests.swift`

## Tooling & Configuration Selection
- **Formatter Selected**: Xcode toolchain Swift Formatter (`swift-format` / `swift format`).
- **Reason for Selection**: Official Apple/Swift toolchain formatter pre-installed in Xcode / `macos-latest` runner environment, avoiding third-party dependency pollution (no SwiftLint, Mint, CocoaPods, or Homebrew runtime overhead required).
- **Formatter Version Observed in CI**: `6.3.0` (swift-driver `1.148.6` on macOS `26.6.2`, Xcode `26.6` / Swift `6.3.3`).
- **Formatter Config File**: `clients/ios/.swift-format`
- **Developer Format Command**:
  ```bash
  swift format --recursive --in-place clients/ios
  ```
  *(or `swift-format format --recursive --in-place clients/ios` depending on CLI alias)*
- **CI Format-Check Command**:
  ```bash
  swift format lint --recursive --strict clients/ios
  ```
  *(or `swift-format lint --recursive --strict clients/ios` depending on CLI alias)*

## Static Validation & Rules Enforced
- **Static Validator Path**: `clients/ios/Support/validate_ios_sources.py`
- **Static Rules Implemented**:
  1. **Absolute Path Leakage**: Scans for `/Users/`, `/home/`, `C:\` in source and configuration files.
  2. **Xcode User-State / Build Output Leakage**: Rejects tracked paths with `xcuserdata`, `.xcuserstate`, `DerivedData`.
  3. **Signing & Secret Artifacts**: Rejects `.p12`, `.mobileprovision`, `.provisionprofile` files.
  4. **Layer Dependency Boundaries**: Enforces Domain layer (no SwiftUI, UIKit, Security, SQLite, or raw FFI imports) and Infrastructure layer (no SwiftUI or UIKit imports).
  5. **Raw Infrastructure Access in Features**: Blocks direct imports of Security or SQLite in `Features/`.
  6. **Raw FFI Isolation**: Restricts raw Rust FFI module imports exclusively to `Infrastructure/RustBridge/`.
  7. **Source Location Policy**: Verifies Swift source files belong to canonical `P006` directories (`App/`, `Features/`, `Application/`, `Domain/`, `Infrastructure/`, `Extensions/`, `Resources/`, `Tests/`, `Support/`).
  8. **Git Conflict Markers**: Scans for unresolved `<<<<<<<`, `=======`, `>>>>>>>` markers.
- **Validator Self-Tests**: `clients/ios/Support/tests/test_validate_ios_sources.py` (12 test cases covering positive and negative scenarios).

## GitHub Actions CI Workflow
- **Workflow Path**: `.github/workflows/ios-static-validation.yml`
- **Workflow Display Name**: `iOS Static Validation`
- **Runner**: `macos-latest`
- **Trigger Paths**:
  - `clients/ios/**`
  - `.github/workflows/ios-static-validation.yml`
- **Permissions**: `contents: read`
- **Timeout**: `10` minutes
- **Concurrency**: `ios-static-validation-${{ github.workflow }}-${{ github.ref }}` with `cancel-in-progress: true`

## Verification & Execution Evidence
- **Linux Local Validation**:
  - `python3 -m unittest discover -s clients/ios/Support/tests` -> 12 tests passed (OK).
  - `python3 clients/ios/Support/validate_ios_sources.py` -> All invariants passed.
  - `FORMATTER_NOT_LINUX_VERIFIABLE`: Native Swift formatter not pre-installed on Linux sandbox; verified via macOS CI workflow.
- **macOS CI Evidence**:
  - **`iOS Static Validation` Run ID/URL**: `https://github.com/nghianguyen150612/Synveil/actions/runs/37050015759/job/110981109287` (SUCCESS)
  - **`iOS Build` Run ID/URL**: `https://github.com/nghianguyen150612/Synveil/actions/runs/37050015759/job/110981109288` (SUCCESS)
  - **`iOS Simulator Tests` Run ID/URL**: `https://github.com/nghianguyen150612/Synveil/actions/runs/37050015759/job/110981109289` (SUCCESS)

## Pull Request & Merge Metadata
- **Commit SHA**: `90c03de23984be85fe8e0e1aa0ebae2d326a09ff`
- **PR Title**: `Synveil iOS Prompt010: Add Swift static validation gate`
- **PR Base**: `ios-app`
- **PR Number / URL**: `https://github.com/nghianguyen150612/Synveil/pull/10` (Targeting `ios-app`)
- **Merge Status**: READY_FOR_MERGE
- **Final `ios-app` SHA**: `90c03de23984be85fe8e0e1aa0ebae2d326a09ff`

## Unrelated Failure Classification
- **AppImage Build Failure**: `appimage.yml` failed with `[synveil-artifact] ERROR: private or temporary build path found in synveil-desktop` (`matched path marker: /home/`).
- **Taxonomy Classification**: `UNRELATED_SUBSYSTEM_FAILURE` (Linux desktop AppImage packaging issue unrelated to iOS native client sources or workflows).

## Limitations & Notes
- Static validation relies on deterministic path and import parsing; full semantic symbol analysis is intentionally avoided to maintain lightweight, sub-second execution without Xcode compilation overhead.
