# Windows repair, upgrade, uninstall, and purge lifecycle

Status: **SOURCE IMPLEMENTED / STATIC VERIFIED; HOSTED NATIVE VERIFIED pending**.
This is the Prompt027 contract. Prompt028 remains the final native acceptance
checkpoint.

## Identity and installed-state snapshot

Setup retains the stable AppId
`{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}`, per-user install root
`%LOCALAPPDATA%\Programs\Synveil`, and Cargo workspace version authority. Before
mutation it reads the exact HKCU uninstall identity, `DisplayVersion`,
`InstallLocation`, the prior closed runtime manifest, shortcut presence, and
explicit command choices. The client authority separately inspects the durable
startup preference, existing profile, and profile-scoped scheduled task. Secret
values are never part of this snapshot.

The package owns only manifest-listed runtime files, `SYNVEIL-MANIFEST.txt`,
Inno support/uninstaller files, its exact current-user shortcuts, and its HKCU
uninstall record. Startup tasks are platform-integration-owned by
`synveil-client`; application configuration, credentials, client sync state,
libraries, and every server-data class are outside Setup ownership.

## Repair and upgrade

**SOURCE IMPLEMENTED.** `/REPAIR=1` is the only accepted repair value. Repair
requires an existing matching registration, canonical install location,
strictly parseable and exactly matching stable version, and trusted prior
manifest. Silent same-version execution without the switch fails. An
interactive same-version rerun is treated as repair. Repair copies the exact
self-contained Setup payload, restores shortcuts and damaged/missing files,
and preserves the observed desktop choice and startup preference unless a new
explicit option is supplied. It is not uninstall/reinstall and performs no
network access.

A newer strict stable version with the same identity is an upgrade. A newer
installed version is rejected before files are selected or mutated; unknown,
pre-release, malformed, and incompatible versions fail closed. Locked files
use Inno's bounded CloseApplications behavior. No reboot replacement,
system-wide process-name kill, `schtasks /End`, or binary rollback is promised.
Failure leaves durable application state untouched and requires inspection or
safe target repair.

### Test-only upgrade fixture

**SOURCE IMPLEMENTED.** CI alone may pass the closed numeric fixture versions
`1.0.0` and `1.1.0` to the build script. Fixture mode never edits Cargo.toml,
creates a tag, calls itself v0.2, or writes a production release manifest. CI
builds two real Setup executables from disposable copies of the runtime closure;
the older copy owns one obsolete fixture file. The native test executes both
Setups under P025's disposable standard user. This tests Setup mechanics and is
not a claim that historical v0.1 shipped this installer.

## Obsolete files and adjacent data

**SOURCE IMPLEMENTED.** Before replacement, Setup validates and retains the
prior manifest in memory. After installing the target closed manifest it
removes only the old-minus-new difference. Every candidate must be a safe
relative identity, normalize beneath the package root, and be a regular
non-reparse-point object. Missing known-owned files are no-ops. Ambiguous paths,
malformed evidence, traversal, symlinks, junctions, and reparse points stop
cleanup without broadening deletion. Setup never scans for or deletes generic
"extra" files. Thus `{app}\user-note.txt` survives repair, upgrade, and ordinary
uninstall; the otherwise-empty package directory may remain to contain it.

## Startup and shortcuts

**SOURCE IMPLEMENTED.** Repair/upgrade without `/STARTUP` preserves the durable
client preference. Disabled remains disabled and has no authoritative task;
enabled reconciliation remains with the profile-scoped P026 client authority.
An explicit newest choice wins. Desktop-shortcut presence is the authoritative
preserved choice unless `/DESKTOPICON=0|1` is explicitly supplied. Repeated
operations use one HKCU registration, one Start Menu link, at most one desktop
link, and one task per enabled profile.

Immediately before uninstaller file removal, Inno invokes the installed,
verified `synveil-client.exe --cleanup-startup-integration` boundary and waits.
The client removes only its authoritative current-profile task and verifies the
result. Failure aborts before removing the binary; absence is a safe no-op. The
startup preference file is APPLICATION_CONFIG and remains preserved.

## Ordinary uninstall and reinstall

**SOURCE IMPLEMENTED.** Tests discover `QuietUninstallString` or
`UninstallString` from the exact HKCU registration, validate that its executable
is beneath the package root, invoke it silently with `/NORESTART`, and then
verify resulting state. Ordinary uninstall removes package-owned files,
installer shortcuts/registration, and client-owned active startup integration.
It contains no AppData tree deletion and preserves application config,
credential presence, client database/sync state, user libraries, server config,
server database/object data, and external dependencies. A real Setup reinstall
then validates the preserved-state and clean-registration behavior.

## Separate destructive boundary

**STATIC VERIFIED.** Purge is not an installer intent or uninstall option. The
generic model can authorize only APPLICATION_CONFIG, and only after a separate
explicit destructive action and confirmation with an exact contained,
non-reparse target. Missing/ambiguous roots cannot expand scope. Credentials
remain SecretStore-owned; client state remains client-owned. User libraries,
server configuration/database/object data, external dependencies, and native
package databases cannot be represented as generic purge targets. In
particular, Setup never follows a configured library root or removes its
contents. Prompt027 intentionally adds no ordinary-uninstaller data checkbox or
generic filesystem purge executable.

## Verification and evidence

**STATIC VERIFIED** covers option/version parsing, ownership difference,
containment/reparse rejection, preservation classes, bounded purge
authorization, registered-uninstaller discovery, and workflow wiring. The
native script damages package files, repairs, performs a real fixture upgrade,
rejects downgrade while comparing package/state snapshots, uninstalls while
preserving unknown and durable sentinels, and reinstalls. It emits bounded JSON
without credentials, passwords, or real user paths.

**HOSTED NATIVE VERIFIED: pending.** This checkout has no configured Git remote,
so PR #41/#42/#43 runs and a final-head Prompt027 workflow could not be queried
or started. No Windows result, workflow ID, or artifact hash is claimed.
Prompt028 retains interactive journey, installed GUI/named-pipe behavior, real
login-trigger execution, the complete native lifecycle matrix, and the final
Windows readiness checkpoint.
