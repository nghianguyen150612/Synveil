# Windows per-user installation

Status: **SOURCE VERIFIED; HOSTED STANDARD-USER VERIFIED pending**

## Trust and privilege model

Synveil v0.2 is an x64, current-user product. Setup keeps the stable AppId
`{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}`, installs to
`{localappdata}\Programs\Synveil`, and fixes Inno Setup to
`PrivilegesRequired=lowest` with privilege overrides disabled. The compiled
Setup manifest is extracted in native CI and must request `asInvoker`; source
and binary checks reject `requireAdministrator` and `highestAvailable`.

The package is intentionally writable by its owning user and is executed only
with that user's non-elevated token. This is not a privileged directory: the
security property is **user-writable code + user-level execution = no privilege
boundary crossing**. Setup must not elevate, install a helper or service, change
PATH, access HKLM ownership, or turn the package into shared executable state.

## Current-user ownership surfaces

Inno owns the following bounded surfaces:

* package: `%LOCALAPPDATA%\Programs\Synveil`;
* Start Menu: `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Synveil.lnk`;
* optional desktop shortcut: the installing user's Desktop only;
* uninstall identity: `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}_is1`;
* diagnostics: `%LOCALAPPDATA%\Synveil\installer`.

There is no corresponding HKLM uninstall ownership in either registry view,
common Start Menu entry, Public Desktop entry, Program Files/ProgramData
payload, user or machine PATH mutation, Windows service, Run key, Startup
folder entry, or product scheduled task. P026, not this installer stage, owns
the reviewed current-user startup mechanism.

## User and ACL isolation

Each Windows user resolves `{localappdata}`, `{userprograms}`, `{userdesktop}`,
HKCU, application state, and Credential Manager independently. Therefore the
same stable AppId may exist in separate HKCU hives without conflict. User B
does not inherit User A's registration, shortcuts, credentials, sync state, or
package path and may install an independent byte-identical copy.

Normal profile ACL inheritance is retained rather than rewritten. The native
test rejects package-root write grants to Everyone, Authenticated Users, or the
built-in Users group. The owner may update their package, while an unrelated
ordinary user receives no shared writable execution location. SYSTEM and
administrative OS principals may retain normal inherited access.

Setup installs code and integration only. It neither reads credentials nor
pre-creates application configuration, sync databases, libraries, or server
state. Uninstall removes package-owned files and shortcuts but preserves a
synthetic sentinel outside the package root; P027 owns the complete lifecycle
and destructive-removal contract.

## Native standard-user acceptance design

The administrative CI controller exists solely to create and delete a randomly
named disposable local user. It generates a non-logged runtime password, masks
it immediately, confirms the account is not in Administrators, and launches a
bounded PowerShell child through Windows `CreateProcessWithLogonW` semantics
(`Start-Process -Credential -LoadUserProfile`). No Task Scheduler transport or
production account-management code is used.

The child records its user token elevation state, integrity SID, and process
architecture, then runs the real Setup with `/STARTUP=0 /DESKTOPICON=0
/LAUNCH=0`. It verifies HKCU/HKLM registration boundaries, current/common
shortcuts, structured ACLs, unchanged user/machine PATH, and absence of a
Synveil service or scheduled task. It runs both installed executables from an
unrelated working directory with the P024 clean-environment probes, uninstalls
using the registered command, verifies state preservation, reinstalls with the
desktop option, and uninstalls again. The controller deletes the account and
profile in `finally` and marks cleanup in a bounded JSON evidence record.

The actual Setup PE manifest is separately extracted with the Windows SDK
`mt.exe`. The acceptance artifact contains no password, SID, or user files.
Full independent simultaneous two-user installation remains **PENDING**; the
implemented live standard-user test plus HKCU/HKLM/common-surface assertions
establish the isolation model but do not fabricate that additional result.

## Evidence status and limits

* **SOURCE VERIFIED:** per-user Inno policy, deterministic static validator,
  disposable-account harness, token/registry/filesystem/ACL/runtime/uninstall
  assertions, and compiled-manifest gate are implemented.
* **HOSTED STANDARD-USER VERIFIED:** **PENDING** a final-head Windows workflow.
  Linux cannot create a Windows logon token or execute Setup.
* **PENDING:** live two-independent-user installs, P026 startup integration,
  P027 full repair/upgrade/uninstall lifecycle, and P028 final clean Windows
  installed-product and named-pipe acceptance.
