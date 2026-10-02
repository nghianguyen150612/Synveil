# Prompt008 Manifest — macOS CI Build Gate

## Execution Metadata
- **Prompt Number**: `008`
- **Goal**: Create a dedicated GitHub Actions workflow that validates the native Synveil iOS Xcode project using an official macOS/Xcode environment.
- **Starting Integration Branch**: `ios-app`
- **Starting SHA**: `cb0d42821abc82371689456e6c0f9b698eddafc7`
- **Work Branch**: `ios/p008-macos-build-ci`

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
- **Runner Image**: `macos-latest`
- **Permissions**: `contents: read`
- **Concurrency Policy**: `group: ios-build-${{ github.workflow }}-${{ github.ref }}`, `cancel-in-progress: true`
- **Timeout**: 20 minutes
- **Path Triggers**:
  - `clients/ios/**`
  - `.github/workflows/ios-build.yml`
  - `workflow_dispatch` (manual execution)
- **Xcode Selection Strategy**: Uses default active Xcode on `macos-latest` after explicitly logging `sw_vers`, `xcodebuild -version`, `swift --version`, and `xcrun --sdk iphonesimulator --show-sdk-version`.

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
- **Workflow Path**: `.github/workflows/ios-build.yml`
- **Workflow Run ID / URL**: PENDING_PR_TRIGGER
- **Host Runner**: PENDING_PR_TRIGGER
- **Xcode Version**: PENDING_PR_TRIGGER
- **Swift Version**: PENDING_PR_TRIGGER
- **iOS Simulator SDK Version**: PENDING_PR_TRIGGER
- **Build Outcome**: PENDING_PR_TRIGGER

*(Note: Pending update after PR creation and CI execution)*
