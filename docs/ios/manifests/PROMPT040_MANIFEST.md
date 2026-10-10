# Prompt040 — Inbound Sync Coordinator & Explicit Sync UX

## Checkout and delivery

- Repository: `https://github.com/nghianguyen150612/Synveil.git`.
- Integration branch: `ios-app`; feature branch: `ios/p040-inbound-sync-coordinator`.
- Actual starting `origin/ios-app` SHA: `3e25912c03118236afb0210319f0b1211b48a560`.
- The initial clean checkout was `work` at `3851ea11927e24255614cfc38adbaccbd345ca03`. It was not used for implementation.
- Executed remote/status inspection, explicit forced ref fetch of `ios-app`, both SHA reads and the required P039 ancestry check (successful). No additional integration commits existed. Created the requested feature branch from `origin/ios-app`; no dirty reset or `codex/` branch.
- Final source feature SHA, genuine PR, exact-head native CI, hosted merge and resulting integration SHA: **PENDING** at implementation time. They will be recorded from GitHub evidence during delivery; this manifest does not assert a merge or native pass in advance.

## Authoritative references

Inspected the iOS product contract, roadmap and platform mapping; P036/P037/P038/P039 manifests; current `api/openapi.yaml`; existing feed/models/application/materializer/ACK, checkpoint/queue/drain, actor-owned SQLite/projection authority, SessionController, Library/Node UI, root/application composition; Android `SyncModels.kt`, `SyncEngine.kt`, `CacheRepository.kt`; server `crates/api/src/sync.rs` and `crates/metadata/src/sync.rs` journal/checkpoint/ACK code.

The actual DeviceBearer routes remain Device/Library `/changes`, `/changes/ack` and `/checkpoint`. GET delivery never advances acknowledgment. Signed HMAC evidence binds the original owner/Device/Library/epoch/from/through/high watermark; replay returns the current checkpoint without rewinding. Client coordination does not generate, parse or replace that opaque token. Server sync permits READ_ONLY; its scope loader rejects an inactive Device, wrong owner and deleting Library. Native QUARANTINED Library controls and durable quarantined scopes remain unavailable. The client never bypasses server authorization.

## Coordinator and invocation boundary

`InboundSyncCoordinatorProtocol` is the injected application boundary. `InboundSyncCoordinator` composes the real P038/P039 `SyncFeedService`, `SyncFeedApplicationService`, `SyncAckService`, `SQLiteNodeProjectionRepository`, existing `DurableMutationQueue`, authenticated provider and the same `MutationQueueSQLiteStore` connection. It never creates a transport, credential store, journal decoder, competing checkpoint or alternate cache. No outbound drain is called.

Only explicit Sync Now invokes `synchronize(scope:configuration:progress:)`. A separately confirmed operation invokes `recoverUnknownAcknowledgement(scope:position:confirmedByUser:progress:)`. Local scope capture and status inspection perform authenticated Keychain/local SQLite reads only. Construction, SQLite initialization/recovery, startup, screen appearance, connectivity and app foreground do not fetch, materialize, ACK, replay or drain.

A private, zero-byte OS advisory lock owned by the existing database actor globally rejects concurrent runs/recoveries, including independent coordinators and separate connections/processes sharing the database. It is opened with private permissions and no symlink following, is matched to the exact run UUID on release, and releases on process death. SQLite transactional projection exclusion, immutable commits, ACK attempt ownership and P039's separate cross-process ACK lock remain the write/dispatch authority; no UI Boolean grants database authority. Runs release ownership after success, typed failure or cancellation.

## Limits and preflight

Foreground default: 200 events/page, eight sequential pages, 4,096 newly applied events. Accepted configuration: page size 1...500, pages 1...64, events 1...4,096. Positive integer budgets are validated before any request; zero never means unlimited. The finite page loop includes resumed pages, reduces the next GET limit to the remaining event budget, and defers a larger already-staged unapplied page without deleting it. P039's stricter durable limits still apply: 32 retained pages per scope, eight ACK attempts per page, bounded cache/evidence capacity. There is no history eviction or silent reset at capacity.

Before networking, capture the exact current authenticated endpoint/owner/Device/credential and lifecycle revision through the existing queue/provider; validate Rust IDs, exact scope, credential binding and quarantine; read verified checkpoint provenance and projection status; inspect oldest unresolved inbound evidence and latest confirmed commit. Missing verified base returns checkpointRequired with no invented epoch/sequence or automatic setup. The UI offers the existing checkpoint preparation boundary with a confirmation explaining that server synchronization state may be initialized.

Status queries are scoped, parameterized, actor-owned and bounded to one oldest unresolved / one latest confirmed record. Unsigned decimal ordering uses string length then canonical text, not a lossy SQL cast. Returned pages are strictly re-decoded through P038, with session/credential/quarantine checks after awaits. ACK counts are bounded to eight. Latest confirmed pages require genuine projection commit and completed dispatched checkpoint evidence. No schema migration or version bump: schema remains v4.

## Durable recovery and sequential ordering

