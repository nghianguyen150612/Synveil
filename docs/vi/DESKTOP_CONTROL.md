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

## Command authentication và boundary credential (Prompt 101)

Prompt 101 mở rộng tagged command set version 1 hiện có với:

```text
Authenticate { enrollment_token: bounded string }
SignOut
```

Token trong request chỉ là wire input transient. Nó bị giới hạn theo encoded
secret hiện có 69 byte, được parse lại ở domain boundary do process sở hữu và
giữ trong wrapper zeroizing/redacted. Nó không bao giờ ghi vào snapshot, event,
tray label, client-side settings store hay log. Request bắt buộc chứa token khi
đi qua authenticated local IPC connection; security property là code
presentation QML/client không persist, inspect hay sở hữu token.

Process xử lý command qua `DesktopControlHandle` và `DesktopSyncHostHandle`.
Chỉ background path này được gọi `HttpEnrollmentClient`, `LocalStateStore` hay
`SecretStore`. Response chỉ là category: `Authenticated`, `SignedOut`,
`InvalidCredentials`, `NetworkUnavailable`, `ServerUnavailable`,
`RateLimited`, `SecureStoreUnavailable`, `Busy`, `ProtocolError` hoặc
`OutcomeUnknown`. Response, status snapshot và event không chứa token, bearer
secret, cookie, authorization header, raw URL, root path hay remote diagnostic.

Không cần capability handshake mới, nên giữ tương thích version 1. Peer cũ
trả protocol error unknown-command hiện có và controller map thành feedback
protocol an toàn. Command đã admit nhưng mất response không retry sau reconnect;
controller trả `OutcomeUnknown` và refresh status authoritative. Authentication
và Sign Out dùng credential lifecycle durable-first hiện có; không thêm IPC
session, auth table, migration, HTTP route hay OpenAPI operation. Xem
[`ADR-042`](../adr/ADR-042-secure-desktop-authentication-and-credential-lifecycle.md).

## Onboarding profile và cấu hình kết nối (Prompt 102)

Protocol control local version 1 thêm các command bounded, không bí mật:

```text
GetProfileConfiguration
ValidateProfileConfiguration { base_url, display_label }
CreateOrConfigureProfile { profile_id, base_url, display_label }
UpdateProfileConfiguration { profile_id, base_url, display_label }
```

Response chỉ có safe profile ID/origin/label/timestamp hoặc outcome hữu hạn như
`Validated`, `Created`, `Updated`, `AlreadyConfigured`,
`InvalidServerAddress`, `NetworkUnavailable`, `Timeout`, `TlsFailure`,
`IncompatibleServer`, `ServerFailure`, `PersistenceFailure`, `Busy` và
`OutcomeUnknown`. Mutation thành công chỉ emit invalidation configuration
bounded; controller refetch snapshot authoritative. Không có raw parser error,
HTTP body, TLS object, credential, SecretStore record hay database row đi qua
IPC. Mất response không replay mà refresh authoritative. Xem
[`ADR-043`](../adr/ADR-043-desktop-profile-onboarding-and-connection-configuration.md).

## Setup library và bind root (Prompt 104)

Control version 1 thêm command bounded không bí mật
`SetupLibrary { name, root_path }` và capability `LibrarySetup`. Client
validate root và logical name, tạo/reconcile library authenticated trên server,
commit replica/root state theo profile hiện có, rồi mới register host/runtime.
Response chỉ là category hữu hạn như `Configured`, `AlreadyConfigured`, name/root
invalid, `AuthenticationRequired`, lỗi server/network, `PersistenceFailure`,
`OutcomeUnknown` hoặc `Busy`.

