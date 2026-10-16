# P051 — Background Transfer Architecture and Security Gate

## PROMPT051B — Secure Background Transfer Recovery

### Recovery checkpoint — 2026-10-16

- Starting P051B feature SHA and existing PR #134 head:
  `d47f719834708dac8d9897b0ec5cc7cce4998f8a`.
- Existing branch: `ios/p051-background-download-architecture`; PR #134 remains
  open against `ios-app`. The local branch was clean and matched the fetched
  branch head after explicitly refreshing its remote-tracking ref.
- Integration SHA is still
  `e1acabe4bfc5db977cabb77db2c30b864a666810`; P050 is an ancestor. No newer
  `ios-app` commit was present. No PR review or security acceptance is recorded.
- P051A containment remains intact. The API process rejects authenticated
  handler redirects; production P046 background upload and P051 background
  download remain disabled; P045 foreground upload and P043 foreground
  download remain available. No URLSession download identifier, transfer-grant
  route, new task, P051 state table, or schema migration is active.
- Current P051B decision: **no positive background-transfer architecture is
  approved for the available deployment model**. Preserve the fail-closed
  gates. The repository has no controlled edge configuration, no identified
  production public origin, and no live deployment or physical-device evidence.

### Architecture comparison and decision

| Option | Security properties | Limitations | Decision |
|---|---|---|---|
| A. Deployment-bound redirect-free transport | A dedicated stable HTTPS origin, direct API routing, API redirect middleware, and a controlled edge configured never to redirect or offload transfer routes can keep the request within the configured Synveil trust boundary. Existing API routes preserve immutable version, range, and upload-offset contracts. | This depends on ongoing control of the TLS edge, DNS, proxy, and CDN configuration. A probe only observes one deployment state. It does not support arbitrary unreviewed reverse proxies or prove future behavior. | Smallest viable option for an operator-controlled, specifically reviewed deployment. It is not enabled here because no such deployment or durable evidence is available. |
| B. Narrow transfer authorization | An opaque grant bound to Device, exact operation, method, version/session, audience, and expiry could reduce the impact of exposing the general DeviceBearer. | iOS may forward the grant on a redirect. Audience validation at Synveil does not prevent disclosure or an attacker relaying it to Synveil. A `307`/`308` can forward plaintext PATCH bytes independently of Authorization. A grant alone is not a safe redirect solution. | Rejected as a standalone fix. No grant endpoint or client protocol is implemented. |
| C. Hybrid deployment and scoped authorization | A controlled dedicated origin plus narrow grants can add defense in depth and bound authorization to one transfer. To protect upload bytes from a redirect destination, the protocol would also need reviewed end-to-end payload confidentiality, integrity, exact operation binding, and replay handling; download content would need corresponding client-bound protection if a redirected destination must be treated as untrusted. | This adds server, client, key lifecycle, replay, revocation, and migration work. It still requires independent review and cannot be inferred safe from API tests or an unauthenticated server capability flag. | Future candidate only. Not approved or implemented. |

**Selected safe behavior:** retain the current deny-by-default capability gate;
do not submit a credential-bearing background request. For a future supported
deployment, Architecture A is the smallest route, but only when its edge and
change-control boundary are explicitly controlled and independently reviewed.
Architecture B cannot make redirect following safe by itself. Architecture C
is not introduced as an unreviewed credential or cryptographic protocol.

The P051A API audit remains valid for the API-process boundary: authenticated
content/upload routes reject the tested handler redirects and do not expose an
object-storage URL. The middleware tests cover synthetic `301`, `302`, `303`,
`307`, and `308` responses and route-specific `304` behavior. They do not test
an external reverse proxy, CDN, DNS/TLS edge, or iOS daemon redirect behavior.
No bearer credential or private file was sent to an adversarial destination.
No assertion is made that iOS strips Authorization on a cross-origin redirect.

### Deployment, authorization, and recovery status

- Deployment evidence freshness cannot be made authoritative using a server
  Boolean or a successful no-follow probe alone. The current probe checklist
  records origin, edge/configuration hash, API build, request methods, statuses,
  and `Location` presence, but no operator deployment was available to run it.
  There is no trusted mechanism in this repository to observe changes to an
  external proxy/DNS/TLS configuration; deployments not under an independently
  reviewed control boundary remain unsupported for background transfer.
- No transfer grant protocol was selected. Raw DeviceBearer or grant values
  are not added to SQLite, task descriptions, URLs, logs, or analytics. Grant
  expiration, revocation, audience, and replay semantics remain design inputs,
  not implemented claims.
- P046 remains unavailable. Historical task authorization withdrawal and
  best-effort cancellation behavior from P051A are retained; unresolved intent,
  task, chunk, and authoritative offset evidence is not erased or retried.
  Foreground recovery remains subject to the existing server-authoritative
  offset flow and must not overlap a live old task.
- P051 remains unavailable. No suspended task, durable P051 binding, staging
  artifact, verified record, or notification candidate is created. P043
  immutable foreground downloads and P044 verified preview/export remain the
  only download path. SQLite remains v9; P047–P050 state is unchanged.
- P048 remains metadata-only and independent. P049 notification eligibility
  remains tied to existing durable VERIFIED evidence. P050 BrowserSession
  conflict-review boundaries are unchanged.
- External deployment tests, native transfer scheduling, signed build, and
  physical iPhone behavior are `NOT_AVAILABLE`. No live or device evidence is
  fabricated. No independent security reviewer has accepted the design.

### P051B validation and next checkpoint

