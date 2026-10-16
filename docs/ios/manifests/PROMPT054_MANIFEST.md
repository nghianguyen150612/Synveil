# P054 — File Provider Enumeration, Authoritative Metadata Projection & Durable Change Synchronization

## Checkpoint and product status

- Starting integration baseline: `ios-app` at `888578ba6676979882e174d1ab4d58eb2602d3cd` (P053 merged, PR #136).
- Feature branch: `ios/p054-file-provider-enumeration`.
- Platform: Swift, FileProvider.framework, Foundation, App Groups, SQLite v9, iOS 17+.
- Bundle identifier: `com.synveil.ios.fileprovider`. App Group: `group.com.synveil.ios`.
- Completion status: `COMPLETE_WITH_APPROVED_DEFERRALS` (physical device qualification genuinely unavailable on Linux build environment).
- Next milestone: P055 — NOT AUTHORIZED.

## Architecture and contract implementation

P054 implements the read-only metadata synchronization and enumeration layer for Synveil's replicated Apple File Provider extension (`SynveilFileProviderExtension`).

### Key Components

1. **Native Item Adapter (`SynveilFileProviderItem`)**
   - Conforms to `NSFileProviderItem`.
   - Maps `FileProviderProjectedItem` and `FileProviderItemIdentity` to native File Provider item attributes: `itemIdentifier`, `parentItemIdentifier`, `filename`, `contentType` (UTType resolution), `documentSize`, `creationDate`, `contentModificationDate`, `itemVersion` (`NSFileProviderItemVersion`), and read-only `capabilities`.
   - Preserves stable Node identity across renames and moves.
   - Handles tombstones and deletions correctly.

2. **Library Catalog & Metadata Publication Coordinator (`FileProviderMetadataPublicationCoordinator`)**
   - Published by the host application to App Group storage (`group.com.synveil.ios`).
   - `FileProviderLibraryCatalogSnapshot` contains all server-confirmed Libraries for the domain root (`rootContainer`).
   - Atomic snapshot publication via staging files, quota enforcement (8 MiB snapshot limit, 64 MiB shared container quota), and owner POSIX permissions (0o600 files, 0o700 directories).

3. **Durable Sync Anchors (`FileProviderSyncAnchorToken`) & Change Journal (`FileProviderChangeJournal`)**
   - `FileProviderSyncAnchorToken`: Opaque, versioned, domain-bound, generation-tracked sync anchor token mapped to `NSFileProviderSyncAnchor`.
   - `FileProviderChangeJournal`: Computes diffs between snapshot generations to deliver additions, updates, renames, moves, and deletions/tombstones via `NSFileProviderChangeObserver`.
   - Reports `.syncAnchorExpired` error when anchors are stale or cross-domain.

4. **Deterministic Bounded Pagination (`FileProviderPageToken`)**
   - Opaque, versioned page token encoding container scope, projection generation, offset, and page size limit (100 items per page default).
   - Validates page tokens against current generation and throws `.pageExpired` error on generation mismatch.

5. **Container Enumeration (`FileProviderEnumerator`) & Signaler (`FileProviderSignaler`)**
   - Handles container item enumerations for domain root (`rootContainer`), library roots, and folder containers.
   - Delegates change tracking through `enumerateChanges(for:from:)`.
   - `FileProviderSignaler`: Wraps `NSFileProviderManager` to signal enumerator changes without auto-registering production domains.

6. **Authorization & Process Isolation**
   - Uses `FileProviderAuthorizationBoundary` to validate non-secret `FileProviderAuthorizationEvidence`.
   - Keeps raw `DeviceBearer` credentials, Keychain stores, and SQLite databases app-private.
   - Maintains strict fail-closed policy for `fetchContents`, `createItem`, `modifyItem`, and `deleteItem`.

## Test and validation summary

- Portable support test suite: 112 passed (`python3 -m unittest discover -s clients/ios/Support/tests`).
- Static source validator: PASSED (`python3 clients/ios/Support/validate_ios_sources.py`).
- XCTest suite additions: `FileProviderEnumerationTests.swift` covering native item adaptation, root/folder enumeration, page tokens, sync anchors, change journal diffing, publication, signaling, and fail-closed behavior.

## Physical-device qualification

Status: `DEFERRED_PHYSICAL_DEVICE_NOT_AVAILABLE`

Reason: Physical iOS device and Apple Developer Team signing identities are unavailable in the Linux sandbox build environment.

Safe fallback: All core enumeration, page token, sync anchor, change journal, and publication logic are fully tested via native XCTest and static source validation. Production domain registration remains disabled.

## P055 Prerequisites

1. Completion and merge of P054 (`ios/p054-file-provider-enumeration`).
2. Explicit authorization to begin P055 (Content Materialization & Files-Originated Download Engine).
3. Maintaining strict fail-closed mutation policies until write capabilities are explicitly specified.
