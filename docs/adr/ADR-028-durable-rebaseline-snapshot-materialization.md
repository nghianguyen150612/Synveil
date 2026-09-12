# ADR-028: Durable rebaseline snapshot materialization and paging / Materialize và phân trang snapshot rebaseline durable

- Status / Trạng thái: **Accepted / Chấp thuận — LOCKED**
- Date / Ngày: 2026-09-08
- Owners / Chủ sở hữu: Sync, Database, Core

## Context (English)

ADR-027 establishes that a complete logical namespace and its journal
continuation boundary must come from one PostgreSQL `REPEATABLE READ` view
under the canonical per-library namespace guard. Its in-memory result cannot
be safely transferred over multiple independent requests: rereading live Nodes
for page two could mix a rename, move, Trash transition, or creation committed
after page one into the same apparent snapshot.

## Decision (English)

Prompt 82 materializes an owner-authorized, library-scoped `RebaselineSnapshot`
artifact in PostgreSQL. A UUIDv7 `RebaselineSnapshotId`, header (typed journal
epoch/resume sequence, count, injected creation/expiry timestamps), and all
logical entry rows commit together in one guarded transaction. The Prompt 81
logical builder remains `REPEATABLE READ`; Prompt 83C durable creation uses
`READ COMMITTED` deliberately so the admission count sees artifacts committed
by a caller that held the namespace guard before a queued caller. PostgreSQL
can establish a repeatable-read snapshot while the advisory-lock statement is
waiting, which would make that count stale. Once the guard is held, all
cooperative namespace mutations and creators are serialized, so the head,
admission count, and materialized projection remain one coherent cut. The
creator first validates the complete Prompt 81 `LogicalSnapshot`, then uses one
`INSERT ... SELECT` to copy its canonical projection. No partial header,
boundary, or entry set can be visible outside a failed transaction.

Subsequent reads never acquire the namespace guard and never query live
`nodes`. They use the immutable `(snapshot_id, node_id)` keyset order, with a
distinct `RebaselineSnapshotPageCursor` carrying the artifact ID and last
immutable Node ID. This cursor is not a `JournalCursor`; the descriptor repeats
the actual journal boundary on every page. The artifact is owner-concealed,
library-scoped, device-independent, and expires when `observed_at >=
expires_at`. Expiry rejects reads but does not start cleanup.

The artifact stores only Prompt 81 logical fields. It excludes physical storage
details, secret material, bytes, device checkpoint state, rebaseline completion,
and client application. No HTTP/OpenAPI/SSE/WebSocket route, UI, client apply,
background worker, cleanup daemon, idempotency protocol, global lock, or retry
loop is introduced by this decision.

## Bối cảnh (Tiếng Việt)

ADR-027 xác lập rằng namespace logical đầy đủ cùng boundary continuation của
journal phải đến từ một PostgreSQL view `REPEATABLE READ` dưới per-library
namespace guard chuẩn. Kết quả in-memory không thể transfer an toàn qua nhiều
request độc lập: đọc lại Node live cho page hai có thể trộn rename, move, Trash
hoặc create commit sau page một vào cùng snapshot bề ngoài.

## Quyết định (Tiếng Việt)

Prompt 82 materialize artifact `RebaselineSnapshot` theo scope Library và owner
đã authorize trong PostgreSQL. `RebaselineSnapshotId` UUIDv7, header (journal
epoch/resume sequence có type, count, timestamp create/expiry inject) và toàn
bộ entry logical commit cùng nhau trong một transaction có namespace guard. Builder
logical Prompt 81 vẫn dùng `REPEATABLE READ`; creation durable Prompt 83C cố ý
dùng `READ COMMITTED` để admission count nhìn thấy artifact đã commit bởi caller
giữ guard trước đó khi caller xếp hàng. PostgreSQL có thể tạo snapshot repeatable
read trong lúc statement advisory-lock đang chờ, làm count bị stale. Sau khi giữ
guard, mọi mutation namespace cooperative và creator đều serialized, nên head,
admission count và projection materialize vẫn là một cut coherent. Creator
validate `LogicalSnapshot` Prompt 81 đầy đủ trước, rồi dùng một `INSERT ...
SELECT` copy projection canonical. Header, boundary hay entry set partial không
thể nhìn thấy ngoài transaction thất bại.

Read sau đó không lấy namespace guard và không query `nodes` live. Chúng dùng
keyset order immutable `(snapshot_id, node_id)`, với
`RebaselineSnapshotPageCursor` riêng mang artifact ID và Node ID immutable cuối
cùng. Cursor này không phải `JournalCursor`; descriptor lặp lại boundary journal
thật trong mọi page. Artifact che giấu theo owner, scope Library,
device-independent và hết hạn khi `observed_at >= expires_at`. Hết hạn từ chối
read nhưng không khởi động cleanup.

Artifact chỉ lưu field logical Prompt 81. Nó loại physical storage detail, bí
mật, byte, state device checkpoint, complete rebaseline và client apply. Quyết
định này không thêm HTTP/OpenAPI/SSE/WebSocket, UI, client apply, background
worker, cleanup daemon, idempotency protocol, global lock hay retry loop.

## Consequences / Hệ quả

- A transfer survives service restart and connection replacement while retaining
  one canonical cut; live namespace mutations resume strictly after the stored
  journal boundary.
- Creation temporarily allocates the validated aggregate, O(total entries),
  but PostgreSQL copies rows set-wise and each page read is O(page size).
- Artifacts occupy one header plus one logical row per Node. Overlapping
  snapshots multiply metadata until a future lifecycle/retention phase removes
  expired rows.
- ADR-027 remains authoritative for the logical cut itself. This ADR supersedes
  only its deferred durable-paging seam.

## Alternatives rejected / Phương án loại bỏ

- Re-querying the live namespace for each page.
- Holding the original PostgreSQL transaction or advisory guard through client
  transfer, process lifetime, or network retries.
- OFFSET pagination, a page-number token, wall-clock continuation, or a
  snapshot-local numeric sequence in place of the typed journal boundary.
- Device-owned duplicate artifacts, checkpoint advancement, or automatic
  rebaseline completion during creation/read.
- Global serialization, retry loops, cleanup polling, or materializing file
  bytes and physical storage metadata.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

Migration 35 is forward-only and adds the minimal header/entry relations. A
future transport phase may serialize the typed cursor; a future lifecycle phase
may add bounded expiry cleanup or deletion only with an explicit retention and
library-deletion proof. A client-side atomic apply protocol remains a separate
decision and must not weaken this artifact's fixed-cut invariant.
