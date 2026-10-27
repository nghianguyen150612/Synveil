# Synveil v0.3 — PROMPT007 Manifest

**Status: SOURCE IMPLEMENTED; NATIVE QUALIFICATION BLOCKED BY LOCAL ENVIRONMENT AND UPSTREAM P006 FORMATTING.** This manifest records the provisional P007 presentation-only scope. It does not claim a working installer or P006 completion.

## Repository baseline

- Repository: `https://github.com/nghianguyen150612/Synveil`
- Base branch: `develop/v0.3`
- Required starting SHA: `953f2e84cef8a1625fef6023715696c590ee2997`
- Working branch: `feat/v03-p007-installer-wizard-foundation`
- Target: one PR to `develop/v0.3`; do not merge automatically and do not start P008.
- The local checkout started at `604499fce0dfd134d82da3d5bb5d4a055c5eea8d` (`main`); it remains unchanged. The P007 branch was created from the fetched exact `develop/v0.3` SHA.

## Preflight record

- P001–P004 are merged through #156, #160, #161, #162; P001A security/CI changes are merged through #157.
- P005A #164 and P005 #163 are merged.
- P006 #165 is merged at the required base SHA.
- Focused P006 workflow run [#40291443669](https://github.com/nghianguyen150612/Synveil/actions/runs/40291443669) failed at **Check formatting** on PR head `9ea0a842b4dc2756c4b29dd2373385bf9556ef04`. All P006 HTTP, ObjectStore, storage, PostgreSQL 17, retained P005 PostgreSQL, and API Clippy steps were skipped. No new security/correctness defect was identified by that run, but it did not qualify the P006 behavior. This P007 change does not depend on those unexecuted assertions.
- `Synveil-v0.3-Context-Roadmap-P001-P060.md` was not found in the repository or workspace. The presentation-only P007 scope remains provisional; P002 architecture notes are context, not an approved roadmap replacement.
- P004-D02, D03, and D11 remain `RECOMMENDED_PENDING_APPROVAL`.

## Implementation

- Added `crates/desktop/src/installer_wizard.rs` with typed process states, closed role/mode choices, generation-fenced async completion, safe retry, invalidation, bounded error state, and no effect/plan/journal API.
- Added `crates/desktop/qml/InstallerWizard.qml`, registered it in the existing `com.synveil.desktop` QML module, and added a preview-only Welcome entry without replacing Host, Connect, authentication, enrollment, library setup, or startup flow.
- Added 16 focused Rust state/presentation tests. Because the desktop package build is blocked before Rust compilation by the missing system DBus development package, these tests were run from a temporary harness that compiled the exact checked-in state-machine source against `synveil-install-engine`.
- Added this manifest and `docs/v0.3/installer/P007_INSTALLER_WIZARD_FOUNDATION.md`.

## Validation record

| Check | Result | Evidence / limit |
|---|---|---|
| `cargo fmt --all -- --check` | **FAIL — upstream P006 formatting** | The only remaining diffs are in `crates/api/src/tests.rs`, added by merged P006 #165. `cargo fmt --package synveil-desktop` passes. No unrelated API test formatting was bundled into P007. |
| `cargo test -p synveil-install-engine --locked` | **PASS** | Completed with exit code 0. |
| Focused wizard Rust tests | **PASS — 16 tests** | Temporary harness compiled the exact `installer_wizard.rs`; covers state, invalidation, blocking-finding redaction, unsupported scopes/platform, Tailscale, duplicate clicks, cancellation, stale results, re-entry, and startup compatibility. |
| `cargo test -p synveil-desktop --locked` | **BLOCKED** | Build stopped in `libdbus-sys`: `pkg-config` could not find `dbus-1` / `dbus-1.pc`; `libdbus-1-dev` is not installed. Desktop Rust bridge and QML resources were not compiled by this command. |
| `cargo clippy -p synveil-desktop --all-targets --locked -- -D warnings` | **BLOCKED** | Stopped in `libdbus-sys` because `pkg-config` could not find `dbus-1` / `dbus-1.pc`; desktop Rust and Qt compilation were not reached. |
| `./scripts/test-desktop-ui.sh` | **BLOCKED** | After staging the intended files, it stopped at Qt selection: the available `qmake` wrapper points to missing `/usr/lib/qt5/bin/qmake`; Qt 6 `qmake6` and `/usr/lib/qt6/bin` are absent. Native QML lint/smoke did not run. |
| `./scripts/validate-docs.sh` | **PASS** | All 24 documentation units passed, including existing product-contract validators. |
| `git diff --check` | **PASS** | No whitespace errors at the recorded check. |
| Hosted Linux/Windows native desktop CI | **NOT RUN** | Must be recorded from the P007 PR checks; do not claim PASS in advance. |

## Scope and safety decisions

- Host choice is available only where the existing managed Host setup recognizes Linux x86-64 Ubuntu 24.04 or Fedora 42. Client choice is available when the existing desktop profile loads. Existing-install configuration is disabled on first-run Welcome because no configured installation is evidenced there.
- Local-only, private-LAN, and advanced/manual choices are preferences only. Tailscale is explicitly unavailable and cannot enroll or connect.
- Review reports the installer preflight as not run, artifact as unavailable, plan preview as unavailable, target scope as undetermined, and privilege as not assessed. No successful installation or trust result is manufactured.
- No selection dispatches an installation or existing Host/client effect. No plan, fingerprint, journal, endpoint, credential, command, listener setting, or native authorization request is produced.
- Existing Linux DEB/RPM, current-user AppImage integration, Windows per-user Setup, and portable ZIP scopes remain owned by their current platform/package code. The wizard does not infer which channel produced the running desktop.

## P008 prerequisites and maintainer decisions

P008 must first add an approved read-only preflight/plan producer through the existing engine and platform owners, bind real verified artifact and target-scope evidence, map actual findings to a redacted DTO, and revalidate the plan before any future effect execution. It must preserve native package manager, AppImage, Windows per-user/portable, Host, and client ownership. Native authorization, journal recovery, artifact signing/downgrade policy, and real Linux/Windows acceptance remain separate gates.

P004-D02 (installer privilege), P004-D03 (Tailscale), and P004-D11 (presentation/effect ownership) remain pending approval. P006 remains unqualified until formatting is fixed and the skipped P006 checks pass on a reviewed exact head. The original P001–P060 roadmap is still unavailable. No P008 work is included.
