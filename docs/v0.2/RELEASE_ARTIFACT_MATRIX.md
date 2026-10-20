# Synveil v0.2.0 release artifact matrix

Status: R03 candidate engineering record. This matrix records concrete CI
output bytes and their verification evidence. Every product candidate below is
unsigned and is not an official release artifact.

## Product artifacts

| Artifact | Target | Expected filename | Classification | Source revision | Size / SHA-256 | Reproducibility | Signing and verification | Remaining gate |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Windows Setup | Windows x86_64 | `SynveilSetup.exe` | `VERIFIED_UNSIGNED_CANDIDATE` | `7f88d114e272ac220056db216030acda84be90e1` | 32,815,179 bytes; `6030cb810afd029732090148e76d872d61fb36af20cbde30e56251d4cfe335ac` | Two Setup builds in the Windows installer workflow matched byte-for-byte | Candidate manifest binds the exact Setup bytes; `asInvoker`, runtime-closure negatives, standard-user install, repair, synthetic upgrade recovery, downgrade rejection, uninstall, reinstall, and state-preservation evidence passed. No Authenticode signature. [Artifact](https://github.com/nghianguyen150612/Synveil/actions/runs/39254944703/artifacts/12190505013) | Protected Authenticode PFX, trusted thumbprint, and approved timestamp host; final post-signature hash/manifest; Windows GUI, genuine-logon, and named-pipe qualification |
| Windows portable ZIP | Windows x86_64 | `synveil-0.2.0-windows-x86_64.zip` | `VERIFIED_UNSIGNED_CANDIDATE` | `7f88d114e272ac220056db216030acda84be90e1` | 51,621,495 bytes; `139ed73ff48eff11198a9c73a28b10b086e25455fcbffbabf4ca4241a83a772b` | Selected Windows installer workflow output was not independently reproduced. Rust CI rebuilt its own ZIP twice byte-identically (`51,620,638` bytes; `51c054eb14ead62c790528c69872bf88e7d936a52eb28c3db0421d7ebf53801`), but those bytes differ from the selected candidate | Outer release manifest binds the selected ZIP. Its embedded inventory covers 1,358 files; all file sizes and SHA-256 values matched, both packaged executables are x86_64 PE, and no unsafe paths or links were found. Contents are unsigned. Same [Windows installer artifact](https://github.com/nghianguyen150612/Synveil/actions/runs/39254944703/artifacts/12190505013) | Reconcile the differing producer outputs and reproduce the exact selected/final signed ZIP; sign desktop/client executables before final ZIP creation and authenticate the final release manifest |
| Ubuntu native package | Ubuntu 24.04 x86_64 | `synveil_0.2.0_amd64.deb` | `VERIFIED_UNSIGNED_CANDIDATE` | `7f88d114e272ac220056db216030acda84be90e1` | 17,511,662 bytes; `63243220397df76b766f2c1ea2ae3f13a3b9f60270ebde9ba380b00e9c1b2578` | Independent rebuilds were byte-identical | Package metadata and inventory were inspected; APT install, non-root launch, removal, and durable-state preservation passed. Package and release manifest are unsigned. [Artifact](https://github.com/nghianguyen150612/Synveil/actions/runs/39254944698/artifacts/12191351453) | Sign the exact release manifest with the approved Ed25519 key and configure the trusted distribution origin/repository |
| Fedora native package | Fedora 42 x86_64 | `synveil-0.2.0-1.x86_64.rpm` | `VERIFIED_UNSIGNED_CANDIDATE` | `7f88d114e272ac220056db216030acda84be90e1` | 17,525,400 bytes; `eb701f7c60b1cd15ecfc23f248be548aae1e6567494f0ece14e9c5f531cf6048` | Independent rebuilds were byte-identical | RPM metadata, contents, and dependencies were inspected; Fedora 42 DNF install, non-root launch, erase, and durable-state preservation passed. RPM is not OpenPGP-signed. [Artifact](https://github.com/nghianguyen150612/Synveil/actions/runs/39254944698/artifacts/12191351453) | Sign with the authorized RPM key, verify against the independently trusted fingerprint, then authenticate the final release manifest |
| AppImage | Linux x86_64 | `Synveil-0.2.0-x86_64.AppImage` | `VERIFIED_UNSIGNED_CANDIDATE` | `7f88d114e272ac220056db216030acda84be90e1` | 49,125,880 bytes; `a0a10f8770d7441a5e89a1dff18d82df2b34b28f4c8fb86cfa75843fd9a9859a` | Two independent builds were byte-identical | Pinned signed AppImageKit runtime verified; AppDir, runtime, manifest, QML smoke, and current-user lifecycle checks passed. Synveil AppImage and manifest are unsigned. [Artifact](https://github.com/nghianguyen150612/Synveil/actions/runs/39254945008/artifacts/12186709692) | Authenticate the product release manifest with an approved key; establish distribution origin and supported Linux baseline before release |

The product files were built by PR CI, not by the local executor. Candidate
copies are staged under the ignored `target/r03-candidate-artifacts/` directory.
GitHub Actions artifact links are temporary candidate storage, not a public
release origin. The manifests and GitHub artifact archive digests provide byte
identity; they do not authenticate Synveil as publisher.

Auxiliary workflows produced separate bytes for the same source commit and are
not substituted for the candidates above. Rust CI's independent Windows ZIP
builds matched each other, but its uploaded ZIP (`51,620,638` bytes,
`51c054eb14ead62c790528c69872bf88e7d936a52eb28c3db0421d7ebf53801`) differs
from the selected Windows installer ZIP above. The Rust CI artifact archive is
51,045,744 bytes, SHA-256
`af8df97ee906b310b58962386ea6b47be8c6ed8e0d24f5a47aa96d7882349d07`
([run 39254944713 / artifact 12193965592](https://github.com/nghianguyen150612/Synveil/actions/runs/39254944713/artifacts/12193965592)).
The Linux clean-machine artifact job's bundle includes a DEB (`17,504,438` bytes,
`3b2c67dcfade955c6cecf8389b38ef4c0ea96e8d0d3727342d171c60e3dfc544`), an RPM
(`17,519,738` bytes,
`06279cffb42d6ed9b3f5952fa206d135fc2aa651e21eb4b1bcabd37707b06555`), and a
different AppImage (`49,351,160` bytes,
`985ffd7f3c6521d05ca760d6da777612dbf05202c5e1ae48e6f82740f4281a5c`). The
selected package candidates come from the dedicated reproducible package
workflow and the selected AppImage comes from its dedicated dual-build
workflow. The Windows native-acceptance workflow also produced a separate
test-only Setup (`32,815,168` bytes,
`9c8c03f71ac35851920981abdd8137f3f83b2d940c94278c98ea9e6040b97001`); the
selected release Setup and its lifecycle evidence come from the Windows
installer workflow above.

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
are upstream evidence, not Synveil publisher signing credentials. No publisher
trust claim is made about the generated Synveil AppImage because it and its
product manifest are unsigned.
