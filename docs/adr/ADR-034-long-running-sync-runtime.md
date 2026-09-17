# ADR-034: Long-running synchronization runtime lifecycle and scheduling

Status / Trạng thái: **Accepted / Chấp thuận — LOCKED**
Date / Ngày: 2026-09-12
Decision owners / Chủ sở hữu quyết định: Synveil client-sync maintainers

## Context / Bối cảnh

Prompt 91 established the only bounded, transport-neutral bidirectional
synchronization operation: `BidirectionalSyncCycleRunner::run_once`. Synveil
now needs a process-local component that can invoke that operation over time,
respond to local or user wakeups, and keep several libraries making progress
without turning one cycle into an unbounded drain.

Prompt 91 đã thiết lập thao tác đồng bộ hai chiều duy nhất, hữu hạn và trung
lập transport: `BidirectionalSyncCycleRunner::run_once`. Synveil cần một
component chỉ trong process có thể gọi thao tác đó theo thời gian, phản hồi
wakeup từ local hoặc người dùng, và giữ cho nhiều library cùng tiến triển mà
không biến một cycle thành drain không giới hạn.

The current client has no separate agent supervisor, durable local library
registry that can safely reconstruct a remote and filesystem adapter by itself,
or OS network/service lifecycle integration. The local `replicas` table remains
the durable library registry, while construction of an authenticated
`SyncRemote`, `LocalReplica`, and Prompt 91 runner remains an explicit caller
responsibility.

Client hiện chưa có agent supervisor riêng, chưa có durable local library
registry có thể tự an toàn dựng lại remote và filesystem adapter, cũng chưa có
tích hợp lifecycle OS/network/service. Bảng local `replicas` vẫn là registry
library durable; việc dựng `SyncRemote` authenticated, `LocalReplica` và
runner Prompt 91 vẫn là trách nhiệm tường minh của caller.

## Decision / Quyết định

Synveil introduces one `SyncRuntime` supervisor in `crates/client-sync`.
Callers register one already-constructed `SyncCycleExecutor` per library. The
production implementation of that port is `BidirectionalSyncCycleRunner`;
controlled executors are permitted only for scheduler tests. The runtime never
calls `InboundSyncEngine`, `OutboundSubmissionEngine`, snapshot APIs, handoff
APIs, or checkpoint APIs directly.

Synveil giới thiệu một supervisor `SyncRuntime` trong `crates/client-sync`.
Caller đăng ký một `SyncCycleExecutor` đã được dựng cho mỗi library. Cài đặt
production của port này là `BidirectionalSyncCycleRunner`; executor controlled
chỉ được phép dùng trong test scheduler. Runtime không bao giờ gọi trực tiếp
`InboundSyncEngine`, `OutboundSubmissionEngine`, API snapshot, API handoff hay
API checkpoint.

### Ephemeral runtime state / Runtime state ephemeral

The runtime keeps only in-memory scheduling state:

- lifecycle phase and join handle;
- per-library running flag and one coalesced pending wake reason;
- next due instant, last safe outcome, and transient-failure count;
- auth-suspended or locally faulted phase; and
- a bounded non-blocking event broadcaster.

Runtime không giữ gì ngoài memory:

- lifecycle phase và join handle;
- cờ running theo library và một wake reason pending đã coalesced;
- thời điểm next due, outcome an toàn gần nhất và số transient failure;
- phase auth-suspended hoặc local fault; và
- broadcaster event bounded, non-blocking.

No server migration, client migration, runtime job table, schedule table,
agent-run record, last-phase record, scheduler dump, or durable retry counter
is added. Restart intentionally discards these values. Startup schedules one
initial Prompt 91 cycle for every registered library, and the lower-level
cursor, candidate, handoff, intent, upload, idempotency, and conflict records
resume the durable work.

Không thêm migration server/client, bảng runtime job, bảng schedule, record
agent-run, record last-phase, scheduler dump hay retry counter durable. Restart
cố ý bỏ các giá trị này. Startup lên lịch một cycle Prompt 91 ban đầu cho mọi
library đã đăng ký; cursor, candidate, handoff, intent, upload, idempotency và
conflict ở tầng dưới tiếp tục công việc durable.

