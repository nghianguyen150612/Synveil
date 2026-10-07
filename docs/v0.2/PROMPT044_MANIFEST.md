# Prompt044 manifest — Installation Resilience Hardening

Status: **source and deterministic resilience coverage implemented; actual
publication, hosted checks, and merge are recorded in the associated real PR
and final completion report after observation. Native VM power-cycle evidence
is unavailable and is not claimed.** P045 is explicitly deferred.

## Repository and baseline

- Repository: `https://github.com/nghianguyen150612/Synveil.git`.
- Repository path: `/workspace/Synveil`.
- Expected baseline: `3a927708142b4bee3a4582cc54e6dab1ea47fdc8`.
- Actual baseline: `3a927708142b4bee3a4582cc54e6dab1ea47fdc8` (`origin/main`).
- P043 merge commit `3a927708142b4bee3a4582cc54e6dab1ea47fdc8` and head
  `cb5fab88d7372c7d840129c2deccfcffc3d0f8ae` remain ancestors.
- The fetch/push origin is
  `https://github.com/nghianguyen150612/Synveil.git`; repository identity was
  checked before edits. Initial checkout was clean on `work`; no work was
  discarded.
- Task branch: `feat/installation-resilience-hardening`, created from verified
  `origin/main`. Its name was checked locally and does not start with `codex/`.
  The actual remote ref must be queried again before PR creation.
- No prior P044 branch or pull request was found at audit time.

## Ownership audit

Reviewed ROADMAP, architecture, product contract, clean-machine acceptance,
P008 journal, P008/P008A manifest, P009 upgrade/repair/uninstall contract,
P010 error model, Linux quick install, Windows lifecycle, P043 hardening and
P043 manifest. Inspected `crates/install-engine/src/` and its tests, acquisition
and channel scripts, Linux quick-install entry points, package integration,
AppImage integration, Windows builder/Inno source, existing Linux/Windows CI,
AppImage CI, and the `INSTALL-JOURNEY-6/7/8` and `UPGRADE-TEMPLATE-1` acceptance
scenarios. Existing P008 journal ownership remains authoritative; no secondary
recovery log or GUI recovery marker was added.

The detailed 30-boundary failure matrix and evidence classes are in
[INSTALLATION_RESILIENCE_HARDENING.md](INSTALLATION_RESILIENCE_HARDENING.md).
P004's `INSTALL-JOURNEY-8` still requires power-cycle evidence; P044 does not
change that acceptance rule.

## Implemented resilience changes

- Journal I/O classifies ENOSPC/quota errors and exposes deterministic fault
  points after write, file sync, no-replace commit, committed-object sync, and
  directory sync. Root creation syncs parent directories; unexpected
  transaction-directory objects fail closed. Empty interrupted transaction
  setup can be retried only after OS lock acquisition and state validation.
- Journal append failure before durable mutation start leaves adapter apply at
  zero. A failed checkpoint after possible mutation returns inspection-required
  and blocks later effects. Unknown outcomes reconcile before retry; completed
  effects are reverified read-only; completion remains after final verification.
- Acquisition maps disk exhaustion to `INSUFFICIENT_DISK_SPACE`. Temp writes,
  file sync, promotion, and parent sync failures cannot return a verified
  result. A visible completed artifact after ambiguous sync must be rehashed
  against authenticated identity before reuse. Partial bytes are not range-
  resumed.
- Quick-install distinguishes pre-native disk-full from native transaction
  ambiguity. Native state must be inspected after interruption; package locks
  remain untouched. A package database absence result plus remaining
  package-owned payload is partial state and stops before package-manager retry.
- AppImage partial integration is reported as incomplete; absent or ambiguous
  ownership does not authorize claiming or deleting an adjacent launcher.
- Windows payload generation already emits the target ownership manifest last
  in the `[Files]` sequence. P044 validates that ordering and adds a hosted
  standard-user test that kills Inno Setup after a changed owned file is copied,
  confirms the prior manifest/registration still agree, and launches a fresh
  compatible Setup to verify the complete target payload. Setup copy/registration
  is not claimed atomic; this is process-interruption evidence, not power loss.
- User/server data, explicit disabled startup, and unknown neighboring files
  remain outside recovery cleanup scope.

## Evidence actually available

| Evidence class | Result |
|---|---|
| Source/static review | P008/P008A ordering, acquisition no-clobber flow, platform ownership and P043 security guards reviewed; static validator added |
| Deterministic fault injection | Passed locally: 77 release-download tests include ENOSPC at temp create, stream write, file sync, promotion, and directory sync; install-engine journal tests inject write/sync/commit boundaries and disk-full before/after mutation |
| Fixture tests | Passed locally: 15 Linux quick-install tests; 9 AppImage integration tests include partial-state and preservation cases |
| Process interruption | Passed locally: Rust journal suite forcibly kills process A after a synced fixture payload mutation; a newly launched process B reloads the journal, reconciles from disk, verifies and completes without duplicate apply. This is `process-interruption`, not power loss. |
| Native CI | Existing Linux package and AppImage workflows provide normal lifecycle evidence. The Windows installer workflow now includes a bounded forced-process-interruption upgrade fixture; final-head results are pending. Linux package-manager and AppImage mutation interruption remain untested natively. |
| Container restart | Not run; would not establish filesystem/controller power-loss behavior |
| VM reboot | Not available in the selected environment |
| VM power-cycle | **BLOCKED / unavailable native power-cycle evidence**; no graceful shutdown, SIGKILL, or fixture is substituted |

