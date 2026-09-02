# ADR-005: Server-coordinated resumable uploads / Tải lên nối tiếp do server điều phối

- Status / Trạng thái: **Accepted / Chấp thuận**
- Date / Ngày: 2026-08-21
- Owners / Chủ sở hữu: Uploads, Storage, API

## Context (English)

Large uploads cross unreliable networks and cannot be held in memory. Retrying
a completed request after a lost response must not create another version.
Database and object storage cannot commit atomically together.

## Decision (English)

Use a persisted public state machine:
`OPEN → VERIFYING → COMMITTING → COMMITTED`, with terminal `ABORTED`, `EXPIRED`,
or `FAILED`. `ASSEMBLING`, `HASHING`, `FINALIZING`, and `READBACK_VERIFY` are
persisted internal recovery phases within `VERIFYING`, not public states.
Initiation fixes destination intent, size/checksum expectations,
part policy, expiry, owner, and an idempotency key. Parts are streamed with
bounded memory and verified individually. Completion is serialized, assembles
or finalizes one immutable object, verifies canonical SHA-256/length, then
atomically creates metadata, journal, audit, and outbox records. The terminal
result is persisted and replayed for duplicate completion. An authorized
cancel may move `OPEN`, `VERIFYING`, or pre-commit `COMMITTING` to `ABORTED`
only when its generation-checked row transition wins; once the logical commit
wins, `COMMITTED` is final and cancellation never deletes the file.

## Consequences (English)

Uploaded bytes are staging, not visible files. Durable bytes written before a
failed DB commit are safe orphan candidates and cannot be deleted until leases
and grace expire. Clients can query parts/status and resume. Expiry cleanup is
idempotent. Implementations need limits on sessions, parts, size, concurrency,
CPU hashing, and temporary capacity.

## Bối cảnh (Tiếng Việt)

Upload lớn đi qua mạng không ổn định và không thể giữ trong RAM. Retry sau khi
server đã commit nhưng mất response không được tạo phiên bản thứ hai. DB và
ObjectStore không thể commit nguyên tử cùng nhau.

## Quyết định (Tiếng Việt)

Dùng máy trạng thái public lưu bền:
`OPEN → VERIFYING → COMMITTING → COMMITTED`, với trạng thái cuối `ABORTED`,
`EXPIRED` hoặc `FAILED`. `ASSEMBLING`, `HASHING`, `FINALIZING` và
`READBACK_VERIFY` là phase recovery nội bộ được lưu trong `VERIFYING`, không
phải trạng thái public. Khởi tạo cố định đích, kích thước/checksum dự kiến,
quy tắc part, hạn dùng, owner và idempotency key. Part được stream bằng bộ nhớ
có giới hạn và kiểm tra riêng. Complete được tuần tự hóa, tạo một Object bất
biến, xác minh SHA-256/kích thước chuẩn rồi tạo metadata, journal, audit và
outbox trong một giao dịch. Kết quả cuối được lưu để trả lại khi complete lặp.
Cancel đã authorize chỉ có thể chuyển `OPEN`, `VERIFYING` hoặc `COMMITTING`
trước commit sang `ABORTED` khi transition row có kiểm tra generation thắng;
khi logical commit đã thắng, `COMMITTED` là cuối và cancel không bao giờ xóa
file.

## Hệ quả (Tiếng Việt)

Byte đã upload chỉ là staging, chưa phải tệp nhìn thấy. Byte bền vững nhưng DB
commit thất bại là ứng viên orphan an toàn và chỉ được xóa sau khi lease cùng
thời gian đệm hết hạn. Client có thể hỏi part/trạng thái và tiếp tục. Cleanup
hết hạn phải idempotent. Cần giới hạn session, part, kích thước, đồng thời, CPU
băm và dung lượng tạm.

Alternatives rejected / Phương án loại bỏ: one in-memory request; client-chosen
physical keys; last-request-wins completion; DB reference before durable bytes.
