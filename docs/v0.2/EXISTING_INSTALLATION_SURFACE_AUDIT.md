# Synveil v0.1 existing installation surface audit

Status: **Prompt002 evidence audit**. Baseline: released `v0.1.0`, commit
`fa23232ff0154f627ebdd221ec5435134f177af0`. The audit was performed from
`main` at `8014ce27d84771c548c4f0639ea7c9fd3c160f8e`; the code and deployment
surface match the release baseline, while this checkout adds the v0.2 product
contract and roadmap. This document records v0.1 reality, not v0.2 delivery.

## Evidence vocabulary and limits

| Classification | Meaning in this audit |
| --- | --- |
| `SOURCE_PRESENT` | Implementation/configuration is present; usability is not implied. |
| `STATICALLY_VALIDATED` | Syntax, source-contract or build checks validate a limited property. |
| `CI_VALIDATED` | A named CI job runs the described path; see its scope below. |
| `NATIVE_RUNTIME_VALIDATED` | The feature was executed on its target OS, with the named environment and scope. |
| `DOCUMENTED_ONLY` | Operator instructions exist, but implementation/execution is not established by those instructions. |
| `FIXTURE_ONLY` | Synthetic, staged, mock or fixture coverage only. |
| `BLOCKED_BY_ENVIRONMENT` | The path exists but the required host/live environment was unavailable for this audit. |
| `NOT_IMPLEMENTED` | No implementation for the claimed user-facing capability was found. |
| `UNKNOWN` | Evidence is insufficient to classify. |

Source and docs are inspected at the v0.1 tag and corresponding unchanged
implementation in the audited checkout. CI workflow definitions prove what a
job is configured to execute, not that a particular run passed. Existing
release documentation records previous gates; it is not live evidence from
this audit. No package was installed into this host during this documentation
audit.

## Executive findings

The easiest implemented path is an x86_64 Linux DEB or RPM installed by the
distribution package manager, followed by opening Synveil from the desktop
menu. The package has an icon and launcher, but declares distro Qt/systemd/DBus
runtime dependencies and has no verified graphical-install acceptance in this
audit. Login startup for the background client is an explicit `systemctl
--user enable --now synveil-client.service` operation. The GUI can also start
its packaged sibling client for the current session.

Windows has a reproducible unsigned self-contained runtime ZIP. The user must
extract it and start `synveil-desktop.exe`; the ZIP is not an installer. There
is no AppImage, supported public quick-install script, product repair UI,
installer-managed upgrade flow, or guided server installer. Ordinary Linux
package removal and the package-neutral default uninstall preserve user and
server data.

The desktop already provides server-profile connection/authentication and a
local first-library setup surface. It does not provision or host a server,
install server dependencies, or provide a clean-machine installation wizard.

## Artifact inventory

