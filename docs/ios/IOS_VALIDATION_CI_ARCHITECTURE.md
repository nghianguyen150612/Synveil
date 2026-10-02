# Synveil iOS v0.1 — Validation & CI Architecture

This document defines the authoritative validation and Continuous Integration (CI) architecture for the native Synveil iOS v0.1 client across prompts P006–P060.

---

## 1. Purpose

The purpose of this architecture is to establish a rigorous, deterministic, and scalable framework for proving the correctness, performance, security, and platform parity of the Synveil iOS v0.1 client.

This architecture governs:
- How developer environments (specifically Linux-based environments like Jules) interact with macOS-based Xcode build and test pipelines.
- How validation responsibilities are partitioned across Linux, macOS CI, iOS Simulator, physical devices, and code-signing boundaries.
- How test quality and signal accuracy are preserved without making CI fragile or dependent on private signing secrets.
- How failures in unrelated parts of the repository (e.g., Linux desktop, Windows installers, web apps, or server components) are classified without masking actual iOS regressions or introducing false positives.

---

## 2. Validation Principles

1. **Explicit Environment Awareness**: Code and tests must explicitly declare their required execution environment. Linux host validation must never be claimed as proof that native Apple Swift code or iOS SDK targets compile.
2. **Unsigned-First CI**: Automated CI pipelines must operate without requiring private Apple Developer signing certificates or provisioning profiles. Signing is restricted strictly to release packaging and physical-device validation.
3. **Decoupled Architecture Verification**: Presentation (SwiftUI), Application Services, Infrastructure/Networking (`URLSession`), Storage (`Keychain`/SQLite), and Domain Logic (Shared Rust via FFI) must be testable in isolation using fakes, mocks, and synthetic transports. SwiftUI view lifetimes must not gate background operations or transfer engines.
4. **Progressive Evidence Hierarchy**: Validation moves systematically from low-cost, highly deterministic static and unit tests (Tier 0–1) up through integration, simulator, physical-device, and signed release tiers (Tier 2–6).
5. **No Concealment of Device Gaps**: Features that depend on physical hardware capabilities (e.g., Secure Enclave, physical background refresh, PhotoKit asset library access, or live File Provider extension sync) must maintain an explicit physical-device evidence matrix rather than fabricating simulator passes.
6. **Strict Signal Isolation**: iOS PR validation must provide clear, actionable signal quality. Failures in unrelated desktop or server subsystems must be systematically categorized with baseline evidence rather than ignored or silently suppressed.

---

## 3. Environment Taxonomy

To avoid ambiguity across documentation, CI job definitions, and prompt deliverables, all validation steps must use exactly these five environment labels:

| Label | Definition & Scope |
| :--- | :--- |
| `LINUX_VERIFIABLE` | Tasks, tests, or static checks that execute natively on an x86_64/AArch64 Ubuntu Linux environment (e.g., Jules developer container or Linux GitHub Actions runner). Includes Rust domain unit tests (`cargo test`), static repository structure inspection, documentation contract verification, JSON/YAML schema validation, and forbidden dependency scanning. |
| `MACOS_CI_VERIFIABLE` | Tasks or builds requiring macOS host capabilities and official Apple command-line tools (`xcodebuild`, `swiftc`, `xcrun`, `simctl`, `swift-driver`). Includes Swift package resolution, native iOS SDK compilation, Swift unit tests, Swift ↔ Rust FFI bridge binding compilation, and macOS host-side integration tests. |
| `SIMULATOR_VERIFIABLE` | Headless or interactive UI, lifecycle, and functional tests executing inside an Apple iOS Simulator instance (`iphonesimulator` SDK) hosted on macOS CI runners or macOS developer workstations. Covers SwiftUI view rendering, navigation flows, mock-server networking, in-sandbox file operations, and functional Keychain API operations. |
| `PHYSICAL_DEVICE_ONLY` | Capabilities or scenarios that strictly require physical iPhone/iPad hardware running iOS 17.0+. Includes Secure Enclave key generation, real `BGTaskScheduler` background execution, physical cellular/Wi-Fi network interface transitions, PhotoKit library authorization/scanning, File Provider extension daemon integration, and real storage pressure/thermal throttling. |
| `SIGNING_REQUIRED` | Tasks or artifacts requiring Apple Developer Team provisioning profiles, code-signing identities (Development, AdHoc, or Apple Distribution), and entitlement matching. Includes physical device deployment, App Store / TestFlight archive creation (`xcodebuild archive`), App Group shared sandbox provisioning, and File Provider extension provisioning. |

