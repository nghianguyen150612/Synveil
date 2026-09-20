# Change-journal synchronization protocol

Status: **Prompt 31–36 foundations and desktop inbound core are VALIDATED.
Prompt 37 server profiles, enrollment groundwork, device authentication,
secure credential persistence, and production HTTP SyncRemote are IMPLEMENTED.
Prompt 91 bounded bidirectional cycle composition and Prompt 92 process-local
runtime scheduling, and Prompt 93 durable-change-first runtime signal
integration are IMPLEMENTED; automatic conflict resolution, OS service
integration, and broader product lifecycle remain a PLANNED normative
blueprint.**

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

Prompt 34 implements the authenticated server boundary
`POST /api/v1/devices/{device_id}/libraries/{library_id}/mutations`: one strict
logical `CREATE_DIRECTORY`, `RENAME_NODE`, `MOVE_NODE`, `TRASH_NODE`, or
`RESTORE_NODE` per request, durable UUID idempotency, canonical fingerprinting,
explicit optimistic preconditions, deterministic persisted conflicts, and one
exact journal event on success. It accepts no file bytes or physical storage
identity and performs no automatic conflict copy, merge, or last-write-wins
resolution. Prompt 35 adds durable conflict evidence, bounded authenticated
inspection, and explicit idempotent manual decisions. Prompt 91 implements the
bounded transport-neutral client cycle that composes the existing inbound and
outbound engines; full product lifecycle, UI, and broader content protocols
remain future behavior. Prompt 36 implements inbound apply, and Prompt 37
connects it to the real server with device credentials.

## Prompt 37 synchronization status

| Capability | Status |
|---|---|
| durable change journal | `VALIDATED` |
| device checkpoints/feed | `VALIDATED` |
| snapshot/rebaseline | `VALIDATED` |
| client mutation submission | `VALIDATED` |
| optimistic conflict detection | `VALIDATED` |
| durable conflict records | `IMPLEMENTED` |
| manual conflict inspection | `IMPLEMENTED` |
| explicit manual resolution | `IMPLEMENTED` |
| desktop inbound sync core | `VALIDATED` |
| desktop server profiles | `IMPLEMENTED` |
| device enrollment groundwork | `IMPLEMENTED` |
| device bearer authentication | `IMPLEMENTED` |
| secure desktop credential persistence | `IMPLEMENTED` |
| production HTTP SyncRemote | `IMPLEMENTED` |
| filesystem observation | `IMPLEMENTED` |
| self-generated change suppression | `IMPLEMENTED` |
| durable outbound intent capture | `IMPLEMENTED` |
| rename/move attribution | `IMPLEMENTED with conservative fallback` |
| watcher overflow/reconciliation | `IMPLEMENTED` |
| bounded automatic outbound mutation submission | `IMPLEMENTED (Prompt 91 one-shot cycle)` |
| process-local long-running sync runtime | `IMPLEMENTED (Prompt 92)` |
| desktop sync host/process lifecycle composition | `IMPLEMENTED (Prompt 94)` |
| automatic conflict resolution | `NOT IMPLEMENTED` |
| desktop GUI/pairing UX | `NOT IMPLEMENTED` |

## Implemented local filesystem observation and outbound intent capture

Prompt 38 adds a desktop-local observation layer in `crates/client-sync`. It is a
control-plane boundary only: native `notify` watcher events and deterministic
test-watchers emit bounded hints, but every logical decision comes from durable
SQLite state, managed-root validation, Prompt 36 operation evidence, and
filesystem reinspection through `LocalReplica`. The observer stores typed
`outbound_intents` (`CREATE_DIRECTORY`, `CREATE_FILE`, `RENAME_NODE`,
`MOVE_NODE`, `DELETE_OR_TRASH_NODE`, `MODIFY_FILE_CONTENT`) with UUIDv7 local
intent IDs, base epoch/applied sequence, Node revision/version context, observed
path and fingerprint, and a semantic dedupe hash. It never stores file bytes.

`.synveil/`, staging, quarantine, operation receipts, and local SQLite/control
files are excluded from user namespace classification. Prompt 36 inbound applies
write durable `observation_suppressions` tied to operation ID, NodeId, expected
path/presence/type, and content fingerprint where relevant; suppression is not a
timer. Once the expected Synveil-generated filesystem result is proven, an
immediate user edit with a different fingerprint is observed as a real outbound
intent. Observation never advances `applied_sequence` or `acknowledged_sequence`.

Watcher overflow, dropped events, backend errors, ambiguous unpaired rename
patterns, unstable hashes, locked/unreadable files, unrepresentable names,
portable-name collisions, symlinks/reparse points, and Linux special files become
durable observation issues or `RESCAN_REQUIRED`; the engine does not guess.
Startup always marks reconciliation required and performs a bounded scan to close
downtime gaps. Rename/move intents require paired native/evidence-based
attribution; remove+create timing alone falls back to `AMBIGUOUS_RENAME`/rescan.
The observer has no `SyncRemote` or HTTP transport and does not call
`POST /api/v1/devices/{device_id}/libraries/{library_id}/mutations`, create
upload sessions, complete uploads, or resolve conflicts.

## Implemented durable conflict and manual-resolution protocol

A managed Prompt 34 resource conflict is durable evidence of rejected client
intent. In the same PostgreSQL transaction that terminalizes the original
mutation as CONFLICT, the server creates exactly one UUIDv7 `SyncConflictId`
and one owner/originating-Device/Library-scoped record. The original operation
remains terminal forever. Its same-ID/same-fingerprint replay returns the same
conflict ID and never creates a second record. Authentication, CSRF, malformed
input, mutation-ID reuse, infrastructure failures, and rebaseline requirements
are not resource conflicts and create no record.

The record contains only the closed Prompt 34 intent fields and immutable
historical logical observation. It has no raw JSON, content bytes, physical
storage identity, or Node foreign key. Its lifecycle is exactly OPEN, RESOLVED,
or DISMISSED. A purged-resource conflict therefore remains inspectable and can
be dismissed, but apply can never recreate the Node.

The implemented routes are:

- `GET /api/v1/devices/{device_id}/libraries/{library_id}/conflicts` for an
  OPEN-only page, default 50 and maximum 100, ordered by immutable
  `(created_at DESC, conflict_id DESC)` with no OFFSET and an opaque
  HMAC-authenticated, scope-bound keyset cursor;
- `GET /api/v1/devices/{device_id}/libraries/{library_id}/conflicts/{conflict_id}`
  for typed original intent, explicitly named historical server observation,
  lifecycle, and terminal-resolution linkage; and
- `POST /api/v1/devices/{device_id}/libraries/{library_id}/conflicts/{conflict_id}/resolve`
  for one strict `ACCEPT_SERVER` or `APPLY_CLIENT_INTENT` decision.

Every route reauthorizes the authenticated owner, ACTIVE Device, owned Library,
and exact conflict scope. Conflict-ID possession grants nothing. GET routes are
CSRF-free reads; POST requires the session-bound CSRF proof. All results and
errors are private/no-store, and the resolution body is limited to 16 KiB.

Every decision has a UUIDv7 `resolution_id` and a canonical typed SHA-256
fingerprint over the conflict ID, action, and presence/value of explicit fresh
preconditions. The raw JSON serialization is irrelevant. Same ID/fingerprint
replays the exact terminal outcome, completion time, and journal linkage after
a lost response; different semantics return `resolution_id_conflict`.

`ACCEPT_SERVER` explicitly chooses the current canonical server state. It
atomically records the decision and moves OPEN to DISMISSED, but changes no
Node, emits no resource journal event, and advances no checkpoint.

`APPLY_CLIENT_INTENT` never reuses stale original revisions. The resolver must
supply the current resource revision for every supported kind, plus the current
parent/destination revision for move and restore. The service acquires the
existing library namespace guard and scoped row locks, re-reads current state,
reconstructs the preserved semantic intent with those fresh preconditions, and
invokes the shared Prompt 34 transaction-local canonical executor. Success
atomically commits exactly one Node mutation, one ordinary resource event, the
resolution result/linkage, and conflict RESOLVED. It does not reopen the
original mutation or advance any checkpoint. Another Device observes the event
through the ordinary feed.

