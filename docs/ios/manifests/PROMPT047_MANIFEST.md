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

Schema remains v8 and uses the existing SQLite actor, transactions, scope quarantine, evidence
capacity accounting and FULL synchronization. No new cache database, Node truth, journal checkpoint,
token, mutation identity, upload identity, download version or background intent encoding is added.
No migration or rewrite of unresolved mutation/ACK/rebaseline/transfer evidence occurs. Positive
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

Linux source validation, all 88 portable support tests and diff checks pass. Strict Swift 6.2.3
formatting and Swift syntax parsing run in the official Linux toolchain. These checks are not Apple
typechecking or XCTest. Native evidence is pending GitHub Actions on the feature head; it will be
recorded below after observed completion.

Twenty-six new XCTest methods supplement all existing suites: eight recovery contract/classifier
tests, fourteen saved inspection/security/SQLite tests, two upload status race/failure tests and two
sync recovery tests. Existing background UI coverage also now asserts that Resume cannot compete
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

## Hardware limits and validation procedure

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

## Delivery status

Feature PR, final head and observed exact-head CI evidence: pending.
Integration/merge: not requested or performed. P048 remains unauthorized.
