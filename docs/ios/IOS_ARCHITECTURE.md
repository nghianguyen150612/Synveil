# Synveil iOS v0.1 Production Architecture Specification

## 1. Purpose

This document defines the production architecture for the native **Synveil iOS client (v0.1)**.

Building upon the product contract baseline (`docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`), platform translation layer (`docs/ios/IOS_PLATFORM_MAPPING.md`), and shared-core audit (`docs/ios/IOS_SHARED_CORE_REUSE_AUDIT.md`), this document specifies explicit layer ownership, dependency rules, service boundaries, data flows, threading model, error handling, security invariants, and platform extension boundaries for all subsequent implementation prompts (P005–P060).

---

## 2. Architecture Goals

The Synveil iOS architecture is designed around six primary goals:

1. **Strict Feature Parity with Android v0.1**: Faithfully replicate the user-visible and system-visible functional surface defined in Prompt001 without introducing platform-specific behavioral divergence.
2. **Idiomatic Swift & Apple System Design**: Leverage native SwiftUI, Swift Structured Concurrency (`async`/`await`, `Actor`, `@MainActor`), `URLSession`, Keychain Services, and `BackgroundTasks` rather than forcing cross-platform desktop daemon/IPC abstractions onto iOS.
3. **Narrow Shared Rust Bridge**: Expose pure domain correctness rules, token parsing, hashing, and sync evaluation from `synveil-core` and `synveil-client-sync` through a minimal, type-safe FFI boundary without leaking Rust Tokio tasks, database handles, or desktop IPC logic into Swift.
4. **Isolated & Testable Layering**: Direct dependency flow strictly downwards. UI views must not invoke raw network sockets, raw FFI calls, Keychain APIs, or SQLite queries directly. Every service interface must be testable via deterministic mocks/fakes.
5. **Decoupled Transfer Engine**: File transfers (streaming downloads and 4 MiB resumable chunked uploads) must be managed by an independent application-level transfer engine independent of SwiftUI view lifecycle.
6. **Clean Extension Boundaries**: Reserve clean architecture extension points for future Apple platform extensions (File Provider Extension in P053–P055 and PhotoKit camera roll sync in P056–P058) without contaminating the primary in-app file browser architecture.

---

## 3. Architectural Principles

1. **Dependency Direction**: UI depends on Presentation Models/ViewModels; Presentation depends on Application/Domain Services; Infrastructure implements Domain Service protocols. Infrastructure depends on Domain interfaces; Domain NEVER depends on Infrastructure, SwiftUI, URLSession, Keychain, or FFI.
2. **UI Isolation**: SwiftUI views are pure functions of state. Views MUST NOT directly execute network calls, query SQLite, invoke FFI functions, or interact with Keychain.
3. **No Raw Secret Persistence**: Authentication secrets (`svd1_` device tokens) MUST reside strictly in Apple Keychain Services (`Security.framework`) with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` protection and in-memory HTTP header composers. Secrets MUST NEVER appear in `UserDefaults`, SQLite database tables, application logs, or SwiftUI view state.
4. **Server Authority**: Local optimistic UI states and pending mutation queues MUST NEVER be mistaken for authoritative server confirmation. Operations remain marked as "Pending sync" until acknowledged by the Synveil server.
5. **No Transfer Loss on View Teardown**: Transfer state, chunk staging, and progress tracking are owned by an infrastructure-level Transfer Engine. Navigating away from a view or backgrounding the app MUST NOT terminate active or queued transfers.
6. **No Automatic Client Conflict Resolution**: Following `ADR-032`, when server sync returns a conflict, the app persists a local conflict summary and displays an "Owner/web review required" banner. The iOS client MUST NOT attempt automated conflict resolution or issue DeviceBearer conflict resolution API calls.
7. **MainActor UI Safety**: All UI state updates and SwiftUI view bindings are strictly bound to `@MainActor`. Heavy computation, hashing, SQLite queries, and FFI bridging MUST execute off the main thread.

---

## 4. High-Level Architecture Diagram

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                          PRESENTATION LAYER                             │
│  SwiftUI Views  │  NavigationStack  │  @Observable View Models / State │
└──────────────────────────────────┬──────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                           APPLICATION LAYER                             │
│  App Launch Orchestrator  │  Session Controller  │  Sync Coordinator    │
│  Transfer Coordinator     │  Profile Switcher    │  Background Scheduler│
└──────────────────────────────────┬──────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                              DOMAIN LAYER                               │
│  Server Profile  │  Device Session  │  Logical Node / Library Models    │
│  Mutation Intent │  Sync Cursor     │  Transfer Job  │  Domain Errors   │
└──────────────────────────────────┬──────────────────────────────────────┘
                                   │
                    ┌──────────────┴──────────────┐
                    ▼                             ▼
┌───────────────────────────────────────┐ ┌───────────────────────────────┐
│        SERVICE INTERFACES             │ │     RUST BRIDGE ADAPTER       │
│  HTTP Client Protocol                 │ │  Swift RustBridge Adapter     │
│  Credential Vault Protocol            │ │  (Identifier, Hash, Sync FFI) │
│  Local Storage / Cache Protocol       │ └───────────────┬───────────────┘
│  Transfer Engine Protocol             │                 │
│  Connectivity Observer Protocol       │                 ▼
└───────────────────┬───────────────────┘ ┌───────────────────────────────┐
                    │                     │    STABLE C ABI BOUNDARY      │
                    ▼                     │ Explicit C Exports (synveil-  │
┌───────────────────────────────────────┐ │ ios-ffi static library)       │
│     INFRASTRUCTURE IMPLEMENTATIONS    │ └───────────────┬───────────────┘
│     INFRASTRUCTURE IMPLEMENTATIONS    │                 │
│  URLSession Transport (HTTP/TLS)      │                 ▼
│  Keychain Vault (Security.framework)  │ ┌───────────────────────────────┐
│  GRDB / SQLite Local Cache Store      │ │   MINIMAL SHARED RUST SUBSET  │
│  FileManager Transfer Staging Store   │ │  synveil-core                 │
│  NWPathMonitor Connectivity           │ │  synveil-object-store (types) │
│  BGAppRefreshTask Scheduler           │ │  synveil-client-sync (pure)   │
└───────────────────────────────────────┘ └───────────────────────────────┘
```

