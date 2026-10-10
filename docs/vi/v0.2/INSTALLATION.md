# Cài đặt Synveil v0.2

> **Chỉ là bản xem trước. v0.2 chưa được phát hành.** Sản phẩm hiện đã phát hành là v0.1.0. Để cài đặt ngay, hãy dùng [hướng dẫn vận hành v0.1](../RELEASE_OPERATIONS.md) và [chính sách package v0.1](../RELEASE_PACKAGING.md).

Các bước dưới đây mô tả những cách cài dự kiến cho v0.2. Chỉ làm theo sau khi trang phát hành chính thức công bố đúng tệp cài đặt và hướng dẫn xác minh đáng tin cậy tương ứng. Không cài tệp CI, tệp của bản phát hành nháp hoặc tệp không có hướng dẫn xác minh đáng tin cậy đi kèm. Chưa có kênh tải công khai hay khóa ký production cho v0.2.

## Trước khi bắt đầu

Máy tính để bàn v0.2 nhắm đến x86_64. Ubuntu 24.04 x86_64 và Fedora 42 x86_64 là các ứng viên Linux được nêu cho việc kiểm thử; kiểm thử P045 trên máy thật vẫn đang mở nên chưa hệ điều hành nào được chứng nhận. Debian và các bản phân phối dẫn xuất không được chứng nhận chỉ vì có thể cài gói DEB. Các bản phân phối RPM khác không thuộc mục tiêu Fedora. Chưa có phiên bản Windows cụ thể hoặc nền tảng tương thích Linux phổ thông nào được duyệt để công bố hỗ trợ. Xem [mức độ sẵn sàng phân phối](DISTRIBUTION_READINESS.md).

Nếu trang phát hành chính thức không liệt kê chính xác phiên bản hệ điều hành và kiến trúc của bạn, hãy dừng lại và dùng môi trường được liệt kê. Không suy diễn rằng ARM64/aarch64, hệ thống 32-bit, macOS, iOS, Android hoặc mọi bản phân phối Linux đều được hỗ trợ.

## Windows

1. Trên phiên bản Windows x86_64 được liệt kê, chỉ tải `SynveilSetup.exe` từ mục phát hành chính thức được tài liệu phát hành của Synveil liên kết.
2. Xác thực tệp tải xuống theo hướng dẫn của bản phát hành đó. Chỉ nhìn tên tệp không chứng minh được nguồn gốc. Dừng lại nếu thiếu dữ liệu xác thực hoặc dữ liệu không khớp.
3. Mở Setup đồ họa khi đang đăng nhập bằng tài khoản Windows thông thường của bạn. Cài đặt dự kiến theo người dùng và không cần nâng quyền quản trị.
4. Đọc và chấp nhận điều khoản hiển thị, sau đó xem các lựa chọn: khởi động Synveil khi đăng nhập, tạo lối tắt trên màn hình nền và mở Synveil sau khi Setup hoàn tất. Giữ hoặc đổi từng lựa chọn theo nhu cầu. Setup tự chọn vị trí cài theo người dùng; đổi phạm vi cài đặt hoặc ép cài cho toàn máy không được hỗ trợ.
5. Chọn Install và đợi Setup hoàn tất. Nếu đã chọn Open after installation, Synveil sẽ khởi động; nếu không, mở ứng dụng từ Start menu hoặc lối tắt màn hình nền nếu đã tạo.
6. Tiếp tục với [thiết lập lần đầu](FIRST_RUN.md). Cài đặt hoàn tất có nghĩa ứng dụng có thể mở; điều đó không có nghĩa máy chủ đã được cấu hình hoặc tệp đã đồng bộ.

Mục tiêu Windows v0.2 là `SynveilSetup.exe` cho x86_64. Windows ARM64 và các kiến trúc khác nằm ngoài ma trận v0.2. Chấp nhận native Windows vẫn chưa hoàn tất.

## Ubuntu và các hệ Debian