If a server rename, move, purge, or other canonical mutation wins first, apply
records a durable replayable stale resolution result, returns
`resolution_conflict`, leaves the conflict OPEN, and commits zero Node change
and zero journal event. It does not refresh, retry, merge, rename, copy, or
select a different action. OPEN evidence survives rebaseline; after the Device
completes rebaseline, a human may inspect current state and submit new fresh
preconditions. Concurrent APPLY/APPLY or ACCEPT/APPLY attempts can produce at
most one terminal conflict decision and at most one resource event. There is no
automatic conflict-resolution policy.

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
6. Every supported logical mutation carries a stable `client_mutation_id`, a
   canonical fingerprint, a base epoch/sequence, and operation-appropriate
   typed preconditions. A repeated mutation returns the persisted outcome and
   does not append another event.
7. Prompt 34 accepts no content bytes. Its metadata conflicts return current
   authoritative logical state for explicit rebase; there is no silent
   last-writer-wins or automatic conflict copy. A future content protocol must
   preserve both verified byte streams.
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

### Prompt 81 internal snapshot foundation

Prompt 81 adds a transport-neutral `LogicalSnapshotService` seam for building a
complete logical library state without adding another HTTP route or changing the
device-scoped bootstrap protocol. The returned `RebaselineSnapshot` contains:

- the owner-authorized library ID and one canonical `LogicalSnapshot` state;
- the current root plus all current `ACTIVE` and `TRASHED` logical Nodes, with
  parent ID, kind, name, revision, and safe current-file content metadata;
- a `JournalHighWatermark`/`JournalCursor` boundary from the same database view.

The builder starts a PostgreSQL `REPEATABLE READ` transaction, takes the existing
per-library namespace guard before the first data read, reads the journal head and
the set-oriented Node projection, orders entries by immutable Node ID, and commits
the state with its boundary. A concurrent cooperative mutation is therefore in
the snapshot and at/before the boundary, or is absent from both and remains in
the feed strictly after the boundary. Wall-clock time is descriptive only.

The snapshot is logical: `PURGING`/purged Nodes and physical object keys,
replica/storage locators, filesystem paths, staging handles, and file bytes are
not part of it. Building or reading it does not create or advance a device
checkpoint, complete rebaseline, apply state on a client, or select a conflict
resolution.

### Prompt 82 durable fixed-cut artifact and paging

Prompt 82 persists the Prompt 81 cut as a library-scoped,
owner-authorized `RebaselineSnapshotId` artifact. Creation stays in one
`REPEATABLE READ` transaction under the same namespace guard: it reads and
validates the complete Prompt 81 aggregate, writes the immutable header with
its `JournalHighWatermark` boundary/count/injected expiry, materializes entries
with one `INSERT ... SELECT`, then commits. If any step fails, rollback leaves
no readable header, boundary, or entry rows.

After commit, `get_rebaseline_snapshot` and
`read_rebaseline_snapshot_page` use only the durable header and entry tables.
They do not retain the creation transaction, acquire the namespace guard, or
query live `nodes`; ordinary mutations can therefore continue while a large
transfer is paged across connections or a process restart. Entries are ordered
by immutable `NodeId` through `(snapshot_id, node_id)` keyset pagination:
`node_id > after_node_id`, ascending, bounded limit. OFFSET is not a snapshot
continuation mechanism.

Every `RebaselineSnapshotPage` repeats its descriptor (artifact ID, Library,
journal boundary, count, timestamps) and yields a snapshot-scoped
`RebaselineSnapshotPageCursor` only after the last returned Node. That cursor
is deliberately not a `JournalCursor`; the journal cursor continues incremental
sync strictly after the captured boundary. The default page size is 256 and the
maximum is 1000. Page memory is O(page size); creation can temporarily allocate
the validated O(total-node-count) aggregate before its set-oriented copy.

Artifacts logically expire at `observed_at >= expires_at` (24-hour Gen-1
default) and then fail with `Expired`. Expired rows may remain physically until
a later lifecycle/retention phase; this prompt adds no cleanup worker, explicit
delete API, retry loop, global lock, HTTP/OpenAPI/SSE/WebSocket route, or client
snapshot application. Snapshot ID guessing by another owner returns `NotFound`,
and creation/read remains checkpoint-neutral.

### Prompt 84 client atomic apply

Client-sync accepts an already-created descriptor through a transport-neutral
page source. It persists at most one candidate per Library in separate SQLite
tables, checking immutable snapshot ID/Library/boundary/count, strict NodeId
ordering, duplicate nodes, opaque cursor cycles, exact terminal count, and the
logical root/parent/reachability invariants before activation. Bounded transfer
uses O(page size) memory; final metadata/path reconstruction is O(nodes).

One short SQLite transaction then replaces only the authoritative remote mirror
(`local_nodes`) and writes `AppliedPendingHandoff`. It neither deletes nor
rewrites outbound intents, their base revisions, upload staging rows, or local
content bytes. Old incremental inbound processing is fenced until a later
handoff phase completes; this phase makes no server ACK/checkpoint call, does
not resolve conflicts, and adds no UI, retry loop, or cleanup daemon.

### Prompt 83C creation-abuse boundary

The durable snapshot service has no existing reusable authenticated rate limiter
or durable quota suitable for this artifact. Gen-1 therefore enforces the narrow
durable bound `MAX_ACTIVE_REBASELINE_SNAPSHOTS_PER_OWNER_LIBRARY = 8`: at most
eight active, unexpired artifacts exist for one owner and one library. The
admission query runs after the existing per-library namespace guard is acquired
and before the O(total-node-count) materialization, in the same guarded
`READ COMMITTED` transaction. The creation transaction deliberately uses
`READ COMMITTED`: PostgreSQL can establish a `REPEATABLE READ` snapshot while
the advisory-lock statement waits, which would let queued creators count stale
committed artifacts. After the guard is acquired, all cooperative namespace
mutations and creators are serialized, so the head, admission count, and
materialized projection remain one guarded cut. It is a transport-neutral
metadata rule, not handler-local SQL, an in-process semaphore, or a global lock.

An artifact is active exactly when `observed_at < expires_at`; equality is
expired and can re-admit a new artifact. Expired rows are not physically
deleted by this phase and do not count against the bound. Reaching the bound
returns the typed admission failure, which HTTP maps to the canonical `429
Too Many Requests` / `rate_limited` response with `Retry-After: 1`. A rejected
create commits no header, entry, checkpoint, acknowledgment, or journal
change. There is no idempotency or retry system: a lost successful response may
leave an artifact behind for a later duplicate POST, but the active durable
bound remains enforced per owner/library.

Direct PostgreSQL/HTTP evidence covers the below-limit concurrent path, the
`N-1` boundary race, owner/library isolation, exact-expiry re-admission,
rejected-create atomicity, authorized-owner expiry as HTTP `410`, foreign-owner
expired concealment as `404`, and the existing `device_revoked` denial across
descriptor, page, and create operations.

### Prompt 85 durable rebaseline checkpoint handoff

Prompt 85 completes the local `AppliedPendingHandoff` state left by Prompt 84.
The one server operation is `DeviceSyncService::complete_rebaseline_handoff`;
its HTTP surface is:

```text
POST /api/v1/rebaseline-snapshots/{snapshot_id}/handoff
```

The route accepts only an authenticated device bearer. The body is empty (the
canonical client may send `{}`), and owner, Library, epoch, and sequence are
never request fields. The bearer supplies the authenticated owner and device;
the server reads the owner-scoped immutable handoff proof to derive Library
and `C = (journal_epoch, snapshot_resume_sequence)`. A physically present
expired payload is still valid for handoff while that proof remains; a missing
or foreign proof is
`404 not_found`. The handoff transaction never reads snapshot entries, changes
the snapshot, changes journal retention, or uses the ordinary feed ACK path.

The server locks the canonical device checkpoint and applies only the monotone
transition required by the proof: a missing or older checkpoint advances to
`C`, an exact `C` is idempotent, a same-epoch checkpoint already beyond `C`
returns `409 checkpoint_conflict`, and a newer or incompatible epoch also
returns the conflict. The library epoch and boundary are validated against
the live library; invalid persisted proof state fails closed. Prompt 85 itself
added no server migration; Prompt 86 later moved the authority from the large
payload header to the small migration-36 proof. Idempotency remains proof and
checkpoint equality.

