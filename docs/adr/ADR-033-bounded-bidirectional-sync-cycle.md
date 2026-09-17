# ADR-033: Bounded bidirectional synchronization cycle

Status / Trạng thái: **Accepted / Chấp thuận — LOCKED**
Date / Ngày: 2026-09-12
Decision owners / Chủ sở hữu quyết định: Synveil core and client-sync maintainers

## Context / Bối cảnh

Prompts 81–90 provide the durable synchronization primitives but do not define
one canonical operation that advances a library in both directions. A caller
needs a small, transport-neutral seam that can be used by a future runtime
without turning the client into a daemon or adding a second synchronization
state machine.

Các Prompt 81–90 đã cung cấp primitive đồng bộ bền vững nhưng chưa định nghĩa
một thao tác chuẩn để tiến một library theo cả hai chiều. Caller cần một seam
nhỏ, trung lập transport để runtime tương lai có thể sử dụng mà không biến
client thành daemon hoặc tạo máy trạng thái đồng bộ thứ hai.

## Decision / Quyết định

Synveil introduces `BidirectionalSyncCycleRunner` (also exported as the
repository-compatible `SyncCycleCoordinator`) in `crates/client-sync`. Its
`run_once(observed_at)` operation is a finite, restart-stateless composition
of the existing engines:

1. inspect the library's durable local state;
2. call `RebaselineConvergenceCoordinator::run_convergence_once()` exactly
   once; and
3. if the returned result and a fresh durable-state inspection establish a
   safe base, call `OutboundSubmissionEngine::process_next_ready_intent()` at
   most once.

The inbound/convergence phase always precedes outbound. The runner owns no
journal, checkpoint, snapshot, handoff, intent, conflict, retry, or cycle-run
record. It only holds references to the existing convergence coordinator,
outbound engine, and shared `LocalStateStore`; it does not issue HTTP requests
or perform synchronization SQL itself.

Synveil giới thiệu `BidirectionalSyncCycleRunner` (đồng thời export tên tương
thích `SyncCycleCoordinator`) trong `crates/client-sync`. Thao tác
`run_once(observed_at)` là composition hữu hạn, không giữ state chu kỳ qua
restart, của các engine hiện có:

1. inspect durable state cục bộ của library;
2. gọi đúng một lần
   `RebaselineConvergenceCoordinator::run_convergence_once()`; và
3. chỉ khi kết quả cùng lần inspect durable mới xác nhận base an toàn, gọi tối
   đa một lần `OutboundSubmissionEngine::process_next_ready_intent()`.

Phase inbound/convergence luôn đứng trước outbound. Runner không sở hữu record
journal, checkpoint, snapshot, handoff, intent, conflict, retry hoặc cycle-run
mới. Runner chỉ giữ reference tới convergence coordinator, outbound engine và
`LocalStateStore` dùng chung; runner không tự phát HTTP request hoặc SQL đồng
bộ.

### Result contract / Hợp đồng kết quả

`SyncCycleResult` preserves both phase results. `InboundCycleOutcome` retains
the Prompt 87 convergence result and maps expected authentication, transport,
rate-limit, and other remote failures to typed outcomes. `OutboundCycleOutcome`
either reports the existing Prompt 88/outbound result or a typed reason why
the one outbound step was not attempted. The result exposes stable
interpretations for durable progress, likely remaining work, idle state,
required conflict resolution, and required authentication.

`SyncCycleResult` giữ nguyên kết quả của cả hai phase. `InboundCycleOutcome`
giữ kết quả convergence Prompt 87 và map các lỗi expected về authentication,
transport, rate-limit và remote failure khác thành outcome typed.
`OutboundCycleOutcome` hoặc báo kết quả outbound/Prompt 88 hiện có, hoặc lý do
typed cho việc không gọi bước outbound duy nhất. Kết quả có các diễn giải ổn
định cho progress bền vững, khả năng còn work, idle, cần giải quyết conflict,
và cần authentication.

Expected operational states are not collapsed into generic `Err`: auth
failure, offline/timeout/TLS transport failure, rate limiting, and a typed
Prompt 87 `RecoveryBlocked` result skip outbound. `Err` remains for local
database failure, protocol/invariant violation, impossible state, or another
unexpected execution failure.

Các trạng thái vận hành expected không bị gộp thành `Err` generic: auth
failure, transport offline/timeout/TLS, rate limit và `RecoveryBlocked` typed
của Prompt 87 đều bỏ qua outbound. `Err` dành cho lỗi database cục bộ,
vi phạm protocol/invariant, state không thể xảy ra hoặc lỗi thực thi bất ngờ.

