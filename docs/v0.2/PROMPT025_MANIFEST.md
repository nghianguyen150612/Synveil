# Prompt025 implementation manifest

Status: **source/static implemented; hosted standard-user evidence pending**

| Field | Evidence |
|---|---|
| Starting main SHA | `9a97ad80e1cb2c7896ce6ce09dacb15053028bc4` |
| Branch | `codex/p025-windows-per-user-installation` |
| P024 inherited hosted status | Final native evidence was pending at handoff; no accessible current repository remote/workflow result exists locally |
| P024 failures fixed | None claimed; P024 gates are retained in the focused workflow |
| Artifact SHA-256 | **PENDING native build** |
| Windows runner version/build | **PENDING hosted run** |
| Standard-user method | Random disposable local non-Administrator; direct credentialed process with loaded profile; bounded five-minute child; guaranteed account/profile cleanup |
| Non-admin membership | Source assertion implemented; **PENDING hosted result** |
| Installer token/elevation/integrity | Non-elevated and non-high/system token assertions implemented; compiled `asInvoker` manifest gate implemented; **PENDING hosted result** |
| Install root | `%LOCALAPPDATA%\Programs\Synveil`; source/static verified; **PENDING hosted result** |
| HKCU / HKLM | HKCU registration and both HKLM registry-view absence checks implemented; **PENDING hosted result** |
| Shortcuts | Current-user Start Menu and both desktop option states; common/Public absence checks implemented; **PENDING hosted result** |
| ACL | Structured SID-based broad-write rejection implemented; **PENDING hosted result** |
| Desktop / client | P024 manifest verification and unrelated-CWD clean-environment probes run inside standard-user process; **PENDING hosted result** |
| Cross-user isolation | Profile/HKCU/common-surface and ACL model asserted; full two-user live install remains **PENDING** |
| Uninstall / reinstall | Registered uninstaller, removal, second install, option cleanup implemented; **PENDING hosted result** |
| State sentinel | External synthetic sentinel preservation implemented; **PENDING hosted result** |
| Cleanup | Account/profile deletion in controller `finally`; **PENDING hosted result** |
| Workflow ID/result | **PENDING** |
| Known blocker | Local host is Linux; native Windows account/token/Setup execution is unavailable |
| Deferred | P026 startup persistence; P027 complete lifecycle; P028 final clean/native acceptance and named-pipe journey |

## Bounded result artifact

Native CI emits `windows-per-user-evidence.json` with source/artifact identity,
host version, architecture, account category, token facts, product-owned
registry/filesystem/ACL checks, installed probes, uninstall/reinstall/state
results, and cleanup. It intentionally excludes the synthetic password, SID,
username, registry snapshots, credentials, and user data.
