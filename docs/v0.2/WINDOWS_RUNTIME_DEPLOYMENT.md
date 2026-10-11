# Windows runtime deployment

Status: **SOURCE IMPLEMENTED / STATIC VERIFIED; HOSTED NATIVE VERIFIED pending final-head run**

## Authority and staging

`deploy/packages/build-windows.sh` is the single runtime producer. On native Windows it invokes the pinned CI Qt 6.8.3 `windeployqt` against the real release `synveil-desktop.exe` and the desktop QML source directory. `windeployqt` selects the Qt DLL, plugin, and QML closure. The builder adds only imported MSVC runtime DLLs from the authenticated active `VCToolsRedistDir`, then audits the full dependency closure. The cross-build allowlist remains a development fallback, not native evidence.

Construction occurs in a fresh temporary tree. A requested export directory is removed and replaced only after validation, so stale bytes cannot survive. The portable ZIP and `SynveilSetup.exe` consume that same tree and `SYNVEIL-MANIFEST.txt`; Setup is never assembled by extracting the ZIP or from a second Qt list.

## Closed package inventory

The package root contains sibling `synveil-desktop.exe` and `synveil-client.exe`, `qt.conf`, `LICENSE`, `NOTICE`, `platforms/qwindows.dll`, and only the Qt/QML/plugin/compiler runtime selected for the application. The manifest binds every runtime file by relative destination, size, and SHA-256. Generation and consumption reject traversal, rooted paths, missing files, digest or size changes, duplicates/case collisions, links, unmanifested files, and unexpected executables. The installed manifest is copied with the common closure; Inno's uninstaller files are separately installer-owned.

The audit rejects headers, import/static libraries, Unix libraries, `include`, `mkspecs`, CMake/pkg-config data, examples, tests, unrelated SDK tools, server executables, and private build paths. It does not bundle the API, worker, database, proxy, language toolchain, Qt SDK, or a service.

## Qt and QML policy

`qt.conf` fixes `Prefix=.` and package-relative `Plugins=.` plus `QmlImports=qml`/legacy `Qml2Imports=qml`. No global Qt variable or PATH change is installed. The required `platforms/qwindows.dll` is mandatory. Native `windeployqt --qmldir` resolves the transitive imports used by embedded `Main.qml`, currently QtQuick, Controls, Dialogs, Layouts, Window, QtQml and their runtime dependencies. This preserves arbitrary-working-directory launch and prevents dependence on a system Qt installation.

Do not enable `windeployqt`'s compiler-runtime deployment for the application package: the hosted Qt toolchain staged `vc_redist.x64.exe`, an installer that violates the closed runtime payload's executable allowlist. Instead, the package scans every staged PE import and copies only imported MSVC CRT DLLs from the authenticated x64 directory under the active `VCToolsRedistDir`. The Windows workflow authenticates the linker and the required runtime DLL identities before packaging; the package manifest hashes every copied runtime file and records the selected CRT directory. No online VC bootstrapper runs at install time. Build evidence records Qt version/architecture, windeployqt identity, MSVC tools version, Rust/Cargo identity, source revision, and the manifest digest available from CI artifact hashing. The revised producer still needs exact-head hosted validation.

## Native binary and dependency audit

The builder requires the desktop and client, and audits every shipped EXE/DLL recursively. Product executables must be AMD64 PE images. Each imported DLL must resolve inside the package or match the reviewed Windows/API-set system-DLL policy; “missing means system” is not accepted. Core Windows DLLs are not copied into the package.

The production launch manager canonicalizes the current desktop executable and resolves only a regular `synveil-client.exe` sibling. It performs no PATH, current-directory, registry, target-tree, or server-directed executable lookup. Packaging does not change the owner-restricted, profile-scoped named-pipe contract.

## Installer and portable parity

The installer builder reads the staged manifest, rehashes every entry, rejects extra files, and emits one explicit Inno `[Files]` line per entry plus the shared manifest. There is no payload wildcard. Consequently the portable archive and installed application share one byte identity; only Inno-owned integration/uninstaller metadata exists outside it. The installer remains per-user at `{localappdata}\Programs\Synveil`, without elevation, service creation, startup registration, install-time network access, or broad ACL changes.

## Installed and clean-environment verification

The focused Windows workflow silently installs Setup, validates every installed payload byte against the installed manifest, verifies both product EXEs are AMD64, and launches the installed desktop from an unrelated directory with the bounded QML smoke switch. It removes `QT_ROOT_DIR`, `QT_PLUGIN_PATH`, `QML2_IMPORT_PATH`, and `QML_IMPORT_PATH`, restricts child PATH to Windows system locations, enables bounded Qt plugin diagnostics, and rejects checkout/hosted Qt paths. It also runs the installed client from that directory and requires its safe, bounded no-profile configuration exit (78).

Disposable negative copies prove rejection after removing `qwindows.dll`, corrupting a DLL, injecting an unexpected DLL, or adding a developer `.lib`. Static checks cover replace-not-overlay staging. The workflow then uninstalls and verifies package removal while preserving an external synthetic state sentinel. P028 retains full clean-machine/named-pipe acceptance authority.

## State and security boundary

Package installation never creates or overwrites application configuration, credentials, client sync state, user libraries, server configuration, databases, or object data. No credential is packaged; Credential Manager remains authoritative. Executable-relative Windows loading and `qt.conf` are used without global PATH mutation or `SetDllDirectory`. Normal LocalAppData/Inno current-user ACL semantics are retained.

## Evidence status and deferred scope

* **SOURCE IMPLEMENTED:** common closure, closed inventory, PE/import/SDK audits, manifest-derived installer payload, installed verifier, isolated desktop/client smokes, negative fixtures, and provenance fields.
* **STATIC VERIFIED:** the network-free validator and local syntax/unit checks listed in `PROMPT024_MANIFEST.md`.
* **HOSTED NATIVE VERIFIED:** **PENDING** final-head Windows workflow. No Linux result is represented as native evidence.
* **Known limitation:** a GitHub-hosted runner still contains developer tools; sanitization substantially reduces contamination but is not a pristine VM. P028 owns that full acceptance claim.
* **Deferred:** P025 per-user qualification, P026 Task Scheduler preference persistence, P027 complete repair/upgrade/uninstall lifecycle, and P028 end-to-end installed-product acceptance.
