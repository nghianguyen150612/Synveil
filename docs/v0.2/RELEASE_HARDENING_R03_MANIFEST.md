# Synveil v0.2 release hardening R03 manifest

## Closeout status

**R03_SOURCE_COMPLETE_WITH_BLOCKERS** is the target closeout classification for
this source work. No supported product artifact, production signature, P036
inventory, stable channel, or native installation result is claimed. The
classification is valid only if the final PR head passes its source checks;
the exact-head CI state and final Git references are recorded in the handoff.

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
declared Ubuntu/Fedora OS. The AppImage workflow runs its producer twice and
requires byte identity. The Linux workflow independently rebuilds packages
and validates DEB/RPM metadata and installation behavior. Windows installer
checks its exact manifest and reproducible unsigned build. None of those
product artifact commands could run in this local environment.

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

This proves the runtime build input only. The Synveil AppImage, repeated build,
AppDir inspection, runtime execution, manifest, and final product digest are
still absent.

## D. Signing and trust boundaries

| Boundary | Producer / signer integration | Verifier | Current trust input state |
| --- | --- | --- | --- |
| AppImage runtime | Pinned upstream AppImageKit release signer | GPG signature + pinned primary fingerprint, release API digest, tag commit | Runtime bytes and upstream license evidence verified locally |
| Release manifest and channel | `scripts/sign_release_metadata.py`, Ed25519 | Existing `release_signature.py`, `release_download.py`, and `release_channel.py` | No approved release key; missing secret `SYNVEIL_RELEASE_ED25519_PRIVATE_KEY_B64` and public variable `SYNVEIL_RELEASE_ED25519_PUBLIC_TRUST_POLICY_B64` |
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
The inventory and all actual files remain absent, so P036 is blocked and no
signature is produced.

No publisher-controlled domain or approved public distribution origin is
configured. The workflow can create a retained GitHub Actions candidate
artifact after the product builds; it does not create a GitHub Release, tag,
APT/YUM repository, stable channel, or public URL. A stable channel must not
point to unpublished or nonexistent artifacts. Channel generation is withheld
until the real published manifests and origin exist.

## F. Validation and environment limits

Source-level suites required by R03 cover release manifest creation and
negative cases, download authentication and staging, channel rollback/freshness,
AppImage input acquisition, candidate assembly, RPM signing blockers, Ed25519
signing, and P036 schema/signing blockers. The Python source suites passed 36
manifest tests, 79 release-download tests, 101 release-channel tests, 14
AppImage/candidate/signing tests, and 12 P036 inventory/source-identity tests
(242 total). The complete Rust install-engine crate passed 531/531 tests,
including all 124 Linux package integration cases; the Linux package
  validation script also passed its 27 metadata packaging tests. YAML parsing,
Python bytecode compilation, shell syntax checks, workspace version identity,
Windows installer/security validators, AppImage static validation, Rust
formatting and Clippy, and documentation validation also passed. Native product
builds remain unavailable in this executor; P036 runtime artifact validation
returns its expected external-input blocker.

The local executor is Linux x86_64. A temporary stable Rust 1.99.1 toolchain
was installed outside the repository, and the full install-engine suite passed
531/531 tests (124 Linux package integration tests); the separate Linux package
metadata suite passed 27/27 tests, with five artifact-dependent checks skipped
because no DEB/RPM output was built. Qt 6 build tools,
`rpmsign`, RPM verification tools, and
PowerShell are unavailable, so the five desktop artifacts, Ubuntu/Fedora server
inputs, native package signatures, Windows signatures, and Windows native
acceptance could not be produced here. `dpkg-deb` and GPG are available. No
production secret or trust variable was present. Native tests that need a
desktop session, polkit, Windows GUI, or disposable managed-host runner remain
for R04.

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
