# Fedora RPM desktop package UX

## User journey and qualification boundary

The native journey is: download the `synveil-<version>-1.x86_64.rpm`, open it
with Fedora's graphical software/package application, choose **Install**, then
launch **Synveil** from the application menu. The package metadata, launcher,
icon, dependencies, and native transaction support this journey. Prompt014's
automated evidence uses DNF in the exact `fedora:42` userspace container; it
does not claim that graphical mouse interaction or a complete Fedora desktop
VM was tested. That clean-machine graphical evidence remains P020 scope.

The initially qualified architecture is x86_64. This evidence does not qualify
aarch64 or other RPM distributions and must not be read as support evidence for
RHEL, CentOS, Rocky Linux, AlmaLinux, or openSUSE.

## Native package contract

`deploy/packages/build.sh` remains the only package builder and renders
`deploy/packages/rpm/synveil.spec.tmpl`. RPM/DNF owns dependency resolution,
the transaction, the package database, package files, and ordinary erase.
Synveil does not emulate the native manager.

The RPM describes the product as a private file synchronization desktop
application and declares the Fedora runtime families used by the installed
binaries:

- `glibc`, `libgcc`, and `libstdc++`;
- `systemd` and `systemd-libs` for units, sysusers, and tmpfiles;
- `dbus-daemon` and `dbus-libs` for the session/system D-Bus runtime;
- `gnome-keyring` for the Secret Service provider;
- `qt6-qtbase`, `qt6-qtbase-gui`, `qt6-qtdeclarative`, and
  `qt6-qtquickcontrols2` for Qt Core/GUI/Widgets/Network, QPA, QML/Quick, and
  Quick Controls.

Compiler toolchains, development packages, Cargo/Rust, database servers,
Docker, and web servers are not desktop runtime requirements.

## Desktop and lifecycle behavior

The canonical desktop entry directly executes `/usr/bin/synveil-desktop` and
uses the `synveil` hicolor scalable icon. The background client unit remains a
user unit at `/usr/lib/systemd/user/synveil-client.service`; package install
does not enable or start it and creates no XDG autostart entry.

RPM scriptlets are bounded to system identity/tmpfiles setup, configuration
directory permissions, and system-manager reload/maintenance shutdown when a
manager exists. They do not infer a user from environment variables, touch a
home directory, launch either Synveil process, create credentials, enable a
unit, run maintenance, bootstrap a server, or contact the network. `%install`
is executed by rpmbuild's `/bin/sh` and therefore uses portable `set -eu`, not
the Bash-only `pipefail` option.

## Acceptance gates

The hosted Fedora 42 userspace gate downloads the exact RPM produced by the
construction job and then:

1. validates real RPM identity, metadata, requirements, file ownership, and
   modes;
2. installs it with DNF and queries the installed RPM database;
3. proves neither desktop nor client was launched and the user service was not
   enabled;
4. runs `/usr/bin/synveil-desktop --qml-smoke-test` offscreen as a dedicated
   non-root user with clean HOME/XDG directories and developer overrides
   removed;
5. creates controlled `/etc/synveil`, `/var/lib/synveil`, user profile, and
   external-library sentinels;
6. removes Synveil with DNF, proves package files and the RPM database entry are
   gone, and proves all durable sentinels remain.

The package job independently rebuilds DEB and RPM artifacts from the same
already-built release executables through the canonical builder, requires
byte-identical SHA-256 results for each format, and only then validates both
artifact and release provenance manifests. Prompt013's focused validator,
DEB inspection, reproducibility, APT installation, installed smoke, removal,
and preservation gates remain intact.

Actual hashes and hosted PASS results belong to the final PR-head workflow
logs; this source document does not predict them. Package signing and repository
publication, AppImage work, quick install, broader distro detection, friendly
first-launch/autostart UX, and graphical clean-machine qualification remain
P015–P020 scope.
