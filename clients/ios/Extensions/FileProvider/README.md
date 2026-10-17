# Synveil replicated File Provider

P054 implemented authorized read-only metadata lookup, domain/Library/folder and working-set
enumeration, deterministic pagination, durable change replay, and native item adaptation.

P055 implements verified offline content materialization, bounded App Group content storage, and
safe File Provider content handoff through `NSFileProviderReplicatedExtension.fetchContents`.
Apple Files support on real physical devices remains `IOS_DEFERRED_AFTER_V0_1` in the product contract.

Production publication and domain registration remain disabled in `FileProviderCapability.production`.
The extension never opens app SQLite v9, Keychain, a network session, or P052 recovery. Create,
modify and delete callbacks remain fail-closed. P046/P051 background transfers remain disabled.

## Verified Content Materialization

P055 establishes a three-stage verified content pipeline:
1. **Host Verification:** The main app downloads, verifies, and durably records immutable file content via P043 (`FileDownloadCoordinator`).
2. **App Group Staging:** Host-side `FileProviderVerifiedContentPublicationCoordinator` copies P043-verified files into versioned App Group storage under `Synveil/FileProvider/v1/content/<domain>/` with a signed, atomic `FileProviderContentManifest`.
3. **Extension Materialization:** The extension's `fetchContents` callback verifies the domain authorization lease, resolves item metadata, matches explicit requested `FileVersion` against `FileProviderItemVersionContract`, computes streaming SHA-256 and byte length in 64 KiB chunks, copies verified content to the `NSFileProviderManager.temporaryDirectoryURL()` volume, and delivers the file URL to Apple's framework.

## Content Storage & Bounds

App Group content storage is placed under `Synveil/FileProvider/v1/content/<domain>/`:
- `objects/<storageKey>`: immutable FileVersion content payloads protected with `completeUntilFirstUserAuthentication` and owner-only POSIX permissions (`0o600`).
- `manifests/<fileVersionID>.json`: atomic JSON metadata describing content identity, expected byte length, expected SHA-256, domain ownership, owner scope fingerprint, and authorization generation.
- `staging/`: crash-safe atomic write staging directory for content and manifest creation.

Bounds: Content storage is subject to an independent 64 MiB total App Group storage quota. Full-file in-memory loading is prohibited; all integrity checks stream chunks in 64 KiB buffers.

## Item Capabilities & Fail-Closed Mutations

`SynveilFileProviderItem` advertises `.allowsReading` for files with valid immutable version identities, and `isDownloaded` evaluates to true when a verified content manifest exists in the App Group store. Folder items continue advertising `.allowsContentEnumerating`.

Write, rename, reparent, delete, and `.allowsAll` capabilities remain strictly disabled. `createItem`, `modifyItem`, and `deleteItem` callbacks remain fail-closed returning `unsupported` errors.

See [P055 manifest](../../../../docs/ios/manifests/PROMPT055_MANIFEST.md) for exact CI, test inventory, and explicit deferred scope.
