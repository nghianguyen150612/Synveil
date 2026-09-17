# ADR-035: Durable-change-first runtime signal integration

Status / Trạng thái: **Accepted / Chấp thuận — LOCKED**

Date / Ngày: 2026-09-12

Decision owners / Chủ sở hữu quyết định: Architecture, Client Sync, Platform

## Context / Bối cảnh

Prompt 92 provides the process-local `SyncRuntime` scheduler for bounded
Prompt 91 cycles, but local event producers already own the facts that create
durable synchronization work. Filesystem observation/reconciliation creates
outbound intents; credential lifecycle persists usable device credentials; a
future platform adapter may report network availability; and a controller may
request one manual synchronization opportunity. These producers must be
connected without making a wake signal a second synchronization state machine or
making signal delivery part of correctness.

The local SQLite store is the source of truth for outbound work. A process may
crash after a database commit, the runtime may already be stopped, a library
may be temporarily unregistered, or a burst of producers may arrive while a
cycle is running. The runtime therefore remains a bounded responsiveness aid;
startup and periodic polling must recover every durable change.

## Decision / Quyết định

Synveil adopts one narrow signal boundary:

```text
event producer
  -> durable local mutation / credential commit
  -> release the per-library writer/transaction boundary
  -> best-effort SyncWakeNotifier signal
  -> scheduling status returned to the caller
```

The durable operation always completes first. A notifier receives only a
`LibraryId` and a closed `SyncRuntimeWakeReason`; it never receives a
credential, token, cookie, path, raw filename, content, checkpoint, conflict
payload, or other synchronization evidence. `LocalStateStore` does not depend
on the concrete runtime. `OutboundObservationEngine` can attach the notifier at
construction, and `OutboundIntentProducer` is the explicit higher-level
boundary for other durable intent creators.

The runtime exposes bounded wake statuses: `Queued`, `Coalesced`,
`AlreadyRunningFollowupRecorded`, `RuntimeStopped`, and `UnknownLibrary`.
Coalescing is accepted work, not an error. A producer reports durable success
separately from wake status; `RuntimeStopped` or `UnknownLibrary` never rolls
back the already committed row. The non-throwing status surface is used by
durable producers, while the existing control surface may map unavailable
states to typed control errors.

Observation aggregates all durable changes from one bounded poll/reconciliation
operation and emits at most one `LocalChange` wake for the affected library.
A multi-batch rescan retains the durable-change bit until the scan completes,
then emits one wake. Exact semantic no-ops, self-generated suppression, and
ignored control paths do not signal. The native `notify` watcher and existing
overflow/rescan logic remain unchanged.

Usable enrollment storage and replacement signal `CredentialChanged` only
after secure-store verification and the existing enrollment transaction have
succeeded. Credential removal/logout emits no synthetic wake. A failed
validation, secure-store operation, or metadata commit emits no usable-
credential signal. The controller deduplicates affected library IDs.

`network_available()` is an external hint only. It schedules registered
libraries through Prompt 92 and does not bypass authentication, conflict
fences, Prompt 91 preconditions, or global concurrency. `sync_now(library_id)`
also schedules only through Prompt 92 and returns a scheduling status; it does
not wait for completion, invoke Prompt 91 directly, or drain a queue.

All `SyncRuntime`, `SyncRuntimeHandle`, and notifier clones share one `Arc`
supervisor. Registration and unregistration remain explicit. A wake racing
registration or arriving after unregistration may report a typed unavailable
status, while the durable local row remains available for startup or periodic
recovery. No persistent wake table, wake broker, OS network monitor, service
manager, route, frontend state, or new filesystem watcher is introduced.

## Consequences / Hệ quả

- Wake delivery improves latency but is never needed to preserve or discover
  synchronization work.
- The writer boundary is visibly outside the notifier callback, so the runtime
  may begin a cycle immediately without holding the local producer guard.
- Prompt 92 remains the only owner of recurrence, backoff, fairness,
  concurrency, lifecycle, and same-library active-cycle exclusion.
- A producer can explain `durable committed, wake unavailable` without
  conflating that result with a failed local operation.
