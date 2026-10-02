# Synveil iOS — Platform Extensions Reservation (`clients/ios/Extensions/`)

## Purpose & Ownership
Reserves clean architectural ownership for future Apple platform extensions without polluting the primary application file browser layout.

### Sub-directories (Deferred Phases)
- `Extensions/FileProvider/`: Reserved for `NSFileProviderExtension` implementation in P053–P055. Runs as an isolated OS extension process.
- `Extensions/PhotoKit/`: Reserved for PhotoKit camera roll backup subsystem in P056–P058.

### Shared Infrastructure Rules
- Platform extensions will share domain models, SQLite cache store, and Keychain credentials via an App Group container (`group.com.synveil.ios`).
- The primary SwiftUI file browser in v0.1 MUST NOT depend on File Provider or PhotoKit extension targets.