---

## 4. Test / Evidence Tiers

Validation progress is organized into seven explicit, sequential tiers:

```text
Tier 0: Static / Source Validation         (LINUX_VERIFIABLE)
  │
  ▼
Tier 1: Shared Core Rust Unit Tests        (LINUX_VERIFIABLE & MACOS_CI_VERIFIABLE)
  │
  ▼
Tier 2: Swift Domain & Unit Tests          (MACOS_CI_VERIFIABLE)
  │
  ▼
Tier 3: Swift / Rust & Network Integration (MACOS_CI_VERIFIABLE)
  │
  ▼
Tier 4: iOS Simulator UI & Lifecycle Tests (SIMULATOR_VERIFIABLE)
  │
  ▼
Tier 5: Physical-Device Hardware Validation(PHYSICAL_DEVICE_ONLY)
  │
  ▼
Tier 6: Signed Distribution & Archive      (SIGNING_REQUIRED)
```

### Tier 0 — Static / Source Validation (`LINUX_VERIFIABLE`)
- **Focus**: Repository layout, naming conventions, manifest consistency, architecture boundaries, secret scanning, forbidden imports (e.g., ensuring generic Rust crates do not import `UIKit` or `Security`), and contract documentation verification.
- **Execution**: Linux local developer shell and Linux CI host.

### Tier 1 — Shared Core Unit Tests (`LINUX_VERIFIABLE` & `MACOS_CI_VERIFIABLE`)
- **Focus**: Pure Rust domain logic in `synveil-core`, `synveil-object-store`, and non-I/O portions of `synveil-client-sync`. Verifies identifier generation (UUIDv7), token parsing (`sve1_`, `svd1_`), cryptographic hashing (SHA-256), change feed evaluation, and conflict resolution semantics.
- **Execution**: `cargo test -p synveil-core -p synveil-object-store`.

### Tier 2 — Swift Unit Tests (`MACOS_CI_VERIFIABLE`)
- **Focus**: Native Swift domain models, error translation, state machines, application view-models (`@MainActor`), local persistence logic (`GRDB` / SQLite), transfer queue state transitions, and `URLSession` request builders.
- **Execution**: `xcodebuild test` using mock dependencies on macOS CI.

### Tier 3 — Swift / Rust & Network Integration (`MACOS_CI_VERIFIABLE`)
- **Focus**: Swift ↔ Rust C-FFI / UniFFI boundary integration, memory allocation/deallocation balance across FFI, async Swift wrappers, `URLSession` communication against mock/local HTTP servers (`httptest` or Swift-native mock server), TLS custom trust evaluation, and transfer resume mechanics.
- **Execution**: Swift SPM / Xcode integration test target on macOS CI.

### Tier 4 — Simulator Tests (`SIMULATOR_VERIFIABLE`)
- **Focus**: End-to-end app launch, SwiftUI navigation stack stability, localized string dynamic type accessibility, functional Keychain credential storage and retrieval in simulator sandbox, mock server authentication flows, file browser directory rendering, and simulated app lifecycle state changes (foreground ↔ background suspension).
- **Execution**: `xcodebuild test -destination 'platform=iOS Simulator,name=iPhone 15,OS=17.2'` on macOS CI.

### Tier 5 — Physical-Device Validation (`PHYSICAL_DEVICE_ONLY`)
- **Focus**: Hardware Secure Enclave properties, real iOS background task scheduling (`BGAppRefreshTask`), File Provider extension system process registration, PhotoKit permission prompts and full photo library scanning, live cellular/Wi-Fi transitions, and large transfer (multi-gigabyte) performance under real iOS memory pressure.
- **Execution**: Manual or automated test runner connected to physical iPhone/iPad running iOS 17.0+.

### Tier 6 — Signed / Distribution Validation (`SIGNING_REQUIRED`)
- **Focus**: Release archive packaging (`.ipa`), entitlement verification, provisioned App Group container binding, TestFlight deployment, and symbol stripping/DSYM export.
- **Execution**: `xcodebuild archive` on dedicated release build worker with Apple Developer credentials.

---

## 5. Dedicated iOS CI Strategy

To keep iOS validation clean, reliable, and independent of desktop/server noise, dedicated iOS workflows will be introduced in Phase B (P007–P009) and expanded through Phase H (P059–P060).

```text
.github/workflows/ios.yml
├── Job 1: Static & Core Gates (Linux)
├── Job 2: Rust Apple Target Compiles (macOS)
├── Job 3: Swift Unit & FFI Tests (macOS)
└── Job 4: iOS Simulator UI & Lifecycle Tests (macOS)
```