The client transport exposes only
`complete_rebaseline_handoff(snapshot_id) -> confirmation`; it has no arbitrary
checkpoint assignment or boundary argument. The HTTP adapter strictly decodes
the snapshot ID, Library ID, and decimal `{epoch, sequence}` response. The
engine accepts the response only when all values exactly equal its local
pending marker. A mismatch, authentication/revocation error, conflict,
not-found result, or transport/protocol failure leaves the marker intact.
After a matching server confirmation, one short SQLite transaction verifies
the marker, sets the existing `replicas.journal_epoch`,
`applied_sequence`, and `acknowledged_sequence` to `C`, sets the lifecycle to
incremental-ready/idle, and deletes the marker. It performs no network work;
outbound intents, base observations, staged uploads, and local content are
untouched. Inbound processing remains fenced until this transaction commits,
so a server success by itself cannot un-fence the old cursor.

The crash proof covers the seven meaningful boundaries: before the request,
after server commit before the response, after response receipt, after server
success before local finalization, SQLite rollback before commit, after local
commit before process exit, and restart/retry. Every retry is safe: a lost
response repeats the header-derived idempotent server operation, while a
committed local finalization returns `AlreadyComplete` without recreating the
candidate or snapshot. The request does not hold a SQLite transaction or local
writer lock across the network, so an outbound intent inserted during the
handoff survives byte-for-byte and is submitted only after the boundary.
Concurrent same-snapshot callers converge; different devices keep independent
checkpoints; other Libraries and owners remain isolated. Prompt 85 adds no
retention/compaction, physical cleanup, conflict policy, retry daemon,
polling loop, background scheduler, UI, or deployment work.

### Prompt 86 journal retention and bounded physical cleanup

Prompt 86 implements the first physical journal/payload retention foundation
as transport-neutral one-shot `SyncRetentionService` calls. The existing
`libraries.minimum_retained_sequence` is now precisely the highest sequence
physically compacted through in the current epoch. `cursor < floor` returns the
existing rebaseline-required/history-unavailable condition; `cursor == floor`
continues safely with events strictly greater than the floor. An absent cursor
is therefore stale after non-zero compaction even when the journal is empty.
Ordinary compaction never changes epoch, head, or any device checkpoint.

The Gen-1 incremental-history minimum is 30 days. One journal call loads and
deletes at most 10,000 oldest rows (hard maximum 100,000) from one contiguous
eligible prefix. Sequence is authoritative: if an early row is newer than the
age cutoff, later rows remain even when their timestamps are older. Row deletion
and floor advancement commit together. The lock order is namespace advisory
guard, Library clock/floor row, proof observation, then journal rows. A feed
holds the Library share lock for its short metadata transaction, so concurrent
cleanup yields either the complete old feed or rebaseline under the new floor;
append cannot be included in a precomputed deletion target.

Migration 36 adds an immutable `rebaseline_snapshot_handoff_proofs` row and
backfills every existing durable snapshot. New snapshot creation writes its
header, entries, and proof atomically. The proof preserves snapshot ID, owner,
Library, epoch/boundary, snapshot timestamps, and a deadline 30 days after
payload expiry; it contains no Object/replica/storage/path/credential data and
has no cascading reference to the payload header.

Payload remains readable only while `observed_at < expires_at`. At equality it
may be removed in batches of at most 32 artifacts, but only if its proof exists;
a missing proof stops cleanup. Header and entries disappear atomically. Owner
descriptor/page lookup remains canonical `SnapshotExpired` while the proof is
retained, foreign lookup remains concealed as `NotFound`, and checkpoint
handoff continues to install exactly the proved boundary after payload removal.
Page header/entry reads share one short repeatable-read snapshot, so a cleanup
race cannot return a torn successful page.

Every retained current-epoch proof pins journal cleanup at its boundary:
`compacted_through <= C`; the smallest compatible boundary wins. Device
checkpoints do not provide this pin and therefore cannot retain history forever.
At/after `proof_expires_at`, a proof whose payload is already absent may be
removed in batches of at most 128 (hard maximum 4,096). Handoff then becomes
`NotFound`, and the next journal call may advance. Prompt 87 owns client
convergence from a stale cursor or missing proof. Prompt 86 adds no endpoint,
daemon, timer, scheduler, retry/backoff, UI, conflict policy, or automatic
client recovery.

### Prompt 87 retained-cursor invalidation and automatic rebaseline convergence

`RebaselineConvergenceCoordinator::run_convergence_once` is a callable,
transport-neutral, bounded client state machine. It has no daemon, timer,
poller, recursive call, retry/backoff, cleanup action, or conflict policy. One
invocation inspects durable SQLite state in this fixed precedence:

1. a real persisted snapshot candidate resumes its same finite page sequence;
2. an `AppliedPendingHandoff` attempts the existing header-derived Prompt 85
   handoff; then
3. normal incremental sync performs at most one bounded operation.

Only a server-authoritative `RebaselineRequired` result (retained history/no
cursor after a nonzero floor, or incompatible epoch) starts snapshot recovery.
For an existing pending handoff, only `NotFound` (missing/concealed proof for
the authenticated local snapshot) and typed checkpoint conflict authorize a
replacement snapshot. Authentication/revocation, permission denial, 429,
server/internal, TLS/DNS/timeout/offline, malformed protocol data, local SQLite
failure, and candidate corruption are not rebaseline triggers. They retain
their typed failure and durable state.

Before the one permitted non-idempotent create POST, the coordinator writes an
inert, library-scoped v5 candidate-row claim. A successful server descriptor
atomically promotes that claim to a normal fetching candidate. A create
response loss or malformed response releases the inert claim before returning
the typed failure, so the same invocation never posts again and a later one may
try once. Same-library calls are serialized only by a library-scoped local
guard plus that durable claim; different libraries are independent. No SQLite
write transaction is held across HTTP.

During missing-proof or checkpoint-conflict recovery, H1 remains installed and
therefore inbound remains fenced while S2 pages stage. Candidate activation is
one SQLite transaction: it swaps the authoritative remote base and upserts H1
to H2, then removes the candidate. Readers can see only `base(S1)+H1` or
`base(S2)+H2`, never a marker-free or mixed pair. The coordinator then delegates
the normal Prompt 85 handoff/finalization for S2. It never sends an old local
boundary to the server. A second missing-proof/checkpoint conflict after S2
returns `RecoveryBlocked(DidNotConverge)`; it cannot create S3.

Outbound intents, their IDs, payloads, preconditions/base revisions, source or
upload references, and submission states are outside both activation paths and
remain unchanged. This phase intentionally does not decide how a preserved
intent conflicts with the new remote base; Prompt 88 owns conflict policy.

The restart proof is durable: a persisted descriptor/candidate resumes before
creation; a complete candidate activates; an activated marker retries its
idempotent Prompt 85 handoff; and after local finalization incremental resume is
strictly after the same server-proved boundary. In proof-loss recovery, a
partially staged S2 coexists with H1 after restart; after replacement activation
the next invocation sees H2. Newly created snapshot proofs continue to pin
retention according to Prompt 86 throughout download, activation, and handoff.

## Push mutations

The implemented Prompt 34 push subset is the strict logical route described
above. The richer content/namespace profile specified in the remainder of this
section is a future extension; it is not a license to add arbitrary JSON
patches, file bytes, object identities, or automatic conflict resolution to
the current route.

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

Retention publishes the compacted-through sequence. An offline device does not
pin the journal forever; when its checkpoint falls below the floor it receives
the canonical rebaseline-required result while its outbound mutation queue is
preserved. A retained durable-snapshot handoff proof does pin temporarily,
because the client may already have atomically applied that snapshot.

Each explicit one-shot retention call locks/checks epoch and head, caps its
target at the oldest compatible handoff proof, deletes only a bounded prefix,
and advances `minimum_retained_sequence` atomically with progress. A cursor at
the boundary is valid; a lower cursor is stale. Event pruning never deletes mutation
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

LOCKED DECISION OD-SYNC-002: Gen-1 journal retention service level
Owner: Sync / Operations / Product
Decision: 30-day minimum age horizon, bounded contiguous prefix deletion, no device-checkpoint pin, and a temporary cap at the oldest compatible durable snapshot handoff proof. See ADR-030. A future capacity policy may supersede this minimum but cannot silently weaken the no-gap proof invariant.
Decision evidence: Prompt 86 large-journal, stale-cursor, snapshot-proof, rollback, restart, and concurrency suites on PostgreSQL 17

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

## Implemented desktop inbound apply boundary (Prompt 36)

