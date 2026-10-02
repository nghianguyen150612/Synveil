# Synveil iOS v0.1 — Shared-Core Reuse Audit & Dependency Map

## 1. Purpose

This document presents the authoritative architecture audit of the existing Synveil Rust shared core for the native iOS client (`v0.1`).

Following the feature inventory in Prompt001 (`docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`) and the platform mapping in Prompt002 (`docs/ios/IOS_PLATFORM_MAPPING.md`), Prompt003 determines with source-code evidence exactly which existing Rust components:
- Can be reused directly by iOS (`DIRECTLY_REUSABLE`);
- Require a thin adapter, FFI wrapper, or decoupled persistence interface (`REUSABLE_WITH_ADAPTER`);
- Are coupled to desktop, Android, or host OS assumptions (`PLATFORM_COUPLED`);
- Must be excluded from iOS reuse (`UNSUITABLE_FOR_IOS_REUSE`).

It establishes a concrete, minimal Rust subset for Apple target compilation in Phase C (Prompt014), identifies future FFI bridge candidates (Prompt013), details components that must remain Swift/iOS-native, and defines architectural constraints for downstream implementation prompts (P004–P060).

---

## 2. Audit Methodology

This audit was conducted via direct source inspection of all workspace crates, Cargo.toml manifests, feature flags, target-specific `cfg(...)` conditions, dependency graphs, and runtime assumptions in the Synveil repository at SHA `453969cf057af3343e9c0639b24fdc427322fe4d`.

### Primary Inputs Inspected
1. **Workspace Manifests**: Root `Cargo.toml` and 11 crate `Cargo.toml` manifests (`crates/*/Cargo.toml`).
2. **Crate Source Tree**: All 11 workspace crates (`synveil-core`, `synveil-object-store`, `synveil-client-sync`, `synveil-platform`, `synveil-client`, `synveil-desktop`, `synveil-install-engine`, `synveil-auth`, `synveil-api`, `synveil-metadata`, `synveil-storage`).
3. **Android v0.1 Reference Source**: Behavioral baseline in `clients/android/` for feature and domain parity comparison.
4. **Architectural Contracts**: `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`, `docs/ios/IOS_PLATFORM_MAPPING.md`, `docs/ios/IOS_V0_1_ROADMAP.md`, `docs/ios/PROMPT001_MANIFEST.md`, `docs/ios/manifests/PROMPT002_MANIFEST.md`, and relevant ADRs (`ADR-001`, `ADR-005`, `ADR-006`, `ADR-015`, `ADR-028`, `ADR-032`, `ADR-033`).

### Validation Criteria
- **Source-Level Verification**: Claims of reuse or platform coupling are backed by exact module paths, function signatures, dependencies, or conditional compilation directives. Crate names alone are never used to infer suitability.
- **Apple Compilation Reality**: Portability was evaluated via source audit (`SOURCE_AUDIT_PORTABLE`). No claims of verified Apple builds are made prior to execution on official Apple tooling (flagged as `APPLE_BUILD_NOT_YET_VERIFIED`).

---

## 3. Classification Taxonomy

Every audited crate, subsystem, or module receives **exactly one** primary classification:

* **`DIRECTLY_REUSABLE`**: Pure or platform-independent Rust logic that can be included in an Apple-target Rust static/dynamic library without changing its semantics or needing OS-specific abstractions.
* **`REUSABLE_WITH_ADAPTER`**: Useful Rust domain logic whose public interface, persistence model, or runtime assumptions require an iOS-facing Swift adapter, C-FFI / UniFFI bridge, async wrapper, or platform-decoupled trait implementation.
* **`PLATFORM_COUPLED`**: Logic structurally or intentionally tied to desktop UI (Qt/QML), Linux systemd/AppImage packaging, Windows process management, OS-specific IPC sockets, or desktop secret keyrings (Secret Service / Windows Credential Manager).
* **`UNSUITABLE_FOR_IOS_REUSE`**: Logic that must not be exposed to the iOS client because it is server-only (PostgreSQL/Axum/Argon2), duplicates an Apple-native responsibility (Keychain/URLSession/BGAppRefreshTask), or introduces unwanted coupling without product value.

Secondary qualifiers used in analysis notes:
- `SOURCE_AUDIT_PORTABLE`: Source code contains zero OS-specific imports and uses standard platform-agnostic Rust types.
- `APPLE_BUILD_NOT_YET_VERIFIED`: Code is portable in theory but has not yet been compiled against Apple target triples (`aarch64-apple-ios`, `aarch64-apple-ios-sim`, `x86_64-apple-ios-simulator`).

---

## 4. Rust Workspace Overview

The Synveil repository is organized as a Cargo workspace with 11 crates:

```
                          ┌───────────────────────────┐
                          │       synveil-desktop     │ (Qt / CXX-Qt Desktop UI)
                          └─────────────┬─────────────┘
                                        │
                                        ▼
                          ┌───────────────────────────┐
                          │      synveil-client       │ (Desktop Client IPC / Process Control)
                          └─────────────┬─────────────┘
                                        │
                                        ▼
                          ┌───────────────────────────┐
                          │    synveil-client-sync    │ (Sync State Engine / Rebaseline / Journal)
                          └──────┬─────────────┬──────┘
                                 │             │
                                 ▼             ▼
  ┌────────────────────────┐  ┌─────┐       ┌──────────────────────┐
  │  synveil-install-engine│  │     │       │   synveil-platform   │ (Platform Adapters / Secret Store)
  └────────────────────────┘  │     │       └──────────┬───────────┘
                              │     │                  │
  ┌────────────────────────┐  │     │                  ▼
  │      synveil-api       │  │     │       ┌──────────────────────┐
  └───────────┬────────────┘  │  S  │       │ synveil-object-store │ (Object Store Traits / Types)
              │               │  H  │       └──────────┬───────────┘
              ▼               │  A  │                  │
  ┌────────────────────────┐  │  R  │                  │
  │      synveil-auth      │  │  E  │                  │
  └───────────┬────────────┘  │  D  │                  │
              │               │     │                  │
              ▼               │  C  │                  │
  ┌────────────────────────┐  │  O  │                  │
  │    synveil-metadata    │  │  R  │                  │
  └───────────┬────────────┘  │  E  │                  │
              │               │     │                  │
              ▼               │     │                  │
  ┌────────────────────────┐  │     │                  │
  │     synveil-storage    │  │     │                  │
  └───────────┬────────────┘  │     │                  │
              │               │     │                  │
              └───────────────┼─────┼──────────────────┘
                              │     │
                              ▼     ▼
                          ┌───────────────────────────┐
                          │       synveil-core        │ (Domain Primitives / Hashes / IDs)
                          └───────────────────────────┘
```

### Client-Relevant Crates Summary
1. `synveil-core`: Domain foundation (UUIDv7, token parsing, cryptographic hashes, time, errors).
2. `synveil-object-store`: Storage abstractions, key structures, hashing stream wrappers.
3. `synveil-client-sync`: Sync engine, journal feed, signed ACK generation, rebaseline snapshot staging, conflict policy.
4. `synveil-platform`: Runtime platform detection, paths, secrets, lifecycle abstractions.
5. `synveil-client`: Desktop process orchestration, control sockets, network controller.
6. `synveil-desktop`: CXX-Qt GUI application shell and C++ bridge.
7. `synveil-install-engine`: Linux AppImage / Debian installation and update engine.
8. `synveil-auth`: Server authentication, Argon2 password hashing, session tokens.
9. `synveil-api`: Axum REST API endpoints, routing, middleware.
10. `synveil-metadata`: Server PostgreSQL database, migrations, garbage collection.
11. `synveil-storage`: Server local file object store integration.

---

## 5. Crate-Level Reuse Matrix

| Crate / Module | Responsibility | Current Consumer | Platform Assumptions | Classification | iOS Plan | Risk |
|---|---|---|---|---|---|---|
| **`synveil-core`** (`crates/core`) | Domain primitives, UUIDv7, token parsing (`sve1_`, `svd1_`), cryptographic hashes (SHA-256), error models, time wrappers. | All workspace crates | None (pure Rust `std`, no OS calls). | `DIRECTLY_REUSABLE` | Include in minimal iOS Rust static library (P014); expose via FFI (P013). | Low. Pure Rust standard library types (`SOURCE_AUDIT_PORTABLE`). |
| **`synveil-object-store`** (`crates/object-store`) | Object store traits, key models (`ObjectKey`), chunk capabilities, hashing wrappers. | `synveil-storage`, `synveil-platform`, `synveil-client-sync` | None (pure traits, `async-trait`, `bytes`, `futures-util`). | `DIRECTLY_REUSABLE` | Include core types in minimal iOS Rust subset if client object hashing or key models run in Rust. | Low. Pure async traits (`SOURCE_AUDIT_PORTABLE`). |
| **`synveil-client-sync`** (`crates/client-sync`) | Sync state machine, journal change feed consumption, signed ACK creation, rebaseline snapshot staging, conflict evaluation rules. | `synveil-client` | SQLite persistence via `sqlx`, HTTP remote via `reqwest`, notify filesystem watcher via `notify`, process runtime via `tokio`. | `REUSABLE_WITH_ADAPTER` | Decouple SQLite persistence and networking behind traits or Swift-C-FFI adapter. Reuse pure sync algorithms and state transitions. | Medium. Uses `sqlx` and `reqwest` internally; must decouple storage/network boundaries for clean FFI integration. |
| **`synveil-platform`** (`crates/platform`) | Host detection, path resolution, keyring secrets (`SecretStore`), service lifecycle. | `synveil-api`, `synveil-client`, `synveil-client-sync` | OS keyrings (Secret Service / Windows Credential Manager), host paths (`~/.config`), Linux/Windows process models. | `PLATFORM_COUPLED` | Do NOT include desktop keyring/path code in iOS Rust library. iOS uses native Swift Keychain (`Security.framework`) and App Sandbox paths. | High if pulled into iOS FFI target due to `keyring` and target OS dependencies (`windows-sys`, `sync-secret-service`). |
| **`synveil-client`** (`crates/client`) | Desktop client process launcher, UNIX domain socket control IPC, background service daemon. | `synveil-desktop` | UNIX domain sockets (`cfg(unix)`), Windows named pipes (`cfg(windows)`), desktop process lifecycle. | `UNSUITABLE_FOR_IOS_REUSE` | Exclude from iOS build entirely. iOS mobile app is a single-process foreground/background app managed by iOS lifecycle. | N/A (Excluded). |
| **`synveil-desktop`** (`crates/desktop`) | Qt / QML UI shell, CXX-Qt bridge logic. | Desktop executable | Qt 6 GUI runtime, CXX-Qt 0.10, desktop windowing. | `UNSUITABLE_FOR_IOS_REUSE` | Exclude from iOS build entirely. iOS uses native SwiftUI views. | N/A (Excluded). |
| **`synveil-install-engine`** (`crates/install-engine`) | Linux AppImage and Debian package installation/update engine. | `synveil-desktop` | Linux package managers, AppImage mount points, `nix` crate system calls. | `UNSUITABLE_FOR_IOS_REUSE` | Exclude from iOS build entirely. iOS apps are distributed via App Store / TestFlight IPAs. | N/A (Excluded). |
| **`synveil-auth`** (`crates/auth`) | Server password hashing (Argon2), database credential store, session management. | `synveil-api` | PostgreSQL metadata store, Argon2 CPU memory cost. | `UNSUITABLE_FOR_IOS_REUSE` | Exclude from iOS build. Client interacts with authentication purely via HTTP endpoints (`/api/v1/device-enrollment/exchange`). | N/A (Excluded). |
| **`synveil-api`** (`crates/api`) | Server REST API server, Axum routers, middleware, CSRF tokens. | Backend server binary | Axum HTTP server runtime, TCP listeners, PostgreSQL connection pools. | `UNSUITABLE_FOR_IOS_REUSE` | Exclude from iOS build. Server-only component. | N/A (Excluded). |
| **`synveil-metadata`** (`crates/metadata`) | Server database models, migrations, scheduled garbage collection workers. | `synveil-api`, `synveil-storage` | PostgreSQL database driver (`sqlx` postgres feature), server migrations. | `UNSUITABLE_FOR_IOS_REUSE` | Exclude from iOS build. Server metadata engine. | N/A (Excluded). |
| **`synveil-storage`** (`crates/storage`) | Server local filesystem storage engine and garbage collector. | `synveil-api` | Arbitrary server disk paths, server background workers. | `UNSUITABLE_FOR_IOS_REUSE` | Exclude from iOS build. Server physical storage engine. | N/A (Excluded). |

