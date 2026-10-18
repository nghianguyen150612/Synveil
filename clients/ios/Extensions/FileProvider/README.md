# Synveil replicated File Provider

P054 implements authorized read-only metadata lookup, domain/Library/folder and working-set
enumeration, deterministic pagination, durable change replay, and native item adaptation. The
principal remains `NSFileProviderReplicatedExtension`, independently launched from the host app.
Apple Files support remains `IOS_DEFERRED_AFTER_V0_1` in the product contract.

Production publication and domain registration remain disabled in `FileProviderCapability.production`.
The extension never opens app SQLite v9, Keychain, a network session, or P052 recovery. Content
fetch, create, modify and delete callbacks remain fail-closed. P046/P051 transfers remain disabled.

## Publication and completeness

`FileProviderMetadataPublicationCoordinator` is a host-only explicit refresh/sync boundary. It
captures an authenticated, server-confirmed complete catalog and canonical hierarchy cut, rejects
pending/local/conflict evidence, revalidates the exact session at commit, then signals committed
changes. It is not installed in startup or view rendering. Existing offline repositories cannot
satisfy its stronger transaction-bound capture contract: partial caches, unacknowledged projections
and snapshot-only records must not be upgraded to authority. Installing a live source requires the
canonical capture and session-withdrawal integration to be independently qualified first.

The v2 commit envelope supports both unchanged complete v1 Library snapshots and complete v2
container partitions. A partition contains a root-to-container ancestor chain and the complete
direct-child listing. Every live folder/root must have exactly one listing, including empty ones.
Repeated ancestors must match exactly; every non-root ancestor must be present in its parent's
children. Duplicate, missing, cyclic, tombstoned-parent and cross-Library graphs fail validation.
No v1 partial snapshot is treated as complete. A Library can exceed 10,000 total items by using
multiple container partitions. A single container that exceeds its unit limit is rejected, never
truncated; the previous valid generation remains available.

Bounds: 10,000 records and 8 MiB per unit; 128 Libraries, 2,048 units, 50,000 encoded records
(including repeated ancestors), and 12 MiB of unit bytes per domain generation; three retained
generations; 64 MiB total provider storage; 8,192 traversal entries. A refresh that cannot fit
these bounds preserves the prior commit. No SQLite migration is required.

## Durable storage and consumption

Storage remains below the P053 protected App Group subtree
`Synveil/FileProvider/v1/generations/<opaque-domain>/`. The directory version is the established
security boundary; file formats have explicit independent versions. Immutable `unit-<UUID>.json`
files are synchronized before one `commit.json` envelope makes the complete catalog and graph
visible. Manifest references contain Library, optional container, filename and digest bindings.
Retained complete generations form the bounded comparison journal, rather than an unbounded event
log. This provides net metadata changes between accepted durable points after process relaunch.

A nonblocking cross-process shared lease covers graph validation through observer delivery. The
writer and revocation use its exclusive counterpart. `rollback.json` is durable before an
`unsettled` marker; readers reject that marker. Only after the new manifest and directory are synced
is the marker removed. Interrupted or uncertain replacement restores the prior durable manifest
before another publication. Failed recovery denies reads. Orphan units are reclaimed under the
writer lease; referenced files remain until the new manifest commits. Protected files use complete
Data Protection; unit reads enforce type/size and `O_NOFOLLOW`. Missing App Group never falls back
to the app sandbox. Checksums detect corruption; the App Group entitlement and host session remain
the authority boundary, not the checksum.

## Enumeration and changes

Pages and anchors are fixed 162-byte opaque binary values, within Apple's 500-byte limits:
version/purpose, three SHA-256 scope bindings (domain/owner/container), authorization epoch,
base/target durable generations, offset, and SHA-256 corruption checksum. Initial native pages
preserve name or modification-date order, with stable identity tie breaking. Subsequent pages
retain that ordering and generation. Each page/change batch contains at most 100 records.
Cross-container, owner, domain, epoch, corrupt, future and expired cursors fail closed. Disappeared
generations expire; they never silently restart at a new offset.

Anchors advance only in the observer completion after the associated batch. Change pagination
pins its base and target and resumes after cold launch; a newer publication sets `moreComing`.
Comparisons exclude tombstones. Rename/move retain Node identity; a move reports the target item
with its new parent to both affected parent enumerators, without declaring a still-live Node
removed. Only globally absent/tombstoned items receive deletion callbacks. Removed Libraries are
not available through old page tokens. Invalidations are checked between observer callbacks and
before successful completion. No unavailable generation produces a successful empty response.

`SynveilFileProviderItem` exposes folders and generic data files, available authoritative size/dates,
separate immutable FileVersion and metadata-revision components, and conservative capabilities.
Unknown components remain empty/unknown; no local timestamp is invented. Files advertise no read
or mutation capability while materialization remains unavailable; folders permit enumeration.

## Signaling and qualification

Signals coalesce affected parents, Library/domain roots and the working set after commit. A durable
receipt is written only after every framework completion succeeds and the same generation/epoch
remains current. Failed, interrupted and unregistered-domain delivery stays pending. The native
manager adapter only discovers existing domains and calls `signalEnumerator(for:)`; it never
registers one. Signaling never replaces durable change enumeration.

See [P054 manifest](../../../../docs/ios/manifests/PROMPT054_MANIFEST.md) for exact-head CI, test
inventory, independent review and physical-device procedures. Device/signing status is
`DEFERRED_PHYSICAL_DEVICE_NOT_AVAILABLE`; Simulator XCTest does not prove Files app recognition.
P055 is not authorized or implemented.
