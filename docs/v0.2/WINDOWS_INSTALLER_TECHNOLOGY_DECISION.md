# Windows installer technology decision

Status: **LOCKED for P022–P028**

Decision: **Inno Setup 6.7.3, compiled as a per-user x86_64 Setup EXE**

Artifact: `SynveilSetup.exe`

This is an architecture decision, not an assertion that a Windows installer
exists. P022 creates the installer project and P028 supplies native installed
product evidence.

## Facts audited at the P021 baseline

The baseline is `508153ac382ad907fd402717c8c4143536c1d5ae`. Current source has
an unsigned, self-contained `windows-x86_64` runtime ZIP. It contains
`synveil-desktop.exe`, the separate `synveil-client.exe`, `qt.conf`, the
Qt/QML/plugin closure selected by native `windeployqt`, compiler runtime files,
and notices. `deploy/packages/build-windows.sh` explicitly says that this ZIP
is not an installer and emits the existing release-manifest entry
`windows-x86_64-portable`.

The runtime facts constrain, rather than invite, installer redesign:

* Windows credentials use the native keyring/Credential Manager provider.
* desktop control is a profile-scoped, owner-only named pipe;
* persistent background launch is an explicitly enabled, current-user Task
  Scheduler logon task implemented by `crates/client/src/launch.rs`;
* desktop and client remain separate processes and no Windows service exists;
* configuration, credentials, sync state and user files are runtime-owned.

The installer therefore consumes a verified staged runtime directory; it does
not discover Qt, store credentials, configure a server, operate sync, or absorb
either executable into itself.

## Required product shape

The ordinary, keyboard-operable Windows 11 flow is exactly:

1. **Welcome** (standard welcome page).
2. **Terms / install options** (license acceptance plus three check boxes:
   **Start Synveil when I sign in**, **Create a desktop shortcut**, and
   **Open Synveil after installation**).
3. **Installing** (standard progress page).
4. **Finish** (standard completion page and the selected launch action).

No destination, Start Menu folder, architecture, Qt, pipe, task XML, database,
credential or server-configuration page appears in the ordinary flow. Standard
Inno controls and wizard pages are used; the options page is one small custom
page made from native Inno controls with labels associated to check boxes,
logical tab order, keyboard mnemonics, and no owner-drawn framework.

## Equal-criteria comparison

Ratings are project decisions based on the same 24 criteria below: **Y** meets
the criterion directly, **C** needs bounded project code or carries a notable
constraint, and **N** conflicts with the v0.2 shape.

| # | Criterion | Inno Setup | WiX MSI | WiX Burn | NSIS | MSIX | custom Rust EXE |
|---:|---|:---:|:---:|:---:|:---:|:---:|:---:|
| 1 | Four-screen `SynveilSetup.exe` | Y | C | Y | C | N | C |
| 2 | Per-user, no-admin default | Y | C | C | Y | Y | C |
| 3 | Deterministic silent CI surface | Y | Y | Y | Y | Y | C |
| 4 | Deterministic build automation | C | Y | Y | C | Y | N |
| 5 | Existing runtime-directory payload | Y | Y | Y | Y | C | C |
| 6 | Start Menu integration | Y | Y | C | Y | Y | C |
| 7 | Optional desktop shortcut | Y | Y | C | Y | C | C |
| 8 | Explicit startup preference | Y | C | C | Y | C | C |
| 9 | Safe current-user task handoff | C | C | C | C | C | C |
| 10 | Repair | C | Y | Y | C | Y | N |
| 11 | Upgrade | Y | Y | Y | C | Y | N |
| 12 | Data-preserving uninstall | Y | Y | C | Y | Y | C |
| 13 | P007–P010 recovery semantics | C | C | C | C | C | N |
| 14 | Verify before success | C | C | C | C | C | C |
| 15 | GitHub `windows-latest` | Y | Y | Y | Y | Y | C |
| 16 | No paid build service | Y | Y | Y | Y | Y | Y |
| 17 | Repository maintainability | Y | C | N | C | C | N |
| 18 | Acceptable toolchain burden | Y | C | N | Y | C | N |
| 19 | Future Authenticode | Y | Y | Y | Y | Y | Y |
| 20 | Exact identity/version/arch metadata | Y | Y | Y | Y | Y | C |
| 21 | No ownership of config/credentials | Y | Y | Y | Y | C | C |
| 22 | P028 clean-machine automation | Y | Y | C | Y | Y | C |
| 23 | MIT project distribution compatible | Y | Y | Y | Y | Y | C |
| 24 | Credible maintenance path | Y | Y | C | Y | Y | N |

### Factual observations versus choices