---

## 6. Capability-to-Shared-Core Map

This matrix maps every iOS v0.1 product capability (from `docs/ios/IOS_PLATFORM_MAPPING.md`) to its existing Rust implementation and iOS boundary responsibility:

| Capability | Prompt001/002 ID | Existing Rust Implementation | Reuse Classification | iOS Boundary / Adapter Plan | Downstream Owner |
|---|---|---|---|---|---|
| Server Profile Creation & Canonical Origin Validation | `CAP-01` | `synveil-core::config`, `synveil-core::tokens` | `DIRECTLY_REUSABLE` | Reuses canonical URL parsing rules via Swift bridge / Swift `URLComponents`. | P013 (FFI), P021 (Profile) |
| First-Run Startup State Machine | `CAP-02` | N/A (Desktop process state in `synveil-client`) | `IOS_NATIVE_EQUIVALENT` | Swift `@Observable` state enum replicating Android startup states. | P022 (Startup) |
| Guided Setup & Enrollment UI | `CAP-03` | N/A (QML screens in `synveil-desktop`) | `IOS_NATIVE_EQUIVALENT` | Native SwiftUI Views. | P023 (UI Setup) |
| Canonical Origin & Strict TLS Policy | `CAP-04` | `synveil-core::config` (Validation logic) | `IOS_NATIVE_EQUIVALENT` | Swift `URLSessionConfiguration` enforcing HTTPS certificate validation. | P025 (Networking) |
| Numeric Loopback HTTP Policy (Debug) | `CAP-05` | `synveil-core::config` | `IOS_NATIVE_EQUIVALENT` | Swift `URLSession` debug policy allowing `127.0.0.1` / `[::1]` strictly under `#if DEBUG`. | P025 (Networking) |
| Health & Readiness Check (`/health/live`, `/health/ready`) | `CAP-06` | Endpoints in `synveil-api::health` | `IOS_NATIVE_EQUIVALENT` | Swift `URLSessionDataTask` bounded to 64 KiB body. Updates `lastConnectedAt` on 200 OK. | P026 (Health) |
| Bounded HTTP Transport Configuration | `CAP-07` | HTTP settings in `synveil-client-sync::http_remote` | `IOS_NATIVE_EQUIVALENT` | Swift `URLSessionConfiguration` with finite timeouts, disabled redirects, custom User-Agent. | P025 (Networking) |
| Enrollment Token Exchange (`sve1_` format) | `CAP-08` | `synveil-core::tokens::sve1` | `DIRECTLY_REUSABLE` (Parsing) / `IOS_NATIVE_EQUIVALENT` (HTTP) | `synveil-core` validates token format (`sve1_` + 64 hex); Swift `URLSession` posts exchange. | P013 (FFI), P027 (Enrollment) |
| Encrypted Credential Vault (`svd1_` pair) | `CAP-09` | `synveil-core::tokens::svd1`, `synveil-platform::secrets` | `IOS_NATIVE_EQUIVALENT` | Apple **Keychain Services** (`Security.framework`) with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`. | P028 (Keychain) |
| DeviceBearer Request Authentication | `CAP-10` | `synveil-core::tokens::svd1` | `DIRECTLY_REUSABLE` (Format) / `IOS_NATIVE_EQUIVALENT` (Header) | `synveil-core` validates bearer string; Swift adds `Authorization: Bearer svd1_...`. | P013 (FFI), P029 (Session) |
| Local Credential Fencing ("Forget on device") | `CAP-11` | N/A | `IOS_NATIVE_EQUIVALENT` | Keychain key deletion via `SecItemDelete` on profile removal or origin change. | P028 (Keychain) |
| Profile-Bound Device Session | `CAP-12` | N/A | `IOS_NATIVE_EQUIVALENT` | Swift `SessionManager` class holding active profile ID, origin, and bearer credential handle. | P029 (Session) |
| Multi-Profile Isolation & Switching | `CAP-13` | N/A | `REQUIRES_PRODUCT_DECISION` / `IOS_NATIVE_EQUIVALENT` | Swift Profile Switcher + Keychain isolation per profile UUID. | P030 (Multi-Profile) |
| Library Discovery & Pagination (`GET /api/v1/libraries`) | `CAP-14` | API in `synveil-api::files`, models in `synveil-core` | `DIRECTLY_REUSABLE` (Models) / `IOS_NATIVE_EQUIVALENT` (HTTP) | Shared JSON models; Swift `URLSession` fetches paginated libraries (100/page, 4096 max). | P031 (Library API) |
| Logical Directory Browsing (`GET /api/v1/libraries/{id}/nodes`) | `CAP-15` | API in `synveil-api::files`, models in `synveil-core` | `DIRECTLY_REUSABLE` (Models) / `IOS_NATIVE_EQUIVALENT` (HTTP) | Shared JSON node models; Swift `URLSession` handles cursor-based pagination (512-byte max). | P032 (Node API) |
| Path Breadcrumb Navigation | `CAP-16` | N/A | `IOS_NATIVE_EQUIVALENT` | SwiftUI Breadcrumb trail view driven by node hierarchy. | P033 (Breadcrumbs) |
| Active vs Trashed Node Display | `CAP-17` | N/A | `IOS_NATIVE_EQUIVALENT` | SwiftUI list filter / segmented control. | P034 (Node UI) |
| Outbound Metadata Mutations (mkdir, rename, move, trash, restore) | `CAP-18` | `synveil-core::ids` (UUIDv7), mutation models | `DIRECTLY_REUSABLE` (Models/IDs) / `IOS_NATIVE_EQUIVALENT` (Queue) | Reuses UUIDv7 intent generation and mutation JSON payloads; Swift manages queue & optimistic state. | P013 (FFI), P035 (Mutations) |
| Durable Mutation Queue & Offline Replay | `CAP-19` | Queue logic in `synveil-client-sync::outbound` | `REUSABLE_WITH_ADAPTER` | Swift/SQLite mutation queue persisting UUIDv7 intents and replaying idempotently on reconnect. | P036 (Mutation Queue) |
| Streaming Content Download | `CAP-20` | `synveil-object-store` (Hashing stream), API in `synveil-api::downloads` | `IOS_NATIVE_EQUIVALENT` | Swift `URLSessionDownloadTask` / `URLSession.bytes` streaming bytes into App Sandbox. | P038 (Downloads) |
| Storage Access Framework (SAF) Integration | `CAP-21` | Android SAF | `ANDROID_ONLY_NOT_APPLICABLE` | Replaced by SwiftUI `.fileImporter` / `UIDocumentPickerViewController` and sandbox staging. | P039 (File Picker) |
| Transfer Staging & Storage Pressure Protection | `CAP-22` | Staging rules in `synveil-client-sync` | `IOS_NATIVE_EQUIVALENT` | Swift `FileManager` checking free space (512 MiB limit, 64 MiB reserve) in `Library/Caches/staging`. | P040 (Transfer Staging) |
| Resumable Chunked Upload & Replace (4 MiB) | `CAP-23` | Session models in `synveil-core`, `synveil-object-store::keys` | `DIRECTLY_REUSABLE` (Hashes/Keys) / `IOS_NATIVE_EQUIVALENT` (Upload) | `synveil-core` SHA-256 calculation; Swift background `URLSessionUploadTask` sends 4 MiB PATCH chunks. | P013 (FFI), P041 (Resumable Upload) |
| Local SQLite Metadata Database | `CAP-24` | Migration scripts in `synveil-client-sync::local_migrations` | `REUSABLE_WITH_ADAPTER` | Swift GRDB / SQLite cache storing libraries, nodes, sync feed checkpoints, and mutation queue. | P042 (Local DB Cache) |
| Offline-First Browsing & Network Observer | `CAP-25` | N/A | `IOS_NATIVE_EQUIVALENT` | Apple **Network framework** (`NWPathMonitor`) + SQLite offline node projection. | P043 (Offline/Network) |
| Change Feed Consumption & Signed ACK Loop | `CAP-26` | `synveil-client-sync::sync_cycle`, `synveil-client-sync::rebaseline` | `REUSABLE_WITH_ADAPTER` | Sync feed parser and signed ACK calculation in Rust FFI; Swift manages HTTP loop & SQLite write. | P013 (FFI), P044 (Sync Loop) |
| Rebaseline Snapshot Manifest Staging & Swap | `CAP-27` | `synveil-client-sync::rebaseline_convergence` | `REUSABLE_WITH_ADAPTER` | Shared rebaseline manifest validation rules via FFI; Swift handles staging table transaction & swap. | P013 (FFI), P045 (Rebaseline) |
| Background Periodic & One-Time Sync Task | `CAP-28` | WorkManager in Android `SyncWorker` | `ANDROID_ONLY_NOT_APPLICABLE` | Apple **BackgroundTasks framework** (`BGAppRefreshTask`) executing bounded Swift SyncEngine loop. | P046 (Background Sync) |
| Local Conflict Summaries & Review Banner | `CAP-29` | Conflict rules in `synveil-client-sync::conflict_policy` | `DIRECTLY_REUSABLE` (Rules) / `IOS_NATIVE_EQUIVALENT` (UI) | Pure conflict detection rules via FFI; SwiftUI renders "Owner/web review required" banner. | P013 (FFI), P047 (Conflict Banner) |
| BrowserSession-Only Conflict Resolution Boundary | `CAP-30` | `ADR-032` security rule | `IOS_NATIVE_EQUIVALENT` | Swift HTTP client blocks DeviceBearer conflict resolution API requests. Resolution remains web-only. | P048 (Conflict Boundary) |
| Content Export & Sharing | `CAP-31` | Android `ACTION_SEND` | `IOS_NATIVE_EQUIVALENT` | SwiftUI `ShareLink` / `UIActivityViewController` and Quick Look (`QLPreviewController`) previewing. | P049 (Sharing/Preview) |
| User Sync Settings & Network Constraints | `CAP-32` | N/A | `IOS_NATIVE_EQUIVALENT` | SwiftUI Settings Form + `UserDefaults` + `NWPathMonitor` constraint checks (Wi-Fi vs Cellular). | P050 (Sync Settings) |
| File Provider Extension (`NSFileProviderExtension`) | `CAP-33` | N/A | `IOS_DEFERRED_AFTER_V0_1` | Apple `NSFileProviderExtension` (Files app virtual filesystem integration). Post-v0.1. | P053–P055 (Deferred) |
| PhotoKit Background Auto-Upload | `CAP-34` | N/A | `IOS_DEFERRED_AFTER_V0_1` | Apple `PhotoKit` framework camera roll photo backup. Post-v0.1. | P056–P058 (Deferred) |

---

## 7. Networking Reuse Assessment

### Current Desktop / Sync HTTP Stack
The current Rust HTTP networking layer in `synveil-client-sync::http_remote` relies on:
- `reqwest` (`version = "0.12.28"` with `rustls-tls`, `json`, `stream`);
- `rustls` (`version = "0.23"` with `std`);
- `tokio` async runtime (`rt-multi-thread`, `io-util`).

### Assessment Findings
1. **Background Transfers**: iOS strictly requires background uploads and downloads to be managed by `URLSessionConfiguration.background`. Operating system daemons execute background HTTP requests when the application is suspended or terminated. Rust `reqwest` running inside an async Tokio runtime cannot participate in iOS background OS transfers.
2. **TLS & Certificate Validation**: Apple platform security guidelines require using system TLS APIs (`Security.framework` / `CFNetwork`). While `rustls` builds for Apple targets, forcing `rustls` inside Swift apps prevents iOS from utilizing system certificate stores, enterprise root CA profiles, and native TLS session resumption.
3. **HTTP Architecture Decision for iOS**:
   - **Do NOT reuse Rust `reqwest`/`rustls` HTTP stack on iOS.**
   - All network transport on iOS will be implemented directly in Swift using Apple **`URLSession`** (supporting both foreground async/await transfers and `URLSessionConfiguration.background` tasks).
   - **Retained Shared Logic**: Protocol JSON request/response serialization models, URI path formatting rules, header constant definitions, and response status validation logic remain shared via `synveil-core`.

---

## 8. Authentication Reuse Assessment

### Current Authentication Architecture
Authentication in Synveil involves two distinct credential levels:
1. **One-Time Enrollment Token (`sve1_`)**: 64 lowercase hex characters string validated during onboarding.
2. **DeviceBearer Keypair (`svd1_`)**: Device credentials generated preflight, sent in `Authorization: Bearer svd1_...` HTTP headers.

### Assessment Findings
1. **Credential Persistence**:
   - On Linux/Windows desktop, `synveil-platform::secrets` uses the `keyring` crate to talk to Linux Secret Service (DBus) or Windows Credential Manager.
   - On iOS, credential persistence **must use Apple Keychain Services (`Security.framework`)** with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`.
   - Desktop keyring code in `synveil-platform` depends on `sync-secret-service` and `windows-sys`, which are incompatible with iOS sandbox security.