| Surface | v0.1 implementation and payload | Runtime/integration assumptions | Evidence |
| --- | --- | --- | --- |
| Windows x86_64 | `deploy/packages/build-windows.sh` creates `synveil-<version>-windows-x86_64.zip`: `synveil-desktop.exe`, `synveil-client.exe`, `qt.conf`, Qt/QML/plugins, `qwindows.dll`, needed C++ runtime DLLs, license/notices and SHA-256 manifest. No installer technology, service, elevation, task registration, or machine-wide autostart is in the archive. | User extracts to a user-owned directory and launches desktop EXE. Task Scheduler is registered at runtime only after explicit user enablement. App files are not installed/uninstalled by Synveil. | `SOURCE_PRESENT`; `CI_VALIDATED` by `.github/workflows/ci.yml` native Windows build, PE/Qt checks, ZIP parity and packaged QML smoke launch. Interactive installed-app, task registration and end-user startup are not established by the smoke test. Native interactive acceptance is `BLOCKED_BY_ENVIRONMENT` for this audit. |
| Debian / Ubuntu x86_64 | `deploy/packages/build.sh --format=deb`; package `synveil`, payload from `deploy/install/MANIFEST`: three executables (desktop, client, scheduled-maintenance one-shot), system maintenance units, user client unit, desktop entry/icon, docs, examples and config skeleton. | Package manager must resolve Qt 6, DBus, systemd and C/C++ runtime dependencies. `postinst` applies `systemd-sysusers`, `systemd-tmpfiles`, sets config directory ownership and reloads systemd if available. It does not enable or start services. Root privilege is supplied by package manager/administrator. | `SOURCE_PRESENT`, `CI_VALIDATED` for build, `dpkg-deb` metadata/content inspection, reproducibility and static/staged lifecycle tests in `linux-packages.yml`. Native GUI double-click install and launch are `BLOCKED_BY_ENVIRONMENT` / unverified here. |
| Fedora / RPM family x86_64 | Same manifest payload; `rpmbuild` with `deploy/packages/rpm/synveil.spec.tmpl`; RPM declares systemd, systemd-libs, glibc, libgcc/libstdc++, DBus and Qt runtime families. | DNF/RPM manager resolves dependencies and invokes scriptlets as privileged package lifecycle. `%post` creates service identity/directories and daemon-reloads but does not enable/start. RPM format is not proof for every RPM distribution/version. | `SOURCE_PRESENT`; `CI_VALIDATED` for RPM build, metadata/file/dependency inspection, deterministic rebuild and static/staged lifecycle checks. Native Fedora GUI install/launch is `BLOCKED_BY_ENVIRONMENT` / unverified here. |
| Generic Linux | No AppImage, Flatpak, Snap, public tarball or supported portable directory found. Linux DEB/RPM are distribution package formats. `deploy/install/install.sh` is a package-neutral administrative/staging layer, not a quick installer. | DEB/RPM require distro runtime dependencies, desktop session and package manager. Secret Service/keyring is needed for durable credentials. | `NOT_IMPLEMENTED` for AppImage/Flatpak/Snap/generic portable and supported quick-install. Package source/build checks are `CI_VALIDATED`; no generic Linux runtime claim. |

The desktop packages are not server distributions. `synveil-api` and
`synveil-worker` are workspace deployment binaries but are not in the desktop
manifest. No PostgreSQL server, Docker, Nginx, Redis, database, or credential
is bundled.

## Windows details

There is no MSI, MSIX, NSIS, Inno Setup, WiX, or other Windows installer
project. Qt dependencies are copied into the ZIP: native packaging uses
`windeployqt`; cross packaging requires genuine Windows binaries and Qt prefix
and copies an explicit runtime/QML allowlist. The directory/ZIP location is
chosen by the user; no installed destination, Start Menu integration,
shortcuts, registry uninstall entry, repair, or upgrade is implemented.

The desktop entrypoint initializes Qt/QML and loads the embedded
`Main.qml`. The desktop manager resolves `synveil-client` as a sibling
executable, probes its per-profile local control endpoint and can launch it
directly. Persistent background startup is current-user Task Scheduler, not a
Windows service. The runtime task definition is user-scoped and designed not
to require elevation/password; registration is only on explicit enablement.
Linux unit commands do not apply on Windows.

Windows local control uses a profile-scoped named pipe; Linux uses a
profile-scoped Unix-domain socket. Neither transport is a public TCP/HTTP
desktop-control listener. Profile/config/data/cache/runtime paths use
`%APPDATA%` / `%LOCALAPPDATA%` platform locations unless explicitly overridden
by test/runtime variables. Credentials are delegated to Windows Credential
Manager; the desktop profile/config does not own durable credential values.

The native Windows CI job builds with Windows Qt/MSVC, tests client policy,
packages the ZIP, verifies PE architecture/runtime closure and starts the
packaged QML shell in an isolated smoke environment. This is native Windows
CI runtime evidence for that smoke scope, not installed-app acceptance: it
does not exercise a Windows installer, interactive UI journey, persistent
Task Scheduler registration/reboot, repair, upgrade or uninstall. The local
audit host is Linux, so those interactive/native installed-app checks remain
`BLOCKED_BY_ENVIRONMENT`.

