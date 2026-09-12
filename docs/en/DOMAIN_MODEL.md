# Synveil canonical domain model

Status: **SKELETON_IMPLEMENTED — initial canonical entities and invariants are validated; the PostgreSQL canonical schema, explicit SQLx mappings, authenticated logical node metadata workflows, persisted upload-session/verified-replica subset, exact-offset HTTP upload transport, owner-authorized immutable content reads, authenticated HTTP full/single-range download transport, authenticated immutable version-history metadata, safe historical-version restore, metadata-only Trash retention/purge execution, FileVersion-based object reference accounting, metadata-only GC grace/lease planning, and crash-safe internal physical Object/ObjectReplica deletion are IMPLEMENTED/VALIDATED; bounded internal GC-worker orchestration and stuck-operation reconciliation are IMPLEMENTED; the durable owner/library-scoped change journal, per-device checkpoints, incremental change feed, acknowledgment, and materialized logical snapshot/rebaseline bootstrap are VALIDATED; typed client mutation submission with durable idempotency, canonical fingerprinting, optimistic concurrency, deterministic conflict persistence, and exact journal integration is IMPLEMENTED/VALIDATED; durable conflict records, manual inspection, and explicit manual resolution are IMPLEMENTED; automatic conflict resolution, the desktop sync agent, download UI, and broader content protocols remain NOT IMPLEMENTED/PLANNED**

This document owns the canonical meanings, fields, relationships, lifecycle
states, and transaction invariants of Synveil domain entities. It does not
prescribe a physical table layout or adapter implementation. Database
migrations and OpenAPI schemas may add representation detail, but they must
not redefine these semantics.

When this document conflicts with an accepted ADR, the ADR wins and this file
must be reconciled before implementation. See
[CONTRIBUTING_ARCHITECTURE.md](CONTRIBUTING_ARCHITECTURE.md) for authority and
change procedure.

## Modeling conventions

### Identity and common fields

- Every public domain ID is an opaque UUIDv7 serialized as a lowercase
  hyphenated string. Clients must not derive creation time, authorization,
  location, shard, storage path, or ordering from it.
- Database-internal numeric keys may exist for indexing but are never a public
  identity or authorization credential.
- UTC instants use RFC 3339 with sufficient precision to round-trip. A local
  capture time and UTC offset, where known, are stored separately rather than
  rewriting history from a later timezone guess.
- Mutable resources carry an integer `revision` that increments on each
  externally observable mutation. Its API representation supplies a strong
  metadata ETag. Immutable resources use their immutable ID/revision in an
  ETag, not a storage path.
- Fields named `created_at`, `updated_at`, and `deleted_at` are server-observed
  instants. A client-origin time is separately named and treated as untrusted
  metadata.
- User-provided display names are Unicode strings with bounded encoded length.
  Their comparison key is server-derived; it never doubles as a filesystem or
  object-store key.
- Hashes use an algorithm-qualified form. The initial canonical plaintext
  integrity hash is `sha256:<lowercase-hex>`.
- Enumerated values are uppercase ASCII in stored and API contracts. Unknown
  future values must cause a deliberate compatibility response, not accidental
  fallback to a destructive state.

Unless an accepted ADR introduces organizations, a `User` is the top-level
principal and ownership boundary. `Library` is the sync journal and policy
boundary. A deduplication domain is explicitly identified and cannot be
inferred from equal hashes.

### Logical topology

```mermaid
erDiagram
    User ||--o{ Session : authenticates
    Session ||--o{ CredentialGeneration : rotates
    User ||--o{ RecoveryCodeSet : recovers
    User ||--o{ Device : registers
    User ||--o{ Library : owns
    Library ||--o{ Node : contains
    User ||--o{ NodeFavorite : bookmarks
    Node ||--o{ NodeFavorite : bookmarked_by
    Node ||--o{ FileVersion : versions
    FileVersion }o--|| Object : references
    Object ||--o{ ObjectReplica : represents
    StorageBackend ||--o{ ObjectReplica : stores
    Library ||--o{ ChangeEvent : journals
    Device ||--o{ SyncCursor : checkpoints
    Library ||--o{ SyncCursor : positions
    Device ||--o{ BackupSet : defines
    BackupSet ||--o| BackupSchedule : configures
    BackupSchedule ||--o{ BackupScheduleRevision : records
    BackupScheduleRevision ||--o{ BackupScheduleOccurrence : materializes
    BackupScheduleOccurrence ||--o| BackupScheduleOccurrenceHandoff : hands_off
    BackupScheduleOccurrenceHandoff ||--|| BackupMaintenanceRun : binds
    BackupSet ||--o{ BackupSnapshot : captures
    BackupSnapshot ||--o{ BackupEntry : manifests
    BackupEntry }o--o| Object : references
    Node ||--o| TrashEntry : records
    Node ||--o{ Share : grants
    Node ||--o{ PhotoAsset : represents
    PhotoAsset }o--o{ Album : groups
    User ||--o{ Tag : owns
    GitIntegration ||--o{ Repository : discovers
    Project }o--o{ Repository : links
    Project }o--o{ Node : links
    FileVersion ||--o{ AIIndexRecord : derives
```

The diagram shows semantic relationships, not a prescribed table layout.
Many-to-many associations require explicit join records with ownership,
timestamps, and auditability.

## Identity and security domain

### `User`

Purpose: human account and principal boundary.

Canonical fields:

- `id`;
- `username` or login identifier plus a normalized uniqueness key;
- optional display name and verified contact/recovery attributes;
- `status`: `PENDING`, `ACTIVE`, `LOCKED`, or `DISABLED`;
- password-credential reference using a vetted Argon2id implementation and
  versioned tunable parameters, never a plaintext or reversible password;
- `is_instance_admin`;
- `session_epoch` for bulk credential invalidation;
- `created_at`, `updated_at`, and `revision`.

Invariants:

- Login identifiers are unique under the chosen normalization policy.
- Disabled users cannot create sessions or mutate resources. Retained data is
  not silently purged by disabling an account.
- Administrator privilege is explicit and audited; ownership is not inferred
  from it.

### `Session`

Purpose: independently revocable authentication grant.

Canonical fields:

- `id`, `user_id`, optional `device_id`, and optional user-visible grant label;
- `kind`: `WEB`, `API`, or `DEVICE`;
- `status`: `PENDING`, `ACTIVE`, `REVOKED`, or `EXPIRED`;
- explicit scopes appropriate to the grant kind;
- issued, last-used, absolute-expiry, idle-expiry, revoked instants;
- `user_session_epoch` snapshot;
- for `WEB` only, the hash/keyed verifier of the browser refresh secret and its
  rotation lineage/generation;
- coarse client metadata appropriate for security review, not an invasive
  fingerprint.

For `WEB`, only the refresh verifier—not its raw secret—is stored on `Session`.
For `API` and `DEVICE`, `Session` is the family aggregate and stores no direct
credential verifier or generation; every verifier and lineage field belongs to
the child `CredentialGeneration`. Browser refresh atomically consumes the old
refresh credential and issues the next one. This browser flow is deliberately
not transparently retryable after an ambiguous response: reuse of the consumed
credential atomically sets the `Session` to `REVOKED`, invalidates every
non-terminal credential in that family, records safe audit reason
`REFRESH_REPLAY_DETECTED`, and requires the user to log in again. No separate
quarantine state exists. That fail-closed rule distinguishes an unknown lost
response from pretending a replay is safe. Session possession never replaces
per-resource authorization.

The public `ApiGrant` representation is not a second authorization aggregate:
it is exactly a `Session` whose kind is `API`, and `grant_id` equals that
`Session.id`. Each `API` or `DEVICE` grant family has child
`CredentialGeneration` records with grant/session ID, monotonic generation,
verifier, status (`PENDING`, `ACTIVE`, `RETIRED`, `REVOKED`, or `EXPIRED`),
issue/activation/retirement/expiry times, and last-use evidence. At most one
generation is `ACTIVE` and one is `PENDING`. Initial activation changes both
the pending generation and grant to `ACTIVE`; rotation activation changes the
old generation to `RETIRED`, the candidate to `ACTIVE`, and leaves the grant
`ACTIVE`. Thus `RETIRED` is a credential-generation status, not a
`Session.status` value.