---

## 5. Layer Ownership

### 5.1 Presentation Layer
* **Components**: SwiftUI Views, `@Observable` ViewModels / State Objects, Navigation Coordinators.
* **Responsibilities**: Rendering user interface, capturing user interactions, formatting presentation state, handling breadcrumb stacks, displaying error banners, and observing ViewModel state.
* **Forbidden**: Direct network requests, direct Keychain manipulation, direct SQLite reads/writes, direct FFI invocations.

### 5.2 Application Layer
* **Components**: `AppOrchestrator`, `SessionController`, `SyncCoordinator`, `TransferCoordinator`, `BackgroundSyncScheduler`.
* **Responsibilities**: Coordinating app startup lifecycle, managing unauthenticated vs authenticated navigation boundaries, handling profile switching, orchestrating background sync tasks, triggering transfer queues, handling logout/recovery cleanup.
* **Forbidden**: Protocol/file correctness math, raw HTTP socket handling, direct SQLite SQL statements.

### 5.3 Domain Layer
* **Components**: Pure Swift Domain Models (`ServerProfile`, `DeviceSession`, `Library`, `LogicalNode`, `MutationIntent`, `TransferJob`, `SyncCheckpoint`, `DomainError`).
* **Responsibilities**: Defining domain entities, value objects, domain error types, and core state contracts.
* **Forbidden**: Imports of `SwiftUI`, `UIKit`, `URLSession`, `Security` framework, or FFI C-pointers.

### 5.4 Services / Protocol Boundary Layer
* **Components**: Swift Protocols (`HTTPTransportProtocol`, `CredentialVaultProtocol`, `LocalCacheStoreProtocol`, `TransferEngineProtocol`, `ConnectivityObserverProtocol`, `RustBridgeProtocol`).
* **Responsibilities**: Defining testable abstract contracts for infrastructure capabilities.

### 5.5 Infrastructure Layer
* **Components**: `URLSessionHTTPTransport`, `KeychainVault`, `GRDBCacheStore`, `FileManagerStagingStore`, `NWPathConnectivityObserver`, `BGTaskSyncScheduler`, `SwiftRustBridgeAdapter`.
* **Responsibilities**: Implementing service protocols using iOS-native system frameworks and calling the FFI bridge.

### 5.6 Shared Rust Core Layer
* **Components**: `synveil-core`, `synveil-object-store` (types), pure `synveil-client-sync` evaluation logic.
* **Responsibilities**: Cross-platform correctness-critical algorithms (UUIDv7, SHA-256, token format validation, change feed evaluation, signed ACK generation, rebaseline snapshot manifest tree hash verification, conflict policy rules).

---

## 6. Dependency Rules

```text
[ Presentation ] ---> [ Application ] ---> [ Domain ] <--- [ Infrastructure ]
                                              ▲
                                              │
                                   [ Service Protocols ]
```

