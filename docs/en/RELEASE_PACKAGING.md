# Production release packaging, installation, upgrade, and uninstall (Prompt 108)

This document is the release-facing contract for the production desktop
runtime. It covers the Linux DEB/RPM artifacts, the Windows portable ZIP, the
package-neutral installer, and the data-preserving upgrade/removal rules.
It does not add an auto-updater, telemetry, crash reporting, analytics, a
Windows service, a new sync/auth/conflict feature, or a database migration.

## Release artifacts and one version source

The authoritative application version is `[workspace.package] version` in
`Cargo.toml`. `deploy/packages/common/version.sh` derives the DEB and RPM
versions from that value; the desktop native shell receives
`env!("CARGO_PKG_VERSION")`. Package names and displayed native application
version therefore cannot be changed by editing a second packaging constant.
`Version=1.0` in `deploy/applications/synveil.desktop` is the desktop-entry
specification version, not a second Synveil application version.

The production outputs are:

| Platform | Builder | Output | Runtime policy |
|---|---|---|---|
| Linux x86_64 | `deploy/packages/build.sh --format=deb` | `synveil_<version>_amd64.deb` | distro Qt 6/systemd/DBus dependencies declared; no server/database daemon bundled |
| Linux x86_64 | `deploy/packages/build.sh --format=rpm` | `synveil-<version>-1.x86_64.rpm` | distro Qt 6/systemd/DBus dependencies declared; no server/database daemon bundled |
| Windows x86_64 | `deploy/packages/build-windows.sh` | `synveil-<version>-windows-x86_64.zip` | self-contained Qt/QML/plugin/C++ runtime closure and `qt.conf`; unsigned portable ZIP, not an installer |

The Linux package payload is assembled through the single authoritative
`deploy/install/MANIFEST`. It includes the root-owned siblings
`/usr/bin/synveil-scheduled-maintenance-once`, `/usr/bin/synveil-client`, and
`/usr/bin/synveil-desktop`, the system and user systemd units, the desktop
entry, the SVG icon, notices, and non-secret examples. It creates only the
configuration skeletons `/etc/synveil` and
`/etc/synveil/credentials`; it does not seed a credential or a database.

The Windows ZIP contains `synveil-desktop.exe` and `synveil-client.exe` beside
`qt.conf`, `platforms/qwindows.dll`, the Qt DLL/QML/plugin closure, any
required target C++ runtime DLLs, `LICENSE`, `NOTICE`, and a deterministic
`SYNVEIL-MANIFEST.txt`. The manifest records the package version, platform,
relative paths, sizes, and SHA-256 values and is validated before archiving.
No service, scheduled task registration, admin elevation, password, or
machine-wide autostart is included in the ZIP. The running client manager may
explicitly create a current-user Task Scheduler registration according to the
Windows desktop contract.

## Build commands

Builds write only to ignored `target/` directories and do not stage, commit,
or publish release artifacts.

```bash
# Build both Linux formats from the current locked release binaries.
./deploy/packages/build.sh --format=all --output-dir=target/packages

# Reuse already audited binaries when assembling another archive.
./deploy/packages/build.sh --format=deb \
  --binary=target/release/synveil-scheduled-maintenance-once \
  --client-binary=target/release/synveil-client \
  --desktop-binary=target/release/synveil-desktop \
  --output-dir=target/packages

# Native Windows runner: windeployqt resolves the genuine Qt closure.
./deploy/packages/build-windows.sh --output-dir=target/windows-packages

# Linux cross-build: supply real x86_64 Windows binaries and a Windows Qt
# prefix; the script copies only its explicit runtime/QML allowlist.
./deploy/packages/build-windows.sh \
  --desktop-binary=path/to/synveil-desktop.exe \
  --client-binary=path/to/synveil-client.exe \
  --qt-prefix=path/to/windows/Qt \
  --output-dir=target/windows-packages
```

