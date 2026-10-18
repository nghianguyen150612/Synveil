# Prompt007 Manifest — Xcode Project Bootstrap

## Execution Metadata
- **Prompt Number**: `007`
- **Goal**: Create the minimal real native iOS Xcode project for Synveil v0.1 (`clients/ios/Synveil.xcodeproj`).
- **Starting Integration Branch**: `ios-app`
- **Starting SHA**: `dee78be56b3f3e03bb512da484e831187cee0d73`
- **Work Branch**: `ios/p007-xcode-bootstrap`

## Authoritative Inputs Inspected
- `clients/ios/README.md`
- Subdirectory `README.md` files under `clients/ios/`
- `docs/ios/IOS_ARCHITECTURE.md`
- `docs/ios/IOS_VALIDATION_CI_ARCHITECTURE.md`
- `docs/ios/IOS_SHARED_CORE_REUSE_AUDIT.md`
- `docs/ios/IOS_PLATFORM_MAPPING.md`
- `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`
- `docs/ios/IOS_V0_1_ROADMAP.md`
- `docs/adr/ADR-058-ios-v0.1-client-architecture.md`
- `docs/ios/manifests/PROMPT006_MANIFEST.md`
- `.github/workflows/ci.yml`

## Project & Configuration Overview
- **Xcode Project Path**: `clients/ios/Synveil.xcodeproj`
- **Application Target**: `Synveil`
- **Product Type**: Native iOS Application (`com.apple.product-type.application`)
- **Shared Scheme**: `clients/ios/Synveil.xcodeproj/xcshareddata/xcschemes/Synveil.xcscheme`
- **Bundle Identifier**: `com.synveil.ios`
- **Deployment Target**: iOS 17.0 (`IPHONEOS_DEPLOYMENT_TARGET = 17.0`)
- **Swift Strict Concurrency**: Complete (`SWIFT_STRICT_CONCURRENCY = complete`)
- **Info.plist Strategy**: Modern Xcode generated Info.plist (`GENERATE_INFOPLIST_FILE = YES`)
- **Build Configurations**: `Debug`, `Release`
- **Signing Policy**: Unsigned Simulator builds (`CODE_SIGNING_ALLOWED = NO`, `CODE_SIGN_IDENTITY = ""`, zero committed team IDs or provisioning profiles)

## Swift Source & Resources
- `clients/ios/App/SynveilApp.swift` (`@main App` entry point)
- `clients/ios/App/BootstrapView.swift` (Minimal placeholder bootstrap view)
- `clients/ios/Resources/Assets.xcassets/Contents.json`
- `clients/ios/Resources/Assets.xcassets/AppIcon.appiconset/Contents.json`
- `clients/ios/Resources/Assets.xcassets/AccentColor.colorset/Contents.json`

## Files Created / Modified
- `clients/ios/App/SynveilApp.swift` (Created)
- `clients/ios/App/BootstrapView.swift` (Created)
- `clients/ios/Resources/Assets.xcassets/Contents.json` (Created)
- `clients/ios/Resources/Assets.xcassets/AppIcon.appiconset/Contents.json` (Created)
- `clients/ios/Resources/Assets.xcassets/AccentColor.colorset/Contents.json` (Created)
- `clients/ios/Synveil.xcodeproj/project.pbxproj` (Created)
- `clients/ios/Synveil.xcodeproj/xcshareddata/xcschemes/Synveil.xcscheme` (Created)
- `clients/ios/README.md` (Modified)
- `docs/ios/manifests/PROMPT007_MANIFEST.md` (Created)

## Exact Build Command
```bash
xcodebuild \
  -project clients/ios/Synveil.xcodeproj \
  -scheme Synveil \
  -configuration Debug \
  -sdk iphonesimulator \
  CODE_SIGNING_ALLOWED=NO \
  build
```

## Commands Run
- `git fetch origin`
- `git checkout -b ios/p007-xcode-bootstrap origin/ios-app`
- `python3 /home/jules/self_created_tools/validate_p007_xcode_project.py`
- `cargo test -p synveil-core -p synveil-object-store`