- A rescan may finish its bounded units before signaling, which preserves one
  wake per reconciliation and leaves startup/periodic polling as the crash
  fallback.
- Credential and network integrations remain platform/controller seams rather
  than speculative OS-specific monitors.

## Alternatives / Phương án khác

1. Wake before the SQLite commit. Rejected because a fast cycle could observe
   no durable work and go idle before the producer commits.
2. Make `LocalStateStore` call a concrete `SyncRuntime`. Rejected because it
   reverses the persistence/scheduler dependency and makes storage lifecycle
   depend on an ephemeral process component.
3. Add a persistent wake queue or scheduler migration. Rejected because
   existing intent, observation, credential, startup, and periodic state
   already provide the required correctness recovery.
4. Add NetworkManager, systemd-networkd, WinRT, macOS reachability, or a new
   watcher in this phase. Rejected; Prompt 93 exposes only the abstract input
   and reuses the existing watcher.
5. Make Sync Now synchronously drain synchronization. Rejected because each
   Prompt 91 call is bounded and callers needing completion must observe the
   runtime status/event surface.

## Validation and review trigger / Điều kiện kiểm chứng và xem xét lại

Focused tests must prove commit-before-wake, writer release, wake-failure
durability, one wake for a durable observation batch/rescan, no wake for
no-op/suppressed events, credential failure/removal behavior, network/manual
coalescing, stopped/unknown handling, clone identity, and bounded multi-library
execution. PostgreSQL 17 live acceptance may exercise the real API and client
stack, but it does not change this process-local decision. Reconsider this ADR
only when a reviewed product controller, service lifecycle, OS connectivity
monitor, or persistent scheduling contract is introduced; that change requires
a superseding ADR rather than editing this locked conclusion.

---

# ADR-035: Tích hợp signal runtime theo thứ tự durable-change trước

Trạng thái / Status: **Accepted / Chấp thuận — LOCKED**

Ngày / Date: 2026-09-12

Chủ sở hữu quyết định / Decision owners: Architecture, Client Sync, Platform

## Bối cảnh / Context

Prompt 92 cung cấp scheduler `SyncRuntime` chỉ trong process cho cycle Prompt
91 bounded, nhưng các producer local mới là owner của fact tạo ra sync work
bền vững. Filesystem observation/reconciliation tạo outbound intent; lifecycle
credential persist device credential usable; platform adapter tương lai có thể
báo network đã có; controller có thể yêu cầu một cơ hội sync manual. Cần nối
các producer này mà không biến wake signal thành state machine sync thứ hai hay
biến signal delivery thành điều kiện đúng đắn.

SQLite local là source of truth cho outbound work. Process có thể crash sau DB
commit, runtime có thể đã stopped, Library có thể tạm thời unregister, hoặc
nhiều producer có thể đến trong lúc cycle đang chạy. Vì vậy runtime chỉ là hỗ
trợ responsiveness bounded; startup và polling định kỳ phải khôi phục mọi
durable change.

## Quyết định / Decision

Synveil chọn một signal boundary hẹp:

```text
event producer
  -> durable local mutation / credential commit
  -> release boundary writer/transaction theo Library
  -> signal best-effort qua SyncWakeNotifier
  -> trả scheduling status cho caller
```

Durable operation luôn hoàn tất trước. Notifier chỉ nhận `LibraryId` và
`SyncRuntimeWakeReason` closed; không nhận credential, token, cookie, path,
filename raw, content, checkpoint, conflict payload hay synchronization
evidence khác. `LocalStateStore` không phụ thuộc runtime concrete.
`OutboundObservationEngine` có thể gắn notifier lúc construct, còn
`OutboundIntentProducer` là boundary higher-level explicit cho producer intent
khác.

Runtime expose status bounded `Queued`, `Coalesced`,
`AlreadyRunningFollowupRecorded`, `RuntimeStopped` và `UnknownLibrary`.
Coalesced là work được accept, không phải error. Producer báo durable success
tách khỏi wake status; `RuntimeStopped` hoặc `UnknownLibrary` không rollback row
đã commit. Surface status non-throwing dùng cho durable producer; control
surface cũ có thể map state unavailable thành typed control error.

