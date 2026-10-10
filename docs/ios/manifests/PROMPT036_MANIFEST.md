# Prompt036 Manifest — Mutation Drain and Idempotent Recovery Engine

## Scope and Verified Baseline

P036 connects the P034 authenticated mutation repository and P035 durable SQLite queue through an explicitly invoked, lease-owned application service. This is an internal execution engine; user-facing editing, startup/network-restoration draining, background scheduling, conflict resolution, ACK, full rebaseline and offline Node caching remain deferred.

- Repository: `nghianguyen150612/Synveil`, reused existing checkout.
- Actual starting `origin/ios-app`: **`c25d2a0f7bc1316438d1a5f932d655f8b7528781`**.
- Feature branch: **`ios/p036-mutation-drain-recovery`**.
- Initial clean cloud `work` HEAD: `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc`.
- Verified requested remote, fetched the explicit ios-app refspec, checked the required P035 ancestry and exact integration tip, then created the feature branch from that tip. No nested clone, main implementation, codex prefix or dirty-worktree reset.

## References Inspected

Current iOS product contract, roadmap, platform mapping, P034/P035 manifests and OpenAPI mutation/checkpoint/error schemas; P034 mutation domain/strict decoder, P035 queue/models/codec/checkpoint service/SQLite store; authenticated Library request provider, SessionController, AppDependencyContainer and Rust bridge. Android `MutationEngine.kt` and `CacheDatabase.kt` ordering, reservations and recovery were inspected. Their automatic UNKNOWN replay and transient return-to-PENDING policies are not imported into P036.

Server evidence: `crates/core/src/domain/mutations.rs`, `crates/api/src/mutations.rs`, `crates/metadata/src/repository.rs`, `crates/metadata/src/mutations.rs`, and `migrations/20260827000002_client_mutation_operations.sql` plus conflict linkage migration.

## Coordinator and Production Capability Boundary

`MutationDrainCoordinator` is MainActor-isolated application orchestration with explicit `drain(scope:maximumOperations:)` and single-attempt `reconcileUnknown(scope:mutationId:)`. A positive limit of at most 100 is required. One invocation per coordinator is active; duplicate drain/reconciliation calls return a typed concurrent-execution stop. Independently created instances are additionally fenced by SQLite transactions, not just the memory flag.

Each execution acquires a committed queue-created `MutationSubmissionLease`, creates a fresh `AuthenticatedClientMutationRepository` with `DurableMutationAttemptAuthorizer(queue:lease:)`, and submits **lease.mutation**. The private lease request-provider adapter validates and uses the **original opaque lease session**; it cannot call provider.begin to borrow a replacement credential. The lease constructor and owner/attempt authority remain private to the queue implementation.

AppDependencyContainer composes the coordinator from its existing queue, Rust bridge, mutation request provider, secure store, SessionController and trusted bounded URLSession transport. Exactly one Keychain store is created. The globally composed mutation repository still has **nil authorizer** and fails closed. Queue/storage failure leaves the coordinator absent and read-only browsing available. No App or Feature invocation, raw SwiftUI mutation dependency, alternate queue/transport or permissive authorizer is added.

## Ordering, Dependencies and Dispatch

Selection requeries the oldest outstanding persisted row after each committed result, ordered by SQLite enqueue_order. P035's query already excludes APPLIED/FAILED_PERMANENT rows before LIMIT, so older terminal rows cannot starve PENDING rows. Maximum invocation/read/record/byte/outstanding limits remain enforced.

The documented conservative policy stops the entire affected scope at any earlier unknown, conflict, submitting or rebaseline-blocked operation, even if a later operation might be independent. No blocked row is discarded. P035 already rejects overlapping outstanding resource reservations at enqueue. P036 rechecks overlap and older outstanding rows under BEGIN IMMEDIATE at acquisition and authorization, and rejects another SUBMITTING record in the same scope across connections. Independent operations execute sequentially in durable order. No revision/precondition reconstruction follows APPLIED.

PENDING -> SUBMITTING commits before authorization/HTTP. Scope, persisted provenance/base, original identity/bytes, session, owner and attempt are verified. The durable dispatch_recorded marker is consumed once. Duplicate/wrong-record/wrong-scope/old-lease authorization fails without another POST. Normal in-process reuse never resets the marker.

Transport uses P034's sole DeviceBearer mutation endpoint, canonical HTTPS origin, stored Device and immutable Library scope, original JSON bytes, 16 KiB request/64 KiB response limits, 10/15-second timeouts, no cookies/CSRF/redirects and no application HTTP retry. BrowserSession direct metadata routes remain unused.

## Verified Outcomes and Persistence Failure

Only P034's strict decoder establishes APPLIED/CONFLICT/rebaseline/identity-conflict/permanent rejection. The existing evidence codec revalidates identity, Node, scope and journal/conflict projections on persistence and read.