### Lifecycle / Vòng đời

`start()` is non-blocking and creates exactly one supervisor. It resets
ephemeral state and schedules all registered libraries at the current Tokio
instant. Calling `start()` again while running returns `AlreadyRunning`; it
does not create another supervisor or another worker for a library.

`start()` không block vô hạn và chỉ tạo đúng một supervisor. Nó reset state
ephemeral và lên lịch mọi library đã đăng ký tại instant Tokio hiện tại. Gọi
`start()` lần nữa khi đang chạy trả `AlreadyRunning`; không tạo supervisor hay
worker thứ hai.

`request_shutdown()` atomically stops new scheduling. `shutdown()` requests the
same stop and then joins; `join()` is the canonical wait path. Repeated
shutdown and join calls are safe. The supervisor wakes timer waits, lets every
already-running bounded Prompt 91 call finish, drains its completion, marks
registered libraries `Stopped`, and only then completes the join. It never
aborts an in-flight lower-level cycle merely to shorten shutdown.

`request_shutdown()` dừng việc lên lịch mới theo cách đồng bộ. `shutdown()`
gửi yêu cầu dừng rồi join; `join()` là đường chờ chuẩn. Gọi shutdown hoặc join
lặp lại là an toàn. Supervisor đánh thức các timer wait, cho mọi Prompt 91
bounded call đang chạy hoàn tất, nhận completion, đánh dấu library đã đăng ký
là `Stopped`, rồi mới hoàn tất join. Nó không abort cycle cấp dưới đang chạy
chỉ để shutdown nhanh hơn.

Dropping the last external runtime controller makes a best-effort graceful
shutdown request. This is only a signal; it does not cancel the Prompt 91
future or replace the explicit `shutdown().await` handoff expected from an
embedder.

Khi drop controller runtime bên ngoài cuối cùng, runtime gửi best-effort yêu
cầu graceful shutdown. Đây chỉ là signal; nó không cancel future Prompt 91 và
không thay thế việc embedder gọi tường minh `shutdown().await`.

### Registration / Đăng ký library

Registration is explicit because a durable SQLite row cannot by itself
reconstruct a safe filesystem root, authenticated credential, remote profile,
and runner. `registered_libraries()` exposes only library IDs. Registering a
library while stopped retains it for the next startup; registering while
running schedules its first cycle immediately. Duplicate registration is
idempotent and leaves the original executor untouched.

Đăng ký là tường minh vì một row SQLite durable không thể tự dựng an toàn root
filesystem, credential authenticated, remote profile và runner.
`registered_libraries()` chỉ expose library ID. Đăng ký khi stopped giữ library
cho startup tiếp theo; đăng ký khi đang chạy lên lịch cycle đầu ngay. Đăng ký
trùng là idempotent và giữ nguyên executor ban đầu.

Unregistration prevents new starts. If a cycle is active, its bounded call is
allowed to finish and the ephemeral entry is removed afterward. Unregistration
never deletes or repairs cursor, nodes, candidate, handoff, intent, conflict,
journal, or checkpoint state.

Unregister ngăn cycle mới bắt đầu. Nếu đang có cycle, bounded call đó được phép
hoàn tất rồi entry ephemeral mới bị xóa. Unregister không xóa hoặc sửa cursor,
node, candidate, handoff, intent, conflict, journal hay checkpoint.

### Wake surface / Bề mặt wake

`wake_library(library_id, reason)` accepts the typed reasons `Startup`,
`LocalChange`, `Manual`, `Periodic`, `NetworkAvailable`,
`CredentialChanged`, and `PreviousProgress`. Reasons are scheduling metadata;
they never change Prompt 91 semantics or bypass its authentication, conflict,
recovery, or outbound preconditions.

`wake_library(library_id, reason)` nhận các reason typed `Startup`,
`LocalChange`, `Manual`, `Periodic`, `NetworkAvailable`,
`CredentialChanged` và `PreviousProgress`. Reason chỉ là metadata lên lịch;
không reason nào đổi semantics Prompt 91 hoặc bypass precondition auth,
conflict, recovery hay outbound.