`synveil-client-sync` is the first reusable desktop-side synchronization core.
It is inbound-only and has no dependency on Axum handlers. A transport adapter
implements `SyncRemote` for checkpoint reads, bounded feed pages, signed
acknowledgements, rebaseline start/page/completion, and logical current-content
downloads. Deterministic tests retain the same checkpoint/completion semantics;
Prompt 37 adds the production device-authenticated HTTP adapter described below.

The engine advances one bounded page or local bootstrap batch per call. Feed
ordering is durable:

1. validate and persist the page plus its events;
2. prepare a typed local operation before a filesystem action;
3. stage/perform and durably receipt the filesystem result;
4. atomically persist the Node mapping, event evidence, and
   `applied_sequence`;
5. persist opaque `ACK_PENDING` evidence;
6. acknowledge the server;
7. persist the confirmed `acknowledged_sequence` and clear the page.

SQLite enforces `acknowledged_sequence <= applied_sequence`. A process crash
after local commit retries the exact redacted acknowledgement evidence rather
than refetching or blindly applying the page. A response lost after the server
acknowledges is safe because acknowledgement is idempotent. Unknown epoch,
scope, schema, event kind, backwards sequence, or sequence gap fails closed.

Bootstrap pages are persisted as a desired manifest, not held only in memory.
The terminal manifest is accepted only when its exact declared count, single
root, parent-directory closure, and reachability are proven. Nodes are applied
parent-first. Only previously tracked clean Nodes absent from that completed
generation are quarantined during sweep; unknown local objects are never
swept. Server completion is retried from durable completion evidence and the
local handoff sets both sequence values exactly to the returned snapshot cut.

`NodeId` is local identity. Relative paths, parent identity, revision, current
version, expected content length/hash, bootstrap generation, presence, and
quarantine location are durable projections. Directory rename/move updates
denormalized descendant paths with one set-based SQLite statement; Unicode
offsets are computed by SQLite character length, not Rust byte length.

Portable logical segments are materialized exactly. The conservative common
Windows/Linux policy rejects separators, controls, Windows-illegal characters,
trailing space/dot, reserved devices, overlong segments, and the `.synveil`
control name. NFKC plus lowercase is used only as a collision key. It never
rewrites a visible name. Managed and unknown case/normalization collisions
become `LOCAL_NAME_COLLISION`.

Before replace, rename, move, Trash, restore, purge, or bootstrap sweep, the
engine verifies the last durable local fingerprint. Unknown destination
occupancy and modified, missing, type-changed, or tree-diverged objects are
durable local blockers. No conflict copy, upload, merge, or last-write-wins
policy runs. Server Trash moves a clean attributed object to controlled
quarantine; current server metadata permits Trash of an empty directory only.
Restore trusts quarantine only after verification and otherwise reconstructs
the canonical file/directory state. Purge removes only safely attributed clean
local state and retains bytes in controlled quarantine in this phase; no
aggressive quarantine cleanup exists.

Current file content is streamed sequentially through a bounded adapter chunk,
written to an operation-specific controlled staging file, hashed while
streaming, checked for declared length and SHA-256, flushed and synchronized,
then exposed. The previous visible file remains until verified bytes are ready.
Unix uses same-filesystem atomic rename plus parent-directory synchronization.
The Windows path closes the staged handle and uses a conservative
operation-specific backup/rename sequence because standard Windows replacement
semantics differ; it is compile-audited here but native Windows runtime evidence
is deferred to the Prompt 40 platform checkpoint.

Deterministic failure points cover page persistence, pre-filesystem action,
post-stage/pre-expose, post-filesystem durable receipt, post-local commit,
post-server acknowledgement, bootstrap page persistence, local bootstrap
completion, and server bootstrap completion response loss. A durable receipt
under `.synveil/staging` distinguishes a Synveil filesystem result from a
racing unknown object. Missing or inconsistent attribution becomes
`LOCAL_RECOVERY_AMBIGUOUS`; exact receipts are removed only after the matching
database operation is committed.

Filesystem watchers, outbound mutation generation, automatic conflict
resolution, desktop UI/pairing UX, service installation, WebSocket/SSE, backup,
and sharing are not part of this boundary. ACL, xattr,
permission, and server-timestamp-to-local-mtime synchronization are explicitly
deferred.

## Implemented remote connection and credential lifecycle (Prompt 37)

Profiles are immutable HTTPS origin-root configurations, identified by local
UUIDv7 `ServerProfileId`. The adapter appends closed `/api/v1/...` route segments
using URL operations, not caller-provided paths. Userinfo, query, fragment,
subpath, malformed port, and parser repairs fail closed. Numeric loopback HTTP
is an explicit test-only construction policy. TLS certificate validation is
mandatory, every redirect is rejected, no cookie jar or ambient proxy is used,
and transparent decompression cannot change logical file length/hash semantics.
No stable server installation identity is currently exposed; verified
origin/TLS is the binding, not an inferred hostname or LibraryId.

An authenticated browser owner creates a bounded, short-lived one-time grant
with existing CSRF protection. Desktop exchanges that grant exactly once and
receives an owner/Device/credential identity plus a redacted bearer secret. The
server persists only domain-separated digests. Exchange is deliberately not
automatically retried: a committed exchange with a lost response requires the
owner to revoke the unknown/issued credential and create a fresh grant. A
lost desktop process before secure storage completes likewise needs explicit
recovery; no enrollment token or bearer is saved in SQLite.

The desktop lifecycle is:

1. persist non-secret `ServerProfile` configuration;
2. exchange a one-time grant over verified HTTPS;
3. pass the opaque profile/origin-bound exchange receipt to `store_enrollment`
   (or explicit `replace_enrollment`), record a non-secret cleanup intent, store
   and read back the bearer through
   `PlatformRuntime::SecretStore`, then commit enrollment identity metadata;
4. load a profile-bound `LoadedDeviceCredential` after restart and construct
   `HttpSyncRemote` from that profile, Device, credential and bounded config;
5. initialize/open a V2 managed root with the same profile and run the existing
   inbound bootstrap/feed/apply/ack/download engine.

No public raw bearer-import API can relabel Server A's exchange as Server B's
enrollment. The SecretStore key uses profile ID plus credential ID rather than
URL aliases. Its bounded versioned envelope additionally binds canonical origin,
transport policy, profile, owner, Device, and credential ID inside secure
storage. A copied/reconstructed SQLite database with identical IDs but another
origin cannot load, overwrite, or delete that secure entry. The HTTP constructor
also compares the loaded secure origin against the requested profile before
creating any bearer header. Malformed, unknown-version, oversized, or old raw
secret entries are rejected without fallback.
Linux uses persistent native Secret Service and Windows uses Credential
Manager; the selected native builder never falls back to the crate's mock
backend. Missing/locked/unavailable secure storage fails closed. Synthetic
in-memory storage exists only in tests. Native Linux persistence was exercised
with an isolated test vault; no native Windows credential-store/TLS execution
is claimed by cross-target compilation. macOS persistence remains unsupported.

SQLite migration V2 leaves V1 schema/checksums untouched. It persists only
profiles, owner/Device/credential IDs, enrollment/forget timestamps and secure
entry cleanup intents. Both the replica row and physical V2 root marker bind
one explicit profile. A different profile, even with the same LibraryId,
cannot open that replica. Legacy unbound replicas cannot silently become
production replicas; automatic rebind is not implemented.

Local forget first writes a durable disconnected marker, then deletes the
secure entry, retaining the owner/Device binding for later explicit
re-enrollment. Failure leaves retryable cleanup metadata and does not reload
the old secret after restart. Replacement accepts only the same owner/Device,
stages/read-verifies the new secret, commits its new credential ID, and retires
the old key through durable cleanup. The engine checks the active credential
generation before each call, so an existing engine stops after local forget or
replacement. Callers must discard direct HTTP objects already holding the old
secret. Forget does not imply an offline server revoke.

Device bearer authority is limited to checkpoint/feed/ack, rebaseline
start/page/complete and logical read/download operations required by inbound
apply. Each route enforces the authenticated Device ID and owned Library.
Browser-cookie writes still require CSRF; only an authenticated device bearer
receives the inbound-route exemption. Device bearer cannot submit Prompt 34
mutations, inspect/resolve Prompt 35 conflicts, or use browser administration.
Server Device/credential revocation is checked again on the next request.

