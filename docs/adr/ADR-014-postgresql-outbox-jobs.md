# ADR-014: PostgreSQL transactional outbox and jobs / Outbox và job giao dịch trên PostgreSQL

- Status / Trạng thái: **Accepted / Chấp thuận**
- Date / Ngày: 2026-08-21
- Owners / Chủ sở hữu: Architecture, Workers, Database

## Context (English)

Core mutations must not be lost between database commit and optional work
publication. An early broker adds an independent durability/operations system
without eliminating the dual-write problem.

## Decision (English)

Persist required work in PostgreSQL in the same transaction as the domain
mutation. Workers claim rows with short transactions and skip-locked/lease
semantics, execute outside the claim transaction, then conditionally complete
using the lease generation. Delivery is at least once; every handler uses a
stable job identity and version-bound idempotency. Store attempts, next-run,
exponential backoff with jitter, lease expiry, priority, error class, and
terminal/dead-letter state. Separate client `ChangeEvent`, internal job, and
security `AuditEvent` contracts.

## Consequences (English)

Worker crashes cause retry, not loss. Poison jobs are visible and do not spin.
Long jobs renew bounded leases and cannot hold database transactions open.
Queue age and terminal failures are metrics/health inputs. A future broker may
fan out delivery from the outbox, but PostgreSQL remains the atomic handoff
until an ADR proves an equivalent guarantee.

## Bối cảnh (Tiếng Việt)

Không được mất công việc lõi ở khoảng giữa DB commit và publish việc tùy chọn.
Broker sớm tạo thêm hệ thống durability/vận hành nhưng không giải quyết dual
write.

## Quyết định (Tiếng Việt)

Lưu công việc bắt buộc trong PostgreSQL cùng giao dịch mutation domain. Worker
claim hàng bằng giao dịch ngắn với skip-locked/lease, chạy ngoài giao dịch claim,
rồi complete có điều kiện theo thế hệ lease. Delivery ít nhất một lần; handler
dùng job identity ổn định và idempotency gắn phiên bản. Lưu attempts, next-run,
backoff mũ có jitter, lease expiry, priority, loại lỗi và trạng thái cuối/dead-
letter. Tách hợp đồng `ChangeEvent` cho client, job nội bộ và `AuditEvent` bảo mật.

## Hệ quả (Tiếng Việt)

Worker crash tạo retry, không mất việc. Poison job phải nhìn thấy và không quay
vòng. Job dài gia hạn lease có giới hạn, không giữ transaction DB mở. Tuổi queue
và lỗi cuối là metric/health. Broker tương lai có thể fan-out từ outbox nhưng
PostgreSQL vẫn là điểm bàn giao nguyên tử đến khi ADR mới chứng minh tương đương.

Alternatives rejected / Phương án loại bỏ: in-memory queues; fire-and-forget
after commit; required Kafka/RabbitMQ/NATS/Redis in early phases.
