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

## Future Files Location
When P007 initializes the Swift sources, the application entry point will be placed here (e.g. `App/SynveilApp.swift`, `App/AppDependencyContainer.swift`).
