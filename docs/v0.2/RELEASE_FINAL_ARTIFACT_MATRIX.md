# Synveil v0.2.0 final artifact matrix — R05

## Release state

**No production-qualified or public v0.2.0 artifacts exist.** The exact
integrated-main unsigned candidate workflow is
[run 39373799679](https://github.com/nghianguyen150612/Synveil/actions/runs/39373799679)
on source `deef2db375764d3157afb5371eb963bbd9b3f1ea`, with
`sign_candidate=false`; source/version validation passed. At the 05:44 UTC
snapshot, Fedora server input completed, Ubuntu server, AppImage, Windows and
desktop Qt reproducibility jobs were in progress, while DEB/RPM packaging was
queued. The separate
exact-main AppImage run completed and recorded its product SHA-256, but the
exact product bytes were retrieved from Actions artifact 12241969666 and
matched against its manifest. The Actions ZIP is an internal CI archive, not a
release download. The earlier candidate from main
`503920aeac5f087f78fea253192579b71ae5e620` and all R03 test-merge outputs are
not R05 release artifacts and must not be reused.

## Product artifact matrix

These are the exact artifact identities required by the v0.2 release
contract. The exact-main cross-platform candidate producer has not yet emitted
its assembled manifest; source/version validation passed but platform artifact
jobs were still in progress or queued at capture. Individual exact-main Windows Setup and AppImage
candidate identities are recorded below. The `Size / SHA-256` column records
observed evidence and explicitly marks missing values. Any unsigned file is candidate-only.

| Artifact | Version | Platform / architecture | Expected product filename | Source SHA | Size / SHA-256 | Signing / verification evidence | Public URL / disposition |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Windows Setup | `0.2.0` | Windows x86_64 | `SynveilSetup.exe` | `deef2db375764d3157afb5371eb963bbd9b3f1ea` | 32,816,688 bytes; SHA-256 `76f811347eaeb88b9099db29ff3140803d1fac5b5c33e3882231f9bf4ed98e96`; retrieved from Actions artifact 12243017639, matched `p028-candidate.json` and manifest. Repeated Setup build matched. Unsigned candidate only. | Standard-user product install step passed on GitHub-hosted Windows Server 2025; workflow correctly blocked at unavailable interactive GUI/logon/named-pipe evidence. No Authenticode signature or production trust. | No URL; not releasable |
| Windows portable ZIP | `0.2.0` | Windows x86_64 | `synveil-0.2.0-windows-x86_64.zip` | `deef2db375764d3157afb5371eb963bbd9b3f1ea` | Product bytes were built in exact-main run 39373764284; file-level size/hash not emitted in accessible logs. Exact Windows Installer/Rust CI cross-producer identity pending. | Unsigned CI candidate only; no final-byte Authenticode or independent verification. | No URL; not releasable |
| Ubuntu package | `0.2.0` | Ubuntu 24.04 x86_64 | `synveil_0.2.0_amd64.deb` | `deef2db375764d3157afb5371eb963bbd9b3f1ea` | Not produced at capture | Exact-main DEB/RPM run 39373764286 was building and reproducing packages; no authenticated release manifest or native Ubuntu acceptance. | No URL; not releasable |
| Fedora package | `0.2.0` | Fedora 42 x86_64 | `synveil-0.2.0-1.x86_64.rpm` | `deef2db375764d3157afb5371eb963bbd9b3f1ea` | Not produced at capture | Exact-main DEB/RPM run 39373764286 was building and reproducing packages; approved RPM/OpenPGP identity and native Fedora acceptance absent. | No URL; not releasable |
| Linux AppImage | `0.2.0` | Linux x86_64 candidate target | `Synveil-0.2.0-x86_64.AppImage` | `deef2db375764d3157afb5371eb963bbd9b3f1ea` | 49,183,224 bytes; SHA-256 `746485e81835a885037d91b2bbe6cfaebc09741caa0f987b6dc60bdbde3833b1`; retrieved from Actions artifact 12241969666 and matched its manifest. | Unsigned hosted candidate; reproducibility/inspection/smoke/current-user lifecycle passed. The bundled AppImageKit runtime was authenticated independently (source revision `dd6cebedcbddde9c82f89b011e8e1d40b6e43868`, SHA-256 `2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d`, upstream signer fingerprint `570C77ACEA40C0F1B758902CBF96CCA56490F695`). This does not authenticate the Synveil product. Native Ubuntu 24.04/Fedora 42 acceptance and production trust absent. | No URL; not releasable |

No product artifact above is signed, independently verified against production
trust roots, natively qualified or publicly downloadable at this snapshot.

## Unsigned Managed Host build inputs

The exact-main candidate workflow also builds Synveil server source inputs for
P036 preparation. These are not production server releases and cannot fill
any P036 inventory entry.

| Input | Target | Producer | Source SHA | Size / SHA-256 | Trust and payload status |
| --- | --- | --- | --- | --- | --- |
| Synveil server input bundle | Ubuntu 24.04 x86_64 | Candidate run 39373799679, Ubuntu build in progress | `deef2db375764d3157afb5371eb963bbd9b3f1ea` | Not produced at capture | Unsigned project build input; not a trusted production runtime. |
| Synveil server input bundle | Fedora 42 x86_64 | Candidate run 39373799679, artifact 12242708471, `synveil-host-server-fedora42-candidate.zip` | `deef2db375764d3157afb5371eb963bbd9b3f1ea` | Archive 17,487,645 bytes; SHA-256 `6e76ef29769adb1f9a4c7e1ae1bc292e018ccee9815f16b446a7df795d44d54a`. Its input manifest records 7 payload/license/provenance files, locally rehashed. | Unsigned source-built project input. Its notice says third-party dependency license/notice inventory is incomplete; no approved publisher signature/keyring. Not a P036 production entry. |

## P036 required eight-entry production inventory

The required files `acceptance/p036/production-artifacts.json` and
`acceptance/p036/production-artifacts.json.sig` are absent. The validator
exits 2 with `BLOCKED_BY_EXTERNAL_INPUT`. Each entry requires actual signed
bytes, immutable source revision, payload inventory, size and SHA-256,
independently trusted signer fingerprint and complete license/notice evidence.

| Component | Ubuntu 24.04 x86_64 | Fedora 42 x86_64 | R05 result |
| --- | --- | --- | --- |
| Synveil Desktop | No signed production artifact or trusted publisher fingerprint | No signed production artifact or trusted publisher fingerprint | **BLOCKED** |
| Synveil Server | No accepted signed production artifact or inventory entry | No accepted signed production artifact or inventory entry | **BLOCKED** |
| PostgreSQL 17 | No authenticated upstream bytes, source/payload record, notices or trusted fingerprint | No authenticated upstream bytes, source/payload record, notices or trusted fingerprint | **BLOCKED** |
| Caddy | No authenticated upstream bytes, source/payload record, notices or trusted fingerprint | No authenticated upstream bytes, source/payload record, notices or trusted fingerprint | **BLOCKED** |

## Historical Windows ZIP discrepancy and R05 fix

The R03 candidate showed a 857-byte difference between the selected Windows
Installer ZIP and a Rust CI ZIP. A member-level comparison found 1,359 members
with identical names, ordering and timestamps; `SYNVEIL-MANIFEST.txt` and
`synveil-client.exe` differed, with the client executable differing by
12,288 bytes. The two old archive hashes were
`139ed73ff48eff11198a9c73a28b10b086e25455fcbffbabf4ca4241a83a772b` and
`51c054eb14ead62c790528c69872bf88e7d936a52eb28c3db0421d7ebf53801`.

R05 found that Windows Installer built desktop and client in one Cargo
invocation while Rust CI built them separately. With resolver 3, selected
workspace packages can change feature unification. PR #154 makes both Windows
producers use the same grouped invocation and updates the clean-target
reproducibility verifier to do the same. On the pre-fix same-source PR run,
Windows Installer and Rust CI ZIP hashes were
`95fe3a29f7c4b6cbc67b215d210bc52576784712d4fed6c52261bac8556971c5` and
`4446a4da43420d2ba2578afebbf69d5b59105f6b56deb316a6feb1c2185a432d`,
respectively. Each producer reproduced its own ZIP, but they did not match.

The exact integrated-main producers are the Windows Installer run 39373764312
and Rust CI run 39373764252. Windows Installer was building and Rust CI's
Windows/Linux Qt, web quality and dependency policy jobs were incomplete at
the 05:44 UTC capture; R05 does not call the
Windows ZIP discrepancy resolved until these exact-source outputs are
byte-identical. If hashes differ, keep the artifact gate blocked and compare
member inventory, manifest, Qt/runtime closure, generated inputs, timestamps,
archive ordering, build flags and producer provenance before selecting a ZIP.

## Signing, trust and distribution records

| Record | Required final evidence | R05 result |
| --- | --- | --- |
| Windows Authenticode | Signed Setup and payload binaries, approved thumbprint, timestamp proof and post-signing hashes | Not signed; authorized credential, thumbprint and timestamp endpoint unavailable |
| RPM/OpenPGP | Signed RPM and independent verification against trusted fingerprint | Not signed; authorized identity and fingerprint unavailable |
| Release metadata | Exact manifest, detached signature, key ID and independently trusted freshness/rollback policy | Not signed; protected Ed25519 key and public trust policy unavailable |
| P036 inventory | `production-artifacts.json`, `.sig`, eight authentic payload entries and runtime verification | Not produced; bytes, approvals, keyring and fingerprints absent |
| Stable channel | Authenticated metadata, generation/freshness evidence and public HTTPS origin | Not provisioned or activated |
| GitHub Release | Public release URL and exact production-signed artifacts | No release exists |

Final signed-byte hashes, public artifact URLs, tag `v0.2.0`, GitHub Release
and stable-channel activation are all **absent**.