HTTP errors retain authentication, revocation, checkpoint/rebaseline/evidence,
rate-limit, dependency/internal, protocol, offline, timeout, TLS, body-limit,
and redirect distinctions. Health uses the existing readiness endpoint and
authenticated checkpoint validation, rejecting HTML or incompatible payloads.
Metadata bodies, download bytes and stream chunks are bounded; timeouts are
finite. The adapter performs no automatic retry. The engine may retry the same
durable acknowledgement/completion proof after interruption. An authentication,
revocation or offline error preserves local files, applied/acknowledged
sequences, and pending evidence; it never wipes or rebinds the replica.

## Deterministic outbound conflicts (Prompt 88)

A conflict means the canonical server precondition rejected a durable local
intent, or an activated authoritative rebaseline snapshot proves that the same
exact precondition must fail. The remote base stays canonical; the original
intent, base revision, payload metadata, and any staged content source stay
unchanged. Timeouts, authentication/authorization failures, rate limits,
dependency failures, and malformed responses are not conflicts.

SQLite schema V6 stores one client-local conflict record per outbound intent,
including its kind, initial safe remote evidence, original local base, status,
resolution audit fields, and optional replacement-intent link. It never stores
content bytes, credentials, cookies, server object keys, or physical paths.
Initial evidence is immutable even if later inbound changes advance the remote
base. Listing is library-scoped and keyset-paged by `(detected_at, conflict_id)`
with a default of 100 and hard maximum of 1,000.

The current outbound queue has no durable causal-dependency graph. Therefore
one unresolved conflict conservatively pauses outbound submission for that
library, including otherwise independent later intents; other libraries remain
independent. Inbound synchronization and ACK continue. For a content conflict,
the inbound engine may expose newer canonical remote bytes only after verifying
the local conflicting bytes already exist in the durable staging area with the
recorded length and SHA-256.

Resolution is explicit and local-only. `AcceptRemote` atomically resolves the
record and cancels the old intent without calling or changing the server or
synchronously deleting staged content. `RetryLocalAgainstCurrentBase` validates
that the operation still applies and that required staged bytes exist, then
atomically supersedes the immutable old intent and creates exactly one linked
new intent using the current local canonical node/parent revisions. A repeated
resolution returns the same result. The ordinary outbound engine submits that
new intent later, so another server change may produce a new conflict.
`KeepBoth`, generic merge, conflict-copy naming, force overwrite, background
resolution, and automatic winner selection remain unsupported.

Rebaseline, checkpoint handoff, and conflict are separate mechanisms. Snapshot
activation can add a conflict only when an exact node or parent precondition is
provably stale; it never resolves one or rewrites an intent. Conflict state does
not trigger rebaseline and does not pin journal retention, snapshot payloads,
handoff proofs, or device checkpoints.

## Bounded bidirectional cycle (Prompt 91)

`BidirectionalSyncCycleRunner::run_once(observed_at)` is the canonical
transport-neutral one-shot composition for one library. It performs, in order:

1. inspect durable local state;
2. call `RebaselineConvergenceCoordinator::run_convergence_once()` once; and
3. after a fresh local eligibility check, call
   `OutboundSubmissionEngine::process_next_ready_intent()` at most once.

The convergence coordinator remains the only owner of ordinary inbound
processing and Prompt 87 retained-history/proof-loss recovery. The outbound
engine remains the only owner of durable intent selection, mutation/upload
execution, idempotency, reconciliation, and the Prompt 88 conflict fence.
Prompt 91 does not duplicate either state machine and does not create a cycle
journal.

Outbound is eligible only after `IncrementalReady`, safe bounded incremental
progress, or `RebaselineConverged`, and only when the second local inspection
finds no incomplete candidate, pending handoff, bootstrap, pending inbound
page/ACK, local issue, or missing root. Candidate and handoff precedence
therefore wins over unrelated outbound work. Idle inbound still permits one
outbound opportunity. An unresolved Prompt 88 conflict does not stop inbound;
the outbound engine returns `BlockedByConflict` without submitting a mutation.

The phase result is typed as `SyncCycleResult { inbound, outbound }`. It
retains the underlying convergence and outbound results and exposes whether
the cycle made durable progress, is idle, may have more work, requires conflict
resolution, or requires authentication. Authentication, transport failure,
rate limiting, and `RecoveryBlocked` skip outbound. Unexpected SQLite,
protocol, invariant, or impossible-state failures remain `Err`.

Ordinary inbound application keeps the existing conservative overlap rule. If
an active outbound intent is affected by an ordinary remote page, the inbound
engine preserves the intent as `NEEDS_REBASE_VALIDATION` and records the typed
`BASE_STATE_CHANGED` observation issue before ACK. The cycle reports the unsafe
inbound result and does not invent a second Prompt 88 classifier or overwrite
the local work. Canonical Prompt 88 conflicts continue to be produced by the
existing server-precondition path or exact Prompt 87 rebaseline
classification.

One ordinary cycle is bounded to one inbound feed page and one outbound
intent/submission unit. Prompt 87 may perform its own finite rebaseline
download and at most one new snapshot creation when recovery is required. The
cycle adds no outer loop, recursion, retry, backoff, sleep, polling, full-feed
drain, or full-queue drain. It has no scheduler or daemon behavior; the Prompt
92 runtime calls it and owns lifecycle policy separately.

The runner is restart-stateless. Durable page/cursor, candidate, handoff,
intent, upload, idempotency, reconciliation, and conflict records remain the
source of truth if a caller exits between phases or loses a response. Same-
library callers reuse the existing per-library convergence and replica-writer
guards; different libraries remain independently progressable. HTTP is kept
outside long SQLite writer transactions.

## Long-running sync runtime (Prompt 92)

`SyncRuntime` repeats the Prompt 91 bounded cycle, but does not become a second
sync state machine. Every execution is delegated through the
`SyncCycleExecutor` port implemented by `BidirectionalSyncCycleRunner`; the
runtime never calls an inbound engine, outbound engine, checkpoint, snapshot,
or handoff API directly and never mutates durable synchronization state.

Registration is explicit. The embedding caller supplies one authenticated,
transport-ready Prompt 91 runner per Library and may later unregister it
without deleting local sync data. This is deliberate because a durable
`replicas` row cannot safely reconstruct a filesystem root, profile, secure
credential, remote adapter, and runner by itself. Registration while stopped
is retained for the next start; registration while running schedules the
Library immediately; duplicate registration leaves the original runner
untouched.

`start()` creates one supervisor and schedules every registered Library once.
`request_shutdown()` prevents new cycles. `stop()`/`shutdown()` request the
same graceful stop and `join()` waits for the supervisor. Active bounded
Prompt 91 calls finish before join returns; no critical lower-level future is
aborted. Repeated shutdown/join calls are safe. Restart has no runtime repair
step: Prompt 87/88 durable cursor, candidate, handoff, intent, upload,
idempotency, reconciliation, and conflict state remains authoritative.

The wake surface accepts `Startup`, `LocalChange`, `Manual`, `Periodic`,
`NetworkAvailable`, `CredentialChanged`, and `PreviousProgress`. There is one
coalesced pending reason per Library. Wakes during an active cycle do not
start a concurrent call; they create a follow-up opportunity. Idle local or
manual wakes interrupt the idle poll. Manual, network-available, and
credential-change wakes may bypass transient backoff. Manual and credential
wakes resume authentication suspension. No wake bypasses Prompt 91/88
authentication, conflict, recovery, or outbound preconditions. With the
current Prompt 91 result contract, rate limiting has no retry-after duration,
so the runtime uses a validated 30-second fallback.

The default idle safety poll is 30 seconds and the accepted range is one
second to one hour. Productive work receives a deterministic 1 ms fair
follow-up rather than an unbounded same-tick drain. The supervisor uses a
round-robin active set, runs at most one cycle per Library, and defaults to
four concurrent Libraries with a validated hard maximum of 1,024. A transient
or recovery-blocked result backs off at 1, 2, 4, 8, 16, 32, then 60 seconds;
there is no jitter or immediate retry loop. Auth-blocked Libraries do not
periodically hammer the server. A conflict-blocked Library continues inbound
safety polling while Prompt 88 keeps outbound fenced. Local fault or panic
state is isolated to that Library and does not stop its peers.

Status and event values are bounded categories and relative durations only.
They contain no credentials, cookies, tokens, absolute paths, raw names,
content, opaque evidence, or conflict payloads. The runtime intentionally
adds no scheduler migration/table, systemd or Windows Service integration,
launchd/autostart, watcher wiring, SSE/WebSocket, broker, server push, route,
frontend persistence, or OS network monitor. Those are separate lifecycle or
producer work.

