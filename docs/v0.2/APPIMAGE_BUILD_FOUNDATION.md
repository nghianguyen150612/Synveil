# AppImage build foundation

## Contract and artifact identity

P015 qualifies one portable target: **Linux x86_64**. The canonical output is
`target/packages/Synveil-<workspace-version>-x86_64.AppImage`; the version is
read from `[workspace.package]` in `Cargo.toml`. `build-appimage.sh` is the
single entry point. It builds the release `synveil-desktop` and
`synveil-client`, stages them in a temporary AppDir, validates the closure,
creates the image, independently repeats the staging/image operation, and
publishes only after byte comparison succeeds. Scheduled server maintenance
is intentionally absent.

The AppDir has `AppRun`, `synveil.desktop`, `synveil.svg`, the two executables
under `usr/bin`, and the deployed libraries, Qt plugins, and QML modules under
`usr`. Nothing in the image is a development artifact or host integration
installer. `AppRun` replaces developer Qt search paths with image-local paths,
checks the client and runtime closure before launch, exposes the packaged
client path as `SYNVEIL_PACKAGED_CLIENT`, and then directly executes the
desktop. It does not manage either process's lifecycle.

## Selected tooling and provenance

The builder uses linuxdeploy `1-alpha-20251107-1` and the linuxdeploy Qt plugin
`1-alpha-20250213-1`. Their x86_64 AppImages are downloaded only over HTTPS
from the corresponding GitHub release URLs and checked against pinned SHA-256
digests before execution. There is no `curl | sh`, floating tool URL, or
unchecked executable. linuxdeploy supplies the dependency deployment and
AppImage construction path; its Qt plugin handles libraries, platform plugins,
and imported QML modules. This is smaller and more maintainable than a custom
ELF/QML dependency resolver.

`SOURCE_DATE_EPOCH` comes from the shared release helper (explicit input, then
the checked-out commit timestamp, then zero for a source archive). AppDir
mtimes are normalized before image creation. The two images are independently
created, not copied or normalized after creation, and `cmp` is a mandatory
gate. Exact hashes are runtime evidence and are never hard-coded as product
evidence in this document.

## Runtime closure and host boundary

The bundled closure includes the dynamically required non-foundational
libraries, Qt 6 Core, GUI, Widgets when linked, Network, QML, Quick and Quick
Controls dependencies, QML module metadata/plugins, and the xcb/offscreen QPA
platform support selected by linuxdeploy. The validator rejects a missing Qt
Core/GUI/Network/QML/Quick library, QML module metadata, or platform plugin.

The image deliberately relies on the host kernel, ELF loader, glibc and its
NSS/resolver integration, libpthread/libdl/librt where supplied by glibc, GPU
drivers, display server client/driver boundary, DBus session, and Secret
Service implementation. Blindly copying every `ldd` result would make those
security-sensitive ABI boundaries less safe. Hosted CI builds on Ubuntu 22.04
and records the highest referenced `GLIBC`, `GLIBCXX`, and `CXXABI` symbol
versions from the actual executables as artifact evidence. The qualified glibc
floor is therefore the observed final-artifact floor from that conservative
builder, not an unverified universal-distro claim.

The initial execution baseline is x86_64, a Linux kernel capable of running
the produced ELF/SquashFS image, a graphical X11 or Wayland desktop for normal
use, a user DBus session, and a Secret Service provider for durable
credentials. Normal type-2 AppImage mounting needs FUSE 2 support. CI also
tests the format's `APPIMAGE_EXTRACT_AND_RUN=1` execution path; this is an
execution fallback, not a portable-home mode or manual-extraction user journey.

## Security and state invariants

The AppImage does not alter authentication, synchronization ownership, or
credential storage. The platform SecretStore remains authoritative; no secret
is stored in/beside the image or in a durable environment variable. Absence of
a Secret Service continues through the existing typed product error path.

The desktop/client IPC remains local and user-scoped through existing XDG
runtime ownership. Packaging adds no TCP listener, shared `/tmp` socket, or
system namespace. Configuration, data/state, cache, runtime files, SecretStore
records, and library roots remain in their existing XDG/user locations. Neither
`$APPDIR` nor the AppImage directory becomes writable application state.

Ordinary execution performs no root operation, package-manager mutation,
write to `/usr`, `/etc`, or `/var`, service installation/activation, desktop or
icon registration, autostart, or `~/.local/bin` symlink. Missing/corrupt
packaged runtime elements fail closed rather than searching developer paths.

## Evidence and scope boundary

`validate-appimage-build.py` always checks the static contract. Given an
AppDir it checks its required payload and runtime closure. Given an AppImage it
extracts and inspects real bytes; `--smoke` launches the exact image from a
disposable XDG home with developer overrides removed and
`QT_QPA_PLATFORM=offscreen`. The generated release manifest hashes the real
artifact and lists only `synveil-desktop` and `synveil-client` as components.

P016 retains optional desktop/icon registration, relocation and stale-entry
handling, persistent background-client integration, and repair/removal. P017
quick install, P018 distro detection/qualification, P019 friendly first-launch
service behavior, and P020 broad clean-machine acceptance also remain pending.
