# Hướng dẫn vận hành Synveil v0.1

Đây là hướng dẫn user/operator cho implementation v0.1 trong repository này.
Mục được đánh dấu hoãn hoặc không hỗ trợ không phải cam kết sản phẩm v0.1.

## Ma trận hỗ trợ

| Khu vực | Trạng thái v0.1 | Điều kiện |
|---|---|---|
| Server/API | Hỗ trợ ở dạng API Rust và binary dùng PostgreSQL | Operator triển khai với PostgreSQL và object storage riêng; desktop package không kèm server/database. |
| Host server | Linux | Cần PostgreSQL, object root bền vững, HTTPS termination và supervisor do operator quản lý. |
| Desktop/client | Linux x86_64 | Có package/runtime native và user systemd supervision. |
| Desktop/client | Windows x86_64 | Có ZIP portable và code path Windows; cross-build/compile không phải native runtime acceptance. |
| macOS | Hoãn; không hỗ trợ trong v0.1 | Không cài hoặc vận hành v0.1 trên macOS. |
| iOS/Android | Hoãn; không hỗ trợ trong v0.1 | Không phát hành mobile client. |
| Synveil OS | Hoãn; không hỗ trợ trong v0.1 | Không phát hành image Synveil OS. |

Native startup, tray, Task Scheduler và filesystem Windows cần Windows runner;
cross-build Linux không phải runtime validation trên Windows.

## Thành phần được phát hành

Linux package có `synveil-client`, `synveil-desktop`, scheduled-maintenance
one-shot, package metadata và user systemd unit. Windows artifact là ZIP
unsigned portable gồm hai executable và Qt runtime closure; không phải
installer, Windows service hay database bundle.

Workspace cũng có `synveil-api`, `synveil-worker` và
`synveil-scheduled-maintenance-once`. Hai binary đầu là deployment component
build từ workspace, không nằm trong desktop package. API có resource dưới
`/api/v1`, probe anonymous `/health/live`, `/health/ready`, và alias `/live`,
`/ready`.

## Cài đặt

### Linux desktop/client

Điều kiện: Linux x86_64, Qt 6 và desktop session được hỗ trợ, user systemd
nếu muốn login supervision, Secret Service/keyring cho credential device và
server HTTPS có thể truy cập.

Build hoặc nhận artifact rồi cài DEB/RPM. Tên package derive từ workspace
version (`0.1.0` trong checkout này):

```sh
./deploy/packages/build.sh --format=all --output-dir=target/packages
sudo dpkg -i target/packages/synveil_0.1.0_amd64.deb
# hoặc hệ RPM:
sudo rpm -Uvh target/packages/synveil-0.1.0-1.x86_64.rpm
```

Package không tự enable/start client:

```sh
systemctl --user daemon-reload
systemctl --user enable --now synveil-client.service
systemctl --user status synveil-client.service
```

Installer staged không đụng host cần đủ ba binary:

```sh
./deploy/install/install.sh --root=/tmp/synveil-root \
  --binary=target/release/synveil-scheduled-maintenance-once \
  --client-binary=target/release/synveil-client \
  --desktop-binary=target/release/synveil-desktop
```

Đây là packaging/deployment tooling, không phải flow cài user thông thường.
Không chạy trên `/` trừ khi administrator dùng host-root override explicit như
`deploy/install/README.md`.

### Windows desktop/client

Nhận `synveil-<version>-windows-x86_64.zip`, giải nén vào thư mục user và chạy
`synveil-desktop.exe`. ZIP không cần elevation, không đăng ký machine-wide
service. Client manager có thể tạo current-user Task Scheduler khi user bật
explicit. Không dùng systemd command trên Windows.

### Điều kiện server

Server cần Linux, PostgreSQL làm metadata/transaction authority, object root
qua `SYNVEIL_OBJECT_ROOT`, HTTPS termination và supervisor/backup destination
do operator quản lý. Repository không cung cấp guided server installer hoặc
PostgreSQL daemon bundled; giữ PostgreSQL/object storage ngoài desktop
package lifecycle.

