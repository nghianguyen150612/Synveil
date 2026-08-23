# ADR-017: Cross-platform deployment profiles and platform boundary / Profile deployment đa nền tảng và ranh giới platform

- Status / Trạng thái: **Accepted / Chấp thuận**
- Date / Ngày: 2026-08-22
- Owners / Chủ sở hữu: Architecture, Platform / Distribution, Release, Product

## Context (English)

Synveil's existing Compose topology is a sound advanced self-hosting target,
but treating it as the only eventual user experience would make Windows,
macOS, Linux Desktop, and non-technical users expensive retrofits. Native
installation also introduces OS service managers, permissions, update,
uninstall, and storage-discovery concerns that do not belong in the domain
core.

## Decision (English)

Define two packaging and operations profiles for one Synveil product:

- **Personal / Home Mode** uses a future native/guided installer, managed
  service lifecycle, safe storage selection, managed personal database
  deployment, one-time bootstrap, and guided device/health onboarding.
- **Advanced / Server Mode** keeps Docker Compose, external PostgreSQL, custom
  reverse proxies/TLS, S3/MinIO, NAS, manual networking, CLI, and detailed
  diagnostics.

Both profiles use the same Rust domain/application core, PostgreSQL metadata
authority, `ObjectStore`/capability model, API protocol, sync journal, backup
semantics, and security invariants. A platform/service port translates stable
operations such as start, stop, health, update, secret access, and storage
discovery to Windows Service, launchd, systemd, a user-session supervisor, or
an operator-managed Compose workflow. The domain core must not depend directly
on those platform APIs.

Windows, macOS, Linux Desktop, and Linux Server are first-class host targets;
Android, iPhone, and iPad are future first-class clients. A platform is not
supported until its install, lifecycle, health, update, uninstall-preserving-
data, backup/recovery, and declared filesystem/capability tests pass.

## Hệ quả (Tiếng Việt)

Topology Compose hiện có vẫn là target self-host nâng cao đúng đắn, nhưng coi
nó là trải nghiệm dài hạn duy nhất sẽ làm Windows, macOS, Linux Desktop và user
không chuyên trở thành retrofit tốn kém. Native installation còn thêm service
manager, permission, update, uninstall và storage discovery theo OS; các việc
này không thuộc domain core.

## Quyết định (Tiếng Việt)

Định nghĩa hai profile đóng gói/vận hành cho cùng một sản phẩm Synveil:

- **Personal / Home Mode** dùng native/guided installer tương lai, managed
  service lifecycle, chọn storage an toàn, managed database cá nhân, bootstrap
  một lần và onboarding device/health có hướng dẫn.
- **Advanced / Server Mode** giữ Docker Compose, PostgreSQL bên ngoài, reverse
  proxy/TLS tùy chỉnh, S3/MinIO, NAS, networking thủ công, CLI và diagnostics
  chi tiết.

Hai profile dùng chung Rust domain/application core, PostgreSQL authority,
`ObjectStore`/capability model, API protocol, sync journal, backup semantics và
security invariant. Platform/service port dịch các thao tác ổn định như start,
stop, health, update, secret access và storage discovery sang Windows Service,
launchd, systemd, supervisor user-session hoặc Compose do operator quản lý.
Domain core không phụ thuộc trực tiếp các API platform đó.

Windows, macOS, Linux Desktop và Linux Server là host target hạng nhất;
Android, iPhone và iPad là client hạng nhất trong tương lai. Chỉ support một
platform sau khi pass test install, lifecycle, health, update, uninstall-giữ-
dữ-liệu, backup/recovery và filesystem/capability đã công bố.

## Alternatives rejected / Phương án loại bỏ

Docker Compose as the only user-facing production path; separate incompatible
Personal and Server data models; platform-specific service calls from the
domain core; declaring a platform supported from compilation alone.

Compose là con đường production duy nhất; tách Personal và Server thành data
model không tương thích; gọi service API riêng của OS từ domain core; coi
platform đã support chỉ vì compile được.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

No data migration is introduced by this planning ADR. Before native service or
installer implementation, `PLATFORM.md` open decisions on managed PostgreSQL,
service supervision, update automation, and migration must be closed or the
implementation scope must remain a disposable prototype. Supersede this ADR if
the two profiles need different domain or protocol semantics.

Không có migration dữ liệu từ ADR quy hoạch này. Trước khi implement native
service/installer, phải đóng open decision trong `PLATFORM.md` về PostgreSQL
managed, service supervisor, update automation và migration; nếu chưa, scope
implementation chỉ được là prototype disposable. Thay thế ADR nếu hai profile
cần domain/protocol semantic khác nhau.
