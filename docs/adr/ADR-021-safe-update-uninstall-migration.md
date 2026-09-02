# ADR-021: Safe update, uninstall, and migration lifecycle / Vòng đời update, uninstall và migration an toàn

- Status / Trạng thái: **Accepted / Chấp thuận**
- Date / Ngày: 2026-08-22
- Owners / Chủ sở hữu: Release, Backup / Recovery, Security, Product

## Context (English)

Making installation easy increases the chance that users will accept updates,
remove an application, or move to a new computer without understanding the
database/object-store boundary. An installer that treats application removal,
database reset, or a failed migration as the same operation can destroy the
user's cloud.

## Decision (English)

The lifecycle is data-preserving by default:

- releases are signed and verified; updates coordinate services, gate database
  migrations, require a verified backup before dangerous changes, and document
  rollback limits and failed-update recovery;
- application binaries, configuration, PostgreSQL state, stored objects,
  cache, logs, credentials, and backup copies have separate uninstall choices;
- “remove application” does not delete the cloud; permanent data deletion is a
  separate, explicit destructive action;
- machine migration follows `inspect → plan → validate → execute → verify`,
  transfers database/object/configuration/key state through verified procedures,
  and explicitly handles device identity, hostname/TLS, remote access, and
  rollback/coexistence.

Compose and advanced deployments may retain administrator-controlled upgrade
flows, but they must obey the same data-safety and compatibility invariants.

## Hệ quả (Tiếng Việt)

Làm installation dễ hơn cũng làm user dễ chấp nhận update, gỡ application hoặc
chuyển máy mà không hiểu ranh giới database/object-store. Installer coi remove
application, reset database và migration thất bại là một thao tác có thể phá
cloud của user.

## Quyết định (Tiếng Việt)

Lifecycle mặc định phải bảo toàn dữ liệu:

- release phải có chữ ký và được verify; update phối hợp service, gate
  migration database, yêu cầu backup đã verify trước thay đổi nguy hiểm và ghi
  rõ rollback limit/failed-update recovery;
- binary application, configuration, PostgreSQL state, stored object, cache,
  log, credential và backup copy có lựa chọn uninstall riêng;
- “remove application” không xóa cloud; xóa data vĩnh viễn là destructive
  action riêng, phải xác nhận rõ;
- machine migration theo `inspect → plan → validate → execute → verify`, chuyển
  database/object/configuration/key qua procedure đã verify và xử lý tường minh
  device identity, hostname/TLS, remote access, rollback/coexistence.

Compose và deployment nâng cao có thể giữ upgrade do admin điều khiển nhưng vẫn
phải tuân invariant data-safety và compatibility.

## Alternatives rejected / Phương án loại bỏ

Unsigned auto-update; automatic database reset on failed upgrade; uninstall
that deletes the data directory by default; migration by copying only a database
or only an object directory; claiming rollback after an irreversible migration.

Auto-update không ký; tự reset database khi upgrade lỗi; uninstall mặc định xóa
data directory; migration chỉ copy database hoặc object directory; claim
rollback sau migration không đảo ngược.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

Every installer/release design must map its actions to this lifecycle and pass
the backup, restore, upgrade, uninstall, and clean-destination migration tests
in `TESTING.md` before a platform is called supported. A format-changing
exception requires a superseding ADR and reader-before-writer/restore proof.

Mọi thiết kế installer/release phải map action vào lifecycle này và pass test
backup, restore, upgrade, uninstall và migration trên destination sạch trong
`TESTING.md` trước khi gọi platform là supported. Ngoại lệ format-changing cần
ADR thay thế cùng bằng chứng reader-before-writer/restore.