2. **Authentication Architecture Decision for iOS**:
   - **Protocol & Parsing Logic (`DIRECTLY_REUSABLE`)**: `synveil-core::tokens` owns token format validation (`sve1_` and `svd1_` regex and checksum checks), header formatting utilities, and secret zeroization (`zeroize`). Reused directly via FFI.
   - **Credential Vault (`IOS_NATIVE_EQUIVALENT`)**: Swift native `KeychainVault` owns persistent storage, querying, and deletion of `svd1_` credentials via `SecItemAdd` / `SecItemCopyMatching` / `SecItemDelete`. Secrets must never be stored in Rust filesystems or SQLite caches.

---

## 9. File/Model/Metadata Reuse Assessment

### Current Domain Primitives
`synveil-core` contains pure, platform-independent Rust domain models:
- **`ids.rs`**: UUIDv7 generation, binary/string encoding, lexicographical sorting (`SOURCE_AUDIT_PORTABLE`).
- **`tokens.rs`**: `sve1_` enrollment token and `svd1_` device token parsing and validation (`SOURCE_AUDIT_PORTABLE`).
- **`hashes.rs`**: SHA-256 cryptographic digest computation, hex formatting, constant-time comparison via `subtle` (`SOURCE_AUDIT_PORTABLE`).
- **`time.rs`**: RFC 3339 timestamp formatting and parsing wrappers around `chrono` / `time` (`SOURCE_AUDIT_PORTABLE`).
- **`errors.rs`**: Domain error enums, error codes, and failure descriptions (`SOURCE_AUDIT_PORTABLE`).