* Inno's `PrivilegesRequired=lowest` selects non-administrative mode; in that
  mode its automatic uninstall record is under HKCU and its automatic program
  group is current-user. It provides `/VERYSILENT`, fixed `/LOG=`, `/NORESTART`,
  `/LOADINF=`, stable success `0` versus nonzero failure semantics, task/icon
  declarations, an automatic uninstaller, Pascal event hooks, and external
  Authenticode tooling. These are tooling facts, documented in the
  [Inno non-administrative mode](https://jrsoftware.org/ishelp/topic_admininstallmode.htm),
  [command-line](https://jrsoftware.org/ishelp/topic_setupcmdline.htm),
  [exit-code](https://jrsoftware.org/ishelp/topic_setupexitcodes.htm), and
  [SignTool](https://jrsoftware.org/ishelp/topic_setup_signtool.htm) references.
* MSI gives the strongest standardized repair/component database. WiX can
  author it, but a truly per-user MSI has Windows Installer component,
  registry/key-path and upgrade constraints, while the requested branded EXE
  would add a Burn wrapper. Synveil has one already-closed runtime payload and
  no prerequisites to chain. **Project choice:** that two-layer complexity
  buys little for v0.2.
* Burn is a bootstrapper/chainer, not a replacement payload installer.
  **Project choice:** reject it because there is no bundle chain; adding Burn
  plus MSI solely to obtain an EXE enlarges identity, repair and signing
  surfaces.
* NSIS supports no-admin files, shortcuts and silent operation but makes more
  lifecycle, rollback, task persistence and accessibility behavior custom
  script responsibility. **Project choice:** Inno's declarative uninstall log,
  application identity and wizard model are the smaller reviewed surface.
* MSIX is per-user and transactional, but package identity/container rules,
  signing/developer installation, restricted package mutation, and Windows
  app-model integration do not naturally match the existing unpackaged Qt app,
  its current-user Task Scheduler authority, `SynveilSetup.exe`, or ordinary
  unsigned CI installation. **Project choice:** do not restructure the runtime
  around MSIX in Phase D.
* A custom Rust bootstrapper would duplicate mature UI, elevation, uninstall
  registration, file-in-use, rollback, signing and accessibility machinery.
  **Project choice:** reject it, and do not introduce Electron.

## Locked toolchain and build inputs

P022 must pin **Inno Setup 6.7.3** by immutable upstream release URL and
SHA-256 in reviewed toolchain metadata. `ISCC.exe` is acquired on native
Windows, verified before execution, and invoked noninteractively. Floating
Chocolatey/winget installs are not authoritative. A future toolchain change is
a reviewed dependency update with a two-build comparison and native lifecycle
smoke; no `latest` resolution is allowed.

The reviewed `.iss` and Pascal include files are source. Generated defines and
the staged payload inventory are build products. Inputs are the repository
revision, Cargo package version, exact Windows runtime closure, normalized
payload metadata, pinned compiler, icons/license/notices, and explicit build
mode. Build A/B byte equality is a required goal and gate; if the upstream EXE
contains unavoidable varying metadata, P022 must diagnose and document it
rather than call a merely functional build reproducible.

## Per-user location and privilege model

The one authoritative v0.2 mode is **PER_USER**:

```text
{localappdata}\Programs\Synveil
{userprograms}\Synveil\Synveil.lnk
HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\<stable AppId>_is1
```

`PrivilegesRequired=lowest` is fixed and privilege overrides are disabled.
There is no P021 per-machine mode. The application directory is not on `PATH`;
all launches use an absolute, validated path. LocalAppData is writable by that
user, which is appropriate because code is neither elevated nor shared. This
avoids a writable privileged executable directory and prevents one user from
changing another user's install or state. Each Windows account receives its
own payload, shortcuts, HKCU uninstall record and optional scheduled task.

Program Files would require elevation and introduce cross-user ownership while
the task, credentials, state, Start Menu choice and runtime are deliberately
user-scoped. Future signed upgrades replace only the same user's directory and
retain the stable product identity. A future per-machine offering requires a
new ADR and an exact elevation/multi-user design; it is not a hidden switch.

## Frozen ownership

`PACKAGE_OWNED` is limited to the installed desktop/client executables, their
Qt DLL/plugin/QML and compiler runtime closure, `qt.conf`, icons, license and
notices, installer-created Start Menu/optional desktop shortcuts, the Inno
uninstaller/log and the HKCU uninstall registration. These may be repaired,
replaced, or removed.

Ordinary repair, upgrade and uninstall preserve `APPLICATION_CONFIG`,
`CREDENTIAL_STATE`, `CLIENT_SYNC_STATE`, `USER_LIBRARY`, `SERVER_CONFIG`,
`SERVER_DATABASE`, and `SERVER_OBJECT_DATA`. They never wildcard-delete a
profile/AppData tree. The user-selected Task Scheduler registration is an
application integration effect, not package payload. A future destructive
purge is a separate, explicit operation and is not an uninstall check box.

## Component boundary and verified completion

The existing conceptual flow remains authoritative:

| Stage/owner | Responsibility |
|---|---|
| common Rust policy | P006 artifact identity/integrity, P007–P010 lifecycle vocabulary, preservation classes, error/reconciliation semantics; not linked into Setup |
| Inno Setup | native wizard, per-user file transaction/uninstall log, shortcuts, HKCU registration, close-app handling and native result |
| thin Windows adapter (P022+) | validate staged manifest, map exact paths/version/AppId, bounded verification helper and typed result; never owns user state |
| desktop/client runtime | named pipe, credential store, profiles, sync, verified foreground launch |
| client Task Scheduler manager | sole create/query/run/delete authority for current-user login startup |
| Windows registration | Inno-owned uninstall entry and shortcuts only |

Setup performs preflight and plan before mutation, installs only a prevalidated
closed inventory, integrates standard shortcuts/registration, then explicitly
verifies every expected file/digest, PE architecture, executable location and
a supported noninteractive client/desktop probe before Complete. A postinstall
launch is not the verification step. If verification fails, Setup returns
failure and rolls back what it can; an interrupted or unknown effect is
inspected against files, uninstall identity, shortcuts and task state before
retry, never blindly replayed. The adapter may be a small signed helper or
bounded executable modes in P022–P024; Inno need not link Rust.

## Startup choice boundary

Inno collects **Start Synveil when I sign in**, but never authors task XML,
calls `schtasks` with an installer-built definition, creates a service/Run key,
or invents another task. A task is profile-scoped and a fresh install has no
profile, so Setup cannot safely register one. After payload verification it
passes the explicit desired state by separated arguments to a reviewed
client-owned P026 helper. That runtime boundary stores the consent as
application-owned preference; first-run applies it through the existing
`BackgroundClientManager` only after a profile exists. Setup never writes the
preference file itself. If the helper cannot durably acknowledge the choice,
Setup does not report integration success.

The preference is tri-state: a fresh interactive selection records enabled or
disabled; repair/upgrade with no new explicit choice preserves observed state;
silent CI must state its desired value. Explicit disable causes the runtime
authority to delete any authoritative task and remains disabled across repair,
upgrade and reopen. Ordinary uninstall requests task deletion through that
same runtime boundary before removing binaries while preserving all durable
state. There is never a duplicate task or hidden enablement.

## Lifecycle identity and policy

* A single constant GUID-form Inno `AppId` is the product/upgrade identity for
  x86_64 per-user Synveil. P022 generates it once and commits it; it is not a
  version or build-derived value.
* Cargo workspace package version is the sole product version. It populates
  `AppVersion`, display version and four-part Windows version metadata through
  a documented deterministic SemVer mapping. Inno owns installed-version
  comparison; release metadata owns artifact selection.
* **Fresh install:** validate, copy, integrate, verify, record success.
* **Same version/reinstall/repair:** rerun the exact Setup with `/REPAIR=1`;
  replace and re-verify every package-owned file and standard integration,
  preserve state/startup preference, and fail on an identity mismatch.
* **Upgrade:** a strictly newer supported version with the same AppId replaces
  the closed payload, removes obsolete package-owned files from an explicit
  manifest, re-verifies, retains preferences/state, and updates one uninstall
  record. No auto-update is introduced.
* **Downgrade:** denied before mutation unless a future authenticated release
  policy explicitly authorizes that exact transition. `/SUPPRESSMSGBOXES`
  cannot bypass it.
* **Uninstall:** the registered per-user uninstaller removes package-owned
  integration and payload, asks the client authority to remove its task, and
  preserves all state classes above. Uninstaller failure is nonzero.

No planned operation requires reboot. CI always passes `/NORESTART` and treats
the dedicated restart-needed code as failure. Locked files cause a bounded
close/retry failure, not reboot-time privileged replacement. Interrupted
outcomes follow P008/P009 reconciliation before retry.

## Deterministic unattended/native-CI contract

P022–P028 must implement these stable wrapper commands (the wrapper resolves
the registered uninstaller rather than hard-coding `unins000.exe`):

```powershell
# fresh install; STARTUP=0 and LAUNCH=0 are explicit
.\SynveilSetup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART `
  /LOG="$env:RUNNER_TEMP\synveil-install.log" /STARTUP=0 /DESKTOPICON=0 /LAUNCH=0

# same-version repair
.\SynveilSetup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /REPAIR=1 `
  /LOG="$env:RUNNER_TEMP\synveil-repair.log" /LAUNCH=0

# upgrade: invoke newer SynveilSetup.exe with the same switches
# uninstall: invoke registered QuietUninstallString plus
# /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /LOG=<fixed-path>
```

Exit `0` alone is necessary, not sufficient: P028 verifies HKCU registration,
the exact LocalAppData path and closed payload hashes, shortcuts, absence or
presence of the one task as selected, and launches installed binaries with
bounded test modes. Setup codes `1–8`, the chosen distinct reboot-required
code (locked by P022), unexpected future codes, timeout, verification failure,
or missing log are failure. Logs are fixed under runner temp in CI, bounded as
uploaded artifacts, and must redact secrets/URLs/user library paths. Product
logs must never contain credentials.

The `windows-latest` job runs as its ordinary runner account with no elevation,
asserts `PrivilegesRequired=lowest`, suppresses Finish launch (`/LAUNCH=0`),
then separately starts the installed desktop/client probes. Interactive Setup
remains the product path; unattended switches are an automation contract.
Interactive diagnostics use
`%LOCALAPPDATA%\Synveil\installer\latest-install.log`; the adapter rotates one
previous bounded log. CI overrides this with the fixed runner-temp paths above.

## Artifact, provenance and signing

P022 produces exactly `target/windows-installer/SynveilSetup.exe`, described by
the existing P005 release-manifest schema as the Windows x86_64 installer (not
a second metadata format). Its manifest record carries canonical artifact ID,
`installer` role/type, `windows`, `x86_64`, Cargo version, byte size, SHA-256,
source revision and build provenance using the P005/P006/P011 rules. Release
selection binds the exact manifest and digest before any mutation.

Unsigned CI and signed release artifacts are distinct provenance modes. CI
uses the unsigned digest and must never claim publisher trust. The release
pipeline signs payload executables first, compiles the installer, then applies
SHA-256 Authenticode plus an approved timestamp to Setup and the generated
uninstaller through Inno's external SignTool boundary. The final signed bytes
receive their final digest/size/signature fields in the same release manifest.
Keys and passwords are external secrets and are never compiler definitions,
source, logs or artifacts. P021 does not choose a CA or enable production
signing.

## Security requirements

The staged payload and compiler are digest-verified before mutation/build.
Installer source uses declarative paths or separated executable/argument
values—never a shell-concatenated string. It downloads nothing at install time,
embeds no password, accepts no trust bypass, exposes no remote command surface,
and uses safe absolute executable paths. DLLs remain beside the application in
the audited runtime closure; verification rejects unexpected/development DLLs
and unsafe traversal/symlinks. No privileged writable directory exists because
the process is never elevated. Diagnostics are typed, bounded and redacted.

## P022 implementation blueprint

P022 starts without another technology decision:

1. Create `deploy/windows/installer/Synveil.iss`, reviewed includes under
   `deploy/windows/installer/include/`, a committed toolchain lock containing
   Inno 6.7.3 URL/SHA-256, and `scripts/build-windows-installer.ps1`.
2. Extend the Windows native packaging job after `build-windows.sh`; consume
   its **staged runtime closure** through a reviewed inventory/export rather
   than extracting an untrusted ZIP or maintaining a second payload list.
3. The PowerShell entrypoint validates version/revision/architecture, verifies
   tool and payload hashes, creates generated defines only under `target/`,
   runs `ISCC.exe`, verifies PE metadata/name, computes SHA-256, and composes
   the P005 manifest entry.
4. Keep `Synveil.iss`, UI text, GUID AppId and lifecycle hooks reviewed;
   generate only version/provenance, normalized inventory, output and manifest.
5. Set output to `target/windows-installer/SynveilSetup.exe`; inject Cargo
   version and source revision, never infer either from a mutable environment.
6. Add the smallest native smoke: compile, install silently as current user
   with all opt-ins false, assert location/registration/payload, execute bounded
   installed binary probes, uninstall silently, assert package removal and
   preserved fixture state. Full UI/startup/upgrade/repair acceptance remains
   P023–P028.

## Risks and deferred decisions

Inno repair is an explicitly tested same-version reinstall, not Windows
Installer's component-level repair. Pascal hooks can become an unsafe second
policy engine, so they remain thin and lifecycle decisions stay in the common
contract/adapter. File-in-use and transaction interruption need native fault
tests. Reproducible EXE bytes are a gate to investigate, not assumed from
deterministic inputs. Accessibility needs native inspection in P023.

Deferred: ARM64, a per-machine mode, auto-update, CA/timestamp provider,
destructive purge, server provisioning, broader Windows versions and any
installer localization. None may silently change the locked per-user ownership
or Task Scheduler boundaries.
