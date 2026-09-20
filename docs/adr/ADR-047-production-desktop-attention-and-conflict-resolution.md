# ADR-047: Production desktop attention and conflict resolution

Status: Accepted — LOCKED Prompt 106
Date: 2026-09-19
Owners: Synveil client-sync, client, desktop, and documentation maintainers

## Context

The production desktop needs to tell a user when synchronization requires a
decision and to provide a small, safe path to resolve an existing conflict.
The repository already has the canonical conflict model and durable local
records. `sync_conflicts` stores one deterministic record per outbound intent
and preserves the conflict kind, base facts, observed remote facts, status,
resolution, and replacement intent. Durable local-apply and observation issues
also contribute to the broader attention count.

The missing product boundary was the user-facing projection. A runtime outcome
alone is not durable enough for a shell that can restart, a client that can
remain active while the GUI is closed, or an authenticated user who is
temporarily offline. The projection must also avoid turning the desktop into a
second sync engine, exposing local absolute paths or content, or replaying a
mutation after an uncertain IPC response.

## Decision

### The existing client-sync conflict model remains canonical

No new merge semantics, server conflict API, or SQLite migration is introduced.
The canonical kinds remain:

- `RemoteRevisionChanged`
- `RemoteContentChanged`
- `RemoteStateChanged`
- `RemoteMissing`
- `NameCollision`
- `ParentChangedOrUnavailable`

Each durable record is `Unresolved` or `Resolved` and is identified by a
stable conflict ID. The record's library ID, outbound intent ID, and
`detected_at_ms` are part of the optimistic presentation fence. The supported
actions are the existing `AcceptRemote` and
`RetryLocalAgainstCurrentBase`. Retry is deliberately unavailable for
`RemoteMissing` and `NameCollision`; those kinds may only be accepted as
remote. The desktop does not invent a merge, conflict copy, overwrite, or
last-writer-wins action.

### Attention is a bounded durable snapshot

`LocalStateStore::attention_snapshot` reads exact unresolved counts from the
existing conflict, local-apply-issue, and observation-issue records. It returns
per-library counts and a bounded page of conflict items. The default page size
is **32** and the hard maximum is **128**. The snapshot explicitly reports
whether the item page is truncated, so the UI never implies that a finite page
is the complete set.

The item projection contains only what is needed for a decision:

- stable library, conflict, intent, and detection-time identifiers;
- one of the six canonical conflict categories;
- a safe managed relative path and optional previous relative path;
- file/directory kind, known lengths, base/observed revisions or states, and
  supported actions.

It contains no content bytes, hashes, staging or quarantine paths, absolute
local roots, credentials, cookies, raw HTTP bodies, or server diagnostics.
Root and `.synveil/` paths are not rendered as ordinary user files, and all
downstream presentation layers enforce bounded UTF-8-safe path lengths and
control-character rejection.

### The request path is one ownership chain

The production path is:

```text
QML attention card
  -> Rust Qt bridge
  -> DesktopController
  -> Prompt 96 local control IPC
  -> DesktopSyncHost
  -> LocalStateStore / canonical sync-conflict subsystem
```

The bridge and controller own presentation state only. They do not open
SQLite, read filesystem bytes, call the server, or resolve a conflict locally.
The control handshake advertises the bounded `Attention` capability. The
protocol adds `GetAttentionSnapshot` and `ResolveConflict`; the event
`AttentionStateChanged` is an invalidation hint, not a second source of truth.
A fresh snapshot is authoritative after startup, reconnect, event loss, or
any stale result.

### Resolution is durable-before-wake and generation-fenced

The host persists the canonical resolution in one state-store transaction and
only then requests a normal synchronization wake. If the client is
`PausedByUser`, resolution remains durable but the wake is suppressed; the
pause state is never cleared or bypassed. The existing runtime remains the
only executor of replacement intents.

The request carries library ID, conflict ID, outbound intent ID, detection
timestamp, and the typed action. The state store rejects a mismatching
library, intent, or detection timestamp as stale before changing state. A
duplicate action against an already resolved record is reported as
`AlreadyResolved`; a stale or missing item is reported without a retry. Only
one attention resolution is admitted per client-control handle and per desktop
presentation boundary. An IPC response that is lost or otherwise uncertain is
`OutcomeUnknown`; the controller refreshes and never replays the action.

### Restart, offline, authentication, and root behavior

The durable records survive a desktop-shell restart and remain visible while
the background client continues without the GUI. Restart reconstructs the
same bounded snapshot; it does not create a new conflict or forget an
unresolved decision. Offline or unauthenticated conditions leave the last
durable local attention state inspectable when the client/control process is
available, while a missing profile, unavailable client, or unavailable root
is reported through the existing generic status categories. No UI path treats
an unavailable root as an empty tree or performs a bulk retry.

### Server and schema boundaries remain unchanged

