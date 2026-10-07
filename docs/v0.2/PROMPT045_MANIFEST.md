# Prompt045 manifest — Cross-platform clean-machine matrix

Status: **infrastructure/source implemented; acceptance checkpoint withheld.**

| Field | Observed value |
| --- | --- |
| Repository | https://github.com/nghianguyen150612/Synveil.git |
| Repository path | `/workspace/Synveil` |
| Expected baseline | `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc` |
| Actual fetched origin/main baseline | `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc` |
| Starting branch / HEAD | `work` / expected baseline |
| Worktree before changes | clean |
| Required branch | `feat/cross-platform-clean-machine-matrix` |
| Naming verification | local name checked before creation; no `codex/` prefix; remote must be queried again before PR |
| P043 ancestry | merge `3a927708142b4bee3a4582cc54e6dab1ea47fdc8` remains ancestor |
| P044 ancestry | merge `631d448773598d789a49d4a7f078d2d66a94e2c2` and head `b5f46b3fd9dfb4b5e7662cf5524e2faa559aaeb8` remain ancestors |
| Intervening main commits / existing P045 | none after expected baseline; no P045 branch/PR found before work |
| Source heads / final reviewed head | commit-derived identities are recorded by each producer and the observed PR completion record; no future self-SHA is invented |
| Producer runs / exact artifact IDs, bytes, hashes | unavailable before publication and observed producer execution |
| Windows target | Windows 11 AMD64; exact tested edition/version/build unavailable |
| Linux targets | Ubuntu 24.04 x86_64; Fedora 42 x86_64 |
| AppImage environments | both exact Linux targets; no universal Linux qualification |
| Debian | detected-not-qualified |
| Authoring host | Debian GNU/Linux 13 x86_64; not a native matrix target |
| Interactive Windows / real logon | unavailable in known hosted topology; runner enumeration denied with HTTP 403 |
| VM/image provenance | Linux locked digests in matrix; no qualifying consumer image/build observed locally |
| GUI / installed IPC / lifecycle / first-run | required; unavailable qualifying final-head records |
| Interruption/power-cycle | minimum remains `interruption-power-cycle`; no process-kill promotion |
| Reproducibility | required final-head hosted reruns; historical desktop mismatch not claimed resolved |
| Cleanup | local diagnostic files scoped to work/; native scenario cleanup not run |
| PR / hosted run/job IDs | unavailable before publication; append only observed identities |
| Merge / resulting main | not merged; acceptance withheld |
| Release boundary | P046 deferred; no RC, release or v0.2.0 tag |

## Contract audit and delivered changes

Audited the roadmap and installation product contract; P004 scenario/result
schemas and inventory; Linux/Windows native acceptance and P020/P028 manifests;
P043/P044 manifests and security/resilience boundaries; existing Linux adapter
and VM control plane; image lock and platform qualification policy; Windows,
Linux package and AppImage producer/consumer workflows and validators.

[The matrix](CROSS_PLATFORM_CLEAN_MACHINE_MATRIX.md) contains every required
row, scenario ID, exact target, minimum evidence and unsupported status.
`deploy/acceptance/p045-matrix-v1.json` is the machine-readable row ledger.
The validator checks that ledger against policy and validates/aggregates real
result-v1 evidence without promoting missing executions. Its adversarial tests
cover weaker evidence, mislabeled fixtures/offscreen sessions, stale sources,
tampered candidate bytes, wrong platforms/images, unknown schema fields,
missing assertions/rows, and process kill mislabeled as power-cycle.

The orchestration reuses platform workflows, passes the true source head to
producers/consumers, separately exercises AppImage and packages on both exact
Linux targets, preserves failed/blocked evidence, and fails when a required
record is missing. Setup records are diagnostic rather than duplicate results.
Artifact acquisition directories and per-scenario authorization-secret lifetime
are corrected; no password is logged. Native handlers remain closed dispatch.

Source/build fixes are limited to owning defects: Windows candidate tool selection
uses authenticated MSVC compiler/linker identities; runtime closure stages x64
Microsoft CRT DLLs app-locally, retaining strict import checks; Unix-only
credential permission/link tests are separately cfg-gated while portable tests
remain enabled. The actual pinned linuxdeploy tool reproduced its custom-AppRun
wrapper when a hook exists. The builder removes only the generated cosmetic Qt
hook in its private AppDir, restores the reviewed AppRun and compares exact
entrypoint bytes. No validator is weakened to accept a broken entrypoint.

These are source/tool-boundary fixes, not clean-machine PASS evidence. Exact
rebuilt Setup/DEB/RPM/AppImage bytes and native reruns remain mandatory.

## Evidence and withheld gates

Every applicable scenario must emit result-v1 with its definition digest, source,
times, observed host, exact candidate and manifest identities, capabilities,
completed steps/assertions, minimum evidence and separately observed cleanup.
The aggregate supplies only contract-valid BLOCKED preflight if no consumer
record exists; its observed host is not rewritten as Windows/Fedora/Ubuntu.