## Debian / Ubuntu details

The DEB is built with `dpkg-deb` when available, with a real ar/tar fallback.
Metadata declares `synveil`, architecture/version, direct DBus/systemd and Qt
runtime dependencies. Payload paths are `/usr/bin`, `/usr/lib/systemd/{system,
user}`, `/usr/share/applications`, `/usr/share/icons`, `/usr/share/doc`, plus
configuration skeleton paths. `postinst`, `prerm`, and `postrm` have POSIX
shell syntax checks and explicit lifecycle cases.

`postinst` requires `systemd-sysusers` and `systemd-tmpfiles`, creates the
`synveil` maintenance identity/state/config skeleton and may reload systemd.
It does not auto-start maintenance or enable the user client. The desktop
launcher is `/usr/share/applications/synveil.desktop`; icon is installed in
hicolor. Opening the menu launcher runs as the signed-in user, not root.

The current package reaches “download .deb → package manager install → open”
only where a graphical package handler and all declared native dependencies
are available. This audit found no native desktop double-click/install
acceptance evidence. Manual persistent-client startup remains the explicit
user-level systemd enable operation. Package install/upgrade/removal uses
package-manager privilege; the package-neutral install layer is administrator
tooling and requires explicit host-root operation.

## Fedora / RPM details

RPM package creation requires `rpmbuild`; the spec owns package metadata,
runtime `Requires`, `%files`, `%post`, `%preun`, `%postun` lifecycle. The
payload and desktop entry/icon match the manifest. Scriptlets create the
maintenance account/config directories and reload the system manager, but do
not enable or start client/maintenance services. RPM installation/removal and
scriptlets run with package-manager privilege. Upgrade replaces package files
and preserves timer enablement. Erase is data-preserving. No separate product
repair operation or graphical Fedora acceptance evidence was found.

The current-to-target blockers for both package families are not archive
creation: there is no recorded qualified clean native desktop environment
showing graphical double-click install, dependency resolution, launch, login
supervision choice, and post-install behavior end to end. P013/P014 own those
Linux UX/qualification journeys; P020 owns clean-machine acceptance.

## Quick-install and generic Linux

Repository search found no supported shell script that jointly detects
architecture and distribution, selects an authenticated release, downloads,
verifies, installs and guides launch. Build/install commands are source-tree
operator tooling. Package builders create local artifacts; install/uninstall
scripts operate a known manifest and do not select/download releases. There is
no `curl | sh` product path, release metadata trust bootstrap, package
signature verification path, distro selector, or user-facing unsupported-OS
experience. HTTPS/checksum functionality in build artifact manifests is not
release-download authentication. A digest published beside an artifact is
not an independent signature/trust root.

`AppImage` does not exist today. P015/P016 are future roadmap work. DEB/RPM
runtime closure is distro-provided and checked by build-time `ldd`; this does
not establish self-contained generic Linux compatibility.

## Desktop first run and control relationship

| Capability | State | Evidence |
| --- | --- | --- |
| Qt/QML desktop entrypoint and embedded UI | Implemented | `crates/desktop/src/main.rs`, `crates/desktop/qml/Main.qml`; Linux CI/offscreen and Windows packaged smoke are limited runtime checks. |
| Desktop starts/attaches to separate client | Implemented | `crates/client/src/launch.rs`, `crates/client/src/control.rs`; GUI lifecycle is separate from sync lifecycle. |
| Linux persistent client supervision | Implemented, opt-in | `/usr/lib/systemd/user/synveil-client.service`; package does not enable it. Desktop can directly start sibling client for current session. |
| Windows persistent client supervision | Implemented, opt-in | Current-user Task Scheduler definition/manager in `crates/client/src/launch.rs`; ZIP itself does not register it. Registration native interactive acceptance is not established by smoke launch. |
| Configure server origin/profile and authenticate | Implemented | UI/control surface; OS SecretStore owns credentials (Linux Secret Service, Windows Credential Manager). SecretStore unavailable/unsupported is an explicit runtime constraint, not permission to persist credentials in UI config. |
| Create first local library | Implemented | UI selects logical name and local folder after authenticated server profile. Existing non-empty folders are treated as local content, not historical deletion instructions. |
| “Welcome”, “Host Synveil”, “Connect to Synveil” install wizard | Not implemented as a guided installation/server-choice flow | The existing UI has a connect-to-server empty state and profile/library forms; it is not a host-server wizard. |
| First-run installation recovery / product repair | Not implemented | Runtime/library recovery surfaces are not package repair. |

