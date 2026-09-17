# ADR-039: IPC-backed desktop controller core

Status / Trạng thái: Accepted / Chấp thuận — LOCKED

Date / Ngày: 2026-09-14

Decision owners / Chủ sở hữu quyết định: Synveil desktop/runtime maintainers

## Context / Bối cảnh

Prompt 96 provides a secure, profile-scoped local control protocol owned by the
running `synveil-client` process. A future native UI or tray surface needs a
small reusable model/client boundary, but it must not open the synchronization
SQLite state, construct another host/runtime, or call Prompt 91–95 internals.
The UI also needs to survive a normal process restart without taking ownership
of synchronization correctness or durable recovery.

Prompt 96 cung cấp protocol điều khiển cục bộ an toàn, scoped theo profile và
do process `synveil-client` đang chạy sở hữu. Native UI hoặc tray trong tương
lai cần một boundary model/client nhỏ có thể tái sử dụng, nhưng không được mở
SQLite synchronization state, tạo host/runtime khác hoặc gọi internal Prompt
91–95. UI cũng cần sống qua restart process bình thường mà không sở hữu
correctness synchronization hay recovery durable.

## Decision / Quyết định

1. The Prompt 97 controller lives in the `synveil-client` package as the
   UI-agnostic `DesktopController` module. Its public surface is a typed
   `DesktopControllerSnapshot`, a latest-state `watch` subscription, and safe
   command results. It is not a GUI, tray, service, autostart, or process
   launcher.
2. A future UI communicates only through `DesktopController`. The controller
   communicates only through the Prompt 96 `DesktopControlClient` and its
   profile/platform endpoint resolver. The controller does not depend on
   `LocalStateStore`, `SyncRuntime`, `DesktopSyncHost`, synchronization
   engines, SQLite tables, PostgreSQL, credentials, or filesystem probing.
3. Construction is side-effect bounded. `DesktopController::new` stores only
   endpoint configuration and timing policy. `start` creates one manager task,
   one bounded command channel, one status/request connection, and one bounded
   event reader relationship. `stop` closes and joins controller-owned IPC
   work only; it never sends `Shutdown` implicitly and therefore cannot stop
   `synveil-client` merely because a UI exits.
4. Each connection attempt performs the Prompt 96 version-1 handshake. The
   controller establishes an event subscription and fetches one complete
   process/library status set before publishing a `Connected`/`Fresh` snapshot.
   Per-library responses are checked for canonical IDs and deterministic
   ordering. The snapshot contains only process status, stable library IDs,
   safe runtime/root/auth/conflict categories, relative scheduling data,
   revision, freshness, and a local connection generation.
5. Prompt 96 events are invalidation hints. A single atomic pending bit and
   notification fold an event burst into status refresh work. At most one
   refresh transaction is in flight; events received during that transaction
   leave at most one follow-up refresh. Event history is never mirrored or
   accumulated by the controller. The latest-state watch does not backpressure
   the IPC reader or retain an unbounded subscriber/task list.
6. A successful coherent refresh increments `revision`. When IPC is lost, the
   last snapshot remains available but is marked `Stale` while the controller
   enters `Reconnecting`; an empty initial state is `Unavailable`. Reconnect
   delays are deterministic and bounded: 250 ms, 500 ms, 1 s, 2 s, 4 s, then
   5 s. A successful connection resets the schedule.
7. Every connection attempt receives a new local generation. Responses and
   invalidation signals are applied only while their generation is current.
   A late response or event from an old connection cannot overwrite the newer
   snapshot. Protocol incompatibility, endpoint security failures, unsupported
   platforms, and malformed server responses become stable typed states rather
   than reconnect storms; an absent endpoint remains a normal bounded
   reconnect condition.
8. `SyncNow` is admitted through a channel of eight items and is sent once
   through Prompt 96. `Accepted`, `Coalesced`, and follow-up results mean
   scheduling only, never completed synchronization. Disconnected commands are
   rejected without indefinite queuing. A lost command response returns
   `OutcomeUnknown`; neither `SyncNow` nor `Shutdown` is automatically replayed.
   `RequestShutdown` returns a typed acceptance result and is the only
   controller operation that asks the process to stop.

