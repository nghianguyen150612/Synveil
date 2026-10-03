# Prompt028 manifest

Status: **in progress / native evidence pending**

| Field | Recorded value |
|---|---|
| Starting SHA | `ea1223d2457d3914903b80dca73b4bae692311fd` |
| Branch | `codex/p028-windows-native-acceptance` |
| Final tested head | unavailable; PR head not yet hosted |
| Candidate SHA-256 / size | unavailable; native producer has not passed |
| Release-manifest digest | unavailable |
| Target | Windows 11, AMD64 |
| Exact Windows version/build | unavailable |
| P028 workflow/job IDs | unavailable |
| Inherited P027 run | `37105707568`, job `111153676555`: **FAIL** |

The inherited failure occurred in **Build runtime payload** (exit 101); all
installer, standard-user, and lifecycle steps were skipped. Artifact upload
also failed because no candidate/evidence existed. Classification:
`INHERITED_WINDOWS_RUNTIME_REGRESSION`; exact compiler diagnostics are not
available through unauthenticated hosted-log access and must be inspected in an
authenticated rerun. This is not native acceptance evidence.

| Gate | Status |
|---|---|
| P021 technology / P022 Setup / P023 UI sources | `STATIC_VERIFIED` |
| P024 installed runtime | `BLOCKED` — inherited native build failed |
| P025 standard-user | `BLOCKED` — inherited downstream step skipped |
| P026 startup / scheduler create / run | `BLOCKED` — no P028 native result |
| P027 lifecycle | `BLOCKED` — inherited downstream step skipped |
| INSTALL-JOURNEY-1 | `BLOCKED` — graphical and IPC evidence absent |
| INSTALL-JOURNEY-6 | `BLOCKED` — native repair result absent |
| INSTALL-JOURNEY-7 | `BLOCKED` — native uninstall result absent |
| Named pipe / no public control listener | `BLOCKED` |
| Real interactive four-screen UI | `BLOCKED_BY_ENVIRONMENT` |
| Real LogonTrigger | `BLOCKED_BY_ENVIRONMENT` |
| Repair / idempotence | `BLOCKED` |
| Upgrade fixture / downgrade rejection | `BLOCKED` |
| Uninstall / state and adjacent-file preservation | `BLOCKED` |
| Reinstall | `BLOCKED` |
| Setup reproducibility | `BLOCKED` — P028 producer has not run |
| Cleanup | `BLOCKED` — no P028 native consumer run |

Evidence vocabulary retained by this checkpoint is `SOURCE_PRESENT`,
`STATIC_VERIFIED`, `CI_NATIVE_SCOPED`, `NATIVE_CLEAN_MACHINE`, `BLOCKED`,
`FAIL`, and `PASS`. No unavailable identifier or result is inferred.
