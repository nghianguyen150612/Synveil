# P052 — App Lifecycle, Durable Transfer Recovery & Session-Safe Restoration

## Checkpoint

- Starting integration branch: `ios-app`.
- Verified starting integration SHA: `02bc2b8f8ac8d8e2968da9f22abb5387ee3d5df3`.
  `origin/ios-app` was fetched and matched this SHA; it is an ancestor of the
  P052 feature branch.
- Feature branch: `ios/p052-app-lifecycle-transfer-recovery`.
- P052 implementation commit: `b16b678d0a5c1db7e7703d01feaf04b1a1d810d0`
  (`ios: add lifecycle-safe transfer recovery (P052)`).
- Formatting correction: `ae205a27e052b667f7f7b0eb0b94ace56f4b82d2`
  (`fix(ios): format lifecycle recovery changes`).
- Accepted-PATCH lifecycle cancellation fix:
  `9283cb16ad00b9a11989f2ce1169d2d551681804`
  (`fix(ios): persist accepted patch before lifecycle stop`).
- Recovery status label correction:
  `5e1bb8a6abe237b5a83e5adaa89f838dc06473cb`
  (`fix(ios): keep upload recovery label within native limit`).
- Bounded lifecycle test-gate correction:
  `842384148b462955d54984c4b77c9ac07fafe9e2`
  (`test(ios): bound lifecycle recovery gates`).
- Validation-record commit: `a1bb252d52aedd24cf69179668449e3e91d22767`
  (`docs(ios): record P052 validation evidence`).
- The final follow-up commit updates this closeout only; no iOS source or tests
  changed after the validated source checkpoint `842384148b462955d54984c4b77c9ac07fafe9e2`.
- PR #135: https://github.com/nghianguyen150612/Synveil/pull/135, open against
  `ios-app`; reviewer `ducminh231011` requested, no review submitted yet.
- No merge or final integration SHA is claimed.
- The validated source checkpoint passed all four required iOS workflows and
  the portable checks recorded below. The final documentation-only head is
  revalidated separately; its exact-head run links are tracked in PR #135 and
  the final report.

## Architecture audit

P043–P051 manifests and the P051 safe-containment closeout were reviewed before
implementation. The existing architecture already provides the durable transfer
and session authorities P052 needs:

- `SynveilApp` owns one `UIApplicationDelegateAdaptor`. The delegate registers
  `BGAppRefreshScheduler` before requesting application bootstrap. SwiftUI and
  OS callbacks await the same `BackgroundUploadApplicationBootstrap` task.
- `AppDependencyContainer.prepareApplicationSession()` is single-flight. It
  initializes Rust validation and SQLite before session restoration, recovers
  interrupted local mutation state, then restores the Keychain session and its
  authorization evidence. P048 scheduling follows that preparation. No saved
  transfer causes a content request during startup.
- P043 downloads already pin immutable `FileVersion` identity, checkpoint only
  staged and synchronized bytes, retain partial ownership, and publish only
  verified content. Resume and restart are explicit actions.
- P045 uploads already retain operation/session identity, source staging,
  authoritative server offsets, and unknown PATCH/COMPLETE outcomes. Resume and
  status checks are explicit operations.
- SQLite v9 stores transfer and recovery evidence. Session/account/device/library/
  credential binding and existing quarantine/invalidation rules remain the
  authority. P047 authorization evidence, P048 sync/ACK records, P049 notification
  ledger, and P050 conflict evidence remain in their existing stores.
- P046 production background upload and P051 background download remain
  capability-disabled. Existing legacy upload task evidence and P051A transport
  capability checks remain in place. P048 remains opt-in metadata refresh;
  P049 navigation still resolves through durable eligibility; P050 conflict
  review remains read-only and BrowserSession-restricted.
- The recovery views already read saved transfer state, but a restored
  `DOWNLOADING` download could be labeled as live activity. Upload scene
  cancellation was view-local. P052 adds one app lifecycle owner, shared
  callback-generation fencing, and accurate interrupted-state presentation.

## Lifecycle architecture and startup ordering

