# Synveil v0.2 R04 release-candidate acceptance matrix

## Decision rule

This is the P045 R04 matrix. It preserves the original native journey scope
and uses only these overall and per-scenario statuses:
`PASS_NATIVE`, `PASS_PARTIAL`, `FAIL`, `BLOCKED_ARTIFACT`,
`BLOCKED_ENVIRONMENT`, `NOT_EXECUTED`, `NOT_APPLICABLE`.
Workflow success and a skipped native job cannot produce `PASS_NATIVE`.

No R04 product candidate was produced. The candidate pipeline is present on
the stacked R03/R04 branch but is absent from `main`, so GitHub will not accept
its manual dispatch until R03 is integrated. R03 candidates from test-merge
source `7f88d114e272ac220056db216030acda84be90e1` are older source artifacts
and are not substituted for R04 candidates. The expected filenames and
candidate source SHA are recorded to make the missing identity explicit.

## Environment matrix

| Environment | Candidate artifact / SHA-256 | Native OS version / runner or VM identity | Install | GUI | First run | Auth | Initial sync | Repair | Uninstall | Upgrade | Data preservation | Security | Overall |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Windows 11 x86_64 | `SynveilSetup.exe` and `synveil-0.2.0-windows-x86_64.zip`; no R04 bytes or digest: `BLOCKED_ARTIFACT` | Windows 11 disposable standard-user interactive VM unavailable; Windows 2025 CI is not this environment | `BLOCKED_ARTIFACT` | `BLOCKED_ENVIRONMENT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` |
| Ubuntu 24.04 x86_64 DEB | `synveil_0.2.0_amd64.deb`; no R04 bytes or digest: `BLOCKED_ARTIFACT` | Ubuntu 24.04 native desktop VM identity unavailable; local Debian 13 container is not equivalent | `BLOCKED_ARTIFACT` | `BLOCKED_ENVIRONMENT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` |
| Fedora 42 x86_64 RPM | `synveil-0.2.0-1.x86_64.rpm`; no R04 bytes or digest: `BLOCKED_ARTIFACT` | Fedora 42 native desktop VM identity unavailable | `BLOCKED_ARTIFACT` | `BLOCKED_ENVIRONMENT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` |
| Ubuntu 24.04 x86_64 quick install | Authenticated DEB/channel candidate absent: `BLOCKED_ARTIFACT` | P045 Ubuntu 24.04 disposable KVM guest unavailable | `BLOCKED_ARTIFACT` | `BLOCKED_ENVIRONMENT` | `NOT_APPLICABLE` | `NOT_APPLICABLE` | `NOT_APPLICABLE` | `NOT_APPLICABLE` | `NOT_APPLICABLE` | `NOT_APPLICABLE` | `NOT_APPLICABLE` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` |
| Fedora 42 x86_64 quick install | Authenticated RPM/channel candidate absent: `BLOCKED_ARTIFACT` | P045 Fedora 42 disposable KVM guest unavailable | `BLOCKED_ARTIFACT` | `BLOCKED_ENVIRONMENT` | `NOT_APPLICABLE` | `NOT_APPLICABLE` | `NOT_APPLICABLE` | `NOT_APPLICABLE` | `NOT_APPLICABLE` | `NOT_APPLICABLE` | `NOT_APPLICABLE` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` |
| AppImage on Ubuntu 24.04 x86_64 | `Synveil-0.2.0-x86_64.AppImage`; no R04 bytes or digest: `BLOCKED_ARTIFACT` | Ubuntu 24.04 native graphical VM identity unavailable | `BLOCKED_ARTIFACT` | `BLOCKED_ENVIRONMENT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ENVIRONMENT` | `BLOCKED_ENVIRONMENT` | `NOT_APPLICABLE` | `BLOCKED_ENVIRONMENT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` |
| AppImage on Fedora 42 x86_64 | `Synveil-0.2.0-x86_64.AppImage`; no R04 bytes or digest: `BLOCKED_ARTIFACT` | Fedora 42 native graphical VM identity unavailable | `BLOCKED_ARTIFACT` | `BLOCKED_ENVIRONMENT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ENVIRONMENT` | `BLOCKED_ENVIRONMENT` | `NOT_APPLICABLE` | `BLOCKED_ENVIRONMENT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` |
| Managed Host on Ubuntu 24.04 x86_64 | P036 eight-entry signed inventory, PostgreSQL 17, Caddy and signed server runtime absent: `BLOCKED_ARTIFACT` | Required `synveil-ubuntu-24.04-p036` and `synveil-p036-vm-control` availability not verifiable; runner API returned 403 | `BLOCKED_ARTIFACT` | `BLOCKED_ENVIRONMENT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` |
| Managed Host on Fedora 42 x86_64 | P036 eight-entry signed inventory, PostgreSQL 17, Caddy and signed server runtime absent: `BLOCKED_ARTIFACT` | Required `synveil-fedora-42-p036` and `synveil-p036-vm-control` availability not verifiable; runner API returned 403 | `BLOCKED_ARTIFACT` | `BLOCKED_ENVIRONMENT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` | `BLOCKED_ARTIFACT` |

