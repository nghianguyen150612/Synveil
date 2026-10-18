# Synveil iOS v0.1 Product Contract and Parity Specification

## Overview

This document establishes the authoritative implementation baseline for the **Synveil iOS native client (v0.1)**.

The Synveil Android client (v0.1) serves as the primary behavioral reference for iOS feature parity. This contract specifies the exact functional surface implemented in Android v0.1, audits shared Rust core components for reuse, classifies platform equivalents, defines the build/validation boundary across Linux and macOS, and sets a testable acceptance surface for iOS v0.1.

---

## 1. Android v0.1 Implemented Surface & Repository Citations

This section provides an exhaustive feature inventory of the Android v0.1 client, citing specific source code files and modules.

### 1.1 Onboarding & Setup Flow
* **Server Profile Creation**: Supports generating opaque UUIDv7 profile IDs, human-readable labels, canonical origin validation (rejecting credentials, non-root paths, queries, fragments, LAN names for HTTP, and malformed ports), and transport policy assignment. A missing root slash and an explicit `/` are both accepted and emitted as the canonical trailing `/`.
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
* **Credential Fence & Safe Local Cleanup**: "Forget on this device" removes local Android Keystore keys and DataStore secrets without claiming server revocation. Origin changes or profile deletions enforce credential cleanup first. Explicit logout is local-only; transient network failures never delete the credential.
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
* **Idempotent Resumable Upload**: Creates a frozen create/replace upload operation via `POST /api/v1/upload-sessions`, appends 4 MiB raw chunks via `PATCH /api/v1/upload-sessions/{upload_session_id}`, and queries the same session route for its authoritative offset after ambiguous responses.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/transfer/ContentOperationEngine.kt`
* **Storage Pressure Protection**: Bounded staging storage (512 MiB limit with 64 MiB free space reserve). Fails deterministically on low space before making network requests.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/transfer/ContentOperationEngine.kt`

### 1.9 Local Caching & Offline Behavior
* **Room Database Cache**: Caches libraries, logical nodes, sync state, pending ACKs, rebaseline staging, mutation queue, and content operations in SQLite via Room.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/data/cache/CacheDatabase.kt`, `clients/android/app/src/main/java/com/synveil/android/data/cache/CacheRepository.kt`
* **Offline-First Browsing**: Renders last-known cached node tree when offline with explicit "Cached/Offline" banners. Disables destructive operations when required sync base is unavailable.
  * *Files*: `clients/android/app/src/main/java/com/synveil/android/feature/library/NodeBrowserScreen.kt`, `clients/android/app/src/main/java/com/synveil/android/data/connectivity/ConnectivityObserver.kt`

### 1.10 Inbound Synchronization & Rebaseline Recovery
* **Journal Feed & Signed ACKs**: Consumes the device-scoped inbound change feed (`GET /api/v1/devices/{device_id}/libraries/{library_id}/changes`), applies updates in Room transactions, and sends signed ACK tokens (`POST /api/v1/devices/{device_id}/libraries/{library_id}/changes/ack`). Replays pending ACK tokens on recovery.
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
* **Sync Settings**: Android exposes enable/disable, ANY vs UNMETERED, a minimum 15-minute period, and a battery-not-low toggle. iOS exposes enable/disable and Wi-Fi-only vs Wi-Fi/cellular eligibility; iOS controls delivery, gates Low Power Mode, and has no direct user-selectable battery-not-low or interval control.
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
| **iOS Extension**| File Provider Extension (Files app integration) | `IOS_DEFERRED_AFTER_V0_1` | Apple `NSFileProviderReplicatedExtension` (Post-v0.1) |
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
* **Keychain Services (`Security.framework`)**: `svd1_` generic-password credentials with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` device-only protection. This is the iOS-native secure-storage equivalent; the current implementation does not create a Secure Enclave key.

### 4.3 Networking & Background Transfers
* **URLSession**: Standard async HTTP networking, custom TLS policy configuration, and bounded streaming.
* **Background URLSession (`URLSessionConfiguration.background`)**: The API exists, but authenticated background uploads and downloads are deliberately capability-off after P051 redirect review. Foreground upload/download flows remain available and verified; background file transfer is not v0.1 parity.

