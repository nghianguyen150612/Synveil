# P051 — Background Transfer Architecture and Security Gate

## Checkpoint

- Verified starting integration SHA: `e1acabe4bfc5db977cabb77db2c30b864a666810`.
- Fetched `origin/ios-app`; fetched tip was exactly the stated SHA and P050 merge #133 is its
  ancestor. Starting checkout was clean.
- Feature branch: `ios/p051-background-download-architecture`.
- Current source SHA: changes are uncommitted at this checkpoint; the final feature SHA will be
  recorded in the completion report and PR head because a commit cannot contain its own SHA.
- Final feature SHA: pending commit.
- PR: pending.
- Integration merge SHA and verified final `ios-app` SHA: not available; no merge is claimed.
- P052 has not started.

## Existing architecture audit

| Area | Existing implementation and P051 decision |
|---|---|
| P043 immutable downloads | `FileDownloadCoordinator` resolves `/api/v1/versions/{version}/content`, pins the `FileVersionId`, validates metadata and exact identity. Reuse remains authoritative. |
| P044 preview/export | `VerifiedFileExportCoordinator` accepts only existing verified content. No second export path is added. |
| Foreground transport | `URLSessionFileContentTransport` uses ephemeral URLSession, no cookies/cache, HTTPS checks, exact response URL, bounded streaming, identity encoding, and a redirect delegate that rejects redirects. Kept available. |
| Staging and quota | `DownloadStagingStore` and P043 reservations own download bytes and publication. Reuse; no second staging allocator. |
| Persistence | `MutationQueueSQLiteStore` owns P043 downloads and P046 upload intents in schema v9 with transactional identity/state protections. No P051 job can be safely created until the capability gate opens, so schema remains v9. |
| P046 uploads | Existing background URLSession identifier is `com.synveil.ios.upload.background.v1`; existing app delegate and event delivery remain unchanged. Apple says background URLSession tasks always follow redirects, so the upload transport's redirect delegate cannot enforce rejection. P051 does not modify P046; its authenticated redirect behavior remains a separate security review item. |
| P047 authorization | Existing Keychain-backed session, authorization evidence, exact endpoint/owner/Device/credential fencing and quarantine remain authoritative. |
| P048 metadata sync | Existing BGAppRefresh registration and opt-in stay independent. No download work is started by metadata refresh. |
| P049 notifications | Existing verified-download notification eligibility remains unchanged. No background-completion notification is introduced. |
| P050 BrowserSession | Existing browser-only conflict review boundary remains unchanged. |
| Android v0.1 | Current content download uses the authenticated transport and SAF destination; OkHttp redirects are disabled. Its test suite explicitly verifies redirects do not leak bearer credentials. This confirms the existing cross-client security contract; it is not a background transfer implementation. |

## Native background URLSession feasibility