### Assessment Findings
All models, identifier utilities, hashing tools, and validation rules in `synveil-core` are pure Rust std logic with zero OS dependencies.
- **Classification**: `DIRECTLY_REUSABLE`.
- **iOS Plan**: Include `synveil-core` in the minimal Rust library compiled for Apple targets in P014. Expose identifier generation, hashing, and token validation functions to Swift via the Prompt013 FFI boundary.

---

## 10. Sync and Conflict Semantics Reuse Assessment

### Current Client Sync Subsystem
`synveil-client-sync` contains correctness-critical sync logic:
- **`sync_cycle.rs`**: Journal change feed processing, sequence tracking, gap detection.
- **`rebaseline.rs` & `rebaseline_convergence.rs`**: Manifest snapshot page download validation, tree hash verification, rebaseline convergence checks (`ADR-028`).
- **`conflict_policy.rs`**: Deterministic client-side conflict classification rules (`ADR-032`).
- **`signatures` / `tokens`**: Signed ACK token generation (`POST /api/v1/libraries/{id}/sync/ack`).

### Assessment Findings
1. **Correctness-Critical Semantics**: Re-implementing rebaseline convergence, signed ACK math, or conflict state transitions in Swift creates severe risk of cross-platform protocol drift between Android and iOS.
2. **Coupling Obstacles**: `synveil-client-sync` currently couples this logic to `sqlx` (SQLite migrations in `local_migrations.rs`) and `reqwest` (HTTP remote in `http_remote.rs`).
3. **Sync Architecture Decision for iOS**:
   - **Classification**: `REUSABLE_WITH_ADAPTER`.
   - **iOS Plan**: Decouple the pure state machine logic (change feed evaluation, signed ACK computation, manifest verification, conflict policy) from disk/network IO. Swift will handle HTTP transfer and SQLite disk persistence, passing raw JSON feeds or manifest payloads into pure Rust FFI functions for evaluation and ACK computation.

