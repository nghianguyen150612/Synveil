# Định nghĩa sản phẩm

- Trạng thái: **Blueprint quy chuẩn**
- Sản phẩm: **Synveil**
- Tagline: **Your data. Your devices. Your cloud.**

## Định nghĩa

Synveil là private cloud đáng tin cậy mà bất kỳ ai cũng có thể self-host, giúp
người dùng sở hữu, bảo vệ, đồng bộ, tổ chức và hiểu dữ liệu cá nhân hoặc dữ liệu
nhóm nhỏ trên nhiều thiết bị.

Nền tảng kết nối file, lịch sử phiên bản, backup, thiết bị, photos, một số dự án
phát triển và lớp intelligence tùy chọn trong cùng sản phẩm nhưng vẫn giữ ranh
giới subsystem rõ ràng. Đây không phải một nhóm tính năng chỉ dùng chung logo:
nền tảng chung là ObjectStore có thể kiểm chứng, metadata giao dịch, identity và
authorization, nhật ký hoạt động cùng quy trình phục hồi.

Sản phẩm phải hoạt động dù không tồn tại dịch vụ Synveil hosted độc quyền. File,
sync, backup và restore lõi vẫn phải dùng được khi AI bị tắt hoặc gặp sự cố.

Trải nghiệm mục tiêu là sự tiện dụng của cloud tiêu dùng cộng với quyền sở hữu
self-hosted, sync đáng tin cậy và backup/recovery an toàn. Sản phẩm phải tiếp
cận được với user không chuyên nhưng không lấy đi quyền kiểm soát nâng cao của
self-hoster.

## Vấn đề và cam kết

Người dùng thường phân tán dữ liệu qua cloud tiêu dùng, hệ thống backup riêng
của thiết bị, thư mục NAS, thư viện ảnh, ổ rời và code forge. Kết quả là quyền sở
hữu không rõ, đường restore mong manh, dữ liệu trùng lặp và khó biết bản sao nào
đang hiện hành.

Cam kết của Synveil được giới hạn để có thể kiểm thử:

- người dùng tìm được dữ liệu logic và lịch sử mà không phải hiểu object key;
- dữ liệu đã được chấp nhận là committed phải bền vững theo backend cấu hình và
  kiểm tra được bằng checksum;
- sync không bao giờ âm thầm bỏ một chỉnh sửa nội dung đồng thời;
- backup retention không thừa hưởng ngữ nghĩa xóa của sync;
- operator self-host theo dõi được health, di chuyển/khôi phục dữ liệu bằng định
  dạng và quy trình đã ghi, đồng thời nâng cấp mà không xóa metadata;
- indexing tùy chọn giúp tìm dữ liệu dễ hơn nhưng không trở thành cách tìm duy
  nhất hoặc kênh âm thầm gửi dữ liệu ra ngoài.

## Người dùng và môi trường dự kiến

Audience hạng nhất gồm technical user, power user, gia đình, người dùng tại nhà,
sinh viên, creator, developer, nhóm nhỏ và user không chuyên. Synveil có thể
chạy trên máy Windows/macOS cá nhân, Linux Desktop, Linux Server, NAS, home
server, VPS hoặc dedicated server. Personal / Home Mode là trải nghiệm native/
guided trong tương lai; Advanced / Server Mode gồm Docker Compose và deployment
do operator quản lý. Compose ban đầu vẫn dùng một PostgreSQL primary và local/NAS
path được cấu hình; S3-compatible là adapter dự kiến chứ không buộc public cloud.

Trong tương lai có thể kết nối:

- trình duyệt cho file, photos, backup, thiết bị, search và administration;
- client Windows, macOS, Linux cho sync/backup thư mục chọn lọc, lý tưởng là
  dùng chung core giao thức/máy trạng thái viết bằng Rust;
- client Android, iPhone và iPad trong tương lai, dùng API native và tôn trọng
  giới hạn background/permission thực tế;
- tích hợp Forgejo để kiểm kê và backup repository.

Synveil ban đầu không phải control plane SaaS multi-tenant cấp doanh nghiệp hay
relay hosted bắt buộc do Synveil vận hành. Ownership và authorization vẫn phải
chặt chẽ để không khóa đường phát triển cho sharing hoặc hosted deployment sau này.

## Nguyên tắc sản phẩm

### Dễ dùng mặc định

User thường có guided installation, chọn storage, bootstrap account, device
pairing, health status, maintenance và recovery dễ hiểu mà không phải quản trị
infrastructure. Journey chính không được terminal-first.

### An toàn mặc định

Automation có thể che độ phức tạp nhưng không che phạm vi destructive hoặc làm
yếu durability, authorization, backup, update và recovery.

### Thiết kế đa nền tảng

Windows, macOS, Linux Desktop và Linux Server là host target hạng nhất. Service
manager, credential store, path semantic và storage accelerator theo platform
nằm sau adapter/capability contract.

### Mạnh khi cần

Advanced user vẫn có Docker Compose, PostgreSQL tùy chỉnh, S3/MinIO, NAS,
reverse proxy/TLS, CLI, API, diagnostics chi tiết và filesystem optimization.
Xem [PLATFORM.md](PLATFORM.md).