### Future Conceptual Workflows & Ownership

1. **iOS Build Workflow (`ios-build.yml` — Owned by P008)**:
   - Triggers: Push/PR touching `clients/ios/**`, `crates/**`, or `.github/workflows/ios*`.
   - Actions: Checkout repository, select and log Xcode version, resolve Swift package dependencies, compile Rust Apple target artifacts (`aarch64-apple-ios-sim`), run `xcodebuild build` for project schemes, and capture derived data log output.

2. **iOS Simulator Test Workflow (`ios-simulator-tests.yml` — Owned by P009)**:
   - Triggers: Push/PR targeting `ios-app`.
   - Actions: Boot designated iOS Simulator device, run `xcodebuild test` for unit and UI test targets, capture `.xcresult` test result bundles, extract test logs, and record failure diagnostics.

3. **Rust Apple-Target Validation (`ios-rust-apple.yml` — Evolution in P014–P016)**:
   - Triggers: Changes to shared Rust crates (`crates/core`, `crates/object-store`, `crates/client-sync`, `clients/ios/rust-bridge`).
   - Actions: Verify Rust compilation against `aarch64-apple-ios` (physical device target) and `aarch64-apple-ios-sim` / `x86_64-apple-ios` (simulator targets) using `cargo check --target` or `cargo build`.

4. **FFI Integration Workflow (`ios-ffi-tests.yml` — Evolution in P016–P020)**:
   - Triggers: FFI header changes or Rust bridge modifications.
   - Actions: Generate UniFFI/C-FFI Swift bindings, compile combined Swift + Rust static archive, run Swift integration unit tests verifying memory safety, string/byte buffer conversion, panic safety, and async concurrency rules.

5. **Release Validation Workflow (`ios-release-validation.yml` — Evolution in P059–P060)**:
   - Triggers: Release tags or manual `workflow_dispatch`.
   - Actions: Aggregate evidence from static, unit, FFI, and simulator test runs, perform unsigned release packaging, verify symbol maps, and generate `SYNVEIL-IOS-RELEASE-MANIFEST.json`.

---

## 6. Linux Validation Role

Ubuntu Linux is the primary interactive development environment (e.g., Jules container). Linux plays a vital role in Tier 0 and Tier 1 validation but cannot validate Apple SDK compilation.

### Permitted Linux Responsibilities:
- Running `cargo test -p synveil-core -p synveil-object-store` for shared domain logic.
- Verifying JSON, YAML, and Markdown documentation contracts.
- Executing custom Python or Bash validation scripts (`validate_p005_docs.py`, manifest checks).
- Performing static code analysis, `cargo fmt`, `cargo clippy`, and license audits.
- Validating API/schema contracts against backend OpenAPI or JSON schema definitions.

### Explicit Linux Non-Responsibilities:
- Linux **must never** be cited as proof that Swift code compiles.
- Linux **must never** be cited as proof that `xcodebuild` succeeds.
- Linux **must never** be cited as proof that iOS Simulator or Keychain APIs behave correctly.

---

## 7. macOS / Xcode CI Role & Runner Policy

The macOS CI environment provides the definitive compiler, linker, and simulator toolchains for Apple platforms.

### Runner Environment Policy
- **Runner Image**: `macos-14` (Apple Silicon M1/M2 runners) or `macos-15` as made available by GitHub Actions.
- **Toolchain Logging**: To prevent opaque environment drift, every iOS CI job **must** log the effective runtime environment at start:
  ```bash
  echo "=== Environment Info ==="
  sw_vers
  xcodebuild -version
  swift --version
  rustc --version
  xcrun simctl list runtimes
  xcrun simctl list devices available
  ```
- **Pinning Strategy**:
  - Xcode version: Pinned explicitly via `DEVELOPER_DIR` or `xcrun xcode-select` (e.g., Xcode 15.3+ / 16.x).
  - iOS Deployment Target: Fixed at iOS **17.0+** in build settings.
  - Rust Toolchain: Fixed via `rust-toolchain.toml` or `dtolnay/rust-toolchain@stable` with `aarch64-apple-ios-sim` target installed.

---

## 8. Simulator Strategy (Capabilities vs Limitations)

The iOS Simulator is a high-fidelity x86_64/AArch64 Darwin process execution environment. It provides fast, deterministic automated feedback for UI and application logic.

