# PROMPT013 — Windows native acceptance manifest

**Status: `SYNVEIL_V03_P013_BLOCKED`** — source harnesses are implemented and
the post-merge P028 candidate identity has been verified, but the candidate
has not been run on a qualified Windows 11 environment. Native GUI, IPC,
logon/logoff, and cleanup results remain unverified.

## Repository preflight

| Item | Observed value |
|---|---|
| Starting branch | `develop/v0.3` |
| Starting SHA / expected base | `f79b302aebebd038211635e00fb39eaca7d67b23` |
| `origin/main` | `604499fce0dfd134d82da3d5bb5d4a055c5eea8d` (unchanged) |
| P006–P011 integration | #165, #167, #166, #168, #169, #170, #171 merged |
| P012 PR #172 | merged from `27c38e3ec23b90dc0eb7f2a2a8191b311cc04552` to the starting SHA |
| Current branch | `feat/v03-p013-windows-native-qualification` |
| Approved P001–P060 v0.3 roadmap | Not found; P013 is a provisional remediation milestone |
| Local OS | Linux; no qualified Windows 11 desktop/logon/pipe environment |

## P012 and P028 evidence at preflight

PR #172's source-head SHA `27c38e3…` has 91 check runs: 87 passed, two were
skipped, and two native consumers failed at the explicit
`Record required unavailable surfaces honestly` blocker step. Linux and
Windows Qt 6 completed successfully. Candidate builds passed in source-head runs
[40614014942](https://github.com/nghianguyen150612/Synveil/actions/runs/40614014942)
and
[40614021598](https://github.com/nghianguyen150612/Synveil/actions/runs/40614021598).
Post-merge SHA `f79b302…` has 24 checks: 23 passed, the same native blocker
step failed in
[40614102098](https://github.com/nghianguyen150612/Synveil/actions/runs/40614102098),
Linux and Windows Qt 6 completed successfully. These results do not establish
GUI, genuine-logon, or installed named-pipe acceptance.

| Evidence item | Result |
|---|---|
| P012 source commit | `27c38e3ec23b90dc0eb7f2a2a8191b311cc04552` |
| Post-merge base commit | `f79b302aebebd038211635e00fb39eaca7d67b23` |
| P028 hosted candidate build | `PASS` (candidate build only) |
| P028 hosted standard-user native job | `FAIL`; hosted run does not qualify GUI/logon/installed IPC; exact failing check is retained |
| Post-merge P028 candidate | Retrieved from run [40614102098](https://github.com/nghianguyen150612/Synveil/actions/runs/40614102098); artifact ZIP: 32,391,302 bytes, SHA-256 `ebef2c9433942e98964eccbc2ecff2377dcb6b52c75ec8a0c69d9652c2a4fabf` |
| Setup candidate identity | Source `f79b302aebebd038211635e00fb39eaca7d67b23`; 32,932,573 bytes; SHA-256 `b960a1790f6fad522e12f41c1048e25dd079e3a2edcc44aa1c2b894cd96c4416`; unsigned |
| Release-manifest SHA-256 | `ed5a9d393e85d025b98444a166c0fd27e8e0b2556c8dc456041d52ade9b32d8c`; matches embedded candidate identity |
| Qualified Windows environment | `BLOCKED_BY_ENVIRONMENT` |
| Installed-component named-pipe E2E | `NOT_RUN`; candidate identity is available but no qualified Windows desktop was available |
| Native Setup GUI journey | `NOT_RUN` |
| Genuine new logon / LogonTrigger | `NOT_RUN` |
| Cleanup independently verified | `NOT_RUN` |

## P013 implementation

- Version 1 fail-closed record validation: `scripts/validate-windows-native-evidence.py`.
- Candidate/environment collection with explicit operator approval, marked
  disposable-user check, and native desktop/WTS/console/token checks:
  `scripts/qualify-windows-native-environment.ps1`.
- Operator-driven UI Automation of actual Setup, options/location/completion,
  installed runtime, first-run launch, and shutdown:
  `scripts/test-windows-gui-journey.ps1` (implemented, not run here). It now
  checks the install root, same-user installed desktop/client processes,
  non-elevated process tokens, the installed desktop's own `Ready` state after
  its controller connects to the installed client, production named-pipe
  protocol probes, cross-user denial, shutdown/absence/restart, no TCP
  listener, and uninstall preservation of a non-secret user-state sentinel. It
  emits UI Automation state classification and records that screenshots were
  not captured.
- Real installed-client IPC probe:
  `crates/client/examples/p013_windows_control_probe.rs`. It resolves the
  profile endpoint through Synveil's production config and endpoint resolver,
  tests v1 Ping plus malformed/oversized/unsupported frames, duplicate
  connections, handshake timeout, cancellation, stale profile, wrong-user
  denial, server termination, endpoint absence, and restart Ping. It starts no
  mock server and discovers endpoints through production configuration.
- Read-only new-logon observer:
  `scripts/collect-windows-new-logon-evidence.ps1`. It correlates the installed
  client's token SID and authentication LUID with Security 4624, checks the
  exact scheduled task definition and correlated trigger/task-start events. It
  never runs tasks or records raw event contents.
- Read-only post-logoff observer:
  `scripts/collect-windows-logoff-evidence.ps1`. In a separate approved admin
  phase, it binds the Security logoff event and task completion to the previous
  genuine logon, then checks for remaining product processes.
- Separate-account support: `scripts/provision-windows-p013-test-user.ps1`
  prompts for a password without persisting it; `scripts/cleanup-windows-p013-test-user.ps1`
  removes only marked P013 accounts and exact profile-scoped tasks, then
  verifies account/profile/task/process absence.
- Source regression suite: `scripts/test_windows_native_evidence.py`.
- Summary aggregator: `scripts/summarize-windows-native-acceptance.py` binds
  environment, GUI, logon, logoff, and cleanup records to exact source,
  candidate bytes, manifest digest, and harness revision. Missing component
  records remain `NOT_RUN`, and a passing summary requires all component
  digests.
- `scripts/validate-windows-native-acceptance.py` now checks the evidence gate,
  collector constraints, UI/IPC/logon harness claims, and source regression
  coverage, disposable identity rules, logoff observer constraints, and
  independent cleanup behavior.
- `.github/workflows/windows-native-acceptance.yml` runs the source evidence
  regressions and existing Windows IPC regression suite, checks the Rust
  probe, and parses all P013 PowerShell tools without executing them. The
  hosted workflow's GUI/logon/IPC blocker remains enforced; source checks do
  not emit native acceptance evidence.

The record status vocabulary is `PASS`, `FAIL`, `BLOCKED_BY_ENVIRONMENT`,
`NOT_RUN`, and `INCOMPLETE`. Its `PASS` policy binds the exact source SHA,
Setup SHA-256 and size, release-manifest digest, Windows edition/build/x64,
harness revision, non-elevated standard-user token, native session/logon/Task
Scheduler observations, every individual P028/P013 scenario, and independently
verified cleanup. Public artifacts exclude account names, user paths,
credentials, raw pipe messages, and raw Windows event contents.

The environment/logon observers are read-only. Provisioning and cleanup are
separate approved administrative phases; product execution remains under a
marked standard user. The GUI and installed-client probe are implemented but
unrun on Windows. Cross-user denial, logon/task event correlation, restart,
logout shutdown, and independent cleanup are not native evidence here.
Existing Rust tests use fixtures and are not described as installed-component
acceptance.

## Independent qualification gates

- **P004:** D02, D03, D11, and D14 remain
  `RECOMMENDED_PENDING_APPROVAL`.
- **P006:** focused run [40320179191](https://github.com/nghianguyen150612/Synveil/actions/runs/40320179191)
  passed on `f768474…`, after #165/#167 integration. The transactional
  PostgreSQL metadata → ObjectStore owner-to-byte integration case remains
  missing.
- **P036:** no production PG17/edge artifact identity, approved production
  publisher keyring/signature provenance, or Phase C evidence. Production
  artifact inventory is not supplied; P036 remains blocked.
- **Roadmap:** the authoritative P001–P060 v0.3 roadmap is unavailable.

## Validation record

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo test -p synveil-install-engine --locked` | PASS |
| `cargo clippy -p synveil-install-engine --all-targets --locked -- -D warnings` | PASS |
| `cargo test -p synveil-desktop --locked` | BLOCKED at `libdbus-sys`: system `dbus-1.pc` is absent |
| `cargo clippy -p synveil-desktop --all-targets --locked -- -D warnings` | BLOCKED at the same missing `dbus-1.pc` |
| `python3 scripts/test_windows_native_evidence.py` | PASS; 41 source-contract regression cases |
| `python3 scripts/validate-windows-native-acceptance.py` | PASS; checkpoint remains blocked |
| `./scripts/validate-installer-engine.sh` | PASS |
| `./scripts/validate-docs.sh` | PASS |
| `git diff --check` | PASS |
| `cargo test -p synveil-client --test desktop_control_ipc --locked` | BLOCKED before tests at missing `dbus-1.pc` |
| Windows-hosted `cargo test -p synveil-client --test desktop_control_ipc --locked` | Added to P013 regression workflow; exact-head CI pending |
| PowerShell parser (qualification, GUI, logon/logoff observers, provision, cleanup) | Final local rerun unavailable (`pwsh` is absent); the Windows workflow parses all six scripts on the exact PR head |
| `cargo check --locked -p synveil-client --example p013_windows_control_probe` | BLOCKED at missing system `dbus-1.pc` |
| Windows-targeted probe check | BLOCKED; local Windows GNU target lacks `x86_64-w64-mingw32-gcc` |
| Native PowerShell harness execution | NOT_RUN; current workspace is Linux |

The previously pushed P013 head's hosted checks showed the two intentional
`standard-user-native` blocker failures while Linux and Windows Qt jobs were
still running. The updated exact-head CI will be reconciled after the final
P013 push. The qualified Windows harness has not run here.
No local Linux result is upgraded to native Windows evidence.

## Handoffs

P014 should obtain the exact P028 candidate bytes and release manifest, run the
environment qualifier and visible UI journey on a fresh isolated Windows 11
x86-64 VM, set `StartupEnabled=true` to register the LogonTrigger task, and
provision owner and peer disposable accounts. Perform manual sign-out/sign-in
transitions for logon and logoff evidence, run independent
cleanup for both accounts, validate all records against the evidence contract,
and confirm exact-head CI. Preserve the hosted blocker and do not activate
production Wizard execution. P004, P006, and P036 remain independent gates.

## Git and PR

| Field | Value |
|---|---|
| Branch | `feat/v03-p013-windows-native-qualification` |
| Target | `develop/v0.3` |
| Evidence contract commit | `eda1d78d194ca670c52a19f7fdbf8be8aa0d959f` |
| Status reconciliation commit | `943b17bb64e67d0835705b62f3ca798f36eaf9d5` |
| GUI harness commit | `ff309b07e8ee41d7df976fc9690188c25da47245` |
| GUI harness hardening commit | `cd65e357871392e36e181a040eba063bebadd457` |
| Draft PR | [#173](https://github.com/nghianguyen150612/Synveil/pull/173) — open, draft |

P013 is not complete. No merge or P014 implementation is authorized by this
manifest.