Creation or rotation first stores one bounded `PENDING` generation and returns
its raw secret exactly once. During rotation the previous `ACTIVE` generation
remains valid until activation. A lost one-time response therefore expires or
is replaced as an unusable pending secret rather than leaving an unknown
active credential. Requested scopes cannot exceed the owner's grantable scope
and never bypass per-resource authorization. Revoking the `Session` is the one
authoritative grant-family revocation: it atomically sets the grant and every
non-terminal generation to `REVOKED`.

A device bearer family is exactly one `Session` with kind `DEVICE` whose
`device_id` points to the `Device`; the device ID and session ID remain distinct.
It uses the same pending-generation activation and rotation rules. The Device
endpoint, rather than the generic Session endpoint, is the authoritative
revocation surface and atomically revokes the device, its credential family,
and every remaining generation.

### `RecoveryCodeSet` and `RecoveryTransaction`

Purpose: self-hosted recovery without retaining a recoverable copy of a code
or silently depending on hosted email.

- A `RecoveryCodeSet` has ID, user ID, generation, state (`PENDING`, `ACTIVE`,
  `RETIRED`, or `EXPIRED`), creation/activation/expiry instants, revision, and
  individually salted/keyed code verifiers with consumed instants. Raw codes
  are returned exactly once and are never stored.
- Creating a replacement first creates one bounded `PENDING` set; the existing
  `ACTIVE` set remains valid until the authenticated user confirms the new
  codes were saved and atomically activates the candidate. A new-key issuance
  atomically expires/replaces any older pending candidate while preserving the
  active set. A lost generation response therefore leaves an unusable candidate
  that can be replaced instead of disabling the last known recovery path.
- A `RecoveryTransaction` stores only a verifier, user/purpose, referenced code
  verifier ID, state (`PENDING`, `CONSUMED`, or `EXPIRED`), short expiry,
  attempt/rate-limit correlation, and consumed instant. Code exchange validates
  and reserves an active unused code. A serialized retry atomically changes any
  older transaction for that code from `PENDING` to `EXPIRED`, releases/rebinds
  its reservation to one new `PENDING` transaction, and returns the new secret
  once; it does not consume the code yet.
- If that response is lost, retrying with the same code invalidates the prior
  pending transaction through that exact `PENDING` → `EXPIRED` transition and
  issues a new one in the same transaction. Timeout expiry releases the
  reservation, so an unreachable transaction cannot consume the user's last
  recovery code. Attempts are serialized and strictly rate-limited.
- Activating a replacement code set atomically retires the old set and expires
  every pending `RecoveryTransaction`/reservation that references it. Password
  reset revalidates that the referenced code set/generation is still `ACTIVE`
  in the consume transaction; retiring a set therefore revokes a stolen code's
  unfinished recovery attempt.
- Password reset atomically consumes both the current recovery transaction and
  its referenced code, changes the Argon2id credential, increments the
  canonical `session_epoch`, invalidates every `WEB`, `API`, and `DEVICE`
  credential grant, changes every affected non-revoked `Device` to `PAUSED`,
  and records audit. Device records and data remain. Owner-driven re-enrollment
  binds a fresh pending `DEVICE` Session/generation to the paused device while
  the old family remains `REVOKED`; activation restores the device to `ACTIVE`.
  No operating-system wipe is claimed.

### `Device`

Purpose: user-visible client identity and policy attachment point.

Canonical fields:

- `id`, `owner_user_id`, display name;
- platform and application/protocol versions;
- declared capability set, credential-family `Session.id`, and current
  credential-generation reference, never a raw credential;
- `status`: `PENDING`, `ACTIVE`, `PAUSED`, or `REVOKED`;
- last-seen, last successful sync, and last successful backup instants;
- created, updated, revoked instants and `revision`.

Capability declarations are input to negotiation, not trusted proof of
security. Registration creates the `Device` in `PENDING` without a usable
credential. Initial credential issuance creates a bounded pending generation
whose raw secret is displayed once; only explicit activation changes the
generation, credential family, and device to `ACTIVE`. Rotation leaves the old
active generation valid until a saved pending replacement is activated. A lost
issuance response therefore leaves no unknown usable device secret. Revocation
atomically invalidates the family/generations and future API access. It does not
prove that downloaded data was erased.

## Storage and namespace domain

### `Library`

Purpose: ownership/policy boundary and one ordered synchronization journal.

Canonical fields:

- `id`, `owner_user_id`, name;
- root `Node` ID;
- `dedup_domain_id`;
- transactionally incremented `sync_head` (the library-row lock is acquired at
  the journal append boundary, after the namespace mutation has been checked);
- `journal_epoch` and minimum retained sequence;
- an internal transaction-scoped namespace-mutation guard used to order short
  `Node` commits in the initial correctness profile;
- quota/policy references;
- `status`: `ACTIVE`, `READ_ONLY`, `QUARANTINED`, or `DELETING`;
- timestamps and `revision`.

Every `Node`, `ChangeEvent`, cursor, share origin, and library-scoped mutation
belongs to exactly one library. A sequence is meaningful only with its library
and epoch.

### `Node`

Purpose: user-visible file or directory identity.

Canonical fields:

- `id`, `library_id`, nullable `parent_node_id` for the single root;
- `kind`: `FILE` or `DIRECTORY`;
- original `name` and server-derived `name_key`;
- for a file, nullable `current_version_id` while an upload has not committed;
- `state`: `ACTIVE`, `TRASHED`, or `PURGING`;
- canonical nullable `trashed_at`, set only for `TRASHED`/`PURGING` and cleared
  on restore; the retention policy derives `restore_deadline` without storing a
  second deadline column;
- metadata revision, created/updated instants, and creator/last-actor IDs.

A directory also exposes a server-issued opaque subtree precondition for
recursive destructive commands. The initial representation is resolved by
`SYNC.md` OD-SYNC-004; clients must not derive it from the ordinary metadata
revision.

Invariants:

- A library has exactly one directory root. The root has no parent and cannot
  be moved, trashed, or shared as a public write root unless separately
  specified.
- Each active parent/name-key pair is unique under the accepted name policy.
- A file has no children. A directory's ancestry must be acyclic and remain in
  one library.
- Rename and move mutate `Node` metadata; they do not modify `FileVersion` or
  relocate an `Object`.
- A content mutation changes `current_version_id` and node revision in the same
  transaction that creates the version and journal event.
- Trash eligibility uses only the server-observed `trashed_at`, the one
  authoritative retention policy, current server time, ownership, and safe
  state. `ACTIVE`, restored, root, and `PURGING` nodes are never fresh purge
  candidates. The current single-node Trash contract rejects non-empty
  directories, so candidate selection cannot orphan children.
- In the initial correctness profile, every short transaction that mutates a
  `Node` takes the per-library namespace guard before domain rows and the late
  `sync_head` lock. This gives directory move and recursive Trash a definite
  order against descendant edits without holding a lock during byte upload.

### `NodeFavorite`

Purpose: the current user's personal bookmark for a file or directory. A
favorite is not owner-wide `Node` metadata and is never inherited by another
member or share recipient.

Canonical fields and invariants:

- `user_id`, `library_id`, `node_id`, server creation/update instants, and a
  relation `revision`; `(user_id, node_id)` is unique;
- the referenced node and library must agree, but the relation never grants
  node access, retains a version/object, changes node revision, or creates a
  library `ChangeEvent`;
- reads always re-authorize the node for the current user, so revoked access is
  hidden immediately and cleanup may remove the now-inaccessible relation
  without exposing whether another user's node still exists;
- a favorite may survive `TRASHED` state so an authorized restore preserves
  the preference, but the normal Favorites view returns only readable
  `ACTIVE` nodes; logical purge removes the relation;
- create/remove are desired-state, idempotent preference mutations. The
  relation ETag supports conditional clients, an idempotency key recovers a
  lost response, and concurrent opposing requests resolve by server commit
  order only for this non-content preference.

### Recent node projection

“Recent” means caller-readable nodes most recently changed by a committed
create, content update, rename, move, restore, or other visible metadata
mutation. It is a derived query, not a stored recently-viewed history: reading,
previewing, or downloading a node does not create tracking state.

The projection orders by server-observed node update instant plus immutable
node ID, uses an initial `as_of` watermark in its opaque keyset cursor, and
optionally filters one authorized library. Mutations after that watermark
appear on refresh rather than moving rows inside an existing traversal. Every
page re-authorizes each node; inaccessible or `TRASHED`/`PURGING` nodes are
omitted immediately, and a shared result exposes no unreadable ancestor path.
The projection grants no access, creates no retention reference or
`ChangeEvent`, and does not claim to be an audit history.