---

## 11. Transfer-Domain Reuse Assessment

### Current Transfer Model
File transfers in Synveil consist of:
1. **Streaming Downloads**: Reading logical node content in 4 MiB chunks with SHA-256 verification.
2. **Resumable Uploads**: Creating upload sessions, staging local file chunks, computing SHA-256 hashes, appending 4 MiB raw chunks via `PATCH`, and querying server offsets (`ADR-005`).

### Assessment Findings
1. **Staging & Hashes (`DIRECTLY_REUSABLE`)**: SHA-256 hash calculation, chunk boundary math, and offset reconciliation calculation in `synveil-core` / `synveil-object-store` are pure domain logic directly reusable via FFI.
2. **Transfer Execution (`IOS_NATIVE_EQUIVALENT`)**:
   - iOS App Sandbox enforces strict storage location boundaries (`Library/Caches/staging`).
   - Transfer execution must use native Swift `URLSessionDownloadTask` and background `URLSessionUploadTask` so transfers survive app suspension.
   - Storage pressure protection (512 MiB staging limit, 64 MiB free space reserve) will be enforced natively in Swift using `FileManager` volume space APIs prior to initiating downloads/uploads.

---

## 12. Filesystem/Platform-Coupling Audit

Source inspection of the repository identified specific platform and host system assumptions that must be handled or avoided for iOS:

| Subsystem / Code Anchor | Host Assumption | iOS Reality / Constraint | Remediation Plan |
|---|---|---|---|
| `synveil-platform::paths` | Host paths (`~/.config/synveil`, `~/.local/share`, Windows `%APPDATA%`) | iOS sandbox container paths (`<Sandbox>/Library/Application Support`, `<Sandbox>/Library/Caches`, `<Sandbox>/tmp`). No arbitrary filesystem paths exist. | Do not use `synveil-platform::paths` on iOS. Swift queries native `FileManager.default.urls(for:in:)`. |
| `synveil-platform::secrets` | Desktop secret stores (`SecretService` via DBus on Linux, `CredentialManager` on Windows) | Apple **Keychain Services** (`Security.framework`). DBus and Windows APIs do not exist on iOS. | Do not include `synveil-platform::secrets` in iOS Rust subset. Use Swift native Keychain wrapper. |
| `synveil-client-sync` (`notify`) | Filesystem event monitoring (`notify` crate using `inotify` on Linux / `kqueue` on macOS) | iOS sandbox files are modified strictly by the app or user document picker. Background filesystem watchers are battery-prohibitive. | Disable `notify` dependency for iOS client target or omit observation module from iOS FFI build. |
| `synveil-client` (`launch`, `process`) | Spawning background daemon processes, UNIX domain sockets (`/tmp/synveil.sock`), Windows named pipes | Single-process mobile sandbox. iOS forbids process spawning (`fork`/`exec`) and background UNIX sockets. | Exclude `synveil-client` completely from iOS builds. |
| `synveil-install-engine` (`nix`) | POSIX mount points, Linux AppImage integration, package execution | iOS apps are packaged as signed IPAs installed by iOS SpringBoard. POSIX package tools do not exist. | Exclude `synveil-install-engine` completely from iOS builds. |

---

## 13. Apple-Target Dependency Risks

Transitive dependencies in `Cargo.toml` were audited for compilation risks on Apple iOS target triples (`aarch64-apple-ios`, `aarch64-apple-ios-sim`, `x86_64-apple-ios-simulator`):

