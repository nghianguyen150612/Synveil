# Thiết lập lần đầu và đồng bộ lần đầu

> **Trạng thái xem trước:** v0.2 chưa được phát hành và quy trình Host đầu-cuối chưa được chứng nhận production. Nội dung này mô tả luồng Connect, đăng nhập, thư viện và tiến độ đã được triển khai, đồng thời nêu rõ giới hạn Host. Hướng dẫn hiện hành cho v0.1 nằm trong [hướng dẫn vận hành](../RELEASE_OPERATIONS.md).

## Chọn cách sử dụng Synveil

Với hồ sơ client mới, Synveil mở màn hình Welcome và đưa ra hai lựa chọn:

- **Connect to Synveil** để kết nối tới máy chủ đã được cài đặt và truy cập được.
- **Host Synveil** để thiết lập máy chủ được quản lý trên thiết bị này theo kế hoạch.

Hồ sơ đã cấu hình sẽ mở ứng dụng bình thường thay vì hiện lại Welcome.

### Kết nối tới máy chủ hiện có

1. Chọn **Connect to Synveil** và nhập địa chỉ máy chủ do quản trị viên cung cấp. Nếu bỏ qua giao thức, Synveil dùng HTTPS an toàn. Kết nối HTTP không mã hóa sẽ bị từ chối.
2. Chọn **Connect**. Synveil kiểm tra máy chủ có thể truy cập và sẵn sàng trước khi lưu kết nối. Nếu bước này thất bại, hãy kiểm tra địa chỉ, mạng, trạng thái máy chủ và chứng chỉ HTTPS hợp lệ. Không tắt kiểm tra TLS.
3. Đăng nhập bằng mã thiết bị dùng một lần do luồng đăng nhập được hỗ trợ trên máy chủ cấp. Chỉ nhập mã trong Synveil. Không bao giờ gửi mã, mật khẩu hoặc nội dung kho thông tin xác thực cho bộ phận hỗ trợ.
4. Nếu máy chủ chưa có thư viện được cấu hình cho thiết bị này, hãy đặt tên thư viện, chọn thư mục cục bộ bằng hộp thoại chọn thư mục của hệ điều hành rồi chọn **Create library**. Các tệp hiện có vẫn nằm nguyên chỗ và có thể được thêm vào thư viện.
5. Đảm bảo thư mục đã chọn luôn khả dụng và có quyền ghi. Synveil bắt đầu công việc đồng bộ thông thường sau khi xác nhận thư viện. Nếu thiết lập bị gián đoạn, hãy chọn lại cùng thư mục để Synveil đối chiếu trạng thái đã có.

Luồng này kết nối với máy chủ hiện có. Nó không cài máy chủ hay cơ sở dữ liệu. Hiện chưa hỗ trợ gắn thiết bị vào một thư viện từ xa tùy ý đã có; quy trình tạo thư viện đầu tiên sẽ tạo thư viện mới và liên kết thư mục trên thiết bị này.

### Host trên thiết bị này

Màn hình Welcome có lựa chọn Host, nhưng điều đó không chứng minh đã có quy trình cài máy chủ hoàn chỉnh. Trên mã nguồn hiện tại, thao tác này báo rằng thiết bị chưa thể Host; hành trình P036 đầu-cuối, bao gồm cấu hình dịch vụ được duyệt, thiết lập quản trị viên ban đầu và chấp nhận native, vẫn đang mở.

Không dựa vào Host như một dịch vụ production cho đến khi bản phát hành chính thức đánh dấu hành trình này đã được chứng nhận. Đừng tự cài PostgreSQL, sửa cài đặt cơ sở dữ liệu hoặc tạo tệp dịch vụ để thay thế luồng Host được quản lý thông thường. Quản trị viên máy chủ v0.1 hiện có nên làm theo [hướng dẫn triển khai hiện hành](../DEPLOYMENT.md).

### Máy chủ nâng cao hoặc do bên ngoài quản lý

Quản trị nâng cao dành cho người vận hành đã sở hữu máy chủ, cơ sở dữ liệu, bộ nhớ, mạng và cấu hình TLS. Contract v0.2 dành riêng một đường dẫn dùng PostgreSQL bên ngoài cho nhóm này; đó không phải quy trình thông thường và cũng không chứng nhận hành trình Host mới. Xem [Cài đặt nâng cao](ADVANCED_INSTALLATION.md) và [hướng dẫn triển khai v0.1 hiện hành](../DEPLOYMENT.md).

## Đọc trạng thái tiến độ

Desktop báo các giai đoạn dựa trên bằng chứng có giới hạn:

| Giai đoạn | Ý nghĩa |
| --- | --- |
| **Synveil ready** | Desktop và tiến trình điều khiển cục bộ đã sẵn sàng. Trạng thái này không báo tiến độ của bộ cài native. |
| **Server ready** | Kết nối máy chủ đã cấu hình có thể sử dụng. |
| **Signed in** | Thiết bị đã xác thực với máy chủ. |
| **Library ready** | Thư viện và thư mục cục bộ trên thiết bị đã được cấu hình. |
| **First sync / Up to date** | Thành phần đồng bộ đã có bằng chứng bền vững rằng công việc hiện tại đã ổn định. |

Tiến độ có thể báo đang chờ hoặc cần thao tác khi mạng không khả dụng, cần đăng nhập, thư mục bị thiếu, có xung đột cần xử lý hoặc đồng bộ đang tạm dừng. Synveil không tự bịa phần trăm hay số lượng tệp. Yêu cầu đồng bộ hoặc một khoảng thời gian tạm thời không có hoạt động không có nghĩa lần đồng bộ đầu đã hoàn tất.

Synveil sẵn sàng sử dụng khi máy chủ, đăng nhập và thư viện đều sẵn sàng, đồng thời giai đoạn đồng bộ lần đầu chuyển sang hoàn tất. Nếu chưa đạt, xem [xử lý sự cố](TROUBLESHOOTING.md); đừng đặt lại thư viện hoặc xóa tệp cục bộ.

## Phục hồi an toàn

- Nếu Connect thất bại, sửa địa chỉ hoặc khôi phục quyền truy cập máy chủ/mạng, sau đó dùng lại thao tác Connect trong ứng dụng khi vấn đề đã được xử lý.
- Nếu đăng nhập thất bại, yêu cầu máy chủ cấp mã thiết bị mới theo luồng được hỗ trợ. Không dùng lại hoặc chia sẻ mã sau khi kết quả không rõ ràng.
- Nếu tạo thư viện bị gián đoạn, chọn lại cùng thư mục để Synveil kiểm tra thiết lập hiện có.
- Nếu thư mục không khả dụng, kết nối lại ổ đĩa hoặc khôi phục quyền truy cập, sau đó dùng **Restore missing folder** nếu ứng dụng hiển thị. Thư mục cục bộ không khả dụng không có nghĩa phải xóa tệp từ xa.
- Nếu thao tác yêu cầu kiểm tra kết quả trước đó, hãy đợi đối chiếu hoặc dùng thao tác sửa chữa/phục hồi được nêu tên. Không tự lặp lại thao tác đã có kết quả không chắc chắn.