1. **Presentation** depends ONLY on **Application** coordinators, **Domain** models, and **Service Protocols**.
2. **Application** depends ONLY on **Domain** models and **Service Protocols**.
3. **Infrastructure** implements **Service Protocols** and depends on **Domain** models.
4. **Domain** has NO external dependencies.
5. **Infrastructure** may call the **Rust Bridge Adapter**, which invokes the **FFI Boundary**.
6. **Rust Core** NEVER depends on Swift, SwiftUI, URLSession, Keychain, or Apple frameworks.

---

## 7. Presentation Architecture

### 7.1 View Model Pattern
The architecture adopts native Swift Observation via `@Observable` classes for feature view models. Redux, TCA, or VIPER third-party frameworks are explicitly forbidden.

* **View State**: Structured as value enums (`.idle`, `.loading`, `.loaded(Data)`, `.failed(PresentableError)`).
* **Navigation**: SwiftUI `NavigationStack` with typed `NavigationPath` value destinations.
* **MainActor**: All view models are annotated with `@MainActor`. View model actions dispatch asynchronous background work to domain services and publish state updates back on the `@MainActor`.

---

## 8. Application Architecture

### 8.1 Startup & Session Lifecycle State Machine
`SessionController` manages startup and state transitions:

```text
                        ┌──────────────────┐
                        │    Uninitialized │
                        └────────┬─────────┘
                                 │
                                 ▼
                        ┌──────────────────┐
                        │   NoProfiles     │
                        └────────┬─────────┘
                                 │ Profile Selected
                                 ▼
                        ┌──────────────────┐
                        │UnconfiguredTesting│
                        └────────┬─────────┘
                                 │ Health Check OK
                                 ▼
                  ┌──────────────┴──────────────┐
                  │                             │ Token Enrolled
                  ▼                             ▼
       ┌──────────────────┐           ┌──────────────────┐
       │   NotEnrolled    │           │     Enrolled     │
       └──────────────────┘           └────────┬─────────┘
                                               │
                                 ┌─────────────┼─────────────┐
                                 ▼                           ▼
                       ┌──────────────────┐        ┌──────────────────┐
                       │ Auth Failed      │        │ Device Revoked   │
                       └──────────────────┘        └──────────────────┘
```

### 8.2 Application Orchestration Tasks
* **Session Restoration**: On launch, loads active `ServerProfile` from `UserDefaults`, retrieves encrypted `svd1_` credential from `KeychainVault`, and validates server readiness via `/health/ready`.
* **Profile Switching**: Clears active session state, resets local SQLite query projections, and rebinds session context to the newly selected profile.
* **Logout & Local Cleanup**: Removes `svd1_` credentials from Keychain, wipes local cached SQLite metadata for the profile, and cancels pending transfers without attempting remote server revocation.

---

## 9. Domain Layer

### 9.1 Core Domain Entities

1. **`ServerProfile`**: `id: UUID` (UUIDv7), `label: String`, `canonicalOrigin: URL`, `transportPolicy: TransportPolicy`, `createdAt: Date`, `lastConnectedAt: Date?`.
2. **`DeviceSession`**: `profileId: UUID`, `ownerId: String`, `deviceId: String`, `bearerToken: DeviceBearerToken` (in-memory value wrapper).
3. **`Library`**: `id: UUID`, `name: String`, `ownerId: String`, `createdAt: Date`, `updatedAt: Date`.
4. **`LogicalNode`**: `id: UUID`, `libraryId: UUID`, `parentId: UUID?`, `name: String`, `isFolder: Bool`, `sizeBytes: Int64`, `sha256Hex: String?`, `isTrashed: Bool`, `updatedAt: Date`.
5. **`MutationIntent`**: `intentId: UUID` (UUIDv7), `libraryId: UUID`, `type: MutationType` (`createFolder`, `rename`, `move`, `trash`, `restore`), `payloadJSON: String`, `baseCheckpoint: Int64`, `createdAt: Date`.
6. **`TransferJob`**: `jobId: UUID`, `libraryId: UUID`, `nodeId: UUID?`, `direction: TransferDirection` (`download`, `upload`), `localStagingURL: URL`, `remoteOffset: Int64`, `totalSize: Int64`, `sha256Hex: String`, `state: TransferState`.
7. **`SyncCheckpoint`**: `libraryId: UUID`, `lastSequence: Int64`, `signedACKToken: String?`, `updatedAt: Date`.
8. **`DomainError`**: Domain-specific error representation with safe user-presentable messages and redacted diagnostic details.

---

## 10. Service & Protocol Boundaries

Every infrastructure capability sits behind a Swift protocol to allow deterministic mock testing:

```swift
// Conceptual Service Interfaces (Swift files created in later prompts)

protocol HTTPTransportProtocol: Sendable {
    func send<T: Decodable>(_ request: HTTPRequest) async throws -> HTTPResponse<T>
    func download(from url: URL, to stagingURL: URL, progress: @escaping (Int64, Int64) -> Void) async throws -> HTTPResponse<Void>
    func uploadChunk(to url: URL, chunkURL: URL, offset: Int64) async throws -> HTTPResponse<UploadChunkResponse>
}

protocol CredentialVaultProtocol: Sendable {
    func storeDeviceBearer(_ token: String, for profileId: UUID) throws
    func fetchDeviceBearer(for profileId: UUID) throws -> String?
    func deleteDeviceBearer(for profileId: UUID) throws
}

protocol LocalCacheStoreProtocol: Sendable {
    func saveLibraries(_ libraries: [Library]) async throws
    func fetchLibraries() async throws -> [Library]
    func saveNodes(_ nodes: [LogicalNode], in libraryId: UUID) async throws
    func fetchNodes(libraryId: UUID, parentId: UUID?, showTrashed: Bool) async throws -> [LogicalNode]
    func enqueueMutation(_ intent: MutationIntent) async throws
    func fetchPendingMutations() async throws -> [MutationIntent]
    func applySyncFeedTransaction(_ feed: ParsedSyncFeed) async throws
}

protocol TransferEngineProtocol: Sendable {
    func enqueueDownload(nodeId: UUID, libraryId: UUID, destinationURL: URL) async throws -> UUID
    func enqueueUpload(libraryId: UUID, parentId: UUID?, localFileURL: URL) async throws -> UUID
    func cancelTransfer(_ jobId: UUID) async
    var transferPublisher: AnyPublisher<[TransferJob], Never> { get }
}

protocol ConnectivityObserverProtocol: Sendable {
    var isConnected: Bool { get }
    var interfaceType: NetworkInterfaceType { get } // wifi, cellular, loopback
    var statusPublisher: AnyPublisher<ConnectivityStatus, Never> { get }
}

protocol RustBridgeProtocol: Sendable {
    func generateUUIDv7() throws -> String
    func validateEnrollmentToken(_ token: String) -> Bool
    func computeSHA256Hex(data: Data) throws -> String
    func computeSignedACK(libraryId: String, seq: Int64, secret: Data) throws -> String
    func parseSyncFeed(json: String) throws -> ParsedSyncFeed
    func verifyRebaselineManifest(json: String) throws -> RebaselineSummary
}
```

---

## 11. Networking Architecture

### 11.1 Transport Selection
Networking uses **Swift `URLSession`** as the primary Apple transport engine, backed by `synveil-core` for URL validation and serialization models.

### 11.2 Request & Response Rules
* **TLS Policy**: Production origins enforce strict HTTPS certificate validation using system security policies. Debug builds permit numeric loopback HTTP (`127.0.0.1`, `[::1]`) under `#if DEBUG`. Hostnames like `localhost` or `.local` remain strictly rejected.
* **Authentication Injection**: `Authorization: Bearer svd1_...` header injected dynamically by `HTTPTransport` from memory state.
* **Redirects & Retries**: Automatic HTTP redirects are explicitly disabled (`urlSession(_:task:willPerformHTTPRedirection:)`). Enrollment exchanges (`sve1_`) operate as single-shot calls with zero retries.
* **Bounded Body Reading**: Health checks (`/health/live`, `/health/ready`) and JSON metadata calls bound response body reading to maximum 64 KiB or 512 KiB buffers.
* **User-Agent Header**: Custom header string: `Synveil-iOS/0.1.0 (iOS <OSVersion>; <DeviceModel>)`.

---

## 12. Authentication & Session Architecture

```text
┌────────────────────────────────────────────────────────────────────────┐
│                          IN-MEMORY SESSION                             │
│ DeviceBearerToken ("svd1_...") kept strictly in heap memory wrapper    │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   │
                     ┌─────────────┴─────────────┐
                     ▼                           ▼
┌─────────────────────────┐         ┌──────────────────────────────────┐
│    KEYCHAIN VAULT       │         │       HTTP TRANSPORT HEADER      │
│  Security.framework     │         │ Authorization: Bearer svd1_...   │
│  Encrypted item         │         │ Memory string added to request   │
└─────────────────────────┘         └──────────────────────────────────┘
```

1. **Vault Engine**: iOS Keychain Services (`Security.framework`).
2. **Protection Class**: `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`. Keypair items are non-migratable across devices and protected by hardware Secure Enclave.
3. **Memory Bounding**: Bearer token strings are held in memory-bounded value wrappers that zeroize memory upon deallocation where possible.
4. **Local Cleanup**: "Forget on this device" executes `SecItemDelete` for the profile ID, wipes local SQLite cache, and resets session state.