APPLIED preserves replay flag, exact journal event/sequence and validated Node projection. CONFLICT preserves available conflict/revision/state/parent/name details, original mutation and replay flag without resolution. Mutation-ID conflict remains a distinct terminal FAILED_PERMANENT identity-conflict category. Rebaseline atomically writes BLOCKED_REBASELINE and reconciliation-required sync-base status, retaining the original epoch/sequence. No mutation result advances acknowledged checkpoint state.

Permanent rejection stops the current batch after persistence. Verified authentication rejection/revocation uses the existing session recovery handler and quarantines scopes; no stale success is published. Transient/invalid/lost responses remain unknown and stop. Typed results distinguish queue failures, result-persistence failures, safe transport/submission categories, unknown requiring explicit recovery, conflict review and rebaseline. Summaries contain counts only, without names, credentials or bodies.

Result persistence runs as one bounded local unstructured task so caller cancellation cannot prevent conservative uncertainty storage. It performs **no HTTP** and retains session/lease fences. Success counts are published only after SQLite finish and a current-session recheck. A failed COMMIT or lost acknowledgement does not report durable APPLIED. No POST is resent to compensate. Last committed SUBMITTING or terminal state survives; reopen converts interrupted SUBMITTING to UNKNOWN and preserves committed terminal states.

Predispatch failures have separate summary counts and allowlisted LOCAL_PRE_DISPATCH evidence. Once a lease exists their durable lifecycle remains conservatively OUTCOME_UNKNOWN, requiring explicit recovery. This deliberately avoids inferring rollback from cancellation or a lost SQLite acknowledgement. If a lease COMMIT outlives cancellation or loses its acknowledgement before a capability is returned, the queue performs one bounded, conditional local UNKNOWN persistence using the exact scope/owner/attempt. It cannot overwrite a rolled-back PENDING row, newer attempt or terminal result. LEASE_COMMIT_ACKNOWLEDGEMENT_LOST evidence is distinct from network ambiguity. When SQLite/quarantine prevents this local cleanup, the last committed interrupted state remains preserved for startup recovery. No networking or stale result publication occurs during cleanup.

## Explicit Same-ID Recovery and Server Contract

Normal drain never replays UNKNOWN. Explicit reconciliation verifies the current exact session/scope, persisted immutable operation, UNKNOWN source state, original verified base, prior uncertainty evidence, ordering/reservations and exclusive scope authority. It then acquires a fresh recovery lease and submits unchanged mutation ID, kind, epoch, sequence, payload and **identical request bytes** once. There is no retry loop, generated replacement ID, checkpoint refresh, Node GET proof or automatic precondition repair.

The inspected server uses `(owner_user_id, device_id, library_id, client_mutation_id)` as the durable operation key. Its version-1 canonical binary SHA-256 fingerprint includes epoch, sequence, closed kind and typed resource/revision/name fields; JSON representation and the mutation ID are excluded from that semantic hash. Namespace/owner/device/library authorization is checked before loading/replaying the operation. Fingerprint mismatch yields mutation_id_conflict.

`submit_client_mutation` loads a matching terminal operation and returns the persisted APPLIED or CONFLICT projection with replayed=true **before** validating the current base/retention floor. A previously uncommitted attempt validates exact journal epoch, retained-history floor and base <= sync head before making a new decision. Node update, journal event and terminal operation share the PostgreSQL transaction. The operation table's owner-pair foreign keys use DELETE RESTRICT; repository/migration searches found no TTL, pruning or deletion path for these operation rows. This describes the inspected implementation, not a promise of perpetual retention or behavior after an operator restore/server contract change. P036 requires an unchanged usable local base; the server remains authoritative and a returned epoch/history failure blocks the scope for rebaseline. No new server API or runtime negotiation guarantee is invented.

## Schema Version 2 and Historical Evidence

A versioned transactional migration verifies the exact P035 version-1 schema, adds `mutation_attempt_history`, replaces only the state-transition trigger with a guarded explicit UNKNOWN -> SUBMITTING path and sets user_version=2. Original table constraints, immutable mutation trigger, system SQLite linkage, encoding version and storage/file policies remain intact. Fresh databases create the same verified v2 schema. Unknown future or damaged schemas fail closed without fallback/repair.

Recovery archives the prior attempt ID, owner, start, dispatch marker and exact uncertainty evidence in the same transaction that changes the current attempt. The transition trigger requires that exact archive and a different attempt ID before evidence/marker reset. Original mutation/base/request columns are never updated. History has update/delete rejection triggers and a maximum of **eight recovery attempts per mutation**; a ninth returns recoveryLimit while retaining all evidence and the current UNKNOWN row. Archived evidence counts toward the existing 16 MiB logical limit and the 64 MiB SQLite page policy. Archive read/validation is bounded. The current attempt stores its eventual verified/unknown result, explaining both prior uncertainty and the new outcome. History descriptions are redacted.

## Crash, Cancellation and Session Isolation

