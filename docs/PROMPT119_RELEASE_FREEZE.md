# Synveil v0.1 release freeze

This engineering record freezes the validated Synveil v0.1 release candidate
for the Prompt120 release gate. It is a source, schema, platform, build, and
release-input manifest; it is not public marketing copy and it does not create
a tag, publish assets, or change product behavior.

## Freeze identity

- Freeze date: `2026-09-27`.
- Starting authoritative HEAD: `5ac915bdbd625a57e586c94c61f53ffb76a90004`.
- Starting `origin/main`: `5ac915bdbd625a57e586c94c61f53ffb76a90004`.
- Starting worktree: clean.
- Previous gate: `SYNVEIL_FINAL_RELEASE_CANDIDATE_VALIDATED`.
- Product version: `0.1.0`.
- Final Prompt119 commit: the commit containing this record; the actual SHA is
  reported after the single commit is pushed. No Git tag is created here.

No feature, product behavior, migration, platform, dependency, UI, or
architecture change is included in this freeze. The only source change beyond
this record is the small deterministic check in `scripts/validate-docs.sh`.

## Version and schema

- The authoritative workspace version is `[workspace.package] version` in
  `Cargo.toml`, and it is `0.1.0`.
- Packaging derives its version from `Cargo.toml` through
  `deploy/packages/common/version.sh`; DEB, RPM, Windows package metadata, and
  release-facing v0.1 documentation agree with `0.1.0`.
- Server migrations: `36` files under `migrations/`; latest is
  `20260910000000_sync_retention_handoff_proofs.sql`.
- Client migrations: `7` files under `crates/client-sync/migrations/`; latest is
  `0007_profile_reconfiguration.sql`.
- LOCAL_SCHEMA_VERSION: `7`, declared in `crates/client-sync/src/lib.rs`.
- No migration or schema declaration was added or modified for Prompt119.

The deterministic count is:

```sh
find migrations -maxdepth 1 -type f -name '*.sql' | wc -l   # 36
find crates/client-sync/migrations -maxdepth 1 -type f -name '*.sql' | wc -l   # 7
rg '^pub const LOCAL_SCHEMA_VERSION: i64 = 7;' crates/client-sync/src/lib.rs
```

Any count other than `36 / 7 / 7`, or any migration diff, is a
`RELEASE_BLOCKER`.

## Platform support and evidence

### Supported v0.1 targets

- Server: Linux, with operator-managed PostgreSQL, object storage, HTTPS
  termination, and supervision.
- Desktop/client: Linux x86_64 native package/runtime path.
- Desktop/client: Windows x86_64 portable ZIP policy. The ZIP is unsigned and
  portable; it is not an installer, Windows service, or database bundle.

### Deferred or unsupported targets

- macOS: deferred and unsupported in v0.1.
- iOS: deferred and unsupported in v0.1.
- Android: deferred and unsupported in v0.1.
- Synveil OS: deferred and unsupported in v0.1.

### Current validation classification

- Linux native validation: **PASS**. Prompt118 desktop, IPC, Linux live RC,
  package lifecycle, and artifact checks are the established evidence.
- Windows source/static policy: **PASS**. Windows packaging, path, manifest,
  and policy tests run on Linux.
- Windows cross-build: **BLOCKED_BY_ENVIRONMENT**. No Windows Rust target,
  linker, or Windows Qt SDK is available in this environment.
- Windows package construction: **BLOCKED_BY_ENVIRONMENT**. No Windows PE
  binaries or native Qt deployment closure are available here.
- Windows PE inspection: **BLOCKED_BY_ENVIRONMENT** because no current Windows
  package exists in this environment.
- Windows native runtime, named pipe, and Task Scheduler: **BLOCKED_BY_ENVIRONMENT**;
  they require a native Windows runner.