### 4.4 Filesystem & Previews
* **App Sandbox Filesystem**: App-private `Library/Caches` and `tmp` directories for staging 4 MiB upload chunks and temporary download staging.
* **Quick Look (`QuickLook` framework)**: In-app previewing of downloaded files (images, PDFs, documents) matching Android SAF preview intents.
* **Share Sheet (`ShareLink` / `UIActivityViewController`)**: System export and file sharing.

### 4.5 Background Execution & Scheduling
* **BackgroundTasks (`BGAppRefreshTask`)**: Default-off, bounded opportunistic metadata synchronization. It is an iOS-native equivalent for background refresh, but has no fixed cadence or Android WorkManager constraint controls.

### 4.6 Explicitly Excluded / Deferred Apple Frameworks for v0.1
* **File Provider (`NSFileProviderReplicatedExtension`)**: *DEFERRED AFTER V0.1*. Files app virtual directory integration is out of scope for baseline parity.
* **PhotoKit (`PHPhotoLibrary`)**: *DEFERRED AFTER V0.1*. Camera roll auto-upload is out of scope for v0.1 baseline parity.

### 4.7 Owner-authorized post-baseline File Provider workstream

P053–P055 are an owner-authorized extension workstream after the v0.1 baseline. P053 establishes an
isolated extension target and security contracts; it does not make Apple Files integration a v0.1
parity requirement, change v0.1 release eligibility, or establish production Files support. The
deferred classification above remains in force until later milestones implement and validate the
user-visible capability.

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
   * Invalid origins (non-root path, query, non-loopback cleartext host, invalid port) are rejected with clear error feedback. An explicit root slash is accepted and canonicalized.
   * Health check verifies `/health/live` and `/health/ready` before committing profile.

2. **Enrollment & Authentication**:
   * Accepts `sve1_` token (64 hex characters) and executes single-shot POST exchange.
   * Preflights local credential generation, stores encrypted `svd1_` credential securely in iOS Keychain.
   * Sends `Authorization: Bearer svd1_...` on all authenticated calls.
   * Restores a locally validated Keychain session at startup, recovers its canonical server endpoint when no bootstrap endpoint is configured, and enters `.authenticated` only after a successful DeviceBearer `GET /api/v1/libraries?limit=1`. Temporary network failure retains the session for explicit retry.
   * P047 offers explicit read-only inspection of eligible saved metadata during narrowly classified temporary restoration failures. The session remains verification-pending; each read revalidates Keychain identity and existing exact credential/owner/Device/origin scope binding, quarantine and projection evidence. Inspection creates no scopes or checkpoints and grants no network, mutation, ACK, rebaseline or file-content authority. Production verification records a durable authorization-check marker before its existing GET; interrupted/rejected checks remain unresolved across relaunch until server verification succeeds. Untrusted recovery storage prevents verification dispatch and retains the credential. Known authorization rejection, revocation, or metadata read denial blocks saved access; cancellation, TLS/protocol failures and untrusted storage do not enable inspection.
   * Provides confirmable local logout from the authenticated shell. Authenticated work is blocked first; restoration work is invalidated; the active Keychain credential is deleted and its absence verified before unauthenticated navigation.
   * If secure deletion fails or cannot be verified, keeps the app unauthenticated in a typed secure-storage recovery state and offers explicit retry. A failed timeout, HTTP 503, DNS lookup, backgrounding, or ordinary restoration cancellation never initiates cleanup.
   * Local logout does not revoke the bearer at the server. Device revocation remains a separate owner-authorized server operation; the local UI must say the server may continue accepting the credential until it is revoked.

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
   * Offers default-off, opportunistic metadata refresh via `BGAppRefreshTask`; iOS determines delivery and no cadence is guaranteed. Background file-content transfers remain disabled by the P051 security decision.
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

### 7.3 Foreground URLSession Staging Lifecycle
* Foreground transfer intents and chunk files are durable, but authenticated background `URLSession` upload/download is disabled under P051 because the OS follows redirects without the app's origin-validation callback. The client must reconcile the saved upload session after interruption before retry.