1. Chỉ dùng DEB khi trang phát hành chính thức liệt kê chính xác phiên bản hệ điều hành và kiến trúc của bạn. Quy tắc đặt tên của bộ tạo gói là `synveil_<version>_amd64.deb`; bản phát hành cung cấp `<version>`, không tự đoán.
2. Xác minh gói theo hướng dẫn xác thực đáng tin cậy của bản phát hành trước khi mở. Checksum đặt cạnh tệp nhưng không được xác thực không đủ để chứng minh nguồn gốc.
3. Trên máy tính để bàn đã được chứng nhận, mở DEB bằng ứng dụng cài gói của hệ điều hành, chọn Install và chấp thuận lời nhắc cấp quyền hiển thị bằng tài khoản được phép cài phần mềm.
4. Khi ứng dụng cài gói báo thành công, mở Synveil từ menu Applications bằng tài khoản người dùng đang đăng nhập. Không chạy giao diện desktop dưới quyền root.
5. Tiếp tục với [thiết lập lần đầu](FIRST_RUN.md).

Ubuntu 24.04 x86_64 là ứng viên kiểm thử, chưa phải hệ điều hành đã được chứng nhận. Debian và các phiên bản hay bản dẫn xuất khác thuộc họ Debian chưa có hỗ trợ v0.2 được xác nhận. Nếu hệ điều hành không có ứng dụng cài package đồ họa, phiên bản không được liệt kê hoặc thông báo cài đặt báo hệ thống không được hỗ trợ, hãy dừng lại; đừng thay bằng lệnh terminal chưa được duyệt.

## Fedora và RPM

1. Chỉ dùng RPM khi trang phát hành chính thức liệt kê chính xác phiên bản Fedora và kiến trúc. Quy tắc đặt tên của bộ tạo gói là `synveil-<version>-1.x86_64.rpm`; dùng đúng tên tệp bản phát hành cung cấp.
2. Xác minh gói theo hướng dẫn xác thực đáng tin cậy của bản phát hành trước khi mở.
3. Mở RPM bằng ứng dụng phần mềm đồ họa của Fedora, chọn Install và chấp thuận yêu cầu cấp quyền hiển thị.
4. Khi ứng dụng cài gói báo thành công, mở Synveil từ menu Applications bằng tài khoản người dùng đang đăng nhập, rồi làm theo [thiết lập lần đầu](FIRST_RUN.md).

Fedora 42 x86_64 là ứng viên kiểm thử, chưa phải hệ điều hành đã được chứng nhận. Định dạng RPM không đồng nghĩa mọi bản phân phối hoặc phiên bản dùng RPM đều được hỗ trợ.

## AppImage trên Linux phổ thông

1. Chỉ dùng AppImage nếu trang phát hành chính thức liệt kê nền tảng tương thích Linux và kiến trúc x86_64 của bạn. Quy tắc đặt tên của bộ tạo gói là `Synveil-<version>-x86_64.AppImage`.
2. Xác minh đúng tệp bằng hướng dẫn xác thực đáng tin cậy của bản phát hành. Dừng lại nếu tệp không xác minh được.
3. Trong trình quản lý tệp, dùng chức năng Trust and Run hoặc cấp quyền thực thi theo tài liệu của hệ điều hành khi hướng dẫn phát hành nêu rõ. Sau đó mở tệp bằng tài khoản người dùng thông thường. Cách cấp quyền bằng terminal không phải quy trình đồ họa đã được chứng nhận.
4. Tiếp tục với [thiết lập lần đầu](FIRST_RUN.md). Nếu trình quản lý tệp không có thao tác mở được tài liệu hướng dẫn, hãy coi môi trường đó là chưa được hỗ trợ và tìm trợ giúp.

Mục tiêu AppImage v0.2 chỉ là x86_64. Khả năng tương thích phụ thuộc vào yêu cầu kernel, glibc, desktop, DBus, Secret Service và FUSE sẽ được công bố. Chưa có nền tảng Linux phổ thông nào được chứng nhận rộng rãi. Xem [Cài đặt nâng cao](ADVANCED_INSTALLATION.md) để biết giới hạn môi trường chạy.

## Sau khi cài đặt

Bước tiếp theo là [thiết lập lần đầu trong ứng dụng](FIRST_RUN.md). Nếu gặp vấn đề khi cài, xem [xử lý sự cố](TROUBLESHOOTING.md). Đừng tự cấu hình PostgreSQL để làm cho quy trình Host thông thường hoạt động; Host quản lý tự động chưa được chứng nhận production.
