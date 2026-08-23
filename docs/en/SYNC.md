# Change-journal synchronization protocol

Status: **PLANNED normative blueprint**

This document specifies Synveil's multi-device synchronization model. It
follows ADR-006 and the domain vocabulary in
[DOMAIN_MODEL.md](DOMAIN_MODEL.md). Canonical object and Trash rules are in
[STORAGE.md](STORAGE.md); content transfer is in
[UPLOADS.md](UPLOADS.md). Backup is intentionally separate and is specified in
[BACKUP.md](BACKUP.md).

The protocol is designed for future web, shared-Rust-core desktop, and Apple
clients, with Android, iPhone, and iPad as future mobile profiles. It does not
claim those clients are implemented. Windows, macOS, Linux Desktop, and Linux
Server are first-class host targets only when their client/service evidence gate
passes.

## Sync versus backup

Synchronization converges a `Library` toward one current logical namespace.
Create, edit, rename, move, Trash, and restore propagate to devices. Backup
captures immutable historical manifests under retention; source absence in a
new backup does not destroy earlier snapshots.

An upload-only backup is therefore not a sync filter and a sync tombstone is
not a backup-retention command. APIs, tables, events, status, and tests keep
these domains distinct.

## Protocol invariants

1. Each `Library` has one journal epoch and one transactionally incremented
   `BIGINT` change head. A sequence has meaning only with that library+epoch.
2. Sequence allocation is serialized by a locked `sync_head` row inside the
   same transaction as the domain mutation and event inserts. It is not a bare
   PostgreSQL sequence and cannot become visible before its event commits.
3. If a client receives a cursor at sequence `n`, every committed event with
   sequence `<= n` is visible in that journal epoch. A later transaction cannot
   commit an earlier allocated sequence behind the cursor.
4. The cursor is an authenticated, opaque, versioned server token. It is not
   an authorization capability and clients never parse, increment, or create
   it.
5. Change pages may be redelivered. A client applies a complete page and saves
   its returned cursor in one local transaction.
6. Every logical mutation carries a stable `client_mutation_id` and an
   operation-appropriate base version/revision. A repeated mutation returns the
   persisted outcome and does not append another event.
7. Content conflicts preserve both verified byte streams. Metadata conflicts
   return current authoritative state for explicit rebase; there is no silent
   last-writer-wins.
8. A `ChangeEvent` is a durable resulting fact/invalidation, not an object-store
   command, authorization grant, arbitrary plugin event, or complete audit log.
9. Trash emits a recursive tombstone for the affected subtree. Physical purge
   and object GC never masquerade as a new client content deletion.
10. A cursor older than retained history fails with directed rebaseline. The
    server never resets it to zero or returns a partial history as complete.
11. All journal reads that establish/advance a cursor use the PostgreSQL write
    authority. A lagging read replica cannot serve them.
12. Cross-library order is undefined. There is no global timestamp or Lamport
    clock that clients may use to merge libraries.

## Server journal model

### Sync head

Each library has exactly one lockable record equivalent to:

```text
library_id
journal_epoch
last_committed_sequence BIGINT
minimum_retained_sequence BIGINT
revision
```

`last_committed_sequence` begins at zero. `journal_epoch` changes only through
a reviewed recovery/migration that invalidates prior cursors. It is not changed
on process restart, retention pruning, device revocation, or ordinary schema
deployment.

`BIGINT` exhaustion is guarded well before the limit. Resetting or wrapping a
sequence is forbidden; a deliberate new epoch and client rebaseline is the only
allowed transition.

### Gap-safe allocation algorithm

For a mutation that produces `k >= 1` journal events:

1. Begin a PostgreSQL transaction against the primary.
2. Reserve and lock the unique idempotency receipt (or re-read the winner after
   its unique insert commits). If an identical mutation already committed,
   return it without touching domain rows or the sync head; if its fingerprint
   differs, fail before mutation. This prevents two first deliveries of the
   same client mutation from both reaching domain writes.
3. Authorize and lock affected domain rows in the documented canonical order.
   Validate base versions/revisions, name uniqueness, state, hierarchy, quota,
   and object durability. Stage the domain row changes in this transaction.
