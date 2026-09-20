# Đóng gói release production, cài đặt, nâng cấp và uninstall (Prompt 108)

Tài liệu này là contract hướng release cho desktop runtime production. Nó
bao phủ artifact DEB/RPM trên Linux, ZIP portable trên Windows, installer
package-neutral và quy tắc nâng cấp/gỡ bỏ bảo toàn dữ liệu. Nó không thêm
auto-updater, telemetry, crash reporting, analytics, Windows service, feature
sync/auth/conflict mới, hay database migration.

## Artifact release và một nguồn version

Version ứng dụng có thẩm quyền là `[workspace.package] version` trong
`Cargo.toml`. `deploy/packages/common/version.sh` derive version DEB/RPM từ
giá trị đó; native shell nhận `env!("CARGO_PKG_VERSION")`. Vì vậy tên package
và version hiển thị native không thể bị đổi bằng một hằng số packaging thứ hai.
`Version=1.0` trong `deploy/applications/synveil.desktop` là version của đặc
tả desktop-entry, không phải version Synveil thứ hai.

Artifact production:

| Platform | Builder | Output | Runtime policy |
|---|---|---|---|
| Linux x86_64 | `deploy/packages/build.sh --format=deb` | `synveil_<version>_amd64.deb` | khai báo Qt 6/systemd/DBus của distro; không đóng server/database daemon |
| Linux x86_64 | `deploy/packages/build.sh --format=rpm` | `synveil-<version>-1.x86_64.rpm` | khai báo Qt 6/systemd/DBus của distro; không đóng server/database daemon |
| Windows x86_64 | `deploy/packages/build-windows.sh` | `synveil-<version>-windows-x86_64.zip` | closure Qt/QML/plugin/C++ self-contained và `qt.conf`; ZIP unsigned portable, không phải installer |

Payload Linux được assemble qua `deploy/install/MANIFEST` duy nhất. Nó gồm ba
executable sibling root-owned `/usr/bin/synveil-scheduled-maintenance-once`,
`/usr/bin/synveil-client`, `/usr/bin/synveil-desktop`, systemd unit system/user,
desktop entry, SVG icon, notices và example không chứa secret. Nó chỉ tạo
skeleton `/etc/synveil` và `/etc/synveil/credentials`; không seed credential
hay database.

ZIP Windows gồm `synveil-desktop.exe`, `synveil-client.exe`, `qt.conf`,
`platforms/qwindows.dll`, closure DLL/QML/plugin Qt, C++ runtime target nếu
cần, `LICENSE`, `NOTICE` và `SYNVEIL-MANIFEST.txt` deterministic. Manifest ghi
version, platform, path tương đối, size và SHA-256 rồi được validate trước khi
archive. ZIP không chứa service, đăng ký scheduled task, elevation, password
hay machine-wide autostart. Client manager đang chạy mới có thể explicit tạo
Task Scheduler current-user theo desktop contract Windows.

## Lệnh build

Build chỉ ghi vào thư mục `target/` đã ignore và không stage, commit hay publish
artifact release.

```bash
./deploy/packages/build.sh --format=all --output-dir=target/packages

./deploy/packages/build.sh --format=deb \
  --binary=target/release/synveil-scheduled-maintenance-once \
  --client-binary=target/release/synveil-client \
  --desktop-binary=target/release/synveil-desktop \
  --output-dir=target/packages

# Native Windows: windeployqt resolve closure Qt thật.
./deploy/packages/build-windows.sh --output-dir=target/windows-packages

# Cross-build Linux: binary Windows thật và Qt prefix Windows thật.
./deploy/packages/build-windows.sh \
  --desktop-binary=path/to/synveil-desktop.exe \
  --client-binary=path/to/synveil-client.exe \
  --qt-prefix=path/to/windows/Qt \
  --output-dir=target/windows-packages
```

`build.sh` cần `cargo`, Qt development/runtime cho desktop crate, `rpmbuild`
cho RPM và `ldd` cho audit dependency Linux. `dpkg-deb` được ưu tiên; fallback
ar/tar rootless tạo đúng container DEB thật khi máy thiếu `dpkg-deb`.
`SOURCE_DATE_EPOCH` có thể là số nguyên không âm. Nếu bỏ trống, builder dùng
timestamp của source revision hiện tại, normalize mtime file/thư mục, sort
archive và normalize ownership; cùng source/binary phải cho cùng byte package.

`ldd` có dependency `not found` là hard failure. DEB khai báo trực tiếp
`libdbus-1-3` và `libsystemd0`, cùng glibc, libgcc/libstdc++ và runtime Qt 6
Core/Gui/Widgets/QML/Quick/QuickControls2/Network. RPM khai báo họ package
systemd, DBus và Qt tương ứng. PostgreSQL server/CLI, Docker, Nginx, Redis và
credential không nằm trong package.