### Proven in Simulator (`SIMULATOR_VERIFIABLE`):
- SwiftUI rendering, dynamic type layout, navigation stack transitions, and `@MainActor` state updates.
- Authentication enrollment flows, bearer token headers, and logout handling.
- `URLSession` HTTP/HTTPS networking against mock endpoints or local test servers.
- `KeychainServices` basic storage, query, and deletion operations (using simulator software Keychain).
- Local SQLite database migrations, transactions, and metadata indexing (`GRDB`).
- File creation, renaming, moving, and deletion inside the app sandbox (`Documents/`, `Caches/`).
- Transfer engine queue management, chunk staging, and progress calculation.
- Basic background suspension/resume application lifecycle events (`sceneDidEnterBackground`).

### Simulator Limitations (Requires `PHYSICAL_DEVICE_ONLY`):
- **Secure Enclave**: Hardware key generation (`kSecAttrAccessControl` with biometric flags) behaves differently or falls back to software in Simulator.
- **Real Background Scheduling**: `BGAppRefreshTask` and `BGProcessingTask` execution depends on iOS system discretion, battery level, and thermal state; Simulator only supports forced manual trigger via `e -l objc -- (void)[[BGTaskScheduler sharedScheduler] _simulateLaunchForTaskWithIdentifier:...]`.
- **PhotoKit Asset Library**: Simulator contains synthetic sample media; real photo library performance, iCloud photo sync state, and photo authorization dialogs require physical device testing.
- **File Provider Extension Daemon**: The live `NSFileProviderReenumerateWithCompletionHandler` sync engine integration with iOS `Files.app` has system-daemon quirks in Simulator.
- **Cellular / Network Switching**: Simulator shares host Mac network interfaces; cannot simulate real LTE/5G cellular dropouts or captive portal redirections naturally.

---

## 9. Physical Device Strategy

For capabilities that cannot be conclusively validated in CI or Simulator, a **Controlled Evidence Matrix** must be maintained throughout development.

### Controlled Physical Device Matrix

| Feature | Reason Device Required | Minimum Evidence | Pre-Merge Req | Pre-P060 Req | Result Artifact |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Secure Enclave Keys** | Hardware isolation & biometric authorization | Verified Keychain hardware flag execution | No (Deferred) | Yes | Device Test Log / Screenshot |
| **Background Refresh (`BGTaskScheduler`)** | Real iOS power/thermal execution scheduling | Device execution log showing background wake & transfer | No (Deferred) | Yes | Device Console Log (`.logarchive`) |
| **PhotoKit Library Sync** | Access to real user media database & iCloud photo state | Photo library scan timing & change observer log | No (Deferred) | Yes | Test Output Log |
| **File Provider Extension** | Live integration with system `Files.app` daemon | Files.app directory browse & upload video capture | No (Deferred) | Yes | Screen Recording (`.mp4`/`.webm`) |
| **Network Interface Transition** | Switching between Wi-Fi and LTE mid-transfer | Resumable upload completion after network swap log | No (Deferred) | Yes | Transfer Engine Audit Log |
| **Large File Thermal / Storage Pressure** | 5 GB transfer under low disk space conditions | Memory profile trace & staging cleanup verification | No (Deferred) | Yes | Xcode Instruments Trace (`.trace`) |

*Rule*: Physical-device gaps during P006–P058 may be temporarily deferred provided they are recorded in the prompt manifest, but **all physical device gates become mandatory before P060 release sign-off**.

---

## 10. Signing Strategy

To ensure CI security and avoid fragile configuration:

1. **Unsigned Development & CI Pipeline**:
   - All standard pull requests and CI workflows compile using `CODE_SIGNING_ALLOWED=NO` or automatic Simulator signing (`CODE_SIGN_IDENTITY=""`, `AD_HOC` code signing for simulator).
   - No private Apple Developer certificates, private keys, or provisioning profiles are stored in PR CI secrets.
2. **Signing-Required Pipeline (`SIGNING_REQUIRED`)**:
   - Signing credentials (`APPLE_DEVELOPER_CERTIFICATE_P12`, `PROVISIONING_PROFILE_BASE64`) are used **only** in dedicated release/device workflows (`ios-release-validation.yml`).
   - Signing secrets are scoped exclusively to protected branches (`ios-app`, release tags) and protected environments.
   - Entitlements (App Groups, Keychain Access Groups, File Provider extension entitlements) are verified against the provisioned profile during release packaging.

---

## 11. Rust Validation Strategy

Shared Rust code provides pure domain rules and cross-platform sync algorithms.

```text
Host Linux Unit Tests  ---> Apple Target Compiles  ---> FFI C-Header / UniFFI  ---> Swift Consumer Tests
(`cargo test` - Tier 1)   (`cargo check` - Tier 1)   (Binding Check - Tier 2)     (`xcodebuild` - Tier 3)
```

