# Synveil testing, verification and benchmark strategy

Status: **Normative quality blueprint; Prompt 27 planning and Prompt 28 physical
GC validation are established, and the bounded Prompt 29 internal-worker gate
is implemented with focused live PostgreSQL/local-ObjectStore evidence**

Synveil tests for preservation of user data and authorization under failure,
not merely endpoint success. The storage, upload, version, synchronization,
backup and restore suites are release contracts. A feature cannot become
`IMPLEMENTED` while a required scenario is skipped, flaky, or represented only
by a mock that omits its real transaction/storage boundary.

This document defines test oracles, suites and phase evidence. The current
foundation gate runs Rust format/check/test/clippy plus `cargo deny check`,
strict web lint/typecheck/test/build, OpenAPI validation, and Python package
syntax/import smoke checks. The PostgreSQL integration test requires an
explicit disposable test database. The in-memory and production local
filesystem adapters now share a backend-neutral conformance suite, with local
managed-layout, corruption, incomplete-state, and practical symlink-containment
fixtures. Property/fuzz runners, Playwright, deterministic crash/power-loss and
disk-exhaustion evidence, Windows runner evidence, and isolated Docker Compose
recovery/upgrade jobs remain future phase evidence.

The current upload-session subset additionally has focused application tests for
exact-offset append, multi-frame streaming and aggregate chunk limits, restart
reconciliation, checksum failure, replacement revision conflict, and
exactly-once completion replay, plus local adapter tests for durable
append/finalize/promote across reopen. HTTP route tests cover authentication,
CSRF, strict headers/media type, owner concealment, offset conflict/recovery,
completion/abort retry, safe errors/request IDs, and the lost-response status/
resume flow. Typed browser-helper tests prove raw `Blob`/`ArrayBuffer` transport
and server-authoritative progress. These tests do not replace PostgreSQL
locking/integration evidence when the disposable database is unavailable.

The local upload capability regression opens the production filesystem adapter,
asserts the required availability, promotion, checksum, read-after-write, file
flush, directory flush, atomic-rename, and conditional-delete profile, and then
creates an upload session through `UploadApplicationService`. On Windows, the
adapter test additionally exercises the writable directory-handle flush used by
the capability probe and promotion path. The test remains enabled on every
native CI runner; a failed durability probe must keep uploads unavailable rather
than being converted into a skipped or relaxed test.

The transport-neutral content-read service additionally has focused application
tests for owner/missing/cross-owner concealment, directories, trashed nodes,
immutable historical versions, full and zero-byte streams, interior/final/
past-end ranges, empty/overflow range construction, missing/unverified
replicas, metadata length/SHA-256 mismatch, and abandoned-stream non-mutation.
One local `ObjectStore` integration test seeds a committed object and proves
one full read plus one range read through the same service. These fakes and the
local adapter validate the service boundary; they do not replace the
environment-gated PostgreSQL content-resolution test.

Focused HTTP download route tests additionally cover authentication without
CSRF, current and historical owner scoping, full `200` and exact single-range
`206` semantics, open-ended/suffix ranges, deterministic `416` handling before
storage open, strong `If-None-Match` `304` responses without streaming, safe
content headers/filename encoding, bounded multi-frame bodies, and the real
content-read service over the local object store. These tests do not claim
PostgreSQL end-to-end content resolution when the disposable database gate is
not configured.

The version-history metadata boundary additionally requires focused tests for
newest-first `(committed_at DESC, id DESC)` ordering, same-timestamp tie
breaking, bounded node-scoped cursor continuation and tamper rejection,
currentness from `Node.current_version_id`, owner/cross-owner concealment,
trashed/purging behavior, directory state handling, direct lookup compatibility
with historical download IDs, safe DTO field allowlisting, private no-store
responses, and the absence of ObjectStore reads. The PostgreSQL integration
gate must exercise the real owner/library/node/version/object joins and report
its status explicitly; an unset `SYNVEIL_TEST_DATABASE_URL` does not certify
end-to-end PostgreSQL coverage.

The safe version-restore boundary additionally has focused tests for
authentication, CSRF, required `If-Match`, bounded idempotency keys, stale
revision conflict details, owner/cross-node concealment, directory/trashed and
current-version rejection, safe response allowlisting, one-new-version retry
replay, and idempotency-key conflict. The ignored PostgreSQL restore test
exercises the real transaction path: verified-replica selection, same-object
reuse, pre-restore parent linkage, node-pointer advancement, historical-row
immutability, no duplicate on replay, stale failure without mutation, current
version rejection, and missing-verified-replica failure. It is not evidence
until a fresh disposable PostgreSQL URL is supplied.

The Trash-retention policy has focused unit coverage for the 30-day default,
configuration validation, derived deadline, inclusive boundary, server-time
eligibility, active/restored/PURGING/root exclusion, and timestamp clearing on
restore. The API metadata test checks additive `trashed_at`,
`restore_deadline`, and `purge_eligible` fields. The ignored PostgreSQL
retention test exercises owner concealment, empty-directory behavior, stable
bounded keyset continuation, restore-after-deadline, before-deadline rejection,
metadata-only `PURGING` begin, repeated-begin revision semantics,
restore-vs-purge locking, and preservation of version/object/replica rows. It
is not PostgreSQL evidence until `SYNVEIL_TEST_DATABASE_URL` identifies a fresh
disposable database.

The metadata-purge PostgreSQL test is also ignored without that gate. It covers
the internal-only boundary for active/trashed/PURGING, root, parent, and child
preconditions; stale revision rejection; same-revision replay; concurrent
duplicate workers; atomic removal of a node and all of its FileVersion rows;
current-version-pointer and restore-operation cleanup; shared-object reference
accounting, including a restore-created reference; preservation of
Object/ObjectReplica rows; candidate clearing when a new FileVersion
re-references an object; upload-parent FK deferral; rollback after a late
completion-record failure; and staging/zero-byte cases. It is
not PostgreSQL evidence until `SYNVEIL_TEST_DATABASE_URL` identifies a fresh
disposable database.

## Quality principles

1. **Invariants before examples.** Each example has a state/authorization/
   durability oracle; “HTTP 200” is not enough.
2. **Real dependencies at boundaries.** PostgreSQL transaction/locking tests use
   supported PostgreSQL, and production adapter gates use the real filesystem/
   S3-compatible implementation.
3. **Deterministic failure injection.** Every unsafe gap between object I/O,
   database commit, response and background work is an addressable test point.
4. **Retry is normal.** Requests, change pages, webhooks and jobs may repeat;
   tests prove idempotent outcomes rather than assuming once-only delivery.
5. **Crash recovery is a feature.** Kill/restart at a named state and assert the
   recovered durable state, not the in-memory state before death.
6. **Adversarial data is ordinary input.** Names, sizes, cursors, MIME types,
   media, archives, repository content and timestamps are untrusted.
7. **Optional means removable.** Core suites run with AI, thumbnailing and
   Forgejo absent, down and slow.
8. **Restore proves backup.** A stored manifest or green backup job is
   insufficient until a clean restore verifies bytes and reports scope.
9. **Compatibility is durable.** Released migrations, objects, cursors/fixtures
   and supported clients remain in an upgrade/conformance matrix.
10. **Adoption is evidence.** A non-technical install, storage choice, pairing,
    health message, update, uninstall, migration, and recovery path is tested
    as a product workflow, not inferred from an operator Compose runbook.
11. **Reproducible evidence.** Record seed, commit/image digest, config,
    dependency versions, hardware/backend, fixture and exact failures/skips.

## Test layers

| Layer | Purpose | Examples | Release use |
|---|---|---|---|
| Static/contract | Reject dependency, type, schema, secret and docs drift before runtime | Rust format/lint/deny, strict TypeScript, OpenAPI diff, migration checksum, dependency/license/SBOM, docs links/parity | Every change |
| Unit | Exercise pure functions/state transitions quickly | name key, state machines, policy, range parsing, errors, retention selection | Every change |
| Property/model | Generate operation sequences and assert invariants | sync model, upload parts, directory graph, GC references, backup retention | Every change for bounded seeds; extended nightly |
| Fuzz | Find parser/decoder/panic/resource-bound defects | cursor, path/name, headers, manifests, codecs, webhooks, media wrappers | Corpus regression on PR; time-budgeted nightly/release |
| Integration | Exercise real PostgreSQL and real adapter semantics | transaction/locks, upload lifecycle, outbox leases, local/S3 conformance | Every relevant change |
| Crash/recovery | Terminate at deterministic failpoints and restart | object/DB split, migration, job, snapshot/restore, GC | Required phase/nightly/release |
| Protocol conformance | Hold server and clients to versioned fixtures | sync cursor/pages/conflicts, backup manifests, upload retry | Required before protocol/client promotion |
| Security | Negative authorization, abuse, secret/egress and parser isolation | IDOR, CSRF/XSS/SSRF, token replay, bombs, no-egress | Every boundary change; full release suite |
| End-to-end | Prove integrated user paths | browser/API/DB/object, login/upload/trash/restore/share/backup | Merge/release; critical smoke on PR |
| Platform/distribution | Prove host, installer, service, storage, connectivity, accessibility, update, uninstall and migration behavior | Windows/macOS/Linux host matrix, storage picker/capability probes, pairing, service crash/reboot, signed update, data-preserving uninstall/migration | Cross-platform foundation and every claimed host release |
| Deployment/upgrade | Prove clean install, health, backup, restore and migration | Compose, previous-version fixture, disaster restore | Release candidate |
| Performance/soak | Establish capacity envelope and detect regressions/leaks | streams, listings, backlog, checksum, queue, restore | Dedicated environment; release gate |

Mocks/fakes are useful for pure application logic and deterministic failures.
They do not certify PostgreSQL isolation, local fsync/rename/symlink behavior,
S3 read-after-write/checksum behavior, Caddy streaming, container privileges or
real browser/platform semantics.

## Test architecture and harnesses

### Required harness components

- **Ephemeral PostgreSQL:** one isolated database/schema per test group, with
  the exact supported server/extension version and real migration path.
- **Temporary local object root:** created under a dedicated test directory,
  never a workspace/home/root path; capable of controlled permissions, space/
  inode exhaustion, bit flip, missing file and symlink substitution tests.
- **Fault-injecting `ObjectStore`:** a contract-faithful adapter decorator that
  fails or crashes before/after a named operation without inventing stronger
  semantics than the real adapter.
- **Deterministic clock/random/ID ports:** allow expiry, retention, UUIDv7 and
  race tests while production uses secure/random/system providers. IDs are not
  used as order or authorization in assertions.
- **Transaction failpoints:** test-only hooks around lock/allocation/insert/
  commit/response boundaries. Never expose failpoints in production endpoints.
- **Reference sync model/client:** an intentionally small state machine that
  consumes versioned fixtures independently from the server implementation.
- **Virtual backup source:** stable file identity plus controlled content,
  rename/delete/unreadable/symlink/change-during-scan and platform metadata.
- **Network fault proxy:** drop, duplicate, delay, truncate and reorder client,
  object backend, AI and Forgejo traffic under bounded reproducible scripts.
- **Compose release lab:** clean install, private-port scan, proxy/TLS, process/
  host restart, system backup/restore and supported-version upgrades. Compose is
  the Advanced / Server profile, not a substitute for Personal / Home evidence.
- **Real-OS release lab:** Windows, macOS, Linux Desktop, and Linux Server
  runners or explicitly documented hardware labs for native/guided install,
  storage discovery/picker, service lifecycle, sleep/reboot/crash recovery,
  signed update, uninstall-with-data-preservation, managed PostgreSQL, and
  machine migration. A Linux Compose pass cannot represent the other profiles.
- **Filesystem capability probe lab:** NTFS/ReFS, APFS, ext4, XFS, Btrfs and
  named NAS/object-store profiles record observed `StorageCapabilities`; tests
  verify portable fallback when optional acceleration is unavailable.
- **Accessibility/onboarding harness:** keyboard/screen reader/contrast checks,
  plain-language error-to-action assertions, progressive-disclosure snapshots,
  pairing-code lifecycle and remote-connectivity states.
- **Artifact/privacy probes:** canary secrets/content/names followed by log,
  metric, trace, error, diagnostic, remote-network and image-layer scans.

All destructive fault tests resolve and validate their isolated temporary
targets before mutation. They never use a real user storage root.