### `FileVersion`

Purpose: immutable record binding one file revision to one canonical object.

Canonical fields:

- `id`, `library_id`, `node_id`, `object_id`;
- nullable `parent_version_id` and nullable `conflict_base_version_id`;
- logical byte length, canonical hash, declared/detected media type;
- client modification time plus server commit time;
- actor device/user, source type (`UPLOAD`, `SYNC`, `RESTORE`, `BACKUP_RESTORE`,
  or `SYSTEM_IMPORT`);
- optional conflict group/reason;
- immutable creation metadata.

A restore creates a new version whose source points to the selected historical
version; it never makes old history mutable. Equal objects may be reused only
inside the same allowed dedup domain.

The implemented metadata API lists these immutable records newest-first with a
bounded node-scoped keyset cursor and reports currentness only from
`Node.current_version_id`. Its public DTO intentionally excludes object,
replica, backend, staging, and filesystem identity. The direct version ID is
also the identifier accepted by the historical content-read route. The
authenticated restore route creates one new immutable head from a selected
historical version, uses the pre-restore head as its parent, reuses only the
same canonical Object with a matching verified replica, and leaves every
historical row unchanged. Trashed or purging file nodes remain concealed by
the active-file visibility contract. PostgreSQL end-to-end restore evidence is
environment-gated by `SYNVEIL_TEST_DATABASE_URL`.

After the Trash retention point of no return, the current metadata-purge
contract permanently removes the purged Node row and all of that node's
FileVersion rows in one transaction. It does not retain tombstoned
FileVersion history because the accepted current schema has no retained-history
tombstone contract. A minimal completed-purge replay identity remains without
filenames or paths. PostgreSQL end-to-end purge evidence is environment-gated
by `SYNVEIL_TEST_DATABASE_URL`.

### `Object`

Purpose: immutable canonical plaintext content identity and logical lifecycle
metadata inside one dedup domain. It is not a user-visible file or a physical
backend representation; one or more `ObjectReplica` records materialize it.

Canonical fields:

- `id` and `dedup_domain_id`;
- canonical plaintext hash and length;
- `state`: `STAGING`, `VERIFIED`, `QUARANTINED`, or `DELETING`;
- durability/verification instants and last verification result;
- creation time and garbage-collection eligibility time.

An `Object` becomes referenceable only in `VERIFIED`. Hash equality is
confirmed against length and verified bytes; a collision or mismatch is
quarantined rather than aliased. The current implementation uses the
`FileVersion -> Object` relation as the authoritative logical reference query;
it does not maintain a mutable global counter. A metadata-only
`object_gc_candidates` row records `unreferenced_at` and source after the last
FileVersion reference is released. That row is not a physical deletion
deadline. The implemented planner adds bounded grace, worker leases, generation
fencing, reference revalidation, and revocable `READY` planning. The internal
physical executor then acquires `GC_DELETING` under the canonical candidate ->
Object lock order, records a durable operation and deterministic replica action
plan, and repeats the final FileVersion/hold/lease proof before every external
delete. An Object may be `AVAILABLE` or `GC_DELETING`; the latter rejects new
FileVersion/ObjectReplica references and active holds. Confirmed absence is
recorded per replica before its metadata is removed; only after all replicas
are absent can the executor remove the candidate and Object and complete the
operation. The implemented internal worker only coordinates bounded recovery
and new-work slices through those services; it persists retry scheduling and
reports metadata-only inconsistencies, but does not auto-delete unknown physical
files. Backup/share/sync hold producers remain planned.

### `StorageBackend`

Purpose: configured object-store adapter and failure boundary.

Canonical fields:

- `id`, type (`LOCAL_FS`, `S3_COMPATIBLE`, `MINIO`, or future registered type);
- non-secret configuration reference and secret reference;
- namespace/prefix owned by Synveil;
- `status`: `ACTIVE`, `READ_ONLY`, `DEGRADED`, `OFFLINE`, or `RETIRED`;
- capability/version declaration, health timestamps, and `revision`.

Backend configuration never exposes credentials through normal reads. A
backend cannot be retired while it is the sole verified location of referenced
objects. Adapter migration is a verified copy-and-switch workflow, not an
in-place storage-key rewrite.

### `StorageCapabilities`

Purpose: versioned evidence about what a configured `StorageBackend` can safely
accelerate or guarantee.

This is a capability value/contract associated with `StorageBackend`, not a
second source of storage truth. It may declare `reflink`, `block_clone`,
`copy_on_write_clone`, `native_snapshot`, `compression`, `checksumming`,
`sparse_files`, `atomic_rename`, `durable_fsync`, `range_reads`, and
`filesystem_health`, together with probe/version evidence and limitations.
Application logic may select an optimization only after the adapter proves the
capability. It must retain a portable correctness path when a capability is
absent. Btrfs or WinBtrfs is never a required state for a backend or library.

### `ObjectReplica` and `ObjectLease`

These internal records are separate concepts even if an initial deployment
stores only one replica:

- `ObjectReplica` binds an object to `storage_backend_id` and opaque
  `storage_key`; representation codec/parameters/version, encryption
  scheme/key reference when applicable, stored length and independent
  stored-byte checksum; backend version evidence; state (`COPYING`,
  `VERIFIED`, `MISSING`, `CORRUPT`, `DELETING`); and verification evidence.
- `ObjectLease` protects staging, active download/assembly, restore, or
  migration work from garbage collection until a bounded expiry.

They let tiering and backend migration evolve without changing `Object`
identity. Leases are not a substitute for durable references and must expire.

## Upload domain

### `UploadSession`

Purpose: resumable, idempotent staging workflow that is not visible as a file
version until commit.

Canonical fields:

- `id`, owner user/device, target library and target node/parent intent;
- operation type (`CREATE_FILE` or `REPLACE_CONTENT`);
- expected total length, optional expected canonical hash, media/name metadata;
- required base node revision or base version;
- negotiated part constraints;
- `state`: `OPEN`, `VERIFYING`, `COMMITTING`, `COMMITTED`, `FAILED`,
  `EXPIRED`, or `ABORTED`;
- nullable cancel-request actor/instant and lease generation;
- expiry, created/updated instants;
- completion idempotency key and committed node/version outcome when present.

State transitions use row-level concurrency control or equivalent compare and
swap. Only one completion outcome can win. Retrying a committed completion
returns the stored outcome; it does not append a second version or event.
`VERIFYING` contains persisted internal recovery phases such as `ASSEMBLING`,
`HASHING`, `FINALIZING`, and `READBACK_VERIFY`; these are not alternate public
states. A generation-checked cancel can move `OPEN`, `VERIFYING`, or
pre-logical-commit `COMMITTING` to `ABORTED`. The logical commit transaction
rechecks state, lease generation, and cancel intent under the session lock; if
it commits first, `COMMITTED` wins and cancellation cannot undo the version.

### `UploadPart`

Purpose: one verified range or numbered part in a session.

Canonical fields:

- upload session ID and stable part number/range;
- declared and observed length;
- checksum algorithm/value;
- opaque staging locator;
- `state`: `PENDING`, `RECEIVING`, `VERIFIED`, or `REJECTED`;
- idempotency fingerprint and timestamps.

Parts may arrive out of order when negotiated. An identical retry is accepted;
a request that reuses a part identity with different bytes is a conflict.
Overlaps, gaps, total overflow, excessive part count, and expired sessions are
rejected before assembly.

## Synchronization domain — Prompt 35 status

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
| automatic conflict resolution | `NOT IMPLEMENTED` |
| desktop sync agent | `NOT IMPLEMENTED` |

### `ChangeEvent`

Purpose: durable committed fact required for clients to advance state.

The implemented foundation fields are:

- `entry_id`, `owner_user_id`, `library_id`, transactionally allocated
  `sequence`, and `journal_epoch`;
- schema-versioned typed resource and change kind, plus subject node ID;
- resulting node revision, parent ID, node kind/state, and current version ID
  when applicable;
- PostgreSQL transaction timestamp as descriptive `occurred_at`.

The unique key is (`library_id`, `journal_epoch`, `sequence`). Sequence order is
commit order for one library, not wall-clock or cross-library order. An event is
appended in the same PostgreSQL transaction as its domain mutation. The
foundation deliberately does not copy names, paths, object/replica identities,
actor-device metadata, or arbitrary JSON; future schema versions may add a
reviewed bounded projection without changing the ordering contract.

### `SyncCursor`