Local IPC uses profile-scoped Unix sockets on Linux and named pipes on Windows,
with local user ownership/access checks. It does not introduce public desktop
TCP/HTTP control. Synchronization belongs to `synveil-client`; packaging and
desktop do not become another sync authority.

## synveil-client startup requirements

`synveil-client` is a separate executable. Its normal packaged invocation is
the sibling executable with no installer-supplied mandatory argument list;
profile/config paths use platform resolvers and can be overridden by
`SYNVEIL_*_DIR` / client config variables for tests or managed deployments. It
needs a user session/runtime path for local IPC, writable user config/data,
local SQLite state, OS SecretStore access for device credentials, a configured
profile/server origin and library root for useful synchronization, and the
matching desktop/client protocol. The desktop starts it when unavailable;
the package itself does not.

Linux login startup requires an administrator/user package installation plus
the signed-in user's systemd user manager and explicit:

```sh
systemctl --user daemon-reload
systemctl --user enable --now synveil-client.service
```

Windows startup requires a user-owned extracted ZIP and explicit enablement of
the current-user scheduled task in the desktop's startup controls. If disabled,
the desktop can launch the client for the active session but does not silently
reverse the preference. Missing executable, inaccessible endpoint, protocol
mismatch and unavailable SecretStore are surfaced as bounded availability or
configuration states. The durable profile and sync database are preserved.

## Server and PostgreSQL deployment reality

The current server is an operator deployment, not an installer journey.
Required components are Linux host, `synveil-api`, PostgreSQL 17, durable
object/content storage, HTTPS termination and an operator-managed process
supervisor/backup destination. Build/API sources include migrations; with
`DATABASE_URL`, API startup requires `SYNVEIL_REBASELINE_TOKEN_KEY`, connects
to PostgreSQL and runs forward migrations. `SYNVEIL_OBJECT_ROOT` enables local
object-store upload/download; without it those HTTP backends are unavailable.
`SYNVEIL_BIND_ADDR` defaults to loopback `127.0.0.1:3000`; public use therefore
needs deliberate bind/reverse-proxy/TLS/network configuration. `synveil-worker`
is a separate optional process and requires `SYNVEIL_OBJECT_ROOT` when enabled.

PostgreSQL installation, cluster initialization, role/database creation,
credentials, network/listener/TLS policy, backup/restore, monitoring and
upgrade lifecycle are administrator-managed. The repository does not bundle
PostgreSQL or automate its provisioning. The scheduled-maintenance unit is a
separate optional one-shot/timer, not API supervision. Its root-owned
`/etc/synveil/credentials/database-url` credential must be provisioned by an
administrator before explicitly enabling its timer. systemd `LoadCredential`
passes a runtime copy to that job. The desktop package does not create server
credentials or run server migrations as an install hook.

The browser bootstrap API/UI is an application-level initial administrator
flow after an API/PostgreSQL service is deployed and reachable; it is not
server installation or PostgreSQL bootstrap. Therefore the v0.2 journey
“Host Synveil on this device” is impossible from the v0.1 desktop installer
surface.

## Services and privilege

