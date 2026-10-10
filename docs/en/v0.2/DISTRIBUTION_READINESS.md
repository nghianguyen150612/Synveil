# v0.2 distribution readiness

**Audit status: documentation is prepared; v0.2 is not released.** The current released product remains v0.1.0. No v0.2 GitHub Release, public download channel, production signing key, or final v0.2 artifact set is published. Workflow artifacts are unsigned CI outputs and are not authenticated production releases.

The P046 documentation baseline is `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc`. P045 remains a separate open draft PR at `035497c05c145e5c6129e744f9455937069680f4`; its results below are evidence only for that source head and do not complete P045.

## Platform and artifact checklist

| Surface | Expected naming / architecture | Status and evidence | Release claim |
| --- | --- | --- | --- |
| Windows Setup | `SynveilSetup.exe`, Windows x86_64 | **Implemented; Build validated** in P045 Windows installer run [37970834502](https://github.com/nghianguyen150612/Synveil/actions/runs/37970834502). Scoped standard-user steps ran, but Windows native acceptance run [37970834767](https://github.com/nghianguyen150612/Synveil/actions/runs/37970834767) failed while recording unavailable required surfaces. **Native qualified: no.** | **Blocked; Not yet published.** Exact supported Windows versions and P045 acceptance are still required. |
| Ubuntu DEB | `synveil_<version>_amd64.deb`, x86_64 | **Implemented; Build validated** as part of the P045 Linux artifact job in run [37970834983](https://github.com/nghianguyen150612/Synveil/actions/runs/37970834983). The Ubuntu 24.04 guest failed before product assertions during boot/readiness. **Native qualified: no.** | Ubuntu 24.04 x86_64 is a candidate only. **Blocked; Not yet published.** |
| Fedora RPM | `synveil-<version>-1.x86_64.rpm`, x86_64 | **Implemented; Build validated** in run [37970834983](https://github.com/nghianguyen150612/Synveil/actions/runs/37970834983). The Fedora 42 guest failed before product assertions during boot/readiness. **Native qualified: no.** | Fedora 42 x86_64 is a candidate only. Other RPM distributions are **Unsupported** unless separately named and qualified. |
| Linux AppImage | `Synveil-<version>-x86_64.AppImage`, x86_64 | **Implemented; Build validated** in run [37970834983](https://github.com/nghianguyen150612/Synveil/actions/runs/37970834983). Ubuntu 24.04 and Fedora 42 guest attempts failed before product assertions. **Native qualified: no.** | No general Linux compatibility baseline is approved. **Blocked; Not yet published.** |
| Linux quick install | `deploy/install/quick-install.sh` → `scripts/linux_quick_install.py` | **Implemented** with exact Ubuntu 24.04/Fedora 42 x86_64 detection and authenticated stable-channel acquisition. Production endpoint, key bootstrap, and channel generation are absent. | **Blocked; Not yet published.** Do not copy an install command from CI or invent trust inputs. |

`Implemented` describes source behavior. `Build validated` describes the cited CI build on P045's source head only. `Native tested` means a scoped native step actually ran; it does not mean the full product journey passed. `Native qualified` requires the named clean-machine acceptance assertions to pass. `Blocked` records an unmet release gate. `Not yet published` means no official public release artifact is available. `Unsupported` is not a compatibility promise.

## Exact support boundary

- Windows: x86_64 Setup is a v0.2 target. No Windows version is qualified yet; Windows ARM64 is outside the matrix.
- Linux package candidates: Ubuntu 24.04 x86_64 for DEB and Fedora 42 x86_64 for RPM. Both still require P045 native acceptance.
- Debian is not qualified. A working DEB package or Debian-family package manager does not establish Debian support.
- Other Debian derivatives and RPM-based distributions are not qualified by file format.
- AppImage targets Linux x86_64, but its compatibility baseline and native launch remain unqualified. It does not mean every Linux kernel, glibc, desktop, or FUSE configuration is supported.
- Linux ARM64/aarch64, 32-bit architectures, macOS, iOS, and Android are outside the v0.2 installation matrix.

## Release identity and publication checklist

- [ ] Name the exact OS versions, architecture, and GUI/runtime requirements that passed native acceptance.
- [ ] Complete P045 Windows, Ubuntu 24.04, Fedora 42, and any explicitly advertised generic-Linux acceptance; preserve exact source and artifact identities.
- [ ] Complete P036 end-to-end managed Host acceptance. Current P045-head self-host run [37970834595](https://github.com/nghianguyen150612/Synveil/actions/runs/37970834595) failed the production-artifact identity gate; platform jobs were not a passing Host qualification.
- [ ] Provision and independently distribute the production public trust root; establish private-key custody and signing operations. The verifier exists, but no production key is provisioned.
- [ ] Publish authenticated stable-channel and release-manifest bytes, the trusted origin, exact artifacts, and signed checksums/manifest data. No public v0.2 channel or artifacts are published now.
- [ ] Verify final producer names, version, architecture, hashes, signature, release links, and paired English/Vietnamese install and recovery links on the exact release source.
- [ ] Run P047 final release-candidate validation only after required P045/P036 and release prerequisites pass. P046 does not create a v0.2.0 tag.

The P045 cross-platform matrix run [37970834983](https://github.com/nghianguyen150612/Synveil/actions/runs/37970834983) built the Linux artifacts but failed its clean-machine evidence gate: Ubuntu 24.04 DEB, Fedora 42 RPM/AppImage guests did not reach readiness, and the aggregate native-evidence gate failed. This result is not a native qualification pass. See the P045 [PR #78](https://github.com/nghianguyen150612/Synveil/pull/78) for its complete checks and retained evidence.

## Current public guidance

Use the v0.1 manuals linked from the [documentation index](../../README.md) for the currently released product. The v0.2 guides in this folder are preview guidance; they do not replace v0.1 instructions or authorize installing unsigned CI artifacts.
