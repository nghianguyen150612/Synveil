# An toàn nâng cấp và trạng thái bền vững v0.1

Schema đóng băng gồm **36 migration PostgreSQL**, **7 migration SQLite client**
và `LOCAL_SCHEMA_VERSION = 7`. Migration chỉ tiến lên; Synveil không có cơ chế
tự downgrade hay reset database để khôi phục.

Dừng application trước khi thay package. Giữ backup đã kiểm chứng của cấu hình
và trạng thái bền vững trước migration. Khởi động bản mới với trạng thái hiện
có, rồi kiểm tra danh tính profile/library, binding, checkpoint, conflict chưa
giải quyết và pause trước khi tiếp tục. Rollback binary chỉ an toàn nếu binary
đó hỗ trợ schema hiện tại; nếu không, dùng phần mềm tương thích hoặc quy trình
restore đã được quản trị viên kiểm chứng. Không xóa database để chạy bản cũ.

Startup SQLite giữ khóa single-writer, yêu cầu prefix migration liên tục và
thành công, kiểm tra table, column, index, safety trigger bắt buộc cùng checksum
SQLx. Object/column schema chưa biết bị từ chối trước migration 0004 rebuild
intent, để không mất dữ liệu cột lạ hoặc cascade qua table foreign key chưa biết.
Database có object nhưng không có ledger bị từ chối. Mỗi migration và
bản ghi version commit trong cùng transaction. Migration lỗi giữ version hoàn
tất cuối cùng; restart thử migration tiếp theo. Migration trước đã commit vẫn
được giữ. Gián đoạn cập nhật execution-time không phải lỗi schema. Test chèn
lỗi SQL, không mô phỏng mất điện vật lý hay hỏng thiết bị lưu trữ.

`LOCAL_SCHEMA_UNSUPPORTED`: version mới hơn client hỗ trợ; giữ database và dùng
phần mềm tương thích. `LOCAL_SCHEMA_INVALID`: metadata migration hoặc object
schema bắt buộc thiếu/sai; giữ bằng chứng, điều tra hoặc restore backup đã kiểm
chứng. `LOCAL_DATABASE_UNAVAILABLE` bao gồm file không đọc được/không phải
SQLite. Startup không xóa, ghi đè, tái tạo hay tự sửa trạng thái chưa biết.
Kiểm tra này xác minh ledger và sự hiện diện schema, không phải công cụ pháp
chứng toàn diện cho mọi schema độc hại hay giá trị dữ liệu hỏng.

SQLx PostgreSQL dùng advisory lock, checksum và transaction riêng cho mỗi
migration đóng băng. Version đã ghi nhưng không được hỗ trợ trả lỗi typed, đã
redact `database_schema_unsupported`. Connection migration lỗi được đóng để advisory lock không còn trong pool.
Table current bắt buộc bị thiếu dừng startup dù ledger ghi mọi migration thành
công. Lỗi migration khác dừng startup; không tự
reset hoặc fallback sang SQLite. Gate live PG17 từ database rỗng là gate môi
trường riêng. Chuỗi migration backfill bảo toàn timestamp Trash, activation
schedule, mặc định misfire và proof handoff snapshot. Thay constraint nằm trong
transaction. Không viết lại SQL lịch sử sau release.

| Trạng thái | Chủ sở hữu và hành vi nâng cấp |
| --- | --- |
| Profile và enrollment reference | SQLite giữ profile ID opaque, canonical origin, owner/device và credential ID. Migration 0007 cho lifecycle chuẩn sửa origin nhưng giữ profile ID bất biến. Replica v1 vẫn chưa bind profile một cách tường minh. |
| Byte device credential | Platform SecretStore; SQLite chỉ giữ reference và cleanup intent. Backup database không phải backup SecretStore. Verifier authentication PostgreSQL thuộc server; byte credential client không được chuyển vào đó. |
| Binding library và sync | SQLite giữ library/profile/root-binding ID, remote root, checkpoint, bootstrap, outbound intent và recovery record. Cấu hình desktop và root marker giữ root vật lý. |
| Công việc pending/ambiguous/completed | Identity intent/request/result bền vững fence recovery. Reopen không biến `SUBMITTING` thành request mới hoặc tự replay việc hoàn tất. `OutcomeUnknown` controller là phân loại response tạm thời; phải refresh trạng thái chuẩn trước retry. |
| Conflict | Giữ identity conflict chưa giải quyết và intent gốc; action resolution và stale fencing hiện có vẫn áp dụng. |
| Pause người dùng | `sync-state.conf` tách khỏi SQLite; restart đọc lại trước sync. Nội dung pause chưa biết fail closed. |
| Recovery runtime | Authentication, root availability, setup pending và lỗi server retryable được tái dựng từ record bền vững, SecretStore và probe hiện tại. Root mất vẫn là `RootUnavailable`, không tạo mass remote delete. |

Manifest package tách payload khỏi cấu hình và state. Upgrade thay artifact
package, giữ `/etc/synveil`, `/var/lib/synveil`, cấu hình/SQLite client,
credential reference/SecretStore, root ngoài và dữ liệu PostgreSQL. Thay package
bị gián đoạn có thể để lại payload thiếu; reinstall sửa payload và giữ dữ liệu.
Đây không phải bảo đảm rollback nguyên package một cách atomic.

Uninstall thường chỉ xóa artifact thuộc package. `--purge` tường minh xóa state
Synveil đã ghi rõ trong `/etc/synveil` và `/var/lib/synveil`, gồm credential quản
trị tại đó. Không xóa nội dung library, object/backup root ngoài, home không
liên quan hay PostgreSQL. Không đặt nội dung ngoài hoặc mounted data volume
trong đường dẫn thuộc purge. Symlink cuối chỉ bị unlink, không theo target;
parent thoát cây bị từ chối trước unlink. Parent symlink chuyển sang state
không thuộc purge ngay trong staged root cũng bị từ chối. Test staged không chứng minh nghiệm
thu package manager trên host thật hoặc race mount.

Không có protocol negotiation release tổng quát hay ma trận downgrade tùy ý
client/server. Adapter hiện tại kiểm tra API, control protocol, remote scope và
schema event version 1; response sai/không tương thích bị chặn trước khi thành
fact mutation local. Dùng component v0.1 đồng bộ và chạy gate PostgreSQL/platform
trong môi trường sẽ hỗ trợ. Fixture prefix chỉ chứng minh transition schema
đóng băng của repository, không chứng minh release lịch sử không được nêu tên.
