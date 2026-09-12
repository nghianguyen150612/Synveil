# ADR-030: Journal retention floor and durable rebaseline handoff proofs / Floor retention journal và proof handoff rebaseline bền vững

- Status / Trạng thái: **Accepted / Chấp thuận — LOCKED**
- Date / Ngày: 2026-09-10
- Owners / Chủ sở hữu: Sync, Metadata

## Context (English)

An indefinitely offline device cannot pin every historical journal row without
making storage growth unbounded. A rebaseline snapshot is different: a client
may already have atomically applied its state at boundary `C` and still need to
complete the server checkpoint handoff. If cleanup discards events after `C`
while that handoff remains authorized, the client permanently misses changes.
Keeping every large snapshot entry solely to prove `C` is also wasteful.

## Decision (English)

The existing current-epoch `libraries.minimum_retained_sequence` value is the
durable **compacted-through** boundary. If it is `R`, every sequence through
`R` may be physically absent. A same-epoch cursor lower than `R` requires the
existing rebaseline result; a cursor exactly at `R` remains valid because the
feed returns only `sequence > R`. Compaction does not increment the epoch,
rewrite the journal head, or alter device checkpoints.

Gen 1 retains at least 30 days of incremental history. One invocation deletes
at most 10,000 rows from the oldest contiguous sequence prefix. Timestamp is an
eligibility policy only: an ineligible early sequence retains every later
sequence even if a later timestamp is older. The library namespace guard and
library clock/floor row serialize snapshot creation, append, feed, and cleanup.
Rows and floor advance in one transaction; feed returns either a complete old
view or a stale result under the new floor, with no retry.

Migration 36 adds one immutable
`rebaseline_snapshot_handoff_proofs` row per durable snapshot and backfills all
existing snapshots. New snapshot creation inserts header, entries, and proof in
one transaction. The proof contains only snapshot ID, owner, Library, journal
epoch/boundary, snapshot times, and its retention deadline; it contains no
physical storage identity. It has no cascading foreign key to the payload
header and therefore survives payload deletion.

Snapshot payload is logically unreadable at `observed_at >= expires_at` and may
then be physically removed in batches of at most 32 only if its proof exists.
Owner descriptor/page reads remain `SnapshotExpired` while the proof survives;
foreign reads remain concealed as `NotFound`. Handoff always derives `C` from
the proof and therefore works after payload deletion.

The proof deadline is 30 days after snapshot payload expiry. A retained proof
pins current-epoch compaction so `compacted_through <= C`; the oldest compatible
proof boundary wins. Proofs for another Library or epoch do not compare as raw
sequence numbers. At or after the deadline, at most 128 proofs whose payload is
already absent may be removed per invocation. Once removed, handoff is
`NotFound` and a later compaction may advance; Prompt 87 owns client recovery.

These are transport-neutral one-shot primitives only. There is no public
cleanup route, daemon, timer, scheduler, retry/backoff, conflict policy, client
recovery orchestration, or global lock.

## Bối cảnh và quyết định (Tiếng Việt)

Device offline vô thời hạn không thể pin toàn bộ lịch sử journal vì sẽ làm tăng
storage không giới hạn. Snapshot rebaseline khác: client có thể đã apply nguyên
tử state tại boundary `C` nhưng chưa hoàn tất handoff checkpoint server. Nếu
cleanup xóa event sau `C` khi handoff còn hợp lệ, client sẽ mất thay đổi vĩnh
viễn. Giữ toàn bộ entry snapshot lớn chỉ để chứng minh `C` cũng lãng phí.

Giá trị current-epoch `libraries.minimum_retained_sequence` hiện có được khóa
nghĩa là boundary **đã compact qua**. Với `R`, mọi sequence tới `R` có thể đã
vắng mặt vật lý. Cursor cùng epoch nhỏ hơn `R` phải nhận kết quả rebaseline hiện
có; cursor đúng `R` vẫn hợp lệ vì feed chỉ trả `sequence > R`. Compaction không
tăng epoch, sửa journal head hay thay đổi checkpoint device.

Gen 1 giữ tối thiểu 30 ngày lịch sử incremental. Mỗi invocation xóa tối đa
10.000 row trong prefix sequence liên tục cũ nhất. Timestamp chỉ quyết định đủ
tuổi: một sequence đầu chưa đủ tuổi sẽ giữ mọi sequence sau nó. Namespace guard
Library và row clock/floor Library serialize tạo snapshot, append, feed và
cleanup. Xóa row và tăng floor commit trong cùng transaction; feed chỉ thấy
toàn bộ view cũ hoặc stale theo floor mới, không có retry.

Migration 36 thêm một row `rebaseline_snapshot_handoff_proofs` bất biến cho mỗi
snapshot durable và backfill toàn bộ snapshot cũ. Tạo snapshot mới ghi header,
entry và proof trong cùng transaction. Proof chỉ chứa snapshot ID, owner,
Library, journal epoch/boundary, thời gian snapshot và deadline; không chứa
identity storage vật lý và không có FK cascade tới header payload.

Payload hết hiệu lực logic tại `observed_at >= expires_at` và chỉ được xóa vật
lý theo batch tối đa 32 khi proof tồn tại. Descriptor/page của owner vẫn trả
`SnapshotExpired` khi proof còn; owner khác vẫn nhận `NotFound`. Handoff luôn
đọc `C` từ proof nên vẫn thành công sau khi payload bị xóa.

Deadline proof là 30 ngày sau expiry payload. Proof còn giữ pin compaction
current epoch để `compacted_through <= C`; boundary tương thích nhỏ nhất thắng.
Proof của Library hay epoch khác không được so sequence như cùng clock. Tại/sau
deadline, mỗi invocation xóa tối đa 128 proof chỉ khi payload đã vắng mặt. Sau
đó handoff trả `NotFound`, compaction sau có thể tiến lên và Prompt 87 xử lý
recovery client.

Đây chỉ là primitive nội bộ one-shot. Không có public cleanup route, daemon,
timer, scheduler, retry/backoff, conflict policy, tự động recovery client hay
global lock.
