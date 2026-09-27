# Synveil v0.1.0 final release

Prompt120 release-gate record, dated 2026-09-27. This record introduces no
product, dependency, architecture, platform, API, UI, or migration change.
The sole intended repository change is this release record.

## Source and freeze

- Authoritative Prompt119 freeze SHA:
  `fa86bc038e597b304b6dc05d8b192c053b4d871f`.
- Previous gate: `SYNVEIL_V0_1_RELEASE_FREEZE_READY`.
- Starting branch: `main`.
- Starting HEAD, origin/main, and live remote main all matched that freeze SHA.
- Starting worktree was clean; no intervening main commit existed.
- No local or remote `v0.1.0` tag existed at the initial collision check.
- Product version: `0.1.0`.
- The v0.1.0 release tag will identify the final commit containing this record.
  No future commit or tag SHA is fabricated here.

## Frozen version and schema

All workspace packages inherit version `0.1.0` from Cargo.toml. The canonical
version helper derives DEB/RPM metadata from it; the Windows builder uses the
same workspace version for its manifest and filename. Public release notes,
operations, packaging, support, security, and upgrade documentation agree with
the v0.1 release contract. The desktop-entry specification version and
dependency versions are not product versions.

- Server SQL migrations: `36`.
- Client SQL migrations: `7`.
- LOCAL_SCHEMA_VERSION: `7`.
- No migration or schema declaration changed during Prompt120.

## Standard local validation

Commands used `CARGO_BUILD_JOBS=1`. Quality gates validate the frozen product
source; the only subsequent source addition is this engineering record.
Documentation and diff checks are also run with this record present before
commit. Generated logs, databases, packages, and fixture scripts are excluded
from the source commit.

