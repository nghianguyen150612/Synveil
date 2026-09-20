# ADR-044: Desktop library onboarding and local root binding / Onboarding library desktop và bind root cục bộ

Status / Trạng thái

Accepted — LOCKED Prompt 103

Date / Ngày

2026-09-19

Decision owners / Chủ sở hữu quyết định

Synveil maintainers / Nhóm maintainers Synveil

## Context / Bối cảnh

Prompt 102 can leave a desktop process in the valid state `Authenticated` with
zero libraries. Prompt 103 needs a small native path from that state to the
existing server library model and the existing bidirectional sync runtime.
The path crosses QML, the native bridge, the IPC-backed controller,
`synveil-client`, the authenticated HTTP client, the existing local SQLite
replica registry, and the filesystem root safety boundary.

The repository already has the canonical `Library`/root-node model on the
server, `GET /api/v1/libraries`, profile-bound device credentials, profiled
managed-root markers, `LocalStateStore` replica rows, `DesktopSyncHost`,
`SyncRuntime`, and root-unavailable fencing. It does not have a supported
server-side attach/import operation or a safe desktop-specific library
registry. The current first-bind filesystem implementation accepts an empty
directory (or an already-owned managed root during recovery) and does not
adopt arbitrary pre-existing files.

## Decision / Quyết định

### 1. Ownership and supported flow / Sở hữu và flow được hỗ trợ

The only production setup path is:

```text
QML name + native folder picker
    -> DesktopUiBridge
       -> DesktopController
          -> Prompt 96 version-1 local IPC
             -> synveil-client DesktopControlHandle
                -> DesktopSyncHostHandle
                   -> authenticated canonical HTTP client
                      -> POST /api/v1/libraries
                   -> LocalStateStore + managed-root marker
                   -> DesktopSyncHost / SyncRuntime registration
```

QML receives only bounded display input, a selected folder URL/path, safe
category outcomes, and the existing library snapshot. It never opens HTTP,
SQLite, `SecretStore`, a filesystem watcher, or sync runtime handles. The
client remains the authority for library identity, profile/device scope, root
validation, persistence, and runtime activation.

The supported v0.1 operation is creation of an empty logical server library.
There is no canonical server attach/import API in this repository, so Prompt
103 does not invent one and does not emulate attach by creating a duplicate.
An existing remote library is used only during response-loss reconciliation
for the same caller-supplied UUID and name.

### 2. Canonical inputs and server contract / Input canonical và contract server

The user supplies only a bounded logical library name and a local folder. The
client generates a UUIDv7 `LibraryId`; users never enter IDs, checkpoints,
generations, cursors, object prefixes, or device-binding values.

The narrow server addition is `POST /api/v1/libraries` with:

```json
{"id":"<UUIDv7>","name":"<logical name>"}
```

It creates the canonical `Library` and its root node, is owner-scoped, and is
idempotent for the caller-supplied UUID. A retry that finds the same UUID with
a different owner is concealed as inaccessible; a different name is an
identity conflict. The route accepts either the existing browser session
boundary (with browser CSRF) or an authenticated device bearer (CSRF is not
applicable to the device class). No local path crosses the server boundary.
No server migration is required because the existing library/root schema is
already sufficient.

### 3. Root safety policy / Chính sách an toàn root

The client validates the candidate before any network mutation. The selected
path must be absolute, canonicalizable, non-redirecting, a writable directory
that is not the filesystem root, process current directory, or user home. It
must be within the bounded manifest/path budget and contain no control
characters. The existing first-bind policy accepts an empty directory or the
exact managed marker used by an interrupted bind; arbitrary existing content,
regular files, missing paths, inaccessible/read-only roots, symlink/junction
redirects, and unsafe platform roots are rejected. Managed-root reopening
still verifies profile, owner, device, library, and binding identity.

Root comparisons use canonical path components, with platform case handling,
not raw string prefixes. Active and pending bindings cannot overlap unless a
future canonical sync design explicitly supports nested libraries. The local
absolute path is never sent to the server and is redacted from diagnostics,
IPC responses, snapshots, and QML feedback.

### 4. Recoverable ordering and unknown outcomes / Thứ tự có thể phục hồi

Setup admission is bounded to one in-flight operation. The durable sequence is:

```text
validate root and name
  -> fsync pending UUID/root manifest binding
  -> create/reconcile the owner-scoped remote library
  -> initialize/reopen the profiled managed root
  -> commit the existing LocalStateStore replica/root binding
  -> fsync active UUID/root manifest binding
  -> register host/runtime and native watcher
  -> emit one configuration refresh and allow normal runtime scheduling
```

The UUID in the pending manifest is the recovery identity. A transport,
timeout, redirect, body-limit, or other ambiguous create result is mapped to
`OutcomeUnknown`; the client performs one bounded authoritative library-list
refresh and accepts only an exact UUID/name match. It never blindly replays
the create request. If the list cannot prove the result, the pending binding
remains available for a later explicit retry. A failed persistence or runtime
admission never reports a false active state and never wakes an unbound
library.