---

## 13. Persistence & Storage Architecture

```text
App Sandbox Directory Structure:

<Sandbox>/
├── Documents/                             # User export files / persistent data
├── Library/
│   ├── Application Support/
│   │   └── synveil_cache.sqlite           # Durable SQLite database (GRDB)
│   ├── Caches/
│   │   └── staging/                       # Staged 4 MiB transfer chunks
│   │       ├── uploads/
│   │       └── downloads/
│   └── Preferences/
│       └── com.synveil.ios.plist          # Non-secret profile configs (UserDefaults)
└── tmp/                                   # Temporary staging files
```

### 13.1 Storage Allocation
1. **Keychain**: Encrypted `svd1_` device credentials.
2. **`UserDefaults`**: Non-secret server profiles, active profile ID, user sync preferences.
3. **SQLite Database (`Library/Application Support/synveil_cache.sqlite`)**: Cached libraries, cached nodes, mutation queue intents, sync checkpoints, rebaseline staging tables.
4. **Transfer Staging (`Library/Caches/staging/`)**: Staged 4 MiB upload chunks and streaming download files.
   * *Storage Pressure Rules*: Maximum staging limit 512 MiB; free space reserve floor 64 MiB. Transfer preflight checks space before writing.

---

## 14. Transfer Architecture

```text
Transfer UI Views
    │
    ▼
Transfer Coordinator (Application)
    │
    ▼
Transfer Engine (Infrastructure)
    ├── Staging Storage Manager (FileManager)
    ├── SHA-256 Hash Engine (Rust Bridge / FFI)
    └── URLSession Transport (URLSessionDownloadTask / URLSessionUploadTask)
```

### 14.1 Transfer State Machine
Deterministic transfer states:
* `queued`: Job created and staged in transfer database.
* `preparing`: Local file hashing, chunk slicing, or session creation in progress.
* `running`: Active `URLSession` data transfer in progress with progress callbacks.
* `paused`: Suspended due to connectivity loss or user action.
* `completed`: Remote server acknowledged transfer or download verified locally.
* `failed`: Deterministic error encountered (network, storage pressure, hash mismatch).
* `cancelled`: User or system explicitly cancelled the transfer.

### 14.2 Resumable Upload Flow (4 MiB Chunks)
1. User picks file via `.fileImporter` -> Staged in `Library/Caches/staging/uploads/`.
2. Compute total file size and SHA-256 hash via FFI.
3. Initiate upload session via `POST /api/v1/libraries/{id}/uploads` (or node replace).
4. Slice input stream into 4 MiB raw chunks.
5. Send 4 MiB chunk via `PATCH /api/v1/uploads/{session_id}`.
6. Upon ambiguous network drop, send query `GET /api/v1/uploads/{session_id}` to obtain remote offset and resume cleanly.
7. Transfer completes ONLY upon authoritative server 200/201 response.

---

## 15. File Operation Architecture

```text
User File Action (e.g. Create Folder, Rename, Delete)
    │
    ▼
Metadata Mutation Engine
    ├── 1. Generate UUIDv7 Intent ID via FFI
    ├── 2. Write Mutation Intent to SQLite Queue ("Pending sync")
    ├── 3. Apply Optimistic Update to Local SQLite Cache Projection
    └── 4. Trigger Outbound Sync Replay Task
```

* **Server Authority**: Local optimistic node updates are tagged as `isPendingSync: true`.
* **Idempotent Replay**: Mutation intents include `baseCheckpoint`. If server rejects mutation with conflict, local intent is flagged and conflict summary banner is presented.

---

## 16. Cache, Offline, and Recovery Architecture

1. **Connectivity Detection**: Apple `NWPathMonitor` continuously tracks Wi-Fi vs Cellular availability.
2. **Offline Mode**: When offline, the app renders cached nodes from local SQLite tables with a prominent "Offline - Showing Cached Data" banner. Mutation operations remain queued locally.
3. **Inbound Sync Loop**:
   * Fetch journal feed `GET /api/v1/libraries/{id}/sync/feed?since={seq}`.
   * Pass raw JSON feed to Rust Bridge for change evaluation and signed ACK computation.
   * Apply changes to local SQLite projection in a single atomic database transaction.
   * Send signed ACK token `POST /api/v1/libraries/{id}/sync/ack`.
4. **Rebaseline Snapshot Recovery**:
   * If journal history is expired, download rebaseline manifest pages.
   * Stage manifest snapshot items in SQLite rebaseline staging table.
   * Validate snapshot tree hashes via Rust Bridge.
   * Atomically swap staging table into active projection table.

