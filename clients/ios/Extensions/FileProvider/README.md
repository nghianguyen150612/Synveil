# Synveil File Provider extension (P053 / P054)

This target is an isolated, read-only File Provider extension implementing metadata enumeration,
pagination, change tracking, and host-app publication for Synveil's replicated Apple File Provider
integration. It uses Apple's replicated File Provider model (`NSFileProviderReplicatedExtension`
and `NSFileProviderEnumerator`), available since iOS 16 and compatible with this project's iOS 17
deployment target. Apple documentation:

- [File Provider](https://developer.apple.com/documentation/fileprovider)
- [Replicated file provider extension](https://developer.apple.com/documentation/fileprovider/replicated-file-provider-extension)
- [`NSFileProviderReplicatedExtension`](https://developer.apple.com/documentation/fileprovider/nsfileproviderreplicatedextension)
- [`NSFileProviderEnumerating`](https://developer.apple.com/documentation/fileprovider/nsfileproviderenumerating)
- [`NSFileProviderItemVersion`](https://developer.apple.com/documentation/fileprovider/nsfileprovideritemversion)
- [`NSFileProviderManager`](https://developer.apple.com/documentation/fileprovider/nsfileprovidermanager)
- [`NSFileProviderDomain`](https://developer.apple.com/documentation/fileprovider/nsfileproviderdomain)

The replicated model is the current Apple API for a provider whose hierarchy is mirrored by the
system. It offers explicit item lookup, enumeration, content fetch, mutation, version, and lifecycle
callbacks. P054 implements read-only metadata enumeration, pagination, sync anchors, and change tracking over validated host-published snapshots.

## Key Features (P054)

1. **Native Item Adapter (`SynveilFileProviderItem`)**
   - Maps `FileProviderProjectedItem` and `FileProviderItemIdentity` to `NSFileProviderItem`.
   - Resolves UTType based on filename extension or `.data`.
   - Maps immutable content/metadata versions into `NSFileProviderItemVersion`.
   - Restricts capabilities to read-only (`.allowsReading`, `.allowsContentEnumerating` for folders).

2. **Domain Root & Library Enumeration (`FileProviderEnumerator`)**
   - Enumerates domain root (`rootContainer`) via `FileProviderLibraryCatalogSnapshot`.
   - Enumerates library roots and nested folder contents via `FileProviderMetadataSnapshot`.
   - Implements `FileProviderPageToken` for deterministic, generation-consistent pagination (100 items/page).
   - Serves `FileProviderSyncAnchorToken` and `enumerateChanges(for:from:)` using `FileProviderChangeJournal`.

3. **Host-App Publication (`FileProviderMetadataPublicationCoordinator`)**
   - Publishes server-confirmed library catalogs and item snapshots into App Group storage (`group.com.synveil.ios`).
   - Uses atomic staging and quotas (8 MiB snapshot, 64 MiB total shared storage).

4. **Change Signaling (`FileProviderSignaler`)**
   - Wraps `NSFileProviderManager` to signal enumerators upon committed snapshot changes without auto-registering production domains.

5. **Fail-Closed Operations**
   - `fetchContents` fails closed (content materialization deferred to P055).
   - `createItem`, `modifyItem`, `deleteItem` fail closed with Apple's `cannotSynchronize`.

## Isolation and disabled capabilities

- The extension has its own principal class and does not load `SynveilApp`, SwiftUI, the app
  dependency container, Rust FFI, the existing SQLite database, a Keychain store, or URLSession.
- Production domain registration remains disabled in `FileProviderCapability.production`.
- No File Provider domain is registered by app startup.