The matrix records the primary blocking status in each field. P045's historical
Linux disposable guest label is `synveil-p045-kvm`; its required runner was
not verified. P036 uses `synveil-ubuntu-24.04-p036`,
`synveil-fedora-42-p036`, and `synveil-p036-vm-control`; their availability is
also unknown because the Actions runner API returned 403. Where the
candidate is absent, downstream product operations were not executed. No
scenario is reported as a pass based on a build, fixture, container, skipped
job, or workflow-level success.

## Candidate identity ledger

### R04 subject

| Field | Value |
| --- | --- |
| Product version | `0.2.0` |
| Candidate product source | `17c70d870fbdd743494469eb97b09b1541a5bfc1` |
| Producer workflow | `.github/workflows/release-artifact-candidate.yml` |
| Verification / acceptance workflows | `linux-clean-machine.yml`, `windows-native-acceptance.yml` |
| Producer run / artifact | Not produced; manual dispatch returns 404 because this workflow is not yet on default branch |
| Signature state | No R04 artifact; approved production signing inputs unavailable |
| Product files | Setup, portable ZIP, Ubuntu DEB, Fedora RPM, AppImage; expected filenames are listed above |
| Executable inventory | Not independently inspectable without candidate bytes. R03 package contracts expect `synveil-desktop`, `synveil-client`, host helper/maintenance executables, and Qt runtime closure; that older inventory is not attributed to R04 bytes. |

| R04 candidate | Product source / version | Platform / architecture | Filename | Product bytes / SHA-256 | Embedded executable inventory | Signing / provenance | Producer / verifier | Acceptance environment |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Windows Setup | `17c70d870fbdd743494469eb97b09b1541a5bfc1` / `0.2.0` intended | Windows / x86_64 | `SynveilSetup.exe` | Not produced | Not inspectable; candidate package build never ran | No bytes/signature; no producer run | `release-artifact-candidate.yml` → `windows-installer.yml`; `windows-native-acceptance.yml` | Windows 11 interactive standard-user VM unavailable; Windows 2025 hosted run is not equivalent |
| Windows portable | Same intended source / version | Windows / x86_64 | `synveil-0.2.0-windows-x86_64.zip` | Not produced | Not inspectable; candidate package build never ran | No bytes/signature; no producer run | `release-artifact-candidate.yml` → `windows-installer.yml`; `windows-native-acceptance.yml` | Same Windows 11 environment blocker |
| Ubuntu package | Same intended source / version | Ubuntu 24.04 / x86_64 | `synveil_0.2.0_amd64.deb` | Not produced | Not inspectable; candidate package build never ran | No bytes/signature; no producer run | `release-artifact-candidate.yml` → `linux-packages.yml`; `linux-clean-machine.yml` | Ubuntu 24.04 graphical guest unavailable; local Debian 13 is not equivalent |
| Fedora package | Same intended source / version | Fedora 42 / x86_64 | `synveil-0.2.0-1.x86_64.rpm` | Not produced | Not inspectable; candidate package build never ran | No bytes/signature; no producer run | `release-artifact-candidate.yml` → `linux-packages.yml`; `linux-clean-machine.yml` | Fedora 42 graphical guest unavailable |
| AppImage | Same intended source / version | Linux / x86_64; supported acceptance hosts are Ubuntu 24.04 and Fedora 42 | `Synveil-0.2.0-x86_64.AppImage` | Not produced | Not inspectable; candidate package build never ran | No product bytes/signature; authenticated AppImageKit runtime input does not authenticate Synveil | `release-artifact-candidate.yml` → `appimage.yml`; `linux-clean-machine.yml` | Ubuntu 24.04 and Fedora 42 graphical guests unavailable |