There is at most one pending wake reason per library. A wake while a cycle is
running sets that one pending slot; it does not spawn a second same-library
cycle. Wake reasons are merged by priority, with credential changes and manual
wakes taking precedence over passive hints. A pending wake is consumed by the
next independent scheduler opportunity, subject to auth/rate/fault policy.

Mỗi library chỉ có tối đa một wake reason pending. Wake khi cycle đang chạy chỉ
đặt slot pending đó; không spawn cycle thứ hai cùng library. Reason được merge
theo priority, trong đó credential change và manual wake ưu tiên hơn hint thụ
động. Wake pending được consume ở scheduling opportunity độc lập tiếp theo,
theo policy auth/rate/fault.

Local changes and manual wakes wake an idle library promptly. An ordinary
local/periodic wake does not bypass an auth block. `Manual` and
`CredentialChanged` explicitly resume an auth-blocked library for one fresh
Prompt 91 attempt; the attempt still performs normal auth checks. `Manual`,
`NetworkAvailable`, and `CredentialChanged` may bypass a transient backoff.
When Prompt 91 exposes no retry-after duration, even manual/network wakes
honor the dedicated rate-limit delay.

Local change và manual wake đánh thức library idle ngay. Local/periodic wake
thông thường không bypass auth block. `Manual` và `CredentialChanged` resume
tường minh một library auth-blocked cho một lần Prompt 91 mới; lần thử vẫn
kiểm tra auth bình thường. `Manual`, `NetworkAvailable` và
`CredentialChanged` có thể bypass transient backoff. Khi Prompt 91 không expose
retry-after duration, cả manual/network wake vẫn tuân theo delay rate-limit
riêng.

### Timer, fairness, and concurrency / Timer, fairness và concurrency

Idle libraries use a 30-second safety poll by default. The validated range is
1 second through 1 hour. A productive result with probable remaining durable
work is tail-requeued after a deterministic 1 ms cooperative delay. This gives
another runnable library a scheduling opportunity and prevents a broken
result from becoming a same-tick infinite loop.

Library idle dùng safety poll 30 giây mặc định. Khoảng hợp lệ là từ 1 giây đến
1 giờ. Kết quả có progress và có khả năng còn durable work được tail-requeue
sau delay cooperative tất định 1 ms. Cách này cho library runnable khác một cơ
hội chạy và ngăn result lỗi tạo infinite loop trong cùng tick.

One global semaphore-like capacity is represented by the supervisor's bounded
active set; the default is 4 and the hard maximum is 1,024. This is a resource
limit, not a global correctness mutex. Existing per-library lower-level guards
remain the correctness boundary, and one library can have at most one active
Prompt 91 call.

Một capacity global được biểu diễn bởi active set bounded của supervisor; mặc
định là 4 và hard maximum là 1.024. Đây là giới hạn tài nguyên, không phải
global correctness mutex. Guard cấp dưới theo library vẫn là boundary đúng đắn,
và mỗi library chỉ có tối đa một Prompt 91 call active.

Round-robin selection starts after the last library selected. A library with a
large outbound queue therefore receives one bounded unit per scheduler turn;
another library is not held behind an unbounded queue drain. A library backing
off, auth-blocked, or faulted does not consume a concurrency slot and does not
stop other libraries.

Việc chọn round-robin bắt đầu sau library được chọn gần nhất. Vì vậy library có
outbound queue lớn chỉ nhận một unit bounded mỗi lượt scheduler; library khác
không bị giữ sau một queue drain vô hạn. Library đang backoff, auth-blocked hoặc
faulted không chiếm slot concurrency và không dừng library khác.

### Result-to-schedule policy / Policy outcome tới lịch chạy

The runtime classifies the safe Prompt 91 result without inspecting or
mutating lower-level durable state:

