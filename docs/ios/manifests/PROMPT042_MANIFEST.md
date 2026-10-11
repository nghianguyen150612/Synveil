# Prompt042 — Rebaseline and full metadata snapshot foundation

## Baseline and scope

Repository: `nghianguyen150612/Synveil`. The initial clean Cloud checkout was branch `work` at `3851ea11927e24255614cfc38adbaccbd345ca03`. The existing checkout was fetched, and `origin/ios-app` resolved to the required P041 integration SHA `760ef1cfd7d5fc4297822b672d7c459b074f55b8`. The required ancestor check passed before creating `ios/p042-rebaseline-snapshot` from that integration tip. No implementation was made on `main`, no nested clone or destructive reset was used, and no `codex/` branch was created.

P042 adds an explicit, foreground-only metadata rebuild. An immutable server manifest is staged on the existing actor-owned SQLite connection, fully verified, sealed into a prepared generation, and activated only after verified server completion. Staging and preparation preserve the previous active cache and confirmed checkpoint. Snapshot Nodes remain a separate limited metadata type; live timestamps, purge/restoration fields and file bytes are never invented.

Authoritative references inspected include the iOS product contract, roadmap and platform mapping; P035/P036/P038/P039/P040/P041 manifests; `api/openapi.yaml`; `crates/api/src/rebaseline.rs`, `crates/metadata/src/rebaseline.rs`, `crates/api/src/sync.rs`, shared core graph validation, relevant migrations and server tests; and the existing authenticated request boundary, durable queue, signed feed/ACK, projection, offline browser, session and app composition. Android bootstrap/cache reconciliation was inspected as a behavioral reference, without importing its proof semantics.

## Authenticated protocol and typed evidence

All requests reuse the P029 DeviceBearer boundary, HTTPS origin binding, credential identity, lifecycle revision, redirect rejection, private URLSession transport and no-cookie policy. There is no CSRF/session-cookie authentication. Construction and status inspection perform no POST or manifest GET.

| Operation | Exact route and request |
|---|---|
| START | `POST /api/v1/devices/{device}/libraries/{library}/rebaseline`, exact body `{}` |
| Manifest | `GET /api/v1/devices/{device}/libraries/{library}/rebaseline/{bootstrap}/nodes?limit=200`, with the original `cursor` on continuation |
| COMPLETE | `POST /api/v1/devices/{device}/libraries/{library}/rebaseline/{bootstrap}/complete`, JSON containing only the original `completion_token` |

The decoder validates required and optional keys, rejects unknown fields and explicit null, checks JSON types and body/header request-ID agreement, and validates IDs and logical names through the existing Rust bridge. Generation, epoch, resume cut, manifest count, revision and content length use canonical bounded unsigned decimal rules. Epoch, generation and Node revision are nonzero. The bootstrap's ID, scope, generation, epoch/cut, count and created/expiry timestamps must match on every page. OPEN, COMPLETED, ABORTED and EXPIRED are decoded; only an OPEN manifest is downloadable and only a verified COMPLETED envelope can confirm a handoff. Server `bootstrap_expired` and `bootstrap_conflict` errors use their actual source-defined names.

START persists uncertainty before dispatch. If its response is lost, local status becomes START_UNKNOWN and an explicit repeat START can retrieve the server's existing OPEN bootstrap. A new generation is accepted only after an expired, blocked or active-complete predecessor and must have a greater generation and different bootstrap ID. Historical generations are retained and never mixed. A reused OPEN bootstrap cannot rewind an already confirmed checkpoint within the same epoch.

Manifest rows contain canonical Node ID, optional parent ID, exact name, kind, ACTIVE/TRASHED state, revision and optional version/content length/hash. DIRECTORY content is forbidden; FILE version and content metadata must appear together; SHA-256 is canonical lowercase hexadecimal. PURGING and self-parenting are rejected. The type contains no live Node timestamps or content availability flag.

## Pagination, bounds and complete graph proof

Cursors and terminal tokens are opaque server evidence. They are preserved and URL-encoded without reconstructing their meaning. Nonterminal pages require nonempty rows and a next cursor, and prohibit terminal evidence. Terminal pages prohibit a cursor and require the original completion token; an empty terminal page can be accepted only when the preceding committed count already matches. Page ordering is strictly increasing by canonical Node ID, including across page boundaries. Repeated cursors, duplicate rows, conflicting page replay, count excess and terminal count mismatch roll back the page.

