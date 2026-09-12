# ADR-027: Sync rebaseline snapshot consistency boundary / Boundary nhất quán snapshot rebaseline sync

- Status / Trạng thái: **Accepted / Chấp thuận — LOCKED**
- Date / Ngày: 2026-09-08
- Owners / Chủ sở hữu: Sync, Database, Core

## Context (English)

An incremental sync cursor can become unusable after journal retention or epoch
change. Rebaseline then needs a complete authoritative logical library state and
an exact continuation point. Reading Nodes and later reading the journal head
would allow a committed mutation to fall between those two observations.

## Decision (English)

The transport-neutral `RebaselineSnapshot` pairs one library-scoped
`LogicalSnapshot` with the existing typed `JournalHighWatermark`/
`JournalCursor`. PostgreSQL builds both from one `REPEATABLE READ` transaction
after taking the existing per-library namespace guard. Snapshot entries are
ordered by immutable `NodeId`, include the canonical active root and current
`ACTIVE`/`TRASHED` logical Nodes, and exclude `PURGING`/purged Nodes and physical
storage identities. Snapshot creation is read-only with respect to device
checkpoints; checkpoint/rebaseline completion remains an explicit later action.

This prompt returns the logical state as an internal in-memory collection for a
bounded service operation. It does not add a migration, durable page token,
public route, client application, retention, conflict policy, cache, or
background runtime. Durable fixed-cut paging, if needed for public large-library
transfer, must preserve the same boundary invariant in a later design.

## Bối cảnh (Tiếng Việt)

Incremental sync cursor có thể không còn dùng được sau journal retention hoặc
epoch change. Khi đó rebaseline cần một logical library state authoritative đầy
đủ và một continuation point chính xác. Nếu đọc Node rồi đọc journal head sau đó,
mutation đã commit có thể rơi vào giữa hai observation.

## Quyết định (Tiếng Việt)

`RebaselineSnapshot` transport-neutral ghép một `LogicalSnapshot` theo scope
Library với `JournalHighWatermark`/`JournalCursor đã có type. PostgreSQL build
cả hai từ một transaction `REPEATABLE READ` sau khi lấy per-library namespace
guard hiện có. Entry order theo immutable `NodeId`, include root active canonical
và Node logical hiện tại ở `ACTIVE`/`TRASHED`, đồng thời loại `PURGING`/đã purge
và physical storage identity. Tạo snapshot không đọc/ghi tiến độ
`DeviceSyncCheckpoint`; hoàn tất checkpoint/rebaseline vẫn là action tường minh ở
prompt sau.

Prompt này trả logical state dưới dạng in-memory collection cho một bounded
service operation nội bộ. Không thêm migration, durable page token, public
route, client apply, retention, conflict policy, cache hay background runtime.
Nếu cần transfer Library lớn qua public API, paging fixed-cut durable ở thiết kế
sau phải giữ nguyên bất biến boundary này.

## Consequences / Hệ quả

- State and continuation use one typed cursor domain; wall-clock timestamps are
  descriptive and cannot become the synchronization boundary.
- A cooperative mutation is either visible in the snapshot and at/before the
  boundary, or is visible in neither and is available after the boundary through
  the existing journal feed.
- The current in-memory result is not a claim of unbounded public snapshot
  scalability. Later paging must keep one fixed cut across every page.
- Existing device-scoped materialized bootstrap and stale-cursor signaling remain
  unchanged; this ADR defines the lower-level consistency foundation they can
  reuse.

## Alternatives rejected / Phương án loại bỏ

- Reading Nodes, ending the transaction, and later reading the journal maximum.
- Using wall-clock time, a device checkpoint, or an arbitrary UUID as the cursor.
- Silently advancing a device checkpoint when a snapshot is generated.
- Exposing object keys, replica locators, filesystem paths, or byte content in
  the logical snapshot.
- Adding durable snapshot tables before a public cross-request paging contract
  requires them.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

No migration is required for this internal foundation. Revisit the decision only
when public snapshot paging, retention interaction, or a non-cooperative writer
requires a durable fixed-cut token or a replacement for the namespace guard;
that design must provide a new consistency proof and supersede this ADR rather
than weakening it.
