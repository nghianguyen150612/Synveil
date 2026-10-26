# Synveil v0.3 — PROMPT008 Manifest

**Status: source implementation complete; local native desktop qualification is blocked and hosted P008 checks are pending.** No install effect is reachable and plan preview remains unavailable. This is a provisional stacked change, not an integrated installer.

## Repository and gate record

- Repository: `https://github.com/nghianguyen150612/Synveil`
- P008 branch: `feat/v03-p008-installer-preflight-preview`
- Starting P007 head: `59b412f628c63423eb894d9b505d4d5cdda2d8b5`
- P008 target while P007 is open: `feat/v03-p007-installer-wizard-foundation`
- P007 PR #166 is open/draft. Do not integrate P008 into `develop/v0.3` until P007 is merged.
- P006 PR #165 is merged at `953f2e84cef8a1625fef6023715696c590ee2997`.
- P006 focused workflow #40291443669 failed at workspace formatting on its exact PR head; HTTP/ObjectStore/storage/PostgreSQL 17/retained P005 PostgreSQL/API Clippy gates were skipped. The formatting-only repair is isolated in open draft PR #167.
- P006 follow-up qualification run #40320179191 passed on the P006 formatting-repair head. P008 remains based on P007, not on #167.
- The authoritative `Synveil-v0.3-Context-Roadmap-P001-P060.md` was not found in the checked-in repository or GitHub code search. P008 scope is provisional.

### P007 checks inspected

- P007 head is `59b412f628c63423eb894d9b505d4d5cdda2d8b5`; its stacked PR base is the merged P006 SHA.
- Rust CI run #40319985311: native Windows Qt 6 shell build, generated QML verification, and packaged QML smoke passed. The Linux Qt 6 desktop job stopped in its format gate before its cargo test/build; workspace formatting also failed on inherited P006 `crates/api/src/tests.rs` changes.
- Linux AppImage run #40319985219 passed the native AppImage build, package inspection, host-decontaminated QML smoke, and current-user integration lifecycle on the P007 PR merge commit. This compiled the P007 bridge after the `E0382` Pin reborrow repair.
- Windows native acceptance run #40319985300 passed standard-user product acceptance, then failed at “Record required unavailable surfaces honestly.” P007 remains draft/unmerged.
- Linux clean-machine acceptance run #40319985223 passed acceptance contracts and built DEB/RPM/AppImage artifacts; its VM acceptance matrix was skipped.

## P008 implementation record

- Added a read-only `installer_preflight` module with typed requests, platform/channel evidence, findings, plan-preview availability, and a binding that includes the exact platform/channel snapshot, target scope, and optional artifact identity.
- Added bounded Linux OS/package-manager/Host dependency/AppImage probes and Windows per-user Setup/portable marker probes. Probe results are reduced before reaching Qt/QML.
- Added a redacted Private LAN capability wrapper in `synveil-server-host`; it reuses the existing read-only `ip` inspection and exports only a candidate count. The inspector does not query interfaces for Client mode.
- Kept all plan outcomes unavailable because no authoritative channel-specific plan builder or trusted artifact acquisition path exists. No artifact status, plan, effect, privilege requirement, or installation success is manufactured.
- Extended the P007 wizard binding, channel selection, platform summary, review findings, and invalidation hooks. No Install, Apply, Repair, or Configure action was added.
- Added focused tests for channel-owner parsing, artifact-input rejection, unavailable plan, scope refusal, binding consistency, generation/stale completion, role/connection/channel/artifact invalidation, cancellation, duplicate clicks, Tailscale, redaction, and existing onboarding routes.
- Added [`P008_PREFLIGHT_PLAN_PREVIEW.md`](../installer/P008_PREFLIGHT_PLAN_PREVIEW.md) and this manifest.

## Security acceptance matrix

