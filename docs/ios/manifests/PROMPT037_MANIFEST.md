# Prompt037 — Native Metadata Mutation UX & Durable Queue Status

## Integration evidence

- Repository: `https://github.com/nghianguyen150612/Synveil.git`
- Target integration branch: `ios-app` (never `main`).
- Feature branch: `ios/p037-metadata-mutation-ux`.
- Verified starting SHA: `a7c9a875d083056c95620bc38fb116e241b3a1a4`.
- Bootstrap: fetched `origin/ios-app`, verified the P036 SHA is an ancestor, and created the feature branch from that fetched integration ref. The starting worktree was clean. The initial checkout at `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc` was not used as the implementation baseline.
- Final feature SHA: `2f7957b2d9cbbbbc6134a5bef8825e9384f17392`.
- Feature PR targeting `ios-app`: [#103](https://github.com/nghianguyen150612/Synveil/pull/103). GitHub reports `merged=true`, `state=closed`, merged at `2026-10-10T04:47:36Z`.
- Squash merge commit and resulting `ios-app` SHA for the P037 feature: `9e0ae71eccd637c8fb68a7f412acb749d51aeb97`.
- The P037 service, UI, XCTest source, and manifest were confirmed present on fetched `origin/ios-app` at that SHA.
- Exact-head hosted iOS CI on `2f7957b2d9cbbbbc6134a5bef8825e9384f17392`: iOS Static Validation [run 38024055166](https://github.com/nghianguyen150612/Synveil/actions/runs/38024055166) passed; iOS Build [run 38024055148](https://github.com/nghianguyen150612/Synveil/actions/runs/38024055148) passed; iOS Simulator Tests [run 38024055302](https://github.com/nghianguyen150612/Synveil/actions/runs/38024055302) passed; dispatched iOS Rust Apple Build [run 38024059058](https://github.com/nghianguyen150612/Synveil/actions/runs/38024059058) passed.

## Scope and references

Prompt037 adds the first native metadata editing and durable activity experience. It reuses P034 typed mutations and strict responses, P035 SQLite queue/checkpoint persistence and session isolation, and the P036 drain and UNKNOWN reconciliation boundaries.

References inspected:

- `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`
- `docs/ios/IOS_V0_1_ROADMAP.md`
- `docs/ios/IOS_PLATFORM_MAPPING.md`
- `docs/ios/manifests/PROMPT034_MANIFEST.md`
- `docs/ios/manifests/PROMPT035_MANIFEST.md`
- `docs/ios/manifests/PROMPT036_MANIFEST.md`
- `api/openapi.yaml`
- Android v0.1 metadata mutation UI and preparation flow. Android's automatic synchronization behavior was not carried into iOS.

## Architecture and dependency boundaries

SwiftUI uses `MetadataMutationViewModel` and `MutationActivityViewModel`, backed by the injectable `MetadataMutationFeatureProtocol` and `MetadataMutationService` facade. The facade is composed once by `AppDependencyContainer` from the existing `DurableMutationQueue`, `SyncCheckpointService`, `MutationDrainCoordinator`, authenticated Library request providers, `NodeRepositoryProtocol`, Rust bridge and UUIDv7 generator.

SwiftUI receives no queue store, submission repository, submission lease, raw bearer credential, or arbitrary HTTP composition. Enqueue goes only through the durable queue. Send and UNKNOWN reconciliation go only through P036. No presentation, launch, refresh, foreground, connectivity, or background callback dispatches a mutation.

Queue initialization failure leaves Library and Node browsing composed without the mutation feature. The existing authenticated Node repository gained a scoped, read-only single-Node metadata method for authoritative directory revisions.

## Synchronization-base setup and offline policy

`Enable Changes for This Library` is shown when the exact authenticated Device/Library has no persisted verified base. The user action calls `SyncCheckpointService.prepare(scope:)`, which performs the documented checkpoint GET and persists strictly matched response provenance. It does not create or submit a metadata mutation. Existing verified bases are reused. Invalid or reconciliation-required bases are not silently replaced. Startup does not initialize checkpoints.

Enqueue requires an authenticated current session, ACTIVE Library, healthy SQLite queue, and exact-scope verified persisted base. Local enqueue can work without network access when the base and validated Node metadata are already available in the live session. A cold restart without restored authentication and a validated in-memory Node snapshot can leave editing unavailable offline. No persistent Node cache was added.

## Mutation preparation and workflows

All operations use one facade path. It validates scope and authoritative metadata, checks the verified base, validates logical names through Rust without rewriting Unicode, obtains one Rust-backed canonical UUIDv7, creates the typed immutable intent/request, and enqueues it. A Queued receipt is published only after durable queue readback confirms the record. A duplicate Save is suppressed. When a SQLite commit acknowledgement is lost, the prepared identity is retained for exact-scope readback and retry; it is not regenerated.

- **Create Folder:** uses the actual route parent ID, including the Library root ID. It uses a validated in-memory parent snapshot or the authenticated DeviceBearer Node metadata endpoint to obtain the exact current revision. Missing metadata blocks preparation. After a confirmed APPLIED batch, transient parent revisions are invalidated and the next folder preparation requires a fresh metadata read; explicit browser refresh also rechecks the current parent.
- **Rename:** starts with the exact validated current name and requires explicit Save. The server-backed Node result is not optimistically changed.
- **Move:** a native hierarchical picker uses validated Node browsing results. Only active same-Library directories are selectable. The source and destination parent revisions are captured. Self, same-parent and locally known descendant cycles are blocked. Server ancestry and conflicts remain authoritative.
- **Move to Trash:** a named destructive confirmation explains logical trash and possible rejection of nonempty directories. One confirmed `TRASH_NODE` is queued; no descendants are synthesized and list data remains authoritative until APPLIED.
- **Restore:** no general Trash browser is shown. The activity screen offers a precondition check only for a durably APPLIED trash operation whose verified returned Node remains TRASHED and includes its original parent ID. A fresh authenticated metadata GET must verify that original parent is active and provide its current revision. Restore is hidden unless the check succeeds, then requires a second explicit confirmation. Pending, UNKNOWN, unavailable-parent and other unverified restore cases remain unavailable.

## Activity, status and explicit execution

The native `Pending Changes` view reads exact-scope durable SQLite records in a maximum batch of 100. The store selects unresolved records first, then fills remaining capacity with terminal history; the displayed result is returned in durable enqueue order. The UI explicitly states that older terminal rows may be omitted. Queue row descriptions contain only operation kind, safe target label, durable state and applicable timestamps/evidence summary; credentials and request bodies are not rendered.

Durable states map to distinct labels and SF Symbols: `PENDING` → Queued; `SUBMITTING` → Sending; `APPLIED` → Applied; `CONFLICT` → Conflict — review needed; `OUTCOME_UNKNOWN` → Outcome unknown; `BLOCKED_REBASELINE` → Sync recovery required; `FAILED_PERMANENT` → Could not apply.

`Send Pending Changes` is explicit, authenticated, duplicate-guarded and bounded to 10 operations through P036. The summary uses coordinator counts that P036 publishes only after terminal SQLite persistence. It does not guarantee all pending operations finish in one invocation. No Applied state is inferred from enqueue or dispatch. Result-persistence uncertainty has its own message.

`Check / Retry Original Operation` requires an accessible confirmation and invokes P036 `reconcileUnknown` for the original mutation ID. Original payload, base and immutable request bytes are not regenerated. The durable eight-attempt history limit is shown and enforced. Normal Send does not replay UNKNOWN operations.

Conflict reasons are displayed only from the documented allowlist: `REVISION_MISMATCH`, `NODE_STATE_CHANGED`, `PARENT_CHANGED`, `NAME_OCCUPIED`, `DESTINATION_CHANGED`, `RESOURCE_PURGED`. No automatic resolution controls are offered. Rebaseline-required rows remain preserved and block sending; no epoch or sequence is fabricated. Permanent rejection stays terminal and distinct from uncertainty and conflict.

## Preconditions, session safety and refresh semantics

Precondition provenance is preserved from validated route/library context, authenticated Node listing or single-Node metadata, a strictly persisted checkpoint response, Rust logical-name validation, the existing UUIDv7 generator, and P034 strict mutation result decoding. The server remains final authority for permission, ancestry and conflict checks.

The service invalidates transient parent snapshots and unresolved enqueue identities on session transition. Queue quarantine and durable lease behavior remain owned by P035/P036. ViewModels fence results with lifecycle revisions and operation generations, clear visible queue data on logout or credential replacement, and suppress duplicate actions. A lost COMMIT acknowledgement is presented as uncertainty rather than rollback.

Queued intent does not rename, move, insert or remove a server-backed Node. After a durably confirmed APPLIED result, Pending Changes allows the browser to explicitly refresh its current directory; the server supplies resulting metadata. No revisions or journal acknowledgements are reconstructed locally.

## Accessibility

Native `Form`, `List`, `Menu`/context actions, `confirmationDialog`, sheets, toolbar buttons and `ProgressView` are used. Stable accessibility identifiers and hints cover mutation entry points, confirmations, pending rows, queue states, retry limits, loading, errors and explicit send. State is conveyed with text plus symbols rather than color alone. Name editors preserve exact Unicode, wrap long text, disable duplicate Save while active, and request keyboard focus. Explanations and error copy wrap under Dynamic Type; no accessibility description includes credentials or request bodies.

## Files added and modified

Added:

- `clients/ios/Application/Mutation/MetadataMutationService.swift`
- `clients/ios/Features/Mutation/MetadataMutationViewModel.swift`
- `clients/ios/Features/Mutation/MetadataMutationView.swift`
- `clients/ios/Tests/SynveilTests/MetadataMutationFeatureTests.swift`
- `clients/ios/Support/tests/test_metadata_mutation_feature_safety.py`
- `docs/ios/manifests/PROMPT037_MANIFEST.md`

Modified:

- `clients/ios/App/AppDependencyContainer.swift`
- `clients/ios/App/RootView.swift`
- `clients/ios/App/SynveilApp.swift`
- `clients/ios/Application/Mutation/DurableMutationQueue.swift`
- `clients/ios/Application/Node/AuthenticatedNodeRepository.swift`
- `clients/ios/Domain/Mutation/MutationQueueModels.swift`
- `clients/ios/Domain/Node/NodeModels.swift`
- `clients/ios/Features/Library/LibraryCatalogView.swift`
- `clients/ios/Features/Node/NodeBrowserView.swift`
- `clients/ios/Features/Node/NodeBrowserViewModel.swift`
- `clients/ios/Infrastructure/Persistence/MutationQueueSQLiteStore.swift`
- `clients/ios/Support/tests/test_mutation_drain_safety.py`
- `clients/ios/Support/tests/test_node_browser_registration.py`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`
- `clients/ios/Tests/SynveilTests/MutationQueueTestSupport.swift`
- `clients/ios/Tests/SynveilTests/NodeBrowserViewTests.swift`

## Tests and validation evidence

- New deterministic XCTest methods: **30 added** (29 feature tests plus one Library-status navigation test; native execution pending). They exercise SQLite-backed operation preparation, root revision provenance and freshness after APPLIED, offline enqueue, missing base setup, explicit checkpoint failure, exact rename Unicode, duplicate Save, lost commit acknowledgement readback, Move guards and revisions, Trash confirmation, APPLIED trash/Restore preconditions, bounded terminal-heavy queue activity, scope isolation, state mapping, explicit send, UNKNOWN cap, session invalidation and SwiftUI composition with a real SQLite-backed enqueue and explicit APPLIED drain.
- Linux Python support suite: **54 tests passed** on 2026-10-10, including new P037 facade/invocation/activity security invariants.
- `python3 clients/ios/Support/validate_ios_sources.py`: passed on 2026-10-10.
- `python3 -m unittest discover -s clients/ios/Support/tests`: passed, 54 tests, on 2026-10-10.
- `bash scripts/validate-docs.sh`: passed on 2026-10-10.
- `git diff --check`: passed on 2026-10-10.
- `swift-format lint --recursive --strict clients/ios`: passed with Swift 6.1.3 in the Linux Docker toolchain on 2026-10-10.
- `find clients/ios -name "*.swift" -print0 | xargs -0 swiftc -frontend -parse`: passed with Swift 6.1.3 in the Linux Docker toolchain on 2026-10-10. This is syntax parsing, not SwiftUI typechecking.
- Host `swiftc`/`xcodebuild` and Apple SDKs are unavailable. SwiftUI typechecking and XCTest have not been claimed locally.
- Native Simulator result on the exact feature SHA: **987 passed, 2 skipped, 0 failed** (989 total) on the iPhone 17 Pro / iOS 26.5 hosted Simulator. The result bundle says `Passed`; see [run 38024055302](https://github.com/nghianguyen150612/Synveil/actions/runs/38024055302).
- The two skips were `KeychainCredentialStoreTests.testSimulatorKeychainRoundTripUsesUniqueTestService` (the unsigned Simulator test process has no Keychain access entitlement) and `MutationQueueSQLiteTests.testNativeDataProtectionAttributes` (the Simulator filesystem does not expose Data Protection attributes; policy was verified separately and physical-device round-trip remains required).
- Real SQLite evidence: the hosted XCTest run includes the P037 SwiftUI composition test using the existing SQLite-backed queue fixture, verifies durable enqueue state before Send, and then exercises the explicit APPLIED drain path. The exact-head native test suite passed. This is not physical power-loss durability evidence.
- Physical-device validation: `NOT_AVAILABLE` in this environment; no device durability or hardware Keychain claims.
- Exact-head hosted results: iOS Static Validation passed (run `38024055166`); iOS Build passed (run `38024055148`); iOS Simulator Tests passed, 987/2/0 (run `38024055302`); dispatched iOS Rust Apple Build passed (run `38024059058`).
- Exact-head unrelated workflow status observed after the feature PR merge: Linux AppImage failed (run `38024055146`); Rust CI had failures in its Windows and Linux test jobs, Windows desktop UI and web quality jobs (run `38024055263`). PostgreSQL 17 and Linux native package jobs were still running when this manifest was finalized; none is an iOS P037 gate. The successful P037 iOS evidence above is recorded independently. No P037 fix was made for those unrelated areas.

## Finalization fields

- Final feature SHA: `2f7957b2d9cbbbbc6134a5bef8825e9384f17392`.
- Feature PR: [#103](https://github.com/nghianguyen150612/Synveil/pull/103), base `ios-app`; marked ready for review before squash merge.
- Exact-head workflow run IDs: iOS Static Validation `38024055166` success; iOS Build `38024055148` success; iOS Simulator Tests `38024055302` success (987 passed, 2 skipped, 0 failed); iOS Rust Apple Build `38024059058` success.
- Hosted merge state verified from GitHub REST: `merged=true`, `state=closed`, `merged_at=2026-10-10T04:47:36Z`.
- Resulting hosted `ios-app` SHA for P037 integration: `9e0ae71eccd637c8fb68a7f412acb749d51aeb97`.
- Feature sources, tests and this manifest were verified on integrated `ios-app` at the resulting SHA. This manifest’s post-merge evidence finalization is submitted separately so the feature merge record remains immutable.
- Unrelated hosted workflow status is listed in Tests and validation evidence; broad Rust, AppImage, desktop, web, PostgreSQL and packaging jobs are outside the P037 iOS validation scope.
- Remaining limitations: no general Trash listing; Restore is APPLIED-operation and precondition gated; no offline persistent Node cache; no background mutation execution; Linux cannot execute Swift/XCTest/Simulator; physical-device behavior remains unverified.
