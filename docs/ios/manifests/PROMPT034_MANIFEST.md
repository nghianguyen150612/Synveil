# Prompt034 Manifest — Device-Scoped Metadata Mutation Foundation

## Goal, Baseline, and Checkout Verification

- Phase E: typed, authenticated metadata mutation submission foundation. Production browsing remains read-only until durable queue and authoritative sync-base lifecycle integration.
- Actual starting hosted `origin/ios-app`: **`92986e2f8e5941af70d76d33388169ff4fb389e6`**.
- Working feature branch: **`ios/p034-device-metadata-mutations`**.
- Reused the existing clean cloud checkout. Initial temporary `work` HEAD was `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc`. Confirmed `origin` is the requested repository; fetched `+refs/heads/ios-app:refs/remotes/origin/ios-app`; verified both the exact integration SHA and the required Prompt033 ancestry. Created the feature branch directly from that integration tip. No implementation used `main`, no nested clone or dirty-worktree reset occurred.

## References Inspected

Current `IOS_V0_1_PRODUCT_CONTRACT.md`, `IOS_V0_1_ROADMAP.md`, `IOS_PLATFORM_MAPPING.md`, P031/P032/P033 manifests, and `api/openapi.yaml` mutation, checkpoint, error, identity and decimal schemas. Existing Library/Node models and decoders, AuthenticatedNodeRepository, authenticated request provider, SessionController, credential sink, KeychainCredentialStore, URLSessionHTTPTransport, AppDependencyContainer, and Rust bridge protocol/adapters were inspected. Android parity references: MutationModels.kt, MutationEngine.kt, CacheRepository.kt and SynveilHttpTransport.kt. Server references: `crates/api/src/mutations.rs`, `crates/api/src/error.rs`, the API mutation route tests, `crates/core/src/ids.rs`, `crates/core/src/domain/mutations.rs`, `crates/metadata/src/mutations.rs`, repository submission/concurrency/idempotency decisions, and PostgreSQL mutation tests.

## Domain, Identity, and Synchronization Base

- Closed `ClientMutationKind` and associated-value `ClientMutationIntent` cover exactly CREATE_DIRECTORY, RENAME_NODE, MOVE_NODE, TRASH_NODE and RESTORE_NODE. Required source/parent preconditions cannot be omitted from typed construction. No JSON Patch, uploads, file bytes or arbitrary dictionaries enter preparation.
- `ClientMutationPayload` explicitly encodes each kind's exact keys. Node and Library IDs reuse Rust-backed syntax validation; Device, mutation, conflict and journal event identities are distinct types using the identical canonical lowercase UUIDv7 parser from shared core's domain ID macro.
- `ClientMutationId.generate` uses the new additive `synveil_ffi_client_mutation_id_generate` export, which calls shared core `ClientMutationId::new()`. Existing buffer ownership, panic boundary, ABI version 1, executor and cancellation policy are preserved. The generator is a narrow separate Swift protocol; existing RustBridgeProtocol conformers require no new fake implementation.
- `ClientMutationBase` binds canonical decimal epoch/sequence to exact server endpoint, owner, registered Device and Library. Epoch zero is rejected. Decimal and NodeRevision text remains exact without conversion to floating point or fixed-width integers.
- Base/scope/prepared initializers and the submission interface are module-internal. No production caller obtains a checkpoint by inventing a Node revision, wall-clock value or success-derived sequence. No checkpoint GET, ACK, rebaseline lifecycle or checkpoint progress is implemented; checkpoint GET can initialize server state and is not treated as a harmless read.
- `PreparedClientMutation` is immutable and retains the supplied mutation identity, base, kind, typed payload and stable sorted-key UTF-8 JSON bytes. Rehydration by the future queue must retain these fields and validate its stored representation. Submission never generates or changes an ID.

## Encoding and Idempotency