| Service/task | Scope and activation | Privilege/evidence |
| --- | --- | --- |
| `synveil-client.service` | Per-user systemd unit, `default.target`; explicit user enable only. `Restart=on-failure`, bounded rate policy. | User session, not root. Unit source/policy CI and staged checks; no inference of every distro's live user manager. |
| Windows background task | Current-user Task Scheduler logon task, explicit user enable. | User context; no Windows Service or admin task in ZIP. Policy is source/CI tested; native registration/reboot acceptance is not established by packaged QML smoke. |
| `synveil-scheduled-maintenance.service` + timer | Root/system systemd one-shot and approximately minutely timer; not auto-enabled. Requires operator database credential. | Package installation/hooks require package-manager privilege. Unit sandbox and staged/live evidence are described in deployment docs; it is unrelated to desktop sync supervision. |
| API and optional worker | Separate processes managed by operator-selected supervisor. | No packaged service unit or automated supervisor install for API/worker. Deployment is documented; the ordinary install path is not implemented. |

Windows ZIP extraction and launch are user-level, with no elevation. Linux DEB
and RPM install/remove invoke native package privilege. The staged package-
neutral install is not a replacement for native package-manager privilege or
a public end-user installer.

## Dependency inventory

| Class | Dependencies / expectations | Evidence |
| --- | --- | --- |
| Build | Rust/Cargo locked workspace; Qt 6 dev/runtime and QML tooling for desktop; Linux `ldd`, `fakeroot`/`dpkg-deb` (fallback ar/tar) for DEB; `rpmbuild` for RPM; Windows native `windeployqt` and PE reader, or real Windows cross-target binaries plus Qt prefix. | `SOURCE_PRESENT`; Linux package and native Windows CI jobs build/audit their respective outputs. |
| Linux runtime | x86_64 Linux, Qt 6 Core/Gui/Widgets/QML/Quick/QuickControls2/Network, DBus, systemd libraries, glibc/libgcc/libstdc++, desktop session; user systemd manager for persistent login startup; Secret Service/keyring for stored credentials. | Package metadata/build dependency audit `CI_VALIDATED`; native end-user install qualification not established here. |
| Windows runtime | x86_64 Windows; ZIP carries Qt/QML/plugin and needed C++ runtime closure; Windows Credential Manager for credentials. | Native CI packaged shell smoke `CI_VALIDATED`; installed-product acceptance is unavailable/not implemented. |
| Server external | Linux process environment, PostgreSQL 17, durable object root, TLS/reverse proxy, supervisor and backup destination. | Config/source present; operator deployment is `DOCUMENTED_ONLY` absent a named production host acceptance. |

## Configuration and environment inventory

| Variable/configuration | Consumer/use | Classification |
| --- | --- | --- |
| `DATABASE_URL` | API PostgreSQL connection; API also requires `SYNVEIL_REBASELINE_TOKEN_KEY` when configured. | Source-present, operator-provisioned secret/config; not seeded by package. |
| `SYNVEIL_REBASELINE_TOKEN_KEY` | API token-key material with PostgreSQL mode. | Source-present; operator secret provisioning. Never include values in audit/release docs. |
| `SYNVEIL_OBJECT_ROOT` | API local object store and worker GC path. | Source-present; operator chooses/creates writable durable location. |
| `SYNVEIL_BIND_ADDR` | API listen address, default loopback. | Source-present; network exposure is operator-configured. |
| `SYNVEIL_PUBLIC_ORIGIN` | API allowed-origin policy when provided. | Source-present; operator-configured. |
| `SYNVEIL_ENV`, `SYNVEIL_ALLOW_INSECURE_COOKIES` | Explicit development cookie policy. | Source-present; production must not copy insecure development setting. |
| `SYNVEIL_SCHEDULED_MAINTENANCE_LEASE_SECONDS` | Optional non-secret scheduled-maintenance tuning via systemd env file. | Source/package example; administrator config. |
| `SYNVEIL_DATABASE_CREDENTIAL_FILE` / systemd `LoadCredential` | Runtime path to protected service credential copy. | Source-present unit contract; secret source is manually provisioned. |
| XDG roots / `APPDATA`, `LOCALAPPDATA`; `SYNVEIL_CONFIG_DIR`, `SYNVEIL_DATA_DIR`, `SYNVEIL_CACHE_DIR`, `SYNVEIL_RUNTIME_DIR`, client config override | User-owned profiles, local databases, IPC/runtime and test isolation. | Platform source-present; defaults resolved by OS adapter. |

