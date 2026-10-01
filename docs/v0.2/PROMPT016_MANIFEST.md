# Prompt016 manifest

## Provenance and acceptance mapping

- Requested hosted starting head: `d6cf0d87e3cba15dc7e3f0e5d01f26b1da2cce40`.
- Supplied local P015 checkpoint: `eef17cc02eb58530430b36a365929effe3955d1b`.
- Authority: `codex/add-appimage-build-foundation`, PR #17.
- Hosted completion, hashes, and merge SHA remain final-head evidence; this
  source manifest does not pre-claim them.

| Acceptance | Evidence |
| --- | --- |
| APPIMAGE-RUNTIME-1–6 | Rust/real-artifact tests cover portable zero mutation, singleton launcher/icon, Unicode/space quoting, stable path, and idempotence. |
| APPIMAGE-RUNTIME-7–11 | Typed stale/partial states, relocation, repair, and owned-only removal are exercised. |
| APPIMAGE-RUNTIME-12–14 | Preservation sentinels and fixed XDG paths prove no data, root, or package-manager effects. |
| APPIMAGE-RUNTIME-15–19 | Fixed client dispatch, stable user unit, disabled default, user-only systemctl, and typed unsupported result. |
| APPIMAGE-RUNTIME-20–24 | Existing IPC/SecretStore remain unchanged; replacement/recovery preserve durable data. |
| APPIMAGE-RUNTIME-25–27 | Hosted QML smoke, A/B image comparison, and shared DEB/RPM workflow are required final gates. |

Tests use disposable HOME and all relevant XDG overrides. The hosted lifecycle
operates on a disposable copy of the real AppImage. P017–P020 remain pending.
