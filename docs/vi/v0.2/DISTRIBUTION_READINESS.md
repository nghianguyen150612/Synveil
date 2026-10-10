# Mức độ sẵn sàng phân phối v0.2

**Trạng thái kiểm toán: đã chuẩn bị tài liệu; v0.2 chưa được phát hành.** Sản phẩm hiện đã phát hành vẫn là v0.1.0. Chưa có GitHub Release v0.2, channel tải công khai, khóa ký production hoặc bộ artifact v0.2 cuối cùng. Artifact từ workflow là đầu ra CI unsigned, không phải bản phát hành production đã xác thực.

Baseline tài liệu P046 là `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc`. P045 vẫn là PR bản nháp riêng, đang mở tại `035497c05c145e5c6129e744f9455937069680f4`; kết quả dưới đây chỉ là bằng chứng cho đúng source head đó, không hoàn thành P045.

## Danh sách nền tảng và artifact

| Bề mặt | Tên dự kiến / kiến trúc | Trạng thái và bằng chứng | Tuyên bố phát hành |
| --- | --- | --- | --- |
| Windows Setup | `SynveilSetup.exe`, Windows x86_64 | **Implemented; Build validated** trong run Windows installer P045 [37970834502](https://github.com/nghianguyen150612/Synveil/actions/runs/37970834502). Một số bước standard-user có chạy, nhưng chấp nhận native Windows run [37970834767](https://github.com/nghianguyen150612/Synveil/actions/runs/37970834767) thất bại khi ghi nhận các bề mặt bắt buộc chưa khả dụng. **Native qualified: không.** | **Blocked; Not yet published.** Vẫn cần chốt phiên bản Windows được hỗ trợ và chấp nhận P045. |
| Ubuntu DEB | `synveil_<version>_amd64.deb`, x86_64 | **Implemented; Build validated** trong job artifact Linux P045 của run [37970834983](https://github.com/nghianguyen150612/Synveil/actions/runs/37970834983). Guest Ubuntu 24.04 thất bại trước assertion sản phẩm ở bước boot/readiness. **Native qualified: không.** | Ubuntu 24.04 x86_64 chỉ là ứng viên. **Blocked; Not yet published.** |
| Fedora RPM | `synveil-<version>-1.x86_64.rpm`, x86_64 | **Implemented; Build validated** trong run [37970834983](https://github.com/nghianguyen150612/Synveil/actions/runs/37970834983). Guest Fedora 42 thất bại trước assertion sản phẩm ở bước boot/readiness. **Native qualified: không.** | Fedora 42 x86_64 chỉ là ứng viên. Các bản RPM khác **Unsupported** trừ khi được nêu tên và chứng nhận riêng. |
| Linux AppImage | `Synveil-<version>-x86_64.AppImage`, x86_64 | **Implemented; Build validated** trong run [37970834983](https://github.com/nghianguyen150612/Synveil/actions/runs/37970834983). Guest Ubuntu 24.04 và Fedora 42 đều thất bại trước assertion sản phẩm. **Native qualified: không.** | Chưa duyệt nền tảng tương thích Linux phổ thông nào. **Blocked; Not yet published.** |
| Linux quick install | `deploy/install/quick-install.sh` → `scripts/linux_quick_install.py` | **Implemented** với phát hiện chính xác Ubuntu 24.04/Fedora 42 x86_64 và tải stable channel đã xác thực. Chưa có endpoint production, bootstrap khóa hoặc generation channel. | **Blocked; Not yet published.** Không sao chép lệnh cài từ CI hoặc tự điền dữ liệu tin cậy. |

`Implemented` mô tả hành vi mã nguồn. `Build validated` mô tả build CI được dẫn liên kết ở source head P045. `Native tested` có nghĩa một bước native giới hạn đã thực sự chạy; không có nghĩa toàn bộ hành trình sản phẩm đã vượt qua. `Native qualified` yêu cầu các assertion chấp nhận máy sạch đã nêu phải thành công. `Blocked` ghi nhận gate phát hành chưa đạt. `Not yet published` có nghĩa chưa có artifact phát hành công khai chính thức. `Unsupported` không phải cam kết tương thích.

## Ranh giới hỗ trợ chính xác

- Windows: Setup x86_64 là mục tiêu v0.2. Chưa phiên bản Windows nào được chứng nhận; Windows ARM64 nằm ngoài ma trận.
- Ứng viên package Linux: Ubuntu 24.04 x86_64 cho DEB và Fedora 42 x86_64 cho RPM. Cả hai vẫn cần chấp nhận native P045.
- Debian chưa được chứng nhận. Có package DEB hoạt động hoặc package manager thuộc họ Debian không chứng minh Debian được hỗ trợ.
- Các bản dẫn xuất Debian và bản phân phối RPM khác không được chứng nhận chỉ dựa trên định dạng tệp.
- AppImage nhắm tới Linux x86_64, nhưng baseline tương thích và khởi chạy native chưa được chứng nhận. Không thể suy ra rằng mọi kernel Linux, glibc, desktop hoặc cấu hình FUSE đều được hỗ trợ.
- Linux ARM64/aarch64, kiến trúc 32-bit, macOS, iOS và Android nằm ngoài ma trận cài đặt v0.2.

## Danh sách xác minh danh tính và xuất bản release

- [ ] Ghi rõ phiên bản hệ điều hành, kiến trúc, yêu cầu GUI/runtime chính xác đã vượt qua chấp nhận native.
- [ ] Hoàn tất chấp nhận P045 cho Windows, Ubuntu 24.04, Fedora 42 và mọi môi trường Linux phổ thông được quảng bá; giữ lại danh tính source/artifact chính xác.
- [ ] Hoàn tất chấp nhận Host được quản lý đầu-cuối P036. Run self-host trên head P045 hiện tại [37970834595](https://github.com/nghianguyen150612/Synveil/actions/runs/37970834595) thất bại ở gate danh tính artifact production; các job nền tảng chưa phải một lần chứng nhận Host đạt.
- [ ] Cấp và phân phối độc lập gốc tin cậy công khai production; thiết lập quản lý khóa riêng và hoạt động ký. Bộ xác minh đã có, nhưng chưa cấp khóa production.
- [ ] Công bố stable channel và byte manifest phát hành đã xác thực, origin tin cậy, artifact chính xác cùng dữ liệu manifest/checksum đã ký. Hiện chưa công bố channel hoặc artifact v0.2.
- [ ] Xác minh tên do producer tạo, phiên bản, kiến trúc, hash, chữ ký, liên kết phát hành và liên kết cài đặt/phục hồi Anh-Việt khớp nhau trên đúng source phát hành.
- [ ] Chỉ chạy xác thực release candidate cuối P047 sau khi các điều kiện P045/P036 và release cần thiết đã đạt. P046 không tạo tag v0.2.0.

Run ma trận cross-platform P045 [37970834983](https://github.com/nghianguyen150612/Synveil/actions/runs/37970834983) đã build artifact Linux nhưng thất bại ở gate bằng chứng máy sạch: guest Ubuntu 24.04 DEB, Fedora 42 RPM/AppImage không đạt trạng thái sẵn sàng và gate tổng hợp bằng chứng native thất bại. Đây không phải lần chứng nhận native thành công. Xem toàn bộ checks và bằng chứng được lưu trong [PR P045 #78](https://github.com/nghianguyen150612/Synveil/pull/78).

## Hướng dẫn công khai hiện tại

Dùng các tài liệu v0.1 được liên kết trong [mục lục tài liệu](../../README.md) cho sản phẩm hiện đã phát hành. Hướng dẫn v0.2 trong thư mục này là tài liệu xem trước; chúng không thay thế hướng dẫn v0.1 hoặc cho phép cài artifact CI unsigned.
