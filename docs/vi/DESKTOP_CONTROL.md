# IPC điều khiển desktop cục bộ an toàn (Prompt 96)

Tài liệu này mô tả protocol điều khiển process cục bộ do `synveil-client`
đang chạy sở hữu. Đây không phải Synveil server HTTP API và không xuất hiện
trong OpenAPI.

## Boundary và ownership

```text
UI / tray / diagnostics tương lai
              |
       DesktopControlClient
              |
      protocol frame local v1
              |
      control server synveil-client
              |
      DesktopSyncHostHandle
              |
      một SyncRuntime -> sync cycle hiện có
```

`DesktopClientProcess` sở hữu đúng một control server, một
`DesktopSyncHost` và một `SyncRuntime`. Code control không mở SQLite, load
secret, tạo runtime/host khác, gọi Prompt 91 engine hoặc giữ state sync thứ
hai. Bind control secure là một phần bootstrap production; lỗi bind là lỗi
bootstrap fatal có type. Server dừng trước khi graceful host shutdown Prompt 95
hiện có tiếp tục.

## Transport và identity endpoint

Linux dùng Unix-domain socket:

```text
<runtime directory canonical của platform>/synveil/<profile ID opaque>.sock
```

Platform path resolver hiện có cung cấp runtime directory. Resolver dùng
`XDG_RUNTIME_DIR` nếu có và fallback state do application sở hữu nếu không có;
process code không tự parse các biến môi trường này. Profile ID là identity
UUIDv7 opaque hiện có. Không token, credential, URL, username hoặc filesystem
root nào tham gia tên.

Windows dùng nhánh named pipe thật với tên deterministic theo profile. Syntax
Windows chỉ nằm trong nhánh transport Windows; không có fallback TCP hoặc
localhost. Trong phase này macOS và generic adapter trả lỗi
unsupported-platform tường minh thay vì hạ thấp security model.

`Debug` và diagnostic thường chỉ hiện endpoint kind. Caller cần kết nối mới
được lấy endpoint sinh ra qua API client; status process/library thông thường
không chứa path endpoint.

## Authorization cục bộ

Linux kiểm tra runtime directory là absolute, thuộc user hiện tại, không phải
symlink và không có quyền ghi cho group/other. Control directory `synveil` có
mode owner-only `0700`; socket sau bind được kiểm tra là socket owner-only
`0600`. Peer chỉ được nhận khi Unix peer credentials báo cùng UID với process.

Nếu path chính xác đã tồn tại, chỉ socket đúng owner mới được kiểm tra stale.
Connect thành công nghĩa là endpoint active và trả
`CONTROL_ENDPOINT_ALREADY_ACTIVE`. Socket refused/not-found được kiểm tra lại,
chỉ remove nếu vẫn là đúng socket non-symlink thuộc user hiện tại. Symlink,
regular file, directory, device, owner sai hoặc state không chắc chắn đều bị
từ chối và không bị thay thế. Drop guard của server cũng chỉ xóa socket đã
verify do chính server sở hữu.

Windows dùng Tokio named pipe từ chối remote client, tối đa 32 instance và
security descriptor protected có allow ACE cho owner rights. Không có ACE cho
Everyone hoặc Anonymous. Nếu không tạo được descriptor, bootstrap control
thất bại thay vì nới quyền.

## Protocol và framing

Protocol ban đầu là version `1`. Mỗi connection gửi `ClientHello` có
`protocol_version`, rồi nhận `ServerHello` có version và capabilities. Version
không hỗ trợ nhận `ProtocolVersionUnsupported` rồi connection đóng an toàn. Mỗi
request sau đó là JSON có `request_id` khác zero; response echo cùng ID.

JSON payload được frame bằng độ dài unsigned big-endian bốn byte, sau đó đúng số
byte đã khai báo. Maximum là `64 KiB`; frame zero-length, oversized, truncated,
malformed hoặc JSON không hợp lệ bị reject mà không allocate theo độ dài peer
chưa kiểm tra. Không dùng `read_to_end`.

Mỗi connection xử lý request tuần tự; server nhận tối đa 32 connection task.
Handshake có timeout ngắn và connection sau handshake có idle timeout. Writer
chậm, disconnect và peer malformed chỉ ảnh hưởng task của peer; không block
sync runtime hoặc shutdown join của server.

## Command Gen-1 và dữ liệu an toàn

| Command | Kết quả |
|---|---|
| `Ping` | Process state an toàn hiện tại. |
| `GetProcessStatus` | `Starting`, `Running`, `Stopping`, `Stopped`, `Faulted` và readiness control. |
| `ListLibraries` | Danh sách bounded gồm library ID ổn định và category tóm tắt. |
| `GetLibraryStatus` | Runtime/root/auth/conflict status tóm tắt của một library ID. |
| `SyncNow` | `Queued`, `Coalesced`, hoặc `AlreadyRunningFollowupRecorded`; chỉ là scheduling. |
| `Shutdown` | `ShutdownAccepted` sau khi lifecycle process nhận request. |
| `SubscribeEvents` | Một connection event bounded. |

Library status được translate từ host/runtime hiện có. Root chỉ có
`Available`, `Unavailable`, `Recovering`, không có raw root. Auth chỉ báo
`Blocked` hoặc kết quả authenticated thành công khi runtime hiện có chứng minh
được; nếu không là `Unknown`. Credential value/ID, cookie, raw authorization
header, server URL, filename, content, checkpoint/snapshot/handoff và conflict
payload đều không xuất hiện.

`SyncNow` gọi `DesktopSyncHostHandle::sync_now`, tức scheduling path Prompt
93/92 hiện có. Nó không gọi Prompt 91 trực tiếp và không tuyên bố cycle đã
hoàn tất. `Shutdown` không gọi `process::exit`, abort runtime hoặc kill task.
Handler ghi `ShutdownAccepted` trước khi đánh thức `DesktopClientProcess`; sau
đó process dừng listener và đi qua adapter/host shutdown Prompt 95 canonical.

## Event và reconnect

Event là invalidation best-effort từ runtime, lifecycle process và root
availability hiện có:

- `ProcessStateChanged`
- `LibraryStatusChanged { library_id }`
- `RootAvailabilityChanged { library_id, state }`
- `SyncCycleCompleted { library_id, outcome }`
- `ControlServerStopping`
- `Lagged { dropped_count }`

Broadcast channel có capacity 256. Subscriber chậm không tạo backpressure cho
producer; nó nhận lag indication và phải reconnect/refetch status. Event không
là journal durable và mất event không ảnh hưởng correctness sync. Client có thể
handshake lại sau khi tự restart hoặc process restart. Không thêm bảng IPC,
session, token, event journal hoặc migration.

## Validation và giới hạn

Target Linux tập trung là `cargo test -p synveil-client --test
desktop_control_ipc --locked`. Target dùng runtime root ngắn và disposable,
kiểm tra socket type, owner, mode, handshake, request ID, version mismatch,
unknown command, frame malformed, reconnect, event và thứ tự
shutdown acknowledgement. Unit test kiểm tra active endpoint collision, stale
recovery và từ chối symlink/regular-file/directory cùng framing bounded.

Target PostgreSQL Prompt 95 còn kiểm tra production bootstrap thật và control
client quanh process được cấu hình; phải chạy bằng hạ tầng PostgreSQL 17
disposable của repository. Validation local trên Linux không tuyên bố native
Windows execution. Tuy vậy nhánh named-pipe Windows là code thật và phải pass
native Windows cùng explicit cross-target compilation trước khi readiness marker
hợp lệ.
