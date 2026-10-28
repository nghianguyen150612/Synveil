# PROMPT013 — Windows native acceptance manifest

**Status: `SYNVEIL_V03_P013_BLOCKED`** — source-level evidence contract
implemented; native acceptance and several harness scenarios remain unrun or
not yet implemented.

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

PR #172's exact source-head checks were not fully complete when inspected.
Linux Qt 6, Rust quality/tests, and Windows installer-security subchecks had
successes; several checks remained queued/in progress. The Windows Qt job was
not complete in the exact source-head rollup. The P012 source-head Windows
native run [40614014942](https://github.com/nghianguyen150612/Synveil/actions/runs/40614014942)
completed candidate build successfully and failed `standard-user-native`.
The workflow-dispatch PR run [40614021598](https://github.com/nghianguyen150612/Synveil/actions/runs/40614021598)
had a successful candidate-build job and a queued native consumer at the last
query. Post-merge run [40614102098](https://github.com/nghianguyen150612/Synveil/actions/runs/40614102098)
on `f79b302…` again had candidate build success and standard-user native
failure. These results do not establish GUI, genuine-logon, or installed
named-pipe acceptance.

| Evidence item | Result |
|---|---|
| P012 source commit | `27c38e3ec23b90dc0eb7f2a2a8191b311cc04552` |
| Post-merge base commit | `f79b302aebebd038211635e00fb39eaca7d67b23` |
| P028 hosted candidate build | `PASS` (candidate build only) |
| P028 hosted standard-user native job | `FAIL`; hosted run does not qualify GUI/logon/installed IPC; exact failing check is retained |
| Candidate SHA-256 / size / release-manifest digest | Unavailable; GitHub artifact download returned HTTP 403. No artifact bytes are claimed. |
| Qualified Windows environment | `BLOCKED_BY_ENVIRONMENT` |
| Installed-component named-pipe E2E | `NOT_RUN` |
| Native Setup GUI journey | `NOT_RUN` |
| Genuine new logon / LogonTrigger | `NOT_RUN` |
| Cleanup independently verified | `NOT_RUN` |

## P013 implementation

- Version 1 fail-closed record validation: `scripts/validate-windows-native-evidence.py`.
- Candidate/environment collection with explicit operator approval and native
  desktop/WTS/console/token checks: `scripts/qualify-windows-native-environment.ps1`.
- Source regression suite: `scripts/test_windows_native_evidence.py`.
- `scripts/validate-windows-native-acceptance.py` now checks the evidence gate,
  collector constraints, and source regression coverage.
- `.github/workflows/windows-native-acceptance.yml` runs the source evidence
  regressions. The hosted workflow's GUI/logon/IPC blocker remains enforced;
  source tests do not emit native acceptance evidence.

The record status vocabulary is `PASS`, `FAIL`, `BLOCKED_BY_ENVIRONMENT`,
`NOT_RUN`, and `INCOMPLETE`. Its `PASS` policy binds the exact source SHA,
Setup SHA-256 and size, release-manifest digest, Windows edition/build/x64,
harness revision, non-elevated standard-user token, native session/logon/Task
Scheduler observations, every individual P028/P013 scenario, and independently
verified cleanup. Public artifacts exclude account names, user paths,
credentials, raw pipe messages, and raw Windows event contents.

The current collector intentionally does not execute Setup, start a task,
provision a test identity, or mutate the machine. It therefore leaves all
product GUI, genuine-logon, installed-IPC, lifecycle, and cleanup checks
unrun. Existing Rust tests exercise the v1 protocol with test servers and are
not described as end-to-end installed-component evidence.

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
| `python3 scripts/test_windows_native_evidence.py` | PASS; 31 source-contract regression cases |
| `python3 scripts/validate-windows-native-acceptance.py` | PASS; checkpoint remains blocked |
| `./scripts/validate-installer-engine.sh` | PASS |
| `./scripts/validate-docs.sh` | PASS |
| `git diff --check` | PASS |
| `cargo test -p synveil-client --test desktop_control_ipc --locked` | BLOCKED before tests at missing `dbus-1.pc` |
| PowerShell environment collector | NOT_RUN; this workspace is Linux and has no `pwsh` |

Hosted P013 jobs start with the draft PR; final exact-head checks are pending.
The qualified Windows harness has not run here. No local Linux result is
upgraded to native Windows evidence.

## Handoffs

P014 must implement and run the actual installed desktop/client named-pipe
driver, native UI Automation across Setup and installed first-run, a disposable
standard-user real-logon controller that correlates the current logon LUID to
Security 4624 and Task Scheduler LogonTrigger evidence, and independent cleanup
verification. It must run on a qualified Windows 11 x86-64 physical acceptance
machine or isolated VM, bind the exact candidate and source, retain the hosted
blocked result, and not activate P012/P013 production Wizard execution. P004,
P006, and P036 handoffs remain independent prerequisites to their respective
security/release claims.

## Git and PR

| Field | Value |
|---|---|
| Branch | `feat/v03-p013-windows-native-qualification` |
| Target | `develop/v0.3` |
| Commit | To be recorded after local validation |
| Draft PR | To be recorded after push |

P013 is not complete. No merge or P014 implementation is authorized by this
manifest.