## Durable-change-first runtime signal integration (Prompt 93)

Prompt 93 wires the existing local producers to the Prompt 92 runtime without
changing Prompt 91 synchronization semantics. The canonical flow is:

```text
filesystem/controller/platform event
  -> classify or validate the event
  -> commit durable local state
  -> release the Library writer/transaction boundary
  -> send one best-effort runtime wake
  -> Prompt 92 schedules a bounded Prompt 91 cycle
```

`OutboundIntentProducer` returns a separate durable result and wake result.
`Committed` means an intent was inserted or an active intent was coalesced;
an exact semantic duplicate is `NoChange`. A `RuntimeStopped` or
`UnknownLibrary` wake status is not a durable failure and never rolls back the
intent. The notifier receives only a Library ID and a closed wake reason, so
paths, content, credentials, cookies, tokens, checkpoints, and conflict
evidence do not cross into the runtime.

The observer uses the same ordering for create, modify, rename, move, delete,
and restore-related local observations. One bounded poll/reconciliation
operation aggregates its changed intents and emits at most one `LocalChange`
wake for the Library. A rescan that spans multiple bounded units retains the
pending change bit and emits once when reconciliation completes. Overflow
detection, authoritative reinspection, rename attribution, and durable
self-generated suppression are unchanged. No-op observations, ignored
`.synveil/` control paths, and suppressed operation results do not wake.

Credential storage remains the existing profile-bound SecretStore lifecycle.
`CredentialChanged` is emitted only after a usable enrollment or replacement
has passed secure-store verification and the enrollment metadata transaction.
Failed validation/persistence emits nothing, and removal/logout does not emit a
synthetic wake. The affected Library list is explicit and deduplicated because
the current client registration owns the mapping.

`network_available()` is a controller/platform hint for registered Libraries.
It may release transient backoff but does not bypass authentication, conflict
fencing, rebaseline recovery, or outbound preconditions. `sync_now(library_id)`
is also a scheduling request only: it goes through Prompt 92 to Prompt 91,
returns `Queued`, `Coalesced`, or another bounded status, and does not claim
completion or synchronously drain work. A manual wake during an active cycle is
stored as one follow-up opportunity.

All runtime and notifier clones address the same supervisor. A registration or
unregistration race may make an immediate wake unavailable, but it cannot
delete a durable intent. If a process crashes after commit and before wake, or
an attached notifier intentionally drops the signal, startup and the periodic
safety poll recover the work. There is no persistent wake queue, runtime
migration, OS-specific network monitor, service integration, new watcher,
server push channel, or frontend change in this phase.

## Desktop sync host and process lifecycle composition (Prompt 94)

`DesktopSyncHost` is the application-level composition root for Prompts 91–93.
It owns one `SyncRuntime` for the supported owner/Device process context and
keeps explicit references to the existing durable Library/replica registry,
Prompt 91 runners, optional observers, the profile-bound credential provider,
and the narrow controller handle. It is not a new synchronization state
machine, and host lifecycle is not persisted in SQLite or PostgreSQL.

The first registered Library establishes that owner/Device context. Subsequent
registrations must match it or receive typed `WrongScope`; one host never
silently combines multiple account/device credential domains.

The host is configured in two stages. Construction opens or receives the
existing `LocalStateStore`, validates each managed-root/profile binding,
constructs the Prompt 91 graph (`InboundSyncEngine` ->
`RebaselineConvergenceCoordinator` plus `OutboundSubmissionEngine` ->
`BidirectionalSyncCycleRunner`), and registers each Library exactly once with
one runtime. It does not start background work. An HTTP Library may be
constructed without an enrollment/usable secret; the host loads credentials
only through the existing profile-bound `SecretStore` when a cycle is run.
With the current immutable `HttpSyncRemote`, a verified credential ID change
rebuilds only the affected Library's authenticated runner graph. Secret bytes
never enter runtime status, events, or controller handles.

Startup starts the single runtime before enabling any configured filesystem
observer. This makes all observer-generated `LocalChange` wakes target a
registered runtime. A missed wake cannot lose work: the durable intent is
committed first and Prompt 92 startup/periodic polling remains the recovery
path. Dynamic registration while running uses the same runtime and starts that
Library's observer only after registration; unregistration removes only the
ephemeral runtime entry and does not delete sync data.

The handle exposes bounded status/events, `sync_now`, `network_available`,
credential-change scheduling, and existing Prompt 93 producer/controller
surfaces. Manual and network calls are hints and cannot bypass Prompt 91/88
authentication, rebaseline, conflict, or outbound-precondition gates. Linux
and Windows lifecycle/network adapters have the same semantics: an embedding
process may deliver shutdown or a positive network-available hint. No adapter
contains a feed loop, retry/conflict policy, checkpoint write, OS network
monitor, service manager, autostart, or UI.

Graceful shutdown marks the host stopping, cancels observer polling, flushes
and marks observer reconciliation, requests Prompt 92 shutdown, allows active
bounded Prompt 91 calls to finish, joins runtime tasks, and closes only
host-owned state. Repeated shutdown/join is safe; a stopped host is terminal.
Process restart creates a new host and re-registers the same durable local
Libraries, allowing pending intents, candidates, handoffs, and conflicts to
resume under their existing owners. `Drop` only requests best-effort
cancellation; the application must explicitly await shutdown/join. ADR-036
locks this composition boundary. The readiness string for the complete gate is
`SYNVEIL_DESKTOP_SYNC_HOST_READY`, but it is not a runtime health claim by
itself.

## Root-loss fencing and production process behavior (Prompt 95)

The foreground `synveil-client` process supplies one `DesktopSyncHost` and one
`SyncRuntime`. Linux `SIGINT`/`SIGTERM` and Windows Ctrl-C enter the same
graceful shutdown path; a best-effort network adapter supplies only positive
`network_available()` hints, with a bounded periodic fallback when native
inspection is unavailable. These adapters never invoke a sync engine directly.

Root absence has a distinct per-library lifecycle:

1. At bootstrap, an existing root is reopened only if its canonical managed
   marker matches the durable profile/scope/binding. A missing root becomes a
   deferred `Unavailable` replica; bootstrap does not create the path or its
   marker.
2. While `Unavailable`, the observer stops and validates before draining any
   queued OS/manual hint. The runtime marks the library `RootBlocked`, so
   ordinary periodic, network, and manual scheduling attempts cannot turn an
   unavailable root into a delete/trash observation.
3. When the configured path reappears, the host checks canonical identity,
   profile, owner/device/library scope, and the original binding. A mismatch
   remains unavailable and is never silently rebound.
4. A valid reappearance moves through `Recovering`, restarts the existing
   watcher once, performs one bounded canonical rescan, and then emits one
   `RootAvailable` wake. Changes found by the rescan become ordinary durable
   observation intents only after the root is valid again.

This fence is applied both at the observer boundary and immediately before a
cycle. The root lifecycle task is per library, so a missing removable volume
does not pause a healthy sibling. Root status is ephemeral and is not a
checkpoint, feed cursor, journal event, SQLite row, or user-facing deletion
fact. Existing Prompt 91 intent, conflict, rebaseline, and upload safety rules
remain authoritative after the gate opens.

The process uses the existing `state.sqlite3.writer.lock` and does not add a
second process lock or database. A missed wake, process crash, or graceful
restart leaves durable work for startup and periodic polling to recover. No
setup-secret, distributed limiter, service/autostart, tray/UI, or server push
channel is introduced by this phase.

## Local control commands and events (Prompt 96)

The `synveil-client` IPC server calls the existing `DesktopSyncHostHandle`; it
does not call `BidirectionalSyncCycleRunner`, `OutboundSubmissionEngine`,
SQLite, or a credential provider directly. `SyncNow` therefore follows the
same Prompt 93/92 wake path as an embedding controller and can return only
`Queued`, `Coalesced`, `AlreadyRunningFollowupRecorded`, `UnknownLibrary`, or
`RuntimeStopped` through the safe protocol. The success response is a
scheduling decision, not a sync result.

`GetProcessStatus`, `ListLibraries`, and `GetLibraryStatus` are non-destructive
projections. Root state comes from the host's existing `Available`,
`Unavailable`, or `Recovering` value. Runtime phases and last outcomes are
translated into bounded categories; no new mirror of sync correctness state is
maintained. Authentication is shown as `Blocked` only when the runtime
observed the canonical auth-blocked outcome, and as `Ready` only for a safe
successful authenticated outcome; otherwise it remains `Unknown`.