- Request contains exactly `mutation_id`, `base_epoch`, `base_sequence`, `kind` and the operation's typed `payload`, without null or extra properties. Logical names use authoritative Rust validation (nonempty, at most 1024 UTF-8 bytes); no path interpretation or normalization occurs. CREATE_DIRECTORY sends the actual parent/root Node ID, unlike root-listing GET's omitted parent query.
- Fully encoded request bodies are checked against **16,384 bytes** at preparation, repository and authenticated dispatch boundaries. Oversize fails locally without truncation or network calls. Production responses are streamed through the existing transport with a **64 KiB** limit, **10-second request / 15-second resource timeouts**, and a matching decoder bound.
- Server fingerprint implementation was inspected: it uses versioned, deterministic binary semantic encoding followed by SHA-256, not arbitrary JSON hashing. P034 sends no invented fingerprint, implements no client-authoritative fingerprint and claims no byte/fingerprint parity. Stable client bytes preserve retry semantics; the server owns durable same-ID/same-semantics replay and changed-semantics mutation_id_conflict detection.
- No automatic POST retry, conflict resolution, revision refresh, replacement mutation ID, recursive trash, content purge or fallback restore destination exists.

## Secure API and Production Gate

- Uses **POST `/api/v1/devices/{device_id}/libraries/{library_id}/mutations`**, `submitDeviceLibraryMutation`, with stored `DeviceBearer` credentials. BrowserSession-only direct create/patch/trash/restore routes are never called.
- Reuses the established Keychain-backed authenticated request provider and SessionController. The POST path uses the stored Device identity and requires exact prepared endpoint/owner/Device matching. The Library is immutable in the prepared scope and must be authorized with its originating sync base by the future durable engine. Server ownership, ancestry, cycles and current-state checks remain authoritative.
- URLComponents preserves HTTPS origin, port and configured base path, with no caller-supplied URL, userinfo, filename path components, query or fragment. Headers are Bearer Authorization, JSON Content-Type/Accept, identity Accept-Encoding and existing iOS User-Agent. GET and POST share one header composer. No browser cookies, CSRF or enrollment credentials are added.
- Uses the existing URLSession implementation: ephemeral sessions, no cookie fallback/cache, no redirect following, bounded streaming and TLS verification. No parallel authentication subsystem or duplicate transport implementation is introduced.
- Production AppDependencyContainer constructs a module-internal mutation repository using the existing Rust bridge, store and controller. Its optional `ClientMutationPreparationAuthorizerProtocol` is **nil**. Submission therefore returns **preparationRequired** before credential acquisition or dispatch.
- The future persisted engine must implement that authorizer to verify persisted identity/bytes, authoritative sync-base provenance/scope and recovery ownership before every attempt. P034 supplies only the seam and deterministic test authorizers. There is no permissive production switch, fake queue, UserDefaults persistence or mutation UI dependency. Read-only Library/Node features do not depend on mutation readiness.

## Strict Results and Failure Lifecycle

- Dedicated DTOs validate APPLIED envelope/data/metadata/Node keys, required fields, JSON types, absent-versus-null optional fields, canonical IDs, names, exact revisions, enums, RFC3339/calendar timestamps and bounded request IDs. Mutation ID/kind and Node Library/target identity must match the prepared operation. Directory creation also checks directory kind and exact parent.
- `ClientMutationNodeResult` represents only the documented mutation projection; no purge_eligible or restore_deadline defaults are fabricated. Journal event ID and journal sequence are validated as committed server metadata, never acknowledged checkpoint progress.
- Durable mutation_conflict decodes its exact typed details when present, preserving optional revisions/state/parent/name absence, six closed conflict reasons, conflict/resource IDs, epoch/sequence and replay flag. NAME_OCCUPIED can identify the existing sibling rather than a submitted resource. No automatic resolution occurs.
- mutation_id_conflict and sync_rebaseline_required are separate typed outcomes. A verified mutation_conflict code with permitted absent details remains a conflict without fabricated details. Error classification requires documented status/code combinations, never message text or the retryable hint. Distinct 400/401/403/404/409/413/500/503 categories are preserved without exposing raw messages or storage details.
- Verified authorization/revocation rejection routes only through SessionController's existing captured-revision recovery handler. 404 does not imply revocation; 503 does not change session or delete credentials. There is no re-enrollment or credential deletion here.
- A synchronous dispatch marker separates proven local failures from ambiguous outcomes. Timeout, connection loss, TLS/transport failure after invocation, cancellation after dispatch, malformed/unverifiable responses and post-dispatch stale identity return **outcomeUnknown** with a safe typed cause. Conservatively classifying a transport invocation as dispatched does not claim server receipt or rollback.
- Every terminal response, including recovery, is fenced after HTTP, Rust and Keychain suspension points. Catch paths also check current lifecycle/credential identity. Late results after logout or credential replacement never publish APPLIED/conflict data into the new session. The original immutable operation remains available to its future durable owner for reconciliation/retry.
- Prepared operations, intents, payloads, Nodes, conflicts and HTTP requests have redacted descriptions. No request/response bodies, logical names, bearer, Keychain envelopes or analytics are logged.