Observation gom mọi durable change của một poll/reconciliation operation bounded
thành tối đa một wake `LocalChange` cho Library. Rescan nhiều batch giữ bit
durable-change đến khi scan hoàn tất rồi mới phát một wake. Exact no-op,
self-generated suppression và control path bị ignore không signal. Native
watcher `notify` và logic overflow/rescan hiện có không thay đổi.

Store enrollment usable và replacement chỉ signal `CredentialChanged` sau khi
secure-store verification cùng enrollment transaction hiện có thành công.
Credential removal/logout không phát synthetic wake. Validation, secure-store
operation hoặc metadata commit thất bại thì không phát usable-credential
signal. Controller deduplicate Library ID bị ảnh hưởng.

`network_available()` chỉ là external hint. Nó schedule Library đã register qua
Prompt 92 và không bypass authentication, conflict fence, precondition Prompt
91 hay global concurrency. `sync_now(library_id)` cũng chỉ schedule qua Prompt
92 và trả scheduling status; không chờ completion, gọi Prompt 91 trực tiếp hay
drain queue.

Mọi clone `SyncRuntime`, `SyncRuntimeHandle` và notifier cùng dùng một
supervisor `Arc`. Registration/unregistration vẫn explicit. Wake race với
registration hoặc đến sau unregister có thể trả status unavailable typed, còn
local row durable vẫn sẵn sàng để startup/periodic recovery. Không thêm
persistent wake table, broker, OS network monitor, service manager, route,
frontend state hay filesystem watcher mới.

## Hệ quả / Consequences

- Wake làm giảm latency nhưng không bao giờ cần cho việc bảo toàn hay discover
  synchronization work.
- Writer boundary rõ ràng nằm ngoài notifier callback; runtime có thể bắt đầu
  cycle ngay mà không giữ producer guard local.
- Prompt 92 vẫn là owner duy nhất của recurrence, backoff, fairness,
  concurrency, lifecycle và loại trừ active cycle cùng Library.
- Producer giải thích được `durable đã commit, wake không khả dụng` mà không
  trộn kết quả đó với local operation thất bại.
- Rescan có thể hoàn tất các unit bounded rồi mới signal, giữ đúng một wake cho
  mỗi reconciliation và để startup/periodic polling làm crash fallback.
- Credential/network integration vẫn là seam cho platform/controller, không
  phải OS monitor speculative.

## Phương án khác / Alternatives

1. Wake trước SQLite commit. Bác bỏ vì cycle nhanh có thể thấy chưa có durable
   work rồi idle trước khi producer commit.
2. Để `LocalStateStore` gọi concrete `SyncRuntime`. Bác bỏ vì đảo dependency
   persistence/scheduler và làm storage lifecycle phụ thuộc process component
   ephemeral.
3. Thêm persistent wake queue hoặc scheduler migration. Bác bỏ vì intent,
   observation, credential, startup và periodic state hiện có đã đủ recovery
   correctness.
4. Thêm NetworkManager, systemd-networkd, WinRT, macOS reachability hoặc watcher
   mới ở phase này. Bác bỏ; Prompt 93 chỉ expose input trừu tượng và reuse
   watcher hiện có.
5. Cho Sync Now drain sync đồng bộ. Bác bỏ vì mỗi call Prompt 91 bounded; caller
   cần completion phải observe status/event runtime.

## Kiểm chứng và điều kiện xem xét lại / Validation and review trigger

Focused test phải chứng minh commit-before-wake, writer release, durability khi
wake fail, một wake cho durable observation batch/rescan, không wake cho no-op/
suppression, hành vi credential failure/removal, network/manual coalescing,
stopped/unknown, clone identity và execution multi-Library bounded. Live
PostgreSQL 17 có thể chạy qua API và client stack thật nhưng không đổi quyết
định process-local này. Chỉ xem xét lại ADR khi có controller sản phẩm,
service lifecycle, OS connectivity monitor hoặc persistent scheduling contract
được review; thay đổi đó phải tạo ADR kế tiếp thay vì sửa kết luận LOCKED.
