# ADR-059: Guided server setup product contract / Hợp đồng sản phẩm thiết lập máy chủ có hướng dẫn

## Status / Trạng thái

Accepted / Chấp thuận — Prompt029 product contract

## Date / Ngày

2026-10-03

## Decision owners / Chủ sở hữu quyết định

Product, Server Architecture, Desktop, Platform, and Security / Sản phẩm, kiến trúc máy chủ, desktop, nền tảng và bảo mật

## Context (English)

The v0.2 installation contract separates desktop installation from first-run Host/Connect, and ADR-050 assigns server bootstrap to its own coordinator. The ordinary self-hosting journey still needs a closed product decision for user intent, profile disclosure, resource ownership, preservation, readiness, and recovery before dependency and platform implementations begin. Current server operation remains operator-managed; this ADR is a target contract, not evidence of guided hosting.

## Decision (English)

1. The first-run product decision is **Host Synveil on this device** or **Connect to existing Synveil**. Host transfers control to server bootstrap; Connect remains a client path and performs no server provisioning.
2. There are exactly two conceptual setup profiles: **Personal / Home Mode** and **Advanced / Server Mode**. They use the same protocol, auth, PostgreSQL authority, object identity, migration rules, and data-safety invariants. Advanced infrastructure controls are not required to make normal hosting possible.
3. PostgreSQL remains Synveil's canonical server metadata and transaction authority. This decision does not choose how PostgreSQL is packaged, provisioned, or updated.
4. Server Bootstrap Coordinator is separate from desktop installation, GUI lifecycle, synchronization, and server runtime. It coordinates canonical resource owners and never becomes the sync engine; synveil-client remains synchronization authority.
5. Server package, configuration, secrets, database, object data, service/network integration, and runtime have distinct owners. Ordinary desktop uninstall and server-software removal preserve durable server data. Repair is reconciliation, not reset; unknown state fails closed.
6. Hosting has no silent public exposure, implicit public bind, mandatory proprietary relay, trust bypass, or plaintext remote-auth shortcut. Remote access requires explicit intent.
7. Infrastructure readiness is distinct from completed first-owner bootstrap and Server Ready. Uncertain external effects require authoritative inspection and reconciliation before retry; global rollback is not promised.
8. P030 chooses dependency strategy; P031 configuration/secrets; P032 storage; P033 service integration; P034 reachability; P035 first-admin bootstrap; P036 composes and proves the Host journey. P029 chooses none of their concrete implementation strategies.

The normative requirements, user-facing language, ownership matrix, state model, readiness conditions, handoff criteria, and deferred decisions are in [SERVER_SETUP_PRODUCT_CONTRACT.md](../v0.2/SERVER_SETUP_PRODUCT_CONTRACT.md).

## Consequences (English)

P030 receives a closed product problem without choosing a technology by implication. Normal-mode acceptance is measured as a private, recoverable, data-preserving Synveil server journey. Existing installation and server ownership authorities remain in force. P029 does not claim platform qualification, a provisioned server, first-admin implementation, end-to-end hosting readiness, or Windows native readiness.

## Alternatives (English)

- Treat Host as a special local-client mode that reads PostgreSQL directly — rejected because it bypasses the normal API/auth boundary and changes metadata authority.
- Create separate Home and Server editions — rejected because they would split protocol and data-safety behavior.
- Ask every user to configure PostgreSQL and services — rejected because infrastructure implementation detail is not a Personal / Home decision.
- Select a bundled database/runtime during P029 — deferred to P030, which must compare platform, security, lifecycle, backup, recovery, and maintenance evidence.

## Migration and review trigger (English)

P030–P036 and later first-run UX must conform to the linked product contract. A change to Host/Connect meaning, PostgreSQL authority, profile compatibility, durable ownership, readiness, preservation, or trust boundaries requires a superseding ADR. Exact provisioning, configuration, storage, service, network, and admin-bootstrap mechanisms belong to their assigned prompts and do not require reopening this ADR when they satisfy its contract.

## Bối cảnh (Tiếng Việt)

Hợp đồng cài đặt v0.2 tách việc cài ứng dụng desktop khỏi lựa chọn Host/Connect ở lần chạy đầu; ADR-050 giao việc khởi tạo máy chủ cho coordinator riêng. Hành trình tự lưu trữ thông thường vẫn cần quyết định sản phẩm rõ ràng về ý định người dùng, mức độ công khai cấu hình, ownership tài nguyên, bảo toàn dữ liệu, readiness và khôi phục trước khi triển khai dependency và nền tảng. Hoạt động máy chủ hiện vẫn do operator quản lý; ADR này là hợp đồng mục tiêu, không phải bằng chứng rằng tính năng lưu trữ có hướng dẫn đã được triển khai.