## Lần chạy đầu

Flow được hỗ trợ:

1. Cài/chạy `synveil-desktop`; `synveil-client` chạy độc lập hoặc được manager
   yêu cầu start.
2. Nhập HTTPS origin và label; client kiểm tra anonymous bounded tới
   `/health/ready` trước khi lưu profile.
3. Authenticate qua server; device credential nằm trong SecretStore OS, GUI
   không sở hữu auth secret bền vững.
4. Tạo library bằng logical name và chọn local folder.
5. Chờ observation/sync bounded ban đầu. Folder ordinary không rỗng được admit
   thành local content ban đầu; entry trở thành create/upload/reconcile work,
   không bị bỏ silently.

Folder phải absolute, writable, không redirect. Không chọn filesystem root,
chính home directory, current working directory, symlink/junction/reparse
redirect hoặc root overlap. Không có attach/import cho remote library đã có.

`.synveil` là control data reserved. Control directory thiếu có thể tạo;
control directory có sẵn chỉ được nhận khi marker `root-id` regular bounded
chứng minh đúng managed root. Marker symlink/directory, incomplete,
incompatible hoặc staging/quarantine chưa hoàn tất sẽ fail closed; không xóa
để ép setup.

## Vận hành server

Server sở hữu PostgreSQL connection, migration, authentication, API state và
object root. API mặc định bind `127.0.0.1:3000`; đặt `SYNVEIL_BIND_ADDR` trong
service configuration. Dùng `SYNVEIL_PUBLIC_ORIGIN` cho HTTPS origin canonical
khi cần allowed-origin policy. Khi có PostgreSQL, API cần
`SYNVEIL_REBASELINE_TOKEN_KEY` và chạy forward migration trước khi serve.

```sh
cargo run --release --locked -p synveil-api --bin synveil-api
cargo run --release --locked -p synveil-api --bin synveil-worker
```

Đây là lệnh operator/developer, không phải claim desktop package cài hoặc
supervise server. Production dùng service manager và secret injection. API
source hiện đọc `DATABASE_URL`; không đặt password thật trong file commit hoặc
tài liệu release. Worker không listen và cần `SYNVEIL_OBJECT_ROOT` khi enable.

Probe `/health/live` cho liveness, `/health/ready` cho readiness; operational
view bảo vệ ở `/api/v1/system/health`. Giữ stdout/stderr qua journal/log policy
của service manager; component Rust tôn trọng `RUST_LOG`. Stop qua supervisor,
cho bounded worker cycle drain rồi start lại matched release.

## Vận hành desktop

`synveil-client` sở hữu synchronization, SQLite state, root probe, SecretStore,
recovery và local IPC. `synveil-desktop` là Qt UI/tray/control surface. Đóng
GUI chỉ đóng controller connection; không dừng client đang chạy.

Linux dùng `/usr/lib/systemd/user/synveil-client.service`, không phải root
system service. Windows dùng current-user Task Scheduler, không phải Windows
Service. Mở lại GUI sẽ reconnect client hiện có, không tạo sync engine thứ hai.

### Pause, Resume và Sync Now

- **Pause** persist user pause, chặn wakeup mới; work đang chạy không bị kill.
- SQLite durable state, pending intent, root binding, conflict record và pause
  setting vẫn còn; Pause không xóa pending work.
- **Resume** chỉ xóa lý do user-pause và cho phép schedule bình thường; không
  reset state hay destructive rescan.
- **Sync Now** khi paused trả paused result và không bypass Pause. Resume trước,
  rồi mới Sync Now nếu cần bounded wake.

### An toàn khi root không khả dụng

**Local root mất, unmount hoặc không khả dụng không được hiểu là xóa mọi file.**
Library giữ trạng thái fenced/deferred `RootUnavailable`. Chờ đúng
folder/mount, rồi check/retry hoặc restart client. Không tạo folder rỗng thay
thế, xóa `.synveil`, hoặc xóa SQLite.

