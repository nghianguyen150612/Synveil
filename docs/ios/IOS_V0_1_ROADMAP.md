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
* **Goal**: Finalize test coverage, performance optimization, accessibility audit, security redaction checks, and release packaging.
* **Key Tasks**:
  * Accessibility (VoiceOver) labels and dynamic type support.
  * Memory leak and bearer logging audit.
  * End-to-end acceptance testing against live/mock Synveil server on macOS CI runner.
  * Production IPA release build setup.
* **Validation**: Complete macOS CI test suite pass, clean security/privacy audit, testable v0.1 release artifact.