Purpose: opaque server token representing a journal position and epoch for a
client/library.

The metadata foundation exposes a distinct `JournalCursor` containing a version,
library ID, journal epoch, and last-delivered sequence. It is bounded, opaque to
callers, integrity-checked, and revalidated against the owner/library/head in
PostgreSQL. The public feed uses a separate bounded HMAC-signed acknowledgment
evidence token; it is integrity evidence only and is never authorization.

### `DeviceSyncCheckpoint`

Purpose: durable consumer progress for one authenticated owner's registered
device and one library.

Canonical fields:

- `owner_user_id`, `device_id`, and `library_id`, with composite ownership
  foreign keys and one unique checkpoint per device/library pair;
- `journal_epoch` and `acknowledged_sequence`, initialized to the current epoch
  and sequence zero on first use;
- monotonic `rebaseline_generation`, used only as a compare-and-set fence so an
  older bootstrap cannot replace newer synchronization progress;
- server-managed `created_at`, `updated_at`, and optional
  `last_seen_high_watermark`.

The checkpoint stores no journal payload, object/replica identity, storage key,
path, or device credential. Only an existing `ACTIVE` device and an owned
library can create, read, fetch, or acknowledge it. Fetch never advances it.
Acknowledgment is a row-locked compare-and-set: the signed page start must
equal the current sequence, the delivered interval must exist contiguously in
the journal, and progress can only advance within the current epoch. Older
valid acknowledgment replays return the current row without rewinding; gaps,
future progress, an epoch mismatch, and unavailable retained history fail
explicitly.

The current HTTP trust model is an authenticated owner session acting on behalf
of the registered device. Pairing, strong device credentials, and attestation
are not part of this phase.

Clients treat a cursor and acknowledgment token as opaque, cannot increment or
manufacture them, and must not use either as authorization. Cursors or
checkpoints for the wrong user/library/epoch, or below retained history, are
rejected with stable `not_found` or `sync_rebaseline_required` outcomes.

### `SyncBootstrap`

Purpose: a restart-safe server-side session binding one immutable logical
manifest to one exact journal handoff cut for one device/library.

Canonical fields are typed `SyncBootstrapId`, `owner_user_id`, `device_id`,
`library_id`, monotonic generation, `snapshot_epoch`,
`snapshot_resume_sequence`, immutable manifest item count and optional terminal
Node ID, state, and PostgreSQL/server-managed creation, expiry, and completion
timestamps. States are closed to `OPEN`, `COMPLETED`, `ABORTED`, and `EXPIRED`.
At most one `OPEN` row exists for a device/library scope. A safe repeated start
returns that row; replacing an expired row increments the checkpoint generation
before creating a new one.

Starting never resets the checkpoint. Completing requires exact terminal-page
evidence plus matching owner/device/library/session/generation/cut claims. In
one row-locked transaction it rechecks the current journal epoch and retained
history, refuses an already-ahead checkpoint, sets the checkpoint exactly to
the captured epoch/resume sequence, and marks the bootstrap `COMPLETED`.
Completed replay returns the committed checkpoint without another reset.

### `LogicalSnapshotNode`

Purpose: one immutable logical projection captured inside a `SyncBootstrap`;
it is neither a live `Node` row nor a backup/archive entry.

Fields are `node_id`, optional `parent_node_id`, logical name, `FILE` or
`DIRECTORY` kind, public `ACTIVE` or `TRASHED` state, revision, optional current
version ID, and—for a current file only—paired content length and SHA-256.
The canonical root is included. Internal `PURGING` rows and permanently purged
Nodes are absent, and complete historical FileVersion history is not copied.
Directory rows cannot carry content metadata; file content length/hash are
both present or both absent according to the current-version projection.

Manifest membership and values are copied in the same transaction that reads
the journal cut, then paged in ascending immutable Node ID order. Manifest rows
deliberately do not foreign-key back to mutable Node/FileVersion/Object rows, so
later rename, move, Trash, purge, content replacement, or version restore cannot
change an existing bootstrap page. They contain no Object/ObjectReplica ID,
storage key, staging handle, filesystem path, backend locator/version,
credential, GC state, or byte content. Retired-session cleanup cascades only to
these copied rows and never to canonical library, Node, FileVersion, Object, or
journal data.

### `LogicalSnapshot` and `RebaselineSnapshot`

Prompt 81 also defines the transport-neutral, non-device snapshot foundation.
`LogicalSnapshot` is the complete logical namespace for one `library_id`, not a
per-device duplicate. It includes the canonical root and is canonically ordered
by immutable `NodeId`; parent relationships must resolve through directory rows
to that one active root. Current `ACTIVE` and `TRASHED` Nodes are represented;
internal `PURGING` rows and permanently purged Nodes are not. The aggregate
contains no physical storage identity or file bytes.

The metadata `RebaselineSnapshot` pairs that state with the existing typed
`JournalHighWatermark`/`JournalCursor`. The PostgreSQL builder derives both from
one `REPEATABLE READ` view while holding the existing library namespace guard,
so a committed cooperative mutation cannot fall between the state and its
continuation boundary. Building the value does not alter any
`DeviceSyncCheckpoint`.

Prompt 82 makes that same validated cut durable without turning it into a
device-owned bootstrap. `RebaselineSnapshotId` is a distinct UUIDv7 identity;
the PostgreSQL header stores the library, full typed journal boundary, immutable
entry count, and injected creation/expiry instants, while one row per
`LogicalSnapshotNode` stores only the canonical logical projection. The header
and all entries commit atomically, are immutable after publication, and contain
no physical object/replica ID, storage key, backend/filesystem locator, staging
handle, secret, or file bytes.

`RebaselineSnapshotDescriptor` and `RebaselineSnapshotPage` are
transport-neutral metadata values. Pages use a distinct
`RebaselineSnapshotPageCursor` (artifact ID plus last immutable `NodeId`), not
a `JournalCursor`; the latter remains the incremental continuation boundary.
Page reads are owner-scoped, use immutable keyset order and one short
repeatable-read header/entry view, and never reread the live namespace or take
its mutation guard. A snapshot is valid only while `observed_at < expires_at`;
expiry rejects payload reads. Prompt 86 may later remove that payload through
an explicit bounded call while retaining its handoff proof.

### `RebaselineSnapshotHandoffProof` and journal retention floor

Purpose: preserve the minimum immutable authority needed to complete a
snapshot checkpoint handoff after the large transfer payload is gone, while
bounding both journal and proof storage.

Canonical proof fields are `snapshot_id`, `owner_user_id`, `library_id`,
`journal_epoch`, `snapshot_resume_sequence`, `snapshot_created_at`,
`snapshot_expires_at`, and `proof_expires_at`. Snapshot identity is unique. All
fields are immutable, the proof is created in the payload transaction, and its
deadline is exactly 30 days after payload expiry. It contains no entry count,
Node projection, Object/ObjectReplica identity, storage key/path, credential,
or device/checkpoint identity. It intentionally does not cascade from the
payload header.

The existing Library `minimum_retained_sequence` is the current epoch's
durable compacted-through floor. It is monotonic within that epoch, never above
`sync_head`, and changes atomically with deletion of exactly the contiguous
prefix through the new value. A cursor lower than the floor is stale; equality
is a valid exclusive-after continuation. Proofs for the same Library and epoch
temporarily cap the floor at their smallest boundary. Ordinary device
checkpoints do not pin this value and cleanup does not mutate them.

Expired payload cleanup changes only `rebaseline_snapshots` and its cascaded
entries; proof cleanup changes only eligible proof rows whose payload is
already absent; journal cleanup changes only journal rows and the Library floor.
All are bounded explicit metadata operations. There is no public retention API,
background runtime, retry, conflict behavior, or Prompt 87 client recovery in
this entity contract.

### `RebaselineConvergenceCoordinator` local lifecycle

Prompt 87 adds no server entity or local schema migration. It composes the
existing v5 `rebaseline_candidates` namespace and one
`rebaseline_applied_handoffs` marker per Library. A real candidate and a prior
pending marker may coexist only during proof-loss/checkpoint-conflict recovery:
the candidate is an untrusted staged remote base, while the old marker remains
the authoritative inbound fence. An inert `FAILED` candidate row is reserved
only as a descriptor-less, library-scoped claim around the one non-idempotent
snapshot-create POST; it is never page-readable or activatable.

