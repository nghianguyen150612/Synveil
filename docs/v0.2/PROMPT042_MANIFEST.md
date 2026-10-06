# Prompt042 manifest — Repair and Recovery UX

Roadmap allocation: final Phase-F prompt. P043 is explicitly deferred.
Expected baseline: `e39088336904f1ca37ca3603ed3a3cb87f4cf374` (PR #70 P041).
Actual starting origin/main: `e39088336904f1ca37ca3603ed3a3cb87f4cf374`.
Repository: `/workspace/Synveil`; origin:
`https://github.com/nghianguyen150612/Synveil.git`.
Initial local branch `work` at `9c54883468117d21dde59c1d8b8ad69eb23d325e`
was clean. Fetch verified P041 on main; no intervening commits.
Created `feat/repair-recovery-ux` from verified origin/main. Its name does not
start with `codex/`; remote name must also be checked before the real PR.

## Source and ownership

The [contract and matrix](REPAIR_RECOVERY_UX.md) records the complete required
ownership audit, support levels, preservation and unsupported behavior.
Added ADR-072 after verifying the number unused on fetched main.

Modified client Cargo/control/launch/lib, client-sync state query/test, desktop presentation/actions/bridge/Main.qml,
roadmap/ADR index/docs validator integration. Added the P042 contract, manifest,
ADR, static validator and focused workflow. No P043 source work, schema changes,
root relocation, resets or installer replacement.

Derived Rust recovery presentation retains ADR-048 canonical categories and
adds finite state/capability. Repair is platform guidance only (Windows/Linux);
unknown platforms are unavailable. Reconnect is the existing P038 form.
Restore missing folder rechecks the original root via SyncNow. Restart is only
supported by the existing running Linux user-service owner with full-stop
proof and authoritative running control status. Windows/direct processes
remain guidance only. Unknown results retain admission/reconciliation before
retry; pending restart phases live only in the existing launch manager.

ROOT UNAVAILABLE != EMPTY TREE; ROOT UNAVAILABLE != DELETE EVERYTHING.
Canonical marker/path/profile/library/redirect validation remains unchanged.
Durable profiles, credentials, identities, bindings, pending work, conflicts,
user pause, startup preferences, first-sync evidence and server/unknown user
files remain preserved. No success is claimed from acknowledgement alone.

## Tests and validation evidence

A required setup-recovery regression inherited from P041 omitted the first-sync
column in prepare_new_library_root. The minimal query fix preserves canonical
identity and the extended existing test checks evidence and descendant retention.
Main Rust CI run 37449876514 confirms the identical baseline Database failure
at state.rs:6897 (291 passed, one failed). This is fixed for required P042
pending-setup recovery, not by weakening the test.

New deterministic tests cover typed same-root recovery, waiting server/root
states, support gating, stale revision/generation/freshness, product copy and
accessibility; restart full-stop parsing, coalescing, readiness, unknown stop,
unknown start, timeout, unsupported invocation and GUI reopen; and unknown
mutation admission fences. Existing root/data and installer suites are retained.

Local validation uses Rust 1.99 and Qt 6.4 in the task-owned Docker container;
the outer non-root workspace lacked Rust/Qt and cannot install system packages.

Observed local results:

| Actual command | Result |
|---|---|
| cargo test -p synveil-client --locked --lib | 93 passed before final restart-reconciliation extensions; final focused P042 run: 11 passed (85 filtered); updated kernel-peer fixture: 1 passed |
| cargo test -p synveil-client-sync --locked --lib | 292 passed after the required root-seeding query fix |
| cargo test -p synveil-install-engine --locked | 503 integration/contract tests passed across all six suites |
| cargo test -p synveil-client --test desktop_control_ipc --locked | 5 passed |
| cargo test -p synveil-client-sync --test inbound --locked | 12 passed |
| cargo test -p synveil-desktop --locked | 91 passed after the final restart callback-order correction |
| cargo check -p synveil-client -p synveil-client-sync -p synveil-install-engine -p synveil-desktop --locked | Passed |
| cargo fmt --all -- --check | Passed on the final source tree |
| python3 scripts/validate-repair-recovery-ux.py | Passed |
| python3 -m py_compile scripts/validate-repair-recovery-ux.py | Passed |
| ./scripts/validate-docs.sh | Passed |
| ./scripts/validate-install-acceptance.sh | Passed |
| git diff --check | Passed |

Strict Clippy reports an inherited double_must_use error at
client-sync/contracts.rs:785. A diagnostic-only rerun allowing that lint also
reports an inherited chunks_exact_to_as_chunks error in the unchanged Windows
UTF-16 helper fixture in client/launch.rs. The original chunks_exact call is
present in the baseline at line 1262. These are not repaired cosmetically or
suppressed in committed source/CI. A further diagnostic invocation permits only
these two baseline lint classes to expose P042 warnings; it is not a strict
quality-gate pass. That diagnostic reaches another unchanged P041
field_reassign_with_default test initializer (baseline presentation.rs:2554).
A final diagnostic allowing only those three inherited lint classes passes
for client/desktop; committed CI still runs the strict commands.
The full desktop script runs 90 passing tests, then fails on existing
object-store double_must_use (types.rs:95 and :359); its later release build,
QML lint and release smoke steps therefore do not run.
Final supplemental debug cargo build succeeds and the actual debug offscreen
--qml-smoke-test exits 0. It reports existing startup-popup accessibility and
sign-out-dialog implicitWidth warnings. Supplemental strict qmllint reports
unresolved QtQml Timer/Connections and existing CXX-Qt integer metadata warnings;
it is not a lint pass. No release smoke pass is claimed.
Native Windows installer and graphical Linux/Windows acceptance, real supervised
restart, live server/root recovery and clean-machine/package acceptance are not
claimed from this source/unit/offscreen evidence.

## Inherited hosted context

Observed PR #70 merged, head `2c650c8bb8febf89ea254eab7b9872a4571bd37f`.
P041 workflow run 37447095584 failed its desktop script at existing object-store
`clippy::double_must_use` diagnostics; its docs/quality job also lacks Qt.
Main Rust CI run 37449876514 includes inherited macOS UnsafeEndpoint tests.
Main Windows installer/native acceptance, AppImage, native packages,
PostgreSQL and first-admin bootstrap workflows also reported failure. These
are baseline failures, not P042 success or P042 regressions; native job details
must be inspected when observing the final P042 head.

## P042-caused hosted failure and fix

Initial focused workflow run 37461335694 was rejected before jobs started:
three unquoted test filters ended in `::`, making invalid YAML. Quoted those
run commands and parsed the full workflow locally (eight jobs, string commands).
Published history is retained; the follow-up commit fixes only workflow syntax
and records the actual failure. The next hosted static/docs job passed the P042
validator but failed the existing docs script because the Ubuntu runner lacked
`rg`; added ripgrep to that job’s dependency installation (no validator bypass).
The local full docs validator remains passing. Final-head hosted results remain
external.

Final source review found a P042 restart callback-order race: confirmed readiness
could arrive before a delayed uncertain callback, leaving a stale status action
that would initiate a new restart. Added a separate manager status-check API
under the same gate (resolved checks are read-only), preserved confirmed UI
readiness over late callbacks, and added deterministic manager/UI-policy tests.
Final focused verification passes 11 client tests and 91 desktop tests. The
static validator also guards the resolved-status and late-callback boundaries.

## Publication evidence

Local commit/tree identifiers are reported externally after commit to avoid a
self-referential commit. Remote commit/tree, equivalence, real PR URL, final-head
hosted results, merge result and resulting main are pending actual publication.
No planned publication or test is recorded as completed evidence.
Phase-F checkpoint: held while required strict source/CI gates remain unsuccessful; publication and hosted handling are recorded externally after the actual events.
Final handoff records external immutable identifiers and post-merge verification.
P043 has not begun.
