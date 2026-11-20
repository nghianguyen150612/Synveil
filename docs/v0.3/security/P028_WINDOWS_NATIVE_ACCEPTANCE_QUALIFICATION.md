# P028 Windows native acceptance qualification

Status: **BLOCKED / implementation incomplete**. This document does not qualify P028 for integration. The authoritative v0.3 roadmap was not recovered.

The integration base is `b8beb109e58c847cb136607e4948c564cf6d2aa7`, the merge of P027 source `441237bb42a83808044111f1eb90a08d5396fe0f`. The existing v0.2 native acceptance workflow, candidate producer/consumer, environment qualification, GUI journey, logon/logoff collectors, installed uninstall test, client probe, summary and evidence validators remain the starting architecture. Historical records are unchanged.

Prior [run 42799146489](https://github.com/nghianguyen150612/Synveil/actions/runs/42799146489) tested P027 source HEAD: candidate build, reproducibility and transfer authentication succeeded; standard-user-native failed at the required unavailable-surfaces checkpoint. This is hosted scoped evidence, not Windows 11 interactive acceptance. Candidate digest and size have not been independently recorded here. No new candidate has been qualified.

The standalone evidence validator now rejects a PASS without genuine interactive session classification and a positive session ID bound to a user SID digest. This is a necessary check, not independent verification of the component artifacts or a complete security qualification. Existing evidence tests include candidate identity, harness mismatch, component and session negative cases.

GUI interaction, actual new logon and LogonTrigger, installed IPC, repair, upgrade/downgrade, uninstall/reinstall, cleanup and clean-machine security matrix remain **BLOCKED**: an explicitly approved isolated Windows 11 AMD64 interactive environment with a disposable ordinary user and authorized event access is not available in this workspace. Missing trusted historical fixtures also block upgrade claims. No native execution was performed here. The hosted service-session blocker must remain.

Remaining independent release requirements include Linux APT non-root Qt/QML and Fedora DNF failures; P036 signing, keyring and runners; P045 clean-machine evidence; server self-host acceptance; independent SQLx SQLite patch security review; historical Windows missing-LICENSE and secret-marker investigations; P004 policy decisions; production RBAC and approval integration; physical mobile testing; real LAN/Tailscale qualification; and roadmap reconciliation. Production RBAC, role persistence, approval APIs, new token issuance and pairing activation remain disabled.

## P028-A audit and changes

The hosted workflow builds a candidate twice, uploads it, authenticates it in the consumer and runs the scoped standard-user test before deliberately failing at the unavailable-surfaces checkpoint. The interactive environment, GUI/IPC, new-logon, separate logoff observer, uninstall and cleanup scripts are separate components; no approved interactive orchestrator invokes the complete sequence. Repair, reinstall, trusted historical upgrades and cross-phase machine identity remain incomplete. The GUI harness drives actual UI Automation and the installed-product probe, but source presence is not native evidence. The logoff collector requires a separate authorized administrator observer; the application test itself must remain an ordinary-user phase.

The aggregator now checks the environment/GUI session ID, logon/logoff authentication LUID digest and event/phase ordering. Missing fields remain NOT_RUN; contradictory values fail. These checks do not authenticate JSON or prove same-machine origin, prevent all replay, or replace independent event inspection. No native PASS is asserted. The hosted consumer now checks the candidate source against the workflow SHA and records the build run ID for locally produced candidates. Candidate digest and size for run 42925276618 could not be independently retrieved: GitHub artifact download returned HTTP 403. The run itself reports producer and transfer success, not native acceptance.

Checklist: [x] inspect branch and PR; [x] strengthen phase correlation and regression tests; [x] bind hosted candidate to workflow SHA; [ ] complete approved multi-session orchestration and machine/run binding; [ ] execute GUI, logon, IPC and lifecycle on an approved disposable Windows 11 environment; [ ] qualify trusted historical artifacts; [ ] independently validate cleanup and final evidence.

## P028-A2: offline phase ledger audit

The read-only `scripts/verify-windows-native-phase-ledger.py` audits a schema-v1 JSON ledger with an ordered prefix of PREFLIGHT, CANDIDATE_VERIFIED, ENVIRONMENT_QUALIFIED, INSTALLED, GUI_VERIFIED, IPC_VERIFIED, STARTUP_CONFIGURED, LOGOFF_OBSERVED, NEW_LOGON_OBSERVED, LOGON_TRIGGER_VERIFIED, LIFECYCLE_VERIFIED, CLEANUP_VERIFIED and EVIDENCE_FINALIZED. It checks a bounded seven-day window, exact per-component run context, unique file names, regular bounded evidence files and their actual SHA-256 bytes. Its context includes distinct source and harness commits, candidate and manifest digests, producer run ID, machine and user digests, OS build and architecture. It makes no changes to machine state and always exits BLOCKED even for a complete internally consistent ledger. Its JSON is an offline audit format, not an authenticated native execution record or a persistent state machine.

The existing summary now rechecks component file bytes against its digests. The standalone evidence validator requires component files for a CLI PASS, and supports explicit component directory verification. These checks detect accidental or uncoordinated modification, not a malicious party able to rewrite both ledger and evidence. Hashing a caller-supplied machine identifier does not establish machine identity, operator approval, event origin, run uniqueness, or replay resistance. No trusted run-ID issuer, independent protected storage, ACL enforcement, finalization seal or OS observer authentication exists yet. Do not use this auditor to authorize product installation or claim native acceptance.

The environment qualification is read-only; GUI drives Setup and the installed IPC probe as standard user; the new-logon collector checks Security and Task Scheduler events; the separate elevated logoff observer consumes a **prior** new-logon record. Consequently those collectors do not currently establish initial logoff followed by new logon. The uninstall component handles preservation checks; full repair/reinstall orchestration and authenticated historical upgrade fixtures are missing. Cleanup requires the existing separate account cleanup phase and independent verification. None is wired into a protected resumable multi-session orchestrator. Native operation remains blocked pending explicit approval of an isolated disposable Windows 11 AMD64 environment and implementation of those protections.

## P028-A3: transactional fixture coordinator core

`scripts/windows_native_coordinator.py` implements an executable **fixture-only** state machine. Explicit fixture approval creates an exclusive `p028-fixture-state.sqlite3` with a random 128-bit run ID, bounded deadline and SQLite transactions. `advance-fixture` serializes the ordered phases with `BEGIN IMMEDIATE`, stores bounded evidence bytes and digest atomically, rejects duplicate phases and different run IDs, and requires separately recorded fixture cleanup before terminal finalization. `status` resumes and checks committed phase order and evidence hashes. SQLite rollback recovers an interrupted uncommitted transaction; the database remains for inspection after a failed initialization. No cleanup deletes files. This is not protected native state: filesystem parent ACLs, Windows ownership, junctions and effective permissions are not verified, and the mode is explicitly rejected as native authority. The fixture approval string is not operator authorization for a Windows installation.

The fixture subject and controller have no enforced trust separation: any actor able to modify the SQLite database can forge a run, and a database copy or rollback can replay it. A SHA-256 digest is not an authenticator. The tool does not collect events, verify OS identity, run Setup, invoke the GUI/IPC or lifecycle scripts, or perform logoff/logon coordination. Existing `BLOCKED_UNVERIFIED_NATIVE_PROVENANCE` remains mandatory. A real Windows controller still needs independently verified ACLs and observer role, reparse-safe protected state, real logoff then new-logon event correlation, lifecycle wiring, durable external anti-rollback authority and Windows execution tests. No claim of protection from a malicious administrator or kernel is made.

## P028-A4: Windows SQLite handle repair and protected-state source

Windows run [42983808328](https://github.com/nghianguyen150612/Synveil/actions/runs/42983808328), job `130400142916`, failed the coordinator tests with WinError 32 during temporary-directory deletion. The coordinator used `with sqlite3.Connection`, which scopes transactions but does not close the handle. The close-path fix adds a `database()` context manager that rolls back open transactions and closes in `finally`; `open_db()` also closes if PRAGMA setup raises. Windows run [42992673483](https://github.com/nghianguyen150612/Synveil/actions/runs/42992673483) then exposed a second, test-owned leak: the corruption test used the same transaction-only context manager, so its UPDATE rolled back and its connection stayed open. The test now commits the intentional tamper and closes explicitly. On the subsequent Windows run [42993049322](https://github.com/nghianguyen150612/Synveil/actions/runs/42993049322), the complete checkpoint wiring step passed, including all 11 coordinator tests, 50 evidence tests and 4 ledger tests; the candidate build proceeded to runtime build. This verifies coordinator tests on Windows, not interactive acceptance.

New `scripts/protect-windows-native-run-state.ps1` provides a Windows-only source path for initializing, verifying and advancing a protected run. It requires elevated administrator execution, an existing protected `C:\ProgramData\Synveil\P028` root, Windows 11 AMD64, an explicitly named local disposable `synveil_p013_XXXXXXXX` account, candidate/source/harness identity and a bounded deadline. It generates a GUID run ID. It applies explicit ACLs with inheritance disabled: SYSTEM and Administrators full control; the subject gets read access to run/trusted state and Modify only on a subject inbox. A separate observer inbox and evidence directory are admin-only. It checks owner, explicit rules, inherited rules, reparse points, machine GUID digest and OS build before accepting a transition. Subject inbox evidence is opened with `FILE_FLAG_OPEN_REPARSE_POINT`, checked through its handle, bounded, and read with write/delete sharing denied. Updates flush a new bounded state file and replace atomically under a named mutex. Exact phase order, run/candidate/source/harness/machine/user binding, evidence digest reuse, independent observer SID on observer phases and terminal finalization are checked. It performs no account creation, product installation, scheduled-task change or cleanup action.

This script has not executed on an approved Windows 11 disposable machine. ACL semantics, effective membership, reparse protections, crash behavior and event provenance remain unqualified until such tests run. The controller SID is recorded as a digest, but an administrator can rewrite state and evidence; no claim against a malicious administrator or kernel is made. The current evidence envelope and observer integration are source-level groundwork, not a secure end-to-end run authority. The existing GUI, IPC, logoff/logon and lifecycle scripts are still not invoked by an orchestrated phase chain. Hosted service-session blocking remains.

For source HEAD `27d1d80860e42779cf04aa268d8c8f8cd340b977`, Windows native run #42995046774 passed the coordinator/source checkpoint and PowerShell syntax parser, including the protected-state script, then remained in `Build runtime once`. Windows installer #42995046801, remaining Rust matrix, Windows P027 compile contracts and remaining installer-security platform jobs were pending at last inspection; selected Windows and PostgreSQL sub-jobs passed. Exact jobs and links are in the P028 manifest. No candidate build result or native interactive result is inferred from a running job.

### P028-A6 ACL contract repair (source/scoped test only)

The admin-only observer inbox and evidence directory were created without a subject ACE but verified using a default that required one. Both initialization and reopen verification now explicitly omit the subject ACE. The inheritance mask comparison is parenthesized and inherited ACEs are rejected explicitly. The disposable SID pattern now includes its account RID. Reopening state now compares both the current controller SID and supplied subject SID to the protected identity hashes.

The new Windows-hosted ACL test constructs and checks security descriptors in memory, including missing/extra subject ACE, wrong owner and unprotected inheritance rejection. It does not establish effective access, resist parent-path races, or replace a test using real administrator, observer and disposable subject tokens. Actual Windows 11 ACL enforcement, junction substitution, crash recovery and event provenance remain BLOCKED pending an explicitly approved isolated machine. In particular, the existing phase records are not independently authenticated Windows events; their presence is not acceptance or authorization for product effects. The native coordinator, lifecycle orchestration and independent cleanup verification remain unfinished.

The RUSTSEC-2026-0486 lockfile update to `rustix` 1.1.6 passed local dependency policy and installer-engine tests; [exact-head Rust CI #43140141790](https://github.com/nghianguyen150612/Synveil/actions/runs/43140141790) was queued/in progress when checked, so its hosted conclusion remains pending. The SQLx SQLite independent security review, signing and all other earlier gates remain open.

Until independently authenticated provenance is available, `Advance` now fails closed with `P028_NATIVE_AUTHORITY_UNAVAILABLE` after identity and terminal-state checks. The remaining transition code is groundwork and cannot record a phase. `Verify` reports only the checks it performs; it is not evidence of effective-access isolation or native acceptance.

## P028-A7: pending-authority verification invariant

The protected run-state controller retains an explicit `Advance` stop until native observer provenance can be authenticated. While that gate exists, `Verify` now rejects run-state JSON claiming any completed phases or finalization, a non-active status or a mode other than `WINDOWS_PROTECTED_PENDING_NATIVE_AUTHORITY`. It also reads state and previously persisted GUI/new-logon/evidence bytes through the pre-existing bounded Windows file-handle reader, rejecting a reparse point on the opened file. Source regressions validate the wiring, not Windows effective access.

This is defensive groundwork only. The handle reader does not prove trust against an administrator/kernel attacker, independently verify event provenance, eliminate every parent-path race or supply cross-session native acceptance. A real observer adapter, token-isolation tests and approved interactive native validation are still required. Native acceptance remains BLOCKED.

## P028-A8: privileged OS event observer foundation

Starting source HEAD was `975540ffa7c6dddbbe468caeda09c7df8f376e96` on `feat/v03-p028-windows-native-acceptance`; PR #189 was open, Draft, and targeted `develop/v0.3`. The remote branch had not advanced. The implementation adds read-only `ObserveInitialSession`, `ObserveLogoff`, `ObserveNewLogon`, and `VerifyObserverSequence` actions to `scripts/protect-windows-native-run-state.ps1`. `ObserveLogonTrigger` is an explicit BLOCKED result.

The observer action uses the actual current Windows identity, local Administrators membership, and `TokenElevation` from `GetTokenInformation`; it then checks the protected run, controller/subject hashes, machine GUID, boot-time identity, OS build, run deadline, root ACLs and path components already required by the controller. It queries the local Security event channel with `Get-WinEvent` and parses the event XML directly. Supported records are Security 4624 (interactive LogonType 2 or 10) and 4634. The parser checks provider, channel, event ID, positive `EventRecordID`, system timestamp, machine name, unique named fields, and agreement between XML and the `EventRecord` properties. Queries are bounded to 4,096 records and observations are limited to a 15-minute window.

To identify the initial and subsequent sessions, it obtains the subject `explorer.exe` process token using `OpenProcessToken` and `GetTokenInformation(TokenUser, TokenStatistics, TokenGroups, TokenElevation)`, then checks that the token is not elevated and contains no Administrators group SID. It checks the Authentication LUID and process session against WTS session state and WTS user/domain identity. The initial 4624 must match the live initial token. A 4634 must match the same subject SID and Authentication LUID, follow that 4624, and the original WTS session must no longer be active. A subsequent 4624 must match a live subject token, follow the 4634, and use a different Authentication LUID. The run also binds `Win32_OperatingSystem.LastBootUpTime`, because Windows documents that Logon IDs are unique only between reboots on the same computer. Multiple or missing matches, event truncation, unsupported schema, stale timestamps, replay, reboot, or identity inconsistency fail closed.

The event fields used here match Microsoft's published event schemas: [Security event 4624](https://learn.microsoft.com/en-us/windows/security/threat-protection/auditing/event-4624) documents `TargetUserSid`, `TargetLogonId`, and `LogonType`; [Security event 4634](https://learn.microsoft.com/en-us/windows/security/threat-protection/auditing/event-4634) documents `TargetUserSid` and `TargetLogonId` and describes correlation to event 4624 by Logon ID. The token and WTS observations use Windows [`GetTokenInformation`](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-gettokeninformation) and [`WTSQuerySessionInformation`](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsquerysessioninformationw). This does not establish Task Scheduler event semantics, which remain deliberately blocked.

Observer outputs are written only to the admin/SYSTEM `observer-inbox`, with `CreateNew`, bounded bytes, flush-to-disk, atomic rename, and a bounded no-reparse readback. They include schema/run/source/harness/candidate/manifest bindings, hashed machine/subject/observer/controller identities, event source and record identifiers, event and observation times, event XML digest, Windows session ID, and hashed Authentication LUID. They contain no raw event XML, raw LUID, names, or MachineGuid. The status is `OBSERVED`, never `PASS`; the outputs are not consumable as phase success. `VerifyObserverSequence` validates the three-record context, hashes, ordering, LUID relation and replay chain, then reports that LogonTrigger remains blocked.

This is a privileged local observer implementation, not a qualified independent authority. Observer and controller are currently the same elevated account in the protected-state invocation; their actual token identity is recorded in separate fields, but this is not separation against a malicious administrator. The predecessor digests detect an inconsistent chain, but no immutable external anchor protects that chain; a digest is not a signature. An administrator or compromised kernel can alter event logs, process tokens, ACLs, state and observer code. The supported trust boundary therefore assumes a trusted Windows kernel and trusted local administrator. No claim is made against those actors.

The existing Task Scheduler event assumptions do not establish that a particular task instance came from the LogonTrigger and ran in the same Authentication LUID as the new logon. The current task events do not give this implementation sufficient independently verified instance/session correlation. `ObserveLogonTrigger` therefore emits `P028_BLOCKED_LOGON_TRIGGER_PROVENANCE_UNAVAILABLE`; it does not accept the existing subject-authored logon JSON. The protected `Advance` path still throws `P028_NATIVE_AUTHORITY_UNAVAILABLE`. The fixture model in `scripts/windows_native_provenance.py` and its tests validates a synthetic end-to-end contract only; it returns `BLOCKED_UNAUTHENTICATED_FIXTURE` and cannot authorize native coordinator state.

The existing path-component reparse check is still path-based and can race with privileged path substitution; effective NTFS access and nested-group behavior are not established by the in-memory ACL contract. Observer output is protected by the same administrator-controlled ACL boundary, not by hardware-backed key isolation. Atomic evidence creation is implemented, but the native lifecycle does not yet resume an interrupted phase chain. Lifecycle, cleanup and finalization are not wired to the observer. These limitations keep native advancement disabled.

### A8 validation evidence

On the starting checkpoint, Windows native workflow [#43351781477](https://github.com/nghianguyen150612/Synveil/actions/runs/43351781477) tested commit `975540ffa7c6dddbbe468caeda09c7df8f376e96`: `protected-acl-contract` PASS (in-memory descriptor contract), `build-windows-candidate` PASS (source wiring, PowerShell parse, reproducible candidate build), and `standard-user-native` reached standard-user product acceptance PASS before failing at the intentional `Record required unavailable surfaces honestly` step. That failure represents hosted service-session GUI/logon/IPC/observer limitations, not a source-test regression and not native acceptance.

At this checkpoint, local tests passed: provenance fixtures 10; protected-state contract 10; fixture coordinator 11; native evidence 50; phase ledger 4; `validate-windows-native-acceptance.py`; `git diff --check`; installer contract and 21 tests. A portable PowerShell 7.7 runtime parsed all 8 Windows acceptance scripts, and `Add-Type` compiled the observer C# declaration. No Windows P/Invoke was invoked. No privileged/unprivileged Windows token, event-channel, logoff/logon, or effective NTFS test was performed. No Windows machine was mutated and no native acceptance is claimed.

On exact source head `20399e9cc61039bfa00ed29033e2c5cdb73dcdac`, [Windows native run #44007390224](https://github.com/nghianguyen150612/Synveil/actions/runs/44007390224) completed. `protected-acl-contract` PASS (in-memory descriptor test); `build-windows-candidate` PASS (checkpoint/source tests, PowerShell parser, runtime build, two reproducible candidate builds, candidate identity checks); `standard-user-native` passed candidate authentication and standard-user product acceptance, then failed at `Record required unavailable surfaces honestly` because the hosted service session cannot provide qualified interactive GUI, real new-logon, IPC and observer evidence. Evidence upload passed. Overall workflow conclusion is FAILURE by this explicit environment gate; this is not native acceptance and not a source-test failure. No final native qualification is inferred from the run.

### Gate assessment after A8 source work

| Gate | State | Evidence and remaining condition |
| --- | --- | --- |
| `P028_ACL_CONTRACT_PASS` | true, scoped | Hosted in-memory ACL contract passed in run #43351781477. It does not prove effective NTFS isolation. |
| `P028_PROTECTED_STATE_IMPLEMENTED` | false | Source protections exist, but effective access, path substitution races, interrupted Windows writes, and real token separation are not qualified. |
| `P028_TRUSTED_OBSERVER_IMPLEMENTED` | false | Security 4624/4634 observer foundation exists. Task LogonTrigger provenance and a complete independently validated observer sequence remain blocked. |
| `P028_COORDINATOR_IMPLEMENTED` | false | Observer evidence is not a durable phase transition; `Advance` remains blocked. |
| `P028_HARNESS_IMPLEMENTED` | false | GUI/IPC, LogonTrigger, lifecycle, cleanup, interruption recovery, and finalization are not orchestrated. |
| `P028_NATIVE_ACCEPTED` | false | No approved disposable Windows 11 AMD64 interactive qualification was performed. |

Independent P027 SQLx security review and P036 signing/key-management blockers remain open. The starting checkpoint's [P027 PostgreSQL run #43351781449](https://github.com/nghianguyen150612/Synveil/actions/runs/43351781449) passed its live PostgreSQL transaction suites but failed the unrelated Linux formatting/strict-Clippy step at `crates/server-storage/src/wizard.rs:1098` for `clippy::needless_bool`. No P027 source change was made here.

### A8 continuation: OS record re-query contract

The continuation from recovered PR head `035dae99df7831a32e6205f6dc3ae67f3c297910` extends `VerifyObserverSequence` so that it re-queries each recorded Security EventRecordID from the local Windows Security channel. The code accepts only event IDs 4624 and 4634, requires one exact record, parses its XML with the same strict provider/channel/record identity checks as initial observation, and compares the OS XML digest, timestamp, subject SID, normalized target logon LUID, and interactive LogonType for 4624. Persisted JSON and a chained digest alone cannot satisfy this re-query. Re-query failure, record rotation/absence, ambiguity, schema drift, or field mismatch blocks verification.

The re-query is a freshness and consistency check under the trusted local Windows kernel/administrator assumption. It is not a signature and does not protect against an administrator or kernel that can alter the event log or controller. The observer and controller still execute under the same elevated identity; separate effective token isolation is unqualified. The output remains `OBSERVED`, not phase success. Verification still ends at `P028_BLOCKED_OBSERVER_SEQUENCE_LOGON_TRIGGER_UNSUPPORTED`; native advancement still fails with `P028_NATIVE_AUTHORITY_UNAVAILABLE` before consuming subject evidence.

The supported observer sources are the local Windows Security event channel via `Get-WinEvent`, Windows event XML via `EventRecord.ToXml()`, process security tokens via `OpenProcessToken` and `GetTokenInformation` (`TokenUser`, `TokenStatistics`, `TokenGroups`, `TokenElevation`), process session IDs via `ProcessIdToSessionId`, and active WTS session/user/domain state via `WTSQuerySessionInformation`. The records used are Security 4624 and 4634. The code requires the fields `TargetUserSid`, `TargetLogonId`, and `LogonType` on 4624, and `TargetUserSid` and `TargetLogonId` on 4634, validates them as present and unique in EventData, and binds the event Logon ID to the live token's AuthenticationId. Windows Logon IDs are compared only within a bound boot identity. Task Scheduler event IDs are not used as proof of an automatic trigger because they do not establish the required task-instance-to-new-session correlation in this implementation.

The threat model trusts the Windows kernel and the local administrator operating the disposable machine. It does not claim security if either is compromised. Subject-authored evidence cannot set native authority; protected ACLs and token checks separate ordinary subject writes from the observer inbox under the stated assumption. A same-admin attacker can still replace controller code/state, alter event logs, or forge observer artifacts. The machine GUID and SID hashes bind records to observed values but are not independent authentication by themselves. Effective ACL/token separation and parent-path race resistance still require real Windows qualification.

Continuation-local checks passed: provenance fixtures (10), protected-state source contract (13), fixture coordinator (11), native evidence (50), phase ledger (4), native acceptance source validator, and `git diff --check`. The original continuation authoring environment lacked PowerShell; during the resumed validation below, portable PowerShell 7.7 parsed all eight acceptance scripts. No Windows API/event/ACL execution is claimed from these checks. Exact-head hosted results and the remaining P027 formatting failure are recorded in the manifest. All six gate states remain as listed there; no native acceptance gate is enabled.

### Exact-head re-query qualification status: `2fc456c0eba02dfac39dcea3264cc48963cab003`

In the resumed workspace, portable PowerShell 7.7 parsed all eight Windows acceptance scripts. No PowerShell observer action or Windows P/Invoke was executed locally. Focused fixture/source tests passed: provenance 10, protected-state contract 13, coordinator 11, evidence 50, phase ledger 4, native and installer source validators, and install contract plus 21 tests.

Hosted [Windows native run #44011579475](https://github.com/nghianguyen150612/Synveil/actions/runs/44011579475) corresponds to this exact branch head. `protected-acl-contract` passed its in-memory descriptor test. `build-windows-candidate` passed checkpoint/source validation, PowerShell parsing, runtime compilation, two reproducible candidate builds, identity checks and evidence upload. Candidate transfer/authentication and standard-user product acceptance passed. The standard-user job then failed at `Record required unavailable surfaces honestly` and uploaded bounded blocker evidence because the GitHub service session does not provide qualified interactive GUI, genuine logoff/new-logon, IPC and observer provenance. This is an intentional environment block, not P028 native acceptance.

The current P027 run [#44011585872](https://github.com/nghianguyen150612/Synveil/actions/runs/44011585872) passed Windows PostgreSQL transactions/recovery and Windows compilation/contracts, and its Linux PostgreSQL transactions/recovery also passed. The Linux formatting/strict-Clippy job failed on the unrelated `crates/server-storage/src/wizard.rs:1098` `clippy::needless_bool` finding. No P027 changes were made; P027 SQLx review and P036 signing/key-management remain independent blockers.

## P028-A9 — scheduler instance attribution and separate tokens

**BLOCKED for native authority and acceptance.** The protected observer now has a bounded, read-only procedure for the specific scheduler and authentication records described below. It can write an `OBSERVED` diagnostic. `ObserveLogonTrigger` and `VerifyObserverSequence` exit 2 even when every supported observation is consistent. `Advance` continues to throw `P028_NATIVE_AUTHORITY_UNAVAILABLE` before reading phase evidence.

### Scheduler and process evidence

The Windows 11 24H2 build 26100.1742 publisher metadata describes Operational event 119 as a launch due to user logon (`TaskName`, `UserName`, `InstanceId`). Event 107 is a time trigger. Event 110 describes a user-initiated launch. Event 100 supplies the start instance and user context, 129 supplies an instance string named `Path` and `ProcessID`, and 200 supplies the action image name and `TaskInstanceId`. The code checks the 129 `Path` as a GUID and requires its PID to resolve to the expected **client** image under an ordinary subject token. An engine PID, missing instance GUID or unrecognized schema blocks the diagnostic. Event 200 v1's `EnginePID` is never used as a client PID. [Archived Windows 11 Task Scheduler publisher metadata](https://github.com/nasbench/EVTX-ETW-Resources/blob/main/ETWProvidersManifests/Windows11/24H2/W11_24H2_Pro_2024102_26100.1742/WEPExplorer/Microsoft-Windows-TaskScheduler.xml)

- The elevated controller's `CaptureLogonTask` reads the exact registered `\\Synveil\\BackgroundClient\\profile-<UUIDv7>` definition through local Task Scheduler COM. It requires one enabled subject-specific LogonTrigger, one least-privilege `InteractiveToken` principal for the subject SID, and a single argument-free Exec into that user's canonical per-user install. It compares the installed file to the expected staged candidate client digest, records the exact XML digest and anchors the Operational log by record ID and XML digest. The baseline binds to the controller's run state and must predate the first observed logoff. The expected client digest must come from the authenticated candidate's trusted staging output. The published release manifest binds Setup bytes and does not independently identify the extracted client bytes. The baseline therefore corroborates the installed path and file and supplies no independent candidate authority. [Task Scheduler schema](https://learn.microsoft.com/en-us/windows/win32/taskschd/task-scheduler-schema)
- The observer re-queries the exact A8 Security 4624/4634/4624 records, including exact XML digests, timestamps, subject SID, interactive logon type and boot-bound LUID. It refuses an extra interactive subject logon in the new-logon window. It checks the live subject WTS session and shell token, then re-queries the registered definition and Operational anchor. From a bounded 4,096-record scheduler scan it requires a single ordered 119/100/129/200 set with one instance GUID and the expected task, subject and action image, within 60 seconds of the exact new 4624. It rejects competing selected events such as manual starts, time triggers, task updates, duplicate starts and restart activity. Each selected record is re-queried by exact ID in its channel. Event record IDs are ordered only within their own channel. [Security event 4624](https://learn.microsoft.com/en-us/windows/security/threat-protection/auditing/event-4624) · [Security event 4634](https://learn.microsoft.com/en-us/windows/security/threat-protection/auditing/event-4634)
- The observer checks the 129 PID against a live process handle, image path, creation time, `TokenUser`, `TokenSessionId`, `TokenStatistics.AuthenticationId`, elevation and groups. It requires one audited Security 4688 v2 creation for that PID in the bounded new-logon window. The audited image and target SID/LUID must agree with the live process and 4624, and the 4688 record must survive exact-ID re-query. An absent or empty target context, unavailable process, or missing audit policy blocks the diagnostic. It re-reads the installed client with bounded, no-reparse handles and matches the baseline. Raw usernames, SIDs, LUIDs, paths, event XML and command lines stay out of emitted observations. [Security event 4688](https://learn.microsoft.com/en-us/windows/security/threat-protection/auditing/event-4688) · [Process image query](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-queryfullprocessimagenamew)
- The scheduler trigger publisher identifies the reason **and instance**, but the 119 template has no authentication LUID, Windows session ID or 4624 cross-reference. The PID and process token corroborate the execution context. They do not independently identify which authentication session originated the trigger when scheduler log delivery, registration races or simultaneous logons make causality ambiguous. Current on-disk file bytes do not attest the image bytes mapped at process creation. Exact re-queries check the Windows records under the trusted kernel and administrator assumption. Hashes in the controller or observer JSON are commitments, and carry no authority on their own.

### Role contract for an approved disposable Windows 11 AMD64 machine

The controller is a local elevated administrator. The observer is a different, non-elevated local `synveil_obs_<8 hex digits>` account with enabled **Event Log Readers** membership. The subject is a third, marked local `synveil_p013_<8 hex digits>` account. The observer and subject token checks inspect raw group SIDs, including deny-only or nested memberships as materialized in the token, and reject unexpected built-in groups, privileges, filtered tokens and AppContainers. Access checks use the same token handle that was checked for the role. Use fresh logons after any operator-provisioned membership change. The observer's effective token must match the SID bound by the controller at initialization. Controller authority is checked from the live process token and controller-owned state. The observer cannot select that role by writing JSON. The contract creates no accounts, tasks, privileged services or channel configuration.

The operator supplies a pre-protected NTFS root at or under `%ProgramData%\\Synveil\\P028` with Administrators and SYSTEM Full Control, an explicit observer Read and Execute ACE, protected inheritance and Administrators ownership. The run root allows both ordinary roles to read it. The trusted subtree grants read access to the observer only. The observer can modify its own inbox and the subject can modify its own inbox. Each is denied the other's inbox, and both are denied the phase-evidence store. The controller checks directory ownership and exact ACEs. `VerifyRoleAccess` takes the live observer and subject process IDs, checks their token identities, applies native `AccessCheck(MAXIMUM_ALLOWED)` to run descriptors and checks ancestors for destructive or ACL-changing rights. This is a diagnostic of the evaluated descriptors and tokens. It does not certify every file operation, inheritance path, share/mandatory label or race; A10 must perform real positive and denied opens/writes/renames under all three tokens. Protected reads pin parent directory handles and reject reparse points and hard-linked leaf files. Write-side ancestry and crash/retry behavior still need qualification. [AccessCheck](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-accesscheck)

On the approved machine, use a fresh `Initialize` (old A8 pending runs lack the observer SID binding). Supply the same `ApprovedRoot`, `RunId`, `SubjectSid`, `ObserverSid`, source/harness commits, Setup/manifest digests, exact UTC expiration and approval string throughout. The controller runs `VerifyRoleAccess` with living role process IDs and `CaptureLogonTask` with `TaskProfileId` and `CandidateClientSha256`. The observer performs `ObserveInitialSession` after the subject's initial interactive sign-in, `ObserveLogoff` after genuine sign-out, then `ObserveNewLogon`, `ObserveLogonTrigger` and `VerifyObserverSequence` after the second sign-in. Capture the task baseline before the initial sign-out. Windows must provide query access to the required event channels, scheduled task, WTS session, live process token and installed file for the observer identity. Lack of access blocks the run. The existing subject new-logon tool and standalone logoff tool supply bounded-scope diagnostics and report their task-origin or completion checks blocked. They do not replace the protected observer.

A10 must qualify the observed scheduler and 4688 behavior on Windows 11 AMD64 with actual automatic and manual runs, competing logons, scheduler/audit log gaps and image substitution. Resolve causal and executed-image provenance before wiring native phase transitions, recovery, lifecycle orchestration, independent cleanup and finalization. Keep the hosted service-session blocker and the independent P027 SQLx review and P036 signing open.

### A9 final-head results — 7ac83060922c8b333f354a39392580f49ca6c63a

The A9 final head is 7ac83060922c8b333f354a39392580f49ca6c63a. On [Windows native push #44226282913](https://github.com/nghianguyen150612/Synveil/actions/runs/44226282913) and [Windows native PR #44226290573](https://github.com/nghianguyen150612/Synveil/actions/runs/44226290573), the protected in-memory ACL contract passed. Candidate jobs passed checkpoint/source tests, PowerShell parsing, the 61-case synthetic task suite and read-only Windows current-token, pinned file read and scheduler metadata smoke checks. Both exact candidate builds, reproducibility, identity verification and artifact upload passed. Standard-user candidate authentication and product acceptance passed before the expected Record required unavailable surfaces honestly failure. The hosted worker has no qualified interactive GUI, actual user sign-out/sign-in, IPC or observer context. Overall Windows native conclusion is FAILURE by this deliberate environment gate. This run does not attest effective NTFS isolation, automatic trigger causality or native acceptance.

[Windows installer #44226290411](https://github.com/nghianguyen150612/Synveil/actions/runs/44226290411) passed. [Installer security #44226290485](https://github.com/nghianguyen150612/Synveil/actions/runs/44226290485) passed. Rust CI [#44226290584](https://github.com/nghianguyen150612/Synveil/actions/runs/44226290584) passed platform tests, format/Clippy, native desktop UI and web quality, and failed only its separate dependency-policy job. The hosted job log download returned HTTP 403, preventing inspection of that step's detailed output.

[P027 #44226290525](https://github.com/nghianguyen150612/Synveil/actions/runs/44226290525) passed Windows PostgreSQL and compilation/contracts. Linux transactions failed the aggregate formatting/strict-Clippy/documentation/whitespace step. A8 had recorded the independent crates/server-storage/src/wizard.rs:1098 clippy::needless_bool finding. The current job log also returned HTTP 403, so the exact current-step output was unavailable. No P027 code was changed. P027's independent SQLx security review and P036 signing remain open.

| A9 gate | State | Current evidence |
| --- | --- | --- |
| P028_ACL_CONTRACT_PASS | PASS, scoped | Final-head hosted in-memory ACL descriptor contract. This is not effective NTFS isolation. |
| P028_PROTECTED_STATE_IMPLEMENTED | BLOCKED | Distinct-role ACL policy and native access diagnostic implemented. Real-token file/ancestor access, write-side races and interrupted writes are not qualified. |
| P028_TRUSTED_OBSERVER_IMPLEMENTED | BLOCKED | Separate-token observer path and Security/Scheduler/process re-query implemented. Task-to-logon causality and mapped-image identity remain unresolved and unqualified. |
| P028_COORDINATOR_IMPLEMENTED | BLOCKED | Native Advance still throws P028_NATIVE_AUTHORITY_UNAVAILABLE. |
| P028_HARNESS_IMPLEMENTED | BLOCKED | GUI/IPC, product lifecycle, interruption recovery, cleanup and finalization orchestration remain absent. |
| P028_NATIVE_ACCEPTED | BLOCKED | No approved disposable Windows 11 AMD64 interactive qualification. |

## P028-A10 — native trust qualification and lifecycle coordinator groundwork

Starting HEAD: `e32577039f281ddd87dc6c2b795d0733db69cda1`; branch `feat/v03-p028-windows-native-acceptance`; PR #189 was Open, Draft, unmerged, targeting `develop/v0.3`. Fetch confirmed the remote feature branch and PR head matched. The original `/workspace/Synveil` worktree on `work` was left unchanged.

### Three-role effective-access harness

`scripts/qualify-windows-native-role-access.ps1` is an opt-in diagnostic requiring Windows 11 build 22000+, AMD64, no CI/hosted-runner environment, the operator approval string, a pre-created `%ProgramData%\Synveil\P028\DISPOSABLE-ROLE-ACCESS-APPROVED.txt` marker with an admin/SYSTEM-only protected DACL, an elevated controller token, the existing marked observer and disposable subject accounts, and expected Setup/manifest hashes plus source commit. It verifies the candidate file against the manifest through bounded no-reparse handles, then records content digests and handle-backed file identity hashes for the candidate and manifest, plus hashes for the qualifier, role probe and observer support source files. The expected digests must be supplied by the independent candidate verifier; the harness does not treat caller-provided values or its own source hashes as authentication. It provisions no identities. Observer and subject credentials come through `PSCredential` prompts or parameters, with `SecureString` passwords passed only to `ProcessStartInfo`; secrets are absent from command arguments and redacted evidence.

The controller creates one GUID-named temporary tree beneath the existing protected P028 root and assigns exact protected NTFS ACEs. It checks each resulting owner and DACL. It starts the two probes under separate OS credentials, checks effective access with the process token through native `AccessCheck(MAXIMUM_ALLOWED)`, and then requires successful real controller/observer/subject file operations plus access-denied outcomes for observer/subject writes to controller state, cross-role inbox writes, observer modification of controller evidence, and subject evidence reads. The controller token must be elevated, unrestricted, non-AppContainer, and a local Administrator; the controller and child role tokens reject unenumerated memberships. The observer probe reads one record from both the Security and Task Scheduler Operational channels under its real token. The child probes call the existing `P028ObserverAccess.AssertRoleToken`, which checks actual token SID, elevation, restricted/AppContainer state, event-reader membership for the observer, and token privileges. Only enumerated standard token groups, the per-logon SID, and the role-required groups are allowed; unknown, nested local/domain, and unsupported group SIDs fail closed. Native `AccessCheck` also evaluates the token's effective group membership. Unexpected ACL group ACEs fail the exact ACL check.

The harness is not executed on this workspace. It neither qualifies the actual roles nor passes a P028 gate until run on an explicitly approved disposable Windows 11 AMD64 machine. It refuses CI, requires a separate machine marker and approval, creates no accounts/services/tasks, and confines cleanup to its uniquely named test tree. The marker and approval are operator attestations, not independent proof of disposability. Its ancestor `Get-Item` path walk is still vulnerable to privileged replacement races; do not claim write-side race elimination. An inability to clean its unique tree is reported as BLOCKED. Its evidence is bounded to 16 KiB, hashes machine/controller identities, and omits raw SIDs, usernames, paths, credentials, and event XML. The emitted `native_acceptance` is always `BLOCKED_NOT_RUN`.

### Task attribution and executable identity

The Windows 11 24H2 Task Scheduler provider metadata already archived above remains the source for event meaning: 119 identifies the user-logon trigger; 107 is a time trigger; 110 is user initiated. The A10 implementation therefore retains the existing 119/100/129/200 exact instance chain and treats 107/110, task updates and competing task instances as conflicts. A 119 `InstanceId` is bound to the start, process-created, and action event IDs. The task definition still must be one enabled subject-specific LogonTrigger with the expected least-privilege InteractiveToken principal and candidate client action. The observer preserves A8's exact Security 4624/4634/4624 record re-query and checks that the initial session logged off, the new interactive logon has a distinct AuthenticationId, and the Scheduler 129 PID has a matching live process token, WTS session, and independently re-queried Security 4688 v2 subject/LUID record. Any extra interactive subject logon in the bounded interval or duplicate/missing/mismatched record blocks.

Event 119 has no authentication LUID, Windows session ID, or direct Security 4624 reference. Its user SID and timing plus the task/process AuthenticationId are strong corroboration, but they do not prove which same-user authentication event originated the trigger in every scheduler-delivery, overlapping-logon, registration, or log-manipulation race. The implementation still emits `BLOCKED` for `LOGON_TRIGGER_VERIFIED`; it does not promote time correlation into causality.

`P028SafeEvidenceReader.Inspect` now gets the executable SHA-256 and `FILE_ID_INFO` volume serial/file ID while hashing through the same opened handle. It requires no reparse point, a single link, stable size and pinned parent/file handles. The pre-logoff task baseline binds the candidate client digest and hashed file identity; after process start, the observer rechecks both and binds the task action path to the process image path. This catches ordinary replacement or modified on-disk bytes between observations. Windows user-mode process/file APIs used here do not establish the exact bytes mapped into the process at creation, including a replacement-and-restore or in-place-write race. The record retains separate `BLOCKED_TASK_EVENT_HAS_NO_AUTHENTICATION_LUID` and `BLOCKED_MAPPED_IMAGE_PROVENANCE_UNAVAILABLE` reasons and overall `BLOCKED_NATIVE_LOGON_TRIGGER_UNVERIFIED`; no kernel driver, hook, or background service was added.

### Pending journal and orchestration contract

`scripts/windows_native_lifecycle.py` is separate from `windows_native_coordinator.py` and its fixture SQLite file. `NativePendingJournal` creates only `p028-native-lifecycle.sqlite3`, tags its mode `NATIVE_PENDING_AUTHORITY`, and binds a 32-hex run ID to machine/boot/role commitments, source and harness commits, candidate and manifest digests, and bounded event/process references. The reference contract also checks subject SID, Windows session, Authentication LUID, event record uniqueness, task instance and process identity. `LOGON_TRIGGER_VERIFIED` requires two interactive Security 4624 records separated by the original session's 4634 logoff, distinct Authentication LUIDs, one common 119/100/129/200 task instance, and consistent process PID/creation time/session/LUID between Task Scheduler 129 and Security 4688. These `RunContext` values are supplied by the caller and are not independently read from the Windows host or authenticated by this module; the fake fixture verifier does not establish native evidence. `BEGIN IMMEDIATE` plus `synchronous=FULL` serializes writes; every phase and observation digest is checked when read. A portable race fixture replaces the leaf path between pre-open inspection and SQLite open; file identity rechecking rejects the substitution before any query or mutation. Identity is checked again after use. This still cannot stop a privileged attacker with ancestor write access from swapping the path and restoring it between checks, so native use requires protected parent ACLs and actual Windows qualification. Interrupted or partial checkpoints require manual reconciliation; no phase is automatically replayed. `simulate_fixture_plan` exercises ordering and interruption decisions using synthetic outcomes without creating native state or mutating the host. The native authority adapter always raises `P028_NATIVE_AUTHORITY_UNAVAILABLE`; even a synthetic observer plus a passing synthetic verifier only leaves `VERIFIED_PENDING_AUTHORITY`. This is a pending checkpoint store, not a trusted ACL boundary or authenticated record: hashes detect accidental changes, not an administrator forging both record and digest.

The adapter contract keeps `Observer`, `IndependentVerifier`, and `TrustedPhaseAdvancer` separate. The only installed native advancer is `NativeAuthorityUnavailable`; callers cannot inject a synthetic advancer through the coordinator constructor. No event verifier or native authority adapter is enabled yet. `simulate_fixture_plan()` exercises ordering and interruption decisions with explicitly synthetic outcomes; it creates no native state, writes no journal and performs no host mutation. Its `FIXTURE_ONLY_*` result never unlocks native advancement.

`orchestration_plan()` lists the required sequence `PREFLIGHT → CANDIDATE_VERIFIED → ENVIRONMENT_QUALIFIED → INSTALLED → GUI_VERIFIED → IPC_VERIFIED → STARTUP_CONFIGURED → LOGOFF_OBSERVED → NEW_LOGON_OBSERVED → LOGON_TRIGGER_VERIFIED → LIFECYCLE_VERIFIED → CLEANUP_VERIFIED → EVIDENCE_FINALIZED` and references existing candidate, environment, ordinary-user Setup/GUI, IPC, task, logon/logoff, uninstall, cleanup and finalization tools. It is a decision plan only: all stages remain `NOT_RUN`, it executes no script, and host-mutating install, lifecycle/uninstall and account cleanup steps require explicit approvals. There is no end-to-end resume runner. Native lifecycle steps that started before interruption require a new independent inspection; installation and uninstall are never replayed automatically. Cleanup and finalization remain unimplemented as independently authenticated evidence transitions.

### A10 local validation and limitations

The following checks ran on this Linux workspace:

- `python3 scripts/test_windows_native_lifecycle.py`: 19 passed, portable journal/contract fixtures only, including fixture decision simulation, LogonTrigger context mismatch cases, simulated leaf substitution, parent reparse rejection and interrupted checkpoint recovery.
- `python3 scripts/test_windows_native_provenance.py`: 12 passed, synthetic event/session data only; `python3 scripts/test_windows_protected_state_contract.py`: 17 passed, source wiring only; `python3 scripts/test_windows_native_coordinator.py`: 12 passed, fixture coordinator.
- `python3 scripts/test_windows_native_evidence.py`: 50 passed; `python3 scripts/test_windows_native_phase_ledger.py`: 4 passed; `python3 scripts/test_windows_lifecycle.py`: 6 passed.
- `python3 scripts/validate-windows-installer.py`, `python3 scripts/validate-windows-native-acceptance.py`, `python3 scripts/validate-installer-security-hardening.py`, and `./scripts/validate-install-acceptance.sh`: passed. The install acceptance suite ran 21 contract tests and explicitly says native execution was not performed.
- Portable PowerShell 7.7 parsed 13 acceptance scripts and compiled the C# declarations. `scripts/test-windows-native-logon-task.ps1` passed 61 synthetic task/process/schema cases. No Windows P/Invoke was executed.
- `scripts/test-windows-protected-acl-contract.ps1` could not run on Linux because .NET Windows Principal APIs throw `PlatformNotSupportedException` there; this is an OS limitation, not a passed test. The hosted Windows ACL job remains necessary and is still only an in-memory descriptor contract.
- `git diff --check` passed. `cargo` is not installed locally, so the Rust example and IPC tests were not run. Candidate build/reproducibility, installer security CI, Windows effective access, token separation, event log, Task Scheduler, WTS, process creation and cleanup were not run locally.

Exact-head A10 CI is pending the push. The last reviewed workflow evidence is on e325770 and is historical only; consult exact new-head workflow runs before reporting hosted outcomes. Preserve the hosted interactive blocker.

| Gate | A10 status | Current evidence and exact blocker |
| --- | --- | --- |
| `P028_ACL_CONTRACT_PASS` | PASS, scoped to prior hosted descriptor check; new exact-head CI pending | In-memory descriptor contract only, not actual NTFS access. |
| `P028_PROTECTED_STATE_IMPLEMENTED` | BLOCKED | A real-token harness is implemented but unexecuted; effective ACL, write-side race resistance and native interruption recovery are unqualified. |
| `P028_TRUSTED_OBSERVER_IMPLEMENTED` | BLOCKED | Exact 4624/4634/4688 and task instance re-query exist, but 119 cannot identify an AuthenticationId and mapped-image bytes are not proven. |
| `P028_COORDINATOR_IMPLEMENTED` | BLOCKED | Durable pending-journal contracts exist; no independently trusted native verifier/advancer or native crash-recovery qualification. |
| `P028_HARNESS_IMPLEMENTED` | BLOCKED | All phases appear in a `NOT_RUN` plan; lifecycle calls, independent cleanup verification, resume, and finalization are not implemented. |
| `P028_NATIVE_ACCEPTED` | BLOCKED | No approved Windows 11 AMD64 interactive end-to-end run or independent cleanup evidence. |

The current independent blockers remain P027 strict Clippy at `crates/server-storage/src/wizard.rs:1098`, Rust dependency advisories for unmaintained `chrono` and `chrono-tz`, the P027 SQLx security review, and P036 signing. They were not changed by A10.

### A10 handoff

Run the opt-in three-role harness on the approved disposable machine and review its redacted record; qualify actual write-denial, ACL inheritance, nested membership, cleanup and crash behavior. Resolve or preserve the privileged path race. Use real automatic and manual trigger runs to evaluate 119 alongside exact 4624/4634/4688 records and task instance/process identities; keep blocked unless the causal relationship is independently established. Resolve mapped-image provenance or keep the executable gate blocked. Then implement protected observer/verifier adapters and authority-owned ordered transitions, integrate the 13-phase ordinary-user lifecycle, require independent cleanup before finalization, and execute complete interactive qualification. Retain `P028_NATIVE_AUTHORITY_UNAVAILABLE` until all these prerequisites pass.

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

## P028-A12 — Windows CI repair and native-readiness audit

Recovery started from live PR head `2b36b6a0374439c2532f538c0632248dd631be07`.
Remote refs and PR #189 matched; both worktrees were clean. Preserved
`/workspace/Synveil` on original `work` (`604499f`) and continued only in
`/workspace/Synveil-p028` on `feat/v03-p028-windows-native-acceptance`.
PR #189 remains Open, Draft and unmerged; no reset or force-push.

### Confirmed CI regression and repair

Exact A11 [Windows run #44669821473](https://github.com/nghianguyen150612/Synveil/actions/runs/44669821473)
failed `build-windows-candidate` / `Validate checkpoint wiring` in
`test_real_adapter_argument_contracts`: `ADAPTER_PATH_INVALID`. The fixture's
`Path('/candidate/setup.exe')` is absolute on POSIX but root-relative without a
drive on Windows. The production `CandidateInputs.validate()` was correct and
is unchanged. Commit `b62bcbcbd68d8689ff55c454c568bff5511c34d5` uses native
absolute paths beneath `TemporaryDirectory`, tests every input field against
relative paths, string impostors and NUL/CR/LF, checks POSIX/Windows rooted-path
semantics, and checks actual argv values. No fictional drive letter is used.
The suite runs on Windows in the existing checkpoint-wiring step. A12 also
compares supplied/required argument names with the real acceptance scripts'
parameter blocks, without invoking those scripts.

### Actual implementation classification

| Component | Source status | Qualification and missing executable behavior |
| --- | --- | --- |
| Protected PowerShell controller | Partial implementation | Role/ACL/ancestor checks, bounded handle reads, mutex and flushed writes exist. Native Advance deliberately throws; no qualified authority-owned resumable executor or monotonic rollback protection. |
| Security observer / scheduler module | Implemented diagnostics, not native-qualified observer authority | Exact 4624/4634/4688 and 119/100/129/200 re-query, task definition/instance, WTS and process-token checks exist. Records are OBSERVED, not phase authorization. Causality and mapped-image guarantees remain blocked. |
| NativePendingJournal / NativeLifecycleCoordinator | Partial implementation | Durable pending observations and verifier receipts; cannot authorize any native phase. Fixture observer/verifier injection only exercises pending state. |
| FixtureExecution | FIXTURE_ONLY, portable tested | Ordered durable execution and unknown-outcome reconciliation work with trusted synthetic dependencies. Not protected authority, cannot resist malicious whole-file rewrite/rollback, and must never become the native state store. |
| Lifecycle adapters | Partial implementation | Four real command builders with strict inputs; no execution-token enforcement behind a qualified native launch boundary. Candidate/startup/event tools remain separate; full native orchestration is absent. |
| Three-role access harness | Implemented but not natively qualified; incomplete overall qualification | Real token/access-check and allowed/denied OS operations exist for a unique temporary ACL tree. No ancestor substitution stress or interrupted protected checkpoint qualification. Does not implement lifecycle acceptance. |
| Cleanup inspector | Partial native diagnostic | CIM account/profile/PID, literal task and exact path queries exist. No authority-owned complete discovery; secrets, retained protected state and pending effects remain UNKNOWN. Always CLEANUP_BLOCKED. |
| Qualification wrapper | Executable opt-in role entrypoint, not qualified | Machine UUID digest plus explicit external approval and existing protected marker required. Neither alone proves disposability. It runs role probes only; native recovery/complete cleanup remain NOT_RUN. |

Audit of intended trust boundaries: controller-owned protected state exists in
source; subject and observer have separated inbox descriptors and cannot submit
phase authority. The verifier re-queries OS records. Fixture/native filenames,
modes and schemas differ and cannot be used interchangeably. A7–A11 regressions
retain forged observer/verifier, context substitution, corrupted journal,
interruption, destructive replay, cleanup and premature-finalization rejection.
Restart does not reinitialize an existing fixture file, including FAILED,
CANCELLED and EXPIRED runs (new reopen tests). None of this establishes real
NTFS isolation or native recovery on an unqualified host.

A12 found a concrete cleanup bug: the role harness ignored a false
`WaitForExit(5000)` after `Kill`. `Stop-P028RoleProbe` now requires observed probe
exit; the finally block preserves the unique ACL tree if any probe exit is
unknown, reports BLOCKED and requires manual reconciliation. Three inert process
fault cases test already-exited, confirmed-exit and unknown-exit behavior. This
does not establish that all possible descendants or other harness processes have
exited; that remains an independent native cleanup responsibility.

The existing hosted read-only Windows smoke test now requires wrong-SID rejection
for controller/observer/subject APIs and, when elevated, rejection of that actual
token for both ordinary roles. It does not provision users, adjust permissions,
launch a task, install software or sign out. A pass is current-token rejection
coverage only, not three-role NTFS qualification.

### Supported provenance limits

Reviewed Microsoft documentation:

- [IRunningTask](https://learn.microsoft.com/en-us/windows/win32/api/taskschd/nn-taskschd-irunningtask)
  exposes instance GUID, engine PID, action/name/path/state. It does not expose
  the authentication LUID of the logon that caused the task.
- [ILogonTrigger](https://learn.microsoft.com/en-us/windows/win32/api/taskschd/nn-taskschd-ilogontrigger)
  configures the trigger's user and delay; it is not a record binding a particular
  invocation to a particular Security 4624. Event 119's implemented schema has
  TaskName/UserName/InstanceId, not AuthenticationId or a Security record reference.
  Event 4688 plus a live token can bind the launched process's LUID; that is not
  independent proof of the triggering session's causality. Timestamp adjacency
  and absence of another observed logon do not supply the missing relationship.
- [QueryFullProcessImageName](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-queryfullprocessimagenamea)
  returns an image name. [GetMappedFileName](https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-getmappedfilenamea)
  returns a mapped file's name for an address. Neither documents a content
  attestation of the original executable mapping or a backing-file identity
  handle suitable for comparison with the candidate. Hashing a newly opened file
  with matching FILE_ID_INFO proves the inspected disk file, not original mapped
  bytes. Hashing loaded memory is not a disk-image hash (relocations/imports and
  mutable pages differ). No supported complete mapped-image proof was established
  in this checkpoint. No driver, unsupported query or signing-policy change added.

Existing narrower file/token/event checks and explicit
`BLOCKED_TASK_EVENT_HAS_NO_AUTHENTICATION_LUID`,
`BLOCKED_MAPPED_IMAGE_PROVENANCE_UNAVAILABLE` and
`P028_NATIVE_AUTHORITY_UNAVAILABLE` remain unchanged. This is a documented gap in
the implemented evidence path, not a claim that every future Windows facility is
incapable of offering stronger evidence.

### Cleanup discovery and qualification boundary

| Resource | Existing independent query / remaining requirement |
| --- | --- |
| Disposable account | Query exact independently recorded SID with Win32_UserAccount; ordinary subject token and ownership must be qualified. |
| User profile | Win32_UserProfile for recorded SID plus separate inventory of actual profile directory; query errors are UNKNOWN. |
| Installed components | Exact inventory path queries only; installation registration, component completeness and candidate ownership still require independent enumeration. |
| Test tasks | Enumerate Get-ScheduledTask and compare literal TaskPath+TaskName; protected pre-effect inventory must establish ownership. |
| Harness processes | Win32_Process for inventoried PID; present/reused PID blocks conservatively. This does not discover escaped/uninventoried descendants. Record creation identity before effects and independently reconcile descendants. |
| Temporary filesystem resources | Get-Item literal path and accessible parent check. Protected ancestors/no-reparse race qualification still required; path-only diagnostic absence is not authority. |
| Temporary credentials | UNKNOWN. No credential enumerator exists. A future scoped CredEnumerate query under each actual role could check declared stored targets only; it cannot prove all transient OS/in-memory secret copies erased. Dispose credential holders and revoke approved disposable accounts; record the precise remaining guarantee. |
| Protected evidence/state | UNKNOWN. Independently reopen and verify retained audit ACL/identity/content and enumerate transient state; fixture cleanup receipts are insufficient. |
| Pending effects/interruption records | UNKNOWN. Requires authority-owned protected operation journal plus fresh OS reconciliation and quiescence; absence of a file or script exit code is insufficient. |

A12 intentionally adds no fake native cleanup success or secret enumerator.
Unverifiable resources block finalization. No native host mutation was attempted.

### Validation executed in this Linux workspace

All following commands exit 0; tests are portable/synthetic unless stated:

| Command | Result |
| --- | --- |
| `python3 scripts/test_windows_lifecycle_execution.py` | 17 tests |
| `python3 scripts/test_windows_native_lifecycle.py` | 19 tests |
| `python3 scripts/test_windows_native_provenance.py` | 12 tests |
| `python3 scripts/test_windows_protected_state_contract.py` | 17 tests |
| `python3 scripts/test_windows_native_coordinator.py` | 12 tests |
| `python3 scripts/test_windows_native_evidence.py` | 50 tests |
| `python3 scripts/test_windows_native_phase_ledger.py` | 4 tests |
| `python3 scripts/test_windows_lifecycle.py` | 6 tests |
| `python3 scripts/validate-windows-installer.py` | PASS |
| `python3 scripts/validate-windows-native-acceptance.py` | PASS, native checkpoint blocked |
| `python3 scripts/validate-installer-security-hardening.py` | PASS |
| `bash scripts/validate-install-acceptance.sh` | PASS, 21 tests |
| `/tmp/p028-pwsh/pwsh -NoProfile -File /tmp/p028-a11/parse.ps1` | PowerShell 7.5.4: 29 scripts parsed, observer native declarations compiled; no Windows API calls |
| `/tmp/p028-pwsh/pwsh -NoProfile -File scripts/test-windows-native-logon-task.ps1` | 66 synthetic cases; Windows-only token branch NOT_RUN locally |
| `git diff --check` | PASS |

137 focused Python tests plus 21 install contracts. Cargo unavailable locally;
Rust checks NOT_RUN here. Existing Windows CI runs Rust IPC probe/contracts,
candidate reproducibility, PowerShell parser and the token/provider smoke checks.
The hosted interactive-surface blocker is untouched. Exact final-head run links
and statuses are recorded on PR #189 after push; an in-progress job is not PASS.
The initial repair run is [#44670557965](https://github.com/nghianguyen150612/Synveil/actions/runs/44670557965)
at `b62bcbc`; ACL passed while candidate wiring was still pending at this audit.

No approved interactive Windows 11 AMD64 host/approval was supplied. Genuine
three-role, NTFS, recovery, GUI/logoff/logon and full cleanup qualification remain
NOT_RUN. Independent P027 Linux strict Clippy (`wizard.rs:1098`), Rust dependency
policy (`chrono`, `chrono-tz`), P027 SQLx security review and P036 signing remain
open and outside this change.

### Executable approved-machine handoff

External operator requirements: approved disposable Windows 11 AMD64 allocation,
explicit scope authorizing the role harness's unique ACL-tree creation, token
processes and confined cleanup; elevated unrestricted controller; separately
provisioned non-elevated Event Log Readers observer (`synveil_obs_<8hex>`) and
ordinary disposable subject (`synveil_p013_<8hex>`, required P013 description).
No extra/nested privilege groups are accepted. Provisioning itself requires
separate explicit authorization; this command does not create accounts.

Use a trusted pinned checkout. Authenticate Setup and release-manifest digests and
source independently, and record the checkout commit and machine UUID digest in
the external approval. Prepare the existing controller-only ProgramData Synveil
P028 directory and protected marker exactly as the A10 harness requires, only
within approved scope. A marker is an additional guard, never approval by itself.
In an elevated PowerShell 7 session, fill the variables from that approval record:

```powershell
# Values must come from the externally reviewed allocation/candidate record.
$observerCredential = Get-Credential -UserName $approvedObserverAccount
$subjectCredential = Get-Credential -UserName $approvedSubjectAccount
$parameters = @{
    OperatorApproval = 'I_APPROVE_P028_DISPOSABLE_MACHINE_ROLE_QUALIFICATION'
    ApprovedMachineUuidSha256 = $approvedMachineUuidSha256
    EvidenceDirectory = $approvedProtectedEvidenceDirectory
    CandidatePath = $authenticatedSetupPath
    CandidateSha256 = $authenticatedSetupSha256
    ManifestPath = $authenticatedManifestPath
    ManifestSha256 = $authenticatedManifestSha256
    ExpectedSourceCommit = $authenticatedSourceCommit
    ObserverCredential = $observerCredential
    SubjectCredential = $subjectCredential
}
& ./scripts/qualify-windows-native-lifecycle.ps1 @parameters
```

Expected evidence: protected `role-access-<nonce>.json`, candidate/manifest and
harness component identities, machine/controller hashes, role checks and scoped
ACL-tree cleanup status. Independently review actual role tokens, OS allowed and
denied operations and tree absence. A result here cannot qualify full acceptance.
If a probe cannot be confirmed exited, retain its tree and reconcile that exact
recorded process/tree manually; do not rerun cleanup blindly. Keep credentials out
of command lines/logs; dispose credential holders after the approved operation.
The operator owns explicit revocation/account/profile cleanup and independent
checks, including resources outside the role harness's unique temporary tree.

Only after role evidence review: qualify protected ancestor/race and interrupted
write recovery on this approved allocation; resolve or retain the two provenance
blockers; implement the missing protected native transition and cleanup behavior;
then separately approve the full interactive lifecycle. No real logoff, task
execution or installation command is authorized by merely running role probes.

| Gate | A12 disposition |
| --- | --- |
| `P028_ACL_CONTRACT_PASS` | Evaluate from exact final-head hosted in-memory contract, linked on PR; not NTFS qualification. |
| `P028_PROTECTED_STATE_IMPLEMENTED` | BLOCKED: real-token NTFS, write races and protected monotonic recovery unqualified. |
| `P028_TRUSTED_OBSERVER_IMPLEMENTED` | BLOCKED: trigger causality and mapped-image proof remain unsupported in the implemented path. |
| `P028_COORDINATOR_IMPLEMENTED` | BLOCKED: no secure enabled native transitions/recovery. |
| `P028_HARNESS_IMPLEMENTED` | BLOCKED: full native adapter orchestration and independent cleanup discovery incomplete. |
| `P028_NATIVE_ACCEPTED` | BLOCKED: approved interactive end-to-end execution NOT_RUN. |

A12 final failure-path follow-up: an inert PowerShell child exiting 2 confirmed
that the caller can continue and exit 0. The role harness now explicitly exits 0
on success, and the qualification wrapper rejects nonzero child exit before
reporting any result. Two portable cases execute the real wrapper invocation and
exit guard against inert temporary child scripts, never the native harness.
The scheduler/cleanup suite now has 66 synthetic cases. At source head
`5d8b0bd1055a4350db27ab755a8a55b9014a0b47`, [Windows #44671032128](https://github.com/nghianguyen150612/Synveil/actions/runs/44671032128)
passed checkpoint wiring (04:00:55 UTC), PowerShell parser and ACL/scheduler job.
That verifies the path repair on Windows. The final failure-path follow-up needs
its own exact-head CI inspection; final links/results remain tracked on PR #189.

## P028-A13 — native qualification and release gate assessment

Decision: **BLOCKED_NATIVE_ENVIRONMENT_UNAVAILABLE**. Native authority BLOCKED;
all 13 native phases NOT_RUN. Security guarantees and native implementation also
remain unresolved; obtaining a host alone will not close P028.

### Recovery and environment

On 2026-11-20, the available `/workspace` was a fresh Linux x86_64 workspace.
Neither `/workspace/Synveil` nor `/workspace/Synveil-p028` existed; no local Git
worktrees, modifications or original `work` branch were available to recover.
Nothing at those prior locations was deleted or reset. Cloned the existing PR
branch into `/workspace/Synveil-p028`. Remote branch, PR ref and PR metadata all
matched starting SHA `fb0ee55c704e61a4e387598e93aafd39eb3fd1e7`; no newer remote
commit existed. Base remains `develop/v0.3` at `b8beb109e58c847cb136607e4948c564cf6d2aa7`.
The clone was clean with one worktree. No AGENTS.md was found. PR #189 was Open,
Draft and unmerged. No replacement PR, reset, force-push or merge was performed.

The connected Windows terminal returned `tunnel_client_not_seen` (HTTP 404,
client absent for 300 seconds) before executing a read-only OS/location query.
Its OS, files and tokens could not be inspected. This is unavailable connectivity,
not evidence of Windows 11 capability. No exact disposable host, operator scope,
three role identities, interactive session, recovery snapshot, auditing/event
permissions or lifecycle-operation approval was supplied or verified. None is
inferred from CI, administrator access, machine identifiers or marker files.
No Windows mutation or native qualification was attempted.

### Completed exact-source CI inspection

[Redacted evidence index](P028_A13_EVIDENCE.json) records **35/35** exact-SHA runs,
including push runs omitted by the PR-only connector. Actions REST listing used
`head_sha=fb0ee55c704e61a4e387598e93aafd39eb3fd1e7&per_page=100`; total count
matched returned count. Every run's job count matched its retrieved jobs. Failed
job logs were read; unlike the earlier checkpoint, downloads succeeded. No raw
logs, credentials, machine identities or event XML are published in the index.

- [Windows PR #44672582317](https://github.com/nghianguyen150612/Synveil/actions/runs/44672582317)
  and [push #44672574186](https://github.com/nghianguyen150612/Synveil/actions/runs/44672574186):
  ACL/scheduler and candidate jobs SUCCESS, including checkpoint wiring, all 17
  lifecycle execution tests, native lifecycle/provenance/protected-state/coordinator/
  evidence/ledger contracts, Rust IPC probe/contracts, install contracts,
  PowerShell parser, runtime compilation, double-build byte identity, candidate
  identity and upload. The A11 Windows ADAPTER_PATH_INVALID regression remains fixed.
  Standard-user candidate authentication and bounded product acceptance SUCCESS;
  the only failed step is `Record required unavailable surfaces honestly`.
  Logs identify Windows Server 2025 service sessions, not an approved Windows 11
  desktop. Both workflows are FAILURE by the required environment blocker.
- [Windows installer #44672582260](https://github.com/nghianguyen150612/Synveil/actions/runs/44672582260)
  and push #44672574178 SUCCESS; all jobs in installer security #44672582290 and
  installation resilience #44672582236 SUCCESS. These do not qualify native isolation.
- Rust CI #44672582302 and push #44672574087: platform tests/checks, quality, UI
  and web SUCCESS; dependency policy FAILURE. Logs confirm unmaintained `chrono`
  RUSTSEC-2026-0543 and `chrono-tz` RUSTSEC-2026-0555. Independent dependency/security
  blockers; no policy or dependency change is authorized by this assessment.
- P027 #44672582320: Windows compilation/contracts SUCCESS and Linux live database
  suites SUCCESS; Linux aggregate quality failed `wizard.rs:1098` needless_bool.
  Windows database setup rejected the runner tools with P027_PG_VERSION_FAILURE
  (reviewed 17.11 required); artifact upload then failed because no evidence files
  existed. Actual installed PostgreSQL version was not printed. Classify as test/
  environment incompatibility, retaining the version guard. P015, P017–P026 and
  repair/recovery quality jobs show the same independent wizard.rs Clippy defect;
  the evidence index enumerates each failed job, not just a representative sample.
- Five Ubuntu desktop setup jobs failed because `qml6-module-qtquick-nativestyle`
  is unavailable on their Ubuntu 26.04 image: environment/package incompatibility.
  No P028 code regression is established by these setup failures.
- Linux packages PR #44672582329 built packages but APT and DNF acceptance exited
  50 (package-manager failure). Logs establish the exit and step, not the detailed
  package-manager cause; classify as independent qualification failures with root
  cause unresolved. Do not guess an authorization failure from the preceding
  “authorization will be requested” message. Push #44672574102 was cancelled;
  downstream acceptance was skipped, not PASS (infrastructure/cancellation).
- Scheduled-maintenance push #44672574139: 19 tests passed, one failed.
  `scheduler_failure_prevents_worker_step` at
  `backup_scheduled_maintenance_cycle_postgres.rs:1656` could not construct its
  corrupt-revision fixture because `backup_schedules_effective_time_order` rejected
  it (SQLSTATE 23514). Independent test incompatibility; determinism/root cause
  beyond that constraint rejection is unqualified. Its PR counterpart succeeded.

The index identifies every remaining successful workflow as well. None of these
independent failures was repaired in this PR. Independent P027 SQLx security review
and P036 signing remain open; skipped signature recording is not signing evidence.
Candidate bytes/digests were not downloaded or independently authenticated here.
CI's candidate identity success is a hosted check, not a native candidate qualification.

### Source audit, unsupported guarantees and architectural decision

A12's component and cleanup classification above remains accurate on recovered
HEAD. Rechecked actual native launch, pending coordinator, protected controller,
role qualification and cleanup code. `launch()` invokes the closed authority guard
before process creation; protected `Advance` still throws. The native pending
journal cannot authorize phases. The executable fixture engine's ordering,
evidence-before-commit, expiry/terminal rejection, interruption/reconciliation and
cleanup protocol are portable contracts, not Windows authority or rollback protection.
No fixture record was converted to native authority.

The existing observer implements exact event re-query, strict schema/context checks,
Security 4624/4634/4688, Scheduler 119/100/129/200 with manual/time-trigger conflicts,
live SID/LUID/WTS/task/process and inspected-file identity checks. No target OS
schema or actual event was qualified in A13. Event 119's task instance and user do
not independently bind the *causing* Security authentication LUID. Process token
LUID establishes execution context, not the cause. The supported executable
statement remains “the inspected on-disk file matched the baseline identity/hash
at the observations”; it does not attest original mapped image bytes or defeat
replacement-and-restore/in-place-write races at process creation.

A possible session-notification broker would be a different architecture, not a
minimal event-parser correction. [WTSRegisterSessionNotification](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsregistersessionnotification)
reports session changes, not a Scheduler-instance-to-4624 causal receipt.
[WTSQueryUserToken](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsqueryusertoken)
requires a highly trusted LocalSystem caller with SeTcbPrivilege; it is not a
least-privilege drop-in for the existing ordinary observer. Neither supplies original
mapped-image content attestation. No supported alternative satisfying all six
requested constraints has been established. Do not add a persistent privileged
service or silently substitute “runs in the new session” for causal attribution.
Required architectural decision: independently review a supported design with
explicit causal and executable provenance guarantees under the existing trusted
kernel/admin boundary, or retain the blocked guarantee. No alternative implemented.

For cleanup, no run resources were created or independently inspected on Windows.
All nine classes (account, profile, installation, tasks, harness processes,
temporary filesystem, credentials, protected state/audit retention, pending effects)
are NOT_RUN with actual presence UNKNOWN. The existing inspector queries exact
SID/PID/task/path references but does not prove complete discovery. Credentials,
protected retention and pending operations remain UNKNOWN by construction.
Authorized retained audit evidence must be reopened and checked for identity,
content and ACLs; it cannot be removed to obtain a clean result. Script exit status
cannot authorize finalization. No cleanup or destructive action was replayed.

### A13 local regression and handoff

Executed on Linux: `python3 scripts/test_windows_lifecycle_execution.py` (17),
`test_windows_native_lifecycle.py` (19), `test_windows_native_provenance.py` (12),
`test_windows_protected_state_contract.py` (17), `test_windows_native_coordinator.py`
(12), `test_windows_native_evidence.py` (50), `test_windows_native_phase_ledger.py`
(4), `test_windows_lifecycle.py` (6): **137 tests PASS**. All names after the first
use the same `python3 scripts/` prefix. Three existing source validators
(`validate-windows-installer.py`, `validate-windows-native-acceptance.py`,
`validate-installer-security-hardening.py`) PASS; `bash scripts/validate-install-acceptance.sh`
PASS with 21 tests. These are source/portable evidence only. Local Cargo and
PowerShell are unavailable; Windows parser/declaration/token/scheduler results above
come from exact-source hosted jobs, not a local execution claim.

No new implementation defect was confirmed, so no code or artificial regression
suite was added. Corrected the older handoff's misleading SOURCE_READY wording and
clarified that its component commands cannot finalize native security acceptance.
A13 changes evidence documentation only. Final documentation-commit CI must be
inspected separately; prior-SHA checks are never relabelled exact-final-head PASS.

Use the A12 **Executable approved-machine handoff** above, with its exact wrapper
parameters and credential prompts, after external approval binds the disposable
Windows 11 AMD64 UUID, snapshot/recovery, candidate/manifests/harness commits,
controller/observer/subject identities, audit policy and allowed role-test mutations.
The first executable step is scoped role qualification only. Review real token and
NTFS allowed/denied operations independently; test inheritance/nested groups,
reparse/ancestor/handle substitution and interrupted protected writes. Any unexpected
access fails qualification. Preserve uncertain processes/tree for reconciliation.
Resolve provenance and protected authority/cleanup prerequisites before independently
approving installation, interactive GUI/IPC, startup, real sign-out/sign-in,
lifecycle/uninstall and finalization. No full secure native runner exists to invoke
now; the older component-command sequence cannot replace it. Stop at the first
unqualified phase, preserve protected audit evidence, never backfill results.

| Gate | A13 assessment at inspected source SHA | Minimum missing evidence |
| --- | --- | --- |
| P028_ACL_CONTRACT_PASS | PASS, hosted in-memory contract at fb0ee55; documentation-head rerun required | Exact-final-head Windows contract; never effective NTFS proof. |
| P028_PROTECTED_STATE_IMPLEMENTED | BLOCKED | Three real tokens, effective NTFS/ancestor/write/handle attacks and protected persistence/recovery qualification. |
| P028_TRUSTED_OBSERVER_IMPLEMENTED | BLOCKED | Target schema/event qualification, independent causal session binding and supported original executable provenance. |
| P028_COORDINATOR_IMPLEMENTED | BLOCKED | Qualified protected native authority, monotonic durable transitions and uncertain-effect recovery. |
| P028_HARNESS_IMPLEMENTED | BLOCKED | Trusted native adapter execution/resume and complete independent nine-class cleanup. |
| P028_NATIVE_ACCEPTED | BLOCKED / NOT_RUN | Approved Windows 11 AMD64 interactive end-to-end evidence with independent cleanup/finalization. |

No closeout candidate exists. Minimum next action: externally allocate and approve
the exact disposable interactive Windows host for scoped role qualification while
independent review resolves the two provenance guarantees and missing native authority.
Keep PR #189 Open, Draft and unmerged, and preserve v0.2 behavior and every guard.
