# ADR-015: Opaque sessions and scoped device credentials / Phiên mờ đục và credential thiết bị có scope

- Status / Trạng thái: **Accepted / Chấp thuận**
- Date / Ngày: 2026-08-21
- Owners / Chủ sở hữu: Security, Auth, Clients

## Context (English)

Browser sessions, automation, and long-lived devices need independent
revocation and audit. Self-hosters also need recovery that does not assume a
Synveil-operated email service. Stateless long-lived bearer JWTs make immediate
revocation and theft response harder.

## Decision (English)

Passwords use a vetted Argon2id library with versioned tunable parameters.
Browser sessions and device/API credentials are random high-entropy opaque
tokens; store only a keyed/cryptographic token hash, metadata, scopes, expiry,
rotation lineage, and revocation. Browser tokens travel only in `Secure`,
`HttpOnly`, scoped cookies with explicit CSRF defense. Device tokens use the
Authorization header and OS-protected credential storage. Each device is a
first-class identity; revoking it invalidates its credentials and future sync
without claiming an OS wipe. API/device credential and public-link issuance
uses an unusable pending generation, one-time secret display, and explicit
activation; rotation retains the old active generation until activation.
Browser refresh instead consumes-and-issues atomically and fails closed to a
fresh login when its response is ambiguous; it does not claim transparent
retry. Bootstrap is one-time. Each recovery-code set is printed once and stored
hashed; code exchange reserves the code, and password reset atomically consumes
both the current transaction and code. Optional mail/MFA/WebAuthn are additive
later.

## Consequences (English)

Every authenticated request performs server-side session/grant validation,
which can be cached only with bounded revocation lag explicitly documented.
Raw tokens never appear in logs or database reads after issuance. API/device
rotation is lost-response-safe through pending activation and a short auditable
old/candidate overlap. Browser refresh loss deterministically requires login;
reuse sets the family to `REVOKED`, invalidates its remaining credentials, and
records `REFRESH_REPLAY_DETECTED`; no quarantine state exists. Rate limits cover
account, source, device, share token, recovery exchange, and expensive operations.

## Bối cảnh (Tiếng Việt)

Phiên trình duyệt, automation và thiết bị sống lâu cần thu hồi/audit riêng.
Self-hoster cũng cần recovery không phụ thuộc dịch vụ email do Synveil vận hành.
Bearer JWT stateless sống lâu làm thu hồi ngay và ứng phó trộm token khó hơn.

## Quyết định (Tiếng Việt)

Mật khẩu dùng thư viện Argon2id đã kiểm chứng với tham số có phiên bản/điều chỉnh
được. Phiên trình duyệt và credential device/API là token ngẫu nhiên entropy cao
mờ đục; chỉ lưu hash mật mã, metadata, scope, expiry, chuỗi rotation và revoke.
Token trình duyệt chỉ đi trong cookie `Secure`, `HttpOnly`, scope hẹp với chống
CSRF rõ ràng. Token thiết bị dùng Authorization và kho credential của OS. Mỗi
thiết bị là identity hạng nhất; revoke làm mất credential và sync tương lai,
không tuyên bố xóa OS. Cấp credential API/device và public link dùng generation
pending chưa usable, hiển thị secret một lần và activation tường minh; rotation
giữ generation active cũ tới activation. Browser refresh thay vào đó
consume-and-issue nguyên tử và fail closed về login mới khi response mơ hồ; nó
không tuyên bố transparent retry. Bootstrap chỉ một lần. Mỗi recovery-code set
được hiển thị một lần và lưu hash; code exchange reserve code, còn reset
password consume nguyên tử cả current transaction và code. Mail/MFA/WebAuthn là
bổ sung sau.

## Hệ quả (Tiếng Việt)

Mỗi request xác thực cần kiểm tra session/grant phía server; cache chỉ được phép
có độ trễ revoke giới hạn và công bố. Raw token không vào log hoặc lần đọc DB
sau phát hành. Rotation API/device an toàn khi mất response qua pending
activation và overlap ngắn giữa old/candidate có thể audit. Mất browser refresh
buộc login một cách tất định; reuse đặt family thành `REVOKED`, invalidate
credential còn lại và ghi `REFRESH_REPLAY_DETECTED`; không có quarantine state.
Rate limit bao phủ account, nguồn, device, share token, recovery exchange và
thao tác đắt.

Alternatives rejected / Phương án loại bỏ: custom crypto; long-lived stateless
JWT as the only device credential; mandatory hosted email recovery.
