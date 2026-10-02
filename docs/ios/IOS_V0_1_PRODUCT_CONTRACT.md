# Synveil iOS v0.1 Product Contract and Parity Specification

## Overview

This document establishes the authoritative implementation baseline for the **Synveil iOS native client (v0.1)**.

The Synveil Android client (v0.1) serves as the primary behavioral reference for iOS feature parity. This contract specifies the exact functional surface implemented in Android v0.1, audits shared Rust core components for reuse, classifies platform equivalents, defines the build/validation boundary across Linux and macOS, and sets a testable acceptance surface for iOS v0.1.

---

## 1. Android v0.1 Implemented Surface & Repository Citations

This section provides an exhaustive feature inventory of the Android v0.1 client, citing specific source code files and modules.

### 1.1 Onboarding & Setup Flow
* **Server Profile Creation**: Supports generating opaque UUIDv7 profile IDs, human-readable labels, canonical origin validation (rejecting trailing slashes, credentials, paths, queries, fragments, LAN names, or malformed ports), and transport policy assignment.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/core/model/ServerProfile.kt`, `clients/android/app/src/main/java/com/synveil/android/data/profile/ServerProfileRepository.kt`, `clients/android/app/src/main/java/com/synveil/android/feature/profile/ProfileScreens.kt`
* **First-Run State Machine**: Manages startup states: `NoProfiles`, `SelectedUnconfigured`, `Testing`, `Enrolled`, `NotEnrolled`, `EnrollmentRecoveryRequired`, `AuthenticationFailed`, `DeviceRevoked`.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/feature/startup/StartupState.kt`, `clients/android/app/src/main/java/com/synveil/android/app/navigation/SynveilNavHost.kt`
* **Guided Setup UI**: Provides edge-to-edge Compose UI for creating a server profile, testing server availability, entering one-time token, and entering the library catalog.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/feature/profile/ProfileScreens.kt`, `clients/android/app/src/main/java/com/synveil/android/feature/enrollment/EnrollmentScreen.kt`

### 1.2 Server Connection & Transport Policy
* **Canonical Origin & TLS Rules**: Enforces HTTPS for production profiles. Numeric loopback cleartext (`127.0.0.1`, `[::1]`) is permitted strictly under `LOOPBACK_TEST_HTTP` in debug builds only. Hostnames (`localhost`, `.local`) and arbitrary HTTP origins remain strictly rejected.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/core/model/ServerProfile.kt`, `clients/android/app/src/main/java/com/synveil/android/data/network/SynveilHttpTransport.kt`
* **Health & Readiness Check**: Calls `/health/live` prior to `/health/ready`. Updates `lastConnectedAt` strictly on a successful `ready` response (`200 OK`). Response bodies are bounded to 64 KiB.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/network/SynveilHttpTransport.kt`
* **Bounded HTTP Transport**: OkHttp client configured with finite connect/read/write/call timeouts, disabled redirects, disabled connection retries, disabled cookies, strict JSON `Accept` headers, and custom user-agent headers.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/network/SynveilHttpTransport.kt`

### 1.3 One-Time Device Enrollment & Authentication
* **Enrollment Token Exchange**: Executes `POST /api/v1/device-enrollment/exchange` using token format `sve1_` + 64 lowercase hex chars. Single-shot request: retries and redirects are explicitly disabled.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/enrollment/EnrollmentManager.kt`, `clients/android/app/src/main/java/com/synveil/android/data/enrollment/EnrollmentModels.kt`
* **Android Keystore Credential Vault**: Generates local `svd1_` credential pair preflight, encrypts it using AES-256-GCM with a hardware-backed key in Android Keystore, and stores encrypted payload with 12-byte IV in Preferences DataStore.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/enrollment/CredentialVault.kt`, `clients/android/app/src/main/java/com/synveil/android/data/enrollment/EnrollmentMetadataStore.kt`
* **DeviceBearer Authentication**: Sends `Authorization: Bearer svd1_...` header on profile-scoped API calls. Keeps secret lifetime bounded to memory; never logs, serializes, or exposes bearer credentials in UI or navigation states.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/session/DeviceSessionManager.kt`, `clients/android/app/src/main/java/com/synveil/android/data/network/SynveilHttpTransport.kt`
* **Credential Fence & Safe Local Cleanup**: "Forget on this device" removes local Android Keystore keys and DataStore secrets without claiming server revocation. Origin changes or profile deletions enforce credential cleanup first.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/enrollment/CredentialVault.kt`, `clients/android/app/src/main/java/com/synveil/android/data/profile/ServerProfileRepository.kt`

