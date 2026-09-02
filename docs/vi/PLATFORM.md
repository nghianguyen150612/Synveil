# Kiến trúc sản phẩm và nền tảng đa nền tảng

Trạng thái: **Blueprint quy chuẩn PLANNED**

Tài liệu này định nghĩa ranh giới nền tảng, phân phối, onboarding, vòng đời và
mức độ phức tạp người dùng của Synveil. Tài liệu bổ sung cho
[PRODUCT.md](PRODUCT.md), [ARCHITECTURE.md](ARCHITECTURE.md),
[DEPLOYMENT.md](DEPLOYMENT.md), [STORAGE.md](STORAGE.md) và
[SECURITY.md](SECURITY.md). Tài liệu không khẳng định installer, client native,
managed service hay hạ tầng remote-access đã tồn tại.

## North star của sản phẩm

Synveil hướng tới trở thành **private cloud mà bất kỳ ai cũng có thể
self-host**:

> Synveil phải làm cho việc sở hữu cloud riêng có cảm giác như dùng một ứng
> dụng tiêu dùng bình thường, nhưng không lấy đi quyền sở hữu hoặc quyền kiểm
> soát nâng cao của người dùng.

Sản phẩm phục vụ người dùng có kỹ thuật, power user, gia đình, người dùng tại
nhà, sinh viên, người sáng tạo, developer, nhóm nhỏ và người không chuyên. Người
không chuyên là nhóm mục tiêu hạng nhất, không phải niche để dành về sau.

Hành trình mặc định mong muốn là:

```text
tải Synveil
    ↓
cài đặt
    ↓
chọn nơi lưu dữ liệu
    ↓
tạo tài khoản
    ↓
kết nối điện thoại hoặc laptop
    ↓
private cloud sẵn sàng
```

Trải nghiệm chính không được yêu cầu người dùng bình thường hiểu container,
PostgreSQL, Btrfs, reverse proxy, TLS certificate, environment variable, port
forwarding hay filesystem mount option. Các cơ chế này có thể tồn tại bên dưới
sản phẩm và vẫn phải mở cho operator nâng cao.

## Nguyên tắc sản phẩm

Các nguyên tắc sau áp dụng cho thiết kế sản phẩm, kiến trúc, roadmap, release
package và tài liệu hỗ trợ:

1. **Dễ dùng mặc định.** Luồng phổ biến an toàn dùng installer, lựa chọn có
   hướng dẫn, kiểm tra tự động, default hợp lý và recovery dễ hiểu.
2. **An toàn mặc định.** Tự động hóa không được làm yếu durability,
   authorization, retention, backup, update hay bảo vệ thao tác destructive.
3. **Thiết kế đa nền tảng.** Windows, macOS, Linux Desktop và Linux Server là
   các target có chủ ý. Khác biệt nền tảng nằm sau capability và service
   abstraction.
4. **Khi cần vẫn mạnh.** Compose, PostgreSQL tùy biến, S3/MinIO, NAS, reverse
   proxy, TLS tùy chỉnh, CLI, API, diagnostics và tối ưu filesystem vẫn có
   trong Advanced / Server Mode.
5. **Tính đúng đắn trước sự thông minh.** Nguyên tắc hiện có này vẫn tuyệt đối
   bắt buộc. Dễ dùng đến từ automation và UX, không từ shortcut thiếu an toàn.
6. **Self-hosted trước tiên.** Core data access vẫn hoạt động khi không có
   control plane hoặc relay bắt buộc do Synveil vận hành.
7. **Progressive disclosure.** Luồng thường dùng khái niệm của người dùng;
   chi tiết infrastructure nằm ở `Settings → Advanced` hoặc admin diagnostics.

### Quy tắc terminal

> **Một tính năng cốt lõi hướng tới người dùng không được buộc người dùng bình
> thường mở terminal để sử dụng.**

Ngoại lệ có thể gồm administration nâng cao, development, môi trường chưa được
hỗ trợ, recovery thủ công và integration chuyên biệt. Ngoại lệ không được định
nghĩa trải nghiệm chính của Personal / Home Mode.

## Hai trải nghiệm deployment, một sản phẩm

Personal / Home Mode và Advanced / Server Mode dùng chung API protocol, domain
model, PostgreSQL authority, object identity, storage correctness, sync journal,
backup semantics và security model. Đây là profile đóng gói/vận hành, không phải
hai sản phẩm hay hai data model không tương thích.