Secret values are deliberately excluded from this inventory.

## Upgrade, repair and uninstall

| Operation | v0.1 reality | Evidence |
| --- | --- | --- |
| Linux package upgrade | Native DEB/RPM package-manager replacement. Hooks preserve maintenance timer enablement; package-neutral install atomically replaces manifest-owned package files. No whole-product transaction/rollback. | Source-present; CI/static and staged lifecycle tests, not a live graphical upgrade in this audit. |
| Windows upgrade | No installer-managed update. User obtains/extracts/replaces portable ZIP files manually; no orchestration, migration, or rollback UI. | `NOT_IMPLEMENTED` as product upgrade. |
| Product repair | No repair command/UI for Windows or Linux installed product. Staged installer idempotence and runtime/library recovery are not a repair product. | `NOT_IMPLEMENTED`. |
| Linux ordinary uninstall | DEB/RPM remove package-owned files; data/config/credentials/external storage remain. RPM erase and DEB purge are intentionally data-preserving. Package-neutral `uninstall.sh` default removes manifest `PACKAGE` entries only. | Source-present; staged lifecycle tests/CI. Native interactive package-manager removal not run here. |
| Explicit package-neutral purge | `deploy/install/uninstall.sh --purge` allows only guarded `/etc/synveil` and `/var/lib/synveil` cleanup; never external volumes/PostgreSQL/home data. Not a native package hook/default. | Source-present; static/staged safety tests. |
| Windows uninstall | User manually removes extracted ZIP directory; profile, SecretStore, cache and scheduled task cleanup are not coordinated as one product uninstall. | `NOT_IMPLEMENTED` as product uninstall. |

## Manual-step inventory

| ID | Current operator/user action | Surface / why it remains manual | Future owner |
| --- | --- | --- | --- |
| MAN-01 | Obtain artifact, select OS/package and verify release identity by trusted release process. | No download selector, signed release metadata or public quick-install. | P005–P006, P017–P018 |
| MAN-02 | Install DEB/RPM through package manager with privilege; resolve platform/runtime eligibility. | Packages exist, graphical double-click journey not qualified. | P012–P014, P020 |
| MAN-03 | Extract ZIP and launch desktop EXE; manually replace/remove directory for lifecycle changes. | Portable artifact only; no Windows installed app. | P021–P027 |
| MAN-04 | Enable persistent client startup: systemd user commands on Linux; explicit Task Scheduler option on Windows. | Install is non-starting by policy; no auto-enable. | P007, P012–P014, P019, P026 |
| MAN-05 | Configure server profile, authenticate, choose library name/folder, start setup. | Existing desktop first-run covers connection/library, not installation. | P029, P035–P036, P041 |
| MAN-06 | Install/configure PostgreSQL, create roles/database, protect URL/key, run/verify API migrations. | No server provisioning wizard or package. | P029–P033, P035–P036 |
| MAN-07 | Provision object root, permissions/capacity/backup; configure API bind, TLS/reverse proxy, firewall/network. | Operator deployment responsibilities. | P032–P034, P036 |
| MAN-08 | Choose/start/supervise API and worker, configure restart/logging/backup/monitoring. | No API/worker service package or supervisor setup. | P033, P036 |
| MAN-09 | For scheduled maintenance, provision root-only DB credential and explicitly enable timer. | Separate optional system service; deliberately non-starting package. | P007, P012, P029–P033 |
| MAN-10 | On app removal, separately remove any user Task Scheduler registration and extracted files; decide whether persistent state should remain. | No coordinated Windows product uninstall. | P027 |