### 1.4 Account & Session Handling
* **Profile-Bound Device Session**: Tracks current active profile, enrollment scope, origin, transport, owner, and device identity.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/session/DeviceSessionManager.kt`
* **Multi-Profile Switching**: Supports switching between stored server profiles with strict cache and credential isolation.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/profile/ServerProfileRepository.kt`, `clients/android/app/src/main/java/com/synveil/android/feature/profile/ProfileViewModels.kt`

### 1.5 File Browsing & Directory Navigation
* **Library Discovery**: Fetches accessible libraries via `GET /api/v1/libraries` using DeviceBearer auth, bounded to 100 items per page, 64 pages max, 4096 total libraries.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/library/LibraryModels.kt`, `clients/android/app/src/main/java/com/synveil/android/feature/library/LibraryScreen.kt`
* **Logical Node Browser**: Paginated child node navigation via `GET /api/v1/libraries/{library_id}/nodes`. Supports opaque cursors (512-byte max) and distinguishes active vs trashed logical nodes. Node names are treated as pure logical metadata, never as local filesystem paths.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/node/NodeModels.kt`, `clients/android/app/src/main/java/com/synveil/android/feature/library/NodeBrowserScreen.kt`
* **Breadcrumb Navigation**: Path breadcrumb tree rendering with root/parent navigation in Compose.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/feature/library/NodeBrowserScreen.kt`

### 1.6 File & Folder Operations (Outbound Metadata Mutations)
* **Metadata Operations**: Directory creation (`CREATE_DIRECTORY`), node rename (`RENAME_NODE`), node move (`MOVE_NODE`), trash node (`TRASH_NODE`), and restore node (`RESTORE_NODE`).
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/mutation/MutationEngine.kt`, `clients/android/app/src/main/java/com/synveil/android/data/mutation/MutationModels.kt`
* **Durable Mutation Queue**: Persists mutation intents in Room database with UUIDv7 intent IDs, base checkpoint, and typed payloads. Offline changes are labeled "Pending sync" and replayed idempotently.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/mutation/MutationEngine.kt`, `clients/android/app/src/main/java/com/synveil/android/data/cache/CacheDatabase.kt`

### 1.7 File Download & Storage Access Framework (SAF)
* **Streaming Content Download**: Downloads logical file bytes from `GET /api/v1/nodes/{node_id}/content` directly into a user-selected Storage Access Framework (`content://`) destination.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/transfer/ContentOperationEngine.kt`, `clients/android/app/src/main/java/com/synveil/android/data/transfer/TransferOperations.kt`
* **SAF Integration**: Uses `ACTION_CREATE_DOCUMENT` for saving downloads, and `ACTION_OPEN_DOCUMENT` for picking files to upload. No broad file permissions (`READ_EXTERNAL_STORAGE` / `MANAGE_EXTERNAL_STORAGE`) or `file://` URIs are used.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/transfer/TransferOperations.kt`

### 1.8 Resumable Uploads & Replace Content
* **File Creation & Content Replacement**: Stages picked SAF input streams in app-private `filesDir/transfer-staging` to obtain exact length and SHA-256 hash.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/transfer/ContentOperationEngine.kt`
* **Idempotent Resumable Upload**: Creates upload session via `POST /api/v1/libraries/{library_id}/uploads` (or replace via `POST /api/v1/nodes/{node_id}/upload-session`), appends 4 MiB raw chunks via `PATCH`. Queries server offset after ambiguous responses to resume cleanly.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/transfer/ContentOperationEngine.kt`
* **Storage Pressure Protection**: Bounded staging storage (512 MiB limit with 64 MiB free space reserve). Fails deterministically on low space before making network requests.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/transfer/ContentOperationEngine.kt`

### 1.9 Local Caching & Offline Behavior
* **Room Database Cache**: Caches libraries, logical nodes, sync state, pending ACKs, rebaseline staging, mutation queue, and content operations in SQLite via Room.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/cache/CacheDatabase.kt`, `clients/android/app/src/main/java/com/synveil/android/data/cache/CacheRepository.kt`
* **Offline-First Browsing**: Renders last-known cached node tree when offline with explicit "Cached/Offline" banners. Disables destructive operations when required sync base is unavailable.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/feature/library/NodeBrowserScreen.kt`, `clients/android/app/src/main/java/com/synveil/android/data/connectivity/ConnectivityObserver.kt`

