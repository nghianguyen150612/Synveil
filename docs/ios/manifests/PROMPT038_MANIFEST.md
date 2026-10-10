# Prompt038 — Sync Change Feed & Signed ACK Foundation

## Integration record

- Repository: `https://github.com/nghianguyen150612/Synveil.git`.
- Target: `ios-app`; feature: `ios/p038-sync-change-feed`.
- Actual starting fetched `origin/ios-app`: `0495e50e850575f1176c99299919b52e32300a10`.
- Existing clean checkout reused. Initial `work` checkout was `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc`; it was not the implementation baseline.
- Verified remote URL, clean status, fetched the explicit integration ref, verified the supplied P037 SHA is an ancestor, and created the feature branch from that fetched ref. No extra integration commits were present.
- Hosted feature SHA, PR, exact-head CI, merge confirmation and resulting integration SHA: pending publication at this manifest's initial commit. These fields must be finalized from genuine hosted evidence; initial implementation alone does not establish readiness for P039.

## Authoritative references

Inspected the product contract, roadmap, platform mapping, P035/P036/P037 manifests and current `api/openapi.yaml`; the existing checkpoint/queue/drain, authenticated Library request provider, session controller, dependency composition and Node models; Android `SyncModels.kt`, `SyncEngine.kt`, `CacheDatabase.kt`, `CacheRepository.kt`; server `crates/api/src/sync.rs`, `error.rs`, API/device-auth tests, and metadata journal/sync implementations.

Current DeviceBearer routes are:

- `GET /api/v1/devices/{device_id}/libraries/{library_id}/changes?limit=200`.
- `POST /api/v1/devices/{device_id}/libraries/{library_id}/changes/ack`.

Older Library-only feed examples in parity documentation are not used. Feed GET observes delivery, not acknowledgment. The journal allocates consecutive sequence values transactionally. Android verifies the first/final event, ascending order and page span; iOS explicitly verifies every consecutive increment with overflow detection.

## Domain and wire validation

Added `SyncJournalEvent`, `SyncChangeKind`, `SyncFeedPage`, `SyncFeedFailure`, `SyncFeedResult`, `SyncAckEvidence`, `SyncAckSubmissionResult`, `InboundSyncPageRecord`, `InboundSyncPageState`, and `SyncJournalPosition`. Existing Library, Node, FileVersion, Device, journal-event, decimal, checkpoint and mutation-scope types are reused.

All eight change kinds are closed: NODE_CREATED, NODE_RENAMED, NODE_MOVED, NODE_TRASHED, NODE_RESTORED, FILE_CONTENT_COMMITTED, FILE_VERSION_RESTORED, NODE_PURGED. Resource kind is exactly NODE and event schema version is exactly 1; unsupported versions and kinds fail closed. Wire schema version remains in each event; original response and encoding version preserve provenance for future migrations.

Strict Codable DTOs are preceded by explicit required/optional/unknown-key validation at envelope, data, meta and every event. OpenAPI optional fields may be omitted but explicit null is rejected. Wrong types, malformed UUIDv7 IDs, invalid enums/timestamps, missing fields, unsafe request IDs and header/envelope request-ID mismatch fail the entire page. All ID validation uses the existing Rust protocol boundary; deterministic tests inject its established validator fixture.

`SyncDecimalValidation` checks canonical unsigned decimal strings through u64 maximum, nonzero epoch, exact sequences and revisions. No floating-point conversions occur. The page epoch/start must match the verified exact-scope acknowledged base. Epoch mismatch classifies rebaseline; start mismatch classifies checkpoint conflict.

Every event must increment the previous sequence exactly by one, beginning at from+1, with checked u64 addition. Event IDs cannot repeat. Final sequence equals through. High watermark cannot precede through; has_more must equal through<high. Empty pages have no token, equal from/through, and no continuation. Empty success returns typed no-new-changes and creates no staging row.

Bounds: preferred page size 200; accepted request limits 1...500; delivered count at most the requested limit and 500; response at most 2 MiB; token 1...256 ASCII bytes in the opaque evidence alphabet; ACK JSON body at most 2,048 bytes. Tokens are preserved byte-for-byte, never reconstructed, logged or treated as credentials. The client does not validate the server HMAC.

## Authenticated transport and explicit operation

The existing authenticated request provider owns both routes. It reuses the Keychain/session identity, lifecycle revision, canonical HTTPS origin, trusted bounded URLSession transport, cancellation and allowlisted failure recovery. Requests carry DeviceBearer, Accept JSON and Accept-Encoding identity; ACK adds Content-Type JSON. No cookies, CSRF headers, token query parameters or redirect fallback are added.