---

## 17. Rust Bridge Architecture

```text
Swift Application / Service Layer
          │
          ▼
Swift RustBridge Adapter (Type-safe async Swift wrappers)
          │
          ▼
Stable C ABI Boundary (`synveil-ios-ffi` `extern "C"` functions)
          │
          ▼
iOS Rust Facade (`synveil-ios-ffi` static library)
          │
          ▼
  ┌───────┴───────────────────────┐
  ▼                               ▼
synveil-core            synveil-client-sync (pure algorithms)
```

### 17.1 Bridge Boundary Rules
1. **Allowed Across Bridge**: C-compatible primitive types, UTF-8 string buffers, byte arrays, integer timestamps, structured JSON payloads, and error code enums.
2. **Forbidden Across Bridge**: Raw database handles (`sqlx`), Tokio runtime pointers, raw socket handles, desktop process handles, platform secret keyrings.
3. **Memory Ownership**: All memory allocated by Rust and passed across the boundary MUST be freed by explicit Rust memory deallocation functions called by Swift.

---

## 18. Concurrency Model

```text
┌────────────────────────────────────────────────────────────────────────┐
│                        @MainActor (UI Thread)                          │
│  SwiftUI Views  │  Navigation  │  ViewModel State Updates             │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   │
                                   ▼
┌────────────────────────────────────────────────────────────────────────┐
│                    Background Swift Concurrency                        │
│  Actors / TaskGroups / async-await for Networking, SQLite, and FFI     │
└────────────────────────────────────────────────────────────────────────┘
```

1. **UI Execution**: SwiftUI view body evaluation and view model state updates are strictly `@MainActor`.
2. **Background Services**: Network IO (`URLSession`), SQLite disk transactions (`GRDB`), file staging (`FileManager`), and FFI calls execute in background `Task` blocks or background `Actor` isolated instances.
3. **Non-Blocking FFI**: Calls across the Rust FFI boundary MUST execute on background cooperative threads and NEVER block the `@MainActor`.

---

## 19. Error Architecture

### 19.1 Layered Error Taxonomy

```text
[ Infrastructure / Transport Error ]
               │
               ▼
       [ Service Error ]
               │
               ▼
        [ Domain Error ]
               │
               ▼
   [ User-Presentable Error ]
```

1. **Transport Error**: Raw network errors (`NSURLErrorTimedOut`, HTTP status 503).
2. **Service Error**: Service layer failures (`KeychainError.itemNotFound`, `CacheError.databaseDiskFull`).
3. **Domain Error**: Domain classification (`DomainError.invalidOrigin`, `DomainError.sessionExpired`, `DomainError.storagePressureExceeded`).
4. **User-Presentable Error**: Localized UI strings with safe actionable resolution steps ("Storage Full: Clear cache or free space on your device to continue downloading").

### 19.2 Redaction & Logging Security
* Diagnostics and log messages MUST redact all bearer tokens (`svd1_`), enrollment tokens (`sve1_`), user passwords, and raw file bytes.
* Custom `description` and `debugDescription` implementations for credentials MUST return `"[REDACTED_SECRET]"`.

---

## 20. Security & Trust Boundaries

```text
               ┌────────────────────────────────────────┐
               │         UNTRUSTED NETWORK / SERVER     │
               └───────────────────┬────────────────────┘
                                   │ HTTPS / TLS
                                   ▼
┌────────────────────────────────────────────────────────────────────────┐
│                          APP CONTAINER SANDBOX                         │
│  ┌────────────────────────┐        ┌────────────────────────────────┐  │
│  │   Keychain Services    │        │  App Private Storage           │  │
│  │  (Hardware Protected)  │        │  - SQLite Cache                │  │
│  │  - svd1_ Device Bearer │        │  - Transfer Staging            │  │
│  └────────────────────────┘        └────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────────┘
```

### 21. Explicit Security Invariants
1. **Zero Secret Leakage in Persistence**: `svd1_` credentials live strictly in Keychain Services and in-memory HTTP headers. They MUST NOT be written to `UserDefaults`, SQLite database tables, or transfer staging files.
2. **Untrusted Server Input Validation**: All server HTTP responses, JSON bodies, and file metadata MUST be validated before local persistence or UI rendering. File names are treated strictly as logical metadata, never as unsanitized local path components.
3. **App Sandbox Adherence**: All file staging and caching MUST occur inside the app container sandbox (`Library/Caches`, `Library/Application Support`).

---

## 21. Lifecycle & Background Execution Architecture

