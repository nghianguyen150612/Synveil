# ADR-007: Backup is not synchronization / Backup không phải đồng bộ

- Status / Trạng thái: **Accepted / Chấp thuận**
- Date / Ngày: 2026-08-21
- Owners / Chủ sở hữu: Backup, Sync, Product

## Context (English)

Sync propagates current state, including deletion. Backup preserves historical
recoverability under retention. Reusing sync deletion semantics can turn a
local mistake or compromised client into destruction of the safety copy.

## Decision (English)

Model `BackupSet`, `BackupSnapshot`, and `BackupEntry` separately from live
`Node`/sync state. A client builds a manifest in `BUILDING`; only an atomically
`COMMITTED` snapshot is restorable and eligible as a retention anchor. Missing
source paths affect the new manifest but do not delete older entries. Retention
selects expired snapshots, then object GC considers all remaining references.
Restore defaults to a non-destructive destination and verifies checksums.

## Consequences (English)

The UI, APIs, policies, events, accounting, and tests must use distinct terms.
Unchanged contents can reuse immutable objects without coupling lifecycle.
Snapshot consistency is labeled `FILESYSTEM_CONSISTENT`, `CRASH_CONSISTENT`,
or `BEST_EFFORT`; the server cannot promise an atomic device scan it did not
receive.

## Bối cảnh (Tiếng Việt)

Sync lan truyền trạng thái hiện tại, kể cả xóa. Backup giữ khả năng phục hồi
lịch sử theo retention. Dùng ngữ nghĩa xóa của sync có thể biến thao tác nhầm
hoặc client bị chiếm quyền thành việc phá hủy bản an toàn.

## Quyết định (Tiếng Việt)

Mô hình hóa `BackupSet`, `BackupSnapshot`, `BackupEntry` tách khỏi `Node`/sync
trực tiếp. Client dựng manifest ở `BUILDING`; chỉ snapshot được commit nguyên tử
sang `COMMITTED` mới khôi phục được và làm mốc retention. Đường dẫn nguồn thiếu
chỉ ảnh hưởng manifest mới, không xóa entry cũ. Retention chọn snapshot hết hạn,
sau đó GC Object xét mọi tham chiếu còn lại. Restore mặc định đến đích không ghi
đè và xác minh checksum.

## Hệ quả (Tiếng Việt)

UI, API, policy, event, kế toán và test phải dùng thuật ngữ tách biệt. Nội dung
không đổi có thể dùng lại Object bất biến mà không ghép vòng đời. Tính nhất quán
snapshot được gắn nhãn `FILESYSTEM_CONSISTENT`, `CRASH_CONSISTENT` hoặc
`BEST_EFFORT`; server không hứa scan nguyên tử mà client không cung cấp.

Alternatives rejected / Phương án loại bỏ: backup as upload-only sync with
shared deletion rules; restorable partial snapshots; destructive restore by
default.