| Profile | Người dùng chính | Điểm bắt đầu thường dùng | Infrastructure mặc định lộ ra | Lối mở rộng nâng cao |
|---|---|---|---|---|
| **Personal / Home Mode** | Người không chuyên, gia đình, desktop user, personal server nhỏ | Native installer hoặc package có hướng dẫn | Chọn storage, account, pairing thiết bị, health dễ hiểu, thông báo update an toàn | `Settings → Advanced`, diagnostics export, migration/recovery có tài liệu |
| **Advanced / Server Mode** | Homelab, NAS operator, sysadmin, VPS user, developer, deployment lớn | Docker Compose, native server package hoặc deployment thủ công | Compose, PostgreSQL, reverse proxy, TLS, S3/MinIO, CLI/config | Đầy đủ adapter, networking, database và operations control |

### Personal / Home Mode

Luồng mục tiêu là:

```text
native installer
    ↓
storage candidate cùng kiểm tra capacity/health
    ↓
user xác nhận vị trí dữ liệu
    ↓
Synveil-managed service và PostgreSQL riêng
    ↓
bootstrap account một lần
    ↓
local connectivity và onboarding thiết bị
    ↓
health check dễ hiểu
    ↓
sẵn sàng
```

Người dùng thường không phải cấu hình PostgreSQL, Caddy, Docker, Compose, TLS
certificate, database role, environment variable, mount option hay reverse
proxy route. Installer/service manager quản lý các chi tiết đó khi platform và
distribution cho phép. Người dùng vẫn chọn/xác nhận nơi lưu dữ liệu bền vững
và nhận thông tin rõ ràng về thứ uninstall sẽ hoặc không sẽ xóa.

Personal / Home Mode ban đầu có thể giới hạn ở một host, một owner cá nhân/gia
đình, một local object backend và profile mạng local được hỗ trợ. Giới hạn được
chấp nhận khi công khai và an toàn; không phải lý do để quay về hướng dẫn
terminal-first.

### Advanced / Server Mode

Advanced / Server Mode tiếp tục hỗ trợ:

- Docker Compose cho homelab, NAS, VPS, developer và operator;
- PostgreSQL bên ngoài và backup database do operator quản lý;
- reverse proxy và TLS tùy chỉnh;
- filesystem local, NAS mount, S3-compatible và MinIO;
- networking thủ công, port forwarding, domain, VPN/tailnet-style access và
  công cụ connectivity do admin chạy;
- CLI/configuration, log/metric chi tiết, diagnostics, API và maintenance
  control.

Compose vẫn là production topology quan trọng. Nó không phải trải nghiệm
production duy nhất về mặt khái niệm và không được là con đường duy nhất trong
product positioning hoặc kiến trúc dài hạn.

## Chính sách nền tảng hạng nhất

### Host và server platform

| Platform | Ý định sản phẩm | Personal / Home packaging về sau | Advanced / Server packaging |
|---|---|---|---|
| Windows | Host hạng nhất cho desktop user thường và operator nâng cao | Installer `SynveilSetup.exe` có ký, MSI hoặc tương đương; Windows Service hoặc tách per-user/service phù hợp | Compose trên runtime được hỗ trợ, native service package khi thực tế cho phép |
| macOS | Host hạng nhất cho desktop và personal server trong giới hạn permission Apple | `.dmg` hoặc `.pkg` có ký/notarize, service do launchd quản lý, giải thích permission rõ | Compose hoặc package native khi thực tế cho phép; proxy/TLS tùy chỉnh vẫn do operator quản lý |
| Linux Desktop | Desktop host hạng nhất, document khác biệt distro | Native package và/hoặc AppImage; Flatpak chỉ cho phần phù hợp sandbox/permission | Native package/service hoặc Compose |
| Linux Server | Server host hạng nhất | Guided native package khi thực tế cho phép; không giả định consumer installer | Compose, native package/service, NAS/VPS/dedicated server |

Platform chỉ được xem hạng nhất khi Rust core compile. Gate hỗ trợ còn gồm
install, chọn storage, service lifecycle, health, upgrade,
uninstall-giữ-dữ-liệu, backup/recovery và ma trận filesystem/capability công
bố.

### Client hạng nhất trong tương lai

Android, iPhone và iPad là các client hạng nhất trong tương lai. Chúng dùng
protocol có version và device/credential model chung, đồng thời tôn trọng giới
hạn background execution, notification, permission, local storage và photo
library của platform. Android phải được đưa vào kế hoạch client dài hạn;
Apple không phải chiến lược mobile duy nhất.