## Validation Results
- **Linux Validation (`LINUX_VERIFIABLE`)**: Passed locally on Ubuntu Linux. Automated python validation script confirmed project structure, well-formed PBXProject syntax, shared scheme existence, relative file paths, bundle identifier (`com.synveil.ios`), deployment target (`17.0`), Swift strict concurrency setting, and absence of user state (`xcuserdata`), secrets, or Rust FFI headers. Shared Rust crate unit tests passed cleanly.
- **macOS / Xcode Validation (`MACOS_CI_VERIFIABLE`)**: `MACOS_CI_VERIFICATION_PENDING_P008` (Development environment is Linux; native `xcodebuild` execution requires macOS runners established in P008).
- **Simulator Validation (`SIMULATOR_VERIFIABLE`)**: Deferred to P009.
- **Physical-Device Validation (`PHYSICAL_DEVICE_ONLY`)**: Deferred per controlled evidence matrix.

## CI Failure Classifications
1. **Job**: `Build, reproduce, inspect, and smoke AppImage` (`appimage.yml`)
   - **Failure**: `[synveil-artifact] ERROR: private or temporary build path found in synveil-desktop` (`matched path marker: /home/`)
   - **Classification**: `UNRELATED_SUBSYSTEM_FAILURE` / `PRE_EXISTING_BASELINE_FAILURE`
   - **Analysis**: Pre-existing failure in Linux desktop AppImage binary path inspection script (`scripts/validate-appimage-build.py`). Prompt007 modifies zero Rust, desktop CXX-Qt, or AppImage packaging code under `crates/desktop` or `deploy/`. All Prompt007 changes are isolated to `clients/ios/` and `docs/ios/`.

2. **Job**: `Native desktop UI (Windows Qt 6)` (`ci.yml`)
   - **Failure**: `cannot find unix in os` / `cannot find unistd in nix` in `crates/install-engine/src/appimage.rs`
   - **Classification**: `UNRELATED_SUBSYSTEM_FAILURE` / `PRE_EXISTING_BASELINE_FAILURE`
   - **Analysis**: Pre-existing Windows compilation issue in non-mobile install engine crate (`synveil-install-engine`). Prompt007 touches zero Rust or Windows install engine code.

3. **Job**: `Native desktop UI (Linux Qt 6)` (`ci.yml`)
   - **Failure**: Exit code 255 during QML type resolution in desktop UI suite
   - **Classification**: `UNRELATED_SUBSYSTEM_FAILURE` / `PRE_EXISTING_BASELINE_FAILURE`
   - **Analysis**: Pre-existing Linux desktop CXX-Qt QML type resolution issue. Prompt007 touches zero CXX-Qt or desktop code.

4. **Job**: `Check (windows-latest)` / `Test (windows-latest)` (`ci.yml`)
   - **Failure**: `cannot find unix in os` / `cannot find unistd in nix` compiling `synveil-install-engine`
   - **Classification**: `UNRELATED_SUBSYSTEM_FAILURE` / `PRE_EXISTING_BASELINE_FAILURE`
   - **Analysis**: Pre-existing Windows compilation issue in non-mobile install engine crate. Prompt007 touches zero Rust or install engine code.

5. **Job**: `Test (ubuntu-latest)` (`ci.yml`)
   - **Failure**: `systemd-tmpfiles dry-run failed: unrecognized option '--dry-run'` in `sysusers_tmpfiles_artifacts_match_authoritative_sources`
   - **Classification**: `UNRELATED_SUBSYSTEM_FAILURE` / `PRE_EXISTING_BASELINE_FAILURE`
   - **Analysis**: Pre-existing Linux systemd tmpfiles test failure on Ubuntu 24.04 runners. Prompt007 touches zero Linux installer or metadata code.

6. **Job**: `Test (macos-latest)` (`ci.yml`)
   - **Failure**: `UnsafeEndpoint` failure in desktop client control tests
   - **Classification**: `UNRELATED_SUBSYSTEM_FAILURE` / `PRE_EXISTING_BASELINE_FAILURE`
   - **Analysis**: Pre-existing desktop client IPC control test issue on macOS runners. Prompt007 touches zero desktop IPC or Rust client code.

## Non-Implementation Confirmations
- **Zero Rust FFI Integration**: No C headers, bridging headers, XCFrameworks, or Rust FFI adapters were introduced (deferred to P013+).
- **Zero Feature Architecture**: No onboarding, auth, navigation shell, transfers, persistence, or network services were implemented (deferred to P021+).
- **Zero Personal Signing Data**: No Apple Team IDs, provisioning profiles, or developer certificates were committed.
- **Zero External Dependency Managers**: No CocoaPods, Carthage, Tuist, or XcodeGen were introduced.

## PR & Merge Status
- **PR Target**: `ios-app`
- **PR Title**: `Synveil iOS Prompt007: Bootstrap native Xcode project`