Activation of a complete candidate is the only local transition that can change
the remote base and pending marker. It transactionally exposes either the old
base plus H1 or the replacement base plus H2, never no marker and never a
mixed pairing. Prompt 85 alone installs the server-proved boundary in the local
cursor and deletes H2. Local outbound intents and upload/submission rows are
not members of either transition. The coordinator has no durable retry count:
one invocation may create one artifact; a second canonical handoff conflict is
a typed stop, and a later caller decides whether to start another invocation.

### Conflict representation

Every client mutation carries a durable `client_mutation_id`, a canonical
fingerprint, a base journal epoch/sequence, and typed resource preconditions.
When a precondition fails, the server preserves the canonical Node and returns
a typed `MutationConflict` with the supported reason, expected value, safe
current logical state when available, and the server epoch/sequence. A
purged resource is reported from its retained `NODE_PURGED` tombstone rather
than resurrected or treated as an empty result.

Prompt 35 gives every managed terminal mutation conflict one typed UUIDv7
`SyncConflictId` and one `sync_conflicts` row in the same transaction that
terminalizes the original `device_mutation_operations` row. The relationship
is one-to-one in both directions. Replaying the original mutation ID and
fingerprint returns the same conflict ID; reusing it with different semantics
is still a mutation-identity conflict. Authentication, CSRF, malformed input,
dependency/internal failures, identity reuse, and `sync_rebaseline_required`
never create managed conflict rows.

The conflict is scoped by owner, originating Device, Library, original client
mutation, and primary logical resource. Its closed lifecycle is `OPEN`,
`RESOLVED`, or `DISMISSED`. Evidence is immutable: conflict/original-mutation
identity, typed kind, reason, resource, original expected context, closed typed
intent fields, historical server revision/state/parent/name, captured
epoch/sequence, and creation timestamp. It stores no raw request JSON, path,
Object/ObjectReplica identity, locator, staging handle, credential, or bytes.
The historical projection is evidence, never current canonical truth. There is
no Node foreign key, so purge and rebaseline do not remove it. Only lifecycle
and terminal resolution linkage can transition once.

`sync_conflict_resolutions` is the smallest durable decision record needed for
UUIDv7 resolution identity, versioned typed SHA-256 fingerprinting, response-
loss replay, concurrency fencing, stale-result audit, and optional normal
journal-event linkage. Its action vocabulary is exactly `ACCEPT_SERVER` and
`APPLY_CLIENT_INTENT`. Same ID and fingerprint returns the original outcome,
timestamp, and event linkage with a replay marker; a semantic mismatch returns
`resolution_id_conflict`.

`ACCEPT_SERVER` moves OPEN to DISMISSED and changes no canonical resource or
journal. `APPLY_CLIENT_INTENT` requires caller-supplied fresh current revisions,
reconstructs the preserved semantic intent, and uses the shared Prompt 34
transaction-local mutation executor and lock order. Success atomically changes
the Node, appends exactly one ordinary `ChangeEvent`, terminalizes the decision,
and moves the conflict to RESOLVED. A stale/purged result terminalizes only that
resolution attempt as stale, keeps the conflict OPEN, and changes no Node,
journal, or checkpoint. Concurrent decisions can produce at most one terminal
conflict transition. The original Prompt 34 operation remains CONFLICT forever.
No automatic conflict-copy, merge, last-writer-wins, silent overwrite, or
automatic action-selection policy exists.

## Backup domain

Prompt 72 status: **durable backup scheduling, occurrence identity,
exactly-once occurrence-to-maintenance handoff, deterministic manual
single-step scheduler tick, bounded restart-safe misfire policy, fenced
scheduled-maintenance worker step, manually invoked bounded
scheduler+worker cycle, service integration boundary, canonical lock
ordering, internal one-shot runtime, and external systemd oneshot+timer
lifecycle IMPLEMENTED/VALIDATED**. The external timer owns recurrence
(approx once per minute, `Persistent=true`, `RandomizedDelaySec=10s`);
the one-shot binary owns exactly one bounded cycle; no daemon, loop,
retry, or lifecycle table is introduced. The schedule,
occurrence ledger, handoff, skip, and claim relations plus the tick and worker
step are control-plane metadata; the handoff creates a canonical maintenance
run and the worker step advances it by at most one fenced transition per
explicit invocation. No scheduler daemon, poll loop, retry queue, heartbeat,
automatic snapshot capture, HTTP route, or UI is implied by this status;
scheduled backups do not run continuously in the background.

### `BackupSet`

Purpose: device-scoped definition of protected sources and retention policy.

Canonical fields:

- `id`, owner user, source device;
- display name, source descriptors with client-stable opaque source IDs;
- include/exclude rules and symlink policy;
- schedule/continuous mode;
- retention-policy reference;
- `status`: `ACTIVE`, `PAUSED`, `DEGRADED`, or `RETIRED`;
- last observation/success, timestamps, and `revision`.

A source descriptor is not trusted as a server path and does not grant access
outside the client-selected source.

### `BackupSchedule` and `BackupScheduleRevision`

Purpose: one durable local-time recurrence for one `BackupSet`, with immutable
configuration history and an authoritative current revision.

`BackupSchedule` has `id`, `owner_user_id`, `backup_set_id`,
`current_revision_id`, `enabled`, `effective_from`, `created_at`, and
`updated_at`. There is at most one schedule for a BackupSet.
`BackupScheduleRevision` has its own ID,
the schedule/owner/BackupSet scope, a positive monotonic `revision_number`,
the idempotent `operation_id`, a versioned canonical semantic fingerprint,
`recurrence_kind` (`DAILY` or `WEEKLY`), an explicit IANA `timezone`, a local
`HH:MM` minute, normalized Monday-through-Sunday `weekly_days`, `misfire_mode`
(`REPLAY_ONE_BY_ONE` or `LATEST_ONLY`), bounded `max_lateness_seconds`, and
`created_at`. The safe default is `LATEST_ONLY` with 604800 seconds; the closed
lateness range is 60 through 2678400 seconds.

The schedule owner and BackupSet scope are checked at every service boundary.
Revisions and operation evidence are append-only; the current pointer must
refer to exactly one in-scope revision and can move only to a higher revision
number. A semantically identical request returns the current revision without
creating a revision, while reusing an operation identity for different
semantics fails closed. Policy changes are semantic changes and append a new
revision; an exact timing-and-policy no-op leaves both revision and
`effective_from` unchanged. Version 1 fingerprint evidence retains its original
timing-only replay interpretation, while new version 2 fingerprints include
normalized timing, mode, and lateness. Daily schedules have no weekdays;
weekly schedules have at least one normalized weekday.

The pure planner accepts an exclusive UTC instant and resolves the configured
local date/time using the stored IANA rules. It returns the next instant
strictly after the reference, advances a nonexistent DST wall time to the
first valid minute on the same local date, and chooses the earlier absolute
instant for an ambiguous fall-back wall time. An occurrence is effective only
when the owning `BackupSet` is `ACTIVE` and the schedule is enabled. Disabling
the schedule suppresses occurrences without deleting its current or historical
configuration.

`effective_from` is a dedicated strict activation boundary. First
configuration, a semantic current-revision change, and `DISABLED -> ENABLED`
advance it; a semantic no-op does not. A current revision's occurrence can be
newly materialized only when its canonical UTC instant is due and strictly
later than this boundary.

### `BackupScheduleOccurrence`

Purpose: immutable durable identity for one scheduled firing opportunity,
separate from any future execution state.

Canonical fields are opaque UUIDv7 `id`, owner/BackupSet/schedule/revision
scope, `local_calendar_date`, actual `resolved_local_wall_time`, revision IANA
timezone, canonical `scheduled_for_utc`, and `materialized_at`. The logical key
is `(schedule_revision_id, local_calendar_date)`; `(schedule_id,
scheduled_for_utc)` is a secondary exact-instant uniqueness fence.

Materialization locks the owning BackupSet and then schedule, checks an
existing logical row first, and only for a new row checks ACTIVE/enabled,
current revision, exact recurrence target, strict effectivity, and due time.
The server recomputes the resolved local and UTC values through the same DST
planner used by Prompt 61. Concurrent requests and lost-response retries
converge to the same occurrence ID. An existing old-revision row remains
replayable after edits/disables; an old revision without a row cannot newly
materialize.

