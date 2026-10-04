# Synveil iOS — App Module (`clients/ios/App/`)

## Purpose & Ownership
The `App` directory contains the application entry point and top-level dependency composition root for the Synveil native iOS client.

### Responsibilities
- **Application Entry Point**: `@main` `SynveilApp` struct and optional system application delegate handlers (`AppDelegate`).
- **Dependency Composition Root**: Instantiating concrete infrastructure services and injecting them into Application-layer coordinators and ViewModels.
- **Root State Wiring**: Observing top-level session state (`SessionController`) to switch between unauthenticated onboarding/login and authenticated main navigation.
- **App Lifecycle Observation**: Responding to iOS system lifecycle events (`scenePhase` active, inactive, background) and dispatching flush/checkpoint signals to application coordinators.

## Dependency Rules
- **Allowed Dependencies**: `Features/`, `Application/`, `Domain/`, `Infrastructure/` (strictly at the root composition seam).
- **Prohibited Dependencies**:
  - Direct HTTP networking code or socket calls.
  - Direct Keychain manipulation or raw SQLite SQL queries.
  - Direct C-FFI Rust invocations.
  - Feature-specific UI view implementations or domain algorithms.

## App Composition & Root Routing
- **`SynveilApp`**: `@main` application entry point. Owns `@State` top-level `AppDependencyContainer` and constructs `RootView` wired to `sessionController.start()`.
- **`AppDependencyContainer`**: `@MainActor` top-level dependency composition root holding `AppConfiguration` and `SessionController`.
- **`RootView`**: Authoritative root SwiftUI shell. Switches on `sessionController.state` to present mutually exclusive application surfaces:
  - `.initializing` -> `LaunchView` (`synveil.root.initializing`)
  - `.needsServerProfile` -> `ServerSetupPlaceholderView` (`synveil.root.server-setup`)
  - `.readyForServerValidation` -> `ServerValidationPlaceholderView` (`synveil.root.server-validation`)
  - `.needsEnrollment` -> `EnrollmentPlaceholderView` (`synveil.root.enrollment`)
  - `.authenticated` -> `AuthenticatedShellPlaceholderView` (`synveil.root.authenticated`)
  - `.recoveryRequired` -> `RecoveryPlaceholderView` (`synveil.root.recovery`)
