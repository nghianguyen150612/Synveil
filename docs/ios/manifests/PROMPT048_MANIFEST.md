# Prompt048 — Native Background Metadata Synchronization & Scheduling

## Baseline and scope

- Actual fetched starting `origin/ios-app`: `a4ec3d177c7e9ee27e5d2a0fec74310ad403984d`.
- P047 PR #130 is merged with that SHA; explicit ref fetch, ancestry and GitHub verification agree.
- Feature branch: `ios/p048-background-metadata-sync`; target: `ios-app`.
- Initial worktree was clean. No applicable AGENTS.md was present. Existing desktop checkout/history
  was preserved; implementation started from the fetched iOS integration ref, not desktop main.
- Reviewed iOS contract, platform mapping, roadmap, P038–P047 manifests, Android WorkManager
  worker/settings, native bootstrap, session restoration, providers, queue ownership, inbound
  application/ACK, projection and P047 authorization evidence. P049 remains outside authorization.

## Architecture and native registration

`BackgroundSyncPolicyStore` owns a versioned, bounded UserDefaults record. `BGAppRefreshScheduler`
owns native registration/submission/cancellation and delivery completion. `BackgroundSyncCoordinator`
owns policy fencing, deadline, bounded fair selection and orchestration. `BackgroundSyncWork` adapts
the existing `InboundSyncCoordinator.synchronize` entry point. Production composes a background
instance of the existing engine with the same SQLite connection and durable queue, and a restricted
transport; no parallel protocol implementation is introduced.

The app bundle is `com.synveil.ios`. The independent task identifier is
`com.synveil.ios.sync.refresh.v1`. The app target declares the Background Modes capability. Both app configurations merge typed input plist arrays:
`BGTaskSchedulerPermittedIdentifiers` contains exactly that identifier; `UIBackgroundModes` contains
only `fetch`. No processing, audio, location or unrelated entitlement is added.

Registration happens once in the existing `BackgroundUploadAppDelegate.didFinishLaunching` before
starting bootstrap. There remains one UIApplicationDelegateAdaptor. A shared session startup task
serves scene-free metadata delivery and the unchanged P046 restoration/event flow. Metadata refresh
does not wait for upload restoration and cannot create, PATCH, COMPLETE or recover an upload.

Official Apple references reviewed:

- [Register task handlers](https://developer.apple.com/documentation/backgroundtasks/bgtaskscheduler/register(fortaskwithidentifier:using:launchhandler:)) — registration deadline and duplicate registration prohibition.
- [BGAppRefreshTask](https://developer.apple.com/documentation/backgroundtasks/bgapprefreshtask).
- [Earliest begin date](https://developer.apple.com/documentation/backgroundtasks/bgtaskrequest/earliestbegindate) — lower boundary, no guaranteed launch date.
- [Expiration handler](https://developer.apple.com/documentation/backgroundtasks/bgtask/expirationhandler).
- [Task completion](https://developer.apple.com/documentation/backgroundtasks/bgtask/settaskcompleted(success:)).

## Settings, scheduling and account policy

- Default OFF. A settings action explicitly verifies authorization through the existing restoration
  service before writing opt-in. Enrollment or migration does not silently enable the feature.
- Opt-in binds canonical HTTPS endpoint, owner, device and non-secret credential identifier, plus a
  local grant UUID. It contains no bearer, signed ACK, private path or server checkpoint.
- Missing, malformed, oversized, unsupported or invalid settings are disabled. Writes are read back.
- Logout/authorization invalidation cancels requests and withdraws the persisted opt-in. An account
  switch cannot use another account's grant. In-flight work is fenced by both grant and lifecycle. A positively identified different account disables the previous opt-in even on cold launch. Failed preference writes keep execution closed in-process and do not report a successful settings save.
- Wi-Fi preferred / unmetered only is the default network setting; Wi-Fi and cellular is optional.
- Low Power Mode and unavailable/disabled Background App Refresh defer execution and submission.
- One pending request per identifier is checked before submission. A six-hour lower boundary and
  persisted submission cooldown apply to both successful and failed submissions, including relaunch.
  This is a conservative scheduling policy, not a measured adaptive strategy or delivery interval.
- There is no timer for periodic scheduling, network polling, keep-alive or guaranteed next-run UI.
  The only sleep is a delivery-scoped, cancellable execution deadline watchdog.
- All previously initialized eligible saved scopes participate; specific-library selection is not
  offered. Two SQL seek reads select at most two identifiers in UUID order, wrapping after the
  persisted selection cursor. Advancing before an attempt prevents persistent errors from starving
  other libraries. The cursor records attempts only and never manufactures sync progress.

## Authorization, ownership and recovery

Production requires authenticated SessionController state, current Keychain identity, endpoint,
device, credential and lifecycle revision. P047 `AUTHORIZED` evidence permits inbound work.
Ordinary `TRANSIENT` evidence without a verification requirement can permit only a fresh bounded
authorization check. CHECKING, REJECTED and VERIFY_REQUIRED cannot authorize background work.
`restorationVerificationPending` is never upgraded by this layer.

Authorization verification first claims the existing global inbound run lock. Feed operations claim
the same lock through the existing inbound coordinator. Background delivery cannot steal ownership
from foreground Sync Now, explicit ACK recovery or rebaseline. SQLite scope/CAS protections and
session validation remain authoritative over durable writes. Multiple native deliveries also have a
delivery fence; expiring a task never force-releases an active durable owner. Cancellation during cached Keychain capture no longer quarantines a healthy foreground owner; a targeted regression holds that owner through canceled background capture.

Only read-only verification, feed retrieval, Node metadata materialization, projection commit and the
initial ACK of a newly fetched/applied page are allowed. Existing unresolved pages, including
RECEIVED_UNAPPLIED and APPLIED_ACK_PENDING from an earlier run, require foreground action under the
background configuration. ACK_IN_FLIGHT retains its original token/attempt evidence and explicit
recovery requirement. No unknown ACK replay, mutation drain/replay, automatic rebaseline, cache
repair, content download/upload, replacement, export, deletion or share presentation is introduced.

TLS, malformed feeds and protocol errors are failures, never connectivity fallbacks. A 503/offline
feed attempt preserves state and stops without polling. Revocation/rejection follows existing
invalidation/quarantine behavior. Background metadata denial quarantines the original library.

## Execution, network and memory budgets

- At most two libraries, sequentially.
- Per library: one feed page, page size eight, at most eight events. Total: two pages / sixteen events.
- Delivered task deadline: 24 seconds from delivery, including cold session startup; earlier system expiration wins.
- Each metadata request uses the existing streaming HTTP reader with a three-second request timeout,
  four-second resource timeout and 64 KiB response limit. No new request begins with less than the
  normal four-second resource bound plus a one-second cleanup reserve.
- Feed, authorization, Node materialization and ACK all pass through the same delivery request gate.
  Library feeds and materialization retain all existing hard validation limits. A previous durable
  feed may require existing bounded 2 MiB validation before it is deferred; no unbounded buffering
  is added. Eight events produce at most sixteen subject/parent materializations, each under the
  existing 16 KiB metadata limit; the existing 2 MiB plan cap remains intact.
- Unmetered transport sets cellular, expensive and constrained access false. Cellular opt-in enables
  cellular/expensive access; constrained access still remains false. Connectivity waiting is disabled.
  URLSession enforces supported request constraints. No NWPathMonitor or interface-stability promise
  is used. iOS policy can change during an in-flight request.
- Startup retains the existing bounded application session-restoration pass. Metadata delivery uses
  the same startup task and checks expiration/cancellation before its own operations. Existing startup
  and P046 callbacks remain independently owned and are not canceled by a metadata expiration.

## Expiration, completion and durable status

Expiration synchronously fences success, cancels the delivery worker and completes the native task
unsuccessfully exactly once. Cleanup does not block that completion callback. Cooperative existing
services preserve staged pages, atomic application and unknown ACK evidence. They release ownership
through the existing finish path when cancellation returns; a second delivery cannot steal it while
cleanup remains active. Termination/suspension still requires the existing durable recovery procedure.

Background orchestration states are transient. Durable progress remains in SQLite v9, with no schema
migration or new table. Successful page publication requires validated ACK_CONFIRMED readback.
Partial success means a bounded confirmed batch, never all-library completeness. Disabled, deferred,
failed, expired, offline, authentication/recovery and invalidated tasks cannot claim full success.

The native settings Form supports Dynamic Type, VoiceOver and stable identifiers. It explains system
control and separates a pending request from execution. The displayed **Last saved server
confirmation** uses the existing confirmed page's SQLite timestamp and positions, including
foreground confirmations; it is not labeled as the last background wake or attempt. No scheduling,
attempt, locally applied or speculative next-run timestamp is substituted. Saved unresolved inbound
evidence has a separate manual-action notice. Privacy-safe OSLog events contain finite state names,
registration/submission outcomes and bounded counters, with no secrets or private payloads.

## Validation and security review

- Portable validation: source validator PASS; 98 Python tests PASS (eight new registration/configuration
  tests); strict official swift-format lint PASS; documentation validation PASS; diff check PASS.
- New targeted native tests: 53 XCTest methods in BackgroundSyncTests and BackgroundSyncSafetyTests.
  They cover registration/default opt-out, corrupt storage, scheduling/cooldown/relaunch, disable
  races, account/session fences, constraints, delivery expiration and duplication, budgets, fairness,
  durable confirmation, actual SQLite feed/application/ACK interruption, foreground cross-connection
  ownership, unknown ACK, rebaseline, 503/offline, revocation and P046 operation separation.
- Existing P001–P047 tests remain registered and enabled.
- Frozen implementation source: `77a2f4550ab3d85b0d1251760e79258f8db989b6`. All four required native workflows passed on this source; no Linux check is represented as Apple execution. Initial native Build failed on a protocol/concrete-store type mismatch and was corrected. The next Build passed; Simulator compilation then found a test assertion using an incorrect token member, now corrected to existing evidence.token. These failures are not counted as test passes. Source `2ae1fe9bd7fe7ccaad5c555d982ec6bb217135e8` then executed 1,797 tests: 1,794 passed, two existing skips, one failure. [Simulator run 38493222157](https://github.com/nghianguyen150612/Synveil/actions/runs/38493222157) identifies only the 503 fixture using an undocumented error code; it is corrected to dependency_unavailable without weakening the protocol. Two additional cancellation/write-failure regressions bring P048 to 53 native methods.
- Implementation security review checks allowed transport methods, opt-in/session binding,
  verification evidence, retained schema v9, global lock ownership, preserved ACK uncertainty and
  separate upload callbacks. This implementation review is not independent PR acceptance.

## Physical-device validation

Status: **NOT_AVAILABLE**. No attached real iOS device or signing environment is available.
Simulator delivery tests cannot establish reliable iOS wake frequency or production qualification.

Real-device procedure (record device/OS, source SHA and bounded privacy-safe diagnostics):

1. Install a signed iOS 17+ build; confirm default OFF and no pending refresh. Enable Background App
   Refresh in system settings and explicitly opt in after authenticated library checkpoint setup.
2. Background the app and allow a genuine eligible OS opportunity. Capture registration/submission
   and real delivered-task logs. Do not count Xcode's simulated launch as natural delivery evidence.
3. Confirm at most two libraries/eight events each; compare SQLite applied/server-confirmed positions
   with the server and ensure no content transfer or outbound mutation occurs.
4. Hold foreground Sync Now ownership while delivering a refresh; verify deferral and unchanged lease.
5. Test locked device after first unlock and before first unlock after restart; unavailable Keychain
   must stop. Restart/relaunch and inspect retained preferences and durable evidence.
6. Enable Low Power Mode and disable system refresh; verify safe deferral. Change Wi-Fi/cellular and
   Low Data Mode during and between requests; check actual URLSession policy behavior.
7. Force quit, then allow time and relaunch. Record the OS behavior without promising delivery.
8. Revoke the device and retry; verify the current-scope quarantine and no new feed/content work.
9. Exercise expiration during GET, staging, application and ACK dispatch; inspect durable received,
   applied or unknown evidence after relaunch and confirm exactly one native completion.
10. Deliver real P046 background URLSession events during a refresh. Verify upload callback persistence
    and completion remain independent and no metadata task initiates upload recovery/finalization.

## GitHub acceptance and blockers

- PR: [#131](https://github.com/nghianguyen150612/Synveil/pull/131), OPEN; ordinary merge commit is required after all four final-head native gates and
  independent required review acceptance. Review requested from write-access collaborator `ducminh231011`; no approval or acceptance is fabricated.
- Initial source SHA: `2729928c41c6187f6e5db84b0a7e229e24cd5894`; corrected composition SHA: `051a2b6d2afa466e817eae6eb49d1fbfeff1d336`. Final implementation source is `77a2f4550ab3d85b0d1251760e79258f8db989b6`; native evidence follows below.
- Merge/integration SHA: NOT_MERGED. Verified integration remains the starting P047 SHA until accepted.
- Blocker: independent review acceptance is not yet obtained; physical-device evidence is
  NOT_AVAILABLE and disclosed rather than fabricated. P049 has not been started.

## Changed files

New files:

- `clients/ios/App/BGAppRefreshScheduler.swift` and `BackgroundSyncInfo.plist`.
- `clients/ios/Application/Sync/BackgroundSyncCoordinator.swift` and `BackgroundSyncWork.swift`.
- `clients/ios/Domain/Sync/BackgroundSyncModels.swift`.
- `clients/ios/Features/Sync/BackgroundSyncSettingsView.swift`.
- `clients/ios/Infrastructure/Network/BackgroundSyncHTTPTransport.swift`.
- `clients/ios/Infrastructure/Persistence/BackgroundSyncPolicyStore.swift`.
- `clients/ios/Support/tests/test_background_sync_registration.py`.
- `clients/ios/Tests/SynveilTests/BackgroundSyncTests.swift` and `BackgroundSyncSafetyTests.swift`.
- This manifest.

Modified files:

- `clients/ios/App/AppDependencyContainer.swift`, `BackgroundUploadAppDelegate.swift`, `SynveilApp.swift`.
- `clients/ios/Application/Library/AuthenticatedLibraryRequestProvider.swift`.
- `clients/ios/Application/Mutation/DurableMutationQueue.swift` (cancellation does not withdraw authorization).
- `clients/ios/Application/Sync/InboundSyncCoordinator.swift`.
- `clients/ios/Domain/Sync/InboundSyncModels.swift`.
- `clients/ios/Features/Library/LibraryCatalogView.swift`.
- `clients/ios/Infrastructure/Network/URLSessionHTTPTransport.swift`.
- `clients/ios/Infrastructure/Persistence/MutationQueueSQLiteStore.swift` (bounded reads/ownership only).
- `clients/ios/Synveil.xcodeproj/project.pbxproj`.

## Verified native evidence and final-head publication

All rows below are actual **push** runs with head SHA
`77a2f4550ab3d85b0d1251760e79258f8db989b6`, not historical or unrelated runs.

| Required workflow | Result | Run |
|---|---|---|
| iOS Static Validation | PASS | [38495285107](https://github.com/nghianguyen150612/Synveil/actions/runs/38495285107) |
| iOS Build | PASS | [38495285041](https://github.com/nghianguyen150612/Synveil/actions/runs/38495285041) |
| iOS Simulator Tests | PASS | [38495284948](https://github.com/nghianguyen150612/Synveil/actions/runs/38495284948) |
| iOS Rust Apple Build | PASS | [38495285053](https://github.com/nghianguyen150612/Synveil/actions/runs/38495285053) |

The native xcresult summary reports **1,799 total: 1,797 passed, 0 failed, 2 skipped**.
`BackgroundSyncSafetyTests` executed 26 and `BackgroundSyncTests` executed 27, all passed.
Existing skips are `KeychainCredentialStoreTests/testSimulatorKeychainRoundTripUsesUniqueTestService`
and `MutationQueueSQLiteTests/testNativeDataProtectionAttributes`. No P048 test is skipped.
The run used Xcode 26.6, Swift 6.3.3 and an iOS 26.5 Simulator. Existing project Swift 5 compatibility
language settings and strict concurrency validation remain unchanged; this is not a claim that
all existing application sources are migrated to Swift 6 language mode. Existing warnings in
transfer/export, staging and Rust bridge code remain visible and unsuppressed.

This evidence update changes documentation only. The current feature Git SHA is available from
[PR #131](https://github.com/nghianguyen150612/Synveil/pull/131). Four additional workflow-dispatch
runs on that final documentation-updated head are required and tracked in the
[PR checks](https://github.com/nghianguyen150612/Synveil/pull/131/checks) and final delivery report.
The production source tree must remain identical to the frozen implementation above.
No old run is substituted for this final-head verification.

P048-owned authorization/feed/Node/ACK requests use the restricted metadata transport. The separately
owned existing application-startup authorization probe and P046 transfers retain their original
transport policies. This preference does not constrain every application network request. Startup
elapsed time still counts against the delivered metadata task's budget. Real-device network, lock,
expiration and delivery behavior remains NOT_AVAILABLE and must follow the procedure above.

## Unrelated CI failures and review disposition

The following actual PR runs at the frozen implementation SHA fail in sources untouched by P048:

- [Rust CI 38495289574](https://github.com/nghianguyen150612/Synveil/actions/runs/38495289574):
  object-store clippy double_must_use, Linux systemd-tmpfiles unsupported --dry-run, and macOS desktop
  control/launch UnsafeEndpoint failures.
- [PostgreSQL 17 38495289643](https://github.com/nghianguyen150612/Synveil/actions/runs/38495289643):
  scheduler schema test expects 34 and observes 36.
- [Linux AppImage 38495289662](https://github.com/nghianguyen150612/Synveil/actions/runs/38495289662):
  desktop artifact inspection detects a private/temporary build-path marker.
- [Linux DEB/RPM 38495289671](https://github.com/nghianguyen150612/Synveil/actions/runs/38495289671):
  still running at evidence publication; no result is fabricated.

No server, desktop, Android, Rust crate, workflow or script source differs from the verified baseline.
These failures have not been disabled or repaired outside P048. PR #131 is open, ready for review,
and unmerged. There are no submitted reviews at evidence capture; a request is not approval.
Ordinary merge-commit integration is authorized only after recorded review acceptance and all four
final-head native gates. Merge SHA: NOT_MERGED. Verified current integration SHA remains
`a4ec3d177c7e9ee27e5d2a0fec74310ad403984d`. P049 is not started.

Implementation disposition: **IMPLEMENTED_PENDING_REVIEW_OR_MERGE**, subject to the final-head
reverification described above. No physical-device or production qualification is claimed.
