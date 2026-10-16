# P052 — App Lifecycle, Durable Transfer Recovery & Session-Safe Restoration

## Checkpoint

- Starting integration branch: `ios-app`.
- Verified starting integration SHA: `02bc2b8f8ac8d8e2968da9f22abb5387ee3d5df3`.
  `origin/ios-app` was fetched and matched this SHA; it is an ancestor of the
  P052 feature branch.
- Feature branch: `ios/p052-app-lifecycle-transfer-recovery`.
- Feature commit(s): pending final source review and commit.
- Current PR, review, merge, and final integration SHAs: not yet available.
- Current checkpoint is **not** a P052 completion claim. Required native iOS
  build, simulator, and Rust Apple workflows have not yet run on P052 source.

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
remains `OUTCOME_UNKNOWN`. Lifecycle cancellation does not trigger a replacement
PATCH or automatic status GET. Cancellation during COMPLETE or an explicit abort
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

## Deterministic XCTest coverage

P052 adds these focused methods (native execution is pending):

- `SynveilBootstrapTests.testDelegateAndSceneLifecycleEventsShareOneCallbackFence`
- `FileDownloadViewTests.testUnavailableDownloadStorageIsNotPresentedAsEmptyHistory`
- `FileDownloadViewTests.testRestoredDownloadingRecordShowsInterruptedAndDoesNotReplay`
- `FileDownloadViewTests.testLifecycleSuspensionCancelsForegroundDownloadAndReloadsDurableState`
- `FileUploadCoordinatorTests.testLifecycleInterruptionPreservesUnknownPatchOutcomeWithoutReplay`
- `FileUploadCoordinatorTests.testLifecycleInterruptionDuringCompleteDoesNotReconcileAutomatically`

`BackgroundUploadTests.testApplicationBootstrapRunsWithoutSceneAndOnlyOnce` was
strengthened to hold startup behind a deterministic gate while concurrent
waiters arrive. Existing P001–P051 test files remain enabled.

Existing deterministic suite coverage relevant to the requested fault matrix
includes download reopen/checkpoint/corruption/version/logout cases in
`FileDownloadRecoveryTests`, upload identity/offset/unknown-outcome/legacy-task
cases in `FileUploadCoordinatorTests` and `BackgroundUploadTests`, Keychain and
session fencing in `SessionRestorationTests`, SQLite corruption/future-schema/
open-failure handling in `MutationQueueSQLiteTests`, independent refresh and
expiration handling in `BackgroundSyncTests`, notification startup/session
fencing in `NotificationNavigationTests`, and durable conflict preservation in
`MetadataMutationFeatureTests`. Exact method-to-case coverage and exact simulator
pass/fail/skip totals will be updated from the final P052 simulator run; no
simulator result is inferred from P051.

### Fault-injection map

| Scenario | Deterministic coverage/evidence | Required invariant |
| --- | --- | --- |
| Download stops before staging/checkpoint commit | `testWriteFailureNeverAdvancesOffset`, `testSynchronizationFailureDoesNotAdvanceCheckpoint` | SQLite offset stays at last committed bytes. |
| Process ends after partial bytes | `testPartialSurvivesRealDatabaseAndFilesystemReopen`; P052 restored-row no-replay test | Preserve the partial and require explicit Resume. |
| Process ends during verification | `testDurableProgressAndVerifyingSeparateFromCompletion`; recovery projection preserves `VERIFYING` | Do not expose bytes as verified; require verification/recovery action. |
| VERIFIED committed before UI publication | `testVerifiedFileSurvivesReopen`, `testVerifiedDuplicateReusesValidatedFile` | Durable VERIFIED evidence is re-read; no duplicate content request. |
| PATCH response lost or lifecycle interrupted after server acceptance | P052 `testLifecycleInterruptionPreservesUnknownPatchOutcomeWithoutReplay`; P045 lost/partial response coverage | Keep prior authoritative offset and OUTCOME_UNKNOWN; no PATCH/GET replay. |
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
scheduling results. The final manifest update will record which exact-head
simulator tests passed, failed, or skipped.

## Portable validation and native CI

At this checkpoint:

- `python3 clients/ios/Support/validate_ios_sources.py` — PASS.
- `python3 -m unittest discover -s clients/ios/Support/tests` — PASS,
  **106 tests**.
- `bash scripts/validate-docs.sh` — PASS before this manifest was added; rerun
  after the final documentation update.
- `git diff --check` — PASS before the final test/doc edits; rerun on the final
  source head.
- Strict Swift format, Xcode build, and simulator tests are unavailable in this
  Linux workspace (`swift`, `swift-format`, and `xcodebuild` are not installed).
  GitHub Actions is required; this is not a substitute pass.
- Required exact-head workflow URLs and outcomes: pending P052 push/PR.
  Required workflows: iOS Static Validation, iOS Build, iOS Simulator Tests,
  iOS Rust Apple Build.
- Full XCTest totals: pending exact-head iOS Simulator Tests.
- Unrelated Rust/server/desktop workflow outcomes: pending and will be reported
  separately if triggered; they do not replace an iOS gate.

## Explicit permitted deferrals

| Requirement | Why unavailable | Risk | Current safe fallback | Evidence still needed | Re-entry condition |
| --- | --- | --- | --- | --- | --- |
| Physical iPhone lifecycle checks: backgrounding during download/upload, lock/unlock, normal termination, force-quit, protected-data changes, network switching, logout, credential revocation, storage exhaustion, notification launch, and P048 delivery | No physical iPhone is available in this execution environment. | Actual OS timing, file protection, storage pressure, and callback behavior can differ from the simulator. | Foreground transfers cancel safely when lifecycle callbacks arrive; durable evidence remains scoped; Resume/status actions stay explicit; P046/P051 content transfers stay disabled. | Device model/iOS version, controlled test records, durable-state readback, session/network/task evidence, and XCTest/physical-test artifacts. Record status as `DEFERRED_PHYSICAL_DEVICE_NOT_AVAILABLE`. | P052 device qualification before claiming physical validation; background-transfer behavior additionally requires separately approved P051 reentry. |
| Actual OS-managed background scheduling/daemon behavior | Linux cannot run iOS background sessions or BGTaskScheduler; simulator scheduling is not device proof. | Scheduling, relaunch, force-quit, redirect, and callback timing are not established here. | P046 upload and P051 download remain OFF; P048 retains its existing opt-in metadata-only behavior. | On-device OS scheduling and callback evidence for the specific supported iOS versions and deployment. | A separately authorized milestone with the P051 security gate satisfied. |
| Live deployment redirect behavior and independent security approval to reactivate P046/P051 | No verified controlled live deployment or independent security approval is available; this is inherited P051 deferred scope. | Authenticated background requests could be redirected or expose credentials/body data. | Existing production transport capability checks remain closed; legacy evidence is retained and not replayed. | Controlled deployment/edge evidence, independent security review, and physical-device transfer tests. | P051 security approval and explicit owner authorization. |

`DEFERRED` is not `PASS`. These deferrals do not block P052 functionality that
can be implemented and deterministically validated. They do not authorize
background upload/download.

## PR, review, and integration state

- PR number/URL: not yet created.
- PR state and review state: pending; no review is claimed.
- Merge SHA and final `ios-app` SHA: not available; no merge is claimed.
- Next safe action: finish source review and portable checks, commit and push the
  feature branch, open exactly one PR against `ios-app`, request code review,
  and record the four required exact-head workflow results.
- P053: not started and not authorized.
