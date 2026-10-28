# P013 — Windows native acceptance qualification

**Status: source harness expanded; native acceptance blocked.** P013 does not
qualify P028. The current workspace is Linux and has no Windows desktop,
interactive logon, installed candidate, or Windows named-pipe endpoint. Source
checks and parser validation do not stand in for GUI, genuine-logon,
installed-component IPC, cross-user authorization, or cleanup results.

## Preflight and scope

- Starting `origin/develop/v0.3`: `f79b302aebebd038211635e00fb39eaca7d67b23`
  (the requested expected base SHA).
- Starting `origin/main`: `604499fce0dfd134d82da3d5bb5d4a055c5eea8d`;
  P013 did not modify it.
- P006 #165, P006 formatting repair #167, and P007–P012 (#166, #168–#172)
  are integrated. PR #172 merged from source head
  `27c38e3ec23b90dc0eb7f2a2a8191b311cc04552`.
- The approved `Synveil-v0.3-Context-Roadmap-P001-P060.md` was not found in
  the repository or available workspace search. P013 is a provisional
  remediation milestone and does not replace that roadmap.
- P012 source-head SHA `27c38e3…` has 91 check runs: 87 passed, two were
  skipped, and two `standard-user-native` runs failed at the explicit hosted
  environment blocker. Native desktop UI (Linux and Windows Qt 6) completed
  successfully. Candidate builds passed
  in push run [40614014942](https://github.com/nghianguyen150612/Synveil/actions/runs/40614014942)
  and PR run [40614021598](https://github.com/nghianguyen150612/Synveil/actions/runs/40614021598).
  Both native-consumer failures were the explicit
  `Record required unavailable surfaces honestly` blocker step.
  Post-merge SHA `f79b302…` has 24 checks: 23 passed, the same
  `standard-user-native` blocker step failed in
  [40614102098](https://github.com/nghianguyen150612/Synveil/actions/runs/40614102098).
  Linux and Windows Qt 6 completed successfully. Neither hosted service job is
  treated as GUI, logon, or installed IPC acceptance.

## Added source components

- `scripts/validate-windows-native-evidence.py` defines the version 1,
  fail-closed acceptance record. It requires complete candidate provenance,
  OS/build/architecture and harness identity, user privilege, independent
  native session observations, matching real-logon and task-trigger
  observations, each scenario status, redaction assertions, and independently
  verified cleanup. `PASS` requires every required scenario to pass on a
  qualified isolated Windows 11 x86-64 environment with a standard-user,
  non-elevated token. Missing results remain `NOT_RUN` or `INCOMPLETE`.
- `scripts/qualify-windows-native-environment.ps1` is a read-only,
  operator-approved qualification collector. It rejects CI/hosted use,
  requires explicit disposable-environment approval, checks the exact Setup
  and release-manifest hashes and sizes, and independently samples the active
  input desktop, WTS session state, console-session match, architecture,
  Windows build, marked P013 local test-account identity, and token elevation.
  It records only a one-way user SID hash and a session ID. It does not start
  Setup, register or run a task, touch the v0.3 Wizard, or change machine state.
- `scripts/test-windows-gui-journey.ps1` is a separate operator-driven
  standard-user harness for the existing Setup executable. It verifies exact
  candidate and release-manifest identity, rejects hosted/headless/elevated
  sessions, and launches Setup without silent flags. Windows UI Automation
  observes Welcome, license/options, install-location, confirmation,
  installation-progress, and completion states while the operator advances
  the visible wizard. It reads the startup, desktop-shortcut, and post-install
  launch selections and compares them with the supplied expected choices. It
  validates the installed runtime manifest, launches the installed desktop in
  the same user session, checks its first-run surface and process identity,
  verifies the installed client is running as the same user, and observes the
  desktop's `Ready` state after its production controller connects to the
  installed client over IPC. Setup, desktop, client, and uninstaller tokens
  must remain non-elevated. It invokes the bounded production protocol probe
  while the visible desktop is open. It tests v1 Ping,
  malformed and oversized frames, unsupported version, duplicate connections,
  handshake timeout and cancellation, stale profile, unrelated-user denial,
  server shutdown, endpoint disappearance, and restart of the exact installed
  client. The unrelated-user probe runs under a separate disposable standard
  account only for the ACL test; alternate credentials are never treated as a
  real interactive logon. The harness checks for TCP listeners, closes the
  desktop, and shuts the client down and restarts it under finite timeouts. It
  verifies the install root as `%LOCALAPPDATA%\Programs\Synveil`, then checks
  that the registered uninstaller removes package files and registration while
  preserving a non-secret user-state sentinel byte-for-byte. Its evidence
  classifies native UI Automation state assertions explicitly and records
  `screenshots_status=NOT_CAPTURED`; screenshots do not replace those
  assertions. It does not remove the account or installed profile.
- `crates/client/examples/p013_windows_control_probe.rs` is the native IPC
  probe. It reads the current user's non-secret `client.conf` profile ID and
  resolves the endpoint with `DesktopControlEndpoint::for_profile`; it does
  not guess a pipe name or start a test server. The UI journey compiles this
  probe from the exact expected source commit in a temporary Cargo home and
  target directory, then removes those directories. The owner probe uses
  `DesktopControlClient` and the production v1 handshake/Ping path, then checks
  unsupported protocol version, malformed JSON, a 64 KiB + 1 frame length,
  concurrent duplicate connections, server handshake timeout, and client
  cancellation. Its `wrong-user`, `shutdown`, `absent`, `stale`, and `ping`
  modes support cross-user denial, server termination, endpoint disappearance,
  stale-profile, and restart checks. It starts no mock server, does not guess
  the pipe name, and emits only bounded check statuses. The helper is a test
  client for negative protocol cases; the production desktop's own controller
  supplies the installed-binary legitimate Ping path.
- `scripts/collect-windows-new-logon-evidence.ps1` is a read-only observer.
  It requires the operator's UTC lower-bound timestamp from before the manual
  sign-out/sign-in, matches the installed client's token SID and authentication
  LUID to Security event 4624, accepts only interactive logon types, inspects
  the exact per-profile task XML for an enabled `LogonTrigger`, matching user,
  `InteractiveToken`, `LeastPrivilege`, and the installed client action, then
  correlates the scheduler trigger and task-start events. It never runs the
  task, provisions a user, prompts for credentials, or records raw event data.
  If event channels are unreadable to the standard-user observer, it records
  `BLOCKED_BY_ENVIRONMENT`.
- `scripts/collect-windows-logoff-evidence.ps1` runs later in a separately
  approved administrative observer phase after the disposable user signs out.
  It rebinds the previous logon record to the exact candidate, matches the
  Security logoff event by one-way SID/LUID commitments, checks task completion
  near logoff, and verifies that no Synveil client or desktop process remains
  under that user. It does not launch, stop, or change the product, and never
  writes raw event contents or logon IDs.
- `scripts/provision-windows-p013-test-user.ps1` creates only an explicitly
  named local account with the P013 disposable-account marker. It runs in a
  separate elevated provisioning phase and prompts for a secure password that
  is not written to a file or evidence. Provision one account for the desktop
  owner and a second standard-user peer for the cross-user IPC check.
- `scripts/cleanup-windows-p013-test-user.ps1` is a separate, approved
  administrative phase. It refuses active profiles or product processes,
  removes only the marked test account's exact profile-scoped task, account,
  and profile, then independently verifies their absence. Any mismatch or
  failed removal is reported as `FAIL`; it does not touch an unmarked account
  or a task it cannot bind to the test profile.
- `scripts/test_windows_native_evidence.py` has source-level negative tests for
  mismatched candidate identity, unsupported architecture/build, elevated
  token claims, false interactive-session claims, alternate credentials,
  manual task runs, hosted `PASS`, IPC denials/failures, silent Setup
  substitution, cleanup, redaction, timeouts, and every individually required
  acceptance surface.
- `scripts/summarize-windows-native-acceptance.py` binds environment, GUI,
  logon, logoff, and cleanup component records to the exact source, Setup bytes,
  release-manifest digest, and harness revision. Missing component records are
  `NOT_RUN`; every component digest is required before a summary can pass.
- The existing Windows acceptance workflow now runs those evidence-contract
  tests, runs the existing Windows IPC regression suite, checks the Rust probe,
  and parses all P013 PowerShell tools without executing the interactive
  harness.
  Its hosted Windows job still reports
  unavailable GUI/logon/IPC evidence as a blocker and fails closed.

## Existing IPC protocol inspected

The production contract is implemented in `crates/client/src/control.rs` and
documented in ADR-038. The Windows endpoint is derived from the configured
profile ID as `\\.\pipe\synveil-<canonical UUIDv7>`; callers must discover the
profile ID from the installed non-secret profile configuration and use the
application endpoint resolver. P013 never guesses or hardcodes a profile
endpoint. The server uses an owner-only pipe security descriptor
`D:P(A;;GA;;;OW)`, rejects remote clients, and the v1 protocol uses a bounded
length-prefixed JSON frame, a `ControlClientHello { protocol_version: 1 }`
handshake, and `ControlCommand::Ping`. The expected reply is a v1
`ControlServerHello` advertising Ping followed by a `Pong` with the process
status. Frame and connection limits are already defined by the application.

Existing Rust IPC tests exercise the protocol and Windows transport branch
with in-process test server fixtures; they are not production acceptance. The
new probe can test the installed client process through the real protocol while
the installed desktop is open. Its owner-mode named-pipe run, cross-user mode,
shutdown/disappearance/restart modes, and no-TCP assertion have not run in a
qualified Windows environment. Cross-user access must fail with the OS access
denial result; an absent endpoint is not an authorization pass. No mock service
is promoted to acceptance.

## Session, logon, and GUI evidence

The collector rejects GitHub Actions, requires a standard-user non-elevated
token, and checks the input desktop is `Default`, WTS reports the current
process session active, that session has a mapped user, and the current token
SID matches the WTS user SID. It records whether the session matches the active
console, but does not use that as the sole desktop test so an active RDP
desktop can be evaluated. Session 0, a disconnected session, a missing user
mapping, or alternate credentials do not qualify. Explorer presence or a
nonzero session ID alone is never evidence.

The v1 contract requires a Security 4624 event whose target SID and logon LUID
match the installed client process token, an interactive logon type, the exact
Task Scheduler `LogonTrigger` definition, and a correlated scheduler trigger
followed by task start for that task. The collector rejects a manual-run event
in that transition window. A scheduled-task `/Run` invocation is never issued
and cannot satisfy the logon evidence. The new-logon, post-logoff, and cleanup
observers are implemented, but none has run on qualified Windows.

The existing standard-user lifecycle script still uses silent Setup and a QML
smoke switch; those remain scoped installer/runtime checks and are not GUI
acceptance. The new UI Automation and named-pipe probe have not run in this
Linux workspace. Their stage observations, options, UAC behavior, first-run
state, protocol responses, process shutdown, and restart remain unverified
until they run on an isolated Windows 11 desktop. Screenshots alone are
insufficient.

## Candidate provenance and security boundaries

The post-merge P028 candidate from run
[40614102098](https://github.com/nghianguyen150612/Synveil/actions/runs/40614102098)
was retrieved and inspected through the connected GitHub artifact interface.
Its artifact ZIP is 32,391,302 bytes with SHA-256
`ebef2c9433942e98964eccbc2ecff2377dcb6b52c75ec8a0c69d9652c2a4fabf`.
The embedded `p028-candidate.json`, Setup bytes, and release manifest agree:
source commit `f79b302aebebd038211635e00fb39eaca7d67b23`, Setup size 32,932,573
bytes, Setup SHA-256
`b960a1790f6fad522e12f41c1048e25dd079e3a2edcc44aa1c2b894cd96c4416`, and
release-manifest SHA-256
`ed5a9d393e85d025b98444a166c0fd27e8e0b2556c8dc456041d52ade9b32d8c`.
The candidate is unsigned. P013 did not produce new candidate bytes and did
not install or run this candidate on Windows. This verifies artifact byte
identity and manifest binding only; it is not publisher authentication or
native acceptance. P036's production publisher trust remains separate.

The environment collector does not enable the v0.3 Wizard execution path or
create an execution capability, trust root, effect catalog, service, or shell
executor. Product processes must remain standard-user and non-elevated. Account
creation and removal use separate approved administrative phases; product
execution and interactive evidence run under the marked standard-user account.
Passwords are prompted into secure in-memory values and are never placed in
arguments, logs, or evidence. Cleanup must independently verify account,
profile, task, process, and temporary harness-state removal before it can pass.

## Independent gates and handoffs

- **P004:** D02 (installer privilege), D03 (Tailscale transport), D11
  (presentation/effect ownership), and D14 (publisher signing/downgrade)
  remain `RECOMMENDED_PENDING_APPROVAL`. P013 makes none of these decisions.
- **P006:** #165 and formatting repair #167 are integrated, and focused P006
  run [40320179191](https://github.com/nghianguyen150612/Synveil/actions/runs/40320179191)
  passed on `f768474…`. The joined PostgreSQL metadata → ObjectStore
  owner-to-byte authorization test is still missing. P013 does not change
  database or object authorization.
- **P036:** production PostgreSQL 17 and edge artifact identity evidence,
  approved publisher keyring/signing provenance, and Phase C native evidence
  remain unavailable. `production-artifacts.json` was not supplied; fixture or
  schema checks are not publisher trust.
- **P014 handoff:** obtain the exact P028 candidate bytes and manifest; run the
  environment qualifier and visible UI journey in a fresh Windows 11 x86-64
  VM; select `StartupEnabled=true` so the installed LogonTrigger task is
  created, and provide the second disposable peer identity for cross-user
  IPC; perform a manual sign-out/sign-in for LogonTrigger evidence, then a
  second sign-out for the separate logoff observer; run independent cleanup
  for both test
  accounts; validate and retain every component record and exact-head CI.
  Preserve the blocker until every required scenario passes.
  Reconcile the missing approved roadmap and applicable P004 maintainer
  decisions before claiming roadmap completion.

## Current gate

`P028_SOURCE_READY / NATIVE_ACCEPTANCE_BLOCKED`.
The current workspace cannot run Windows tests. Source-level evidence contract
tests do not qualify the native desktop, logon, installed IPC, or lifecycle.
P013 remains blocked and must not publish a readiness marker.
