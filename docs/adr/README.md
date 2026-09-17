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
| [023](ADR-023-linux-service-identity-and-filesystem-ownership.md) | Linux service identity and filesystem ownership / Identity dịch vụ Linux và ownership filesystem | Accepted / Chấp thuận — LOCKED Gen-1 |
| [024](ADR-024-linux-package-lifecycle-and-data-preserving-uninstall.md) | Linux package lifecycle and data-preserving uninstall / Vòng đời package Linux và uninstall bảo toàn dữ liệu | Accepted / Chấp thuận — LOCKED Gen-1 |
| [025](ADR-025-linux-runtime-credential-delivery.md) | Linux runtime credential delivery via systemd LoadCredential / Phân phối credential runtime Linux qua systemd LoadCredential | Accepted / Chấp thuận — LOCKED Gen-1 |
| [026](ADR-026-systemd-sandbox-hardening.md) | Systemd sandbox hardening for scheduled-maintenance service / Vỏ bọc systemd cho dịch vụ bảo trì lên lịch | Accepted / Chấp thuận — LOCKED Gen-1 |
| [027](ADR-027-sync-rebaseline-snapshot-consistency.md) | Sync rebaseline snapshot consistency boundary / Boundary nhất quán snapshot rebaseline sync | Accepted / Chấp thuận — LOCKED |
| [028](ADR-028-durable-rebaseline-snapshot-materialization.md) | Durable rebaseline snapshot materialization and paging / Materialize và phân trang snapshot rebaseline durable | Accepted / Chấp thuận — LOCKED |
| [029](ADR-029-client-rebaseline-atomic-apply.md) | Client rebaseline atomic apply and outbound-intent preservation / Apply rebaseline client nguyên tử và bảo toàn outbound intent | Accepted / Chấp thuận — LOCKED |
| [030](ADR-030-journal-retention-floor-and-durable-rebaseline-handoff-proofs.md) | Journal retention floor and durable rebaseline handoff proofs / Floor retention journal và proof handoff rebaseline bền vững | Accepted / Chấp thuận — LOCKED |
| [031](ADR-031-automatic-retained-cursor-rebaseline-convergence.md) | Automatic retained-cursor rebaseline convergence / Hội tụ rebaseline tự động khi retained-cursor không còn hợp lệ | Accepted / Chấp thuận — LOCKED |
| [032](ADR-032-deterministic-sync-conflict-preservation.md) | Deterministic sync conflict preservation and explicit resolution / Bảo toàn xung đột đồng bộ tất định và phân giải tường minh | Accepted / Chấp thuận — LOCKED |
| [033](ADR-033-bounded-bidirectional-sync-cycle.md) | Bounded bidirectional synchronization cycle / Chu kỳ đồng bộ hai chiều bounded | Accepted / Chấp thuận — LOCKED |
| [034](ADR-034-long-running-sync-runtime.md) | Long-running synchronization runtime lifecycle and scheduling / Lifecycle và scheduling runtime đồng bộ chạy dài | Accepted / Chấp thuận — LOCKED |
| [035](ADR-035-durable-change-first-runtime-signal-integration.md) | Durable-change-first runtime signal integration / Tích hợp signal runtime theo thứ tự durable-change trước | Accepted / Chấp thuận — LOCKED |
| [036](ADR-036-desktop-sync-host-and-process-lifecycle.md) | Desktop synchronization host and process lifecycle composition / Host đồng bộ desktop và composition lifecycle tiến trình | Accepted / Chấp thuận — LOCKED |
| [037](ADR-037-desktop-process-bootstrap-platform-lifecycle-root-availability.md) | Production desktop process bootstrap, platform lifecycle, and root availability / Bootstrap process desktop production, lifecycle platform và availability của root | Accepted / Chấp thuận — LOCKED |
| [038](ADR-038-secure-local-desktop-control-ipc.md) | Secure local desktop process-control IPC / IPC điều khiển tiến trình desktop cục bộ an toàn | Accepted / Chấp thuận — LOCKED |
| [039](ADR-039-ipc-backed-desktop-controller-core.md) | IPC-backed desktop controller core / Core controller desktop dựa trên IPC | Accepted / Chấp thuận — LOCKED |
| [040](ADR-040-native-qt-desktop-shell.md) | Native Qt 6/QML desktop shell and system tray / Shell desktop native Qt 6/QML và system tray | Accepted / Chấp thuận — LOCKED Prompt 98 |
| [041](ADR-041-production-desktop-launch-orchestration.md) | Production desktop launch orchestration and user-level background supervision / Điều phối khởi chạy desktop production và giám sát background theo user | Accepted — LOCKED |

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
