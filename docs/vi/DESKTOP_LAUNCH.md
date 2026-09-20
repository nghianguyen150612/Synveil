# Khởi chạy desktop, autostart theo user và đóng gói runtime (Prompt 99)

Tài liệu này mô tả boundary khởi chạy production được thêm sau shell Qt trong
Prompt 98. Đây là tài liệu vận hành đi kèm
[`ADR-041`](../adr/ADR-041-production-desktop-launch-orchestration.md) và
[`DESKTOP_CONTROL.md`](DESKTOP_CONTROL.md).

## Topology runtime

Synveil Desktop gồm hai process với lifecycle cố ý khác nhau:

```text
synveil-desktop
  Qt 6/QML, tray, presentation bounded, request launch
        -> BackgroundClientManager + DesktopController
           -> Prompt 96 local control IPC
              -> synveil-client
                 DesktopSyncHost + SyncRuntime + writer lock
```

GUI có thể yêu cầu manager làm cho client available. GUI không trở thành owner
của sync runtime, không mở SQLite client, không probe library root, không đọc
credential, không tạo IPC frame và không tự implement retry/correctness sync.
Client vẫn chạy độc lập và được supervisor độc lập quản lý.

## Hành vi startup

Sau khi Qt load profile hợp lệ, bridge tạo một controller và một
`BackgroundClientManager` theo profile. Controller bắt đầu handshake/reconnect
Prompt 97 bình thường. Manager thực hiện một availability inspection bounded và
chỉ được yêu cầu start khi client absent/stopped hoặc user supervisor inactive.
Controller tiếp tục flow reconnect của chính nó và publish coherent snapshot
fresh khi client available.

GUI thread không chờ process, systemd, Task Scheduler, socket hay pipe. Request
launch được coalesce bằng Tokio gate dùng chung. Request đồng thời trả
`AlreadyStarting`; attempt thành công hoặc thất bại còn có cooldown bounded.
Điều này chặn reconnect controller, nhiều cửa sổ hoặc thao tác nhanh của user
tạo spawn storm.

Vocabulary kết quả an toàn là:

| Result | Ý nghĩa | Tự động launch thay thế? |
|---|---|---:|
| `AlreadyRunning` | Endpoint client hiện có khỏe | Không |
| `StartRequested` | Supervisor nhận request start | Không request thêm |
| `StartedSupervised` | Supervisor đã start client | Không request thêm |
| `StartedDirect` | Fallback detached từ sibling packaged đã start | Không request thêm |
| `AlreadyStarting` | Đang có attempt/cooldown bounded khác | Không |
| `NotInstalled` | Thiếu executable/unit/task packaged | Không |
| `SupervisorUnavailable` | Không dùng được supervisor | Tối đa một direct fallback |
| `LaunchDenied` | Policy quyền hoặc launch từ chối | Không |
| `UnsafeState` | Endpoint/writer state không an toàn | Không |
| `Failed` | Attempt bounded fail hoặc timeout | Có cooldown |

Security, protocol-incompatible, malformed-control, active-writer và terminal
controller state được fail-closed. Chúng không tạo launch thay thế. QML chỉ
nhận label generic; stderr, command output, path, PID, credential, URL và token
không bao giờ là UI state.

## Độc lập giữa GUI và client

`DesktopController::stop()` chỉ đóng và join IPC work do controller sở hữu. Tray
Quit và window close không gửi Prompt 96 `Shutdown`, không gọi supervised stop
của manager và không terminate `synveil-client`. Vì vậy:

- đóng GUI vẫn để client đang chạy tiếp tục;
- mở lại GUI reuse endpoint hiện có, không launch thêm;
- client được supervisor recover khi GUI đã đóng; và
- khi client crash, controller thành stale/reconnecting rồi generation mới trở
  thành fresh sau recovery của supervisor.

Writer lock cùng profile của Prompt 95 vẫn là authority cuối cùng khi hai GUI
race. Launch orchestration không thay thế hoặc bypass lock này.

## Linux user service và autostart

Package cài unit client tại
`/usr/lib/systemd/user/synveil-client.service`. Unit không được cài tại
`/usr/lib/systemd/system/synveil-client.service` và không phải Synveil root/
system service.

Unit dùng entrypoint/config semantics hiện có:

```ini
[Service]
Type=simple
ExecStart=/usr/bin/synveil-client
Restart=on-failure
RestartSec=30s
RestartPreventExitStatus=78
```

Unit có thêm `StartLimitIntervalSec=5min` và `StartLimitBurst=5`. Exit 78 là
configuration failure permanent do source hiện tại định nghĩa; exit 70 là
bootstrap/runtime failure generic. Không thêm dependency network-online vì
Prompt 92 sở hữu network availability và recovery.

Autostart khi login là thao tác explicit của user:

```sh
systemctl --user daemon-reload
systemctl --user enable synveil-client.service
systemctl --user start synveil-client.service
systemctl --user status synveil-client.service
systemctl --user disable synveil-client.service
systemctl --user stop synveil-client.service
```