`AppLifecycleEventDelivery` retains the most recent bounded phase and forwards
both UIApplicationDelegate and SwiftUI `scenePhase` callbacks to the same
`AppLifecycleRecoveryCoordinator`. Duplicate phases coalesce. Every distinct
phase advances one in-memory generation. Transfer coordinators receive the
generation; inactive/background phases cancel only their foreground task, and
activation advances the callback fence without resuming work. Late transfer and
saved-record callbacks from an older lifecycle generation cannot update the new
view-model state. A local saved-state read interrupted by a phase transition is
retried once under the current generation.

No `applicationWillTerminate` callback is required. UIKit termination, force
quit, memory pressure, and system relaunch are not treated as guaranteed callback
events. No lifecycle database or competing application/session state machine was
added.

Startup remains ordered as follows:

1. Load non-secret `AppConfiguration` and construct one dependency container.
2. Register native BGAppRefresh before starting the shared application bootstrap.
3. Open and validate the existing SQLite v9 store and recover interrupted local
   operations while composing Rust, storage, authorization, transfer, sync, and
   notification dependencies.
4. Synchronize the current lifecycle generation with the newly composed
   foreground coordinators.
5. Restore the Keychain session and validate its current authorization evidence.
6. Restore only P046 legacy task evidence under its closed production gate; retain
   safe identifiers/orphans, withdraw local authorization, and cancel tasks as
   already implemented. No PATCH or automatic status GET is authorized.
7. Process pending native callback evidence, then preserve the existing P048
   BGAppRefresh scheduling order.
8. Let SwiftUI await the same startup task, read durable transfer presentation,
   and publish safe read-only recovery notices. No transfer starts automatically.

## Recovery and safety behavior

### Foreground downloads

Lifecycle suspension cancels the current P043 foreground task. The coordinator
persists only its last committed checkpoint; it does not invent bytes or mark
incomplete content verified. Reopened `DOWNLOADING`, `PARTIAL`, `CANCELLED`, and
recoverable records are labeled as interrupted, while `VERIFYING` is labeled
verification-required. A saved partial file is explicitly described as
unavailable offline. Only the existing explicit Resume/Restart controls can
request content, and resume revalidates the record's pinned immutable version.
An already running task remains single-flight through cancellation; activation
does not create another request.

### Foreground uploads

Lifecycle suspension cancels only the P045 foreground task and advances the
presentation fence. Cancellation during PATCH preserves the original operation,
session, source, and SQLite-confirmed server offset; a possible accepted chunk
remains `OUTCOME_UNKNOWN`. If the PATCH response returns an authoritative offset
after cancellation, P052 persists that offset and stops before the next PATCH or
COMPLETE. Lifecycle cancellation does not trigger a replacement PATCH or
automatic status GET. Cancellation during COMPLETE or an explicit abort
also retains the uncertain outcome without an automatic follow-up status GET.
Only the existing explicit Resume, Check Server Status, and Abort controls can
start recovery. A stale session or lifecycle result cannot invoke a completion
presentation callback.

### Scope, storage, and compatibility

View models retain their existing authenticated-session revision checks.
Download saved reads are keyed to the current session and lifecycle generation;
upload saved reads retain their status generation and now also fence lifecycle
changes. Transfer rows continue to be read using current endpoint, account,
Device, Library, operation, and credential scope. A failed storage lookup remains
unavailable/error presentation, never an empty history. Logout/session
invalidation remains responsible for clearing protected UI and invoking existing
coordinator invalidation/quarantine.

P046, P048, P049, P050, and P051 implementation files outside the app delegate
and foreground presentation/coordinators were not changed. No background
download session, transfer grant API, or background transfer capability was
added. SQLite remains v9; **no migration** was needed.

### P051 deferred-scope contract

- P043 foreground downloads, P044 verified preview/export, and P045 foreground
  uploads remain available through their existing explicit actions.
- P046 OS-managed background upload remains disabled. Legacy task/job identity,
  source/chunk intent, saved authoritative offset, and uncertain outcomes remain
  under the P051A closed capability and transport checks; startup does not
  resume, PATCH, or status-read them.
- P047 offline metadata inspection remains authorization-gated. P048 remains
  explicit opt-in, bounded, metadata-only BGAppRefresh. P049 notification
  eligibility and routing are unchanged. P050 conflict evidence remains
  durable/read-only and no mutation is replayed.
- P051 OS-managed background download remains unimplemented. No background
  download session, transfer grant API, deployment security change, or security
  approval is introduced by P052.