Apple's current documentation states that background session redirects are always followed and
that `urlSession(_:task:willPerformHTTPRedirection:newRequest:completionHandler:)` is not called
for background tasks. Sources: [Downloading files in the background](https://developer.apple.com/documentation/Foundation/downloading-files-in-the-background)
and [URLSession redirect delegate](https://developer.apple.com/documentation/foundation/urlsessiontaskdelegate/urlsession%28_%3Atask%3Awillperformhttpredirection%3Anewrequest%3Acompletionhandler%3A%29).

Synveil attaches its DeviceBearer Authorization header to the authenticated immutable content URL.
The client cannot enforce the final response origin before the OS follows a redirect. Post-transfer
response-origin and SHA-256 validation can reject bytes but cannot undo credential disclosure to a
redirected origin. The inspected server/API contract does not establish a redirect-free guarantee.
TLS validation remains system enforced, but TLS alone does not prove the redirected origin is the
original endpoint. No server API change was authorized.

**Decision: fail closed.** `BackgroundDownloadCapability.current` is explicitly unavailable with
reason `backgroundRedirectsCannotBeRejected`. The SwiftUI background action is disabled and gives
the user the reason; the existing foreground Download action remains usable. No P051 background
URLSession task or OS task identifier is created or adopted. In particular, do not treat
`com.synveil.ios.download.background.v1` as an active session identifier until the server contract
can make the transfer security-equivalent. Do not claim P051 background downloads are implemented.

This is a platform/security blocker, not evidence that background URLSession can never be used.
Reconsider only after a server-side contract or endpoint guarantees same-origin redirect-free
content delivery, with a testable response policy. That would require a newly authorized server/API
change or equivalent documented existing guarantee before credential-bearing task submission.

## State, ownership, storage and integration decision

There is no P051 job state machine or task binding while the security gate is closed. This avoids a
second download authority, duplicate execution state, unbounded resume blobs, false recovery state,
or schema migration without any permitted task to bind. Existing P043 state remains authoritative:
`PREPARED`, `DOWNLOADING`, `PARTIAL`, `VERIFYING`, `VERIFIED` and its existing failure states.
P043 continues exact version metadata, expected length/SHA-256, private staging reservation,
streaming hash, durable verified commit and notification observation after verification.

No P051 task can reach temporary-file handoff, hashing, publication, Quick Look, Share Sheet,
notification eligibility, cancellation, resume or relaunch adoption. Current foreground/background
download exclusion is therefore preserved by not submitting any background download. P046's own
upload ownership and callbacks continue through their existing owner. P048 does not submit content
downloads. SQLite remains v9 and keeps P043 reservations, P046 background upload rows, P047
authorization evidence, P049 notification state and P050 conflict evidence untouched.

## UI and tests

- The native transfer section shows a disabled **Download in Background** action with an explicit
  accessibility hint and visible reason. Existing foreground Download/Resume/Restart controls and
  verified export checks are unchanged.
- Added one deterministic XCTest case verifying the current security reason and that a valid
  authenticated fixture still permits the foreground action without submitting a request.
- Existing small-screen and largest Dynamic Type rendering coverage remains enabled.
- No OS scheduling simulator test is claimed because no OS download task is created.
- Native XCTest inventory and result: pending exact-head Apple CI; this Linux environment does not
  provide Xcode or an iOS Simulator.
- Portable support tests: **105 passed, 0 failed**.
- `python3 clients/ios/Support/validate_ios_sources.py`: **PASS**.
- `bash scripts/validate-docs.sh`: **PASS**.
- `git diff --check`: **PASS** before the final manifest metadata edit; rerun before commit.
- Strict Swift formatter: **NOT RUN**; neither `swift-format` nor `swift format` is installed on
  this Linux runner.
- Swift parsing/native build/XCTest: **NOT RUN**; Swift/Xcode/iOS SDK are absent on this runner.

## Crash/fault recovery disposition under the gate

Every scenario below occurs before P051 OS submission because capability evaluation denies
submission. Durable state: no P051 intent, binding, staged file or notification candidate is
created. Network work: no background request is permitted. Preview: only a pre-existing P043
verified record can be exposed. Recovery: user may use the existing explicitly selected foreground
download under P043, subject to its current immutable version and authorization rules. Notification:
only existing P049 verified evidence is eligible.

This rule covers termination before/during submission, relaunch before delegate initialization,
missing/moved temporary files, staged-but-unverified data, integrity failure, protected-data lock,
pause/completion races, logout/account switch, duplicate callbacks, foreground contention,
concurrent P046/P048 callbacks, low storage, redirects, vanished tasks, post-VERIFIED notification
crash windows, and repeated inventory: P051 has no submitted task or inventory to adopt. No
background success is fabricated. Existing P043/P046/P048/P049 recovery evidence remains owned by
those systems and is not rewritten by P051.

## Validation, evidence and next safe step

- Starting ancestry: verified as above.
- Four native exact-head CI workflows and their URLs: pending PR; no CI result is claimed.
- Physical device evidence: `NOT_AVAILABLE` in this environment. No signed device build, iPhone,
  device model, iOS version or runtime transfer evidence is available.
- User force-quit, task scheduling delay, protected-data behavior and actual iOS redirect request
  header behavior are not inferred from Simulator or source-only checks.
- Remaining blocker: server/API contract does not establish redirect-free authenticated content
  delivery. P051 background downloading must remain unavailable until that security proof exists.
- Review/merge state: pending; no review acceptance or merge is claimed.
- Next safe step: complete local validation, commit and open one PR targeting `ios-app`; keep it
  unmerged unless security review accepts the explicit blocked disposition and required checks pass.

## Final integration record

- Feature branch: `ios/p051-background-download-architecture`.
- Final feature SHA: pending.
- PR and review status: pending.
- Merge SHA: none.
- Verified final `ios-app` SHA: not available.
- P051 status: `BLOCKED_REQUIRES_FIXES`.
