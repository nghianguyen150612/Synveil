# P028 checkpoint

Status: **BLOCKED; partial implementation**. Branch `feat/v03-p028-windows-native-acceptance`; base `b8beb109e58c847cb136607e4948c564cf6d2aa7`. P027 merge ancestry verified. The initial checkout was clean and had one worktree; no prior P028 PR or newer remote commit was found. No unrelated worktrees were changed.

Implemented: reject a standalone evidence PASS when session classification is not genuine interactive or user/session identity binding is missing or has session ID zero; regression tests cover these cases. The existing harness and hosted blocker are retained. See [qualification](../security/P028_WINDOWS_NATIVE_ACCEPTANCE_QUALIFICATION.md).

Local results: native evidence tests 47 PASS; native acceptance source validator PASS (checkpoint remains blocked); installer source validator PASS; install acceptance contract and 21 tests PASS; `git diff --check` PASS. Cargo and PowerShell are unavailable in this Linux workspace; Rust, Clippy, PowerShell and Windows native execution were not run. Prior hosted run is linked in the qualification document; it is not evidence for this branch HEAD.

Resume: inspect all native harness and lifecycle paths, complete the missing qualification and negative matrix, obtain an explicitly approved disposable interactive Windows 11 environment and trusted fixtures, execute and bind evidence, then record exact final-head hosted results. Do not mark acceptance complete from hosted service-session evidence. Use `git log -1` and `git status` for current HEAD and worktree state; consult the draft PR for CI and push status.

## P028-A continuation

Recovered clean `76df05f0bb1a1ec43e6e5ec36bab57d7ac0a644c`, PR #189 open and draft; no new remote commits. Implemented phase correlation checks for GUI session, logon/logoff LUID and timestamp ordering, with negative tests; hosted consumer binds the source to `GITHUB_SHA` and locally built candidate records its run ID. Evidence test suite: 48 PASS. Native source validator, installer source validator, install acceptance contract and 21 tests, and whitespace check PASS. Rust and PowerShell are unavailable locally. Run 42925276618 at recovered HEAD: candidate build and transfer SUCCESS; standard-user-native FAILURE at intentional unavailable-surfaces checkpoint. Artifact retrieval returned HTTP 403, so digest and size remain unverified. Final-head CI belongs to the subsequent commit and must be inspected on PR #189.

`P028_HARNESS_IMPLEMENTED`: no; complete approved multi-session orchestration, machine/run and replay binding, lifecycle and negative matrix. `P028_NATIVE_ACCEPTED`: no; no approved disposable interactive Windows 11 environment or trusted historical upgrade fixture was available. Next: implement and test the missing phases without weakening the hosted boundary, then run them in an authorized environment. See qualification for audit and independent blockers.

## P028-A2 continuation

Recovered clean `8d019184dd389568455f6ebeb43d05b6dee2ba91` and open draft PR #189. Added read-only phase-ledger auditor, byte verification in summary/validator, adversarial tests and hosted test invocation. This is not an orchestrator: trusted run creation, protected ACL state, actual independent machine identity, phase transitions and logoff-then-logon coordination are not implemented. Ledger integrity cannot prevent a writer with control of both evidence and ledger from forging records; it deliberately returns BLOCKED. `P028_HARNESS_IMPLEMENTED` and `P028_NATIVE_ACCEPTED` remain false.

