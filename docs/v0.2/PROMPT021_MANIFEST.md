# Prompt021 Manifest — Windows Installer Technology Decision

## Outcome

Prompt021 locks **Inno Setup 6.7.3** and an x86_64 **PER_USER**
`SynveilSetup.exe` design for P022–P028. The detailed decision and ADR-058
freeze privilege, UI, ownership, Task Scheduler, lifecycle, unattended CI,
artifact and signing boundaries. P022 implements the first installer skeleton;
no installer binary or native installer acceptance is claimed here.

## Baseline and inherited P020 audit

| Item | Value |
|---|---|
| requested/current `origin/main` | `508153ac382ad907fd402717c8c4143536c1d5ae` |
| audit date | 2026-10-03 UTC |
| branch | `codex/p021-windows-installer-decision` |

Current hosted runs were inspected through the live GitHub Actions API rather
than copied from a PR summary. Main-head AppImage and Rust CI completed as
failures during the audit; native-package and PostgreSQL runs were still in
progress. Public unauthenticated API access exposed job/step results but
returned 403 for log archives, so causal detail below is limited to
source/history or prior recorded evidence and is not guessed. No failure is
represented as PASS.

| Current/last observable area | Observation | Classification | P021 action |
|---|---|---|---|
| AppImage build/runtime | `508153a` failed in the build/reproduce step (exit 1). HEAD changes AppRun invocation validation and packaging arguments, but the public API does not expose the causal log. | `P020_OWNED_REGRESSION` remains open; exact cause is `ENVIRONMENT_LIMITATION` without the hosted log | Rechecked source; no blind duplicate fix and no PASS claim. |
| desktop reproducibility | native-package reproducibility job was still running. HEAD contains the QML generator diagnostics/fixes inherited from P020. | `ENVIRONMENT_LIMITATION` until hosted completion; historical `P020_OWNED_REGRESSION` | Do not weaken/reimplement; retain hosted gate. |
| Windows/macOS Rust portability | checks passed on both platforms, but workspace tests failed on Windows, macOS and Ubuntu; Windows Task Scheduler policy also failed. | `P020_OWNED_REGRESSION` remains open; exact test failures unavailable from public API | Local workspace validation; no speculative platform edit without causal output. |
| native desktop Qt CI | Linux build/test and Windows launch/Task Scheduler policy jobs failed. | `P020_OWNED_REGRESSION`; local reproduction is `ENVIRONMENT_LIMITATION` without native Qt/Windows | Preserve native gates; no false PASS. |
| Linux native package build/reproducibility | main-head build and reproducibility jobs were still running. Local contract reproduction found the new diagnostic's blanket `done || true` violated the failure-honest workflow policy. | `P020_OWNED_REGRESSION` | Replaced the blanket mask with explicit grep no-match handling while preserving real read/grep errors; focused contract and shell syntax pass. |
| Linux clean-machine acceptance | workflow is not automatically triggered on push and no qualifying result exists; manifest says native evidence pending. | `ENVIRONMENT_LIMITATION` | Keep Phase-C checkpoint withheld. |
| PostgreSQL 17 | current run pending; preceding failure is the recorded `daticulocale` server-schema issue. | `PRE_EXISTING_UNRELATED_FAILURE` | No source change. |

The live failures are retained as owned regressions, not relabeled as toolchain
failures merely because their private logs are unavailable. The one locally
causal regression was fixed; no other edit can be justified from observable
output. P021 leaves the other hosted gates open. The forbidden Linux readiness
markers remain unissued.

## Decision evidence

The audit verified the current runtime ZIP and manifest producer, native
`windeployqt` closure, separate executables, Windows Credential Manager backend,
owner-only named pipes and explicit current-user Task Scheduler manager. The
decision compares Inno Setup, WiX MSI, Burn, NSIS, MSIX and a custom Rust
bootstrapper over the same 24 criteria, records factual tooling behavior
separately from Synveil choices, and provides exact P022 paths/entrypoint,
payload input, output, metadata injection, smoke and CI handoff.

## Scope truth

This prompt does not build, sign or test `SynveilSetup.exe`; does not add a
service, startup task, Run key, credential or server setup; and does not assert
Phase-C native evidence. P028 remains owner of the Windows clean-machine gate.
