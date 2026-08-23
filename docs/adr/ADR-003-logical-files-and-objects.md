# ADR-003: Separate logical files from stored objects / Tách tệp logic khỏi Object lưu trữ

- Status / Trạng thái: **Accepted / Chấp thuận**
- Date / Ngày: 2026-08-21
- Owners / Chủ sở hữu: Storage, Domain

## Context (English)

Names, folders, shares, trash, and sync state change independently from large
immutable byte sequences. Binding a user path directly to a physical path makes
rename expensive and weakens version reuse, backup, deduplication, and safe GC.

## Decision (English)

Use `Node → FileVersion → Object → ObjectReplica`. `Node` is the stable
user-visible identity and hierarchy entry. Each immutable `FileVersion`
references one canonical immutable `Object`: the backend-neutral identity of
verified plaintext within a dedup domain. One or more `ObjectReplica` records
bind that identity to an immutable encoded representation, backend, and opaque
physical key. Keys are never derived from user paths. Rename/move update
metadata; restore creates a new current state; history is not rewritten. Object
deletion requires proof that no protected reference or replica/operation lease
exists.

## Consequences (English)

Logical and physical accounting differ. One object may support multiple
versions and multiple physical replicas inside the allowed dedup domain.
Authorization begins at a logical resource/reference and cannot be inferred
from an object or replica ID. Reconciliation and grace-period GC are mandatory
because database and object store do not share a transaction.

## Bối cảnh (Tiếng Việt)

Tên, thư mục, share, trash và trạng thái sync thay đổi độc lập với chuỗi byte
lớn bất biến. Gắn đường dẫn người dùng trực tiếp với đường dẫn vật lý làm rename
tốn kém và phá khả năng dùng lại phiên bản, backup, dedup và GC an toàn.

## Quyết định (Tiếng Việt)

Dùng `Node → FileVersion → Object → ObjectReplica`. `Node` là định danh ổn định
người dùng nhìn thấy trong cây thư mục. Mỗi `FileVersion` bất biến tham chiếu một
`Object` chuẩn bất biến: identity không phụ thuộc backend của plaintext đã
verify trong một dedup domain. Một hoặc nhiều `ObjectReplica` gắn identity đó
với representation encoded bất biến, backend và physical key mờ đục. Key không
sinh từ đường dẫn người dùng. Rename/move chỉ đổi metadata; restore tạo trạng
thái hiện tại mới; không viết lại lịch sử. Chỉ xóa Object khi chứng minh không
còn tham chiếu được bảo vệ hoặc lease replica/operation.

## Hệ quả (Tiếng Việt)

Dung lượng logic và vật lý khác nhau. Một Object có thể phục vụ nhiều phiên bản
và nhiều replica vật lý trong miền dedup cho phép. Phân quyền bắt đầu từ tài
nguyên/tham chiếu logic, không suy ra từ Object ID hay replica ID. Bắt buộc đối
soát và GC có thời gian an toàn vì DB và ObjectStore không chung giao dịch.

Alternatives rejected / Phương án loại bỏ: user path equals storage path; mutable
objects; rewriting old versions during restore.
