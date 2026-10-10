# Nâng cấp, sửa chữa và gỡ cài đặt

> **Kiểm thử vòng đời v0.2 vẫn đang mở.** Các trang này mô tả chính sách dự kiến và giới hạn mã nguồn đã kiểm tra. Với thao tác v0.1 hiện hành, dùng [chính sách gói cài đặt](../RELEASE_PACKAGING.md) và [hướng dẫn an toàn nâng cấp](../UPGRADE_SAFETY.md).

## Các thao tác trong vòng đời

| Thao tác | Quy trình v0.2 dự kiến | Trạng thái hiện tại của bản phát hành |
| --- | --- | --- |
| Cài mới | Dùng tệp chính thức và quy trình Setup hoặc trình cài của hệ điều hành trong [Cài đặt](INSTALLATION.md). | Mã nguồn bộ tạo đã có; tệp v0.2 chưa được phát hành. |
| Sửa chữa cùng phiên bản | Dùng Repair của Windows Setup hoặc quy trình bảo trì gói của hệ điều hành nếu bản phát hành liệt kê rõ. | Đã có quy tắc vòng đời trong mã nguồn; sửa chữa trên hệ điều hành thật chưa được chứng nhận trên toàn ma trận. |
| Nâng cấp tương thích | Chỉ dùng bản phát hành mới hơn và phiên bản nguồn được metadata phát hành xác thực liệt kê là tương thích. | Chưa công bố channel production và bằng chứng tương thích v0.1 lên v0.2; không tự cho rằng được hỗ trợ. |
| Phục hồi cài đặt bị gián đoạn | Dùng luồng phục hồi Setup hoặc đối chiếu qua trình quản lý gói của hệ điều hành được nêu tên. | Phục hồi phải kiểm tra kết quả trước đó rồi mới thao tác tiếp; kiểm thử trên hệ điều hành thật khi bị gián đoạn chưa hoàn tất. |
| Gỡ cài đặt thông thường | Dùng trình gỡ cài đã đăng ký của Windows, nút Remove trong ứng dụng cài gói của hệ điều hành, hoặc xóa AppImage và chỉ phần tích hợp đã chủ động bật. | Bảo toàn trạng thái là contract v0.2; chưa được chứng nhận đầy đủ trên các hệ điều hành thật. |
| Cài đặt lại | Chỉ cài lại sau khi quy trình Repair/gỡ cài được hỗ trợ xác định trạng thái đã cài. | Dữ liệu người dùng hiện có không phải tệp cài đặt có thể bỏ đi. |
| Hạ cấp | Không cam kết hỗ trợ hạ cấp. | Dừng lại nếu bản phát hành không liệt kê rõ lifecycle cho đúng cặp phiên bản nguồn/đích. |

Không dùng bộ cài khác để ghi đè cài đặt chưa rõ chủ sở hữu hoặc phiên bản. Không hạ cấp bằng cách cài gói cũ hoặc chỉ khôi phục một phần cơ sở dữ liệu.

## Tệp thuộc ứng dụng và dữ liệu thuộc người dùng

| Tệp thuộc ứng dụng | Dữ liệu thuộc người dùng hoặc dữ liệu bền vững |
| --- | --- |
| Tệp thực thi desktop/client Synveil đã xác minh, các thành phần chạy được đóng gói, shortcut/mục menu và tích hợp do bộ cài tạo, đã được ghi nhận là của Synveil. | Thư viện và tệp cá nhân, hồ sơ cục bộ và trạng thái đồng bộ, thông tin đăng nhập đã lưu, cấu hình máy chủ, cơ sở dữ liệu và dữ liệu đối tượng máy chủ, bộ nhớ ngoài và bản sao lưu. |

Repair chỉ được khôi phục tệp và tích hợp đã biết chủ sở hữu. Repair phải giữ lại dữ liệu, cài đặt thuộc người dùng. Gỡ cài đặt thông thường xóa tệp và tích hợp đã biết của ứng dụng; đây không phải thao tác xóa sạch dữ liệu. Contract v0.2 yêu cầu giữ dữ liệu người dùng/máy chủ, nhưng chấp nhận native chưa hoàn tất nên bản xem trước này chưa phải cam kết production. Với hành vi hiện tại của v0.1, làm theo chính sách phát hành v0.1 liên kết phía trên.

Trước khi chạy quy trình quản trị riêng có tính phá hủy, hãy xác định chính xác loại dữ liệu Synveil thuộc phạm vi bị xóa, tạo và xác minh bản sao lưu cần thiết, đồng thời xác nhận quy trình nêu rõ đích xóa. Không coi nút Uninstall thông thường là cho phép xóa dữ liệu. Hướng dẫn này không quảng bá thao tác xóa toàn bộ dữ liệu desktop.

Bộ script lifecycle package-neutral trong repository có thao tác `--purge` riêng cho các đường dẫn cấu hình/trạng thái máy chủ nằm trong allowlist. Đây là giao diện đóng gói chỉ dành cho quản trị viên, không phải lệnh dọn desktop; không dùng trong gỡ cài thông thường. Phạm vi cụ thể và ranh giới giữ dữ liệu bên ngoài được mô tả trong [tài liệu tham chiếu lifecycle package](../../../deploy/install/README.md).

## Nếu quá trình thiết lập bị gián đoạn

Có thể bắt đầu lại tải qua mạng chỉ khi thao tác cài chưa bắt đầu thay đổi dữ liệu. Nếu Setup, package manager hoặc thiết lập máy chủ có thể đã đổi trạng thái, hãy dùng thao tác phục hồi của thành phần đó để xác định việc gì đã xảy ra. Giữ bằng chứng installer/journal ban đầu; không xóa trạng thái chưa rõ, cơ sở dữ liệu package, thư mục thư viện hoặc tệp cơ sở dữ liệu. Nếu phục hồi không xác định được trạng thái an toàn, hãy dừng lại và tìm hỗ trợ.

Rollback tệp ứng dụng không đồng nghĩa rollback cơ sở dữ liệu. Nâng cấp tương thích phải được hỗ trợ rõ ràng; không ép ứng dụng cũ dùng schema mới hơn. Với phục hồi quản trị, chỉ dùng quy trình backup/restore phối hợp đã được tài liệu hóa.