1. **Host Linux Tests (`synveil-core`, `synveil-object-store`)**:
   - Run via `cargo test` on every PR.
   - Validates deterministic UUIDv7 generation, SHA-256 block hashing, token string regex rules, and change journal conflict rules.
2. **Apple-Target Compilation**:
   - Validates that Rust crates build cleanly against `aarch64-apple-ios` and `aarch64-apple-ios-sim` targets without relying on C libraries missing on iOS.
3. **FFI Layer Testing**:
   - Validates C ABI compatibility, `extern "C"` functions, UniFFI interface definition language (IDL) files, and panic handling (`std::panic::catch_unwind`).
4. **Swift Consumer Tests**:
   - Swift imports compiled Rust static library (`libsynveil_ffi.a` or `.xcframework`) and executes Swift unit tests exercising the wrapper structs.

---

## 12. Swift Validation Strategy

Native Swift presentation, services, and infrastructure are validated using XCTest / Swift Testing framework.

- **SwiftUI View Models**: Tested as `@MainActor` isolated classes using fake service dependencies. Views must be state-driven functions; view-models must expose observable state properties (`@Published` / `@Observable`).
- **Service Layer**: State machines (e.g., `NodeBrowser`, `AuthViewModel`, `SyncEngine`) tested using async/await assertions with mock network transports and fake storage repositories.
- **Error Handling**: Every Swift service must implement error mapping converting network/FFI/storage errors into user-facing domain errors (`SynveilError`). Unit tests must explicitly check each error branch.

---

## 13. FFI Validation Architecture

The Swift ↔ Rust FFI bridge (established in P013–P020) requires specialized safety validation:

1. **Memory Allocation & Lifetime**: Verification that every buffer or string allocated by Rust and passed to Swift is explicitly freed by calling Rust deallocation functions (`synveil_free_string`, `synveil_free_bytes`).
2. **Panic Containment**: FFI functions must catch Rust panics at the C boundary and convert them into explicit error return codes. Panics must never cross the FFI boundary into Swift, as doing so causes an unrecoverable process crash.
3. **Concurrency & Thread Safety**: FFI calls from Swift must be thread-safe. Long-running Rust calculations must run on background dispatch queues or Swift detached tasks (`Task.detached`), never blocking `@MainActor`.
4. **String Encoding**: Strict UTF-8 validation across C string boundaries.

---

## 14. Networking Validation

iOS networking relies natively on Apple `URLSession`. Validation requires:

- **Malformed Server URL Handling**: Input validation for user-entered origins (e.g., rejecting missing schemes, invalid ports, or trailing slash issues).
- **TLS Policy Verification**: Testing custom server trust evaluation (`URLSessionDelegate` handling `serverTrust`) for self-signed certificates or private CAs, while maintaining hostname validation.
- **Error Mapping Matrix**:
  - `401 Unauthorized` → Session invalidation & authentication prompt.
  - `403 Forbidden` → Access denied error mapping.
  - `404 Not Found` → Node/resource missing error.
  - `409 Conflict` → Sync / transfer conflict signal.
  - `413 Payload Too Large` → Chunk size re-segmentation or upload rejection.
  - `500/502/503` → Bounded backoff retry rule validation.
- **Cancellation & Timeout**: Proving that cancelling a Swift `Task` or calling `URLSessionTask.cancel()` immediately terminates in-flight sockets and cleans up temporary staging files.

---

## 15. Authentication Validation

Authentication security is paramount. Tests must verify:

1. **Enrollment Exchange (`sve1_` → `svd1_`)**: Mock HTTP exchange sending enrollment key `sve1_...` and receiving device bearer token `svd1_...`.
2. **Keychain Vault Storage**: Verifying token storage using Security framework (`kSecClassGenericPassword`) with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`.
3. **Secret Non-Leakage Audit**:
   - Automated grep / assertion checking that `svd1_` bearer tokens never appear in `UserDefaults`, SQLite database tables, application log streams (`os_log` / `Logger`), or SwiftUI view debug descriptions.
4. **Logout & Device Clean Removal**: Invoking "Forget Device" must purge Keychain items, clear local SQLite caches, delete staging files, and reset UI state to onboarding.

---

## 16. File Operation Validation

File browser operations (`NodeBrowser`) require authoritative server acknowledgement for remote node mutations:

- **Folder Creation / Rename / Move / Delete**: Verified via mock server endpoints and local SQLite state updates.
- **Stale State Handling**: Directory listings must gracefully handle server modification where a node was deleted remotely.
- **Offline Queuing & Replay**: Metadata mutations performed offline must enqueue locally and replay upon network restoration with conflict handling.

---

## 17. Transfer Validation

The central Transfer Engine operates independently of SwiftUI view lifetimes.

### Transfer Test Matrix

| Test Scenario | Goal & Assertion | Environment |
| :--- | :--- | :--- |
| **Small File Upload / Download** | Single-shot streaming request completion | `SIMULATOR_VERIFIABLE` |
| **4 MiB Chunked Upload** | Resumable upload chunking, offset tracking, and final manifest commit | `SIMULATOR_VERIFIABLE` |
| **Progress Reporting** | Smooth `Progress` updates sent to UI without blocking main thread | `SIMULATOR_VERIFIABLE` |
| **Transfer Cancellation** | Intermediate staging files removed immediately upon user cancel | `SIMULATOR_VERIFIABLE` |
| **Interrupted Transfer Resume** | Upload resumes from last acknowledged 4 MiB offset after simulated connection drops | `SIMULATOR_VERIFIABLE` |
| **App Background Transition** | Background `URLSessionUploadTask` / `URLSessionDownloadTask` completes when app enters background | `PHYSICAL_DEVICE_ONLY` |
| **Storage Pressure Protection** | Transfer fails gracefully with `insufficientStorage` error when free space is under threshold | `SIMULATOR_VERIFIABLE` |
| **Checksum / Hash Integrity** | Uploaded SHA-256 matches local source file hash | `MACOS_CI_VERIFIABLE` |

---

## 18. Persistence / Cache Validation

Local SQLite database storage (`GRDB` or SQLite C API) manages node metadata, transfer state, and change feeds.

- **Schema Migration**: Database migrations tested from schema v1 through future versions without data loss.
- **Transaction Atomicity**: Multi-row change feed applications must fail atomically if any journal item violates constraints.
- **Cache Eviction**: Least-recently-used (LRU) cached file downloads evicted when cache size limit is reached.
- **Isolation of Secrets**: Non-secret metadata stored in SQLite; secrets stored exclusively in Keychain.

---

## 19. Lifecycle / Background Validation

- **Foreground ↔ Background Transitions**: Application state preserved when app is suspended.
- **Background Refresh (`BGAppRefreshTask`)**: Testing background sync registration and task completion within Apple's allotted execution time (~30 seconds).
- **Relaunch Recovery**: App relaunch after forced crash checks transfer queue in SQLite and resumes pending downloads/uploads.

---

## 20. File Provider Validation (P053–P055 Boundary)

The iOS File Provider Extension (`NSFileProviderExtension`) exposes Synveil files directly inside iOS `Files.app`.

- **Domain Registration**: Registering `NSFileProviderDomain` with system.
- **App Group Sandboxing**: Extension and main application share local database and file staging directory via `containerURL(forSecurityApplicationGroupIdentifier:)`.
- **Enumeration & Fetching**: Validating `NSFileProviderEnumerator` directory listing and item download on demand.

---

## 21. PhotoKit Validation (P056–P058 Boundary)

PhotoKit auto-upload integrates with Apple `PHPhotoLibrary`:

- **Authorization State Handling**: Testing app behavior under `.authorized`, `.limited`, `.denied`, and `.restricted` permissions.
- **Asset Change Observers**: `PHPhotoLibraryChangeObserver` detecting new camera roll photos and queuing background upload tasks.
- **Deduplication**: SHA-256 or local asset identifier check preventing duplicate uploads of previously backed-up media.

---

## 22. Failure Classification Model

Repository CI workflows may fail for reasons unrelated to iOS code (e.g., Linux desktop Qt builds, Windows packaging, web app linting, PostgreSQL service issues). To prevent confusion and enforce high signal quality, all CI failures must be classified into one of six standard classes:

```text
                  CI Failure Detected
                           │
    ┌──────────────────────┼──────────────────────┐
    ▼                      ▼                      ▼
IOS_CHANGE_CAUSED   PRE_EXISTING_BASELINE  UNRELATED_SUBSYSTEM
    │                      │                      │
(Fix Required)      (Compare Base)        (Ignore/Report)
    │                      │                      │
    ▼                      ▼                      ▼