| Prompt 91 category | Runtime state and next action |
|---|---|
| `Idle` | reset transient backoff; `Idle`; safety poll |
| `Progress` or probable remaining work | reset transient backoff; fair 1 ms follow-up |
| conflict or conflict-fenced outbound | `Idle`; ordinary safety poll; inbound remains active |
| offline/transport | `BackingOff`; deterministic exponential delay |
| server internal transient | `BackingOff`; deterministic exponential delay |
| rate limited | `BackingOff`; fallback 30 s because no duration is exposed |
| authentication required/revoked | `AuthBlocked`; no ordinary periodic network poll |
| recovery blocked / did not converge | bounded transient-style delay |
| local invariant/protocol failure | `Faulted`; no automatic retry |

Runtime phân loại result Prompt 91 an toàn mà không inspect hay mutate durable
state cấp dưới:

| Category Prompt 91 | State runtime và hành động tiếp theo |
|---|---|
| `Idle` | reset transient backoff; `Idle`; safety poll |
| `Progress` hoặc có khả năng còn work | reset transient backoff; follow-up fair 1 ms |
| conflict hoặc outbound bị fence bởi conflict | `Idle`; safety poll thường; inbound vẫn chạy |
| offline/transport | `BackingOff`; exponential delay tất định |
| server internal transient | `BackingOff`; exponential delay tất định |
| rate limited | `BackingOff`; fallback 30 giây vì chưa expose duration |
| auth required/revoked | `AuthBlocked`; không poll network thường kỳ |
| recovery blocked / did not converge | delay bounded kiểu transient |
| local invariant/protocol failure | `Faulted`; không retry tự động |

Transient delays default to 1, 2, 4, 8, 16, 32, then 60 seconds and remain
capped. There is no nondeterministic jitter. A successful safe cycle resets the
counter. A `NetworkAvailable` or manual wake can schedule a new independent
attempt immediately for transient failures; it cannot bypass the Prompt 91
checks.

Transient delay mặc định là 1, 2, 4, 8, 16, 32 rồi 60 giây và giữ ở mức cap.
Không có jitter không tất định. Cycle an toàn thành công reset counter. Wake
`NetworkAvailable` hoặc manual có thể lên lịch một attempt độc lập ngay cho
transient failure; nó không bypass các check Prompt 91.

The current Prompt 91 contract carries a rate-limited classification but no
retry-after duration. Runtime therefore uses the validated 30-second fallback
and never loops immediately on a 429. If a later lower-level contract adds a
safe bounded duration, this policy may honor it through a superseding decision.

Contract Prompt 91 hiện có classification rate-limited nhưng chưa có
retry-after duration. Runtime dùng fallback 30 giây đã validate và không loop
ngay trên 429. Nếu contract cấp dưới sau này thêm duration bounded an toàn,
policy có thể honor nó qua decision supersede.

An unresolved Prompt 88 conflict fences outbound only. Runtime continues normal
inbound safety polling and does not immediately retry the blocked outbound
queue. Candidate, handoff, and recovery ownership remains with Prompt 87 and
Prompt 85. The runtime does not create a recovery branch of its own.

Conflict Prompt 88 chưa resolve chỉ fence outbound. Runtime vẫn tiếp tục inbound
safety polling bình thường và không retry ngay queue outbound bị block.
Candidate, handoff và recovery vẫn thuộc Prompt 87 và Prompt 85. Runtime không
tạo recovery branch riêng.

### Status and events / Status và event

`SyncRuntimeLibraryStatus` exposes only a library ID, phase, relative next-due
duration, last safe outcome category, pending-wake bit, and transient-failure
count. Phases are `Idle`, `Scheduled`, `Running`, `BackingOff`, `AuthBlocked`,
`Faulted`, and `Stopped`. `SyncRuntimeEvent` is a bounded `broadcast` stream;
publishing is non-blocking and ignored when there are no subscribers. Event
delivery is never required for correctness.

`SyncRuntimeLibraryStatus` chỉ expose library ID, duration next-due tương đối,
outcome an toàn gần nhất, cờ wake pending và số transient failure. Phase gồm
`Idle`, `Scheduled`, `Running`, `BackingOff`, `AuthBlocked`, `Faulted` và
`Stopped`. `SyncRuntimeEvent` là stream `broadcast` bounded; publish
non-blocking và bỏ qua khi không có subscriber. Correctness không phụ thuộc
event delivery.

