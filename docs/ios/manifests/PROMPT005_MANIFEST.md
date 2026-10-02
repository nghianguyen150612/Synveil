# Prompt005 Manifest — Synveil iOS Validation & CI Architecture

## Execution Metadata
- **Prompt Number**: `005`
- **Goal**: Establish the authoritative validation and CI architecture for Synveil iOS v0.1 across prompts P006–P060.
- **Starting Integration Branch**: `ios-app`
- **Starting SHA**: `0fa868e67a5182036976aed011fff3bd53b52a5f`
- **Work Branch**: `ios/p005-validation-ci-architecture`

## Authoritative Inputs Inspected
- `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`
- `docs/ios/IOS_V0_1_ROADMAP.md`
- `docs/ios/IOS_PLATFORM_MAPPING.md`
- `docs/ios/IOS_SHARED_CORE_REUSE_AUDIT.md`
- `docs/ios/IOS_ARCHITECTURE.md`
- `docs/adr/ADR-058-ios-v0.1-client-architecture.md`
- `docs/ios/PROMPT001_MANIFEST.md`
- `docs/ios/manifests/PROMPT002_MANIFEST.md`
- `docs/ios/manifests/PROMPT003_MANIFEST.md`
- `docs/ios/manifests/PROMPT004_MANIFEST.md`
- `.github/workflows/ci.yml`
- `.github/workflows/android.yml`
- `.github/workflows/appimage.yml`
- `.github/workflows/linux-packages.yml`
- `.github/workflows/postgres-17.yml`

## Workflows and Tests Inspected
- Inspections of existing GitHub Actions workflows confirmed that repository CI includes broad Linux, Windows, desktop Qt, and server tests.
- Shared Rust crate tests (`synveil-core`, `synveil-object-store`) run and pass locally on Ubuntu Linux.
- Android CI (`android.yml`) serves as a behavioral reference for host vs emulator/device gate separation.

## Files Created / Modified
- `docs/ios/IOS_VALIDATION_CI_ARCHITECTURE.md` (Created)
- `docs/ios/manifests/PROMPT005_MANIFEST.md` (Created)

## Commands Run
- `git fetch origin`
- `git checkout origin/ios-app -b ios/p005-validation-ci-architecture`
- `cargo test -p synveil-core -p synveil-object-store`
- `python3 /home/jules/self_created_tools/validate_p005_docs.py`

## Validation Environment Statuses
- **Linux Validation (`LINUX_VERIFIABLE`)**: Verified locally on Ubuntu Linux. Static documentation checks and shared Rust unit tests (`cargo test -p synveil-core -p synveil-object-store`) pass cleanly.
- **macOS CI Validation (`MACOS_CI_VERIFIABLE`)**: Deferred to P007/P008 when Xcode project and dedicated macOS CI build workflows are established.
- **Simulator Validation (`SIMULATOR_VERIFIABLE`)**: Deferred to P009 when iOS Simulator test suite is created.
- **Physical-Device Validation (`PHYSICAL_DEVICE_ONLY`)**: Deferred per controlled evidence matrix; mandatory prior to P060 release claim.
- **Signing-Required Validation (`SIGNING_REQUIRED`)**: Deferred to release packaging (P059/P060); CI pipelines operate unsigned-first without signing secrets.

## Architectural Decisions
- **No New ADR Created**: ADR-058 remains the authoritative architectural record. Prompt005 defines validation architecture consistent with ADR-058 without altering core design decisions.
- **Standardized Environment Taxonomy**: Fixed 5 labels (`LINUX_VERIFIABLE`, `MACOS_CI_VERIFIABLE`, `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY`, `SIGNING_REQUIRED`).
- **Standardized Failure Classification**: Fixed 6 classes (`IOS_CHANGE_CAUSED`, `PRE_EXISTING_BASELINE_FAILURE`, `UNRELATED_SUBSYSTEM_FAILURE`, `CI_INFRASTRUCTURE_FAILURE`, `FLAKY_OR_NONDETERMINISTIC`, `UNKNOWN_REQUIRES_INVESTIGATION`).

## Scope & Non-Implementation Confirmation
- **Zero Application Code Created**: No Swift files, SwiftUI views, Objective-C/C headers, or Rust FFI code were added.
- **Zero Xcode Files Created**: No `.xcodeproj` or `.xcworkspace` files were added.
- **Zero CI Workflow Modifications**: Existing GitHub Actions workflows were untouched. Dedicated iOS CI workflows remain deferred to P008/P009.
- **Zero Signing Secrets Introduced**: No certificates, keys, or profiles were added.

## Limitations & Deferrals
- Documentation and validation architecture specification only.
- Physical device testing relies on manual evidence logs until physical runner infrastructure is available.

## PR & Merge Status
- **Commit SHA**: `0e9d8a3d1669305034b9a3c64e5fb4f65126da6a`
- **PR Target**: `ios-app`
- **PR URL**: Pending creation
- **Merge Status**: Pending merge