## Stable blocker inventory

| ID | Blocker and v0.2 effect | Evidence | Architectural/design owner | Delivery/acceptance owners |
| --- | --- | --- | --- | --- |
| INS-01 | Windows ZIP is portable only; no install destination, installer UI, owned-file lifecycle, or installed-app verification. | Source-present ZIP; CI smoke only. | P003, P021 | P022–P028 |
| INS-02 | Linux package creation does not prove graphical install → launch; distro/runtime qualification absent. | Package CI only. | P003, P004 | P012–P014, P020 |
| INS-03 | No authenticated release selection/download/install path or supported quick-install. | No implementation found. | P003, P005–P006 | P017–P018 |
| INS-04 | No generic self-contained Linux artifact; AppImage absent. | No artifact/build path. | P003, P005 | P015–P016, P020 |
| INS-05 | Background client persistent startup requires a separate explicit OS supervisor action; no installer consent/coordination. | Unit/task code and package no-start policy. | P003, P007 | P012–P014, P019, P026 |
| INS-06 | No unified installation transaction/recovery/repair/upgrade orchestration across package-owned files and platform state. | Linux file install lifecycle is narrow; no whole-product coordinator or Windows lifecycle. | P003, P008–P010 | P009, P020, P027–P028 |
| INS-07 | Current desktop cannot host/provision a server or its PostgreSQL/storage/supervisor dependencies. | API requires operator config; packages omit server and DB. | P003, P029–P034 | P030–P036 |
| INS-08 | PostgreSQL setup, secrets, object root, TLS/reachability and process supervision are separate operator tasks. | Source/docs, no provisioning implementation. | P003, P030–P034 | P031–P036 |
| INS-09 | No coordinated Windows uninstall that removes product integration while preserving or explicitly purging user state. | Portable ZIP has no uninstall entry. | P003, P009 | P027 |
| INS-10 | Installed-app native acceptance across clean supported hosts is absent from current audit evidence. | Existing CI is build/package/smoke scoped. | P004 | P020, P028, P036, P045 |

P003 must resolve component and package ownership, installer common stages,
transaction/recovery and rollback boundaries, privilege/consent rules, server
provisioning boundary, and repair/upgrade/uninstall data ownership. It must
preserve: client owns synchronization; desktop does not; GUI/client lifecycle
independence; OS SecretStore credential ownership; user-scoped local IPC;
no public desktop-control listener; data-preserving uninstall; unknown schema
or unknown mutation outcome fails closed.

## v0.1 reality → v0.2 contract delta

| Area | v0.1 reality | v0.2 contract delta | Principal blockers |
| --- | --- | --- | --- |
| Windows | Extract unsigned ZIP and run EXE. | `SynveilSetup.exe` guided per-user install; install desktop/client/runtime; explicit login-start choice; launch on finish; repair/upgrade/uninstall. | INS-01, INS-05, INS-06, INS-09, INS-10 |
| Debian / Ubuntu | DEB + package-manager install; menu entry exists; background client not enabled by package. | Qualified double-click → Install → Open on named supported releases. | INS-02, INS-05, INS-10 |
| Fedora / RPM | RPM + package manager; distro Qt/systemd dependencies; no GUI acceptance proof. | Equivalent qualified graphical install and first launch. | INS-02, INS-05, INS-10 |
| Generic Linux | No AppImage or generic portable path. | Supported self-contained AppImage with no required terminal chmod; bounded compatibility statement. | INS-04, INS-10 |
| Quick-install | No release-selecting/downloading/verifying installer script. | Optional authenticated, distro/architecture-aware command path. | INS-03 |
| First run | Existing connect/auth/profile/library flow after application is available. | Install then guided choose host or connect, first library and progress; no terminal for ordinary supported desktop. | INS-01–INS-03, INS-07, INS-08 |
| Server | Operator provisions PostgreSQL, API, storage, TLS and supervisor. | Guided self-host path with safe service/network/admin bootstrap choices. | INS-07, INS-08 |
| Lifecycle | Linux package manager plus narrow manifest installer; Windows manual file replacement; no product repair. | Consistent recoverable install, upgrade, repair and data-preserving uninstall. | INS-06, INS-09, INS-10 |