Bridge nhận kết quả native folder picker nhưng không expose absolute path trong
snapshot, diagnostic hoặc feedback. Khi snapshot authenticated có zero library,
bridge đặt `library_setup_required`; admission setup bounded và kết quả success
hoặc uncertain đều refresh authoritative. Client sở hữu pending/active manifest
recovery, nên GUI restart không mất setup dang dở và đóng GUI không stop client
hay runtime. Flow tạo remote library mới có thể admit một local tree đã có dữ
liệu: client verify marker/control tree, seed `root_node_id` authoritative vào
local state, rồi để observer bounded tạo intent bình thường. Không có bulk
uploader hay import path riêng; attach/import remote library hiện hữu vẫn chưa
được hỗ trợ. Xem
[`ADR-044`](../adr/ADR-044-desktop-library-onboarding-and-local-root-binding.md)
và [`ADR-045`](../adr/ADR-045-existing-root-bootstrap-and-initial-upload-admission.md).

## Điều khiển sync và setting desktop thiết yếu (Prompt 105)

Control version 1 thêm boundary global do process sở hữu:

```text
GetSyncControlState
PauseSync
ResumeSync
```

Handshake advertise capability bounded `SyncControl`. State chỉ là `Running`
hoặc `PausedByUser`; mutation result chỉ là `Paused`, `Resumed`,
`AlreadyPaused`, `AlreadyRunning`, `PersistenceFailure` hoặc `Busy`. `SyncNow`
khi paused trả result typed `Paused`, không resume ngầm và không bypass pause.

Client ghi `paused`/`running` vào file setting non-secret nhỏ cạnh `client.conf`
rồi mới signal runtime. Write fail không đổi runtime. Khi paused, admission
gate của runtime chặn periodic, local-change, network/inbound, credential và
manual scheduling; cycle bounded đang chạy được hoàn tất an toàn. Durable
observation local và profile/auth/control vẫn dùng được. Resume chỉ xóa lý do
user-pause và cho scheduler hiện có xử lý eligibility đã giữ, không broad rescan
hay queue thứ hai. `SyncControlStateChanged` chỉ là invalidation event, nên
controller refetch khi lag hoặc reconnect.

Bridge chỉ expose label state, feedback generic và busy flag. Mất response
pause/resume là `OutcomeUnknown`, sau đó refresh, không blind replay. Không có
path file pause, endpoint IPC, runtime object, root path, credential hay raw OS
diagnostic đi qua boundary. Extension này không thêm server route, OpenAPI,
SQLite migration hay credential/session state.

## Attention production và phân giải conflict (Prompt 106)

Control version 1 expose projection attention durable local qua capability
`Attention`:

```text
GetAttentionSnapshot
ResolveConflict {
    library_id,
    conflict_id,
    intent_id,
    detected_at_ms,
    action: AcceptRemote | RetryLocalAgainstCurrentBase
}
```

Snapshot trả exact count unresolved của canonical sync conflict và blocker
durable khác, summary theo library, cùng tối đa 32 conflict item mặc định và
hard maximum 128. Snapshot ghi rõ `truncated`. Item chỉ có stable ID, một
trong sáu conflict kind hiện có, managed relative path/previous path bounded,
kind file/directory, length đã biết, revision/state metadata và action được
canonical policy cho phép. Không có content bytes, hash, absolute root,
staging path, credential, cookie, raw HTTP body hay server diagnostic. Root
unavailable không được biến thành action empty-tree.

State store `client-sync` vẫn là authority. Host persist resolution trong
transaction conflict hiện có trước rồi mới request runtime wake bình thường.
`PausedByUser` luôn giữ nguyên: resolve conflict không resume và không bypass
sync. Library, conflict, intent và detection timestamp tạo generation fence;
request stale bị reject. Duplicate resolution trả `AlreadyResolved`, retry
không được hỗ trợ trả `UnsupportedAction`, và mỗi boundary control/presentation
chỉ admit một resolution tại một thời điểm.

`AttentionStateChanged` chỉ là invalidation event bounded. Fresh snapshot mới
authoritative sau reconnect, restart, mất event, stale result hoặc response
không chắc chắn. Mất response trả `OutcomeUnknown`; controller refresh và không
replay action. Desktop vẫn hiển thị attention durable sau GUI restart mà không
trở thành sync engine thứ hai. Extension không thêm server route, OpenAPI,
SQLite migration hay credential/session state. Xem
[`ADR-047`](../adr/ADR-047-production-desktop-attention-and-conflict-resolution.md).

