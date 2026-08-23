# ADR-009: React, TypeScript, and Vite for web / React, TypeScript và Vite cho web

- Status / Trạng thái: **Accepted / Chấp thuận**
- Date / Ngày: 2026-08-21
- Owners / Chủ sở hữu: Web, Architecture

## Context (English)

Synveil Web is an authenticated application with streaming/upload workflows and
little SEO-driven server rendering. It needs a typed API boundary and a simple
static deployment behind Caddy.

## Decision (English)

Use React, strict TypeScript, and Vite. Generate or validate API types from the
reviewed OpenAPI contract; do not duplicate domain truth in handwritten client
models. The browser never receives storage/integration/database credentials.
Server state, local UI state, resumable transfer state, and future sync state
remain distinct. Accessibility and keyboard behavior are release gates.

## Consequences (English)

The web build can be served as static assets and independently cache-busted.
Authentication uses secure cookies and CSRF protection as specified by the API.
Large data remains streamed; browser memory does not become a file buffer.
Framework changes require measured need and an ADR, not an assumption that
Next.js is mandatory.

## Bối cảnh (Tiếng Việt)

Synveil Web là ứng dụng đã đăng nhập với luồng upload/stream, không phụ thuộc
nhiều vào SSR cho SEO. Ứng dụng cần ranh giới API có kiểu và triển khai tĩnh đơn
giản sau Caddy.

## Quyết định (Tiếng Việt)

Dùng React, TypeScript strict và Vite. Sinh hoặc kiểm tra kiểu API từ OpenAPI đã
review; không chép lại sự thật domain bằng model viết tay. Trình duyệt không bao
giờ nhận credential storage/integration/database. Tách trạng thái server, UI
local, transfer nối tiếp và sync tương lai. Accessibility cùng thao tác bàn phím
là gate phát hành.

## Hệ quả (Tiếng Việt)

Build web là tài sản tĩnh với cache bust độc lập. Xác thực dùng cookie an toàn
và CSRF theo API. Dữ liệu lớn luôn stream, không biến RAM trình duyệt thành buffer
tệp. Chỉ đổi framework khi có nhu cầu đo được và ADR; không mặc định cần Next.js.

Alternatives rejected / Phương án loại bỏ: required Next.js; backend-rendered
application pages; untyped ad hoc API clients.
