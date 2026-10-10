# Prompt039 — SQLite Node Cache & Atomic Feed Application

## Integration record

- Repository: `https://github.com/nghianguyen150612/Synveil.git`.
- Actual starting fetched `origin/ios-app`: `f0807e070a5290170f8f9b3826df690784012cd0`.
- Feature branch: `ios/p039-atomic-node-projection`; target: `ios-app`.
- Reused the existing clean repository. Initial `work` checkout was `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc`, not the implementation baseline.
- Inspected remote URL and status, fetched the explicit integration ref, checked the supplied P038 integration SHA is an ancestor, and created the feature branch from that ref. The fetched tip exactly matched the supplied SHA; no additional integration commits needed review. No destructive reset or main-based implementation occurred.
- Frozen final feature SHA: `7d189beafa8947d5345304a924edcd6e5948eaa1`.
- Genuine feature PR: [#110](https://github.com/nghianguyen150612/Synveil/pull/110), base ios-app, ready (`draft=false`) before squash merge.
- Independent GitHub REST confirmation: `merged=true`, `state=closed`, exact frozen feature head; merged at `2026-10-10T08:54:11Z`.
- Source squash merge and resulting hosted ios-app SHA: `c390f01c2f5a114578c286ee24cda8416aa9d986`.
- GitHub ref API and explicit integration fetch independently matched that SHA. Diff of clients/ios and the initial manifest between frozen feature head and integrated head was empty.
- Hosted evidence is finalized on documentation-only `ios/p039-manifest-finalization`, based on that source merge. No production Swift, tests, project or workflows change after the frozen feature head. Its resulting hosted tip is recorded in the final report rather than embedded as its own circular commit identity.

## References and server contract

Inspected the iOS product contract, roadmap and platform mapping; P035, P036, P037 and P038 manifests; the current OpenAPI SyncFeed, SyncChange, NodeResource, NodeAttributes and SyncCheckpoint schemas; existing domain/feed/ACK, authenticated Node/Library provider, mutation queue/checkpoint, session and production composition code. Android SyncModels, SyncEngine, CacheDatabase and CacheRepository establish bounded work and parity context.

Server sources include `crates/api/src/sync.rs`, `files.rs`, `router.rs`, and metadata `journal.rs`, `sync.rs`, `repository.rs` and `retention.rs`. Journal facts are appended with the canonical mutation transaction. Feed pages use a bounded consistent journal read; the ordinary Node GET returns current metadata, with no historical snapshot guarantee. Purge deliberately omits live parent/state/version metadata and retains identity, kind and last logical revision. Signed ACK replay returns the current checkpoint when it already covers an older token; a new token cannot skip the current checkpoint.

Production uses the established DeviceBearer feed/ACK endpoints. Canonical Node metadata uses the P031/P033 `getNodeMetadata(libraryId:nodeId:)` repository and the same authenticated provider infrastructure. There is no new HTTP transport implementation, file-content route or browser credential mechanism.

## Architecture and authoritative local boundary

P038 durable `inbound_pages` → explicit `SyncFeedApplicationService` → bounded `SyncNodeMaterializer` → one non-suspending transaction on the existing `MutationQueueSQLiteStore` actor → Nodes, event identities, locally applied position, immutable projection commit and APPLIED_ACK_PENDING together → explicit P038 `SyncAckService` with production `CommittedSQLiteSyncProjectionStorage` → verified atomic checkpoint confirmation.

`SQLiteNodeProjectionRepository` exposes authenticated scoped local Node, children and projection-state reads. It performs Keychain/session/Rust validation and SQLite queries, without HTTP or POST. Reads recheck immutable rows after Rust suspension and validate the original session before publication. The P032 live catalog/browser/details presentation remains in place.

`AppDependencyContainer` constructs all three new application boundaries with the existing Rust bridge, session, Keychain, providers, mutation queue and database. Database initialization failure leaves authenticated read-only browsing available and no unpersisted ACK fallback. Construction and reopen initiate no feed, Node materialization or ACK request.

## Schema v4 and migration preservation

Schema version advances from 3 to 4. Fresh creation and exact v1, v2 and v3 upgrades run in a transaction. Prior schema SQL is compared exactly; existing integrity and foreign keys are checked before migration. Unknown future versions, damaged schema, malformed private encoding and unsupported projection versions fail closed. Migration failure rolls back DDL and user_version.

The scopes, sync_bases, mutations, dependencies, mutation_attempt_history and inbound_pages tables are retained. v3 migration adds projection objects and replaces only the prior application gate. It does not rewrite immutable mutation UUIDs, request/payload bytes, original epochs/sequences/preconditions, dependencies, APPLIED/CONFLICT/OUTCOME_UNKNOWN evidence, recovery history, checkpoint provenance, quarantine, inbound response bytes or signed tokens. Existing v1/v2 upgrade fixtures now remove only empty v4 objects before reconstructing their exact historical schema.

New tables:

| Table | Durable purpose |
| --- | --- |
| cached_libraries | Versioned authoritative Library observation, keyed by existing scope and epoch |
| cached_nodes | Canonical identity, exact revision/sequence, parent, lifecycle, provenance and optional complete/last-known metadata |
| node_projection_state | Epoch, incremental anchor, locally applied sequence, server-confirmed sequence and completeness |
| projection_commits | Immutable page key, commit UUID, event count and exact canonical feed evidence |
| projection_events | Unique scoped epoch+sequence and event identity, subject, revision, kind and owning page |
| sync_ack_attempts | Original page ownership, immutable attempt UUID/owner/bounds, durable dispatch marker, completion and verified checkpoint response |

cached_nodes has a composite foreign key to the exact projection scope/epoch; commits reference the original scoped inbound page; events and ACK attempts reference their commit. Unique keys use canonical IDs rather than names. The existing scope identity includes origin/base endpoint, owner, Device and Library; credential binding and quarantine are enforced at every storage operation. Epoch keys prevent sharing projection data between journals.

## Metadata model and completeness

`CachedNodeRecord` separates ACTIVE, TRASHED and PURGED lifecycle from CANONICAL, EVENT and LAST_KNOWN provenance. Available authoritative metadata stores Node/Library/parent/FileVersion IDs, exact revision, Unicode logical name, kind/state, created/updated/trash timestamps, restore deadline and purge eligibility. IDs reuse Rust validators; revisions and sequences remain canonical u64 decimal strings. Private metadata encodings are version 1, bounded and validated again on read. No size, hash, MIME, path, byte cache or transfer status is added.

Only an ACTIVE canonical record whose metadata revision equals its recorded revision is an ordinary complete cached item. Trash last-known metadata and purge/event records cannot become ordinary files/folders or authorized Restore targets. A Node read includes its projection state; stale/reconciliation data is distinguishable. Children are bounded to 1...500, ordered by canonical Node ID, omit nonactive records and refuse active browsing through known inactive ancestry. Known local cycles and excessive ancestry depth fail closed.

Completeness vocabulary is UNINITIALIZED, PARTIAL, COMPLETE and REBASELINE_REQUIRED. P039 never installs COMPLETE. The initialized server checkpoint is an incremental cursor, not evidence of a full Library tree. The first applied page records that anchor and a truthful partial projection. Empty partial/missing directories do not claim authoritative remote emptiness. COMPLETE readback fails closed until a later proven snapshot implementation exists.

## Materialization and event policies

Network work completes before the write transaction. The service reads and strictly validates durable feed evidence, captures the exact authenticated session, groups final per-Node facts, performs sequential bounded authenticated lookups, validates immutable prepared metadata, rechecks the session, then enters SQLite. Parent observations verify directory identity and scope; they are not installed as unproven historical parent metadata.

Canonical metadata must equal the final event revision represented for that Node within the page, with exact parent and current FileVersion agreement and compatible kind/state. Earlier events for a repeated Node remain immutable journal facts; only its final materialization is installed. Current metadata later than that revision returns typed reconciliationRequired; earlier metadata returns revisionRegression. Neither case applies or ACKs the page. No page-cut snapshot guarantee is invented.

| Change | Application policy |
| --- | --- |
| NODE_CREATED | Validated canonical active metadata; no invented name/timestamps |
| NODE_RENAMED | Same canonical Node row; exact Unicode name from authoritative metadata |
| NODE_MOVED | Canonical parent, directory validation and bounded local cycle checks; one atomic row membership change |
| NODE_TRASHED | Incomplete logical trash record; preserve valid last-known fields with explicit provenance; no synthetic trash/restore/content fields or required GET |
| NODE_RESTORED | Fresh validated active canonical metadata supersedes trash evidence within the page transaction |
| FILE_CONTENT_COMMITTED | Validated canonical file metadata and exact current FileVersion; no bytes |
| FILE_VERSION_RESTORED | Same metadata-only policy with canonical current FileVersion identity |
| NODE_PURGED | No GET; invalidate active metadata and retain permanent scoped revision/sequence tombstone; absent subjects are valid |

A final purge supersedes earlier facts without stale GET materialization. A purge cannot be followed by resurrection inside a page, and persisted purges cannot be updated/recreated by later/stale events or canonical responses. Event revisions strictly advance for each Node except purge may retain the last revision, matching the server contract. New events cannot overwrite equal/older local revisions. Duplicate IDs/sequences across committed pages and conflicting page evidence fail closed; valid same-page application and feed replay are idempotent.

## Atomic application and progress

The transaction rechecks credential binding/quarantine, original durable page, verified checkpoint epoch/provenance, current locally applied position and exact page start. P038 decoding already verifies every consecutive sequence and exact through/high bounds; the service never advances to high_watermark. A later page cannot skip an unapplied range.

All event rows, Node/lifecycle/parent changes, immutable commit identity, applied position and inbound APPLIED_ACK_PENDING transition commit together. SQLite error/capacity/busy/cancellation before transaction leaves the original page unapplied; a first/final Node write or COMMIT fault rolls back everything. COMMIT acknowledgment loss is resolved by original-page/projection readback. Post-commit cancellation/session loss never claims rollback.

Local applied and server-confirmed positions remain separate. Outbound APPLIED results do not update either. Server-confirmed refresh/ACK is monotonic: regressing checkpoints retain original position/provenance and mark reconciliation; observations beyond proven applied progress cannot advance the local projection or acknowledged base. ACK confirmation retains the latest strictly validated checkpoint response and never rebases existing outbound operations. Incompatible outstanding bases remain immutable and require reconciliation for future submission.

## Database gate and genuine receipt authority

The v4 inbound transition trigger requires connection-local SQLite function authority for the specific apply/claim/confirm mode plus matching immutable projection commit, exact canonical page, complete event count and applied-position evidence. Protected projection table writes are guarded; commit/event evidence is immutable and retained; purge updates are forbidden; ACK identity/bounds are immutable and dispatch/completion are monotonic. A standalone state UPDATE or arbitrary Swift sequence/Boolean cannot authorize ACK, including on a separate raw connection without the registered function. The function is available only on the actor-owned connection and enabled only inside authenticated non-suspending storage operations.

Production claim requires a committed applied page and atomically inserts an attempt and transitions APPLIED_ACK_PENDING → ACK_IN_FLIGHT. Proof binds original signed token/page/scope, applied/previously confirmed positions and commit UUID plus attempt UUID. P038's private receipt further binds capability owner and exact session. Validation rechecks durable evidence; changed token/scope/bounds/commit or another service's receipt cannot dispatch.

A private zero-byte OS advisory lock serializes live ACK ownership across connections/processes in addition to SQLite's transactional ownership. It contains no token or metadata, is opened without following symlinks with private permissions, and is released when the explicit operation ends or its process exits. Conservative policy permits one live ACK attempt per database. A durable dispatch marker is acquired before HTTP; duplicate receipt callers cannot POST twice or release another active attempt. No dispatch authorization is returned before lease COMMIT.

## ACK, ambiguity and explicit recovery

ACK submits the original token unchanged to POST `/api/v1/devices/{device_id}/libraries/{library_id}/changes/ack`, using DeviceBearer with no Cookie/browser CSRF. The strict checkpoint decoder validates the response envelope, Device, Library, epoch and canonical sequence. It must cover the delivered page, be monotonic and stay within proven locally applied progress. SQLite atomically stores the confirmed checkpoint, confirmed position, ACK_CONFIRMED state and attempt completion before success is reported.

Timeout, cancellation, response loss, malformed response, late session loss or confirmation persistence failure retains Nodes, commit, token and ACK_IN_FLIGHT uncertainty. A confirmation already committed but whose local return fails remains durably confirmed, without a false rollback claim. Reopen does only local P035 recovery and restores evidence; it never sends network requests.

Explicit `recoveryReceipt` acquires a fresh serialized attempt for the same committed page/token/epoch/current credential, preserves previous attempt evidence, refuses live ownership and earlier missing/unconfirmed pages, and makes one POST only when explicitly acknowledged. Attempts are capped at eight. A replay checkpoint further ahead is accepted only with committed intervening page evidence; otherwise it blocks for reconciliation. No regenerated token, automatic retry, forced ACK, infinite loop or background worker exists.

sync_rebaseline_required, checkpoint_conflict or checkpoint-ahead responses preserve cache, staging, tokens and queued mutations while durably marking the scope/inbound evidence for reconciliation. No snapshot fetch/swap, cache deletion, journal reset or automatic conflict resolution is implemented.

## Session, security and storage limits

Existing session revision, credential/origin/owner/Device fencing and P035 quarantine extend to projection access/application and ACK publication. Logout/replacement invalidates original authenticated operations; durable Nodes/evidence remain scoped and quarantined. Failed durable quarantine also closes projection/ACK authority in the current process. Reads retain no in-memory result cache. Live read-only browsing remains independent.

Inherited storage policy remains private Application Support, WAL, synchronous FULL, foreign keys, bounded busy timeout, requested Data Protection attributes, backup exclusion, 16 MiB logical evidence budget and 64 MiB SQLite page ceiling. Cache/commit/event/attempt bytes are included in accounting. Further bounds: one page of at most 500 events, at most 1,000 sequential subject+parent lookups/prepared records, 2 MiB materialization plan, 16 KiB per metadata record, at most 500 subject writes + 500 event writes plus fixed transaction overhead, 16,384 cached Nodes and 16,384 applied event identities across the database, 32 retained pages per scope and eight ACK attempts per page. No recovery-critical data is evicted; reaching a bound rolls back/preserves staging and cannot authorize ACK.

## Tests and local validation

Added **151 XCTest methods**: 55 NodeProjectionTests, 54 NodeProjectionSQLiteTests and 42 CommittedProjectionAckTests. Final portable Swift 6.2 run: **526 executed / 525 passed / 1 skipped / 0 failed**, including P035–P038 queue/checkpoint/drain/feed/ACK regression suites. The registered XCTest suites are NodeProjectionTests, NodeProjectionSQLiteTests and CommittedProjectionAckTests, with NodeProjectionTestSupport using real temporary file-backed SQLite and the production storage capability. Dedicated P038 fake projection fixtures remain only in their original protocol tests.

Coverage includes invalid metadata/IDs/revisions/timestamps/private versions, exact decimals and Unicode; all eight events, current-revision advance, malformed/missing/wrong-scope/kind/state/parent metadata, parent cycles, repeated final subjects, trash/restore/move/purge and stale resurrection; scoped reads and truthful partial directories; v3 preservation of APPLIED/CONFLICT/UNKNOWN request/payload/evidence/history/checkpoint/staging/quarantine and prior v1/v2 paths; migration rollback/corruption/future schemas; page rollback, real uncommitted connection interruption, lost COMMIT readback, reopen, capacity/busy, concurrent same-page application and feed replay; genuine production ACK proofs, forged evidence, receipt ownership, cross-connection lease exclusion, same-token recovery, bounded history, checkpoint boundaries, original authenticated endpoint/token, response loss, confirmation failures, outbound immutability and logout/revocation/replacement/quarantine faults.

Deterministic QueueGate and QueueFaultInjector boundaries cover before/after materialization, first/final Node write, projection COMMIT and return, ACK lease COMMIT, server-response/local-confirmation separation, confirmation COMMIT, migration and quarantine. No arbitrary sleeps or hardware power-cut claims are used.

Linux validation uses a downloaded Swift 6.2 portable SwiftPM harness in locally ignored work files, copying production Domain/Application/SQLite sources unchanged. It links actual system SQLite through its C header/module map. Temporary test copies adapt MainActor synchronous XCTest discovery; the real-Rust rehydration case is skipped only in this harness because no Rust toolchain is installed. Linux link uses allow-shlib-undefined for the downloaded toolchain's Observation library. This is not Simulator evidence.

Required source validator, **64 Python tests** (six new P039 cases), documentation validator, git diff --check, official strict swift-format lint and portable Swift 6 compilation/typechecking/test execution all **PASS** on the source prepared for publication. No Linux Rust FFI unit-test or Apple run is claimed.

## Hosted native and physical validation

All four genuine GitHub Actions runs independently report completed/success and exact head `7d189beafa8947d5345304a924edcd6e5948eaa1`. Prior P038 CI is not substituted. The Rust Apple workflow was explicitly dispatched because its path filters do not automatically include Swift-only changes.

| Workflow | Exact-head run | Result |
| --- | --- | --- |
| iOS Static Validation | [38038824906](https://github.com/nghianguyen150612/Synveil/actions/runs/38038824906) | SUCCESS |
| iOS Build | [38038824861](https://github.com/nghianguyen150612/Synveil/actions/runs/38038824861) | SUCCESS |
| iOS Simulator Tests | [38038824960](https://github.com/nghianguyen150612/Synveil/actions/runs/38038824960) | SUCCESS |
| iOS Rust Apple Build | [38038915988](https://github.com/nghianguyen150612/Synveil/actions/runs/38038915988) | SUCCESS |

Genuine xcresult summary: **1,275 total / 1,273 passed / 2 skipped / 0 failed**, result Passed, on iPhone 17 Pro with iOS Simulator 26.5. All **55 NodeProjectionTests**, **54 NodeProjectionSQLiteTests** and **42 CommittedProjectionAckTests** passed without a P039 skip. Existing P024–P038 regression suites ran in the same native test target; real-Rust identity rehydration passed. Artifact `ios-simulator-test-results`, ID `11665247740`, was uploaded with seven-day retention. Apple Rust target/header/static-artifact checks passed; no separate Linux Rust unit-test pass is claimed.

Real native file-backed SQLite evidence includes exact v3 migration and preserved immutable request/payload/result/history/base/staging/quarantine; retained prior v1/v2 upgrades; transaction and migration rollback; first/final Node-write faults; actual uncommitted connection interruption; committed applied position plus ACK-pending evidence across reopen; purge protection and parent moves; capacity/busy; concurrent application and feed replay; durable production receipt acquisition, separate-connection ACK lease exclusion, original signed-token POST and confirmation persistence, interrupted explicit same-token recovery, replay bounds and session/quarantine fencing. These are actual SQLite operations, not fake projection-only storage or physical power-cut evidence.

Crash/recovery matrix:

| Boundary | Durable behavior verified |
| --- | --- |
| Before projection COMMIT | Original inbound page remains RECEIVED_UNAPPLIED; no Nodes/progress/proof escape |
| After projection COMMIT | Nodes, event identities, applied position and APPLIED_ACK_PENDING survive together |
| Before ACK lease COMMIT | No returned proof or authorized POST; page remains pending |
| After ACK lease COMMIT | Original token and attempt survive as ACK_IN_FLIGHT; local/explicit recovery only |
| After simulated server acceptance/response loss | Applied Nodes survive, checkpoint remains unconfirmed until verified evidence |
| After confirmation COMMIT | Confirmed position, response, completion and ACK_CONFIRMED survive reopen |

Exactly two cases skipped in this P039 native run:

- KeychainCredentialStoreTests.testSimulatorKeychainRoundTripUsesUniqueTestService: unsigned Simulator process has no Keychain access entitlement.
- MutationQueueSQLiteTests.testNativeDataProtectionAttributes: Simulator filesystem does not expose Data Protection attributes; requested policy is verified separately.

Neither skip is counted as passed. Physical-device validation is **NOT_AVAILABLE**. SQLite rollback/reopen and Simulator results do not establish physical power-loss durability, hardware Keychain or device protection attribute round-trip.

## Files and exclusions

Added: Domain/Sync/NodeProjectionModels.swift; Application/Sync/SyncNodeMaterializer.swift and SyncFeedApplicationService.swift; Infrastructure/Persistence/NodeProjectionSQLiteSchema.swift, SQLiteNodeProjectionRepository.swift and CommittedSQLiteSyncProjectionStorage.swift; Tests/SynveilTests/NodeProjectionTestSupport.swift, NodeProjectionTests.swift, NodeProjectionSQLiteTests.swift and CommittedProjectionAckTests.swift; Support/tests/test_node_projection_foundation.py; this manifest.

Modified: AppDependencyContainer.swift; SyncAckService.swift; SyncFeedService.swift's idempotent applied-page staging readback; MutationQueueSQLiteStore.swift; project.pbxproj; existing inbound/queue/recovery migration test fixtures; P038 source safety test for audited production authority.

No unrelated Rust/server/Android/desktop/AppImage/PostgreSQL/package/workflow source changes. No automatic polling/ACK/replay, background synchronization/drain, rebaseline manifest ingestion/replacement, file-content cache/transfers, conflict resolution, Quick Look/Share Sheet/File Provider/PhotoKit or offline-browser substitution.

Source delivery is independently confirmed merged/closed into ios-app, with all required exact-head iOS gates and native regression suites successful. The documentation-only finalization preserves the verified source tree. Prompt040 may build on durable metadata application and explicitly invoked signed ACK; complete snapshots, background execution, transfers and offline UI remain deferred.

## Unrelated hosted CI snapshot

At source merge/finalization, exact-feature-head non-iOS workflows included:

- Linux AppImage [38038824923](https://github.com/nghianguyen150612/Synveil/actions/runs/38038824923): FAILURE. Inspected error: private or temporary build path found in synveil-desktop, also documented by P038. Duplicate PR run was still running at the snapshot.
- Rust CI [38038824886](https://github.com/nghianguyen150612/Synveil/actions/runs/38038824886) and duplicate PR run 38038848567: queued at the snapshot; no successful conclusion claimed.
- PostgreSQL 17 scheduled-maintenance runs 38038824884 and 38038848499 and Linux DEB/RPM runs 38038824949 and 38038848528: still running at the snapshot; no successful conclusion claimed.

No unrelated source was altered to address these workflows. Their statuses do not substitute for the four successful iOS gates.