Occurrence rows reject UPDATE and DELETE. Materialized means only “durably
recognized”; it does not mean a backup ran or succeeded. The separate
`BackupScheduleOccurrenceHandoff` relation binds one such occurrence exactly
once to one canonical `BackupMaintenanceRun`; it does not add progress to the
occurrence itself. Execution claims live only in the separate Prompt 66
`BackupScheduledMaintenanceClaim` relation. This domain performs no snapshot,
journal, ObjectStore, sync, restore, prune, GC, or physical-storage mutation
as part of the handoff.

### `BackupScheduleOccurrenceHandoff`

Purpose: immutable provenance binding from one already-materialized occurrence
to the one canonical maintenance run created for that scheduled firing.

The relation contains only `occurrence_id`, owner/BackupSet/schedule scope,
`maintenance_run_id`, and `created_at`. PostgreSQL enforces one relation per
occurrence and one scheduled occurrence per maintenance run, with composite
foreign keys proving that the occurrence, schedule, BackupSet, owner, and run
share one logical scope. UPDATE and DELETE are rejected; retargeting an
occurrence or attaching an arbitrary pre-existing manual run is not supported.

The handoff service requires the occurrence to exist first. It serializes
`BackupSet → BackupSchedule → Occurrence → maintenance-run/policy → handoff`,
checks a committed relation before evaluating current schedule/set fences, and
returns the canonical run and relation as `CREATED` or `EXISTING`. A new run is
created through the Prompt 49 primitive in `CREATED` state, binding its current
immutable retention-policy revision and child operation identities. Handoff is
not completion: it does not capture a snapshot, plan expiry, advance the run,
or provide a scheduler daemon, retry policy, or automatic execution loop.

### `BackupScheduleMisfireSkip` and manual scheduler tick

`BackupScheduleMisfireSkip` is immutable control-plane evidence that one
activation epoch intentionally advanced over an expired prefix. It stores an
opaque UUIDv7 ID, owner/BackupSet/schedule/revision scope,
`activation_effective_from`, the `(resolved_from_exclusive_utc,
resolved_through_utc]` range, observation time, and the immutable revision's
mode/lateness snapshot. Composite foreign keys, activation/policy validation,
monotonic insertion, range checks, unique boundaries, and an UPDATE/DELETE
trigger fence the ledger.

Prompt 65 extends the explicit `BackupSchedulerService::run_scheduler_tick`
call with an injected `observed_at_utc`. Per activation, the resolution
reference is `max(effective_from, latest handed-off occurrence, latest skip
resolved_through_utc)`. Materialization alone never advances it. Each schedule
derives at most one `SKIP_EXPIRED`, existing-handoff, or
materialize-and-handoff action. Global ordering uses the action instant, then
stable schedule and revision IDs. The service has no durable or in-memory
cursor and does not materialize future or collapsed rows merely to discover
work.

The exact cutoff is `observed_at_utc - max_lateness_seconds`; equality remains
eligible. `REPLAY_ONE_BY_ONE` resolves an expired prefix first and otherwise
selects the oldest eligible occurrence. `LATEST_ONLY` selects the newest
eligible occurrence; a successful handoff itself resolves earlier backlog. If
all unresolved work is expired, one range row resolves through the latest
canonical due occurrence with no occurrence, handoff, or maintenance run.

The selected candidate is passed through Prompt 62 materialization and Prompt 63
handoff; direct scheduler inserts into the occurrence, handoff, or maintenance
tables are forbidden. The scheduler handoff adds an atomic current-revision and
`effective_from` activation-epoch fence while preserving Prompt 63's direct
replay behavior. Historical, disabled-period, superseded, and re-enabled old
unhanded occurrences remain audit evidence and are not automatically caught up.

The tick result is `IDLE`, `SKIPPED_EXPIRED`, `HANDED_OFF_EXISTING`, or
`MATERIALIZED_AND_HANDED_OFF`. It creates at most one occurrence, one handoff,
and one `CREATED` maintenance run. It never advances the run or performs
snapshot, expiry, prune/GC, journal, sync, or ObjectStore work. This is a
restart-safe manual invocation, not a daemon, poller, retry policy, lease, or
continuous background scheduler.

### `BackupScheduledMaintenanceClaim` and fenced worker step

Purpose: durable authorization for exactly one canonical Prompt 49 transition
from one expected maintenance state, plus the lease that fences stale holders.

Canonical fields are opaque UUIDv7 `claim_id`, owner/`BackupSet`/schedule/
occurrence/maintenance-run scope, `expected_state` (only `CREATED`,
`SNAPSHOT_CAPTURED`, or `EXPIRY_PLANNED`), the predetermined `resulting_state`
(`SNAPSHOT_CAPTURED`, `EXPIRY_PLANNED`, or `COMPLETED` respectively),
`lease_worker_id`, unpredictable `lease_token`, monotonically increasing
`lease_generation` starting at 1, `lease_acquired_at`, `lease_expires_at`
(strictly later), `completed_at`, and timestamps. Claim identity is
`(maintenance_run_id, expected_state)` with database uniqueness; lease tokens
are unique. Completion requires `completed_at` and `resulting_state` together;
a half-completed receipt cannot commit.

Provenance is database-fenced to a committed Prompt 63 handoff binding the
same occurrence, run, owner, `BackupSet`, and schedule, so manual runs can
never gain a claim and cross-scope forgery is rejected. A claim starts at
generation 1 and incomplete. An incomplete claim accepts exactly two
transitions: a takeover at or after expiry (generation N → N+1 with a fresh
token and a new lease interval) or a completion sealing the predetermined
result with lease identity frozen. Completed receipts reject UPDATE, all rows
reject DELETE, and provenance columns are immutable.

`ScheduledMaintenanceWorkerService` exposes `claim_next_...`,
`execute_claimed_...`, and the combined `run_scheduled_maintenance_worker_step`,
all explicitly invoked with an injected `observed_at_utc` and a bounded lease
duration (default 120 seconds, 10 through 900 accepted). Discovery scans
scheduled runs globally by `occurrence.scheduled_for_utc`, `schedule_id`,
then `maintenance_run_id`; unexpired foreign leases skip without blocking,
expired oldest leases take over first, and oldest already-resulted claims
reconcile first. Execution verifies the full lease fence inside the same
transaction that commits the single Prompt 49 transition, reusing the
canonical capture, expiry-planning, and expiry-execution operations with the
run's durable child-operation identities. Crash-before-advance is taken over;
crash-after-advance reconciles without a second transition and stops;
lost completion responses replay canonically; unexpected states fail closed;
`STALE` runs report a typed stale outcome. One invocation performs at most one
semantic transition or recovery action. There is no daemon, polling or
heartbeat loop, retry/backoff, public API, UI, or physical identity in this
primitive.

### `ScheduledMaintenanceCycleResult` and manual bounded cycle

Purpose: one manually invoked orchestration boundary composing exactly one
scheduler tick and exactly one worker step, with no new durable state.

`ScheduledMaintenanceCycleService::run_scheduled_maintenance_cycle` takes an
explicit `worker_id`, an injected `observed_at_utc`, and a bounded lease
duration, then runs the canonical tick before the canonical worker step and
returns `ScheduledMaintenanceCycleResult { tick, worker }`. The tick side is
`Idle`, `SkippedExpired`, `HandedOffExisting`, or `MaterializedAndHandedOff`
with `outcome()`/`skip_outcome()` accessors mirroring
`BackupSchedulerTickResult`; the worker side is `Idle` or
`Stepped(ScheduledMaintenanceWorkerStepOutcome)`. Failures are typed as
`Scheduler(BackupSchedulerError)` or `Worker(ScheduledMaintenanceWorkerError)`;
a scheduler failure skips the worker step, while a worker failure preserves
committed scheduler state without a spanning transaction. At most one
maintenance transition commits per invocation, global claim ordering and lease
fencing are unchanged, and no cycle table, cursor, heartbeat, retry, daemon,
or physical identity is introduced.

### `BackupSnapshot`

Purpose: immutable committed manifest view for one backup set.

Canonical fields:

- `id`, `backup_set_id`, source device;
- parent snapshot ID when incremental;
- `state`: `BUILDING`, `VERIFYING`, `COMMITTED`, `FAILED`, or `EXPIRED`;
- scan start/end and server commit instants;
- manifest format/version, root hash, entry count, logical/unique byte counts;
- consistency class (`FILESYSTEM_CONSISTENT`, `CRASH_CONSISTENT`, or
  `BEST_EFFORT`), completeness, and client/software metadata;
- retention deadline/legal hold where applicable.