- P051 live redirect/deployment proof, device validation, and independent
  approval to reactivate background file transfers remain deferred. P052 does
  not require those deferred capabilities to pass its foreground lifecycle
  recovery gates.

## Deterministic XCTest coverage

P052 adds these focused methods:

- `SynveilBootstrapTests.testDelegateAndSceneLifecycleEventsShareOneCallbackFence`
- `FileDownloadViewTests.testUnavailableDownloadStorageIsNotPresentedAsEmptyHistory`
- `FileDownloadViewTests.testRestoredDownloadingRecordShowsInterruptedAndDoesNotReplay`
- `FileDownloadViewTests.testLifecycleSuspensionCancelsForegroundDownloadAndReloadsDurableState`
- `FileUploadCoordinatorTests.testLifecycleInterruptionPreservesUnknownPatchOutcomeWithoutReplay`
- `FileUploadCoordinatorTests.testLifecycleInterruptionAfterAcceptedPatchPreservesOffsetAndStopsBeforeNextChunk`
- `FileUploadCoordinatorTests.testLifecycleInterruptionDuringCompleteDoesNotReconcileAutomatically`

`BackgroundUploadTests.testApplicationBootstrapRunsWithoutSceneAndOnlyOnce` was
strengthened to hold startup behind a deterministic gate while concurrent
waiters arrive. Existing P001–P051 test files remain enabled.

The first simulator attempt exposed an unbounded wait in the COMPLETE lifecycle
test fixture. That fixture now uses a one-byte source and waits for either the
COMPLETE gate or operation completion, so an early exit is reported immediately
instead of hanging the suite.

Existing deterministic suite coverage relevant to the requested fault matrix
includes download reopen/checkpoint/corruption/version/logout cases in
`FileDownloadRecoveryTests`, upload identity/offset/unknown-outcome/legacy-task
cases in `FileUploadCoordinatorTests` and `BackgroundUploadTests`, Keychain and
session fencing in `SessionRestorationTests`, SQLite corruption/future-schema/
open-failure handling in `MutationQueueSQLiteTests`, independent refresh and
expiration handling in `BackgroundSyncTests`, notification startup/session
fencing in `NotificationNavigationTests`, and durable conflict preservation in
`MetadataMutationFeatureTests`. Exact simulator totals are recorded below; no
simulator result is inferred from P051.

## Physical-device procedure (deferred)

Status for every procedure below: `DEFERRED_PHYSICAL_DEVICE_NOT_AVAILABLE`.
Run with a disposable test account/device, known fixture bytes, and a server-side
request ledger. Before and after each step, record the durable state, operation
identity, saved server offset, current session revision, and preview eligibility.
Capture request counts and response classifications without bearer tokens,
Keychain contents, private paths, or filenames. Each step must show that relaunch
alone creates no transfer request; only an existing supported explicit action
may do so.