| Local policy | Bound |
|---|---|
| Default / hard page size | 200 / 1,000 Nodes |
| Foreground invocation budget | Up to 16 pages; callers may select a smaller positive budget |
| Total manifest pages / rows | 1,024 / 16,384 |
| Manifest response / individual stored Node | 2 MiB / 16 KiB |
| START or completion response | 16 KiB |
| Cursor / original terminal token | 320 / 336 UTF-8 bytes |
| COMPLETE request body | 2,048 bytes |
| Snapshot evidence storage | 16 MiB, also charged to the configurable existing shared persistence budget |
| Database / completion attempts | Existing 64 MiB file ceiling / 8 explicitly initiated attempts per bootstrap |

Invocation budget exhaustion leaves DOWNLOADING and offers explicit continuation. It does not truncate or declare completion. Resource exhaustion returns a typed capacity failure and preserves active metadata. Limits include raw page envelopes, canonical page bytes, row metadata, session and attempt evidence; staging shares the queue's capacity accounting.

Terminal receipt is recorded as TERMINAL_RECEIVED, which does not claim graph verification. Preparation rereads every original page from SQLite, redecodes it, checks canonical bytes, exact cursor chain, terminal token, global order and exact row/blob/column correspondence, and compares total unique rows to the declared count. It then validates the complete bounded topology: exactly one ACTIVE DIRECTORY root with the trusted catalog root ID, every parent present and a DIRECTORY, all rows connected to the root, no cycle, and valid logical ancestry including trashed subtrees. Names remain metadata rather than identity; duplicate names at different IDs and canonical Unicode are supported. This proof is repeated before dispatching completion and before activation.

## SQLite v5, preparation and distributed handoff

The existing `MutationQueueSQLiteStore` connection and actor own all transactions. Version 5 adds start attempts, immutable sessions, immutable manifest pages and Nodes, completion attempts and a per-scope active generation pointer. Exact v4 schema verification precedes transactional v4-to-v5 migration. Original v1/v2/v3 upgrade paths first produce the original verified v4 schema, then v5. Future version 6 or corrupted/modified schemas fail closed. WAL, FULL synchronous configuration, foreign keys, file protection policy, existing bounded busy timeout and queue storage are reused.

Existing mutation IDs, request bytes, epoch/sequence bases, UNKNOWN/conflict outcomes, dispatch/attempt history, inbox pages, signed ACK tokens, projection commit/event evidence and quarantine are retained. The migration replaces only the necessary projection authority triggers and creates new tables. It does not reset, rebase, drain or delete the queue.

Page commits atomically persist immutable rows, raw/canonical page evidence, committed counts, next cursor and terminal evidence. Exact canonical page replay is idempotent even when an envelope request ID differs. Counters come from committed SQLite rows. Reopen reads local state and resumes the exact cursor without networking.

Preparation seals the already immutable, isolated Node generation with a durable prepared ID after full readback and graph proof. The prepared generation is a deterministic replacement projection; no mutable full-array copy or unverified active-cache switch is needed. Before any COMPLETE POST, the store verifies the prepared generation, binds an attempt to its prepared ID, original token, scope, credential and OS owner, and commits SERVER_COMPLETION_IN_FLIGHT.

The ordering is:

1. Stage all pages and verify exact terminal count.
2. Verify persisted manifest and graph; durably seal PREPARED_FOR_HANDOFF.
3. Commit a bound completion attempt before dispatch.
4. Send the original terminal token once for the explicitly requested attempt.
5. Validate COMPLETED bootstrap, exact generation and checkpoint epoch/cut, timestamp and boolean replay field.
6. Durably store SERVER_COMPLETION_CONFIRMED separately.
7. In one local transaction, replace active membership, set the snapshot pointer, save Library metadata and exact completion/checkpoint provenance, update anchor/applied/confirmed positions and publish ACTIVE_COMPLETE/COMPLETE.

There is no network await inside a write transaction. Activation removes all previous active cached Node rows for the scope, so omitted old rows cannot leak into the replacement. It retains historical inbox, mutation, event and attempt records. The active immutable snapshot serves limited rows; later canonical/event rows overlay them by Node ID and lifecycle. Revision comparisons and retained purge event evidence prevent stale resurrection. Historical pre-cut/pre-epoch inbox evidence remains stored but is excluded from post-snapshot work selection.

A failure before activation COMMIT leaves old membership and local confirmed checkpoint intact. If server confirmation was durably stored, explicit recovery performs only local activation; it does not issue another POST. If dispatch, response, validation, cancellation or confirmation persistence is uncertain, the prepared generation and original token remain recoverable as OUTCOME_UNKNOWN (an interrupted in-flight attempt is presented equivalently). Explicit recovery sends that same token once, accepts only a fully validated replay, and never creates a new bootstrap. Authoritative retention expiry or bootstrap conflict during completion puts the durable handoff in RECONCILIATION_REQUIRED and blocks fresh START; no remote rollback or physical power-loss guarantee is claimed.