### Required state inspection

Tests need safe read-only helpers that can assert:

- visible library tree, current versions and immutable history;
- object/replica state, protected references, leases and stored/plain checksums;
- upload session/part and persisted idempotency outcome;
- library epoch/head and ordered change events;
- backup snapshot/entry and restore operation results;
- jobs/outbox attempts/leases/dead letters and audit facts;
- derived record source version/freshness/provenance;
- storage namespace inventory and orphan/quarantine candidates.

Helpers are test/internal tooling, not unauthenticated production debug APIs.
Assertions prefer public behavior plus invariant audit; direct database reads
explain failures and verify atomicity but must not normalize a broken API.

## Universal domain invariants

Every mutation/fault suite asserts the applicable invariants:

1. No visible `FileVersion` references an object that is partial, missing,
   unverified or only temporary.
2. Canonical bytes, length and `sha256:` hash never change for an immutable
   object/version/snapshot entry.
3. Metadata mutation, `ChangeEvent`, required `AuditEvent`, outbox work and
   persisted idempotency outcome are atomically present or absent.
4. Same idempotency identity plus same fingerprint returns one stored outcome;
   a different fingerprint is rejected and changes no state.
5. Authorization derives from current principal/resource relationship; an ID,
   storage key, hash, cursor or stale index record grants nothing.
6. Retry/crash may leave staging/orphan work but never a second logical outcome
   or an inaccessible committed reference.
7. Restore creates a new current state/explicit output and does not rewrite
   immutable history.
8. Garbage collection never deletes a referenced or leased object; uncertain
   state is retained/quarantined, not guessed away.
9. Optional worker/provider/connector failure changes only derived/integration
   freshness, not core correctness.
10. Logs/errors/metrics/traces/diagnostics contain no raw secret or fixture
    content and do not reveal cross-user existence.

## Static and contract validation

Every pull request runs or records the applicable checks:

- Rust formatting, lint with reviewed warning policy, unit/integration tests,
  unsafe-code policy and dependency/license/advisory analysis;
- strict TypeScript typecheck, lint, unit/component tests, production Vite build
  and generated API drift check;
- Python format/type/test/dependency/model-license checks for optional AI paths;
- OpenAPI syntax/lint, stable error/idempotency/auth annotations and intentional
  compatibility diff review;
- migration ordering, immutable released checksum, fresh apply and upgrade
  apply; schema model drift where used;
- container/config/Compose/Caddy validation, secret scan, SBOM and image
  vulnerability policy;
- Markdown links, Mermaid parsing where supported, terminology/status rules and
  required English/Vietnamese document pair presence;
- no empty aspirational client/service scaffolding or feature status promotion
  without its gate evidence.

Generated files declare their source and regeneration command. A test fails if
hand edits or stale generation are detected.

## Unit-test catalogue

Minimum pure/unit coverage includes:

- UUIDv7 parse/canonical serialization while proving no order/authorization
  behavior depends on timestamp bits;
- portable Unicode normalization/name key, forbidden characters, bounded
  length, reserved names, sibling uniqueness and deterministic conflict names;
- revision/ETag and `If-Match` parsing, stable error mapping and safe error
  redaction;
- byte range parsing, overflow, unsatisfiable and multipart/range policy;
- auth/session/device/share/recovery state, expiry, rotation lineage, scope and
  authorization policy decisions;
- `UploadSession`/part transition table and invalid transitions;
- directory ancestry/cycle detection, move/rename/copy/delete/restore policy;
- conflict classification and deterministic preservation metadata;
- cursor encode/decode/version/integrity/scope/epoch checks;
- backup snapshot/entry/restore transition table, retention selection and hold;
- object reference/lease/GC eligibility and storage accounting;
- job attempt/backoff/jitter/lease-generation/dead-letter/idempotency;
- compression eligibility/metadata and bounded decode policy;
- AI/photo/Git provenance, freshness and optional-degradation state.

State-transition tests enumerate every state and ensure undefined transitions
fail without mutation.

## Prompt 31 durable change-journal validation

The durable journal foundation has an explicit live-PostgreSQL gate in
`crates/metadata/tests/postgres.rs`. On a fresh disposable database, the
enabled ignored suite proves migration application, typed schema round-trip,
one event for each supported logical mutation, upload and version-restore
idempotent replay, purge tombstone survival after Node deletion, owner/library
scope, bounded ordered paging, cursor resume, reader/write interleaving,
concurrent writers, a late journal failure rollback, and database-enforced
append-only history. The upload finalization test also runs two concurrent
completion calls and asserts one stored completion and one journal fact.

The foundation-specific unit tests cover canonical change vocabulary, cursor
version/length/integrity/overflow handling, PostgreSQL `BIGINT` boundaries, and
separation from the file-history and purge cursor namespaces. The live suite is
not run against a persistent development database. A representative explicit
run is:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://<disposable-loopback-db> \
  cargo test -p synveil-metadata --test postgres --locked -- \
  --ignored --test-threads=1
```

This validates the journal foundation only. The Prompt 32 checkpoint/feed,
Prompt 33 server-side rebaseline, and Prompt 34 client-mutation tests are
documented below. Client-side staged installation and automatic conflict
resolution remain future sync-phase tests.

## Prompt 35 synchronization status and validation

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

The Prompt 32 live PostgreSQL test uses a fresh disposable database and covers
checkpoint creation/uniqueness, concurrent creation, owner and library
concealment, deterministic epoch/zero initialization, bounded ascending feed
pages, stable repeat reads before acknowledgment, signed delivery evidence,
monotonic compare-and-set acknowledgment, same-page and older-token replay,
gap/out-of-order and future-progress rejection, independent devices and
libraries, concurrent readers, concurrent acknowledgments, a journal writer
overlapping a feed read, retained-history expiry, epoch rollover, and revoked
device denial. API tests cover session auth, CSRF asymmetry, private/no-store
headers, body/limit bounds, safe errors, scope concealment, logical-only DTOs,
and token non-disclosure. The required live command is:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-metadata --test postgres --locked -- \
  --ignored --test-threads=1
```

The Prompt 34 API and metadata suites implement and test the authenticated
logical mutation boundary. A local watcher, staged desktop agent, conflict
auto-resolver, WebSocket/SSE, and broker remain outside this phase.

## Prompt 33 logical snapshot/rebaseline validation

Unit coverage proves the closed bootstrap state vocabulary; typed logical
projection invariants; canonical root/directory/file shapes; cursor round trip,
scope binding, tamper and oversize rejection; completion-token round trip and
owner/device/library/session/generation claim binding; distinct cursor/token
HMAC domains; empty-manifest terminal encoding; and redacted secret `Debug`.
API coverage exercises start and complete authentication plus CSRF, GET auth
without CSRF, 2 KiB body bounds, strict JSON fields, page-limit bounds,
malformed/tampered/oversized cursor and completion token, cross-owner
concealment, revoked/unknown device denial, private/no-store success and error
responses, deterministic page/retry mapping, terminal proof, exact checkpoint
result, response-loss replay, and a DTO allowlist that contains no physical
storage field.

The ignored live PostgreSQL test
`postgres_sync_rebaseline_bootstrap_is_coherent_bounded_and_fenced` is required
on a fresh disposable database. It applies the Prompt 33 migration, verifies
the scoped state/manifest schema and absence of physical columns, and then
proves:

- root-only/empty-user-content completion at sequence zero; inclusion of
  current `ACTIVE` and `TRASHED` Nodes; exclusion of `PURGING`/purged Nodes; and
  safe current version/length/hash projection;
- one coherent epoch/resume cut captured with an immutable manifest and stable
  Node-ID keyset ordering, bounded `limit + 1` page reads, deterministic first
  and terminal retries, service restart continuation, and byte-for-byte
  unchanged checkpoints throughout page reads;
- real concurrent directory create, rename, move, Trash, metadata purge,
  verified upload finalization/content replacement, and version restore. Each
  assertion requires the committed outcome in the captured projection exactly
  when its journal event is at/before the cut; otherwise the event must be
  strictly post-cut;
- a multi-page snapshot followed after page one by concurrent rename, move,
  Trash, create, and content finalization still returns only the captured old
  projection. Post-cut feed events reconcile those changes and every returned
  sequence is strictly greater than the snapshot resume sequence;
- completion rejects non-terminal evidence without changing the checkpoint,
  then atomically replaces epoch/acknowledged sequence exactly at the cut;
  committed response-loss replay is idempotent;
- an already-ahead checkpoint, expired/replaced generation, epoch rotation, or
  minimum-retained-sequence invalidation fails closed without rewind; and
  concurrent starts return the same one OPEN bootstrap;
- bounded retired-session cleanup removes only bootstrap/manifest state and
  preserves the canonical root and journal facts.

The exact focused live invocations are:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-metadata --test postgres --locked -- \
  postgres_change_journal_is_ordered_scoped_resumable_and_atomic \
  --ignored --exact --test-threads=1
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-metadata --test postgres --locked -- \
  postgres_device_sync_checkpoints_and_feed_are_bounded_monotonic_and_scoped \
  --ignored --exact --test-threads=1
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-metadata --test postgres --locked -- \
  postgres_sync_rebaseline_bootstrap_is_coherent_bounded_and_fenced \
  --ignored --exact --test-threads=1
```

Passing a fake backend or leaving the ignored test unexecuted is not Prompt 33
PostgreSQL/no-gap evidence. The final gate also reruns upload finalization,
version restore, Trash/restore, metadata purge, journal atomicity, sync
feed/ack, GC planning/execution/worker, local ObjectStore conformance, and
available Windows upload-durability regression coverage.

## Prompt 34 client mutation submission validation

Unit coverage in `synveil-core` and `synveil-metadata` proves the closed
mutation vocabulary, canonical UUIDv7 mutation identity, typed payload
construction, decimal epoch/sequence boundaries, deterministic versioned
SHA-256 fingerprinting, conflict-reason parsing, exact logical result replay,
and the explicit replay marker. No arbitrary JSON patch, byte payload, object
identity, or physical storage field is part of the contract.

API coverage proves strict JSON field allowlists for the envelope and every
typed payload, exact kind parsing, authentication and session-bound CSRF,
16 KiB request-body rejection, private/no-store success and error responses,
owner/device/library concealment, safe logical applied results, safe conflict
details, mutation-ID conflict mapping, and rebaseline-required mapping.

The live PostgreSQL tests
`postgres_client_mutations_are_atomic_idempotent_scoped_and_bootstrap_safe`
and `postgres_client_mutations_are_race_safe_for_duplicate_and_stale_pairs`
run on a fresh disposable database and prove:

- all five supported namespace/state mutations commit one canonical Node
  change, one journal event, and one terminal operation row atomically;
- retry after a lost response returns the exact persisted Node/event or
  conflict with `replayed: true`, while the originating device checkpoint is
  unchanged and other devices can read the event from the feed;
- stale revision, changed parent/name, future base sequence, wrong epoch,
  retained-history expiry, and purged-resource cases fail deterministically;
- same-ID/different-payload reuse is rejected, cross-owner/library/device
  access is concealed, and transient rollback leaves no operation or journal
  fact;
- concurrent duplicate submissions, rename-vs-rename, move-vs-move,
  Trash-vs-rename, same-name directory creation, and two-device stale writes
  are serialized by the library namespace guard without deadlock or duplicate
  canonical facts; and
- a bootstrap cut followed by a client mutation hands the post-cut event to
  the feed while the immutable bootstrap manifest remains unchanged.

The exact live command is:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-metadata --test postgres --locked -- \
  --ignored --test-threads=1
```

The focused tests use PostgreSQL row locks, foreign-key scope, the
transaction-scoped per-library advisory guard, and the real journal/feed,
rather than a fake backend. Automatic conflict resolution, local staged
application, content-byte mutations, and desktop sync remain unimplemented.

## Prompt 35 durable conflict and manual-resolution validation

Unit coverage proves typed UUIDv7 conflict/resolution parsing, closed lifecycle
and action vocabularies, canonical typed SHA-256 fingerprint stability and
semantic-change detection, bounded page limits, strict deny-unknown-fields
resolution DTOs, and owner/device/library-bound cursor round trip, oversize,
scope, and tamper rejection. API coverage proves list/detail authentication,
GET without CSRF, POST with session-bound CSRF, 16 KiB body rejection,
private/no-store successes and errors, cross-scope/revoked-Device concealment,
stable stale and terminal error envelopes, exact replay projection, and DTO
allowlists with no physical-storage field.

