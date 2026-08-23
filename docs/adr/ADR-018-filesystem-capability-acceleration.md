# ADR-018: Filesystem capability acceleration / Tăng tốc filesystem theo capability

- Status / Trạng thái: **Accepted / Chấp thuận**
- Date / Ngày: 2026-08-22
- Owners / Chủ sở hữu: Storage, Architecture, Clients, Security

## Context (English)

ADR-004 already requires a capability-aware `ObjectStore`, but a local adapter
can still accidentally turn Linux/Btrfs behavior into a product requirement.
Synveil must work on ordinary Windows, macOS, Linux, NAS-mounted, and object
storage configurations. Filesystem snapshots, copy-on-write, compression, and
checksums have different names and guarantees across platforms.

## Decision (English)

Introduce a capability declaration boundary:

```text
StorageBackend → StorageCapabilities → storage policy/optimization
```

Capabilities include, where proven by adapter tests, `reflink`, `block_clone`,
`copy_on_write_clone`, `native_snapshot`, `compression`, `checksumming`,
`sparse_files`, `atomic_rename`, `durable_fsync`, `range_reads`, and
`filesystem_health`. Synveil correctness logic uses portable operations and
explicit capability checks. Filesystem-specific behavior is an accelerator,
never the definition of file identity, versioning, sync, backup, retention,
integrity, deduplication, or restore.

Btrfs is an optional advanced Linux optimization and may be recommended for a
profile. WinBtrfs is optional/experimental/community-oriented. Neither is
required for Windows or Linux support. NTFS, ReFS, APFS, ext4, XFS, supported
future ZFS, generic local filesystems, NAS mounts, and object stores remain
within the planned correctness model subject to adapter conformance.

## Hệ quả (Tiếng Việt)

ADR-004 đã yêu cầu `ObjectStore` nhận biết capability, nhưng local adapter vẫn
có thể vô tình biến hành vi Linux/Btrfs thành yêu cầu sản phẩm. Synveil phải
chạy trên cấu hình Windows, macOS, Linux, NAS mount và object storage thông
thường. Snapshot filesystem, copy-on-write, compression và checksum có tên gọi
và guarantee khác nhau giữa platform.

## Quyết định (Tiếng Việt)

Thêm ranh giới khai báo capability:

```text
StorageBackend → StorageCapabilities → storage policy/optimization
```

Capability có thể gồm `reflink`, `block_clone`, `copy_on_write_clone`,
`native_snapshot`, `compression`, `checksumming`, `sparse_files`,
`atomic_rename`, `durable_fsync`, `range_reads` và `filesystem_health` khi
adapter đã chứng minh bằng test. Logic correctness của Synveil dùng thao tác
portable và kiểm tra capability tường minh. Hành vi riêng filesystem chỉ là
accelerator, không định nghĩa file identity, version, sync, backup, retention,
integrity, dedup hay restore.

Btrfs là tối ưu Linux nâng cao tùy chọn và có thể được khuyến nghị cho một
profile. WinBtrfs là tùy chọn/experimental/community-oriented. Không cái nào
là yêu cầu để support Windows hay Linux. NTFS, ReFS, APFS, ext4, XFS, ZFS được
support về sau, filesystem local generic, NAS mount và object store vẫn nằm
trong correctness model theo conformance của adapter.

## Alternatives rejected / Phương án loại bỏ

Requiring Btrfs or WinBtrfs; using filesystem snapshots as Synveil backup;
exposing adapter-specific behavior in domain invariants; silently copying a
weak capability into a stronger guarantee.

Bắt buộc Btrfs hoặc WinBtrfs; dùng filesystem snapshot làm Synveil backup; đưa
hành vi riêng adapter vào domain invariant; âm thầm coi capability yếu là
guarantee mạnh hơn.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

Existing local/object data requires no migration. A new capability or native
format needs adapter conformance, corruption/recovery evidence, a feature flag
or fallback path, and a reader-before-writer/rollback plan. Supersede this ADR
if a platform-specific feature becomes a correctness requirement.

Dữ liệu local/object hiện tại không cần migration. Capability mới hoặc format
native cần conformance adapter, evidence corruption/recovery, feature flag hay
fallback và kế hoạch reader-before-writer/rollback. Thay thế ADR nếu tính năng
riêng platform trở thành yêu cầu correctness.