### 1.10 Inbound Synchronization & Rebaseline Recovery
* **Journal Feed & Signed ACKs**: Consumes inbound change feed (`GET /api/v1/libraries/{library_id}/sync/feed`), applies updates in Room transactions, and sends signed ACK tokens (`POST /api/v1/libraries/{library_id}/sync/ack`). Replays pending ACK tokens on recovery.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/sync/SyncEngine.kt`, `clients/android/app/src/main/java/com/synveil/android/data/sync/SyncModels.kt`
* **Rebaseline Snapshot Recovery**: When change history is expired, downloads manifest pages, stages rebaseline snapshots in Room, and atomically swaps active projection upon complete manifest verification.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/sync/SyncEngine.kt`
* **Sync Coordinator & WorkManager**: Manages per-scope locks for synchronization across foreground refreshes and WorkManager background tasks (15-min periodic or one-time sync).
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/sync/SyncCoordinator.kt`, `clients/android/app/src/main/java/com/synveil/android/work/SyncWorker.kt`

### 1.11 Conflict Handling & Security Boundary
* **Safe Local Conflict Summaries**: Detects server conflict status, persists local conflict summaries, and displays "Owner/web review required" UI banner.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/sync/SyncEngine.kt`, `clients/android/app/src/main/java/com/synveil/android/feature/library/NodeBrowserScreen.kt`
* **BrowserSession-Only Boundary**: Never fabricates DeviceBearer conflict resolution requests or auto-selects conflict winners; conflict inspection and manual resolution remain strictly web browser workflows.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/sync/SyncEngine.kt`, `docs/adr/ADR-032-deterministic-sync-conflict-preservation.md`

### 1.12 Previews & Sharing
* **SAF Content Sharing**: Shares downloaded files strictly via `content://` URIs with temporary read grants using system share intents (`ACTION_SEND`). No file paths or broad file access.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/transfer/TransferOperations.kt`

