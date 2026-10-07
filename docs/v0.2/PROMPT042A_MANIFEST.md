# Prompt042A manifest — Phase-F strict readiness gate closure

Corrective follow-up to Prompt042 and PR #72. This manifest records gate
closure evidence without changing the historical Prompt042 manifest. Prompt043
Installer Security Hardening is out of scope and has not begun.

## Baseline and preserved state

Expected and fetched `origin/main` baseline: commit
`3caf47d033101d596c7868820f896ccc2df9574b`, the P042 merge for PR #72. P041
and earlier Phase-F history remain ancestors. Work is on
`fix/phase-f-readiness-gates`; the name does not use the prohibited `codex/`
prefix. No P042A commit, remote branch, PR, or merge existed at continuation
recovery. The working changes were preserved on the required branch.

The P042 recovery safety contract remains in place: ROOT UNAVAILABLE is not an
empty tree and does not authorize deleting data; `OutcomeUnknown` does not
blindly replay a mutation. No recovery, identity, fencing, pause, conflict,
restart, executable-resolution, or writer-lock behavior was redesigned.

## Reproduced blockers and corrections

Strict P042 Clippy evidence reproduced `double_must_use` on
`RemoteContent::into_stream`; subsequent strict desktop/workspace runs exposed
redundant must-use attributes in object-store and storage stream APIs,
`chunks_exact_to_as_chunks` in Windows task XML decoding, a test-only
`field_reassign_with_default`, and a nested conditional lint in server-config.
The stream APIs now carry specific `must_use` reasons, UTF-16 parsing uses
`as_chunks::<2>()` while rejecting odd byte counts and invalid UTF-16 as before,
and the test/config code uses idiomatic initialization and a let-chain. A
regression test covers valid UTF-16LE, malformed byte counts, invalid
surrogates, and unsupported byte order.

The locked baseline selected `quick-xml` 0.38.4 and failed the repository
advisory gate for RUSTSEC-2026-0194 and RUSTSEC-2026-0195. The workspace
requirement is now exact 0.41.0 and Cargo.lock contains only that corresponding
quick-xml package movement. Changelog review identified 0.41.0 as the first
patched release for both issues. `cargo deny check` now passes advisories,
bans, licenses, and sources without advisory ignores.

The real desktop script initially surfaced Qt 6.4 QML metadata/import
diagnostics for QtQml types and CXX-Qt integer/QObject metadata. The fix adds
the QtQml module dependency and the missing QtQml runtime modules to the
relevant Linux CI jobs. The vendored CXX-Qt build helper normalizes only
equivalent `int32_t`/`int` and globally-qualified QObject names in generated
moc metadata, and supplies Qt's QObject metatype JSON without compiling a
duplicate QObject implementation. The QML itself uses a Qt 6.4 compatible
folder URL conversion and a string-built live-test signature. No qmllint
warnings are disabled. The full `test-desktop-ui.sh` gate reached qmllint and
both fresh debug/release offscreen smokes successfully.

## Changed paths

- `.github/workflows/authentication-ux-polish.yml`
- `.github/workflows/ci.yml`
- `.github/workflows/connection-setup-simplification.yml`
- `.github/workflows/installation-to-sync-progress.yml`
- `.github/workflows/library-first-run-wizard.yml`
- `.github/workflows/repair-recovery-ux.yml`
- `.github/workflows/unified-welcome.yml`
- `Cargo.toml`, `Cargo.lock`
- `crates/client-sync/src/contracts.rs`
- `crates/client/src/launch.rs`
- `crates/desktop/build.rs`, `crates/desktop/qml/Main.qml`,
  `crates/desktop/src/presentation.rs`
- `crates/object-store/src/types.rs`
- `crates/server-config/src/model.rs`
- `crates/storage/src/downloads.rs`, `crates/storage/src/uploads.rs`
- `scripts/validate-library-first-run-wizard.py`,
  `scripts/validate-unified-welcome.py`,
  `scripts/validate-installation-to-sync-progress.py`
- `docs/v0.2/ROADMAP.md`
- `vendor/cxx-qt-build/src/lib.rs`
- The focused `repair-recovery-ux` workflow also adds strict workspace Clippy
  and dependency-policy jobs and triggers on this corrective branch.
- This manifest.

Workflow package changes provide missing QtQml modules/tools and `rg` where
existing validators require them; desktop jobs also provision the canonical
`/usr/src/synveil` test directory with the runner user as owner.

## Local commands and results