### Conflict và attention

Desktop hiển thị attention bền vững cho conflict được hỗ trợ:

- **Accept Remote** chấp nhận quyết định server khi conflict expose action đó.
- **Retry Local** retry local intent với current base khi được expose; không phải
  unconditional replay.

Không có automatic merge/resolution promise. Chỉ dùng action được hiển thị và
chờ authoritative refresh.

### Kết quả không rõ

Response mất sau khi operation có thể đã commit sẽ thành `OutcomeUnknown` và
refresh durable state. Synveil có thể phải reconcile thành công hay chưa trước
khi retry an toàn. Không bấm lặp destructive action, xóa local state hoặc tạo
library thứ hai.

## Recovery

| Tình huống | Hành vi tự động/chờ | User action |
|---|---|---|
| Client unavailable | Supervisor/reconnect bounded | Start client hoặc xem service status. |
| Authentication required | Runtime chờ, không bỏ library | Sign in/enroll lại; không xóa SecretStore. |
| Server unavailable | Retry/backoff bounded | Khôi phục network/TLS/server rồi Check again. |
| Local root unavailable | Root fenced; không mass-delete | Gắn đúng mount/folder rồi check. |
| Pending setup | Pending identity durable được reconcile khi restart | Resume với cùng server/profile/folder; không tạo library trùng. |
| Request ambiguous | Receipt/fence được refresh; không blind replay | Chờ reconcile, chỉ retry khi UI cho action an toàn. |
| Conflict attention | Conflict giữ durable | Chỉ dùng action hiển thị. |

## Backup và restore

Backup riêng từng domain:

1. PostgreSQL: dùng logical/physical backup nhất quán với PostgreSQL; không copy
   live data directory như logical backup generic.
2. Server config: `/etc/synveil` và service-manager environment/secret
   reference; tránh plaintext secret trong archive thường.
3. Server-owned state: `/var/lib/synveil` nếu deployment dùng, object/content
   root và external backup destination; chúng khác PostgreSQL metadata.
4. Client state: profile/config và SQLite. Linux mặc định là
   `$XDG_CONFIG_HOME/synveil`, `$XDG_DATA_HOME/synveil` (fallback
   `~/.config/synveil`, `~/.local/share/synveil`); Windows là
   `%APPDATA%\Synveil`, `%LOCALAPPDATA%\Synveil`.
5. Credentials: device credential trong Linux Secret Service hoặc Windows
   Credential Manager. Copy directory không capture SecretStore.
6. Synced content: external library root là user data, backup theo storage của nó.

Restore thận trọng: restore PostgreSQL durable state và object/content root,
restore config và credential reference, restore SecretStore qua OS, start
server/verify readiness, rồi reconnect client. Nếu không restore SecretStore
thì authenticate/enroll lại device. Không claim automated disaster recovery.

## Upgrade

Trước upgrade, stop client/API/worker phù hợp và verify backup config,
PostgreSQL, object root, client state. Package chỉ thay PACKAGE artifact và giữ
durable state; migration forward-only, không reset database.

Schema v0.1 đóng băng ở **36 PostgreSQL migration**, **7 client SQLite
migration**, `LOCAL_SCHEMA_VERSION = 7`. Unknown future schema fail closed,
không đảm bảo automatic downgrade. Không delete/recreate database để binary cũ
chạy; dùng binary tương thích hoặc restore được administrator xác minh.

Sau upgrade verify profile, library binding, root availability, pause,
unresolved conflict, health/readiness và client reconnect. Partial package có
thể sửa bằng reinstall đúng version; đó không phải whole-package rollback.

## Uninstall và purge

