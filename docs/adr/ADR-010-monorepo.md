# ADR-010: Initial monorepo / Monorepo giai đoạn đầu

- Status / Trạng thái: **Accepted / Chấp thuận**
- Date / Ngày: 2026-08-21
- Owners / Chủ sở hữu: Architecture, DevOps

## Context (English)

API, domain crates, web, OpenAPI, migrations, deployment, optional AI, protocol
fixtures, and bilingual docs evolve together. Multiple repositories would make
atomic contract changes and greenfield CI harder.

## Decision (English)

Keep the initial and medium-stage product in one repository with explicit path
owners and workspace boundaries. Canonical docs, ADRs, API, migrations, tests,
deployment assets, and source are versioned together. Create a directory only
when a scoped task owns real content; do not scaffold future clients as empty
promises. Generated artifacts identify their source and are checked, not edited.

## Consequences (English)

CI must use path-aware jobs without allowing untested cross-contract changes.
One integration change can update API, server, clients, fixtures, and docs
atomically. A component may split repositories only with release/versioning,
security, and contributor workflow evidence in a new ADR.

## Bối cảnh (Tiếng Việt)

API, crate domain, web, OpenAPI, migration, deployment, AI tùy chọn, fixture
giao thức và tài liệu song ngữ cùng tiến hóa. Nhiều repository làm khó thay đổi
hợp đồng nguyên tử và CI greenfield.

## Quyết định (Tiếng Việt)

Giữ sản phẩm giai đoạn đầu/trung trong một repository với owner theo đường dẫn
và ranh giới workspace rõ ràng. Docs, ADR, API, migration, test, deployment và
source chuẩn được version cùng nhau. Chỉ tạo thư mục khi task có nội dung thực;
không scaffold client tương lai bằng thư mục rỗng. Artifact sinh tự động phải
ghi nguồn, được kiểm tra và không sửa tay.

## Hệ quả (Tiếng Việt)

CI theo đường dẫn nhưng không được bỏ test cho thay đổi xuyên hợp đồng. Một thay
đổi tích hợp có thể cập nhật nguyên tử API, server, client, fixture và docs. Chỉ
tách repository khi ADR mới có bằng chứng về release/version, bảo mật và quy
trình contributor.

Alternatives rejected / Phương án loại bỏ: repository per crate/service now;
unstructured single source directory; empty aspirational scaffolding.