### 1.13 Settings & Diagnostics
* **Sync Settings**: User-configurable periodic background sync enable/disable, network constraint selection (ANY vs UNMETERED), and battery constraint toggle.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/settings/SyncSettings.kt`, `clients/android/app/src/main/java/com/synveil/android/feature/home/SyncSettingsScreen.kt`

---

## 2. Feature Parity Matrix

Every Android capability is classified below into exactly one of six parity buckets.

| Capability Area | Android v0.1 Implemented Feature | Parity Classification | iOS Native Equivalent / Target API |
|---|---|---|---|
| **Onboarding** | Server Profile Creation & Canonical Origin Validation | `IOS_NATIVE_EQUIVALENT` | SwiftUI + Swift `URLComponents` / regex validation |
| **Onboarding** | First-run state machine (`NoProfiles` -> `Enrolled`) | `IOS_NATIVE_EQUIVALENT` | Swift `ObservableObject` / `@Observable` state machine |
| **Connection** | Canonical Origin & TLS verification | `IOS_NATIVE_EQUIVALENT` | `URLSessionConfiguration` with strict TLS |
| **Connection** | Numeric loopback HTTP debug policy | `IOS_NATIVE_EQUIVALENT` | Swift `URLSession` transport policy |
| **Connection** | Bounded HTTP transport & health check | `IOS_NATIVE_EQUIVALENT` | `URLSessionDataTask` / `async/await` URLSession |
| **Authentication** | `sve1_` One-time enrollment exchange | `IOS_NATIVE_EQUIVALENT` | Swift `URLSession` single-shot POST |
| **Authentication** | Encrypted credential storage | `IOS_NATIVE_EQUIVALENT` | Apple **Keychain Services** (SecItem) with AES/GCM or Keychain-backed keys |
| **Authentication** | DeviceBearer header formatting | `IOS_NATIVE_EQUIVALENT` | Swift HTTP request header composer |
| **Authentication** | Local credential fencing ("Forget on device") | `IOS_NATIVE_EQUIVALENT` | Keychain key deletion (`SecItemDelete`) |
| **File Browsing** | Library discovery & pagination | `IOS_NATIVE_EQUIVALENT` | SwiftUI List + Swift URLSession |
| **File Browsing** | Directory navigation & child pagination | `IOS_NATIVE_EQUIVALENT` | SwiftUI `NavigationStack` + custom state |
| **File Browsing** | Active vs Trashed logical node display | `IOS_NATIVE_EQUIVALENT` | SwiftUI list filter / state |
| **File Operations**| Outbound metadata mutations (mkdir, rename, move, trash, restore) | `IOS_NATIVE_EQUIVALENT` | Swift Async MutationEngine + SQLite / SwiftData / CoreData |
| **Transfers** | Streaming download to local storage | `IOS_NATIVE_EQUIVALENT` | `URLSessionDownloadTask` / `URLSession.bytes` |
| **Transfers** | Resumable chunked upload & replace | `IOS_NATIVE_EQUIVALENT` | `URLSessionUploadTask` / background `URLSession` |
| **Transfers** | Local file picking / Document picker | `IOS_NATIVE_EQUIVALENT` | SwiftUI `.fileImporter` / `UIDocumentPickerViewController` |
| **Transfers** | File opening and export/share | `IOS_NATIVE_EQUIVALENT` | SwiftUI `ShareLink` / `UIActivityViewController` |
| **Local Cache** | SQLite local metadata database | `IOS_NATIVE_EQUIVALENT` | GRDB.swift / SQLite.swift / CoreData / SwiftData |
| **Offline** | Connectivity monitoring | `IOS_NATIVE_EQUIVALENT` | Apple **Network framework** (`NWPathMonitor`) |
| **Offline** | Offline-first cached node browsing | `IOS_NATIVE_EQUIVALENT` | Local SQLite database read projection |
| **Sync** | Journal change feed & signed ACK loop | `IOS_NATIVE_EQUIVALENT` | Swift SyncEngine + Local DB |
| **Sync** | Rebaseline snapshot manifest staging & atomic swap | `IOS_NATIVE_EQUIVALENT` | Swift SyncEngine + Local DB transaction |
| **Sync** | Background periodic/one-time sync task | `IOS_NATIVE_EQUIVALENT` | Apple **BackgroundTasks framework** (`BGAppRefreshTask`) |
| **Conflicts** | Local conflict summary & Owner/Web review state | `IOS_NATIVE_EQUIVALENT` | SwiftUI Conflict banner |
| **Security** | Redacted logs, memory-bounded bearer strings | `IOS_NATIVE_EQUIVALENT` | Swift `CustomStringConvertible` redaction |
| **Shared Core** | Core Domain Types & Identifier Parsing (UUIDv7, Hashes) | `SHARED_WITH_ADAPTER` | Rust `synveil-core` crate via UniFFI / C-FFI |
| **Shared Core** | Sync protocol state machine & feed validation logic | `SHARED_WITH_ADAPTER` | Rust `synveil-client-sync` or Swift adaptation |
| **Android Specific**| Android WorkManager scheduling & AndroidX DataStore | `ANDROID_ONLY_NOT_APPLICABLE` | Replaced by `BGAppRefreshTask` & Keychain/UserDefaults |
| **Android Specific**| Android SAF (`content://` URIs, DocumentFile) | `ANDROID_ONLY_NOT_APPLICABLE` | Replaced by iOS App Sandbox & `UIDocumentPicker` |
| **iOS Extension**| File Provider Extension (Files app integration) | `IOS_DEFERRED_AFTER_V0_1` | Apple `NSFileProviderExtension` (Post-v0.1) |
| **iOS Extension**| PhotoKit Background Auto-Upload | `IOS_DEFERRED_AFTER_V0_1` | Apple `PhotoKit` / `PHPhotoLibrary` (Post-v0.1) |
| **Product Decision**| Multi-profile Keychain Access Group Sharing | `REQUIRES_PRODUCT_DECISION` | Apple Keychain Access Group configuration |

---

## 3. Shared-Core Reuse Audit

This audit evaluates existing Rust crates in the repository for reuse in the iOS client.

### 3.1 Directly Reusable Rust Components
* **`synveil-core` (`crates/core`)**: Domain primitives, UUIDv7 generation, cryptographic hash wrappers (SHA-256), token parsing (`sve1_`, `svd1_`), time helpers, error models. Completely platform-agnostic (`no_std` compatible or pure Rust std).
* **`synveil-object-store` (`crates/object-store`)**: Key abstraction models and blob storage traits. Fully reusable if local file staging or object operations run in Rust.