Ordinary uninstall chỉ gỡ PACKAGE artifact đã biết, giữ `/etc/synveil`,
credential, `/var/lib/synveil`, user profile, PostgreSQL, external object/backup
root và synced content. Explicit purge mới xóa allowlist Synveil-owned
`/etc/synveil` và `/var/lib/synveil` sau containment check; vẫn không xóa
PostgreSQL, external storage, mounted volume, home data hay library root.

```sh
./deploy/install/uninstall.sh --root=/tmp/synveil-root --purge
```

Không đặt external content/mounted pool trong purge-owned path. Script từ chối
symlink/parent escape và không tự xóa account `synveil`.

## Security, networking và diagnostics

Device credential do OS SecretStore sở hữu; GUI chỉ giữ profile metadata không
secret và reference. Local IPC user-scoped, không có public control TCP
listener. Onboarding yêu cầu HTTPS certificate validation, không redirect
fallback và `/health/ready`. API mặc định loopback HTTP; TLS termination thuộc
reverse proxy operator. Synveil không claim end-to-end encryption hay bảo vệ
host/operator bị compromise.

Lệnh chẩn đoán không destructive:

```sh
systemctl --user status synveil-client.service
journalctl --user -u synveil-client.service -n 100 --no-pager
systemctl --user restart synveil-client.service
curl --fail --silent https://server.example/health/live
curl --fail --silent https://server.example/health/ready
```

| Triệu chứng | Nhóm | Hành động an toàn | Không làm |
|---|---|---|---|
| Desktop không tới client | IPC/process | Start client, xem status, check lại. | Không expose IPC qua TCP hoặc xóa profile. |
| Client không chạy | Supervisor/install/config | Kiểm package path và status; dùng sibling đúng package. | Không chạy client thứ hai cùng profile/root. |
| Server unreachable | Network/TLS/service | Check health, certificate và server log. | Không hạ xuống HTTP plaintext/tắt verify. |
| Authentication required | SecretStore/session | Sign in/enroll qua UI. | Không chép token vào log/config. |
| Root missing | Mount/path | Gắn đúng mount rồi Check again. | Không thay folder rỗng/xóa `.synveil`. |
| Sync paused | User control | Resume rồi Sync Now nếu cần. | Không kỳ vọng Sync Now bypass Pause. |
| Conflict attention | Conflict action | Chỉ dùng Accept Remote/Retry Local được hiển thị. | Không sửa SQLite/blind replay. |
| Package/Qt dependency lỗi | Runtime/package | Kiểm dependency, Qt, user manager, keyring. | Không trộn binary release hoặc chép Qt ngẫu nhiên. |
| Upgrade/schema reject | Durable state | Giữ state, dùng release tương thích hoặc restore backup verify. | Không drop/wipe/downgrade database mù quáng. |

## Giới hạn v0.1

- Native Windows runtime acceptance là environment gate; cross-build/MinGW
  link không phải native Windows acceptance.
- macOS, iOS, Android và Synveil OS không được hỗ trợ trong v0.1.
- Desktop package không phải server/database installer; PostgreSQL, TLS,
  backup và server lifecycle do operator phụ trách.
- Không đảm bảo automatic downgrade hoặc transactional rollback package set.
- Filename semantics khác giữa platform; unsupported/reserved name, case
  collision, redirect và marker lỗi có thể fail an toàn thay vì sync.
- Copy config/SQLite không bao gồm credential trong SecretStore.
- Native Windows và disposable-PostgreSQL là gate validation riêng, có thể bị
  block bởi môi trường; không tính là lỗi tài liệu.

## Tài liệu nguồn

- Package/install: `docs/vi/RELEASE_PACKAGING.md`, `deploy/install/README.md`.
- Upgrade: `docs/vi/UPGRADE_SAFETY.md`.
- Desktop: `docs/vi/DESKTOP_LAUNCH.md`, `docs/vi/DESKTOP_CONTROL.md`.
- API/health: `docs/vi/API_ARCHITECTURE.md`.
- Security: `docs/vi/SECURITY.md`.
- Release summary: `docs/vi/RELEASE_NOTES_v0.1.md`.