This feature is a local projection and action adapter over the existing
authenticated sync subsystem. It adds no server route, OpenAPI operation,
server migration, client SQLite migration, credential state, or background
retry daemon. Migration counts remain server **36** and client schema **V7**
with **7 client migrations**.

## Consequences

- Users get a bounded, restart-safe attention surface with explicit actions.
- The conflict state shown by the desktop is durable rather than inferred only
  from the most recent runtime cycle.
- The UI can explain conflict category and safe managed path without exposing
  local machine topology or file contents.
- Resolution remains idempotent and stale-safe, while the existing runtime and
  server contract remain the only synchronization authorities.
- Other durable blockers contribute exact counts but do not acquire invented
  conflict actions; their existing status surfaces remain responsible for
  diagnosis and recovery.
- Native Qt, live client/server, offline/auth transition, restart, and root
  availability evidence must be reported separately from source or unit-test
  evidence. This ADR does not turn unavailable environment gates into a
  readiness claim.

## Alternatives rejected

- Deriving the banner only from `SyncRuntimeOutcome`: it disappears after a
  restart and misses durable unresolved records.
- Adding a server attention endpoint or a second conflict table: it would
  duplicate canonical ownership and widen the API/migration surface.
- Sending content bytes, absolute paths, or staging paths to QML: it would
  violate the safe presentation boundary and is unnecessary for the two
  supported decisions.
- Blindly retrying a resolution after timeout or reconnect: a committed
  action could be applied twice or against a newer generation.
- Clearing user pause when resolving: it would make a conflict card an
  implicit scheduler control and violate Prompt 105.
- Implementing merge, overwrite, conflict-copy, or last-writer-wins controls
  in the desktop: those semantics are outside the locked canonical model.

## Migration and review trigger

No migration is required. Review this ADR if attention must be remotely
managed, if a future conflict action needs server-side semantics, if per-user
or per-library policy is introduced, or if the item projection must expose
content rather than bounded metadata. Any such change requires a new contract
review and explicit migration/API decisions.

## Tóm tắt tiếng Việt

### Bối cảnh và quyết định

Desktop production cần cho user biết khi sync cần quyết định, nhưng source of
truth vẫn là conflict model durable đã có trong `client-sync`. Không thêm merge
semantics, server API, bảng conflict mới hay SQLite migration. Sáu kind giữ
nguyên là `RemoteRevisionChanged`, `RemoteContentChanged`,
`RemoteStateChanged`, `RemoteMissing`, `NameCollision` và
`ParentChangedOrUnavailable`. Record có trạng thái `Unresolved` hoặc
`Resolved`, conflict ID ổn định, library ID, outbound intent ID và
`detected_at_ms` làm generation fence. Action chỉ gồm `AcceptRemote` và
`RetryLocalAgainstCurrentBase`; retry bị cấm với `RemoteMissing` và
`NameCollision`.

`LocalStateStore::attention_snapshot` đọc exact count durable từ conflict,
local-apply issue và observation issue hiện có. Snapshot có count theo library,
page bounded mặc định **32**, tối đa **128**, cùng cờ `truncated`. Item chỉ
chứa ID, category, managed relative path an toàn, previous path nếu có,
file/directory kind, length, revision/state và action được phép. Không đưa
content bytes, hash, absolute root, staging/quarantine path, credential,
cookie, raw HTTP body hay server diagnostic sang UI. Root và `.synveil/` không
được hiển thị như file bình thường.

Request đi đúng một chain: QML → Rust bridge → `DesktopController` → Prompt
96 local IPC → `DesktopSyncHost` → state store/conflict subsystem. Bridge và
controller không mở SQLite, đọc bytes hay tự resolve. `AttentionStateChanged`
chỉ là invalidation; fresh snapshot mới là authoritative.

Resolve gửi library ID, conflict ID, intent ID, detection timestamp và typed
action. State store persist trong transaction trước, sau đó mới wake runtime;
nếu đang `PausedByUser` thì vẫn persist nhưng không wake và không clear pause.
Mismatch generation là stale, duplicate là `AlreadyResolved`, mất response là
`OutcomeUnknown` rồi refresh, tuyệt đối không replay. Mỗi boundary chỉ cho một
resolution in flight.

Record vẫn tồn tại qua GUI restart và có thể inspect khi client background còn
chạy. Offline/auth/root unavailable được báo theo status generic hiện có;
không coi root unavailable là empty tree và không bulk retry. Không thêm server
route, OpenAPI, server migration, client migration hay retry daemon. Server vẫn
**36**, client schema **V7**, tổng **7 client migrations**.

### Hệ quả và điều kiện xem xét lại

Desktop có attention surface bounded, restart-safe, không lộ topology máy hay
bytes. Runtime và server vẫn là authority duy nhất. Native Qt, live
client/server, offline/auth, restart và root evidence phải báo cáo riêng; unit
test hoặc source inspection không tự tạo readiness claim. Xem xét lại ADR nếu
cần remote management, action server-side mới, policy per-user/per-library,
hoặc projection phải chứa content.
