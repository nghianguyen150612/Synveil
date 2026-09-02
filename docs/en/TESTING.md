# Synveil testing, verification and benchmark strategy

Status: **Normative quality blueprint**

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

## Synchronization test contract

Synchronization receives unusually deep testing because an apparently small
ordering/retry defect can silently lose changes across every device.

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
8. Content based on a stale version preserves incoming and current bytes in the
   defined conflict representation. No last-writer-wins byte loss.
9. Metadata conflicts return current state/rebase instructions; directory
   ancestry remains acyclic and sibling names remain unique under the portable
   profile.
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
| `SYNC-014` Offline content/content conflict | Server v4; A offline produces v5A, B v5B; reconnect A then B, then reverse order in another run | First valid head and incoming stale bytes both persist under deterministic conflict grouping/copy; no byte loss; all devices converge. |
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