| Dependency | Location | Risk Factor on iOS | Mitigation / Strategy |
|---|---|---|---|
| `cxx`, `cxx-qt`, `cxx-qt-lib` | Workspace `Cargo.toml` (`synveil-desktop`) | Requires CXX-Qt build tools, Qt 6 native C++ libraries, and Qt GUI frameworks. Will fail to link on iOS without full Qt iOS SDK. | Keep strictly isolated inside `synveil-desktop`. Never include in iOS client dependency tree. |
| `keyring` | Workspace `Cargo.toml` (`synveil-platform`) | Enables `sync-secret-service` (DBus) and `windows-native`. Fails or pulls heavy Linux C libraries on mobile targets. | Keep inside `synveil-platform` desktop targets (`cfg(any(target_os = "linux", target_os = "windows"))`). Omit from iOS static library. |
| `nix` | Workspace `Cargo.toml` (`synveil-install-engine`) | Invokes Linux/POSIX system calls (`nix::fs`, `nix::user`). Incompatible with iOS sandbox. | Keep isolated in `synveil-install-engine`. Omit from iOS static library. |
| `sqlx` | Workspace `Cargo.toml` (`synveil-client-sync`) | Compiles SQLite engine with C-bindings. May conflict with Swift SQLite wrappers (GRDB) if C symbols overlap (`libsqlite3`). | Swift will own SQLite persistence via GRDB/SQLite3. Avoid embedding static `sqlx` SQLite runtime inside the Rust iOS static library. |
| `notify` | Workspace `Cargo.toml` (`synveil-client-sync`) | Uses `fsevent_sys` or `kqueue`. `fsevent` requires macOS frameworks (`CoreServices`) unavailable on iOS. | Configured with `features = ["macos_kqueue"]` for macOS, but filesystem watching is omitted on iOS. |
| `reqwest` / `rustls` | Workspace `Cargo.toml` (`synveil-client-sync`) | Pulls `ring` / `aws-lc-rs` native C/assembly crypto assemblies. Increases binary size and conflicts with native iOS `URLSession`. | Omit `reqwest`/`rustls` from iOS Rust subset. Networking is performed by Swift `URLSession`. |

---

## 14. Proposed iOS Rust Subset

To ensure minimal binary size, zero platform conflicts, and clean static compilation for Apple targets in Phase C (Prompt014), the proposed Rust static library (`synveil_ios_core`) will contain **only** the following minimal subset:

```
synveil_ios_core (Static Library Target: staticlib)
├── synveil-core (Pure Domain Core)
│   ├── ids (UUIDv7 generation & parsing)
│   ├── tokens (sve1_ & svd1_ format validation)
│   ├── hashes (SHA-256 computation & verification)
│   ├── time (RFC 3339 timestamps)
│   └── errors (Domain error classification)
├── synveil-object-store (Core Types Only)
│   ├── keys (ObjectKey formatting)
│   └── capabilities (Chunk size math)
└── synveil-client-sync (Pure Logic Subset via FFI)
    ├── sync_cycle (Change feed evaluation)
    ├── rebaseline_convergence (Manifest tree hash validation)
    ├── conflict_policy (Conflict state classification rules)
    └── signatures (Signed ACK token generation)
```

### Excluded from iOS Rust Subset
- `synveil-platform` (Keyring, host paths, lifecycle daemons)
- `synveil-client` (Desktop sockets, process control)
- `synveil-desktop` (Qt/QML GUI)
- `synveil-install-engine` (AppImage/Debian packaging)
- `synveil-auth`, `synveil-api`, `synveil-metadata`, `synveil-storage` (Server crates)
- Tokio multi-threaded runtime, Reqwest HTTP stack, SQLx SQLite driver inside Rust static library.

---

## 15. Future FFI Exposure Candidates

The following explicit domain functions and data types are identified as candidate interfaces for the future C-FFI / UniFFI bridge (to be specified in Prompt013 and built in Prompt014):

### 1. Identifier & Token Domain
- `fn generate_uuidv7() -> String`: Generates time-ordered UUIDv7 string.
- `fn validate_enrollment_token(token: &str) -> bool`: Validates `sve1_` token format.
- `fn validate_device_bearer_token(token: &str) -> bool`: Validates `svd1_` credential format.

### 2. Cryptography & Hashes
- `fn compute_sha256_hex(data: &[u8]) -> String`: Computes SHA-256 digest hex string.
- `fn verify_sha256_digest(data: &[u8], expected_hex: &str) -> bool`: Constant-time hash check.

### 3. Sync & Rebaseline Semantics
- `fn parse_change_feed(json_str: &str) -> Result<ParsedFeed, SyncError>`: Validates inbound journal feed.
- `fn compute_signed_ack(library_id: &str, seq: u64, secret: &[u8]) -> String`: Calculates signed ACK token.
- `fn verify_rebaseline_manifest(manifest_json: &str) -> Result<ManifestSummary, SyncError>`: Validates snapshot tree hashes.
- `fn classify_sync_conflict(local_state: &str, remote_state: &str) -> ConflictStatus`: Pure conflict rule evaluation (`ADR-032`).

---

## 16. Components That Must Remain Swift/iOS-Native

The following responsibilities **must remain 100% Swift/iOS-native**, with zero Rust exposure:

1. **User Interface & Layout (`SwiftUI`)**: All screens, form inputs, navigation stacks, breadcrumbs, dialogs, and progress bars.
2. **Credential Vault (`Security.framework`)**: Apple Keychain Services (`SecItemAdd`, `SecItemCopyMatching`, `SecItemDelete`) with Secure Enclave protection (`kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`).
3. **HTTP Transport & Background Networking (`URLSession`)**: Foreground async/await HTTP transport, custom TLS certificate validation, and `URLSessionConfiguration.background` tasks.
4. **Local Database Persistence (`GRDB.swift` / SQLite3)**: SQLite database storage for cached libraries, nodes, mutation queue, and sync checkpoints in `<Sandbox>/Library/Application Support/`.
5. **Background Task Scheduling (`BackgroundTasks`)**: `BGAppRefreshTask` registration and scheduling under iOS system Doze heuristics.
6. **Filesystem Staging & Sandbox Management (`FileManager`)**: Managing upload/download staging in `<Sandbox>/Library/Caches/staging` and enforcing 512 MiB limit / 64 MiB free space reserve.
7. **Document Import & Previews (`UIKit` / `QuickLook`)**: `.fileImporter` document picking, `QLPreviewController` document previewing, and `UIActivityViewController` file sharing.
8. **Network Connectivity Monitoring (`Network.framework`)**: `NWPathMonitor` for monitoring Wi-Fi vs Cellular network state.