4. Determine the bounded event group and payloads from resulting state.
5. Late in the transaction, lock this library's `sync_head` row `FOR UPDATE`.
   Every code path that allocates events obeys this order; no path holds the
   head and then waits for unordered domain locks.
6. Set `first = head + 1`, `last = head + k` with checked arithmetic; update
   the head in the transaction and insert exactly the contiguous events
   `first..last` with one transaction/group ID.
7. Insert the audit fact, required outbox/jobs, and the mutation receipt with
   resulting IDs, revisions, versions, and sequence interval.
8. Commit while still holding the head lock. The next allocator cannot observe
   or allocate after this interval until commit/rollback releases the row.

If the transaction rolls back, the head update and event rows roll back, so the
next transaction may reuse the number; no cursor-visible gap is created. If the
response is lost after commit, mutation receipt lookup returns the same interval
and result.

The per-library head lock is intentionally a brief correctness serialization
point. Hashing, object writes, network requests, and long validation scans occur
before the transaction or outside it. A future sharded/multi-writer clock needs
a superseding ADR and protocol proof; it cannot silently replace this rule.

### Lock ordering and directory cycles

Ordinary node locks use canonical UUID order after required parent/name guards.
Directory moves need predicate protection against two concurrent moves creating
a cycle, and subtree Trash must not commit concurrently with an unchecked
descendant edit.

The initial correctness profile takes one transaction-scoped per-library
namespace-mutation advisory lock (or an equivalent dedicated guard row) before
any short transaction that mutates a `Node`, then locks source/target/domain
rows in canonical order. Object upload and hashing occur before this transaction,
so the guard does not cover byte transfer. This deliberately serializes logical
node commits within one library, rejects moves into self/descendant/across
libraries/under a file, and gives edit-versus-subtree-delete a definite order.
It acquires `sync_head` only after the domain mutation is ready.

A measured scale revision may replace the coarse guard with shared ancestor and
exclusive subtree locks plus an `ltree`, closure table, or materialized-path
projection. It must preserve cycle safety, subtree fencing, move atomicity,
stable node IDs, and the established lock order through conformance tests; it
does not quietly weaken them for throughput.

## Change event contract

Each event contains:

- event ID, library ID, epoch, sequence, event group/transaction ID, and event
  schema version;
- kind, subject node ID, resulting node revision/state, and current version ID
  when applicable;
- minimal resulting projection such as kind, parent ID, display name/name-key
  version, previous parent/name where needed for cache invalidation, recursive
  flag, or conflict origin;
- actor user/device safe identity, server commit time, and causal
  `client_mutation_id`/operation correlation;
- an explicit projection-completeness flag or resource URL when the client
  must refetch current state.

Events carry resulting server facts rather than unsafe path deltas. A client
can ignore an event whose resulting node revision is older than the revision it
already holds, while still recording the event as applied. Unknown required
schema versions stop cursor advancement with `unsupported_event_version`;
unknown optional fields are ignored.

Initial registered kinds should include:

| Kind | Required meaning |
|---|---|
| `NODE_CREATED` | A new active file/directory identity and projection exists. |
| `CONTENT_UPDATED` | An existing file has a new current immutable version. |
| `NODE_RENAMED` | Same node ID and content, new display/name comparison projection. |
| `NODE_MOVED` | Same node ID and content, new parent; may also include rename. |
| `NODE_TRASHED` | Root is soft-deleted; `recursive=true` invalidates its known subtree. |
| `NODE_RESTORED` | Trashed root is active at the resulting parent/name. |
| `NODE_PURGED` | Logical Trash record is no longer restorable; prior live tombstone remains semantically valid. |
| `VERSION_RESTORED` | Existing file has a newly created head version sourced from history. |
| `CONFLICT_CREATED` | A visible conflict/recovered node/version preserves an incoming edit. |

One logical transaction can emit multiple events. Feed pages do not split an
event group: `limit` is a target, and the server may return the remainder of one
bounded group up to a documented hard maximum. Bulk/subtree operations use a
compact root event or bounded transactional batches rather than an unbounded
event group.

