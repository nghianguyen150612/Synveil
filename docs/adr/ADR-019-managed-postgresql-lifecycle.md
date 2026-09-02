# ADR-019: Managed PostgreSQL lifecycle for personal deployments / Vòng đời PostgreSQL managed cho deployment cá nhân

- Status / Trạng thái: **Proposed / Đề xuất**
- Date / Ngày: 2026-08-22
- Owners / Chủ sở hữu: Database, Release, Security, Product

## Context (English)

PostgreSQL remains the correct metadata and transactional authority under
ADR-002, but asking ordinary users to install and administer it defeats the
Personal / Home Mode goal. Replacing it with SQLite would create two data
models, migration paths, and correctness profiles merely to avoid packaging
work.

## Proposed decision (English)

Keep one canonical PostgreSQL model. Add a future managed-database adapter for
supported Personal / Home installations. It may provision a private bundled,
packaged, or system-managed PostgreSQL service; initialize least-privilege
roles; run migrations; supervise startup/shutdown; monitor health; coordinate
backup/restore; and perform compatible upgrade/recovery operations. Advanced /
Server Mode continues to accept external operator-managed PostgreSQL.

The adapter must not put database administration in the domain core, expose
database credentials to the normal UI, or imply that uninstall deletes data.
The exact provisioning/distribution choice remains the `OPEN DECISION`
OD-PLAT-001 in `PLATFORM.md`.

## Bối cảnh (Tiếng Việt)

PostgreSQL vẫn là authority đúng cho metadata và transaction theo ADR-002,
nhưng bắt user thường tự cài/quản trị nó sẽ phá mục tiêu Personal / Home Mode.
Thay bằng SQLite sẽ tạo hai data model, migration path và correctness profile
chỉ để né công việc đóng gói.

## Quyết định đề xuất (Tiếng Việt)

Giữ một PostgreSQL model canonical. Thêm managed-database adapter tương lai cho
Personal / Home được hỗ trợ. Adapter có thể provision PostgreSQL private,
bundled, packaged hoặc system-managed; khởi tạo role least-privilege, chạy
migration, quản lý startup/shutdown, theo dõi health, phối hợp backup/restore và
thực hiện upgrade/recovery tương thích. Advanced / Server Mode tiếp tục nhận
PostgreSQL bên ngoài do operator quản lý.

Adapter không đưa database administration vào domain core, không lộ database
credential trong UI thường và không ngụ ý uninstall sẽ xóa dữ liệu. Lựa chọn
provision/distribution cụ thể vẫn là `OPEN DECISION` OD-PLAT-001 trong
`PLATFORM.md`.

## Consequences and open questions / Hệ quả và câu hỏi mở

The single authority preserves storage, backup, migration, and transaction
correctness, but bundled/private PostgreSQL affects installer size, platform
support, security patching, data-directory ownership, uninstall, and recovery.
The decision must evaluate bundled/private distribution, system dependency,
packaged service, upgrade compatibility, database backup/restore, Windows and
macOS support, and security boundaries before implementation.

Giữ một authority duy nhất bảo toàn correctness storage, backup, migration và
transaction, nhưng PostgreSQL bundled/private ảnh hưởng kích thước installer,
support platform, security patch, ownership data directory, uninstall và
recovery. Phải đánh giá các lựa chọn bundled/private, system dependency,
packaged service, upgrade compatibility, backup/restore DB, Windows/macOS và
security boundary trước implementation.

## Alternatives / Phương án khác

SQLite as a second production authority; requiring manual PostgreSQL setup in
Personal / Home Mode; silently depending on a proprietary hosted database.

SQLite như authority production thứ hai; bắt Personal / Home tự cài PostgreSQL;
phụ thuộc âm thầm vào database hosted độc quyền.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

This proposal must be accepted or superseded before native Personal / Home
database implementation. No SQLite schema, dual-driver code, or installer
behavior may be introduced to “test” the choice without a new reviewed ADR.

Đề xuất phải được chấp thuận hoặc thay thế trước khi implement database native
cho Personal / Home. Không thêm SQLite schema, dual-driver code hay installer
behavior để “thử” lựa chọn nếu chưa có ADR mới được review.
