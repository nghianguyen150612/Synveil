# ADR-041: Production desktop launch orchestration and user-level background supervision / Điều phối khởi chạy desktop production và giám sát background theo user

Status / Trạng thái

Accepted — LOCKED

Date / Ngày

2026-09-16

Decision owners / Chủ sở hữu quyết định

Synveil maintainers / Nhóm maintainers Synveil

## Context / Bối cảnh

Prompts 91–98 established the synchronization cycle, long-running runtime,
production `synveil-client` process, secure local control IPC, UI-neutral
`DesktopController`, and the native Qt 6/QML `synveil-desktop` shell. The two
processes must now be usable by a non-technical desktop user without making the
GUI the synchronization owner:

```text
synveil-desktop (Qt/QML, tray, presentation)
        -> DesktopController
           -> Prompt 96 local control IPC
              -> synveil-client (background sync runtime)
```

Starting the GUI must be able to request one safe background-client start when
the client is absent. Closing or crashing the GUI must not stop the client.
Unexpected client failure must be recoverable while the GUI is open or closed
when a user-level supervisor is enabled. Launch management is process
orchestration only; synchronization correctness remains in the existing
Prompt 91–95 layers and the Prompt 95 writer lock.

The implementation must also produce installable Linux package payloads and a
portable Windows runtime layout without introducing a root/system Synveil
daemon, a Windows Service, an installer wizard, a server route, an OpenAPI
operation, a web feature, or a new durable synchronization record.

## Decision / Quyết định

### 1. Process ownership and narrow launch boundary

Add `BackgroundClientManager` behind the public `synveil-client` crate
boundary. `synveil-desktop` may construct and call this manager, but it does
not implement `systemctl`, Task Scheduler, process writer locks, IPC frames,
SQLite access, root observation, credentials, or sync retry logic.

The manager owns only:

- background endpoint/supervisor availability classification;
- one bounded request to start the packaged background client;
- user-autostart status, enablement, disablement, and explicit run/stop
  operations;
- platform supervisor state mapping;
- canonical packaged executable resolution; and
- one profile/context-scoped in-flight gate and retry cooldown.

It does not own `DesktopController`, `DesktopSyncHost`, `SyncRuntime`,
`LocalStateStore`, library state, credentials, checkpoints, filesystem
observation, sync correctness, or sync retries. Prompt 95 writer arbitration
remains the final duplicate-process protection.

The public launch result is a finite enum:
`AlreadyRunning`, `StartRequested`, `StartedSupervised`, `StartedDirect`,
`AlreadyStarting`, `NotInstalled`, `SupervisorUnavailable`, `LaunchDenied`,
`UnsafeState`, and `Failed`. Raw command output, stderr, PIDs, paths, tokens,
URLs, and credentials are not product state.

### 2. Startup and coalescing

The desktop startup sequence is:

1. Qt starts and loads the configured profile identity.
2. The bridge constructs one `DesktopController` and one
   `BackgroundClientManager` for that profile.
3. The controller starts its normal Prompt 97 connection/reconnect work.
4. The manager performs one bounded endpoint/supervisor inspection.
5. Only endpoint absence, a stopped client, or an inactive supervisor may
   produce one start request.
6. The controller continues its own reconnect and coherent-fresh-snapshot
   flow; the Qt GUI thread never waits on process or supervisor I/O.

Cloned managers for the same profile share a Tokio mutex gate. A concurrent
`ensure_running` call returns `AlreadyStarting`; after a successful or failed
attempt, a bounded cooldown prevents a reconnect loop from issuing a launch
storm. Running, protocol-incompatible, malformed, endpoint-security,
writer-conflict, and terminal-controller states never trigger replacement
launches.

The manager's direct fallback uses the canonical sibling `synveil-client`
beside the canonical `synveil-desktop` executable, null standard I/O, and
platform detachment flags. It is a bounded fallback for environments without a
usable user supervisor; packaged production prefers the supervisor path.

### 3. Linux user-level supervision

Linux packages install the client unit at:

```text
/usr/lib/systemd/user/synveil-client.service
```

The unit is `Type=simple` and invokes the actual packaged binary with the
existing configuration contract:

```text
ExecStart=/usr/bin/synveil-client
```

It uses `Restart=on-failure`, `RestartSec=30s`, and bounded
`StartLimitIntervalSec=5min` / `StartLimitBurst=5`. The current client source
defines configuration failure as exit 78 and runtime/bootstrap failure as exit
70; the unit uses `RestartPreventExitStatus=78` so a permanent configuration
failure does not loop forever. Network readiness is not a unit dependency;
Prompt 92 owns network recovery and scheduling behavior.

User autostart is explicit `systemctl --user enable synveil-client.service`.
The package-neutral installer and package hooks never enable or start it, and
the GUI never silently re-enables it after an explicit user disable. Status,
enable, disable, run, start, and stop operations are exposed through the
typed manager/backend boundary. The unit is never installed as
`/usr/lib/systemd/system/synveil-client.service` and is never a root/system
service.

### 4. Windows per-user supervision

Windows uses a per-user Task Scheduler definition, not a Windows Service and
not an administrator-only helper. The task has:

- the current user as `UserId` and `InteractiveToken` logon semantics;
- `LeastPrivilege` run level;
- a profile-scoped, sanitized task name containing no token, credential, root,
  or server URL;
