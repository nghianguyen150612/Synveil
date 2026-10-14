# P050 — Conflict Review Center, Durable Conflict Visibility & Browser-Only Safety

## Identity and current decision
- Starting `ios-app` SHA: `136bc43944a0060c8422414799d4e7c3d3df0b22`.
- Verified ancestry: `136bc43944a0060c8422414799d4e7c3d3df0b22...ios-app` was **identical** before branching.
- Feature branch: `ios/p050-conflict-review-safety`; target `ios-app`.
- Final feature SHA: **PENDING exact-head validation** (read current PR head).
- Integration SHA: **PENDING**; `ios-app` remains at the starting SHA.
- Decision: **BLOCKED_REQUIRES_FIXES** until full P050 test matrix, exact-head native gates and independent review pass. Do not start P051.

## Audit of reused conflict infrastructure
| Existing capability | Finding |
|---|---|
| Durable SQLite queue (v9), original intent/evidence, session capture and post-read fence | Already implemented; reused unchanged |
| Outstanding-first bounded activity, 100 maximum; reasons decoded from persisted HTTP evidence | Already implemented, insufficiently surfaced in Library/Node UI |
| MutationActivityView (“Pending Changes”) and MutationConflictPresentation | Already implemented; preserved for mutable user actions and reason text |
| CONFLICT / OUTCOME_UNKNOWN / BLOCKED_REBASELINE separate states | Already implemented; projection preserves distinctions |
| Native read-only conflict center and catalog/navigation entry points | Initially missing; added as a narrow presentation layer |
| Exact total count over every outstanding conflict without activity-history decode | Still missing; bounded lower-bound behavior only |
| BrowserSession resolution | Explicitly excluded by P050 iOS scope; no new API |
| P049 local notifications | Unchanged; no new event category |
| Native conflict-specific review and fault-injection matrix | Missing full regression verification |

Reviewed canonical iOS Product Contract, roadmap, ADR-032 and existing service, view model and persistence interfaces. ADR-032's broader AcceptRemote/RetryLocal operations are not authorized for this iOS milestone. Android's v0.1 conflict behavior is described in the product contract; no Android source changes are included.

## Implementation and provenance
Existing `MetadataMutationFeatureProtocol.activity(in:limit:)` reads scoped persisted queue evidence via `DurableMutationQueue.activity`. The latter binds the current credential identity, rehydrates durable records, and checks the request session after the database read. No alternative queue, HTTP API or database was added.

`ConflictReviewSnapshot` separates confirmed `CONFLICT` records from `OUTCOME_UNKNOWN` and `BLOCKED_REBASELINE`. With fewer than 100 returned activity records, the available activity scope is exhaustively inspected. At the maximum page, the count is **incomplete** even if no confirmed conflict appears; no false all-clear is allowed. This is a bounded projection, not an exact SQL aggregation of every conflict. Persisted timestamps are shown as queued timestamps; no unsupported server observation timestamp is fabricated.

`ConflictReviewViewModel` is observation-backed and session-fenced by lifecycle revision, with generation-checked async completion, coalesced reads and immediate invalidation. It calls only `activity`, never enqueue, send, retry, ACK, rebaseline, or conflict-resolution APIs.

The read-only `ConflictReviewView` displays safe operation kind, typed conflict reason, original opaque operation reference, saved enqueue time, separate unknown/rebaseline notices, incomplete-count warning and browser review guidance. It deliberately has no resolution buttons or direct browser URL because no route has been verified. `ConflictReviewEntryView` creates per-Library scoped status links in Library Catalog and Node Browser without overlaying local mutation intent onto canonical Nodes or preventing folder browsing.

## Security and storage
- No new SQLite schema or migration; SQLite v9 remains authoritative.
- No new credential storage, BrowserSession emulation, DeviceBearer resolution request, bearer URL, payload export or signed evidence exposure.
- No automatic conflict resolution, rebase, retry, mutation identity rewriting, ACK replay or change to inbound synchronization.
- No P046/P048 background lifecycle or P049 notification scheduling change.
- Unavailable durable evidence displays unavailable rather than zero.
- Scope invalidation discards observable review state and ignores stale async completions.
- Offline access still requires the existing authenticated persisted-queue credential authority; offline capability is **not independently qualified**.

## Known gaps requiring fixes before acceptance
1. The catalog currently performs lazy, bounded activity projection for each visible Library; an exact and efficient scoped conflict-count aggregation should replace history enumeration for compact banners.
2. Full fault injection (database corruption, invalid evidence, failed commit, process death, re-authorization pending, credential replacement, concurrent reads, notification navigation), accessibility/small-screen and storage regression tests are not yet verified.
3. Review center is read-only, but no deep link to the specific pre-existing Pending Changes record is offered.
4. Browser handoff remains guidance-only pending a verified server browser route.
5. No physical-device validation has been performed. Full native XCTest and exact-head CI evidence are pending.

## Tests and qualification
Source changes add four focused native XCTest methods in `MetadataMutationFeatureTests.swift`: seven-state classification, full-page incomplete count, read-only logout fencing and storage-failure non-zero semantics. These cases are **added but not yet claimed passed**. All preexisting test files remain enabled.

Portable verification:
- `clients/ios/Support/validate_ios_sources.py`: NOT_RUN
- `python3 -m unittest discover -s clients/ios/Support/tests`: NOT_RUN
- `bash scripts/validate-docs.sh`: NOT_RUN
- `git diff --check`: NOT_RUN
- Official Swift formatting / parser: NOT_RUN

Exact-head native P050 gate URLs and results:
- iOS Static Validation: PENDING
- iOS Build: PENDING
- iOS Simulator Tests: PENDING
- iOS Rust Apple Build: PENDING

P050 XCTest pass/fail/skip counts: **NOT_AVAILABLE**. P049's 1,900 pass / 0 fail / 2 skip outcome is historical background and does not qualify P050. P049 also documented unrelated inherited desktop Rust, AppImage, PostgreSQL and Linux packaging CI failures; this P050 branch does not change those components. P050-specific unrelated CI status has not yet been assessed.

Physical-device validation: **NOT_AVAILABLE** for iPhone navigation, VoiceOver, large Dynamic Type, offline review, logout/account switching, browser handoff, background sync, notifications, relaunch and low storage.

PR review: PENDING. No independent approval and no merge authorized. Final integration SHA: NOT_AVAILABLE.

## Acceptance and non-goals
This is an intentionally limited and unverified first implementation. Do not mark P050 complete until the requested A–L XCTest suites, deterministic fault injection, exact-head native validation, security review and ordinary reviewed merge are complete. Do not start P051.