`build.sh` requires `cargo`, Qt development/runtime packages for the desktop
crate, `rpmbuild` for RPM output, and `ldd` for the Linux dependency audit.
`dpkg-deb` is preferred for DEB creation; the checked-in rootless fallback
creates the same real ar/tar DEB container when `dpkg-deb` is unavailable.
`SOURCE_DATE_EPOCH` may be set to a non-negative integer. When it is omitted,
both builders use the checked-out source revision timestamp, normalize staged
file and directory mtimes, sort archive entries, and normalize archive
ownership. Rebuilding the same source and audited binaries must produce the
same package bytes. Release Cargo builds also disable incremental compilation
and remap checkout, Cargo-cache, and temporary target paths to stable synthetic
prefixes. The Linux builder writes
`SYNVEIL-LINUX-ARTIFACT-MANIFEST.txt` beside the packages; its source-tree
fingerprint, toolchain, ELF build IDs, sizes, and SHA-256 values bind expected
metadata to the exact build inputs. It is generated per build and is not a
checked-in binary hash.

The release parity check does not compare against a checked-in desktop binary:
the repository has no such binary or expected SHA-256. Its existing package
test compares each package's executable bytes with the current
`target/release` output. The desktop release build uses CXX-Qt's native
`cc::Build` path and Qt's QML AOT compiler. The shared builder applies Rust and
C/C++ prefix maps and sets `QT_HASH_SEED=0` for build tools so QHash iteration
cannot reorder generated QML code between processes. The desktop build script
tracks those environment inputs. It also wraps the host qmake/rcc tools:
QML-module resources are copied under Cargo's disposable output directory and
the copies receive the fixed SOURCE_DATE_EPOCH before rcc runs. The source
QML files are never touched. The vendored CXX-Qt build helper sorts Qt module
names before emitting native link directives, so Rust HashSet iteration cannot
change ELF symbol and text layout. CI clears only Cargo's release outputs,
rebuilds with the same flags, then compares exact executable bytes.
The manifest path is also required to name the exact `target/release` file
whose hash it records.

Linux `ldd` output is a hard gate: a `not found` dependency aborts the build.
The DEB declares the direct `libdbus-1-3` and `libsystemd0` dependencies in
addition to glibc, libgcc/libstdc++, and the Qt 6 Core/Gui/Widgets/QML/Quick/
QuickControls2/Network runtime. RPM declares the corresponding systemd,
DBus, and Qt runtime package families. PostgreSQL server/CLI, Docker, Nginx,
Redis, and any credential are intentionally absent.

Native Windows packaging requires `windeployqt` plus `llvm-readobj` or
`dumpbin` (and `zip`). Cross packaging requires a real Windows Qt prefix and
PE binaries. Every non-system PE import must be present in the ZIP; Linux
shared libraries, SDK material, headers, import libraries, static archives,
developer paths, and repository paths are rejected.

## Install and system integration

DEB/RPM package hooks and `deploy/install/install.sh` are deliberately
non-starting. They may apply `systemd-sysusers`, `systemd-tmpfiles`, and a
manager reload on a real host, but they do not silently enable the maintenance
timer, start a maintenance cycle, enable the desktop user unit, or register a
Windows task. Provision the administrator-controlled database credential first:

```text
/etc/synveil/credentials/database-url  root:root  0600
```

The package does not contain that file. The scheduled-maintenance service
receives it with systemd `LoadCredential`; ordinary configuration contains
only non-secret tuning. Enable the system timer only after reviewing the
credential and deployment policy. Enable the user client explicitly with
`systemctl --user enable --now synveil-client.service` when that is the chosen
desktop policy; opening the GUI does not silently undo an explicit disable.

The desktop/client processes are sibling executables. If the client is not
installed, unavailable, or cannot be started, the GUI exposes a generic
recovery/availability message and preserves the profile state; it does not
show a stack trace, raw session token, credential, or arbitrary filesystem
path. A missing optional profile or an empty first-run state remains a normal
setup path, not an installer-side database migration.

## User data, upgrade, and uninstall

The desktop profile boundary is user-owned and is separate from package files:

| Platform | Configuration | Persistent state/profile | Cache/runtime |
|---|---|---|---|
| Linux | `$XDG_CONFIG_HOME/synveil` or `~/.config/synveil` | `$XDG_DATA_HOME/synveil` or `~/.local/share/synveil` | XDG cache plus runtime under the user runtime/state boundary |
| Windows | `%APPDATA%\Synveil` | `%LOCALAPPDATA%\Synveil` | `%LOCALAPPDATA%\Synveil\Cache` and `Runtime` |

OS-backed Secret Service/Credential Manager storage remains the secret
boundary. Credentials, profile preferences, local sync SQLite state, and
external storage roots are not package-owned files.

An upgrade replaces only PACKAGE artifacts and preserves configuration,
profile/auth state, SecretStore entries, local sync databases, and external
data. The installer uses path containment checks and atomic replacement for
PACKAGE files. It does not run a migration as a packaging side effect. The
current schema contract remains 36 server migration files, 7 client-sync
migration files, and `LOCAL_SCHEMA_VERSION = 7`.

The package-neutral uninstall default removes only the known PACKAGE files.
It preserves `/etc/synveil`, credentials, `/var/lib/synveil`, user profiles,
external pools, and PostgreSQL. It never recursively follows a package-path
symlink or removes a shared parent. Native DEB purge and RPM erase are also
data-preserving. The explicit staged-root administrative operation below is
the only supported destructive application-data cleanup:

```bash
./deploy/install/uninstall.sh --root=/tmp/synveil-root --purge
```

`--purge` removes the allowlisted `/etc/synveil` and `/var/lib/synveil`
targets after containment checks, but still never removes external storage,
mounted volumes, home data, or PostgreSQL. Account removal is not automated.

## Validation contract

The required repository gates are:

```bash
cargo fmt --all -- --check
cargo check --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
git diff --check
cargo test -p synveil-metadata --test production_packaging_units --locked -- --nocapture
cargo test -p synveil-metadata --test linux_native_packaging_units --locked -- --nocapture
cargo test -p synveil-metadata --test linux_install_lifecycle --locked -- --nocapture
scripts/verify-release-build-reproducibility.sh
scripts/validate-release-artifacts.sh \
  --manifest=target/packages/SYNVEIL-LINUX-ARTIFACT-MANIFEST.txt
```

The packaging unit set is organized as PACKAGE-UNIT-1 through PACKAGE-UNIT-10:
metadata, executable layout, desktop entry/icon, version consistency, install
path safety, upgrade state preservation, uninstall data preservation, runtime
dependency closure, Windows manifest, and Linux ownership/mode policy. Artifact
inspection runs when local DEB/RPM/ZIP outputs exist; otherwise the source
contract remains testable and the test reports the artifact check as skipped.

The release-artifact unit set is ARTIFACT-UNIT-1 through ARTIFACT-UNIT-6:
stable same-source metadata, intentional hash-tamper detection, developer-path
rejection, temporary-build-path rejection, generated-manifest parity, and
mismatch diagnostics. A manifest from another source fingerprint fails closed
with the expected/current provenance values; the validator never accepts an
arbitrary hash or suppresses a mismatch.

Live package gates on a Linux packaging runner are `systemd-analyze verify`,
DEB/RPM metadata and file-list inspection, two-byte-identical package builds,
and a disposable staged install/upgrade/uninstall/purge rehearsal. Native
Windows gates are `windeployqt` closure resolution, PE import audit, ZIP
manifest verification, current-user Task Scheduler behavior, and portable
startup on a Windows host. A Linux cross-build is not evidence of native
Windows execution.

This release slice intentionally leaves package signing, repository
publication, a full guided Windows installer, auto-update, rollback of a
partially installed package set, and native macOS/iOS/Android packaging to a
later release-engineering scope. PostgreSQL integration tests remain an
environment gate when `SYNVEIL_TEST_DATABASE_URL` is unset; that limitation
must not be reported as package or product failure.
