# v0.2 distribution readiness

### R05 release status — 2026-10-20

**`R05_INTEGRATED_RELEASE_BLOCKED`.** Synveil `v0.2.0` has not been released.
There is no stable tag, public GitHub Release, authenticated download, or
activated stable channel. The v0.2 candidate outputs are unsigned CI inputs;
do not install them or distribute their Actions artifact archives.

The released `v0.1.0` guidance remains the public installation guidance. The
R05 qualification and blocker record is in
[the R05 release manifest](../../v0.2/RELEASE_HARDENING_R05_MANIFEST.md).
This page describes qualification targets and their current limits; it does
not declare those targets supported.

## Qualification targets

| Candidate format | Target | Current status |
| --- | --- | --- |
| `SynveilSetup.exe` | Windows 11 `x86_64` | **Blocked; Not yet published.** The exact-main build produced a reproducible unsigned Setup candidate and its standard-user product acceptance step passed on GitHub-hosted Windows Server 2025. The workflow then correctly blocked at unavailable interactive GUI, genuine logon and named-pipe surfaces. Required Windows 11 evidence remains outstanding. |
| `synveil_<version>_amd64.deb` | Ubuntu 24.04 `x86_64` | **Blocked; Not yet published.** No authenticated candidate completed the native desktop and lifecycle journey. |
| `synveil-<version>-1.x86_64.rpm` | Fedora 42 `x86_64` | **Blocked; Not yet published.** No authenticated candidate completed the native desktop and lifecycle journey. |
| `Synveil-<version>-x86_64.AppImage` | Linux `x86_64` | **Blocked; Not yet published.** The exact integrated-main build, independent reproduction, inspection, smoke and current-user lifecycle checks passed on Ubuntu 22.04 CI; native Ubuntu 24.04/Fedora launch, relocation and lifecycle acceptance remain outstanding. |

`Implemented` describes source behavior. `Build validated` describes a
completed build check. `Native tested` describes a scoped machine test.
`Native qualified` requires the complete applicable clean-machine journey.
`Blocked` records missing mandatory evidence. `Not yet published` means no
public v0.2 download exists. `Unsupported` means this release plan makes no
compatibility claim.

No v0.2 target is `Native qualified`. The target list above must not be used as
a support claim or as a source for download links.

## Connect and Host

**Connect** means connecting the desktop client to an existing Synveil server.
**Host** means setting up a managed Synveil server on this device. These are
product modes, not qualified v0.2 installation journeys. First-run Connect and
Host acceptance remain blocked.

Managed Host qualification needs authentic, signed Synveil server,
PostgreSQL 17 and Caddy inputs for Ubuntu 24.04 and Fedora 42, plus the approved
public keyring, independently pinned publisher fingerprints, complete license
and notice evidence, and a passing signed eight-entry P036 inventory. Those
production trust inputs and the installed Host acceptance evidence have not
been supplied.

## Upgrade, repair and data

The Windows and Linux package policies have empty `upgrade_from` lists. No
automatic `v0.1.0` to `v0.2.0` upgrade is approved or tested. Repair,
uninstall/reinstall and data preservation are not qualified across the target
matrix. No database rollback has passed. Keep a verified backup of released
data and do not test candidate installers against the only copy.

## Signing and downloads

There are no production Authenticode, RPM/OpenPGP or Ed25519 signatures and no
independently trusted v0.2 public key policy. No stable-channel metadata or
public v0.2 checksums exist. Do not treat a green build, a CI archive, or an
unsigned candidate manifest as publisher authentication.

Before a future download is trusted, the release must publish the exact signed
artifact and authenticated manifest, then verify the downloaded bytes,
signature, source identity and channel generation. That verification procedure
cannot be completed against unpublished files.

## Unsupported platforms and current guidance

This v0.2 release plan does not qualify Debian as a separate target, other
Linux distributions, `ARM64/aarch64`, 32-bit systems, `macOS`, `Android`, or
`iOS`. Source code presence does not establish support. This release does not
claim an Android or iOS store release.

Continue to use the published `v0.1.0` guidance in the
[v0.1 release notes](../RELEASE_NOTES_v0.1.md). The source tree is available
through the repository [README](../../../README.md). No v0.2 installer steps,
download links or release checksums are available yet.
