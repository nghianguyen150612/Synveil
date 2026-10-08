# Synveil iOS v0.1 Platform Mapping & Architectural Translation Layer

## 1. Purpose

This document establishes the authoritative architectural translation layer for the **Synveil iOS native client (v0.1)**.

It maps every Android v0.1 product capability identified in Prompt001 (`docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`) and the Android client codebase to its exact iOS implementation category, Apple platform API equivalent, shared-core reuse plan, validation strategy, and platform constraints.

This document serves as an immutable contract and constraint specification for subsequent implementation prompts (P003–P060).

---

## 2. Mapping Taxonomy

Every capability in Synveil v0.1 is classified into **exactly one** of the following six top-level categories:

1. **`SHARED_AS_IS`**: Pure, platform-agnostic Rust shared core code (`crates/core`, `crates/object-store`) consumed directly via FFI bindings without platform-specific modifications.
2. **`SHARED_WITH_ADAPTER`**: Rust shared core logic (`crates/client-sync`) re-used through a thin Swift-C-FFI / UniFFI adapter layer or platform bridge to handle storage/network abstraction.
3. **`IOS_NATIVE_EQUIVALENT`**: Feature implemented natively in Swift using idiomatic Apple frameworks (SwiftUI, URLSession, Security/Keychain, SQLite/GRDB, Network framework) to replicate Android product behavior.
4. **`ANDROID_ONLY_NOT_APPLICABLE`**: Architectural mechanism specific to the Android platform (e.g. WorkManager, Storage Access Framework `content://` URIs, Foreground Services, AndroidX DataStore, Room-specific abstractions) that does not exist on iOS and must be replaced by native iOS platform mechanisms.
5. **`IOS_DEFERRED_AFTER_V0_1`**: Capability or Apple system extension explicitly out of scope for iOS v0.1 baseline parity (e.g., File Provider Extension `NSFileProviderExtension`, PhotoKit Auto-Upload `PHPhotoLibrary`).
6. **`REQUIRES_PRODUCT_DECISION`**: Feature mapping where source code inspection reveals technical trade-offs or platform differences requiring product-owner decision before final execution.

---

## 3. Platform Principles

