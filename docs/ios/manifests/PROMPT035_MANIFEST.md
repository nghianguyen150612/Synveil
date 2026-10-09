# Prompt035 — Durable Mutation Queue & Authoritative Sync Base

## Goal and Checkout

Implement the native SQLite metadata mutation persistence and authoritative checkpoint foundation. P035 provides no mutation worker, retry loop, editing UI, offline Node cache, signed ACK, or rebaseline execution.

- Repository: `nghianguyen150612/Synveil`.
- Integration target: **ios-app**.
- Actual fetched starting SHA: **`bab199d139382b09660495d3d03affb6d0dc9e94`**.
- Feature branch: **ios/p035-durable-mutation-queue**.
- The working tree was clean. Explicit fetch of `refs/heads/ios-app` and `git merge-base --is-ancestor bab199d139382b09660495d3d03affb6d0dc9e94 origin/ios-app` succeeded. HEAD and origin/ios-app agreed; no additional integration commits existed. The feature branch was created from that fetched integration ref.

## References Inspected

- `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`, `IOS_V0_1_ROADMAP.md`, `IOS_PLATFORM_MAPPING.md`, and P033/P034 manifests.
- `api/openapi.yaml`: implemented device/library checkpoint GET, SyncCheckpointResponse/Data, decimal and response metadata schemas; mutation request/result/error contracts.
- Existing P034 mutation models/decoder/repository, authenticated Library/Node provider, SessionController, secure credential protocol, Keychain implementation, URLSession transport, app composition and Xcode project.
- Android MutationModels, MutationEngine, CacheRepository and CacheDatabase: scoped storage, ordering, dependencies and attempt evidence. Room, automatic draining and its JSON-derived fingerprint were not ported.
- Server `crates/api/src/sync.rs`, `crates/metadata/src/sync.rs`, mutation idempotency/journal code in `crates/metadata/src/repository.rs`, and P034 shared domain/identity references. First checkpoint GET can insert acknowledged sequence zero at the actual journal epoch. Device/owner/Library authorization remains server-owned. Mutation POST does not ACK the consumer checkpoint.

## SQLite Architecture, Schema and Transactions

`MutationQueueSQLiteStore` is an actor owning one native SQLite3 connection. All transactions are synchronous inside that actor and never suspend while holding a writer transaction. Production composes one store. SQLite writer locking and uniqueness also protect independent connections used by tests.

- **Schema version 1**, registered with `PRAGMA user_version`.
- Tables: `scopes`, `sync_bases`, `mutations`, `dependencies`; two indexes and two defensive triggers.
- Scope identity: canonical endpoint, owner, Device and Library, enforced by a unique composite key. A non-secret credential identifier binds the original credential generation; quarantine is durable. Credentials are never database inputs.
- Mutation identity: unique `(scope_id, mutation_id)`. Database-generated `INTEGER PRIMARY KEY AUTOINCREMENT` provides durable enqueue order. Timestamp is metadata, not execution order.
- Initial version-0 migration is allowed only for an empty database. Schema creation and version registration share a transaction. No older deployed queue schema exists. Unknown versions, malformed current schema, extra/missing schema objects, failed quick checks and broken foreign keys fail closed. No destructive reset or drop/recreate recovery exists.
- Current-schema SQL definitions are checked against the registered schema, including immutable-field and transition triggers. Future changes require an explicit versioned migration preserving pending operations.
- Connection configuration: **foreign_keys=ON, journal_mode=WAL, synchronous=FULL**, bounded **250 ms busy timeout** (validated test override up to 5 seconds), WAL auto-checkpoint every 64 pages and 2 MiB retained journal-size policy.
- Transactions use **BEGIN IMMEDIATE / COMMIT**, parameterized data bindings and rollback on pre-commit errors. Static SQL identifiers and validated local PRAGMA policy values never incorporate untrusted strings.
- SQLite busy/locked, full, corrupt/not-a-database/schema/constraint, I/O and open errors have typed safe outcomes. No raw SQLite diagnostics or names are logged. Failed rollback poisons the connection: subsequent reads/writes fail closed, preventing exposure of uncommitted working rows. A separate WAL reader/relaunch retains the last committed state.
- Success follows COMMIT. An injected lost acknowledgement after COMMIT produces `commitAcknowledgementLost`; enqueue reads back and verifies the exact durable operation. Cancellation/logout after a completed write yields `committedButSessionChanged`, retaining the committed record rather than claiming rollback.