```text
State Transitions:

      [ App Launch ]
            │
            ▼
      [ Foreground Active ] <───────┐
            │                       │
            ▼                       │
      [ Background Inactive ] ──────┤
            │                       │
            ▼                       │
      [ Background Suspended ] ─────┘
            │
            ▼
      [ Terminated ]
```

1. **App Launch**: Initialize `SessionController`, restore profile/credentials, start `NWPathMonitor`, register `BGAppRefreshTask`.
2. **Background Transition**: Flush pending SQLite transactions, suspend non-essential UI timers, commit pending transfer state checkpoints to transfer database.
3. **Background Sync (`BGAppRefreshTask`)**:
   * iOS system triggers `BGAppRefreshTask` based on usage habits and battery level.
   * Worker executes a bounded SyncEngine loop (fetching journal feed, applying updates, posting ACK).
   * Task completes within 30 seconds and calls `task.setTaskCompleted(success: true)`.

---

## 22. Apple Platform Service Ownership

| Apple System API / Framework | Responsible Architectural Component | Test Seam / Mock Boundary |
|---|---|---|
| **SwiftUI** | Presentation Layer Views | View Inspector / ViewModels |
| **Security.framework (Keychain)** | `KeychainVault` (Infrastructure) | `CredentialVaultProtocol` Mock |
| **URLSession** | `URLSessionHTTPTransport` (Infrastructure) | `HTTPTransportProtocol` Mock / URLProtocol |
| **FileManager** | `FileManagerStagingStore` (Infrastructure) | Staging Store Mock / Temporary Directories |
| **Network.framework (NWPathMonitor)** | `NWPathConnectivityObserver` (Infrastructure) | `ConnectivityObserverProtocol` Mock |
| **BackgroundTasks (BGTaskScheduler)** | `BGTaskSyncScheduler` (Infrastructure) | Background Task Delegate Mock |
| **QuickLook (QLPreviewController)** | `QuickLookPreviewPresenter` (Presentation) | Preview Protocol Mock |
| **UIKit / ShareSheet (UIActivityViewController)** | `ShareSheetPresenter` (Presentation) | Share Activity Mock |
| **UniformTypeIdentifiers (UTType)** | File Type Converter Utilities | Pure Swift Helper Unit Tests |

---

## 23. File Provider Extension Boundary

* **Deferred Scope**: Reserved for P053–P055.
* **Architecture Boundary**: File Provider Extension (`NSFileProviderExtension`) runs as a separate Apple OS extension process.
* **Shared Infrastructure**: When implemented in P053, File Provider will share Domain Models, SQLite Cache Store, and Keychain Credentials via a shared App Group container (`group.com.synveil.ios`).
* **Isolation**: The core SwiftUI file browser in v0.1 MUST NOT depend on File Provider.

---

## 24. PhotoKit Extension Boundary

* **Deferred Scope**: Reserved for P056–P058.
* **Architecture Boundary**: Camera roll auto-backup will run as a background service task utilizing `PhotoKit` (`PHPhotoLibrary`).
* **Shared Infrastructure**: Photo backup will enqueue upload jobs directly into the core `TransferEngineProtocol` without modifying the logical file browser models or views.

---

## 25. Testing & Validation Strategy

```text
┌────────────────────────────────────────────────────────────────────────┐
│                          TESTING SEAMS                                 │
├──────────────────────────────┬─────────────────────────────────────────┤
│ Pure Domain & FFI Logic      │ Linux Verifiable (cargo test)           │
├──────────────────────────────┼─────────────────────────────────────────┤
│ Swift ViewModels & Services  │ macOS CI Verifiable (xcodebuild test)   │
├──────────────────────────────┼─────────────────────────────────────────┤
│ SwiftUI Layout & Navigation  │ Simulator Verifiable (iOS Simulator)    │
├──────────────────────────────┼─────────────────────────────────────────┤
│ Hardware Keychain & BGTasks  │ Physical Device Testing                 │
└──────────────────────────────┴─────────────────────────────────────────┘
```

1. **Linux Verifiable**:
   * Unit tests for `synveil-core` and `synveil-object-store` (`cargo test -p synveil-core -p synveil-object-store`).
   * Schema and documentation static analysis.
2. **macOS CI Verifiable**:
   * Swift Package Manager / Xcode compilation (`xcodebuild build`).
   * Unit tests for ViewModels, Domain Services, and Mocks (`xcodebuild test`).
3. **Simulator Verifiable**:
   * SwiftUI layout rendering, navigation stack transitions, mocked URLSession HTTP transfers, SQLite persistent cache transactions.