### Self-hosted trước tiên

Luôn phải có một deployment production nâng cao được hỗ trợ bằng Docker Compose,
đồng thời mô hình user dài hạn có native/guided Personal / Home installation.
Chỉ mô tả Kubernetes như hướng scale sau khi có nhu cầu đo được; không biến nó
thành điều kiện chức năng.

### Người dùng sở hữu dữ liệu

Vị trí storage, retention, sharing, việc dùng AI từ xa, integration và hành vi
recovery phải minh bạch, cấu hình được. Byte file chuẩn không bị nhốt trong
PostgreSQL hay representation độc quyền không tài liệu.

### Tính đúng đắn trước sự “thông minh”

Storage, upload, sync, backup, restore, trash và version history được ưu tiên
hơn recommendation, semantic indexing, nén nâng cao, chunk dedup và tiering.
Mọi tối ưu làm yếu durability hoặc recovery đều bị loại.

### Ranh giới lỗi mô-đun

AI, làm giàu photos, repository polling, thumbnail và consumer khác chạy bất
đồng bộ. Sự cố chỉ làm tính năng dẫn xuất stale, không làm nội dung gốc đã commit
mất khả dụng.

### Trạng thái trung thực

Màn hình, tài liệu, release note và API dùng chung `IMPLEMENTED`, `IN PROGRESS`,
`PLANNED`, `EXPERIMENTAL`, `NON-GOAL`. Blueprint không biến một khả năng thành
đã triển khai.

### Hợp đồng đa nền tảng

Web, desktop và mobile dùng cùng HTTP contract có phiên bản cùng ID mờ đục.
Server không giả định path POSIX, iOS được chạy background vô hạn hay mọi nền
tảng đều hỗ trợ placeholder native.

### Progressive disclosure

UI thường dùng Files, Photos, Backups, Devices, Shared, Search, Activity,
Settings và Health. PostgreSQL, object store, sync cursor, GC, compression, S3,
reflink và maintenance chỉ hiện trong `Settings → Advanced` hoặc admin
diagnostics. Core user-facing feature không được buộc user thường mở terminal.

## Các miền sản phẩm

| Miền | Kết quả cho người dùng | Mức ưu tiên ban đầu |
|---|---|---|
| Identity và devices | Biết ai/thiết bị nào hành động; thu hồi truy cập. | Core |
| Drive | Tổ chức/truyền file, folder với kiểm tra toàn vẹn. | Core |
| Versions và trash | Hoàn tác thay đổi, phục hồi dữ liệu xóa mềm. | Core |
| Sync | Hội tụ Library được chọn mà không ghi đè im lặng. | Core |
| Backup và restore | Giữ lịch sử thiết bị theo retention rõ ràng. | Core |
| Sharing | Cấp quyền có giới hạn, thu hồi được và public link. | Near-term |
| Photos | Giữ original, cung cấp timeline/albums/renditions. | Near-term |
| Tối ưu storage | Giảm dung lượng vật lý mà không đổi sự thật logic. | Near-term/advanced |
| Search | Luôn có metadata search; bổ sung full-text/semantic theo giai đoạn. | Near-term/advanced |
| Tích hợp code | Liên kết Forgejo và artifact được bảo vệ với Project. | Advanced |
| AI | OCR, embedding, tag và hiểu repository tùy chọn. | Advanced |
| Smart storage | Files on demand và tiering tất định. | Advanced |
| Thí nghiệm intelligence/scale | Anomaly signal, model experiment và scale-out nhiều node. | Experimental |

`FEATURES.md` là danh mục đầy đủ. Tên miền là ranh giới hợp đồng, không có nghĩa
mỗi miền phải thành network service riêng.

## Kiến trúc thông tin cho người dùng

Web có thể được đưa vào dần theo cấu trúc:

```text
Login
Dashboard
Files / My Drive / Shared / Recent / Favorites / Trash
Photos / Timeline / Albums / Search
Backups / Devices / Snapshots / Restore / Policies
Devices
Code / Repositories / Projects / Git Servers
Search
Activity
Settings / Account / Storage / Security / Integrations / AI
```

Favorite là bookmark cá nhân theo từng user, không phải metadata file dùng
chung; nó không bao giờ cấp access hoặc giữ content sống sau khi mọi retention
reference kết thúc.
View Shared discover received grant hiện còn authorize mà không đòi user biết
share ID hoặc node ID nội bộ.
Recent nghĩa là node hiện đọc được ordered theo mutation server đã commit,
không phải lịch sử không công bố về nội dung user đã xem hay download.

Navigation giai đoạn sớm phải ẩn hoặc ghi rõ section chưa dùng được. Mock rỗng
không được làm người dùng hiểu rằng dữ liệu đã được bảo vệ.

## Hành trình chính

### Cài đặt ban đầu

```text
Personal / Home Mode:
  cài đặt
  → chọn storage location
  → Synveil-managed service và PostgreSQL
  → tạo account
  → xác minh health
  → kết nối device

Advanced / Server Mode:
  lấy bản release đã pin
  → cấu hình database/object path và secret
  → start Compose/native service có tài liệu
  → mở setup route qua TLS
  → tạo bootstrap administrator một lần
  → xác minh storage và database health
  → tạo Library đầu tiên
```

