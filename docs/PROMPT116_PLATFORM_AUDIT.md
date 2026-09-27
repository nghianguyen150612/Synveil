# Prompt116 cross-platform integration audit

Starting branch: `main`; clean worktree. Local HEAD, tracking `origin/main`,
and live remote were `d556ec0e0231f86c5b969d00ce1b3989e7794f52` before edits.
The preceding Prompt115 upgrade gate is preserved. This audit adds no platform,
protocol, feature, service architecture, updater, installer framework or migration.

## Findings and final behavior

Root overlap remains component-aware, including missing descendants resolved
through their existing canonical parent. Linux identity now preserves exact
native bytes instead of converting components through lossy UTF-8. Previously,
distinct non-UTF-8 components could become the same replacement-character key.
Windows keeps the existing conservative lowercase comparison policy; components
that cannot be represented losslessly fail closed. This is a conservative
product policy, not a claim to reproduce every filesystem's Unicode collation
or support Windows directories configured for case-sensitive operation.

The new same-root helper shares that policy with root overlap. Pending-library
retry identity, binding promotion, root revalidation and home/current-directory
guards now use it. Canonical/stored paths retain their case; display paths are
not rewritten. Windows drive/share roots and ambiguous relative roots remain
rejected by absolute-path and parent checks. UNC descendants use native path
components; a host-neutral component test does not establish live UNC acceptance.

Library configuration and onboarding now reject overlap with the resolved
application data, config, cache and runtime roots and the effective manifest
directory, including ancestors and descendants. Rejection occurs before a
pending binding or remote side effect, and client bootstrap applies the same
check before opening sync state. This protects application-owned state without
adding arbitrary restrictions on ordinary library directories.

Windows onboarding also no longer treats a directory's READONLY attribute as
write permission: [Microsoft documents that the attribute is not honored on
directories](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setfileattributesw).
Unix mode checks remain intact. Windows ACL-denied writes still fail through
the existing filesystem boundary; no permission emulation or preflight mutation
is added. A Windows-only regression sets the directory attribute and checks
admission plus an actual ordinary write, then restores the fixture attribute.

