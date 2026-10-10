# Cài đặt nâng cao

> Hướng dẫn này dành cho quản trị viên và kỹ sư phân phối. Người dùng desktop thông thường nên dùng [hướng dẫn cài đặt](INSTALLATION.md). v0.2 chưa phát hành; command, dữ liệu tin cậy công khai và chấp nhận native chưa sẵn sàng cho production.

## Entry point cài Linux qua terminal và độ tin cậy bản phát hành

Repository có entry point quick-install đã xác minh là `deploy/install/quick-install.sh`, gọi `scripts/linux_quick_install.py`. Phần triển khai kiểm tra hệ điều hành và kiến trúc trước khi thay đổi trạng thái, hiện chỉ định nghĩa profile Ubuntu 24.04 x86_64 và Fedora 42 x86_64. Tùy chọn profile chỉ là xác nhận; nó không thể làm cho máy không được hỗ trợ trở thành hợp lệ.

Công cụ yêu cầu vị trí channel tường minh, origin đáng tin cậy, generation tối thiểu của channel và một trong hai: pin SHA-256 của channel được tin cậy độc lập hoặc chính sách khóa công khai Ed25519 cục bộ tường minh. Sau đó công cụ xác thực stable channel, liên kết manifest phát hành chính xác, chọn đúng một artifact tương thích, xác minh byte, stage riêng tư, hiển thị kế hoạch, yêu cầu quyền native hiển thị và xác minh trạng thái cài. Công cụ chạy bằng tài khoản người dùng thông thường; chỉ thao tác package manager native chính xác mới yêu cầu quyền cao hơn.

Parser trong mã nguồn chấp nhận `--channel-url`, `--trusted-origin`, `--minimum-channel-generation` và một trong hai `--trusted-channel-sha256` hoặc `--trust-policy`. Các tùy chọn bổ sung là `--platform-profile` (chỉ `debian-x86_64` hoặc `fedora-x86_64`, và chỉ xác nhận máy đang chạy), `--detect-only` (đầu ra kiểm tra chỉ đọc) và `--yes` (chấp thuận không tương tác sau khi hiển thị kế hoạch). Đây là tên option trong parser repository, không phải lệnh công khai có thể chạy ngay.

Các cơ chế này chưa dùng được làm lệnh cài công khai cho đến khi release engineering công bố vị trí stable channel thật, origin được duyệt, generation đã xác thực, bootstrap tin cậy để xác minh và artifact production phù hợp. Chưa công bố khóa production hoặc endpoint channel nên hướng dẫn này cố ý không đưa lệnh cài có thể chạy, URL, checksum hoặc khóa. Không dùng `curl | sh` hay tin checksum tải từ cùng nguồn chưa xác thực với artifact.

Metadata được hỗ trợ chỉ có stable. Không có channel beta/nightly và không có updater chạy nền. Chọn bản phát hành phải là thao tác tường minh, đã xác thực, tương thích với phiên bản đã cài và có bảo vệ metadata channel cũ hoặc bị rollback. P011 yêu cầu bằng chứng high-water do caller sở hữu; nó không tự tạo endpoint công khai hay gốc tin cậy production.

## Xác thực artifact

Độ tin cậy bắt đầu từ danh tính bản phát hành được thiết lập độc lập với máy chủ tải xuống. Channel đã xác thực liên kết byte manifest chính xác; manifest liên kết loại, nền tảng, kiến trúc, phiên bản, dung lượng và SHA-256 của artifact; artifact được xác minh trước khi cài. Kiểm tra HTTPS và allowed-origin vẫn bật qua các redirect. Checksum không ký đặt cạnh tệp không tự xác thực được chính nó.

Mã nguồn P043 đã triển khai xác minh Ed25519 và kiểm tra chính sách tin cậy cục bộ. Việc cấp khóa production, quản lý khóa riêng, phân phối gốc công khai có chủ đích và metadata channel công khai vẫn là blocker release engineering. Không tự tạo khóa, đoán URL, dùng digest mẫu hoặc lấy artifact CI để lấp chỗ trống.

## Ranh giới runtime AppImage

Bộ tạo AppImage nhắm đến Linux x86_64 và đóng gói Synveil desktop/client cùng runtime Qt đã khóa. Máy chủ vẫn cung cấp kernel Linux tương thích, ELF loader và nền glibc, driver đồ họa/màn hình, phiên X11 hoặc Wayland, phiên DBus người dùng và dịch vụ Secret Service. Gắn AppImage thông thường cần FUSE 2. Mức glibc tương thích chính xác phải được công bố dựa trên artifact cuối; chỉ riêng phiên bản máy build CI không tạo thành tuyên bố hỗ trợ.