| Gate | Command | Result |
| --- | --- | --- |
| Format | `cargo fmt --all -- --check` | PASS |
| Workspace check | `cargo check --workspace --locked` | PASS |
| Strict Clippy | `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | PASS |
| Workspace tests | `cargo test --workspace --locked` | PASS; 979 passed, 0 failed, 444 ignored, 86 result summaries |
| Dependency policy | `cargo deny check` | PASS; advisories, bans, licenses, sources |
| Diff hygiene | `git diff --check` | PASS |
| Documentation | `./scripts/validate-docs.sh` | PASS; all seven checks |
| Desktop | `./scripts/test-desktop-ui.sh` | PASS; uninterrupted wrapper, 70 tests passed, strict Clippy, debug/release builds, qmllint, both offscreen launches |

The desktop wrapper is run once and includes desktop Rust tests, strict
Clippy, debug/release builds, qmllint, and both offscreen launches. Offscreen
startup is not a claim of interactive tray or native Windows acceptance.
The disposable child left by the smoke wrapper was identified by its deleted
fixture cwd and SQLite descriptors and gracefully terminated; pre-existing
client processes were preserved.

## Linux live smoke

PASS using isolated non-secret configuration and disposable SQLite/runtime
paths. Real release executables were launched, and desktop logs confirmed
`Connected / Current` over the profile-scoped Unix socket. Desktop exited
with status 0; exact client PID `927244` remained alive and answered Ping
with `RUNNING`. A real IPC shutdown returned `ShutdownAccepted`; client exited
with status 0, the endpoint disappeared, and SQLite remained readable with
integrity `ok` and migration version `7`.

The first temporary fixture attempt stopped on an assertion expecting
lowercase `running`; the protocol correctly returned `RUNNING`. The fixture
assertion was corrected, and the complete smoke passed. No product source was
changed for this fixture issue, and both fixture attempts reaped their own
processes. No long-duration stress run was started.

## Architecture and product safety

Source inspection and current passing coverage reconfirm:

- synveil-client owns DesktopSyncHost and synchronization runtime; the desktop
  uses DesktopController and background process management.
- GUI exit does not terminate synchronization, as the exact-PID live smoke
  also demonstrates.
- Durable authentication credentials remain in platform SecretStore; the
  process manifest carries non-secret identity/references, and desktop
  authentication inputs are not durable credentials.
- RootUnavailable fences/defer work; unavailable roots are not mass deletion.
- OutcomeUnknown requests authoritative refresh/reconciliation without blind
  replay.
- Future/unknown/malformed schemas fail closed without automatic reset.

The workspace suite includes existing focused authentication boundary,
profile configuration, library onboarding, existing non-empty root bootstrap,
bounded scheduling, pause/resume, conflicts, root loss, recovery, ambiguous
outcomes, and durable restart tests. Desktop control IPC (5 tests) and
controller/reconnect (6 tests) passed. No PostgreSQL-dependent harness was
introduced.

## SQLite migration safety

PASS; all six current `local_migrations::tests` passed in the workspace run:
fresh schema and every frozen historical prefix through version 7, reopen,
future/damaged schema rejection, non-database preservation, failed migration
transaction rollback/restart, v6 durable data preservation, and rejection of
unrecognized historical columns. All use disposable databases; no real user
database was opened or changed.

## Packages, integrity, and scans

Pre-commit workspace package suites: PASS; release artifact units 7 passed,
production packaging units 11 passed, Linux native packaging units 26 passed,
Linux install lifecycle 37 passed, and Windows source/static packaging policy
3 passed. The current source fingerprint/hash validator and real artifact
corruption-rejection tests passed before the desktop wrapper. The optional
second-root manifest is absent, so two-root parity is not claimed. Native
Windows ZIP and dpkg-deb inspections are unavailable. DEB final payload
inspection uses the builder's real ar/tar equivalent-format fallback; RPM
inspection uses installed RPM tooling. The final clean-commit canonical package
build and payload audits are deliberately performed after push, as required
by the release sequence below.

The mandatory final source-bound artifact gate runs after this record is
committed and pushed, from a clean checkout of that exact commit. Its manifest
must record the final source revision and actual source fingerprint. Binary
hash/size/build-ID, current DEB/RPM payload parity, version/metadata, private
path/secret scans, and intentional-corruption rejection must pass before the
annotated tag is created. A failure blocks tagging; expected hashes are never
edited to bypass a mismatch.

Post-commit artifact sizes, hashes, manifest identity, and actual final gate
results are reported in the final Prompt120 handoff/external evidence. They
are not inserted into this file through a self-referential second commit.

The established Prompt113 reproducibility contract is unchanged: locked,
path-remapped, non-incremental release builds, stable Qt inputs/hash seed,
sorted Qt module links, source-bound manifests, and fail-closed validation.
The two-root clean reproducibility and long stress runs are not repeated.

Release-facing documentation scans found no private keys, access tokens,
real Authorization headers, or developer checkout/home paths. Candidate
database URLs were explicitly redacted or clearly local development/test
examples. Production binaries and package payloads are scanned separately;
test/build helper paths are not production payloads.

## Platform and publication classification

| Item | Classification and evidence |
| --- | --- |
| Linux native validation | PASS; current workspace, desktop, IPC, live smoke, and package evidence |
| Windows source/static policy | PASS; current Linux-hosted policy tests |
| Windows cross-build | BLOCKED_BY_ENVIRONMENT; no Windows target/linker/Qt SDK |
| Windows package construction | BLOCKED_BY_ENVIRONMENT; no real current Windows PE/Qt closure |
| Windows PE dependency inspection | BLOCKED_BY_ENVIRONMENT; no current Windows package |
| Windows native Qt/process and named-pipe runtime | BLOCKED_BY_ENVIRONMENT; Linux-only host |
| Windows Task Scheduler live integration | BLOCKED_BY_ENVIRONMENT; requires native Windows |
| PostgreSQL 17 live acceptance | BLOCKED_BY_ENVIRONMENT; binaries unavailable, database URL unset, Docker daemon inaccessible |
| macOS | KNOWN_LIMITATION; unsupported/deferred in v0.1 |
| iOS, Android, Synveil OS | KNOWN_LIMITATION; deferred/unsupported in v0.1 |
| GitHub Release publication | Not applicable to the established repository process; package CI uploads unsigned workflow artifacts, with no GitHub Release publishing workflow |

The accepted Prompt118/119 environment classification is preserved. Ignored
PostgreSQL tests are not counted as passes. No new PostgreSQL installation,
Windows SDK/toolchain, Windows ZIP, PE asset, or placeholder artifact is
created. The source tag can complete without optional GitHub Release
publication. No GitHub Release or asset publication is claimed by this record.

Known limitations remain: operator-managed server/PostgreSQL/TLS/backups, no
bundled server/database in desktop packages, unsigned packages, no package
repository publication/automatic update, no guaranteed automatic downgrade or
transactional whole-package rollback, filesystem naming differences, and
credentials excluded from ordinary configuration/SQLite backups. No end-to-end
encryption or zero-knowledge storage claim is made.

## Release blockers

Pre-commit classification: `RELEASE_BLOCKER: none` after mandatory locally
executable gates above pass. Exact-final-commit packages and artifacts remain
a mandatory post-push gate before tagging. Environment absence under the
accepted freeze classification is not a product release blocker.

## Canonical final release commands

```sh
git add docs/PROMPT120_FINAL_RELEASE.md
git diff --cached --name-only
git diff --cached --stat
git diff --cached --check
git commit -m "chore: release synveil v0.1.0"
git push origin main
git rev-parse HEAD origin/main
git status --porcelain
CARGO_BUILD_JOBS=1 ./deploy/packages/build.sh --format=all --output-dir=target/packages
scripts/validate-release-artifacts.sh --manifest=target/packages/SYNVEIL-LINUX-ARTIFACT-MANIFEST.txt
CARGO_BUILD_JOBS=1 cargo test -p synveil-metadata --test release_artifact_units --locked -- --nocapture
CARGO_BUILD_JOBS=1 cargo test -p synveil-metadata --test production_packaging_units --locked -- --nocapture
CARGO_BUILD_JOBS=1 cargo test -p synveil-metadata --test linux_native_packaging_units --locked -- --nocapture
sha256sum target/packages/synveil_0.1.0_amd64.deb target/packages/synveil-0.1.0-1.x86_64.rpm target/release/synveil-client target/release/synveil-desktop target/release/synveil-scheduled-maintenance-once
# Only after exact-commit artifact gates pass and no release blocker remains:
git tag -a v0.1.0 -m "Synveil v0.1.0"
git rev-list -n 1 v0.1.0
git push origin v0.1.0
git ls-remote origin refs/heads/main refs/tags/v0.1.0 'refs/tags/v0.1.0^{}'
git rev-parse HEAD origin/main
git status --porcelain
```

## Explicit Prompt120 staged file manifest

- `docs/PROMPT120_FINAL_RELEASE.md` — this release-gate record only.

No generated binaries/packages/manifests/checksums, target files, logs,
databases, runtime sockets, fixture roots/scripts, credentials, migration,
product source, or build infrastructure changes are staged. After the remote
annotated tag is verified, v0.1.0 is immutable: any future fix belongs to a new
version and must not move, delete/recreate, or force-update this tag.
