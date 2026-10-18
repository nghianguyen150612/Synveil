# P050 — Conflict Review Center, Durable Conflict Visibility & Browser-Only Safety

## Repository and identity

- Integration branch: `ios-app`.
- Fetched `origin/ios-app` before implementation; its SHA was
  `136bc43944a0060c8422414799d4e7c3d3df0b22`.
- The P049 merge SHA is an ancestor of the feature branch; the fetched integration
  tip matched that baseline.
- Feature branch: `ios/p050-conflict-review-safety`.
- Final implementation source SHA used by the four required iOS gates:
  `2e12e03fde95b76dc1af43090bb1058a7bb2ab8b`.
- P050 pull request: [#133](https://github.com/nghianguyen150612/Synveil/pull/133), targeting
  `ios-app`. This is the already-existing P050 draft PR; no second PR was created.
- Pre-correction source head: `2f8897b9b931d697c2890775cbb50c2235c999e9`. Its native
  Build run found a missing explicit return in the new reason mapper. That fix was
  committed as `0aab0a35fe519caf57ee3e0f060651cba00265d1`. The final implementation
  source SHA above includes that fix and both later UI/read-only-refresh regression tests.
- This manifest records evidence for the implementation source SHA above. The evidence-only
  commit SHA and any later integrated `ios-app` SHA are reported from GitHub and the final task
  report; this file does not claim a merge.
- P051 was not started.

## Existing conflict capability audit

| Capability | Classification | P050 finding |
|---|---|---|
| Durable SQLite mutation rows, immutable operation intent, state and response evidence | Already implemented correctly | Reused the existing queue, codec, scope identity and transaction behavior. |
| `CONFLICT`, `OUTCOME_UNKNOWN`, `BLOCKED_REBASELINE`, permanent failure and ordinary pending states | Already implemented correctly | Preserved separate durable states; the conflict projection selects only `CONFLICT`. |
| Strict typed conflict decoding and existing conflict reason presentation | Implemented but insufficiently surfaced | Reused the typed decoder/reason enum and added read-only explanations and owner/web guidance. Unknown protocol values fail closed. |
| Pending Changes activity | Implemented but insufficiently surfaced | Extended the existing activity screen. No second mutation history or queue was added. Existing actions remain separate from conflict review. |
| Library Catalog / Node Browser conflict entry points | Missing and necessary | Added bounded local status entry points. They do not attach conflict intent to canonical Nodes or block folder browsing. |
| State-filtered, exact-scope durable conflict read | Missing and necessary | Added one bounded SQLite query over the existing queue and a narrow safe presentation projection. |
| P047 saved metadata authorization and P049 notifications | Already implemented correctly | Reused P047 local eligibility/fencing. P049 notification behavior is unchanged; no conflict notification category was introduced. |
| Android v0.1 comparison | Already implemented; not copied | Android's `MutationEngine` persists typed local conflict results and `CacheDatabase` scopes open conflict rows by profile/device/Library. Android's Node Browser aggregates general pending work (including conflicts); its sync architecture preserves owner/web review and the BrowserSession boundary. No Android file was changed. |
| New resolution API, automatic winner, Retry Local / Accept Remote, Keep Both, merge, conflict copy, mutation rewriting | Explicitly excluded | No such client action, route, or server/Android/desktop change is part of P050. |

Reviewed P035–P049 manifests, `IOS_V0_1_PRODUCT_CONTRACT.md`,
`IOS_PLATFORM_MAPPING.md`, `IOS_V0_1_ROADMAP.md`, ADR-032, Android v0.1 conflict
handling, the mutation queue/service/view model, SQLite v9, inbound sync and ACK
components, saved Node browsing, Library Catalog, Node Browser, SessionController,
and P049 notification/navigation composition. ADR-032's broader resolution
protocol is not authorized by the iOS v0.1 contract for this milestone.

## Conflict projection and count semantics

`DurableMutationQueue.conflicts` uses the existing
`MutationQueueStorageProtocol`/`MutationQueueSQLiteStore`. SQLite validates the
current Library scope's credential identity and quarantine flag, then selects
only `state='CONFLICT'`, ordered by immutable enqueue order, with the existing
maximum read batch of 100. Rehydration strictly validates persisted conflict
evidence and a post-read session fence runs before publication. Storage,
credential, quarantine, malformed-row, or response-evidence failures produce an
unavailable result, never a fabricated zero.

The queue's existing global mutation-row cap is 4,096; the new query returns at
most 100 decoded rows and performs no separate history enumeration or SwiftUI
polling. This keeps even the state-filtered scope walk bounded by existing store
policy without adding a schema index or migration.

The UI projection contains the original operation UUID, safe operation kind,
durable state, queued/attempt times, and typed server reason. It does not contain
the canonical payload, request or response body, resource IDs, bearer material,
signed evidence, staging paths, or raw JSON. Ordering is deterministic.

The SQLite read is bounded. A result with fewer than the requested limit is a
complete count for the verified persisted queue scope. A full page is shown as
“At least N”; no exact total is inferred and a truncated page cannot claim an
all-clear. Unknown outcomes and rebaseline blockers remain distinct and are not
counted or displayed as metadata conflicts.

## UI and navigation

- Library Catalog and Node Browser show “Conflicts need review” only when durable
  conflict evidence is verified. A failed read is shown as unavailable. Status
  buttons open existing Pending Changes/review surfaces and do not submit a
  mutation or start synchronization.
- The Node Browser retains the canonical Node projection and its live/saved
  provenance. It adds no Node-specific conflict badge or speculative local
  metadata overlay; unrelated folder browsing remains available.
- The existing Mutation Activity / Pending Changes view has a read-only conflict
  section with state, operation category, typed reason, timestamps, provenance,
  incomplete-count guidance, and a refresh that reads local durable state.
- The P047 saved metadata view can inspect eligible saved conflicts offline and
  labels them as local saved information. It does not claim current server state.
- All supported reasons have non-destructive explanations: revision mismatch,
  node state changed, parent changed, name occupied, destination changed, and
  resource purged/unavailable. The UI does not claim which side is newer or
  preferred. Unknown reason codes fail closed.
- Accessibility labels/identifiers and multiline layout support are included;
  system text sizing is retained. No color alone communicates conflict state.

## BrowserSession boundary and security review

`docs/en/SYNC.md` and `docs/en/API_ARCHITECTURE.md` document conflict list/detail/
resolution routes as BrowserSession-only. P050 makes no request to those routes
and does not invent a web destination. No trusted browser UI route was verified,
so the app gives instructions to use the existing Synveil web interface and
states that browser sign-in is separate from the iOS device session. It adds no
URL, redirect handler, token, credential bridge, or BrowserSession emulation.

Inspection invokes no mutation POST, ACK, rebaseline, retry, upload, or resolution
API. No DeviceBearer conflict-resolution call is available from the new UI. The
original mutation UUID, base, preconditions, payload and persisted server
evidence are not rewritten. No automatic conflict winner or retry is selected.
P049 notifications are not expanded; no conflict details are added to lock
screens, notification payloads, or notification navigation.

## Offline, session, and permission isolation

Offline review uses P047's `SavedMetadataSessionProvider`, saved Library
eligibility, exact endpoint/owner/Device/Library/credential scope, authorization
evidence for pending restoration, quarantine checks, and pre-/post-read session
validation. It is local-only and does not bind an unbound queue scope, clear
quarantine, infer current server permission, or publish a stale completion.
Logout/session revision changes invalidate observable details; credential
replacement and stale query completion fail closed. Persisted SQLite evidence
does not itself grant server permission.

## Persistence and synchronization decision

SQLite remains schema v9. P050 adds no table, migration, or conflict database;
the existing durable mutation row and server response evidence are sufficient.
No migration path, future-schema behavior, background upload intent,
authorization-check evidence, notification ledger, inbound feed, ACK semantics,
checkpoint, or rebaseline state was changed. Existing outbound queue blocker
policy remains authoritative.

## Tests and fault-injection inventory

Seventeen new XCTest methods were added relative to P049 across
`MetadataMutationFeatureTests`, `SavedMetadataInspectionTests`, and
`ClientMutationTests`. They cover durable-only projection/no write or network,
bounded ordering and lower-bound counts, concurrent reads, queue reopen,
pending-only exact empty result, state classification, unknown outcome
separation, storage failure, corrupted evidence, logout during read, offline
P047 scope/no-network behavior, credential replacement before/during the read,
unknown conflict reason failure, refresh through the read-only facade, and a
SwiftUI host smoke at 320-point width with accessibility Dynamic Type. Existing
P001–P049 suites remain enabled.

| Fault scenario | Durable state | Expected UI | Permitted operation | Forbidden side effect | Evidence / limit |
|---|---|---|---|---|---|
| A conflict is committed, then the queue is reopened | Same original ID remains `CONFLICT` with original intent/evidence | Saved conflict appears after reopen | Bounded local read | Drop/recreate identity, retry, resolve, or report success | `testConflictReviewSurvivesQueueReopen` validates queue reopen; no OS process-kill harness. |
| Conflict evidence commit fails | Transaction leaves `SUBMITTING` with no partial evidence; interrupted recovery becomes `OUTCOME_UNKNOWN` | Do not show conflict; show unknown/unavailable through existing recovery state | Existing same-ID unknown-outcome recovery only | Fabricate `CONFLICT`, regenerate ID, or mark applied | Existing `DurableMutationQueueTests.testFailureDuringConflictPersistenceRollsBackResult`. |
| Logout during conflict read | Original durable row is retained and quarantined by logout | Protected details are cleared; stale completion is discarded | Logout and later reauthorization | Publish former-session detail or delete evidence | `testLogoutDuringConflictReadDiscardsLateProtectedResult`. |
| Credential replacement during SQLite read | Original row remains `CONFLICT`; replacement does not inherit its credential scope | Read fails closed; no former-account result | Revalidate current credential | Publish the stale read or clear quarantine | `testCredentialReplacementDuringConflictStorageReadDiscardsTheResult`, deterministic gate/no sleep. |
| Conflict outcome and ordinary transport uncertainty are observed | 409 stays `CONFLICT`; timeout/offline stays `OUTCOME_UNKNOWN` | Separate conflict review from unknown-outcome recovery | Read conflict; use existing explicit unknown recovery where eligible | Treat offline as conflict or blindly retry 409 | P050 state tests plus existing mutation-drain no-blind-retry and recovery tests; no combined unrelated-operation sequence harness. |
| Same original operation receives a repeated conflict | Original ID/evidence remains terminal `CONFLICT` | Same conflict remains reviewable | Read existing evidence | Duplicate conflict record, replacement ID, retry, or winner selection | Existing `MutationDrainCoordinatorTests.testReplayedConflictRemainsConflict`. |
| Conflict page is truncated | Returned rows remain `CONFLICT`; later rows are unobserved | “At least N”; never claim exact count/all-clear | Bounded next local read on explicit refresh | Infer resolution from absence in a truncated result | P050 `testTruncatedConflictReviewReportsAtLeastCountInStableOrder`; its test-only SQLite fixture seeds a second strictly decodable conflict because production FIFO blocks later submission after an unresolved conflict. |
| SQLite failure or corrupted conflict evidence | Durable evidence is retained; read is not trusted | Unavailable, no count | Show failure and offer local refresh | Convert failure to zero, repair, delete, or network retry | P050 storage-failure and corrupt-evidence tests. |
| Browser review route is unavailable/unverified | Local `CONFLICT` evidence remains unchanged | Guidance-only text; no link is shown | User may open the existing web interface separately | Invent URL, append token, or authenticate browser with DeviceBearer | No route or URL construction exists; verified by API contract review and static source audit, not a dynamic browser test. |
| Resource disappears after conflict was recorded | Conflict row remains independent of Node projection | Conflict remains visible without a Node badge | Read durable conflict summary | Fetch/overlay stale Node or erase conflict because a resource is absent | Projection architecture has no resource association/fetch; no dedicated remove-after-record test. |
| Notification tap occurs after account switch | P049 notification authority is scoped to original account; P050 creates no conflict event | Old notification cannot navigate into new account | Existing P049 scope revalidation | Open stale conflict or mutate on tap | Existing `NotificationNavigationTests.testDifferentAccountAfterRelaunchCannotResolve`; no conflict notification path. |
| Inbound sync runs while outbound conflict blocks queue draining | Conflict remains durable; inbound feed/ACK checkpoint follows existing protocol state | Conflict remains visible; permitted inbound status remains separate | Existing inbound sync when protocol permits | Fabricate ACK/checkpoint or globally disable inbound work | Existing inbound-sync and mutation-drain suites remain enabled; no new combined concurrent integration harness. |
| P047 restoration authorization is pending or revoked | Saved evidence is readable only with existing local authorization proof | Saved local information only when eligible; otherwise unavailable | P047-authorized, exact-scope local read | Treat SQLite possession as current server permission | Existing saved-metadata authorization/quarantine suite plus P050 offline-scope, logout, and credential-replacement tests; no additional conflict-specific revoked-device test. |
| Concurrent conflict summary reads | Queue evidence is unchanged and results have stable ordering | Same bounded snapshot semantics | Concurrent read-only SQLite queue queries | Competing mutation, duplicate subscription, or count inflation | P050 `testConcurrentConflictReviewReadsReturnTheSameStableProjection`. |

The listed uncombined cases are coverage limits, not claims that a new P050
notification, browser API, Node association, or combined sync path was added.

## Portable and native validation

Portable validation was rerun on the corrected source tree that is being committed with this
manifest:

- `python3 clients/ios/Support/validate_ios_sources.py`: passed.
- `python3 -m unittest discover -s clients/ios/Support/tests`: 105 passed.
- `bash scripts/validate-docs.sh`: passed.
- `git diff --check`: passed.
- Swift 6.2.3 `swift format lint --recursive --strict clients/ios`: passed.
- Swift 6.2.3 `swiftc -parse` over all iOS Swift sources: passed.
- New XCTest methods: 17. Native XCTest counts are recorded only from the P050
  simulator workflow; portable Python/Swift parsing is not XCTest evidence.

Four required native workflow runs for implementation source SHA
`2e12e03fde95b76dc1af43090bb1058a7bb2ab8b`:

- [iOS Static Validation — run 38664121765](https://github.com/nghianguyen150612/Synveil/actions/runs/38664121765): passed.
- [iOS Build — run 38664121783](https://github.com/nghianguyen150612/Synveil/actions/runs/38664121783): passed.
- [iOS Simulator Tests — run 38664121825](https://github.com/nghianguyen150612/Synveil/actions/runs/38664121825): passed, 1,917 passed / 0 failed / 2 skipped (1,919 total).
- [iOS Rust Apple Build — run 38664121789](https://github.com/nghianguyen150612/Synveil/actions/runs/38664121789): passed.

All four required gates are green on the same implementation source SHA. Intermediate failures were
corrected and are not substituted for this result:

- The simulator run on `bce3ff0458cbdc0fe75dc02cf285a631efd77e51` executed 1,919 tests: 1,914
  passed, 3 failed, 2 skipped. The three new tests tried later FIFO operations after an earlier
  unresolved conflict and hit the existing `dependencyConflict` gate. Commit
  `c51667bdb578658570d7af44f740b6126aff3e40` isolated durable-state cases and made the pagination
  fixture use a test-only persisted second row.
- The static run on `c51667bdb578658570d7af44f740b6126aff3e40`
  ([run 38662157382](https://github.com/nghianguyen150612/Synveil/actions/runs/38662157382)) failed strict
  formatting on three multiline guards; `c846be0a0fe067d50d793138f0f927dafe51d00b` fixed those.
- The simulator run on `c846be0a0fe067d50d793138f0f927dafe51d00b` executed 1,919 tests: 1,916
  passed, 1 failed, 2 skipped. The test-only fixture attempted a `PENDING` → `CONFLICT` jump,
  rejected by SQLite's state-transition trigger. Commit `2e12e03fde95b76dc1af43090bb1058a7bb2ab8b`
  now seeds it through `PENDING` → `SUBMITTING` → `CONFLICT`; production FIFO policy is unchanged.
- An earlier simulator run on `0aab0a35fe519caf57ee3e0f060651cba00265d1` was canceled when the
  branch advanced and is not qualifying evidence. That source passed Static Validation, Build, and
  Rust Apple Build before the final two regression tests were added.

## Unrelated CI results

The following workflows ran on implementation source SHA
`2e12e03fde95b76dc1af43090bb1058a7bb2ab8b`; they are outside P050's iOS source scope and were not
counted as native iOS passes:

- [Linux AppImage — run 38664121759](https://github.com/nghianguyen150612/Synveil/actions/runs/38664121759): failed its desktop artifact check because it found a private build path marker (`/home/`) in `synveil-desktop`.
- [Rust CI — run 38664121837](https://github.com/nghianguyen150612/Synveil/actions/runs/38664121837): failed in cross-platform workspace tests, Linux/Windows desktop UI jobs, dependency policy, and format/clippy jobs. P050 changed no Rust, server, or desktop source.
- [Linux native packages — run 38664121786](https://github.com/nghianguyen150612/Synveil/actions/runs/38664121786): in progress when this evidence was recorded.
- [PostgreSQL 17 scheduled-maintenance — run 38664121800](https://github.com/nghianguyen150612/Synveil/actions/runs/38664121800): in progress when this evidence was recorded. An earlier run on a prior P050 source failed a schema-count assertion (36 migrations found, 34 expected); the current run has not completed.

No unrelated failure is reported as a P050 pass or hidden behind an overall CI label. No desktop/server
files were changed to address out-of-scope results.

## Physical-device procedure and evidence

Physical iPhone validation is **NOT_AVAILABLE**. Before production qualification,
exercise: conflict review, VoiceOver, large Dynamic Type, offline saved review,
logout while open, account switch, separate browser sign-in/return, banner
dismissal without durable mutation, background sync while a conflict exists,
notification navigation (if a future authorized category exists), force quit and
relaunch, and low-storage behavior. No device or production-readiness success is
claimed. Simulator validation alone is not device qualification.

## Review and integration

- PR #133 is the single P050 PR targeting `ios-app`; it is open and ready for review. The
  independent review request to `ducminh231011` succeeded; the review list is currently empty,
  so acceptance remains pending. The PR is not merged.
- Merge commit and integrated `ios-app` SHA: **NOT_AVAILABLE** until ordinary
  reviewed merge occurs. This manifest does not claim merge or P051 readiness.
