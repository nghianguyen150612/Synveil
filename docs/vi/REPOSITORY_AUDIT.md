# Kiểm kê repository

- Trạng thái: **Baseline đã chấp thuận**
- Ngày kiểm kê: 2026-08-21
- Revision được kiểm kê: `5f70a0b` trên nhánh `main`

## Kết luận chính

Synveil bắt đầu từ một repository greenfield sạch, lấy kiến trúc làm nền tảng.
Tại revision được kiểm kê, repository chỉ có đúng hai tệp được Git theo dõi:
`README.md` và `LICENSE`. Không có mã sản phẩm cần refactor, định dạng dữ liệu
bền vững cần migration hay hợp đồng API cũ phải giữ tương thích.

Kết luận này cho phép viết tài liệu và scaffold Phase 0 một cách có chủ đích;
không cho phép tuyên bố sản phẩm đã hoạt động.

## Nội dung hiện có

| Tệp | Kết quả | Cách xử lý |
|---|---|---|
| `README.md` | Placeholder hai dòng. Câu mô tả bị cắt ở `organizatio` và không phân biệt kế hoạch với phần đã triển khai. | Thay bằng mục lục blueprint có trạng thái trung thực. |
| `LICENSE` | MIT License, bản quyền 2026 Nguyen Nghia. | Giữ nguyên. Mọi đề xuất đổi giấy phép là quyết định riêng của owner/pháp lý. |

Trước khi làm blueprint, working tree sạch, nhánh theo dõi `origin/main` và chỉ
có một commit (`Initial commit`). Không có source ẩn, tệp triển khai chưa track
hay chỉ dẫn agent cục bộ trong repository.

## Thành phần có thể tái sử dụng

- Tên dự án **Synveil** đã được xác lập.
- Placeholder nêu đúng hướng self-hosted kết hợp storage, backup, sync, photos,
  code và tổ chức dữ liệu bằng AI.
- Repository và remote sạch là nền tảng phù hợp để xây monorepo.

Không có component, schema, migration, API, test, container hay deployment
manifest nào có thể tái sử dụng.

## Mâu thuẫn và giả định

### Hướng giấy phép

Repository hiện dùng MIT, trong khi brief ưu tiên AGPL-3.0 cho core/server/web
và Apache-2.0 cho một số SDK tích hợp cần giấy phép rộng. Blueprint không thể âm
thầm thay đổi một quyền cấp pháp lý. Vì vậy ADR-011 giữ trạng thái `Proposed` và
`LICENSE` không thay đổi.

### Trạng thái triển khai

README cũ có thể khiến người đọc hiểu rằng các tính năng đã tồn tại. Thực tế
không có tính năng nào. README mới dùng taxonomy chung và đánh dấu repository ở
giai đoạn trước triển khai.

### Hướng kỹ thuật

Chưa có stack nào được khởi tạo để mâu thuẫn với hướng Rust, React, PostgreSQL,
Python, Docker Compose và Caddy. Mọi sơ đồ/module trong tài liệu là thiết kế mục
tiêu.

## Hạ tầng còn thiếu

Kiểm kê xác nhận chưa có:

- Rust workspace, crate, source, cấu hình format/lint và chính sách dependency;
- metadata workspace JavaScript và ứng dụng React/Vite;
- môi trường Python hoặc service AI;
- schema/migration PostgreSQL;
- adapter storage, OpenAPI, domain code, giao thức sync/backup và jobs;
- Compose, Dockerfile, cấu hình Caddy, cấu hình mẫu và xử lý secret;
- CI, test, fixture, benchmark, fuzz target và bộ thử recovery;
- quy trình contribution, security, governance, release, upgrade;
- đặc tả song ngữ và ADR.

Đây là đầu vào cho roadmap, không phải lỗi của một implementation đang chạy.

## Khuyến nghị chuyển tiếp

Xem Phase 0 là scaffold greenfield. Chỉ thêm thư mục khi một task có gate thực
sự dùng đến. Không tạo yêu cầu tương thích giả cho mã chưa tồn tại và không
triển khai module tính năng trong giai đoạn blueprint.

Ngay khi có dữ liệu bền vững, giả định greenfield hết hiệu lực. Mọi thay đổi
schema và storage format về sau phải tuân theo migration tiến cùng quy tắc tương
thích trong `DEPLOYMENT.md` và ADR đã chấp thuận.

## Bằng chứng và khả năng lặp lại

Việc kiểm kê chỉ dùng thao tác đọc: trạng thái/lịch sử/cây Git, liệt kê toàn bộ
tệp ngoài `.git`, đọc nội dung và kiểm tra tính toàn vẹn object Git. Task triển
khai sau này ít nhất phải kiểm tra lại trạng thái và inventory vì tài liệu này
là baseline tại một thời điểm, không phải khẳng định luôn đúng.
