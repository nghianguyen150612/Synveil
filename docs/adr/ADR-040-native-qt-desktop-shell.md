# ADR-040: Native Qt 6/QML desktop shell and system tray / Shell desktop native Qt 6/QML và system tray

Status / Trạng thái

Accepted / Chấp thuận — LOCKED Prompt 98

Date / Ngày

2026-09-15

Decision owners / Chủ sở hữu quyết định

Synveil maintainers / Nhóm maintainers Synveil

## Context / Bối cảnh

Prompts 91–97 provide the bounded synchronization cycle, long-running runtime,
production `synveil-client` process, secure local control IPC, and the
controller/presentation API. Prompt 98 is the first user-visible desktop
application. It needs one shared Linux/Windows native shell while preserving
the separation between the sync process and the UI process.

The shell must remain useful when `synveil-client` is absent or restarting, but
it must not create a second sync runtime, open the sync SQLite database, probe
library roots, access credentials, or implement a second IPC client. A GUI
close or crash must not stop synchronization. Prompt 96 exposes an explicit
process `Shutdown` command, but a normal GUI/tray quit must not send it.

## Decision / Quyết định

1. Add one dedicated `synveil-desktop` binary in `crates/desktop`. It is a
   separate process from `synveil-client`, which remains the background/foreground
   synchronization process. The UI stack is Qt 6 with QML/Qt Quick, bridged
   through one CXX-Qt integration. Qt 6.4 is the minimum supported Qt baseline;
   Windows CI pins Qt 6.8.3 and Linux CI must provision a Qt 6.4-or-newer
   distribution package. QML resources are embedded in the binary;
   source-tree-relative QML paths are not a production dependency. The shared
   native code supports Linux and Windows; macOS remains deferred.

2. The Rust `DesktopUiBridge` creates exactly one Prompt 97
   `DesktopController` for the UI process and one controller-owned async
   runtime. The main window and system tray consume the same bridge,
   controller, and latest presentation snapshot. QML receives only bounded,
   redacted values: connection/process state, stable library IDs and labels,
   root/auth/conflict/runtime categories, safe scheduling feedback, and bounded
   counts. Raw roots, URLs, credentials, cookies, authorization headers, file
   content, SQLite state, transport frames, and raw diagnostics never become
   QML properties.

3. The UI depends on the accepted controller/presentation boundary. It does
   not directly own or link to `SyncRuntime`, `DesktopSyncHost`, inbound or
   outbound sync engines, filesystem observers, local sync storage, PostgreSQL,
   server metadata adapters, or Prompt 96 raw transport. QML does not open a
   Unix socket or Windows named pipe and does not serialize protocol frames.
   Controller reconnect and generation fencing remain authoritative.

4. The initial shell contains a process/connection summary, a stable-ID library
   list with a bounded per-library `Sync Now` action for eligible rows, a
   selected-library detail view, root availability, authentication, conflict
   and runtime/scheduling status, and a manual `Sync Now` action.
   Presentation mapping uses one coherent snapshot at a time, retains a safe
   last-known list as explicitly `Stale`, clears a removed selection, bounds
   the presented rows, and never turns an accepted scheduling result into a
   synchronization-complete claim. Generic English strings are i18n-ready;
   credentials, paths, and secrets are never used as labels.

5. The native tray exposes exactly `Open Synveil`, `Sync Now`, and
   `Quit Synveil Desktop` for Gen 1. Tray status uses the same safe presentation
   state. `Sync Now` calls only `DesktopController::sync_now`; admission is
   bounded and rapid clicks coalesce or return safe feedback. Tray quit and
   window close stop/join only UI-owned controller work. They never request
   Prompt 96 `Shutdown` and never stop `synveil-client` or its synchronization
   runtime. When a tray is available, closing the window hides it; when no tray
   is available, the UI exits cleanly.

6. All controller I/O, reconnect work, and command waits remain off the Qt GUI
   thread. QML-visible state is applied through the CXX-Qt/Qt thread boundary.
   Latest-state delivery is coalesced rather than an unbounded callback/task
   queue. The focused suite covers UI1–UI40, 10,000 rapid snapshot updates,
   library churn, a 1,000-library model, bounded Sync Now clicks, stale/fresh
   reconnect behavior, selection safety, tray policy, and GUI/controller
   threading.

7. Linux CI provisions Qt 6 and runs Rust formatting, tests, Clippy, the native
   build, Qt 6 QML lint, and an offscreen complete-tree smoke load. Windows CI
   installs a native Qt 6 MSVC toolchain and hard-fails the native desktop test
   and build. PostgreSQL is used only by the existing live process/controller
   Sync Now acceptance target; the QML shell itself has no database path.
   No server route, OpenAPI operation, web client, migration, sync behavior,
   service manager, autostart, installer, or background-process launch policy
   is added by this ADR. Prompt 99 or later may address launch/packaging.

## Consequences / Hệ quả

The first desktop UI has a small, auditable boundary and can be restarted
independently of synchronization. A missing process produces a safe
disconnected/reconnecting presentation instead of a local fallback runtime.
The same UI code and controller contract are compiled on Linux and Windows,
while native tray and endpoint details remain below the presentation layer.

The shell intentionally cannot configure roots, log in, browse files, upload,
share, manage settings, or expose sync internals. Those are separate product
and security decisions. Native Windows runtime evidence still depends on the
Windows CI environment; a Linux pass does not claim it.

## Alternatives / Phương án khác

- Turn `synveil-client` into the GUI process: rejected because it would couple
  Qt lifecycle and synchronization lifecycle and make GUI exit unsafe.
- Use a web wrapper or a second UI framework: rejected because Prompt 98
  requires native Qt 6/QML and one shared Linux/Windows UI codebase.
- Let QML call Prompt 96 IPC directly: rejected because it duplicates protocol,
  reconnect, redaction, and generation-fencing policy outside Prompt 97.
- Spawn or autostart `synveil-client` from the shell: deferred to Prompt 99 or
  later so Prompt 98 has no hidden process-launch policy.
- Let tray Quit send Prompt 96 `Shutdown`: rejected because a UI lifecycle
  action must not terminate synchronization.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

No database or wire migration is required. The shell consumes the existing
Prompt 97 controller contract. A future change that adds process launching,
autostart, packaging, login/settings, raw diagnostic disclosure, a new IPC
surface, or a different UI framework requires a new or superseding ADR. A
change that alters the controller snapshot, command semantics, or tray quit
independence must update this ADR and the synchronized English/Vietnamese
architecture, domain-model, sync, and testing documentation before release.
