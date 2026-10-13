# Prompt047 — Client-Wide Offline Behavior, Error Recovery & Reliability Hardening

## Verified baseline and scope

- Repository: `https://github.com/nghianguyen150612/Synveil.git`.
- Integration branch: `ios-app`; feature: `ios/p047-offline-error-recovery`.
- Recorded and actual latest fetched starting SHA: `9383db314fcb78fc6679465727cc6a43fcadf8be`.
- The environment contained a separate Beszel checkout, which was left untouched. Synveil was
  cloned into `/workspace/Synveil`, with a clean initial worktree. All remote branch and tag
  references were fetched before implementation. No user changes, reset, main-based checkout,
  or `codex/` branch were used.
- Git ancestry checks passed for the recorded baseline and P046 feature merge
  `dc849942bc20eae35b273e57b807b609079d6aa1`. GitHub independently confirms P046 [PR #128](https://github.com/nghianguyen150612/Synveil/pull/128)
  and evidence [PR #129](https://github.com/nghianguyen150612/Synveil/pull/129) are merged; #129's
  integration is the recorded baseline. GitHub's baseline-to-`ios-app` comparison was identical.
- Inspected the product contract, roadmap, P037–P046 manifests, Rust identifier/FFI boundary,
  authenticated transport and credential/session boundaries, actor-owned SQLite, feature services,
  SwiftUI models, XCTest fixtures and portable registration checks. No applicable AGENTS.md exists.
- P048 is not authorized and is not implemented. No server API, sync protocol, signing,
  distribution, desktop or Android changes are included.

## Pre-implementation capability inventory

| Classification | Existing capability and P047 decision |
|---|---|
| Already implemented and sufficient | P035–P037 immutable mutation identities, canonical bytes, scope binding, cross-connection leases, ordering, durable UNKNOWN evidence and explicit same-ID reconciliation. Retained. |
| Already implemented and sufficient | P038–P040 staged feed evidence, ordered atomic Node application, genuine commit receipts, separate local/server positions, durable ACK attempts, bounded explicit original-token recovery. Retained. |
| Already implemented and sufficient | P041 `OfflineNodeBrowserService` liveFirst/savedOnly/serverOnly policy, exact-scope SQLite reads, active ancestry and identity validation, partial/missing/snapshot presentation. Retained. |
| Already implemented and sufficient | P042 immutable manifests, topology proof, separate server confirmation and atomic local activation, explicit confirmation and unresolved mutation/ACK gates. Retained. |
| Already implemented and sufficient | P043–P044 pinned download identity, digest/length, prefix/range/ETag checks, private staging, verified publication and scoped export leases. Retained. |
| Already implemented and sufficient | P045 upload identities/canonical requests, frozen replacement revisions, source staging, authoritative offset/status reconciliation and final publication evidence. Retained. |
| Already implemented and sufficient | P046 SQLite v8 intents, file-backed tasks, background authorization, callback persistence barrier, task mapping, foreground exclusion, pause/logout fencing. Retained. |
| Implemented but missing recovery coverage | Saved inspection during restoration failure; permission rejection across relaunch; stale status readers; storage failure while loading background ownership. Added deterministic coverage. |
| Implemented but inconsistent across features | Unknown errors became offline in transport/catalog/restoration; generic upload refresh swallowed storage errors; foreground Resume remained offered for an unresolved background chunk; failed ACK recovery could appear as ordinary offline retry. Hardened. |
| Missing and required by P047 | A shared typed recovery presentation contract, locally validated pending-session saved inspection, saved catalog eligibility derived from existing scope/projection records, consistent durable-versus-request recovery notices. Added. |
| Intentionally excluded | Automatic retries/draining/ACK replay/rebaseline, new schedulers or background polling, offline file editing, new uploads/replacements without confirmation, automatic conflict resolution, fresh timestamps without durable evidence, P048 release work. |

## Recovery contract and transport classification

`ClientRecoveryPresentation` supplies typed classification, evidence provenance, explanation and
suggested feature-specific actions. It retains existing richer errors and operation states.
Classifications cover offline/server failure, authorization/revocation/permission, invalid protocol,
partial/stale cache, reconciliation/conflict/rebaseline, storage/integrity, cancellation and session
invalidation. Evidence distinguishes transient requests, durable operations, committed projections,
snapshots and previously loaded data. Suggestions never authorize execution and have no generic
retry dispatcher. Existing coordinators enforce captured session, scope, persisted identity,
ownership, attempt bounds and confirmation on every operation.

Recognized URLError connectivity codes remain transient; DNS, timeout, TLS and cancellation remain
distinct. Unrecognized errors now fail as unverifiable protocol responses rather than offline.
This rule applies to the HTTP transport, catalog/Node readers, restoration verifier and transfer
policies. Neither localized strings nor server diagnostic text determine recovery. HTTP 403 for
content transfers is distinct permissionDenied rather than a fabricated session change.
The bounded HTTP reader explicitly cancels its URLSession task on exit, including body-limit and
cancellation failures. TLS, redirects, private ephemeral sessions, cookies, credentials and HTTP
composition boundaries remain unchanged.

No NWPathMonitor is introduced: the existing authoritative request outcomes already provide the
needed offline status, and no operation requires reachability scheduling. Tests inject actual typed
request outcomes. There is no connection-change handler, polling or persisted availability flag.
Restoration and read-only retries remain explicit. A transient server failure cannot enable a new
mutation, blind PATCH, ACK replay or snapshot handoff.

## Relaunch and saved metadata inspection

Temporary restoration failure previously retained credentials but left all saved metadata behind
the authenticated shell. P047 adds an explicit **Inspect Saved Metadata** action for locally
validated stored sessions with serverUnavailable, dnsFailure, networkFailure or timeout. The
controller remains restorationVerificationPending. The authenticated catalog also exposes the
same explicit local inspection surface when live catalog loading is unavailable. Successful server validation is still required
to enter authenticated and to use the existing network providers. Cancellation, authentication
rejection, revocation, scope mismatch, TLS/protocol and credential/storage failures do not enable
inspection.

`SavedMetadataSessionProvider` exposes only local begin/validate and scope capture. Every read
checks the original pending session, lifecycle revision and endpoint, reloads Keychain, and requires
exact session equality before publication. It implements no request method. The projection repository
now depends on this narrow local-validation protocol; the authenticated production provider still
satisfies it. `OfflineNodeBrowserService` uses a narrow scope-capture protocol, also implemented by
the existing inbound coordinator. There is one projection implementation and one browser service.

The saved catalog queries existing SQLite scope bindings for exact endpoint, owner, Device and
credential identity with quarantine=0. It strictly rereads Library/projection evidence at the current
epoch. It creates no scope, initializes no checkpoint, clears no quarantine and makes no HTTP call.
Directories use the unchanged P041 savedOnly path, canonical/snapshot row validation, existing
directory completeness, ancestry and bounded 500-item policy. Partial, missing, truncated and
rebaseline-required results remain visibly distinct; unknown absence never becomes an empty folder.
Saved inspection offers no mutations, content download/export, upload, ACK or rebaseline controls.

Validated complete live catalogs now record Library observations through the existing
`cached_libraries` persistence model, only for already bound scopes. They cannot hydrate Nodes,
upgrade completeness or manufacture a sync base. A proven catalog omission or quarantined Library
withdraws saved-read eligibility transactionally. A failed or partial live catalog cannot prove
absence. Metadata HTTP 403 quarantines only its exact captured Library scope. Startup authentication
rejection/revocation also invokes the existing durable quarantine owner. Failed quarantine closes
projection authority in-process and records the local failure; original rows and recovery evidence
are retained. Old responses cannot borrow a replacement session to invalidate another account.
Quarantine is never silently cleared as a recovery convenience.

Saved catalog/directory UI clears or fences results on logout, verification retry, credential
replacement and lifecycle changes. Library lists are explicitly incomplete and stale. No last-sync
timestamp is fabricated. The product contract records this inspection exception without relaxing
the authenticated-state gate.

## Durable authorization verification and the crash boundary

Saved inspection adds a direct security dependency: a server rejection followed by termination or
failed local persistence must not be forgotten on the next offline launch. Before the existing
read-only verification GET, production now commits CHECKING with the original endpoint/owner/Device/
credential identity and an opaque local attempt UUID. Verification cannot dispatch without this
marker. Missing, full or untrusted storage produces typed localStorageFailure, retains Keychain and
provides storage/reopen guidance; it does not issue an unrecorded request.

Terminal AUTHORIZED, TRANSIENT, REJECTED and VERIFY_REQUIRED evidence is committed only for the
original current lease after Keychain identity revalidation. A stale lease cannot overwrite a newer
check across connections. A completed ordinary transient failure can preserve prior eligible scope
access. An interrupted CHECKING record, rejected outcome, TLS/protocol condition or lost terminal
commit remains a verification requirement across reopen. A later timeout/offline response cannot
clear that requirement. Only a newly verified authorized response resolves it. Root presentation
withholds saved inspection and explains the unconfirmed authorization evidence.

A complete zero/one-Library observation in the existing limit=1 authorization response also
withdraws eligibility for absent or quarantined Libraries atomically with confirmation. A partial,
inconsistent or unsupported observation cannot prove absence. The full catalog still owns ordinary
Library discovery and positive Library metadata observations; no additional GET is introduced.

The v9 table contains no credential, response body, file path, token, journal position or invented
freshness time. Immutable identity and connection-local authority triggers prevent direct marker
forgery/deletion. Records are bounded to 1,024 identities and charged to the existing evidence budget.
Exact v8 schema validation precedes the transactional addition, with all v1–v8 migration paths,
FULL synchronization, corruption checks, future-version rejection and rollback retained. P046's
background intent schema and callback lifecycle are unchanged.

Library observation persistence also now compares validated, canonically encoded metadata when
revisions match. Older valid JSON key ordering is accepted; a genuine field change at the same
revision, malformed/unsupported metadata, revision regression or changed readback still stops.
No unverified cache row is silently repaired by a live catalog refresh.

## Feature recovery integration

| Feature | P047 behavior and preserved execution authority |
|---|---|
| Browser | Shared partial/stale/recovery notice alongside existing source, completeness and separate positions. Live-first success wins, protocol/auth failures never fall back, saved-only remains local, server-only remains explicit. |
| Mutations | SUBMITTING, UNKNOWN, conflict and blocked-rebaseline notices remain different. UNKNOWN suggests only the original-ID reconciliation. Sending remains bounded, sequential and explicit through P036; payload/base/IDs are unchanged. |
| Inbound sync/ACK | After a failed run/recovery, bounded local status inspection preserves a durable unknown ACK as unknown. It does not become ordinary offline Sync retry. Eight-attempt gates and original-token confirmation remain intact. Temporary feed/server failures have explicit bounded retry. |
| Rebaseline | Unknown START/completion and confirmed-but-uninstalled projections get separate recovery explanations. Existing explicit confirmation, manifest proof, mutation/ACK exclusion and atomic activation remain intact. |
| Download | Connection, permission, integrity and private storage failures get consistent typed notices. Resume/restart still pin the same immutable version; verification and export authority are unchanged. |
| Foreground upload | Resume is withheld until local operation and background ownership reads succeed. Status checks still reconcile original sessions, and completion/revision rules are unchanged. |
| Background upload | Unresolved chunk ownership disables foreground Resume/enable controls. Check Server Status and Pause remain the existing P046 paths. Generic UI recovery cannot submit a competing PATCH. No lifecycle/scheduler changes are made. |

Upload activity loading is injectable for deterministic fault tests. Saved operation and background
job state are assembled together before publication, with lifecycle and generation checks after
every await. Storage errors are exposed and retain the previous blocking evidence; **Reload Saved
Upload Status** is a local-only action. A successful reload clears the unavailable state only after
both record and ownership reads succeed. Status/abort completion rechecks the session after its
saved-state refresh, closing a late publication gap.

## Persistence, integrity and security

Schema advances transactionally from v8 to v9 and uses the existing SQLite actor, transactions, scope quarantine, evidence
capacity accounting and FULL synchronization. No new cache database, Node truth, journal checkpoint,
token, mutation identity, upload identity, download version or background intent encoding is added.
The migration retains every P046 v8 table, intent and guard unchanged; it adds only bounded session authorization-check evidence. No unresolved mutation/ACK/rebaseline/transfer evidence is rewritten. Positive
Library observations use existing versioned validation and transactional writes; negative access
evidence changes only existing quarantine fields.

Network ambiguity remains durable rather than reclassified as safe replay. Request failure does
not alter authoritative upload offsets or server-confirmed positions. Completed bytes do not imply
verified downloads or uploaded publication. Hash, identity, range and revision failures remain
distinct from ordinary outages. Private paths, bearer values, canonical requests, ACK tokens,
completion tokens, SQL and server bodies are absent from recovery UI and accessibility copy.

Native SwiftUI labels, symbols, explanatory text, wrapping layouts, stable recovery identifiers,
Done/reload controls and Dynamic Type are used. State does not rely on color. Current authorization
cannot be inferred while offline; the inspection surface explicitly says verification is pending.

## Validation and evidence

Linux source validation, all 90 portable support tests, documentation validation and diff checks
pass, including the continuation rerun on 2026-10-13. Strict recursive Swift 6.2.3 lint and Swift
syntax parsing pass in the official Linux toolchain using the repository's formatting procedure.
These Linux checks are not Apple typechecking or XCTest. Observed native evidence follows.

Forty-three new XCTest methods supplement all existing suites: eight recovery contract/classifier
tests, fifteen saved inspection/security/SQLite tests, four background/status/migration tests, two
sync recovery tests and fourteen authorization-evidence tests. Existing background UI coverage also now asserts that Resume cannot compete
with an unresolved chunk. No test or gate is disabled. Portable tests check target registration,
local-only inspection authority, absence of generic retry/network schedulers, original-session
fencing and fail-closed classification. The existing quarantine registration assertion is updated
for the expanded lifecycle condition and still verifies invalidation precedes state publication.

Deterministic cases include real file-backed projection reopen with applied-but-unacknowledged
evidence, no network calls during inspection, exact stored identity, four eligible outage classes,
seven forbidden restoration classes, Keychain replacement and logout during reads, verification
retry fencing, startup rejection and metadata denial across reopen, quarantine disk-full failure,
complete catalog removal, no automatic scope/base creation, unknown Foundation/transport failures,
integrity/permission distinction, recovery limits, background ownership read failure and late logout
publication, explicit bounded server retry and unknown ACK dominance. Native hosting fixtures
exercise the new notices and inspection surface at accessibility Dynamic Type.

Final review additionally covers upload-only scopes without sync bases (they cannot poison an
otherwise eligible saved catalog), bounded scope queries, withholding new upload controls when
ownership reads fail, and retaining a failed Pause explanation after successful local status reload.

## Hardware limits and validation procedure

The first implementation head `f0be4ee816c39ac04870b3aea6bace7925ee9967` passed hosted static
validation ([38415122659](https://github.com/nghianguyen150612/Synveil/actions/runs/38415122659))
and Xcode app build ([38415122737](https://github.com/nghianguyen150612/Synveil/actions/runs/38415122737)).
The Simulator gate ([38415121907](https://github.com/nghianguyen150612/Synveil/actions/runs/38415121907))
stopped during test compilation because one new `queueScope` fixture supplied owner before device.
The argument order is corrected; no test is removed or skipped. This intermediate head is not
native qualification.

The next source head `87b110e6d46e0adb9656308e95da6e764536bb7f` passed static, app build and Rust
Apple gates. Simulator [38417074112](https://github.com/nghianguyen150612/Synveil/actions/runs/38417074112)
executed 1,730 tests: 1,727 passed, two existing skips and one failure in repeated Library metadata
observation. The failure exposed noncanonical JSON key order being treated as a revision change;
validated canonical comparison fixes it without weakening revision guards. This is intermediate
evidence, superseded by the final source-head results below.

Physical-device evidence is **NOT_AVAILABLE**. P046's real-device scheduling, force-quit,
first-unlock, OS header-storage and Data Protection limits remain unchanged. No production
qualification, real-server or physical power-loss guarantee is claimed.

Before production qualification, record a signed build SHA, device/OS and controlled server traces:

1. Establish an authenticated scope and partial projection, stop the server and relaunch. Inspect
   saved metadata explicitly; verify no mutation, feed, ACK, upload or rebaseline request occurs.
2. Switch Wi-Fi/cellular, DNS/TLS faults and server 503. Validate live versus saved versus previous
   data labels and unknown/incomplete absence. TLS/protocol and permission errors must not select
   fallback. Verify metadata read denial blocks a subsequent offline relaunch.
3. Interrupt mutation, ACK and snapshot completion at dispatch/response/SQLite boundaries. Record
   original IDs/tokens/manifest and positions; recovery must require the existing explicit actions.
4. Interrupt a pinned download and upload PATCH; verify retained identity, range/digest/length and
   authoritative GET offset. Unverified content must never appear in native export surfaces.
5. Keep a P046 chunk OS-owned while opening foreground recovery, pausing, logging out, and changing
   accounts. Validate no competing PATCH, stale progress publication or automatic COMPLETE.
6. Test low storage, lock/first-unlock, app termination and delayed callbacks. Record actual durable
   evidence and cleanup outcomes; storage faults must stop recovery and preserve unresolved facts.

## Recovered execution state and final source evidence

The quota-recovery continuation found a clean worktree on the existing feature branch at
`8c9d6534707c8b854cad6ee013177fb8b4b4d565`, identical to the fetched remote feature head. No newer
work, untracked implementation, duplicate branch or duplicate feature PR existed. All four
published implementation commits were retained: `f0be4ee`, `87b110e`, `37e0603` and `8c9d653`.
The fetched `origin/ios-app` remains `9383db314fcb78fc6679465727cc6a43fcadf8be` and is an ancestor.

Final implementation source SHA: **`8c9d6534707c8b854cad6ee013177fb8b4b4d565`**.
Its `clients/ios` Git tree is **`6b9f7b543f47821cc2e8b385eb1e7d1e7d6112c8`**.
This continuation changes only this evidence manifest; the later documentation commit retains that
source tree. Its final feature SHA and any subsequent checks are visible in PR #130's commit/check
record without inventing a self-referential commit SHA in this file.

The GitHub run API independently reports `completed/success`, `pull_request` event and the exact
implementation SHA above for every required gate:

| Required gate | Observed result and exact-source run |
|---|---|
| Static Validation | PASS — [38423249673](https://github.com/nghianguyen150612/Synveil/actions/runs/38423249673) |
| iOS Build | PASS — [38423249646](https://github.com/nghianguyen150612/Synveil/actions/runs/38423249646) |
| iOS Simulator Tests | PASS — [38423249810](https://github.com/nghianguyen150612/Synveil/actions/runs/38423249810) |
| iOS Rust Apple Build | PASS — [38423250300](https://github.com/nghianguyen150612/Synveil/actions/runs/38423250300) |

Simulator job `115385836512` printed the actual `xcresulttool get test-results summary`: **1,744
passed, zero failed, two skipped, 1,746 total; zero expected failures**. The uploaded
[result bundle](https://github.com/nghianguyen150612/Synveil/actions/runs/38423249810/artifacts/11828251400)
is artifact `11828251400` (`ios-simulator-test-results`, SHA-256
`bfb64e0617b74b0a8f6ac56596a5b0df6b72369b3a1517610145374a4561bd83`). The runner used Xcode's
iPhone 17 Pro arm64 Simulator on iOS 26.5; this is not physical-device evidence.

Every added XCTest method was enumerated by comparing test method identities against the starting
baseline and matched individually to a passing native `Test Case` log entry: **43 added, 43 passed,
zero missing, zero failed, zero skipped**. The distribution remains eight `ClientRecoveryTests`,
15 `SavedMetadataInspectionTests`, 14 `SessionAuthorizationEvidenceTests`, four additions to
`BackgroundUploadTests` and two additions to `SyncStatusViewModelTests`.

Focused regression suites executed within the same complete native run, rather than a substituted
test subset:

| Suite | Passed / failed / skipped |
|---|---|
| BackgroundUploadTests | 56 / 0 / 0 |
| SavedMetadataInspectionTests | 15 / 0 / 0 |
| SessionAuthorizationEvidenceTests | 14 / 0 / 0 |
| SessionRestorationTests | 43 / 0 / 0 |
| ClientRecoveryTests | 8 / 0 / 0 |
| MutationDrainCoordinatorTests | 66 / 0 / 0 |
| MutationQueueSQLiteTests | 39 / 0 / 1 |
| MutationRecoverySQLiteTests | 24 / 0 / 0 |
| NodeProjectionSQLiteTests | 55 / 0 / 0 |
| InboundSyncCoordinatorTests | 48 / 0 / 0 |
| SyncAckTests | 28 / 0 / 0 |
| SyncStatusViewModelTests | 35 / 0 / 0 |
| RebaselineManifestTests / RecoveryTests / SQLiteTests / TransportTests / ViewModelTests | 10 / 17 / 12 / 11 / 11 passed; zero failed or skipped |

Migration coverage includes preservation of original mutation/checkpoint bytes, retention and
restoration of a v8 unresolved background intent, failed-migration DDL/version rollback, raw SQLite
authority rejection and stale authorization leases across separate connections. All pass.
The two existing native skips are `KeychainCredentialStoreTests.testSimulatorKeychainRoundTripUsesUniqueTestService`
(unsigned test host has no Keychain entitlement) and `MutationQueueSQLiteTests.testNativeDataProtectionAttributes`
(Simulator filesystem exposes no physical Data Protection attributes). Neither is newly skipped by
P047, and the hardware requirements remain open.

The same-source push Simulator [38423240972](https://github.com/nghianguyen150612/Synveil/actions/runs/38423240972)
has a failed workflow conclusion, but its test job was cancelled before checkout or test execution.
Its check annotation states: **"The job was not acquired by Runner of type hosted even after
multiple attempts"**, with macOS arm64 capacity notices and an empty runner name/step list. It is a
runner-acquisition failure, not an unresolved XCTest failure. It does not override the successful
exact-source PR run. The earlier fixture compilation and canonical Library observation defects
are both resolved in that full passing run.

## Final audit and unrelated workflow failures

Continuation review covered local-only saved authority, exact Keychain identity, scope quarantine,
unknown directory absence, strict fallback classification, sticky authorization uncertainty,
transactional migration and ownership triggers, original mutation/ACK/upload identity, explicit
rebaseline, pinned download verification, foreground/background exclusion and lifecycle fencing.
No reproducible P047 defect requiring another source correction was found. Existing regression
suites remain registered and enabled. No automatic mutation, ACK replay, rebaseline, upload CREATE,
PATCH or COMPLETE path was added. This review is implementation audit evidence, not another person's
PR acceptance or production qualification.

Other PR workflows still fail at the implementation SHA. The PR's full change set touches only
`clients/ios` and `docs/ios`; the affected server, desktop, packaging and workflow sources are
identical to the starting baseline. Failures are disclosed separately and have not been weakened:

- [Rust CI 38423250211](https://github.com/nghianguyen150612/Synveil/actions/runs/38423250211):
  object-store `clippy::double_must_use`; Linux `systemd-tmpfiles --dry-run` unsupported by the runner;
  Windows install-engine AppImage code uses Unix-only APIs; macOS desktop control/launch tests
  reject an `UnsafeEndpoint`. Linux/Windows native desktop jobs fail in those unrelated components.
- [PostgreSQL 17 38423250317](https://github.com/nghianguyen150612/Synveil/actions/runs/38423250317):
  `postgres_scheduler_tick_schema_is_current_on_postgresql_17` expects schema 34 but observes 36.
- [Linux AppImage 38423250065](https://github.com/nghianguyen150612/Synveil/actions/runs/38423250065):
  artifact inspection finds a private/temporary `/home/` build path in `synveil-desktop`.
- [Linux DEB/RPM 38423249794](https://github.com/nghianguyen150612/Synveil/actions/runs/38423249794):
  desktop reproducibility comparison finds differing build-A/build-B bytes; package installation
  follow-up jobs are skipped after that failure.

## Delivery and review status

Existing feature PR: [#130](https://github.com/nghianguyen150612/Synveil/pull/130), base `ios-app`;
no duplicate feature PR was opened. GitHub reports it open and mergeable without conflicts.
The base is unprotected, there are no active rulesets or required repository approval/status rules,
and ordinary merge commits are supported. The authenticated repository account has push/admin
permission. Unrelated failing checks explain the aggregate `unstable` merge status.

At final audit there are **zero submitted reviews, zero review threads and zero explicit acceptance
comments**. The continuation instruction explicitly requires review approval or acceptance before
merging. Implementation and evidence are ready for review, but that acceptance gate has not been
satisfied. The PR is being moved from draft to ready for review; it remains unmerged pending recorded
acceptance. No merge commit exists, and the verified integrated `ios-app` SHA remains
`9383db314fcb78fc6679465727cc6a43fcadf8be`.

Completion status: **IMPLEMENTED_PENDING_REVIEW_OR_MERGE**. No P047 implementation/native-validation
blocker was found in the verified evidence. Physical-device evidence remains **NOT_AVAILABLE**;
P047 is not production-qualified, and P048 remains unauthorized.
