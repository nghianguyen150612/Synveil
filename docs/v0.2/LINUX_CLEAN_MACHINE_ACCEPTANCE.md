# Linux Clean-Machine Acceptance (Prompt020)

## Status

**Not accepted. Phase C is not closed.**

This document describes the Linux clean-machine acceptance layer added by
Prompt020. The contract, the executor and the hosted workflow exist and are
tested. The native evidence does not exist yet, and this document records that
plainly rather than implying otherwise.

## What this is

Prompt004 delivered the acceptance *contract* only: scenario JSON, a result
schema, and a validator that could never return `PASS`. Prompt020 adds the
Linux execution layer and the hosted matrix that will eventually produce native
evidence.

## Qualified platforms

Exactly two platforms are qualified, inherited from Prompt018:

| Platform | Profile | Package manager | Graphical handler |
| --- | --- | --- | --- |
| Ubuntu 24.04 x86_64 | `debian-x86_64` | `apt-get` | `gnome-software` |
| Fedora 42 x86_64 | `fedora-x86_64` | `dnf` | `plasma-discover` |

The following are **detected but not qualified** and must not be described as
supported. `scripts/linux_acceptance.py` reports them as detected rather than
inferring support from `ID_LIKE`, the `.deb` format, or APT:

Debian, Linux Mint, Pop!_OS, Rocky, AlmaLinux, openSUSE, CachyOS, Manjaro, Arch,
RHEL, CentOS.

Debian deserves an explicit note. `INSTALL-JOURNEY-2` historically read
"Debian or Ubuntu". Prompt018 qualified Ubuntu 24.04 only. The scenario title
and platform metadata were corrected in Prompt020 to say Ubuntu, and Debian is
now recorded as detected-not-qualified.

## Components

| Path | Role |
| --- | --- |
| `scripts/install_acceptance.py` | P004 contract validator; unchanged behaviour, still green |
| `scripts/linux_acceptance.py` | P020 Linux executor: host facts, artifact binding, clean-machine probe, closed action dispatch, result-v1 emission |
| `scripts/linux-acceptance-vm.sh` | VM control plane: pinned image acquisition, cloud-init seed, boot, snapshot, power cut |
| `deploy/acceptance/images.lock` | Pinned OS image digests |
| `.github/workflows/linux-clean-machine.yml` | Dedicated P020 hosted workflow |

## Design constraints

These are enforced by code and tests, not by convention.

* **No execution from JSON.** Every `steps[].action` resolves to a handler in a
  closed dispatch table. A string from scenario JSON is never evaluated, never
  used as `argv[0]`, and never passed to a shell. An unregistered action is
  reported `blocked`, not run.
* **Artifact identity is verified, not trusted.** The recorded SHA-256 is
  recomputed from the bytes on disk; a mismatch fails closed.
* **Cleanliness is inspected, never assumed.** Prior Synveil package,
  integration, unit or application state is detected and reported. A dirty
  machine blocks; it is never silently repaired to fake a clean start.
* **Absence of evidence is `BLOCKED`, not `PASS`.** Unqualified platform,
  missing capability, unverified artifact or missing driver all block.
* **Diagnostics are redacted.** Tokens, `postgres://user:pass@`, private keys
  and `password=` forms are stripped before a result record is emitted.
* **Bounded waits.** Every subprocess and every guest wait has a finite
  timeout. Fixed sleeps are not the primary synchronisation mechanism.

## Required evidence map

Statuses are current and truthful. Nothing below is claimed as accepted.