## Quyết định (Tiếng Việt)

1. Lựa chọn sản phẩm ở lần chạy đầu là **Host Synveil on this device** hoặc **Connect to existing Synveil**. Host chuyển quyền điều khiển sang bootstrap máy chủ; Connect vẫn là luồng client và không provision server.
2. Có đúng hai profile thiết lập mang tính khái niệm: **Personal / Home Mode** và **Advanced / Server Mode**. Cả hai dùng chung protocol, auth, authority PostgreSQL, định danh object, quy tắc migration và bất biến an toàn dữ liệu. Không bắt buộc cấu hình hạ tầng Advanced để hosting thông thường hoạt động.
3. PostgreSQL tiếp tục là authority chuẩn tắc cho metadata máy chủ và giao dịch của Synveil. Quyết định này không chọn cách đóng gói, provision hoặc cập nhật PostgreSQL.
4. Server Bootstrap Coordinator tách biệt khỏi cài đặt desktop, lifecycle GUI, đồng bộ và runtime server. Coordinator phối hợp các owner tài nguyên chuẩn tắc và không trở thành sync engine; synveil-client vẫn là authority đồng bộ.
5. Package server, cấu hình, secret, database, object data, tích hợp service/network và runtime có owner riêng. Gỡ desktop thông thường và gỡ phần mềm server phải bảo toàn dữ liệu server bền vững. Repair là đối soát, không phải reset; state không rõ phải fail closed.
6. Hosting không được âm thầm công khai ra Internet, bind public ngầm định, bắt buộc relay proprietari, bỏ qua kiểm tra trust hoặc dùng remote auth plaintext. Truy cập từ xa cần ý định rõ ràng.
7. Hạ tầng sẵn sàng khác với bootstrap owner đầu tiên đã hoàn tất và Server Ready. Hiệu ứng ngoài có kết quả chưa rõ phải được kiểm tra và đối soát bằng state có thẩm quyền trước khi thử lại; không hứa rollback toàn cục.
8. P030 chọn chiến lược dependency; P031 cấu hình/secret; P032 storage; P033 tích hợp service; P034 reachability; P035 bootstrap admin đầu tiên; P036 kết hợp và chứng minh hành trình Host. P029 không chọn cơ chế triển khai cụ thể của các prompt đó.

Yêu cầu chuẩn tắc, ngôn ngữ hướng người dùng, ma trận ownership, mô hình state, điều kiện readiness, ranh giới bàn giao và quyết định được hoãn nằm trong [SERVER_SETUP_PRODUCT_CONTRACT.md](../v0.2/SERVER_SETUP_PRODUCT_CONTRACT.md).

## Hệ quả (Tiếng Việt)

P030 nhận một bài toán sản phẩm đã đóng mà không bị ngụ ý phải chọn công nghệ cụ thể. Acceptance chế độ thường được đo bằng hành trình tạo máy chủ Synveil riêng tư, có thể khôi phục và bảo toàn dữ liệu. Các authority cài đặt và ownership máy chủ hiện có vẫn có hiệu lực. P029 không tuyên bố nền tảng đã đủ điều kiện, server đã được provision, admin đầu tiên đã triển khai, hosting end-to-end đã sẵn sàng, hoặc Windows native đã sẵn sàng.

## Phương án khác (Tiếng Việt)

- Coi Host là chế độ client cục bộ đặc biệt đọc trực tiếp PostgreSQL — bác bỏ vì bỏ qua ranh giới API/auth thông thường và thay đổi authority metadata.
- Tạo phiên bản Home và Server riêng — bác bỏ vì sẽ chia tách protocol và hành vi an toàn dữ liệu.
- Yêu cầu mọi người dùng tự cấu hình PostgreSQL và service — bác bỏ vì chi tiết triển khai hạ tầng không phải lựa chọn của Personal / Home.
- Chọn database/runtime đóng gói trong P029 — hoãn sang P030 để so sánh bằng chứng về nền tảng, bảo mật, lifecycle, backup, khôi phục và bảo trì.

## Điều kiện di chuyển và xem xét lại (Tiếng Việt)

P030–P036 và UX lần chạy đầu về sau phải tuân thủ hợp đồng sản phẩm đã liên kết. Thay đổi ý nghĩa Host/Connect, authority PostgreSQL, tính tương thích giữa profile, ownership bền vững, readiness, bảo toàn dữ liệu hoặc ranh giới trust cần ADR thay thế. Cơ chế cụ thể về provisioning, cấu hình, storage, service, mạng và bootstrap admin thuộc các prompt được giao và không cần mở lại ADR này nếu tuân thủ hợp đồng.
