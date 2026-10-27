# PROMPT012 — implementation and qualification manifest

**Result: blocked pending qualification.** P012 source changes are on
`feat/v03-p012-last-use-adapter-contracts`, based on the verified
`develop/v0.3` SHA `4e3833044522e167c138c0f272f804511619d52f`. The v0.3 roadmap
file named in earlier phase documentation was not found, so P012 remains
provisional.

## Preflight

| Item | Result |
|---|---|
| `origin/develop/v0.3` | `4e3833044522e167c138c0f272f804511619d52f` at preflight; no advancement |
| `origin/main` | `604499fce0dfd134d82da3d5bb5d4a055c5eea8d`; not modified |
| P006–P010 | Merged: #165, formatting #167, #166, #168, #169, #170 |
| P011 #171 | Merged at the expected SHA |
| Authoritative v0.3 roadmap | Not found in the repository/workspace search |

## P011 exact-head inherited CI

On merge commit `4e3833044522e167c138c0f272f804511619d52f`, hosted Rust
quality/tests and native Linux Qt 6 / Windows Qt 6 jobs passed. Linux DEB/RPM,
AppImage, Windows installer smoke, and the later PG17 live suite passed. The
Windows `standard-user-native` job failed because the hosted runner cannot
provide a qualified interactive desktop or genuine new-logon session, and
named-pipe end-to-end acceptance is not wired. The job recorded those missing
surfaces and completed harness cleanup; it did not waive P028. P011 source
integration and P011 UI success are not security certification.

PR #171 was merged under a maintainer-requested expedited integration
exception with no independent reviews submitted. Its pre-merge head had 88
checks: 80 success, 2 failure, 2 in progress, 2 cancelled, and 2 skipped. None
of the non-success results counted as passes. On the merge commit, the latest
rollup had 20 success and the single P028 `standard-user-native` failure:
[Linux Qt 6](https://github.com/nghianguyen150612/Synveil/actions/runs/40508827216/job/122205035135),
[Windows Qt 6](https://github.com/nghianguyen150612/Synveil/actions/runs/40508827216/job/122205035044),
[P028 standard-user](https://github.com/nghianguyen150612/Synveil/actions/runs/40508827312/job/122216005005),
[PG17 live suites](https://github.com/nghianguyen150612/Synveil/actions/runs/40508827366/job/122205035555).

## P012 implementation summary

- Added versioned, non-serializable last-use evidence types and a Linux
  no-follow, bounded-memory exact-size/SHA-256 verifier.
- Bound expected artifact identity/size to manifest identity/generation,
  plan fingerprint, platform/channel, capability generation, and
  installation-state generation. The trusted source handoff is crate-private
  and has no production caller.
- Linux preserves the open file description during verification and detects
  path replacement on revalidation. The eventual consumer cannot accept this
  handle yet, so use-time identity remains explicitly unguaranteed. Windows
  and other non-Linux platforms return `PlatformFileIdentityUnavailable`.
- Added typed Linux/Windows adapter capability categories and effect ownership
  validation over the P011 closed catalog. The validator cannot run package
  managers, shell commands, UAC, Polkit, sudo, Host operations, journal writes,
  or effects. A test catalog never becomes a production-approved catalog.
- Integrated last-use and adapter evidence into the P011 readiness report.
  Publisher policy unavailable, recovery, native authorization, P004 policy,
  and `ProductionExecutionCapabilityDisabled` remain blocking.
- Added bounded Wizard Review status fields; the UI still says “Preview only
  — installation is not started.” and does not present publisher trust as
  verified.
- Added exact authenticated artifact size to validated plan evidence and the
  review fingerprint. Source-provenance reference and trust-policy identity
  are also required typed plan fields; all three are bound into the review
  fingerprint, last-use handle comparison, and adapter effect bindings. Missing
  fields deserialize empty and fail validation.

## Local validation

| Command or suite | Outcome |
|---|---|
| `cargo fmt --all -- --check` | Passed after formatting |
| `cargo test -p synveil-install-engine --locked` | Passed; 50 unit tests and all package integration suites, including the P012 cases |
| `cargo clippy -p synveil-install-engine --all-targets --locked -- -D warnings` | Passed |
| `cargo test -p synveil-desktop --locked` | Blocked at `libdbus-sys`: system `dbus-1.pc` is absent |
| `cargo clippy -p synveil-desktop --all-targets --locked -- -D warnings` | Blocked at the same missing `dbus-1.pc` |
| `scripts/validate-installer-engine.sh` | Passed; includes the installer-engine suite |
| `scripts/validate-release-artifacts.sh` | Blocked: generated `target/packages/SYNVEIL-LINUX-ARTIFACT-MANIFEST.txt` is absent |
| `scripts/test-desktop-ui.sh` | After staging the P012 modules, source preflight passed; the run then stopped because the qmake shim points to missing `/usr/lib/qt5/bin/qmake` |
| `scripts/validate-docs.sh` | Passed |
| `git diff --cached --check` and `git diff --check` | Passed after removing trailing whitespace |
| Release channel tests | Passed: 101 |
| Release manifest tests | Passed: 36 |
| Release download/staging-race tests | Passed: 79 |
| Installer security tests | Passed: 23 |
| `scripts/validate-install-journal.sh` | Passed: 91 journal contract tests |
| `scripts/validate-installer-security-hardening.py` | Passed |
| `scripts/validate-p036-production-artifacts.py` | Blocked: `production-artifacts.json` has not been supplied |

## P012 native hosted qualification

Exact-head P012 GitHub Actions status is recorded after the latest source and
documentation commit is pushed. Required Linux and Windows compilation/UI
checks must complete on that exact head. Queued, skipped, cancelled, or
environment-blocked jobs do not count as passes. P028's interactive
desktop/new-logon/named-pipe acceptance remains an independent release gate
even if the P012 native UI jobs pass.

## Remaining policy and qualification blockers

- P004-D02, D03, D11, and D14 remain `RECOMMENDED_PENDING_APPROVAL`.
- No approved production publisher trust keyring/policy is connected to the
  Rust Wizard path; P036 production artifact identity remains unqualified.
- No approved production effect catalog or native authorization executor is
  available. Production execution remains unconditionally disabled.
- P006 joined PostgreSQL metadata-to-ObjectStore owner-to-byte coverage is
  still required.
- P028 still needs qualified interactive Windows, new-logon, and named-pipe
  acceptance evidence.
- P012 Windows file-handle identity and same-handle consumption are not
  implemented; non-Linux verification blocks.
- P013 prerequisites are listed in
  [`P012_LAST_USE_ARTIFACT_NATIVE_ADAPTERS.md`](../installer/P012_LAST_USE_ARTIFACT_NATIVE_ADAPTERS.md).

## Git and pull request

Branch: `feat/v03-p012-last-use-adapter-contracts`

Target: `develop/v0.3`
Draft PR: [#172](https://github.com/nghianguyen150612/Synveil/pull/172)

Latest commit and exact-head hosted status: recorded after the final source and
documentation update is pushed.
