# Synveil v0.2 release hardening R03 manifest

## Closeout status

**R03_SOURCE_COMPLETE_WITH_BLOCKERS** is the closeout classification. Exact
CI-produced unsigned candidates now exist for Windows Setup, Windows portable
ZIP, Ubuntu DEB, Fedora RPM, and AppImage. No signed product artifact,
authenticated P036 inventory, stable channel, or full native installation
qualification is claimed. The selected candidate files and their evidence are
recorded below; none is an official release.

## A. Starting state and integration base

- Repository: `nghianguyen150612/Synveil`.
- Starting authoritative `main`: `b01c3b2328b271529d02cb4ee30d97be43a42611`.
- R01 PR [#148](https://github.com/nghianguyen150612/Synveil/pull/148) was
  open and unmerged at `24f84d6e9460d82380f3ab017a3a2d36d24f2879`.
- R02 PR [#149](https://github.com/nghianguyen150612/Synveil/pull/149) was
  open and unmerged at `f6f2591f44caf54ff13de2b0fd869257674de655`, based on
  R01.
- R03 branch: `release/v02-production-artifact-readiness`.
- R03 integration base: `feat/v02-managed-host-production-readiness` at the
  verified R02 head above. This is a stacked PR; it must not target `main`
  until R01 and R02 are merged.
- No R01/R02 commits, iOS history, native acceptance cases, or historical
  evidence were removed or rewritten.

## B. Release identity and producer changes

The previous Cargo workspace version was `0.1.0`. R03 changes the development
candidate version to `0.2.0` in `[workspace.package].version`; this does not
create a tag, release, or supported `0.1.0` to `0.2.0` transition. The full
workspace version audit covers all 18 workspace crates and their `Cargo.lock`
entries. DEB, RPM, AppImage, Windows Setup, portable ZIP, server runtime, and
manifest producers read the same Cargo identity or reject a mismatch.

Native upgrade policies remain empty. Existing installs receive no new
cross-version compatibility authorization. A data-preserving v0.1-to-v0.2
upgrade still requires R04's machine-level tests and an explicit reviewed
policy.

The version scan also found independent pre-release metadata outside the R03
Windows/Linux release identity: private `apps/web/package.json` and optional
`services/ai/pyproject.toml` both declare `0.1.0`; `api/openapi.yaml` declares
`0.1.0-blueprint`; Android's Gradle release-name default is `0.1.0`; and the
unreleased iOS client still uses a `0.1.0` User-Agent literal. Those values do
not feed the Windows/Linux artifact producers. The iOS Rust FFI package itself
inherits Cargo `0.2.0`; R03 makes no iOS or Android application release claim.

Implementation areas changed:

- Version and package integration: `Cargo.toml`, workspace entries in
  `Cargo.lock`, `deploy/linux/package-integration-v1.json`,
  `deploy/release/native-upgrade-policy.json`,
  `deploy/release/windows-upgrade-policy.json`,
  `crates/install-engine/src/linux_package.rs`, its integration tests,
  `scripts/native_upgrade_policy.py`,
  `scripts/build-windows-installer.ps1`, and
  `scripts/validate-linux-package-integration.sh`.
- Manifest and authenticated download: `scripts/release_manifest.py`,
  `scripts/release_download.py`, both v1 release schema files, and their
  negative tests. Manifest creation now rejects a version that differs from
  Cargo and a source hash that is not the real checkout `HEAD`. Signature
  descriptors bind exact manifest filename, byte count, and SHA-256.
- AppImage source input: `scripts/acquire-appimage-runtime.py`, the vendored
  MIT license notice, `deploy/packages/build-appimage.sh`,
  `scripts/validate-appimage-build.py`, and both AppImage-producing workflows.
  Acquisition checks immutable release/tag identity, upstream API asset hashes,
  signature fingerprint, redirects, architecture, and license bytes.
- Signing integrations: `scripts/sign_release_metadata.py`,
  `scripts/materialize-release-trust-policy.py`,
  `scripts/sign_rpm_candidate.py`, `scripts/sign-windows-candidate.ps1`, and
  `scripts/sign_p036_inventory.py`. The Windows timestamp host policy is empty
  until an approved service is supplied. Production keys and certificates are
  not in this repository.
- Candidate pipeline: `.github/workflows/release-artifact-candidate.yml`
  calls the existing DEB/RPM, AppImage, and Windows workflows, builds unsigned
  server inputs on Ubuntu 24.04 and Fedora 42, verifies exact product bytes,
  and provides protected signing and P036 gates. It uses read-only repository
  permissions and does not publish releases, tags, or channels.
- Candidate tooling tests: `scripts/tests/test_appimage_runtime_acquisition.py`,
  `scripts/tests/test_release_candidate_assembly.py`,
  `scripts/tests/test_release_signing.py`,
  `scripts/tests/test_rpm_signing.py`, and
  `scripts/tests/test_p036_signing.py`.

Expected producer commands are `./deploy/packages/build.sh --format=all`,
`./deploy/packages/build-appimage.sh`,
`./deploy/packages/build-windows.sh --output-dir=target/windows-packages
--staging-dir=target/windows-installer-runtime`,
`./scripts/build-windows-installer.ps1`, and
`./scripts/build-host-server-input.sh target/host-server-candidate` on each
declared Ubuntu/Fedora OS. The AppImage workflow ran its producer twice and
required byte identity. The Linux package workflow independently rebuilt the
DEB/RPM, inspected metadata and contents, and passed Ubuntu APT and Fedora 42
DNF install/launch/removal/preservation checks. The Windows installer workflow
produced a reproducible Setup candidate and portable ZIP; its standard-user
lifecycle evidence is bound to the selected Setup digest. These product builds
ran in CI because the local executor lacks Qt and Windows packaging tools.

## C. Local external runtime evidence

The actual AppImageKit runtime input was acquired to the ignored local path
`target/r03-appimage-runtime/runtime-x86_64`:

- Upstream repository and tag: `https://github.com/AppImage/type2-runtime`,
  release `20251108`.
- Source commit: `dd6cebedcbddde9c82f89b011e8e1d40b6e43868`.
- File: 944,632 bytes, SHA-256
  `2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d`.
- Signature: verified against pinned upstream primary fingerprint
  `570C77ACEA40C0F1B758902CBF96CCA56490F695`; GitHub release API digest and
  tag commit were independently checked by the acquisition program.
- Redistribution license: MIT. The checked notice SHA-256 is
  `aa154fc9070614bbe7921f89db11efd1dba7a1f3a41685958110e2230f9c0ca1` and
  the upstream notice names musl, libfuse, squashfuse, libzstd, and zlib.
- Runtime architecture: ELF64 little-endian x86_64. Acquisition uses pinned
  versioned assets and restricted HTTPS redirects; it does not use a `latest`
  endpoint.
- The GPG signer fingerprint is a reviewed upstream pin in the R03 source and
  is cross-checked against the detached signature and GitHub release asset
  digest. No separate organization-provisioned bootstrap root for this upstream
  key is configured.

This proves the runtime build input only. The Synveil AppImage was separately
built and verified by the dedicated AppImage workflow; its exact bytes and
manifest are recorded in the next subsection.

### CI-produced product candidates

All five selected product candidates have `source_commit`
`7f88d114e272ac220056db216030acda84be90e1`, a real GitHub pull-request test
merge commit whose parents are R02 head
`f6f2591f44caf54ff13de2b0fd869257674de655` and R03 code head
`7bee96edf2509eb26cea4b5fd352761f5e72d458`. This is not an integration merge:
PR #150 remains open. Product file hashes and sizes below were independently
recomputed from downloaded GitHub Actions artifacts and matched their release
manifests.

| Product candidate | Size / SHA-256 | Build and verification evidence | GitHub Actions artifact |
| --- | --- | --- | --- |
| Windows Setup `SynveilSetup.exe` | 32,815,179 bytes; `6030cb810afd029732090148e76d872d61fb36af20cbde30e56251d4cfe335ac` | Windows installer workflow built twice byte-identically. Candidate manifest matched. `asInvoker`, runtime closure negatives, disposable standard-user install, repair, synthetic upgrade recovery, downgrade rejection, uninstall/reinstall, and state preservation passed. No Authenticode signature. | [Run 39254944703 / artifact 12190505013](https://github.com/nghianguyen150612/Synveil/actions/runs/39254944703/artifacts/12190505013); archive 135,154,302 bytes, SHA-256 `dc296076a931555f790b2b7f3307434bf2771bb09e4bf37bbf9cb90c56df1459` |
| Windows portable ZIP `synveil-0.2.0-windows-x86_64.zip` | 51,621,495 bytes; `139ed73ff48eff11198a9c73a28b10b086e25455fcbffbabf4ca4241a83a772b` | Outer release manifest matched. Embedded inventory had 1,358 files; every size and SHA-256 matched, both packaged executables are x86_64 PE, and no unsafe paths or links were found. Contents are unsigned. Rust CI independently built its own ZIP twice byte-identically, but that output (`51,620,638` bytes; `51c054eb14ead62c790528c69872bf88e7d936a52eb28c3db0421d7ebf53801`) differs from this selected file; cross-workflow equivalence is not claimed. | Same Windows installer artifact as above |
| Ubuntu DEB `synveil_0.2.0_amd64.deb` | 17,511,662 bytes; `63243220397df76b766f2c1ea2ae3f13a3b9f60270ebde9ba380b00e9c1b2578` | Independent builds byte-identical; package metadata, contents, provenance, and manifest validated. Ubuntu 24.04 APT install, non-root launch, removal, and durable-state preservation passed. Unsigned. | [Run 39254944698 / artifact 12191351453](https://github.com/nghianguyen150612/Synveil/actions/runs/39254944698/artifacts/12191351453); archive 34,839,225 bytes, SHA-256 `381cc643f99a065760924c1f9629ebd82c3da7c48d39c4608a2ee5557efa2108` |
| Fedora RPM `synveil-0.2.0-1.x86_64.rpm` | 17,525,400 bytes; `eb701f7c60b1cd15ecfc23f248be548aae1e6567494f0ece14e9c5f531cf6048` | Independent builds byte-identical; RPM metadata, contents, dependencies, provenance, and manifest validated. Fedora 42 DNF install, non-root launch, erase, and durable-state preservation passed. No RPM OpenPGP signature. | Same Linux package artifact as above |
| AppImage `Synveil-0.2.0-x86_64.AppImage` | 49,125,880 bytes; `a0a10f8770d7441a5e89a1dff18d82df2b34b28f4c8fb86cfa75843fd9a9859a` | Dedicated workflow acquired the verified runtime, built twice byte-identically, inspected AppDir/runtime/manifest, passed host-decontaminated QML smoke and current-user lifecycle tests. The product and manifest are unsigned. | [Run 39254945008 / artifact 12186709692](https://github.com/nghianguyen150612/Synveil/actions/runs/39254945008/artifacts/12186709692); archive 49,004,027 bytes, SHA-256 `cf9d3e14bdaa3377978454ca8a75250f4d90d78774aab3d830b0f1e06520e6c4` |

The dedicated candidate-producing workflows are selected above. Auxiliary
acceptance workflows built distinct bytes from the same source commit and are
not interchangeable with those candidates. The Linux acceptance bundle's DEB,
RPM, and AppImage hashes were respectively `3b2c67dcfade955c6cecf8389b38ef4c0ea96e8d0d3727342d171c60e3dfc544`,
`06279cffb42d6ed9b3f5952fa206d135fc2aa651e21eb4b1bcabd37707b06555`, and
`985ffd7f3c6521d05ca760d6da777612dbf05202c5e1ae48e6f82740f4281a5c`; the
dedicated package and AppImage workflows above provide the selected reproducible
outputs. The Windows native-acceptance workflow separately produced a test-only
Setup with SHA-256
`9c8c03f71ac35851920981abdd8137f3f83b2d940c94278c98ea9e6040b97001`; it is
not the selected release Setup. Candidate copies are staged locally under the
ignored `target/r03-candidate-artifacts/` directory. GitHub Actions artifacts
are temporary staging links, not a public download origin.

Rust CI run [39254944713](https://github.com/nghianguyen150612/Synveil/actions/runs/39254944713)
also uploaded its separate reproducible Windows ZIP as artifact 12193965592
(archive 51,045,744 bytes, SHA-256
`af8df97ee906b310b58962386ea6b47be8c6ed8e0d24f5a47aa96d7882349d07`). Its
two ZIP builds matched at 51,620,638 bytes, SHA-256
`51c054eb14ead62c790528c69872bf88e7d936a52eb28c3db0421d7ebf53801`; this
does not reproduce the selected Windows installer workflow ZIP above.

## D. Signing and trust boundaries

| Boundary | Producer / signer integration | Verifier | Current trust input state |
| --- | --- | --- | --- |
| AppImage runtime | Pinned upstream AppImageKit release signer | GPG signature + pinned primary fingerprint, release API digest, tag commit | Runtime bytes and upstream license evidence verified locally |
| Synveil product manifests and stable channel | `scripts/sign_release_metadata.py`, Ed25519 | Existing `release_signature.py`, `release_download.py`, and `release_channel.py` | Candidate manifests are unsigned; no approved release key. Missing secret `SYNVEIL_RELEASE_ED25519_PRIVATE_KEY_B64` and public variable `SYNVEIL_RELEASE_ED25519_PUBLIC_TRUST_POLICY_B64` |
| RPM package | `scripts/sign_rpm_candidate.py`, OpenPGP RPM signature | `rpmkeys --checksig` with an isolated database containing only configured public key | Missing `SYNVEIL_RPM_SIGNING_PRIVATE_KEY_B64`, `SYNVEIL_RPM_TRUSTED_PUBLIC_KEY_B64`, and independently approved `SYNVEIL_RPM_SIGNER_FINGERPRINT` |
| Windows executables and Setup | `scripts/sign-windows-candidate.ps1`, Authenticode with SHA-256 and timestamp | `signtool verify /pa /all /tw` plus exact certificate thumbprint | Missing protected PFX/password, signer thumbprint, approved HTTPS timestamp URL, and reviewed timestamp host. `windows-signing-policy-v1.json` currently has an empty allowlist |
| Managed Host component artifacts | Existing component detached-signature contracts; P036 inventory signing helper | R02 runtime registrar and P036 validator against independent GPG keyring/fingerprints | No inventory, component signatures, public keyring, or fingerprints supplied |

GitHub environment `synveil-release-signing` must be configured with required
reviewers and allowed deployment branches before a signing dispatch. Exact
required values are listed in the workflow. Configure the RPM private-key
secret `SYNVEIL_RPM_SIGNING_PRIVATE_KEY_B64`, public-key variable
`SYNVEIL_RPM_TRUSTED_PUBLIC_KEY_B64`, and approved fingerprint variable
`SYNVEIL_RPM_SIGNER_FINGERPRINT`. Windows requires protected secrets
`SYNVEIL_WINDOWS_SIGNING_PFX_B64` and
`SYNVEIL_WINDOWS_SIGNING_PFX_PASSWORD`, plus variables
`SYNVEIL_WINDOWS_SIGNING_THUMBPRINT` and
`SYNVEIL_WINDOWS_TIMESTAMP_URL`; add the approved timestamp host to
`deploy/release/windows-signing-policy-v1.json`. Public keys/fingerprints are
independent trust configuration; private keys and the Windows PFX/password are
protected environment secrets. The code uses no
test-key fallback. The RPM secret must contain an unencrypted GPG private-key
export because `rpmsign` runs noninteractively; the key is written only into a
0600 file under the temporary signing home, imported into an isolated GPG home,
and removed with that home when the job exits. The release and P036 signing
integrations likewise materialize their private inputs only in bounded
temporary storage. Ed25519 rotation and rollback behavior reuse the existing
release contracts: unknown keys fail, overlapping trusted keys are supported,
and lower channel generations or same-generation equivocation are rejected.

For Ed25519, `SYNVEIL_RELEASE_ED25519_PRIVATE_KEY_B64` is a base64-encoded
32-byte seed. `SYNVEIL_RELEASE_ED25519_PUBLIC_TRUST_POLICY_B64` is base64 JSON
with exactly `schema_version: 1`, purpose
`production-release-verification`, and `keys` entries containing only
`key_id`, `scheme: ed25519`, and 64-character lowercase `public_key_hex`;
`key_id` must equal `synveil-release-` plus SHA-256 of the raw public key.
P036 uses secret `SYNVEIL_P036_INVENTORY_SIGNING_KEY_B64`, optional passphrase
secret `SYNVEIL_P036_INVENTORY_SIGNING_PASSPHRASE`, public variables
`SYNVEIL_P036_TRUSTED_KEYRING_B64` and the five inventory/component
fingerprints documented in `HOST_RUNTIME_INPUTS.md`. No real key material or
fingerprint values are present in source.

No production key, PFX, fingerprint, timestamp endpoint, or publication origin
was invented. Test signing uses an ephemeral Ed25519 key isolated under a
temporary test directory only.

Rotation and revocation remain controlled by independently provisioned trust
configuration. For release metadata, add the new Ed25519 public key to the
trusted policy and deploy that policy through the trusted bootstrap before
changing `SYNVEIL_RELEASE_ED25519_PRIVATE_KEY_B64`; this integration signs with
one configured key per run. After consumers have the new key, remove the old
key from the policy and its private secret. Revocation requires delivery of the
updated trusted policy to existing consumers; there is no online trust-on-first-
use or remote revocation feed. RPM and P036 use the same order with their
independent public-key variables/keyring and exact fingerprints. Windows
certificate rollover updates the protected PFX/thumbprint after trusted
Windows consumers accept the new chain; an issuing-CA revocation and removal of
the old configured thumbprint revoke that identity. The signing job has no
dual-key fallback, and no trust bootstrap or revocation endpoint is configured
in R03, so no rotation or revocation is claimed as completed.

## E. P036 and distribution

The R02 P036 contract still requires exactly eight real entries: desktop,
server, PostgreSQL 17, and Caddy on Ubuntu 24.04 and Fedora 42. The existing
public variable contract is unchanged:

- `SYNVEIL_P036_TRUSTED_KEYRING_B64`
- `SYNVEIL_P036_INVENTORY_FINGERPRINT`
- `SYNVEIL_P036_DESKTOP_FINGERPRINT`
- `SYNVEIL_P036_SERVER_FINGERPRINT`
- `SYNVEIL_P036_POSTGRESQL17_FINGERPRINT`
- `SYNVEIL_P036_EDGE_FINGERPRINT`

The protected signing job first runs the component, provenance, payload,
license, platform, version, and signature checks without treating an
unsigned inventory as accepted; then it signs the exact inventory bytes and
runs the normal validator against the independent keyring. Source staleness
checks still reject code changes; they permit only the explicit P036 inventory
and release-evidence documentation paths to follow the bound product source.
The P036 inventory, detached signature, trusted keyring, approved fingerprints,
and eight authenticated platform/component entries remain absent. The
server-self-host workflow failed closed at `Materialize approved public
publisher keyring` because `SYNVEIL_P036_TRUSTED_KEYRING_B64` and its five
fingerprint variables were empty. P036 is blocked and no signature is produced.

No publisher-controlled domain or approved public distribution origin is
configured. The workflow can create a retained GitHub Actions candidate
artifact after the product builds; it does not create a GitHub Release, tag,
APT/YUM repository, stable channel, or public URL. A stable channel must not
point to unpublished or nonexistent artifacts. Channel generation is withheld
until the real published manifests and origin exist.

## F. Validation and environment limits

Local source-level suites passed: 36 manifest tests, 79 release-download tests,
101 release-channel tests, 14 AppImage/candidate/signing tests, and 12 P036
inventory/source-identity tests (242 total). The Rust install-engine crate
passed 531/531 tests, including 124 Linux package integration tests. The local
Linux package metadata suite passed 27 tests; five artifact-dependent cases
were skipped before CI artifacts existed. YAML parsing, Python bytecode
compilation, shell syntax, workspace identity, Windows installer/security
validators, AppImage static validation, Rust formatting/Clippy, and docs
validation passed.

On R03 code head `7bee96edf2509eb26cea4b5fd352761f5e72d458`, 16 of the 18
exact-head PR workflows succeeded. The Windows native-acceptance workflow
failed closed with an expected environment blocker, and the server self-host
workflow remained queued. Passing release gates included AppImage
build/reproduction/smoke/lifecycle, Linux DEB/RPM reproducibility and
Ubuntu/Fedora package lifecycle tests, Windows Setup reproducibility and
standard-user lifecycle, PostgreSQL 17, all three iOS workflows, installer
security hardening, and Rust CI. Rust CI independently rebuilt the Windows
client and desktop binaries byte-identically (SHA-256
`0a6ef56ebf07b7359e4d6da64c6ab6c0e6a2dfbac49fdb66a00ead93b6d9c81c` and
`83db59d040c6ca98b09926d6e79b1833cb2914b4c78b58920da0b4487f9c31f7`),
reproduced its own ZIP twice at SHA-256
`51c054eb14ead62c790528c69872bf88e7d936a52eb28c3db0421d7ebf53801`, and
passed release-manifest validation, QML module/PE checks, packaged QML smoke,
and artifact upload. That ZIP differs from the selected Windows installer
workflow ZIP in the candidate table; exact selected-ZIP reproduction remains
an open gate. The Windows native-acceptance
workflow passed its disposable standard-user install but failed closed with
explicit `BLOCKED` evidence for interactive GUI, genuine new logon, and
named-pipe acceptance. The server self-host workflow failed closed because the
approved P036 public keyring variable was absent; its three custom self-hosted
runner jobs remained queued.

The Linux clean-machine PR workflow's static contract and artifact-build jobs
passed, but its native VM and evidence-gate jobs were skipped because those
jobs run only for `workflow_dispatch`. This is not Linux clean-machine
acceptance. The Ubuntu APT and Fedora 42 DNF userspace lifecycle jobs in the
dedicated package workflow did run and passed; R04 still owns VM-based
clean-machine qualification on the signed final bytes.

The local executor is Linux x86_64. Qt 6 build tools, `rpmsign`, RPM query and
verification tools, and PowerShell are unavailable locally; CI produced the
five unsigned product candidates. No production secret, signing certificate,
trusted publisher keyring, or authorized fingerprint was supplied. The
repository has no configured native P036 self-hosted runners. No test result
from the local executor or hosted userspace tests is promoted to a claim about
Windows GUI/new-logon/named-pipe acceptance or P036 native registration.

## G. R04/R05 handoff

R04 must receive exact signed candidate artifacts and the corresponding
authenticated release manifest. It must then execute Windows GUI, genuine new
logon, and named-pipe acceptance; Ubuntu 24.04 and Fedora 42 clean-machine
install, repair, and removal; and P036 registration on native runners.
Upgrade, rollback, repair, and uninstall tests must prove preservation of
existing PostgreSQL data, server object data, credentials, storage identity,
and user libraries. No v0.1-to-v0.2 upgrade policy is approved by R03.

R05/P048 retains final release qualification. R03 does not merge ahead of R01
or R02 and does not create the final `v0.2.0` tag or stable publication.