| Persisted page state | Explicit action |
| --- | --- |
| No unresolved page | Read one feed page from the verified checkpoint |
| RECEIVED_UNAPPLIED | Apply the exact original staged page before any new GET |
| APPLIED_ACK_PENDING | Obtain genuine receipt and ACK without rematerializing |
| ACK_IN_FLIGHT | Stop ordinary sync; expose uncertainty and confirmed recovery |
| ACK_CONFIRMED | Validate commit/checkpoint consistency, then continue without reapply/re-ACK |
| BLOCKED_REBASELINE | Stop and preserve cache, staged evidence, tokens and operations |

For each page: authenticated feed GET → strict validation/durable staging → exact canonical Node materialization → atomic SQLite projection COMMIT → genuine committed-page receipt and durable ACK lease → original signed token POST → strict checkpoint validation → atomic confirmation COMMIT/readback → next GET, only after original-session revalidation and budget checks. No cursor, sequence arithmetic or high-watermark shortcut is introduced. A repeated stale page fails strict checkpoint matching rather than looping.

P039 exact revision matching, all eight event kinds, complete-page atomicity, lifecycle/tombstones and purge protection are unchanged. A canonical revision that has advanced or regressed stops with metadataChanged; missing materialization remains distinct. No event is skipped, reinterpreted as an empty page or materialized indefinitely.

ACK outcomes remain confirmed/failed/unknown. Confirmation is read from durable projection/base evidence before publishing confirmed pages or advancing. A lost post-confirmation local return can be proven committed by readback, but that run stops progressed without another GET. A failed confirmation transaction or response loss preserves ACK_IN_FLIGHT. Receipt acquisition that commits then loses its return is also recognized by local inspection. An unused receipt is released locally if its captured session differs from the run's original identity; it never dispatches or claims rollback.

Explicit unknown recovery verifies oldest applied page, exact position/epoch/scope/current credential and P039 committed identity; uses recoveryReceipt; sends exactly one original-token replay; persists strictly verified checkpoint; stops without automatically fetching another page. Wrong position or missing confirmation cannot dispatch. Eight-attempt exhaustion disables recovery. There is no automatic token replacement or retry.

## Progress and stopping states

Immutable snapshots distinguish checking/fetching/staged/applying/applied/acknowledging/confirmed/stopped. Events increment only after verified application COMMIT/readback; pages increment only after verified ACK confirmation. Already applied resumed pages add no newly applied events. Locally applied, server-confirmed and latest observed high-watermark positions remain distinct. An observed empty feed updates the observed position and returns upToDate with no staging, materialization or ACK. Incremental success does not imply a complete snapshot.

Typed stop reasons include upToDate, progressed, moreWork, checkpointRequired, awaitingAckRecovery, reconciliationRequired, rebaselineRequired, metadataChanged, missingMaterialization, authenticationRequired, deviceRevoked, offline, serverUnavailable, storageFailure, protocolFailure, cancelled, committedButSessionChanged, scopeMismatch, invalidConfiguration, alreadyRunning, recoveryLimitReached and recoveryConfirmationRequired. Budget exhaustion reports “More changes may be available. Sync again to continue.” Only actual empty-feed observation produces “No new changes.”

Checkpoint regression/ahead/conflict stop with reconciliation guidance and P039 durable blocking. Rebaseline retains cache, tokens and queued operations; no bootstrap/reset/sequence-zero fallback is offered. Successful ACK may make immutable older outbound bases ineligible: P039 confirmation marks reconciliation, and this run stops while preserving original mutation ID/base/preconditions/payload/request bytes. P037 Send Pending Changes stays separate.

Projection UNINITIALIZED/PARTIAL/COMPLETE/REBASELINE_REQUIRED semantics are retained. Incremental-only caches remain PARTIAL; empty partial directories are not authoritative. No “Full Offline Copy” claim or offline substitution for the live Node Browser. No last-sync timestamp is fabricated; the UI shows actual saved server-confirmed positions and current-run counters.

## Session, cancellation and native UX

Original session is checked around every suspension boundary. ACK receipt identity must match that original session, preventing credential borrowing. Existing provider authentication/revocation recovery and SessionController lifecycle/quarantine remain authoritative. Late feed/materialization/readback/ACK results cannot publish another session's metadata. MainActor ViewModels retain immutable Library identity and session revision; logout invalidates/clears observable status and cancels the owned child task. Different Libraries get independent models.

Structured cancellation preserves committed facts: before GET nothing is dispatched; after staging the original page remains; during materialization no incomplete projection is published; after projection COMMIT pending ACK remains; after ACK dispatch cancellation is uncertain and requires explicit recovery. View disappearance or leaving active scene cancels the user-owned run and never schedules continuation. Parent cancellation forwards to the child. Local status reload after an uncertain cancellation exposes its durable recovery action without networking. No rollback is inferred from cancellation or response loss.