### Eligibility and ordering / Điều kiện và thứ tự

Outbound is eligible only after a safe convergence result (`IncrementalReady`,
safe bounded incremental progress, or `RebaselineConverged`) and a second
local inspection finds no candidate, pending handoff, bootstrap, pending feed
page/ack, local issue, or missing replica root. A candidate or H1 pending
handoff therefore wins before any unrelated outbound work. Prompt 87 retains
its own precedence and at-most-one-new-snapshot rule.

Outbound chỉ đủ điều kiện sau kết quả convergence an toàn
(`IncrementalReady`, incremental progress hữu hạn an toàn hoặc
`RebaselineConverged`) và lần inspect cục bộ thứ hai không thấy candidate,
handoff đang chờ, bootstrap, feed page/ack đang chờ, local issue hoặc replica
root bị thiếu. Vì vậy candidate hoặc handoff H1 luôn được xử lý trước outbound
không liên quan. Prompt 87 vẫn sở hữu precedence và quy tắc tối đa một snapshot
mới của chính nó.

An idle inbound result still permits one outbound attempt. An existing
unresolved Prompt 88 conflict does not stop inbound; the existing outbound
engine returns `BlockedByConflict` without a mutation request. Conflict
resolution remains an explicit Prompt 88 operation.

Inbound idle vẫn cho phép một lần thử outbound. Conflict Prompt 88 chưa giải
quyết không chặn inbound; outbound engine hiện có trả `BlockedByConflict` mà
không gửi mutation. Việc giải quyết conflict vẫn là thao tác tường minh của
Prompt 88.

When ordinary feed application encounters an active local outbound intent for
the affected node, the accepted lower-level engine conservatively preserves
the intent as `NEEDS_REBASE_VALIDATION` and records its typed
`BASE_STATE_CHANGED` observation issue before acknowledging the page. Prompt
91 reports that unsafe inbound result and does not synthesize a new Prompt 88
conflict or clobber local work. Canonical Prompt 88 conflicts remain owned by
the existing outbound server-precondition path and Prompt 87 rebaseline
classification. This distinction is deliberate: Prompt 91 composes the
accepted semantics and does not introduce a competing conflict classifier.

Khi apply feed ordinary gặp outbound intent đang hoạt động trên node bị ảnh
hưởng, engine cấp thấp đã được chấp thuận sẽ bảo toàn intent ở trạng thái
`NEEDS_REBASE_VALIDATION` và ghi observation issue typed
`BASE_STATE_CHANGED` trước khi ACK page. Prompt 91 báo kết quả inbound không an
toàn đó, không tự tạo Prompt 88 conflict mới và không ghi đè local work.
Conflict Prompt 88 chuẩn vẫn do đường server-precondition outbound hiện có và
phân loại rebaseline Prompt 87 sở hữu. Đây là chủ ý: Prompt 91 composition các
semantics đã chấp thuận, không tạo conflict classifier cạnh tranh.

### Boundedness / Giới hạn

One invocation has these bounds:

- one Prompt 87 convergence traversal, including only the finite recovery work
  that Prompt 87 itself permits;
- at most one new recovery snapshot creation through Prompt 87;
- one ordinary inbound feed page at the Prompt 81–90 engine boundary;
- at most one outbound intent/submission unit, including that engine's bounded
  prepare/upload/mutate work for the selected intent;
- no outer loop, recursion, retry, backoff, sleep, polling, feed drain, or
  queue drain.

Một invocation có các giới hạn sau:

- một traversal convergence Prompt 87, bao gồm chỉ recovery hữu hạn mà Prompt
  87 cho phép;
- tối đa một snapshot recovery mới qua Prompt 87;
- một ordinary inbound feed page ở boundary engine Prompt 81–90;
- tối đa một unit intent/submission outbound, kể cả prepare/upload/mutate hữu
  hạn của engine cho intent được chọn;
- không outer loop, recursion, retry, backoff, sleep, polling, drain feed hoặc
  drain queue.

Prompt 91 does not encode a retry counter, delay, scheduler timestamp, or
long-running lifecycle. A future Prompt 92 runtime may call this operation and
own scheduling policy separately.

Prompt 91 không encode retry counter, delay, timestamp scheduler hoặc lifecycle
chạy dài. Runtime Prompt 92 tương lai có thể gọi thao tác này và sở hữu policy
lên lịch ở tầng riêng.

### Crash and concurrency / Crash và concurrency

