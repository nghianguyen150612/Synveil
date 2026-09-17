# Desktop launch, user autostart, and runtime packaging (Prompt 99)

This document describes the production launch boundary added after the native
Qt shell in Prompt 98. It is an operational companion to
[`ADR-041`](adr/ADR-041-production-desktop-launch-orchestration.md) and
[`DESKTOP_CONTROL.md`](DESKTOP_CONTROL.md).

## Runtime topology

Synveil Desktop is two processes with deliberately different lifecycles:

```text
synveil-desktop
  Qt 6/QML, tray, bounded presentation, launch request
        -> BackgroundClientManager + DesktopController
           -> Prompt 96 local control IPC
              -> synveil-client
                 DesktopSyncHost + SyncRuntime + writer lock
```

The GUI may ask the manager to make the client available. It never becomes the
sync-runtime owner, opens client SQLite, probes library roots, reads
credentials, constructs IPC frames, or implements sync retry/correctness. The
client remains independently runnable and independently supervised.

## Startup behavior

After Qt has loaded a valid profile, the bridge constructs one controller and
one profile-scoped `BackgroundClientManager`. The controller starts its normal
Prompt 97 handshake/reconnect work. The manager then performs one bounded
availability inspection and may request one start only for an absent/stopped
client or an inactive user supervisor. The controller continues its own
reconnect flow and publishes a fresh coherent snapshot when the client is
available.

The GUI thread does not wait for a process, systemd, Task Scheduler, socket,
or pipe operation. Launch requests are coalesced by a shared Tokio gate. A
concurrent request returns `AlreadyStarting`; successful or failed attempts
also use a bounded cooldown. This prevents controller reconnects, multiple
windows, or rapid user actions from creating a process-spawn storm.

The safe result vocabulary is:

| Result | Meaning | Automatic replacement launch? |
|---|---|---:|
| `AlreadyRunning` | Existing client endpoint is healthy | No |
| `StartRequested` | A supervisor accepted the start request | No further request |
| `StartedSupervised` | Supervisor started the client | No further request |
| `StartedDirect` | Detached packaged-sibling fallback started it | No further request |
| `AlreadyStarting` | Another bounded attempt is in flight/cooldown | No |
| `NotInstalled` | Packaged executable/unit/task is unavailable | No |
| `SupervisorUnavailable` | Supervisor cannot be used | At most the one direct fallback |
| `LaunchDenied` | Permission or terminal launch policy rejected it | No |
| `UnsafeState` | Endpoint/writer state is unsafe | No |
| `Failed` | Bounded attempt failed or timed out | Cooldown applies |

Security, protocol-incompatible, malformed-control, active-writer, and known
terminal controller states are fail-closed. They do not cause replacement
launches. Messages exposed to QML are generic labels; raw stderr, command
output, paths, PIDs, credentials, URLs, and tokens are never UI state.

## GUI and client independence

`DesktopController::stop()` closes and joins only controller-owned IPC work.
Tray Quit and no-tray window close do not send Prompt 96 `Shutdown`, do not
call the manager's supervised stop operation, and do not terminate
`synveil-client`. Therefore:

- closing the GUI leaves an already-running client running;
- reopening the GUI reuses the existing endpoint without an extra launch;
- a supervised client can recover while the GUI is closed; and
- a client crash makes the controller stale/reconnecting and then allows a new
  generation to become fresh after supervisor recovery.

The Prompt 95 same-profile writer lock remains the final authority if two
desktop processes race. Launch orchestration does not replace or bypass it.

## Linux user service and autostart

The package installs the client unit at
`/usr/lib/systemd/user/synveil-client.service`. It is not installed at
`/usr/lib/systemd/system/synveil-client.service` and is never a root/system
Synveil service.

The authoritative unit uses the existing client entrypoint and configuration:

```ini
[Service]
Type=simple
ExecStart=/usr/bin/synveil-client
Restart=on-failure
RestartSec=30s
RestartPreventExitStatus=78
```

The unit also has `StartLimitIntervalSec=5min` and `StartLimitBurst=5`.
Exit 78 is the current source-defined permanent configuration failure; exit 70
is the generic bootstrap/runtime failure. Network-online ordering is not
added because Prompt 92 owns network availability and recovery.

Persistent login autostart is an explicit user action:

```sh
systemctl --user daemon-reload
systemctl --user enable synveil-client.service
systemctl --user start synveil-client.service
systemctl --user status synveil-client.service
systemctl --user disable synveil-client.service
systemctl --user stop synveil-client.service
```

The package-neutral installer, DEB/RPM hooks, and GUI do not enable the unit
silently. An explicit disable is respected. Tests use temporary linked units
and disposable names; they do not enable the real Synveil unit in a developer
session.

If the user manager is unavailable, the native backend can use the canonical
packaged sibling as a detached direct fallback. That fallback keeps the client
independent of the GUI but cannot provide supervisor crash recovery; the
packaged production path prefers the user service whenever it is available.