Windows graphical Setup, actual standard-user logon and installed named-pipe
acceptance remain blocked by environment/implementation. Linux reviewed GUI,
verified quick-install trust, native preservation/lifecycle and installed first-run
handlers remain incomplete. The VM controller can issue a hard power cut but
does not yet establish the required product mutation/reboot/reconciliation
assertions at every interruption point. First-run Connect needs a controlled live
server and SecretStore; Host needs qualified production runtime identities and
its real supported UI path. No hidden manual provisioning qualifies them.

P044 historical Rust/client/watcher, desktop reproducibility and PostgreSQL
failures are diagnostic until rerun on this source. Missing
`acceptance/p036/production-artifacts.json` is an explicit Host prerequisite,
not a fabricated production identity. No unrelated server fix or new ADR is
invented without a reproduced owning defect.

Source checks, hosted results, artifact identities and the PR publication are
reported only after observation. A blocked infrastructure PR stays open; green
source checks alone cannot complete P045 or unlock P046.

P046 deferred. No v0.2.0 release/tag is created.

## Local validation before publication

- `cargo fmt --all -- --check`: passed after formatting the scoped cfg change.
- Strict Clippy for `synveil-install-engine --all-targets --locked`: passed.
- `cargo test -p synveil-install-engine --locked`: passed, 526 tests.
- `cargo deny --locked check`: passed; existing advisory warnings retained.
- P004 contract tests: 21 passed; P020 adapter tests: 15 passed.
- P045 adversarial matrix/controller tests: 14 passed, including bounded timeout
  and reject-before-SSH checks. These are fixtures, not native acceptance.
- Docs, installer security, resilience, Windows installer/native, AppImage
  source, Linux platform/quick-install/DEB/RPM/first-launch and Windows rustflags
  validators: passed. Shell syntax, YAML parsing and `git diff --check`: passed.
- Broad workspace strict Clippy: blocked locally at missing `dbus-1.pc` /
  `libdbus-1-dev`; no broad source PASS is claimed. The orchestration installs
  native DBus/Qt prerequisites and gates candidate consumers on the real hosted
  format/Clippy/dependency checks.
- The Linux first-launch source validator still referenced removed Qt settings
  persistence. It now checks the canonical client startup preference store's
  versioned enabled/disabled parsing and the desktop's persist-before-apply path.
  This reconciles a stale validator without changing startup product behavior.
- The VM controller script was not executable at baseline despite direct workflow
  invocation. Its executable mode is corrected along with bounded child metadata
  reload, QEMU output isolation and bounded artifact staging.

No native graphical install, launch, named-pipe, first-run, live server or product
power-cycle PASS was produced locally. Candidate SHA-256/size/build identities
remain unavailable until actual hosted producers pass.

## Initial publication and diagnostic correction

- PR [#78](https://github.com/nghianguyen150612/Synveil/pull/78) is open as a
  draft from exactly `feat/cross-platform-clean-machine-matrix` to `main`.
  GitHub's branch API verified that name and initial head
  `9e1ea1cb7ca1adaa730014d20d8a0885b9495ff8` immediately before creation;
  initial tree `690c924ab26001a8eea0d82af5ced86c1ca32d37` matched publication.
- Initial Windows native push run `37632007612`, job `112828403852`, failed
  before runtime compilation in the new tool-identity preparation. The active
  hosted toolchain was Visual Studio 18, MSVC `14.51.36231`; its redist lacked
  the helper's assumed `Microsoft.VC143.CRT` directory. This was a P045 harness
  selection defect, not an installed product FAIL. The helper now selects
  exactly one generation-named CRT directory strictly inside the active x64
  redist root and records the observed name. It does not search System32 or
  silently select another SDK.
- Initial Windows installer push run `37632007596` also failed before Setup.
  Initial AppImage push `37632007346` and aggregate PR `37632164383` had not
  completed at correction time. Their superseded-head results are diagnostic
  only; the correction requires final-head reruns and no PASS is reused.

Acceptance remains withheld. No Windows 11 build, candidate hash, native
journey, power-cycle, merge or resulting main is inferred from these runs.

## Runtime import-audit correction

On corrected source `9c9580cc2ca17c65db6696f9bb1d4d9cb9d23bb6`, Windows native
push run `37632560247`, job `112830314309`, selected the active toolchain,
compiled the runtime, staged the app-local CRT and reached the exhaustive import
audit. The remaining failure was `UIAutomationCore.DLL` imported by the Qt
Windows platform plugin. Microsoft's
[UiaReturnRawElementProvider API requirements](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationcoreapi/nf-uiautomationcoreapi-uiareturnrawelementprovider)
identify this as the OS UI Automation DLL. It is now classified as a Windows
system API; MSVCP140/VCRUNTIME140 remain non-system and must be shipped. A focused
classifier regression checks that boundary. This is a corrected audit defect,
not a claim that the complete installed product or Windows 11 journey passed.

Toolchain diagnostics now retain observed source, compiler/linker versions and
hashes, active CRT generation/DLL hashes, and detected producer OS caption,
version/build/architecture even after a later candidate build failure. Producer
Windows Server identity cannot qualify the Windows 11 target. Subsequent exact
candidate builds and final-head reruns remain required.

Broad workspace strict Clippy subsequently passed locally using scenario-owned
DBus development metadata and Qt 6.7.3 staged under work/. The earlier missing
local dependency result is preserved above. This remains source evidence on
Debian 13, not native acceptance.
