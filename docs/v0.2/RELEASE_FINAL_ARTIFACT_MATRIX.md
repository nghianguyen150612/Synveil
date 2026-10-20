# Synveil v0.2.0 final artifact matrix — R05

## Release state

**No production-qualified or public v0.2.0 artifacts exist.** The exact-source
candidate producer is [run 39359470865](https://github.com/nghianguyen150612/Synveil/actions/runs/39359470865)
on source `d0b1c43c144493e4adf9b44d3fd24d02f2bd7dbb`. It is an unsigned CI
candidate workflow, not a production release. At the 2026-10-20 02:41 UTC
snapshot, the Fedora 42 host-input job had failed before compilation because
rustup found `HOME=/github/home` while running as root; the exact-source
workflow could not assemble a candidate. A CI fix is in progress. Populate
sizes and digests only from the completed exact-source producer and
independent verifier. Do not use the R03 test-merge candidate or a different
source SHA.

## Product artifacts

| Artifact | Platform / architecture | Expected product filename | Exact source | Size / SHA-256 | Signature / verification | Public release URL | R05 disposition |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Windows Setup | Windows x86_64 | `SynveilSetup.exe` | `d0b1c43c144493e4adf9b44d3fd24d02f2bd7dbb` candidate workflow | Not assembled at snapshot | Unsigned; production Authenticode identity and final-byte verification absent | None | Candidate producer pending; not releasable |
| Windows portable ZIP | Windows x86_64 | `synveil-0.2.0-windows-x86_64.zip` | `d0b1c43c144493e4adf9b44d3fd24d02f2bd7dbb` candidate workflow | Not assembled at snapshot | Unsigned; Windows producer discrepancy must be resolved against exact R05 Rust CI output | None | Candidate producer pending; not releasable |
| Ubuntu package | Ubuntu 24.04 x86_64 | `synveil_0.2.0_amd64.deb` | `d0b1c43c144493e4adf9b44d3fd24d02f2bd7dbb` candidate workflow | Not assembled at snapshot | Unsigned; no production channel metadata or authenticated release manifest | None | Candidate producer pending; not releasable |
| Fedora package | Fedora 42 x86_64 | `synveil-0.2.0-1.x86_64.rpm` | `d0b1c43c144493e4adf9b44d3fd24d02f2bd7dbb` candidate workflow | Not assembled at snapshot | Unsigned; approved RPM/OpenPGP signer and independent fingerprint absent | None | Candidate producer pending; not releasable |
| Linux AppImage | Linux x86_64 candidate target | `Synveil-0.2.0-x86_64.AppImage` | `d0b1c43c144493e4adf9b44d3fd24d02f2bd7dbb` candidate workflow | Not assembled at snapshot | Unsigned; authentic product signer, exact distro acceptance and authenticated release manifest absent | None | Candidate producer pending; not releasable |

The expected filenames above come from the repository's v0.2 release
contract. They are not claims that an R05 file exists. Once run 39359470865
finishes, append exact artifact ID, file size, SHA-256, manifest identity,
independent verification result and any test-consumer evidence. A file from an
Actions artifact is temporary candidate storage, not a public download.

## Unsigned server build inputs

These inputs, if produced by the exact-source workflow, are build inputs for
P036 preparation. They are not production-signed Managed Host runtime
artifacts and do not replace any of the eight P036 entries.

| Input | Target | Producer Actions artifact | Source SHA | Size / SHA-256 | Signing / provenance | Status |
| --- | --- | --- | --- | --- | --- | --- |
| Synveil server input bundle | Ubuntu 24.04 x86_64 | Actions artifact `synveil-host-server-ubuntu24-candidate`, ID `12236364682` | `d0b1c43c144493e4adf9b44d3fd24d02f2bd7dbb` | Actions ZIP: 17,481,143 bytes; SHA-256 `a652c4ccdb9dde220b62068f11d295557b35ee9ad7566c0ddf27ccb7697ac92e` | Downloaded bytes match GitHub artifact digest. Manifest binds source/version and hashes four ELF64 x86_64 binaries. Unsigned self-declared provenance; bundled NOTICE is incorrectly the desktop notice and is rejected for P036. | None | **Not a production runtime.** Source fix supplies server-specific notice and discloses missing dependency-license notices; exact-source rebuild still required |
| Synveil server input bundle | Fedora 42 x86_64 | `synveil-host-server-fedora42-candidate` from run 39359470865 | `d0b1c43c144493e4adf9b44d3fd24d02f2bd7dbb` | Not produced | Unsigned candidate input | **Failed** before compile: rustup `HOME`/euid mismatch; workflow fix pending |

The Ubuntu source input's checked inventory was: `input-manifest.json` 2,130
bytes, SHA-256 `2f4e1537487e15962cc2645f5cd665d6b8d79b879f24f0be25dbd499e4db70d1`;
`PROVENANCE` 306 bytes, SHA-256
`5fc97c0207e550387392645a69eb115c93fd28318f718a43c0db8d287937777a`;
`LICENSE` 1,069 bytes, SHA-256
`94d3b4c28edd85a2f063556af4e3171d6297db9488e352a17b2a4237d5ef0179`;
`NOTICE` 494 bytes, SHA-256
`dad06768812d78cb7bd18f6c552cd1164bc0b6dd63bf40c1dea9e889e97fee7c`;
`synveil-api` 18,746,688 bytes, SHA-256
`956b1323e3cdae6d0204ddcc48af337b202c5b8c11a42079671fd767198c422e`;
`synveil-worker` 11,437,368 bytes, SHA-256
`ec6a6996912f47c3674fec29ab897545be9360bc176c58d5954cb376f4a419ca`;
`synveil-server-migrate` 8,791,040 bytes, SHA-256
`72fde8185b452941ee5196d80310267d18c97d4c5dda17d349f357c74cb3a110`;
`synveil-host-helper` 11,297,560 bytes, SHA-256
`c2727c109403cfe8dfa7d6cbd20c002eb15200272612e53d6923c9bc4eb218f2`.
The input manifest names the MIT project license and source revision, but its
`NOTICE` bytes say “Synveil desktop runtime packaging notice” and describe the
desktop shell and Qt. R05 treats that as an invalid server notice. The new
server notice states the server components and explicitly says it is not a
third-party Rust dependency license catalogue; production registration still
requires authentic dependency-license inventory and notices.

## P036 required eight-entry production inventory

`acceptance/p036/production-artifacts.json` and
`acceptance/p036/production-artifacts.json.sig` are absent. The validator exits
2 because the inventory was not supplied. Actual artifact names, exact sizes
and SHA-256 values cannot be listed until authentic inputs and signatures are
provided. No P036 row is considered present based on a schema or unsigned
candidate bundle.

| Component | Ubuntu 24.04 x86_64 | Fedora 42 x86_64 | R05 result |
| --- | --- | --- | --- |
| Synveil Desktop | Not supplied; no size/digest or trusted publisher signature | Not supplied; no size/digest or trusted publisher signature | **BLOCKED** |
| Synveil Server | Unsigned build input only if candidate job completes; no production signature or inventory entry | Unsigned build input only if candidate job completes; no production signature or inventory entry | **BLOCKED** |
| PostgreSQL 17 | Authentic upstream bytes, source revision, payload, notices, signature and trusted fingerprint not supplied | Authentic upstream bytes, source revision, payload, notices, signature and trusted fingerprint not supplied | **BLOCKED** |
| Caddy | Authentic upstream bytes, source revision, payload, notices, signature and trusted fingerprint not supplied | Authentic upstream bytes, source revision, payload, notices, signature and trusted fingerprint not supplied | **BLOCKED** |

## Signing, trust and distribution records

| Record | Required final file or evidence | R05 result |
| --- | --- | --- |
| Windows Authenticode | Signed Setup and payload binaries, approved thumbprint, timestamp proof, post-signing hashes | Not signed; authorized credential and thumbprint not supplied |
| RPM/OpenPGP | Signed RPM, signature verification against independent trusted fingerprint | Not signed; authorized identity and fingerprint not supplied |
| Release metadata | Exact `SYNVEIL-RELEASE-MANIFEST.json`, detached signature and authentication descriptor | Not signed; protected Ed25519 key and independent trust policy not supplied |
| P036 inventory | `production-artifacts.json`, `production-artifacts.json.sig`, eight authentic payload entries | Not produced; inputs and authorized signing/trust roots absent |
| Stable channel | Authenticated metadata, generation/freshness/rollback evidence, HTTPS public origin | Not provisioned or activated |
| GitHub Release | Public release URL, exact signed artifacts and authenticated checksums | No release exists |

## Historical Windows ZIP discrepancy

The R03 test-merge candidate showed a 857-byte difference between the selected
Windows Installer ZIP and a separate Rust CI ZIP. Those exact old archives
were successfully recovered for this investigation and compared member by
member: each contains 1,359 members with the same names, order and timestamps;
the embedded package manifest and `synveil-client.exe` differ. The selected
archive is 51,621,495 bytes (`139ed73ff48eff11198a9c73a28b10b086e25455fcbffbabf4ca4241a83a772b`);
the Rust CI archive is 51,620,638 bytes
(`51c054eb14ead62c790528c69872bf88e7d936a52eb28c3db0421d7ebf53801`). The
client executables differ by 12,288 bytes. Shared Qt/runtime closure, member
ordering, timestamps and most manifest fields do not explain the discrepancy.

The old selected Windows build invokes Cargo once for desktop and client;
Rust CI invokes them separately. That difference is a plausible feature/build
graph cause, but it is not proven. The R04/R05 selected Windows producer
independently rebuilds its own ZIP and manifest and fails on a mismatch; this
proves same-producer reproducibility only. It does not prove cross-workflow
identity. R05 must compare the completed exact-source Windows installer and
Rust CI ZIPs, inspect differing members/manifests, and document the cause or
leave the artifact gate blocked. Do not choose either file arbitrarily.

## Publication and hash verification

- Final signed-byte hashes: none.
- Public artifact URLs: none.
- Tag `v0.2.0`: absent.
- GitHub Release: absent.
- Stable channel: not activated.
- All product artifact rows remain **not releasable** pending exact-source
  producer completion, approved production signing, independent verification,
  native acceptance and P048 approval.
