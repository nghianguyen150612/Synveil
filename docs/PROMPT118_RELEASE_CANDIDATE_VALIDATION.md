# Synveil v0.1 Release Candidate Validation

Prompt118 validation record, completed 2026-09-27.

## Result

**PASS** — all locally executable mandatory RC gates passed. No unresolved
`RC_BLOCKER` remains. PostgreSQL live acceptance and Windows native execution
are classified as environment blockers, not passes.

## Source Control

- Starting HEAD: `e5843169b5f9bb3c6696d73e10be7bfbdec5707c`.
- Starting `origin/main`: `e5843169b5f9bb3c6696d73e10be7bfbdec5707c`.
- Starting worktree: clean.
- Source changes during validation: none before this record.

## Version and Schema

- Product/workspace version: `0.1.0`.
- Server migration files: `36`.
- Client migration files: `7`.
- `LOCAL_SCHEMA_VERSION`: `7`.
- No migration was added or changed during Prompt118.

## Standard Gates

| Gate | Command | Result |
| --- | --- | --- |
| Format | `cargo fmt --all -- --check` | PASS |
| Workspace check | `cargo check --workspace --locked` | PASS |
| Workspace Clippy | `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | PASS |
| Workspace tests | `cargo test --workspace --locked` | PASS; 979 passed, 0 failed, 444 ignored across 86 result summaries |
| Dependency policy | `cargo deny check` | PASS; advisories, bans, licenses, and sources passed |
| Diff check | `git diff --check` | PASS |

The workspace test run includes the SQLite migration, profile, onboarding,
authentication-boundary, IPC, sync runtime, pause/resume, conflict,
recovery, restart, and package lifecycle unit/integration evidence. PostgreSQL
tests were correctly ignored because no test database was available.

## Desktop and IPC

- `./scripts/test-desktop-ui.sh`: PASS. Desktop tests, strict Clippy, QML
  lint, debug build, release build, and both offscreen smoke launches passed.
- Focused authentication boundary: PASS — `cargo test -p synveil-client --lib authentication --locked` (6 passed).
- Desktop authentication presentation: PASS — `cargo test -p synveil-desktop --locked authentication_feedback` (1 passed).
- IPC suite: PASS — `cargo test -p synveil-client --test desktop_control_ipc --locked` (5 passed).
- Controller/reconnect generation suite: PASS — `cargo test -p synveil-client --test desktop_controller --locked` (6 passed).
- Windows packaging policy suite: PASS — `cargo test -p synveil-metadata --test windows_desktop_packaging_units --locked` (3 passed).
- The evidence confirms one canonical background client owner, fail-closed
  malformed/unsafe IPC handling, bounded frames/connections, profile-scoped
  endpoint identity, reconnect generation fencing, and controller-only GUI
  lifecycle actions.

## Product Validation

- Authentication: PASS for local controller, credential-boundary, redaction,
  persistence-boundary, restart/sign-out contract coverage. No live server
  authentication claim is made without PostgreSQL/server infrastructure.
- Profile onboarding: PASS for configuration-required state, canonical URL
  validation, profile identity isolation, origin fencing, and zero-library
  configuration coverage.
- Library onboarding: PASS for root validation, dangerous/application-state
  overlap rejection, pending binding, durable registration, and restart
  reconstruction coverage.
- Existing-folder bootstrap: PASS; existing-tree scans create initial content
  intents and do not infer deletions. Root marker and remote-root seeding
  coverage passed.
- Sync runtime: PASS; bounded concurrency, fairness, retry/backoff, wake
  coalescing, watcher overflow/reconciliation, restart reconstruction, and
  graceful shutdown coverage passed.
- Pause/resume: PASS; durable pause state blocks normal scheduling, preserves
  pending work, survives reopen, and resume emits runtime wake coverage.
- Conflicts/recovery: PASS; bounded attention projection, supported-action
  matrix, stale/duplicate resolution handling, durable-before-wake ordering,
  and `OutcomeUnknown` reconciliation coverage passed.
- Persistent state: PASS; fresh, historical upgrade, future/malformed schema,
  interrupted rollback, and reopen SQLite coverage passed without changing
  the migration set.

## Linux Live RC

Using disposable paths under `/tmp` and a fixed non-secret profile identity:

- `LIVE-RC118-1`: PASS. `target/release/synveil-client` created its real
  profile-scoped Unix IPC socket; `target/release/synveil-desktop` connected
  through the local endpoint and reported `Connected / Current`.
- `LIVE-RC118-2`: PASS. Desktop exit left the client process and endpoint
  available; the desktop evidence reported the client as still running.
- `LIVE-RC118-3`: PASS. A real IPC `Ping` returned `Running`, a real IPC
  shutdown returned `SHUTDOWN_ACCEPTED`, the endpoint was removed, and the
  disposable SQLite database remained readable.
- No stress run longer than the established focused tests was started.

## Packages and Artifacts

- `./deploy/packages/build.sh --format=all`: PASS. Built current-source DEB
  and RPM artifacts for version `0.1.0`; the release artifact manifest binds
  all three production binaries to the current source fingerprint.
- `scripts/validate-release-artifacts.sh`: PASS; 3 binary entries verified.
- `cargo test -p synveil-metadata --test release_artifact_units --locked`:
  PASS; 7 passed.
- DEB payload inspection: PASS; real `ar`/tar Debian format with expected
  `debian-binary`, control, and data members. `dpkg-deb` is not installed, so
  the builder's documented equivalent-format fallback was used.
- RPM metadata/payload inspection: PASS; version `0.1.0-1`, x86_64 payload,
  expected package-owned paths, and no credentials.
- `cargo test -p synveil-metadata --test linux_install_lifecycle --locked`:
  PASS; 37 passed. Install, upgrade, ordinary uninstall, purge separation,
  symlink containment, and data-preservation cases passed.
- Artifact hashes from the final package build:
  - DEB: `eb122c44e4aa5af6cc4bccbf9de0d7ee09222336a27a69b20d526a8e3314c8b2`.
  - RPM: `7a958bb1836fa289dc342ec532d9a853c195e1cba59a1d0da941c4560af9a059`.
  - Client: `a35b6a427d12402f4ef17e62f99ff8c5e8eb6008e6dfb15834832dd78293951f`.
  - Desktop: `44f1318755abe84a12328c693edd423c4afb369e17b0cde7fe59fd94633ee9b0`.
  - Maintenance: `7533fdea03e04639e4ba07cbefd5482cfd2ba868f1e4a3bc48006d6bb673d26f`.
- Artifact strings scans found no developer checkout paths, credentials,
  authorization headers, or private-key material.

The first artifact-unit invocation detected a stale pre-validation manifest
whose source fingerprint did not match current main. The canonical package
builder was rerun from the final source, after which artifact validation and
all 7 artifact tests passed. No source defect was found and no code fix was
needed.

## Documentation and CI

- `./scripts/validate-docs.sh`: PASS; all six documentation checks passed.
- Release-facing version, package names, schema counts, support matrix,
  upgrade, backup, troubleshooting, limitations, and release notes were
  consistent with current main.
- CI workflow audit: PASS for the existing Linux, Windows, package, and
  PostgreSQL job definitions. No CI correction was necessary.
- Full Prompt113 two-root reproducibility rebuild was not repeated because
  Prompt118 did not change release reproducibility/build infrastructure.

## Environment Status

- PostgreSQL 17 live acceptance: **BLOCKED_BY_ENVIRONMENT**. `initdb`,
  `pg_ctl`, `postgres`, `psql`, and `pg_isready` are unavailable;
  `SYNVEIL_TEST_DATABASE_URL` is unset; Docker is installed but the daemon
  socket is inaccessible. Workspace PostgreSQL tests therefore remain
  correctly ignored and are not counted as passed.
- Windows cross-build, package construction, PE inspection, named-pipe,
  Task Scheduler, and native runtime gates: **BLOCKED_BY_ENVIRONMENT**. This
  host has no Windows target/toolchain, MinGW, Qt Windows deployment tool, or
  native Windows runtime. Source/static Windows packaging policy tests passed.
- macOS native validation: **KNOWN_LIMITATION** for the v0.1 support matrix.

## Blocker Classification

- `PASS`: all locally executable mandatory Prompt118 gates listed above.
- `BLOCKED_BY_ENVIRONMENT`: PostgreSQL 17 live acceptance; Windows native,
  cross-build, package, PE, named-pipe, Task Scheduler, and runtime gates.
- `KNOWN_LIMITATION`: macOS is unsupported in v0.1.
- `RC_BLOCKER`: none.

## Prompt118 File Manifest

Only this reviewed source file is added by Prompt118:

- `docs/PROMPT118_RELEASE_CANDIDATE_VALIDATION.md`

Generated binaries, packages, temporary databases, disposable roots, logs,
and test artifacts remain untracked/ignored and are not part of the commit.
