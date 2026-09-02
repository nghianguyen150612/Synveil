# ADR-013: Opaque IDs, SHA-256 integrity, bounded dedup / ID mờ đục, SHA-256 và dedup có ranh giới

- Status / Trạng thái: **Accepted / Chấp thuận**
- Date / Ngày: 2026-08-21
- Owners / Chủ sở hữu: Domain, Storage, Security

## Context (English)

Public IDs, integrity hashes, physical keys, and deduplication identities solve
different problems. Collapsing them leaks content existence, couples APIs to a
layout, or turns a path/hash into accidental authorization.

## Decision (English)

Public domain records use UUIDv7 serialized as opaque lowercase canonical UUID
strings. Clients may compare IDs but not interpret their timestamp bits.
Canonical plaintext length plus SHA-256 is the initial integrity and whole-
object equality key. Stored representations also carry an independent stored-
byte checksum and encoding metadata. Physical object keys are server-generated,
opaque, versioned, and do not reveal a user path or content hash. Whole-object
reuse is limited to one owner/dedup domain by default; cross-user presence must
not be revealed through timing, quota, or API responses.

## Consequences (English)

Deduplication is an optimization after full verification, not proof based on a
client hash. A suspected SHA-256 collision or inconsistent length is quarantined
and compared byte-for-byte/treated as distinct rather than overwriting. Quotas
report logical usage separately from physical saved bytes. Hash/ID/format
algorithm changes need version fields and migration, not reinterpretation.

## Bối cảnh (Tiếng Việt)

Public ID, hash toàn vẹn, khóa vật lý và định danh dedup giải quyết vấn đề khác
nhau. Gộp chúng làm lộ sự tồn tại nội dung, ghép API với layout hoặc biến
path/hash thành phân quyền ngoài ý muốn.

## Quyết định (Tiếng Việt)

Bản ghi domain công khai dùng UUIDv7 dưới dạng chuỗi UUID chuẩn chữ thường nhưng
phải được coi là mờ đục. Client chỉ so sánh, không diễn giải bit thời gian. Kích
thước plaintext cùng SHA-256 là khóa toàn vẹn và bằng nhau toàn Object ban đầu.
Representation lưu trữ có checksum byte riêng và metadata encoding. Khóa vật lý
do server sinh, mờ đục, có phiên bản, không lộ path người dùng hay hash nội dung.
Mặc định chỉ tái dùng Object trong cùng owner/dedup domain; timing/quota/API
không được làm lộ sự hiện diện giữa người dùng.

## Hệ quả (Tiếng Việt)

Dedup là tối ưu sau xác minh đầy đủ, không tin hash do client gửi. Nếu nghi va
chạm SHA-256 hoặc kích thước không nhất quán, cách ly và so byte/giữ Object riêng
thay vì ghi đè. Quota báo dung lượng logic tách khỏi tiết kiệm vật lý. Đổi thuật
toán hash/ID/format cần trường phiên bản và migration, không diễn giải lại.

Alternatives rejected / Phương án loại bỏ: content hash as public object ID or
storage path; random IDs without time locality; global cross-owner dedup by
default; trusting client-provided hashes.
