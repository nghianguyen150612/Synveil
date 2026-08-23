# ADR-004: Capability-aware ObjectStore / ObjectStore nhận biết năng lực

- Status / Trạng thái: **Accepted / Chấp thuận**
- Date / Ngày: 2026-08-21
- Owners / Chủ sở hữu: Storage

## Context (English)

Synveil must support local filesystems first and later NAS mounts, MinIO, S3-
compatible systems, and future adapters. Filesystem rename/listing semantics do
not generalize to object services.

## Decision (English)

Define a streaming `ObjectStore` port for staged creation, immutable final
read/range read, metadata/head, abort, and controlled delete. Adapters declare
capabilities such as atomic promotion, conditional create, multipart support,
checksum metadata, read-after-write, and listing consistency. Application logic
uses generated opaque keys and may not assume rename. The local adapter is the
first production target; S3/MinIO is a later conformance-tested adapter.

## Consequences (English)

The contract is slightly richer than a lowest-common-denominator CRUD API.
Every adapter must pass the same durability, range, retry, corruption, and
orphan tests. Storage migration is copy, verify, transactional location switch,
rollback window, and later retirement—not changing a path string.

## Bối cảnh (Tiếng Việt)

Synveil cần hỗ trợ filesystem cục bộ trước, sau đó NAS mount, MinIO, S3 tương
thích và adapter tương lai. Ngữ nghĩa rename/listing của filesystem không áp
dụng chung cho object service.

## Quyết định (Tiếng Việt)

Định nghĩa port `ObjectStore` dạng luồng cho tạo staging, đọc/range-read bản bất
biến, head metadata, hủy và xóa có kiểm soát. Adapter khai báo năng lực như
promote nguyên tử, conditional create, multipart, checksum, read-after-write và
tính nhất quán listing. Logic ứng dụng dùng khóa mờ đục sinh bởi server, không
được giả định có rename. Adapter local là mục tiêu production đầu; S3/MinIO đến
sau với bộ test tuân thủ.

## Hệ quả (Tiếng Việt)

Hợp đồng chi tiết hơn CRUD mẫu số chung thấp nhất. Mọi adapter phải qua cùng bộ
test durability, range, retry, corruption và orphan. Di chuyển storage phải
copy, verify, chuyển location bằng giao dịch, chờ cửa sổ rollback rồi mới bỏ bản
cũ—không chỉ đổi chuỗi đường dẫn.

Alternatives rejected / Phương án loại bỏ: direct filesystem calls throughout
the domain; pretending S3 rename is atomic; requiring S3 for the first release.