Windows native cần `windeployqt` cùng `llvm-readobj` hoặc `dumpbin` và `zip`.
Cross packaging cần Qt prefix Windows thật và PE binaries. Mọi PE import
non-system phải có trong ZIP; Linux shared library, SDK, header, import
library, static archive, developer path và repository path đều bị reject.

## Install và system integration

Hook DEB/RPM và `deploy/install/install.sh` không tự start. Chúng có thể chạy
`systemd-sysusers`, `systemd-tmpfiles` và reload manager trên host thật, nhưng
không tự enable maintenance timer, không chạy cycle, không enable user unit,
không đăng ký Windows task. Admin phải provision credential:

```text
/etc/synveil/credentials/database-url  root:root  0600
```

Package không chứa file này. Scheduled-maintenance nhận nó qua systemd
`LoadCredential`; env config thông thường chỉ có tuning không bí mật. Chỉ
enable system timer sau khi review credential và deployment policy. User client
chỉ enable explicit bằng `systemctl --user enable --now synveil-client.service`;
mở GUI không tự undo disable.

Desktop/client là hai executable sibling. Nếu client chưa cài, không reachable
hoặc không start được, GUI chỉ hiện label recovery/availability generic và giữ
nguyên profile state; không hiện stack trace, session token, credential hay
path filesystem raw. Profile thiếu hoặc first-run rỗng là setup bình thường,
không phải migration do installer tạo.

## User data, upgrade và uninstall

Profile desktop thuộc user và tách khỏi package files:

| Platform | Configuration | Persistent state/profile | Cache/runtime |
|---|---|---|---|
| Linux | `$XDG_CONFIG_HOME/synveil` hoặc `~/.config/synveil` | `$XDG_DATA_HOME/synveil` hoặc `~/.local/share/synveil` | XDG cache và runtime trong user runtime/state boundary |
| Windows | `%APPDATA%\Synveil` | `%LOCALAPPDATA%\Synveil` | `%LOCALAPPDATA%\Synveil\Cache` và `Runtime` |

Secret Service/Credential Manager của OS vẫn là ranh giới secret. Credential,
profile preference, local sync SQLite state và external storage root không phải
package-owned.

Upgrade chỉ thay PACKAGE artifacts và giữ config, profile/auth state, SecretStore,
local sync database và external data. Installer kiểm tra containment và atomic
replace PACKAGE file. Nó không chạy migration như side effect. Schema hiện tại
vẫn là 36 server migration, 7 client-sync migration và
`LOCAL_SCHEMA_VERSION = 7`.

Uninstall package-neutral mặc định chỉ xóa PACKAGE files đã biết. Nó giữ
`/etc/synveil`, credential, `/var/lib/synveil`, user profile, external pool và
PostgreSQL; không follow package-path symlink đệ quy và không xóa shared
parent. Native DEB purge/RPM erase cũng bảo toàn dữ liệu. Cleanup application
data destructive duy nhất là operation explicit:

```bash
./deploy/install/uninstall.sh --root=/tmp/synveil-root --purge
```

`--purge` chỉ xóa allowlist `/etc/synveil` và `/var/lib/synveil` sau containment
check; vẫn không xóa external storage, mounted volume, home data hay PostgreSQL.
Không tự động xóa account.

## Validation contract

Gate repository bắt buộc:

```bash
cargo fmt --all -- --check
cargo check --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
git diff --check
cargo test -p synveil-metadata --test production_packaging_units --locked -- --nocapture
cargo test -p synveil-metadata --test linux_native_packaging_units --locked -- --nocapture
cargo test -p synveil-metadata --test linux_install_lifecycle --locked -- --nocapture
```

PACKAGE-UNIT-1..10 lần lượt kiểm metadata, executable layout, desktop
entry/icon, version consistency, install path safety, upgrade state,
uninstall data, runtime dependency closure, Windows manifest và ownership/mode
Linux. Artifact inspection chạy khi DEB/RPM/ZIP local tồn tại; nếu chưa có,
source contract vẫn chạy và test báo artifact check skip.

Live Linux package gate gồm `systemd-analyze verify`, inspection metadata/file
list DEB/RPM, hai lần build byte-identical và rehearsal staged
install/upgrade/uninstall/purge disposable. Native Windows gate gồm
`windeployqt`, PE import audit, manifest ZIP, Task Scheduler current-user và
portable startup trên host Windows. Cross-build Linux không phải bằng chứng
native Windows execution.

Scope release này chưa gồm signing, publish repository, full guided Windows
installer, auto-update, rollback package set partial và packaging macOS/iOS/
Android. PostgreSQL integration vẫn là environment gate khi
`SYNVEIL_TEST_DATABASE_URL` unset; không được báo limitation này như product
hay package failure.
