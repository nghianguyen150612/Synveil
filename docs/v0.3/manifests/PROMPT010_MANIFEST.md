# Synveil v0.3 — PROMPT010 Manifest

**Status: blocked for production artifact-plan integration.** P010 adds a
provisional, fail-closed contract increment. It does not claim production
artifact provenance or an available installation plan.

## Repository preflight

| Item | Evidence |
|---|---|
| Repository | `https://github.com/nghianguyen150612/Synveil` |
| Starting branch/SHA | `origin/develop/v0.3` / `1ac67076e9e1e8c475b4a69578273d3f777a6ae9` (exact expected SHA) |
| Main at preflight | `604499fce0dfd134d82da3d5bb5d4a055c5eea8d` |
| Working branch | `feat/v03-p010-artifact-plan-builder` |
| Roadmap | No authoritative v0.3 P001–P060 roadmap found. v0.2 P010 is the installer error model, recorded implemented but untested; this prompt is provisional and numbering conflicts with that older roadmap. |
| P006 #165 / repair #167 | Both merged. P006's joined PostgreSQL/ObjectStore owner-to-byte integration coverage remains unresolved. |
| P007 #166 / P008 #168 / P009 #169 | All merged. |
| P009 qualification | Mixed/failed, not qualified: hosted Linux Qt, quality, docs, source-contract, desktop/QML and standard-user-native checks failed; Windows Qt passed. Other green checks do not override these failures. |
| P028 | Windows standard-user/native acceptance is incomplete/blocked; full GUI/logon/IPC evidence is absent. |
| P036 | Production artifact identity and approved public keyring are absent; keyring materialization failed. |
| P004 | D02, D03, D11 and D14 remain pending. |

## Implemented source changes

- `crates/install-engine/src/artifact_plan.rs`: bounded read-only evidence
  categories, platform and channel types, artifact-source interface and typed
  fail-closed blockers, plus a synthetic-only bounded native package fixture
  validated through `InstallerEngine::validate()`. There is no production
  source implementation, trusted root, effect catalog, plan construction, or
  acquisition path.
- `crates/install-engine/src/lib.rs`: exports the plan boundary types.
- `crates/install-engine/src/review_contract.rs`: fingerprint binds manifest
  generation and existing-installation observation generation; focused tests
  assert both invalidate an existing fingerprint.
- `crates/desktop/src/installer_preflight.rs` and
  `crates/desktop/src/installer_wizard.rs`: report that authenticated release
  evidence and approved production trust policy are unavailable.
- `docs/v0.3/installer/P010_ARTIFACT_PROVENANCE_PLAN_BUILDER.md`: detailed
  design, limits, channel matrix and P011 prerequisites.

The existing P006 Python channel, manifest, signature, and download validator
contracts were inspected and are reused by reference. They were not translated
to Rust or invoked with signing material. Existing engine tests provide
synthetic-only examples of `InstallerEngine::validate()` and P009 snapshot
validation. No production plan was available to validate.

## Trust boundaries and security limits

Authenticity and integrity remain separate. The P005 release manifest is an
inventory; P006 authenticates exact metadata only when rooted in a local pin or
approved local public key policy. P010 found no independently approved
production key policy, final signed artifacts, P036 production inventory, or
revocation policy. A digest, unsigned CI artifact, fixture, filename, URL,
HTTPS transport, local path, or user-supplied publisher label cannot create
verified evidence. No `VerifiedReleaseArtifact` is created by the new code.

No installer effect, journal, subprocess, network request, package mutation,
sudo/pkexec/UAC/privileged IPC, or OS authorization is added. The UI retains
“Preview only — installation is not started.” There is no Install button.

## Validation results

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | PASS after `cargo fmt --all` |
| `cargo test -p synveil-install-engine --locked` | PASS; 17 unit tests plus all integration suites, including existing AppImage, lifecycle, engine and Linux package coverage |
| `cargo clippy -p synveil-install-engine --all-targets --locked -- -D warnings` | PASS |
| `./scripts/validate-release-channel.sh` | PASS; 101 tests |
| `./scripts/validate-release-manifest.sh` | PASS; 36 tests |
| `./scripts/validate-release-download.sh` | PASS; 79 acquisition/authentication tests and 36 manifest tests |
| `python3 -m unittest discover -s tests/installer_security -p 'test_security.py'` | PASS; 23 tests |
| `./scripts/validate-docs.sh` | PASS; all documentation validation units |
| `git diff --check` | PASS before final doc update; rerun for final tree |
| `cargo test -p synveil-desktop --locked` | BLOCKED before crate compile: system `dbus-1.pc` / development library absent |
| `cargo clippy -p synveil-desktop --all-targets --locked -- -D warnings` | BLOCKED before crate compile: same missing D-Bus development dependency |
| `./scripts/test-desktop-ui.sh` | BLOCKED: Qt shim resolves missing `/usr/lib/qt5/bin/qmake`; no Qt QML compile/smoke ran |
| `./scripts/validate-release-artifacts.sh` | BLOCKED: generated `target/packages/SYNVEIL-LINUX-ARTIFACT-MANIFEST.txt` is absent |
| Native Linux/Windows P010 CI | Not yet run on the PR head; P009 exact-head baseline has failing Linux Qt and passing Windows Qt, and is not P010 qualification |

Cargo is installed at `/home/agent/.cargo/bin/cargo`, outside the initial
`PATH`. No native Windows runtime is available in this environment. Hosted
and native evidence is not inferred from local engine tests.

## P011 handoff

P011 requires independent production trust-root provisioning and rotation,
approved revocation and anti-downgrade rules, approved per-channel effect
catalogs and owners, exact Linux/Windows native acceptance, preservation and
verification contracts, and a narrow authenticated-evidence-to-P009 builder
integration. P010 does not start P011.