### Trade-off đóng gói

- Windows installer phải tính đến elevation cho service, data per-machine hay
  per-user, binary có ký, Defender/SmartScreen, coordination khi upgrade và
  prompt uninstall. Windows Service chỉ là adapter implementation, không phải
  dependency của domain.
- macOS package phải tính đến signing, notarization, launchd, Full Disk Access
  hoặc permission cần cho backup folder, Keychain, sleep/wake và uninstall.
  Synveil không phụ thuộc private Apple API.
- Linux native package tích hợp tốt với systemd, path distro, package upgrade
  và policy admin nhưng làm tăng chi phí duy trì distribution. AppImage tăng
  portability nhưng tích hợp service/privileged storage yếu hơn. Flatpak tăng
  isolation desktop nhưng không tự động phù hợp với server privileged,
  database, storage root hay system service. Có thể xem UI Flatpak riêng với
  managed server.
- Linux Server nên có native package/service dùng systemd khi thực tế cho phép
  nhưng không loại Compose. Package chỉ được support sau khi test migration,
  backup, service, permission và rollback.

Đây là target roadmap. Tài liệu không triển khai installer.

## Ranh giới abstraction cho platform và service

Domain/application core không được gọi trực tiếp Windows Service API, launchd,
systemd, Docker, shell command hay platform credential API. Implementation sau
này dùng port/adapter khái niệm:

```text
PlatformRuntime
    ├── ServiceLifecycle
    ├── ProcessSupervisor
    ├── SecretStore
    ├── StorageDiscovery
    ├── NetworkDiscovery
    ├── UpdateInstaller
    └── DiagnosticsProvider
```

Platform adapter dịch các capability này sang Windows Service, launchd, systemd,
supervisor của user session hoặc workflow Compose/operator. Domain chỉ thấy
state/event ổn định, không thấy exit code riêng của platform.

Service set managed có thể gồm:

```text
Synveil API
Synveil worker/background job
PostgreSQL
storage subsystem và health check
update coordinator
log và rotation
```

Lifecycle contract phải bao phủ install, configure, start, stop, graceful
drain, restart, crash recovery, startup ordering, health, upgrade, giới hạn
rollback và uninstall. AI/integration worker tùy chọn bị crash không được
restart-loop hoặc làm storage core mất khả dụng.

## PostgreSQL không cần admin thủ công

PostgreSQL vẫn là authority canonical cho metadata và trạng thái giao dịch theo
ADR-002. Personal / Home Mode không thay bằng SQLite chỉ để package dễ hơn và
không yêu cầu user thường tự cài role, tạo database, đặt `DATABASE_URL` hay
chạy SQL.

Managed database deployment tương lai có thể provision PostgreSQL private hoặc
system service được hỗ trợ, khởi tạo data directory được bảo vệ, tạo role
least-privilege, apply migration, start/stop/monitor service, tạo backup phối
hợp, verify upgrade và recover sau crash. Database vẫn nằm trong cùng security
và backup model với PostgreSQL external ở Advanced / Server Mode.

Managed database adapter phải tách:

- provision và dò package/runtime;
- database initialization và role bootstrap;
- migration execution;
- service lifecycle và health;
- backup/restore và version compatibility;
- ownership data directory và uninstall policy;
- sinh, lưu, rotate và recovery credential.

UI cho user báo “System database” và health state. Diagnostics admin có thể cho
biết version PostgreSQL và migration state nhưng không lộ stack trace hoặc bắt
user sửa database thủ công.

## Chọn storage và capability model

Trải nghiệm storage lần đầu dùng ngôn ngữ người dùng:

```text
Chọn nơi Synveil lưu cloud của bạn

D:\\Synveil
3.4 TB còn trống

[Dùng vị trí này]
```

hoặc:

```text
External SSD
1.8 TB còn trống
Được tối ưu cho Synveil
```

Discovery layer có thể kiểm tra filesystem type, capacity, writable,
removable, path safety, capability và health an toàn. UI thường không buộc user
hiểu NTFS, APFS, Btrfs, mount option hay object-store. Chi tiết đặt ở
`Settings → Advanced`.

Abstraction storage dựa trên capability:

```text
StorageBackend
    ↓ khai báo
StorageCapabilities
    ↓ được dùng bởi
storage correctness và policy tối ưu của Synveil
```