| Procedure | Steps and evidence to capture | Expected safe result |
| --- | --- | --- |
| Background during foreground download | Start a pinned-version download, record committed bytes, background Synveil while content is in flight, then reactivate and reopen that file. | No claim of continued foreground URLSession execution; only committed bytes remain; no automatic retry; explicit Resume revalidates the same FileVersion. |
| Background during foreground upload | Start an upload and background during PATCH; record the server operation and final durable row after reactivation. | If response is lost, retain OUTCOME_UNKNOWN and the previous authoritative offset; if a response arrived, retain its returned offset; no next PATCH/COMPLETE until explicit recovery. |
| Lock the iPhone | Repeat the download and upload cases, lock while each is active, then unlock and inspect protected-data availability and saved presentation. | No stale progress publication; protected data remains gated until available; no automatic transfer replay. |
| Normal system termination | Start a transfer, background the app, allow the OS to terminate it under ordinary resource pressure, then cold-launch. Record whether termination was actually delivered or observed. | Durable evidence is sufficient without relying on a termination callback; no transfer starts on launch. If the OS does not terminate the app during the attempt, record it as not observed, not PASS. |
| Cold relaunch | After normal termination, launch from the app icon with a valid session and navigate to the relevant item. | Saved transfer state is read locally; no file-content request or fabricated live progress; recovery action remains explicit. |
| User force-quit | Begin a transfer, use the app switcher to force-quit, relaunch, and inspect the durable record and server ledger. | Force-quit is not treated as cancellation evidence or replay permission; no automatic content request. |
| Protected-data changes | While a saved record exists, lock/unlock the device and observe protected-data availability callbacks and Keychain/storage reads. | Inaccessible data is shown as unavailable/authentication-required, never as an empty queue; retry only after the protected store is available. |
| Network switching | Interrupt Wi-Fi/cellular while PATCH or download content is in flight, switch networks, reactivate, and compare server and SQLite offsets. | Preserve the prior authoritative offset when uncertain; do not duplicate the operation; require explicit Resume/status recovery. |
| Account logout | Start a transfer, log out during a response, then inspect UI and durable evidence before signing into another account. | Old protected UI is cleared and callbacks are fenced; the former account's record cannot attach to the new session. |
| Device credential revocation | Revoke the device credential server-side while a transfer is active, then reactivate and attempt only a local recovery inspection. | No request uses the revoked credential; present session/recovery-required state and retain scoped evidence under existing cleanup policy. |
| Storage exhaustion | Fill the test device or app staging quota during a partial transfer, then inspect the typed storage result and saved checkpoint. | Show storage unavailable; retain last committed bytes and ownership; do not mark incomplete content verified or treat the read as empty. |
| Notification-triggered launch | Schedule an eligible local completion notification, terminate the app, tap it, and observe restoration/navigation ordering. | Revalidate the current session and durable notification eligibility before routing; no transfer replay, confidential filename, or duplicate completion notification. |
| P048 metadata refresh delivery | Explicitly opt in, allow the OS to deliver BGAppRefresh without a foreground scene, and inspect refresh/ACK and file-request ledgers. | Refresh remains bounded and metadata-only; no content request or transfer scheduling; expiration does not cancel unrelated foreground ownership. |
| Repeated foreground/background transitions | Repeat active/inactive/background callbacks during one transfer, then inspect generation changes and request ledger. | Old callbacks cannot publish into the new generation; activation never replays a transfer. |

Use a supported iOS version and retain the device model, OS build, timestamps,
sanitized request ledger, SQLite readback, and UI/accessibility observations with
the test record. Actual background scheduling and daemon behavior remain
separately deferred; forcing a simulator callback does not satisfy this device
procedure.

### Fault-injection map

