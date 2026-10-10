# Advanced installation

> This guide is for administrators and distribution engineers. Ordinary desktop users should use the [installation guide](INSTALLATION.md). v0.2 is not released; commands, public trust material, and native qualification are not ready for production use.

## Linux terminal entrypoint and release trust

The repository contains the verified quick-install entrypoint `deploy/install/quick-install.sh`, which runs `scripts/linux_quick_install.py`. The implementation detects the OS and architecture before mutation and currently defines only Ubuntu 24.04 x86_64 and Fedora 42 x86_64 profiles. A profile option is an assertion; it cannot make an unsupported host qualify.

The tool requires an explicit channel location, trusted origin, minimum channel generation, and either an independently trusted channel SHA-256 pin or an explicit local Ed25519 public-key policy. It then authenticates the stable channel, binds the exact release manifest, selects one compatible artifact, verifies its bytes, stages privately, displays a plan, requests visible native package authorization, and verifies installed state. It runs as the ordinary user; only the exact native package-manager operation requests privilege.

The source parser accepts `--channel-url`, `--trusted-origin`, `--minimum-channel-generation`, and one of `--trusted-channel-sha256` or `--trust-policy`. Optional controls are `--platform-profile` (only `debian-x86_64` or `fedora-x86_64`, and only as a host assertion), `--detect-only` (read-only qualification output), and `--yes` (noninteractive consent after the plan). These names document the repository parser; they are not a ready-to-run public command.

These controls are not usable as a public install command until release engineering publishes the real stable-channel location, approved origin, authenticated generation, trusted verification bootstrap, and matching production artifacts. No production key or channel endpoint is published, so this guide deliberately provides no executable install invocation, URL, checksum, or key. Never use `curl | sh` or trust a checksum downloaded from the same unauthenticated location as the artifact.

The supported metadata is stable-only. There is no beta/nightly channel and no background updater. Release selection must be explicit, authenticated, compatible with the installed version, and protected against stale or rolled-back channel metadata. The P011 implementation requires caller-owned high-water evidence; it does not create a public endpoint or production trust root.

## Artifact authentication

Trust starts with a release identity established independently of the download host. The authenticated channel binds exact manifest bytes; the manifest binds the artifact's type, platform, architecture, version, size, and SHA-256; the artifact is verified before installation. HTTPS and allowed-origin checks remain enabled through redirects. A nearby unsigned checksum does not authenticate itself.

The P043 source implements Ed25519 verification and local trust-policy validation. Production key provisioning, private-key custody, deliberate public-root delivery, and public channel metadata remain release-engineering blockers. Do not fill this gap with a generated key, guessed URL, placeholder digest, or a CI artifact.

## AppImage runtime boundary

The AppImage producer targets Linux x86_64 and bundles the Synveil desktop/client and Qt runtime closure. The host still supplies a compatible Linux kernel, ELF loader and glibc baseline, graphics/display drivers, X11 or Wayland session, user DBus session, and Secret Service provider. Normal AppImage mounting requires FUSE 2 support. The exact compatible glibc floor must be published from the final artifact; CI's build host version is not by itself a support claim.

`APPIMAGE_EXTRACT_AND_RUN` is exercised as a CI fallback, not a qualified ordinary-user installation path. Do not prescribe manual extraction or a terminal permission change as a generic workaround. A release must list the exact graphical launch and runtime requirements for each qualified environment.

## Managed and externally managed servers

Personal/Home Host is intended to provision a Synveil-managed private PostgreSQL 17 dependency through the guided setup. Ordinary users should not install PostgreSQL, choose a connection string, or create service files for this path. P036's end-to-end Host acceptance and native evidence are still missing, so do not present managed hosting as production-qualified.

Advanced/Server reserves a configuration path for operator-managed PostgreSQL, custom storage, bind/origin, and external TLS/reverse-proxy integration. The operator owns database lifecycle, credentials, backup/restore, firewall policy, and service supervision. Use the current v0.1 [deployment guide](../DEPLOYMENT.md) for the released server procedure; v0.2 must publish its exact validated external-server workflow before use.

No Host path should open a public listener, change router/firewall rules, or add a mandatory relay without explicit user action and a qualified release procedure. Connect expects an already reachable server with a valid HTTPS endpoint; TLS verification must remain enabled.

## Service identity, privilege, and diagnostics

Windows Setup's ordinary path is a non-elevating per-user install. DEB/RPM use the operating system's visible package-manager authorization, then the desktop runs as the signed-in user. AppImage and its optional integration are user-level. Do not run the desktop or the whole quick installer as root/administrator. The package manager remains owner of its package database; do not delete locks or edit its database.

For diagnostics, use only the bounded safe error text and support reference shown by Synveil. There is no automatic diagnostic upload. A support report may include product version, OS version, architecture, the action taken, and the support reference. Redact usernames, absolute paths, library names, server addresses, credentials, and personal file names. No diagnostic export command is currently promised.

## Recovery, rollback, and distribution limits

An unknown mutation result requires reconciliation before retry. Native package state remains owned by the native package manager. Preserve transaction/recovery evidence and use the platform's named repair path; do not remove lock files, reset a database, or blindly replay an interrupted install.

The source implements scoped lifecycle and recovery policies, but full native interruption/power-cycle evidence remains open. Package replacement does not prove database rollback; downgrade is not generally supported. A schema rollback requires the documented compatible recovery owner and coordinated backup, not a partial file restore.

Use the [distribution readiness checklist](DISTRIBUTION_READINESS.md) before publishing any artifact. A source implementation, CI build, package inspection, or scoped native test does not equal native clean-machine qualification. For the trust contracts, see [download integrity](../../v0.2/RELEASE_DOWNLOAD_INTEGRITY.md), [channel selection](../../v0.2/RELEASE_CHANNEL_SELECTION.md), and [Linux quick install](../../v0.2/LINUX_QUICK_INSTALL.md).
