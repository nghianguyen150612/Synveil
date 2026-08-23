# ADR-001: Rust modular monolith / Khối mô-đun bằng Rust

- Status / Trạng thái: **Accepted / Chấp thuận**
- Date / Ngày: 2026-08-21
- Owners / Chủ sở hữu: Architecture, Rust Backend

## Context (English)

Storage and synchronization need memory safety, predictable resource use,
streaming I/O, explicit errors, and one coherent transaction boundary. Early
microservices would add network partitions and contract deployment problems
before Synveil has measured scaling pressure.

## Decision (English)

Build the core API and durable worker in Rust using Axum, Tokio, Tower, Serde,
SQLx, and `tracing`. Organize the code as domain/application/port/adapter crates
with enforced dependency direction. Produce separate API and worker processes
from one workspace, but keep core domains in one modular deployment. Python is
allowed only for the optional AI runtime with a durable asynchronous boundary.

## Consequences (English)

- Core invariants can share types and database transactions without RPC.
- CPU-heavy hashing/compression needs bounded blocking pools so Tokio is not
  starved.
- Crates must not become pseudo-services or a cyclic “common” graph.
- A domain may become a service only after profiling, an ADR, and a versioned
  contract show a material benefit.

## Bối cảnh (Tiếng Việt)

Storage và đồng bộ cần an toàn bộ nhớ, tài nguyên dự đoán được, I/O dạng luồng,
lỗi rõ ràng và một ranh giới giao dịch nhất quán. Microservice quá sớm sẽ thêm
lỗi phân vùng mạng và khó triển khai hợp đồng khi chưa có áp lực scale đo được.

## Quyết định (Tiếng Việt)

Xây dựng API lõi và worker bền vững bằng Rust với Axum, Tokio, Tower, Serde,
SQLx và `tracing`. Tổ chức thành các crate domain/application/port/adapter với
chiều phụ thuộc bắt buộc. Tạo tiến trình API và worker riêng từ cùng workspace,
nhưng giữ các miền lõi trong một khối mô-đun. Chỉ dùng Python cho AI tùy chọn
qua ranh giới bất đồng bộ bền vững.

## Hệ quả (Tiếng Việt)

- Các bất biến lõi dùng chung kiểu dữ liệu và giao dịch DB mà không cần RPC.
- Băm/nén tốn CPU phải chạy trong pool blocking có giới hạn để không làm nghẽn
  Tokio.
- Không biến crate thành service giả hoặc đồ thị `common` vòng lặp.
- Chỉ tách service khi có số đo, ADR và hợp đồng có phiên bản chứng minh lợi ích.

Alternatives rejected / Phương án loại bỏ: Node.js/Go/Python as core backend;
network microservices per domain; Kubernetes-first deployment.
