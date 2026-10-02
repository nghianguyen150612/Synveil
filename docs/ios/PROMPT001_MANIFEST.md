# Prompt001 Manifest: iOS Product Surface Audit and Feature-Parity Contract

## Execution Summary

* **Starting Commit SHA**: `6be50b134bb03ec58f8444a6f2ea3ec1825fae16`
* **Task Scope**: Establish an authoritative implementation baseline for a native iOS client (Synveil iOS v0.1) based on the completed Android v0.1 reference implementation and shared Rust crates.
* **Implementation State**: Audit and specification only. No iOS application or Xcode project code was created or modified.

---

## Files Inspected

### Android Reference Client
* `clients/android/README.md`
* `clients/android/ANDROID_ROADMAP.md`
* `clients/android/app/src/main/java/com/synveil/android/core/model/ServerProfile.kt`
* `clients/android/app/src/main/java/com/synveil/android/data/profile/ServerProfileRepository.kt`
* `clients/android/app/src/main/java/com/synveil/android/data/network/SynveilHttpTransport.kt`
* `clients/android/app/src/main/java/com/synveil/android/data/enrollment/EnrollmentManager.kt`
* `clients/android/app/src/main/java/com/synveil/android/data/enrollment/CredentialVault.kt`
* `clients/android/app/src/main/java/com/synveil/android/data/enrollment/EnrollmentMetadataStore.kt`
* `clients/android/app/src/main/java/com/synveil/android/data/session/DeviceSessionManager.kt`
* `clients/android/app/src/main/java/com/synveil/android/data/library/LibraryModels.kt`
* `clients/android/app/src/main/java/com/synveil/android/data/node/NodeModels.kt`
* `clients/android/app/src/main/java/com/synveil/android/data/mutation/MutationEngine.kt`
* `clients/android/app/src/main/java/com/synveil/android/data/mutation/MutationModels.kt`
* `clients/android/app/src/main/java/com/synveil/android/data/transfer/ContentOperationEngine.kt`
* `clients/android/app/src/main/java/com/synveil/android/data/transfer/TransferOperations.kt`
* `clients/android/app/src/main/java/com/synveil/android/data/cache/CacheDatabase.kt`
* `clients/android/app/src/main/java/com/synveil/android/data/sync/SyncEngine.kt`
* `clients/android/app/src/main/java/com/synveil/android/data/sync/SyncCoordinator.kt`
* `clients/android/app/src/main/java/com/synveil/android/work/SyncWorker.kt`
* `clients/android/app/src/main/java/com/synveil/android/feature/startup/StartupState.kt`
* `clients/android/app/src/main/java/com/synveil/android/feature/profile/ProfileScreens.kt`
* `clients/android/app/src/main/java/com/synveil/android/feature/enrollment/EnrollmentScreen.kt`
* `clients/android/app/src/main/java/com/synveil/android/feature/library/LibraryScreen.kt`
* `clients/android/app/src/main/java/com/synveil/android/feature/library/NodeBrowserScreen.kt`

### Shared Rust Crates & Documentation
* `crates/core/` (`synveil-core`)
* `crates/object-store/` (`synveil-object-store`)
* `crates/client-sync/` (`synveil-client-sync`)
* `crates/platform/` (`synveil-platform`)
* `docs/adr/ADR-001-rust-modular-monolith.md`
* `docs/adr/ADR-005-resumable-uploads.md`
* `docs/adr/ADR-006-change-journal-sync.md`
* `docs/adr/ADR-015-credential-model.md`
* `docs/adr/ADR-028-durable-rebaseline-snapshot-materialization.md`
* `docs/adr/ADR-032-deterministic-sync-conflict-preservation.md`
* `docs/adr/ADR-033-bounded-bidirectional-sync-cycle.md`
* `docs/en/ARCHITECTURE.md`
* `docs/en/SYNC.md`
* `docs/en/SECURITY.md`

---

## Files Created

* `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`
* `docs/ios/IOS_V0_1_ROADMAP.md`
* `docs/ios/PROMPT001_MANIFEST.md`

---

## Commands & Tests Run

* `git status` / `git log -1 --oneline` (Repository baseline verification)
* `cargo check -p synveil-core -p synveil-object-store` (Shared Rust crates compilation check)
* `cd clients/android && ./gradlew test` (Android reference unit suite verification - 100% pass)

---

## Key Findings

1. **Android v0.1 Functional Parity Baseline**:
   * The Android client v0.1 implements server profile management, health check preflight, single-shot `sve1_` token enrollment, hardware-encrypted `svd1_` credential storage in Android Keystore, DeviceBearer request authentication, paginated library and logical directory browsing, outbound metadata mutations (mkdir, rename, move, trash, restore), streaming file download to SAF, 4 MiB resumable chunk uploads with server offset reconciliation, durable Room metadata caching, change journal feed consumption with signed ACK replay, rebaseline snapshot manifest staging with atomic projection swap, WorkManager background sync, local conflict summaries, and strict BrowserSession-only conflict resolution boundaries.

2. **Shared Core Reuse**:
   * `synveil-core` and `synveil-object-store` are pure, platform-agnostic Rust crates directly reusable on iOS via C-FFI / UniFFI.
   * `synveil-client-sync` encapsulates sync feed, ACK, rebaseline, and conflict policy logic, but requires a thin C-FFI/UniFFI wrapper or Swift adaptation to decouple SQLite persistence.

3. **Apple Platform Equivalents**:
   * Android Keystore & DataStore -> Apple **Keychain Services** (`Security.framework`).
   * Android OkHttp -> Apple **URLSession** with custom TLS configuration.
   * Android WorkManager -> Apple **BackgroundTasks framework** (`BGAppRefreshTask`).
   * Android SAF (`content://` URIs) -> Apple **App Sandbox** & **`UIDocumentPickerViewController`**.
   * Android Compose Material 3 -> Apple **SwiftUI**.
   * File sharing / previewing -> Apple **Quick Look** & **`ShareLink`**.

4. **Linux / macOS Boundary**:
   * Jules Ubuntu Linux VM can run shared Rust crate checks, Android unit tests, documentation generation, and schema verification.
   * Compilation of Swift/iOS targets (`xcodebuild`), iOS Simulator testing, and code signing strictly require a macOS CI runner.

---

## Unresolved Questions / Deferred Scope

* **File Provider Extension**: Deferred until post-v0.1.
* **PhotoKit Auto-Upload**: Deferred until post-v0.1.
* **Multi-profile Keychain Access Group Sharing**: Requires a product decision on shared App Group configuration.

---

## Linux / macOS Validation Boundary Summary

* **Linux (Jules Sandbox)**:
  * `cargo check -p synveil-core -p synveil-object-store` -> Passed.
  * `./gradlew test` (Android reference suite) -> Passed (61/61 tasks).
* **macOS CI (Future)**:
  * Xcode compilation (`xcodebuild build`), iOS Simulator execution, code signing (`xcodebuild archive`).