Installer package-neutral, DEB/RPM hooks và GUI không tự enable/start unit.
Disable explicit được tôn trọng. Test dùng unit link tạm và tên disposable,
không enable Synveil thật vĩnh viễn trong session developer.

Nếu user manager không available, native backend có thể dùng direct fallback
detached tới packaged sibling canonical. Fallback vẫn độc lập GUI nhưng không
có crash recovery của supervisor; production package ưu tiên user service khi
có thể.

## Windows user task

Windows dùng Task Scheduler theo user. Không cài Windows Service, không yêu cầu
administrator, không dùng `SYSTEM` và không lưu password. Task có:

- current user và semantics logon `InteractiveToken`;
- run level `LeastPrivilege`;
- tên ổn định theo profile dưới `\Synveil\BackgroundClient\`;
- action tới đúng sibling canonical `synveil-client.exe`;
- policy nhiều instance `IgnoreNew`; và
- restart finite `PT30S`, count 5.

Register, query, run và remove gọi `System32\schtasks.exe` tuyệt đối với argv
tường minh. Không tạo shell command hoặc chuỗi PowerShell/cmd interpolate.
Task name không có token, credential, root path hoặc server URL.
`WindowsTaskDefinition` có test deterministic trên Linux; native registration
và task recovery phải được chạy trên Windows runner.

## Resolve executable và security

Manager canonicalize desktop executable packaged hiện tại và chỉ chấp nhận đúng
tên product cùng sibling client. Không nhận path từ QML, IPC, server, library
manifest hay lookup `PATH` mutable. Direct fallback dùng null stdio và cờ detach
theo platform. Supervisor request dùng unit/task identity cố định và config
semantics process hiện có, không bịa command-line flag.

Autostart metadata chỉ là process-management state. Nó không ghi SQLite,
checkpoint, handoff record, sync intent, root state hay credential. Unit, task
identity, package metadata và QML projection không chứa credential, session
cookie, authorization header, raw root path, file content hoặc secret-store
value.

## Nội dung Linux package

`deploy/install/MANIFEST` là nguồn sự thật duy nhất để stage DEB và RPM. Payload
Prompt 99 gồm:

| Payload | Đường dẫn cài đặt |
|---|---|
| Client | `/usr/bin/synveil-client` |
| Desktop shell | `/usr/bin/synveil-desktop` |
| User service | `/usr/lib/systemd/user/synveil-client.service` |
| Application entry | `/usr/share/applications/synveil.desktop` |
| Application icon | `/usr/share/icons/hicolor/scalable/apps/synveil.svg` |
| Maintenance runtime hiện có | `/usr/bin/synveil-scheduled-maintenance-once` |
| Maintenance units/fragments/template hiện có | Các path Prompt 75/79 hiện có |
| License và notice | `/usr/share/doc/synveil/LICENSE` và `NOTICE` |

Desktop entry có `Name=Synveil`, `Exec=/usr/bin/synveil-desktop`,
`Type=Application`, categories, `Icon=synveil` và `Terminal=false`. Đây là
application metadata bình thường, không có semantics XDG/login autostart.

Hai archive native được build từ manifest stage chung. Artifact check xác nhận
exact payload tree, mode executable, byte parity với source, parity unit/
metadata, không có secret file và không có path repository, `/tmp`, build hay
development. Signing và publish repository để release engineering xử lý.

## Windows portable package

`deploy/packages/build-windows.sh` tạo ZIP unsigned reproducible dưới thư mục
package bị ignore. ZIP có hai EXE production, `qt.conf`, Qt runtime DLL,
`platforms/qwindows.dll`, QML module/plugin cần thiết, C++ runtime DLL nếu Qt
prefix target thật cung cấp, `LICENSE` và `NOTICE`.

Trên Windows, `windeployqt` là công cụ closure authority với compiler runtime và
phân tích embedded QML. Cross package trên Linux yêu cầu Qt prefix Windows thật
và chỉ copy closure runtime/QML đã nêu; script reject header, import library,
static archive, Linux library và developer path. PE import audit bằng
`llvm-readobj`/`dumpbin` yêu cầu mọi non-system dependency có trong ZIP. ZIP
portable độc lập repository, Cargo target, `/tmp` và Qt cài trên developer.
Đây chưa phải installer.

## Validation và giới hạn

Focused gate trong repository kiểm tra launch manager, coalescing và bounded
1,000 request, canonical path, Linux unit policy, source/lifecycle user-unit
Linux, package manifest, nội dung artifact DEB/RPM, task definition Windows và
policy Windows ZIP. Các command liên quan là:

```text
cargo test -p synveil-client --lib --locked -- --nocapture
cargo test -p synveil-metadata --test linux_desktop_launch_units --locked -- --nocapture
cargo test -p synveil-metadata --test windows_desktop_packaging_units --locked -- --nocapture
```

Linux user-manager test link một unit disposable, start/inspect/stop rồi
cleanup.

Task Scheduler registration native Windows, startup portable package Windows và
native tray Windows là runtime gate cần Windows runner. Kết quả Linux/cross
build không được mô tả là native Windows execution. PostgreSQL không phải
dependency của launch/package boundary; PostgreSQL integration bị ignore khi
`SYNVEIL_TEST_DATABASE_URL` unset vẫn là unverified.

Prompt 99 không đổi server migration, client schema, HTTP route, OpenAPI
operation hoặc web feature. Phase kế tiếp là checkpoint và ownership release
gate của Prompt 100, không phải một launch implementation thứ hai.

## Authentication độc lập với launch và quit (Prompt 101)

Prompt 101 không đổi ownership process launch. `synveil-client` background vẫn
là owner của HTTP enrollment, access `LocalStateStore`/`SecretStore` theo
profile, runtime wake và credential reload. Qt shell chỉ expose enrollment
field masked, transient và route request qua `DesktopController` cùng IPC local
Prompt 96 hiện có.

GUI close, tray Quit, background-client restart và process termination không phải
Sign Out. Chúng không xóa credential durable và không gửi credential-change
wake. Sign Out explicit là controller command tới background host, ghi
forgotten marker hiện có, hoàn tất cleanup secure-store rồi mới wake library bị
ảnh hưởng. Cleanup fail trả typed result an toàn và suppress wake.

Sau background-client restart, client reload profile metadata durable và secure
credential. Sau GUI restart, controller chỉ dựng lại auth status an toàn và
affordance Sign Out; không gửi enrollment token hay bearer value vào QML. Nếu
mất IPC response, controller trả `OutcomeUnknown` và không replay enrollment
hay Sign Out khi reconnect. Bound 69 byte, gate một auth operation, result
generic và profile isolation áp dụng giống nhau cho client launch qua
supervisor hoặc direct.

Prompt 101 không thêm supervisor registration, package payload, server route,
OpenAPI operation, schema migration, installer behavior hay credential store
theo platform. Live enrollment, restart recovery và native Windows execution
vẫn là validation gate tường minh. Xem
[`ADR-042`](../adr/ADR-042-secure-desktop-authentication-and-credential-lifecycle.md).

## Onboarding profile do client sở hữu (Prompt 102)

First launch chỉ tạo process profile identity không bí mật và khởi chạy client/
control process hiện có với zero library. Native shell nhận server origin và
display label; client thực hiện parse canonical, anonymous readiness verification
và tạo profile durable. Khi thành công, controller chuyển từ
configuration-required sang state unauthenticated của Prompt 101; auth vẫn là
enrollment operation riêng.

Edit connection probe target mới trước durable apply. Đổi origin giữ opaque
profile ID nhưng fence enrollment cũ và SecretStore trước khi wake runtime.
GUI close, tray Quit và GUI restart vẫn độc lập với client lifecycle. Restart
đọc manifest canonical, SQLite profile state và secure store, không cần QML
cache. Migration `0007_profile_reconfiguration.sql` chỉ đổi trigger semantics
phía client; không cần server migration. Xem
[`ADR-043`](../adr/ADR-043-desktop-profile-onboarding-and-connection-configuration.md).

## Setting desktop thiết yếu (Prompt 105)

Panel settings dùng launch manager hiện có để đọc/đổi login startup theo user.
Linux dùng unit `systemd --user` cố định; Windows dùng Task Scheduler của user
hiện tại. Bridge nhận category authoritative `Enabled`, `Disabled` hoặc
`Unavailable`, gọi typed manager API, không tạo service thứ hai, không truyền
shell text và không đưa path/profile, URL, credential hay token vào metadata.

Disable chỉ áp dụng login sau này, không stop client đang chạy. Enable không
spawn duplicate. Toggle nhanh được coalesced thành latest intent sau một bounded
operation; lỗi hoặc outcome không chắc chắn sẽ refresh status authoritative.

Close-to-tray là setting riêng cục bộ desktop lưu bằng `QSettings`. Chỉ khi
setting bật và Qt có system tray thật thì close window mới hide; nếu không dùng
controller-only shell exit hiện có. Cả hai đường không gửi client `Shutdown`,
không terminate `synveil-client`, không đổi global pause và không đụng
credential. Setting không lưu trong SQLite, client manifest, `SecretStore`,
server hay IPC snapshot. Xem
[`ADR-046`](../adr/ADR-046-essential-desktop-settings-and-user-sync-controls.md).

## Launch recovery tương tác (Prompt 107)

Khi controller báo background client hiện có không khả dụng, recovery card
desktop có thể gọi `BackgroundClientManager::ensure_running`. Đây vẫn là
manager bounded dùng cho startup settings: resolve packaged sibling canonical,
không PATH lookup hoặc shell interpolation, một attempt in-flight và
coalescing/cooldown. Action recovery không bật login startup, không truyền
profile path/credential làm argument và không spawn client thứ hai. Sau request,
controller reconnect và fetch status authoritative. Xem
[`ADR-048`](../adr/ADR-048-production-desktop-recovery-and-resilience-ux.md).