### 3.2 Reusable After Thin FFI / Platform Adapter
* **`synveil-client-sync` (`crates/client-sync`)**: Implements client-side sync state machine, feed consumption, ACK generation, rebaseline snapshot staging logic, and conflict policy rules.
  * *Adaptation Needed*: Currently uses SQLite via Rust `sqlx` or `rusqlite` and platform secret storage traits in `synveil-platform`. To reuse in iOS v0.1, requires exposing C-FFI / UniFFI bindings or decoupling database persistence so Swift can pass in SQLite handles or memory state.

### 3.3 Unsuitable for Direct Reuse on iOS
* **`synveil-client` (`crates/client`)**: Contains Linux/desktop IPC control sockets and desktop process launcher logic. Irrelevant for iOS mobile sandbox.
* **`synveil-desktop` (`crates/desktop`)**: Qt/QML UI shell and desktop bridge code.
* **`synveil-install-engine` (`crates/install-engine`)**: Linux AppImage/Debian packaging engine.

### 3.4 Coupled Code Requiring Refactoring (Do NOT Refactor in Prompt001)
* **`synveil-platform` (`crates/platform`)**: Contains platform secret storage implementations (`linux.rs`, `macos.rs`, `windows.rs`). `macos.rs` currently targets macOS Keychain APIs (via Security framework), which differs slightly from iOS Keychain API requirements (e.g. accessibility classes `kSecAttrAccessibleAfterFirstUnlock`).
* **`synveil-auth` (`crates/auth`) & `synveil-api` (`crates/api`)**: Server-side crates; client code must only interact with these via open HTTP endpoints.

---

## 4. Apple Platform Requirements

The native iOS v0.1 client requires the following native Apple frameworks and system components:

### 4.1 UI & Layout Architecture
* **SwiftUI**: Declarative UI layout matching Android Compose design patterns. Navigation via `NavigationStack` and `NavigationPath`.
* **Uniform Type Identifiers (`UTType`)**: Used for file type identification, document picking, export, and preview handling.

### 4.2 Credential & Data Security
* **Keychain Services (`Security.framework`)**: For hardware-backed secure storage of `svd1_` device credentials, matching Android Keystore behavior. Access control configured with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`.

### 4.3 Networking & Background Transfers
* **URLSession**: Standard async HTTP networking, custom TLS policy configuration, and bounded streaming.
* **Background URLSession (`URLSessionConfiguration.background`)**: For background resumable chunk uploads and streaming file downloads that continue when the app is suspended.

### 4.4 Filesystem & Previews
* **App Sandbox Filesystem**: App-private `Library/Caches` and `tmp` directories for staging 4 MiB upload chunks and temporary download staging.
* **Quick Look (`QuickLook` framework)**: In-app previewing of downloaded files (images, PDFs, documents) matching Android SAF preview intents.
* **Share Sheet (`ShareLink` / `UIActivityViewController`)**: System export and file sharing.

### 4.5 Background Execution & Scheduling
* **BackgroundTasks (`BGAppRefreshTask`)**: Periodic background metadata synchronization matching WorkManager periodic sync (bounded by iOS system battery and usage scheduling).

### 4.6 Explicitly Excluded / Deferred Apple Frameworks for v0.1
* **File Provider (`NSFileProviderExtension`)**: *DEFERRED AFTER V0.1*. Files app virtual directory integration is out of scope for baseline parity.
* **PhotoKit (`PHPhotoLibrary`)**: *DEFERRED AFTER V0.1*. Camera roll auto-upload is out of scope for v0.1 baseline parity.

---

## 5. Build and Validation Boundary

Due to environment constraints (Jules runs on Ubuntu Linux without macOS, Xcode, or iOS Simulators), validation is divided across environments.

```
+-----------------------------------------------------------------------+
|                         LINUX (Jules VM)                              |
| - Rust shared crates check & unit tests (`synveil-core`, `object-store`) |
| - Swift syntax & documentation verification scripts                   |
| - Android reference verification (`./gradlew test`)                   |
| - OpenAPI schema conformance check                                    |
+-----------------------------------------------------------------------+
                                   |
                                   v
+-----------------------------------------------------------------------+
|                         macOS CI RUNNER                               |
| - Xcode project compilation (`xcodebuild build`)                       |
| - Swift unit & integration tests (`xcodebuild test`)                  |
| - iOS Simulator execution (`xcodebuild -destination 'platform=iOS...'`)|
| - SwiftLint & SwiftFormat checks                                     |
| - Code signing and IPA packaging (`xcodebuild archive`)               |
+-----------------------------------------------------------------------+
                                   |
                                   v
