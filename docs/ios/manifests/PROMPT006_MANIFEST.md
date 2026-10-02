# Prompt006 Manifest — Synveil iOS Repository Layout

## Execution Metadata
- **Prompt Number**: `006`
- **Goal**: Create the canonical repository layout for the Synveil native iOS client (`clients/ios/`) without prematurely implementing Xcode project files or Swift source files.
- **Starting Integration Branch**: `ios-app`
- **Starting SHA**: `8fe8e6ec7fed5b7fa4a3a4fa2ff23bf79095f7a3`
- **Work Branch**: `ios/p006-repository-layout`

## Authoritative Inputs Inspected
- `docs/ios/IOS_ARCHITECTURE.md`
- `docs/ios/IOS_VALIDATION_CI_ARCHITECTURE.md`
- `docs/ios/IOS_SHARED_CORE_REUSE_AUDIT.md`
- `docs/ios/IOS_PLATFORM_MAPPING.md`
- `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`
- `docs/ios/IOS_V0_1_ROADMAP.md`
- `docs/adr/ADR-058-ios-v0.1-client-architecture.md`
- `docs/ios/manifests/PROMPT004_MANIFEST.md`
- `docs/ios/manifests/PROMPT005_MANIFEST.md`
- `clients/README.md`
- `clients/android/README.md`

## Repository Layouts Inspected
- `clients/` root convention
- `clients/android/` Gradle single-module structure
- Shared Rust workspace crates (`crates/`) layout

## Established Directory Layout
```text
clients/ios/
├── README.md                          # Main iOS layout & architecture specification
├── App/README.md                      # Entry point & composition root ownership
├── Features/README.md                 # SwiftUI presentation layer ownership
├── Application/README.md              # Use-case orchestration layer ownership
├── Domain/README.md                   # Pure domain entities & service protocols
├── Infrastructure/
│   ├── README.md                      # Platform implementations overview
│   ├── Network/README.md              # URLSession transport ownership
│   ├── Persistence/README.md          # GRDB SQLite local cache store ownership
│   ├── Security/README.md             # Keychain Vault credential storage ownership
│   ├── Files/README.md                # FileManager staging store ownership
│   ├── Transfers/README.md            # Transfer Engine implementation ownership
│   └── RustBridge/README.md           # Swift C-FFI Rust adapter ownership
├── Extensions/README.md               # Deferred FileProvider/PhotoKit extensions reservation
├── Resources/README.md                # Asset catalogs, String catalogs, Info.plist
├── Tests/README.md                    # Unit, Integration, Architecture tests & Mocks
└── Support/README.md                  # Repository scripts & reference configs
```

## Files Created / Modified
- `clients/ios/README.md` (Created)
- `clients/ios/App/README.md` (Created)
- `clients/ios/Features/README.md` (Created)
- `clients/ios/Application/README.md` (Created)
- `clients/ios/Domain/README.md` (Created)
- `clients/ios/Infrastructure/README.md` (Created)
- `clients/ios/Infrastructure/Network/README.md` (Created)
- `clients/ios/Infrastructure/Persistence/README.md` (Created)
- `clients/ios/Infrastructure/Security/README.md` (Created)
- `clients/ios/Infrastructure/Files/README.md` (Created)
- `clients/ios/Infrastructure/Transfers/README.md` (Created)
- `clients/ios/Infrastructure/RustBridge/README.md` (Created)
- `clients/ios/Extensions/README.md` (Created)
- `clients/ios/Resources/README.md` (Created)
- `clients/ios/Tests/README.md` (Created)
- `clients/ios/Support/README.md` (Created)
- `docs/ios/manifests/PROMPT006_MANIFEST.md` (Created)

## Commands Run
- `git fetch origin`
- `git checkout -b ios/p006-repository-layout origin/ios-app`
- `mkdir -p ...` (Created directory tree under `clients/ios/`)
- `cargo test -p synveil-core -p synveil-object-store`
- `python3 /home/jules/self_created_tools/validate_p006_layout.py`

## Validation Environment Statuses
- **Linux Validation (`LINUX_VERIFIABLE`)**: Passed locally on Ubuntu Linux. Automated python validation script confirmed directory structure, file absence invariants (zero `.swift`, `.xcodeproj`, `.xcworkspace`, `.h`, `.c`, or secret files), and documentation section completeness. Shared Rust crate unit tests passed cleanly.
- **macOS CI Validation (`MACOS_CI_VERIFIABLE`)**: Deferred to P007/P008 when Xcode project and dedicated macOS CI build workflows are established.
- **Simulator Validation (`SIMULATOR_VERIFIABLE`)**: Deferred to P009 when iOS Simulator test suite is created.
- **Physical-Device Validation (`PHYSICAL_DEVICE_ONLY`)**: Deferred per controlled evidence matrix.

## Non-Implementation Confirmations
- **Zero Swift Source Files**: No `.swift` files were introduced.
- **Zero Xcode Project Files**: No `.xcodeproj` or `.xcworkspace` files were introduced.
- **Zero FFI / C Header / Rust Modifications**: No C headers, Objective-C bridges, or Rust crate changes were made.
- **Zero Dependencies Added**: No Swift Package Manager, CocoaPods, Carthage, or XcodeGen configs were added.
- **Zero Secrets / Signing Material**: No certificates, keys, profiles, or bearer credentials were added.
- **Zero Unrelated Subsystem Changes**: Android, desktop, web, and server code remain completely untouched.

## Reserved Architectural Locations
- **Xcode Project Location for P007**: `clients/ios/Synveil.xcodeproj`.
- **Rust Bridge Location for P013–P016**: `clients/ios/Infrastructure/RustBridge/`.
- **File Provider Location for P053**: `clients/ios/Extensions/FileProvider/`.
- **PhotoKit Location for P056**: `clients/ios/Extensions/PhotoKit/`.

## Unrelated CI Check Failure Analysis
1. **Job**: `Build, reproduce, inspect, and smoke AppImage`
   - **Failure**: `[synveil-artifact] ERROR: private or temporary build path found in synveil-desktop` (`matched path marker: /home/`).
   - **Classification**: `UNRELATED_SUBSYSTEM_FAILURE` / `PRE_EXISTING_BASELINE_FAILURE`
   - **Analysis**: Pre-existing failure in Linux desktop AppImage artifact string inspection pipeline (`deploy/packages/common/reproducible.sh`). Prompt006 is strictly iOS repository structure and architectural documentation under `clients/ios/`. Zero Rust, CXX-Qt, or desktop packaging code was touched.
2. **Job**: `PG17 live suites (scheduling, worker, cycle, lifecycle, stress)`
   - **Failure**: `Process completed with exit code 101` in `.github/workflows/postgres-17.yml`.
   - **Classification**: `UNRELATED_SUBSYSTEM_FAILURE` / `PRE_EXISTING_BASELINE_FAILURE`
   - **Analysis**: Unrelated server PostgreSQL 17 test suite failure. Prompt006 touches zero server or PostgreSQL database code.

## Limitations & Deferrals
- This prompt establishes repository structure and documentation only.
- Xcode project bootstrap, Swift source compilation, FFI bridging, and CI workflow creation are deferred to downstream prompts P007–P016.

## PR & Merge Metadata
- **Logical Commit SHA**: `2e489ea1440ebbe785f3c93b07a6cc134422a517`
- **PR Target**: `ios-app`
- **PR Title**: `Synveil iOS Prompt006: Establish canonical client layout`
- **Merge Status**: Ready for submission / push