1. **Replicate Product Behavior, Not Android Architecture**: Synveil iOS replicates the user-visible and system-visible product behavior of Android v0.1, not its internal Android/Java software design patterns.
2. **Apple Security & Privacy Boundaries**: Hardware-backed credential security utilizes iOS Keychain Services with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` protection attributes.
3. **App Sandbox Compliance**: Filesystem operations are strictly constrained within the app container sandbox (`Library/Caches` for transfer staging, `Documents` or app-private storage for SQLite cache). No arbitrary filesystem access is assumed or attempted.
4. **Platform-Native Transfer & Background Execution**: Android persistent background services and WorkManager are replaced by Apple `BGAppRefreshTask` (BackgroundTasks framework) and `URLSessionConfiguration.background`.
5. **No Direct UI Parity Hacks**: UI is implemented using idiomatic SwiftUI patterns (`NavigationStack`, `@Observable`/`ObservableObject`), matching iOS Human Interface Guidelines rather than Compose layouts.

---

## 4. Capability Mapping Matrix

| ID | Capability | Android Source Anchor | Classification | iOS Equivalent / Target API | Shared-Core Plan | Validation | Risk / Platform Constraint |
|---|---|---|---|---|---|---|---|
| **CAP-01** | Server Profile Creation & Canonical Origin Validation | `ServerProfile.kt`, `ServerProfileRepository.kt` | `IOS_NATIVE_EQUIVALENT` | Swift `URLComponents` + IDN / regex canonical origin parser | Reuses `synveil-core` validation rules via Swift bridge | `LINUX_VERIFIABLE`, `SIMULATOR_VERIFIABLE` | Must enforce exact HTTPS rules, trailing slash normalization, and reject userinfo/query/LAN origins. |
| **CAP-02** | First-Run Startup State Machine | `StartupState.kt`, `SynveilNavHost.kt` | `IOS_NATIVE_EQUIVALENT` | Swift `@Observable` / `ObservableObject` state enum | Swift-native state machine | `LINUX_VERIFIABLE`, `SIMULATOR_VERIFIABLE` | Identical state set: `NoProfiles`, `SelectedUnconfigured`, `Testing`, `Enrolled`, `NotEnrolled`, `EnrollmentRecoveryRequired`, `AuthenticationFailed`, `DeviceRevoked`. |
| **CAP-03** | Guided Setup & Enrollment UI | `ProfileScreens.kt`, `EnrollmentScreen.kt` | `IOS_NATIVE_EQUIVALENT` | SwiftUI Views (`ProfileView`, `EnrollmentView`) | No shared core | `SIMULATOR_VERIFIABLE`, `MACOS_CI_VERIFIABLE` | Native SwiftUI form inputs, edge-to-edge layout, dynamic type support. |
| **CAP-04** | Canonical Origin & Strict TLS Policy | `SynveilHttpTransport.kt` | `IOS_NATIVE_EQUIVALENT` | `URLSessionConfiguration` with strict TLS certificate & domain policy | Swift-native transport | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | Production builds enforce HTTPS. Reject invalid/untrusted certs without user bypass. |
| **CAP-05** | Numeric Loopback HTTP Policy (Debug) | `ServerProfile.kt`, `SynveilHttpTransport.kt` | `IOS_NATIVE_EQUIVALENT` | `URLSession` debug transport policy for `127.0.0.1` / `[::1]` | Swift-native transport | `SIMULATOR_VERIFIABLE` | Strict `#if DEBUG` condition. Hostnames like `localhost` or `.local` remain forbidden even under test mode. |
| **CAP-06** | Health & Readiness Check (`/health/live`, `/health/ready`) | `SynveilHttpTransport.kt` | `IOS_NATIVE_EQUIVALENT` | `URLSessionDataTask` / async `URLSession` HTTP requests | Swift-native transport | `LINUX_VERIFIABLE`, `SIMULATOR_VERIFIABLE` | Bounded body reading (64 KiB limit). `lastConnectedAt` updated strictly on 200 OK from `/health/ready`. |
| **CAP-07** | Bounded HTTP Transport Configuration | `SynveilHttpTransport.kt` | `IOS_NATIVE_EQUIVALENT` | `URLSessionConfiguration` (timeoutIntervalForRequest, timeoutIntervalForResource) | Swift-native transport | `LINUX_VERIFIABLE`, `SIMULATOR_VERIFIABLE` | Finite timeouts, redirects explicitly disabled (`urlSession(_:task:willPerformHTTPRedirection:)`), custom User-Agent headers. |
| **CAP-08** | Enrollment Token Exchange (`sve1_` format) | `EnrollmentManager.kt`, `EnrollmentModels.kt` | `IOS_NATIVE_EQUIVALENT` | `URLSession` single-shot POST request | Swift-native transport | `LINUX_VERIFIABLE`, `SIMULATOR_VERIFIABLE` | Token regex verification (`sve1_` + 64 hex). Single-shot: zero retries, zero redirects. |
| **CAP-09** | Encrypted Credential Vault (`svd1_` pair) | `CredentialVault.kt`, `EnrollmentMetadataStore.kt` | `IOS_NATIVE_EQUIVALENT` | Apple **Keychain Services** (`Security.framework`) with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` | Swift-native security layer | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | Hardware-backed key protection via Secure Enclave / Keychain. Replaces Android Keystore + DataStore combination. |
| **CAP-10** | DeviceBearer Request Authentication | `DeviceSessionManager.kt`, `SynveilHttpTransport.kt` | `IOS_NATIVE_EQUIVALENT` | Swift `URLRequest` Authorization header composer | Swift-native transport | `LINUX_VERIFIABLE`, `SIMULATOR_VERIFIABLE` | In-memory header formatting `Authorization: Bearer svd1_...`. Zero credential exposure in logs or UI. |
| **CAP-11** | Local Credential Fencing ("Forget on device") | `CredentialVault.kt`, `ServerProfileRepository.kt` | `IOS_NATIVE_EQUIVALENT` | Keychain key deletion via `SecItemDelete` | Swift-native security layer | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | Wipes local Keychain secrets on profile delete or origin change without claiming server-side revocation. |
| **CAP-12** | Profile-Bound Device Session | `DeviceSessionManager.kt` | `IOS_NATIVE_EQUIVALENT` | `SessionController` + `SessionRestorationService` + Keychain | Swift-native session logic | `LINUX_VERIFIABLE`, `SIMULATOR_VERIFIABLE` | Restores the canonical Keychain-bound endpoint and verifies DeviceBearer authorization using bounded `GET /api/v1/libraries?limit=1` before entering `.authenticated`. |
| **CAP-13** | Multi-Profile Isolation & Switching | `ServerProfileRepository.kt`, `ProfileViewModels.kt` | `REQUIRES_PRODUCT_DECISION` | SwiftUI Profile Switcher + Keychain / UserDefaults | Swift-native session logic | `SIMULATOR_VERIFIABLE` | Requires decision on Keychain Access Group sharing across multi-profile instances and future extensions. |
| **CAP-14** | Library Discovery & Pagination (`GET /api/v1/libraries`) | `LibraryModels.kt`, `LibraryScreen.kt` | `IOS_NATIVE_EQUIVALENT` | SwiftUI `List` + `URLSessionDataTask` | Swift-native / `synveil-core` models | `SIMULATOR_VERIFIABLE`, `MACOS_CI_VERIFIABLE` | Paginated library list (100 items/page, max 64 pages, max 4096 total libraries). |
| **CAP-15** | Logical Directory Browsing (`GET /api/v1/libraries/{id}/nodes`) | `NodeModels.kt`, `NodeBrowserScreen.kt` | `IOS_NATIVE_EQUIVALENT` | SwiftUI `NavigationStack` + async cursor loader | Swift-native / `synveil-core` models | `SIMULATOR_VERIFIABLE`, `MACOS_CI_VERIFIABLE` | Paginated child node fetching (opaque 512-byte cursor). Node names treated purely as logical metadata. |
| **CAP-16** | Path Breadcrumb Navigation | `NodeBrowserScreen.kt` | `IOS_NATIVE_EQUIVALENT` | SwiftUI Breadcrumb trail view | Swift-native UI | `SIMULATOR_VERIFIABLE`, `MACOS_CI_VERIFIABLE` | Dynamic root/parent directory stack navigation. |
| **CAP-17** | Active vs Trashed Node Display | `NodeBrowserScreen.kt` | `IOS_NATIVE_EQUIVALENT` | SwiftUI segmented control / filter list | Swift-native UI | `SIMULATOR_VERIFIABLE` | Visually distinguishes active items from trashed items. |
| **CAP-18** | Outbound Metadata Mutations (mkdir, rename, move, trash, restore) | `MutationEngine.kt`, `MutationModels.kt` | `IOS_NATIVE_EQUIVALENT` | Swift `MutationEngine` + SQLite database | Reuses `synveil-core` mutation payloads | `LINUX_VERIFIABLE`, `SIMULATOR_VERIFIABLE` | Generates UUIDv7 intent IDs, base checkpoints, and optimistic local state updates. |
| **CAP-19** | Durable Mutation Queue & Offline Replay | `MutationEngine.kt`, `CacheDatabase.kt` | `IOS_NATIVE_EQUIVALENT` | GRDB.swift / SQLite durable mutation table | Swift / SQLite queue | `LINUX_VERIFIABLE`, `SIMULATOR_VERIFIABLE` | Offline mutations stored as "Pending sync" and replayed idempotently upon connectivity recovery. |
| **CAP-20** | Streaming Content Download (`GET /api/v1/nodes/{id}/content`) | `ContentOperationEngine.kt`, `TransferOperations.kt` | `IOS_NATIVE_EQUIVALENT` | `URLSessionDownloadTask` / `URLSession.bytes` | Swift-native transfer engine | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | Downloads streaming content bytes into app-private sandbox staging directory. |
| **CAP-21** | Storage Access Framework (SAF) Integration | `TransferOperations.kt` | `ANDROID_ONLY_NOT_APPLICABLE` | SwiftUI `.fileImporter` / `UIDocumentPickerViewController` | Swift-native document import | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | Android `content://` URIs do not exist on iOS. Replaced by security-scoped file URLs and App Sandbox container. |
| **CAP-22** | Transfer Staging & Storage Pressure Protection | `ContentOperationEngine.kt` | `IOS_NATIVE_EQUIVALENT` | `FileManager` free space preflight in `Library/Caches` | Swift-native storage manager | `LINUX_VERIFIABLE`, `SIMULATOR_VERIFIABLE` | Bounded staging storage (512 MiB cap, 64 MiB free space reserve). Fails deterministically before network requests. |
| **CAP-23** | Resumable Chunked Upload & Replace (4 MiB chunks) | `ContentOperationEngine.kt` | `IOS_NATIVE_EQUIVALENT` | Background `URLSessionUploadTask` / `URLSession` raw PATCH chunks | Swift-native transfer engine | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | 4 MiB raw chunk staging, SHA-256 hash calculation, session creation, offset reconciliation query. |
| **CAP-24** | Local SQLite Metadata Database | `CacheDatabase.kt`, `CacheRepository.kt` | `IOS_NATIVE_EQUIVALENT` | GRDB.swift / SQLite.swift in app `Application Support` | Swift SQLite wrapper | `LINUX_VERIFIABLE`, `SIMULATOR_VERIFIABLE` | Persists cached libraries, nodes, sync feed checkpoints, mutation queue, and rebaseline staging tables. Replaces Android Room. |
| **CAP-25** | Offline-First Browsing & Network Observer | `NodeBrowserScreen.kt`, `ConnectivityObserver.kt` | `IOS_NATIVE_EQUIVALENT` | Apple **Network framework** (`NWPathMonitor`) + SQLite reads | Swift-native observer | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | Displays cached node tree when offline with explicit offline banners. Disables destructive sync actions when offline. |
| **CAP-26** | Change Feed Consumption & Signed ACK Loop | `SyncEngine.kt`, `SyncModels.kt` | `SHARED_WITH_ADAPTER` | Swift `SyncEngine` + `synveil-client-sync` C-FFI / UniFFI bridge | `synveil-client-sync` via FFI adapter | `LINUX_VERIFIABLE`, `SIMULATOR_VERIFIABLE` | Consumes `GET /api/v1/libraries/{id}/sync/feed`, processes changes in SQLite transaction, posts signed ACK tokens (`POST /sync/ack`). |
| **CAP-27** | Rebaseline Snapshot Manifest Staging & Swap | `SyncEngine.kt` | `SHARED_WITH_ADAPTER` | Swift `SyncEngine` + SQLite staging table transaction | `synveil-client-sync` / Swift transaction | `LINUX_VERIFIABLE`, `SIMULATOR_VERIFIABLE` | Manifest page downloading, staging projection, and atomic swap upon complete verification. |
| **CAP-28** | Background Periodic & One-Time Sync Task | `SyncWorker.kt`, `SyncCoordinator.kt` | `ANDROID_ONLY_NOT_APPLICABLE` | Apple **BackgroundTasks framework** (`BGAppRefreshTask`) | Swift-native scheduler | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | Replaces Android WorkManager. Bounded by iOS system Doze/battery heuristics. Per-scope locks enforced in Swift. |
| **CAP-29** | Local Conflict Summaries & Review Banner | `SyncEngine.kt`, `NodeBrowserScreen.kt` | `IOS_NATIVE_EQUIVALENT` | SwiftUI Conflict Banner View | Swift-native UI / DB model | `SIMULATOR_VERIFIABLE`, `MACOS_CI_VERIFIABLE` | Displays "Owner/web review required" banner when server returns conflict status. |
| **CAP-30** | BrowserSession-Only Conflict Resolution Boundary | `SyncEngine.kt` | `IOS_NATIVE_EQUIVALENT` | Swift HTTP client enforcement | Swift-native transport | `LINUX_VERIFIABLE`, `SIMULATOR_VERIFIABLE` | Prevents DeviceBearer conflict resolution requests. Manual conflict resolution remains strictly a browser workflow. |
| **CAP-31** | Content Export & Sharing | `TransferOperations.kt` | `IOS_NATIVE_EQUIVALENT` | SwiftUI `ShareLink` / `UIActivityViewController` / Quick Look | Swift-native UI | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | Temporary file export and in-app file preview. Replaces Android `ACTION_SEND` intents. |
| **CAP-32** | User Sync Settings & Network Constraints | `SyncSettings.kt`, `SyncSettingsScreen.kt` | `IOS_NATIVE_EQUIVALENT` | SwiftUI Settings Form + `UserDefaults` / `BGTaskScheduler` | Swift-native settings store | `SIMULATOR_VERIFIABLE` | User controls for background sync enable/disable and network constraint selection (Wi-Fi vs Cellular via `NWPathMonitor`). |
| **CAP-33** | File Provider Extension (`NSFileProviderExtension`) | N/A | `IOS_DEFERRED_AFTER_V0_1` | Apple `NSFileProviderExtension` | Deferred | `PHYSICAL_DEVICE_ONLY` | Files app virtual directory integration deferred to P053–P055. |
| **CAP-34** | PhotoKit Background Auto-Upload (`PHPhotoLibrary`) | N/A | `IOS_DEFERRED_AFTER_V0_1` | Apple `PhotoKit` framework | Deferred | `PHYSICAL_DEVICE_ONLY` | Camera roll photo sync deferred to P056–P058. |