`SyncFeedService.readAndStage` is infrastructure-only and performs one explicit bounded GET, complete validation and durable stage/readback. It never loops on has_more, invents a cursor, updates the server/local acknowledged sequence, triggers a mutation POST or materializes Nodes. P037 Send Pending Changes remains outbound-only. Dependency construction and reopen initiate no feed or ACK request.

Scope includes endpoint, authenticated owner, registered Device and Library, credential identity, current session/lifecycle revision and exact journal position. The queue's existing validated session capture and verified-base read are reused. Sessions are checked before and after asynchronous boundaries. SQLite rechecks binding and quarantine inside the non-suspending transaction. Logout/replacement invalidates the original capability; a replacement credential is never borrowed. Commit and publication are separate: late session changes produce uncertainty/committed-but-session-changed, not a false rollback claim.

## SQLite v3 inbox and migration

The actor-owned P035/P036 connection now uses schema v3. Fresh creation, v1-to-v3 and v2-to-v3 migrations are transactional. Before migration, the exact prior schema is verified. Unknown future versions and damaged schemas fail closed. v2 migration only adds the inbound table and guards; it does not change scopes, sync bases, mutations, dependencies, immutable payload/request bytes, attempt history or submission/outcome semantics.

`inbound_pages` stores scope foreign key, epoch/start/through/high watermark, bounded original response, sorted canonical data representation, encoding version 1, typed state and creation/update transaction timestamps. Events and original ACK token remain in immutable validated wire evidence and are strictly re-decoded on readback. Original event order is retained. No credentials/headers are stored. Credential ID is the existing nonsecret scope fence, not a bearer cache.

One page key is scope+epoch+from_sequence. Identical range/events/token/high watermark are idempotent even when a repeated response has another request ID or JSON key order. Original stored response and timestamps remain unchanged. Conflicting page/token evidence fails closed without overwriting or merging. The service returns the original durable record via scoped readback.

Staging verifies response provenance, exact stored verified base, unquarantined credential binding and capacity, then uses the existing BEGIN IMMEDIATE/COMMIT transaction/fault boundary. INSERT/COMMIT failures roll back. Lost COMMIT acknowledgment is resolved through exact-scope validated readback. Reopen never applies or ACKs a received page. Broken rollback poisons the connection through the existing store behavior. Evidence corruption fails strict readback and schema corruption fails reopen.

Inbox limits are 32 pages per scope and the existing bounded total record policy. Staged original+canonical bytes share the existing 16 MiB storage budget with queue/checkpoint/history evidence; the database retains its 64 MiB page limit. Capacity and simulated disk-full failures preserve existing records; no unacknowledged pages are evicted. WAL, synchronous FULL, busy timeout, private Application Support directory, permissions, Data Protection requests and backup exclusion remain inherited. Portable/Simulator SQLite results do not prove physical power-loss durability.

## Application prerequisite and ACK capability

Typed states are RECEIVED_UNAPPLIED, APPLIED_ACK_PENDING, ACK_IN_FLIGHT, ACK_CONFIRMED and BLOCKED_REBASELINE. P038 can stage only RECEIVED_UNAPPLIED or block existing evidence. An immutable-evidence trigger prevents mutation of page provenance. An application-gate trigger rejects progression from staging to any applied/ACK state. P039 must replace that guard through a reviewed versioned migration coupled to its atomic projection transaction; a state setter is not an authorization mechanism.

`AppliedFeedCommitReceipt` has a fileprivate initializer. Its only factory is the ACK capability, which requires an injected `CommittedSyncProjectionStorageProtocol` to prove atomic projection commit and claim durable ACK_IN_FLIGHT. The receipt binds proof, owning capability and exact session. The transport itself requires this receipt, not an arbitrary token. Projection authority is revalidated before dispatch and before confirmation; another capability's receipt or a revoked proof cannot dispatch.

There is NO production committed-projection conformer and NO `SyncAckService` instance in `AppDependencyContainer`. No DEBUG factory, public receipt initializer, staging override or fake production cache exists. Only dedicated test-target fixtures authorize controlled ACK protocol tests. Production read/stage can never ACK.

ACK serializes exactly {ack_token: original token}, sends one POST and strictly reuses the checkpoint decoder. Returned scope and epoch must match; acknowledged sequence must cover the delivered page and not regress behind previously confirmed progress. Replay may return a legitimately further checkpoint only within the proven locally applied boundary. Progress ahead of the local projection blocks synchronization rather than being accepted as locally synchronized. Durable confirmation belongs to the future projection authority and must not rewrite outstanding immutable mutation bases.