The runner is restart-stateless. If a caller dies after inbound, the durable
page/cursor or recovery handoff remains the source of truth for the next call.
If it dies during outbound, Prompt 88's durable mutation/upload idempotency and
reconciliation state controls the next call. If the server committed before a
response was lost, the next outbound invocation replays the durable mutation
identity and cannot create a duplicate accepted mutation. A persisted conflict
remains unresolved and fenced after a caller crash.

Runner không giữ cycle state bền vững riêng. Nếu caller chết sau inbound,
page/cursor hoặc recovery handoff bền vững là source of truth cho lần gọi sau.
Nếu chết trong outbound, state idempotency và reconciliation mutation/upload
bền vững của Prompt 88 điều khiển lần gọi sau. Nếu server đã commit trước khi
mất response, invocation outbound sau sẽ replay identity mutation bền vững và
không tạo mutation accepted trùng. Conflict đã persist vẫn unresolved và vẫn
fence sau crash.

Same-library callers reuse the existing per-library convergence guard and
replica-writer guard. They do not get a process-global lock; the lower-level
durable state and idempotency boundaries prevent duplicate recovery or accepted
mutation. Different libraries retain independent guards and may progress
concurrently. HTTP remains outside long SQLite writer transactions; only short
local durable transitions are serialized.

Caller cùng library dùng lại convergence guard theo library và replica-writer
guard hiện có. Không có lock process-global; durable state và boundary
idempotency cấp thấp ngăn recovery hoặc mutation accepted bị nhân đôi. Library
khác giữ guard độc lập và có thể tiến đồng thời. HTTP nằm ngoài SQLite writer
transaction dài; chỉ transition durable cục bộ ngắn được serialize.

## Consequences / Hệ quả

The canonical caller contract is deterministic and suitable for a future
runtime, while each existing engine remains the sole owner of its own durable
state machine. A completely idle cycle reports `IncrementalReady` plus
`NoReadyIntent`; inbound-only progress reports the bounded convergence result
plus `NoReadyIntent`; outbound-only progress reports idle inbound plus one
outbound result; and a combined cycle reports both progresses independently.

Hợp đồng caller chuẩn là tất định và phù hợp cho runtime tương lai, trong khi
mỗi engine hiện có vẫn là owner duy nhất của máy trạng thái durable riêng. Idle
hoàn toàn trả `IncrementalReady` cùng `NoReadyIntent`; inbound-only trả kết quả
convergence hữu hạn cùng `NoReadyIntent`; outbound-only trả inbound idle cùng
kết quả outbound duy nhất; cycle kết hợp báo progress của cả hai độc lập.

No server or client migration, public route, OpenAPI operation, frontend, or
deployment change is part of this decision. No cycle table is added. No
credentials, cookies, tokens, content bytes, raw paths, or raw conflict
payloads are stored in or exposed by the cycle result.

Quyết định này không thêm migration server/client, route public, operation
OpenAPI, frontend hoặc thay đổi deployment. Không thêm cycle table. Cycle
không lưu hoặc expose credential, cookie, token, content bytes, path raw hay
conflict payload raw.

## Alternatives / Phương án khác

Rejected alternatives are a full queue/feed drain, outbound-before-inbound,
an outer retry loop, a daemon or timer, a new cycle journal/table, a
process-global lock, and a new conflict/recovery implementation. Each would
either violate the bounded one-shot seam, duplicate an accepted lower-level
state machine, obscure crash ownership, or prematurely take Prompt 92 scope.

Các phương án bị loại gồm drain toàn bộ queue/feed, outbound trước inbound,
outer retry loop, daemon hoặc timer, cycle journal/table mới, lock
process-global và implementation conflict/recovery mới. Mỗi phương án hoặc vi
phạm seam one-shot bounded, duplicate máy trạng thái cấp thấp đã chấp thuận,
làm mờ ownership khi crash hoặc lấy trước scope của Prompt 92.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

There is no migration. Review this locked decision only when a later prompt
introduces a lifecycle runtime, durable dependency scheduling, or an explicit
replacement for the accepted lower-level inbound conflict fence. Such a change
must supersede this ADR rather than silently changing the cycle's ordering,
bounds, or ownership rules.

Không có migration. Chỉ review quyết định locked này khi prompt sau giới thiệu
runtime lifecycle, scheduling dependency bền vững hoặc replacement tường minh
cho inbound conflict fence cấp thấp đã chấp thuận. Thay đổi đó phải supersede
ADR này, không âm thầm đổi thứ tự, giới hạn hoặc ownership của cycle.