## Windows user task

Windows uses a per-user Task Scheduler definition. It does not install a
Windows Service, require administrator rights, use `SYSTEM`, or store a
password. The generated task has:

- the current user and `InteractiveToken` logon semantics;
- `LeastPrivilege` run level;
- a stable profile-scoped name under `\Synveil\BackgroundClient\`;
- the exact canonical sibling `synveil-client.exe` action;
- `IgnoreNew` multiple-instance policy; and
- finite `PT30S` restart interval and count 5.

Registration, query, run, and removal call the fixed Windows
`System32\schtasks.exe` with explicit argv. No shell command or interpolated
PowerShell/cmd string is constructed. The task name contains no token,
credential, root path, or server URL. The structured
`WindowsTaskDefinition` is tested on Linux; native registration and native
task recovery must still be exercised on a Windows runner.

## Executable resolution and security

The manager canonicalizes the current packaged desktop executable and accepts
only the expected product name and its sibling client. It never accepts an
executable path from QML, IPC, the server, a library manifest, or a mutable
`PATH` lookup. Direct fallback uses null standard I/O and platform detachment
flags. Supervisor requests use fixed unit/task identities and existing process
configuration semantics without invented command-line flags.

Autostart metadata is process-management state only. It never writes SQLite,
checkpoints, handoff records, sync intents, root state, or credentials. No
credential, session cookie, authorization header, raw root path, file content,
or secret-store value is present in the unit, task identity, package metadata,
or QML projection.

## Linux package contents

`deploy/install/MANIFEST` is the one source of truth for DEB and RPM staging.
The Prompt 99 package payload contains:

| Payload | Installed path |
|---|---|
| Client | `/usr/bin/synveil-client` |
| Desktop shell | `/usr/bin/synveil-desktop` |
| User service | `/usr/lib/systemd/user/synveil-client.service` |
| Application entry | `/usr/share/applications/synveil.desktop` |
| Application icon | `/usr/share/icons/hicolor/scalable/apps/synveil.svg` |
| Existing maintenance runtime | `/usr/bin/synveil-scheduled-maintenance-once` |
| Existing maintenance units/fragments/template | Existing Prompt 75/79 paths |
| License and notice | `/usr/share/doc/synveil/LICENSE` and `NOTICE` |

The desktop entry has `Name=Synveil`, `Exec=/usr/bin/synveil-desktop`,
`Type=Application`, categories, `Icon=synveil`, and `Terminal=false`. It is
ordinary application metadata and has no XDG/login-autostart semantics.

Both native archives are built from the staged manifest. Artifact checks verify
the exact payload tree, executable modes, source byte parity, unit/metadata
parity, no secret files, and no repository, `/tmp`, build, or development
paths. Package signing and repository publication remain release-engineering
work.

## Windows portable package

`deploy/packages/build-windows.sh` produces an unsigned reproducible ZIP under
the ignored package output directory. It contains the two production EXEs,
`qt.conf`, the Windows Qt runtime DLLs, `platforms/qwindows.dll`, the required
Qt QML modules/plugins, C++ runtime DLLs when supplied by the genuine target
Qt prefix, `LICENSE`, and `NOTICE`.

On Windows, `windeployqt` is the authoritative closure tool with compiler
runtime and embedded-QML analysis. On Linux, cross packaging requires an
explicit real Windows Qt prefix and copies only the named runtime/QML closure;
it rejects headers, import libraries, static archives, Linux libraries, and
developer paths. `llvm-readobj`/`dumpbin` import auditing requires every
non-system PE dependency to be present in the ZIP. The ZIP is portable and
package-relative; it does not rely on the repository, Cargo target, `/tmp`, or
a developer Qt installation at runtime. It is not an installer.

## Validation and limitations

Repository-owned focused gates cover the launch manager, coalescing and
1,000-request boundedness, canonical path policy, Linux unit policy, the
Linux user-unit source/lifecycle test, package manifest, DEB/RPM artifact
contents, Windows task definition, and Windows ZIP policy. The relevant
commands are:

```text
cargo test -p synveil-client --lib --locked -- --nocapture
cargo test -p synveil-metadata --test linux_desktop_launch_units --locked -- --nocapture
cargo test -p synveil-metadata --test windows_desktop_packaging_units --locked -- --nocapture
```

The Linux user-manager test links a disposable unit, starts/inspects/stops it,
and cleans it afterward.

Native Windows Task Scheduler registration, Windows portable-package startup,
and native Windows tray interaction are runtime gates for a Windows runner.
A Linux or cross-build result must not be described as native Windows
execution. PostgreSQL is not required by the launch/package boundary; ignored
PostgreSQL integration tests remain unverified when
`SYNVEIL_TEST_DATABASE_URL` is unset.

Prompt 99 changes no server migration, client schema, HTTP route, OpenAPI
operation, or web feature. The next phase is Prompt 100 checkpointing and
release-gate ownership, not a second launch implementation.