## Changed files

- `.github/workflows/installation-resilience-hardening.yml`
- `.github/workflows/windows-installer.yml`
- `crates/install-engine/src/error_model.rs`
- `crates/install-engine/src/journal.rs`
- `crates/install-engine/tests/appimage_integration.rs`
- `crates/install-engine/tests/error_model_contract.rs`
- `crates/install-engine/tests/journal_contract.rs`
- `docs/v0.2/INSTALLATION_RESILIENCE_HARDENING.md`
- `docs/v0.2/INSTALLATION_TRANSACTION_JOURNAL.md`
- `docs/v0.2/INSTALLER_ERROR_MODEL.md`
- `docs/v0.2/LINUX_QUICK_INSTALL.md`
- `docs/v0.2/PROMPT044_MANIFEST.md`
- `docs/v0.2/ROADMAP.md`
- `scripts/linux_quick_install.py`
- `scripts/invoke-windows-standard-user-test.ps1`
- `scripts/release_download.py`
- `scripts/test-windows-installer-lifecycle.ps1`
- `scripts/validate-docs.sh`
- `scripts/validate-install-error-model.py`
- `scripts/validate-installation-resilience-hardening.py`
- `scripts/validate-installer-security-hardening.py`
- `scripts/validate-windows-installer.py`
- `tests/install-error-model/p006-acquisition-codes.json`
- `tests/linux_quick_install/test_linux_quick_install.py`
- `tests/release_download/test_release_download.py`

## Local commands and exact results

| Command | Result |
|---|---|
| `cargo fmt --all` followed by `cargo fmt --all -- --check` | Passed |
| `cargo test -p synveil-install-engine --locked --quiet` | Passed: 526 tests across AppImage, engine, error model, journal, lifecycle, and Linux package integration suites |
| `cargo clippy -p synveil-install-engine --all-targets --locked -- -D warnings` | Passed |
| `python3 -m unittest discover -s tests/release_download -v` | Passed: 77 tests |
| `python3 -m unittest discover -s tests/linux_quick_install -v` | Passed: 15 tests |
| `python3 scripts/validate-install-error-model.py` | Passed: 20 exact P006 acquisition codes |
| `python3 scripts/validate-installer-security-hardening.py` | Passed |
| `python3 scripts/validate-installation-resilience-hardening.py` | Passed |
| `python3 scripts/validate-windows-installer.py` | Passed |
| `python3 -m unittest discover -s scripts -p test_windows_lifecycle.py -v` | Passed: 6 lifecycle-model tests |
| `python3 -m py_compile scripts/validate-installation-resilience-hardening.py scripts/release_download.py scripts/linux_quick_install.py scripts/validate-install-error-model.py` | Passed |
| `./scripts/validate-docs.sh` | Passed, including P043/P044 validators |
| `./scripts/validate-install-acceptance.sh` | Passed: 11 scenarios; native execution not performed |
| `git diff --check` | Passed |

Local tests used Rust 1.99 installed into the task workspace after the initial
toolchain check found no Rust commands. No repository toolchain or lockfile was
changed. Hosted CI must still run final-head platform suites before merge.

## Publication record

Observed GitHub state for the initial published head:

- Required remote branch: `feat/installation-resilience-hardening`; the GitHub
  branch API returned that exact name, with no `codex/` prefix.
- Local commit/tree: `84ad1209da1a8f70b0748d8cfa0598dd75d1789d` /
  `81f205dadc42ecac4311a0441b18212b09bb5d60`.
- Remote commit/tree after the initial push: the same commit/tree as local.
- Comparison against `main`: `ahead_by=1`, `behind_by=0`; the sole commit was
  `84ad1209da1a8f70b0748d8cfa0598dd75d1789d`.
- PR: [#75 — Synveil v0.2 P044: Installation Resilience Hardening](https://github.com/nghianguyen150612/Synveil/pull/75), base `main`.
- Hosted checks on the initial head: the journal/process-restart, lifecycle,
  Linux package-boundary, AppImage, preservation, docs/static, strict P044
  quality and Ubuntu acquisition jobs passed. The Windows acquisition job
  exposed that its directory-sync fault fixture patched Unix `os.fsync` rather
  than the shared platform boundary; the fixture was corrected in a follow-up.
  The existing Windows native acceptance workflow failed before invoking its
  installer test because its build selected Git's `/usr/bin/link` instead of
  MSVC `link.exe`; that workflow is unchanged by P044 and this is classified as
  an inherited Windows toolchain failure. Remaining hosted checks, including
  the real Windows installer interruption fixture, are observed on the final
  PR head before merge.
- Merge/resulting-main SHA and post-merge clean worktree are intentionally
  recorded only after GitHub reports the real merge. VM power-cycle evidence
  remains **BLOCKED / unavailable native power-cycle evidence**.

No ADR is added: this change validates/enforces the existing P008/P008A,
P009/P010, P006 and platform ownership architecture. ADR-074 was checked as
unused at baseline but is not consumed. No v0.2.0 tag or release is created.

P045, P046, P047 and P048 remain open. Do not begin P045 as part of P044.