Only `COMMITTED` snapshots are restorable. Snapshot commit atomically records
the complete verified manifest references and durable follow-up work. A
`BUILDING` or `FAILED` snapshot may protect staging objects only through
bounded leases.

### `BackupEntry`

Purpose: one immutable manifest entry, not a live `Node`.

Canonical fields:

- snapshot ID, stable entry ID, parent entry ID;
- type (`FILE`, `DIRECTORY`, `SYMLINK`, or explicitly supported type);
- original relative name/path components as untrusted metadata;
- file object/version reference, size, canonical hash, timestamps, and bounded
  portable metadata;
- capture result (`PRESENT`, `UNCHANGED`, `UNREADABLE`, `EXCLUDED`, or
  `MISSING_OBSERVATION`) with error classification.

An absent entry in a later snapshot does not mutate a library node and does not
retroactively remove it from an earlier retained snapshot.

### Restore operation

A restore is a durable operation record with source snapshot/version, explicit
destination, collision policy, actor, state, per-entry results, byte/hash
verification, and idempotency key. Partial completion is visible and
restartable; “successful” means every required result is verified or an
explicitly accepted skip is recorded.

## Sharing, trash, and policy domain

### `Share`

Purpose: revocable authorization grant over a node/subtree.

Canonical fields:

- `id`, owner/grantor, origin library/node;
- grantee user ID for private shares or hashed random-secret verifier for
  public links;
- permission (`READ` or `WRITE`) and whether it includes descendants;
- optional password verifier, expiry, access limits, created/revoked instants;
- `status`: `PENDING`, `ACTIVE`, `REVOKED`, or `EXPIRED`; revision and audit
  correlation.

A share never grants direct `Object` access independently of an authorized
download decision. Moving a node within its allowed library preserves its
identity; moving or copying across ownership boundaries requires explicit
share behavior and cannot silently broaden access.

Private-share discovery is an authorization-filtered view of `Share`, not a
client-maintained ID list. An active private grant appears in the grantee's
“shared with me” collection with only a safe share-root `Node` projection;
ancestor paths the grantee cannot read, other grantees, object/replica locators,
and public-link secrets are excluded. The grantor's “shared by me” collection
may include pending, active, expired, or revoked grants under an explicit status
filter, but never returns a raw capability or password. Expiry/revocation removes a
grant from grantee discovery immediately at authorization time even if cleanup
or a previously issued page cursor lags. Discovery itself does not retain
content or broaden the underlying grant.

A public-link secret has at least 128 bits of cryptographic randomness before
encoding. Creation stores the public share as `PENDING` with only its keyed/
cryptographic verifier and returns the raw secret exactly once. A pending link
cannot authorize access; explicit owner activation changes it to `ACTIVE`
after the capability is saved. If the response is lost, idempotency replay
returns only safe candidate metadata. The inert candidate must be revoked, then
a new share created with a new key—raw capability material is never stored,
replayed, listed, or logged. Private user grants may be created directly as `ACTIVE` because they
carry no one-time bearer secret.

### `TrashEntry`

Purpose: deletion context and retention deadline for a trashed node.

Canonical fields:

- node ID and library ID;
- former parent/name projection;
- deletion actor/device, deletion sequence and time;
- scheduled purge time, restore policy, and optional subtree operation ID.

Restoring is conditional because the former parent or name may now conflict.
Purge is irreversible at the logical layer but physical bytes remain until all
other authoritative references and safety windows are cleared.

### Policy and quota

Policy records are versioned and attached explicitly to a user, library,
device, backup set, or share. Effective-policy resolution is deterministic and
auditable. Quota reservations cover staging and commit races; logical,
retained, staged, and physical bytes are reported separately.

## Photos domain

### `PhotoAsset`

Purpose: photo-specific projection over one or more canonical file resources.

Canonical fields:

- `id`, owner/library, primary node and source file-version IDs;
- media kind (`IMAGE`, `VIDEO`, `LIVE_GROUP`);
- capture instant, original UTC offset, metadata confidence/source;
- dimensions, duration/orientation where safely extracted;
- favorite flag, screenshot classification and provenance;
- processing state (`PENDING`, `READY`, `PARTIAL`, `FAILED`, or `STALE`);
- source device and device-scoped import identity where supplied;
- timestamps and `revision`.

The asset does not own or rewrite original bytes. A new current
`FileVersion` makes derived data stale until reprocessed. Location and face
data are sensitive derived metadata with separate policy.

### `PhotoResource` and `PhotoDerivative`

- `PhotoResource` associates a role (`PRIMARY_IMAGE`, `MOTION_VIDEO`,
  `DEPTH`, or future registered role) with a node/version, allowing
  Live-Photo-like grouping without losing originals.
- `PhotoDerivative` records source version, kind/size, derivative object,
  generator and parameters, state, and integrity. It is replaceable and never
  becomes the canonical original.

### `Album` and membership

`Album` has ID, owner, title, type (`MANUAL` or a future saved query), cover
reference, timestamps, and revision. Membership is an ordered explicit join
with added-at/actor fields. Removing membership never deletes the asset.

### `Tag` and assignment

`Tag` has ID, owner, normalized name key, display label, optional color, and
revision. An assignment links a tag to a subject with provenance `USER`,
`SYSTEM`, or `AI`, confidence only when meaningful, model/rule reference, and
review state. User edits are not overwritten by reindexing.

[PHOTOS.md](PHOTOS.md) owns ingestion, privacy, duplicate, derivative, and
client behavior.

## Integration and workspace domain

### `GitIntegration`

Purpose: configured connection to an external Git service.

Canonical fields:

- `id`, owner, provider type (`FORGEJO` initially);
- canonical base URL plus separately stored secret reference;
- provider installation/account identity;
- requested and verified capability scopes;
- `status`: `PENDING`, `ACTIVE`, `DEGRADED`, `REAUTH_REQUIRED`, `PAUSED`, or
  `REVOKED`;
- last successful poll/webhook, health summary, timestamps, and revision.

URLs and credentials are security-sensitive. Integration records do not make
Synveil the authority for Git permissions.

### `Repository`

Purpose: cached external-repository identity and backup subject.

Canonical fields:

- `id`, integration ID, immutable provider repository ID;
- owner/name/full-name display metadata and canonical provider URL;
- visibility as last observed, default branch, archived state;
- last observed provider revision/updated time;
- sync health and freshness;
- timestamps and revision.

Provider metadata is stale when Forgejo is unreachable and must say so.
Possession of cached metadata does not grant repository access.

### `RepositoryBackup`

Purpose: immutable verified repository recovery point, distinct from a device
`BackupSnapshot`.

Canonical fields:

- `id`, repository ID, state (`BUILDING`, `VERIFYING`, `COMMITTED`,
  `INCOMPLETE`, `FAILED`, or `EXPIRED`);
- capture start/end, provider identity/version;
- consistency class and observed refs;
- versioned manifest/root hash;
- references to Git data, Git LFS, supported release artifact objects and
  per-component results;
- retention and verification evidence.

Only `COMMITTED` backups satisfying their declared consistency class are
offered as complete restores. `INCOMPLETE` records may be retained for
diagnostics but are not mislabeled.

### `Project`

Purpose: optional workspace linking independently owned subjects.

Canonical fields:

- `id`, owner, name, description;
- status, timestamps, and revision.

Typed link records connect a project to repositories, library nodes, backup
sets/snapshots, devices, and metadata. A link does not transfer ownership,
broaden permissions, change retention, or cascade-delete its target.

[CODE_INTEGRATION.md](CODE_INTEGRATION.md) owns provider, backup, restore,
webhook/polling, and project-link behavior.

## AI-derived domain

### `AIIndexRecord`

Purpose: replaceable derived index unit tied to an immutable source revision.

Canonical fields:

- `id`, owner and authorization scope;
- source type/ID and immutable source revision or version;
- modality and chunk/region locator;
- pipeline, extractor, model, model-license, and configuration versions;
- inference mode (`LOCAL` or `REMOTE`) and consent/policy revision;
- content hash/fingerprint of the indexed input;
- status (`PENDING`, `READY`, `FAILED`, `STALE`, `DELETING`);
- derived text/tag/vector references, sensitivity classification;
- attempt/error class, created/updated/indexed/expires instants.

The record is never canonical user data. Search authorization is evaluated
against current source access, not only a stale ACL snapshot in the index.
Deletion, share revocation, source-version change, model change, or consent
change invalidates or removes affected derived records under
[AI.md](AI.md).

### `AIJob`

