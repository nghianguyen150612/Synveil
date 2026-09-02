# Synveil documentation / Tài liệu Synveil

Synveil's product and architecture blueprint is maintained in parallel English
and Vietnamese editions. Most documents specify planned behavior; the API
contract and foundation status markers identify the limited transport evidence
that exists. They are not claims that product capabilities exist.

Blueprint sản phẩm và kiến trúc Synveil được duy trì song song bằng tiếng Anh
và tiếng Việt. Phần lớn tài liệu mô tả hành vi dự kiến; API contract và status
foundation xác định bằng chứng transport hữu hạn đang tồn tại. Chúng không
khẳng định capability sản phẩm đã tồn tại.

- [English blueprint](en/PRODUCT.md)
- [Bản thiết kế tiếng Việt](vi/PRODUCT.md)
- [Cross-platform product and platform architecture](en/PLATFORM.md)
- [Kiến trúc sản phẩm và nền tảng đa nền tảng](vi/PLATFORM.md)
- [OpenAPI contract skeleton](../api/openapi.yaml)
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
