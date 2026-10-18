# Synveil iOS v0.1 Implementation Roadmap

This document outlines the high-level phased roadmap for building the native Synveil iOS client (v0.1) to reach feature parity with the Android v0.1 client.

---

## Phase Structure Overview

```
Phase A: Product Contract & Platform Audit
   │
   ▼
Phase B: iOS Project & Build Foundation
   │
   ▼
Phase C: Shared-Core / Swift Boundary (FFI / Interop)
   │
   ▼
Phase D: Authentication & Server Connectivity
   │
   ▼
Phase E: File Browser & File Operations
   │
   ▼
Phase F: Transfer, Cache, & Offline Behavior
   │
   ▼
Phase G: Apple Platform Integration (BackgroundTasks, Quick Look, Share Sheet)
   │
   ▼
Phase H: Hardening, Validation, & v0.1 Release Readiness
```

---

## Phase Details

### Phase A: Product Contract and Platform Audit
* **Goal**: Establish authoritative requirements, feature parity inventory, Apple framework mapping, and validation boundaries.
* **Deliverables**: `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`, `docs/ios/IOS_V0_1_ROADMAP.md`, `docs/ios/PROMPT001_MANIFEST.md`.
* **Validation**: Codebase review, Android source inspection, documentation check.

### Phase B: iOS Project and Build Foundation
* **Goal**: Initialize native iOS project, SPM package structure, SwiftUI app entry point, GitHub Actions macOS CI workflow, and build settings targeting iOS 17.0+.
* **Key Tasks**:
  * Xcode project layout under `clients/ios/`.
  * SwiftLint & build settings setup.
  * macOS CI runner configuration for `xcodebuild`.
* **Validation**: macOS CI build (`xcodebuild build`) succeeds cleanly.

### Phase C: Shared-Core / Swift Boundary
* **Goal**: Establish C-FFI / UniFFI bridge between shared Rust crates (`synveil-core`, `synveil-client-sync`) and Swift client code.
* **Key Tasks**:
  * Expose Rust identifier generation, hashing, and token parsing to Swift.
  * Wrap FFI functions in Swift-idiomatic async/await interfaces.
* **Validation**: Rust crate unit tests + Swift SPM bridge unit tests on macOS CI.

### Phase D: Authentication and Server Connectivity
* **Goal**: Implement server profile management, health check preflight, `sve1_` device enrollment exchange, and Keychain credential storage (`svd1_`).
* **Key Tasks**:
  * `ServerProfile` storage in `UserDefaults`.
  * `URLSession` HTTP transport with custom TLS policy.
  * `KeychainVault` implementation using iOS Security framework.
  * DeviceBearer auth header generation and "Forget on device" cleanup.
* **Validation**: Unit tests for origin parsing, single-shot enrollment mocks, and Keychain mock tests on macOS CI.

### Phase E: File Browser and File Operations
* **Goal**: Build SwiftUI library list, paginated folder browser, path breadcrumbs, and metadata mutation actions (mkdir, rename, move, trash, restore).
* **Key Tasks**:
  * SwiftUI `NavigationStack` and library catalog views.
  * `NodeBrowser` with paginated child node loading.
  * Local mutation queue for offline metadata changes.
* **Validation**: SwiftUI View Inspector / snapshot tests and ViewModel mock tests on macOS CI.

### Phase F: Transfer, Cache, and Offline Behavior
* **Goal**: Deliver streaming file downloads, resumable 4 MiB chunked uploads, SQLite local metadata caching, and offline state handling.
* **Key Tasks**:
  * Local SQLite cache (via GRDB or CoreData/SwiftData).
  * `URLSessionDownloadTask` and background `URLSessionUploadTask`.
  * Staging storage management in `Caches` directory with storage pressure protection.
  * Sync change feed consumption, signed ACK replay, and rebaseline manifest atomic swap.
* **Validation**: Mocked HTTP transfer tests, SQLite transaction tests, offset reconciliation tests.

### Phase G: Apple Platform Integration
* **Goal**: Integrate native iOS platform capabilities for background sync, previews, file picking, and sharing.
* **Key Tasks**:
  * `BGAppRefreshTask` integration for background sync.
  * Quick Look framework in-app file previewing.
  * `ShareLink` / `UIActivityViewController` file export.
  * Document picker (`.fileImporter`) file selection.
* **Validation**: Simulator UI automation tests on macOS CI.

### Phase H: Hardening, Validation, and v0.1 Release
* **Goal**: P059 records the Android–iOS parity audit and regression hardening. P060 is the final v0.1 release gate and owns release packaging and evidence-based certification. This roadmap phase does not move release-gate work into P059.
* **Key Tasks**:
  * Accessibility (VoiceOver) labels and dynamic type support.
  * Memory leak and bearer logging audit.
  * End-to-end acceptance testing against live/mock Synveil server on macOS CI runner.
  * Production IPA release build setup.
* **Validation**: Complete macOS CI test suite pass, clean security/privacy audit, testable v0.1 release artifact.

### P060 final release gate status

P060 closes the engineering assessment with `BLOCKED_RELEASE`. PAR-03/CAP-13A (persistent labeled profiles) and PAR-04/CAP-13B (multi-profile switching with isolated sessions) remain blocked; no owner-approved scope revision was supplied. Physical-device acceptance, live authenticated-server acceptance, explicit release version/build and privacy review, and a signed distribution artifact remain outstanding. P059 parity totals are unchanged. Do not distribute or integrate `ios-app` into `main` until the release and integration gates are satisfied. See `IOS_V0_1_RELEASE_REPORT.md` and `manifests/PROMPT060_MANIFEST.md`.

## Owner-authorized post-baseline extensions

The v0.1 release roadmap and feature claims remain defined by the product contract. After that
baseline, the owner authorized a separate P053–P055 File Provider workstream. P053–P054 add
isolated, fail-closed `NSFileProviderReplicatedExtension` infrastructure; production Apple Files
registration and content operations remain disabled. P055 PR #139 is still open and unmerged.
PhotoKit P056–P058 is a separate owner-authorized post-baseline workstream, merged through P058
PR #142 at `c0c4c42`. The host app implements opt-in metadata discovery and foreground upload
orchestration over ordinary FileVersions. Server PhotoAsset/Live Photo binding, BackupSnapshot
membership and retention proof remain absent. Background upload, silent iCloud retrieval, remote
deletion, and treating these PhotoKit flows as v0.1 release parity remain deferred.


## P048 implementation record

P048 was merged into `ios-app` through PR #131 on 2026-10-14. The native opportunistic
background metadata refresh remains disabled by default, binds explicit opt-in to the current
authenticated endpoint/account/device/credential, and uses BGAppRefreshTask with a bounded workload.
Only new inbound pages and their normal initial ACK can run; unknown ACK recovery, outbound
mutations, rebaseline and file-content transfers remain separate authorized actions. iOS controls
delivery and no periodic interval is guaranteed. SQLite v9 and P046 upload ownership are retained.
P049–P058 are subsequent implemented milestones; P059 records current parity and regression evidence.

See [P048 manifest](manifests/PROMPT048_MANIFEST.md) for its original implementation evidence,
budgets, restrictions, and physical-device NOT_AVAILABLE status.
