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
reference accounting are implemented without object-byte deletion; physical
object GC, download UI, sync, backup, and sharing remain out of scope;
disposable-PostgreSQL end-to-end evidence remains environment-gated; the
implemented transport does not expose storage keys or physical paths.

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
FileVersion tới Object đã implement mà không xóa object byte; physical object
GC, Download UI, sync, backup hay sharing vẫn nằm ngoài scope; bằng chứng
end-to-end PostgreSQL disposable vẫn bị gate theo môi trường; transport đã
implement không expose storage key hay physical path.

- [English blueprint](en/PRODUCT.md)
- [Bản thiết kế tiếng Việt](vi/PRODUCT.md)
- [Cross-platform product and platform architecture](en/PLATFORM.md)
- [Kiến trúc sản phẩm và nền tảng đa nền tảng](vi/PLATFORM.md)
- [OpenAPI metadata and resumable-upload API contract](../api/openapi.yaml)
- [Architecture Decision Records / Biên bản quyết định kiến trúc](adr/README.md)
- [Repository audit / Kiểm kê repository](en/REPOSITORY_AUDIT.md)

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
