# Synveil documentation / Tài liệu Synveil

Synveil's product and architecture blueprint is maintained in parallel English
and Vietnamese editions. Most documents specify planned behavior; the API
contract and foundation status markers identify the limited transport evidence
that exists. The current implemented slice includes authenticated logical
file/folder metadata, the exact-offset resumable upload HTTP/API-helper
boundary, a transport-neutral owner-authorized immutable content-read service,
and authenticated HTTP full/single-range current and historical content
downloads, plus authenticated immutable version-history metadata listing and
lookup. Safe historical-version restore is implemented at the authenticated
API/metadata boundary as one new immutable FileVersion reusing the verified
historical Object. Internal metadata-purge execution and FileVersion-to-Object
reference accounting, metadata-only GC grace/lease planning, internal physical
replica execution, and opt-in internal GC-worker orchestration are implemented.
Physical GC has no HTTP route: it uses final PostgreSQL reference/hold/lease
revalidation, durable per-replica reconciliation, and ObjectStore-only deletion
before Object metadata removal. The worker performs bounded `run_once()`
cycles, resumes durable operations before new planning, persists retry
scheduling, and reports metadata-only inconsistencies; it does not scan or
auto-delete unknown physical files. Download UI, desktop GUI/background lifecycle,
automatic conflict resolution, backup, and sharing remain out of scope;
disposable-PostgreSQL end-to-end evidence remains environment-gated; the
implemented transport does not expose storage keys or physical paths. The
owner/library-scoped durable PostgreSQL change-journal foundation is now
implemented with atomic mutation publication, ordered resumable reads, and
bounded opaque cursors; per-device/per-library checkpoints, an authenticated
bounded server change feed, and signed checkpoint acknowledgment are
implemented and validated. The server-side logical snapshot/rebaseline
bootstrap is implemented with an immutable PostgreSQL manifest, bounded
keyset pages, terminal HMAC proof, and exact checkpoint handoff at one coherent
journal cut. It contains no file bytes or physical storage identity. Client
mutation submission is implemented as one strict logical mutation per request
with durable UUID idempotency, canonical SHA-256 fingerprints, explicit
optimistic preconditions, deterministic persisted conflicts, and exact journal
integration. It accepts no file bytes; automatic conflict resolution,
last-write-wins, conflict-copy rename, merge engines, and a desktop GUI/watcher
remain unimplemented.
Managed mutation conflicts now also create one durable immutable conflict row
in the original mutation transaction. Authenticated owner/device/library-
scoped list and detail routes expose only typed intent and clearly historical
logical evidence. Explicit `ACCEPT_SERVER` and fresh-precondition
`APPLY_CLIENT_INTENT` decisions are durable, fenced, idempotent, and replayable;
only a successful apply emits one normal resource journal event. No automatic
policy chooses or merges a resolution.

