# P051 — Background Transfer Architecture and Security Gate

## PROMPT051A continuation checkpoint

This section is the current P051A status and supersedes conflicting P051-only
statements below, including branch/ref evidence and P046 behavior. Later source
architecture notes are retained as historical context; do not use their older
CI links as P051A or final-head validation.

- Recovery subtask: PROMPT051A, authorized iOS client + directly related
  Rust API/security-contract work.
- Existing branch and PR retained: `ios/p051-background-download-architecture`,
  [PR #134](https://github.com/nghianguyen150612/Synveil/pull/134).
- PROMPT051A starting feature SHA: `65f2d207b169515d4201d89cff5190c4dc417089`.
- P050 integration baseline remains `e1acabe4bfc5db977cabb77db2c30b864a666810`
  and is an ancestor. P052 remains unauthorized and has not started.
- Implementation and validation records below are pending final exact-head
  checks; do not treat the old P051-only CI URLs as validation of PROMPT051A.

### Apple platform limitation, reviewed 2026-10-15

Official Apple Foundation documentation states that background URLSession
redirects are always followed and the redirect-decision callback is not invoked.
Background transfer executes in a process separate from the app. Apple also
requires background uploads to use file-backed upload tasks. The callback
completion handler is delivered after URLSession events finish, and the download
temporary file is only guaranteed through the download delegate callback.
Sources: [Downloading files in the background](https://developer.apple.com/documentation/foundation/downloading-files-in-the-background)
and [URLSession redirect delegate](https://developer.apple.com/documentation/foundation/urlsessiontaskdelegate/urlsession%28_%3Atask%3Awillperformhttpredirection%3Anewrequest%3Acompletionhandler%3A%29).

Consequences:

- P046's existing `willPerformHTTPRedirection` method does not reject
  background redirects.
- A redirect can disclose DeviceBearer credentials if the OS forwards them;
  no header-stripping behavior is assumed.
- HTTP `307`/`308` can preserve PATCH method/body. Uploaded file bytes can be
  disclosed even if Authorization is stripped.
- Post-transfer status, origin, offset or SHA-256 verification cannot undo
  credential/body exposure.
- TLS authenticates a connection peer; it does not prove a redirected host is
  the original authorized Synveil origin.
- The app controls request construction and task binding, not iOS daemon request
  persistence, header retention, scheduling or cancellation completion.

### Threat model and source audit

Assets: DeviceBearer; immutable download bytes; upload chunks; upload-session
and FileVersion identities; account/Device/Library scope; endpoint identity;
durable local task ownership.

Trust boundaries: iOS app; OS background URLSession daemon; DNS/TLS; external
proxy/TLS terminator/CDN; Axum API; content/upload application service; storage.

| Threat | API process | iOS background transfer | External deployment | Disposition |
|---|---|---|---|---|
| API handler emits `301/302/303/307/308` | New middleware replaces it with structured `502 transfer_redirect_prohibited` and no `Location` | Not submitted by current P051 | Not applicable to handler response | Prevented at API process boundary; exact 5 statuses tested |
| Invalid/revoked auth or login redirect | Auth middleware returns JSON 401/403; no browser login redirect route | No submission after the app gate | Proxy may still rewrite before Axum | API behavior audited; edge remains for operator validation |
| Object-store/CDN redirect | Content service streams through API; no storage URL is exposed | Background GET could follow any edge redirect | Proxy/CDN may redirect | No API offload; external behavior unverified |
| Cross-origin GET redirect | API handler does not generate one | OS follows before app can decide; bearer forwarding cannot be ruled out | Proxy/DNS can introduce it | Unresolved at edge |
| `307/308` PATCH redirect | API handler does not generate one | OS redirect callback is not an enforcement point; body can be forwarded | Proxy can preserve method/body | User file exposure remains possible |
| HTTPS downgrade, alternate port/host, same-origin then cross-origin, multi-hop, slash normalization, login page, storage redirect | No such policy in direct API handler | Client cannot intercept before follow | No repository-managed proxy config exists | Must be prevented in each operator deployment; no deployment evidence supplied |
| Header persistence in OS storage | App never persists raw bearer in SQLite | Submitted headers are OS-managed | N/A | Residual platform behavior; no claim of app control |

Direct server audit: `crates/api/src/downloads.rs` serves current or immutable
version content through `ContentReadApplicationService`; it does not construct
external storage URLs. `crates/api/src/uploads.rs` streams PATCH bytes to
`UploadApplicationService`, with exact-offset and content-type validation. The
router has no canonical-path redirect. `crates/api/src/auth.rs` and
`device_auth.rs` return structured auth errors. Upload CREATE's `201 Location`
identifies the created upload resource and is not a redirect. No server handler
or authentication code uses an HTTP redirect response.

The new `reject_content_redirect` middleware is attached to authenticated
content routes and permits only the documented conditional `304` among 3xx
responses. `reject_transfer_redirect` is attached to upload routes and rejects
every 3xx, including `304`. Both return a non-redirect 502 error and drop
`Location`. Unit tests exercise controlled 301, 302, 303, 307, and 308
responses, reject `304` for upload routes, and allow it for content reads. A
portable architecture check asserts the two router attachments. This is
defense against a future handler regression, not a proxy control and not proof
of physical iOS networking behavior.

Deployment audit: `docs/en/DEPLOYMENT.md` describes operator-managed HTTPS
termination and Caddy edge responsibilities. No Caddyfile, Nginx, Traefik,
Compose proxy, or reverse-proxy example is checked into this repository. No
specific operator proxy, CDN, DNS provider, or live HTTPS deployment was
available for testing. The new
[`AUTHENTICATED_TRANSFER_SECURITY.md`](../../en/AUTHENTICATED_TRANSFER_SECURITY.md)
defines the operator checklist and repeatable no-follow GET/PATCH probe using a
test DeviceBearer, immutable test version, disposable upload session and
synthetic one-byte payload. The procedure is not evidence that any deployment
has passed.

### P046 remediation and P051 decision

P046 flaw: the background upload transport contained a redirect-decision
callback that Apple's docs say is not invoked for background sessions. P046
therefore submitted DeviceBearer-authenticated file-backed PATCH tasks without
app-enforced redirect rejection. The upload body disclosure risk is distinct
from credential disclosure.

Remediation in this continuation:

- `BackgroundUploadCapability.current` is typed unavailable because no trusted
  deployment-bound redirect evidence is available to the app.
- P045 foreground upload remains available and keeps its secure transport.
- The P046 UI explains the PATCH-body redirect risk, disables its per-file
  background toggle, and does not present a working background Resume action.
- `FileUploadCoordinator` rejects background opt-in before session creation or
  local upload preparation. `BackgroundUploadCoordinator.schedule` has a
  second capability guard. Native `submit` and `resume` also fail closed.
- On restoration with P046 capability off, unresolved job authorization is
  durably withdrawn before OS cancellation when SQLite commits that change. If
  the write fails, the in-process gate remains closed and cancellation is still
  attempted; the prior unresolved record remains intact for a later recovery
  attempt. Unknown and duplicate task IDs are recorded as bounded orphan
  observations when SQLite is available. Existing operation ID, chunk, task
  mapping, response evidence and authoritative offset remain intact. No automatic
  status GET, PATCH retry, offset advance, new session or COMPLETE is initiated.
  A late callback may be persisted as bounded evidence but cannot start
  authenticated recovery/continuation while the gate is closed.
- Cancellation cannot undo bytes already sent or guarantee an in-flight request
  stopped. Explicit foreground recovery must wait for the task to disappear and
  use P045's original server-authoritative status GET.

P051 background download remains disabled. Although the direct API handler
contract is now explicit and handler-generated redirects are guarded, no
operator-managed deployment is controlled or attested by the app. The iOS
client cannot ensure that an external proxy stays redirect-free for the task's
full lifetime. We did not add a transfer-ticket protocol, server capability
assertion, status-check gate, or speculative credential workaround. No P051
background URLSession, download intent table, or task identifier is activated.
Foreground P043 remains available. SQLite remains v9.

### Tests and validation checkpoint

- New Rust middleware tests cover 301, 302, 303, 307, 308 rewriting and the
  content-only allowed 304 response; upload routes reject 304. The tests also
  assert a structured 502 without `Location`, redirected destination text, or
  synthetic upload sentinel in the response. Existing `device_auth` and
  upload/content API suites remain enabled. They were not executable locally
  because `cargo`/`rustc` are absent; results remain pending CI.
- Six new P046 XCTest cases assert the production upload capability remains
  unavailable; new background opt-in stops before server CREATE; background
  Resume stops before status/authenticated work; legacy task restoration
  withdraws authorization and cancels without offset change/network retry;
  injected SQLite withdrawal failure still cancels while preserving the
  unresolved job; and late completion evidence persists without status
  recovery. The existing unknown-task test also now checks bounded orphan
  evidence.
- Existing P046 fake-transport state-machine tests remain in place with an
  explicitly test-only mechanics capability. Those tests validate durable
  coordinator mechanics, not iOS redirect safety or deployment support.
- Portable checks on the current working tree: iOS source validator passed;
  **106 support tests passed**; docs validation passed; `git diff --check`
  passed; OpenAPI YAML parsed successfully. Rust formatting/API tests could not
  run because `cargo` is not installed. Swift/Xcode/XCTest could not run locally
  because those tools are absent. Exact final-head native CI and Rust API
  results must be recorded after the final source commit.
- No external proxy test, production endpoint test, signed iPhone build or
  physical-device evidence has been run. Physical device: `NOT_AVAILABLE`.
- Security review acceptance is outstanding. The gate stays OFF pending a
  deployment-bound approach reviewed independently. Keep PR #134 open and
  unmerged.

Exact feature HEAD, working-tree state, portable/API/native test results,
workflow URLs and next safe step are updated at final handoff below.

## Original P051-only checkpoint (historical)

The earlier version of this manifest recorded an implementation checkpoint at
`ab65d6422b64f8e9af65002db9d2e0312105f321` and a candidate PR head
`2e75fb6a584031fe1cac57897b83b6803d450e21`. Those values are retained only as
historical notes; at PROMPT051A recovery, local Git and PR #134 both verified
the actual P051 feature head `65f2d207b169515d4201d89cff5190c4dc417089`.
The P051-only workflow results below apply to that pre-remediation SHA and are
not evidence for PROMPT051A changes.

- Verified starting integration SHA: `e1acabe4bfc5db977cabb77db2c30b864a666810`.
- Fetched `origin/ios-app`; fetched tip was exactly the stated SHA and P050 merge #133 is its
  ancestor. Starting checkout was clean.
- Feature branch: `ios/p051-background-download-architecture`.
- Original implementation source checkpoint:
  `ab65d6422b64f8e9af65002db9d2e0312105f321` (historical).
- Original candidate PR head:
  `2e75fb6a584031fe1cac57897b83b6803d450e21` (historical, superseded by `65f2d207...`).
- P051A starting feature SHA: `65f2d207b169515d4201d89cff5190c4dc417089`.
- P051A final feature SHA and post-recovery workflow URLs are recorded in the
  current completion report below; no merge is claimed.
- PR: [#134](https://github.com/nghianguyen150612/Synveil/pull/134), open against `ios-app`, not
  merged.
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
| P046 uploads | Existing background URLSession identifier remains `com.synveil.ios.upload.background.v1`; P051A audits and closes new production scheduling/Resume while preserving durable intent, cancellation and foreground reconciliation. See the PROMPT051A section above and the P046 manifest update. |
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
reason `deploymentRedirectPolicyUnverified`. The SwiftUI background action is disabled and gives
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
download exclusion is therefore preserved by not submitting any background download. P046's durable
upload ownership remains in its existing store; PROMPT051A independently gates new background upload
work and cancels old OS tasks while retaining unresolved intent, evidence and authoritative offsets.
P048 does not submit content downloads. SQLite remains v9 and keeps P043 reservations, P046 background upload rows, P047
authorization evidence, P049 notification state and P050 conflict evidence untouched.

## UI and tests

- The original P051 UI test showed a disabled **Download in Background** action with an explicit
  accessibility hint and visible reason. Existing foreground Download/Resume/Restart controls and
  verified export checks are unchanged.
- Added one deterministic XCTest case verifying the current security reason and that a valid
  authenticated fixture still permits the foreground action without submitting a request.
- Existing small-screen and largest Dynamic Type rendering coverage remains enabled.
- No OS scheduling simulator test is claimed because no OS download task is created.
- Native XCTest inventory: one new fail-closed capability/foreground-independence test in
  `FileDownloadViewTests`; existing test suites remain enabled.
- Original P051-only PR head `65f2d207b169515d4201d89cff5190c4dc417089` iOS Simulator result:
  **1,918 passed, 2 skipped, 0 failed, 1,920 total** on iPhone 17 Pro / iOS 26.5 Simulator.
  The original P051 capability test passed. This is historical Simulator evidence, not
  P051A validation or physical-device qualification.
- Portable support tests: **105 passed, 0 failed**.
- `python3 clients/ios/Support/validate_ios_sources.py`: **PASS**.
- `bash scripts/validate-docs.sh`: **PASS**.
- `git diff --check`: **PASS** after the implementation and manifest edits.
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
background success is fabricated. P051 itself creates no P051 task or P051 recovery evidence.
PROMPT051A may update only the existing P046 background authorization/state needed to stop unsafe
continuation; it retains the original upload operation, chunk, task evidence and server offset.
P043, P047, P048, P049 and P050 recovery evidence is not rewritten.

## Validation, evidence and next safe step

- Starting ancestry: verified as above.
- P051-only exact-head results on `65f2d207b169515d4201d89cff5190c4dc417089` (not P051A):
  [iOS Static Validation](https://github.com/nghianguyen150612/Synveil/actions/runs/38672739341),
  [iOS Build](https://github.com/nghianguyen150612/Synveil/actions/runs/38672739397),
  [iOS Simulator Tests](https://github.com/nghianguyen150612/Synveil/actions/runs/38672739343),
  [iOS Rust Apple Build](https://github.com/nghianguyen150612/Synveil/actions/runs/38672739349).
- These old P051 passes do not validate P051A. Current feature SHA, P051A portable/API test
  results, and P051A exact-head workflow links are recorded in the current handoff section.
- The original P051 PR notes link a Linux AppImage failure that flagged an embedded `/home/`
  marker in `synveil-desktop`; they also record separate non-iOS PostgreSQL scheduled-maintenance
  and broader Rust CI failures. Those were not P051A runs and do not validate P051 iOS code.
- Physical device evidence: `NOT_AVAILABLE` in this environment. No signed device build, iPhone,
  device model, iOS version or runtime transfer evidence is available.
- User force-quit, task scheduling delay, protected-data behavior and actual iOS redirect request
  header behavior are not inferred from Simulator or source-only checks.
- Remaining blocker at the original P051 checkpoint: server/API contract did not establish
  redirect-free authenticated content delivery. PROMPT051A adds an API-process guard and operator
  validation contract, but an arbitrary proxy is still not controlled or attested; P051 background
  downloading remains unavailable.
- Review/merge state: PR #134 is open, no review is recorded, and no merge is claimed.
- Next safe step: inspect the four exact-head iOS workflows and obtain security review. Keep the PR
  open and unmerged while the redirect blocker remains unresolved.

## Final integration record

- Feature branch: `ios/p051-background-download-architecture`.
- Final feature SHA: recorded in the completion report and exact PR head after this evidence update.
- PR #134 is open; review is pending.
- Merge SHA: none.
- Verified final `ios-app` SHA: not available.
- P051 status: `BLOCKED_REQUIRES_FIXES`.