Local validation used the task-owned Linux container with Rust 1.99 and Qt
6.4.2. The development/test profiles omitted debug symbols to limit build
space; lint levels and warning policy were unchanged.

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed; strict workspace gate |
| `cargo clippy -p synveil-client -p synveil-client-sync -p synveil-install-engine --all-targets --locked --no-deps -- -D warnings` | Passed |
| `./scripts/test-desktop-ui.sh` | Passed on the corrected QML head; strict desktop Clippy, 91 desktop tests, qmllint, and debug/release offscreen smokes completed |
| `cargo deny check` | Passed: advisories, bans, licenses, sources |
| `cargo check --workspace --all-targets --exclude synveil-desktop --locked` | Passed; matches the repository's non-Qt workspace check |
| `cargo test --locked -p synveil-client-sync -p synveil-object-store -p synveil-client -p synveil-desktop -p synveil-install-engine` | Passed: 1,012 tests across the five requested crates; includes the Windows task XML UTF-16 regression |
| Six P037–P042 validators (`python3 scripts/validate-*.py`) | Passed |
| `./scripts/validate-docs.sh` | Passed |
| `./scripts/validate-install-acceptance.sh` | Passed; 21 contract tests |
| `git diff --check` | Passed |

All GitHub Actions workflow YAML files also parse successfully with PyYAML.

No native Windows acceptance, clean-machine installation, release, or P045
evidence is claimed by this corrective gate task.

## Hosted publication and Phase-F readiness checkpoint

Published branch: `fix/phase-f-readiness-gates` (not a `codex/*` branch).
PR #73 targets `main`:
<https://github.com/nghianguyen150612/Synveil/pull/73>.
The P042A source head `90e60b2f3b88b1a5de4243535149b946a7d53d70` is the second
commit on the preserved branch, after `2cc4c23007330b39e09b2cd62307c047f6812dc5`.

Hosted required gates passed on that source head:

| Workflow | Run | Result |
|---|---:|---|
| P042A repair and recovery UX | 37549502746 | Passed all 9 jobs: focused and workspace strict Clippy, desktop/QML, dependency policy, validators/docs, lifecycle, recovery, and installer repair contract |
| P037 Unified Welcome | 37549502784 | Passed source contract, generated-metadata qmllint, and offscreen Welcome smoke |
| P038 Connection Setup | 37549502849 | Passed all 3 jobs |
| P039 Authentication UX | 37549502838 | Passed all 4 jobs |
| P040 Library First-Run Wizard | 37549502757 | Passed all 3 jobs |
| P041 Installation-to-Sync Progress | 37549502827 | Passed all 4 jobs, including desktop/QML and sync-truth regressions |

The hosted Qt 6.4 QML lint initially identified `Accessible.name` attached to
the startup-choice `Popup`. It now labels the `ColumnLayout` item instead, and
the P037 generated-metadata lint/smoke and P042A desktop/QML script both pass
without disabling qmllint warnings.

Rust CI run 37549502796 also passed quality/format/Clippy, dependency policy,
Ubuntu and macOS checks, and the Linux Qt 6 desktop job. The roadmap records
**V0.2 FIRST RUN EXPERIENCE READY** after those strict Phase-F gates passed.
This is a first-run UX readiness checkpoint only. It does not claim a v0.2
release, P043 completion, native Windows acceptance, or clean-machine matrix;
P043 has not begun. PR #73 remained open at the time this manifest checkpoint
was recorded. The PR and final task report carry the actual merge identity.

Unrelated failures observed on the same PR head remain separate from P042A:

- Rust CI `Test (ubuntu-latest)` failed
  `package_unit_6_upgrade_preserves_state`: its packaging fixture expects
  schema 7 while the current constant is 8. This is the same mismatch as the
  P042 baseline run 37471365351.
- Rust CI `Test (macos-latest)` had three `UnsafeEndpoint` Unix-socket test
  failures, matching the baseline run. Windows workspace and native Qt tests
  also hit existing POSIX-root-path and Windows pipe parity fixtures; the
  P042A UTF-16 decoder regression test passed.
- Rust CI run 37549502796 `Check (windows-latest)` still compiles Unix-only permission APIs in
  server-config/API test targets. The Linux AppImage run failed its existing
  run 37549502776 `APPIMAGE-8: AppRun does not exec packaged desktop` check.
- Windows installer run 37549502896, Windows native acceptance run
  37549502904, Linux clean-machine acceptance run 37549502888, and PostgreSQL
  scheduled-maintenance run 37549502815 failed in their separate
  release/native/server scopes. They are outside P042A's required Phase-F gate
  set and are not claimed complete here.

The pre-P042A baseline evidence remains as recorded above: focused P042 run
37469628690 failed strict `quality` and `desktop-and-qml`; Rust CI run
37471365351 also reported RUSTSEC-2026-0194/0195, strict server-config Clippy,
and a Linux desktop runner unable to create `/usr/src/synveil`. P042A fixes
those required gates without changing the unrelated failures listed here.