Capability có thể gồm `reflink`, `block_clone`, `copy_on_write_clone`,
`native_snapshot`, `compression`, `checksumming`, `sparse_files`,
`atomic_rename`, `durable_fsync`, `range_reads` và `filesystem_health`. Đây là
bằng chứng do adapter cung cấp, không suy ra từ tên OS. Thiếu acceleration thì
fallback về đường đi portable đúng đắn hoặc state unsupported rõ ràng.

File identity, version history, sync state, backup state, object reference,
change journal, conflict, retention, integrity metadata, dedup metadata và
restore semantics do Synveil sở hữu, độc lập với filesystem. Btrfs snapshot
không phải Synveil backup; APFS snapshot không phải Synveil version history;
filesystem replication không phải Synveil sync.

### Ý định hỗ trợ filesystem

Correctness model hướng tới NTFS, ReFS, APFS, Btrfs, ext4, XFS, ZFS được hỗ trợ
về sau, filesystem local generic, NAS-mounted filesystem và object store. Btrfs
có thể là accelerator Linux nâng cao được khuyến nghị. Nó không bắt buộc.
WinBtrfs là tùy chọn/community-oriented và không bao giờ là yêu cầu của
Windows.

## Hazard về portability

Contract platform/client phải test và ghi rõ:

- path separator, case sensitivity/preservation, reserved name, Unicode
  normalization, path/component length, trailing dot/space;
- symlink, junction, hard link, permission, ACL, timestamp, file lock, atomic
  rename, sparse file, watcher, extended attribute, hidden/system file;
- removable drive, network mount, sleep/hibernate, process/service startup,
  user-session lifecycle, reboot và write bị gián đoạn;
- health signal filesystem và hành vi khi device disconnect hoặc storage
  identity thay đổi.

Synveil giữ metadata có nghĩa thay vì âm thầm normalize mất thông tin.
Canonical internal name/comparison khác platform-native presentation. Tên local
không biểu diễn được trở thành state `NAME_CONFLICT`/`UNREPRESENTABLE` rõ ràng,
giữ nguyên identity trên server.

## Onboarding không chuyên và device pairing

Luồng pairing mong muốn:

```text
Synveil Server
    ↓ Add device
hiện QR code hoặc pairing code ngắn
    ↓
client scan/nhập code
    ↓
xác nhận server identity và user approval
    ↓
cấp credential và đăng ký device
    ↓
chọn policy sync và/hoặc backup
    ↓
inventory ban đầu và xác nhận health
```

Pairing code ngắn hạn, dùng một lần hoặc chống replay, rate-limit, bind vào
account/instance dự định và invalid sau approve, expiry, cancel hoặc vượt giới
hạn lỗi. Thiết kế phải giải quyết pairing local/remote, MITM qua xác nhận server
identity, device mất, cấp credential, revoke và audit. QR chỉ là tiện ích, không
thay TLS hoặc user approval.

## Remote access cho user thường

Remote access là bài toán kiến trúc nhiều lớp, không phải lời hứa một thủ thuật
mạng hoạt động mọi nơi. Các nhóm dự kiến:

1. LAN discovery và kết nối trực tiếp local;
2. direct public connection khi operator đã có DNS/TLS/firewall an toàn;
3. NAT traversal khi hai endpoint và mạng cho phép;
4. relay/coordination service tùy chọn;
5. tích hợp VPN hoặc tailnet-style do user sở hữu tùy chọn;
6. networking thủ công trong Advanced / Server Mode.

Personal / Home Mode nên nói “remote access sẵn sàng”, “chỉ hoạt động trong
mạng nhà” hoặc “cần quản trị mạng” bằng ngôn ngữ người dùng. Không che lỗi bằng
jargon và không khẳng định đã giải quyết CGNAT, DNS, firewall hay certificate
khi chưa giải quyết.

Synveil vẫn hữu ích khi không có service hosted bắt buộc. Nếu sau này có
coordination/relay, docs và UI phải nói rõ metadata nhìn thấy, file content có
đi qua hay không, service có optional/self-host được không, failure behavior,
privacy, cost, abuse, availability và retention.

Hướng khuyến nghị là self-hosted-first: LAN/manual/VPN luôn có, direct/NAT
traversal khi an toàn, relay optional và disclosure riêng chỉ khi thực sự tăng
adoption. Cơ chế cụ thể vẫn là open.

## Maintenance, health và diagnostics

Personal / Home Mode nên tự động hóa và lên lịch, có status rõ ràng, cho:

- database vacuum/maintenance và readiness migration;
- garbage collection, integrity scan, backup verification, retention cleanup;
- thumbnail/cache cleanup, certificate renewal, service recovery và log
  rotation;