Quyết định tương ứng: UI tương lai chỉ giao tiếp qua `DesktopController`, còn
controller chỉ giao tiếp qua `DesktopControlClient` của Prompt 96. Model là
ephemeral latest-state, snapshot đầu tiên coherent, event chỉ là invalidation
hint, refresh được coalesce, reconnect có backoff bounded, generation fence
chặn response/event cũ, command admission bounded và không replay command khi
mất response. `SyncNow` chỉ schedule; `Shutdown` chỉ thực hiện khi được gọi
tường minh. UI/controller exit không dừng `synveil-client`. Không thêm GUI,
tray, service, autostart, migration, route, OpenAPI operation hay durable
controller cache.

## Consequences / Hệ quả

The controller is suitable for a later Qt/QML/native surface without exposing
raw roots, server URLs, file content, credential material, cookies,
authorization headers, or SQLite/sync handles. A restarted process causes a
new handshake, new event subscription, and new coherent snapshot; no durable
controller repair is needed. A slow presentation consumer observes the newest
safe state and does not block synchronization or IPC processing.

Controller phù hợp cho native surface Qt/QML sau này mà không expose root path,
server URL, file content, credential, cookie, authorization header hay handle
SQLite/sync. Process restart tạo handshake mới, event subscription mới và
snapshot coherent mới; không cần sửa chữa controller durable. Consumer hiển
thị chậm chỉ nhận state an toàn mới nhất và không block synchronization hay
IPC processing.

The controller remains an application component, not a domain entity. Its
revision, freshness, connection generation, reconnect timer, pending refresh
bit, command channel, and subscriber watch have no persistence or correctness
authority. Prompt 91–96 remain responsible for durable sync state and process
lifecycle. Native Windows execution is not implied by Linux tests; the named
pipe branch is validated by the existing cross-target/native gates.

Controller vẫn là application component, không phải domain entity. Revision,
freshness, connection generation, reconnect timer, pending refresh bit,
command channel và subscriber watch không persist và không có authority về
correctness. Prompt 91–96 vẫn chịu trách nhiệm durable sync state và lifecycle
process. Test Linux không suy ra native Windows execution; nhánh named pipe
vẫn phải qua gate cross-target/native hiện có.

## Alternatives rejected / Phương án bị từ chối

- Direct SQLite or `SyncRuntime` access: duplicates ownership and exposes
  synchronization correctness state to future UI code.
- A controller per Library: duplicates connections and makes process-level
  lifecycle/reconnect semantics inconsistent.
- An unbounded event history or command queue: allows UI burst/lag to consume
  unbounded memory and task capacity.
- Automatic process launch or OS kill: expands Prompt 97 into lifecycle,
  autostart, installer, or service ownership and risks stopping sync on UI exit.
- Replaying a command after a lost response: fabricates transport certainty and
  is unsafe for `Shutdown`; the controller preserves conservative outcome
  semantics.
- Qt/QML, tray, WebSocket/SSE, HTTP, localhost TCP, or a new server route:
  these are later product/transport decisions and are outside this core.

## Review trigger / Điều kiện xem xét lại

Supersede this ADR before adding a native UI, durable controller state, event
replay, multi-profile management, process-launch ownership, a third transport,
or a controller API that exposes a new synchronization capability. Any change
to Prompt 96 command semantics, endpoint authorization, snapshot redaction,
generation fencing, or shutdown independence requires a new locked decision
and synchronized EN/VI documentation.

Phải tạo ADR mới thay thế trước khi thêm native UI, controller state durable,
event replay, quản lý nhiều profile, ownership process launch, transport thứ
ba hoặc capability synchronization mới trong controller API. Mọi thay đổi về
command Prompt 96, endpoint authorization, redaction snapshot, generation
fence hoặc shutdown independence đều cần decision LOCKED mới và cập nhật đồng
thời tài liệu EN/VI.