`SubscribeEvents` consumes the existing runtime event stream plus process/root
signals and translates them into invalidations such as `LibraryStatusChanged`,
`RootAvailabilityChanged`, and `SyncCycleCompleted`. The broadcast is bounded
and best-effort. A lagging subscriber receives `Lagged` and must reconnect or
refetch status; event loss cannot change durable intent ordering, conflict
fences, root fencing, or cycle correctness.

## IPC-backed desktop controller model (Prompt 97)

The future native UI uses one `DesktopController` per profile/process context.
The controller uses only the Prompt 96 `DesktopControlClient`; it never reads
the synchronization SQLite database, probes a root, loads a `SecretStore`, or
links to `DesktopSyncHost`, `SyncRuntime`, or Prompt 91 engines. The endpoint is
resolved through the existing Prompt 96/platform path so Linux UDS and Windows
named-pipe differences remain below the model boundary.

Startup performs a fresh v1 handshake, subscribes to bounded events, fetches
`GetProcessStatus` and `ListLibraries`, fetches each listed library status, and
publishes one coherent snapshot. The model maps only the safe process state and
the existing runtime/root/auth/conflict categories. Raw roots and all
credential/transport material remain absent. A `watch` receiver exposes the
newest snapshot without retaining a callback queue or blocking a slow UI.

Events are best-effort invalidation. A single pending-refresh bit and one
event-reader task coalesce a burst, permit only one refresh in flight, and keep
one follow-up opportunity when a signal arrives during a refresh. A refresh
failure caused by a dropped connection marks the retained snapshot stale and
enters reconnecting; it does not fabricate fresh status. Reconnect uses the
bounded 250 ms, 500 ms, 1 s, 2 s, 4 s, 5 s schedule, resets after success, and
creates a new connection generation. Old replies and old event readers are
fenced by that generation.

`sync_now(library_id)` is admitted through a bounded controller command path
and sends exactly one Prompt 96 `SyncNow`. `Accepted`, `Coalesced`, and
`AlreadyRunningFollowupRecorded` still mean schedule only. Disconnected calls
return a bounded disconnected/unavailable result; an admitted request whose
response is lost returns `OutcomeUnknown` and is never resent. The same
no-replay rule applies to `RequestShutdown`. Only that explicit command asks
the process lifecycle to stop. `DesktopController::stop()` closes and joins
controller-owned IPC/event work only, so closing the future UI leaves
`synveil-client` and its sync runtime alive. This application boundary adds no
durable state, migration, route, GUI, tray, service, or autostart behavior.

## Native Qt 6/QML shell sync boundary (Prompt 98)

The native shell is a presentation process, not a second synchronization
engine. Its one Rust `DesktopUiBridge` owns one Prompt 97
`DesktopController`; the main window and tray use that same controller and
latest snapshot:

```text
QML or tray Sync Now
        -> DesktopUiBridge
           -> DesktopController::sync_now
              -> Prompt 96 local control IPC
                 -> synveil-client / DesktopSyncHost / SyncRuntime
```

The Qt GUI thread never performs a blocking connect, read, reconnect, or
command wait. Controller work and refreshes stay on the controller's async
runtime. Snapshot delivery is latest-state and bounded; a complete snapshot is
mapped atomically, while a retained list is marked `Stale` until a fresh
generation is received. Old controller generations cannot overwrite newer
state, and a removed library cannot remain selected.

The shell displays only the controller's safe categories for process,
connection, freshness, root, authentication, conflict, runtime, and
scheduling. It does not read synchronization SQLite, inspect a filesystem
root, access credentials, or open Prompt 96 transport. It also does not spawn
or autostart `synveil-client`; absence and restart are handled by Prompt 97
reconnect semantics and remain usable as disconnected/reconnecting UI states.

`Sync Now` is admitted through the controller's bounded command path. The UI
may report only generic scheduling feedback: accepted means requested,
coalesced means already running/request recorded, and an unknown response is
not completion. Rapid clicks do not create unbounded tasks. Root unavailable,
authentication blocked/required, conflict attention, stale status, and an
unavailable process keep the action disabled or return bounded safe feedback;
the UI does not bypass those controller gates.

Tray `Open`, `Sync Now`, and `Quit Synveil Desktop` share the same model.
Normal close-to-tray hides the window when a tray is available. Tray Quit and
no-tray fallback stop/join only UI-owned controller work. They never send
Prompt 96 `Shutdown`, terminate `synveil-client`, or alter durable sync
correctness. PostgreSQL remains relevant only to the live process/controller
acceptance path that exercises a real Sync Now request; the Qt shell itself has
no database dependency. The locked boundary is in
[`ADR-040`](../adr/ADR-040-native-qt-desktop-shell.md).

## Production desktop launch and supervision boundary (Prompt 99)

Prompt 99 composes launch management above the existing controller without
moving synchronization ownership into the Qt process:

```text
synveil-desktop / DesktopUiBridge
        -> BackgroundClientManager (typed, bounded process-management API)
           -> Linux systemd --user or Windows per-user Task Scheduler
              -> synveil-client
                 -> DesktopSyncHost / SyncRuntime / Prompt 95 writer lock
```

The manager runs after controller startup and performs one bounded availability
inspection. It may request a start for endpoint absence, a stopped client, or
an inactive user supervisor. It never starts a replacement for endpoint
security, protocol incompatibility, malformed control, writer conflict, or a
terminal controller state. A shared profile-scoped gate returns
`AlreadyStarting` to concurrent callers and a bounded cooldown prevents
reconnect-driven spawn storms. Controller reconnect, generation fencing, and
freshness remain Prompt 97 responsibilities.

The manager's public results are the finite categories
`AlreadyRunning`, `StartRequested`, `StartedSupervised`, `StartedDirect`,
`AlreadyStarting`, `NotInstalled`, `SupervisorUnavailable`, `LaunchDenied`,
`UnsafeState`, and `Failed`. No result carries stderr, a PID, a path, a URL,
credentials, or a token. The Qt projection exposes only a generic launch label.

Linux uses the packaged `/usr/lib/systemd/user/synveil-client.service` with
`Type=simple`, exact `ExecStart=/usr/bin/synveil-client`, `Restart=on-failure`,
bounded `RestartSec` and `StartLimit*`, plus source-derived
`RestartPreventExitStatus=78`. It has no `network-online.target` dependency.
User login autostart is explicit `systemctl --user enable`; install hooks and
GUI startup do not silently enable it, and explicit disablement is respected.
The unit is never a system/root service.

Windows uses a current-user, least-privilege Task Scheduler definition with a
logon trigger, exact canonical sibling `synveil-client.exe`, `IgnoreNew`, and
finite restart settings. Registration uses fixed `System32\\schtasks.exe`
argv and no stored password, SYSTEM principal, administrator requirement, or
shell interpolation. The existing Prompt 95 writer lock remains the final
same-profile ownership protection.

The Linux package manifest now includes the client, desktop, user unit, normal
desktop entry/icon, existing maintenance payload, and license/notices. The
Windows packager creates an unsigned portable ZIP with both executables, the
real Qt platform/runtime/QML closure, C++ runtime files when required,
`qt.conf`, and notices. It rejects SDK/development files, Linux libraries,
developer paths, and missing non-system PE imports. Autostart metadata is
process-management state only: Prompt 99 adds zero synchronization records,
client/server migrations, HTTP routes, OpenAPI operations, or web behavior.

The locked decision is recorded in
[`ADR-041`](../adr/ADR-041-production-desktop-launch-orchestration.md). The
implementation and operating procedures are in
[`DESKTOP_LAUNCH.md`](DESKTOP_LAUNCH.md).

## Secure enrollment and credential lifecycle (Prompt 101)

Desktop authentication is a one-time enrollment-grant exchange owned by the
existing HTTP client in `synveil-client`. The GUI path is
`QML -> DesktopUiBridge -> DesktopController -> Prompt 96 IPC ->
DesktopSyncHostHandle -> HttpEnrollmentClient -> existing exchange`; QML and
the desktop bridge never open HTTP, SQLite, or `SecretStore` and never receive
the returned bearer secret.

The enrollment field is masked, transient, and bounded to 69 encoded bytes. It
is cleared after dispatch and is absent from snapshots, events, tray labels,
clipboard operations, and logs. The controller rejects invalid input before
IPC and admits one authentication operation at a time. The bounded local
command path adds `Authenticate` and `SignOut` with finite category results;
it does not add retry tasks.

