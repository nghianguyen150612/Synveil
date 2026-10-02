# Linux platform detection and qualification

Prompt018 detects the Linux environment in which the installer is executing.
In a container or chroot this is that environment, not necessarily the physical
host. Detection is deliberately separate from support qualification.

## Exact v0.2 policy

`deploy/install/linux-platforms-v1.json` is the single machine-readable policy.
The table is reconciled with that file and with the pinned hosted environments:

| Distribution | Version | Arch | Artifact / profile | Manager | Status |
| --- | --- | --- | --- | --- | --- |
| Ubuntu | 24.04 | x86_64 | DEB / `debian-x86_64` | APT | Qualified |
| Fedora | 42 | x86_64 | RPM / `fedora-x86_64` | DNF | Qualified |
| Debian | any detected version | x86_64 | none | none | Detected, not qualified |
| Linux Mint, Rocky, Arch, CachyOS | any detected version | x86_64 | none | none | Detected, not qualified |
| Any distribution | any | aarch64, 32-bit, or unknown | none | none | Unsupported architecture |

Versions are exact strings, not ranges or prefixes. Ubuntu derivatives do not
inherit Ubuntu qualification, and RHEL/Fedora-like distributions do not inherit
Fedora qualification. A future version remains unqualified until it receives a
reviewed policy row and acceptance evidence.

## Safe detection

The detector reads at most 64 KiB from `/etc/os-release`, falling back to
`/usr/lib/os-release` only when the former is absent. It parses shell-compatible
quoting as data with Python's standard library: it never sources, evaluates, or
expands the file. Missing/duplicate critical fields, malformed quoting, NUL,
invalid UTF-8, and oversized input fail closed. `ID` and exact `VERSION_ID` are
authoritative; `ID_LIKE` is diagnostic data only and cannot grant support.

`x86_64` and `amd64` normalize to `x86_64`; `aarch64` and `arm64` normalize to
`aarch64`. Only x86_64 is qualified. There is no 32-bit, ARM, unknown-machine,
version, distro, package-family, or filename fallback. Only after an exact
policy match does the detector check `apt-get` plus `dpkg-query`, or `dnf` plus
`rpm`; tool presence never establishes distribution identity.

## CLI and ordering

Run `./deploy/install/quick-install.sh --detect-only` for deterministic JSON.
It reports `schema_version`, identity, version, architecture, qualification
status, target/profile/artifact/manager, and a reason code. It performs no
release request, staging, sudo, manager transaction, or integration mutation.
It includes no machine or user identifiers.

Normal installation auto-selects the profile. `--platform-profile` remains an
optional assertion for diagnostics and must equal the qualified result; it can
never bypass policy. Qualification and manager consistency checks precede the
unchanged P011/P006 authenticated acquisition chain and all native mutation.
Unsupported and malformed environments exit 10 with a typed, useful reason.

P018 does not launch the application or enable autostart (P019), and does not
claim the broad clean-machine matrix owned by P020.
