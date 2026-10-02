# Synveil iOS Native Client (`clients/ios/`)

## 1. Purpose
`clients/ios/` is the canonical home of the native Synveil iOS client (`v0.1`). It establishes a clean, production-grade Swift application layout adhering strictly to ADR-058 (`docs/adr/ADR-058-ios-v0.1-client-architecture.md`), `IOS_ARCHITECTURE.md`, and `IOS_VALIDATION_CI_ARCHITECTURE.md`.

## 2. Current Project Status
- **Phase**: Repository Foundation (Prompt006).
- **Status**: Directory structure and architectural boundary documentation established.
- **Xcode Project Bootstrap**: Reserved for Prompt007 (`clients/ios/Synveil.xcodeproj`). Zero `.xcodeproj` or Swift source files exist in Prompt006.

## 3. Canonical Directory Tree

```text
clients/ios/
├── README.md                          # Client layout & architecture specification
├── App/                               # Application entry point & composition root
│   └── README.md
├── Features/                          # SwiftUI Presentation Layer
│   └── README.md                      # Onboarding, Auth, FileBrowser, Transfers, Settings
├── Application/                       # Application Orchestration & Use-Cases
│   └── README.md                      # AppOrchestrator, SessionController, SyncCoordinator
├── Domain/                            # Pure Swift Domain Entities & Service Protocols
│   └── README.md                      # ServerProfile, LogicalNode, TransferJob, DomainError
├── Infrastructure/                    # Concrete Platform & System Implementations
│   ├── README.md
│   ├── Network/                       # URLSession HTTP/TLS transport & header injection
│   ├── Persistence/                   # Durable non-secret SQLite database cache (GRDB)
│   ├── Security/                      # Security.framework Keychain vault for svd1_ bearer
│   ├── Files/                         # FileManager staging store & sandbox file export
│   ├── Transfers/                     # Resumable 4 MiB upload & streaming download engine
│   └── RustBridge/                    # Swift adapter wrapping narrow Rust FFI boundary
├── Extensions/                        # Deferred Apple Platform Extensions
│   └── README.md                      # FileProvider (P053) & PhotoKit (P056) reservations
├── Resources/                         # Application Resources & Asset Catalogs
│   └── README.md                      # Asset catalogs, String catalogs, Info.plist
├── Tests/                             # Test Suite Organization
│   └── README.md                      # Unit, Integration, Architecture tests & Mocks
└── Support/                           # Repository Scripts & Reference Configs
    └── README.md
```

## 4. Directory Ownership & Responsibilities

| Directory | Future Ownership & Responsibilities |
|---|---|
| `App/` | `@main` `SynveilApp` entry point, `AppDependencyContainer` composition root, lifecycle observation (`scenePhase`), root navigation state switching. |
| `Features/` | Feature-facing SwiftUI views, `@Observable` ViewModels (`@MainActor`), presentation state value enums, navigation routing. |
| `Application/` | Use-case coordinators (`SessionController`, `SyncCoordinator`, `TransferCoordinator`, `BackgroundSyncScheduler`). UI-independent orchestration. |
| `Domain/` | Pure Swift domain entities, value objects, domain error taxonomy, and service protocol contracts (`HTTPTransportProtocol`, `CredentialVaultProtocol`, `LocalCacheStoreProtocol`, `RustBridgeProtocol`). |
| `Infrastructure/` | Concrete service implementations using Apple frameworks (`URLSession`, `Security`, `FileManager`, `GRDB`) and Rust FFI adapter. |
| `Extensions/` | Platform extensions reservation: `FileProvider/` (`NSFileProviderExtension` in P053) and `PhotoKit/` camera roll sync (in P056). |
| `Resources/` | Asset catalogs (`Assets.xcassets`), String localizations (`Localizable.xcstrings`), privacy usage keys (`Info.plist`). |
| `Tests/` | Unit test suites, service integration tests with in-memory SQLite/URLProtocol mocks, architecture boundary tests, test doubles (`Mocks/`). |
| `Support/` | Build support scripts, repository static checks, and reference configurations. |

## 5. Allowed Dependency Direction

The architecture enforces strict unidirectional dependency flow down to pure domain entities and abstract service interfaces:

```text
               ┌───────────────────────┐
               │     App / Features    │
               └───────────┬───────────┘
                           │
                           ▼
               ┌───────────────────────┐
               │      Application      │
               └───────────┬───────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────┐
│                       Domain                        │
│  (Entities, Value Objects, Domain Errors, Protocols)│
└──────────────────────────▲──────────────────────────┘
                           │
                           │ Implements Service Protocols
                           │
┌──────────────────────────┴──────────────────────────┐
│                   Infrastructure                    │
│   Network │ Persistence │ Security │ Files │        │
│   Transfers │ RustBridge                            │
└─────────────────────────────────────────────────────┘
```

More explicitly:
- `Features` depends on `Application` coordinators, `Domain` entities, and Service Protocols.
- `Application` depends on `Domain` entities and Service Protocols.
- `Infrastructure` implementations depend on `Domain` protocols and entities.
- `Domain` has **zero external dependencies** (depends only on Swift `Foundation`).

## 6. Prohibited Dependencies
- **SwiftUI / UIKit in Domain or Infrastructure**: Domain and Infrastructure layers MUST NOT import `SwiftUI` or `UIKit`.
- **System Framework Leaks in Domain**: Domain MUST NOT import `URLSession`, `Security.framework` (Keychain), or `SQLite`/`GRDB`.
- **Direct Infrastructure Calls in UI**: SwiftUI views MUST NOT invoke raw `URLSession` network calls, raw Keychain operations, raw SQLite queries, or C-FFI Rust functions.
- **Circular Layer Dependencies**: Layers must never depend on higher layers.