Blueprint sản phẩm và kiến trúc Synveil được duy trì song song bằng tiếng Anh
và tiếng Việt. Phần lớn tài liệu mô tả hành vi dự kiến; API contract và status
foundation xác định bằng chứng transport hữu hạn đang tồn tại. Slice hiện đã
implement gồm logical file/folder metadata đã authenticate và boundary HTTP/API
helper upload có thể tiếp tục theo exact offset, cùng content-read service bất
biến trung lập transport đã authorize theo owner và HTTP download full/range cho
current cùng historical version, cùng metadata version-history bất biến listing
và lookup. Safe historical-version restore đã implement ở boundary API/metadata
đã authenticate như một FileVersion bất biến mới dùng lại Object historical đã
verify. Metadata-purge execution nội bộ và reference accounting từ
FileVersion tới Object cùng GC planning grace/lease metadata-only, physical
replica execution nội bộ và GC-worker orchestration nội bộ opt-in đã implement.
Physical GC không có HTTP route; nó dùng final revalidation reference/hold/
lease trong PostgreSQL, reconciliation bền theo từng replica và chỉ xóa qua
ObjectStore trước khi dọn metadata Object. Worker chạy chu kỳ `run_once()` có
giới hạn, resume operation bền trước planning mới, persist retry scheduling và
báo inconsistency chỉ từ metadata; nó không scan hay auto-delete file vật lý
không rõ. Download UI, GUI/background lifecycle phía desktop, automatic conflict
resolution, backup hay sharing vẫn nằm ngoài scope; bằng chứng
end-to-end PostgreSQL disposable vẫn bị gate theo môi trường; transport đã
implement không expose storage key hay physical path. Nền tảng change journal
PostgreSQL bền vững theo owner/library đã được implement với publication nguyên
tử cùng mutation, read có thứ tự có thể resume và cursor opaque có giới hạn;
checkpoint theo device/library, server change feed một chiều đã authenticate
và checkpoint acknowledgment có ký đã được implement và validate. Bootstrap
snapshot/rebaseline logical phía server đã implement bằng manifest PostgreSQL
bất biến, page keyset có giới hạn, terminal proof HMAC và handoff checkpoint
chính xác tại một journal cut nhất quán. Payload không chứa byte file hay định
danh storage vật lý. Client mutation submission đã implement như một logical
mutation mỗi request với UUID idempotency bền vững, fingerprint SHA-256
canonical, precondition optimistic explicit, conflict deterministic và journal
integration chính xác. Payload không nhận byte file; automatic conflict
resolution, last-write-wins, conflict-copy rename, merge engine và desktop
GUI/watcher vẫn chưa implement.
Managed mutation conflict hiện tạo đúng một conflict row bền vững, bất biến
trong cùng transaction của mutation gốc. Route list/detail đã authenticate và
scope theo owner/device/library chỉ expose intent có type cùng evidence logical
lịch sử được ghi nhãn rõ. Quyết định `ACCEPT_SERVER` và
`APPLY_CLIENT_INTENT` với fresh precondition là explicit, bền vững, có fence,
idempotent và replay được; chỉ apply thành công phát đúng một resource journal
event thông thường. Không policy tự động nào chọn hoặc merge resolution.

## Prompt 35 historical status / trạng thái tại gate Prompt 35

| Capability | Status |
|---|---|
| durable change journal | `VALIDATED` |
| device checkpoints/feed | `VALIDATED` |
| snapshot/rebaseline | `VALIDATED` |
| client mutation submission | `VALIDATED` |
| optimistic conflict detection | `VALIDATED` |
| durable conflict records | `IMPLEMENTED` |
| manual conflict inspection | `IMPLEMENTED` |
| explicit manual resolution | `IMPLEMENTED` |
| automatic conflict resolution | `NOT IMPLEMENTED` |
| desktop sync agent | `NOT IMPLEMENTED` |

## Prompt 36–38 desktop sync status / trạng thái desktop sync Prompt 36–38

| Capability | Status |
|---|---|
| server journal/checkpoint/feed/rebaseline/mutations/conflicts | `VALIDATED` |
| desktop inbound sync core | `VALIDATED` |
| desktop remote/device auth | `VALIDATED` |
| desktop local crash-safe state | `IMPLEMENTED` |
| snapshot/feed local apply | `IMPLEMENTED` |
| file download/apply | `IMPLEMENTED` |
| local divergence detection | `IMPLEMENTED` |
| durable HTTPS server profiles and production HTTP remote | `IMPLEMENTED` |
| one-time Device enrollment and revocable bearer credentials | `IMPLEMENTED` |
| Linux Secret Service / Windows Credential Manager adapter | `IMPLEMENTED` |
| filesystem observation | `IMPLEMENTED` |
| self-generated change suppression | `IMPLEMENTED` |
| durable outbound intent capture | `IMPLEMENTED` |
| rename/move attribution | `IMPLEMENTED with conservative fallback` |
| watcher overflow/reconciliation | `IMPLEMENTED` |
| automatic outbound mutation submission | `NOT IMPLEMENTED` |
| automatic conflict resolution | `NOT IMPLEMENTED` |
| desktop GUI | `NOT IMPLEMENTED` |

`crates/client-sync` is the reusable, UI-free inbound boundary. It uses a
transport-neutral `SyncRemote`, a narrow `LocalReplica`, and independently
migrated SQLite local state. The core is implemented and tested on Linux; the
Windows code path is compile-audited and client-sync/platform test executables
are linked with MinGW, but have not been run on a native Windows host in this
phase. Prompt 37 implements verified-HTTPS profiles, the production
HTTP remote, one-time Device enrollment, digest-only server credential storage,
and OS-backed desktop SecretStore persistence. Browser sessions retain CSRF;
Device bearer routes are inbound-only. Native Linux Secret Service persistence
is tested in an isolated vault. Prompt 38 adds a local-only watcher/reconcile
observer: raw watcher events are hints, `.synveil` control data is excluded,
filesystem truth is reinspected before classification, Prompt 36 operation
suppressions prevent inbound echo, and durable outbound intents remain offline in
SQLite until a future explicit submission phase.