- PostgreSQL 17 live acceptance: **BLOCKED_BY_ENVIRONMENT**. PostgreSQL
  binaries and a usable Docker daemon are unavailable; `SYNVEIL_TEST_DATABASE_URL`
  is unset. Prompt118's ignored live tests remain correctly classified.
- macOS native validation: **KNOWN_LIMITATION** and outside the v0.1 support
  matrix.

The Prompt118 classification is preserved. In particular, no Windows or
PostgreSQL environment blocker is rewritten as `PASS` without new evidence.

## Release checklist

- [x] Version consistent — **PASS** (`0.1.0`).
- [x] Schema frozen — **PASS** (`36 / 7 / 7`).
- [x] Workspace validation previously PASS — **PASS** in Prompt118.
- [x] Desktop RC validation PASS — **PASS** in Prompt118.
- [x] Linux live RC PASS — **PASS** in Prompt118.
- [x] Package lifecycle PASS — **PASS** in Prompt118.
- [x] Artifact integrity PASS — **PASS** in Prompt118 and current integrity tests.
- [x] Docs validation PASS — **PASS** after this record is included.
- [x] Linux package builder available — **PASS**; canonical builder is present.
- [x] Windows limitations classified — **BLOCKED_BY_ENVIRONMENT**, not a pass.
- [x] PostgreSQL live limitation classified — **BLOCKED_BY_ENVIRONMENT**, not a pass.
- [x] No release blocker known — **PASS**; `RELEASE_BLOCKER: none`.
- [x] Worktree clean before release gate — verified before staging and again
  after push.

## Canonical build and validation paths

There is one authoritative release recipe per package family:

- Linux package build: `./deploy/packages/build.sh --format=all --output-dir=target/packages`.
- Linux artifact validation:
  `scripts/validate-release-artifacts.sh --manifest=target/packages/SYNVEIL-LINUX-ARTIFACT-MANIFEST.txt`.
- Windows package build: `./deploy/packages/build-windows.sh --output-dir=target/windows-packages`.
  Native Windows uses `windeployqt` plus `llvm-readobj` or `dumpbin`; the
  documented cross-build fallback requires real Windows PE binaries and a
  Windows Qt prefix. The builder validates the ZIP manifest and PE import
  closure; current Linux cannot claim that artifact exists.
- Release package policy and CI references are `docs/en/RELEASE_PACKAGING.md`,
  `.github/workflows/linux-packages.yml`, and `.github/workflows/ci.yml`.

Prompt118's Linux package build established the release evidence. Because the
new freeze record and validator are included in the source fingerprint,
Prompt119 regenerated the canonical current-source DEB/RPM artifacts and
validated their source-bound manifest. Prompt119 does not repeat the Prompt113
two-root reproducibility run because release-build infrastructure and product
inputs are unchanged.

## Reproducibility and integrity contract

The frozen Prompt113 contract remains active:

- `deploy/packages/common/reproducible.sh` supplies stable source revision and
  fingerprinting, `SOURCE_DATE_EPOCH`, non-incremental release builds, and
  stable checkout/Cargo/temporary path remapping.
- Qt/QML inputs are copied into disposable build output with stable timestamps;
  `QT_HASH_SEED=0` stabilizes QML tool behavior.
- The vendored CXX-Qt build helper sorts Qt module link inputs, preventing
  randomized link ordering.
- The Linux artifact manifest binds the exact source fingerprint, toolchain,
  binary build IDs, sizes, and SHA-256 values to the generated artifacts.
- Windows manifests bind version, platform, source fingerprint, and every ZIP
  file's SHA-256 and size; native package construction also audits PE imports.
- `release_artifact_units` proves modified artifacts fail the hash check. An
  expected artifact with a matching hash is **PASS**; a modified artifact with
  a mismatch is **FAIL**. Integrity checks remain fail closed.

## Release input inventory

These are the reviewed source inputs for traceability. Generated `target/`
artifacts, packages, logs, temporary databases, caches, and forensic binaries
are not release inputs and are not staged.