Change events are retained immutable until journal-retention work advances the
minimum sequence. Corrections are new events/state, not edits to history.

## Cursor format and validation

The public cursor is an opaque encoded token whose authenticated server claims
include:

```text
token_format_version
signing_key_id
library_id
journal_epoch
position_sequence
optional feed/profile version
```

The encoding is integrity-protected (for example, MAC or authenticated
envelope) with versioned key rotation. It must not expose raw database secrets
or be accepted as authorization. Old signing keys remain verifiable for at
least the supported journal/retry horizon or produce a directed rebaseline;
key rotation cannot cause silent history loss.

Validation distinguishes:

- `invalid_cursor`: malformed, failed integrity, wrong library, unsupported
  token version, or cursor beyond the committed head;
- `cursor_epoch_mismatch`: otherwise valid token for an old/new epoch;
- `cursor_expired`: sequence is below retained history;
- `permission_denied`: authenticated principal/device no longer has access,
  without disclosing library details.

Errors include a stable recovery action/route where safe, not decoded cursor
claims or signing detail.

## Pull API

```http
GET /api/v1/libraries/{library_id}/changes?cursor=<opaque>&limit=<bounded>
```

The server:

1. authenticates/authorizes current library access and device state;
2. decodes/validates cursor, epoch, minimum retained sequence, and feed profile;
3. in a short `REPEATABLE READ`, read-only transaction on the primary, checks
   the retained boundary, captures a committed head `H`, and queries events
   `sequence > cursor.sequence AND sequence <= H` in order without splitting a
   group; the fixed MVCC snapshot prevents concurrent retention from removing
   rows between boundary validation and the event query;
4. returns events and a server-generated `next_cursor` at the last returned
   sequence, or at `H` when the bounded query proves no event is omitted;
5. may update a device acknowledgement/checkpoint separately as an operational
   hint. That checkpoint never retroactively makes an unreturned event safe.

Example response shape:

```json
{
  "events": [],
  "next_cursor": "opaque",
  "has_more": false,
  "journal_epoch": "opaque-display-value-if-contract-requires",
  "server_time": "RFC3339"
}
```

Exact JSON belongs in reviewed OpenAPI. A response may be repeated after a
lost network acknowledgement. Long polling/WebSocket notification can later
say "changes may be available," but the journal pull remains truth; a dropped
notification cannot drop data.

### Client apply rule

For each page, a client uses one local database transaction:

1. verify library/profile/event versions and order;
2. deduplicate already applied `(epoch, sequence)` facts;
3. apply resulting projections/tombstones in ascending sequence, using node
   revision to avoid regressing a newer bootstrap projection;
4. record any bounded content-download work separately;
5. persist `next_cursor` only after every event in the page is durably applied;
6. commit, then request the next page.

If local commit or process shutdown fails, the old cursor causes safe replay.
File bytes download into a temp, verify canonical hash/length, and atomically
replace a local materialization. Metadata convergence does not wait for all
cloud-only content to hydrate.

## Initial bootstrap and cursor rebaseline

A plain paginated live-tree listing can miss/mix concurrent changes unless it
is paired with a journal boundary. The initial protocol uses a bounded
server-side bootstrap session:

1. `POST /api/v1/libraries/{library_id}/sync-bootstrap` authenticates the device,
   captures current epoch and head `H` on the primary, stores the feed/policy
   profile, creates an expiry, and pins retention at/before `H` for that bounded
   bootstrap lifetime. Capture and pin commit atomically under the same
   retention/head guard that pruning checks, so pruning cannot pass `H` between
   the two actions.
2. The client pages the current authoritative node projection using an opaque
   bootstrap page cursor ordered only by immutable node ID, never mutable path,
   name, or modification time. The server may include current revisions newer
   than `H`; this is safe because later events carry resulting revisions.
3. The client writes pages into a staging local namespace. Parents may arrive
   after children; it validates/relinks before exposing the completed view.
