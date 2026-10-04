# Synveil iOS — Prompt021 Manifest

## 1. Prompt Number
Prompt021: App Shell & Startup State Foundation

## 2. Goal
Establish the application-level startup state machine (`AppStartupState`), session controller (`SessionController`), root SwiftUI shell (`RootView`), dependency composition root (`AppDependencyContainer`), and placeholder root surfaces for Synveil iOS v0.1.

## 3. Starting Integration Branch
`ios-app`

## 4. Exact Starting SHA
`6017cd39c4c87d17c14ab9a592f3d98971a7d156`

## 5. Actual Work Branch
`ios/p021-app-shell-startup`

## 6. Authoritative Files Inspected
- `docs/ios/IOS_ARCHITECTURE.md`
- `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`
- `docs/ios/IOS_V0_1_ROADMAP.md`
- `docs/ios/manifests/PROMPT020_MANIFEST.md`
- `clients/ios/App/SynveilApp.swift`
- `clients/ios/App/BootstrapView.swift`
- `clients/ios/App/README.md`
- `clients/ios/Application/README.md`
- `clients/ios/Application/Configuration/AppConfiguration.swift`
- `clients/ios/Application/Configuration/AppEnvironment.swift`
- `clients/ios/Application/Services/RustBridgeProtocol.swift`
- `clients/ios/Domain/README.md`
- `clients/ios/Domain/Configuration/ServerEndpoint.swift`
- `clients/ios/Infrastructure/RustBridge/*`
- `clients/ios/Tests/SynveilTests/*`
- `clients/ios/Support/validate_ios_sources.py`
- `clients/ios/Support/tests/*`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`

## 7. Phase D Entry Status
Prompt021 initiates Phase D: Authentication & Server Connectivity by establishing the root application startup state machine and SwiftUI root shell.

## 8. AppStartupState Path
`clients/ios/Application/Session/AppStartupState.swift`

## 9. Exact State Cases
- `.initializing`
- `.needsServerProfile`
- `.readyForServerValidation`
- `.needsEnrollment`
- `.authenticated`
- `.recoveryRequired(AppRecoveryReason)`

## 10. Recovery Reason Path & Cases
`clients/ios/Application/Session/AppStartupState.swift`
- `.configuration`
- `.authentication`
- `.deviceRevoked`
- `.secureStore`
- `.enrollmentAmbiguous`
- `.transport`

## 11. SessionController Path
`clients/ios/Application/Session/SessionController.swift`

## 12. MainActor Status
Isolated to `@MainActor` via `@MainActor public final class SessionController`.

## 13. Observation Mechanism
Native Swift `@Observable` macro (`import Observation`).

## 14. Initial State
`.initializing`

## 15. Startup Method
`public func start() async`

## 16. Startup Idempotence
Guarded by internal `isStarted: Bool` flag; calling `start()` multiple times produces the same state without duplicated resolution.

## 17. Nil-Endpoint Resolution
`AppConfiguration(serverEndpoint: nil)` -> `.needsServerProfile`

## 18. Configured-Endpoint Resolution
`AppConfiguration(serverEndpoint: validEndpoint)` -> `.readyForServerValidation`

## 19. Production Automatic Authenticated Status
`authenticated` is NOT selected automatically during startup; production clean launch safely resolves to `.needsServerProfile`.

## 20. Root View Path
`clients/ios/App/RootView.swift`

## 21. Root State -> Surface Mapping
- `.initializing` -> `LaunchView`
- `.needsServerProfile` -> `ServerSetupPlaceholderView`
- `.readyForServerValidation` -> `ServerValidationPlaceholderView`
- `.needsEnrollment` -> `EnrollmentPlaceholderView`
- `.authenticated` -> `AuthenticatedShellPlaceholderView`
- `.recoveryRequired(reason)` -> `RecoveryPlaceholderView`

## 22. Accessibility Identifiers
- `synveil.root.initializing`
- `synveil.root.server-setup`
- `synveil.root.server-validation`
- `synveil.root.enrollment`
- `synveil.root.authenticated`
- `synveil.root.recovery`

## 23. AppDependencyContainer Status/Path
Created at `clients/ios/App/AppDependencyContainer.swift`.

## 24. SynveilApp Composition
`SynveilApp` owns `@State private var container: AppDependencyContainer` and injects `container.sessionController` into `RootView`.

## 25. `.task` Startup Behavior
`.task { await container.sessionController.start() }` attached to `RootView` in `SynveilApp.swift`. Idempotent execution safe across SwiftUI view recreations.

## 26. Persistence Status / Deferred Marker
`PROFILE_PERSISTENCE_DEFERRED_AFTER_P021` — No profile persistence or storage modified in P021.

## 27. Network Status / Deferred Marker
`SERVER_HEALTH_CHECK_DEFERRED_AFTER_P021` — No HTTP or network requests made in P021.

## 28. Keychain Status / Deferred Marker
`CREDENTIAL_LIFECYCLE_DEFERRED_AFTER_P021` — No Security framework or Keychain interaction in P021.

## 29. Enrollment Status / Deferred Marker
`ENROLLMENT_LIFECYCLE_DEFERRED_AFTER_P021` — Enrollment flow placeholder established without secret/exchange logic.

## 30. Authenticated Feature Status / Deferred Marker
`AUTHENTICATED_SHELL_DEFERRED_AFTER_P021` — Authenticated shell placeholder established without fake library/file content.

## 31. Static Validator Changes
Updated `clients/ios/Support/validate_ios_sources.py` to enforce that `Application/` layer sources do not import forbidden UI/storage modules (`SwiftUI`, `UIKit`, `Security`, `GRDB`, `SQLite3`).

## 32. SessionController Unit Tests
`clients/ios/Tests/SynveilTests/SessionControllerTests.swift` (6 tests passing).

## 33. Root Shell Tests
`clients/ios/Tests/SynveilTests/RootViewTests.swift` (1 test covering all 11 state cases passing).

## 34. Preview Behavior
SwiftUI previews instantiate states deterministically without network, Keychain, or Rust dependencies.

## 35. P020 Regression Status
Zero regressions. All 16 FFI Rust unit tests, 10-symbol C ABI validations, and RustBridgeProtocol tests remain 100% green.

## 36. ABI Version / Export Invariant
ABI version remains `1`. Exact 10 exported C ABI symbols preserved.

## 37. Host Validation
`cargo fmt`, `cargo check`, `cargo test`, `cargo clippy`, `validate_ios_sources.py`, Python unittest suites all PASSED cleanly.

## 38. Final Implementation Commit
`d8064fc4eb9c14f95280fd8af2031cc641df8d3e` (merge-review hardened executable head).

## 39. PR Number / URL
PR #64 — https://github.com/nghianguyen150612/Synveil/pull/64

## 40. PR Base
`ios-app`

## 41–48. CI & Merge Evidence
Final-head workflow IDs and merge evidence are recorded in the PR discussion after the final CI run to avoid an evidence-commit loop.


## 49. Merge-Review State-Machine Hardening
Before merge, root transition safety was tightened:

- `markServerReadyForValidation()` only transitions from `.needsServerProfile`.
- `requireEnrollment()` only transitions from `.readyForServerValidation`.
- `markAuthenticated()` only transitions from `.needsEnrollment`.
- Invalid attempts to enter `.authenticated` from `.initializing`, `.needsServerProfile`, or `.recoveryRequired` are ignored.
- Root previews/tests now construct later states through the valid lifecycle path.
- `SessionControllerTests` includes a regression test proving lifecycle gates cannot be bypassed.

## 50. Evidence Discipline
This is the final Prompt021 manifest metadata correction. Final-head CI and merge evidence will be recorded in the PR discussion. No additional evidence-only manifest commit should be created.