The fresh disposable PostgreSQL tests
`postgres_sync_conflicts_are_durable_inspectable_and_manually_resolvable` and
`postgres_sync_conflict_resolution_is_fenced_race_safe_and_replayable` prove:

- a managed terminal Prompt 34 conflict and its operation row are linked
  one-to-one in the same transaction, original replay returns the same conflict
  ID, managed reasons persist, and non-managed failures create no conflict;
- immutable historical evidence, bounded OPEN keyset pages, stable descending
  order, detail lookup, owner/Device/Library concealment, revoked-Device denial,
  and survival through rebaseline plus Node purge;
- `ACCEPT_SERVER` moves OPEN to DISMISSED with no Node or journal change, is
  exactly replayable by ID/fingerprint, and fences a different second decision;
- `APPLY_CLIENT_INTENT` requires mutation-appropriate fresh current revisions,
  reuses the real Prompt 34 executor, commits exactly one Node change/event and
  RESOLVED linkage, leaves the original operation CONFLICT, and replays the
  original timestamp/event after response loss;
- stale revision, rename, move, and purge winners persist an explicit stale
  result, leave the conflict OPEN, and produce zero resolution mutation/event;
- APPLY/APPLY and ACCEPT/APPLY races permit at most one terminal decision and
  at most one resource event, while original mutation replay during resolution
  cannot duplicate the conflict or mutate canonical state; and
- successful apply is visible to another Device through the Prompt 32 feed,
  the originating checkpoint is byte-for-byte unchanged, accept-server emits
  no feed event, and a post-rebaseline fresh apply succeeds or safely conflicts.

The Prompt 35 tests use real PostgreSQL advisory/row locks with per-race
10-second timeouts and assert no deadlock. They also inspect the migration
schema for forbidden physical columns and attempt a direct evidence rewrite to
prove the database trigger rejects it. The focused commands are:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-metadata --test postgres --locked -- \
  postgres_sync_conflicts_are_durable_inspectable_and_manually_resolvable \
  --ignored --exact --test-threads=1
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-metadata --test postgres --locked -- \
  postgres_sync_conflict_resolution_is_fenced_race_safe_and_replayable \
  --ignored --exact --test-threads=1