4. After the listing completes, the server returns the cursor for exactly `H`.
   The client atomically installs the staged view and then drains all events
   after `H`, ignoring lower/equal resulting revisions as appropriate.
5. The bootstrap retention pin remains until acknowledged completion or TTL.
   If it expires, the client discards staging and restarts; the server never
   supplies a cursor whose intervening events may already be pruned.

Why this converges during writes:

- a node changed after `H` may appear in its newer state in the listing, and its
  event repeats that resulting revision;
- a node deleted after `H` may be absent or present depending on page timing,
  but the retained tombstone after `H` establishes the final state;
- a node created and deleted during the scan has both retained facts even if it
  never appears in the listing;
- stable ID ordering prevents rename/move from skipping pagination position.

The client does not expose the mixed bootstrap as a final synchronized state
until it has drained changes to a chosen observed head.

`cursor_expired` and epoch mismatch return this workflow. They do not ask a
client to upload every local file as new. Before rebaseline, clients preserve
unsent local mutations in their durable outbound queue; after rebaseline they
rebase/submit them using original base facts and conflict rules.

## Push mutations

Normal file/folder APIs and/or a reviewed sync mutation endpoint accept:

- `client_mutation_id`, unique within device+library;
- operation kind and normalized request fingerprint;
- expected node metadata revision/ETag for metadata operations;
- `base_version_id` for content operations;
- target parent/name and expected parent revision where required;
- a server-issued subtree precondition for recursive directory Trash/purge
  confirmation;
- client capability/protocol version;
- content upload session result for content-bearing operations.

A persistent mutation receipt has a unique key
`(device_id, library_id, client_mutation_id)` and stores request fingerprint,
state, result/error, node/version IDs, event interval, and commit time. An
identical retry returns it. Reusing the ID for a different request returns
`idempotency_conflict`.

Receipts for committed content/namespace mutations should be compactly retained
for at least the lifetime of the affected library/device, not pruned merely
because the journal event aged out. Otherwise a very late lost-response retry
could create a second version after its deduplication record disappeared.

### Operation-specific preconditions

- **Content replace:** base current `FileVersion` is the causal precondition.
  A rename/move may commute if content head and state remain compatible. An
  interactive caller may add a strict node ETag to reject any concurrent
  metadata change.
- **Rename/move:** expected node metadata revision plus target parent/name
  constraints. Content changing meanwhile causes a conflict only if the ETag
  contract covers it; the API returns current state for rebase.
- **Trash/delete:** expected node revision/state. Recursive directory Trash also
  carries an opaque subtree precondition that changes when a retained
  descendant is created, edited, moved, or deleted. A stale delete never
  silently removes a newer edit. An explicit "delete current subtree anyway"
  confirmation is a new mutation with a freshly issued precondition; content
  remains in Trash retention.
- **Restore:** expected Trash/node revision and explicit collision target/policy.
- **Create:** expected parent revision when required plus active name uniqueness.

This avoids treating a physical object ETag, client timestamp, or wall clock as
a causal version.

## Deterministic conflict behavior

Deterministic means the accepted transaction order and a versioned server rule
produce one observable result; it does not mean device clocks choose a winner.
Client timestamps and lexicographic device IDs never silently arbitrate data.

### Concurrent content edits

Given server head `v4` and two offline edits based on `v4`:

```text
Device A: v4 -> bytes A
Device B: v4 -> bytes B
```

The first successful commit under the node lock creates `v5A` and keeps the
original node as head. When the second verified upload revalidates its base:

1. it does not replace `v5A`;
2. in the same transaction, it creates one sibling conflict `Node` with a new
   UUIDv7 and one current `FileVersion` referencing bytes B;
3. that version records `conflict_base_version_id=v4`, a conflict group/reason,
   source device, original node ID, and safe server commit time;
4. it assigns a server-generated portable conflict display name with a
   versioned naming algorithm and collision-proof opaque suffix;
5. it appends `CONFLICT_CREATED` (and the needed node projection) to the same
   event group, stores the mutation receipt, audit, and outbox atomically;
6. retry of Device B's mutation returns the same conflict node/version.