| Scenario | Deterministic coverage/evidence | Required invariant |
| --- | --- | --- |
| Download stops before staging/checkpoint commit | `testWriteFailureNeverAdvancesOffset`, `testSynchronizationFailureDoesNotAdvanceCheckpoint` | SQLite offset stays at last committed bytes. |
| Process ends after partial bytes | `testPartialSurvivesRealDatabaseAndFilesystemReopen`; P052 restored-row no-replay test | Preserve the partial and require explicit Resume. |
| Process ends during verification | `testDurableProgressAndVerifyingSeparateFromCompletion`; recovery projection preserves `VERIFYING` | Do not expose bytes as verified; require verification/recovery action. |
| VERIFIED committed before UI publication | `testVerifiedFileSurvivesReopen`, `testVerifiedDuplicateReusesValidatedFile` | Durable VERIFIED evidence is re-read; no duplicate content request. |
| PATCH response lost or lifecycle interrupted around server acceptance | P052 `testLifecycleInterruptionPreservesUnknownPatchOutcomeWithoutReplay` and `testLifecycleInterruptionAfterAcceptedPatchPreservesOffsetAndStopsBeforeNextChunk`; P045 lost/partial response coverage | Keep prior offset when the outcome is unknown; persist a returned authoritative offset; never replay a PATCH automatically. |
| COMPLETE response lost or interrupted | P052 `testLifecycleInterruptionDuringCompleteDoesNotReconcileAutomatically`; existing lost-completion test | Preserve uncertainty; no automatic COMPLETE/status reconciliation. |
| SQLite commit/readback interruption | `MutationQueueSQLiteTests.testCommitAcknowledgementLossReadsBackCommittedIdentity`; transfer commit fault tests | Exact readback or typed unavailable failure; never guessed state. |
| Duplicate cold start / no foreground scene | Strengthened bootstrap single-flight test; `testDuplicateStartupCallsDoNotCreateDuplicateLoadsOrProbes` | One shared startup operation; no transfer submission. |
| Delegate and SwiftUI lifecycle callbacks overlap | P052 callback-fence test; shared event relay and bootstrap | Duplicate phases coalesce; stale callbacks are fenced. |
| Logout during download completion | `testLogoutFencesLateContent`, `testCapabilityRevokedAfterLogout` | Old session cannot publish a capability or view result. |
| Account/credential replacement during inspection | `testAnotherAccountCannotAdoptDelayedEvidence`, `testRetryDoesNotProbeCredentialThatChangedInKeychain` | No cross-account row or callback publication. |
| Stale callback after new session | Existing session-revision and delayed-read tests across transfer, sync, and notification suites | Old revision cannot mutate new session UI. |
| Credential restoration fails | `testTemporaryKeychainUnavailabilityRetainsRecoveryState`, `testReadFailureIsNotTreatedAsMissingSession` | Preserve recovery state; do not assume logout or empty history. |
| SQLite unavailable/corrupt/unsupported | `testOpenFailureIsTypedAndDoesNotBypassGate`, `testCorruptDatabaseIsPreserved`, `testUnknownFutureSchemaDoesNotResetDatabase` | Retain evidence and show unavailable status. |
| Protected data inaccessible | Keychain-unavailable restoration tests and native data-protection policy tests | Keep gated until protected storage can be read; device behavior remains deferred. |
| P046 legacy task remains | `testProductionRestoreWithdrawsLegacyAuthorizationAndCancelsWithoutRetry`, `testLateLegacyCallbackIsPersistedWithoutBackgroundStatusReadOrRetry` | Retain evidence; no PATCH, status GET, or reauthorization. |
| P051 capability after relaunch | Production capability OFF and restore-withdrawal tests; disabled download UI test | Capability remains OFF. |
| BGAppRefresh launch without scene | `testColdStartupConsumesTheDeliveredBudgetBeforeDispatch`, expiration/session-fencing tests | Metadata-only, bounded work; no file-content request. |
| Notification arrives before session restoration | `testTapBeforeStartupWaits`, stale-session and repeated-tap navigation tests | Wait for current authorization; no transfer replay or stale route. |
| Repeated active/inactive/background transitions | P052 lifecycle callback-fence test and download cancellation/re-entry test | Each transition advances the fence; activation never resumes a transfer. |
| FileVersion changes during recovery | `testCurrentVersionChangesDoNotSwitchPinnedVersion` | Keep the original immutable version or fail closed. |
| Queued mutation conflict exists | `testConflictReviewSurvivesQueueReopen`, `testUnknownOutcomeIsNeverReclassifiedAsConflict` | Preserve the conflict; lifecycle recovery performs no mutation. |

These are deterministic native test expectations, not physical-device or OS
scheduling results. The executed exact-head simulator totals are recorded in
the validation section below.

## Portable validation and native CI

Portable validation was rerun on the current workspace, including the P052
recovery assertions, bounded gate behavior, and device procedure documentation:

- `python3 clients/ios/Support/validate_ios_sources.py` — PASS.
- `python3 -m unittest discover -s clients/ios/Support/tests` — PASS,
  **106 tests**.
- `bash scripts/validate-docs.sh` — PASS.
- `git diff --check` — PASS.
- Strict Swift format, Xcode build, and simulator tests are unavailable in this
  Linux workspace (`swift`, `swift-format`, and `xcodebuild` are not installed).
  GitHub Actions is required; this is not a substitute pass.
- Validated source checkpoint: `842384148b462955d54984c4b77c9ac07fafe9e2`.
  GitHub Actions on PR #135 reported all four required gates successful:
  - iOS Static Validation: run `38841723718`,
    https://github.com/nghianguyen150612/Synveil/actions/runs/38841723718
  - iOS Build: run `38841723914`,
    https://github.com/nghianguyen150612/Synveil/actions/runs/38841723914
  - iOS Simulator Tests: run `38841723808`,
    https://github.com/nghianguyen150612/Synveil/actions/runs/38841723808
  - iOS Rust Apple Build: run `38841723772`,
    https://github.com/nghianguyen150612/Synveil/actions/runs/38841723772
- Simulator: iPhone 17 Pro, iOS 26.5. XCTest result: **1,933 total; 1,931
  passed, 0 failed, 2 skipped**. `xcresulttool` reported `Passed`; the artifact
  and run log are retained at the Simulator Tests URL above.
