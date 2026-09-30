# Debian/Ubuntu desktop package UX

Status: **Prompt013 implemented; native-clean-machine acceptance deferred**.

## Normal journey and ownership

The ordinary journey is: download the versioned DEB, double-click it, authorize
the native graphical package application, choose Install, and open Synveil from
the application menu. Synveil provides no second installer wizard. The native
frontend and APT/dpkg own privileged authorization, dependency resolution, the
package database, files, upgrade, and removal. Terminal commands are validation
and advanced administration tools, not normal-user steps.

The DEB metadata identifies `synveil` `0.1.0`, `amd64`, and declares the exact
P012 runtime dependency list, including Qt/QML runtime packages. Developer Qt
packages are not runtime requirements and no repository checkout is needed.

## Desktop integration and privilege boundary

The package installs `/usr/share/applications/synveil.desktop` and the hicolor
icon `/usr/share/icons/hicolor/scalable/apps/synveil.svg`. The visible,
desktop-neutral entry directly runs `/usr/bin/synveil-desktop`, uses icon name
`synveil`, and sets `Terminal=false`. It contains no shell or privilege helper.
Package installation can be privileged; launcher execution is deliberately the
signed-in user's process and must never be run by a root package hook.

Installation does not launch the GUI, start or enable the user client service,
create an XDG autostart entry, enable maintenance, provision a server, migrate a
database, create credentials, or contact a Synveil server. The user-level client
unit is shipped but remains inactive. Hooks are deterministic, bounded,
network-free, noninteractive, and preserve durable configuration and data.

## Evidence and limits

The Prompt013 validator closes its schema, reconciles P012/build/manifest and
desktop sources, uses `desktop-file-validate` where available, and explicitly
reports artifact absence. With an actual DEB it inspects metadata, exact
dependencies, contents, ownership, modes, and byte-identical installed desktop
entry. Native CI builds the real artifact and installs that exact DEB through APT on
the disposable Ubuntu runner. It verifies dpkg registration, installed desktop
metadata/icon, and executes the installed `/usr/bin/synveil-desktop` as the
non-root runner through the existing Qt offscreen QML smoke path. The job then
removes the package through APT and verifies scoped config/state sentinels remain.
This is `ci-native-scoped`, not proof that GNOME Software, Ubuntu App Center,
GDebi, or Discover was exercised.

No Debian or Ubuntu release number, derivative, or ARM64 system is advertised.
P019 owns final first-launch/background-client opt-in behavior. P020 owns real
graphical double-click and native clean-machine qualification. P014–P018 and
P019–P020 remain deferred, and the Phase C checkpoint remains pending.


## Shared native-package CI portability note

The first hosted Prompt013 artifact run successfully built the DEB, then exposed a pre-existing RPM `%install` portability defect. RPM executes that block with POSIX `/bin/sh`, where `pipefail` is not defined. The template now uses `set -eu`, retaining fail-fast and unset-variable checks while remaining POSIX-compatible. This is a narrow shared-CI unblock so the DEB UX evidence can proceed; Fedora/RPM desktop UX remains P014-owned.

## RPM reproducibility limitation

Hosted Prompt013 validation proves the DEB rebuild is byte-identical. The
corresponding RPM rebuild is currently not byte-identical even when it consumes
the same already-built binaries and package payload. That RPM-only defect is
deferred to P014 and is not called a pass. CI keeps the strict RPM comparison
enabled after the scoped DEB artifact, APT-install, launch, removal, static
packaging and systemd evidence so Prompt013 can be reviewed independently while
the overall native-package workflow remains red until the RPM defect is fixed.