A visible sibling is preferred to a hidden alternate version because older
clients can preserve and display it even without a specialized conflict UI.
The original and conflict objects remain immutable/addressable. Resolution is
an explicit later operation: keep one, manually merge into a new version, or
Trash a resolved copy. Synveil never claims an automatic binary merge.

If identical verified bytes arrive from both edits, policy may avoid a
user-visible conflict only if the resulting content hash/length is exactly
equal and metadata changes commute; the mutation receipt still records that
the stale submission was resolved as `IDENTICAL_CONTENT`.

### Delete versus offline edit

- If Trash commits first and an offline content edit later arrives from the
  former version, the original stays trashed. Synveil creates one visible
  recovered-conflict node using the verified incoming bytes. It chooses the
  former parent if active/authorized; otherwise the nearest retained active
  ancestor, finally the library root, and uses a collision-proof recovered
  name. The event reason is `DELETE_VS_EDIT`.
- If the content edit commits first and a stale file delete arrives, the delete
  returns `version_conflict` with current revision/version. For a directory,
  the changed subtree precondition produces the same conflict even when the
  directory's own display metadata did not change. The client/user must
  explicitly confirm deletion of current state with a new mutation.
- A directory Trash versus an edit to a descendant follows the same rule. It
  never mutates a node inside the trashed subtree back to active implicitly.

This preserves bytes without defeating a deliberate deletion or hiding data
inside an inaccessible subtree.

### Metadata conflicts

- Concurrent renames/moves: one expected revision commits; the stale request
  gets `version_conflict` plus current projection and may rebase with a new
  mutation ID. No duplicate empty file is manufactured.
- Concurrent creates with the same comparison key: one wins; the other gets
  `name_conflict` and must choose/accept an explicit non-destructive rename
  policy.
- Rename/move and content update may commute by stable node ID when the content
  base remains current and the content caller did not demand strict metadata
  ETag matching.
- Move to a deleted/trashed/unauthorized parent fails with current safe state;
  it never falls back to root silently.
- Directory move cycles are prevented by the structural lock/ancestry rule.
- Restore into an occupied former name returns conflict unless the caller
  explicitly selects a new target/name; it never overwrites the occupant.

### Conflict lifecycle

Conflict nodes are ordinary visible nodes after creation: they sync, version,
share, Trash, back up, and count toward quota. Their conflict metadata remains
for explanation/audit but does not grant access to the origin node. Renaming a
conflict does not remove its history. A future resolution UI can group them by
conflict group but cannot delete bytes without an ordinary authorized mutation.

## Rename, move, copy, Trash, and restore propagation

- Rename/move events preserve node ID and content version. Clients update local
  paths with platform-safe atomic operations or mark a local name conflict; they
  never reupload bytes merely because the path changed.
- Copy creates a new node/version identity and journal fact. Object bytes may be
  reused within the dedup domain.
- `NODE_TRASHED recursive=true` invalidates the root and every locally known
  descendant from the active view. Bytes/versions remain server-side through
  Trash retention.
- `NODE_RESTORED` restores the retained subtree at the resulting parent/name.
  A client that discarded local bytes redownloads by version; it does not infer
  resurrection from stale filesystem residue.
- `NODE_PURGED` removes the item from Recently Deleted. Clients that already
  applied `NODE_TRASHED` do not perform a second live deletion; object GC is
  never exposed as a sync event.

When a local platform cannot represent a server name (case collision, reserved
name, unsupported code point), the client records an explicit local
`UNREPRESENTABLE`/`NAME_CONFLICT` state and preserves the server node identity.
It must not rename the server copy without user/policy consent.

## Selective sync and files on demand

Initial sync cursors are library-wide. All authorized node metadata and
tombstones required for convergence are delivered; device policy decides which
content versions to hydrate. This avoids a filter-boundary move silently losing
delete/create semantics.

Suggested local materialization states are:

```text
LOCAL
CLOUD_ONLY
PINNED
DOWNLOADING
UPLOADING
CONFLICT
UNAVAILABLE
EXCLUDED_CONTENT
```

