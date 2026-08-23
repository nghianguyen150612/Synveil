# ADR-002: PostgreSQL for metadata / PostgreSQL cho siêu dữ liệu

- Status / Trạng thái: **Accepted / Chấp thuận**
- Date / Ngày: 2026-08-21
- Owners / Chủ sở hữu: Architecture, Database

## Context (English)

Nodes, versions, authorization, upload state, ordered changes, backup manifests,
jobs, and audit facts require constraints and multi-record transactions. Binary
contents are large and have different streaming/lifecycle requirements.

## Decision (English)

PostgreSQL is the source of truth for metadata and transactional state. Use
SQLx with reviewed forward migrations, explicit isolation/locking, foreign and
unique constraints, and transactionally persisted outbox work. Canonical file
contents normally live in `ObjectStore`, never as PostgreSQL BLOBs. Later
`pgvector` is optional derived-index storage, not canonical truth.

## Consequences (English)

One primary is the initial write authority. Read replicas may serve only stale-
tolerant queries. Database backup alone cannot restore files; operators need a
coordinated database, object-store, configuration, and secret recovery plan.
Released migrations are immutable and upgrades never assume a database wipe.

## Bối cảnh (Tiếng Việt)

`Node`, phiên bản, phân quyền, trạng thái upload, thay đổi có thứ tự, manifest
backup, job và audit cần ràng buộc cùng giao dịch nhiều bản ghi. Nội dung nhị
phân lớn có yêu cầu luồng và vòng đời khác.

## Quyết định (Tiếng Việt)

PostgreSQL là nguồn sự thật cho siêu dữ liệu và trạng thái giao dịch. Dùng SQLx
với migration tiến có review, isolation/lock rõ ràng, khóa ngoại/ràng buộc duy
nhất và outbox ghi cùng giao dịch. Nội dung tệp chuẩn nằm trong `ObjectStore`,
không lưu thông thường dưới dạng BLOB PostgreSQL. `pgvector` về sau chỉ là chỉ
mục dẫn xuất tùy chọn.

## Hệ quả (Tiếng Việt)

Ban đầu một primary có quyền ghi. Replica chỉ phục vụ truy vấn chấp nhận dữ liệu
trễ. Chỉ backup DB không thể khôi phục tệp; cần kế hoạch phối hợp DB, ObjectStore,
cấu hình và secret. Migration đã phát hành là bất biến và nâng cấp không được
giả định xóa DB.

Alternatives rejected / Phương án loại bỏ: SQLite as the production authority;
custom metadata database; storing normal file bodies in PostgreSQL.
