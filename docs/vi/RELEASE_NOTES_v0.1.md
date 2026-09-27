# Ghi chú phát hành Synveil v0.1

Synveil v0.1 là slice đầu tiên hướng tới self-hosting và desktop sync trong
release. Nó cung cấp PostgreSQL-backed server/API foundation và kiến trúc
desktop native hai process cho Linux và Windows được hỗ trợ.

## Khả năng người dùng thấy được

- API self-host dùng PostgreSQL với profile/library authenticate, logical
  file/folder metadata, health/readiness probe, upload/download, version
  history và bounded sync/change-feed protocol.
- Qt UI/control surface native `synveil-desktop` đi cùng process sync độc lập
  `synveil-client`.
- Profile onboarding, HTTPS readiness verification, authentication và device
  credential lưu trong SecretStore của OS.
- Library setup với local folder do user chọn; folder ordinary không rỗng có
  sẵn được admit an toàn thành local content ban đầu.
- Nền tảng bidirectional sync với durable local state, pause/resume, recovery
  bounded và conflict attention bền vững.
- Conflict action explicit khi được hỗ trợ: **Accept Remote** và
  **Retry Local** theo current base.
- Xử lý an toàn root unavailable, pending setup, client/server recovery và
  response ambiguous mà không blind replay hay mass deletion.
- Policy package DEB/RPM Linux và ZIP portable Windows. Linux dùng user
  systemd client unit; Windows dùng current-user Task Scheduler khi user bật.

## Ghi chú tương thích

v0.1 chưa có public compatibility contract trước đó. Upgrade được hỗ trợ theo
frozen migration và durable-state contract trong `docs/vi/UPGRADE_SAFETY.md`,
không phải downgrade matrix tùy ý giữa client/server lịch sử. Schema tương lai
không biết sẽ fail closed, migration forward-only và không đảm bảo automatic
downgrade.

## Giới hạn quan trọng

- Desktop package không đóng gói PostgreSQL hoặc server installer.
- macOS và mobile hoãn, không hỗ trợ trong v0.1.
- Native Windows runtime acceptance là gate riêng; cross-build không phải
  native Windows validation.
- Package signing, repository publication, auto-update và rollback package
  transaction đầy đủ nằm ngoài contract release này.
- Khác biệt filename semantics và reserved/unsupported name có thể bị reject
  an toàn thay vì sync.

Đọc [hướng dẫn vận hành](RELEASE_OPERATIONS.md) trước khi cài/upgrade và
[package policy](RELEASE_PACKAGING.md) để xem artifact detail.