CI_INFRASTRUCTURE    FLAKY_OR_NONDETERM   UNKNOWN_INVESTIGATE
```

### Classification Categories & Required Evidence

1. **`IOS_CHANGE_CAUSED`**:
   - *Definition*: Failure caused directly by changes introduced in the active PR or prompt (e.g., Swift syntax error, broken unit test, broken Rust FFI compilation).
   - *Required Action*: **Must be fixed before PR merge**.
   - *Evidence*: Failure trace points directly to modified files in `clients/ios/` or modified Rust crates.
2. **`PRE_EXISTING_BASELINE_FAILURE`**:
   - *Definition*: Failure that exists on the base branch (`ios-app` target commit) prior to introducing PR changes.
   - *Required Action*: Document in PR and manifest with baseline commit comparison. Does not block iOS PR merge if unrelated.
   - *Evidence*: Identical failure step and error message observed on base commit run.
3. **`UNRELATED_SUBSYSTEM_FAILURE`**:
   - *Definition*: Failure in a non-iOS workflow or job (e.g., `desktop-windows-native`, `postgres-17`, `web-quality`, `appimage.yml`).
   - *Required Action*: Document in PR and manifest. Does not block iOS PR merge.
   - *Evidence*: Modified files do not overlap with failing subsystem path or dependency tree.
4. **`CI_INFRASTRUCTURE_FAILURE`**:
   - *Definition*: Failure caused by GitHub Actions runner timeouts, network outages downloading Xcode components, or runner disk exhaustion.
   - *Required Action*: Re-run workflow. Document infrastructure error if persistent.
   - *Evidence*: System error code (e.g., HTTP 503 during checkout, runner lost communication).
5. **`FLAKY_OR_NONDETERMINISTIC`**:
   - *Definition*: Intermittent failure in async timing, network sockets, or simulator boot timing.
   - *Required Action*: Re-run job. If flaky test is in iOS codebase, isolate and fix or mark for stabilization.
   - *Evidence*: Test passes on re-run without code changes.
6. **`UNKNOWN_REQUIRES_INVESTIGATION`**:
   - *Definition*: Ambiguous failure where relationship to iOS changes is unclear.
   - *Required Action*: Perform root-cause investigation; classify into one of the above 5 classes before proceeding.

---

## 23. Baseline Comparison Strategy

To prove that a failure is `PRE_EXISTING_BASELINE_FAILURE` or `UNRELATED_SUBSYSTEM_FAILURE`:

1. Record current base commit SHA (`origin/ios-app`).
2. Compare PR workflow run logs against base commit workflow run logs.
3. Inspect `git diff origin/ios-app...HEAD --name-only` to confirm touched paths.
4. If touched files are strictly under `clients/ios/**` or `docs/ios/**`, failures in Linux desktop Qt scripts or Windows PE binary checks are confirmed unrelated.

---

## 24. CI Artifact & Evidence Retention Policy

All future iOS CI workflows must generate structured, non-sensitive evidence artifacts uploaded to GitHub Actions run artifacts:

- **Xcode Test Results**: `.xcresult` bundles containing detailed test timelines and assertions.
- **Build Logs**: Raw `xcodebuild` output logs (`xcodebuild.log`).
- **Rust Test Summaries**: `cargo test` stdout/stderr outputs.
- **Redaction Requirement**: CI scripts and upload steps **must strictly sanitize logs** to ensure no authorization tokens (`sve1_`, `svd1_`), private keys, or passwords appear in build artifacts.

---

## 25. Path-Trigger Strategy

Future dedicated iOS workflows must use targeted path filtering to save CI runner minutes while ensuring shared correctness:

```yaml
on:
  push:
    paths:
      - "clients/ios/**"
      - "crates/core/**"
      - "crates/object-store/**"
      - "crates/client-sync/**"
      - "docs/ios/**"
      - ".github/workflows/ios*.yml"
  pull_request:
    paths:
      - "clients/ios/**"
      - "crates/core/**"
      - "crates/object-store/**"
      - "crates/client-sync/**"
      - "docs/ios/**"
      - ".github/workflows/ios*.yml"
```

---

## 26. Pull Request Gate Model

For all iOS prompts (P006–P060), PRs targeting `ios-app` must adhere to this gate model:

1. **Target Branch**: Must be `ios-app`.
2. **Applicable Checks**:
   - Prompt-specific required checks (e.g., Linux docs checks for P005; macOS Xcode build for P007+; Simulator tests for P009+).
3. **Merge Rule**: PR may merge if all `IOS_CHANGE_CAUSED` checks pass. Unrelated pre-existing baseline or desktop failures do not block merge provided evidence is documented in the PR description and manifest.
4. **Physical Device Deferral Rule**: Physical device gaps during early phases may be deferred to later prompts provided they are explicitly listed in the prompt manifest evidence section.

---

## 27. Prompt / Phase Validation Matrix

This matrix maps all 60 roadmap prompts to their required minimum validation environments and major gate transitions.

| Prompt Range & Phase | Focus Area | Required Validation Environments | Major Gate Transitions |
| :--- | :--- | :--- | :--- |
| **P001–P005** (Phase A) | Product Contract, Audit, Architecture & CI | `LINUX_VERIFIABLE` | Document checks & manifest verification. **P005: Validation Architecture established.** |
| **P006–P012** (Phase B) | iOS Project Foundation & Swift Setup | `MACOS_CI_VERIFIABLE`, `SIMULATOR_VERIFIABLE` | **P007: macOS Build Gate newly mandatory.** **P008: iOS Build CI Workflow newly mandatory.** **P009: Simulator Test Gate newly mandatory.** |
| **P013–P020** (Phase C) | Shared-Core / Swift FFI Boundary | `LINUX_VERIFIABLE`, `MACOS_CI_VERIFIABLE` | **P014+: Rust Apple Target Compilation Gate newly mandatory.** **P016+: Swift ↔ Rust FFI Integration Gate newly mandatory.** |
| **P021–P028** (Phase D) | Authentication & Connectivity | `MACOS_CI_VERIFIABLE`, `SIMULATOR_VERIFIABLE` | **P025+: Keychain Functional Validation Gate newly mandatory.** Secrets leak audit mandatory. |
| **P029–P037** (Phase E) | File Browser & Metadata Mutations | `SIMULATOR_VERIFIABLE` | SwiftUI view-model & paginated navigation tests mandatory. Offline mutation queue tests mandatory. |
| **P038–P047** (Phase F) | Transfer Engine, Cache & Sync | `SIMULATOR_VERIFIABLE`, `MACOS_CI_VERIFIABLE` | **P038+: Transfer Integrity & Resumable 4 MiB Upload Gate newly mandatory.** SQLite atomic transaction gate mandatory. |
| **P048–P052** (Phase G) | Platform Integration (Quick Look, BG Tasks) | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | In-app file preview and simulated background task launch mandatory. |
| **P053–P055** (Phase G) | File Provider Extension | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY`, `SIGNING_REQUIRED` | **P053+: File Provider Subsystem Gate newly mandatory.** Shared App Group sandbox validation mandatory. |
| **P056–P058** (Phase G) | PhotoKit Auto-Upload Subsystem | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | **P056+: PhotoKit Asset Library Gate newly mandatory.** Deduplication & authorization matrix mandatory. |
| **P059–P060** (Phase H) | Hardening, Parity & v0.1 Release | `MACOS_CI_VERIFIABLE`, `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY`, `SIGNING_REQUIRED` | **P059/P060: Aggregated Release Gate newly mandatory.** Complete test suite pass & signed archive validation mandatory. |

---

## 28. Release Validation Progression

Before a release claim (P060) can be finalized:

1. **Tier 0–4 Automated Suite Pass**: 100% pass rate on all static, unit, FFI, and Simulator tests in CI.
2. **Physical Device Matrix Clearance**: Every row in the Controlled Physical Device Matrix (Section 9) must have recorded, verified evidence.
3. **Security & Secrets Audit**: Confirmed zero secrets in logs, database, or source code.
4. **Signed Release Archive**: Signed `.ipa` successfully archived with distribution provisioning profile.

---

## 29. Known Limitations

- **No macOS Xcode Build on Linux**: Linux developers cannot perform native Xcode builds or launch iOS Simulators directly; macOS CI host is required.
- **Simulator Background Task Limits**: Simulator requires programmatic background task trigger rather than natural iOS power/thermal scheduling.
- **Physical Device CI Gap**: Automated physical device farm is not configured in v0.1 CI; device validation relies on manual/controlled runner tests with recorded artifacts.

---

## 30. Downstream Requirements for P006–P060

- **P006–P007**: Initialize `clients/ios` Swift package/project structure and verify initial macOS build.
- **P008**: Implement `ios-build.yml` in `.github/workflows/`.
- **P009**: Implement `ios-simulator-tests.yml` in `.github/workflows/`.
- **P013–P020**: Enforce Apple-target Rust compilation and FFI deallocation safety tests.
- **P025**: Enforce Keychain mock unit tests and secrets redaction assertions.
- **P038**: Enforce transfer engine resumable upload integration tests.
- **P053–P058**: Implement File Provider and PhotoKit extension tests.
- **P059–P060**: Produce release manifest and aggregate validation evidence.
