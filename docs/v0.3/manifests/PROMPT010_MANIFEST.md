# Synveil v0.3 — PROMPT010 Manifest

**Result: provisional fail-closed source increment; production provenance and
plan preview blocked.** This is not production artifact integration and is not
installation readiness.

## Repository and remote preflight

| Item | Evidence |
|---|---|
| Repository | `https://github.com/nghianguyen150612/Synveil` |
| Expected starting SHA | `1ac67076e9e1e8c475b4a69578273d3f777a6ae9` |
| Actual starting SHA | `1ac67076e9e1e8c475b4a69578273d3f777a6ae9` |
| Base | `origin/develop/v0.3` at exact expected SHA |
| Main | `604499fce0dfd134d82da3d5bb5d4a055c5eea8d`; unchanged |
| Roadmap | Named P001–P060 roadmap absent; increment is provisional |
| P006 #165 / formatting #167 | Both merged |
| P007 #166 / P008 #168 | Both merged |
| P009 #169 | Merged at exact expected base SHA |
| P009 hosted CI | Not qualified: merged head failed Rust quality, Linux Qt 6, Windows standard-user native, and other presentation/workflow checks; Windows Qt 6 passed. P008 #168's inherited artifact-identity report failed while materializing the approved public keyring. |
| P006 PostgreSQL/ObjectStore | Joined metadata-to-object-byte owner integration remains unimplemented per P006 records |
| P028 | Windows `standard-user-native` failed in “Record required unavailable surfaces honestly”; native acceptance remains withheld |
| P036 | Production PG17/edge artifact identity is missing; clean-machine and Phase C evidence gates were skipped |
| P004 | D02, D11 and D14 remain `RECOMMENDED_PENDING_APPROVAL` |

Current P009 exact-head examples: merged head `c4afc7dc717efa094cc007f3f7d6f4193a71307f` had failed `Quality (format and clippy)`, `Native desktop UI (Linux Qt 6)`, and `standard-user-native`; `Native desktop UI (Windows Qt 6)` passed. The Windows standard-user job failed at run `40441400135`, job `122009197619`. The run's named failed step was “Record required unavailable surfaces honestly.” P009's inherited P008 #168 record identifies an `artifact-identity` job failure at “Materialize approved public publisher keyring.” Historical PR merge exceptions are not qualification.

## Implementation

- Reused the release trust system's vocabulary and fail-closed boundaries from
  `scripts/release_channel.py`, `scripts/release_manifest.py`,
  `scripts/release_signature.py`, `scripts/release_download.py`, and their
  signing/materialization tools. No Python verifier was translated to Rust and
  no arbitrary process or network behavior was added.
- Added typed `ArtifactEvidenceKind` observations and
  `AuthoritativePlanBuilder::build`. Missing, local-untrusted,
  invalid/stale/revoked/unsupported, ambiguous, unsupported
  platform/architecture, platform/channel mismatch, and invalid scope paths
  remain blocked. Candidate observations cannot manufacture authenticated
  evidence. Read-only desktop preflight maps its bounded observations into
  this API and passes blocker categories to the Wizard view.
- Recognized only Ubuntu 24.04 x86-64 DEB, Fedora 42 x86-64 RPM, current-user
  Linux AppImage, Windows 11 x86-64 per-user Setup, Windows 11 x86-64 portable
  ZIP, and existing-installation maintenance as channel identities. All remain
  production blocked. No production trust root or approved effect catalog is
  provisioned to the wizard.
- Added a crate-private P009 handoff that requires verified-release evidence,
  validates through `InstallerEngine::validate`, then calls the existing
  crate-private snapshot constructor. Desktop/QML cannot construct a review
  snapshot; no production builder reaches this handoff.
- A changed artifact digest, authenticated manifest generation, or existing
  installation-state generation produces a different fingerprint and rejects
  the prior review. P010 has no artifact-byte source or use-time rehash, so it
  cannot observe an on-disk replacement; that part of TOCTOU protection remains
  blocked until a trusted acquisition source is connected.
- The Wizard continues disabled/read-only with no Install action and states the
  missing independent publisher trust, effect catalog, and pending P004
  decisions. It preserves “Preview only — installation is not started.”
