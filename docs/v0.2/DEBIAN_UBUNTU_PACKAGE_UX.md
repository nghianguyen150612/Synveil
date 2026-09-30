# Debian and Ubuntu desktop package UX

## User journey and qualification boundary

The native package journey is:

```text
download synveil_<version>_amd64.deb
→ open it with the desktop's graphical package application
→ Install
→ open Synveil from the application menu
```

The package is built only by `deploy/packages/build.sh`. It installs the
`Synveil` application launcher, the `synveil` hicolor icon, the production
desktop and client executables, and the client user unit. The launcher directly
executes `/usr/bin/synveil-desktop` as the signed-in user. Package installation
does not launch either process, enable the user service, or add an XDG
autostart entry.

Prompt013 qualifies the exact newly built `amd64` DEB on the GitHub-hosted
`ubuntu-latest` image. CI inspects the real archive, installs it with APT on a
fresh runner, performs the bounded installed `/usr/bin/synveil-desktop
--qml-smoke-test` offscreen as the non-root runner, and removes it with APT.
This is native package-manager and installed-runtime evidence, not automated
mouse interaction with GNOME Software and not qualification of every Debian or
Ubuntu release. Broader distribution discovery and clean-machine graphical
acceptance remain P018 and P020.

## Runtime closure

The DEB declares glibc and C++ runtimes; systemd and DBus integration; a user
DBus session and GNOME Secret Service provider; Qt 6 Core, GUI, Widgets,
Network, QML, Quick and Quick Controls libraries; the Qt QPA plugins; and each
QML module imported by the embedded desktop UI. Compiler, SDK, `-dev`, Cargo,
and server packages are not runtime dependencies. The installed application
therefore does not need a source checkout, Cargo target directory, developer Qt
paths, or environment overrides.

## Removal and preservation

Ordinary `apt-get remove synveil` removes package-owned executables, launcher,
icon, and units. It preserves administrator configuration under
`/etc/synveil`, application state under `/var/lib/synveil`, per-user
configuration/profile state, and all external synchronized library roots.
The native removal path is not a destructive purge operation.

## Determinism and provenance

The package workflow builds primary DEB/RPM artifacts, then invokes the same
canonical builder with `--output-dir=target/packages-reproducible`. Only after
that real producer completes do byte comparison and primary/rebuilt release
manifest validation run. A focused regression test binds the producer command,
its exact output root, both produced manifests, and their consumers so a step
reorder or path drift fails before package publication.

RPM construction remains a shared packaging regression gate only. Fedora/RPM
desktop UX and RPM-specific acceptance remain P014. First-launch/autostart UX
remains P019.