- the exact canonical sibling `synveil-client.exe` as its action;
- `IgnoreNew` multiple-instance policy; and
- a finite `PT30S` restart interval with count 5.

Registration, query, explicit run, and removal use a fixed absolute
`System32\\schtasks.exe` path and explicit argument vectors. No password,
`SYSTEM` principal, elevation, registry `Run` fallback, shell interpolation,
or user-provided executable path is used. The structured
`WindowsTaskDefinition` is available on non-Windows targets for deterministic
policy tests; native Task Scheduler execution remains a Windows-runtime gate.

### 5. Canonical path and process safety

The manager canonicalizes the current packaged desktop executable and accepts
only the exact product names `synveil-desktop` / `synveil-desktop.exe` and its
sibling `synveil-client` / `synveil-client.exe`. It does not search `PATH` when
the packaged relationship is known and does not accept a path from QML, the
server, IPC, QML metadata, or a library root. Process creation uses direct
`Command` plus explicit argv or the native Task Scheduler API; it never builds
`sh -c`, `cmd.exe /C`, PowerShell, or another interpolated shell command.

The existing writer lock, state/profile binding, and process bootstrap remain
authoritative if two desktop processes race. Launch management does not weaken
those protections or manufacture a second sync runtime.

### 6. Packaging and desktop integration

The Linux package-neutral `deploy/install/MANIFEST` remains authoritative for
both DEB and RPM. The Prompt 99 payload contains:

- `synveil-client` and `synveil-desktop`;
- the Linux user unit under `/usr/lib/systemd/user`;
- the `Synveil` desktop entry and scalable application icon;
- existing scheduled-maintenance units, sysusers/tmpfiles fragments, and
  environment template; and
- `LICENSE` and `NOTICE`.

The desktop entry is ordinary application metadata (`Type=Application`,
`Exec=/usr/bin/synveil-desktop`, `Terminal=false`, categories, and icon). It
does not implement login autostart. The actual DEB and RPM archives are built
from the same staged manifest and audited for path, mode, byte parity, secret,
and development-tree leakage.

The Windows package is an unsigned reproducible ZIP, not an installer. Native
Windows uses `windeployqt --compiler-runtime --no-translations` with the
embedded QML import directory. Cross packaging accepts only a real Windows Qt
prefix and copies the named Qt DLL/QML/platform/runtime closure, including
`platforms/qwindows.dll`, `qt.conf`, and license/notices; it never copies an
entire SDK or developer import tree. PE import auditing requires every
non-system dependency to be in the ZIP.

### 7. Scope and schema

Prompt 99 adds zero server migrations, zero client migrations, zero HTTP
routes, zero OpenAPI operations, and zero web changes. Autostart registration,
supervisor enablement, and package metadata are platform process-management
state, not synchronization domain state. No credentials, session cookies,
authorization headers, raw roots, file contents, or secret-store values enter
the launch API, unit, task identity, package metadata, or QML projection.

There is no settings UI in this ADR. The reversible autostart API is a
foundation for a future product/settings surface. No login/password UI,
installer wizard, macOS implementation, Windows Service, root daemon, kernel
driver, uploads, sharing, or sync-architecture rewrite is included.

## Consequences / Hệ quả

Launching the desktop no longer requires a user to start `synveil-client`
manually. A missing client has one safe, typed start opportunity and then the
existing controller reconnects normally. A configured user supervisor can
restart a crashed client independently of the GUI, with observable and bounded
restart policy. GUI close still stops/join only controller work and does not
send Prompt 96 `Shutdown`.

The Linux user unit and Windows task are intentionally user-scoped. They do
not provide system-wide availability before user login, and a disabled
autostart setting is respected. Native Windows task registration, portable
package startup, and native tray behavior require a Windows runtime; a Linux
or cross-build result must not be reported as native Windows execution.

Package signing, repository publication, guided installation, managed database
lifecycle, and full cross-platform release-lab support remain later release
engineering decisions.

## Alternatives / Phương án khác

- Make `synveil-desktop` own the sync runtime: rejected because GUI lifecycle
  would become synchronization lifecycle and GUI close/crash would be unsafe.
- Put the client in a root/system systemd service: rejected because Gen 1
  requires user-level ownership and no root background daemon.
- Install a Windows Service or require administrator elevation: rejected for
  the same least-privilege and per-user boundary.
- Use an XDG shell script or registry `Run` fallback: rejected because the
  native user supervisor provides explicit lifecycle, restart, and disable
  semantics without shell expansion.
- Let QML or the controller construct commands: rejected because it would
  expose process-manager details and create path/argument injection risk.
- Store autostart state in SQLite or PostgreSQL: rejected because it is
  process-management metadata, not synchronization domain state.
- Silently enable autostart whenever the GUI opens: rejected because explicit
  user disablement must remain durable in the platform supervisor.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

No database or wire migration is required. A future change that adds a new
supervisor, changes the task/unit identity, introduces a privileged helper,
changes packaged executable resolution, adds settings/login/installer behavior,
or exposes new diagnostic data must supersede or amend this locked decision.
Any change to controller reconnect, Prompt 95 writer ownership, process exit
taxonomy, package manifest authority, or GUI/client independence requires
updates to the synchronized English/Vietnamese architecture, domain-model,
sync, testing, and desktop deployment documentation before release.
