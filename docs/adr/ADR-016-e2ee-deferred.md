# ADR-016: Defer E2EE as a separate product mode / Hoãn E2EE như một chế độ sản phẩm riêng

- Status / Trạng thái: **Accepted / Chấp thuận**
- Date / Ngày: 2026-08-21
- Owners / Chủ sở hữu: Security, Storage, Product, Clients

## Context (English)

End-to-end/zero-knowledge encryption changes who can decrypt and therefore
changes server previews, compression, deduplication, OCR, semantic search,
sharing, recovery, key rotation, and multi-device onboarding. Adding encryption
fields without a full key and recovery protocol would create false privacy and
data-loss risk.

## Decision (English)

Early Synveil requires TLS and supports encryption at rest through encrypted
filesystems/backends/provider controls. E2EE is not an early feature and is not
implied by “user ownership.” If pursued, it is a separately designed library or
account mode with a reviewed threat model, standard primitives/protocol,
versioned envelope/chunk format, multi-device key exchange, recovery UX,
revocation limits, sharing design, metadata-leak analysis, migration, and
independent security review. The order for readable content is generally
compress then encrypt; ciphertext is not expected to compress or server-dedup.

## Consequences (English)

The server can initially provide previews, indexing, deduplication, and restore
under operator trust. Documentation must not call this zero knowledge. Future
E2EE users must see which features become client-side, unavailable, or leak
metadata. Convergent encryption is not adopted as a shortcut because it leaks
content equality and confirmation.

## Bối cảnh (Tiếng Việt)

Mã hóa đầu-cuối/zero-knowledge thay đổi bên có thể giải mã, vì vậy ảnh hưởng
preview, nén, dedup, OCR, semantic search, share, recovery, rotate key và kết nối
nhiều thiết bị. Chỉ thêm trường encryption mà không có giao thức key/recovery
đầy đủ tạo cảm giác riêng tư giả và nguy cơ mất dữ liệu.

## Quyết định (Tiếng Việt)

Synveil giai đoạn đầu bắt buộc TLS và hỗ trợ mã hóa at-rest qua filesystem/
backend/provider mã hóa. E2EE không phải tính năng sớm và không tự suy ra từ
“quyền sở hữu dữ liệu”. Nếu làm, đây là chế độ library/account thiết kế riêng
với threat model review, primitive/protocol chuẩn, format envelope/chunk có
phiên bản, trao đổi key nhiều thiết bị, UX recovery, giới hạn revoke, thiết kế
share, phân tích lộ metadata, migration và review bảo mật độc lập. Với nội dung
đọc được, thứ tự thường là nén rồi mã hóa; ciphertext không thể nén/dedup server
hiệu quả.

## Hệ quả (Tiếng Việt)

Ban đầu server có thể preview, index, dedup và restore trong mô hình tin operator.
Tài liệu không được gọi đây là zero knowledge. Người dùng E2EE tương lai phải
thấy tính năng nào chuyển client-side, bị mất hoặc lộ metadata. Không dùng
convergent encryption làm đường tắt vì nó làm lộ nội dung giống nhau/xác nhận
nội dung.

Alternatives rejected / Phương án loại bỏ: bolt-on E2EE; custom encryption;
convergent encryption for global dedup; claiming maximum server intelligence
and zero knowledge simultaneously.