---

## 5. Android Concepts with No Direct iOS Equivalent

The following architectural mechanisms from Android v0.1 do not exist on iOS and are classified as `ANDROID_ONLY_NOT_APPLICABLE`:

1. **Android WorkManager & `Worker` / `ListenableWorker`**:
   * *Android Mechanism*: Android `SyncWorker` relies on WorkManager for 15-minute periodic background execution and exact constraint triggers (metered network, charging status).
   * *iOS Reality*: iOS strictly prohibits unconstrained or guaranteed periodic background workers. Background execution must use `BGAppRefreshTask` or `BGProcessingTask` from the Apple `BackgroundTasks` framework, which are scheduled at the discretion of the iOS system based on user usage patterns and battery state.

2. **Storage Access Framework (SAF), `content://` URIs, and `DocumentFile`**:
   * *Android Mechanism*: Android transfers rely on `content://` URIs produced by `ACTION_CREATE_DOCUMENT` and `ACTION_OPEN_DOCUMENT`.
   * *iOS Reality*: iOS operates strictly within an App Sandbox using standard `file://` URLs. System file selection uses `.fileImporter` or `UIDocumentPickerViewController`, which grant security-scoped access (`startAccessingSecurityScopedResource()`). Files must be copied or staged into the app's private sandbox container (`Library/Caches` or `tmp`).