Native Library Catalog → Library-specific Sync Status uses SwiftUI List/navigation, progress indicator, Sync Now, cancel, explicit offline retry, checkpoint confirmation and separate original-ACK recovery confirmation. Saved unapplied and applied-pending states have distinct guidance. Unsafe retry is unavailable for conflicts/rebaseline/protocol/storage/authentication; READ_ONLY inbound sync remains eligible. Missing SQLite/coordinator presents honest unavailability while live browsing/mutation routing remains independently composed.

Stable `synveil.library.sync.*` / `synveil.sync.*` identifiers, VoiceOver phase/progress/position labels, readable updates, disabled-control explanations, confirmation identifiers, wrapping Library names and native Dynamic Type are included. Neither observable state nor accessibility text exposes bearer/token/raw SQL/server diagnostic bodies/provider paths.

## Source changes

Added:

- `Domain/Sync/InboundSyncModels.swift`.
- `Application/Sync/InboundSyncCoordinator.swift`.
- `Features/Sync/SyncStatusViewModel.swift`, `SyncStatusView.swift`.
- `Tests/SynveilTests/InboundSyncTestSupport.swift`, `InboundSyncCoordinatorTests.swift`, `InboundSyncRecoveryTests.swift`, `InboundSyncRaceTests.swift`, `SyncStatusViewModelTests.swift`.
- `Support/tests/test_inbound_sync_coordinator.py` and this manifest.

Modified existing database scoped reads/run ownership, ACK unused-receipt cleanup, checkpoint narrow protocol conformance, dependency/root/app/catalog injection, and Xcode source/test membership. SQLite linkage is preserved and all PBX identities/target memberships are checked. Six earlier test files receive only formatter-required blank-line separation before `@testable import`; their test behavior is unchanged.

## Tests and local validation

Added **129 XCTest methods**: 48 coordinator, 27 recovery, 22 cancellation/race and 32 status/native rendering tests. Native source registration retains all P024–P039 tests. Deterministic QueueGate, dispatch hooks and production SQLite fault injection replace sleeps.

New coverage includes config/identity/auth/checkpoint preflight; one/two/three-page ordering and actual commit-before-ACK / confirmation-before-next-GET inspection; exact budgets including 4,096 applied events and reduced final limit; no invented cursor/high skip/repeated-page loop; all eight events and revision/missing metadata stops; received/pending/unknown/confirmed/blocked restart states; real reopen, duplicate/conflicting evidence; application/lease/confirmation faults and lost returns; original-token single recovery/eight-attempt limit/concurrent recovery; separate coordinator/connection exclusion; quarantine/logout/readback/credential replacement; cancellation before GET, during GET/materialization, after stage/application, before lease and after dispatch/confirmation; scoped partial cache; injected and real-coordinator UI, double taps, offline/auth/revocation/setup/unknown/rebaseline/limits/long names/native Dynamic Type. Existing P039 materialization/parent/purge/partial atomicity and P037 mutation/browser suites remain registered and unchanged.

Local validation performed with downloaded official Swift 6.2 on Linux, unchanged production Domain/Application/SQLite/ViewModel sources and actual system file-backed SQLite in an ignored SwiftPM harness. Temporary test copies adapt synchronous MainActor XCTest discovery. The real Rust adapter test is skipped only in this harness because this worker has no Rust toolchain; native CI retains the original test. The harness uses allow-shlib-undefined for the downloaded Observation library. This is **Linux evidence, not Simulator execution**.

- Portable final test result: **653 executed / 652 passed / one Linux-only Rust skip / zero failures**.
- `validate_ios_sources.py`: PASS.
- Python Support suite: 70 passed (six new).
- Official strict swift-format and Swift parsing of all registered Swift files: PASS.
- Swift 6 production compilation/concurrency checking: PASS.
- Documentation validation and `git diff --check`: PASS before publication; repeated with this manifest below.
- No Linux Rust FFI unit-test result is claimed; hosted Rust Apple validation is required separately.

## Hosted native evidence and limitations

Fresh exact-source-head iOS Static Validation, iOS Build, iOS Simulator Tests and explicitly dispatched iOS Rust Apple Build: **PENDING**. Required native suite totals, artifact/run IDs, final feature SHA, genuine ios-app PR/merge and unrelated workflows will be filled from GitHub results, not inferred from P039.

The two known P039 Simulator limitations remain accurately reported if the native runner skips them: unsigned Keychain round-trip entitlement and unavailable Simulator filesystem Data Protection attribute verification. They are never counted as passes. Physical-device validation: **NOT_AVAILABLE**. No hardware-backed Keychain, device filesystem-protection round-trip or physical power-loss durability is claimed. Real file-backed SQLite transaction/reopen tests establish their exercised boundaries only.

## Deferred work

No startup/background/reachability/timer sync, automatic ACK/replay/outbound drain, conflict resolution, full rebaseline/snapshot bootstrap, file-content caching/transfers/downloads/uploads, PhotoKit/Quick Look/Share Sheet/File Provider or offline-browser replacement. Prompt041 readiness requires genuine hosted merge into ios-app and verified source/tests/manifest presence; it is not asserted before those gates.
