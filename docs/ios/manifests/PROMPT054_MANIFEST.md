# P054 — File Provider enumeration and durable metadata synchronization

## Checkpoint and integration state

- Starting `origin/ios-app`: `888578ba6676979882e174d1ab4d58eb2602d3cd`, exactly the verified
  P053 merge baseline. Fetched before branch creation; ancestry verified; initial worktree clean.
- Feature branch: `ios/p054-file-provider-enumeration`.
- PR: [#137](https://github.com/nghianguyen150612/Synveil/pull/137), targeting `ios-app`.
- Review requested from `ducminh231011`; no submitted GitHub approval or merge is claimed.
- Status at this checkpoint: `IMPLEMENTED_PENDING_NATIVE_VALIDATION`.
- Exact final feature SHA, native totals and workflow URLs will be recorded after CI completes.
- P053 historical manifest remains unchanged. P052 lifecycle and P051 deferred-scope closeout were
  inspected; their transfer restrictions remain in force. P055 has not started.

## Implementation and compatibility

`SynveilFileProviderItem` maps stable scoped Node/Library identities and parent relationships,
authoritative names, available size/dates, generic data/folder content types and separate immutable
FileVersion/metadata revision components. Unknown version components remain empty; unknown fields
remain nil. It never claims downloaded content. Folder capability is enumeration only; files expose
no unsupported read/write operations. Renames/moves retain identity; tombstones never become items.

`SynveilFileProviderExtension` uses the real replicated callbacks and returns an authorized
`SynveilFileProviderEnumerator`, implements metadata lookup, and maps typed authorization, missing
item, incompatible/corrupt data, storage, page/anchor expiration and cancellation errors to actual
Apple error codes. Root lookup validates the graph. Unsupported content/mutation callbacks remain
fail-closed. The extension principal is still compiled only in the extension; native metadata helpers
are additionally compiled in the host for direct XCTest coverage. The app still embeds the `.appex`.

P053 identity, authorization, capability, lifecycle and v1 snapshot contracts are reused. The domain
is Codable only for the non-secret app-issued commit envelope. No credential is added to shared
storage. v1 Library snapshots retain complete-hierarchy semantics and their validation bounds;
partial v1 snapshots are never reinterpreted. An explicit v2 container-partition format adds a
complete direct-child listing and root-to-container ancestor chain. Every live folder/root must
have exactly one listing, including empty containers. Ancestors must agree across units and appear
in their parents' children. Missing, duplicate, cyclic, cross-Library and uncertain graphs reject
publication. Large Libraries may span units; a test publishes a 10,003-item Library using three
independently bounded units. A single over-limit container or domain rejects the refresh safely.

The complete catalog is the exact Library set in the commit. Existing files on disk never confer
Library authorization. An empty catalog or folder is authoritative only in a validated complete
commit; absent or corrupt storage never produces fake empty success.

## Host publication and authorization

`FileProviderMetadataPublicationCoordinator` is a host-only explicit refresh/verified-sync boundary.
It obtains a complete confirmed capture, requires equal locally applied/server-confirmed cut,
excludes unresolved mutation/conflict-review captures, checks graph/scope/limits, then revalidates the
exact authenticated account/device/credential and cut synchronously at commit. Its source contract
must read canonical state from one existing SQLite transaction; it cannot authorize a series of
unrelated offline reads. It creates no new SQLite writer and requires no schema migration (v9).

The existing repositories intentionally expose partial and snapshot-only cache records. No live
production source is installed because those records alone cannot prove this stronger complete
canonical domain capture. The deterministic capture/commit/signaling pipeline is fully injectable
and tested; production metadata publication remains disabled as permitted by P054's production
prerequisite gate. It is not called from startup, P052 recovery or SwiftUI view rendering.

The manifest binds domain, owner, credential fingerprint, current app-issued authorization epoch and
all unit generations. Readers evaluate the reused P053 authorization boundary and protected-data
availability on every leased operation. Units carry exact Library/domain/owner/generation bindings.
A nonblocking shared cross-process lease covers validation through observer callbacks. Revocation,
logout/account-switch withdrawal take the exclusive lease and persist denied evidence. A successful
withdrawal fences old callbacks; a busy/failed withdrawal throws and must not be reported successful.
Credential replacement changes domain; epoch replacement drops retained history and expires old
tokens. Deleting files is not the reader fence. Cold extension launch needs only committed evidence;
it cannot read DeviceBearer or invoke host authentication, Keychain, SQLite or network sessions.

## Formats, bounds and crash consistency

Storage: `Synveil/FileProvider/v1/generations/<opaque-domain>/` in `group.com.synveil.ios`.
The established security subtree remains v1; each internal format has an explicit version.

- `unit-<UUID>.json`: immutable v1 complete Library snapshot or v2 complete container partition.
  Per-unit maximum: 10,000 records and 8 MiB. Files use complete Data Protection and owner-only mode.
- `commit.json`: v2 domain manifest, authorization evidence and at most three retained generation
  records. Unit references bind Library, optional container, filename and digest. The entire manifest
  is at most 8 MiB. Unknown formats reject; no implicit migration or partial compatibility fallback.
- Per generation: at most 128 Libraries, 2,048 units, 50,000 encoded records (including ancestor
  repetition), and 12 MiB of unit payloads. All provider storage stays capped at 64 MiB; traversal
  remains capped at 8,192 entries. Temporary/rollback bytes count against quota too.
- `rollback.json` and `unsettled`: previous durable manifest and an uncertainty marker. Units are
  validated, privately staged, file-synchronized and directory-synchronized before commit. Rollback
  is durable before the marker; readers deny the marker. The new manifest is atomically renamed and
  directory-synchronized before marker removal. A crash/I/O failure restores the prior manifest
  under the writer lease; unsuccessful recovery denies reads. Orphan cleanup never removes current
  references before a replacement commits. Interrupted first publication remains unauthorized.
- Retained complete generations are the bounded durable comparison journal. Added/changed/renamed/
  moved/tombstoned/deleted items and Library membership are computed between accepted durable states.
  This retains net current state, not every transient event; no partial scan or failed network read
  causes deletion. Three-generation retention defines expiration; no unbounded snapshot history.
- `signals.json`: authorization/generation receipt, committed only after all native signal completions
  succeed and the manifest still matches. Failure, cancellation and unregistered domains remain pending.

Readers enforce schema, digest, owner, generation, graph, bounded regular-file reads and symlink/path
protections, including `O_NOFOLLOW`. Missing entitlement never falls back to app-private storage.
Directory traversal/I/O failures fail quota validation. Checksums detect corruption, not authority;
App Group entitlements and the authenticated host are the authority boundary. No process opens or
mutates the main SQLite database through this storage.

## Pagination, anchors and change delivery

Page and anchor wire version 1: fixed 162 bytes. Layout is version/purpose (2 bytes), three 32-byte
SHA-256 domain/owner/container bindings, authorization/base/target/offset big-endian UInt64 values
(32 bytes), and 32-byte SHA-256 checksum. Both comply with Apple's 500-byte limits. Purposes separate
identifier/name/date pages and anchors. Native initial page ordering is name or modification date,
with stable identity ties; the token pins subsequent ordering. No item arrays appear in tokens.

Pages and change batches contain at most 100 records, are deterministic and generation-bound.
Corrupt, future, wrong-container/domain/owner/epoch and expired tokens fail. Retained pages never
switch generations; removed Library access invalidates historical pages. Anchors identify durable
base and target generations; in-progress batches retain offset and target across cold launch.
Compacted bases expire with `syncAnchorExpired`. Anchors are delivered only after corresponding
updates/deletions; `moreComing` reflects remaining batch records or a newer committed generation.
Moves update the live target item to both affected parent enumerators. A still-live moved Node is
never globally deleted, regardless of parent callback order. Tombstones and global absence in a
complete generation deliver deletion. Unknown containers are not successful empty change windows.
Invalidation checks occur between callbacks and before successful completion/anchor advancement.

The native manager adapter discovers existing domains and calls `signalEnumerator(for:)`; it never
registers one. Affected parents, Library/domain roots and the working set are deduplicated, including
old parents on moves and outstanding notifications. The working set covers this bounded complete
metadata projection. Signals are scheduling hints; durable changes remain available independently.

## Validation and review evidence

Portable validation at the implementation checkpoint:

- `validate_ios_sources.py`: PASS.
- `python3 -m unittest discover -s clients/ios/Support/tests`: 117 passed, 0 failed.
- Strict official Swift 6.2.3 formatter available locally; formatting/lint performed.
- Documentation validation and `git diff --check`: final results pending below.
- New native suite: `FileProviderEnumerationTests`; final method count/result pending native CI.
  Covers native fields/capabilities/version identity; roots/folders/empty catalogs; wrong scope and
  unknown items; multi-page bounds/order/repeatability; generation retention/expiration; persisted
  anchors/corruption; rename/move/deletion/tombstones; multi-batch replay; atomic/fault/disk-full/
  rollback/corrupt/future/quota/symlink cases; logout/epoch/protection/leases; native callbacks/
  cancellation; signaling and confirmed host capture; v1/v2 compatibility and large partitioned tree.
- Existing P001–P053 XCTest sources remain enabled; no tests removed, skipped or weakened.
- First native attempt on `1aa903b` caught Apple's initial-page constants importing as `NSData`;
  conversion to typed `NSFileProviderPage` was corrected. That attempt is not a native pass.
- Independent local code review found move-as-deletion, rename/directory-sync uncertainty and
  callback cancellation defects. Each was corrected with deterministic regression tests. Review
  also identified the oversized-Library unit gap, resolved with v2 partitions. This is code review,
  not a submitted GitHub approval or real-device evidence.
- Four exact-final-source workflows, simulator totals and extension embedding evidence: pending.
  No P053 CI result is reused. Unrelated desktop/Rust/PostgreSQL/package results are separate.

## Physical-device procedures and explicit deferrals

Status: `DEFERRED_PHYSICAL_DEVICE_NOT_AVAILABLE`. No iPhone, signing identity or Apple Developer
provisioning is available; deterministic XCTest does not establish Files execution.

1. Sign host and extension with the same team, correct bundle IDs, activated `group.com.synveil.ios`
   and compatible profiles. Verify installed `.appex` and OS recognition; verify no automatic domain.
2. In an explicitly qualified test build, install the transaction-bound canonical capture source and
   session withdrawal hooks. Publish an authorized complete catalog, inspect durable readback, then
   explicitly register a disposable test domain through a separately approved acceptance harness.
3. In Files, enumerate root, multiple Libraries, nested/empty folders and a large paginated folder;
   compare IDs, parents, names, versions and counts against server-confirmed metadata. Validate a
   partitioned Library over 10,000 records without claiming content downloads or write support.
4. Confirm server rename/move preserves identity and updates both parents; confirm tombstones and
   complete-catalog removals propagate without duplicate completion or mass deletion on network loss.
5. Terminate/relaunch the extension; recover anchors mid-batch. Compact history and verify native
   expiration safely re-enumerates. Inject unit/manifest/uncertainty-marker corruption and disk full;
   verify no partial generation, fake empty root or unsupported success.
6. Lock before first unlock and after unlock, then unlock. Verify complete file protection denies
   locked reads, and callbacks resume only with valid evidence. Test protected-data changes during delivery.
7. Logout, revoke, replace credentials and switch account during enumeration/publication/signaling.
   Confirm successful withdrawal fences old callbacks and old pages/anchors do not attach to the next account.
8. Remove entitlement in a disposable profile; invalidate/disconnect the test domain; lose host
   network connectivity during refresh. Confirm explicit errors, retained prior committed state,
   no implicit registration, no extension credentials/network, no content or mutation operations.

| Deferred requirement | Reason | Risk | Safe fallback | Evidence needed | Reentry condition |
| --- | --- | --- | --- | --- | --- |
| Signed App Group/extension and Files app device qualification | No physical iPhone, signing identity or provisioning | Simulator cannot establish entitlement/OS callback behavior | Production publication and registration OFF | Signed-device procedure above with device/iOS/version logs and server comparison | Device/signing available and independent acceptance |
| Live production publication source and session lifecycle installation | Existing offline APIs cannot attest a complete canonical domain cut; entitlement qualification unavailable | Partial/stale cache could become remote authority; missing withdrawal hook could retain access | Fully tested source contract and deterministic coordinator, production gate OFF | Transaction-bound capture and logout/revocation/account-switch integration evidence | Independently qualify source/withdrawal before changing gate |
| Production domain registration | End-to-end metadata/signing acceptance outstanding | Exposes unqualified provider behavior to Files | No automatic registration; signal unregistered outcome deferred | Signed Files acceptance and product/security review | Explicit activation authorization after acceptance |
| Real OS-managed File Provider scheduling | No device/registered domain | Relaunch/scheduling differs from deterministic tests | Durable generation journal; no scheduling claims | On-device relaunch, signals, anchor recovery logs | Device qualification |
| P055 content materialization and Files-originated operations | Not authorized in P054 | Unverified bytes or unsupported remote writes | All content/mutation callbacks fail-closed; conservative capabilities | Separately authorized P055 design and acceptance | P055 authorization; not started here |
| Extension credential-sharing redesign | Not authorized; P053 isolation retained | Credential exposure | Host-only Keychain and authenticated requests | Separate reviewed credential/security design | Explicit separate authorization |
| P046/P051 background-transfer activation and live deployment acceptance | P051 deployment/security gates remain unmet | Credential/PATCH forwarding and OS behavior unresolved | Background transfers OFF; no new URLSession engine | Existing P051 deployment, independent security and device evidence | Separately approved P051 reentry |

`DEFERRED` is not `PASS`. Core enumeration, partition validation, journal, anchors and authorization
fencing are implemented and must pass native validation; device unavailability does not waive them.

## P055 prerequisites and next safe action

Resolve any native failures; require all four gates on final P054 source SHA and independent GitHub
review before merge. Do not merge without authorization and acceptance gates. Record final feature
SHA, PR/review state, exact integration SHA and merge SHA only if actually merged. Next milestone
remains unauthorized. P055 must preserve complete metadata authority, immutable FileVersion identity,
revocation fencing and P051 restrictions while separately designing verified content materialization.