```

The final gate also runs every ignored metadata PostgreSQL test sequentially on
one fresh database, authentication and storage-GC ignored suites, the complete
non-live workspace, strict Clippy, cargo-deny, OpenAPI lint, diff checks, and a
forbidden-scope/security-field scan. Automatic conflict resolution and the
desktop sync agent remain unimplemented.

## PostgreSQL integration tests

Use the supported PostgreSQL version, not SQLite or a simplistic repository
mock, to prove:

- foreign/unique/check constraints and ownership boundaries;
- transaction isolation, lock acquisition order and bounded retry/deadlock;
- per-library clock allocation commits in visibility order and rollback does
  not publish/advance an unusable cursor position;
- concurrent name/move/cycle and content-completion winners;
- metadata/journal/audit/outbox/idempotency atomicity;
- job claim with skip-locked/lease generation, expiry, renewal and conditional
  completion across worker replicas;
- cursor/snapshot keyset pagination while rows are inserted/changed;
- retention/GC/reference queries over every protected relation;
- migration advisory lock, transactional/nontransactional interruption and
  immutable checksum;
- runtime role cannot migrate, read unrelated secret material or bypass row/
  application authorization assumptions.

Each concurrency test uses barriers to force the dangerous interleaving rather
than relying on timing luck.

## `ObjectStore` adapter conformance

Every production adapter—local, NAS profile, S3/MinIO and future—passes one
versioned suite:

The checked-in v1 shared helper currently covers staged identity, streamed
integrity checks, pre-promotion invisibility, create-only promotion/conflict,
zero/large objects, full/edge/invalid ranges, metadata, exists, abort, ordinary
and conditional delete, and repeat deletion for both the in-memory and local
adapters. Local integration tests add managed-layout, incomplete-state,
corruption, marker, and practical symlink/reparse containment fixtures. The
broader matrix below remains the release contract; crash injection, resource
exhaustion, concurrent-process races, cancellation/backpressure, and platform
labs are not claimed by the current unit/integration pass.

| Area | Cases and oracle |
|---|---|
| Staged create | Exclusive generated locator; zero/one/large streams; short/long declaration; cancel; writer crash; no committed visibility. |
| Finalize | Expected length/checksum; duplicate same finalize; concurrent finalize; adapter capability path; success is read-after-write verified. |
| Immutable read | Full and valid/invalid/edge ranges; concurrent readers; cancellation; exact bytes and headers; no partial served as valid. |
| Head/metadata | Stable length/checksum/encoding/capability result; missing/permission/network distinctions map safely. |
| Abort/cleanup | Repeated abort; abort during failed/expired upload; never deletes committed/shared data. |
| Delete | Server-controlled exact key; repeated/missing delete; lease/reference gate; no prefix/glob/symlink traversal. |
| Corruption | Truncate, bit-flip, metadata mismatch, missing object; quarantine/fail, no silent content. |
| Capacity | Disk/inode/quota exhaustion at partial write/finalize; reserved recovery headroom and no visible version. |
| Retry/network | Timeout before/after remote completion, duplicate part, lost response, reconnect; deterministic outcome. |
| Enumeration | Listing is reconciliation aid only; eventual/duplicate/missing listing cannot authorize deletion without DB references/grace. |
| Durability | Process/host crash at write/flush/promote/directory-sync boundaries under the declared profile. |
| Migration | Copy, verify, switch, rollback window, interrupted copy, source/target outage and later retirement. |

The S3 suite never treats ETag as canonical SHA-256 unless the adapter has
explicitly established equivalent semantics for that exact operation. The
local suite performs symlink/junction/TOCTOU/root-identity tests.

## Upload, version, trash and GC tests

### Upload lifecycle matrix

| Scenario | Steps | Required result |
|---|---|---|
| Initiate replay | Send identical initiation twice with one idempotency key | One `UploadSession`; same negotiation/outcome returned. |
| Initiate key mismatch | Reuse key with different target/size/hash | Stable idempotency conflict; no second session or target mutation. |
| Out-of-order parts | Upload negotiated part numbers/ranges in shuffled order | Each independently verified; completion succeeds only when coverage is exact. |
| Duplicate same part | Lose part response and resend identical bytes/fingerprint | Stored verified part returned; staged bytes/accounting not duplicated. |
| Duplicate different part | Reuse part identity with one changed byte | Conflict/checksum error; original verified part and session remain uncorrupted. |
| Gap/overlap/overflow | Submit missing, overlapping, negative/overflow or too many ranges | Rejected before unsafe assembly/allocation; no visible file. |
| Expiry race | Completion and expiry worker race under a barrier | Exactly one terminal state; committed outcome remains committed, expired data cannot commit. |
| Concurrent completion | Two processes complete one session | One serialized object/version/event/outcome; loser returns same result or stable in-progress retry. |
| Checksum mismatch | All parts arrive but canonical length/SHA differs | No visible version; session terminal/retry state follows contract; bad bytes quarantined/cleanable. |
| Disk full mid-part | Exhaust capacity after partial write | Bounded failure; verified earlier parts safe; no unverified part/visible version. |
| Disk full at promotion | Complete assembly then fail final durability | No DB reference; resumable/failure state explicit; recovery reserve intact. |
| Object final, DB rollback | Fail before/during metadata transaction | Object unauthorized/unreferenced; no node/version/event/outcome; orphan protected then reconciled. |
| DB commit, response loss | Kill connection/process after commit before response | Retry returns exact committed node/version/ETag; no duplicate object reference/event/audit/outbox. |
| Restart during verify | Kill process while hashing/assembling | Session recovers to retryable/failed state; no partial visible metadata; lease expires safely. |
| Aborted/expired cleanup | Run cleanup repeatedly and concurrently | Staging reclaimed once; committed/reference bytes untouched; audit/metrics stable. |
| Quota race | Two uploads each fit alone but exceed quota together | Transaction/lock admits only allowed result; no quota bypass or stranded visible reference. |

### Versions, trash and restore

- Content replacement creates a new immutable `FileVersion`; old hash/bytes and
  attribution remain unchanged.
- Rename/move never creates/rewrites object bytes. Copy has explicit logical
  identity/version/reference behavior and authorization.
- Restore old version creates a new head linked to the selected source; retry is
  one outcome; current conflicting mutation uses a base precondition.
- Soft-delete creates the defined tombstone/trash state and journal fact;
  recursive work is bounded/restartable. Restore handles original-parent/name
  collision deterministically.
- Version/trash retention selects only eligible references. A concurrent share,
  backup snapshot, restore, download lease, legal hold or migration blocks
  deletion.
- GC mark and sweep are separated by a grace/checkpoint. New references during
  either phase cannot point to a deleted object. Sweep is idempotent and uses
  exact validated keys.
- A dry-run reports candidates/reasons without mutation. An invariant audit
  compares database references, replicas and storage inventory while treating
  eventual listing conservatively.

### Prompt 27 metadata-only GC planning matrix

The PostgreSQL GC-planning integration gate covers the implemented boundary:

| Scenario | Required evidence |
|---|---|
| Policy/defaults | 24-hour default grace, 15-minute default lease, default batch 100, hard batch maximum 500, and rejection of zero/invalid durations or batch sizes. |
| Grace boundary | A candidate before grace is not claimable; the exact `now >= unreferenced_at + grace` boundary and an older candidate are claimable using PostgreSQL time. |
| Bounded stable claim | `FOR UPDATE SKIP LOCKED` claims no more than the configured batch in `(unreferenced_at, object_id, dedup_domain_id)` order. |
| Reference truth | A candidate with a committed `FileVersion` is cancelled by the exact `(object_id, object_dedup_domain_id)` `NOT EXISTS` recheck and cannot become leased/ready. |
| Lease fencing | Non-expired claims block; expiry permits reclaim; generation increments; the prior opaque ID/generation cannot renew, release, or revalidate the successor. |
| Renewal/release | A matching live lease renews; release is safe and idempotent; stale, expired, and already-cancelled outcomes are explicit. |
| Re-reference cancellation | A new reference clears `ELIGIBLE`, `LEASED`, and `READY` candidates, and a re-reference followed by purge creates a fresh lifecycle. |
| Ready planning | `READY` is metadata-only, revalidation is repeatable, and a reference after ready removes the row before any physical action. |
| Crash/reconnect | A committed claim survives connection loss, blocks another worker until expiry, and can be reclaimed by a newly connected worker; ready state remains revocable. |
| Real races | Concurrent claim/claim, claim/reference, and ready/reference operations leave disjoint leases and no candidate with a committed reference. |
| Preservation | Object and ObjectReplica row counts and representative rows remain unchanged; no ObjectStore delete or byte deletion is invoked. |

### Prompt 28 physical-GC execution matrix

The disposable-PostgreSQL plus production-local-ObjectStore gate runs only
against a caller-declared fresh database and a unique temporary managed object
root. It proves the following implemented behavior without a public API:

| Scenario | Required evidence |
|---|---|
| Entry and durable plan | `READY` plus a live matching lease is required; operation and deterministic action rows exist before storage effects. |
| Final fence | Candidate, Object lifecycle, lease/generation, zero FileVersion references, and zero active holds are checked in short transactions before each delete and completion. |
| Replica handling | Exactly one verified replica is deleted per call with key/hash/length/version evidence; metadata survives until absence is confirmed. |
| Local deletion | The production local adapter conditionally deletes by opaque version through a managed rename-to-tombstone workflow; interrupted tombstones reconcile as `InProgress`, not absent. |
| Ambiguity and absence | Lost responses reconcile to exact-key absence or retryable presence; already-absent replicas converge safely; evidence mismatch fails closed. |
| Recovery | Crash before persistence, after one of many replicas, after lease expiry/reclaim, reconnect, and after final physical absence all resume deterministically. |
| Concurrency | GC worker races converge on one operation; lifecycle triggers prevent a new FileVersion, restore-equivalent reference, active hold, or replica writer from making deleted content usable. |
| Completion | All actions and ObjectReplica rows must be absent before candidate/Object removal; completed operation replay is deterministic. |

The physical service never reads full object bytes to decide deletion, and the
test verifies that its reconciliation path makes zero `get` calls. Scheduling,
rate controls, and bounded durable-operation reconciliation are covered by the
Prompt 29 gate below; storage inventory reconciliation, backup/share/sync hold
producers, and public GC APIs are not part of either implemented gate.

### Prompt 29 internal GC-worker orchestration matrix

The worker gate uses `run_once()` directly for deterministic cycle tests. Live
tests require a caller-declared fresh disposable PostgreSQL database and a
unique temporary managed local ObjectStore root; no user database or storage
path is valid test input.

| Scenario | Required evidence |
|---|---|
| Configuration/disabled mode | The worker defaults disabled; zero/invalid durations, contradictory concurrency, invalid bounds, and invalid retry policy are rejected before work. A disabled `run_once()` is cheap and has no metadata effects. |
| Bounded cycle | Candidate claims, active operation slices, replica actions, task concurrency, and storage-delete concurrency respect configured caps. No library loop sleeps or repeats unboundedly. |
| Recovery priority | Due incomplete operations, including expired/released leases and incomplete terminal cleanup, are claimed oldest-first before new work. A recovery claim defers new destructive claims to the following cycle. |
| Durable retry | A transient/ambiguous local-store outcome increments `attempt_count`, persists a PostgreSQL-clock `next_attempt_at`, releases the slice safely, and is not retried before due time. Delay is capped exponential with deterministic bounded jitter. |
| Intervention | Evidence mismatch, unsafe durable state, unsupported routing, and an exhausted retry budget become fenced `NEEDS_ATTENTION`; they do not hot-loop or report completion. |
| Reconciliation | Bounded set-based discovery reports `GC_DELETING` without an operation, operation without candidate, expired `READY` without incomplete work, terminal action cleanup pending, and intervention state. A re-reference cancels unsafe progress through the existing fences. |
| Unknown physical files | The worker does not recursively list a storage root and does not delete unknown physical files. Missing bytes outside an active action remain integrity/reconciliation findings, not Object metadata deletion. |
| Outage and shutdown | Database failure advances no new destructive work; ObjectStore failure persists recovery rather than false cleanup. Shutdown stops new claims, drains only the configured bounded cycle, and leaves timed-out fenced work for restart reconciliation. |
| Multi-worker race | Concurrent process-equivalent workers use PostgreSQL `SKIP LOCKED`, candidate lease generations, and Prompt 28 fences to converge on one durable physical operation without leader election. |
| Real integration | A worker-driven `READY` candidate passes through Prompt 28 and the production local adapter to exact physical absence and final metadata completion. |

## Synchronization test contract

Synchronization receives unusually deep testing because an apparently small
ordering/retry defect can silently lose changes across every device.

The implemented Prompt 31 scope is the server-side durable journal foundation:
transaction-local publication, typed projections, per-library ordering,
bounded cursor reads, and rollback/idempotency/concurrency evidence. Prompt 32
adds the authenticated one-way server feed, durable per-device/per-library
checkpoints, signed acknowledgment evidence, and explicit rebaseline-required
outcomes. Prompt 33 adds the server-side immutable logical manifest, coherent
cut, bounded retryable pages, terminal proof, and exact checkpoint handoff.
Prompt 34 adds authenticated typed logical mutation submission, durable
mutation identity/fingerprint persistence, optimistic preconditions, and
deterministic conflict outcomes. Client-side staged installation, arbitrary
content-byte mutation, and automatic conflict resolution remain planned.

### Sync invariants

For one `Library` and journal epoch:

1. Every committed client-visible namespace/content mutation emits its defined
   `ChangeEvent` projection in the same transaction. A rolled-back mutation
   emits none.
2. Event sequence is unique and reflects commit visibility. Once the server
   issues a cursor through sequence `n`, a client will not later discover a
   previously hidden committed event at `≤ n`.
3. A page is ordered and may repeat already delivered events; it never silently
   skips an event between the supplied and returned cursor.
4. A cursor is authenticated, versioned, bound to authorized user/library and
   epoch, and opaque to the client. UUIDv7/time is never fallback ordering.
5. A client persists the returned cursor only after applying the complete page
   atomically to durable local state. Crash before that repeats safely.
6. Initial snapshot/rebaseline plus its server checkpoint converges under
   concurrent writes: every node state is represented by the snapshot or a
   later event, never omitted between them.
7. The same `client_mutation_id`/fingerprint produces one mutation/event.
   Different payload reuse is rejected.
8. The Prompt 34 mutation boundary accepts only typed logical namespace/state
   operations; it never accepts file bytes, object/replica identities, or
   storage locators. A future content protocol must preserve incoming and
   current bytes without last-writer-wins loss.
9. Failed mutation preconditions return the persisted expected/current logical
   conflict projection and do not silently overwrite, merge, or create a
   conflict copy. Directory ancestry remains acyclic and sibling names remain
   unique under the portable profile.
10. Tombstones/history outlive the documented active cursor window. Below-
    retention or wrong-epoch clients receive explicit rebaseline, never an
    empty page that implies deletion convergence.
11. Client clock/timezone does not determine journal order. Client timestamps
    remain untrusted metadata.
12. Revoked devices cannot fetch/apply new server changes or submit mutations;
    already downloaded bytes are outside recall.

### Core sync scenario matrix

Each scenario runs through the HTTP API and reference client, asserts public
tree/version/cursor behavior, then runs invariant inspection. `A` and `B` use
independent durable local states and credentials.

| ID | Setup and operations | Required convergence and evidence |
|---|---|---|
| `SYNC-001` Empty initial sync | New library; B requests paginated snapshot/checkpoint, then changes | Empty durable local tree/root metadata matches; checkpoint is valid; repeat is harmless. |
| `SYNC-002` Create on A → B | A creates folder and uploads file; B consumes pages | B has exact hierarchy, version ID, length/hash/metadata; one logical create; bytes verify. |
| `SYNC-003` Nested subtree creation | A creates deep/wide directories/files with page boundaries | Parent prerequisites apply deterministically; no orphan local nodes; page replay is safe. |
| `SYNC-004` Content modify on A → B | B has v1; A commits v2 with base v1 | B advances same `Node` to immutable v2, retains/handles v1 per policy, downloads verified v2. |
| `SYNC-005` Rename on A → B | Rename large file without content change | B updates name/name key; object/version content identity unchanged; one rename fact. |
| `SYNC-006` Move on A → B | Move file and directory subtree between parents | B hierarchy converges; descendant object bytes are not re-uploaded; no duplicate descendants. |
| `SYNC-007` Portable name collision | A/B create names equal under normalization/case fold | Exactly one normal placement; other gets defined conflict/error; no invisible platform collision. |
| `SYNC-008` Directory cycle | Concurrent/stale move attempts parent under descendant | Cycle-causing mutation fails transactionally; no event/partial ancestry change. |
| `SYNC-009` Delete on A → B | A trashes active file after B checkpoint | B observes tombstone/trash transition and follows local policy; server version/history remains retained. |
| `SYNC-010` Recursive folder delete | Trash a paginated large subtree | Defined bounded event/projection semantics converge; restart/replay never leaves mixed invisible state. |
| `SYNC-011` Restore on A → B | Restore trashed node, original parent free | B sees restored identity/revision and content; new event, no re-upload/rewrite of old object. |
| `SYNC-012` Restore name collision | Original parent now contains colliding name | Defined server conflict/name/destination behavior; both resources preserved/addressable. |
| `SYNC-013` Purge versus stale client | Retention purges tombstone/history while B is below retained cursor | B receives cursor-expired/rebaseline; never resurrects or silently retains wrong live state. |
| `SYNC-014` Offline content/content conflict | Reserved for the future content mutation protocol; Prompt 34 accepts no file bytes | No content-conflict claim is made by Prompt 34; a later protocol must preserve both byte streams without last-writer-wins loss. |
| `SYNC-015` Content versus rename | A edits from base while B renames same node | Compatible operations rebase/merge per spec or return explicit conflict; content and intended current name remain explainable and journaled. |
| `SYNC-016` Content versus delete | A edits offline while B trashes node | Defined edit/delete conflict preserves incoming bytes and deletion history; no silent resurrection/overwrite. |
| `SYNC-017` Restore versus new edit | A restores old version while B advances head | Base precondition chooses one head; stale outcome is explicit/conflict-preserved; old history immutable. |
| `SYNC-018` Rename versus rename | A/B offline choose different names from one revision | One commit; other receives current/rebase or deterministic conflict; no nondeterministic last arrival. |
| `SYNC-019` Move versus move | Same node moved to two parents concurrently | One commit; loser gets current state/rebase; ancestry/uniqueness intact. |
| `SYNC-020` Move versus parent delete | A moves into parent B concurrently trashes | Transaction lock/precondition prevents active child under invalid parent; explicit winner/error and journal. |
| `SYNC-021` Duplicate mutation request | Send identical `client_mutation_id` concurrently and serially | One domain revision/event/audit/outbox; every response resolves to stored outcome. |
| `SYNC-022` Mutation ID payload mismatch | Reuse ID with changed name/base/content | Stable idempotency conflict; no second outcome or leaked prior cross-principal data. |
| `SYNC-023` Lost success response | Commit mutation, drop response, reconnect/retry | Exact prior result returned; no duplicate version/change; subsequent page contains logical event once (transport may repeat page). |
| `SYNC-024` Duplicate change page | Client receives/applies same page twice | Local state and conflict records unchanged after second apply; cursor remains valid. |
| `SYNC-025` Client crash before page commit | Kill after applying subset in memory but before atomic local DB commit | Restart uses old cursor and reapplies full page; final state exactly once logically. |
| `SYNC-026` Client crash after page commit | Commit local page/cursor, kill before acknowledgement/next request | Restart continues from stored cursor; no missed event or duplicate user-visible conflict. |
| `SYNC-027` Writes during paginated change feed | Barrier writes/moves/deletes between page requests | Returned cursors/pages cover all events in order, possibly on later page; none appear below an already advanced boundary. |
| `SYNC-028` Writes during initial snapshot | Mutate nodes while B scans multiple pages | Snapshot/checkpoint protocol yields old state plus later event or new state consistently; B reaches authoritative final tree. |
| `SYNC-029` Large backlog | Inactive B returns after a large retained event set | B catches up in bounded pages/memory, survives duplicates/retries, reports lag/progress and converges. |
| `SYNC-030` Cursor below retention | Advance retention beyond B checkpoint | Stable cursor-expired error with paginated rebaseline route; no reset-to-zero guess and no omitted tombstone. |
| `SYNC-031` Wrong library/user cursor | Present A/library-1 cursor to B/library-2 or another user | Rejected without revealing library existence/content; no cursor progress/mutation. |
| `SYNC-032` Tampered/unknown-version cursor | Flip bits/truncate/change version/epoch | Stable invalid/unsupported cursor error; no panic, fallback timestamp or data leak. |
| `SYNC-033` Epoch rollover/rebaseline | Administrative/protocol event changes journal epoch under documented process | Old cursor explicitly rejected; authoritative snapshot returns new checkpoint; client cannot combine epochs. |
| `SYNC-034` Revoked/paused device | Revoke after cursor/upload acquired, before next page/mutation | Future requests rejected according to pause/revoke policy; no unauthorized event/object access. |
| `SYNC-035` Extreme clock skew | A/B clocks differ by days and client mtimes reverse | Server commit sequence and base versions decide behavior; mtimes preserved only as metadata. |
| `SYNC-036` Network reorder/loss | Delay mutation response, fetch change page elsewhere, duplicate/reorder requests | Idempotency/journal facts converge; no dependence on arrival order beyond committed winner. |
| `SYNC-037` Server restart mid-upload | Restart API/worker after parts, during verify, before completion retry | Upload resumes/status is correct; no visible partial file; final commit produces one event. |
| `SYNC-038` Server restart after commit | Kill after DB commit/before response/outbox consumption | Retry returns result; worker later handles at-least-once; one current version/change. |
| `SYNC-039` Disk full/checksum mismatch | Fail incoming conflict upload or normal update during staging/finalize | Current server head unchanged; no event for failed content; client receives retryable/terminal error accurately. |
| `SYNC-040` PostgreSQL unavailable | Attempt mutation/change fetch during DB outage | No successful untracked mutation; staged transport behavior explicit; retry after recovery converges. |
| `SYNC-041` Object store unavailable | Request content update/download and metadata-only operation | No content commit referencing missing bytes; read error stable; only explicitly safe metadata operation may commit/journal. |
| `SYNC-042` Authorization changes in backlog | Share/device access revoked after events exist but before fetch | Current authorization wins; cursor does not leak old event metadata/bytes; grantee receives defined removal/access state. |
| `SYNC-043` Event schema compatibility | Old supported client sees an additive/new event projection | It applies, invalidates/re-fetches or reports unsupported safely; never maps unknown state to delete/purge. |
| `SYNC-044` Multiple libraries | Heavy writes in library X while Y syncs | Ordering/cursor scopes remain independent; no cross-library event or avoidable global serialization/data leak. |

### Journal concurrency proofs

Tests force these database interleavings with barriers:

1. transaction T1 modifies metadata before acquiring the library clock; T2
   reaches the same clock; commit order and allocated sequence remain aligned;
2. T1 allocates then rolls back; the next committed sequence/head/cursor cannot
   cause a client to wait for or skip an invisible fact;
3. one mutation emits multiple defined events; the cursor boundary never splits
   an atomic fact set in a way that produces impossible local state;
4. page query runs while mutations commit above its supplied cursor; returned
   cursor advances only through events actually covered by the page contract;
5. retention deletes old events while a fetch/rebaseline begins; it returns a
   valid complete page/snapshot or explicit cursor-expired, never partial silent
   success;
6. two workers/tasks attempt journal cleanup or epoch change; one serialized
   result and audit fact wins.

### Model-based sync testing

Generate long operation sequences over multiple devices:

- create file/directory, replace bytes, rename, move, copy, trash, restore,
  purge eligibility, share/revoke, go offline/online, pause/revoke device;
- drop/duplicate/delay requests and pages, crash before/after local/server
  durable boundaries, advance retention and skew clocks;
- choose valid and invalid base revisions, normalized names, parents and cursors.

After each quiescent point, compare server and clients with a simple reference
model. All authorized online clients must converge on active/trash hierarchy,
current version/hash, explicit conflicts and cursor/epoch. History and protected
objects satisfy the universal invariants. The generator prints and persists
the seed and shrinks a failure into a regression fixture.

### Client/platform sync tests

The server suite is necessary but not sufficient for native clients. Each
claimed Windows, macOS, Linux Desktop, and Linux Server profile tests:

- Unicode normalization, case-only rename, reserved names, trailing
  dot/space, long components, collisions and deterministic local encoding;
- symlink/reparse/junction policy, hard links, sparse files, permission/xattr
  support and file identities;
- watcher overflow/lost notification/coalescing plus authoritative rescan;
- Prompt 38 deterministic watcher tests for create/modify/delete, paired
  rename/move, ambiguous unpaired remove+create fallback, inbound suppression,
  user-edit-after-inbound races, bounded queue overflow, restart scan recovery,
  and control-path exclusion;
- native Linux `notify` tests for live create plus disposable-root rename/move,
  editor-style temp replace, delete, and symlink/special-entry safety where the
  host exposes those semantics; Windows evidence is cross-target compile unless
  a native Windows host actually runs the watcher suite;
- atomic local temp-write/hash/replace, locked/open file, process kill and low
  disk/inodes;
- local state database transaction, corruption, backup/rebuild and cursor apply;
- credential Keychain/OS storage, log/diagnostic redaction and revoke;
- files-on-demand capability states where supported; local eviction never
  becomes server deletion.

The platform/distribution matrix additionally tests:

- clean install and reinstall discovery of a retained storage identity;
- service start/stop/drain, process crash, host reboot, sleep/wake, partial
  update, failed preflight, and bounded recovery without data reset;
- storage candidate selection, capacity/inode reporting, missing/removable root,
  path/junction safety, capability downgrade, and optional Btrfs/WinBtrfs
  acceleration disabled;
- short-lived pairing code generation, single use, expiry, wrong-user/device,
  replay, concurrent claim, revoke, and clear remediation;
- self-hosted LAN/direct and operator-configured remote access states, TLS/proxy
  diagnostics, no mandatory relay, and offline/air-gapped behavior;
- user versus administrator versus developer health/error presentation, including
  stable code, correlation ID, remediation, and secret/path redaction;
- signed artifact verification, interrupted update, migration incompatibility,
  rollback/restore requirement, application-only uninstall, data deletion
  confirmation, machine migration plan/verify/resume, and old-device coexistence.

Unsupported metadata/capability is reported; it is not silently discarded in a
way that prevents later restore.

### Sync performance/soak

Measure, without inventing universal targets:

- mutation throughput and lock wait versus active devices per one and many
  libraries;
- listing/initial snapshot/change page tail latency by directory/event shape;
- backlog catch-up events/bytes, client local apply cost and memory;
- concurrent upload/download plus change-feed latency;
- journal storage growth, retention/cleanup time and inactive-device impact;
- 24-hour-or-longer randomized sync soak with restart/network faults, checking
  convergence and object/reference drift at intervals.

Publish environment and accepted regression budget. A faster result cannot
trade away commit-order, checksum, conflict or authorization invariants.

## Backup and restore test contract

Backup testing is deliberately separate from sync. Reusing a sync test and
calling upload-only behavior “backup” does not prove retained history.

### Backup invariants

1. A `BUILDING`, `VERIFYING`, `FAILED` or expired snapshot is not offered as
   restorable. Only atomically `COMMITTED` manifests are restore anchors.
2. Every committed entry references a verified immutable object or explicitly
   records a non-content result permitted by the manifest contract.
3. A missing/deleted/unreadable/excluded source affects the new observation; it
   never mutates an older retained snapshot or emits a live `Node` deletion.
4. Repeated backup submission/idempotency yields one snapshot outcome; unchanged
   content reuse does not couple snapshot lifecycle.
5. Snapshot consistency is labeled honestly (`FILESYSTEM_CONSISTENT`,
   `CRASH_CONSISTENT`, or `BEST_EFFORT`) from client evidence.
6. Retention first selects complete snapshots under policy/hold, then GC
   considers all remaining entries/references. It cannot delete content needed
   by a retained snapshot or active restore.
7. Restore defaults to a new/non-destructive destination. Overwrite requires an
   explicit collision policy and precondition.
8. A successful restore result verifies manifest membership, required entry
   count/skip policy, length and canonical hash after writing.
9. A lost source device/credential does not prevent an authorized owner from
   restoring retained server backup through a new device/session.
10. Backup UI/status distinguishes last attempt, last complete snapshot,
    consistency, age, errors and restore verification.

### Snapshot and retention scenario matrix

| ID | Setup and operations | Required result |
|---|---|---|
| `BACKUP-001` Initial complete backup | Select nested source with empty, small and large files; upload/build/verify/commit | One `COMMITTED` snapshot; exact manifest hierarchy/count/hash/bytes; all objects verified; consistency label shown. |
| `BACKUP-002` Empty source | Commit a valid empty selected folder | Restorable empty root manifest, not a failed/missing-source ambiguity. |
| `BACKUP-003` Unchanged repeated backup | Run again with identical stable identities/content | New snapshot per policy reuses verified objects/entries; logical history distinct; physical use/accounting correct. |
| `BACKUP-004` Local file deletion | Delete one source file after snapshot 1; create snapshot 2 | Snapshot 1 still restores the file; snapshot 2 records absence by manifest semantics; no live sync deletion is emitted. |
| `BACKUP-005` Local folder deletion | Remove a populated directory between snapshots | Older subtree remains intact under retention; new snapshot omission is bounded/explicit, not recursive server purge. |
| `BACKUP-006` Rename/move | Rename/move source without byte change | New manifest reflects relative hierarchy and may reuse object; old snapshot path unchanged; no object rewrite. |
| `BACKUP-007` Changed small file | Modify between snapshots | New entry references verified new object/version; old bytes remain restorable. |
| `BACKUP-008` Changed large file | Modify beginning/middle/end of large input | Whole-object path uploads/verifies complete new bytes initially; interrupted transfer resumes; old snapshot safe. Future chunks are separate gate. |
| `BACKUP-009` File changes during read | Mutate/truncate/replace while scanner reads | Client detects identity/size/mtime/hash change and retries or records honest failure/`BEST_EFFORT`; never labels mixed bytes verified. |
| `BACKUP-010` Tree changes during scan | Rename/delete/create across paginated scan | Snapshot uses declared consistency model and explicit errors; no fabricated `FILESYSTEM_CONSISTENT` claim. |
| `BACKUP-011` Unreadable file | Permission/open error for one selected file | Entry/result and snapshot completeness follow documented policy; prior copy retained; UI reports path safely; no silent success. |
| `BACKUP-012` Exclusion policy | Include/exclude file type/path rules, then change policy | Manifest records policy/version and expected exclusions; excluded data is not uploaded; older retained data obeys retention, not immediate delete. |
| `BACKUP-013` Symlink/reparse source | Include link to inside/outside source and loop | Policy records/skips link metadata without traversing outside or looping; restore cannot escape destination. |
| `BACKUP-014` Hard link/sparse/special file | Platform-specific source entries | Behavior is supported and verified or explicitly recorded unsupported/metadata-limited; no false complete restore promise. |
| `BACKUP-015` Interrupted scan before upload | Kill client mid-enumeration | No committed partial snapshot; restart/retry bounded; staging/BUILDING state later reclaimed. |
| `BACKUP-016` Interrupted upload | Kill after some new objects/parts | Resume reuses verified parts; no committed partial manifest; prior snapshots unaffected. |
| `BACKUP-017` Server crash during verify/commit | Kill at object finalize, manifest verify, DB commit and response | Exactly one committed snapshot or explicit noncommitted state; orphan/leases reconcile; retry returns stored outcome after commit. |
| `BACKUP-018` Duplicate snapshot submission | Concurrent/repeated same backup mutation identity | One outcome/manifest and defined repeated response; no duplicate retention anchor/accounting. |
| `BACKUP-019` Quota exhausted | Exhaust quota during content and at manifest commit | Snapshot does not commit with missing references; prior snapshots/restores work; error/status identifies remediation. |
| `BACKUP-020` Disk/inode full | Fail staging/finalization/server manifest work | No partial committed snapshot; recovery headroom/read/restore remain; retry after capacity works. |
| `BACKUP-021` Stored object corrupt | Bit-flip an entry object before verify/restore | Snapshot/entry is reported affected; bytes not returned as valid; verified redundant/system backup recovery only. |
| `BACKUP-022` Source device paused/revoked | Revoke before/during scan/upload and after commit | Unauthorized future submissions stop; already committed retained snapshot remains owner-restorable; no OS wipe claim. |
| `BACKUP-023` Device removed/lost | Delete/retire device registration under policy | Retained snapshot ownership/restore persists; device-specific credential unnecessary for authorized recovery; deletion policy explicit. |
| `BACKUP-024` Retention selection | Seed hourly/daily/monthly ages and advance clock | Exact policy/hold selects expected snapshots deterministically; time zones/DST do not alter UTC retention unexpectedly. |
| `BACKUP-025` Retention with shared objects | Snapshots/live versions share equal object | Expiring one reference never deletes bytes needed by another; logical/physical accounting reconciles. |
| `BACKUP-026` Retention versus restore | Barrier active restore lease while retention/GC runs | Object remains protected until verified restore/lease completion; retry safe. |
| `BACKUP-027` Retention crash/retry | Kill after marking snapshot, before/after reference removal/sweep | Restart resumes idempotently; no retained snapshot loses entry; audit/status coherent. |
| `BACKUP-028` Legal/administrative hold | Apply/remove hold with authorization | Held snapshot excluded from retention; action audited; removal does not instantly bypass grace/verification. |
| `BACKUP-029` Reconciliation drift | Remove/add storage object or reference in isolated fixture | Auditor reports missing/orphan/uncertain accurately; never auto-deletes uncertainty or rewrites manifest to green. |
| `BACKUP-030` Many small files | Snapshot very large entry count under bounded pages/batches | Memory/transaction/manifest sizes bounded; restart progress stable; count/root hash correct. |

### Restore scenario matrix

| ID | Setup and operations | Required result |
|---|---|---|
| `RESTORE-001` Single file to new path | Choose retained entry and empty destination | Exact verified bytes/length/hash and portable metadata; durable success report. |
| `RESTORE-002` Directory subtree | Restore nested subtree with empty files and large objects | Exact hierarchy/required count; bounded batches; per-entry verification/report. |
| `RESTORE-003` Complete snapshot to clean device | No old device credential/cache; authenticate owner and restore | All required entries restored/verified; skips/unsupported metadata explicit; proves device-loss recovery. |
| `RESTORE-004` Prior file version | Restore selected immutable version into live library | New current version/source attribution and journal event; old/current history unchanged; base precondition enforced. |
| `RESTORE-005` Destination collision: rename | Existing newer/different file; choose keep-both | Deterministic portable conflict name; neither byte stream lost. |
| `RESTORE-006` Destination collision: skip | Choose skip policy | Existing bytes untouched; skipped result explicit and included in final summary. |
| `RESTORE-007` Destination collision: overwrite | Explicit overwrite with base precondition | Only matching destination replaced atomically; stale precondition conflicts; overwritten history retained where applicable. |
| `RESTORE-008` Interrupted restore | Kill after arbitrary entry/write/verify boundary | Operation remains restartable; verified completed entries reused; partial temp bytes not reported complete. |
| `RESTORE-009` Duplicate/lost response | Retry same restore mutation after commit/response loss | One restore operation/outcome per entry; no duplicate versions/files. |
| `RESTORE-010` Corrupt/missing source object | Restore includes affected entry | Fail affected result loudly/quarantine; do not write corrupt destination or mark whole restore success. |
| `RESTORE-011` Quota/disk full at destination | Capacity ends mid-restore | Safe partial report and restart; verified prior outputs intact; temp partial cleaned; no destructive collision fallback. |
| `RESTORE-012` Malicious manifest names | `..`, separators, absolute/reserved/Unicode collision and symlink entries | Server/client maps/rejects safely under portable policy; no escape from selected destination. |
| `RESTORE-013` Permissions/metadata unsupported | Restore POSIX metadata to Windows or vice versa | Bytes remain correct; unsupported metadata reported, not silently claimed or escalated. |
| `RESTORE-014` Restore versus retention/GC | Start restore, expire source snapshot concurrently | Lease/reference serializes safely; restore completes or explicit cancellation before deletion. |
| `RESTORE-015` Authorization/revocation | Share/read-only/device credential attempts restore; revoke mid-plan | Unauthorized mutation rejected; owner operation follows documented initiation/recheck boundary; no object leak. |
| `RESTORE-016` Post-restore verification | Independently hash/read every required output or deterministic sample policy | Final status is success only under specified verification; report includes bytes, entries, skips/conflicts/errors. |

### Backup property/model testing

Generate sequences of source create/modify/rename/delete/unreadable/exclude,
snapshot start/interrupt/commit, retention/hold, device revoke, object corruption,
restore/interrupt and GC. Compare with a simple immutable-manifest model:

- each retained committed snapshot reconstructs its declared observations;
- source absence cannot mutate previous manifests/live nodes;
- all retained/active-restore objects remain protected;
- expired/non-held snapshots eventually cease protection after grace;
- restore output equals selected manifest bytes or has an explicit accepted
  result for every required entry.

Seeds shrink into permanent fixtures. Use UTC deterministic clocks and include
retention boundary instants.

### Backup performance/soak

Benchmark separate source shapes: many tiny files, mixed home-directory data,
few very large files, high unchanged ratio, high churn and slow backend. Record
scan/hash/upload/manifest/commit/retention/restore throughput, memory, database/
object operations, unique/logical bytes, queue age and failures. Repeated soak
creates/retains/expires snapshots while restoring sampled snapshots and running
GC/invariant audit.

No performance result permits incomplete snapshots to appear committed or
disables post-restore integrity verification silently.

## Synveil system-backup and disaster-recovery tests

Product `BackupSnapshot` tests do not replace server system-backup tests. The
release lab must:

1. seed users, sessions/devices, libraries/nodes/versions/trash, shares,
   uploads, sync cursors, backup snapshots, jobs/audit and optional encrypted
   integration secrets;
2. enter the documented mutation fence and capture PostgreSQL, committed object
   namespaces, configuration/storage identity, release/migration metadata and
   irreplaceable master key through the supported method;
3. restore to new database/object paths on a clean isolated host/project;
4. run invariants and authenticate with recovered credentials/recovery process;
5. hash/range-read representative and boundary-size versions, list journals,
   restore an old version and a complete user backup, process/reconcile jobs,
   and decrypt/rotate an integration secret;
6. prove missing DB, missing object set, wrong storage identity, wrong key,
   corrupt manifest and unsupported release fail loudly before writers start;
7. record time, size and every excluded transient (for example incomplete
   staging), then test its documented post-restore state.

At least one full clean restore is release evidence. A backup command exit code
without restore is not.

## Security test suites

[SECURITY.md](SECURITY.md) owns the complete threat/control matrix. Automated
and manual evidence includes:

### Authentication and authorization

- username enumeration, brute force/backoff, Argon2 resource/concurrency,
  bootstrap race, recovery-code reuse, session fixation/rotation replay, expiry,
  logout/password reset/session epoch and device revoke; browser refresh
  pre-commit failure is retryable, while post-commit response loss plus reuse
  deterministically sets the family to `REVOKED`, records
  `REFRESH_REPLAY_DETECTED`, and requires fresh login;
- session list redaction and independent revoke (including current-session
  cookie clearing); recovery-set one-time display, lost generation response,
  pending activation race/expiry, atomic same-code exchange race, exchange
  response loss before/after commit, replacement of the unreachable pending
  transaction through `PENDING` → `EXPIRED`, reservation expiry without code
  consumption, last-code reset, old-set exchange followed by set replacement,
  set-activation versus reset common-lock race with one winner, one-use reset,
  canonical-epoch invalidation of every `WEB`/`API`/`DEVICE` grant, retained
  device records/data, and fresh-authentication requirement;
- API-grant least-scope/expiry enforcement, exact `ApiGrant`/`Session` identity,
  one-time issue/rotation response, `one_time_secret_unavailable`, lost-response
  pending replacement/expiry, activation race, old-generation retirement,
  sole Session-family revoke, replay, and proof a grant never amplifies owner
  access; the same initial/rotation/activation matrix covers device credentials,
  including owner-only activation, rejection of pending-secret self-activation,
  old-generation survival until activation, password-reset pause with the old
  family still `REVOKED`, fresh `PENDING` family re-enrollment and activation to
  `ACTIVE`, refusal to resurrect an explicitly revoked device, and atomic revoke;
- cross-user/cross-library/cross-object/cross-upload/cross-backup/cross-photo/
  cross-repository IDOR; read-only/writable/inherited share matrix; administrator
  versus owner behavior;
- per-user `NodeFavorite` isolation, idempotent opposing mutations, Trash/
  restore/purge behavior, and proof that favorites neither grant access nor
  retain content; received/sent share discovery reauthorizes every projection,
  hides unreadable ancestors, lists safe pending sent links for their owner,
  and drops revoked/expired received grants mid-pagination;
- public share token entropy/verifier storage, inert pending creation,
  lost-response/idempotency replay without capability disclosure, explicit
  revoke-then-create-new recovery, activation versus revoke/expiry races,
  guessing limits, expiry/password, revoke, bytes/concurrency and
  information-equivalent errors.

### Browser/API

- CSRF with missing/wrong token, cross-site origin and trusted same-origin;
  exact CORS, proxy-header spoof and host-header behavior;
- XSS corpus in filenames/tags/errors/repository/AI text; active SVG/HTML;
  content disposition/type/nosniff/CSP/frame/referrer headers;
- SQL injection corpus for filters/sort/search/IDs and safe error/stack trace
  behavior;
- request/header/body/JSON depth/count/range/pagination/idempotency bounds and
  slow-client timeouts.

### Storage and parsers

- traversal/absolute/NUL/Unicode/separator/reserved names, symlink/junction/
  reparse/hard-link races, root identity and exact-key deletion;
- oversize, zip/decompression bombs, huge dimensions/pages/frames, malformed
  image/video/PDF/archives, timeout/OOM/crash and no-network parser isolation;
- direct object key/presigned URL misuse, foreign references, bucket/prefix
  policy, corruption/quarantine and cross-owner dedup presence/timing/accounting.

### SSRF, webhooks, Git and AI

- alternate URL encodings, userinfo, scheme/port, redirect chains, DNS rebinding,
  loopback/link-local/cloud metadata/private address; explicit private Forgejo
  allowlist positive case;
- webhook missing/bad signature, old/future timestamp, replay/duplicate,
  oversized/misbound payload and later polling reconciliation;
- Git argument/shell injection, malicious repository names/refs/submodule/LFS
  URLs, process limits, credential redaction and restore overwrite refusal;
- network capture proving no AI/model/provider egress in `DISABLED` and `LOCAL`;
  remote consent and withdrawal, exact provider, payload minimization, logs,
  stale/delete/ACL/prompt-injection and provider-failure behavior.

### Secret and telemetry canaries

Seed unique synthetic password, session/device/share/recovery/provider/Git/
database secrets and sensitive names/content. Exercise success and failure, then
scan logs, traces, metrics, audit, HTTP errors, diagnostics, crash output,
container inspect/history and outbound captures. Raw canaries must not appear
outside their authorized boundary. Audit must contain safe opaque evidence for
the security action.

## Job/outbox tests

For every handler and job schema version:

- mutation commits with outbox, rolls back without it, and works when worker is
  absent;
- two workers race to claim; one lease generation executes at a time;
- worker crashes before work, after external/byte work, after output commit and
  before job acknowledgement; retry is idempotent and source-version bound;
- lease expires/renews/steals under deterministic clock; stale generation
  cannot mark a newer claim complete;
- retryable versus terminal error, exponential backoff/jitter, max attempt and
  dead-letter; poison job does not spin;
- disabled optional class remains queued/coalesced/cancelled under policy
  without blocking required jobs; re-enable/replay is safe;
- payload/version mismatch fails visibly; no deserialization panic or accidental
  destructive default;
- queue age/attempt/dead-letter/heartbeat metrics and restricted operator
  controls are accurate and audit replay/pause actions.

## Photos and media tests

- Original upload/download/backup/restore hashes stay unchanged through every
  derivative job and metadata edit.
- EXIF orientation/timezone/location and missing/malformed metadata; location
  ACL and share-stripped derivative; no original rewrite.
- Huge pixel dimension, frame/page count, truncated/polyglot/active SVG,
  unsupported HEIC/HEVC and large video; bounded parser and graceful
  no-thumbnail behavior.
- Thumbnail/rendition duplicate job, stale source, crash, corrupt output,
  purge/rebuild and storage accounting.
- Exact duplicate within/other dedup domain; perceptual suggestion false
  positive never auto-deletes.
- Live-photo-style grouped still/video/resource roles, partial group upload,
  duplicate client import and restore.
- Phone source deletion under upload-only policy preserves retained server
  original; any future mirror mode has separate explicit tests.
- Timeline/album/favorite/search pagination and authorization under concurrent
  metadata/ACL/delete changes.

## AI tests

- Run the complete core regression suite with no AI service/schema extension
  where supported, with AI disabled, unreachable, slow and returning errors.
- Job output is bound to exact source version/model/config; a stale result cannot
  become current after update/delete/restore/ACL change.
- OCR/embedding/tag results are replaceable and provenance-labeled; user tag is
  not overwritten; confidence/unsupported output is bounded.
- Index query enforces current authorization even if index-time authorization
  differed. Cross-user/project fixtures and revoked shares return no snippets,
  names, distances or timing-based existence signal beyond accepted bounds.
- Delete/exclude/provider withdrawal stops new work and purges/rebuilds derived
  state under documented lag; canonical content remains.
- Prompt-like document/repository text cannot invoke shell, network, SQL,
  storage/admin tools, disclose system secrets or change policy.
- Local/remote model quality is evaluated on a documented, licensed, privacy-
  safe corpus. Quality thresholds are feature-specific and never replace
  deterministic metadata search.
- Model download/hash/license, resource exhaustion, provider rate limit/timeout,
  credential rotation and no-payload-log behavior pass.

## Forgejo and repository tests

Use a supported Forgejo compatibility matrix with isolated instances and
repositories covering empty/large histories, unusual refs/names, submodules,
LFS, release artifacts and permissions.

- Inventory/poll pagination, rate/backoff, credential expiry and outage expose
  accurate fresh/stale/error state without core failure.
- Webhook is only a hint: lost/duplicate/reordered/forged events converge after
  polling.
- Backup manifest states exactly which Git refs/reachable data, LFS, artifacts
  and metadata are included/omitted. Verify Git integrity with the selected
  supported tool and compare LFS/artifact hashes.
- Kill at export/upload/manifest commit; repeat/lost response is idempotent;
  prior repository backups remain restorable.
- Restore into a new empty destination, verify refs/objects/LFS/artifacts and
  reject incompatible/unauthorized/occupied target by default.
- Repository content cannot create arbitrary network fetch/subprocess argument,
  and Forgejo credential never enters Git output/log/error.
- Forgejo absent/bad version cannot break Files, sync, user backup or existing
  verified repository backup reads.

## Web end-to-end and accessibility

Playwright or an equivalent real-browser suite runs against Caddy/API/
PostgreSQL/object storage for:

- first bootstrap/login/logout/recovery and secure cookie/CSRF behavior;
- Personal / Home onboarding, storage picker/capacity explanation, pairing,
  plain-language health/error remediation, progressive disclosure, and the
  Advanced / Server handoff without requiring terminal steps in the core flow;
- Files list/pagination/sort/filter, folder/create/move/rename, drag/drop
  resumable upload, progress/retry, range/download, multi-select/bulk bounds;
- version history/restore, Trash restore/purge confirmation and lost-response
  status;
- personal Favorites across rename/Trash/restore and access loss; Shared
  received/sent discovery without pre-known IDs; share create/copy/expiry/
  password/revoke and unauthenticated public view;
- Recent pagination watermark/order under concurrent mutation, immediate
  auth/Trash filtering, minimized shared paths, and proof that reads/downloads
  do not create hidden view-tracking state;
- device list/pause/revoke, sync conflict/activity/freshness;
- backup set/snapshot/consistency/error/restore and clean-destination report;
- optional Photos/AI/Code pages in ready/degraded/disabled/stale states.

Keyboard navigation, focus order/restoration, semantic labels, contrast, screen-
reader announcements, reduced motion and large transfer/error progress are
release gates under ADR-009. A UI must not label a pending/failed snapshot,
stale index, unsupported codec or simulated placeholder as complete.

## Deployment and health tests

For each claimed Personal / Home, Advanced / Server, or developer profile:

- clean guided/native or operator install, config validation/bootstrap and
  idempotent restart;
- only Caddy ports externally reachable; container users/capabilities/mounts/
  read-only roots/private networks match policy; no Docker socket/secret image;
- Caddy TLS/redirect/host/proxy/header/streaming/range/cache behavior;
- `/health/live`, `/health/ready`, restricted details and worker heartbeat under
  API/DB/object/worker/AI/Forgejo outages; no expensive probe or information
  leak;
- capacity reserve/inode exhaustion, storage identity missing/wrong mount and
  secret permission/master-key failure;
- SIGTERM/drain/restart with active upload/download/job, host reboot simulation
  and startup reconciliation;
- native service lifecycle on Windows Service, launchd, systemd, or the declared
  adapter; crash, reboot, sleep/wake, permission/elevation and update-recovery
  behavior;
- managed PostgreSQL provision/discover/start/backup/restore/upgrade where the
  profile claims it, with no SQLite fallback contract;
- storage picker safety, capability probe, missing/removable root, NAS/S3
  boundary, optional filesystem acceleration and path migration validation;
- one-time pairing expiry/replay/revoke and direct/LAN/remote access failure
  states without requiring a hosted relay;
- signed artifact/compatibility preflight, verified backup before risky
  migration, application-only uninstall/reinstall data discovery, and guided
  machine migration/recovery with inspect/plan/validate/execute/verify evidence;
- metrics cardinality/labels, alerts/runbooks, log rotation/disk use and no
  telemetry egress by default;
- coordinated system backup and clean restore as specified above.

Optional failure does not make core readiness false. Required PostgreSQL/object
failure never yields a false successful content mutation.

## Migration, release and upgrade testing

### Version fixtures

Each supported released version contributes an immutable fixture containing:

- database dump/snapshot and migration checksum/version;
- object representations and storage identity;
- configuration schema with redacted secret references;
- users/sessions/devices/shares, libraries/names, versions/trash, journal/
  cursors, uploads, backups/restores, jobs/audit and optional feature state;
- edge cases introduced/fixed in that release.

Never use production user data as a fixture.

### Upgrade matrix

For each supported source → target path:

1. restore source fixture and verify it on the source release;
2. validate target preflight/config and take the supported system backup;
3. apply target migrations exactly once and concurrently attempt a second
   migrator to prove locking;
4. inject interruption at each nontransactional/resumable migration/backfill;
5. start target API/worker, run invariants and representative old/new reads/
   writes/sync/backup/restore;
6. verify old object formats and job/event payload versions remain readable or
   have completed the documented readers-before-writers migration;
7. test only documented binary rollback when schema compatible; otherwise
   restore complete pre-upgrade backup and verify source release again;
8. reject unsupported skip upgrade, newer unknown schema, changed released
   migration checksum, missing object/backend identity and missing master key
   before writers start.

Large backfills test progress/resume, bounded locks/load and mixed reader/writer
rules. No test “fixes” an upgrade by wiping the database.

### Release-candidate gates

- All required static/unit/integration/conformance/security/E2E suites green on
  supported matrix; no release-required skip/flaky quarantine.
- Full storage/sync/backup crash suites and at least one randomized soak green.
- Clean install, system backup, independent clean restore and supported upgrade/
  rollback evidence green.
- Performance regression reviewed on comparable dedicated environment.
- Image/source/checksum/SBOM/license/provenance and dependency findings reviewed.
- English/Vietnamese status, limitations, migration and runbooks aligned.
- No unresolved high-severity data-loss, authorization, secret or upgrade
  finding; critical exception cannot be silently waived.

## Performance benchmark methodology

Benchmarks answer capacity questions, not marketing claims. Every report records:

- commit/release, compiler/runtime/dependency versions and exact configuration;
- CPU/cores, memory, disk/filesystem/mount, database, network/TLS, object backend
  and warm/cold-cache state;
- data distribution: zero/small/medium/large files, directory depth/width,
  entropy/compressibility, event/backlog, backup churn and concurrency;
- warm-up, sample count/duration, errors/retries, percentiles/distribution,
  CPU/RSS/allocations, blocking threads, DB pool/queries, disk/network bytes and
  queue/storage growth;
- baseline comparison, statistical/noise treatment and accepted regression.

Required benchmark categories:

- concurrent resumable upload and range/full download;
- memory per active stream and independence from total file size;
- SHA-256, stored checksum, compression/decompression and dedup lookup;
- metadata listing/search/move and large directory pagination;
- per-library and multi-library mutation/change-feed/backlog latency;
- snapshot build/commit/retention and clean restore for small-file/large-file
  mixes;
- object verification/reconciliation/GC and storage migration;
- job claim/throughput/queue fairness and optional AI/photo/Git isolation;
- web initial/load interaction and client initial scan/apply/hydration where
  supported.

Performance gates initially use reproducible baselines and bounded-resource
invariants. Product SLOs are set only after deployment-class evidence. Disabling
checksums, fsync, authorization, conflict preservation, post-restore verification
or durable job publication is not an accepted optimization.

## CI cadence and ownership

The checked-in `.github/workflows/ci.yml` provides the initial Rust portability
baseline. A quality job runs formatting and Clippy, while native Cargo check
and test jobs run on `ubuntu-latest`, `windows-latest`, and `macos-latest`.
These jobs validate source portability of the current Rust workspace; they do
not by themselves claim that a Synveil product deployment or native installer
is supported on any operating system.

| Cadence | Minimum scope | Owner/action on failure |
|---|---|---|
| Local/pre-submit | Changed-module unit/property seeds, static/type/lint, targeted real integration | Implementer fixes or reports exact block; no “works on my machine” completion |
| Pull request | Workspace static/unit, Postgres/local adapter integration, contract/migration/docs/security checks, targeted E2E | Component owner; contract owner reviews public/schema changes |
| Main/merge | Full integration, Compose smoke, browser critical path, generated drift, invariant audit | Integration owner reverts/fixes before further dependent merge |
| Nightly | Extended fuzz/property, crash matrix, S3/MinIO/NAS labs where supported, randomized sync/backup soak, dependency/image scan | QA triages with saved seed/artifacts; data-safety regression blocks release branch |
| Release candidate | Supported OS/backend/browser/client, full security/recovery/upgrade, performance, clean system restore, artifact provenance | Release owner and independent security/data-safety reviewers sign evidence |
| Periodic | Long soak, restore older retained backups, dependency/model/Forgejo compatibility, incident game day | Domain/operations owner updates support matrix/runbook |

Flaky tests are defects. A quarantined test that protects a storage, sync,
backup, auth or migration invariant blocks its gate until replaced by equivalent
deterministic evidence. Save failing random seeds, request IDs, sanitized logs,
database/object manifests and exact environment; do not save user secrets/data.

## Coverage and mutation quality

Line coverage is diagnostic, not the release oracle. Critical state machines,
authorization branches, transaction failures and decoder errors require branch/
transition evidence. Mutation testing may be introduced for pure domain policy
to prove tests fail when authorization, checksum, retention, cursor or
idempotency conditions are inverted/removed. An arbitrary repository-wide
percentage never substitutes for scenario and invariant coverage.

## Bug regression protocol

Every data-loss, corruption, authorization, retry, migration or privacy defect
adds:

1. the smallest sanitized deterministic reproduction or saved random seed;
2. an assertion against the violated invariant and public behavior;
3. a fault/concurrency interleaving if timing contributed;
4. upgrade/backward fixture when released data may contain the state;
5. recovery/detection logic for already affected installations;
6. security/runbook/documentation updates and an evidence link.

A code patch without a failing-before/passing-after regression is exceptional
and requires a written reason plus an alternative verification method.

## Testing `OPEN DECISION` items

### OPEN DECISION OD-T01: fault-injection mechanism

- **Owner:** QA, Architecture, Storage, Database
- **Needed by:** `SV-G1-STORAGE`
- **Options:** trait/decorator failpoints; process-level proxy/filesystem faults;
  test-build named hooks; combined approach
- **Recommendation:** combined contract adapter and test-build named crash hooks,
  verified by process-kill tests against real PostgreSQL/local storage. No
  production remote failpoint endpoint.
- **Decision evidence:** coverage of every commit boundary, determinism, binary
  isolation and maintenance burden.

### OPEN DECISION OD-T02: reference sync model implementation language

- **Owner:** Sync, QA, Clients
- **Needed by:** Phase 4 protocol fixture work
- **Options:** pure Rust independent module; language-neutral fixture generator
  plus second-language model; model-checking tool for bounded states
- **Recommendation:** small pure model plus language-neutral JSON fixtures and at
  least one independently implemented consumer; add bounded formal/model checker
  if it finds interleavings more effectively.
- **Decision evidence:** independence from production code, shrinkability,
  desktop/Apple reuse and CI cost.

### OPEN DECISION OD-T03: supported filesystem/NAS fault lab

- **Owner:** Storage, DevOps, QA
- **Needed by:** first NAS production claim
- **Options:** local ext4/XFS baseline; NFS/SMB vendor profiles; faulted userspace
  filesystem lab
- **Recommendation:** narrow named filesystem/mount profiles with reproducible
  power-loss/network/capability tests; do not claim generic “NAS support.”
- **Decision evidence:** adapter conformance, fsync/rename/locking, outage,
  permission and name behavior.

### OPEN DECISION OD-T04: performance regression environment

- **Owner:** QA, Release, DevOps
- **Needed by:** first alpha performance gate
- **Options:** dedicated reference host; self-hosted runner class; manual release
  lab with statistical comparison
- **Recommendation:** one controlled reference environment plus portable scripts
  and published config; shared noisy CI provides functional bounds, not precise
  regression claims.
- **Decision evidence:** variance study, cost and reproducibility.

### OPEN DECISION OD-T05: real-OS and release-lab matrix

- **Owner:** QA, Release, Platform / Distribution
- **Needed by:** `SYNVEIL_CROSS_PLATFORM_FOUNDATION_READY` and every first-class
  host support claim
- **Options:** dedicated Windows/macOS/Linux runners; a hardware lab with
  recorded manual evidence; community/experimental labels until a host has
  coverage
- **Recommendation:** use automated runners where service/package behavior is
  reproducible and a small declared hardware lab for filesystem, sleep/reboot,
  removable-storage, and migration cases; never infer host parity from Linux
  Compose.
- **Decision evidence:** runner availability, filesystem/NAS capability results,
  installer/service/update/recovery coverage, accessibility evidence, and
  maintenance cost.

### OPEN DECISION OD-T06: package and installer test boundary

- **Owner:** Installer / Updater, Security, Release, QA
- **Needed by:** Personal / Home implementation and `SYNVEIL_DEPLOYMENT_READY`
- **Options:** package-level smoke only; full install/upgrade/uninstall lab;
  signed artifact plus isolated VM/snapshot matrix
- **Recommendation:** require signed artifacts plus isolated clean-host,
  upgrade-failure, uninstall-preservation, reinstall-discovery, and migration
  tests before a native package is called supported.
- **Decision evidence:** elevation/IPC threats, rollback/restore behavior,
  service crash/reboot/sleep, key/data retention, and reproducible clean-host
  results.

## Phase quality exit checklist

Before the gate owner records any roadmap token:

- required unit/integration/property/fuzz/crash/conformance/security/E2E cases
  for the phase are green on its support matrix;
- invariants pass after test, after restart and after cleanup/retention;
- no required result depends solely on a mock or manual happy path;
- memory/count/time/disk/network bounds and performance method are recorded;
- logs/audit/metrics/health expose safe actionable failure evidence;
- install/upgrade/backup/restore behavior for the new data/schema/config is
  tested;
- all skipped/flaky tests have a release disposition and none protect a
  release-blocking invariant;
- status and English/Vietnamese docs accurately describe support, degradation,
  residual risk and non-goals.

## Prompt 36 desktop inbound evidence

The client-sync suite uses disposable SQLite files and managed roots under the
OS temporary directory. It does not touch a user-selected real root. A
deterministic in-process `SyncRemote` enforces rebaseline completion,
per-device checkpoints, bounded feed/ack behavior, and chunked logical content
download without external internet.

Current direct coverage includes:

- migration from zero, schema version, reopen, WAL/FULL/foreign-key/busy-timeout
  pragmas, foreign-key/check rejection, pending ack/operation/bootstrap restart,
  per-library scope, and exclusive second-writer rejection;
- strict relative paths, Windows-reserved/control/trailing/separator names,
  Unicode/case collision keys, Unicode descendant path updates, root marker and
  wrong scope/root binding, and Linux symlink redirect rejection;
- root-only and multi-page bootstrap, durable manifest pages, terminal topology
  proof, parent-first materialization, response-loss retry, local-complete
  recovery, tracked generation sweep, and preservation of unknown files;
- empty, single-event, and multi-event feed pages, all eight current journal
  kinds, exact applied/acknowledged transitions, cross-device independence,
  wrong epoch, sequence gap, unknown kind, page/event replay, and offline state;
- directory create/rename/move/Trash/restore/purge replay, managed and unknown
  case collision, occupied destination, missing source, stale quarantine,
  divergent file replacement/purge, and racing unknown-directory attribution;
- sequential multi-chunk download, length and SHA-256 mismatch, old-visible-file
  retention, staged restart, replace-before-database recovery, and final
  receipt cleanup; and
- deterministic failures after page intent, before filesystem action, after
  staged content, after filesystem receipt but before SQLite state, after local
  event commit, after server ack, after bootstrap page, during bootstrap apply,
  after local bootstrap completion, and after server completion before local
  handoff.

The authoritative local commands are:

```text
cargo test -p synveil-client-sync --all-targets --locked
cargo clippy -p synveil-client-sync --all-targets --all-features --locked -- -D warnings
```

The release handoff also reruns the full locked workspace gates and the existing
Prompt 31-35 ignored PostgreSQL integration cases against a fresh disposable
database. Linux provides the filesystem runtime evidence for Prompt 36. Native
Windows execution, power-loss testing, and hostile same-user path-race testing
remain later platform/release-lab gates and must not be inferred from Linux. A
locked `cargo check -p synveil-client-sync --target
x86_64-pc-windows-gnu` with the matching official Rust 1.98 toolchain passes,
so the Windows conditional Rust path is compile-audited; that check does not
claim a native Windows link or runtime result.

## Prompt 37 desktop remote evidence

The continuation audit is in
[`PROMPT37_CONTINUATION_AUDIT.md`](../PROMPT37_CONTINUATION_AUDIT.md). It records
the inherited dirty worktree, requirement classification, demonstrated defects,
and final evidence separately. Earlier test output is not a substitute for
rerunning the final source tree.

### Client, protocol, and credential coverage

The Linux client-sync suite contains 55 unit tests (23 HTTP adapter tests,
21 profile/SecretStore tests, and 11 existing local-state/path tests), plus
12 inbound integration tests. HTTP tests use real local Axum/TCP/TLS fixtures
and the production reqwest adapter; the explicit numeric-loopback HTTP policy
does not relax the production HTTPS constructor. Coverage includes:

- exact method, route, query, DTO, bearer header, scope and evidence for all
  seven `SyncRemote` methods; canonical IDs/decimals, unknown fields/kinds,
  epoch/sequence/generation, revision-zero Nodes, revision drift, tombstones,
  and terminal rebaseline proof;
- JSON/error limits with and without `Content-Length`, malformed/HTML bodies,
  compressed or duplicate content encodings, typed HTTP errors, connect/
  header/metadata/idle/total deadlines, no automatic retry, and a maximum of
  500 feed events or 1,000 manifest Nodes per request;
- bounded streaming chunks, exact immutable version and validator, length/
  SHA-256 mismatch, early EOF, extra bytes, and a deadline independent of idle
  progress; no whole-file response buffering;
- a two-server redirect fixture whose destination receives zero requests and
  no bearer/cookie; wrong profile/owner/Device rejection before network access;
  and an ephemeral self-signed TLS fixture rejected before HTTP authentication;
- strict URL normalization/rejection, durable profile and replica/root binding,
  migration from the original SQLite schema, versioned secure-store envelopes,
  copied/reconstructed SQLite isolation, and scans of database/WAL/SHM files
  for synthetic plaintext secrets;
- failed store/read-back/delete, restart after cleanup intent, explicit
  replacement and forget, unavailable secure storage with no plaintext
  fallback, concurrent store attempts, and stale live-engine rejection after
  forget or re-enrollment; and
- preserving managed bytes, bootstrap state, applied/acknowledged sequences,
  pending ACK evidence and local issues through offline/auth/revocation failure.

Core tests cover 256-bit versioned secret syntax, purpose-separated digest
vectors, canonical IDs, redacted Debug and bounded parsing. API tests cover
browser/device principal separation, CSRF, duplicate or mixed authentication,
restricted routes, strict enrollment JSON, and secret-safe errors/tracing.
The trace regression runs in a fresh test process so another parallel test's
tracing callsite cache cannot suppress the capture. It must observe real trace
events and reject secret leakage, including a bearer/grant copied into
`X-Request-Id` or an unmatched URI.

### Fresh PostgreSQL and real HTTP acceptance

There are 27 ignored PostgreSQL cases. Run each case against its own newly
created disposable database with all forward migrations applied by the test.
Use `SYNVEIL_TEST_DATABASE_URL`, not `DATABASE_URL`; reused databases invalidate
the single-owner/count assumptions in these fixtures. The continuation uses a
new loopback-only PostgreSQL 17 container and leaves the interrupted session's
container untouched.

| Package / integration target | Cases |
|---|---:|
| `synveil-metadata` / `postgres` | 14 |
| `synveil-auth` / `postgres` | 2 |
| `synveil-storage` / `gc` | 5 |
| `synveil-metadata` / `device_credentials_postgres` | 1 |
| `synveil-auth` / `device_credentials_postgres` | 3 |
| `synveil-auth` / `bootstrap_precision_postgres` | 1 |
| `synveil-api` / `desktop_remote` | 1 |

Discover exact cases with `cargo test -p <package> --test <target> --locked --
--list --ignored`, then use a fresh database for each invocation:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://postgres@127.0.0.1:5432/fresh_test_database \
  cargo test -p <package> --test <target> --locked -- <exact_test_name> \
  --ignored --exact --test-threads=1
```