3. **Android Keystore System & Preferences DataStore**:
   * *Android Mechanism*: Master AES-GCM keys are generated in Android Keystore, and encrypted payloads are stored in Proto/Preferences DataStore.
   * *iOS Reality*: iOS uses **Keychain Services** (`Security.framework`). Credentials (`svd1_` device keys) are directly added and queried as Keychain items (`SecItemAdd`, `SecItemCopyMatching`) protected by Secure Enclave hardware.

4. **Android Room Database & KSP / SQLite Drivers**:
   * *Android Mechanism*: Android uses AndroidX Room with KSP annotation processing and SQLite open helpers.
   * *iOS Reality*: iOS has no native Room library. Local persistent caching will use a lightweight Swift-native SQLite library (such as **GRDB.swift** or direct SQLite C-API wrapper).

5. **Android System Intents & Notification Channels**:
   * *Android Mechanism*: Intent actions (`ACTION_SEND`, `ACTION_VIEW`) and Android 8.0+ notification channels.
   * *iOS Reality*: Replaced by standard SwiftUI `ShareLink` / `UIActivityViewController` for export, `QuickLook` framework (`QLPreviewController`) for previews, and `UserNotifications` framework for local notifications.

---

## 6. iOS-Native Replacements

The Android mechanisms described above are replaced by native Apple platform equivalents:

| Android Mechanism | iOS-Native Replacement | Apple Framework / API | Implementation Plan |
|---|---|---|---|
| WorkManager Periodic Sync | `BGAppRefreshTask` | `BackgroundTasks.framework` | Register task identifier in `Info.plist`, submit `BGAppRefreshTaskRequest`, and execute bounded sync loop. |
| SAF `content://` URIs | Security-Scoped `file://` URLs & Sandbox Staging | `UIKit` / `SwiftUI` (`.fileImporter`, `UIDocumentPickerViewController`) | Access security-scoped resources, stage file streams in `Library/Caches`, and release access scope promptly. |
| Android Keystore + DataStore | Keychain Services | `Security.framework` (`SecItemAdd`, `SecItemCopyMatching`, `SecItemDelete`) | Store `svd1_` device tokens with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`. |
| Room SQLite Cache | GRDB.swift or SQLite C-API | `GRDB` / SQLite3 | Maintain typed relational tables for libraries, nodes, mutation queue, sync feed, and ACK buffers in `Application Support`. |
| SAF File Sharing Intent | Quick Look & Activity View Controller | `QuickLook.framework`, SwiftUI `ShareLink` | Present in-app document previews via Quick Look and present system share sheet via `UIActivityViewController`. |
| Android NetworkCallback | Network Path Monitor | `Network.framework` (`NWPathMonitor`) | Monitor interface type (Wi-Fi, Cellular) and connectivity state asynchronously. |

---

## 7. Shared-Core Reuse Summary

### 7.1 Direct Shared-Core Reuse (`SHARED_AS_IS`)
* **`synveil-core` (`crates/core`)**: Domain models, UUIDv7 parsing/generation, cryptographic SHA-256 hash utilities, token format validation (`sve1_`, `svd1_`), and timestamp handling. Completely platform-agnostic and directly consumable via UniFFI / C-FFI bindings.
* **`synveil-object-store` (`crates/object-store`)**: Object store abstractions and hashing traits.

### 7.2 Shared-Core Reuse via Adapter (`SHARED_WITH_ADAPTER`)
* **`synveil-client-sync` (`crates/client-sync`)**: Encapsulates change feed processing, signed ACK generation, rebaseline snapshot manifest staging, and conflict evaluation rules.
  * *Adapter Requirements*: Expose C-FFI / UniFFI entry points so Swift can pass HTTP response data or local database handles into Rust sync evaluation logic without coupling Rust directly to iOS platform code.

### 7.3 Swift-Native Implementation (`IOS_NATIVE_EQUIVALENT`)
* **User Interface & Navigation**: SwiftUI views, state management (`@Observable`), `NavigationStack`, breadcrumb navigation.
* **HTTP Transport**: `URLSession` async/await, custom TLS validation, custom header composition.
* **Credential Vault**: Apple Keychain Services (`Security.framework`).
* **File Operations & Staging**: `FileManager` sandbox operations, Document Picker, Quick Look, Share Sheet.
* **Background Scheduling**: `BGTaskScheduler` and `BGAppRefreshTask`.

### 7.4 Explicitly Deferred Scope (`IOS_DEFERRED_AFTER_V0_1`)
* **File Provider Extension (`NSFileProviderExtension`)**: Deferred to P053–P055.
* **PhotoKit Auto-Upload (`PHPhotoLibrary`)**: Deferred to P056–P058.

---

## 8. Product-Decision Queue

The following items are classified as `REQUIRES_PRODUCT_DECISION` and require product-owner alignment:

### Item DECISION-01: Keychain Access Group Sharing across App Extensions & Multi-Profile Switcher
* **Exact Question**: Should Synveil iOS configure an explicit Keychain Access Group (`$(AppIdentifierPrefix)com.synveil.ios.shared`) in v0.1 to allow future App Extensions (e.g. File Provider, Share Extension) to access stored `svd1_` credentials, or strictly restrict Keychain items to the primary app bundle (`kSecAttrAccessGroup` default)?
* **Why Source Code Cannot Answer It**: Android uses single-process app sandboxing where DataStore is accessible to WorkManager within the same APK. iOS App Extensions run in separate process sandboxes with separate default keychains.
* **Options**:
  1. *Option A (App-Only Default)*: Omit Keychain Access Groups in v0.1. Primary app owns all credentials. Fast execution, simpler provisioning.
  2. *Option B (Shared Access Group Foundation)*: Entitle Keychain Access Group in v0.1 foundation. Prepares for extensions without breaking existing Keychain items in later prompts.
* **Consequences**: Option A requires migration when File Provider (P053) is built. Option B requires standardizing Developer Team IDs in build configurations early.
* **Recommended Default**: **Option A for v0.1 baseline foundation**, transitioning to Option B when P053 (File Provider) commences.

---

## 9. v0.1 Deferred Items

To maintain strict scope discipline, the following items are explicitly categorized:

### 9.1 Android Concepts Not Applicable on iOS (`ANDROID_ONLY_NOT_APPLICABLE`)
* WorkManager background sync workers (`SyncWorker.kt`).
* Storage Access Framework (`content://` URIs and `DocumentFile`).
* AndroidX DataStore and Android Keystore AES-GCM wrappers.
* Android Room database ORM annotations.