After dispatch, timeout/reset/cancellation, malformed checkpoint, late session loss or local result-persistence failure returns explicit outcomeUnknown. Original token and applied evidence remain durable under the future authority's ACK_IN_FLIGHT contract; no automatic replay occurs. Rebaseline/checkpoint-conflict responses request durable blocking from that authority. Staging GET rebaseline/epoch mismatch marks existing inbox rows BLOCKED_REBASELINE and sync base RECONCILIATION_REQUIRED without resetting epoch/sequence, deleting mutations or downloading a snapshot. HTTP 503 preserves credentials and evidence.

## Files

Added:

- `clients/ios/Domain/Sync/SyncFeedModels.swift`
- `clients/ios/Domain/Sync/SyncFeedResponseDTO.swift`
- `clients/ios/Application/Sync/SyncFeedService.swift`
- `clients/ios/Application/Sync/SyncAckService.swift`
- `clients/ios/Tests/SynveilTests/SyncFeedTestSupport.swift`
- `clients/ios/Tests/SynveilTests/SyncFeedTests.swift`
- `clients/ios/Tests/SynveilTests/InboundSyncSQLiteTests.swift`
- `clients/ios/Tests/SynveilTests/SyncAckTests.swift`
- `clients/ios/Support/tests/test_sync_feed_foundation.py`
- This manifest.

Modified the authenticated Library provider, dependency container, existing SQLite store, Xcode project source membership, and prior SQLite tests/fixture downgrades for the new schema. SQLite linkage is unchanged. The source validator and Python registration suite verify membership and unique PBX identifiers.

## Tests and local evidence

- 125 new XCTest methods: 73 feed/wire tests, 28 file-backed SQLite/feed-operation tests, and 24 ACK capability/protocol tests (73 + 28 + 24 = 125).
- Feed cases include all eight kinds, UUID/schema/enum/type/null/unknown-key errors, decimals/overflow/timestamps, scope/request IDs, duplicates/gaps/order, through/high/has_more, empty/nonempty tokens and 500/requested event limits.
- Real file-backed SQLite cases include v2 migration with immutable UNKNOWN queue bytes and attempt history, verified base and quarantine preservation, migration rollback, stage/reopen, duplicate/conflicting evidence, atomic INSERT/COMMIT rollback, lost commit readback, corruption, capacity, simulated disk full, scope/credential isolation, concurrent serialized staging, logout/replacement fencing, immutable application gating, no POST/base advancement and no network on reopen.
- ACK cases include the actual authenticated provider request, original token/body/header/route, private capability ownership, missing/revoked application authorization, checkpoint strictness/scope/epoch/regression/projection boundary, replay at further confirmed sequence, timeout/redirect/cancellation/session loss, result persistence uncertainty, conflict/rebaseline and 503 credential preservation. Dedicated projection fixtures are test-only and are not evidence that a Node cache exists.
- Linux Swift 6.2 portable SwiftPM harness copies production Domain/Application/SQLite sources and selected XCTest suites into ignored `work/`. System SQLite is real/file-backed. Temporary test copies adapt synchronous MainActor XCTest discovery. The existing real-Rust identity case is skipped because this worker has no Rust toolchain; genuine native CI retains it unchanged. The downloaded Linux Swift Observation library needs `--allow-shlib-undefined` at link; production Swift code is unchanged.
- Portable result before hosted publication: 365 tests executed, 1 skipped, 0 failures (364 passed); includes 125 new P038 cases and existing P035/P036 checkpoint/queue/drain regression cases. This is Linux evidence, not Apple Simulator execution.
- iOS Python Support suite: 58 passed (4 new); architecture/source validator, strict official Swift formatting, Swift 6 portable compilation, documentation validator and git diff --check passed before publication. Exact final checks and hosted evidence are finalized after the feature commit.

## Native and physical validation

Hosted exact-feature-head iOS Static Validation, iOS Build, iOS Simulator Tests and dispatched iOS Rust Apple Build are pending at the initial manifest commit. P037 results are not reused. Native XCTest includes P038 real SQLite and all registered P024–P037 regressions.

Known prior Keychain-entitlement and Simulator filesystem Data Protection skips are not claimed passed. Fresh native totals and skip reasons must be recorded from P038 results. Physical-device validation is NOT_AVAILABLE. Hardware Keychain, native device protection attributes and power-loss durability are unverified here.

No persistent Node projection, canonical metadata application, automatic feed/ACK polling or drain, rebaseline execution/snapshot swap, background task, transfer or sync UI was introduced. P039 must implement canonical Node materialization, purge semantics, exact locally applied position, atomic projection+pending ACK evidence, durable authority and recovery before enabling production ACK.