Windows [`GetUserNameW`](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getusernamew)
includes its terminating NUL in the returned length.
The former code included that NUL in Task Scheduler XML. Decoding now validates
the API length, removes only its terminator, rejects malformed UTF-16/control
characters and preserves the username. Task principal/executable values fail
closed on control characters; valid XML values are escaped. Execution still
uses structured process arguments, an absolute packaged sibling executable,
InteractiveToken/LeastPrivilege, one deterministic profile task name and no
stored password. The logon trigger now names that same user explicitly: an
omitted [LogonTrigger UserId](https://learn.microsoft.com/en-us/windows/win32/taskschd/logontrigger-userid)
activates on any user's logon. Explicit enable refreshes the same task's XML,
so an upgraded/relocated executable or trigger definition can be corrected
without duplicate entries; it reports a missing scheduler as unavailable.
The trigger keeps `Enabled` before `UserId`, as required by the
[base-trigger sequence](https://learn.microsoft.com/en-us/windows/win32/taskschd/taskschedulerschema-triggerbasetype-complextype)
and its logon extension; the existing XML regression checks that order.
Repeated registration remains one task; disable deletes only
the profile task and never owns uninstall or user data.

Windows supervised stop formerly used `schtasks /End`, which bypassed the
canonical client shutdown path. It now sends the existing profile-bound IPC
Shutdown command under a bounded timeout. Success means ShutdownAccepted;
the client process owns the subsequent host drain and resource release.
Unavailable/uncertain IPC is a typed failure, with no forced-termination fallback.
Linux supervisor stop continues to deliver its existing graceful signal.
GUI close/tray Quit does not issue either stop operation.

The Windows ZIP stages `platforms/qwindows.dll` beside its executables. Its
`qt.conf` previously selected `Plugins=plugins`, a different directory.
`Plugins=.` now matches both windeployqt's layout and the cross-package fallback,
using the executable-relative [Qt configuration path policy](https://doc.qt.io/qt-6/qt-conf.html).
QML remains under `qml`; native Qt/C++ import closure, architecture, licenses,
notices and manifest checks retain their existing fail-closed behavior. The
native Windows CI smoke now extracts the ZIP outside the source/build tree,
clears SDK plugin/QML overrides and SDK PATH entries and loads the actual packaged Windows
plugin with Qt Quick software rendering. The existing native sync-host check
now runs its unit tests, and a native named-pipe command/shutdown test runs in
the existing client unit step. No additional CI job or build strategy is added.

The DEB previously required only Qt shared-library packages. Debian separates
the [Controls library](https://packages.debian.org/bookworm/libqt6quickcontrols2-6)
from its [QML module](https://packages.debian.org/bookworm/qml6-module-qtquick-controls);
QPA plugins are only recommended by the GUI library. The package now explicitly
requires QPA and the QtQuick/Controls/Dialogs/Layouts/Window imports plus their
QtQml/Models/WorkerScript/Templates module dependencies. Existing Qt Linux CI
jobs install the same runtime modules, and the existing dependency regression
checks exact DEB package entries. RPM retains its distro Qt package grouping.
This verifies declared dependency policy, not clean Debian/Fedora installation.

## Preserved contracts and limitations

Linux UDS validation retains same-UID peer checks, private runtime/control
directories, socket mode 0600, bounded framing and fail-closed stale-endpoint
inspection. Windows named pipes retain a protected owner-only DACL, remote-client
rejection, bounded framing, first-instance exclusivity and canonical UUIDv7
profile validation. Shared dispatch defines Ping, status/library commands,
SyncNow, Shutdown, events and all existing auth/profile/settings/attention/recovery
extensions; neither transport introduces a TCP/HTTP control listener.

systemd remains a user unit; Task Scheduler remains an explicit per-user
InteractiveToken task, without normal-use elevation or a Windows service.
No-tray close remains usable: hiding requires both the preference and actual
tray availability; otherwise close exits only the desktop shell.

The existing notify adapters feed one bounded queue. Unsupported paths, native
watcher errors and queue overflow request reconciliation; durable observations,
root-availability fencing, rename/move handling and periodic safety polling remain
canonical. No watcher redesign or platform-dependent content transformation is
introduced. Replica operations retain symlink checks and Windows reparse-attribute
checks, plus no-follow file opens. Portable APIs do not eliminate every concurrent
check/open race; native junction/reparse, hostile races and live UNC shares are
not verified on this Linux host.

Existing name portability and conservative collision keys reject reserved
Windows devices, invalid characters, trailing dot/space and unsupported names.
Inbound/local collision gates produce existing typed LocalNameCollision or
unrepresentable-name failures before replacing content. Exact names are never
silently renamed. Some case-only renames/Unicode aliases can therefore be blocked
conservatively in v0.1. Non-UTF-8 root identity is lossless on Linux; managed
logical names and the persisted desktop manifest still require UTF-8.
Content is bytes; mixed LF/CRLF/CR, NUL and invalid text bytes now round-trip in
the existing verified staging regression. POSIX mode/owner/ACL emulation on
Windows is not part of the supported sync metadata model.

Linux packages retain distro Qt/runtime dependencies, shared manifest staging,
user startup integration, license/notice and Prompt108/115 state-preserving
install/upgrade/remove/purge rules. Windows retains the production portable ZIP
and external per-user application state; ordinary payload replacement/removal
does not own libraries, profiles or credentials. There is no Windows installer
or purge framework to certify. Real package-manager installation is distinct
from staged lifecycle tests. Reproducibility infrastructure is unchanged; no
Prompt113 two-root build is required or performed.

## Required test mapping

| Prompt test | Evidence |
| --- | --- |
| PLATFORM-UNIT-1 | Existing overlap test extended with same/nested roots, missing children and `missing-old` boundary. |
| PLATFORM-UNIT-2 | Host-neutral Windows component/case/UNC policy test; native deferred-root/drive test in Windows CI. |
| PLATFORM-UNIT-3 | New exact Linux case and distinct non-UTF-8 native-name identity regression. |
| PLATFORM-UNIT-4 | Existing home/current/root/relative guards; new platform-state overlap preservation and native Windows drive/relative-root and directory-attribute tests. |
| PLATFORM-UNIT-5 | Existing UDS owner/mode/stale/redirection tests; extended pipe UUID/prefix validation; native Windows pipe round-trip in CI. |
| PLATFORM-UNIT-6/7 | Existing fake-backend enable/disable test now repeats both actions and checks final state; deterministic native task name/fixed user unit policy. |
| PLATFORM-UNIT-8 | Existing manager-drop/desktop action tests and explicit lightweight real GUI/client fixture. |
| PLATFORM-UNIT-9 | Existing Prompt114 preference + actual-tray close disposition and QML regression. |
| PLATFORM-UNIT-10 | Existing inbound/local collision and unrepresentable-name preservation tests; expanded reserved/illegal-name table. |
| PLATFORM-UNIT-11 | Existing verified staging test now checks exact mixed text/binary content after publication. |
| PLATFORM-UNIT-12 | Existing Linux/Windows payload/runtime contracts; exact DEB QML/QPA dependency regression, qt.conf-to-platforms layout regression and packaged Windows CI smoke. |

The new lightweight Linux live target reuses the established Prompt99 fixture.
It launches the actual client and Qt desktop, proves GUI quit leaves the exact
client PID answering UDS Ping, then checks SIGINT, SIGTERM and IPC Shutdown for
successful process exit, endpoint removal and SQLite integrity across restarts.
It does not register a startup entry or touch a user's libraries/state.

## Validation record

All Cargo gates use `CARGO_BUILD_JOBS=1` on this Linux host.

| Gate | Result and evidence |
| --- | --- |
| `cargo fmt --all -- --check` | **Passed** after the final XML correction. |
| `cargo check --workspace --locked` | **Passed**. |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | **Passed**. |
| `cargo test --workspace --locked` | **Passed**: 979 passed, 0 failed, 444 ignored across 86 result summaries, including doctests. Ignored database/live suites are not acceptance passes. |
| `cargo deny check` | **Passed**: advisories, bans, licenses and sources. |
| Focused launch/background-manager units | **Passed**: 16, including final task XML, repeated enable/disable and canonical graceful stop. |
| Focused desktop control IPC integration | **Passed**: 5, including state-root rejection without mutation. |
| Focused Linux launch/install/native packaging, production packaging, Windows packaging policy and release artifact suites | **Passed**: 89 across 6 targets; subcheck skips are listed below. |
| Desktop units | **Passed**: 70; also included in the final workspace run. |
| Desktop debug/release builds, QML lint and both offscreen startup smokes | **Passed** with Qt 6.11.2, above the Qt 6.4 minimum. |
| Lightweight real Linux client/Qt/UDS graceful-shutdown fixture | **Passed**: exact client PID survives GUI quit; SIGINT, SIGTERM and IPC Shutdown exit successfully, remove endpoint and preserve SQLite integrity/version across restarts. |
| Existing real Linux production launch fixture | **Passed**: all LIVE-LAUNCH1..10 phases, including client reuse, systemd user-service crash recovery, unsafe/incompatible endpoint suppression and package-shaped outside-source launch. |
| Canonical Linux DEB/RPM construction and strict release-artifact validator | **Passed**: real release executables and source-bound artifact manifest; staged payload/source parity and intentional artifact mutation checks passed. DEB construction used the supported ar/tar fallback because `dpkg-deb` is unavailable. |
| Bash syntax, ShellCheck with shared sources, CI YAML parsing and `git diff --check` | **Passed** for the affected files. |

`scripts/test-desktop-ui.sh` itself was **interrupted** after its 70 tests passed
when its desktop-only Clippy step began a duplicate full Qt binding build.
Its remaining gates were completed explicitly: the stronger workspace strict
Clippy, current debug and canonical release builds, and the unchanged script's
QML lint/offscreen smoke tail. The whole wrapper is not reported as a passed
invocation. A queued workspace test build was also stopped before tests ran
to avoid inspecting the obsolete pre-Prompt116 package manifest; the final
complete workspace invocation passed against current packages.

The focused package tests reported three **skipped** subchecks: native DEB
metadata via `dpkg-deb` (unavailable), actual Windows ZIP manifest (no ZIP on
this host), and primary-versus-two-root artifact metadata (no second manifest;
the Prompt113 full two-root rebuild is excluded here). These skips do not
replace real Windows acceptance or a clean distro installation.

The live fixtures removed their temporary user unit and processes. Offscreen
startup left one disposable background client, as permitted by the ownership
contract; it was identified by its exact fixture runtime path and stopped with
SIGTERM. No user startup entry, library or application state was retained.

| Windows gate | Status |
| --- | --- |
| WINDOWS-1 source/static platform tests | **Passed** on Linux: host-neutral case/username/task XML/pipe-name policy and Windows ZIP/CI layout tests. |
| WINDOWS-2 Rust cross-build | **Blocked by environment**: only `x86_64-unknown-linux-gnu` rustlib is installed; rustup and a Windows linker/Qt SDK are absent. |
| WINDOWS-3 actual ZIP construction | **Blocked by environment**: no Windows Qt runtime or Windows client/desktop PE binaries. Packaging source policy and shell syntax passed. |
| WINDOWS-4 actual PE/runtime dependency inspection | **Blocked by environment**: `llvm-readobj` exists, but there are no real Windows binaries/ZIP to inspect. |
| WINDOWS-5 native named-pipe runtime | **Blocked by environment**: no native Windows host. The new native test is represented in the existing Windows CI unit step, not executed locally. |
| WINDOWS-6 live Task Scheduler integration | **Blocked by environment**: no native Windows host/Task Scheduler. Source XML, username, single-task policy and shared enable/disable semantics passed. |

Existing Windows CI is reviewed and strengthened; a configured future CI job
is not a passed native run. Windows compile, actual package construction and
native runtime acceptance are therefore not claimed.
Linux real tray/pixel-level acceptance and real package-manager installation
remain **skipped**; offscreen/fixture tests do not establish those claims.
The Linux artifacts are built with this host's Qt/glibc toolchain. Their
successful local loading does not certify ABI compatibility with another
distribution or its Qt version; native distro builds/install acceptance remain
separate release evidence.

Migration state remains server **36**, client **7**, `LOCAL_SCHEMA_VERSION` **7**.
No migration/catalog/schema version is changed.

## Explicit Prompt116 source manifest

Only these source paths belong to the single Prompt116 commit:

```text
.github/workflows/ci.yml
.github/workflows/linux-packages.yml
.github/workflows/postgres-17.yml
crates/api/tests/desktop_launch_live.rs
crates/client-sync/src/lib.rs
crates/client-sync/src/names.rs
crates/client-sync/src/replica.rs
crates/client/src/config.rs
crates/client/src/control.rs
crates/client/src/launch.rs
crates/client/src/process.rs
crates/client/tests/desktop_control_ipc.rs
crates/metadata/tests/linux_native_packaging_units.rs
crates/metadata/tests/windows_desktop_packaging_units.rs
deploy/packages/build.sh
deploy/packages/build-windows.sh
docs/PROMPT116_PLATFORM_AUDIT.md
docs/en/PLATFORM.md
docs/vi/PLATFORM.md
```

Packages, target outputs, native/cross tool caches, fixtures, logs and secrets
are excluded. The one commit and successful push/remote equality are verified
after all mandatory gates pass and reported outside this self-referential file.
