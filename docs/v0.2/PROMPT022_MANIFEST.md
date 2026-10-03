# Prompt022 manifest — SynveilSetup.exe skeleton

## Identity and implementation

| Item | Value |
|---|---|
| starting SHA | `21ae1bb47d5dc574068716c338b44a9d6f528854` |
| branch | `codex/p022-windows-installer-skeleton` |
| Inno Setup | `6.7.3` |
| official immutable asset | `https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe` |
| authenticated asset SHA-256 | `9c73c3bae7ed48d44112a0f48e66742c00090bdb5bef71d9d3c056c66e97b732` |
| stable AppId | `{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}` |
| install root | `{localappdata}\Programs\Synveil` |
| privilege mode | `PrivilegesRequired=lowest`; overrides disabled |
| output | `target/windows-installer/SynveilSetup.exe` |
| artifact identity | `windows-x86_64-installer` / `windows_installer` / `primary_installer` |
| components | `synveil-desktop`, `synveil-client` |
| payload source | `deploy/packages/build-windows.sh --staging-dir=...` |

The installer consumes the same post-`windeployqt`, PE/import-audited,
manifested runtime closure as the retained portable ZIP. It does not maintain
a second Qt DLL list. Reviewed source is `Synveil.iss`, `toolchain.lock`, and
the PowerShell adapter. Version definitions, source revision, explicit Inno
file entries, normalized inventory, artifact, and release manifest are
generated only below `target/`.

## Files

Added: `deploy/windows/installer/Synveil.iss`,
`deploy/windows/installer/toolchain.lock`,
`scripts/build-windows-installer.ps1`,
`scripts/validate-windows-installer.py`,
`.github/workflows/windows-installer.yml`, this manifest, and
`WINDOWS_INSTALLER_SKELETON.md`. Changed:
`deploy/packages/build-windows.sh`, `ROADMAP.md`, and
`RELEASE_ARTIFACT_MANIFEST.md`.

## Evidence status

The official distribution bytes were downloaded directly from the locked URL
on 2026-10-03 and locally hashed to the recorded value before it was committed.
The network-free source validator, Python compilation, Bash syntax, release
manifest tests, formatting/check validation, a portable PowerShell parser, and
repository diff checks are the local validation surface. This Linux environment
has no native Windows/Inno execution, so it cannot compile or run Setup locally.

At implementation time, the public Actions API reported the final P021/main
head workflows as queued with no conclusions. This is classified
`ENVIRONMENT_LIMITATION`; no P021-owned regression or hosted PASS is inferred.
The focused P022 `windows-latest` result is **pending until this branch is
pushed**. Native runtime build, tool acquisition, compiler output, silent
install/uninstall, HKCU/Start Menu checks, installed smoke, sentinel
preservation, and Setup byte reproducibility are therefore **not yet claimed**.

Reproducibility design is a strict byte comparison across two independent
compiler invocations with unchanged runtime and generated-input paths. Its
status is **pending hosted Windows execution**, not PASS. The Inno 6 Setup
bootstrap is expected to be I386 while the script and payload are x64-only;
this reviewed toolchain property is documented rather than mislabeled AMD64.

Known limitations are the standard skeleton wizard, no desktop shortcut,
unsigned payload/Setup, and no enabled startup option. P023 (polished UI), P024
(runtime maturity), P025 (per-user qualification), P026 (Task Scheduler option),
P027 (complete repair/upgrade/uninstall), and P028 (full native acceptance) are
explicitly deferred. No Linux Phase-C or final Windows readiness marker is
asserted by this manifest.