## Files

Created:

- `clients/ios/Domain/Mutation/ClientMutationModels.swift`
- `clients/ios/Domain/Mutation/ClientMutationResponseDTO.swift`
- `clients/ios/Application/Mutation/AuthenticatedClientMutationRepository.swift`
- `clients/ios/Tests/SynveilTests/ClientMutationTests.swift`
- `clients/ios/Support/tests/test_client_mutation_registration.py`
- `docs/ios/manifests/PROMPT034_MANIFEST.md`

Modified:

- `clients/ios/Application/Library/AuthenticatedLibraryRequestProvider.swift`
- `clients/ios/App/AppDependencyContainer.swift`
- `clients/ios/Infrastructure/RustBridge/RustBridgeAdapter.swift`
- `clients/ios/Infrastructure/RustBridge/RustBridgeAsyncAdapter.swift`
- `clients/ios/Infrastructure/RustBridge/Generated/synveil_ios_ffi.h`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`
- `crates/ios-ffi/src/lib.rs`
- `crates/ios-ffi/cbindgen.toml`
- `scripts/build-ios-rust-artifacts.sh`
- `scripts/validate_ios_rust_artifact.py`
- `scripts/tests/test_validate_ios_rust_artifact.py`

All new Swift files have unique PBX IDs, one file-group reference and exact app/test target membership. The Apple artifact builder/validator and fixtures now require the additive generator export and P034 mapping metadata, preserving existing status/memory/concurrency/ABI contracts. No server, Android, desktop, database or web implementation changes are included.

## Tests and Local Validation

- **123 new XCTest methods**: 86 domain/encoding/response tests and 37 repository/authentication/transport/race tests. Include every kind, unknown kinds, real Rust canonical parsing/generation/name validation, exact decimals, mandatory payload shape, root/parent identity, UTF-8 boundaries, deterministic immutable retry bytes, exact 16 KiB bounds, zero-dispatch oversized/gated/unauthenticated submissions, strict APPLIED/conflict schemas, all failure categories, ambiguity, no automatic retry/checkpoint progress and deterministic cancellation/logout/credential-replacement races.
- **3 new Python registration/composition regressions**; full iOS Support suite: **41 passed**.
- **2 new Rust FFI tests**; `cargo +1.88.0 test -p synveil-ios-ffi --lib`: **18 passed**, 0 failed. Linux static-library build: **PASS**. Generated header alignment with pinned cbindgen 0.28.0: **PASS**.
- **2 new artifact-validator regressions** verify missing generator export/declaration rejection; artifact-validator suite: **6 passed**. Shell syntax validation: **PASS**.
- Official Swift 6.2.3 Linux tooling installed into ignored `work/`. Recursive strict Swift formatting: **PASS**. Domain/Application Swift 6 strict-concurrency type checking: **PASS**.
- Temporary Linux SwiftPM harness copies production Domain/Application/Rust bridge sources, links the real Rust static library and runs new mutation plus existing Node, Library, HTTP contract and Rust bridge XCTest suites. Only synchronous test entrypoints in temporary copies become async for Linux XCTest discovery. Result: **391 passed**, 0 failed. No Apple frameworks or native Simulator execution are claimed from Linux.
- iOS architecture/source validator: **PASS**. Documentation validation and `git diff --check`: **PASS**.
- Hosted Simulator, exact-head CI, final feature SHA, PR and merge evidence: **PENDING** at initial feature authoring; finalized from genuine hosted evidence after verification.
- Existing native real-Keychain round-trip entitlement limitation is not claimed resolved. Deterministic injected Keychain regressions remain required in hosted CI. Physical-device validation: **NOT_AVAILABLE**.

## Hosted Delivery Evidence

Pending feature publication and exact-head hosted validation. This manifest must not be interpreted as a readiness or merge claim before the verified evidence is finalized.