## Coexistence, sessions and truthful offline UX

A run holds the same inbound and ACK advisory locks used by P040 across requests, with SQLite authority checks at each write. Durable start/session guards continue blocking ordinary inbound work, checkpoint replacement, signed ACK authorization and outbound enqueue/dispatch/recovery after the live process releases its locks. Outstanding nonterminal mutations (including UNKNOWN, conflict or dispatching work) and unresolved inbox/ACK work conservatively block START. Their original evidence is preserved. Completed history is compatible. No automatic reconciliation, mutation rebase, conflict resolution, outbound drain or retry loop is added.

A COMPLETE snapshot supports post-cut P038/P040 incremental application and signed ACK. The ACK guard now permits COMPLETE only when the active snapshot pointer proves completion provenance. Applied and server-confirmed positions remain distinct. P035 reads the stored, strictly verified completion envelope as checkpoint provenance rather than manufacturing a checkpoint response.

Sync Status includes native Rebuild Saved Metadata, explicit START confirmation, bounded Continue Snapshot Download, Verify and Prepare, explicit checkpoint handoff/recovery confirmation, committed page/Node counters and cancellation. View appearance reads status only. After each explicit operation, the surrounding Sync Status refreshes its persisted checkpoint and cache state locally; this does not initiate feed or ACK work. Scene inactivity and disappearance cancel foreground work. Session invalidation cancels coordinator work, and credential/scope/lifecycle checks fence late results before commits and publication. Quarantined or wrong-Library/credential data cannot be recovered or exposed.

During staging, P041 saved browsing keeps the old cache. After activation, snapshot directories navigate by canonical IDs and limited snapshot files show name, kind, revision, version, safe length/hash and explicit unavailable live/restoration metadata. Full canonical live Nodes are never constructed from snapshot rows. Snapshot and later journal rows share folders-first, numeric-aware case-insensitive name ordering and ID tie-breaking. A bounded/truncated listing remains partial even under a complete global topology. COMPLETE means saved folder structure at the snapshot cut, with separately applied newer events; newer server changes may still exist. It does not promise current live fields or file contents. Open, preview, QuickLook, upload, download, export and content availability are not introduced.

The SwiftUI flow uses wrapping text, Dynamic Type, native List/NavigationStack controls, VoiceOver labels/identifiers and frequently-updating counter traits. Tokens, bearer credentials, SQL and private page bytes are not shown or logged. The app composes the new coordinator from its existing provider, queue, database, session and Rust bridge without network execution at startup.

## Inventory

Added:

- `clients/ios/Application/Sync/RebaselineCoordinator.swift`
- `clients/ios/Domain/Sync/RebaselineModels.swift`
- `clients/ios/Domain/Sync/RebaselineResponseDTO.swift`
- `clients/ios/Features/Node/SnapshotNodeInformationView.swift`
- `clients/ios/Features/Sync/RebaselineProgressView.swift`
- `clients/ios/Features/Sync/RebaselineViewModel.swift`
- `clients/ios/Infrastructure/Persistence/RebaselineSQLiteSchema.swift`
- `clients/ios/Support/tests/test_rebaseline_snapshot.py`
- `clients/ios/Tests/SynveilTests/RebaselineManifestTests.swift`
- `clients/ios/Tests/SynveilTests/RebaselineRecoveryTests.swift`
- `clients/ios/Tests/SynveilTests/RebaselineSQLiteTests.swift`
- `clients/ios/Tests/SynveilTests/RebaselineTestSupport.swift`
- `clients/ios/Tests/SynveilTests/RebaselineTransportTests.swift`
- `clients/ios/Tests/SynveilTests/RebaselineViewModelTests.swift`
- `docs/ios/manifests/PROMPT042_MANIFEST.md`

Modified:

- `clients/ios/App/AppDependencyContainer.swift`
- `clients/ios/App/RootView.swift`
- `clients/ios/App/SynveilApp.swift`
- `clients/ios/Application/Library/AuthenticatedLibraryRequestProvider.swift`
- `clients/ios/Application/Mutation/DurableMutationQueue.swift`
- `clients/ios/Application/Node/OfflineNodeBrowserService.swift`
- `clients/ios/Domain/Sync/NodeProjectionModels.swift`
- `clients/ios/Features/Library/LibraryCatalogView.swift`
- `clients/ios/Features/Node/NodeBrowserView.swift`
- `clients/ios/Features/Node/NodeBrowserViewModel.swift`
- `clients/ios/Features/Sync/SyncStatusView.swift`
- `clients/ios/Features/Sync/SyncStatusViewModel.swift`
- `clients/ios/Infrastructure/Persistence/MutationQueueSQLiteStore.swift`
- `clients/ios/Support/tests/test_inbound_sync_coordinator.py`
- `clients/ios/Support/tests/test_sync_feed_foundation.py`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`
- `clients/ios/Tests/SynveilTests/InboundSyncSQLiteTests.swift`
- `clients/ios/Tests/SynveilTests/MutationQueueSQLiteTests.swift`
- `clients/ios/Tests/SynveilTests/MutationRecoverySQLiteTests.swift`
- `clients/ios/Tests/SynveilTests/NodeProjectionSQLiteTests.swift`
- `clients/ios/Tests/SynveilTests/NodeProjectionTestSupport.swift`
- `clients/ios/Tests/SynveilTests/SyncStatusViewModelTests.swift`

All seven new production and six new test Swift files have unique PBX IDs and correct application/test target membership. SQLite linkage, deployment target and Swift 6 concurrency settings are retained. Architecture tests verify registration and security boundaries.

## Test coverage and validation

61 new native XCTest methods: 11 transport, 10 manifest, 12 SQLite, 17 recovery and 11 ViewModel/browser/UI. Two of the eleven UI methods require SwiftUI/UIKit. The fixtures match the actual server schema and use real file-backed SQLite; these are deterministic scripted responses rather than a live production server.

Parameterized methods cover the requested malformed-key/null/type/state/scope/decimal cases and page proof combinations. Coverage includes exact authenticated routes/bodies; original cursors/tokens and request bounds; immutable identity, count, ordering and content metadata; trusted root, missing/file parents, cycles and trashed ancestry; v4 migration, original UNKNOWN/ACK/attempt evidence, rollback/future schema; active-cache preservation and omitted-row removal; multi-page reopen and preparation; immutable duplicate replay; capacity and corrupt persisted evidence; pre-POST durable attempts, response loss, confirmation/activation failures and same-token recovery; expiry/generation isolation; explicit attempt budget and cancellation; independent-connection inbound/outbound exclusion; logout/credential/Library fences; truthful limited metadata, post-cut incremental signed ACK; local-only appearance, explicit confirmation, committed counters, expired-retention blocking, mixed-row sorting, native accessibility and Unicode.

Existing v1/v2/v3 migrations, no-redirect/authentication contracts, signed ACK races, mutation lease/recovery and P024–P041 suites remain registered. Existing native tests were updated only for v5 schema/table counts and the implemented recovery message; no native regression tests were removed or bypassed. Six Python architecture/registration tests were added.

Local validation:

- Static iOS source validator: PASS.
- Python source/architecture self-tests: 76 PASS.
- Official Swift 6.2 strict recursive formatter lint: PASS.
- Swift 6.2 syntax parse: all 149 Swift files PASS.
- Swift 6.2 portable production concurrency compilation: PASS.
- Portable real SQLite/regression run: 1,049 executed, 1,034 passed, 15 skipped for native Rust/SwiftUI dependencies, 0 failed. All 59 portable P042 methods passed.
- Documentation validator and `git diff --check`: PASS, including after manifest creation.

The temporary Linux harness links official SQLite 3.50.4 and production sources; only scratch test copies adapt synchronous MainActor discovery and skip unavailable native Rust/SwiftUI cases. Native repository tests are unchanged by these portability adaptations. No local Xcode/Apple SDK, Simulator or Rust compiler is available. Native Rust/SwiftUI and full Apple integration are verified by the required hosted workflows below.

## Hosted delivery record

Feature SHA, PR URL, exact-head iOS Static Validation, iOS Build, iOS Simulator Tests, explicitly dispatched iOS Rust Apple Build, native result counts/artifact, merge proof and fetched integration tip are pending hosted execution. These are completed in a documentation-only delivery-record update after the source PR's actual hosted merge, avoiding fabricated future evidence or a self-referential Git commit ID.

Native Keychain/Data Protection skip classifications and unrelated CI statuses will be recorded from the actual P042 run. P041 results are not reused as P042 validation. Physical-device validation: **NOT_AVAILABLE**. Native SQLite commit/reopen tests do not establish physical power-loss durability.

Deferred: file-content cache/transfer/preview/export/upload, background rebaseline/sync/ACK/drain, automatic uncertain-completion retry, automatic mutation rebase/conflict reconciliation, cold-start offline authentication restoration, and physical-device validation. Libraries exceeding the documented local ceilings remain capacity-blocked with previous metadata preserved. An uncertain handoff whose replay evidence has expired requires authoritative reconciliation outside this foundation.

P042 readiness remains pending until the genuine source PR is merged into `ios-app`, required exact-head iOS workflows pass, and the final manifest update and integration contents are verified.