Setup thường không được yêu cầu sửa SQL, quản trị PostgreSQL, sửa environment
variable, cấu hình reverse proxy hay port forwarding. Advanced setup có thể lộ
các control đó. Installer/package command cụ thể vẫn là `PLANNED` cho đến khi có
release artifact; `git clone` là đường phát triển, không phải mô hình upgrade
production duy nhất.

### Kết nối thiết bị tương lai

```text
cài client đáng tin
→ scan/nhập pairing code ngắn hạn sau khi xác minh server identity
→ approve và đăng ký Device có tên
→ nhận device credential có scope, thu hồi được
→ chọn chính sách sync và/hoặc backup riêng
→ lấy inventory ban đầu nhất quán
→ bắt đầu nhận change theo cursor và gửi trạng thái
```

Pairing là security-sensitive: phải thiết kế lifetime code, replay, approval,
device identity, credential issuance, revoke, pairing local/remote và MITM
protection trước khi promote convenience.

### Phục hồi sau khi mất thiết bị

Người dùng revoke credential thiết bị mất, kiểm tra audit, đăng ký thiết bị thay
thế, chọn `BackupSnapshot` committed hoặc lịch sử Drive, restore đến đích không
phá dữ liệu và nhận kết quả kiểm checksum. Thu hồi credential Synveil không có
nghĩa xóa được cả hệ điều hành.

### Remote access

Remote access dự kiến là lựa chọn nhiều lớp: LAN discovery, direct connection,
NAT traversal an toàn, VPN/tailnet do user sở hữu tùy chọn và Advanced / Server
networking thủ công. Relay/coordination service tùy chọn vẫn là quyết định mở,
ưu tiên self-hosted. Synveil phải dùng được khi không có dependency hosted độc
quyền và UI phải nói rõ connection đang local, ready, degraded hay cần admin
mạng.

## Non-goal giai đoạn sớm

Synveil ban đầu không trở thành:

- giải pháp thay thế hoàn chỉnh cho iPhone backup hoặc backup cả OS;
- dịch vụ đồng bộ iMessage;
- GitHub/Forgejo thay thế hay Git transport tự viết;
- office suite, chat platform hoặc hệ workflow chung;
- distributed filesystem/consensus database Kubernetes-native;
- enterprise IAM suite;
- hệ thống zero-knowledge mã hóa mọi thứ;
- nền tảng transcoding media đầy đủ;
- relay hoặc control plane Synveil-hosted bắt buộc;
- installer operating-system hoặc custom filesystem trong blueprint phase này.

Các ranh giới này giảm nguy cơ mất dữ liệu và giữ core recovery có thể kiểm thử.

## Bằng chứng thành công và phát hành

Không dùng con số throughput/scale tự nghĩ ra. Gate phát hành yêu cầu bằng chứng:

- topology công bố triển khai được và nâng cấp được từ bản hỗ trợ trước;
- write được chấp nhận sống sót qua các crash point đã test, hoặc nhìn thấy và
  xác minh được, hoặc chỉ là staging không tham chiếu có thể dọn an toàn;
- retry sau khi mất response là idempotent;
- kịch bản sync hội tụ và giữ byte conflict;
- snapshot backup tuân retention độc lập với xóa live, restore có verify;
- test authorization/quota bao phủ mọi mutation và download;
- health, metric, audit và recovery runbook làm lỗi quan sát được;
- user thường có thể cài, chọn storage, pair device, xem health, update, gỡ mà
  không xóa data, chuyển máy và recovery qua guided flow khi capability đã được
  implementation;
- status công khai cùng docs tiếng Anh/Việt khớp bằng chứng đã ship.

`TESTING.md` định nghĩa hạng mục benchmark và regression budget; mục tiêu được
đặt từ baseline Phase 1 trên phần cứng ghi rõ.

## Hướng nguồn mở bền vững

Core dự kiến luôn hữu ích cho self-hoster không cần module trả phí. Dịch vụ kinh
doanh có thể gồm managed hosting, support/SLA, managed backup, enterprise admin
hoặc SSO. Các dịch vụ này không được bắt người dùng phụ thuộc control plane độc
quyền để truy cập hay export thông thường.

Giấy phép pháp lý hiện tại là MIT. Hướng chia AGPL/Apache chỉ là quyết định mở
của owner trong ADR-011, bao gồm hệ quả contributor và App Store.

## Rủi ro sản phẩm

Rủi ro lớn nhất là ngữ nghĩa sync mơ hồ, lệch metadata/Object, restore chưa được
thử, giới hạn background của nền tảng, E2EE xung đột tính năng server, rò rỉ dữ
liệu qua AI và scope phình to. Kiến trúc kiểm soát bằng bất biến rõ ràng, version
bất biến, conflict bảo thủ, backup tách biệt, service tùy chọn bất đồng bộ, gate
theo giai đoạn và `OPEN DECISION` có hạn.