Credential cases verify schema constraints, digest-only storage, wrong owner/
Device, inactive owner/Device, single/all-credential revoke, concurrent exchange
with one winner, lost-response replay rejection and fresh-grant recovery. The
lock-expiry regression holds the Device row lock until the grant has expired;
the queued production exchange must leave the Device PENDING and create neither
a credential nor a consumption record. A timestamp captured before waiting
does not satisfy the expiry check.

The acceptance case uses a real router, fresh PostgreSQL, actual logical
storage/upload services, the production `HttpSyncRemote`, fresh SQLite and a
temporary managed root. Only its platform SecretStore is an explicit test
implementation. It bootstraps a browser owner/session, creates and exchanges a
grant, downloads a 192-KiB file, bootstraps the replica, consumes/ACKs a browser
mutation, replaces content with 128 KiB, and injects failure after local apply
before ACK. It revokes the credential, verifies all sync/rebaseline/content
routes reject it, preserves bytes and pending progress, then explicitly
re-enrolls the same Device and recovers ACK. Wrong-owner/library/Device/content
requests, browser CSRF, mixed auth, and device attempts to use upload/mutation/
conflict/admin routes are rejected by that same real router.

### Native secure storage and Windows evidence

Native Linux persistence is a separate ignored test, not an inference from the
test store. Run it in a private `dbus-run-session` with fresh temporary
`XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_RUNTIME_DIR`, and an unlocked synthetic
GNOME Secret Service vault. Never point the fixture at a personal keyring. Wait
for `org.freedesktop.DBus.NameHasOwner` on `org.freedesktop.secrets`; introspection
can accidentally auto-activate a second daemon before the owned daemon is ready.
The unlock password is synthetic and supplied through stdin, not argv or a file.

