# Prompt012 Manifest — Dependency Boundaries & Service Protocols

## 1. Metadata
- **Prompt**: `012`
- **Goal**: Establish the minimum stable Swift service-contract layer and dependency inversion boundaries for the native Synveil iOS client (`v0.1`), focusing on HTTP transport abstraction, raw Rust FFI boundary restriction, and static boundary enforcement.
- **Starting Integration Branch**: `ios-app`
- **Starting SHA**: `342ab8ef6ab4c743252bc523b8feaa2ade5068c7`
- **Actual Work Branch**: `ios/p012-dependency-boundaries-3720636280902373748`
- **Validated Implementation Head**: `0ce94904790c29f839f041050f6c0f8cfb752d8e`
- **PR**: #39 — https://github.com/nghianguyen150612/Synveil/pull/39
- **PR Base**: `ios-app`
- **Authoritative Files Inspected**:
  - `clients/ios/README.md`
  - `clients/ios/Domain/README.md`
  - `clients/ios/Application/README.md`
  - `clients/ios/Features/README.md`
  - `clients/ios/Infrastructure/README.md`
  - `clients/ios/Infrastructure/Network/README.md`
  - `clients/ios/Infrastructure/RustBridge/README.md`
  - `clients/ios/Support/validate_ios_sources.py`
  - `clients/ios/Support/tests/test_validate_ios_sources.py`
  - `clients/ios/Synveil.xcodeproj/project.pbxproj`
  - `docs/ios/IOS_ARCHITECTURE.md`
  - `docs/ios/IOS_PLATFORM_MAPPING.md`
  - `docs/adr/ADR-058-ios-v0.1-client-architecture.md`
  - `docs/ios/manifests/PROMPT011_MANIFEST.md`

## 2. Dependency Direction & Service Contracts
- **Dependency Direction**:
  - `Features` presentation layer depends on `Application` coordinators and `Domain` service abstractions.
  - `Application` orchestration layer depends on `Domain` service abstractions and value entities.
  - `Infrastructure` layers implement `Domain` service protocol contracts.
  - `Domain` layer depends solely on Swift Standard Library and `Foundation`, with zero framework side effects.
- **HTTP Transport Abstraction**:
  - `HTTPMethod` (`Domain/Services/Transport/HTTPMethod.swift`): Enum for `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`.
  - `HTTPTransportRequest` (`Domain/Services/Transport/HTTPTransportRequest.swift`): Immutable `Sendable`, `Equatable` request value carrying `url: URL`, `method: HTTPMethod`, `headers: [String: String]`, `body: Data?`.
  - `HTTPTransportResponse` (`Domain/Services/Transport/HTTPTransportResponse.swift`): Immutable `Sendable`, `Equatable` response value carrying `statusCode: Int`, `headers: [String: String]`, `body: Data`.
  - `HTTPTransportProtocol` (`Domain/Services/Transport/HTTPTransportProtocol.swift`): `public protocol HTTPTransportProtocol: Sendable { func send(_ request: HTTPTransportRequest) async throws -> HTTPTransportResponse }`.
- **Concrete Networking Status**: `NOT_IMPLEMENTED_P012` (concrete `URLSession` implementation deferred to P023+). Zero network I/O or HTTP requests executed in production or tests.
- **Rust Bridge Boundary Decision**:
  - Raw Rust C-FFI / UniFFI imports, C headers, and low-level bindings are restricted strictly to `Infrastructure/RustBridge/`.
  - The callable `RustBridgeProtocol` Swift method surface is explicitly deferred until P013–P016, where real FFI signatures, ABI type mappings, memory ownership, and error encoding will be established.
  - Synthetic, fake, or empty marker protocols were intentionally omitted to prevent dead abstraction boilerplate.

## 3. Boundary Validation & Static Enforcement
- **Validator Rules Extended** (`clients/ios/Support/validate_ios_sources.py`):
  - Added `strip_comments_and_strings()` preprocessor to avoid false positives on comments and string literals.
  - Prohibits direct usage of raw networking symbols (`URLSession`, `URLSessionTask`, `URLSessionConfiguration`, `HTTPURLResponse`, etc.) in `Domain/`, `Application/`, and `Features/` layers.
  - Prohibits direct Keychain Security API symbol usage (`SecItemAdd`, `SecItemCopyMatching`, etc.) in upper layers.
  - Prohibits direct SQLite/GRDB API symbol usage (`DatabaseQueue`, `sqlite3_open`, etc.) in upper layers.