Startup continues to run only P035's local SQLite recovery, without networking. Both initial and recovery SUBMITTING attempts become UNKNOWN on reopen, preserving original identity/base/bytes and prior history. Already committed APPLIED/CONFLICT remains terminal; late old-lease errors/successes and duplicate result writes cannot replace a terminal outcome.

Logout/recovery invalidates memory capabilities synchronously and serializes durable quarantine. Credential replacement, Device/origin/owner changes and lifecycle revisions fence acquisition, authorization, HTTP result handling, finish and publication. A stale request cannot borrow the new credential. Cancellation before acquisition sends nothing; after dispatch it preserves UNKNOWN or conservative interrupted state; cancellation after a committed result never claims rollback. Quarantined scopes are not automatically rebound by a matching Library ID or new login.

## Security and Deferred Features

P035 private Application Support storage, permissions, backup exclusion and requested Data Protection policy are preserved. No bearer/enrollment token, Authorization header, Keychain envelope, password or raw HTTP diagnostic is persisted or logged. Verified bounded result projections retain necessary logical metadata as before; counts and closed error categories remain safe. No claim of hardware encryption or physical power-loss durability.

Deferred: editing controls, automatic startup/restoration execution, timers/background workers, automatic UNKNOWN replay, conflict APIs/resolution, ACK, full rebaseline, Node cache, replacement intents and end-to-end user-facing offline editing.

## Files and Validation

Created:

- `clients/ios/Application/Mutation/MutationDrainCoordinator.swift`
- `clients/ios/Tests/SynveilTests/MutationDrainCoordinatorTests.swift`
- `clients/ios/Tests/SynveilTests/MutationRecoverySQLiteTests.swift`
- `clients/ios/Support/tests/test_mutation_drain_safety.py`
- `docs/ios/manifests/PROMPT036_MANIFEST.md`

Modified: AppDependencyContainer; AuthenticatedClientMutationRepository; DurableMutationQueue; MutationQueueModels; MutationPersistenceCodec; MutationQueueSQLiteStore; MutationQueueTestSupport; MutationQueueSQLiteTests (v2 assertions); RustBridgeConcurrencyTests (deterministic pre/in-flight cancellation scheduling, retaining both assertions); P035 source registration test; Xcode project. Three Swift files have unique app/test target membership; all four existing system SQLite link settings remain.

**88 new XCTest methods** (64 coordinator, 24 migration/recovery SQLite) plus three Python safety tests. Existing P024-P035 tests are retained. Gates/fault injection cover committed leases/markers before HTTP, order/limit/exclusive scope execution, original session/bytes, verified outcomes and no ACK, explicit replay, retained/bounded history, real migration/reopen/rollback, cancellation/logout/credential replacement, failed result persistence and terminal overwrite prevention. The P034/P035 regression suites supply additional strict DTO, origin, precondition, dependency, transport, SQLite corruption/busy/full and lifecycle cases; no sleep-based synchronization was added.

Local source validator, Python Support suite **47 passed**, documentation validator, strict Swift formatting and diff whitespace checks: passed at implementation inspection. Rust FFI suite **18 passed / 0 failed**. All Swift files passed syntax parsing; Domain/Application/Rust bridge/SQLite code compiled under Swift 6 strict concurrency in a temporary SwiftPM harness linked to real system SQLite and the actual Rust FFI static library. The harness ran P036, P034/P035, Library/Node, HTTP contract and Rust bridge suites: **629 tests passed / 0 failed**, including all 88 P036 methods. Only synchronous XCTest entrypoints in temporary test copies were made async for Linux actor-aware discovery; production sources were copied unchanged. Swift 6.2.3 was used after the initial downloaded Swift 6.2 runtime hit an unresolved Observation-library symbol during linking. A later portable rerun exposed the existing RustBridgeConcurrencyTests cancellation-start race (also reproduced in a targeted run). Its two cancellation assertions were retained; inherited MainActor scheduling fixes pre-cancellation ordering, and worker semaphores replace sleep assumptions for in-flight cancellation. No production Rust/FFI implementation changed. Linux execution is not Apple Simulator evidence; exact-head native CI is recorded below.

## Hosted Delivery Evidence

At initial manifest creation, feature publication/native CI/merge evidence is **PENDING**, not claimed complete. The source/test head will be frozen before verification; final hosted evidence is recorded through a documentation-only finalization commit so an immutable source SHA is never made self-referential.

Final feature SHA, PR URL, native Simulator totals, four exact-head iOS workflow runs, merge confirmation, integration SHA and unrelated CI status: **PENDING**.

Known P035 Simulator limitations remain: real Keychain entitlement round-trip and unavailable filesystem Data Protection attributes. Actual current-run skips will be recorded from native results. Physical-device validation: **NOT_AVAILABLE**. No physical power-loss test is claimed.

Readiness: **NOT_YET_VERIFIED** until genuine GitHub evidence confirms P036 merged into ios-app.