- Workspace/dependency metadata: `Cargo.toml`, `Cargo.lock`, `deny.toml`.
- Server crates and binaries: `crates/api`, `crates/auth`, `crates/core`,
  `crates/metadata`, `crates/object-store`, `crates/platform`,
  `crates/storage`, and the server binaries under `crates/api/src/bin`.
- Client and desktop crates: `crates/client-sync`, `crates/client`, and
  `crates/desktop`.
- Qt/QML/native desktop resources: `crates/desktop/qml/Main.qml`,
  `crates/desktop/build.rs`, and `crates/desktop/src/native`.
- API contract: `api/openapi.yaml`.
- Frozen schema: `migrations`, `crates/client-sync/migrations`, and their
  migration documentation in `migrations/README.md`.
- Packaging scripts/templates: `deploy/packages/build.sh`,
  `deploy/packages/build-windows.sh`, `deploy/packages/common`,
  `deploy/packages/debian`, and `deploy/packages/rpm`.
- Install/uninstall assets: `deploy/install/install.sh`,
  `deploy/install/uninstall.sh`, `deploy/install/MANIFEST`, and
  `deploy/install/common.sh`.
- Service/runtime assets: `deploy/systemd`, `deploy/systemd-user`,
  including `deploy/systemd-user/synveil-client.service`, plus
  `deploy/sysusers.d`, `deploy/tmpfiles.d`, and `deploy/config`.
- Windows packaging policy: `deploy/packages/build-windows.sh`,
  `docs/en/RELEASE_PACKAGING.md`, `docs/en/PLATFORM.md`, and the Windows CI
  gates in `.github/workflows/ci.yml`.
- Release-facing documentation: `README.md`, `docs/README.md`,
  `docs/en/RELEASE_OPERATIONS.md`, `docs/en/RELEASE_PACKAGING.md`,
  `docs/en/RELEASE_NOTES_v0.1.md`, `docs/en/UPGRADE_SAFETY.md`,
  `docs/en/SECURITY.md`, `docs/en/DEPLOYMENT.md`, `docs/en/TESTING.md`,
  `docs/en/PLATFORM.md`, and their maintained Vietnamese release counterparts
  under `docs/vi`.
- Legal notices: `LICENSE` and `deploy/NOTICE`.

## Expected release outputs

### Linux

- `synveil-client`.
- `synveil-desktop`.
- `synveil-scheduled-maintenance-once`.
- DEB: `synveil-0.1.0_amd64.deb`.
- RPM: `synveil-0.1.0-1.x86_64.rpm`.
- Generated release manifest:
  `target/packages/SYNVEIL-LINUX-ARTIFACT-MANIFEST.txt`.

The workspace also builds `synveil-api` and `synveil-worker` as server
deployment components; they are not included in the desktop DEB/RPM payload.
Linux packages include the current `LICENSE` and `NOTICE`.

### Windows

- Expected policy output: `synveil-0.1.0-windows-x86_64.zip`.
- Expected layout: `synveil-client.exe`, `synveil-desktop.exe`, `qt.conf`,
  the audited Qt/QML/plugin/C++ runtime closure, `LICENSE`, `NOTICE`, and
  `SYNVEIL-MANIFEST.txt`.
- No Windows ZIP is claimed as generated in the current environment;
  cross-build, package construction, PE inspection, and native runtime remain
  **BLOCKED_BY_ENVIRONMENT**.

## Release-facing documentation and security freeze

The existing v0.1 release notes accurately cover the self-hosted server,
native desktop architecture, background client, authentication, profile/server
setup, library onboarding, existing-folder bootstrap, bidirectional sync,
pause/resume, conflicts, recovery, Linux packaging, and the qualified Windows
portable ZIP/runtime policy. Known limitations remain explicit, including
unsupported/deferred platforms, no bundled server/database, no signing or
publication, no automatic update, no guaranteed downgrade/transactional
rollback, and the Windows/PostgreSQL environment gates. Internal Prompt
history is not part of public release notes.