- storage health, capacity reserve, job backlog, update readiness và device
  connectivity.

Maintenance phải an toàn, quan sát được, khôi phục được và có rate limit. Công
việc quan trọng không âm thầm xóa dữ liệu còn khôi phục được. Destructive work
vẫn phải theo `inspect → plan → validate → execute → verify`.

UI thường tách:

```text
user diagnostics: Storage Healthy; Database Healthy; Backups Healthy;
  Remote access Connected; Devices 4 connected
admin diagnostics: dependency, capacity, migration, job, recovery chi tiết
developer diagnostics: correlation ID, structured log, trace, technical error
  đã redaction
```

Thông báo đầy disk cho user là “Storage Synveil gần đầy. Còn 182 GB. [Quản lý
storage]”; admin view vẫn có thể giữ `ENOSPC`, object ID và correlation ID.
Stable machine-readable API error vẫn tồn tại phía sau presentation layer.

## Automatic update

Personal / Home và native server về sau có thể có channel stable/beta, nhưng
update phải yêu cầu:

- release có ký và verify cryptographic trước khi cài;
- service coordination, health preflight, capacity/config check và gate
  compatibility migration database;
- backup đã verify trước migration nguy hiểm hoặc format change;
- notification, consent, maintenance window, progress và failure state rõ;
- giới hạn rollback, failed-update recovery và giữ artifact/config cũ khi có
  thể rollback.

Update không được âm thầm xóa PostgreSQL, khởi tạo object root mới hoặc claim
rollback sau migration không đảo ngược. Compose và Advanced / Server có thể
tiếp tục dùng upgrade pinned do admin kiểm soát. Không có unsigned/opaque
auto-updater trong kiến trúc.

## UX uninstall, migration và recovery

Binary ứng dụng, configuration, database state, stored user object, cache, log,
credential và backup copy có lifecycle riêng. Lựa chọn uninstall thường dùng:

```text
Gỡ ứng dụng Synveil
Giữ dữ liệu và configuration để cài lại
Export hoặc chuẩn bị migration
Xóa vĩnh viễn dữ liệu Synveil  [xác nhận destructive]
```

Gỡ application không đồng nghĩa xóa cloud. Xóa vĩnh viễn cần confirmation rõ
phạm vi, confirmation lần hai cho dữ liệu không đảo ngược và audit record khi
service còn hoạt động.

Di chuyển từ máy cũ sang máy mới về sau:

```text
prepare migration
    ↓ inspect source và chọn destination
    ↓ validate database, object store, secret, version, capacity
    ↓ copy/transfer và verify
    ↓ activate destination
    ↓ reconnect hoặc re-register device khi cần
```

Giữ nguyên quy tắc `inspect → plan → validate → execute → verify`. Migration
phải xử lý PostgreSQL state, object reference, application master key, device
identity, hostname/TLS, remote access, coexistence tạm thời, rollback window và
hành vi instance cũ. Thiếu encryption key là recovery blocker phải báo rõ; key
mới không được giả vờ là continuity.

Recovery UX phải bao phủ file xóa nhầm, previous version, laptop bị mất, drive
hỏng, object corrupt, update hỏng, database recovery và chuyển server. Action
đầu tiên phải dễ hiểu, không destructive; hướng dẫn terminal/admin là escalation.

## Progressive disclosure và thuật ngữ

Navigation/copy thường dùng:

```text
Files · Photos · Backups · Devices · Shared · Search · Activity · Settings
```

`Settings → Advanced` có thể hiện object store, chunk manifest, sync cursor,
garbage collection, PostgreSQL, compression profile, S3 endpoint, reflink,
storage backend, database maintenance và network control chi tiết.

Engineering identifier vẫn giữ nguyên giữa hai ngôn ngữ: `StorageBackend`,
`StorageCapabilities`, `SyncCursor`, `ChangeEvent`, `FileVersion` không được
đổi tên không nhất quán. UI có thể dùng “Storage”, “Recent changes”, “Sync
status”, “Version history”, “System database” mà không đổi identifier contract.

## Nguồn mở và hạ tầng tùy chọn

Native installer, phân phối desktop/mobile, shared Rust client library, SDK
boundary, package Microsoft/Apple và App Store có thể ảnh hưởng thảo luận
license trong ADR-011. Blueprint không đổi license. Relay, discovery,
notification, update hoặc domain-assistance service tùy chọn phải tách rõ khỏi
core self-hosted và không biến thành SaaS dependency âm thầm.

