# ADR-038: Secure local desktop process-control IPC

Status / Trạng thái: Accepted / Chấp thuận — LOCKED

Date / Ngày: 2026-09-13

Decision owners / Chủ sở hữu quyết định: Synveil desktop/runtime maintainers

## Context / Bối cảnh

The production `synveil-client` process from Prompt 95 owns one
`DesktopSyncHost`, one `SyncRuntime`, the existing SQLite state, credential
boundary, root-availability state, and graceful lifecycle. A future native UI,
tray, controller, or diagnostics tool needs a local control boundary without
opening SQLite, reading credentials, constructing another host/runtime, or
calling the synchronization engine directly.

Tiến trình `synveil-client` production của Prompt 95 sở hữu một
`DesktopSyncHost`, một `SyncRuntime`, state SQLite hiện có, boundary credential,
state availability của root và lifecycle graceful. UI native, tray, controller
hoặc công cụ chẩn đoán trong tương lai cần một boundary điều khiển cục bộ mà
không mở SQLite, đọc credential, tạo host/runtime thứ hai hoặc gọi trực tiếp
sync engine.

## Decision / Quyết định

1. The control plane is machine-local and transport-neutral at the protocol
   layer. Linux uses one Unix-domain socket and Windows uses one named pipe.
   Endpoint identity is deterministic and profile-scoped: Linux derives
   `<canonical-runtime-dir>/synveil/<opaque-profile-id>.sock`; Windows derives
   a profile-scoped `\\.\pipe\synveil-<opaque-profile-id>` name in the
   Windows transport branch. No credential, token, raw root path, server URL,
   or localhost TCP fallback is permitted.
2. One control server belongs to one running `synveil-client`. Binding is part
   of production bootstrap and a secure bind failure is a typed fatal process
   bootstrap error. The server is stopped and joined before the existing
   Prompt 95 host shutdown proceeds. No second daemon, public HTTP route,
   WebSocket/SSE service, GUI, tray, service, autostart, or CLI is introduced.
3. Linux validates the canonical runtime directory and creates a profile
   control directory owned by the current user with mode `0700`; the socket is
   owned by the current user with mode `0600`. Accepted peers must have the
   current UID from Unix socket peer credentials. A stale entry is removed only
   at the exact generated path after ownership, socket type, non-symlink, and
   inactive-listener checks. Symlinks, regular files, directories, devices,
   wrong owners, and active listeners are refused without replacement.
4. Windows creates the real Tokio named-pipe branch with `reject_remote_clients`,
   a bounded instance count, and an explicit protected security descriptor
   granting full control only to the object owner (`OW`). An Everyone or
   Anonymous fallback is forbidden; inability to construct the descriptor is a
   typed bootstrap failure.
5. Protocol version `1` uses a `ClientHello`/`ServerHello` handshake and
   big-endian four-byte length-prefixed JSON frames. The encoded payload is
   bounded to `64 KiB`, request IDs are echoed, malformed input is isolated to
   its connection, and no arbitrary `read_to_end` is used. At most 32 active
   connection tasks are admitted; requests on one connection are sequential.
6. Gen-1 commands are `Ping`, `GetProcessStatus`, `ListLibraries`,
   `GetLibraryStatus`, `SyncNow`, `Shutdown`, and `SubscribeEvents`. Status
   responses contain only safe process/library categories, stable library IDs,
   relative scheduling data, and root/auth/conflict categories that existing
   components can prove. They contain no raw roots, content, URLs, cookies,
   authorization headers, credentials, or secret-store identifiers.
7. `SyncNow` calls the existing `DesktopSyncHostHandle::sync_now` scheduling
   surface, which routes through Prompt 92/93 to Prompt 91. A successful reply
   means queued/coalesced/follow-up scheduling, never completed synchronization.
   `Shutdown` requests the outer Prompt 95 lifecycle. The handler writes
   `ShutdownAccepted` before waking that lifecycle and never exits, aborts, or
   kills the process itself.
8. Events are an ephemeral bounded broadcast invalidation stream translated
   from existing runtime/lifecycle/root signals. `ControlServerStopping`, safe
   library/root/cycle notifications, and `Lagged { dropped_count }` are
   supported. Event loss requires status re-fetch and never affects sync
   correctness. There is no IPC journal, session, token, request, or migration.

Quyết định tương ứng: control plane chỉ chạy trên máy cục bộ; Linux dùng Unix
domain socket, Windows dùng named pipe; OS authorization giới hạn cùng user;
protocol v1 có frame bounded; status chỉ chứa dữ liệu an toàn; `SyncNow` đi qua
Prompt 93/92; `Shutdown` đi qua lifecycle graceful Prompt 95; event là
best-effort/bounded; không có durable IPC state, credential mutation, SQL,
SQLite trực tiếp, sync-engine trực tiếp hoặc fallback TCP. Chính sách socket
stale, symlink và endpoint bất thường là từ chối an toàn.

## Consequences / Hệ quả

The reusable `synveil-client` control client can reconnect after a UI or
process restart without durable IPC recovery. A stalled peer consumes only a
bounded per-connection slot and can be terminated during server shutdown; it
cannot block the synchronization supervisor. The production process does not
report `Running` until the secure endpoint is bound and its server task starts.

Client control có thể kết nối lại sau khi UI hoặc process restart mà không cần
khôi phục IPC durable. Peer bị stall chỉ chiếm một slot connection bounded và
có thể bị kết thúc khi server shutdown; nó không chặn supervisor đồng bộ.
Process production chỉ báo `Running` sau khi endpoint secure đã bind và task
server đã start.

The endpoint contract is currently Linux/Windows-specific. Native Windows
execution and full PostgreSQL-backed live IPC scenarios remain separate
validation gates; Linux disposable-runtime tests validate the common protocol
and UDS behavior. No readiness marker is emitted unless all repository,
cross-target, live-regression, and dependency/web/deployment gates pass.

Endpoint hiện tại dành riêng cho Linux/Windows. Native Windows execution và
scenario IPC live đầy đủ với PostgreSQL vẫn là gate validation riêng; test
Linux dùng runtime disposable kiểm tra protocol chung và UDS. Không phát
readiness marker nếu toàn bộ gate repository, cross-target, live regression,
dependency/web/deployment chưa pass.

## Alternatives rejected / Phương án bị từ chối

- Localhost TCP, HTTP, WebSocket, SSE, or a public daemon: different trust and
  exposure model; explicitly out of scope.
- A filesystem secret, PID sentinel, or durable IPC table: adds state and
  recovery obligations without improving same-user OS authorization.
- Direct SQLite or Prompt 91 engine access: duplicates ownership and bypasses
  Prompt 92/93 scheduling and Prompt 95 lifecycle fences.
- World-readable ACLs, Everyone/Anonymous Windows permissions, or an insecure
  fallback when native security setup fails: unacceptable security downgrade.

## Review trigger / Điều kiện xem xét lại

Supersede this ADR before adding a native controller, credential UX, durable
event replay, multi-user authorization, a third transport, or a public service
boundary. Any change to the endpoint ownership, same-user authorization,
framing limit, command semantics, or shutdown ordering requires a new locked
decision and synchronized EN/VI protocol documentation.
