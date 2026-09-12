# ADR-032: Deterministic sync conflict preservation and explicit resolution / Bảo toàn xung đột đồng bộ tất định và phân giải tường minh

Status / Trạng thái: Accepted / Chấp thuận — LOCKED
Date / Ngày: 2026-09-12
Decision owners / Chủ sở hữu quyết định: Sync, Client, Storage, Security

## Context / Bối cảnh

English: Incremental sync and retained-history rebaseline keep a client replica
convergent, but they cannot choose a winner when a durable local user intent and
the canonical server state legitimately diverge. Treating a 409 as a transient
failure would repeatedly submit stale work; silently rebasing or deleting that
work would lose either auditability or user data.

Tiếng Việt: Đồng bộ tăng dần và rebaseline khi lịch sử đã bị loại bỏ giữ replica
client hội tụ, nhưng không thể tự chọn bên thắng khi intent cục bộ bền vững của
người dùng và trạng thái server chuẩn phân kỳ hợp lệ. Xem 409 như lỗi tạm thời sẽ
gửi lại công việc đã cũ; tự động rebase hoặc xóa công việc đó sẽ làm mất dấu vết
kiểm toán hoặc dữ liệu người dùng.

## Decision / Quyết định

English:

- The server-derived remote base remains canonical. A conflict never rewrites
  that base and is not a rebaseline trigger.
- The original outbound intent, its base/precondition, and any durable staged
  content source remain immutable evidence. No timestamp-, revision-, client-,
  or server-wins policy runs automatically.
- A client-local SQLite conflict is durable state. A unique intent reference
  permits at most one conflict record per intent; its initial evidence is not
  rewritten by later remote changes. Conflict detection and the outbound
  submission fence commit in one transaction.
- An unresolved conflict conservatively stops outbound submission for its
  library because the current queue has no durable dependency graph. This
  prevents dependent work from overtaking it. Inbound synchronization remains
  enabled. When conflicting content has a hash-verified staged copy, inbound may
  expose the canonical remote bytes while retaining the local source.
- `AcceptRemote` is an explicit local transaction: resolve the conflict and
  cancel the old intent, with no server mutation and no synchronous source
  deletion.
- `RetryLocalAgainstCurrentBase` is an explicit local transaction: validate
  current local canonical node/parent state and content source, resolve the old
  conflict, supersede (but do not rewrite) the old intent, and create exactly
  one new linked intent with the current base. Network submission happens later.
- `KeepBoth`, generic merge, force overwrite, automatic conflict-copy naming,
  background resolution, and conflict-history cleanup are deferred.
- Client conflicts do not pin the server journal, snapshot payload, retention
  floor, device checkpoint, or handoff proof.

Tiếng Việt:

- Remote base do server cung cấp luôn là chuẩn. Xung đột không sửa base này và
  không phải trigger rebaseline.
- Intent outbound gốc, base/precondition của nó và nguồn nội dung staged bền
  vững (nếu có) là bằng chứng bất biến. Không tự động áp dụng timestamp-wins,
  revision-wins, client-wins hoặc server-wins.
- Xung đột SQLite chỉ ở client là trạng thái bền vững. Tham chiếu intent duy
  nhất bảo đảm tối đa một bản ghi xung đột cho mỗi intent; bằng chứng ban đầu
  không bị sửa bởi thay đổi remote về sau. Phát hiện xung đột và hàng rào chặn
  gửi outbound commit trong cùng một transaction.
- Xung đột chưa giải quyết chặn bảo thủ outbound của library vì queue hiện tại
  chưa có dependency graph bền vững. Nhờ đó công việc phụ thuộc không vượt qua
  intent xung đột. Inbound vẫn chạy. Khi nội dung xung đột có bản staged đã xác
  minh hash, inbound có thể đưa byte remote chuẩn ra replica mà vẫn giữ nguồn
  cục bộ.
- `AcceptRemote` là transaction cục bộ tường minh: giải quyết xung đột và hủy
  intent cũ, không mutation server và không xóa nguồn đồng bộ ngay lập tức.
- `RetryLocalAgainstCurrentBase` là transaction cục bộ tường minh: kiểm tra
  trạng thái node/parent chuẩn hiện tại và nguồn nội dung, giải quyết xung đột,
  supersede (không sửa) intent cũ, rồi tạo đúng một intent mới có liên kết và
  base hiện tại. Việc gửi mạng diễn ra sau đó.
- `KeepBoth`, merge tổng quát, force overwrite, tự đặt tên conflict-copy, phân
  giải nền và dọn lịch sử xung đột được hoãn.
- Xung đột client không pin journal, payload snapshot, retention floor,
  checkpoint thiết bị hoặc handoff proof trên server.

## Consequences / Hệ quả

English: Conflict listing is library-scoped, keyset-paged in stable
`(detected_at, conflict_id)` order, and bounded to 100 by default and 1,000 at
maximum. Resolution history remains locally queryable. Independent outbound
work in the same library is intentionally delayed until dependency semantics
are represented durably; work in other libraries and all inbound work remain
independent.

Tiếng Việt: Danh sách xung đột được giới hạn theo library, phân trang keyset ổn
định theo `(detected_at, conflict_id)`, mặc định 100 và tối đa 1.000. Lịch sử
phân giải vẫn truy vấn được cục bộ. Công việc outbound độc lập trong cùng
library được trì hoãn có chủ đích cho tới khi dependency có biểu diễn bền vững;
library khác và toàn bộ inbound vẫn độc lập.

## Alternatives / Phương án khác

English: Last-write-wins, automatic server-wins/client-wins, rewriting the old
precondition, repeated 409 retries, and implicit conflict copies were rejected
because each hides divergence or can lose data. A per-node queue was rejected
until dependencies between rename, move, create, delete, and content intents
can be proven.

Tiếng Việt: Last-write-wins, tự động server-wins/client-wins, sửa precondition
cũ, lặp lại 409 và tự tạo conflict-copy bị loại vì che giấu phân kỳ hoặc có thể
mất dữ liệu. Queue theo node bị loại cho tới khi chứng minh được dependency giữa
rename, move, create, delete và content intent.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

English: SQLite migration `0006_sync_conflicts.sql` introduces the ledger and
resolution linkage; server schema and HTTP routes do not change. Revisit this
decision only when a durable dependency graph or an explicit product/UI policy
for merge/KeepBoth exists. Any replacement must retain the lossless and
no-silent-resolution invariants.

Tiếng Việt: Migration SQLite `0006_sync_conflicts.sql` thêm ledger và liên kết
phân giải; schema và route HTTP server không đổi. Chỉ xem xét lại khi có
dependency graph bền vững hoặc policy sản phẩm/UI tường minh cho merge/KeepBoth.
Mọi phương án thay thế phải giữ bất biến không mất dữ liệu và không phân giải
ngầm.
