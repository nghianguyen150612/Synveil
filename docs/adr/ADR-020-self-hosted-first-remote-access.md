# ADR-020: Self-hosted-first remote access / Remote access ưu tiên self-hosted

- Status / Trạng thái: **Proposed / Đề xuất**
- Date / Ngày: 2026-08-22
- Owners / Chủ sở hữu: Networking / Connectivity, Security, Product

## Context (English)

NAT, CGNAT, DNS, firewalls, TLS, and changing home networks are adoption
barriers for non-technical users. A proprietary mandatory relay would reduce
friction but could turn a self-hosted product into a silent SaaS dependency and
would create a new metadata/content trust boundary.

## Proposed decision (English)

Use a layered remote-access architecture: LAN discovery, direct connections
when the operator has configured them, safe NAT traversal where feasible,
user-owned VPN/tailnet-style integration, and documented Advanced / Server Mode
networking. Evaluate an optional coordination/relay service only as a separately
disclosed, failure-tolerant convenience layer. Core local, LAN, and manual
access must remain useful when it is unavailable and no proprietary Synveil
service may be mandatory.

Before selection, the protocol and product disclosure must specify relay
metadata, whether file contents traverse it, encryption/authorization,
self-hostability, retention, cost/abuse controls, outage behavior, and how a
user exits to a direct/VPN/manual path. Pairing and remote access must preserve
server-identity verification, credential scope, revocation, and audit.

## Bối cảnh (Tiếng Việt)

NAT, CGNAT, DNS, firewall, TLS và mạng gia đình thay đổi là rào cản adoption
cho user không chuyên. Relay bắt buộc độc quyền có thể giảm ma sát nhưng biến
product self-hosted thành SaaS dependency âm thầm và thêm trust boundary về
metadata/content.

## Quyết định đề xuất (Tiếng Việt)

Dùng kiến trúc remote access nhiều lớp: LAN discovery, direct connection khi
operator đã cấu hình, NAT traversal an toàn khi khả thi, VPN/tailnet-style do
user sở hữu và networking Advanced / Server có tài liệu. Chỉ xem relay/
coordination tùy chọn như convenience layer disclosure riêng và chịu failure.
Local, LAN và manual access của core phải vẫn dùng được khi relay unavailable;
không service Synveil độc quyền nào là bắt buộc.

Trước khi chọn cơ chế, protocol và product disclosure phải nêu relay thấy
metadata gì, file content có đi qua không, encryption/authorization,
self-hostability, retention, cost/abuse, outage behavior và cách chuyển sang
direct/VPN/manual. Pairing/remote access vẫn phải giữ server-identity
verification, credential scope, revoke và audit.

## Alternatives / Phương án khác

Mandatory Synveil relay; port forwarding as the only experience; promising that
all NAT/CGNAT environments are automatically solved; hiding relay operation from
users.

Relay Synveil bắt buộc; chỉ dùng port forwarding; hứa giải quyết tự động mọi
môi trường NAT/CGNAT; che giấu relay với user.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

The recommendation remains open until NAT/CGNAT, threat, privacy, abuse-cost,
usability, self-hosted-relay, and outage tests exist. No client may silently
depend on a hosted endpoint before this ADR is accepted and the corresponding
security and product disclosures are reviewed.

Khuyến nghị vẫn mở cho đến khi có test NAT/CGNAT, threat, privacy,
abuse-cost, usability, self-hosted relay và outage. Client không được âm thầm
phụ thuộc endpoint hosted trước khi ADR được chấp thuận cùng disclosure
security/product tương ứng.
