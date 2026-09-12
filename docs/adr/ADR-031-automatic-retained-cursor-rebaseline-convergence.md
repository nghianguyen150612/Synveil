# ADR-031: Automatic retained-cursor rebaseline convergence / Hội tụ rebaseline tự động khi retained-cursor không còn hợp lệ

- Status / Trạng thái: **Accepted / Chấp thuận — LOCKED**
- Date / Ngày: 2026-09-11
- Owners / Chủ sở hữu: Sync, Desktop

## Context (English)

Prompt 86 intentionally bounds journal history, snapshot payloads, and
handoff proofs. A client may consequently have a cursor below the retained
floor, an incompatible epoch, or an already applied snapshot whose server proof
has since been removed. Continuing ordinary inbound sync would be unsafe, but
trusting a client-held old boundary would let the client advance a server device
checkpoint without current server authority.

## Decision (English)

Use one callable, finite `RebaselineConvergenceCoordinator` invocation above
the existing inbound engine, durable snapshot applier, and Prompt 85 handoff.
The local precedence is real candidate, then `AppliedPendingHandoff`, then one
ordinary incremental operation. A candidate always resumes; a valid pending
handoff always tries the existing server operation before any replacement.

Automatic snapshot recovery is permitted only for server-authoritative retained
history/epoch rebaseline, or for an existing handoff returning concealed/missing
proof (`NotFound`) or typed checkpoint conflict. Authentication/revocation,
authorization, rate limiting, server/network/TLS/timeout, malformed protocol,
local database failure, and candidate corruption do not trigger recovery.

One invocation may issue at most one snapshot create POST. It makes a durable,
library-scoped v5 candidate-row claim before the non-idempotent POST, promotes
that claim only from a validated server descriptor, and never repeats the POST
after a response loss. A later invocation may make its own one allowed POST.
There is no daemon, polling, backoff, recursive call, generic retry, global
process lock, cleanup action, or conflict policy.

For proof-loss/checkpoint-conflict recovery, the old pending marker H1 remains
while S2 is fetched. The existing v5 candidate plus one applied-handoff row
represent this safely. One activation transaction swaps the authoritative base
and replaces H1 with H2, so readers observe old-base/H1 or new-base/H2, never a
handoff-free or mixed state. Prompt 85 remains the only operation that can ask
the server to install a checkpoint and the only local finalization that can set
the cursor and remove H2. The old client boundary is informational only and is
never sent as checkpoint authority.

Outbound intents and their preconditions, source references, and submission
state are not rewritten by this decision. Prompt 88 owns conflict policy.

## Bối cảnh và quyết định (Tiếng Việt)

Prompt 86 cố ý bound journal history, snapshot payload và handoff proof. Vì
vậy client có thể có cursor thấp hơn retained floor, epoch không tương thích,
hoặc snapshot đã apply nhưng proof server của nó đã bị xóa. Tiếp tục inbound
thông thường là không an toàn; nhưng tin boundary cũ do client giữ sẽ cho phép
client advance checkpoint device server mà không có authority server hiện tại.

Dùng một invocation `RebaselineConvergenceCoordinator` hữu hạn, có thể gọi,
ở trên inbound engine hiện có, snapshot applier durable và handoff Prompt 85.
Precedence local là candidate thật, rồi `AppliedPendingHandoff`, rồi một
operation incremental thường. Candidate luôn được resume; pending handoff hợp
lệ luôn thử server operation hiện có trước bất kỳ replacement nào.

Snapshot recovery tự động chỉ được phép khi server-authoritative retained
history/epoch trả rebaseline, hoặc handoff hiện có trả proof bị thiếu/conceal
(`NotFound`) hay checkpoint conflict typed. Authentication/revocation,
authorization, rate limit, server/network/TLS/timeout, protocol malformed,
database local failure và candidate corruption không trigger recovery.

Mỗi invocation tạo nhiều nhất một POST snapshot. Nó tạo durable claim scope
Library bằng candidate row v5 trước POST không idempotent, chỉ promote claim từ
descriptor server đã validate và không lặp POST sau response loss. Invocation
sau có thể có một POST được phép riêng. Không có daemon, polling, backoff,
gọi đệ quy, generic retry, global process lock, cleanup action hay conflict
policy.

Khi recovery proof-loss/checkpoint-conflict, marker pending cũ H1 vẫn giữ trong
lúc tải S2. Candidate v5 hiện có cùng một row applied-handoff biểu diễn an toàn
trạng thái này. Một transaction activation swap base authoritative và thay H1
bằng H2, nên reader chỉ thấy old-base/H1 hoặc new-base/H2, không handoff-free
hay pair lẫn. Prompt 85 vẫn là operation duy nhất có thể yêu cầu server cài
checkpoint và finalization local duy nhất đặt cursor/xóa H2. Boundary cũ client
chỉ mang tính thông tin, không bao giờ được gửi như checkpoint authority.

Outbound intent, precondition, source reference và submission state không bị
ghi lại bởi quyết định này. Prompt 88 sở hữu conflict policy.

## Consequences / Hệ quả

- Client schema remains version 5; server migrations remain 36.
- No public route or OpenAPI operation is added; the four durable snapshot
  operations are reused.
- A lost create response can leave a bounded server artifact; it never causes a
  second create in the same invocation.
- Live retained-floor, proof-removal, retention-race, and checkpoint-ahead
  evidence remains a required separate PostgreSQL 17 gate.
