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
- `scripts/validate-library-first-run-wizard.py`
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
| `./scripts/test-desktop-ui.sh` | Passed; strict desktop Clippy, qmllint, debug and release offscreen smoke all completed |
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

## Hosted publication and checkpoint

Remote branch, PR identity, hosted workflow results, and merge identity are
pending actual publication and observation. The roadmap readiness marker is
still withheld until all required local and hosted Phase-F gates pass and the
corrective PR is merged. No hosted result is inferred from local validation.

The fetched P042 baseline has real hosted evidence. Focused P042 run
37469628690 passed six jobs and failed its strict `quality` and
`desktop-and-qml` jobs: `double_must_use` blocked strict client Clippy,
`field_reassign_with_default` blocked strict desktop Clippy, and QML was
skipped after that Clippy failure. On merged P042 baseline 3caf47d,
Rust CI run 37471365351 independently reported the same two RustSec
advisories, strict server-config Clippy, and the Linux desktop job could not
create `/usr/src/synveil`; the latter is fixed by provisioning that path for
the runner. P042A's local required gates now pass, but its hosted results are
still pending.

Other failures on that baseline run were classified outside P042A's required
gates: a packaging fixture still expects schema 7 while the current constant
is 8; Unix socket/permission assumptions fail on macOS and Windows test
runners, and the Windows API test target uses Unix-only permission APIs. The
main Web test job, Windows installer/native acceptance, Linux AppImage/package
reproducibility, PostgreSQL scheduled-maintenance stress, and server first
admin response-loss jobs also failed. These are inherited unrelated workflow,
platform, release, or server gates and are not being claimed as complete here.
They do not replace the exact P042A focused strict/QML/dependency gates, whose
real PR-head results must still be observed.