| Requirement | Source/test evidence | Current result |
|---|---|---|
| No installation effect | Inspector and wizard bridge contain no `InstallerEngine::execute`; source-boundary test checks this. | Focused module harness passed the source-boundary test; no effect API is reachable. |
| No journal record | Inspector and wizard bridge do not access `InstallationJournal`; source-boundary test checks this. | Focused module harness passed the source-boundary test; no journal API is reachable. |
| No listener change | Preflight only calls the existing read-only Host candidate probe for Host + Private LAN; it never binds a listener. | Implemented; native behavior check pending. |
| No privileged helper contact | `pkexec` is only checked as a fixed path; preflight has no helper invocation. | Implemented; source audit. |
| No UAC/Polkit prompt | Windows uses bounded `reg.exe query`; Linux runs no authorization helper. | Implemented; native behavior check pending. |
| User input cannot manufacture verified artifact status | `PreflightRequest` contains no artifact evidence; binding artifact identity remains `None`. | Focused module harness passed. |
| Missing/unverified artifact blocks authoritative preview | Preview is unavailable with the exact verified-artifact-required message. | Focused module harness passed. |
| Unsupported scope fails closed | Scope is derived from channel evidence; system scope is never inferred from UID or admin membership. | Focused module harness passed. |
| Stale completion ignored | Generation/token check discards old results. | Focused module harness passed. |
| Role change invalidates preview | Role change increments generation and clears the outcome. | Focused module harness passed. |
| Connection change invalidates preview | Connection change increments generation and clears the outcome. | Focused module harness passed. |
| Artifact/channel changes invalidate preview | Channel selection and artifact-change hook invalidate the outcome. | Focused module harness passed. |
| No secret/raw path in QML state | Summary DTOs contain fixed labels and allowlisted OS fields; redaction test checks QML-facing summaries. | Focused module harness passed. |
| Tailscale cannot enroll | Tailscale is unavailable in Rust and QML; no enrollment API is referenced by the inspector. | Focused module harness passed. |
| No real installation success reported | No execution action exists; plan availability stays `Unavailable`; QML preserves preview-only copy. | Focused module harness passed. |
| Existing onboarding remains reachable | Main/Welcome Host, Connect, and controller routes remain; a Rust source-contract test checks routes and preview-only copy. | Focused module harness passed; native QML smoke is still pending. |

## Local validation record

| Check | Result | Evidence / limit |
|---|---|---|
| `cargo fmt --package synveil-desktop --package synveil-server-host -- --check` | **PASS** | Updated Rust owners are formatted, including the P007 bridge reborrow call sites. |
| `cargo fmt --all -- --check` | **FAIL — inherited P006 formatting drift** | Remaining diffs are in `crates/api/src/tests.rs`; P008 does not edit or import the #167 formatting patch. |
| `cargo test -p synveil-install-engine --locked` | **PASS** | 214 install-engine, AppImage, and package-integration tests passed. |
| `cargo test -p synveil-server-host --locked` | **PASS** | 2 server-host tests passed. |
| Focused P008 wizard/preflight tests | **PASS — standalone module harness** | 29 tests passed against the exact `installer_wizard.rs` and `installer_preflight.rs` sources. This harness does not compile the desktop bridge or qualify a desktop build. |
| Focused P008 module Clippy | **PASS — standalone module harness** | `cargo clippy --all-targets -- -D warnings` passed for the two included source modules and their tests. This does not lint the desktop bridge or qualify a desktop build. |
| `cargo test -p synveil-desktop --locked` | **BLOCKED BY LOCAL BUILD DEPENDENCY** | `pkg-config` could not find `dbus-1`; no P008 desktop test executed. |
| `cargo clippy -p synveil-desktop --all-targets --locked -- -D warnings` | **BLOCKED BY LOCAL BUILD DEPENDENCY** | Clippy stopped in `libdbus-sys` because `dbus-1.pc`/`libdbus-1-dev` is missing; the desktop crate was not linted. |
| `./scripts/test-desktop-ui.sh` | **BLOCKED BY LOCAL Qt TOOLING** | It stopped before build/test at the `qmake` shim, which points to missing `/usr/lib/qt5/bin/qmake`; Qt 6 `qmake6` is absent. No native QML smoke ran. |
| `./scripts/validate-docs.sh` | **PASS** | All 24 documentation validation units passed, including the existing installer and release-contract checks. |
| `git diff --check` | **PASS** | No whitespace errors in the working tree at the last check. |
| P008 hosted Linux/Windows Qt and QML checks | **PENDING** | Run on the exact P008 PR head. Pending jobs are not a pass. |

The local platform is Debian 13 x86-64. DEB package-manager tools and the `systemctl`/`ip` paths are present; RPM tooling, `/usr/bin/pkexec`, Qt 6 `qmake6`, and DBus development metadata are absent. The managed Host matrix does not include Debian 13. No active AppImage marker was verified. The wizard's Client availability still depends on the existing desktop profile at runtime.

## P004 gates and P009 prerequisites

- P004-D11, desktop/platform presentation versus install-engine-owned plan/effect/recovery boundaries, remains pending maintainer approval.
- P004-D14, publisher trust, artifact signing, revocation, downgrade policy, provenance, and reproducible release qualification, remains pending maintainer approval.
- P008 approves neither decision and adds no Tailscale enrollment, pairing, DeviceVeil, RBAC, privilege escalation, or execution action.
- Later installation work needs approved artifact acquisition/verification, authoritative platform/channel plan builders, `InstallerEngine::validate` integration, fresh plan reinspection, approved native authorization, journal recovery, and Linux/Windows acceptance.
- P007 must be integrated before P008 can be integrated. No P009 work is included.