+-----------------------------------------------------------------------+
|                     PHYSICAL DEVICE / TESTFLIGHT                       |
| - iOS Keychain hardware security verification                         |
| - BackgroundTasks (`BGAppRefreshTask`) execution under real iOS Doze  |
| - Real network interface transitions (Wi-Fi <-> Cellular)             |
| - Apple Push Notifications / System lifecycle under memory pressure   |
+-----------------------------------------------------------------------+
```

### 5.1 Runnable in Jules (Linux VM)
* Rust shared crate compilation and tests (`cargo test -p synveil-core -p synveil-object-store`).
* Android host reference suite (`cd clients/android && ./gradlew test`).
* Documentation, schema, and specification linting.

### 5.2 Requires macOS CI Runner
* Swift compiler (`swiftc`) and Xcode build tools (`xcodebuild`).
* Swift Package Manager (SPM) dependency resolution for iOS targets.
* iOS Simulator test execution (`xcodebuild test -scheme Synveil -destination 'platform=iOS Simulator,name=iPhone 16'`).
* SwiftLint static analysis.

### 5.3 Requires Physical iPhone / iPad
* Hardware Keychain security enforcement (`kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`).
* True iOS BackgroundTasks scheduling and low-power mode behavior.
* Real cellular/Wi-Fi transitions via `NWPathMonitor`.

### 5.4 Requires Signing / Provisioning
* TestFlight distribution, Apple Developer Team provisioning profiles, and IPA bundle packaging.

---

## 6. iOS v0.1 Acceptance Surface

To achieve feature parity with Android v0.1, the iOS client must satisfy this concrete, testable contract:

1. **Onboarding & Connection**:
   * Users can enter server label, origin URL, and policy.
   * Invalid origins (path, query, LAN name, trailing slash, invalid port) are rejected with clear error feedback.
   * Health check verifies `/health/live` and `/health/ready` before committing profile.

2. **Enrollment & Authentication**:
   * Accepts `sve1_` token (64 hex characters) and executes single-shot POST exchange.
   * Preflights local credential generation, stores encrypted `svd1_` credential securely in iOS Keychain.
   * Sends `Authorization: Bearer svd1_...` on all authenticated calls.
   * Provides confirmable "Forget on this device" local credential cleanup.

3. **Browsing & Metadata Operations**:
   * Displays paginated library list and logical folder hierarchy.
   * Supports directory creation, node rename, move, trash, and restore with optimistic UI updates and pending sync indicators.
   * Shows active vs trashed items.

4. **Transfers & Content Replacement**:
   * Downloads files directly to local sandbox and provides Quick Look preview / Share Sheet export.
   * Picks local files using document picker, stages upload chunks, creates upload session, and handles resumable 4 MiB chunk uploads with server offset reconciliation.

5. **Synchronization & Offline**:
   * Maintains local SQLite cache of libraries, nodes, sync feed checkpoints, and mutation queue.
   * Consumes change feed, applies updates in local DB transaction, and replays signed ACK tokens.
   * Handles rebaseline snapshot staging and atomic cache swap upon complete manifest download.
   * Performs periodic background sync via `BGAppRefreshTask`.
   * Surfaces offline status and preserves local data safely.

6. **Conflict & Security Boundary**:
   * Detects server conflict states, displays "Owner/web review required" banner, and never attempts automatic conflict resolution via DeviceBearer.
   * Enforces zero bearer credentials in logs, UI state objects, or unencrypted storage.

---

## 7. Risks and Blockers

### 7.1 Rust ↔ Swift Boundary Risk
* Calling Rust code from Swift requires C-FFI or UniFFI bindings. Memory management across the C boundary (string allocation, pointer ownership) and error mapping must be tested thoroughly to avoid memory leaks or crashes on iOS.

### 7.2 Background Execution Restrictions on iOS
* Unlike Android WorkManager, iOS `BGAppRefreshTask` execution timing is entirely controlled by iOS system heuristics (battery level, app usage habits). Background sync cannot guarantee fixed 15-minute intervals.

### 7.3 Background URLSession Staging Lifecycle
* iOS background `URLSession` requires upload source files to remain untouched in the app sandbox until transfer completion. Staged 4 MiB chunk files in `Library/Caches` must be protected from OS eviction during low-disk states.

### 7.4 Simulator vs Physical Device Keychain Gaps
* The iOS Simulator Keychain behaves differently from physical device Secure Enclave hardware (e.g. simulator Keychain persists across app re-installs unless reset). Automated tests on simulators must account for this behavior.