An asynchronous, at-least-once job carries an idempotency identity based on
pipeline + immutable source version + configuration version. It has bounded
attempts, lease, next-attempt time, safe error class, and terminal/dead-letter
state. A poison input cannot block unrelated indexing.

## Audit and asynchronous delivery domain

### `AuditEvent`

Purpose: append-only security and material-operation fact.

Canonical fields:

- event ID, server time, actor user/session/device or system actor;
- action, target type/ID, library/owner scope;
- outcome and stable reason code;
- request/correlation ID and coarse network/client context allowed by policy;
- redacted structured details and schema version.

Audit records never contain passwords, bearer tokens, public-link secrets,
object-store credentials, raw file content, embeddings, or full sensitive
paths unless an explicit audited policy requires a bounded representation.
Audit retention and administrator access are separate policy decisions.

### `OutboxEvent`

Purpose: durable delivery of follow-up work after a committed core mutation.

Canonical fields:

- ID, aggregate type/ID/revision, event type and schema version;
- transaction commit time, redacted payload/reference;
- availability time, attempt/lease state, delivered/terminal timestamps.

It is inserted in the same PostgreSQL transaction as the mutation. Consumers
are idempotent and delivery is at least once; “delivered” never means every
external side effect is exactly once. An optional consumer outage only
increases lag.

## Atomicity and consistency invariants

The following groups are single PostgreSQL transactions:

| Mutation | Required atomic metadata effects |
|---|---|
| Content commit | Verify the durable receipt; select or create the canonical `Object`; create or attach a verified `ObjectReplica` binding the backend, key, and representation; create `FileVersion`; create/update the `Node` head; reserve/finalize quota; append `ChangeEvent`, `AuditEvent`, and `OutboxEvent`; store the idempotent outcome. |
| Rename or move | Take the namespace guard; validate authorization/name/acyclic ancestry and expected revision; mutate node; append change, audit, outbox, and idempotent outcome. |
| Trash or restore | Take the namespace guard; validate node revision and the server-issued subtree precondition for recursive actions; mutate node state and `TrashEntry`; append change/audit/outbox; restore may resolve a name/parent conflict only by explicit policy. |
| Share mutation | Mutate grant/revision and append audit/outbox. Share revocation is checked at request time. |
| Backup snapshot commit | Transition verified complete manifest to `COMMITTED`, finalize authoritative object references/accounting, record audit/outbox and idempotent outcome. |
| Repository backup commit | Record consistency class, complete component manifest, verified object references, audit/outbox, and one terminal outcome. |

PostgreSQL cannot roll back a filesystem or S3 write. The safe protocol is:

1. create an opaque staging object and bounded lease;
2. stream bytes with size/quota limits;
3. durably finalize and verify the object;
4. commit all logical references and events in PostgreSQL;
5. on database failure, leave an unreferenced object for delayed,
   safety-windowed reconciliation;
6. on lost response, return the stored result for the same idempotency key.

Garbage collection never relies solely on age or a mutable reference counter.
It enumerates authoritative references, leases, object state, retention/legal
hold, and a minimum safety delay; deletion is two-phase and auditable.

## Authorization and privacy boundaries

- A principal is authorized by authenticated identity, owner/library
  relationship, share grant, role, and current policy. An opaque ID, hash,
  cursor, cached repository row, or object key is never sufficient.
- Queries scope by ownership before pagination and aggregation. Counts, timing,
  quota savings, dedup hits, search scores, and error differences must not leak
  another user's data.
- Public links map a random secret to a bounded `Share` and still enforce
  expiry, password, scope, rate limit, and revocation.
- Backup, photo, AI, and repository-derived metadata inherits the source's
  sensitivity and cannot silently acquire broader project or album access.
- Storage adapters receive opaque keys and bytes, not user paths. AI remote
  providers receive content only under explicit mode/policy/consent.
- Deleting a user-facing reference may leave retained versions, snapshots,
  verified repository backups, or audit facts. The UI and erasure workflow
  disclose each retention domain rather than claiming immediate physical
  erasure.

## Required failure behavior

| Case | Domain outcome |
|---|---|
| Two completions race | One `UploadSession` terminal outcome wins; equivalent retry returns it, conflicting request returns `completion_conflict`. |
| Object write succeeds and DB transaction fails | No visible version exists. Object is unreferenced, leased/safety-windowed, and reconciled later. |
| DB commits and API response is lost | Idempotency lookup returns the original IDs/revisions; no duplicate journal event. |
| Object later fails verification | The affected `ObjectReplica` becomes `CORRUPT` or `MISSING`; the `Object` becomes `QUARANTINED` only if no trustworthy replica remains. References stay known, reads fail safely, and recovery seeks another verified replica or backup. |
| Directory move races with descendant move | Transactional ancestry guard/serialization prevents a cycle; one operation retries or conflicts. |
| Cursor is outside retained history | Cursor is rejected; client obtains an authoritative paginated snapshot and new server checkpoint. |
| Backup input disappears | New snapshot records observation under policy; older snapshot references remain until retention expires. |
| Share is revoked while download is in progress | New authorization and range requests fail. Already delivered bytes cannot be recalled; implementation defines whether an active stream is terminated without claiming erasure. |
| AI or photo processing repeats | Idempotency key/source-version uniqueness replaces or returns the same derived record; originals are unchanged. |
| Forgejo identity is reused or renamed | Match the immutable provider repository ID, not display path; a destructive restore requires fresh provider identity and explicit confirmation. |

## Bounded open decisions

OPEN DECISION OD-DOM-001: filename comparison and normalization
Owner: Architecture / Storage / Sync
Needed by: Phase 1 schema and namespace contract gate
Options: byte-preserving names with case-sensitive NFC comparison key; platform-neutral case-insensitive comparison key; per-library comparison policy fixed at creation
Recommendation: preserve the original Unicode display name and use one immutable portable case-insensitive comparison key initially; keep the exact Unicode normalization/case-fold algorithm and version open until the fixture suite selects it
Decision evidence: cross-platform fixture suite covering Unicode normalization, case collisions, reserved names, and round-trip behavior

OPEN DECISION OD-DOM-002: initial multi-user ownership model
Owner: Architecture / Security / Product
Needed by: Phase 3 trusted-access and sharing schema gate
Options: user-owned libraries with node shares only; explicit owner plus library membership; first-class household/team organization
Recommendation: use an explicit owner plus membership model with no cross-owner deduplication, while deferring enterprise organizations; Phase 1 may begin owner-only but cannot encode assumptions that prevent membership
Decision evidence: product requirements and threat review for administration, offboarding, quotas, and shared ownership

OPEN DECISION OD-DOM-003: metadata ETag encoding
Owner: Architecture / API
Needed by: Phase 1 reviewed OpenAPI gate
Options: quoted opaque token from resource ID and revision; signed representation token; server-stored random version token
Recommendation: a quoted opaque encoding of resource kind, ID, and revision protected from client manufacture; never expose SQL transaction IDs
Decision evidence: conditional-request contract tests, proxy compatibility tests, and information-leak review

OPEN DECISION OD-DOM-004: active-stream behavior after authorization revocation
Owner: Security / API / Storage
Needed by: Phase 3 sharing gate
Options: terminate an active response when revocation is observed; authorize once per bounded response; short-lived signed internal read lease with byte/time cap
Recommendation: authorize each request and range request, cap stream duration, and document that already delivered bytes cannot be recalled; evaluate termination only if reliable across adapters
Decision evidence: threat review, streaming implementation test, and user-expectation review

## Client-local `SyncConflict`

`SyncConflict` is a durable client projection, not a server entity and not part
of the change journal. It identifies one `OutboundIntent` and records one of the
truthful categories `REMOTE_REVISION_CHANGED`, `REMOTE_CONTENT_CHANGED`,
`REMOTE_STATE_CHANGED`, `REMOTE_MISSING`, `NAME_COLLISION`, or
`PARENT_CHANGED_OR_UNAVAILABLE`. Optional node revision/state/parent and
journal-position fields are safe evidence captured at first detection; absence
means the canonical response did not establish that fact.

The lifecycle is `UNRESOLVED → RESOLVED`. Resolution is either
`ACCEPT_REMOTE`, with no replacement, or `RETRY_LOCAL_AGAINST_CURRENT_BASE`,
with one new linked `OutboundIntent`. The unique intent relation makes both
detection and lost-response resolution idempotent. The old intent and its
precondition remain historical evidence; no resolved row is recycled when a
replacement later conflicts.
