# ADR-046: Essential desktop settings and user sync controls

Status: Accepted — LOCKED Prompt 105
Date: 2026-09-19
Owners: Synveil client-sync, client, desktop, and packaging maintainers

## Context

The native desktop shell needs three operational settings that users can
understand and recover from without exposing synchronization internals:

1. a durable global Pause Sync / Resume Sync control;
2. a user-login setting for starting the existing background client; and
3. a close-to-tray preference for the desktop window.

These settings cross different ownership boundaries. Synchronization
correctness belongs to the one `synveil-client` runtime, login startup belongs
to the existing `BackgroundClientManager`, and close-to-tray is local window
behavior. A Qt property or a second scheduler cannot be the source of truth
for all three.

## Decision

### Global pause is client-owned and durable

`synveil-client` owns the global user pause state. It stores one bounded,
non-secret value, `paused` or `running`, beside the existing profile-bound
`client.conf` manifest in the platform configuration directory. The value is
written to a temporary file, flushed, atomically renamed, and followed by a
best-effort parent-directory sync on Unix. A missing file means `running`;
unknown content fails closed during client bootstrap.

The client loads the value before starting its existing `DesktopSyncHost` and
`SyncRuntime`. The runtime exposes `Running` and `PausedByUser` as a separate
process-wide reason; it is not an alias for authentication, network, root,
conflict, or runtime-fault state. No SQLite migration or server record is
created for this preference.

While paused, the existing runtime supervisor does not start periodic cycles,
manual `SyncNow` cycles, inbound/network wake cycles, credential wake cycles,
or filesystem durable-change wake cycles. A bounded cycle already in flight is
allowed to finish. Durable local observations and other control-plane
operations remain usable. Non-manual wake intent is retained as one merged
in-memory pending reason where the existing scheduler needs it; a manual
request is declined with a typed `Paused` result and never becomes a hidden
resume.

The local IPC v1 extension is deliberately small:

```text
GetSyncControlState
PauseSync
ResumeSync
```

The server writes the pause file first and only then signals the runtime. A
failed write returns `PersistenceFailure` and leaves runtime state unchanged.
Resume persists `running`, clears only the user-pause reason, wakes the one
existing scheduler, and does not perform a broad rescan or invent a second
work queue. IPC responses and events contain only the finite state/result
categories. The controller treats a lost response as `OutcomeUnknown` and
refreshes state rather than replaying the mutation.

### Login startup remains process management

The desktop calls the existing `BackgroundClientManager` to read or change
user-login startup. Linux uses the fixed `systemd --user` unit and Windows
uses the existing per-user Task Scheduler definition. Commands continue to use
fixed executable paths and typed argument vectors; no shell interpolation,
server URL, root path, credential, or token enters the supervisor definition.

Changing login startup changes only future login behavior. Disabling it does
not stop a currently running client. Enabling it does not spawn a duplicate;
the existing launch manager gate, supervisor identity, and client writer lock
remain authoritative. Rapid UI changes are coalesced to the latest requested
value behind one in-flight bounded supervisor operation. A failed or unknown
operation is followed by a status refresh, not an unbounded replay loop.

### Close-to-tray is desktop-local

The Qt shell stores `close_to_tray` through `QSettings` using the application
metadata already established by the native application. It is not written to
the `SecretStore`, client manifest, SQLite state, server, or IPC snapshot.
When enabled and a real system tray is available, closing the window hides the
window. Otherwise closing requests the existing controller-only shell exit
path. In both cases the background client remains independent and is not
implicitly shut down.

### Safe presentation

The controller snapshot includes only the sync-control category and freshness
information. The bridge exposes generic labels, bounded busy/feedback state,
and the three settings actions as QML properties/invokables. It never exposes
the pause file path, supervisor command, IPC endpoint, process ID, raw OS
diagnostic, root path, server URL beyond the already approved profile
onboarding surface, or any credential material.

## Consequences

- Pause/resume survives desktop-shell and background-client restart because the
  client owns the durable value.