These are device-local projections, not alternate server object states. The
server supplies immutable version ID, length, hash, media metadata, and range
download authorization. A client writes downloads to a temp, verifies, atomically
installs, and records the hydrated version. Local filesystem watchers suppress
echo using durable node/version mapping, not a timing sleep.

Future server-side filtered feeds require a versioned filter identity embedded
in cursor, synthetic enter/leave events for moves, authorization analysis, and
rebaseline rules. They cannot reuse an unfiltered cursor.

## Client platform obligations

All clients use the same journal, mutation, version, conflict, backup, and
pairing protocol. Platform adapters own only host behavior:

- Windows handles NTFS/ReFS naming, reparse/junction policy, Windows Service or
  the declared supervisor, Credential Manager/DPAPI, locked files, sleep/reboot,
  and selected-folder watcher/rescan behavior.
- macOS handles APFS naming/case behavior, launchd, Keychain, sandbox/privacy
  permissions, file coordination, sleep/wake, and selected-folder/photo access.
- Linux Desktop handles the declared filesystem/service/key-store profiles,
  watcher variance, desktop notifications, portable-name warnings, and
  user-service versus system-service policy.
- Linux Server handles host/service and remote-operator behavior; it is not
  assumed to provide a desktop shell, watcher, or user-session credential UI.
- Future Android, iPhone, and iPad clients negotiate background execution,
  filesystem/photo-library access, local cache, notification, and credential
  capabilities. They may schedule best-effort work, but cannot claim continuous
  background sync or full-device backup unless a separate gate proves it.

No adapter may turn a local cache eviction, unavailable placeholder, watcher
miss, unsupported metadata field, or service restart into a server deletion.
When a capability is absent, the client reports a stable unsupported/degraded
state, preserves server identity, and uses authoritative rescan/rebaseline or
explicit user action.

## Device checkpoints and journal retention

A device/library checkpoint records last acknowledged cursor sequence, last
contact, client/protocol version, and health. It supports UI and retention
warning but is not trusted proof that local bytes exist.

Retention uses both age and storage bounds and publishes the minimum retained
sequence. A bootstrap session temporarily pins its start boundary. An offline
device cannot pin the journal forever by default; when it falls behind minimum,
it becomes `REBASE_REQUIRED` and uses bootstrap while preserving its outbound
mutation queue.

Before pruning, the retention job locks/checks epoch and head, honors active
bootstrap pins, deletes only a bounded prefix, and advances
`minimum_retained_sequence` atomically with progress. Cursors at the boundary
have one tested inclusive/exclusive rule. Event pruning never deletes mutation
receipts, `FileVersion` history, Trash records, audit facts, or backup manifests
as a side effect.

## Error contract

Sync APIs use stable codes:

- `invalid_cursor`, `cursor_expired`, `cursor_epoch_mismatch`;
- `unsupported_protocol_version` and `unsupported_event_version`;
- `version_conflict`, `name_conflict`, `idempotency_conflict`;
- `resource_in_trash`, `parent_not_found`, `invalid_move`, `cycle_detected`;
- `upload_incomplete`, `checksum_mismatch`, `quota_exceeded`;
- `device_paused`, `device_revoked`, `permission_denied`;
- `storage_unavailable`, `storage_capability_unavailable`, `pairing_required`,
  `rate_limited`, and `internal_error`.

Conflict responses include the caller-authorized current node revision/version,
safe retry guidance, and whether verified incoming content was preserved as a
conflict node. They never leak another user's collision or object existence.

## Event, outbox, and notification boundary

The mutation transaction inserts the client `ChangeEvent`, security
`AuditEvent`, and required internal outbox/jobs together. They are not one
table or contract:

- journal delivery is ordered per library and retained for cursor replay;
- internal work is at-least-once, unordered unless its handler defines a stable
  aggregate revision check;
- audit is accountability evidence with separate access/retention;
- WebSocket/push notification is an optional hint containing no canonical
  mutation. On reconnect/drop, clients pull the journal.

Optional index/thumbnail/notification failure cannot roll back or block a sync
mutation. A derivative handler binds work to immutable `FileVersion` and no-ops
if already complete or stale.