`crates/client-sync` là boundary inbound tái sử dụng, không có UI. Crate dùng
`SyncRemote` trung lập transport, `LocalReplica` hẹp và local state SQLite có
migration độc lập. Core đã implement và test trên Linux; code path Windows đã
được audit để compile và test executable client-sync/platform đã link bằng
MinGW, nhưng chưa chạy trên host Windows native trong phase này.
Prompt 37 đã implement profile xác minh HTTPS, HTTP remote production,
enrollment Device một lần, server chỉ lưu digest credential và desktop lưu
secret trong SecretStore của hệ điều hành. Browser session vẫn cần CSRF;
Device bearer chỉ được gọi route inbound cho phép. Persistence Linux Secret
Service được test bằng vault cô lập. Prompt 38 thêm observer local-only:
watcher event chỉ là hint, `.synveil` bị loại trừ, filesystem được reinspect
trước khi classify, suppression dựa trên operation evidence Prompt 36, và
outbound intent durable chỉ nằm trong SQLite chờ phase submit explicit sau này.

- [English blueprint](en/PRODUCT.md)
- [Bản thiết kế tiếng Việt](vi/PRODUCT.md)
- [Cross-platform product and platform architecture](en/PLATFORM.md)
- [Kiến trúc sản phẩm và nền tảng đa nền tảng](vi/PLATFORM.md)
- [OpenAPI metadata and resumable-upload API contract](../api/openapi.yaml)
- [Architecture Decision Records / Biên bản quyết định kiến trúc](adr/README.md)
- [Repository audit / Kiểm kê repository](en/REPOSITORY_AUDIT.md)
- [Prompt 37 continuation audit / Kiểm tra tiếp tục Prompt 37](PROMPT37_CONTINUATION_AUDIT.md)

## Reading order / Thứ tự đọc

1. `PRODUCT.md`, `PLATFORM.md`, and `FEATURES.md` — product promise, audience,
   deployment experience, scope, and honest status.
2. `ARCHITECTURE.md` and ADRs — system boundaries and authoritative decisions.
3. `DOMAIN_MODEL.md` and `API_ARCHITECTURE.md` — names and public contracts.
4. `STORAGE.md`, `UPLOADS.md`, `SYNC.md`, `BACKUP.md` — data-safety protocols.
5. `SECURITY.md`, `DEPLOYMENT.md`, `TESTING.md` — trust and production gates.
6. `ROADMAP.md`, `TEAM_PLAN.md`, `CONTRIBUTING_ARCHITECTURE.md` — execution.

---

1. `PRODUCT.md`, `PLATFORM.md` và `FEATURES.md` — cam kết, audience, deployment,
   phạm vi và trạng thái trung thực.
2. `ARCHITECTURE.md` và ADR — ranh giới hệ thống và quyết định có thẩm quyền.
3. `DOMAIN_MODEL.md` và `API_ARCHITECTURE.md` — thuật ngữ và hợp đồng công khai.
4. `STORAGE.md`, `UPLOADS.md`, `SYNC.md`, `BACKUP.md` — giao thức an toàn dữ liệu.
5. `SECURITY.md`, `DEPLOYMENT.md`, `TESTING.md` — trust và gate production.
6. `ROADMAP.md`, `TEAM_PLAN.md`, `CONTRIBUTING_ARCHITECTURE.md` — thực thi.

When documents disagree, follow the authority order in
`CONTRIBUTING_ARCHITECTURE.md`. Accepted ADRs are bilingual in one file. English
core specifications are the temporary tie-breaker, but release documentation
requires semantic parity with Vietnamese.

Khi tài liệu mâu thuẫn, áp dụng thứ tự thẩm quyền trong
`CONTRIBUTING_ARCHITECTURE.md`. ADR đã chấp thuận chứa hai ngôn ngữ trong cùng
tệp. Bản tiếng Anh tạm thời là chuẩn phân xử, nhưng gate phát hành yêu cầu bản
tiếng Việt tương đương về nghĩa.
