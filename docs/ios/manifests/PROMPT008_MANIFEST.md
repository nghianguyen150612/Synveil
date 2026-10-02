# Prompt008 Manifest — macOS CI Build Gate

## Execution Metadata
- **Prompt Number**: `008`
- **Goal**: Create a dedicated GitHub Actions workflow that validates the native Synveil iOS Xcode project using an official macOS/Xcode environment.
- **Starting Integration Branch**: `ios-app`
- **Starting SHA**: `cb0d42821abc82371689456e6c0f9b698eddafc7`
- **Work Branch**: `ios/p008-macos-build-ci-8659052179116251463`
- **Primary Implementation Commit**: `3167b7a225a686e23df3ebc8c48af2dc2c376a32`
- **PR**: #32 — https://github.com/nghianguyen150612/Synveil/pull/32
- **PR Target**: `ios-app`

## Authoritative Inputs Inspected
- `clients/ios/Synveil.xcodeproj/project.pbxproj`
- `clients/ios/Synveil.xcodeproj/xcshareddata/xcschemes/Synveil.xcscheme`
- `clients/ios/App/SynveilApp.swift`
- `clients/ios/App/BootstrapView.swift`
- `clients/ios/README.md`
- `docs/ios/IOS_VALIDATION_CI_ARCHITECTURE.md`
- `docs/ios/IOS_ARCHITECTURE.md`
- `docs/ios/IOS_V0_1_ROADMAP.md`
- `docs/adr/ADR-058-ios-v0.1-client-architecture.md`
- `docs/ios/manifests/PROMPT007_MANIFEST.md`
- `.github/workflows/**`

## Workflow Specification
- **Workflow File**: `.github/workflows/ios-build.yml`
- **Workflow Display Name**: `iOS Build`
- **Job Name**: `Xcode Simulator Build`
- **Runner Image**: `macos-latest`
- **Permissions**: `contents: read`
- **Concurrency Policy**: `group: ios-build-${{ github.workflow }}-${{ github.ref }}`, `cancel-in-progress: true`
- **Timeout**: 20 minutes
- **Path Triggers**:
  - `clients/ios/**`
  - `.github/workflows/ios-build.yml`
  - `workflow_dispatch` (manual execution)
- **Xcode Selection Strategy**: Uses the active Xcode selected on `macos-latest` after explicitly logging `sw_vers`, `xcodebuild -version`, `swift --version`, and `xcrun --sdk iphonesimulator --show-sdk-version`.

## Exact Build Command
```bash
xcodebuild \
  -project clients/ios/Synveil.xcodeproj \
  -scheme Synveil \
  -configuration Debug \
  -sdk iphonesimulator \
  CODE_SIGNING_ALLOWED=NO \
  -derivedDataPath "$RUNNER_TEMP/SynveilDerivedData" \
  build
```

## Linux Validation
- Validation tool `/home/jules/self_created_tools/validate_ios_build_workflow.py` executed cleanly.
- Verified path filtering, least-privilege permissions, environment logging commands, and exact xcodebuild flags.
- `cargo test -p synveil-core -p synveil-object-store` passed cleanly on Linux host.
- `git diff --check` clean.

## macOS CI Execution Evidence
- **Workflow Run**: `37040470487` — https://github.com/nghianguyen150612/Synveil/actions/runs/37040470487
- **Job**: `110949052298` — `Xcode Simulator Build`
- **Validated Commit**: `3167b7a225a686e23df3ebc8c48af2dc2c376a32`
- **Event**: `pull_request` for PR #32 targeting `ios-app`
- **GitHub Runner Label**: `macos-latest`
- **Resolved Runner Image**: `macos-26-arm64`, image version `20260907.0351.1`
- **Host OS**: macOS `26.6.2` (build `25G83`)
- **Xcode Version**: Xcode `26.6` (build `17F113`)
- **Swift Version**: Apple Swift `6.3.3` (`swiftlang-6.3.3.1.3 clang-2100.1.1.101`)
- **iOS Simulator SDK Version**: `26.5`
- **Build SDK Root**: `iphonesimulator26.5`
- **Code Signing**: disabled with `CODE_SIGNING_ALLOWED=NO`
- **Result**: **SUCCESS** — workflow and job both completed successfully; Xcode log ended with `** BUILD SUCCEEDED **`.
- **Architectures Built**: simulator `arm64` and `x86_64` universal app output.
- **Deployment Target Observed**: iOS `17.0`.

## P008 Build-Gate Conclusion
The dedicated `iOS Build` workflow successfully compiled the committed native Synveil Xcode project using GitHub-hosted Apple tooling. This closes P007's `MACOS_CI_VERIFICATION_PENDING_P008` validation gap.

The `iOS Build` workflow is the project-process build gate for future iOS implementation changes that match its path triggers. This statement does not claim GitHub branch-protection configuration.

## Scope / Limitations
- P008 performs compilation only; it does not boot an iOS Simulator or run tests.
- Simulator test execution is owned by P009.
- No signing certificates, provisioning profiles, Team IDs, App Store Connect credentials, or device-only validation were introduced.
- No product feature, networking, Keychain, persistence, transfer, or Rust FFI implementation was added.
- Existing non-iOS repository workflow failures remain classified separately under the P005 taxonomy and do not replace the dedicated iOS build result.

## Merge Status
- **PR #32**: open at evidence-recording time, targeting `ios-app`.
- **Merge Status**: pending final-head iOS Build verification and merge.