## 7. Reserved Locations for Future Increments
- **Xcode Project Location (P007)**: `clients/ios/Synveil.xcodeproj` (inside `clients/ios/`, not at repository root).
- **Test Organization (P007–P009)**: `clients/ios/Tests/` (contains `UnitTests/`, `IntegrationTests/`, `ArchitectureTests/`, `Mocks/`).
- **Application Resources**: `clients/ios/Resources/` (`Assets.xcassets`, `Localizable.xcstrings`).
- **Rust Bridge Boundary (P013–P016)**: `clients/ios/Infrastructure/RustBridge/` (Swift wrapper around C-static library `synveil_ios_core`).
- **File Provider Extension (P053–P055)**: `clients/ios/Extensions/FileProvider/` (separate Apple OS extension process sharing App Group `group.com.synveil.ios`).
- **PhotoKit Extension (P056–P058)**: `clients/ios/Extensions/PhotoKit/`.

## 8. Build, Secret, and Artifact Policies

### 8.1 Temporary Build Outputs (Untracked)
The following local outputs must NEVER enter source control:
- Xcode `DerivedData/` and `build/` directories.
- Xcode workspace user state (`*.xcodeproj/xcuserdata/`, `*.xcworkspace`).
- Compiled binaries, `.app` bundles, `.ipa` packages, `.dSYM` symbols, `.xcarchive` archives.
- Temporary XCFrameworks or Rust static libraries (`*.a`, `*.xcframework`).
- Local Rust build targets specific to iOS (`target/aarch64-apple-ios/`, `target/x86_64-apple-ios/`).

### 8.2 Secret and Signing Policy
The following security artifacts are strictly forbidden from source control:
- Apple provisioning profiles (`*.mobileprovision`, `*.provisionprofile`).
- Certificate private keys, `.p12` files, `.cer` files, `.pem` keys.
- Apple account credentials, App Store Connect API keys.
- Real device bearer tokens (`svd1_`), enrollment tokens (`sve1_`), or user passwords.
- Personal server credentials or private TLS keys.

### 8.3 Generated Code Policy
- Generated Swift/C bridge bindings (from UniFFI or C-FFI generators) will live in `clients/ios/Infrastructure/RustBridge/` once established in Phase C (P013–P016).
- Hand-editing generated bridge bindings is forbidden.
- Temporary build outputs remain untracked in `.gitignore`.

## 9. Architecture Invariants
1. **INVARIANT-01**: Bearer tokens (`svd1_`) reside strictly in `Security.framework` Keychain Services (`kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`) and memory HTTP request header compositors. They MUST NEVER be stored in `UserDefaults`, SQLite, application logs, or SwiftUI view models.
2. **INVARIANT-02**: The Transfer Engine is an application-level infrastructure service decoupled from SwiftUI view lifetimes. Navigating away from a view or backgrounding the app MUST NOT terminate active transfers.
3. **INVARIANT-03**: SwiftUI views do not directly invoke raw network transport, Keychain, SQLite, or C-FFI Rust functions. Views interact solely through ViewModels and Application coordinators.
4. **INVARIANT-04**: Domain code does not know about concrete UI, network, Keychain, or SQLite technologies.
5. **INVARIANT-05**: Transfer success is reported ONLY upon authoritative server 200/201 acknowledgment or local verification.
6. **INVARIANT-06**: Client code MUST NOT attempt automated conflict resolution or issue DeviceBearer conflict resolution API calls (`ADR-032`).

## 10. Source Ownership Examples

| Task / Entity | Canonical Destination |
|---|---|
| Server profile domain entity | `Domain/Entities/ServerProfile.swift` |
| Session startup orchestration | `Application/Session/SessionController.swift` |
| Server profile onboarding screen | `Features/Onboarding/ProfileOnboardingView.swift` |
| URLSession HTTP transport | `Infrastructure/Network/URLSessionHTTPTransport.swift` |
| Keychain credential vault | `Infrastructure/Security/KeychainVault.swift` |
| Durable SQLite cache store | `Infrastructure/Persistence/GRDBCacheStore.swift` |
| Resumable transfer engine | `Infrastructure/Transfers/TransferEngine.swift` |
| Swift Rust Bridge wrapper | `Infrastructure/RustBridge/SwiftRustBridgeAdapter.swift` |
| Mock HTTP transport test double | `Tests/Mocks/MockHTTPTransport.swift` |

## 11. Repository Convention Audit

The `clients/ios/` layout is designed specifically for Swift and Xcode maintainability while aligning with Synveil monorepo conventions:
- **vs. `clients/android/`**: Android uses a single Gradle module (`app/src/main/java/com/synveil/android/`) with Jetpack Compose. iOS uses standard Swift architectural layering (`App/`, `Features/`, `Application/`, `Domain/`, `Infrastructure/`) tailored for SwiftUI and Xcode targets without copying Android-specific Kotlin DSL or Room structures.
- **vs. Shared Rust Crates (`crates/`)**: Shared Rust crates (`synveil-core`, `synveil-client-sync`) are compiled into a minimal C-static library (`synveil_ios_core`). iOS consumes them via `Infrastructure/RustBridge/` without embedding Tokio or SQLx runtimes into Swift.
- **vs. Desktop / Web Shells**: Desktop Qt and Web React trees use desktop IPC sockets and web bundlers. iOS avoids process IPC and desktop daemons, operating fully within the mobile sandbox using Apple system APIs.