No status, event, diagnostic, or error contains a token, cookie, credential,
raw filename, absolute path, file content, opaque evidence, or raw conflict
payload.

Không status, event, diagnostic hay error nào chứa token, cookie, credential,
filename raw, path tuyệt đối, nội dung file, opaque evidence hay conflict
payload raw.

### Shutdown and transport boundary / Boundary shutdown và transport

`HttpSyncRemote` already configures finite connect, metadata, header, stream-idle,
and download deadlines and disables automatic HTTP retries. Graceful runtime
shutdown reuses these existing transport bounds. The runtime does not add
arbitrary cancellation inside snapshot activation, handoff finalization,
conflict persistence, or outbound submission.

`HttpSyncRemote` đã có deadline hữu hạn cho connect, metadata, header,
stream-idle và download, đồng thời tắt HTTP retry tự động. Graceful runtime
shutdown dùng lại các bound transport hiện có. Runtime không thêm cancellation
tùy tiện bên trong activation snapshot, finalize handoff, persist conflict hay
outbound submission.

## Consequences / Hệ quả

The client now has one reusable long-running lifecycle component while Prompt
91 stays the only execution primitive. Initial scheduling makes restart safe
without runtime-specific database repair. Coalesced wakes are responsive but
remain a hint; periodic polling and durable lower-level state are the safety
net after process loss.

Client hiện có một component lifecycle chạy dài có thể tái sử dụng trong khi
Prompt 91 vẫn là execution primitive duy nhất. Initial scheduling làm restart
an toàn mà không cần repair database riêng cho runtime. Wake coalesced phản hồi
nhanh nhưng chỉ là hint; periodic polling và durable state cấp dưới là safety
net sau khi process mất.

The component is a library/runtime boundary only. It does not add a systemd
user service, Windows Service, launchd integration, desktop tray lifecycle,
filesystem watcher wiring, OS NetworkManager/WinRT monitoring, SSE/WebSocket,
message broker, or public API route. Later work may provide those producers
with the narrow wake handle.

Component chỉ là boundary library/runtime. Không thêm systemd user service,
Windows Service, tích hợp launchd, lifecycle desktop tray, wiring filesystem
watcher, monitoring NetworkManager/WinRT, SSE/WebSocket, message broker hay
public API route. Prompt sau có thể cung cấp wake handle hẹp cho các producer đó.

## Alternatives / Phương án khác

Rejected alternatives are a second synchronization state machine, direct
calls from the runtime to inbound/outbound engines, a durable runtime schedule
table, an unbounded per-event task/channel, a global correctness mutex, a
same-library concurrent worker pool, immediate 429 retry, periodic auth
hammering, forcibly aborting active cycles, and OS-specific service/network
wiring in this prompt. Each would duplicate ownership, weaken crash safety,
create an unbounded resource, or exceed the lifecycle-only scope.

Các phương án bị loại gồm state machine đồng bộ thứ hai, runtime gọi trực tiếp
inbound/outbound engine, bảng schedule runtime durable, task/channel không giới
hạn theo event, global correctness mutex, worker pool concurrent cùng library,
retry 429 ngay, hammer auth định kỳ, abort cycle đang chạy, và wiring
service/network theo OS trong prompt này. Mỗi phương án duplicate ownership,
làm yếu crash safety, tạo resource không bounded hoặc vượt scope lifecycle-only.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

There is no migration. Review this locked decision only when a later prompt
needs durable scheduler ownership, OS/service lifecycle integration, server
push, or a changed lower-level Prompt 91 contract. Such work must supersede
this ADR and preserve the rule that durable synchronization correctness stays
below the runtime.

Không có migration. Chỉ review decision locked này khi prompt sau cần scheduler
ownership durable, tích hợp lifecycle OS/service, server push hoặc thay đổi
contract Prompt 91 cấp dưới. Công việc đó phải supersede ADR này và giữ quy tắc
correctness đồng bộ durable nằm dưới runtime.