- Source and architecture audit found no approved server/client grant protocol
  or deployment evidence that can open either background capability.
- The safe P051A checks and exact-head CI are inherited evidence only for their
  recorded source SHAs; they are not P051B validation. P051B must record fresh
  portable and exact-head iOS results after the documentation checkpoint is
  committed.
- Existing API redirect tests are the relevant API-level synthetic coverage;
  no new server behavior was introduced in this checkpoint. Live edge attack
  tests remain unavailable and must use throwaway authorization plus synthetic
  bytes if later performed.
- Current status: `BLOCKED_REQUIRES_FIXES`. Keep PR #134 open and unmerged.
  The next safe step is independent security review of an operator-controlled
  deployment profile or a separately reviewed, body-confidential transfer
  protocol, followed by validation against an actual supported deployment.
  Do not start P052.

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
- The PROMPT051A implementation was validated at
  `196d5b89f812d650939a634cc8c9c47a22f97025`; exact-head results and remaining
  blockers are recorded in the final handoff below. Do not treat the old
  P051-only CI URLs as validation of PROMPT051A.

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
  synthetic upload sentinel in the response. On the final implementation SHA,
  `synveil-api` passed **112 tests, 0 failed**, including both redirect
  middleware tests. The wider Rust CI job later failed in an unrelated Linux
  metadata lifecycle test because the runner's `systemd-tmpfiles` rejects
  `--dry-run`.
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
- Portable checks on the final implementation tree: iOS source validator
  passed; **106 support tests passed**; docs validation passed;
  `git diff --check` passed; OpenAPI YAML parsed successfully. Rust formatting
  passed in CI after two focused formatting corrections. Local Rust and Swift/
  Xcode tools are unavailable in this Linux environment; native validation is
  supplied by exact-head CI below.
- Final implementation SHA Simulator XCTest result: **1,924 passed, 2 skipped,
  0 failed, 1,926 total**, iPhone 17 Pro / iOS 26.5 Simulator. This is not a
  physical-device qualification.
- No external proxy test, production endpoint test, signed iPhone build or
  physical-device evidence has been run. Physical device: `NOT_AVAILABLE`.
- Security review acceptance is outstanding. The gate stays OFF pending a
  deployment-bound approach reviewed independently. Keep PR #134 open and
  unmerged.

The exact feature source SHA, working-tree state, portable/API/native test
results, workflow URLs and next safe step are recorded at final handoff below.

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
- Final implementation/source SHA validated: `196d5b89f812d650939a634cc8c9c47a22f97025`.
- The final manifest handoff is a documentation-only follow-up to the validated
  source SHA. The current PR head and that follow-up commit are shown by PR #134.
- PR #134 is open; no review submissions or security acceptance are recorded.
- Merge SHA: none.
- `origin/ios-app` remains at verified baseline
  `e1acabe4bfc5db977cabb77db2c30b864a666810`; no merge is claimed.
- P051 status: `BLOCKED_REQUIRES_FIXES`.

### Final implementation validation on `196d5b89f812d650939a634cc8c9c47a22f97025`

Required iOS GitHub Actions gates all passed on this exact implementation SHA:

- [iOS Static Validation](https://github.com/nghianguyen150612/Synveil/actions/runs/38729897321) — passed.
- [iOS Build](https://github.com/nghianguyen150612/Synveil/actions/runs/38729897312) — passed.
- [iOS Simulator Tests](https://github.com/nghianguyen150612/Synveil/actions/runs/38729897351) — passed; **1,924 passed, 2 skipped, 0 failed, 1,926 total** on iPhone 17 Pro / iOS 26.5 Simulator.
- [iOS Rust Apple Build](https://github.com/nghianguyen150612/Synveil/actions/runs/38729897338) — passed.
- [Rust CI](https://github.com/nghianguyen150612/Synveil/actions/runs/38729897444) — overall failed on unrelated workspace checks. The `synveil-api` test target passed **112 tests, 0 failed**; `cargo fmt --all -- --check` passed. Clippy stopped in unchanged `crates/object-store/src/types.rs` on `clippy::double_must_use`. Other failures: Linux metadata lifecycle test (`systemd-tmpfiles --dry-run` unsupported), Windows workspace build using Unix-only AppImage APIs, and the dependency advisory `RUSTSEC-2026-0349` for locked rustls `0.23.45` (advisory recommends `>=0.23.46`). These files/dependencies are outside the P051A transfer changes.
- [Linux AppImage](https://github.com/nghianguyen150612/Synveil/actions/runs/38729897332) — failed because the existing package audit found a `/home/` path marker in `synveil-desktop`.
- [PostgreSQL 17 scheduled-maintenance](https://github.com/nghianguyen150612/Synveil/actions/runs/38729897318) — failed in the unrelated `postgres_scheduler_tick_schema_is_current_on_postgresql_17` assertion after 24 tests passed.

Portable checks on the same source tree passed: `validate_ios_sources.py`, **106** iOS support tests, `validate-docs.sh`, OpenAPI YAML parsing, and `git diff --check`. Rust formatting passed in the exact-head Rust CI job. API test count and Simulator totals are from workflow output/result bundle, not inferred from source.

Physical device: `NOT_AVAILABLE`. No signed iPhone build or real deployment/proxy test was run. No independent reviewer is recorded; keep PR #134 open and unmerged. Background transfer capabilities remain OFF. The next safe step is to obtain independent security review and deployment-bound evidence or a reviewed transfer-authorization design that addresses both credential and PATCH-body forwarding before enabling either P046 or P051.