4. **Physical Device Only**:
   * Hardware Secure Enclave Keychain protection, `NWPathMonitor` Wi-Fi <-> Cellular transitions, real `BGAppRefreshTask` Doze execution.

---

## 26. Module / Package Layout Proposal

The proposed source layout for `clients/ios/` (to be created in Prompt006) is:

```text
clients/ios/
├── SynveilApp/                        # Application Entry Point & Launch Handlers
│   ├── SynveilApp.swift
│   └── AppDelegate.swift
├── Presentation/                      # SwiftUI Presentation Layer
│   ├── Navigation/
│   ├── Common/
│   ├── Onboarding/
│   ├── Library/
│   ├── NodeBrowser/
│   ├── Transfers/
│   └── Settings/
├── Application/                       # Application Orchestration & Coordinators
│   ├── Orchestrators/
│   ├── Session/
│   ├── Sync/
│   ├── Transfers/
│   └── Background/
├── Domain/                            # Pure Domain Models & Error Definitions
│   ├── Entities/
│   ├── ValueObjects/
│   └── Errors/
├── Services/                          # Protocol Interfaces
│   ├── Transport/
│   ├── Security/
│   ├── Storage/
│   ├── Transfers/
│   └── RustBridge/
├── Infrastructure/                    # Concrete Platform Implementations
│   ├── Network/
│   ├── Persistence/
│   ├── Security/
│   ├── Files/
│   ├── Transfers/
│   └── RustBridge/
└── Tests/                             # Unit & Integration Test Suites
    ├── DomainTests/
    ├── ViewModelTests/
    ├── ServiceTests/
    └── Mocks/
```

---

## 27. Architecture Invariants

All developers and LLM agents working on the Synveil iOS client MUST obey these strict architectural invariants:

1. **INVARIANT-01**: Bearer tokens (`svd1_`) MUST live strictly in Keychain Services and memory HTTP request header compositors. They MUST NEVER be stored in `UserDefaults`, SQLite, or logs.
2. **INVARIANT-02**: Transfer engine execution MUST be independent of SwiftUI view lifecycle. Navigating away from a screen MUST NOT cancel an ongoing transfer.
3. **INVARIANT-03**: No transfer operation shall report success to the user without authoritative server acknowledgement (or local verification for downloads).
4. **INVARIANT-04**: Client code MUST NOT execute automatic conflict resolution or invoke DeviceBearer conflict resolution API endpoints (`ADR-032`).
5. **INVARIANT-05**: MainActor thread MUST NOT perform synchronous disk IO, heavy cryptographic hashing, SQLite transactions, or blocking FFI calls.
6. **INVARIANT-06**: Generic Rust crates MUST NOT import or depend on Apple platform frameworks (`Security`, `UIKit`, `CFNetwork`).

---

## 28. Unresolved Decisions

1. **Keychain Access Group Entitlement (`DECISION-01`)**:
   * *Status*: Unresolved product decision from Prompt002.
   * *Architecture Mitigation*: v0.1 foundation uses default app bundle Keychain isolation (`kSecAttrAccessGroup` omitted). The `KeychainVault` protocol interface is designed to accept an optional access group identifier parameter without breaking callers when File Provider (P053) introduces shared App Groups later.

---

## 29. Downstream Constraints for Prompts P005–P060

* **P005 (Validation & CI Architecture)**: Must configure macOS CI workflow for `xcodebuild` without altering the dependency boundaries defined herein.
* **P006 (Repository Layout)**: Must instantiate the `clients/ios/` directory tree strictly following Section 26.
* **P007 (Xcode Bootstrap)**: Must set deployment target to iOS 17.0+ and enforce Swift Structured Concurrency warnings (`SWIFT_STRICT_CONCURRENCY = complete`).
* **P013–P020 (Rust/Swift Bridge)**: Must implement the narrow FFI boundary without importing Tokio or Reqwest into the iOS C-static library.
* **P021–P028 (App/Auth)**: Must implement `KeychainVault` using `Security.framework` and enforce token zeroization.
* **P029–P037 (File Browser)**: Must implement SwiftUI Views and ViewModels driven by local SQLite cached projections.
* **P038–P047 (Transfers/Cache/Offline)**: Must implement the decoupled `TransferEngine` with 4 MiB resumable uploads and 512 MiB staging pressure limits.
* **P048–P052 (Native iOS Experience)**: Must integrate `BGAppRefreshTask`, Quick Look, and Share Sheet.
* **P053–P055 (File Provider)**: Must build `NSFileProviderExtension` as a separate extension subsystem utilizing shared App Group storage.
* **P056–P058 (PhotoKit)**: Must build camera roll sync on top of the shared `TransferEngine`.
