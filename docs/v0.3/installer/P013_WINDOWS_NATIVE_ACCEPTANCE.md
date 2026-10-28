# P013 — Windows native acceptance qualification

**Status: source contract added; native acceptance blocked.** P013 does not
qualify P028. The current workspace is Linux and has no Windows desktop,
interactive logon, installed candidate, or Windows named-pipe endpoint. The
new evidence validator and environment collector are source-level tools. They
do not stand in for the GUI, genuine-logon, installed-component IPC, or cleanup
results.

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
- P012 has no clean completed exact-head CI rollup at this evidence snapshot.
  On source head `27c38e3…`, Rust quality, Linux Qt, Rust Linux/Windows test
  jobs and Windows installer-security subchecks reported success; the Windows
  Qt job and several other checks were still in progress/queued. The exact
  P028 push run [40614014942](https://github.com/nghianguyen150612/Synveil/actions/runs/40614014942)
  built its candidate successfully and failed `standard-user-native`.
  Post-merge run [40614102098](https://github.com/nghianguyen150612/Synveil/actions/runs/40614102098)
  on `f79b302…` likewise has candidate-build success and
  `standard-user-native` failure. PR run [40614021598](https://github.com/nghianguyen150612/Synveil/actions/runs/40614021598)
  remained queued at the last query. No pending/queued/cancelled job is
  described as passing.

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
  Windows build, and token elevation. It records only a one-way user SID hash
  and a session ID. It does not provision accounts, start Setup, register or
  run a task, touch the v0.3 Wizard, or change machine state. It intentionally
  leaves GUI, named-pipe, genuine-logon, and cleanup scenarios unrun.
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
  then shuts it down under a finite timeout. Evidence contains stage and
  choice assertions, without screenshots, usernames, paths, or UI text dumps.
  Its cleanup result remains `NOT_RUN`; profile/task/machine cleanup is a
  separate required gate.
- `scripts/test_windows_native_evidence.py` has source-level negative tests for
  mismatched candidate identity, unsupported architecture/build, elevated
  token claims, false interactive-session claims, alternate credentials,
  manual task runs, hosted `PASS`, IPC denials/failures, silent Setup
  substitution, cleanup, redaction, timeouts, and every individually required
  acceptance surface.
- The existing Windows acceptance workflow now runs those evidence-contract
  tests and parses both PowerShell acceptance tools without executing them.
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

Existing Rust IPC tests exercise the protocol and Windows transport branch,
including Ping, malformed/unsupported requests, lifecycle, and endpoint
security. They use in-process test server fixtures. They do not establish an
end-to-end connection between the exact installed Synveil desktop/client
binaries, and they do not constitute P013 production IPC acceptance. P013
currently adds the evidence slots and fail-closed requirements for the
installed-binary identity, endpoint discovery, same-user Ping, wrong-user
denial, unsupported version, oversized/malformed messages, stale/expired
endpoint, termination/restart, duplicate connection, cancellation, and no TCP
fallback. Those checks remain `NOT_RUN` until a qualified native harness drives
the real installed components. No mock service is promoted to acceptance.

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
match the current process token, an interactive logon type, and an independently
observed Task Scheduler LogonTrigger event for the exact product task. A
`schtasks /Run` event cannot satisfy it. The current collector does not yet
collect or verify that event chain, so genuine-logon and startup-task results
remain `NOT_RUN`.

The existing standard-user lifecycle script still uses silent Setup and a QML
smoke switch; those remain scoped installer/runtime checks and are not GUI
acceptance. The new UI Automation harness has not run in this Linux workspace.
Its stage observations, option choices, UAC behavior, installed first-run
state, and shutdown remain unverified until it runs on an isolated Windows 11
desktop. Screenshots alone are insufficient.

## Candidate provenance and security boundaries

The P012 candidate producer job passed, but the candidate artifact could not
be downloaded in this workspace (GitHub artifact storage returned HTTP 403),
so its Setup SHA-256, size, and release-manifest digest are not recorded here.
P013 has not produced a candidate. A P013 evidence record must compare the
actual candidate bytes and manifest to the expected identity and source commit;
a checksum match is byte identity only and is not publisher authentication.
P036's production publisher trust remains separate.

The environment collector does not enable the v0.3 Wizard execution path or
create an execution capability, trust root, effect catalog, service, or shell
executor. Product processes must remain standard-user and non-elevated. Any
account provisioning/removal used by a future native driver must be a separate
administrative setup phase; product execution evidence must be collected from
the standard-user session. The harness contract requires redacted bounded
evidence and independent removal/profile/task/state verification before
cleanup can pass.

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
- **P014 handoff:** first complete the installed-component named-pipe driver
  and standard-user authorization cases; run and qualify the new Setup UI
  Automation harness; implement an isolated disposable-user LogonTrigger controller
  with matching native event evidence and verified cleanup; then run the
  candidate-bound harness on a qualified Windows 11 x86-64 physical machine
  or isolated VM. Preserve the failure/block status until every scenario is
  complete. Reconcile the missing approved roadmap and applicable P004
  maintainer decisions before claiming roadmap completion.

## Current gate

`P028_SOURCE_REMEDIATION_INCOMPLETE / NATIVE_ACCEPTANCE_BLOCKED`.
The current workspace cannot run Windows tests. Source-level evidence contract
tests do not qualify the native desktop, logon, installed IPC, or lifecycle.
P013 remains blocked and must not publish a readiness marker.
