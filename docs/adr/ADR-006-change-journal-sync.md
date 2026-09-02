# ADR-006: Ordered change-journal synchronization / Đồng bộ bằng nhật ký thay đổi có thứ tự

- Status / Trạng thái: **Accepted / Chấp thuận**
- Date / Ngày: 2026-08-21
- Owners / Chủ sở hữu: Sync, Database, Clients

## Context (English)

Offline clients need incremental changes, safe retries, tombstones, and a clear
recovery path. Timestamp polling and bare PostgreSQL sequences can skip changes:
a later sequence can commit while an earlier transaction is still invisible.

## Decision (English)

Each `Library` has an epoch and transactionally updated `sync_head`. Mutations
lock that row late in their transaction, increment a gap-safe `BIGINT`, append
one or more `ChangeEvent` records, and commit before the next allocation is
visible. The public `SyncCursor` is an authenticated opaque versioned token with
library, epoch, and last sequence.
`GET /api/v1/libraries/{library_id}/changes?cursor=...` is ordered and may
repeat events. Mutations carry a `client_mutation_id` and base ETag/version.
Conflicting content preserves incoming bytes as a deterministic conflict copy;
metadata conflicts return current state for rebase. Expired cursors require a
snapshot/rebaseline workflow.

The initial correctness profile also takes one short transaction-scoped
per-library namespace-mutation guard before `Node` row locks and the late
`sync_head` lock. It is never held while uploading bytes. This fences directory
cycles and orders recursive Trash against descendant edits. Recursive subtree
commands require a distinct server-issued opaque subtree precondition; the
Phase 4 protocol gate may select its scalable representation but may not weaken
that ordering invariant.

## Consequences (English)

Mutations serialize briefly per library to guarantee cursor safety. Clients
store server cursors only after applying the page atomically to local state and
must tolerate duplicates. Journal retention is an operational contract.
Sharded clocks or multi-writer journals require a superseding ADR and protocol
conformance proof. Replacing the coarse namespace guard requires ancestor/
subtree race proofs and protocol-compatible preconditions.

## Bối cảnh (Tiếng Việt)

Client offline cần thay đổi tăng dần, retry an toàn, tombstone và lối phục hồi
rõ ràng. Poll theo thời gian và PostgreSQL sequence thuần có thể bỏ sót: sequence
sau có thể commit khi giao dịch sequence trước còn chưa nhìn thấy.

## Quyết định (Tiếng Việt)

Mỗi `Library` có epoch và `sync_head` cập nhật trong giao dịch. Mutation lock
hàng này gần cuối giao dịch, tăng `BIGINT` không tạo lỗ do rollback, ghi
`ChangeEvent`, rồi commit trước khi số tiếp theo có thể xuất hiện. `SyncCursor`
công khai là token mờ đục, xác thực, có phiên bản, chứa library/epoch/sequence
cuối. `GET /api/v1/libraries/{library_id}/changes?cursor=...` trả thứ tự và có
thể lặp event. Mutation mang `client_mutation_id` cùng ETag/version gốc. Xung
đột nội dung giữ byte đến bằng
bản conflict xác định; xung đột metadata trả trạng thái hiện tại để rebase.
Cursor hết hạn phải chạy snapshot/rebaseline.

Profile correctness ban đầu còn lấy một namespace-mutation guard ngắn theo
`Library` trong transaction, trước khi lock hàng `Node` và trước `sync_head`
cuối. Guard không bao giờ được giữ khi upload byte. Nó ngăn cycle thư mục và xếp
thứ tự recursive Trash so với chỉnh sửa descendant. Lệnh subtree đệ quy cần một
subtree precondition mờ đục riêng do server cấp; gate giao thức Phase 4 có thể
chọn representation scale tốt hơn nhưng không được làm yếu bất biến thứ tự này.

## Hệ quả (Tiếng Việt)

Mutation bị tuần tự hóa ngắn theo library để bảo đảm cursor. Client chỉ lưu
cursor sau khi áp dụng nguyên tử cả page vào trạng thái local và phải chịu được
event lặp. Thời gian giữ journal là hợp đồng vận hành. Clock phân mảnh hoặc
journal nhiều writer cần ADR thay thế và bằng chứng test giao thức. Muốn thay
namespace guard thô phải có test race ancestor/subtree và precondition tương
thích giao thức.

Alternatives rejected / Phương án loại bỏ: timestamp polling; uncoordinated DB
sequences; silent last-writer-wins; cursor reset that omits tombstones.