## Immutable Records, Rehydration and Idempotency

Rows store mutation ID, scope FK, exact decimal epoch/sequence TEXT, closed kind, canonical payload BLOB, exact original request BLOB, encoding version, enqueue order/time, closed lifecycle state, attempt ID/owner/time/dispatch marker and bounded outcome evidence. Immutable fields cannot be updated through the database trigger.

Version-1 rehydration validates the complete record, scope IDs with the existing Rust bridge, canonical UUIDv7 identity, nonzero U64 epoch and exact sequence, kind-specific payload keys/types/revisions/names, resource dependency rows, attempt metadata and evidence. It constructs the original P034 `PreparedClientMutation` with its original ID and base, using **P034's existing sole request encoder**, and requires byte-for-byte equality with both stored canonical payload and request bytes. Truncated/noncanonical/mismatched/unsupported records fail closed and remain preserved. No repair, new ID or body rewrite occurs.

Identical duplicates return the existing row and order. Different immutable semantics produce `duplicateIdentity`. The transaction and unique key protect racing enqueue calls. There is no `INSERT OR REPLACE` and no local authoritative semantic fingerprint; server idempotency remains authoritative.

## Checkpoint and Sync-Base Provenance

`SyncCheckpointService.prepare(scope:)` is explicit and reuses the existing authenticated Library request provider, Keychain and session fence. The route is:

`GET /api/v1/devices/{device_id}/libraries/{library_id}/checkpoint`

No startup, browsing, enqueue or recovery method invokes checkpoint preparation automatically. The GET can initialize server state and is not represented as side-effect-free. HTTPS/base-path/origin binding, finite existing transport limits, no cookies, no CSRF, no credential URLs and current-session recovery protections are reused.

The strict checkpoint decoder bounds the envelope to 16 KiB, checks JSON content type, required root/data/meta fields and unknown fields, exact requested canonical Device and Library identities, safe body/header request IDs, canonical U64 decimal strings, nonzero epoch, timestamps with created <= updated, and optional watermark >= acknowledged sequence. Explicit null/numeric/overflow/noncanonical values fail closed. Exact values through `18446744073709551615` remain text; floating-point conversion is absent.

Verified original response bytes and epoch/sequence are committed atomically under the scope. A read redecodes the provenance and requires agreement with stored base columns. Typed statuses distinguish VERIFIED, INVALID and RECONCILIATION_REQUIRED; absence is `syncBaseUnavailable` and wrong authenticated scope is rejected. Malformed successful checkpoint data invalidates an existing verified base. Offline/503 failures retain a valid older base and credentials. Verified auth/revocation responses use the existing SessionController recovery owner.

Offline enqueue requires an already verified exact-scope persisted base. No epoch/sequence defaults are manufactured. Server-returned sequence `"0"` is valid. A changed checkpoint incompatible with any outstanding record, or a rewind, records reconciliation-required status and blocks preparation/attempts. Original mutation IDs, bases, payloads and request bytes remain untouched. P035 cannot automatically clear this reconciliation condition.

## Persistent State Machine, Ownership and Recovery

Closed states: PENDING, SUBMITTING, OUTCOME_UNKNOWN, APPLIED, CONFLICT, BLOCKED_REBASELINE and FAILED_PERMANENT, enforced by SQLite constraints.

Allowed graph: PENDING -> SUBMITTING; SUBMITTING -> APPLIED / CONFLICT / OUTCOME_UNKNOWN / BLOCKED_REBASELINE / FAILED_PERMANENT. Source state, exact scope and attempt ownership are checked transactionally. Terminal states cannot return to PENDING or accept late network overwrites. P035 deliberately provides no OUTCOME_UNKNOWN replay transition; explicitly authorized same-ID reconciliation belongs to P036/later lifecycle work.

