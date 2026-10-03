# Windows installer skeleton

## Scope and identity

P022 implements the first real Synveil Windows Setup producer without changing
the technology decision in ADR-058. `deploy/windows/installer/Synveil.iss` is a
minimal Inno Setup 6.7.3 project. Its stable product identity is
`{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}`. Setup is current-user only, uses
`{localappdata}\Programs\Synveil`, requests `lowest` privileges, disables
privilege overrides, and permits only x64 Windows. It creates one current-user
Start Menu shortcut and lets Inno own the HKCU uninstall registration.

The skeleton does not create a service, Run key, scheduled task, machine-wide
shortcut, PATH entry, or application state. `/STARTUP=1` is deliberately
rejected until P026; `/DESKTOPICON=1` is rejected until P023. `/STARTUP=0`,
`/DESKTOPICON=0`, and `/LAUNCH=0` are accepted, and every malformed Boolean
value fails closed. Ordinary uninstall owns only installed package files and
the shortcut. It never targets `%APPDATA%\Synveil`, `%LOCALAPPDATA%\Synveil`,
credentials, libraries, server state, or other application state.

## Build and trust path

Run on native Windows from the repository root:

```powershell
pwsh ./scripts/build-windows-installer.ps1
```

The default path asks `deploy/packages/build-windows.sh` to produce its normal
portable ZIP and export the exact same validated runtime closure. CI instead
builds that closure once and supplies `-RuntimeStagingDirectory`. The export is
replaced, never overlaid. The runtime's closed `SYNVEIL-MANIFEST.txt` binds each
relative path to its size and SHA-256. The adapter rejects traversal, rooted
paths, links/reparse points, missing files, unmanifested files, checksum/size
mismatches, duplicate case-insensitive destinations, and missing required core
files. It then generates one explicit `[Files]` entry per reviewed inventory
item; no wildcard reads an uncontrolled directory.

`toolchain.lock` pins the official immutable GitHub release asset for Inno
Setup 6.7.3 to SHA-256
`9c73c3bae7ed48d44112a0f48e66742c00090bdb5bef71d9d3c056c66e97b732`.
The script uses bounded HTTPS download/retry into a private temporary directory
and authenticates the bytes before running the upstream installer. That
installer is invoked unattended with `/CURRENTUSER`, `/NOICONS`, and an
explicit temporary `/DIR`; the temporary compiler installation is removed in
`finally`, so system-wide developer state is not changed. An explicit offline
directory is accepted only when its `ISCC.exe` has the locked 6.7.3 product
version; arbitrary PATH discovery is never used.

The Cargo workspace package version is parsed as strict SemVer. P022 supports
only stable three-component versions whose components are at most 65535 and
maps `major.minor.patch` to Windows `major.minor.patch.0`. Pre-release/build
metadata and non-representable values fail closed. The full current Git SHA is
embedded in generated build input and used by the release manifest.

All generated inputs live below `target/windows-installer-generated`, the
runtime export below `target/windows-installer-runtime`, and the exact output
is `target/windows-installer/SynveilSetup.exe`. Output validation checks exact
name, nonzero bytes, PE structure and recognized Inno loader machine, scans for
bounded secret markers, derives the SHA-256/size from the bytes, creates the
P005 manifest entry `windows-x86_64-installer`, and validates it against the
artifact root.

## Native smoke and reproducibility

`.github/workflows/windows-installer.yml` builds the authoritative native Qt
runtime on `windows-latest`, builds Setup twice from unchanged paths and input,
and compares the inventory, generated includes, and Setup bytes. It then runs
a non-elevated very-silent install with all opt-ins false, verifies the
LocalAppData root, core closure, current-user Start Menu link, and the unique
HKCU uninstall entry, and executes the installed QML smoke mode with a timeout.
The registered uninstall command—not a guessed `unins000.exe` name—is invoked
silently. Package files and integration must disappear while a synthetic state
sentinel outside the installation root remains. Bounded logs and the unsigned
artifact are retained for seven days.

Inno Setup 6 uses an x86 Setup bootstrap executable even when its script is
x64-only; the installed Synveil executables are AMD64 and the installer rejects
non-x64 systems. Verification therefore accepts the reviewed Inno bootstrap PE
machine (I386) or AMD64 while enforcing `ArchitecturesAllowed=x64`. This is not
an ARM64 support claim. P028 owns final native qualification.

P022 does not implement production signing. The intended ordering remains:
sign payload executables, build Setup, sign Setup, then derive final release
digests/manifests. P023 owns polished UI and desktop shortcut choice; P024 and
P025 own mature runtime/per-user behavior; P026 owns client-managed startup;
P027 owns full lifecycle UX; P028 owns native acceptance.
