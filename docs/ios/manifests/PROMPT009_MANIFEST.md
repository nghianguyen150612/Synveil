# Prompt009 Manifest — iOS Simulator Test Gate

## Execution Metadata
- **Prompt Number**: `009`
- **Goal**: Create a minimal native iOS unit test target (`SynveilTests`), XCTest bootstrap test, shared scheme test action, and dedicated GitHub Actions Simulator testing workflow.
- **Starting Integration Branch**: `ios-app`
- **Starting SHA**: `10cb665e01c3e267b7014112855636e67699b6a7`
- **Work Branch**: `ios/p009-simulator-test-gate`
- **PR**: Pending
- **PR Target**: `ios-app`
- **Merge Status**: Pending

## Authoritative Inputs Inspected
- `clients/ios/Synveil.xcodeproj/project.pbxproj`
- `clients/ios/Synveil.xcodeproj/xcshareddata/xcschemes/Synveil.xcscheme`
- `clients/ios/App/SynveilApp.swift`
- `clients/ios/App/BootstrapView.swift`
- `clients/ios/README.md`
- `.github/workflows/ios-build.yml`
- `docs/ios/IOS_VALIDATION_CI_ARCHITECTURE.md`
- `docs/ios/IOS_ARCHITECTURE.md`
- `docs/ios/IOS_V0_1_ROADMAP.md`
- `docs/ios/manifests/PROMPT008_MANIFEST.md`

## Codebase Modifications
### Files Created
- `clients/ios/Tests/SynveilTests/SynveilBootstrapTests.swift`
- `.github/workflows/ios-simulator-tests.yml`
- `docs/ios/manifests/PROMPT009_MANIFEST.md`

### Files Modified
- `clients/ios/Synveil.xcodeproj/project.pbxproj`
- `clients/ios/Synveil.xcodeproj/xcshareddata/xcschemes/Synveil.xcscheme`
- `clients/ios/README.md`

## Xcode Test Target Specification
- **Test Target Name**: `SynveilTests`
- **Target Type**: iOS Unit Testing Bundle (`com.apple.product-type.bundle.unit-test`)
- **Bundle Identifier**: `com.synveil.ios.tests`
- **Test Host**: `$(BUILT_PRODUCTS_DIR)/Synveil.app/$(BUNDLE_EXECUTABLE_FOLDER_PATH)/Synveil`
- **Test Source File**: `clients/ios/Tests/SynveilTests/SynveilBootstrapTests.swift`
- **Test Invariant**: `@testable import Synveil` linking check & `BootstrapView` instantiation.
- **Shared Scheme**: `clients/ios/Synveil.xcodeproj/xcshareddata/xcschemes/Synveil.xcscheme` updated `<TestAction>` with `<TestableReference>` pointing to `SynveilTests`.

## Workflow Specification
- **Workflow Path**: `.github/workflows/ios-simulator-tests.yml`
- **Workflow Display Name**: `iOS Simulator Tests`
- **Job Name**: `Xcode Simulator Unit Tests`
- **Runner Image**: `macos-latest`
- **Permissions**: `contents: read`
- **Concurrency Policy**: `group: ios-simulator-tests-${{ github.workflow }}-${{ github.ref }}`, `cancel-in-progress: true`
- **Timeout**: 25 minutes
- **Path Triggers**:
  - `clients/ios/**`
  - `.github/workflows/ios-simulator-tests.yml`
  - `workflow_dispatch`

## Dynamic Simulator Selection Strategy
1. Execute `xcrun simctl list devices available -j` to query available devices in JSON format.
2. Search available runtimes for iOS simulator runtimes sorted in reverse order.
3. Select the first available iPhone simulator device UDID.
4. Export `SIMULATOR_UDID`, `SIMULATOR_NAME`, and `SIMULATOR_RUNTIME` to `$GITHUB_ENV`.
5. Issue `xcrun simctl boot "$SIMULATOR_UDID"` and wait for readiness with `xcrun simctl bootstatus "$SIMULATOR_UDID" -b`.

## Exact Test Command
```bash
xcodebuild \
  -project clients/ios/Synveil.xcodeproj \
  -scheme Synveil \
  -configuration Debug \
  -destination "platform=iOS Simulator,id=$SIMULATOR_UDID" \
  CODE_SIGNING_ALLOWED=NO \
  -derivedDataPath "$RUNNER_TEMP/SynveilDerivedData" \
  -resultBundlePath "$RUNNER_TEMP/SynveilTests.xcresult" \
  test
```

## Result Bundle & Artifacts
- **Result Bundle Path**: `$RUNNER_TEMP/SynveilTests.xcresult`
- **Uploaded Artifact Name**: `ios-simulator-test-results`
- **Upload Condition**: `if: always()`
- **Retention Period**: 7 days

## Linux Validation Evidence
- `validate_pbxproj.py`: verified object definitions and references in `project.pbxproj`.
- `xml.etree.ElementTree`: verified XML syntax of `Synveil.xcscheme`.
- `pyyaml`: verified YAML syntax of `.github/workflows/ios-simulator-tests.yml`.
- `cargo test -p synveil-core -p synveil-object-store`: passed cleanly.
- `git diff --check`: clean.

## macOS CI Execution Evidence (To be populated after GitHub Actions run)
- **iOS Build Run**: Pending
- **iOS Simulator Tests Run**: Pending
- **Selected Simulator Model**: Pending
- **Selected Simulator Runtime**: Pending
- **Selected Simulator UDID**: Pending
- **Xcode Version**: Pending
- **Swift Version**: Pending
- **iOS Simulator SDK Version**: Pending
- **Executed / Passed Tests Count**: Pending

## Unrelated CI Classifications
- Non-iOS workflows remain classified under P005 taxonomy as `PRE_EXISTING_BASELINE_FAILURE` or `UNRELATED_SUBSYSTEM_FAILURE`.

## Scope / Limitations
- Prompt009 establishes unit test infrastructure and one bootstrap test.
- UI tests, snapshot tests, and third-party test dependencies (Quick/Nimble/SnapshotTesting) are excluded.
- Signing secrets, provisioning profiles, and physical device testing are excluded.