The successful path is durable-first: exchange once; validate the receipt's
profile/owner/device; persist and read back the existing profile enrollment and
secure-store value; clean superseded secret material; then emit
`CredentialChanged`; then let the runtime reload and publish status. A failed
write, readback, cleanup, or validation suppresses the success result and the
post-persistence wake. A stopped/coalesced wake does not undo durable state;
startup and periodic status refresh remain the safety fallback.

Sign Out is explicit. It writes the existing forgotten/tombstone metadata and
finishes secure-store deletion before waking the affected HTTP libraries. The
runtime then reloads no credential and reports its existing `AuthBlocked`
behavior. GUI close, tray quit, GUI/client restart, and process shutdown do not
implicitly sign out.

If an admitted auth or Sign Out response is lost, the controller returns
`OutcomeUnknown`, refreshes authoritative status, and never replays the
one-time exchange or removal after reconnect. Revoke/expiry remains the
existing runtime `AuthBlocked` path. Profile isolation filters wake targets and
rejects mismatched receipts. No auth database, route, OpenAPI operation,
schema migration, OAuth/password/API-key scheme, or sync-cycle rewrite is
added. See [`ADR-042`](../adr/ADR-042-secure-desktop-authentication-and-credential-lifecycle.md).

## Desktop profile onboarding and connection correction (Prompt 102)

The desktop can now begin with a process-owned profile ID but no configured
server and no libraries. The native onboarding path stays
`QML -> DesktopUiBridge -> DesktopController -> Prompt 96 IPC -> synveil-client`.
The client uses the existing strict `CanonicalBaseUrl` contract and the
existing anonymous `GET /health/ready` DTO before applying a profile. There is
no GUI-owned HTTP client, profile registry, or durable draft.

Configuration success is durable-first. Exact repeats are idempotent; a label
edit keeps the profile ID and connection timestamp; an origin edit fences the
old profile-bound credential and wakes the runtime only after the new state is
committed. The controller exposes profile metadata separately from connection
freshness and maps lost mutation responses to `OutcomeUnknown` with refresh,
never blind replay. `0007_profile_reconfiguration.sql` is the only new client
migration; no server migration is required. Empty library state remains safe
and is not a root deletion signal. See
[`ADR-043`](../adr/ADR-043-desktop-profile-onboarding-and-connection-configuration.md).

## Production library and local-root onboarding (Prompt 104)

An authenticated profile with zero libraries is a valid state. The supported
first-library path is `QML -> DesktopUiBridge -> DesktopController -> Prompt
96 IPC -> synveil-client`; QML never performs HTTP, SQLite, credential, watcher,
or sync-runtime work. The user supplies only a bounded logical name and a
native-selected folder. The client generates the UUIDv7 library identity and
uses the authenticated canonical `POST /api/v1/libraries` contract to create
the empty server library and root node. The repository has no canonical
attach/import API, so onboarding does not invent one or duplicate an existing
remote library.

The root must be absolute, canonicalizable, non-redirecting, writable, and
outside filesystem root, home, and the process current directory. Prompt 104
allows ordinary pre-existing files and directories when the flow creates the
new remote library. A conflicting or incomplete `.synveil` control tree,
missing path, read-only root, symlink/junction redirect, and other unsafe root
are still rejected. Duplicate and nested/overlapping roots are checked by
canonical path components rather than string prefixes. The local absolute path
never crosses the server boundary or the safe presentation surface.

Onboarding records a pending UUID/root manifest entry before remote mutation,
reconciles an ambiguous create response with one bounded authoritative library
list, commits the existing profiled managed root and `LocalStateStore` binding,
then promotes the active manifest entry before host/runtime/watcher
registration. The authoritative remote root NodeId is durably seeded before
watcher/runtime admission. Existing files are discovered by the bounded
observer as ordinary create intents; directory parents are submitted first,
then nested observations resume after the normal server NodeId assignment.
File bytes use the existing verified staging/upload-session pipeline, and no
bulk uploader or second sync engine is introduced. `OutcomeUnknown` is never
blind replay. The existing SyncRuntime receives only a post-persistence
eligibility/wake signal and performs normal bounded initial/rebaseline work.

An unavailable active root remains fenced and deferred. It is never interpreted
as an empty tree, mass deletion, detach, or sign-out. On restart, active
bindings reconstruct the same host context, while a pending binding provides a
safe recovery identity for an interrupted setup. The locked decision is in
[`ADR-044`](../adr/ADR-044-desktop-library-onboarding-and-local-root-binding.md)
and [`ADR-045`](../adr/ADR-045-existing-root-bootstrap-and-initial-upload-admission.md).

## Essential desktop settings and global user pause (Prompt 105)

Prompt 105 keeps synchronization ownership in the one process-wide
`synveil-client` runtime. A small non-secret setting beside the existing
profile manifest stores only `paused` or `running`; it is not a SQLite row,
server preference, per-library flag, credential, or sync journal. The client
loads it before starting `DesktopSyncHost`, and a malformed setting fails
closed rather than inventing a state.

`PausedByUser` is distinct from auth, network, root, conflict, and runtime
fault categories. At the existing runtime admission boundary it blocks
periodic, manual, local-change, network/inbound, and credential wake paths.
Filesystem/local durable observation may continue to record bounded durable
facts, but no filesystem wake bypasses the pause. A cycle already in flight
finishes through the existing bounded operation; the client does not hard-kill
or abort it. `SyncNow` returns `Paused` and never acts as an implicit resume.

Resume persists `running` before signaling the runtime, clears only the user
pause reason, wakes the existing scheduler, and makes retained eligibility
available without broad rescan or a new worker. Auth/sign-out, profile
configuration, library setup, root status, and local control remain usable
while paused. A failed setting write leaves runtime state unchanged; a lost
IPC response is `OutcomeUnknown` followed by authoritative refresh rather than
replay. The local protocol uses `GetSyncControlState`, `PauseSync`, and
`ResumeSync`, plus a best-effort state-change invalidation event.

Login startup remains process management through `BackgroundClientManager`,
not sync state. Close-to-tray remains a Qt-local `QSettings` value. Neither
setting changes sync correctness or client lifecycle. Server migration remains
**36** and client schema remains **V7** with **7 client migrations**. See
[`ADR-046`](../adr/ADR-046-essential-desktop-settings-and-user-sync-controls.md).

## Production desktop attention projection (Prompt 106)

The desktop attention card is a local projection over the existing durable
client-sync conflict model. It does not add a server conflict API, merge
policy, conflict-copy operation, or SQLite migration. The six canonical kinds
remain `RemoteRevisionChanged`, `RemoteContentChanged`, `RemoteStateChanged`,
`RemoteMissing`, `NameCollision`, and `ParentChangedOrUnavailable`. The only
desktop decisions are `AcceptRemote` and, except for `RemoteMissing` and
`NameCollision`, `RetryLocalAgainstCurrentBase`.

`LocalStateStore` returns exact unresolved counts for conflicts, local-apply
issues, and observation issues, plus a bounded conflict page: 32 items by
default and 128 maximum. Items carry stable IDs, safe managed relative paths,
file/directory kind, known lengths, revision/state metadata, and supported
actions. They carry no bytes, hashes, absolute roots, staging paths,
credentials, or raw server diagnostics. `truncated` is explicit. Resolution
uses the existing durable transaction and generation fence, then wakes the
normal runtime only after persistence. A user pause is preserved. Duplicate,
stale, missing, unsupported, busy, and uncertain outcomes are typed; uncertain
responses trigger refresh and are never replayed. See
[`ADR-047`](../adr/ADR-047-production-desktop-attention-and-conflict-resolution.md)
and [`DESKTOP_CONTROL.md`](DESKTOP_CONTROL.md).

## Recovery and resilience boundary (Prompt 107)

The desktop recovery card does not add a sync engine. `Check again` is the
existing bounded `SyncNow` wake, so runtime backoff, root fencing, auth gating,
and `PausedByUser` remain canonical. A same-path root return is observed by the
existing root lifecycle and wakes normal eligibility; root loss never means an
empty tree or mass deletion. Server-transient conditions stay waiting while
auth/root/local durable blockers can be action-required. Prompt 106 attention
is independent and is not auto-resolved. See
[`ADR-048`](../adr/ADR-048-production-desktop-recovery-and-resilience-ux.md).