## UX recovery và resilience desktop production (Prompt 107)

Recovery là projection derived từ controller snapshot coherent mới nhất, không
phải durable store thứ hai hay generic repair console. Controller chỉ phân
loại evidence đã thuộc boundary client/process/runtime:

| Category typed | Giải thích an toàn trên desktop | Owner của action |
|---|---|---|
| `ClientUnavailable` | Background client không khả dụng | `BackgroundClientManager` hiện có |
| `ProfileConfigurationRequired` | Cần cấu hình kết nối server | Profile form/controller hiện có |
| `AuthenticationRequired` | Cần đăng nhập | Flow authentication Prompt 101 |
| `RootUnavailable` | Folder local không khả dụng | Root validation/runtime wake hiện có |
| `ServerRetryable` | Đang chờ server | Wake `SyncNow` bounded hiện có |
| `LocalFailure` | Sync local cần attention | Runtime/controller recheck hiện có |
| `LibrarySetupIncomplete` | Setup library chưa hoàn tất | Reconciliation pending/active hiện có |

Reconnect, root `Recovering` và runtime backoff là waiting với automatic
recovery bounded, không phải failure action-required. `PausedByUser` vẫn là
user control; conflict Prompt 106 vẫn nằm trong attention durable. Hai surface
này có thể cùng xuất hiện với recovery mà không duplicate control.

`DesktopControllerRecoverySummary` được derive từ state connected/fresh. Khi
controller unavailable hoặc stale, UI chỉ đưa client-level item waiting hoặc
unavailable, không ghép nó với library list giả như đang fresh. Mỗi item chỉ có
library ID ổn định, enum category/action cố định, cờ action-required/waiting và
connection generation. Projection tối đa 128 item và báo truncation. Không có
root path, SecretStore detail, token, cookie, HTTP body, SQLite row hay raw OS
error đi vào QML.

Không cần command Prompt 96 mới. Invokable cố định của bridge dùng owner hiện
có:

- `Start client` gọi `BackgroundClientManager::ensure_running`, giữ nguyên
  executable sibling canonical, không PATH/shell interpolation, một admission
  in-flight và cooldown launch.
- `Check again` gọi path scheduling `SyncNow` hiện có. Nó chỉ wake runtime,
  không reset backoff, xóa state, broad rescan hay replay mutation có thể đã
  commit. Mất response là `OutcomeUnknown`, sau đó refresh authoritative và
  không replay.
- `Sign in` và cấu hình connection vẫn dùng form Prompt 101/102 hiện có.
  `SecureStoreUnavailable` hiển thị secure-storage tạm thời không khả dụng,
  không xóa hoặc ghi đè credential.
- `Resume setup` mở lại setup library hiện có. User phải chọn đúng folder đã
  cấu hình; pending identity và bootstrap reconciliation vẫn do client sở hữu.
  Không thêm root relocation.

Root biến mất không bao giờ được hiểu là tree rỗng và không tạo mass deletion.
Khi đúng root trở lại, root probe và runtime wake hiện có khôi phục status
canonical mà không reset sync state. Marker hoặc identity mismatch tiếp tục
fail-closed. Setup, attention và runtime durable state thuộc client, nên GUI
đóng hoặc controller mới sau reconnect vẫn dựng lại recovery view như cũ.

Mọi transition durable vẫn theo thứ tự durable-first: persist credential/setup/
conflict/root ở subsystem canonical, signal hoặc wake sau đó, rồi refresh. Bridge
desktop không mở SQLite, sửa manifest, xóa state, ghi đè marker, gọi server HTTP
hay chạy shell repair command. Xem
[`ADR-048`](../adr/ADR-048-production-desktop-recovery-and-resilience-ux.md).