- The first simulator attempt on source checkpoint
  `5e1bb8a6abe237b5a83e5adaa89f838dc06473cb` (run `38839104055`) was cancelled
  while the COMPLETE lifecycle test waited on an unbounded test gate. Its
  captured partial log showed 564 completed tests passed, 0 failed, and the
  blocked test unfinished. That attempt has no full-suite totals and is not
  counted as a gate pass. The bounded gate correction was included in
  `842384148b462955d54984c4b77c9ac07fafe9e2`, whose full simulator run passed.
- A later manifest-only follow-up changes no iOS source or tests. Its four
  exact-head run URLs are tracked in PR #135 and the final report. No iOS source
  changes were made after the validated source checkpoint.
- Unrelated workflows on this source checkpoint: Rust CI (`38841723715`), Linux
  AppImage (`38841723722`), and PostgreSQL scheduled maintenance (`38841724312`)
  failed in Rust/dependency, desktop/web, and server areas outside the P052
  changes. Linux DEB/RPM packaging (`38841723949`) was still running at the time
  of this manifest update. These results do not replace or count as an iOS gate.

## Explicit permitted deferrals

| Requirement | Why unavailable | Risk | Current safe fallback | Evidence still needed | Re-entry condition |
| --- | --- | --- | --- | --- | --- |
| Physical iPhone lifecycle checks: backgrounding during download/upload, lock/unlock, normal termination, force-quit, protected-data changes, network switching, logout, credential revocation, storage exhaustion, notification launch, and P048 delivery | No physical iPhone is available in this execution environment. | Actual OS timing, file protection, storage pressure, and callback behavior can differ from the simulator. | Foreground transfers cancel safely when lifecycle callbacks arrive; durable evidence remains scoped; Resume/status actions stay explicit; P046/P051 content transfers stay disabled. | Device model/iOS version, controlled test records, durable-state readback, session/network/task evidence, and XCTest/physical-test artifacts. Record status as `DEFERRED_PHYSICAL_DEVICE_NOT_AVAILABLE`. | P052 device qualification before claiming physical validation; background-transfer behavior additionally requires separately approved P051 reentry. |
| Actual OS-managed background scheduling/daemon behavior | Linux cannot run iOS background sessions or BGTaskScheduler; simulator scheduling is not device proof. | Scheduling, relaunch, force-quit, redirect, and callback timing are not established here. | P046 upload and P051 download remain OFF; P048 retains its existing opt-in metadata-only behavior. | On-device OS scheduling and callback evidence for the specific supported iOS versions and deployment. | A separately authorized milestone with the P051 security gate satisfied. |
| Live deployment redirect behavior and independent security approval to reactivate P046/P051 | No verified controlled live deployment or independent security approval is available; this is inherited P051 deferred scope. | Authenticated background requests could be redirected or expose credentials/body data. | Existing production transport capability checks remain closed; legacy evidence is retained and not replayed. | Controlled deployment/edge evidence, independent security review, and physical-device transfer tests. | P051 security approval and explicit owner authorization. |

`DEFERRED` is not `PASS`. These deferrals do not block P052 functionality that
can be implemented and deterministically validated. They do not authorize
background upload/download.

## Open risks and limitations

- An ordinary foreground URLSession task may stop when iOS backgrounds or
  terminates the app. P052 preserves only durable checkpoints and requires an
  explicit supported Resume or status-recovery action.
- Protected-data and real OS scheduling/daemon behavior require the deferred
  physical-device procedures above. No simulator result proves those behaviors.
- P046 background uploads remain OFF and P051 background downloads remain
  unimplemented. Reentry requires the separate deployment evidence, physical
  validation, and independent security approval listed above.
- Upload outcomes whose server response was not received remain uncertain until
  a user explicitly invokes the existing reconciliation workflow; P052 does not
  claim server progress from a local lifecycle event.

## PR, review, and integration state

- PR number/URL: #135, https://github.com/nghianguyen150612/Synveil/pull/135.
- PR state and review state: open; reviewer `ducminh231011` requested; no review
  submitted and no approval is claimed.
- Merge SHA: not available; no merge is claimed. `ios-app` remains at the
  verified starting SHA `02bc2b8f8ac8d8e2968da9f22abb5387ee3d5df3` pending review.
- Next safe action: await the requested code review. Merge only after required
  review and repository protections pass; no merge is claimed here.
- P053: not started and not authorized.