The pending record is intentionally retained as an append-only recovery hint;
the active record is authoritative for process materialization. On restart,
only active records are admitted to the host, while pending records let the
user safely finish an interrupted operation without creating a second remote
library.

### 5. Runtime and root-loss behavior / Runtime và mất root

The GUI lifecycle remains independent of the client and sync lifecycle. The
host registers the library only after local durable state is correct, and the
existing runtime schedules bounded initial/rebaseline work. Prompt 103 does
not implement a GUI sync engine or block setup on complete convergence.

If an active root disappears or becomes temporarily unavailable, the existing
deferred replica and runtime root fence are preserved. The runtime does not
interpret the missing root as an empty tree, emit mass deletions, detach the
library, sign out, or erase durable bindings. When the root reappears, marker
identity is rechecked and the existing wake/recovery path may resume safely.

### 6. Profile isolation and presentation / Cô lập profile và presentation

Every setup request is bound to the process profile, loaded enrollment owner,
and enrolled device. The HTTP scope and local state store must agree before
the remote mutation or binding is accepted. Tokens, credentials, local paths,
raw HTTP responses, filesystem errors, and internal IDs are not exposed to
QML. The bridge shows only generic setup feedback such as invalid name/root,
authentication required, server unavailable, response unknown, persistence
failure, or configured.

## Consequences / Hệ quả

An authenticated zero-library desktop reaches a real, durable first-library
path without a second registry or a fake default library. Duplicate creation
is prevented across normal retries and ambiguous responses by the generated
UUID plus authoritative reconciliation. Runtime activation is ordered after
the existing local binding, and restart/root-loss behavior reuses the already
tested host and replica fences.

The current repository intentionally does not claim arbitrary non-empty-folder
adoption, existing-remote-library attach, Windows runtime execution, or
PostgreSQL live acceptance without those gates being run. The UI remains a
minimal onboarding surface; library management, migration, conflict UX,
selective sync, and deletion UX stay out of scope.

## Alternatives / Phương án khác

- A GUI-owned HTTP client or SQLite/library registry was rejected because it
  would duplicate the client authority and bypass Prompt 96 security.
- A fake default library was rejected because zero-library authenticated state
  is valid and must not silently bind a root.
- Blindly retrying `POST /api/v1/libraries` was rejected because response loss
  must not create duplicates; UUID reconciliation is the narrowest safe
  idempotency mechanism supported by the existing model.
- Sending the local absolute path to the server was rejected because the
  server stores logical metadata, not desktop filesystem locations.
- Adopting arbitrary existing files or inventing an attach API was rejected
  because the current managed-root initializer does not support those flows.
- Registering the watcher before durable local state was rejected because an
  early event could race an unbound library and violate data-loss fences.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

No server migration or client migration is added for this decision. Review the
ADR if the server introduces a first-class attach/import contract, if the local
replica schema gains a canonical path/binding transaction that changes the
ordering, or if the filesystem layer explicitly supports safe non-empty-root
adoption or nested libraries. Such a change requires a superseding ADR and
new acceptance evidence.

## Bản tóm tắt tiếng Việt

Trạng thái `Authenticated` nhưng có zero library là hợp lệ. Đường onboarding
duy nhất là `QML -> DesktopUiBridge -> DesktopController -> IPC Prompt 96 ->
synveil-client`. QML chỉ nhận tên library bounded và chọn thư mục bằng native
folder picker; client sở hữu HTTP, profile/device scope, SQLite, `SecretStore`,
marker managed-root và runtime.

Prompt 103 chỉ hỗ trợ flow canonical tạo logical library rỗng bằng
`POST /api/v1/libraries` với UUIDv7 do client sinh. Repository chưa có API
attach/import library hiện hữu, nên không tự phát minh attach và không tạo
duplicate remote library để giả lập attach. UUID pending được ghi bền vững để
reconcile response-loss bằng authoritative list; không blind replay.

Root phải là thư mục absolute, canonical, writable, không phải `/`, home hay
current directory, không redirect/symlink/junction, không control character và
không có arbitrary content. Root rỗng hoặc marker managed đúng scope mới được
nhận. So sánh overlap dùng path components/case policy, không dùng string
prefix. Đường dẫn cục bộ không gửi lên server và bị redaction khỏi UI/log.

Thứ tự thành công là validate -> ghi pending -> tạo/reconcile remote -> tạo
hoặc mở managed root -> commit binding `LocalStateStore` -> ghi active ->
register host/runtime/watcher. Runtime không được wake trước durable binding.
Root mất không biến thành empty tree hoặc mass deletion; library bị fence và
chỉ resume sau khi root xuất hiện lại, marker được kiểm tra lại. Không thêm
migration server/client; attach, conflict UX, selective sync và library
management để phase sau.