## Tiêu chí thành công cấp sản phẩm

Khi capability tương ứng đã implementation và promote, user bình thường phải
làm được các việc sau mà không cần kiến thức infrastructure:

- cài Synveil và chọn storage;
- tạo account và kết nối thiết bị khác;
- upload/sync file, bật backup và bảo vệ ảnh mobile;
- restore file xóa hoặc previous version;
- xem photos, share file và kiểm tra system health;
- nhận update đã verify;
- chuyển Synveil sang máy khác;
- recover sau device mất hoặc server storage hỏng.

Đây là outcome, không phải claim blueprint hiện tại. Không gắn con số
performance/adoption tự tạo.

## Non-goal

Revision này không thêm custom operating system, custom filesystem, custom
database, custom cryptography, mandatory Synveil-hosted SaaS, Kubernetes-first
deployment hay full-device OS backup. Không phải mọi môi trường mạng đều tự
động reachable, và kế hoạch native packaging không phải installer đã
implementation.

## Open decisions

### OPEN DECISION OD-PLAT-001: distribution PostgreSQL managed

- **Owner:** Database, Release, Security, Product
- **Needed by:** implementation Personal / Home Mode và native installer đầu tiên
- **Options:** PostgreSQL bundled/private; PostgreSQL do system quản lý;
  Synveil-managed service package riêng; chỉ external PostgreSQL do admin cài
- **Recommendation:** có đường private/system service do Synveil quản lý cho
  Personal / Home platform được hỗ trợ, đồng thời giữ external PostgreSQL ở
  Advanced / Server. Không hiện database-choice wizard cho user thường.
- **Decision evidence:** packaging Windows/macOS/Linux, security boundary,
  upgrade compatibility, database backup/restore, data-directory lifecycle,
  uninstall, resource footprint và support cost.

### OPEN DECISION OD-PLAT-002: ranh giới native service supervisor

- **Owner:** Platform / Distribution, Release, Security
- **Needed by:** native installer architecture gate trước service code theo OS
- **Options:** một supervisor privileged; per-user service cộng helper privileged
  hẹp; OS-native service từng platform; chỉ process trong user session
- **Recommendation:** giữ contract `ServiceLifecycle` trung lập platform và
  dùng OS-native model ít quyền nhất có thể bảo vệ data root/recover core.
- **Decision evidence:** install elevation, IPC authentication, crash recovery,
  sleep/reboot, multi-user host, update/uninstall và threat test.

### OPEN DECISION OD-PLAT-003: cơ chế remote-access coordination

- **Owner:** Networking / Connectivity, Security, Product
- **Needed by:** remote-access beta gate
- **Options:** LAN/direct only; NAT traversal không relay; relay do Synveil vận
  hành nhưng optional; relay self-host được; ưu tiên VPN/tailnet integration
- **Recommendation:** làm direct/LAN/manual/VPN trước và chỉ xem relay optional,
  disclosed, self-hostable sau khi có privacy contract metadata/content rõ.
- **Decision evidence:** ma trận NAT/CGNAT, threat model, failure behavior,
  privacy review, cost/abuse control và usability test.

### OPEN DECISION OD-PLAT-004: mức tự động update

- **Owner:** Release, Security, Product, Operations
- **Needed by:** Personal / Home stable release đầu tiên
- **Options:** chỉ notification; download opt-in; install opt-in trong
  maintenance window; chỉ automatic security update
- **Recommendation:** bắt đầu bằng notification có ký và opt-in rõ, backup đã
  verify và migration preflight; chỉ mở rộng sau evidence rollback/recovery và
  policy operator/air-gapped.
- **Decision evidence:** update failure injection, backup/restore, migration
  compatibility, user comprehension, channel policy và support burden.

### OPEN DECISION OD-PLAT-005: migration portability package

- **Owner:** Backup / Recovery, Database, Storage, Clients, Release
- **Needed by:** machine-to-machine migration gate được support
- **Options:** guided local transfer; portable encrypted migration archive;
  workflow backup/restore phối hợp; procedure chỉ dành cho admin
- **Recommendation:** cung cấp workflow có hướng dẫn dựa trên primitive
  backup/restore đã verify; chỉ thêm archive portable encrypted khi key,
  identity, format và resume semantics được đặc tả đầy đủ.
- **Decision evidence:** clean destination drill, key recovery, transfer dữ liệu
  lớn bị gián đoạn, device re-register, hostname/remote-access change và
  rollback test.