### 9.2 Functionality Intentionally Deferred (`IOS_DEFERRED_AFTER_V0_1`)
* **File Provider Extension (`NSFileProviderExtension`)**: Integration into Apple Files app (Roadmap P053–P055).
* **PhotoKit Camera Roll Sync (`PHPhotoLibrary`)**: Automatic background photo backup (Roadmap P056–P058).
* **Push Notification Triggers (`APNs`)**: Remote background wakeups via Apple Push Notification service.

---

## 10. Validation Matrix

Because Synveil developer tasks execute on Linux sandboxes while iOS native code requires Apple execution tools, validation requirements are partitioned across environments:

| Environment | Applicable Capabilities | Verification Methods |
|---|---|---|
| **`LINUX_VERIFIABLE`** | - Shared Rust crate logic (`synveil-core`, `synveil-object-store`) <br>- OpenAPI schema & HTTP contract verification <br>- Android host reference suite (`./gradlew test`) <br>- Documentation & mapping taxonomy linting | `cargo test -p synveil-core -p synveil-object-store`<br>`cd clients/android && ./gradlew test`<br>Markdown syntax and taxonomy scripts |
| **`MACOS_CI_VERIFIABLE`** | - Swift compilation (`swiftc` / `xcodebuild build`) <br>- Swift Package Manager (SPM) dependency resolution <br>- SwiftUI View & ViewModel unit/mock tests <br>- SwiftLint & SwiftFormat static checks | `xcodebuild build -scheme Synveil`<br>`xcodebuild test -scheme Synveil -destination 'platform=iOS Simulator,name=iPhone 16'` |
| **`SIMULATOR_VERIFIABLE`**| - SwiftUI UI layout & state navigation <br>- `URLSession` mocked HTTP transport <br>- SQLite / GRDB persistent cache transactions <br>- In-app Quick Look preview & Document Picker mock | iOS Simulator execution via Xcode / `xcodebuild` |
| **`PHYSICAL_DEVICE_ONLY`**| - Apple Keychain Secure Enclave hardware enforcement <br>- True `BGAppRefreshTask` background Doze scheduling <br>- Cellular <-> Wi-Fi network transitions via `NWPathMonitor` <br>- Apple Developer code signing & TestFlight IPA archive | Physical iPhone testing under iOS 17.0+ |

---

## 11. Downstream Constraints for Implementation Prompts (P003–P060)

All subsequent implementation prompts must strictly obey these concrete architectural constraints:

1. **No Xcode / Swift Code in P002**: Prompt002 is documentation only. Do not bootstrap Xcode projects, SPM packages, or Swift source files until Prompt003+.
2. **Strict Taxonomy Compliance**: Future prompts must not reclassify mapped Android features or invent new classifications without updating this contract.
3. **App Sandbox Storage**: Staged upload files must reside in `Library/Caches/staging/` and obey the 512 MiB total size limit and 64 MiB free space reserve before transfer initialization.
4. **Credential Isolation**: Bearer tokens (`svd1_`) must reside strictly in Keychain Services and in-memory HTTP header formatters. Never log, serialize, or persist bearer tokens in `UserDefaults`, SQLite database tables, or UI state models.
5. **Conflict Preservation**: Never implement automatic client-side conflict resolution or DeviceBearer conflict resolution API calls. Conflicts must remain represented as local summary banners directing users to web review.
6. **No Preempting Deferred Features**: Do not introduce `FileProvider` or `PhotoKit` framework imports into early v0.1 foundational prompts.