---

## 17. Required Shared-Core Changes

The following refactorings and adaptations are required in shared Rust crates before or during Phase C/D reuse, assigned to downstream prompts:

| Required Change | Purpose / Impact | Target Crate | Downstream Owner Prompt |
|---|---|---|---|
| **Decouple `synveil-client-sync` Pure Logic from `sqlx` & `reqwest`** | Allows compiling sync feed parsing, ACK calculation, and rebaseline manifest verification without linking SQLx or Reqwest into the iOS C-FFI static library. | `crates/client-sync` | Prompt013 (FFI Contract) / Prompt014 (Rust C-FFI) |
| **Expose Pure FFI Entry Points in Crate Boundary** | Provides `extern "C"` or UniFFI bridge exports for identifier generation, hashing, token parsing, and sync calculations. | `crates/core`, `crates/client-sync` | Prompt013 (FFI Contract) / Prompt014 (Rust C-FFI) |
| **Isolate `synveil-platform` Desktop Keyring Imports** | Ensures `keyring` and `windows-sys` dependencies are guarded by target OS flags so they never enter mobile build targets. | `crates/platform` | Prompt014 (Apple Target Setup) |

---

## 18. Dependency Map

The authoritative dependency direction for Synveil iOS v0.1 is established as follows:

```
               ┌───────────────────────────────────────────┐
               │    SwiftUI Views & Navigation Stack       │
               └─────────────────────┬─────────────────────┘
                                     │
                                     ▼
               ┌───────────────────────────────────────────┐
               │  Swift Domain Services & Platform Adapters│
               │  (Keychain, URLSession, GRDB, BGTasks)    │
               └─────────────────────┬─────────────────────┘
                                     │
                                     ▼
               ┌───────────────────────────────────────────┐
               │  Swift FFI Wrapper Layer (Async/Await)    │
               └─────────────────────┬─────────────────────┘
                                     │
                                     ▼
               ┌───────────────────────────────────────────┐
               │  Future C-FFI / UniFFI Bridge (Prompt013)  │
               └─────────────────────┬─────────────────────┘
                                     │
                                     ▼
               ┌───────────────────────────────────────────┐
               │  Minimal Synveil Rust iOS Subset Target   │
               │  (synveil-core + pure sync logic)         │
               └───────────────────────────────────────────┘
```

### Dependency Rules
1. **No Downstream Apple Dependencies in Generic Rust Crates**: Apple platform frameworks (`Security`, `UIKit`, `BackgroundTasks`, `CFNetwork`) must **never** be imported or referenced inside generic shared Rust crates (`synveil-core`, `synveil-client-sync`).
2. **Narrow FFI Interface**: Swift code interacts with Rust strictly through the FFI wrapper layer. Internal Rust crate structs or raw pointers must not leak into SwiftUI view models.
3. **Unidirectional Control Flow**: Swift controls application lifecycle, UI state, and network IO, calling Rust strictly as a functional library for deterministic validation, hashing, identifier generation, and sync calculation.

---

## 19. Risks and Blockers

### Identified Technical Risks
1. **FFI C-String & Memory Management Risk**: Crossing the Swift-Rust FFI boundary with allocated strings or byte buffers can cause memory leaks or undefined behavior if memory ownership is ambiguous.
   - *Mitigation*: Prompt013 will specify explicit memory ownership rules (e.g. C-compatible buffer freeing functions or UniFFI auto-generated memory management).
2. **Background Transfer Staging Eviction Risk**: iOS system low-disk cleanup may evict files in `Library/Caches` while a background `URLSessionUploadTask` is queued.
   - *Mitigation*: Staged upload files will use `resourceValue` flags (`isExcludedFromBackup`) and storage pressure pre-checks will fail deterministically before queuing tasks.
3. **SQLite Driver C-Symbol Collision**: Linking a static C-FFI Rust library containing `sqlx` SQLite bindings alongside a Swift SQLite library (GRDB) could cause duplicated symbol linker errors (`_sqlite3_open`).
   - *Mitigation*: Exclude `sqlx` from the iOS Rust subset entirely; Swift owns SQLite database creation and queries via GRDB.

---

## 20. Downstream Constraints (P004–P060)

All downstream implementation prompts must strictly enforce these concrete constraints:

1. **Strict Minimal Rust Subset (P014+)**: Do not attempt to compile `synveil-client`, `synveil-desktop`, `synveil-install-engine`, `synveil-platform`, `synveil-auth`, or `synveil-api` for Apple iOS target triples.
2. **No Rust Network or Storage Driver in iOS App**: Do not use `reqwest`, `rustls`, or `sqlx` inside the iOS client. Network IO belongs to Swift `URLSession`; local SQLite persistence belongs to Swift GRDB/SQLite3.
3. **No Keyring Crate on iOS**: Credential persistence for `svd1_` device tokens must use native Swift Keychain Services (`Security.framework`).
4. **App Sandbox Compliance**: File staging must reside in `Library/Caches/staging` and obey the 512 MiB limit and 64 MiB free space reserve before transfer initialization.
5. **Deterministic Conflict Preservation**: Never implement automatic client-side conflict resolution or DeviceBearer conflict resolution API calls. Conflicts must remain represented as local summary banners directing users to web review (`ADR-032`).
6. **No Preempting Deferred Features**: Do not introduce `FileProvider` (`NSFileProviderExtension`) or `PhotoKit` (`PHPhotoLibrary`) framework imports into early v0.1 foundational prompts.