The security declaration matches implementation:

- `SecretStore` owns durable device credentials; desktop UI/configuration owns
  only references and non-secret profile metadata.
- Local control IPC is user-scoped (Unix peer credentials on Linux and
  owner-restricted named pipes on Windows).
- There is no public control TCP listener.
- Filesystem containment, overlap, root, and package-path protections are
  enforced at their respective boundaries.
- No end-to-end encryption or zero-knowledge storage claim is made.

The upgrade and data-safety contract is unchanged:

- Forward migrations are supported for the frozen schema history.
- Unknown/future schema fails closed; automatic downgrade is not guaranteed.
- Ordinary uninstall preserves state; explicit purge is bounded to owned
  allowlisted state.
- `ROOT UNAVAILABLE != DELETE EVERYTHING`.
- `OutcomeUnknown != blind replay`.
- `existing non-empty local folder != historical deletion set`.
- `unknown/newer schema != reset database`.

## Repository hygiene and scans

- Tracked source contains no generated packages, temporary databases,
  reproducibility forensic binaries, logs, local credentials, or editor
  temporary files at the clean starting HEAD.
- Release-facing secret scan found no private keys, real access tokens,
  authorization headers, credentials, or developer absolute paths. Clearly
  synthetic examples remain allowed by policy.
- `LICENSE` and `NOTICE` are included by both Linux and Windows packaging
  paths; no license or notice policy is changed.
- CI/release workflow audit found no concrete release-critical drift. Existing
  package, Windows, Linux, docs, and PostgreSQL gates remain authoritative.

## Prompt118 evidence and validation

Prompt118 is the release-candidate gate referenced by this freeze:
`docs/PROMPT118_RELEASE_CANDIDATE_VALIDATION.md`. Its locally executable
workspace, desktop, Linux live RC, package lifecycle, artifact integrity, and
documentation gates were PASS. Its Windows native and PostgreSQL live evidence
remain environment-classified as recorded above.

Prompt119 validation is intentionally light and release-focused:

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `git diff --check` | PASS |
| `cargo deny check` | PASS |
| `./scripts/validate-docs.sh` | PASS, including DOC-UNIT-7 freeze drift checks |
| Deterministic schema/version checks above | PASS, `0.1.0 / 36 / 7 / 7` |
| `./deploy/packages/build.sh --format=all --output-dir=target/packages` | PASS; current-source DEB/RPM build |
| `scripts/validate-release-artifacts.sh --manifest=target/packages/SYNVEIL-LINUX-ARTIFACT-MANIFEST.txt` | PASS; 3 Linux binaries verified |
| Release artifact unit tests | PASS; 7 passed |
| Production packaging unit tests | PASS; 11 passed; environment-dependent Windows/DEB inspections skipped |
| Full two-root reproducibility rebuild | Not repeated; unchanged since Prompt113 |
| Full Prompt118 workspace suite | Not repeated; no Rust/product code changed |

## Blocker classification

- `PASS`: version, schema, Prompt118 RC evidence, Linux native/package paths,
  docs, integrity contract, legal assets, and release input inventory.
- `BLOCKED_BY_ENVIRONMENT`: PostgreSQL 17 live acceptance and all Windows
  cross/native/package/runtime gates listed above.
- `KNOWN_LIMITATION`: deferred/unsupported v0.1 platforms and documented
  package/signing/publication/rollback limitations.
- `RELEASE_BLOCKER: none`.

## Explicit Prompt119 staged file manifest

Only these reviewed paths are intended for the single Prompt119 commit:

- `docs/PROMPT119_RELEASE_FREEZE.md` — this immutable engineering freeze
  record.
- `scripts/validate-docs.sh` — DOC-UNIT-7 deterministic freeze-record drift
  check.

No migration, Rust/product source, package, target output, generated artifact,
workflow, license, or release tag is staged by Prompt119.