- No install engine `execute`, adapter effect, acquisition, persistent journal,
  sudo/pkexec/UAC/native authorization, or OS mutation is called.

## Existing evidence and remaining trust input

Release channel authentication supports local pinned-digest or detached
signature policies, exact channel-byte binding, generation floors and persisted
high-water comparison. The download path checks approved HTTPS origins and
redirects, bounded metadata/downloads, exact size/digest, private staging, safe
paths and use-time revalidation. Release manifest schema/validator binds
inventory fields but is not a trust root. CI unsigned candidates and generated
local checksums are not publisher authentication. Production public keys are
externally provisioned; no approved desktop policy source is present.

P036 schema validation proves inventory shape only. The public keyring is
externally provided to CI; this checkout has no approved P036 production
keyring, no PG17/edge production artifact identity evidence, and no native P036
clean-machine proof. Windows signing policy does not contain approved
timestamp hosts or independently establish a certificate root. Test
certificates/signatures are fixture-only.

## Validation results

| Check | Result | Evidence |
|---|---|---|
| `cargo fmt --all -- --check` | **PASS** | Final source formatted with locked workspace toolchain |
| `cargo test -p synveil-install-engine --locked` | **PASS** | 20 unit tests plus existing engine, AppImage, lifecycle, journal, package and review suites pass |
| `cargo clippy -p synveil-install-engine --all-targets --locked -- -D warnings` | **PASS** | Strict lint on final engine source |
| `cargo test -p synveil-desktop --locked` | **BLOCKED** | `libdbus-sys` cannot find `dbus-1.pc`; desktop Rust/QML compilation not reached |
| `cargo clippy -p synveil-desktop --all-targets --locked -- -D warnings` | **BLOCKED** | Same missing `dbus-1.pc`; lint not reached |
| `./scripts/validate-release-manifest.sh` | **PASS** | 36 release-manifest tests |
| `./scripts/validate-release-artifacts.sh` | **BLOCKED** | Generated `target/packages/SYNVEIL-LINUX-ARTIFACT-MANIFEST.txt` is absent; no product artifact was generated for this read-only task |
| `./scripts/test-desktop-ui.sh` | **BLOCKED** | qmake shim cannot execute `/usr/lib/qt5/bin/qmake`; Qt 6 qmake is absent |
| `./scripts/validate-docs.sh` | **PASS** | Final documentation validation suite passed (24 units) |
| `git diff --check` | **PASS** | Final staged source and documentation diff clean |
| Release channel/manifest/signature/download and installer-security suites | **PASS** | Channel 101; manifest 36; release download 79; installer security 23; release signing 3; P036 signing 1 |
| `python3 scripts/validate-p036-production-artifacts.py` | **BLOCKED** | External `production-artifacts.json` absent |
| Hosted Linux and Windows desktop/native P010 checks | **PENDING** | Exact-head jobs required; P009 failures are inherited context, not P010 passes |

P010 tests use synthetic evidence strictly to check bounded plan shape and
engine validation. They do not mint `VerifiedReleaseArtifact`, and there is no
claim of production publisher authentication or acquisition.

## P011 handoff and release gates

P011 needs an authoritative v0.3 roadmap, independently provisioned approved
production publisher roots and rotation/revocation/anti-downgrade decisions,
full artifact identity per channel, approved effect ownership catalogs,
P004-D02/D11/D14 approval, a runtime trust-policy source, complete freshness
revalidation before future effects, and passing exact-head Linux/Windows Qt and
native acceptance. P006's joined PostgreSQL/ObjectStore coverage, P028
standard-user acceptance, and P036 production artifact evidence remain separate
security qualification gates.

## Git and PR record

| Item | Value |
|---|---|
| Branch | `feat/v03-p010-artifact-plan-builder` |
| Base | `develop/v0.3` at `1ac67076e9e1e8c475b4a69578273d3f777a6ae9` |
| Target | `develop/v0.3` |
| PR | Existing open PR [#170](https://github.com/nghianguyen150612/Synveil/pull/170), target `develop/v0.3`; update after merge-resolution push |
| Merge | Not requested or performed |
