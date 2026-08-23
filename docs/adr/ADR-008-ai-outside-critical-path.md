# ADR-008: AI outside the critical path / AI nằm ngoài đường găng

- Status / Trạng thái: **Accepted / Chấp thuận**
- Date / Ngày: 2026-08-21
- Owners / Chủ sở hữu: AI, Architecture, Privacy

## Context (English)

OCR, embeddings, image understanding, and repository intelligence use different
runtimes, may be slow or unsafe to parse, and may be disabled. Making them part
of upload success would weaken availability and privacy.

## Decision (English)

Core content is validated, stored, and transactionally committed before an AI
job is published through the durable outbox. A separate Python worker produces
version-bound derived records. Modes are `DISABLED`, `LOCAL`, and explicitly
configured `REMOTE`. Remote egress requires informed administrator/user policy
and never occurs silently. Derived failure or stale results do not mutate or
block canonical file, sync, backup, download, or restore state.

## Consequences (English)

Search exposes index freshness and always retains non-AI metadata search.
Workers need sandboxing/resource limits for untrusted documents and idempotent
re-indexing. Deleting or revoking content schedules index removal with
observable lag. Remote providers need data-category, retention, credential,
audit, and deletion controls.

## Bối cảnh (Tiếng Việt)

OCR, embedding, hiểu ảnh và hiểu repository dùng runtime khác, có thể chậm hoặc
nguy hiểm khi parse, và có thể bị tắt. Đưa chúng vào điều kiện upload thành công
sẽ làm giảm độ sẵn sàng và quyền riêng tư.

## Quyết định (Tiếng Việt)

Nội dung lõi được kiểm tra, lưu và commit giao dịch trước khi job AI đi qua
outbox bền vững. Worker Python riêng tạo bản ghi dẫn xuất gắn với phiên bản.
Chế độ gồm `DISABLED`, `LOCAL` và `REMOTE` được cấu hình rõ ràng. Dữ liệu chỉ ra
ngoài khi admin/người dùng hiểu và cho phép, không bao giờ âm thầm. Lỗi hoặc chỉ
mục trễ không sửa/chặn trạng thái chuẩn của file, sync, backup, download, restore.

## Hệ quả (Tiếng Việt)

Search hiển thị độ mới chỉ mục và luôn giữ tìm kiếm metadata không AI. Worker
cần sandbox/giới hạn tài nguyên cho tài liệu không tin cậy và re-index
idempotent. Xóa/thu hồi nội dung tạo job xóa chỉ mục với độ trễ quan sát được.
Provider từ xa cần kiểm soát loại dữ liệu, retention, credential, audit và xóa.

Alternatives rejected / Phương án loại bỏ: synchronous indexing; mandatory
cloud model; AI as the only search; derived output replacing originals.