- Acquiring a lease commits SUBMITTING and non-secret attempt metadata before future transport invocation. A private queue-created memory capability binds one exact prepared operation, opaque current session and queue-owner generation.
- `DurableMutationAttemptAuthorizer` verifies the SQLite row, original identity/bytes/base/scope, strict rehydration, verified persisted checkpoint, current Keychain/lifecycle, SUBMITTING state and exact owner/attempt. It transactionally consumes a single durable dispatch marker. Concurrent or repeated authorization cannot both succeed.
- On startup/local recovery, SUBMITTING becomes OUTCOME_UNKNOWN transactionally, preserving every immutable field and attempt marker. Queue-owner memory is invalidated. Recovery never POSTs, regenerates IDs, rewrites bases or reports definitive server failure.
- APPLIED persistence retains replay flag, journal event ID, exact journal sequence and the actual validated Node projection. CONFLICT retains optional conflict identity/reason, server epoch/sequence and optional observed details. Evidence is serialized as bounded local projections and decoded again through the P034 strict response decoder on read. Server diagnostic messages and HTTP headers are not retained.
- Verified permanent rejection and mutation-ID conflict are terminal. Rebaseline-required results retain the operation and mark its persisted base reconciliation-required. Unknown/cancelled/local attempt failures remain conservative unknown outcomes.
- APPLIED never advances acknowledged sequence or creates a fabricated Node cache.

## Dependencies and Resource Limits

Node/parent/destination identities explicitly present in P034 intents are stored in dependency rows. Outstanding operations, including conflicts and unknown outcomes, transactionally reserve overlapping resources. Independent Node operations retain distinct IDs and enqueue order. P035 has no Node cache from which to invent unknown current-parent dependencies and does not claim a completed scheduler.

Local defensive policies (not server limits): 256 outstanding operations per scope; 16 MiB combined request/payload/outcome/checkpoint BLOB bytes; 4096 total mutation records including terminal evidence; 1024 scopes; maximum 100 rows per read; existing 16 KiB request/payload and 64 KiB outcome bounds. Existing records are never evicted to make capacity. A 64 MiB SQLite page cap and startup size check additionally bound database pages; WAL checkpoint/journal retention policies complement the logical limits. Resource-limit errors preserve pending work.

## Storage Security, Backup and Session Isolation

Production location: app-private **Application Support/Synveil/MutationQueue/mutations.sqlite**. Directory permissions are 0700; database/WAL/SHM permissions are 0600. iOS **completeUntilFirstUserAuthentication** protection is set on the directory and database/sidecars, matching Keychain **AfterFirstUnlockThisDeviceOnly** availability. Parent protection covers sidecar creation; attributes are reapplied before commits. Physical-device initialization/commits additionally read back and verify the protection raw value, accepting Foundation String/wrapper representations and failing closed on mismatch. Symbolic-link database/queue-directory paths fail closed.

The queue directory and database/sidecars are deliberately **excluded from OS backup** because operations are device-credential-bound. This trades away restoration of unsent work after reinstall/device loss. No silent migration to a replacement Device/owner/origin occurs. Platform file protection does not establish application-level or end-to-end SQLite encryption.

Bearer/enrollment tokens, Keychain envelopes, Authorization headers, cookies, passwords and raw diagnostics are absent from persistence APIs. Prepared operations, queue records and evidence have redacted descriptions; errors carry closed safe categories.

SessionController's existing transition owner invalidates queue capabilities immediately when leaving authentication. Quarantine writes are serialized and awaited before new authenticated queue actions. Logout/recovery preserve inaccessible scoped records rather than deleting valuable edits. Credential replacement is detected through full opaque-session comparison at capture/fence points, including a deterministic replacement between two capture checks. Persisted credential IDs and quarantine block inheritance on restart. Wrong Device/owner/origin is rejected; wrong Library cannot look up or borrow another Library's base/record. SQLite writes can outlive task cancellation; final fences deliberately distinguish committed-but-unpublishable records.

## Production Composition and Deferred Dispatch

AppDependencyContainer initializes SQLite away from MainActor, recovers interrupted local records without networking, and composes the queue/checkpoint service with the existing Rust bridge, Keychain store, SessionController and browser request provider. SQLite initialization/recovery failure leaves existing read-only browsing available and exposes only a typed queue failure.

**The production mutation repository retains P034's nil-authorizer gate.** The new lease-bound authorizer is tested independently and is not installed in production. No fallback unpersisted POST exists. Native Node Browser/File Details remain read-only. No automatic worker, startup POST, retries, timer polling, conflict resolution, rebaseline execution, ACK, offline Node cache, transfer, preview/sharing or background scheduler is added.

## Files

Added:

- `clients/ios/Domain/Mutation/MutationQueueModels.swift`
- `clients/ios/Domain/Mutation/MutationPersistenceCodec.swift`
- `clients/ios/Domain/Mutation/SyncCheckpointModels.swift`
- `clients/ios/Domain/Mutation/SyncCheckpointResponseDTO.swift`
- `clients/ios/Application/Mutation/DurableMutationQueue.swift`
- `clients/ios/Application/Mutation/SyncCheckpointService.swift`
- `clients/ios/Infrastructure/Persistence/MutationQueueSQLiteStore.swift`
- `clients/ios/Tests/SynveilTests/MutationQueueTestSupport.swift`
- `clients/ios/Tests/SynveilTests/MutationQueueSQLiteTests.swift`
- `clients/ios/Tests/SynveilTests/DurableMutationQueueTests.swift`
- `clients/ios/Tests/SynveilTests/SyncCheckpointTests.swift`
- `clients/ios/Support/tests/test_durable_mutation_queue_registration.py`
- `docs/ios/manifests/PROMPT035_MANIFEST.md`

Modified:

- `clients/ios/App/AppDependencyContainer.swift`
- `clients/ios/Application/Library/AuthenticatedLibraryRequestProvider.swift`
- `clients/ios/Application/Session/SessionController.swift`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`

All new Swift sources have unique PBX references and exact app/test membership. All four target build configurations link system `-lsqlite3`. No deployment target, server, Rust, Android, desktop, web or workflow source changes.

## Tests and Local Validation

- **149 new XCTest methods**: 40 native/portable file-backed SQLite tests, 49 checkpoint tests and 60 queue/lifecycle/authorizer tests. Test support is separate and registered in the test target.
- Deterministic real SQLite tests cover initialization/versioning/schema verification, reopen/multiple connections, WAL/full durability, atomic enqueue/rollback, duplicates/concurrent callers, capacity/order/scope isolation, busy writer locks, injected disk-full/I/O failures, unsupported/corrupt database preservation, immutable triggers, bounded reads, original BLOBs, permissions and native Data Protection/backup attributes.
- Faults cover before INSERT, after INSERT/before COMMIT, COMMIT failure/lost acknowledgement, migration, SUBMITTING, dispatch ownership, before APPLIED and during conflict persistence. Async gates additionally test real commit completion before cancellation/logout reaches the caller. Tests use temporary databases and no sleep-based synchronization assumptions.
- Checkpoint tests cover all strict wire validations, exact large decimals, route/auth/origin, explicit initialization, durable provenance, missing/offline/wrong-scope bases, changed checkpoints, auth/revocation and stale results.
- Queue tests cover every P034 kind, real Rust validation, immutable rehydration/corruption, all states/graph, preserved journal/conflict/unknown evidence, crash-boundary recovery, no replay, dependencies, leases/concurrency, logout/replacement/recovery and production gate.
- **3 new Python tests**, full iOS Support suite **44 passed**. Artifact validator regressions **6 passed**. Source/architecture validator, strict Swift formatting, all Swift syntax parsing, Domain/Application Swift 6 strict-concurrency typechecking, documentation checks and `git diff --check`: **PASS**.
- Linux SwiftPM harness compiles copied production Domain/Application/Rust bridge/SQLite code under Swift 6, links native system SQLite and the actual Rust FFI static library, and runs new P035 plus existing P034/Library/Node/HTTP/Rust bridge suites. Only synchronous XCTest entrypoints in temporary copies become async for Linux actor-aware discovery. **540 tests passed, 0 failed**. This is portable execution, not Apple Simulator evidence.
- Rust FFI regression suite: **18 passed, 0 failed**; no Rust source changed.
- Initial feature head `b6878ba8d063d67ae1bdcd6db91775104bc3337f` native Simulator run [37971903665](https://github.com/nghianguyen150612/Synveil/actions/runs/37971903665) reported **866 total / 864 passed / 1 skipped / 1 failed**. The sole failing method cast Foundation Data Protection attributes only to the Swift wrapper. The fix normalizes wrapper/String values, separately verifies requested policy and requires physical-device read-back. A Simulator-only skip is permitted only if all protection attributes are actually absent; no physical protection claim follows from that case.
- Final-head hosted native Simulator, exact-head workflow and merge evidence: **PENDING publication/verification**. No native result is inferred from Linux.
- Established real-Keychain Simulator round-trip skip through P034 is preserved. P035 will report its actual final-head result. Physical-device validation: **NOT_AVAILABLE**. No physical power-loss, device encryption or real Keychain success is claimed.

## Hosted Delivery Evidence

Feature commit, genuine PR targeting ios-app, exact-head required iOS workflow results, native SQLite/Keychain counts and verified merge/integration SHA will be finalized only after actual GitHub evidence exists. This implementation-stage manifest does not claim completion or readiness for Prompt036.
