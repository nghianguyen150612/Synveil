# Prompt015 manifest

## Provenance and change boundary

- Authoritative starting SHA: `0356023dc52f9e44e22e5ade1e0f5c3d5d43a2e6`.
- Branch: `codex/p015-appimage-build-foundation`.
- Commit subject: `feat: add AppImage build foundation`.
- No PostgreSQL/SQLite migration, protocol, sync model, or product-version
  change is introduced.

## APPIMAGE evidence map

| Requirement | Evidence |
| --- | --- |
| APPIMAGE-1–3 | Builder and validator derive `Synveil-<version>-x86_64.AppImage` from workspace version and reject non-x86_64 before output. |
| APPIMAGE-4–8 | Real AppDir inspection requires desktop, client, canonical metadata/icon, and direct packaged-desktop AppRun target. |
| APPIMAGE-9–12 | AppRun resets search paths; inspection requires Qt libraries, QML module metadata, and QPA plugin. |
| APPIMAGE-13–17 | Source contract and focused documentation preserve SecretStore, user IPC/XDG state, and prohibit privileged or automatic integration. |
| APPIMAGE-18 | Hosted validator runs the exact AppImage `--qml-smoke-test` with the packaged `xcb` plugin under isolated Xvfb and a decontaminated environment. |
| APPIMAGE-19 | Builder independently stages/constructs twice and requires `cmp` byte identity. |
| APPIMAGE-20 | Manifest generator measures the published file; validator recomputes its size and SHA-256. |
| APPIMAGE-21 | Builder/validator fail on missing binaries, metadata, AppRun, Qt/QML/plugin closure, tool failure, or absent output; temporary staging prevents partial publication. |
| APPIMAGE-22 | Architecture gate precedes binary building and publication. |

Qt 6.7.3 is pinned on the Ubuntu 22.04/glibc 2.35 host. The shared Linux release
link policy removes the sole differing GNU build-id note at link time rather
than rewriting output. Final desktop/AppImage hashes, ABI symbol floors, and
pass/fail results still come from the final PR-head hosted run. Static
validation or an absent artifact is `SKIPPED`, never runtime PASS. Existing
DEB/RPM remains an independent gate.
