# Synveil iOS Native Client (`clients/ios/`)

## 1. Purpose
`clients/ios/` is the canonical home of the native Synveil iOS client (`v0.1`). It establishes a clean, production-grade Swift application layout adhering strictly to ADR-058 (`docs/adr/ADR-058-ios-v0.1-client-architecture.md`), `IOS_ARCHITECTURE.md`, and `IOS_VALIDATION_CI_ARCHITECTURE.md`.

## 2. Current Project Status
- **Phase**: Configuration, Server URL & Safe Defaults (Prompt011).
- **Status**: Dedicated GitHub Actions static validation gate (`.github/workflows/ios-static-validation.yml`), build gate (`.github/workflows/ios-build.yml`), and Simulator test gate (`.github/workflows/ios-simulator-tests.yml`) established.
- **Application Target**: `Synveil` (Swift + SwiftUI, bundle identifier `com.synveil.ios`, deployment target iOS 17.0).
- **Test Target**: `SynveilTests` (XCTest Unit Testing Bundle, bundle identifier `com.synveil.ios.tests`).
- **Signing Policy**: Configured for unsigned Simulator builds (`CODE_SIGNING_ALLOWED=NO`, zero committed team IDs or provisioning profiles).

## 3. Configuration & Server Endpoint Architecture (Prompt011)

### 3.1 Non-Secret Configuration Ownership
Configuration answers what non-secret parameters the app starts with, distinct from persisted user server profiles or authenticated sessions.
- **`ServerEndpoint`** (`Domain/Configuration/ServerEndpoint.swift`): Pure immutable value object representing normalized server base endpoints.
- **`AppEnvironment`** (`Application/Configuration/AppEnvironment.swift`): Application execution environment (`.production`, `.development`, `.testing`).
- **`AppConfiguration`** (`Application/Configuration/AppConfiguration.swift`): App composition configuration holding `environment` and optional `serverEndpoint`.

### 3.2 Endpoint Validation & Normalization Rules
`ServerEndpoint(validating: rawString)` enforces strict safety rules using `URLComponents`:
- **Allowed Schemes**: `https` (preferred/production) and `http` (local development/home-LAN self-hosting).
- **Rejected Inputs**: Empty/whitespace inputs, missing schemes, unsupported schemes (`ftp`, `file`), missing hosts, userinfo credentials (`user:pass@`), query parameters (`?key=val`), and fragments (`#anchor`).
- **Deterministic Normalization**: Trims surrounding whitespace, lowercases scheme/host, normalizes trailing slashes (preserves root `/` or subpath without trailing slash).

### 3.3 Safe Production Defaults
- In production builds, `AppConfiguration.serverEndpoint` defaults strictly to `nil` (unconfigured server).
- `localhost` and `127.0.0.1` are **never** used as production defaults.
- Developers can pass non-secret process environment variables (`SYNVEIL_SERVER_URL` and `SYNVEIL_APP_ENV`) during local testing without mutating global state.

### 3.4 Secret Boundary Policy
- Configuration is strictly **non-secret**.
- No bearer tokens (`svd1_`), enrollment tokens (`sve1_`), passwords, private keys, or personal server URLs are permitted in configuration, source code, or `.xcconfig` files.

### 3.5 Test Configuration Injection
Tests construct explicit `AppConfiguration` value instances or inject mock environment dictionaries into `AppConfiguration.load(processEnvironment:)` without mutating process-wide state.

## 4. Canonical Directory Tree

```text
clients/ios/
├── README.md                          # Client layout & architecture specification
├── .swift-format                      # Canonical Swift formatter rules configuration
├── Synveil.xcodeproj/                 # Native Xcode project & shared scheme
│   ├── project.pbxproj
│   └── xcshareddata/xcschemes/Synveil.xcscheme
├── App/                               # Application entry point & composition root
│   ├── SynveilApp.swift               # @main App entry point
│   ├── BootstrapView.swift            # Minimal bootstrap root view
│   └── README.md
├── Features/                          # SwiftUI Presentation Layer
│   └── README.md                      # Onboarding, Auth, FileBrowser, Transfers, Settings
├── Application/                       # Application Orchestration & Use-Cases
│   ├── Configuration/                 # AppEnvironment, AppConfiguration
│   └── README.md                      # AppOrchestrator, SessionController, SyncCoordinator
├── Domain/                            # Pure Swift Domain Entities & Service Protocols
│   ├── Configuration/                 # ServerEndpoint, EndpointValidationError
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
│   ├── Assets.xcassets/               # AppIcon and AccentColor asset catalog
│   └── README.md                      # Asset catalogs, String catalogs, Info.plist
├── Tests/                             # Test Suite Organization
│   ├── SynveilTests/                  # Canonical unit test target source
│   └── README.md                      # Unit, Integration, Architecture tests & Mocks
└── Support/                           # Repository Scripts & Reference Configs
    ├── validate_ios_sources.py        # Static source & architecture validator
    ├── tests/                         # Validator unit self-tests
    └── README.md
```

## 5. Command-Line Build & Continuous Integration Contract

### 5.1 iOS Static Validation Gate (`.github/workflows/ios-static-validation.yml`)
Enforces formatting consistency and repository/architectural invariants:

```bash
# Developer formatting command (in-place source formatting)
swift format --recursive --in-place clients/ios
# or: swift-format format --recursive --in-place clients/ios

# Non-mutating CI formatting check
swift format lint --recursive --strict clients/ios
# or: swift-format lint --recursive --strict clients/ios

# Execute static source validator and self-tests
python3 -m unittest discover -s clients/ios/Support/tests
python3 clients/ios/Support/validate_ios_sources.py
```

### 5.2 iOS Build Gate (`.github/workflows/ios-build.yml`)
To build the native iOS project compilation target on macOS with Xcode tooling locally or in CI:

```bash
xcodebuild \
  -project clients/ios/Synveil.xcodeproj \
  -scheme Synveil \
  -configuration Debug \
  -sdk iphonesimulator \
  CODE_SIGNING_ALLOWED=NO \
  -derivedDataPath "${RUNNER_TEMP:-/tmp}/SynveilDerivedData" \
  build
```

### 5.3 iOS Simulator Test Gate (`.github/workflows/ios-simulator-tests.yml`)
To execute XCTest suites inside a booted iOS Simulator destination locally or in CI:

```bash
xcodebuild \
  -project clients/ios/Synveil.xcodeproj \
  -scheme Synveil \
  -configuration Debug \
  -destination "platform=iOS Simulator,id=$SIMULATOR_UDID" \
  CODE_SIGNING_ALLOWED=NO \
  -derivedDataPath "${RUNNER_TEMP:-/tmp}/SynveilDerivedData" \
  -resultBundlePath "${RUNNER_TEMP:-/tmp}/SynveilTests.xcresult" \
  test
```

## 6. Directory Ownership & Responsibilities

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
| `Support/` | Build support scripts, static source validator (`validate_ios_sources.py`), and reference configurations. |

## 7. Allowed Dependency Direction

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
└──────────────────────────┴──────────────────────────┘
```

More explicitly:
- `Features` depends on `Application` coordinators, `Domain` entities, and Service Protocols.
- `Application` depends on `Domain` entities and Service Protocols.
- `Infrastructure` implementations depend on `Domain` protocols and entities.
- `Domain` has **zero external dependencies** (depends only on Swift `Foundation`).

## 8. Prohibited Dependencies
- **SwiftUI / UIKit in Domain or Infrastructure**: Domain and Infrastructure layers MUST NOT import `SwiftUI` or `UIKit`.
- **System Framework Leaks in Domain**: Domain MUST NOT import `URLSession`, `Security.framework` (Keychain), or `SQLite`/`GRDB`.
- **Direct Infrastructure Calls in UI**: SwiftUI views MUST NOT invoke raw `URLSession` network calls, raw Keychain operations, raw SQLite queries, or C-FFI Rust functions.
- **Circular Layer Dependencies**: Layers must never depend on higher layers.

## 9. Build, Secret, and Artifact Policies

### 9.1 Temporary Build Outputs (Untracked)
The following local outputs must NEVER enter source control:
- Xcode `DerivedData/` and `build/` directories.
- Xcode workspace user state (`*.xcodeproj/xcuserdata/`, `*.xcworkspace`).
- Compiled binaries, `.app` bundles, `.ipa` packages, `.dSYM` symbols, `.xcarchive` archives.
- Temporary XCFrameworks or Rust static libraries (`*.a`, `*.xcframework`).
- Local Rust build targets specific to iOS (`target/aarch64-apple-ios/`, `target/x86_64-apple-ios/`).

### 9.2 Secret and Signing Policy
The following security artifacts are strictly forbidden from source control:
- Apple provisioning profiles (`*.mobileprovision`, `*.provisionprofile`).
- Certificate private keys, `.p12` files, `.cer` files, `.pem` keys.
- Apple account credentials, App Store Connect API keys.
- Real device bearer tokens (`svd1_`), enrollment tokens (`sve1_`), or user passwords.
- Personal server credentials or private TLS keys.

### 9.3 Generated Code Policy
- Generated Swift/C bridge bindings (from UniFFI or C-FFI generators) will live in `clients/ios/Infrastructure/RustBridge/` once established in Phase C (P013–P016).
- Hand-editing generated bridge bindings is forbidden.
- Temporary build outputs remain untracked in `.gitignore`.

## 10. Architecture Invariants
1. **INVARIANT-01**: Bearer tokens (`svd1_`) reside strictly in `Security.framework` Keychain Services (`kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`) and memory HTTP request header compositors. They MUST NEVER be stored in `UserDefaults`, SQLite, application logs, or SwiftUI view models.
2. **INVARIANT-02**: The Transfer Engine is an application-level infrastructure service decoupled from SwiftUI view lifetimes. Navigating away from a view or backgrounding the app MUST NOT terminate active transfers.
3. **INVARIANT-03**: SwiftUI views do not directly invoke raw network transport, Keychain, SQLite, or C-FFI Rust functions. Views interact solely through ViewModels and Application coordinators.
4. **INVARIANT-04**: Domain code does not know about concrete UI, network, Keychain, or SQLite technologies.
5. **INVARIANT-05**: Transfer success is reported ONLY upon authoritative server 200/201 acknowledgment or local verification.
6. **INVARIANT-06**: Client code MUST NOT attempt automated conflict resolution or issue DeviceBearer conflict resolution API calls (`ADR-032`).