- **Validator Unit Self-Tests** (`clients/ios/Support/tests/test_validate_ios_sources.py`):
  - Added 4 test cases verifying upper-layer symbol rejection, comment/string comment preservation, and transport protocol abstraction permission. Total 16 self-tests pass cleanly.

## 4. Testability & Xcode Target Membership
- **XCTest Contract Suite Added**:
  - `clients/ios/Tests/SynveilTests/HTTPTransportContractTests.swift`: Contains `StubHTTPTransport` proving protocol substitutability, async flow, request/response value preservation, error propagation, and `HTTPMethod` cases without `URLSession` or network dependencies.
- **Xcode Target Membership** (`clients/ios/Synveil.xcodeproj/project.pbxproj`):
  - Production contract files added to `Synveil` app target (`HTTPMethod.swift`, `HTTPTransportRequest.swift`, `HTTPTransportResponse.swift`, `HTTPTransportProtocol.swift`).
  - Unit test suite added to `SynveilTests` unit test bundle (`HTTPTransportContractTests.swift`).

## 5. Linux Verification & CI Evidence
- **Linux Verification**:
  - `swift format lint --recursive --strict clients/ios` -> SUCCESS.
  - `python3 -m unittest discover -s clients/ios/Support/tests` -> SUCCESS (16 validator self-tests).
  - `python3 clients/ios/Support/validate_ios_sources.py` -> SUCCESS.
- **Validated implementation-head CI runs (`0ce94904790c29f839f041050f6c0f8cfb752d8e`)**:
  - `iOS Static Validation` — run `37100990887` — SUCCESS
  - `iOS Build` — run `37100990948` — SUCCESS
  - `iOS Simulator Tests` — run `37100990924` — SUCCESS
  - `Linux AppImage` — failure, classified `UNRELATED_SUBSYSTEM_FAILURE`
  - `PostgreSQL 17 scheduled-maintenance` — failure, classified outside native iOS scope
  - `Rust CI` — failure, classified outside native iOS P012 scope unless inspection proves otherwise
  - `Linux native packages (DEB + RPM)` — failure, classified outside native iOS scope
- **Evidence-only manifest correction**: this commit updates only `docs/ios/manifests/PROMPT012_MANIFEST.md`; if GitHub re-runs PR workflows due to the overall PR diff, the three native iOS gates must pass again on the resulting final PR head before merge.

## 6. Files Summary
- **Files Created**:
  - `clients/ios/Domain/Services/Transport/HTTPMethod.swift`
  - `clients/ios/Domain/Services/Transport/HTTPTransportRequest.swift`
  - `clients/ios/Domain/Services/Transport/HTTPTransportResponse.swift`
  - `clients/ios/Domain/Services/Transport/HTTPTransportProtocol.swift`
  - `clients/ios/Tests/SynveilTests/HTTPTransportContractTests.swift`
  - `docs/ios/manifests/PROMPT012_MANIFEST.md`
- **Files Modified**:
  - `clients/ios/README.md`
  - `clients/ios/Domain/README.md`
  - `clients/ios/Infrastructure/RustBridge/README.md`
  - `clients/ios/Support/validate_ios_sources.py`
  - `clients/ios/Support/tests/test_validate_ios_sources.py`
  - `clients/ios/Synveil.xcodeproj/project.pbxproj`

## 7. Scope & Limitations
- P012 establishes Swift service protocols, value types, and static boundary enforcement only.
- No concrete `URLSession` network transport is implemented (deferred to P023+).
- No C ABI, UniFFI, or Rust static library bindings are implemented (deferred to P013–P016).
- No Keychain credential vault or SQLite local store is implemented (deferred to P024+ / P038+).
- No network I/O is performed in production or test suites.

## 8. Merge Status
- **PR**: #39 — https://github.com/nghianguyen150612/Synveil/pull/39
- **Merge Status**: READY_FOR_FINAL_HEAD_VERIFICATION
- **Final `ios-app` SHA**: PENDING_MERGE
