# Synveil iOS — Features / Presentation Module (`clients/ios/Features/`)

## Purpose & Ownership
The `Features` directory houses the feature-facing SwiftUI presentation layer. Each feature area is self-contained with its SwiftUI views, `@Observable` view models, and local presentation models.

### Sub-feature Directories
- `Features/Onboarding/`: Server profile setup, canonical origin validation, server health preflight UI.
- `Features/Authentication/`: Enrollment token (`sve1_`) entry, device session authorization status, recovery banners.
- `Features/FileBrowser/`: Logical library list, directory tree navigation, file node details, breadcrumb bar, offline/cached banners.
- `Features/Transfers/`: Download/upload progress queue views, chunk status, staging storage indicators.
- `Features/Settings/`: Profile switching, local credential wiping ("Forget on this device"), sync schedule preferences.
- `Features/Common/`: Shared SwiftUI components (status banners, progress bars, custom modifiers, theme styling).

## Dependency Rules
- **Allowed Dependencies**: `Application/` (coordinators), `Domain/` (entities/errors), Service interfaces.
- **Prohibited Dependencies**:
  - `URLSession` or raw HTTP networking code.
  - `Security.framework` (Keychain) or raw SQLite database queries.
  - Raw Rust C-FFI exports (`synveil_ios_core`).
  - Direct cross-feature state mutations (features interact through `Application` layer coordinators).

## Architecture Rules
- SwiftUI views are pure functions of state.
- ViewModels use native `@Observable` and run on `@MainActor`.
- Heavy hashing, database queries, and FFI processing must occur off the main thread.