## Failure and recovery matrix

| Failure | Required behavior |
|---|---|
| Process dies before transaction commit | Mutation, events, receipt, and head increment all roll back; same client mutation retries. |
| Process dies after commit before response | Receipt returns identical result/event interval; no second version/conflict. |
| Earlier transaction waits while later starts | `sync_head` lock makes allocation/commit visibility ordered; later cannot expose a cursor past uncommitted earlier event. |
| Event insert fails after staged domain update | Entire PostgreSQL transaction rolls back. |
| Outbox consumer is offline | Mutation/journal remains committed; durable job lag is observable. |
| Change response is lost | Old cursor requests the page again; client sequence dedupe makes replay safe. |
| Client local DB crashes mid-page | Cursor was not advanced; entire page replays. |
| Cursor is pruned/old epoch | Directed bootstrap; no empty/partial feed claimed current. |
| Bootstrap runs during heavy mutation | Start boundary is pinned; stable-ID listing plus all post-boundary events converges. |
| Device loses local database but retains files | Rebaseline identities content; unmatched local modifications submit with explicit base/conflict, never bulk overwrite. |
| Two content writes race | First accepted head wins journal order; second verified bytes become one conflict copy. |
| Trash races offline edit | Trash remains; edit becomes visible recovered conflict, or stale delete is rejected depending on commit order. |
| Two directory moves could form a cycle | Structural serialization makes one observe/reject the other's result. |
| Object backend unavailable | Metadata pull continues where safe; content hydration reports retryable unavailable; no fake zero-byte file. |
| Device credential revoked | New pull/push fails immediately under auth policy; already downloaded bytes cannot be recalled. |

## Required sync scenarios

### Basic convergence

- create file/directory on A -> exact node/version appears on B;
- content update on A -> B atomically replaces only after hash verification;
- rename on A -> B changes path without upload/download when content local;
- move file and nonempty directory on A -> B preserves node IDs and subtree;
- Trash on A -> recursive tombstone on B; restore -> retained subtree reappears;
- purge removes Recently Deleted state but does not invent a second live delete;
- copy creates a distinct node/version and may reuse server object bytes;
- old-version restore creates a new head and syncs as a new immutable version.

### Offline and conflict matrix

- A and B edit `v4` offline with different bytes; reconnect A-then-B and
  B-then-A, proving one head plus one visible conflict and no lost bytes;
- repeat losing mutation after response loss, proving no duplicate conflict;
- identical-content concurrent edits resolve without byte loss or duplicate
  object and with documented outcome;
- rename/rename, move/move, rename/move, create/create name collision;
- rename on A while B edits content; content can commute by stable ID/base;
- edit then delete and delete then edit for files and descendants of a trashed
  directory, including subtree-precondition invalidation;
- restore while another client creates the former name;
- move a directory under its descendant and two concurrent inverse moves;
- offline client returns after Trash retention/purge and submits queued edits;
- case/Unicode/reserved-name conflict across Linux, Windows, and macOS fixtures.

### Journal/cursor correctness

- force transaction T1 to stage an event then pause; run T2 and prove no cursor
  can advance past T1 through commit inversion;
- force T1 rollback after head lock and prove its sequence is safely reused/no
  visible gap;
- allocate multi-event groups and prove pages never split them;
- duplicate page, overlapping retries, empty page that advances to captured
  head, page limit boundary, very large backlog, and concurrent retention;
- forged, wrong-library, beyond-head, malformed, expired, old-epoch, and
  old-signing-key cursors;
- primary versus deliberately lagged replica test proves cursor reads never use
  the replica;
- event schema unknown-required behavior stops local cursor; optional fields do
  not;
- journal pruning boundary is exact and active bootstrap pin blocks unsafe
  deletion.

### Bootstrap/rebaseline

- mutate/create/move/Trash/purge nodes between every bootstrap page; after
  draining post-`H` events client matches authoritative server state;
- parent arrives after child, node revision in snapshot exceeds early replayed
  event, node created+deleted entirely during scan, and path changes never
  disturb stable-ID pagination;
