# Synveil v0.2.0 release artifact matrix

Status: R03 candidate engineering record. This matrix records concrete bytes
only. Producer code, a locally calculated checksum, or a green source test does
not create an artifact record.

## Product artifacts

| Artifact | Target | Expected filename | Classification | Source revision | Size / SHA-256 | Reproducibility | Signing and verification | Remaining gate |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Windows Setup | Windows x86_64 | `SynveilSetup.exe` | `NOT_PRODUCED` | Not established | — | Not run | Protected Authenticode producer is implemented; no publisher certificate, thumbprint, or approved timestamp host is configured | Produce and verify final timestamped Setup and its embedded payload on a protected Windows runner |
| Windows portable ZIP | Windows x86_64 | `synveil-0.2.0-windows-x86_64.zip` | `NOT_PRODUCED` | Not established | — | Not run | ZIP contents are rebuilt after signing the desktop and client executables; the release manifest authenticates final ZIP bytes | Produce, inspect exact ZIP inventory, and authenticate its final manifest |
| Ubuntu native package | Ubuntu 24.04 x86_64 | `synveil_0.2.0_amd64.deb` | `NOT_PRODUCED` | Not established | — | Not run | No detached DEB signature is used; the planned authenticated Ed25519 release manifest binds its exact bytes. No APT repository is configured | Build and verify package bytes, then sign the exact release manifest with an approved key |
| Fedora native package | Fedora 42 x86_64 | `synveil-0.2.0-1.x86_64.rpm` | `NOT_PRODUCED` | Not established | — | Not run | RPM OpenPGP signing and isolated public-key verification are implemented; no authorized key pair or trusted fingerprint is configured | Build, sign, independently verify the RPM, then rehash the final package and manifest |
| AppImage | Linux x86_64 | `Synveil-0.2.0-x86_64.AppImage` | `NOT_PRODUCED` | Not established | — | Not run | Pinned type-2 runtime input was acquired and signature-verified; the product AppImage was not built | Run the pinned Qt/Rust producer twice, compare bytes, execute smoke tests, and verify its manifest |

No product artifact file was built in this workspace. The local environment is
Linux x86_64; a temporary Rust toolchain was installed outside the repository for
source tests, but Qt 6 build tools, RPM tools, and PowerShell are unavailable. CI build
workflows exist, but R03's final-head workflow results and artifact downloads
must be recorded after the PR run; they are not inferred from source tests.

## Managed Host runtime inputs

| Component | Target | Expected signed runtime input | Classification | Source revision | Size / SHA-256 | Reproducibility | Remaining gate |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Synveil server | Ubuntu 24.04 x86_64 | `server-<Synveil-source-commit>` input bundle | `NOT_PRODUCED` | Not established | — | Not run | Source producer exists and emits an unsigned input; build, package, sign, and verify the API, worker, migration, and helper inventory |
| Synveil server | Fedora 42 x86_64 | `server-<Synveil-source-commit>` input bundle | `NOT_PRODUCED` | Not established | — | Not run | Fedora 42 source build and signing evidence are absent |
| PostgreSQL 17 | Ubuntu 24.04 x86_64 | Signed PostgreSQL runtime manifest and payload | `NOT_PRODUCED` | Not established | — | Not run | Authentic upstream bytes, immutable source revision, package inventory, license notices, signature, and trusted publisher key are absent |
| PostgreSQL 17 | Fedora 42 x86_64 | Signed PostgreSQL runtime manifest and payload | `NOT_PRODUCED` | Not established | — | Not run | Same requirements must be met independently for Fedora 42; Ubuntu compatibility is not inferred |
| Caddy edge | Ubuntu 24.04 x86_64 | Signed Caddy runtime manifest and payload | `NOT_PRODUCED` | Not established | — | Not run | Authentic upstream bytes, immutable source revision, payload inventory, license notices, signature, and trusted publisher key are absent |
| Caddy edge | Fedora 42 x86_64 | Signed Caddy runtime manifest and payload | `NOT_PRODUCED` | Not established | — | Not run | Same requirements must be met independently for Fedora 42 |
| P036 inventory | Ubuntu 24.04 and Fedora 42 x86_64 | `production-artifacts.json` and detached `.sig` | `NOT_PRODUCED` | Not established | — | Not run | All eight signed entries, component fingerprints, approved inventory fingerprint, public keyring, and native runner validation are required |

The P036 inventory stays absent until actual component artifacts and their
signatures exist. The validator is not weakened and a metadata-only inventory
does not satisfy this matrix.

## Verified external build input

| Component | Classification | Source identity | Size / SHA-256 | Evidence |
| --- | --- | --- | --- | --- |
| AppImageKit type-2 x86_64 runtime (build input, not a Synveil release artifact) | `VERIFIED_SIGNED_CANDIDATE` | `AppImage/type2-runtime` tag `20251108`, source commit `dd6cebedcbddde9c82f89b011e8e1d40b6e43868`; MIT | 944,632 bytes; `2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d` | Upstream detached signature verified to pinned fingerprint `570C77ACEA40C0F1B758902CBF96CCA56490F695`; GitHub release API asset digest, tag commit, ELF64 x86_64 header, and MIT notice bytes are checked by `scripts/acquire-appimage-runtime.py` |

The acquired bytes are available only in the local ignored build directory
`target/r03-appimage-runtime/`. The runtime release signature and public key
are upstream evidence, not Synveil publisher signing credentials. No trust
claim is made about the generated Synveil AppImage because it does not exist.