### Historical comparison only: R03 unsigned test-merge artifacts

These records are retained for traceability and reproducibility investigation;
they are not accepted R04 artifacts.

| Product artifact | Platform / architecture | Source SHA | Product bytes / SHA-256 | Signature / producer provenance | Separate Actions archive identity |
| --- | --- | --- | --- | --- | --- |
| Windows Setup `SynveilSetup.exe` | Windows / x86_64 | `7f88d114e272ac220056db216030acda84be90e1` | 32,815,179 / `6030cb810afd029732090148e76d872d61fb36af20cbde30e56251d4cfe335ac` | Unsigned; Windows installer producer [run 39254944703, artifact 12190505013](https://github.com/nghianguyen150612/Synveil/actions/runs/39254944703/artifacts/12190505013) | 135,154,302 / `dc296076a931555f790b2b7f3307434bf2771bb09e4bf37bbf9cb90c56df1459` |
| Windows portable ZIP `synveil-0.2.0-windows-x86_64.zip` | Windows / x86_64 | `7f88d114e272ac220056db216030acda84be90e1` | 51,621,495 / `139ed73ff48eff11198a9c73a28b10b086e25455fcbffbabf4ca4241a83a772b` | Unsigned; selected Windows installer producer, same artifact archive as Setup | Same Windows archive as Setup |
| Ubuntu DEB `synveil_0.2.0_amd64.deb` | Ubuntu 24.04 / x86_64 | `7f88d114e272ac220056db216030acda84be90e1` | 17,511,662 / `63243220397df76b766f2c1ea2ae3f13a3b9f60270ebde9ba380b00e9c1b2578` | Unsigned; Linux package producer [run 39254944698, artifact 12191351453](https://github.com/nghianguyen150612/Synveil/actions/runs/39254944698/artifacts/12191351453) | 34,839,225 / `381cc643f99a065760924c1f9629ebd82c3da7c48d39c4608a2ee5557efa2108` |
| Fedora RPM `synveil-0.2.0-1.x86_64.rpm` | Fedora 42 / x86_64 | `7f88d114e272ac220056db216030acda84be90e1` | 17,525,400 / `eb701f7c60b1cd15ecfc23f248be548aae1e6567494f0ece14e9c5f531cf6048` | Unsigned; same Linux package producer as DEB | Same Linux archive as DEB |
| AppImage `Synveil-0.2.0-x86_64.AppImage` | Linux / x86_64 | `7f88d114e272ac220056db216030acda84be90e1` | 49,125,880 / `a0a10f8770d7441a5e89a1dff18d82df2b34b28f4c8fb86cfa75843fd9a9859a` | Unsigned Synveil product; upstream AppImageKit runtime input was signature-verified; AppImage producer [run 39254945008, artifact 12186709692](https://github.com/nghianguyen150612/Synveil/actions/runs/39254945008/artifacts/12186709692) | 49,004,027 / `cf9d3e14bdaa3377978454ca8a75250f4d90d78774aab3d830b0f1e06520e6c4` |

Product-file hashes and outer Actions archive hashes are distinct values. The
portable ZIP's independent Rust CI result was 51,620,638 bytes,
`51c054eb14ead62c790528c69872bf88e7d936a52eb28c3db0421d7ebf53801` (Actions
archive 51,045,744 bytes,
`af8df97ee906b310b58962386ea6b47be8c6ed8e0d24f5a47aa96d7882349d07`). It
differs from the selected ZIP by 857 bytes. R04 requested both source archives
again; the artifact service returned HTTP 403, so the member-level difference
remains unresolved. No R03 row is promoted to the R04 subject.

### Historical embedded executable inventory and acceptance provenance

These details describe only the R03 source
`7f88d114e272ac220056db216030acda84be90e1` candidate bytes above. They do not
establish the R04 inventory.

| Artifact | Embedded executable inventory recorded for R03 | Producer / verifier | Acceptance environment and evidence strength |
| --- | --- | --- | --- |
| Windows Setup and portable ZIP | `synveil-desktop.exe`, `synveil-client.exe`, Qt 6 DLLs, `platforms/qwindows.dll`, QML modules/plugins and required runtime DLL closure. Portable ZIP inventory: 1,358 files. | Producer: `release-artifact-candidate.yml` → `windows-installer.yml` → `deploy/packages/build-windows.sh`. Exact candidate hash checked by `windows-native-acceptance.yml`. | Windows 2025 disposable standard-user fixture passed bounded installer/runtime lifecycle checks. Interactive Windows 11 GUI, genuine new logon and end-to-end named-pipe evidence remained blocked; this is not P045 native acceptance. |
| Ubuntu DEB and Fedora RPM | `/usr/bin/synveil-scheduled-maintenance-once`, `/usr/bin/synveil-client`, `/usr/bin/synveil-desktop`, `/usr/libexec/synveil-host-helper`, `/usr/libexec/synveil-host/prepare-host-runtime.py`, `/usr/libexec/synveil-host/register-host-inputs.py`, package services/configuration and declared dependencies. | Producer: `release-artifact-candidate.yml` → `linux-packages.yml`. Native consumer: `linux-clean-machine.yml` exact-run path. | R03 Ubuntu 24.04 APT and Fedora 42 DNF package-manager fixture checks ran. They did not complete the P045 graphical first-run, real authentication and synchronization journeys. |
| AppImage | `/usr/bin/synveil-desktop`, `/usr/bin/synveil-client`, `/usr/bin/synveil-appimage-integration`, Qt libraries/plugins/QML closure and AppRun integration. | Producer: `release-artifact-candidate.yml` → `appimage.yml`. Native consumer: `linux-clean-machine.yml` exact-run path. | R03 upstream runtime acquisition, AppDir inspection, QML smoke and current-user lifecycle checks ran in CI. No R03 native clean-machine GUI first-run or authenticated sync acceptance is claimed. |

The AppImageKit type-2 runtime used as the R03 AppImage build input was
944,632 bytes, SHA-256
`2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d`; upstream
commit `dd6cebedcbddde9c82f89b011e8e1d40b6e43868`, detached-signature
fingerprint `570C77ACEA40C0F1B758902CBF96CCA56490F695`. That authenticates the
upstream runtime input only, not the Synveil AppImage.

## Native CI evidence interpretation

The R04 PR checks at [PR #151](https://github.com/nghianguyen150612/Synveil/pull/151)
are source and harness validation. In particular, the Linux clean-machine PR
path does not run the guest matrix, and the Windows hosted worker cannot
produce Windows 11 interactive desktop/new-logon/named-pipe evidence. The
actual exact-head workflow IDs, failed jobs, skipped jobs, and required next
inputs are recorded in
[RELEASE_CANDIDATE_QUALIFICATION_R04.md](RELEASE_CANDIDATE_QUALIFICATION_R04.md).