| ID | Requirement | Status |
| --- | --- | --- |
| LINUX-CLEAN-1 | Ubuntu 24.04 x86_64 clean environment proven | BLOCKED — image digest unpinned |
| LINUX-CLEAN-2 | Fedora 42 x86_64 clean environment proven | BLOCKED — image digest unpinned |
| LINUX-CLEAN-3 | Exact DEB artifact identity recorded | PARTIAL — built and hashed; not yet consumed by a native machine |
| LINUX-CLEAN-4 | Ubuntu graphical DEB install succeeds | NOT RUN — GUI driver not implemented |
| LINUX-CLEAN-5 | DEB authorization is visible | NOT RUN |
| LINUX-CLEAN-6 | DEB application-menu launch succeeds | NOT RUN |
| LINUX-CLEAN-7 | DEB first-launch requires no manual systemctl | NOT RUN |
| LINUX-CLEAN-8 | Fedora graphical RPM install succeeds | NOT RUN |
| LINUX-CLEAN-9 | RPM authorization is visible | NOT RUN |
| LINUX-CLEAN-10 | RPM application-menu launch succeeds | NOT RUN |
| LINUX-CLEAN-11 | RPM first-launch requires no manual systemctl | NOT RUN |
| LINUX-CLEAN-12 | Real AppImage launches on Ubuntu 24.04 | NOT RUN |
| LINUX-CLEAN-13 | Real AppImage launches on Fedora 42 | NOT RUN |
| LINUX-CLEAN-14 | AppImage requires no root | NOT RUN |
| LINUX-CLEAN-15 | AppImage no-terminal executable path proven | NOT RUN — file-manager property flow not implemented |
| LINUX-CLEAN-16 | Portable AppImage causes zero integration mutation | NOT RUN |
| LINUX-CLEAN-17 | AppImage startup opt-in integrates safely | NOT RUN |
| LINUX-CLEAN-18 | AppImage unit has no transient mount path | NOT RUN |
| LINUX-CLEAN-19 | Ubuntu quick install auto-detects correctly | NOT RUN |
| LINUX-CLEAN-20 | Fedora quick install auto-detects correctly | NOT RUN |
| LINUX-CLEAN-21 | Quick-install trust chain remains P006/P011 authenticated | NOT RUN |
| LINUX-CLEAN-22 | Unsupported platform causes zero mutation | TESTED — unqualified platforms block before any mutation |
| LINUX-CLEAN-23 | Startup Off persists | NOT RUN |
| LINUX-CLEAN-24 | Startup On persists | NOT RUN |
| LINUX-CLEAN-25 | Settings disable remains respected | NOT RUN |
| LINUX-CLEAN-26 | Native repair restores only package-owned state | NOT RUN |
| LINUX-CLEAN-27 | AppImage repair restores only integration-owned state | NOT RUN |
| LINUX-CLEAN-28 | Native uninstall removes package payload | NOT RUN |
| LINUX-CLEAN-29 | Ordinary uninstall preserves application state | NOT RUN |
| LINUX-CLEAN-30 | Ordinary uninstall preserves SecretStore identity | NOT RUN |
| LINUX-CLEAN-31 | Ordinary uninstall preserves client sync state | NOT RUN |
| LINUX-CLEAN-32 | Ordinary uninstall preserves user library digest | NOT RUN |
| LINUX-CLEAN-33 | Unknown schema fails closed | NOT RUN |
| LINUX-CLEAN-34 | Acquisition interruption is reconciled | NOT RUN |
| LINUX-CLEAN-35 | Payload-mutation interruption is reconciled | NOT RUN |
| LINUX-CLEAN-36 | Platform-integration interruption is reconciled | NOT RUN |
| LINUX-CLEAN-37 | Verification interruption is reconciled | NOT RUN |
| LINUX-CLEAN-38 | `OutcomeUnknown` is never blindly replayed | NOT RUN |
| LINUX-CLEAN-39 | DEB reproducibility remains green | FAIL — see below |
| LINUX-CLEAN-40 | RPM reproducibility remains green | UNVERIFIED |
| LINUX-CLEAN-41 | AppImage reproducibility remains green | FAIL — see below |
| LINUX-CLEAN-42 | P012–P019 regression suite remains green | FAIL — see below |
| LINUX-CLEAN-43 | Upgrade boundary is reported truthfully, not fabricated | HONOURED — see below |
| LINUX-CLEAN-44 | All evidence records validate against acceptance schema | TESTED — `linux_acceptance.py self-test` |
| LINUX-CLEAN-45 | Phase C checkpoint reflects only actual native evidence | HONOURED — checkpoint not claimed |

## Known blocking defects

### Artifact path leak in `synveil-desktop`

`synveil_assert_no_private_paths` rejects `synveil-desktop` because a private
or temporary build path is embedded in the binary. Only the desktop binary
fails; `synveil-client` and `synveil-scheduled-maintenance-once` pass both the
scan and byte-identity, which isolates the cause to the Qt/cxx-qt build rather
than to the general Rust toolchain path policy.

cxx-qt compiles QML sources through `qt_add_qml_module`, and the generated C++
embeds absolute QML source paths as string literals. Neither rustc's
`--remap-path-prefix` nor GCC's `-ffile-prefix-map` rewrites string literals,
so no existing remap can reach them.

Prompt020 improved the diagnostic so the failure names the leaking input
instead of a bare `/home/`, and added a `RUSTUP_HOME` remap as defensive
coverage. **The underlying root cause is not yet fixed.** The accurate next
step is to run the build with the improved diagnostic and fix the specific
input it names.

### Release binary reproducibility

`verify-release-build-reproducibility.sh` reports `synveil-desktop build-b
differs`. This is the same desktop/Qt link as above and is expected to resolve
with it.

### Rust CI failures classified

| Failure | Classification |
| --- | --- |
| `sysusers_tmpfiles_artifacts_match_authoritative_sources` | ENVIRONMENT_LIMITATION — `systemd-tmpfiles --dry-run` needs systemd >= 250; the runner and the authoring host provide 249 |
| Windows/macOS `synveil-install-engine` compile errors | LINUX_INSTALL_REGRESSION — the crate is not cfg-gated off Unix |
| `Linux AppImage` private path scan | LINUX_INSTALL_REGRESSION, still failing on `main` |
| `Linux native packages` `build-b differs` | LINUX_INSTALL_REGRESSION |
| `PostgreSQL 17` `d.daticulocale` missing | PRE_EXISTING_UNRELATED_FAILURE — server schema, outside Linux install scope |

## Upgrade boundary

`UPGRADE-TEMPLATE-1` remains **blocked, not release-applicable**. The workspace
product version is still `0.1.0`, and no version-distinct v0.1.0 to v0.2.0
release artifact exists. Prompt020 did not edit package versions to manufacture
an upgrade. The real version-distinct upgrade belongs to the later RC/release
sequence.

`FIRST-RUN-1` and `FIRST-RUN-2` remain out of scope: they belong to P037–P042
and P029–P036 respectively.

## What closes Phase C

Phase C closes only when real hosted native evidence exists for the graphical
DEB and RPM journeys, real AppImage launch on both qualified distributions,
verified quick install through the full P006/P011 trust chain, repair and
uninstall preservation, and VM power-cut interruption reconciliation.

Until then these tokens are not emitted:

```
SYNVEIL_LINUX_CLEAN_MACHINE_ACCEPTANCE_READY
SYNVEIL_V0_2_LINUX_INSTALL_EXPERIENCE_READY
```