# Synveil Architecture Decision Records / Biên bản quyết định kiến trúc

ADRs are the highest-level design authority below compatibility obligations
created by released data. Every ADR contains English and Vietnamese in one file
so its status and consequences cannot diverge between editions.

ADR là cấp thẩm quyền thiết kế cao nhất, chỉ đứng sau nghĩa vụ tương thích do dữ
liệu đã phát hành tạo ra. Mỗi ADR chứa cả tiếng Anh và tiếng Việt trong cùng
một tệp để trạng thái và hệ quả không bị lệch giữa hai bản.

## Status / Trạng thái

- `Proposed / Đề xuất`: not yet authority; implementation must not depend on it.
- `Accepted / Chấp thuận`: normative until superseded.
- `Deprecated / Không còn khuyến nghị`: retained for history; do not use anew.
- `Superseded / Bị thay thế`: replaced by a named later ADR.

## Index / Mục lục

| ADR | Decision / Quyết định | Status / Trạng thái |
|---|---|---|
| [001](ADR-001-rust-modular-monolith.md) | Rust modular monolith / Khối mô-đun bằng Rust | Accepted / Chấp thuận |
| [002](ADR-002-postgresql-metadata.md) | PostgreSQL metadata / Siêu dữ liệu PostgreSQL | Accepted / Chấp thuận |
| [003](ADR-003-logical-files-and-objects.md) | Logical files vs objects / Tệp logic và Object | Accepted / Chấp thuận |
| [004](ADR-004-object-store-abstraction.md) | ObjectStore abstraction / Lớp trừu tượng ObjectStore | Accepted / Chấp thuận |
| [005](ADR-005-resumable-uploads.md) | Resumable upload state machine / Máy trạng thái tải lên nối tiếp | Accepted / Chấp thuận |
| [006](ADR-006-change-journal-sync.md) | Change-journal sync / Đồng bộ bằng nhật ký thay đổi | Accepted / Chấp thuận |
| [007](ADR-007-backup-is-not-sync.md) | Backup separated from sync / Tách backup khỏi sync | Accepted / Chấp thuận |
| [008](ADR-008-ai-outside-critical-path.md) | AI outside critical path / AI ngoài đường găng | Accepted / Chấp thuận |
| [009](ADR-009-react-typescript-vite.md) | React + TypeScript + Vite | Accepted / Chấp thuận |
| [010](ADR-010-monorepo.md) | Initial monorepo / Monorepo ban đầu | Accepted / Chấp thuận |
| [011](ADR-011-open-source-licensing.md) | Open-source licensing / Giấy phép nguồn mở | Proposed / Đề xuất |
| [012](ADR-012-forgejo-integration.md) | Forgejo integration / Tích hợp Forgejo | Accepted / Chấp thuận |
| [013](ADR-013-identifiers-hashes-dedup-domain.md) | IDs, hashes, dedup domain / ID, hàm băm, miền dedup | Accepted / Chấp thuận |
| [014](ADR-014-postgresql-outbox-jobs.md) | PostgreSQL outbox/jobs / Outbox và job trên PostgreSQL | Accepted / Chấp thuận |
| [015](ADR-015-credential-model.md) | Session and device credentials / Thông tin xác thực phiên và thiết bị | Accepted / Chấp thuận |
| [016](ADR-016-e2ee-deferred.md) | E2EE deferred as a product mode / Hoãn E2EE như một chế độ sản phẩm | Accepted / Chấp thuận |
| [017](ADR-017-cross-platform-deployment-profiles.md) | Cross-platform deployment profiles / Profile deployment đa nền tảng | Accepted / Chấp thuận |
| [018](ADR-018-filesystem-capability-acceleration.md) | Filesystem capability acceleration / Tăng tốc filesystem theo capability | Accepted / Chấp thuận |
| [019](ADR-019-managed-postgresql-lifecycle.md) | Managed PostgreSQL lifecycle / Vòng đời PostgreSQL managed | Proposed / Đề xuất |
| [020](ADR-020-self-hosted-first-remote-access.md) | Self-hosted-first remote access / Remote access ưu tiên self-hosted | Proposed / Đề xuất |
| [021](ADR-021-safe-update-uninstall-migration.md) | Safe update, uninstall, and migration / Update, uninstall và migration an toàn | Accepted / Chấp thuận |
| [022](ADR-022-progressive-disclosure-complexity-boundary.md) | Progressive disclosure / Progressive disclosure và ranh giới phức tạp | Accepted / Chấp thuận |

## Process / Quy trình

Use the template below. Never edit the conclusion of an accepted ADR to hide a
change; add a superseding ADR. Clarifications that do not alter the decision
may be appended with a date.

Dùng mẫu dưới đây. Không sửa kết luận của ADR đã chấp thuận để che giấu thay
đổi; hãy tạo ADR mới thay thế. Có thể bổ sung diễn giải không làm đổi quyết định
kèm ngày.

```text
# ADR-NNN: title
Status / Trạng thái
Date / Ngày
Decision owners / Chủ sở hữu quyết định
Context / Bối cảnh
Decision / Quyết định
Consequences / Hệ quả
Alternatives / Phương án khác
Migration and review trigger / Điều kiện di chuyển và xem xét lại
```