- bootstrap expires mid-scan, retention pressure occurs, and restart does not
  reuse an unsafe page/cursor;
- stale device keeps an outbound edit queue through rebaseline and resolves
  against current base without treating every local file as new;
- million-node synthetic library uses bounded server/client memory and no
  long-lived PostgreSQL transaction.

### Idempotency, crash, and model testing

- each mutation crashes before/after domain write, head allocation, event,
  receipt, DB commit, and response; outcome is zero or one semantic mutation;
- two API processes and multiple workers preserve head/event/receipt
  invariants;
- randomized commands over nodes, versions, Trash, devices, and cursors compare
  server plus multiple client models after arbitrary disconnect/reorder/replay;
- property: advancing a valid cursor never omits a committed event at or below
  its sequence;
- property: every accepted content byte stream remains reachable as current,
  historical, Trash, or explicit conflict until a documented retention action;
- fuzz cursor decoder, event payload versioning, names, path depth, mutation
  batches, and checked sequence arithmetic.

## Observability and release gate

Metrics cover events committed/read, per-library head/minimum, event-group size,
head-lock wait, change page latency/bytes, duplicate page/mutation rate, device
lag, stale/rebase-required devices, bootstrap age/pages, retention backlog,
conflict kind/count, unsupported protocol/events, and content hydration errors.
Traces correlate request, device, mutation, upload completion, journal group,
and outbox without logging cursor secrets, tokens, content, or unnecessarily
full paths.

The sync phase cannot be marked complete until the reference client state
machine, fault-injected PostgreSQL/object integration suite, multi-client model
tests, all scenarios above, journal retention/rebaseline runbook, and protocol
version compatibility fixtures pass.

## Open decisions

OPEN DECISION OD-SYNC-001: portable conflict display-name format
Owner: Sync / Storage / Clients / Product
Needed by: Phase 4 protocol fixture freeze
Options: original stem plus device/date/opaque suffix; dedicated conflict directory plus original name; UI-only label over an opaque safe stored name
Recommendation: create a sibling/recovered visible node with a versioned portable name derived from preserved display stem, safe device label, server UTC date, and collision-proof opaque suffix; exact Unicode folding follows the accepted namespace policy
Decision evidence: cross-platform round-trip fixtures, accessibility/usability review, path-length limits, and deterministic collision tests

OPEN DECISION OD-SYNC-002: journal retention service level
Owner: Sync / Operations / Product
Needed by: Phase 4 production operations gate
Options: fixed age only; size cap only; age target with protected minimum size and forced rebaseline beyond it; device-ack pinning
Recommendation: use a configurable age target plus capacity guard, bounded bootstrap pins, and explicit stale-device/rebaseline status; do not allow an abandoned device to pin history forever
Decision evidence: event-volume benchmark, household/device offline expectations, disk-capacity test, and rebaseline duration measurement

OPEN DECISION OD-SYNC-003: directory ancestry representation
Owner: Database / Sync / Storage
Needed by: Phase 1 move schema gate
Options: adjacency list with serialized directory moves; PostgreSQL `ltree` materialized path; closure table
Recommendation: begin with adjacency list plus one per-library namespace-mutation guard for short Node commits and indexed recursive queries; introduce finer-grained path/closure locking only after measured contention, never as a user-visible identity
Decision evidence: concurrent cycle tests, million-node move/list benchmarks, migration complexity, and extension portability review

OPEN DECISION OD-SYNC-004: recursive subtree precondition representation
Owner: Sync / Database / Clients
Needed by: Phase 1 schema/API freeze, before Phase 2 recursive Trash
Options: per-directory subtree revision updated along ancestors; opaque snapshot token plus journal descendant-conflict query; library-head precondition as a conservative coarse guard
Recommendation: expose an opaque subtree ETag backed initially by a per-directory subtree revision updated under the namespace guard; keep it separate from display-metadata revision so clients know which precondition they are presenting
Decision evidence: delete-versus-descendant-edit model tests, deep-tree write benchmark, move/Trash races, and offline client fixtures
