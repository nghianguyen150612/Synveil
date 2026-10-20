# Mức độ sẵn sàng phân phối v0.2

### Trạng thái phát hành R05 — 2026-10-20

**`R05_INTEGRATED_RELEASE_BLOCKED`.** Synveil `v0.2.0` chưa được phát hành.
Hiện chưa có stable tag, GitHub Release công khai, bản tải xuống đã xác thực
hoặc stable channel được kích hoạt. Các đầu ra ứng viên v0.2 là đầu vào CI
chưa ký; không cài đặt chúng và không phân phối archive artifact của Actions.

Hướng dẫn cài đặt công khai hiện tại vẫn là hướng dẫn cho bản đã phát hành
`v0.1.0`. Bản ghi điều kiện và vấn đề chặn R05 nằm trong
[manifest phát hành R05](../../v0.2/RELEASE_HARDENING_R05_MANIFEST.md).
Trang này mô tả các mục tiêu cần chứng nhận và giới hạn hiện tại, không tuyên
bố các mục tiêu đó đã được hỗ trợ.

## Các mục tiêu cần chứng nhận

| Định dạng ứng viên | Mục tiêu | Trạng thái hiện tại |
| --- | --- | --- |
| `SynveilSetup.exe` | Windows 11 `x86_64` | **Blocked; Not yet published.** Bản build chính xác từ main sau tích hợp đã tạo Setup ứng viên chưa ký và bước kiểm tra sản phẩm với người dùng tiêu chuẩn đã đạt trên GitHub-hosted Windows Server 2025. Workflow sau đó đã chặn đúng vì thiếu giao diện GUI tương tác, đăng nhập thật và các bề mặt named pipe. Bằng chứng Windows 11 bắt buộc vẫn còn thiếu. |
| `synveil_<version>_amd64.deb` | Ubuntu 24.04 `x86_64` | **Blocked; Not yet published.** Chưa có ứng viên đã xác thực hoàn tất hành trình desktop native và vòng đời cài đặt. |
| `synveil-<version>-1.x86_64.rpm` | Fedora 42 `x86_64` | **Blocked; Not yet published.** Chưa có ứng viên đã xác thực hoàn tất hành trình desktop native và vòng đời cài đặt. |
| `Synveil-<version>-x86_64.AppImage` | Linux `x86_64` | **Blocked; Not yet published.** Build chính xác trên integrated-main, tái lập độc lập, kiểm tra manifest/runtime, smoke và vòng đời người dùng hiện tại đã đạt trên CI Ubuntu 22.04; kiểm tra khởi chạy native, di chuyển vị trí và vòng đời trên Ubuntu 24.04/Fedora vẫn còn thiếu. |

`Implemented` mô tả hành vi trong source. `Build validated` mô tả một kiểm tra
build đã hoàn tất. `Native tested` mô tả một kiểm tra máy có phạm vi giới hạn.
`Native qualified` yêu cầu hoàn tất toàn bộ hành trình máy sạch áp dụng.
`Blocked` ghi nhận bằng chứng bắt buộc còn thiếu. `Not yet published` nghĩa là
chưa có bản tải v0.2 công khai. `Unsupported` nghĩa là kế hoạch phát hành này
không đưa ra cam kết tương thích.

Không mục tiêu v0.2 nào đạt `Native qualified`. Không được dùng danh sách mục
tiêu trên làm cam kết hỗ trợ hoặc nguồn tạo liên kết tải xuống.

## Connect và Host

**Connect** là kết nối ứng dụng desktop với máy chủ Synveil đang có.
**Host** là thiết lập máy chủ Synveil được quản lý trên thiết bị này. Đây là
các chế độ sản phẩm, chưa phải hành trình cài đặt v0.2 đã được chứng nhận.
Việc chấp nhận first-run cho Connect và Host vẫn bị chặn.

Chứng nhận Managed Host cần đầu vào Synveil server, PostgreSQL 17 và Caddy xác
thực, đã ký cho Ubuntu 24.04 và Fedora 42; đồng thời cần public keyring được
phê duyệt, fingerprint publisher được ghim độc lập, bằng chứng license và
notice đầy đủ, cùng inventory P036 tám mục đã ký và vượt qua validator. Các
đầu vào trust production và bằng chứng chấp nhận Host sau cài đặt chưa được
cung cấp.

## Nâng cấp, sửa chữa và dữ liệu

Các policy cho Windows và package Linux có danh sách `upgrade_from` rỗng.
Chưa có nâng cấp tự động từ `v0.1.0` lên `v0.2.0` nào được phê duyệt hoặc kiểm
thử. Sửa chữa, gỡ/cài lại và bảo toàn dữ liệu chưa được chứng nhận trên toàn
bộ ma trận mục tiêu. Chưa có rollback cơ sở dữ liệu nào đạt. Hãy giữ bản sao
lưu đã xác minh của dữ liệu đang phát hành và không thử installer ứng viên trên
bản dữ liệu duy nhất.

## Chữ ký và tải xuống

Chưa có chữ ký Authenticode, RPM/OpenPGP hoặc Ed25519 production, cũng chưa có
policy public key v0.2 được tin cậy độc lập. Không có metadata stable-channel
hay checksum v0.2 công khai. Build xanh, archive CI hoặc manifest ứng viên
chưa ký không chứng minh danh tính publisher.

Trước khi tin cậy một bản tải trong tương lai, cần phát hành artifact đã ký
chính xác cùng manifest được xác thực, sau đó xác minh byte tải xuống, chữ ký,
source identity và generation của channel. Chưa thể hoàn tất quy trình đó khi
các tệp chưa được phát hành.

## Nền tảng không hỗ trợ và hướng dẫn hiện tại

Kế hoạch phát hành v0.2 này không chứng nhận Debian như một mục tiêu riêng,
các bản phân phối Linux khác, `ARM64/aarch64`, hệ thống 32-bit, `macOS`,
`Android` hoặc `iOS`. Có source không đồng nghĩa với được hỗ trợ. Bản phát hành
này không tuyên bố có bản Android hoặc iOS trên store.

Tiếp tục dùng hướng dẫn `v0.1.0` đã phát hành trong
[release notes v0.1](../RELEASE_NOTES_v0.1.md). Có thể xem source tree trong
[README của repository](../../../README.md). Hiện chưa có bước cài đặt, liên
kết tải xuống hoặc checksum phát hành v0.2.