`APPIMAGE_EXTRACT_AND_RUN` được chạy thử như phương án CI dự phòng, không phải quy trình cài đặt người dùng thông thường đã được chứng nhận. Không hướng dẫn giải nén thủ công hoặc đổi quyền bằng terminal như cách khắc phục chung. Bản phát hành phải liệt kê yêu cầu runtime và thao tác mở bằng đồ họa chính xác cho từng môi trường được chứng nhận.

## Máy chủ được quản lý và máy chủ do bên ngoài quản lý

Luồng Host Personal/Home dự kiến cung cấp một PostgreSQL 17 riêng do Synveil quản lý thông qua thiết lập có hướng dẫn. Người dùng thông thường không cần cài PostgreSQL, chọn connection string hoặc tự tạo tệp dịch vụ cho luồng này. Chấp nhận Host đầu-cuối P036 và bằng chứng native vẫn còn thiếu nên không được giới thiệu Host được quản lý như đã chứng nhận production.

Advanced/Server dành riêng đường dẫn cấu hình cho PostgreSQL do quản trị viên quản lý, storage tùy chỉnh, bind/origin và tích hợp TLS/reverse proxy bên ngoài. Quản trị viên sở hữu vòng đời cơ sở dữ liệu, thông tin đăng nhập, backup/restore, chính sách firewall và giám sát dịch vụ. Dùng [hướng dẫn triển khai v0.1](../DEPLOYMENT.md) cho quy trình máy chủ đã phát hành; v0.2 phải công bố quy trình máy chủ bên ngoài đã xác thực chính xác trước khi sử dụng.

Không luồng Host nào được tự mở listener công khai, thay đổi router/firewall hoặc thêm relay bắt buộc nếu không có hành động rõ ràng của người dùng và quy trình phát hành đã chứng nhận. Connect yêu cầu máy chủ hiện có, truy cập được và có endpoint HTTPS hợp lệ; phải giữ nguyên kiểm tra TLS.

## Danh tính dịch vụ, quyền và chẩn đoán

Quy trình Windows Setup thông thường là cài theo người dùng, không nâng quyền. DEB/RPM dùng quyền cấp phép hiển thị của package manager hệ điều hành, sau đó desktop chạy bằng người dùng đang đăng nhập. AppImage và tích hợp tùy chọn hoạt động ở phạm vi người dùng. Không chạy desktop hoặc toàn bộ quick installer bằng root/administrator. Package manager vẫn sở hữu cơ sở dữ liệu package; không xóa khóa hay sửa cơ sở dữ liệu đó.

Để chẩn đoán, chỉ dùng nội dung lỗi an toàn có giới hạn và mã hỗ trợ Synveil hiển thị. Không có tự động tải chẩn đoán lên. Báo cáo hỗ trợ có thể nêu phiên bản sản phẩm, phiên bản hệ điều hành, kiến trúc, thao tác đã làm và mã hỗ trợ. Hãy che tên người dùng, đường dẫn tuyệt đối, tên thư viện, địa chỉ máy chủ, thông tin đăng nhập và tên tệp cá nhân. Hiện chưa cam kết có lệnh xuất chẩn đoán.

## Giới hạn phục hồi, rollback và phân phối

Kết quả thao tác đã thay đổi trạng thái nhưng không rõ ràng phải được đối chiếu trước khi thử lại. Trạng thái native package vẫn do package manager sở hữu. Giữ bằng chứng giao dịch/phục hồi và dùng Repair theo nền tảng; không xóa tệp khóa, reset cơ sở dữ liệu hoặc phát lại cài đặt bị gián đoạn một cách mù quáng.

Mã nguồn có chính sách lifecycle và phục hồi theo phạm vi, nhưng bằng chứng native đầy đủ khi bị gián đoạn/mất điện vẫn đang mở. Thay package không chứng minh rollback cơ sở dữ liệu; nhìn chung không hỗ trợ hạ cấp. Rollback schema cần chủ thể phục hồi tương thích được tài liệu hóa và backup phối hợp, không phải khôi phục một phần tệp.

Kiểm tra [danh sách mức độ sẵn sàng phân phối](DISTRIBUTION_READINESS.md) trước khi xuất bản artifact. Có mã nguồn, build CI, kiểm tra package hoặc một native test giới hạn không đồng nghĩa đã được chấp nhận trên máy sạch. Contract về tin cậy: [tính toàn vẹn tải xuống](../../v0.2/RELEASE_DOWNLOAD_INTEGRITY.md), [chọn channel](../../v0.2/RELEASE_CHANNEL_SELECTION.md) và [quick install Linux](../../v0.2/LINUX_QUICK_INSTALL.md).
