# ADR-029: Client rebaseline atomic apply and outbound-intent preservation / Apply rebaseline client nguyên tử và bảo toàn outbound intent

- Status / Trạng thái: **Accepted / Chấp thuận — LOCKED**
- Date / Ngày: 2026-09-09
- Owners / Chủ sở hữu: Sync, Desktop

## Context (English)

An immutable server snapshot can require many page requests. Replacing the
client mirror while those requests are in progress would expose mixed state and
can accidentally discard unaccepted local work.

## Decision (English)

The client stores at most one durable candidate per library in separate SQLite
tables. Every bounded page is checked against the fixed snapshot ID, library,
journal boundary, count, canonical NodeId order, and opaque cursor progression
before it is committed. A terminal candidate is revalidated for one active
directory root, parent existence/type, reachability, and exact count.

The final short SQLite transaction replaces only the authoritative
`local_nodes` mirror, writes `rebaseline_applied_handoffs`, and removes the
candidate. It does not update `outbound_intents`, mutation preconditions,
upload-session rows, or files under the managed root. Existing local byte
cleanup is explicitly deferred. The marker fences the incremental inbound
engine until the future handoff protocol resolves it; it is not a server ACK,
checkpoint advance, or conflict decision. A retry of the committed same
snapshot is locally idempotent; a different snapshot is rejected while a
candidate or pending handoff exists.

Transfer memory is O(page size), excluding SQLite storage. Final validation and
activation construct an O(number of nodes) metadata/path index; no content
bytes are buffered. There is no polling, retry worker, cleanup daemon, UI, or
new server API.

## Bối cảnh và quyết định (Tiếng Việt)

Snapshot server bất biến có thể cần nhiều request page. Thay mirror client khi
đang tải sẽ lộ state trộn lẫn và có thể làm mất local work chưa được server
chấp nhận.

Client lưu tối đa một candidate durable cho mỗi Library trong bảng SQLite tách
riêng. Mỗi page bị kiểm tra snapshot ID, Library, journal boundary, count,
NodeId order và tiến triển cursor opaque trước khi commit. Candidate terminal
được kiểm tra lại một root directory ACTIVE, parent hợp lệ, reachability và
count chính xác.

Transaction SQLite ngắn cuối cùng chỉ thay `local_nodes`, ghi
`rebaseline_applied_handoffs` và xóa candidate. Nó không sửa `outbound_intents`,
precondition mutation, upload-session hay byte dưới managed root. Marker chặn
inbound incremental cũ cho tới handoff phase sau; nó không phải ACK/checkpoint
server hoặc conflict decision. Không có daemon, retry worker, UI hay API server
mới.