Local: 50 existing evidence tests, 4 ledger tests (multiple table-driven negative cases), Windows native and installer source validators, install acceptance contract and 21 tests, and whitespace check PASS. Rust and PowerShell unavailable. At recovered HEAD [Windows native #42950006110](https://github.com/nghianguyen150612/Synveil/actions/runs/42950006110) built the candidate but failed at the intended environment checkpoint; [Rust CI #42950006113](https://github.com/nghianguyen150612/Synveil/actions/runs/42950006113), Windows installer #42950006252 and P027 #42950006281 succeeded. These do not validate the new head.

Next: implement an independently protected Windows run coordinator and ACL/role verification; obtain real observer evidence of the initial logoff and subsequent logon; wire GUI/IPC, repair, uninstall/reinstall and cleanup with bounded phases; add authenticated run/machine provenance and replay-resistant finalization; validate on an explicitly approved disposable VM and record exact-head hosted CI. Candidate digest remains unverified, and historical artifacts and independent release/security blockers listed in the qualification remain open.

## P028-A3 continuation

Recovered clean `73713fee5ea8ed4886fd083ab6f12a055c62ac6a`; no newer remote work, PR #189 open and draft. Implemented executable fixture-only transactional coordinator core with unique run, ordered phase advancement, bounded evidence, durable SQLite commits, concurrent transition serialization, evidence corruption checks, independent fixture cleanup and terminal state; no native OS operations are enabled. The existing Windows workflow now runs its fixture tests. Seven coordinator tests, 50 evidence tests, 4 ledger tests, Windows native and installer source validators, install acceptance contract and 21 tests, and whitespace check passed locally. Rust and PowerShell were unavailable. These tests do not qualify Windows ACLs or native acceptance.

`P028_COORDINATOR_IMPLEMENTED=false`: Windows ACL and observer authority, secure run identity, real multi-session orchestration, lifecycle integration, independent replay-resistant finalization and Windows qualification remain missing. `P028_HARNESS_IMPLEMENTED=false`; `P028_NATIVE_ACCEPTED=false`. The core is fixture-only and can be modified by anyone with filesystem access; no native trust should be inferred. Next: build and verify a Windows protected controller and subject/observer boundary, wire the existing collectors with actual logoff-before-new-logon semantics, implement lifecycle and cleanup, then execute on an explicitly approved isolated Windows 11 VM. Existing release blockers and missing historical fixtures remain open. Exact final-head CI is recorded in PR #189.

## P028-A4 continuation

Recovered `d4ccf26ff578f84923627c48249d9fa6d12fc14a`, same clean branch/PR. Repaired coordinator connections with explicit `database()` close/rollback behavior and PRAGMA failure close. Fixed the corruption regression itself: explicit commit then close instead of SQLite's transaction-only context manager. Added Windows deletion-after-operation/error, PRAGMA-failure close, reopen-after-child-exit coverage. Local coordinator suite: 11 PASS; evidence 50 PASS; phase ledger 4 PASS; native/installer source validators and install acceptance contract plus 21 tests PASS; whitespace PASS.

Windows run #42983808328 originally failed with handle leak in coordinator. Run #42992673483 showed a second leak in test-owned `sqlite3.Connection` context manager; suite reached `Ran 11 tests` but one cleanup ERROR. On exact fix head `5554524e52aa0398c5c359e4fea420f6a7f5c4ef`, Windows native run #42993049322 passed `Validate checkpoint wiring` (coordinator/evidence/ledger/source/install contracts) and proceeded to `Build runtime once`; candidate build completion was still pending when recorded. This is coordinator source-test qualification, not native GUI/logon acceptance.

Added `scripts/protect-windows-native-run-state.ps1` as source-level Windows ACL/state groundwork; workflow parses it but does not execute its mutating Initialize/Advance paths. It requires elevated admin, preprotected admin-owned root, disposable local non-admin identity and explicit approval; sets protected run/trusted/observer/evidence ACLs, subject-only inbox write ACL, reparse-safe evidence handle reads, OS/candidate identity checks, bounded atomic phase state and mutex. It is not yet qualified on a Windows disposable VM. `P028_COORDINATOR_IMPLEMENTED=false`, `P028_HARNESS_IMPLEMENTED=false`, `P028_NATIVE_ACCEPTED=false`. Still missing independent observer event proof, protected ACL integration tests, proof formats matching existing collectors, actual logoff-then-new-logon orchestration, calling GUI/IPC/lifecycle scripts, repair/reinstall and independently verified cleanup. Continue only after exact final-head Windows parser/test CI validates the new script; keep hosted interactive gate BLOCKED.

### Exact-head hosted status: `27d1d80860e42779cf04aa268d8c8f8cd340b977`

Windows native run [#42995046774](https://github.com/nghianguyen150612/Synveil/actions/runs/42995046774): `Validate checkpoint wiring` PASS; `Parse PowerShell acceptance tools without running them` PASS, including `protect-windows-native-run-state.ps1`; `Build runtime once` IN_PROGRESS at last check. Candidate reproducibility and upload had not run. This confirms the fixed SQLite coordinator tests are green on Windows and the candidate job advances beyond them, not that its build has completed.

Windows installer [#42995046801](https://github.com/nghianguyen150612/Synveil/actions/runs/42995046801): installer-smoke PENDING. Rust CI [#42995046809](https://github.com/nghianguyen150612/Synveil/actions/runs/42995046809): Windows target and Quality jobs PASS; other matrix checks/jobs PENDING. P027 [#42995056422](https://github.com/nghianguyen150612/Synveil/actions/runs/42995056422): Linux and Windows PostgreSQL live transaction suites PASS; Windows compilation/contracts PENDING. Installer security [#42995056281](https://github.com/nghianguyen150612/Synveil/actions/runs/42995056281): security structural/docs and acquisition/channel suites PASS; other platform suites pending. Do not summarize these partial workflows as all-green.

### P028-A6 recovery checkpoint

Starting HEAD `452e1de07c20c7d61dcd0be0849dc6748e9c086a` contains the `rustix` 1.1.6 RUSTSEC-2026-0486 lockfile repair. On inspection, [Rust CI #43140141790](https://github.com/nghianguyen150612/Synveil/actions/runs/43140141790) was still queued/in progress, including dependency policy; this is not a hosted pass. Earlier local `cargo deny check` and install-engine tests passed.

The protected-state script now verifies both observer-only directories using the same no-subject policy as initialization, groups its inheritance bitmask comparison, rejects inherited ACEs, accepts a full local account SID including RID, and compares the stored subject and controller identities on every subsequent operation. A safe hosted Windows job exercises in-memory ACL construction and negative classifications; it does not exercise effective access with a subject token. Source regression checks cover policy wiring and identity checks. Check the final-head Actions results before reporting this job as passing.

**Resume:** review path-component substitution and handle-based state/evidence access, effective access and nested group membership, interrupted writes and replay detection before treating state as trusted. Implement an independently authenticated native coordinator and integrate the actual collectors and lifecycle harnesses; the existing fixture coordinator remains fixture-only. No approved disposable Windows 11 interactive environment has been supplied. `P028_PROTECTED_STATE_IMPLEMENTED=false`, `P028_COORDINATOR_IMPLEMENTED=false`, `P028_HARNESS_IMPLEMENTED=false`, `P028_NATIVE_ACCEPTED=false`. Preserve all prior release/security gates and the hosted service-session blocker.

`Advance` is deliberately blocked with `P028_NATIVE_AUTHORITY_UNAVAILABLE`: JSON and directory location do not authenticate an event collector. Remove this gate only when independent provenance validation and the protected-state boundary are qualified. This checkpoint does not implement a native adapter.

## P028-A7: deny-only protected-state read hardening

Recovered GitHub branch `feat/v03-p028-windows-native-acceptance` at `e05e5486a0a91067a58174c23c45e39a98864672` with PR #189 still open/draft, targeting `develop/v0.3`. The Chat On Steroids workspace tunnel was unavailable; this scoped checkpoint was authored through the GitHub connector rather than claiming local Windows or worktree execution.

The protected-state controller now uses its existing bounded, handle-backed no-reparse reader for the persisted run-state, saved GUI/new-logon phase evidence, and existing destination evidence during replay detection. It also rejects any recorded phase, finalization, or terminal/mismatched mode during `Verify` while native advancement is deliberately disabled; a purely local state edit cannot make the pending-native checkpoint look verified. Source-level regressions pin these invariants. No new observer trust anchor or independent event signature is implemented; `P028_NATIVE_AUTHORITY_UNAVAILABLE` remains in place.

`P028_PROTECTED_STATE_IMPLEMENTED=false`, `P028_TRUSTED_OBSERVER_IMPLEMENTED=false`, `P028_COORDINATOR_IMPLEMENTED=false`, `P028_HARNESS_IMPLEMENTED=false`, `P028_NATIVE_ACCEPTED=false`. Effective-token ACL qualification, authenticated observer provenance, lifecycle orchestration, approved Windows 11 interactive evidence, P027 SQLx review and P036 signing remain outstanding. Exact-head CI must be checked after push; do not infer a pass from the prior head.

## P028-A8: trusted Windows observer foundation

Starting HEAD: `975540ffa7c6dddbbe468caeda09c7df8f376e96`; branch `feat/v03-p028-windows-native-acceptance`; PR #189 remained open, Draft, and unmerged; base `develop/v0.3` at `b8beb109e58c847cb136607e4948c564cf6d2aa7`. Remote branch had no newer commits.

Extended `scripts/protect-windows-native-run-state.ps1` with privileged observer actions for initial interactive session, logoff, and subsequent new logon. These query local Security event records 4624/4634 directly, inspect the actual elevated admin token, reject subject process tokens that are elevated or contain the Administrators group SID, correlate the subject's live process token Authentication LUID with WTS session identity, and bind event XML provider/channel/record ID/timestamps/computer, run, machine, boot, subject, controller, candidate, and harness identities. The boot identity prevents comparing Logon IDs across a restart, since Windows documents those IDs are only unique between reboots. The actions write bounded `OBSERVED` evidence through a flushed temporary file and atomic rename into the existing admin-only observer inbox. They never report phase success. `VerifyObserverSequence` checks order, session/LUID distinction, record uniqueness, context and predecessor digests.

`ObserveLogonTrigger` remains explicitly BLOCKED. Existing Task Scheduler 107/100/110 assumptions do not prove one task instance was triggered by a genuine logon and ran in the new Authentication LUID. The native `Advance` gate remains `P028_NATIVE_AUTHORITY_UNAVAILABLE`; the observer artifacts are not fed into native phase success. `scripts/windows_native_provenance.py` and `scripts/test_windows_native_provenance.py` define and adversarially test only a synthetic correlation model, whose result is always `FIXTURE_ONLY` and unauthenticated.

This remains source implementation, not a qualified authority: no Windows PowerShell parser or native Windows execution was available locally; no disposable Windows 11 token, event, ACL, logoff/new-logon or cleanup qualification was performed. The existing ACL contract job is an in-memory descriptor test, not effective NTFS access. The path-component reparse scan remains susceptible to privileged replacement races. Observer and controller currently use the same elevated identity, and a malicious administrator or kernel can change state and event evidence. These conditions, missing Task Scheduler session binding, GUI/IPC/lifecycle wiring, durable interruption recovery and cleanup keep native acceptance disabled.

At starting SHA `975540ffa7c6dddbbe468caeda09c7df8f376e96`, [Windows native run #43351781477](https://github.com/nghianguyen150612/Synveil/actions/runs/43351781477) had `protected-acl-contract` PASS (in-memory contract), `build-windows-candidate` PASS (source checks, PowerShell parse, candidate reproducibility), and `standard-user-native` pass the scoped standard-user product acceptance before the intentionally failing unavailable-surfaces step. It is not native acceptance. Local focused tests: provenance fixture 10, protected-state contract 10, coordinator 11, evidence 50, ledger 4; installer contract and 21 tests; native acceptance validator and whitespace check. PowerShell 7.7 parsed all 8 workflow acceptance scripts and compiled the observer C# declaration without invoking Windows APIs.

Exact A8 source head `20399e9cc61039bfa00ed29033e2c5cdb73dcdac` was tested by [Windows native run #44007390224](https://github.com/nghianguyen150612/Synveil/actions/runs/44007390224): in-memory ACL contract PASS; `build-windows-candidate` PASS, including the new provenance tests, PowerShell parser, runtime build and reproducible candidate checks; standard-user candidate authentication and product acceptance PASS. The workflow then failed only at the explicit unavailable-surfaces gate for the hosted service-session GUI/logon/IPC/observer limitations; evidence upload passed. This is an expected native-environment blocker, not native acceptance. The A8 observer still does not authorize native coordinator transitions.

| Gate | State | Basis |
| --- | --- | --- |
| `P028_ACL_CONTRACT_PASS` | true, scoped | In-memory ACL contract job passed; effective access remains unqualified. |
| `P028_PROTECTED_STATE_IMPLEMENTED` | false | Windows ACL effective access, parent-path substitution and real token-separation checks are incomplete. |
| `P028_TRUSTED_OBSERVER_IMPLEMENTED` | false | 4624/4634 observer foundation exists; LogonTrigger/session provenance is still blocked. |
| `P028_COORDINATOR_IMPLEMENTED` | false | Native phase advancement is still blocked; observer records are observations, not accepted phase records. |
| `P028_HARNESS_IMPLEMENTED` | false | Complete GUI/logon/IPC/lifecycle/cleanup and recovery orchestration is missing. |
| `P028_NATIVE_ACCEPTED` | false | No approved native interactive Windows qualification was executed. |

The starting checkpoint's [P027 PostgreSQL run #43351781449](https://github.com/nghianguyen150612/Synveil/actions/runs/43351781449) passed PostgreSQL transaction suites and failed strict Clippy/format validation at `crates/server-storage/src/wizard.rs:1098` (`clippy::needless_bool`). This unrelated issue was inspected and left untouched. Independent P027 SQLx security review and P036 signing blockers remain visible.

### A8 continuation from recovered PR head `035dae99df7831a32e6205f6dc3ae67f3c297910`

Recovery found `/workspace/Synveil` clean on local branch `work` at `604499fce0dfd134d82da3d5bb5d4a055c5eea8d`; it was not the requested P028 checkout. The live PR #189 branch had advanced beyond the supplied `975540f` to `035dae9`. The PR head was fetched and checked out on the requested branch name; the pre-existing `work` branch and files were preserved. PR #189 remains open, Draft, unmerged, and based on `develop/v0.3`.

This continuation closes a source-level gap in `VerifyObserverSequence`: saved observer JSON and its chained digest are no longer the only event reference checked. The verifier re-queries each exact Security `EventRecordID` from Windows with `Get-WinEvent`, validates the 4624/4634 provider, channel and schema again, and compares the OS event XML digest, timestamp (accounting for the record's second-resolution serialized time), subject SID, normalized Authentication LUID, and interactive logon type. A missing, changed, duplicate, unsupported, or ambiguous record fails closed. This recheck is still not a cryptographic signature or protection against a malicious administrator/kernel. `ObserveLogonTrigger` remains BLOCKED and `Advance` still throws `P028_NATIVE_AUTHORITY_UNAVAILABLE` before reading subject evidence.

Local checks on this continuation: `test_windows_native_provenance.py` 10 PASS (synthetic fixtures only); `test_windows_protected_state_contract.py` 12 PASS (source wiring only); coordinator 11 PASS; evidence 50 PASS; phase ledger 4 PASS; `validate-windows-native-acceptance.py` PASS with checkpoint blocked; `git diff --check` PASS. This environment has no PowerShell executable, so PowerShell parse and Windows API execution were not run locally. These results do not qualify actual Windows ACLs, tokens, event records, logoff/logon, or effective isolation.

At head `035dae99df7831a32e6205f6dc3ae67f3c297910`, Windows native run [#44010167037](https://github.com/nghianguyen150612/Synveil/actions/runs/44010167037) had `protected-acl-contract` PASS (in-memory descriptor only), `Validate checkpoint wiring` PASS, and PowerShell parser PASS; the candidate runtime build was still in progress when inspected. The earlier same-head run [#44010159460](https://github.com/nghianguyen150612/Synveil/actions/runs/44010159460) had its candidate validation step in progress and in-memory ACL job PASS. These are exact-head source/contract checks, not completed candidate or native acceptance results. The next code commit requires its own exact-head CI inspection.

The separate P027 run [#44007397382](https://github.com/nghianguyen150612/Synveil/actions/runs/44007397382) tested `20399e9cc61039bfa00ed29033e2c5cdb73dcdac`: PostgreSQL transaction/recovery suites passed on Windows and Linux, but Linux strict Clippy failed at `crates/server-storage/src/wizard.rs:1098` for `clippy::needless_bool`. This is unrelated to P028 and was not changed. The later P027 run at `035dae9` was pending at inspection. Independent P027 SQLx security review and P036 signing blockers remain open.

| Gate | State after this continuation | Evidence and remaining conditions |
| --- | --- | --- |
| `P028_ACL_CONTRACT_PASS` | true, scoped | Hosted in-memory ACL contract passes; it does not prove effective NTFS isolation. |
| `P028_PROTECTED_STATE_IMPLEMENTED` | false | Effective access, parent-component substitution races, interrupted Windows writes, and separate real controller/observer token qualification remain. |
| `P028_TRUSTED_OBSERVER_IMPLEMENTED` | false | Direct 4624/4634 observation and exact-record re-query exist; LogonTrigger provenance, independently separated controller/observer authority, and native Windows qualification remain. |
| `P028_COORDINATOR_IMPLEMENTED` | false | Native advancement remains blocked by `P028_NATIVE_AUTHORITY_UNAVAILABLE`; fixture coordinator is not a native coordinator. |
| `P028_HARNESS_IMPLEMENTED` | false | GUI/IPC, LogonTrigger, lifecycle, cleanup, interruption recovery, and finalization are not orchestrated. |
| `P028_NATIVE_ACCEPTED` | false | No approved disposable Windows 11 AMD64 interactive qualification was run. |

Next scope: qualify the observer and protected directory with real ordinary-user and elevated tokens on an explicitly approved disposable Windows 11 AMD64 VM; implement or prove an independently attributable LogonTrigger-to-session binding; then wire authenticated observations to resumable native phase transitions and the lifecycle/cleanup harness. Keep the phase guard closed until those conditions and the relevant native qualifications pass.

### Exact-head validation after the re-query change: `2fc456c0eba02dfac39dcea3264cc48963cab003`

On this resumed workspace, portable PowerShell 7.7 parsed all eight Windows acceptance scripts. Focused local tests passed: provenance 10, protected-state source contract 13, fixture coordinator 11, native evidence 50, phase ledger 4; native acceptance validator passed with advancement blocked; installer validator passed; install acceptance contract plus 21 tests passed; whitespace check passed. No Windows API was invoked.

The exact-head [Windows native run #44011579475](https://github.com/nghianguyen150612/Synveil/actions/runs/44011579475) has `protected-acl-contract` PASS (in-memory only) and `build-windows-candidate` PASS, including the re-query source tests, PowerShell parse, Windows runtime build, exact-candidate double build, identity verification and evidence upload. `standard-user-native` authenticated the candidate and passed standard-user product acceptance, then failed at `Record required unavailable surfaces honestly`; evidence upload passed. The overall workflow is FAILURE because the GitHub service session cannot supply qualified interactive GUI, genuine logoff/new-logon, IPC and observer evidence. No native acceptance is claimed.

The exact-head [P027 PostgreSQL workflow #44011585872](https://github.com/nghianguyen150612/Synveil/actions/runs/44011585872) passed Windows PostgreSQL transactions/recovery and Windows compilation/contracts; Linux PostgreSQL transactions/recovery also passed, but Linux strict Clippy/format validation failed in `crates/server-storage/src/wizard.rs:1098` with `clippy::needless_bool`. This is unrelated to P028 and was not changed. Independent P027 SQLx security review and P036 signing remain open.

## P028-A9 checkpoint

Recovered the open Draft PR #189 and remote branch at `17ef16fbe6188f36cedaa1df9bc17dbe71fe9b58`. Worked in `/workspace/Synveil-p028`, preserving the original clean `work` branch at `604499fce0dfd134d82da3d5bb5d4a055c5eea8d`.

The controller records a protected, pre-logoff task definition and Operational log anchor. The separate observer can corroborate a 119 LogonTrigger instance with 100/129/200 records, the live client image and ordinary-user token, 4688 target SID/LUID, and the exact 4624/4634 sequence. Scope and account requirements are in the [qualification](../security/P028_WINDOWS_NATIVE_ACCEPTANCE_QUALIFICATION.md). Unsupported, contradictory or inaccessible sources block. The scheduler does not expose the triggering authentication LUID, and installed file bytes do not attest the mapped image. No diagnostic advances native state. The legacy collectors leave task provenance blocked.

Portable checks passed on Linux with PowerShell 7.7.0: 61 synthetic PowerShell cases, 12 Python provenance, 15 protected-state source, 12 fixture coordinator, 50 evidence, 4 ledger and 21 install contract tests. The PowerShell scripts parsed and C# declarations compiled. Windows installer, native acceptance and installer security source validators passed. Native Windows execution and NTFS effective access were unavailable locally. The hosted workflow adds read-only current-token, pinned read and provider metadata smoke checks without starting a task or relaxing the required interactive surface check.

A8 exact-head [Windows native #44014905082](https://github.com/nghianguyen150612/Synveil/actions/runs/44014905082) and [#44014915391](https://github.com/nghianguyen150612/Synveil/actions/runs/44014915391) passed in-memory ACL and candidate jobs. Both failed the intentional hosted interactive surface step. The [Windows installer](https://github.com/nghianguyen150612/Synveil/actions/runs/44014915503) and [installer security](https://github.com/nghianguyen150612/Synveil/actions/runs/44014915461) workflows passed. [P027 #44014915132](https://github.com/nghianguyen150612/Synveil/actions/runs/44014915132) passed Windows PostgreSQL and compilation/contracts. The Linux strict Clippy checkpoint failed on the independent `wizard.rs:1098` finding tracked at A8. The first A9 push at `85216a1abe766ec23bafa35b48b1598ba57a2e3c` passed the in-memory ACL step in [Windows native #44225980846](https://github.com/nghianguyen150612/Synveil/actions/runs/44225980846). Its candidate job was preparing Qt when checked before the pinned-read follow-up. Exact-head runs for the follow-up are tracked in the draft PR.

| Gate | Status | Evidence or blocker |
| --- | --- | --- |
| `P028_ACL_CONTRACT_PASS` | PASS, scoped | Hosted in-memory descriptor contract passed at the first A9 implementation head. Track the follow-up rerun on PR #189. |
| `P028_PROTECTED_STATE_IMPLEMENTED` | BLOCKED | Approved-machine effective access, write-side path races and interruption recovery are unqualified. |
| `P028_TRUSTED_OBSERVER_IMPLEMENTED` | BLOCKED | Scheduler-to-authentication causality, loaded-image proof and independent Windows qualification remain open. |
| `P028_COORDINATOR_IMPLEMENTED` | BLOCKED | Native authority and native phase advancement are unavailable. |
| `P028_HARNESS_IMPLEMENTED` | BLOCKED | Native GUI/IPC, lifecycle, recovery and independent cleanup/finalization orchestration are incomplete. |
| `P028_NATIVE_ACCEPTED` | BLOCKED | No approved Windows 11 AMD64 interactive qualification. |

### Final A9 hosted qualification — 7ac83060922c8b333f354a39392580f49ca6c63a

The final A9 head is the PR #189 head and remains Open, Draft and unmerged. Final-head hosted results:

- [Windows native push #44226282913](https://github.com/nghianguyen150612/Synveil/actions/runs/44226282913) and [Windows native PR #44226290573](https://github.com/nghianguyen150612/Synveil/actions/runs/44226290573) both passed the protected ACL in-memory descriptor contract, checkpoint/source wiring, PowerShell parser, 61-case LogonTrigger fixture suite, Windows current-token/pinned-read/provider metadata smoke checks, runtime build, two reproducible candidate builds, candidate identity verification and artifact upload. Standard-user candidate authentication and product acceptance passed, then both workflows failed at the existing Record required unavailable surfaces honestly step. The hosted service session lacks qualified interactive GUI, real logoff/new-logon, IPC and trusted observer evidence. This is the required hosted blocker, not native acceptance.
- [Windows installer PR #44226290411](https://github.com/nghianguyen150612/Synveil/actions/runs/44226290411) passed. [Installer security PR #44226290485](https://github.com/nghianguyen150612/Synveil/actions/runs/44226290485) passed all platform jobs. [Rust CI PR #44226290584](https://github.com/nghianguyen150612/Synveil/actions/runs/44226290584) passed its Windows, Linux and macOS checks, format/Clippy, native desktop UI and web quality jobs. Its independent dependency-policy step failed. GitHub's job-log download returned HTTP 403 during inspection, so the cause is unclassified and no A9 relation is established.
- [P027 PR #44226290525](https://github.com/nghianguyen150612/Synveil/actions/runs/44226290525) passed Windows PostgreSQL transactions/recovery and Windows compilation/contracts. Linux PostgreSQL transactions/recovery failed the formatting/strict-Clippy/documentation/whitespace step, the same aggregate failure category recorded at A8 for the independent crates/server-storage/src/wizard.rs:1098 clippy::needless_bool issue. GitHub's job-log download also returned HTTP 403, so the exact failing output for this attempt could not be re-read. P027 source was not changed.

A9 gate snapshot: P028_ACL_CONTRACT_PASS is PASS only for the hosted in-memory descriptor contract. P028_PROTECTED_STATE_IMPLEMENTED, P028_TRUSTED_OBSERVER_IMPLEMENTED, P028_COORDINATOR_IMPLEMENTED, P028_HARNESS_IMPLEMENTED, and P028_NATIVE_ACCEPTED remain BLOCKED. Observer token separation and effective NTFS checks still need approved-machine positive/negative access qualification. Scheduler event 119 still lacks an AuthenticationId link to Security 4624, and live file identity cannot prove the image bytes mapped at process creation. Native Advance remains unavailable. A10 owns those qualifications plus native lifecycle orchestration, restart recovery, cleanup and finalization.

## P028-A10 — pending-native coordinator and access qualification groundwork

Starting HEAD was `e32577039f281ddd87dc6c2b795d0733db69cda1` on `feat/v03-p028-windows-native-acceptance`; local worktree clean. The fetched branch and PR #189 were at the same SHA. PR #189 remains Open, Draft, unmerged, and based on `develop/v0.3`. The separate `/workspace/Synveil` `work` branch and worktree were preserved.

The A10 changes add `scripts/qualify-windows-native-role-access.ps1` and `scripts/windows-native-role-probe.ps1`. This is an opt-in qualification harness, not a test run on this host. It requires Windows 11 AMD64, a protected operator-created disposable-machine marker, an explicit approval string, the elevated controller token, and the existing separately provisioned observer/subject accounts. The marker and approval are operator attestations; the script cannot independently prove a VM is disposable. It requires the exact Setup and release-manifest digests and source commit, checks the installer against the manifest, and reports handle-backed candidate/manifest file identities plus hashes for each harness component and their aggregate. Those expected values must come from the independent candidate verifier; these local hashes alone are not authentication. It creates a unique temporary NTFS ACL tree only after those guards, starts role probes with `ProcessStartInfo` credentials that are never placed in arguments or output, uses the actual role tokens and `AccessCheck(MAXIMUM_ALLOWED)`, and performs positive and denied OS file operations. It verifies the controller as an elevated unrestricted local administrator; unknown and nested local/domain group SIDs fail closed for all roles. The observer probe also reads the Security and Task Scheduler channels under its actual token. It creates no account, task, service, installation, or logoff. Its tree cleanup is confined to its unique run directory. The probe does not remove the need to assess privileged parent-path races or real NTFS recovery behavior.

`scripts/windows_native_lifecycle.py` defines the 13 ordered lifecycle phases, observation/verifier/authority interfaces, a separate fixture decision simulator, and a SQLite `NATIVE_PENDING_AUTHORITY` journal distinct from fixture coordinator state. Bounded references bind to the run, machine, boot, commits, candidate, manifest, subject SID, Windows session, Authentication LUID, task instance, and process identity. For LogonTrigger evidence, the contract requires an initial interactive 4624, matching 4634, subsequent interactive 4624 with a distinct LUID, exactly one shared 119/100/129/200 task instance, and matching task 129/Security 4688 PID, process creation time, session and LUID. SQLite uses immediate transactions and FULL synchronous mode. Interrupted or partial checkpoints require manual reconciliation; no phase is automatically replayed. The native authority adapter always raises `P028_NATIVE_AUTHORITY_UNAVAILABLE`; even a synthetic observer plus a passing synthetic verifier only leaves `VERIFIED_PENDING_AUTHORITY`. `simulate_fixture_plan` exercises ordered decisions with synthetic outcomes, records no native state, and performs no host mutation. The native orchestration plan names existing tools and explicit mutation approvals, but reports every step `NOT_RUN` and starts no Setup, GUI, task, logoff, uninstall, or cleanup operation. This is coordinator contract/journal groundwork, not an enabled native coordinator or complete lifecycle harness.

The task observer still uses the documented Windows 11 24H2 event 119 logon-trigger record, joined to the exact 100/129/200 instance/process/action chain. It retains 107 (time trigger) and 110 (user-initiated) as conflicts. Exact event re-query, Security 4624/4634/4688 records, subject WTS/token SID/session/AuthenticationId and event windows remain in place. Scheduler event 119 has no AuthenticationId or direct 4624 reference, so this is a bounded corroboration and ambiguous overlap remains blocked. The executable read now gets SHA-256 and `FILE_ID_INFO` volume/file identity from the same no-reparse, one-link handle, and compares that identity at baseline and after launch. This detects ordinary path replacement or changed bytes on disk; it does not prove the bytes mapped into the already running process. The evidence explicitly retains both `BLOCKED_TASK_EVENT_HAS_NO_AUTHENTICATION_LUID` and `BLOCKED_MAPPED_IMAGE_PROVENANCE_UNAVAILABLE`, with overall status `BLOCKED_NATIVE_LOGON_TRIGGER_UNVERIFIED`.

No approved interactive disposable Windows 11 AMD64 machine was available. The opt-in role test was not executed; there is no real-token/effective-NTFS qualification or P028 native acceptance. No live Task Scheduler, Security, WTS, process-token or executable-mapping APIs were queried on this host. Workflow changes parse the new scripts and run the portable coordinator contracts; CI must not invoke the opt-in role harness.

The pending journal now checks its leaf file identity before and after SQLite use. A portable race fixture replaces the path after pre-open inspection and verifies the open is rejected before any query or mutation. This does not eliminate a privileged swap-and-restore race or protect parent directories; native use remains blocked pending actual protected-parent qualification.

Local validation on this A10 checkout:

- `python3 scripts/test_windows_native_lifecycle.py`: 19 tests PASS (portable contract fixtures, including fixture decision simulation, LogonTrigger context mismatches, leaf/parent path substitutions and interrupted checkpoint recovery).
- `python3 scripts/test_windows_native_provenance.py`: 12 PASS (synthetic provenance only); `python3 scripts/test_windows_protected_state_contract.py`: 17 PASS (source contracts only); `python3 scripts/test_windows_native_coordinator.py`: 12 PASS (fixture coordinator).
- `python3 scripts/test_windows_native_evidence.py`: 50 PASS; `python3 scripts/test_windows_native_phase_ledger.py`: 4 PASS; `python3 scripts/test_windows_lifecycle.py`: 6 PASS.
- `python3 scripts/validate-windows-installer.py`, `python3 scripts/validate-windows-native-acceptance.py`, `python3 scripts/validate-installer-security-hardening.py`, and `./scripts/validate-install-acceptance.sh`: PASS; install acceptance contract ran 21 tests and labels itself non-native.
- Portable PowerShell 7.7 parsed the acceptance scripts and compiled observer C# declarations. The 61-case Task Scheduler suite uses synthetic records. The in-memory ACL script cannot run on this Linux host because .NET Windows Principal APIs throw `PlatformNotSupportedException`; hosted Windows CI runs that check. No Windows API or effective NTFS check is claimed locally.
- `git diff --check`: PASS. `cargo` is not installed in this workspace, so Rust checks were not run.

Exact-head A10 CI has not run yet. The last inspected exact-head CI is for e325770; its hosted interactive-session failure remains a required blocker, while the unrelated dependency-policy and P027 strict-Clippy findings are recorded in the qualification report.

| Gate | A10 state | Evidence and remaining requirement |
| --- | --- | --- |
| `P028_ACL_CONTRACT_PASS` | PASS, scoped to prior hosted contract; A10 exact-head CI pending | Descriptor contract checks only. A10 role harness is source-only until approved native execution. |
| `P028_PROTECTED_STATE_IMPLEMENTED` | BLOCKED | Real-token qualification code added; not run. Write-side parent/reparse races and native interrupted-write recovery remain unqualified. |
| `P028_TRUSTED_OBSERVER_IMPLEMENTED` | BLOCKED | Existing exact event/session/process checks retained; event 119 to a specific AuthenticationId remains an ambiguous correlation, and mapped-image provenance is unavailable. |
| `P028_COORDINATOR_IMPLEMENTED` | BLOCKED | Pending journal and adapter contracts added; no trusted authority, native transition, or Windows crash-recovery qualification. |
| `P028_HARNESS_IMPLEMENTED` | BLOCKED | Phase plan covers all 13 stages as `NOT_RUN`; lifecycle orchestration, approvals, resume reconciliation, cleanup verification, and finalization are not wired end to end. |
| `P028_NATIVE_ACCEPTED` | BLOCKED | No approved Windows 11 AMD64 interactive lifecycle run or independently verified cleanup. |

The independent P027 Linux strict-Clippy blocker at `crates/server-storage/src/wizard.rs:1098`, Rust dependency-policy advisories for unmaintained `chrono` and `chrono-tz`, the P027 SQLx security review, and P036 signing remain outside this change. No unrelated implementation was changed.

### A10 handoff

Before native advancement can be enabled: run the role harness on an explicitly approved disposable Windows 11 AMD64 machine; close the write-side path race or preserve its exact threat boundary; qualify interrupted protected-state recovery; prove or retain a blocker for event 119/session causality and mapped-image identity; provide independent native observer and verifier adapters; implement authority-owned durable transitions and manual reconciliation rules; wire ordinary-user Setup/GUI/IPC/startup, genuine sign-out/sign-in, lifecycle/uninstall, independent cleanup and finalization; validate exact candidate/manifests and all references; then run and independently review the full interactive qualification. Keep PR #189 Draft and unmerged and retain `P028_NATIVE_AUTHORITY_UNAVAILABLE` until those prerequisites pass.

## P028-A11 — durable fixture execution, recovery and cleanup protocol

Recovery on 2026-11-20 found `/workspace/Synveil` clean on `work` at
`604499fce0dfd134d82da3d5bb5d4a055c5eea8d`. Preserved that worktree/branch and
created `/workspace/Synveil-p028` on `feat/v03-p028-windows-native-acceptance`
from the live PR #189 head `6c0402232fc85776fab8947a98db37ea8ac1cc3d`.
Remote PR head matched the supplied SHA; no newer commit was discarded.
PR remains Open, Draft and unmerged, based on `develop/v0.3`.

Source implementation now includes:

- `windows_lifecycle_execution.py`: executable, separately named FIXTURE_ONLY
  SQLite coordinator for all 13 phases. Immediate transactions and FULL sync
  serialize reservations/completions. New database directory entries are synced
  on POSIX. Context, phase, attempt and bounded unique evidence references are
  checked on every checkpoint load. A phase requires all preceding phases VERIFIED.
- Protocol: PREPARED → EXECUTING → RECONCILING → VERIFIED; UNKNOWN → BLOCKED,
  confirmed failure → FAILED. No automatic retries, lease stealing or phase reset.
  Before preparation there is no effect. A crash after PREPARED permits the first
  execution. After EXECUTING intent, including timeout or a crash after the effect,
  outcome is unknown: only independent re-query can continue. Recovery must prove
  the original operation quiescent and successful; PREPARED cannot be reconciled
  as success. A crash in verification/finalization rolls back the SQLite transaction
  and requires fresh re-query. No external effect is enclosed in a pretend database
  transaction. Cancellation/expiry stop advancement and preserve pending resources;
  independent cleanup auditing remains available. No exception text or credentials
  enter the journal.
- Cleanup protocol: immutable pre-effect inventory, bounded hashed references,
  explicit DELETE versus RETAIN_PROTECTED audit state, all nine resource classes
  mandatory. Missing/unknown/present resources block cleanup. Both cleanup and
  finalization re-query inventory; finalization also re-queries every earlier
  evidence receipt. No component deletes unfamiliar resources.
- `windows_lifecycle_adapters.py`: typed candidate inputs and deterministic argv
  builders matching actual environment, combined Setup/GUI/IPC journey, installed
  uninstall and disposable-account cleanup script interfaces. Subject journey and
  uninstall remain ordinary-user; account cleanup requires controller elevation.
  Contracts specify effects, timeouts, evidence/recovery/cleanup obligations. Launch
  discards raw output and treats timeout as unknown. Native launch is unreachable:
  `P028_NATIVE_AUTHORITY_UNAVAILABLE` executes before process creation.
- `inspect-windows-native-cleanup.ps1`: read-only native diagnostic queries for
  accounts, profiles, tasks, processes and exact filesystem/component references.
  No deletion. Errors, secrets, protected retention and pending-operation queries
  without authority-owned enumerators remain UNKNOWN; overall CLEANUP_BLOCKED.
  Caller-supplied inventory hash/context is integrity input, never native authority.
- `qualify-windows-native-lifecycle.ps1`: opt-in entrypoint checks explicit operator
  approval bound to the Windows machine UUID SHA-256, Windows 11 AMD64 and non-hosted
  environment before invoking the existing three-role harness with its actual
  credential interface. An external operator must approve the disposable machine;
  neither the UUID nor A10's local marker proves disposability. Creates no account,
  task or logoff. Reports recovery/complete cleanup NOT_RUN and race qualification
  BLOCKED. This is a qualification entrypoint, not complete native acceptance.
- Hosted CI runs the new integration/adversarial tests and parses the new scripts;
  it does not invoke mutating qualification and retains its interactive blocker.

This is working portable execution and cleanup **protocol**, not qualified native
orchestration. Dependency-injected fixture runner/verifier/inspector are trusted
synthetic test code; their receipts cannot authorize native launch or advancement.
The fixture journal detects accidental corruption and rejects context substitution,
partial transactions and evidence replay. It cannot authenticate a maliciously
rewritten database or detect replacement by a whole older snapshot. Protected
monotonic authority-owned state, parent/leaf race resistance and real Windows
interruption durability are prerequisites to any native implementation. A10's
pending native journal and guard are unchanged. Candidate verifier, startup task,
Security observer and native logon scripts retain their existing interfaces; no
claim is made that these are connected to a trusted native executor.

Local validation (all listed executed commands exit 0):

| Command | Result |
| --- | --- |
| `python3 scripts/test_windows_lifecycle_execution.py` | 15 tests; complete 13-phase fixture with crash after install effect, reopen, resume, independent cleanup and final chain check |
| `python3 scripts/test_windows_native_lifecycle.py` | 19 tests |
| `python3 scripts/test_windows_native_provenance.py` | 12 tests |
| `python3 scripts/test_windows_protected_state_contract.py` | 17 tests |
| `python3 scripts/test_windows_native_coordinator.py` | 12 tests |
| `python3 scripts/test_windows_native_evidence.py` | 50 tests |
| `python3 scripts/test_windows_native_phase_ledger.py` | 4 tests |
| `python3 scripts/test_windows_lifecycle.py` | 6 tests |
| `python3 scripts/validate-windows-installer.py` | PASS |
| `python3 scripts/validate-windows-native-acceptance.py` | PASS; checkpoint blocked |
| `python3 scripts/validate-installer-security-hardening.py` | PASS |
| `bash scripts/validate-install-acceptance.sh` | PASS; 21 tests, non-native |
| `/tmp/p028-pwsh/pwsh -NoProfile -File /tmp/p028-a11/parse.ps1` | PowerShell 7.5.4; Parser.ParseFile on all 29 scripts, dot-source observer support/task declarations; no Windows API invocation |
| `/tmp/p028-pwsh/pwsh -NoProfile -File scripts/test-windows-native-logon-task.ps1` | 61 synthetic cases PASS |
| `git diff --check` | PASS |

135 focused Python tests plus 21 install-contract tests. New adversarial coverage
includes concurrent reservation/completion, duplicate submissions, execution replay,
crash boundaries, UNKNOWN reconciliation, confirmed failure, corrupted/partial writes,
run/machine substitutions, incorrect observer identity, untrusted verifier shapes,
cleanup failure for every class, changed final-chain evidence and native launch denial.
Existing provenance suites cover stale/missing/modified OS record fixtures. All
completion is FIXTURE_ONLY. No approved Windows machine exists in this environment;
real token, NTFS, GUI, IPC, event/session or cleanup acceptance is NOT_RUN/BLOCKED.
`cargo` is unavailable locally; Rust checks NOT_RUN.

Recovered-head CI: [Windows native #44489877389](https://github.com/nghianguyen150612/Synveil/actions/runs/44489877389)
passed ACL contract and candidate jobs; standard-user job failed exactly at
`Record required unavailable surfaces honestly`. [P027 #44489877285](https://github.com/nghianguyen150612/Synveil/actions/runs/44489877285)
passed Windows transactions/contracts and failed Linux qualification. Preserve the
independent `wizard.rs:1098` strict-Clippy blocker. Rust dependency-policy advisories
for `chrono`/`chrono-tz`, independent P027 SQLx review and P036 signing remain open;
none were changed or bypassed.

| Gate | A11 status |
| --- | --- |
| `P028_ACL_CONTRACT_PASS` | PASS scoped to recovered-head hosted in-memory ACL contract; current-head CI tracked on PR |
| `P028_PROTECTED_STATE_IMPLEMENTED` | BLOCKED: real tokens, NTFS effective operations, path races, monotonic state and native persistence recovery |
| `P028_TRUSTED_OBSERVER_IMPLEMENTED` | BLOCKED: causal LogonTrigger→new Authentication LUID and actual mapped image provenance |
| `P028_COORDINATOR_IMPLEMENTED` | BLOCKED: portable engine implemented, trusted native transitions/recovery unqualified |
| `P028_HARNESS_IMPLEMENTED` | BLOCKED: command contracts implemented, native orchestration and complete independent resource enumerators unqualified/incomplete |
| `P028_NATIVE_ACCEPTED` | BLOCKED: approved interactive Windows 11 AMD64 end-to-end run unavailable |

A12 requirements: independently approve/provision a disposable Windows 11 AMD64
machine and three distinct roles; execute positive/negative NTFS role probes and
ancestor/write-race assessment; qualify durable protected monotonic state under
interruption; supply supported independent causal and mapped-image proofs or retain
the explicit blockers; implement authority-owned inventory/secret/pending-operation
queries and protected retained-evidence verification; bind every adapter to real
verified execution tokens and candidate/run/boot context; exercise full interactive
journey, unknown-outcome recovery and independent cleanup. Native authority cannot
safely be enabled on the strength of this portable run.

## P028-A12 — CI recovery and native-readiness audit

Recovered live PR #189 at `2b36b6a0374439c2532f538c0632248dd631be07`; no newer
head or uncommitted changes. Preserved both worktrees and original `work` branch.
Fixed confirmed Windows `ADAPTER_PATH_INVALID` in `test_real_adapter_argument_contracts`
using native absolute temporary paths (commit `b62bcbc`). Production validation
remains strict and unchanged. Added malformed/relative/cross-platform path coverage,
actual script parameter/argv checks, terminal restart tests, read-only Windows
wrong-token rejection checks, and a concrete role-probe cleanup guard: if exit
cannot be confirmed after termination request, preserve the unique tree and block.

Local exit-0 validation: lifecycle execution 17, native lifecycle 19, provenance 12,
protected-state 17, fixture coordinator 12, evidence 50, ledger 4, lifecycle 6
(137 Python tests); install acceptance 21; all three Windows/installer security
validators; PowerShell parse of 29 scripts and observer declaration compilation;
64 synthetic scheduler/cleanup cases; `git diff --check`. Cargo unavailable locally.
The Windows-only token branch and candidate reproducibility require hosted CI.
Initial repair [run #44670557965](https://github.com/nghianguyen150612/Synveil/actions/runs/44670557965)
is at `b62bcbc`; final-head results are tracked on PR after push, without claiming
pending steps passed. A11 [#44669821473](https://github.com/nghianguyen150612/Synveil/actions/runs/44669821473)
failed at checkpoint wiring, so its candidate/interactive jobs did not qualify.

See the [A12 security audit and executable handoff](../security/P028_WINDOWS_NATIVE_ACCEPTANCE_QUALIFICATION.md#p028-a12--windows-ci-repair-and-native-readiness-audit)
for component classification, provenance API review, per-resource cleanup discovery
and limitations, exact commands, role/account/approval requirements, and the opt-in
PowerShell invocation. No approved interactive Windows host was supplied; native
qualification NOT_RUN. The source fixture engine is working; the native coordinator,
complete adapter orchestration and independent cleanup enumerators are incomplete.
No further placeholder authority layers were added.

All five native implementation/acceptance gates remain BLOCKED. Evaluate
`P028_ACL_CONTRACT_PASS` separately from exact-head CI (in-memory descriptor only).
Do not enable `P028_NATIVE_AUTHORITY_UNAVAILABLE`: protected monotonic recovery,
real-token NTFS/race qualification, automatic-trigger Authentication LUID causality,
and original mapped-executable provenance remain unproven. Unknown credentials,
retained protected evidence or pending effects block cleanup/finalization.
P027 Linux strict Clippy `wizard.rs:1098`, `chrono`/`chrono-tz` advisories,
independent SQLx review and P036 signing remain untouched. PR #189 stays Open,
Draft and unmerged. No P029/P036 or v0.2 behavior changes.