### 7.4 Simulator vs Physical Device Keychain Gaps
* Simulator Keychain tests do not qualify physical-device `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` behavior. The current generic-password implementation does not create a Secure Enclave key.

### Prompt028 — Authentication Error UX

Native recovery surfaces explain what failed, how saved authorization is protected,
and the available next step. Typed presentation distinguishes rejected authorization,
revocation, secure-storage unavailability, corrupt/unsupported storage, invalid
credentials, origin mismatch, TLS/protocol failures, and ambiguous enrollment.
Permanent recovery provides trusted-owner guidance without an enrollment replay or
credential-reset action. Transient restoration retains the credential and uses the
existing read-only verification retry; local logout cleanup uses verified deletion.
Diagnostics expose only bounded, allowlisted request identifiers, never server text,
secret-shaped identifiers, bearer/grant values, envelopes, or OSStatus details.


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

## P059 source-audit reconciliation

The inventory above records the original Android parity intent. The following corrections are
based on the current Android and iOS source, OpenAPI, and merged P022–P058 implementation records.

| Area | Current source-audited status |
| --- | --- |
| Server origin policy | Android accepts an absent path or `/`, canonicalizes to `/`, rejects other paths, rejects all whitespace and backslashes, caps input at 2,048 UTF-8 bytes, requires HTTPS except explicitly enabled numeric loopback HTTP. P059 aligns iOS validation and transport defaults to that policy. |
| Upload routes | Create/replace uses one immutable operation request at `POST /api/v1/upload-sessions`; chunk append and status use `PATCH`/`GET /api/v1/upload-sessions/{upload_session_id}`, followed by `/complete` or `/abort`. |
| Sync routes | Feed, checkpoint, ACK, and rebaseline are device-scoped under `/api/v1/devices/{device_id}/libraries/{library_id}`; the active feed paths end in `/changes` and `/changes/ack`. |
| Implemented and verified host-app behavior | Enrollment and Keychain session recovery/logout; paginated library/node browsing; metadata mutations with durable SQLite queueing; foreground resumable upload, verified download and export; offline projection; feed/ACK/rebaseline; notification preferences; bounded opportunistic background metadata refresh. Each claim is backed by source and the milestone tests/CI recorded in P022–P058 and the P059 verification section. |
| Valid iOS-native equivalents | SwiftUI replaces Compose; Keychain replaces Android Keystore/DataStore; app-private SQLite replaces Room; security-scoped document picker URLs and share sheets replace SAF/content URIs and Android intents; `NWPathMonitor` and `BGAppRefreshTask` provide platform-native connectivity and best-effort refresh. Delivery timing and OS grants are not identical. |
| Android-only mechanisms | WorkManager, Android Doze constraints, Room, DataStore, Android Keystore APIs, SAF `content://` URIs, and Android notification channels do not apply on iOS. The native equivalents above preserve supported product behavior where one exists. |
| Product-deferred or capability-off | Files app provider registration/content operations remain outside v0.1; iOS File Provider source stays unregistered and fail-closed, and P055 PR #139 remains unmerged and excluded. Authenticated background file transfers remain disabled by P051. PhotoKit work P056–P058 is opt-in and outside the original Android v0.1 parity inventory; it uploads ordinary FileVersions but has no verified PhotoAsset/Live Photo binding or BackupSnapshot membership/retention proof. |
| Missing or awaiting product/device evidence | iOS setup has no human-readable server profile label, profile list, or Android-style switcher; the product/access-group decision remains unresolved. Physical-device Keychain and background-scheduling evidence is unavailable. PhotoKit and Files remain post-baseline/deferred. P059 does not claim these as verified or start the P060 release gate. |

P059 also corrects the earlier iOS-only endpoint defect: before this audit, Swift accepted arbitrary
HTTP hosts and user-supplied API base paths even though the transport restricted cleartext to
loopback. The validated source now rejects non-loopback HTTP and non-root paths before session
configuration, with regression cases for policy bypasses and malformed ports. Foreground file-content
transports retain a stricter HTTPS-only policy, including debug loopback; this is a debug-only
capability difference and does not affect production HTTPS behavior.