```text
cargo test -p synveil-platform --lib --locked -- \
  native_secrets::tests::native_secret_survives_backend_recreation_and_is_deleted \
  --ignored --exact
```

The test writes a fresh opaque key, starts a new process to read the entry
through the native adapter, then deletes it and verifies absence. The child
receives only the non-secret key name; the secret is not passed in its arguments
or environment. Stop only the fixture's own daemon/session afterward.

With matching official Rust 1.98 host/Windows standard libraries, MinGW GCC,
headers, libraries and binutils, set `RUSTC`, `CARGO_TARGET_DIR`,
`CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER`, `CC_x86_64_pc_windows_gnu`, and
`AR_x86_64_pc_windows_gnu` to that isolated toolchain, then run:

```text
cargo check --workspace --all-targets --locked --target x86_64-pc-windows-gnu
cargo test -p synveil-client-sync -p synveil-platform --all-targets --locked \
  --target x86_64-pc-windows-gnu --no-run
```

This checks the workspace and links actual Windows test executables, including
bundled SQLite and the Windows Credential Manager adapter. It does not use host
SQLite through a pkg-config cross-check workaround. No native Windows runtime,
Credential Manager persistence, TLS, NTFS, reboot or power-loss evidence is
claimed; those remain platform/release-lab gates.

### Final validation and dependency policy

After the final code changes, rerun all fresh database/native cases above and:

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p synveil-client-sync --all-targets --locked
cargo clippy -p synveil-client-sync --all-targets --all-features --locked -- -D warnings
cargo deny check
npx --yes @redocly/cli lint api/openapi.yaml
git diff --check
```

The reviewed TLS license additions are exact-version exceptions in `deny.toml`:
ISC for `ring@0.17.14`, `rustls-webpki@0.103.15`, and `untrusted@0.9.0`, and
CDLA-Permissive-2.0 for `webpki-roots@1.0.9`. The global allowlist is unchanged;
upgrades require renewed review. Distributions must preserve the applicable
upstream license notices and root-data agreement. No advisory was suppressed.
OpenAPI lint warnings for public probe/status 4xx responses and unused shared
components are reported separately; they are not resolved by inventing API
behavior. DeviceBearer is not advertised for privileged system health.
