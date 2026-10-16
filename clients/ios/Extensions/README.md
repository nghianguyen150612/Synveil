# Synveil iOS — Platform Extensions (`clients/ios/Extensions/`)

## Purpose & Ownership
Keeps Apple platform extensions isolated from the primary application's composition root and file browser.

### Sub-directories (Deferred Phases)
- `Extensions/FileProvider/`: P053 foundation for Apple's replicated File Provider API. The extension target runs as an independent OS process; production domain registration, metadata publication, authenticated network, content materialization, and mutation remain disabled.
- `Extensions/PhotoKit/`: Reserved for PhotoKit camera roll backup subsystem in P056–P058.

### Shared Infrastructure Rules
- `group.com.synveil.ios` provides only an explicitly entitled, versioned shared-container boundary. Do not move the app's SQLite v9 database or DeviceBearer Keychain item into shared storage.
- File Provider may consume only validated, bounded, read-only metadata snapshots after a later authorization review. No snapshot is published by P053 app startup.
- The primary SwiftUI file browser in v0.1 MUST NOT depend on File Provider or PhotoKit extension targets.

P053's target structure, operation behavior, identities, metadata schema, and security decisions are
documented in [`FileProvider/README.md`](FileProvider/README.md) and
[`PROMPT053_MANIFEST.md`](../../docs/ios/manifests/PROMPT053_MANIFEST.md).