The easiest current path is a Linux DEB/RPM through a package manager on a
compatible x86_64 system. Current Windows is manual ZIP extraction. AppImage
and supported quick-install do not exist. Product-level repair does not exist.
Linux package lifecycle and the manifest tool preserve persistent state;
Windows has no coordinated uninstall/upgrade. Current first-run can connect to
an existing server and create a first library, but cannot host one. Native
Windows smoke and Linux package CI exist; this audit does not establish clean
GUI package install, Task Scheduler reboot, RPM graphical install or production
server deployment acceptance.

## Evidence summary

| Surface | Classification summary |
| --- | --- |
| Linux DEB/RPM packaging/build/metadata | `SOURCE_PRESENT`, `CI_VALIDATED`; live graphical install `BLOCKED_BY_ENVIRONMENT` / unverified in this audit. |
| Linux staged install/upgrade/uninstall safety | `SOURCE_PRESENT`, `CI_VALIDATED`, `FIXTURE_ONLY` for staged-root lifecycle tests; not host graphical acceptance. |
| Windows ZIP package and native smoke launch | `SOURCE_PRESENT`, `CI_VALIDATED`, `NATIVE_RUNTIME_VALIDATED` for the Windows CI runner's packaged QML smoke only. |
| Windows installed application, interactive startup registration/reboot, repair/uninstall | `NOT_IMPLEMENTED`; native product acceptance unavailable. |
| AppImage/Flatpak/Snap/quick-install | `NOT_IMPLEMENTED`. |
| Desktop connect/auth/local library first run | `SOURCE_PRESENT`; UI/controller focused CI/runtime evidence exists, but it is not the v0.2 install or host-server journey. |
| API/PostgreSQL/object-store code | `SOURCE_PRESENT`; CI has PostgreSQL 17 integration environment, but that is not a packaged server installer or production-host acceptance. Deployment documentation/manual steps are `DOCUMENTED_ONLY` until a deployment run is independently recorded. |
| Server installer, PostgreSQL provisioning, API/worker service installation | `NOT_IMPLEMENTED`. |
| Native clean-machine DEB/RPM GUI acceptance, production deployment | `BLOCKED_BY_ENVIRONMENT` / not evidenced by this audit. |

## Evidence map

Primary implementation/configuration: `deploy/packages/build.sh`,
`deploy/packages/build-windows.sh`, `deploy/packages/{debian,rpm}/`,
`deploy/install/{MANIFEST,install.sh,uninstall.sh,README.md}`,
`deploy/systemd-user/synveil-client.service`, `deploy/systemd/`,
`deploy/applications/synveil.desktop`, `crates/desktop/src/main.rs`,
`crates/desktop/qml/Main.qml`, `crates/client/src/{main.rs,launch.rs,control.rs,config.rs}`,
`crates/platform/src/{paths.rs,native_secrets.rs}`, and
`crates/api/src/bin/{synveil-api.rs,synveil-worker.rs}`.

CI evidence definitions: `.github/workflows/linux-packages.yml` and the
`desktop-windows-native` job in `.github/workflows/ci.yml`. User/operator
procedures and scope limits: `docs/en/RELEASE_PACKAGING.md`,
`docs/en/RELEASE_OPERATIONS.md`, `docs/en/DEPLOYMENT.md`,
`docs/en/DESKTOP_LAUNCH.md`, and `docs/en/TESTING.md`. v0.2 target is
`docs/v0.2/INSTALLATION_PRODUCT_CONTRACT.md`; future prompt ownership is
`docs/v0.2/ROADMAP.md`.
