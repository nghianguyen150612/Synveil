# P049 — Native Local Notifications, Secure Navigation and Lifecycle Integration

## Repository and scope

- Verified starting integration SHA: `0f39b9ab141d46df0bf7df76f5d4886fca044a4a`.
- `origin/ios-app` was fetched explicitly; P048 merge ancestry passed. The checkout was clean.
- Feature branch: `ios/p049-native-notifications-lifecycle`; target: `ios-app`.
- Initial source SHA: `d45fc29d02cb4041f4262581731a2ee9ba97309a`; final source head is recorded by [PR #132](https://github.com/nghianguyen150612/Synveil/pull/132) and the final report.
- P043–P048 manifests, product contract, architecture, roadmap and platform mapping were reviewed.
- No desktop/server/Android/API changes. No remote push, APNs, extension, PhotoKit or new scheduler.
- P050 is not authorized and has not been started.

## Architecture and native configuration

The classifier accepts already-established durable records. The authority adapter reopens those
records under the current Keychain-backed session. The policy store holds independent non-secret
preferences. The coordinator checks system permission, scope, lifecycle, category, expiry and
deduplication. The scheduler alone wraps UserNotifications. A separate bounded delivery ledger
records eligibility and irrevocable scheduling claims. The navigation coordinator holds one opaque
pending intent and resolves it through authenticated repositories into existing SwiftUI screens.

The existing `BackgroundUploadAppDelegate` installs the single UNUserNotificationCenterDelegate.
It retains P046 URLSession handling and P048 BGAppRefresh registration/startup order. Delegate
completions return immediately; a bounded bootstrap relay accepts callbacks before dependencies
exist. A SwiftUI scene is not required to initialize session or presentation persistence.
There is no push entitlement or remote-registration call. Requests use generic text, no sound,
badge, critical or time-sensitive interruption. Foreground presentation returns no system banner;
an independent observable counter never claims that the user saw an outcome.

## Permission lifecycle and Settings

Global notifications and every category default OFF. Only the explicit Settings enable action can
request alert permission, and only while system status is notDetermined. Denial is never reprompted.
Granted permission does not enable categories. Actual system settings are reread before scheduling
and on foreground/Settings entry. Provisional, ephemeral, denied, unavailable and disabled alerts
fail closed. Settings explains iOS delivery control and opens system Settings when appropriate.
Native Form/Toggle controls support accessibility identifiers, multiline text and Dynamic Type.
Background synchronization retains its separate P048 opt-in; neither setting enables the other.

## Categories and authoritative eligibility

| Category | Required durable authority | Destination |
|---|---|---|
| Verified download | P043 final VERIFIED commit/readback, exact length and SHA-256; read-only published-cache recheck | Existing file details |
| Committed upload | P045 COMMITTED record, server COMMITTED state, exact persisted completion evidence, matching Node/content/session, no uncertainty | Existing file details |
| Background sync updates | P048 meaningful completed refresh plus nonempty P039 ACK_CONFIRMED page, projection commit and confirmed-checkpoint readback | Existing sync status |
| Recovery requires attention | Existing uncertain upload state requiring explicit status, or durable ACK_IN_FLIGHT page | Existing upload browser or sync status |

Download hooks run after final commit and session fencing. Upload observations run only after
exact durable readback; ordinary PATCH/readyToComplete, OS chunk completion, partial downloads,
integrity failures and offline failures do not produce completion events. Background summaries
bind the original refresh lifecycle revision and exact committed confirmation date/position. They
are conservative: no-work, deferred, partial, canceled, expired and recovery results do not become
success notifications. Related summaries share a six-hour presentation quiet period.

Recovery notification identity uses the original operation and authoritative position, not retry
callbacks. The same unresolved condition is suppressed, including a recurrence at the same
position. A new durable position/page can qualify again. Notification failures do not create
recovery state or modify transfer, mutation, projection, quarantine, ACK or rebaseline evidence.

## Privacy and typed events

Titles are exactly the generic category texts; body is “Open Synveil to view the current status.”
Identifiers contain an opaque random local event UUID. userInfo contains only that same UUID.
No filename, Library/account name, host, private URL/path, token, digest, upload-session ID,
response body, request body or signed ACK enters payloads, system identifiers or logs.

Stored candidates contain only local operation references, internal Library identifiers and
non-secret scope/proof fingerprints. Scope fingerprints bind endpoint, owner, Device and credential
identity; they contain no bearer. Each typed event binds its evidence category, opaque event UUID,
underlying operation, authorization scope, policy generation, observed lifecycle revision,
eligibility state and seven-day expiry. Transient credential-bearing request scopes stay inside
the existing secure request boundary and never enter ledger, navigation or observable state.

## Persistence, retention and crash windows

SQLite remains **v9**, with no migration or operation-schema changes. Existing migration paths,
future-schema rejection and every recovery/background table remain untouched. The separate
presentation snapshot reuses MutationQueueFilePolicy (private 0700 directory, 0600 files,
backup exclusion and completeUntilFirstUserAuthentication). Writes use a bounded temporary file,
fsync, same-directory rename and directory fsync under an instance-independent file lock.
Truncated, corrupt, oversized or future-version metadata disables presentation rather than
resetting deduplication. Storage failure cannot poison the operation database.

Bounds: 16 owned system pending requests; 16 unexpired claimed/scheduled ledger records including
uncertain receipts; 512 event/dedup records and 512 KiB snapshot; one pending navigation intent;
one typed diagnostic value; bounded foreground observation counter. Routing expires after seven
days; pending startup intents expire after ten minutes. Same-generation dedup tombstones remain
until explicit policy generation retirement. At the record cap, new presentation fails closed.
Disabling notifications/logout retires a generation; new-generation insertion can remove its
invalidated presentation records, without deleting any operation evidence.

There is **no strict exactly-once delivery claim** across Apple and local storage:

1. Process death after outcome commit but before observation/eligibility recording can lose a
   notification. Core outcome remains authoritative. Startup does not scan historical operations
   and does not fabricate new events.
2. Persisted unclaimed eligibility can be rechecked and claimed after relaunch.
3. Claim commits before calling Apple. Crash between claim and system call can lose a notification.
4. Crash after system scheduling but before receipt acknowledgment leaves an irrevocable claim.
   It is never blindly retried, even if iOS removed, suppressed or delivered the request.
5. A lost claim-write receipt fails closed; the durable claim, if committed, still prevents retry.
6. Native delivery, Focus restrictions and preview settings are controlled by iOS.

## Navigation, logout and account isolation

Only a default-action response with an exact one-key UUID payload matching the owned identifier
is accepted. The UUID is an intent, never a capability. Navigation waits for session initialization,
retains an intent during pending verification, and uses the existing unauthenticated root surfaces
when login is required. It rechecks policy generation, expiry, current endpoint/owner/Device/
credential scope, durable event and resource availability. Current authenticated catalog and Node
reads validate access before constructing an existing typed route. Removed/inaccessible resources
show a generic unavailable state. Repeated taps consume a durable handled marker once.

Logout synchronously fences publication and navigation, retires policy generation, then removes
only identifiable Synveil-owned pending/delivered requests. Late scheduling receipts are removed.
An old routing record cannot be adopted by a new account. Physical notifications already displayed
cannot always be recalled; notification removal is not server credential revocation. Taps never
export, preview, share, complete uploads, synchronize, mutate, rebaseline or recover ACKs.

## P046 and P048 coexistence

Notification observations dispatch independent presentation tasks. No Apple scheduling call is
awaited by a transfer commit, background URLSession completion handler or BGAppRefresh completion.
P046 chunk scheduling/status/offset/finalization protocol is unchanged; a completed OS chunk is not
upload publication. P048 work budgets, authorization, opt-in, refresh scheduling and unknown-ACK
boundaries are unchanged. Notification callbacks do not submit refresh tasks or retrieve feeds.

## Test inventory and local validation

Five native suites plus shared test support cover permission lifecycle, independent defaults,
external permission changes, eligibility, privacy, claims/crash windows, concurrent calls, caps,
relaunch, account/logout races, startup intents, tampered payloads, removed resources, actual
file-backed transfer/inbound authorities, ledger corruption/faults and small-screen/Dynamic Type
SwiftUI rendering. Seven new Python guards verify target membership, delegate ownership, capability
exclusions, remote-push exclusion and SQLite v9 isolation. Existing P001–P048 tests remain enabled.

New XCTest inventory: NotificationCoordinatorTests 41; NotificationEligibilityTests 18;
NotificationNavigationTests 21; NotificationPersistenceTests 19; NotificationViewTests 4.
Total new cases: **103**. Local Python Support suite: **105 passed, 0 failed**. Source validation,
official Swift 6.2 strict formatting/lint, Swift parser, docs validation and diff whitespace: PASS.
Xcode/Apple SDKs are unavailable on this Linux host; local parsing is not a native build.
A supplemental Swift 6.2.3 Linux harness passed **80 XCTest cases, 0 failures, 0 skips** for
presentation policy, ledger and navigation. It uses production presentation sources/Observation,
OpenSSL SHA-256 in place of CryptoKit and interface fixtures for Apple/operation adapters. The
SQLite cleanup integration case, eligibility and SwiftUI suites remain enabled in Xcode and are
excluded from this supplemental harness; these results are not Apple or physical-device evidence.

## Native CI evidence and frozen implementation

Frozen implementation SHA: `21ef0ae7fdddc2945f78d1b653ffcc29d6f44b96`.
All four following runs report this exact `head_sha` and completed/success:

| Gate | Source validation run | Result |
|---|---|---|
| iOS Static Validation | [38558533310](https://github.com/nghianguyen150612/Synveil/actions/runs/38558533310) | PASS |
| iOS Build | [38558533298](https://github.com/nghianguyen150612/Synveil/actions/runs/38558533298) | PASS |
| iOS Simulator Tests | [38558533371](https://github.com/nghianguyen150612/Synveil/actions/runs/38558533371) | PASS |
| iOS Rust Apple Build | [38558533303](https://github.com/nghianguyen150612/Synveil/actions/runs/38558533303) | PASS |

These pull-request runs check GitHub's synthetic merge of the feature into the unchanged verified
P048 baseline. Final feature-head workflow_dispatch runs must additionally verify the documentation
finalization commit itself; their exact IDs/SHA/results are recorded in PR #132 and the final report,
without recursively creating another documentation commit after those runs. The live final-head
[Static](https://github.com/nghianguyen150612/Synveil/actions/workflows/ios-static-validation.yml?query=branch%3Aios%2Fp049-native-notifications-lifecycle),
[Build](https://github.com/nghianguyen150612/Synveil/actions/workflows/ios-build.yml?query=branch%3Aios%2Fp049-native-notifications-lifecycle),
[Simulator](https://github.com/nghianguyen150612/Synveil/actions/workflows/ios-simulator-tests.yml?query=branch%3Aios%2Fp049-native-notifications-lifecycle), and
[Rust Apple](https://github.com/nghianguyen150612/Synveil/actions/workflows/ios-rust-apple-build.yml?query=branch%3Aios%2Fp049-native-notifications-lifecycle)
gate indexes expose their run metadata; qualification requires `head_sha` equal to the current PR head.

Xcode's xcresulttool summary reports **1,900 passed, 0 failed, 2 skipped, 1,902 total** on
Xcode 26.6 / Apple Swift 6.3.3, arm64 iPhone 17 Pro Simulator, iOS 26.5. All **103 P049 cases passed**:
41 coordinator, 18 eligibility, 21 navigation, 19 persistence and 4 SwiftUI cases.
The previously failed committed-upload test passes, as does the new uncertain-COMPLETE regression.
Result bundle: [artifact 11892121654](https://github.com/nghianguyen150612/Synveil/actions/runs/38558533371/artifacts/11892121654),
`ios-simulator-test-results`, retained by the existing workflow for seven days. The workflow's
Verify Test Result Bundle step inspected the actual bundle and emitted the native summary.

The two skips are unchanged inherited tests: `KeychainCredentialStoreTests.testSimulatorKeychainRoundTripUsesUniqueTestService`
(unsigned Simulator process lacks Keychain entitlement) and
`MutationQueueSQLiteTests.testNativeDataProtectionAttributes` (Simulator filesystem lacks Data
Protection attributes). No P049 test is skipped, disabled or quarantined. Source validation and
strict lint pass; expected toolchain/Simulator diagnostics are neither hidden nor suppressed.
The P049 eligibility suite has **zero SQLite unlink warnings** after cleanup ownership correction.
The same full run still reports 80 such diagnostics in inherited fixtures; P048 had 73. Counts
vary with teardown timing. These remain a disclosed inherited test-lifetime limitation, separate
from the corrected upload fixture and not evidence of a production database/schema change.

The failed checkpoint runs [38531563388](https://github.com/nghianguyen150612/Synveil/actions/runs/38531563388)
and [38531558557](https://github.com/nghianguyen150612/Synveil/actions/runs/38531558557) are retained as
failure history, not qualification. Intermediate `a1b3544` native jobs were canceled by the existing
workflow concurrency policy after the cleanup correction advanced the feature; they are not evidence.

## Unrelated workflow failures

The feature diff changes only `clients/ios/**` and this manifest. At recovered checkpoint `dad048c`,
[ordinary Rust CI](https://github.com/nghianguyen150612/Synveil/actions/runs/38531563406) failed in
desktop UnsafeEndpoint tests, object-store Clippy double_must_use checks and the Linux systemd-tmpfiles
`--dry-run` environment check. [AppImage](https://github.com/nghianguyen150612/Synveil/actions/runs/38531564253)
failed its desktop artifact audit for a build-path marker.
[PostgreSQL 17](https://github.com/nghianguyen150612/Synveil/actions/runs/38531563380) failed the inherited
scheduler schema count assertion (36 observed versus 34 expected).
[Linux native packages](https://github.com/nghianguyen150612/Synveil/actions/runs/38531563371) also failed;
its packaging failure was not changed or treated as an iOS gate. These workflows are distinct from
successful iOS Rust Apple compilation. Their code/workflows were left untouched, and their latest
status is reported separately in PR #132/final delivery. No blanket repository-CI success is claimed.

## Quota recovery and focused native defect correction

The clean published branch and PR #132 were recovered at
`dad048c3c37c5b82d9f6f2443897f8f993788afc`; integration remained the verified P048 baseline.
No reset, force push, recreated implementation or duplicate PR was used.

[Native failure 38531563388](https://github.com/nghianguyen150612/Synveil/actions/runs/38531563388)
reported 1,898 passed, one failed and two inherited skips (1,901 total). The sole failure was
`NotificationEligibilityTests.testUploadCompletionHookRequiresExplicitDurableCommit`.
Its reused `UploadCoordinatorProvider` always injected a first-PATCH timeout, including when
configured with zero accepted bytes. Reconciliation correctly confirmed zero offset advancement,
so the real coordinator returned `.stopped(record, .timeout)` with uncertain durable evidence;
COMPLETE was never called. This was an invalid success fixture, not premature production publication.

The provider now accepts an optional first-PATCH fault: nil exercises ordinary validated PATCH,
while all pre-existing integer fault configurations retain their exact behavior. The P049 success
fixture uses nil and still requires `.completed`, persisted completion evidence, one COMPLETE call
and one committed observation. A new ambiguous-COMPLETE case loses its response and authoritative
status read, requiring uncertain state, no durable completion and no completed notification.
READY_TO_COMPLETE and COMMITTED-without-completion are both rejected by the classifier.
Duplicate scheduling, logout fencing and P046 explicit-finalization tests remain enabled.

SQLite warning investigation used both failed P049 logs and the successful P048 baseline
[run 38499912716](https://github.com/nghianguyen150612/Synveil/actions/runs/38499912716).
That baseline already reports 73 `vnode unlinked while in use` diagnostics across inherited
session/logout fixtures. The shared temporary-directory teardown can precede asynchronous
quarantine/cache cleanup retaining SQLite connections. P049's real-logout authority test also
exposed this lifetime issue; it is independent of the upload's deliberate PATCH timeout.
`FileDownloadCoordinator.invalidateSession` now returns its existing cleanup task as a discardable
handle; its actions, fencing and ordering are unchanged. The P049 test awaits that task and drains
queue quarantine through existing `capture` before directory teardown. No SQLite diagnostics are
suppressed, and no schema or transfer/recovery behavior was rewritten. The passing native eligibility suite confirms the P049 teardown warning is absent; inherited
fixture warnings remain disclosed separately.

Focused changes: `FileUploadCoordinatorTests.swift` (shared scripted provider only),
`NotificationEligibilityTests.swift`, and `FileDownloadCoordinator.swift` (cleanup ownership).
Native execution is unavailable locally on Linux; the required full Xcode workflow includes the
focused upload case, all notification suites, P045/P046 regressions and persistence integrity tests.
The supplemental 80-case presentation harness remains distinct from native Xcode evidence.

## Physical-device validation — NOT_AVAILABLE

No real iPhone is attached. These procedures remain required; none is claimed executed:

1. On an iPhone, open Settings → Local Notifications, explicitly enable permission and separately
   enable desired categories. Confirm fresh installation never prompts.
2. Complete a verified download and background the app before the committed outcome. Confirm
   exact bytes/hash and durable VERIFIED state before accepting the notification as presentation.
3. Perform an upload and its existing explicit COMPLETE action. Confirm only COMMITTED evidence
   produces “Synveil upload completed”; background chunk completion alone must not do so.
4. Separately opt into P048 background sync and the sync notification category. Observe a real
   meaningful refresh and confirm durable projection/ACK progress; no-change refresh stays quiet.
5. Deny permission; browse, download, upload and synchronize normally. Reopen Settings and confirm
   truthful denial, no repeat prompt and the system Settings action.
6. Disable each category and the global preference. Confirm unrelated opt-ins remain independent.
7. Tap a delivered notification from the lock screen; unlock/authenticate and check current access.
   Confirm details do not automatically export, preview or share private content.
8. Terminate the process and cold-launch from a notification; test pending session verification and
   retry. Repeated callbacks/taps must not duplicate protected navigation.
9. Log out before opening an old notification. Confirm existing authentication UI or unavailable
   state, no former-account metadata and no old callback scheduling.
10. Enroll another account/Device/credential; old routing records must remain unusable.
11. Force quit and reopen; record actual iOS delivery behavior without guaranteeing timing.
12. Exercise P046 URLSession events alongside a P048 refresh; verify each existing completion
    handler, offset/commit boundary and task budget remains intact.
13. Inspect lock-screen title/body, previews and accessibility copy for private metadata. Inspect
    recorded identifiers/userInfo for only the random local handle.
14. Test Focus, disabled alerts, notification summary/delivery restrictions, Low Power Mode and
    Background App Refresh restrictions. Document delayed/suppressed notifications truthfully.
15. Audit VoiceOver, largest Dynamic Type, small iPhone layout, protected storage while locked,
    account-switch/logout races and real device persistence after interruption.

## Risks, review and integration state

Physical notification timing, hardware storage protection, VoiceOver audit and production-server
end-to-end validation are NOT_AVAILABLE. At-most-once scheduling favors loss over duplicate spam.
Scope and system permission can change after a system call begins; synchronous lifecycle fencing
and late-request removal minimize this unavoidable nontransactional window. Delivered lock-screen
presentation cannot be guaranteed retractable. Conservative record/claim caps can suppress further
notifications; core operations remain available.

PR: [#132](https://github.com/nghianguyen150612/Synveil/pull/132), OPEN. Independent review requested from `ducminh231011`; acceptance is PENDING. Merge SHA: NOT_MERGED.
Final integrated `ios-app` SHA at this snapshot: `0f39b9ab141d46df0bf7df76f5d4886fca044a4a`.
Ordinary merge-commit integration is permitted only after all exact-head gates and explicit review
acceptance. Missing approval/checks leave the PR open. Final evidence will be recorded without
fabricated review, counts, GitHub identifiers or physical qualification.

## Final file inventory and focused audit

The feature changes **27 files** (26 iOS source/test/project/support files and this manifest):

- `clients/ios/App/AppDependencyContainer.swift`
- `clients/ios/App/BackgroundUploadAppDelegate.swift`
- `clients/ios/App/RootView.swift`
- `clients/ios/App/SynveilApp.swift`
- `clients/ios/Application/Notifications/NotificationAuthority.swift`
- `clients/ios/Application/Notifications/NotificationCoordinator.swift`
- `clients/ios/Application/Notifications/NotificationEventClassifier.swift`
- `clients/ios/Application/Notifications/NotificationNavigationCoordinator.swift`
- `clients/ios/Application/Sync/InboundSyncCoordinator.swift`
- `clients/ios/Application/Transfer/FileDownloadCoordinator.swift`
- `clients/ios/Application/Transfer/FileUploadCoordinator.swift`
- `clients/ios/Domain/Notifications/NotificationModels.swift`
- `clients/ios/Features/Library/LibraryCatalogView.swift`
- `clients/ios/Features/Notifications/NotificationSettingsView.swift`
- `clients/ios/Infrastructure/Notifications/LocalNotificationScheduler.swift`
- `clients/ios/Infrastructure/Persistence/NotificationDeliveryLedger.swift`
- `clients/ios/Infrastructure/Persistence/NotificationPolicyStore.swift`
- `clients/ios/Support/tests/test_notification_registration.py`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`
- `clients/ios/Tests/SynveilTests/FileUploadCoordinatorTests.swift`
- `clients/ios/Tests/SynveilTests/NotificationCoordinatorTests.swift`
- `clients/ios/Tests/SynveilTests/NotificationEligibilityTests.swift`
- `clients/ios/Tests/SynveilTests/NotificationNavigationTests.swift`
- `clients/ios/Tests/SynveilTests/NotificationPersistenceTests.swift`
- `clients/ios/Tests/SynveilTests/NotificationTestSupport.swift`
- `clients/ios/Tests/SynveilTests/NotificationViewTests.swift`
- `docs/ios/manifests/PROMPT049_MANIFEST.md`

Focused audit: preferences remain globally/per-category OFF by default; Settings is the sole
permission prompt; current Apple authorization and authenticated scope are checked before claims.
Generic payloads carry only an opaque local handle and no logging/analytics path was added.
Eligibility still requires readback of VERIFIED/COMMITTED/confirmed meaningful progress; taps
revalidate current resource access and cannot execute a transfer, mutation or recovery. Logout
fences scope/generation and navigation synchronously. Claims/caps/expiry favor fail-closed loss
over repeated scheduling. P046 URLSession and P048 BGTask completions never await notification
scheduling/navigation. SQLite v9, its migration/recovery tables and all regression suites remain
intact. Only cleanup-task ownership was exposed; its existing work/order is unchanged.

Review snapshot: PR OPEN, ready for review, reviewer `ducminh231011` still requested; zero submitted
reviews and zero unresolved threads. GitHub reports MERGEABLE, `ios-app` unprotected and no branch
rules returned. Explicit independent acceptance is nevertheless required by P049 and remains absent.
No merge was attempted. The resulting integrated `ios-app` remains
`0f39b9ab141d46df0bf7df76f5d4886fca044a4a`; P050 has not begun. Final remote review/check/integration
state must be rechecked before any ordinary merge commit.