- Auth, sign-out, profile configuration, library setup, root availability, and
  connection status remain usable while synchronization is paused.
- The existing single runtime/scheduler remains the only synchronization
  executor; no broad rescan, force-sync bypass, or second worker is introduced.
- Login startup and close-to-tray can be changed independently of global sync
  pause and independently of one another.
- A response timeout can leave the UI temporarily uncertain, but it cannot
  justify a blind replay. The controller refreshes canonical state and shows
  generic `OutcomeUnknown`/unavailable feedback.
- Settings validation is split into source, focused unit/integration, native
  desktop build, and live OS evidence. Linux-local work does not claim native
  Windows Task Scheduler or tray execution.

## Alternatives rejected

- A QML-only pause bit: it would be lost on shell restart and could not stop
  the background process's periodic or inbound work.
- A server-side or per-library pause record: it would broaden scope and could
  not represent the local process-wide control without changing sync/API
  contracts.
- Killing or suspending the client process: it would break durable shutdown,
  auth/profile usability, and bounded in-flight operation semantics.
- `SyncNow` as an implicit resume: it would make the user-visible pause
  control unreliable and create a scheduling bypass.
- Shell-based login commands or a system/root service: they would broaden
  privilege and injection risk beyond the existing typed launch manager.
- Storing close-to-tray in client sync state: window behavior is a shell-local
  preference and does not belong in synchronization correctness data.

## Migration and review trigger

This decision adds no migration. The server migration count remains **36**;
the client SQLite schema remains **V7** with **7 client migrations**. The
pause file is a small process setting, not a SQLite schema version.

Review this ADR if synchronization pause must be remotely administered,
scoped per library/profile, synchronized across devices, or exposed to a web
client; any of those would require a new server/API and durable-data decision.

## Quyết định / Vietnamese summary

Desktop Prompt 105 có ba setting nhưng ba ownership khác nhau. `synveil-client`
và `SyncRuntime` sở hữu global Pause/Resume; `BackgroundClientManager` sở hữu
login startup theo user; còn close-to-tray là hành vi cục bộ của Qt shell.

Pause được lưu bằng một file non-secret rất nhỏ cạnh `client.conf`, chỉ nhận
`paused` hoặc `running`, ghi temporary rồi flush/rename atomic. File mất nghĩa
là `running`; nội dung sai khiến bootstrap fail closed. Runtime dùng reason
`PausedByUser` riêng, không trộn với auth/network/root/conflict/fault.

Khi paused, scheduler hiện có không chạy periodic, inbound/network,
filesystem wake, credential wake hay `SyncNow`; operation bounded đang chạy
được hoàn tất an toàn. Durable observation và các control-plane khác vẫn hoạt
động. Wake non-manual được merge bounded khi cần; `SyncNow` trả typed
`Paused`, không tự resume. Server IPC ghi durable state trước rồi mới signal
runtime; write fail trả `PersistenceFailure` và không đổi runtime. Resume ghi
`running`, chỉ xóa lý do user-pause, đánh thức scheduler hiện có và không broad
rescan.

IPC thêm `GetSyncControlState`, `PauseSync`, `ResumeSync` cùng result/event
category bounded. Mất response là `OutcomeUnknown` rồi refresh authoritative,
không replay mutation. Login startup dùng systemd `--user` Linux hoặc Task
Scheduler per-user Windows qua API launch hiện có; disable không kill client
đang chạy, enable không spawn duplicate, rapid clicks chỉ giữ latest intent.

Close-to-tray dùng `QSettings`, không dùng SecretStore/server/SQLite/client
manifest. Chỉ hide khi setting bật và tray thật sự có; nếu không thì dùng
controller-only shell exit, luôn giữ client độc lập.

Không thêm migration: server **36**, client schema **V7**, tổng **7 client
migrations**. Native Windows, live user-supervisor và GUI runtime phải được
báo cáo riêng, không suy diễn từ Linux source hoặc cross-build.
