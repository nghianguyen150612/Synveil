# ADR-011: Open-source licensing policy / Chính sách giấy phép nguồn mở

- Status / Trạng thái: **Proposed / Đề xuất**
- Date / Ngày: 2026-08-21
- Owner / Chủ sở hữu: Project owner, Legal review

## Context (English)

The repository currently carries an MIT License. The desired sustainable model
prefers keeping improvements to a network-served core available while allowing
broad adoption of selected integration SDKs. Changing a license is a legal and
community decision, not an architecture-only edit.

## Proposed decision (English)

Evaluate an explicit future split:

- AGPL-3.0-or-later for core server and web application;
- Apache-2.0 for clearly separated SDKs/client libraries where permissive use
  materially improves interoperability;
- separately licensed third-party/model/assets retained under their terms.

Until the owner approves a rights-compatible relicensing procedure, the
existing MIT `LICENSE` remains authoritative and no conflicting license file or
header is added.

## Consequences and gates (English)

AGPL network-source obligations support an open service core but can affect
commercial embedding and contribution expectations. Apache SDK boundaries must
not become a loophole that contains the server. Mobile/App Store distribution
needs specialist review because copyleft conditions and store terms may
conflict. Dual licensing requires consolidated contributor rights/CLA or
another valid permission model and transparent governance. Managed hosting,
support, SLA, managed backup, enterprise SSO, and administration remain possible
without crippling self-hosted core features.

**Owner decision required before:** accepting external contributions under a
new policy, adding license headers, or publishing differently licensed crates.

## Bối cảnh (Tiếng Việt)

Repository hiện dùng MIT License. Mô hình bền vững mong muốn giữ cải tiến của
core phục vụ qua mạng ở trạng thái mở, đồng thời cho phép SDK tích hợp được dùng
rộng rãi. Đổi giấy phép là quyết định pháp lý/cộng đồng, không chỉ là sửa kiến
trúc.

## Quyết định đề xuất (Tiếng Việt)

Đánh giá cách tách rõ ràng trong tương lai:

- AGPL-3.0-or-later cho core server và web;
- Apache-2.0 cho SDK/client library tách biệt khi giấy phép rộng thực sự tăng
  tính tương tác;
- dependency/model/asset bên thứ ba giữ điều khoản riêng.

Cho đến khi owner phê duyệt quy trình đổi giấy phép tương thích quyền tác giả,
`LICENSE` MIT hiện tại vẫn có hiệu lực và không thêm giấy phép/header xung đột.

## Hệ quả và gate (Tiếng Việt)

Nghĩa vụ source khi phục vụ mạng của AGPL giúp core mở nhưng ảnh hưởng nhúng
thương mại và kỳ vọng contributor. Ranh giới SDK Apache không được trở thành lỗ
hổng chứa logic server. Phân phối App Store/mobile cần chuyên gia xem xét vì
copyleft có thể xung đột điều khoản store. Dual license cần quyền contributor
tập trung/CLA hoặc cơ chế cho phép hợp lệ và governance minh bạch. Managed
hosting, support, SLA, managed backup, enterprise SSO/admin vẫn khả thi mà không
làm core self-host mất công dụng.

**Cần quyết định của owner trước khi:** nhận đóng góp ngoài theo chính sách mới,
thêm header giấy phép hoặc phát hành crate có giấy phép khác.

Alternatives / Phương án: retain MIT everywhere; Apache-2.0 everywhere; AGPL
everywhere. Legal review decides—this ADR does not change the current license.
