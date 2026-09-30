# Prompt014 manifest

## Provenance and scope

- Accepted Prompt013/013A starting SHA: `8f8115182ba4a449eae579e5cd24b312e03c4d7d`.
- Branch: `codex/p014-fedora-rpm-package-ux`.
- Commit subject: `feat: deliver Fedora RPM package UX`.
- No database migration, schema version, sync behavior, or product-version
  change is part of Prompt014.

The implementation improves the canonical RPM spec, aligns shared package
metadata, adds a focused source/real-artifact validator, and extends the shared
native-package workflow with a separate Fedora 42 userspace acceptance job.
The Debian package path remains a required regression gate.

## RPM-UX evidence map

| IDs | Deterministic evidence |
| --- | --- |
| RPM-UX-1–2 | Focused validator checks the real RPM name, required metadata, and x86_64 architecture. |
| RPM-UX-3–5 | Source validator locks the canonical desktop entry, icon identity, and direct installed executable. |
| RPM-UX-6–7 | Spec and built-requirement checks lock the Fedora runtime closure and reject development/server leakage. |
| RPM-UX-8–11 | Payload and executable-scriptlet audits preserve the user-unit boundary and prohibit activation, process launch, autostart, home guessing, and network access. |
| RPM-UX-12 | Real RPM query checks the canonical payload's exact root ownership and file modes. |
| RPM-UX-13 | Fedora job installs the downloaded build artifact through DNF and queries RPM's database. |
| RPM-UX-14 | Fedora job launches the installed desktop's bounded QML smoke as a clean non-root user. |
| RPM-UX-15–16 | DNF erase, removed package/file assertions, and controlled system/user/external sentinels prove native data-preserving removal. |
| RPM-UX-17 | A second canonical package build must compare byte-identically with the primary RPM. |
| RPM-UX-18 | Both output roots' artifact and release manifests are consumed only after the second producer completes. |
| RPM-UX-19 | Validator rejects Bash-only `pipefail` in rpmbuild's `/bin/sh` `%install` section. |
| RPM-UX-20 | Prompt013 validator plus DEB build, inspection, reproducibility, APT, launch, erase, and preservation stay in the workflow. |

## Truthful evidence boundary

`fedora:42` is explicitly a Fedora userspace/container native-package
qualification, not full graphical clean-machine acceptance. Final artifact
hashes and PASS/FAIL status are read from the final PR-head workflow rather
than predeclared in source. P015–P020 remain pending.
