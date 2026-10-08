# Cross-platform clean-machine matrix (P045)

Completed hosted evidence on source `931736c9be13230bf7c3389462c084bedde6a227`
is diagnostic until the required native rows qualify. P045 run `37635214022`
passed source gates but failed docs validation because ripgrep was absent; its
32 BLOCKED aggregate records remain valid. The documentation prerequisite is
now explicit. Windows run `37635213226` failed before Setup on `ncrypt.dll` from
the Qt Schannel backend. Linux packages run `37635213211` failed exact desktop
byte equality; client/maintenance and the separate Qt job matched. AppImage run
`37635213434` passed build/reproduction/manifest/smoke checks. Linux clean-machine
run `37635213585` passed its producer/static jobs but skipped native consumers
under the ordinary PR manual-run condition. The P045 caller requests native
execution and was prevented by contract failure. None is a native row PASS.

Status: **infrastructure/source implemented; acceptance checkpoint withheld.**

Continuation on `9b8cebcd`: Windows native run `37705499583`, producer job
`113078759532`, passed actual runtime closure and AMD64 checks, then failed
archive creation because `zip` was absent. The Windows producers now install
and verify Info-ZIP 3.0 before building; no archive audit is removed.
AppImage PR run `37705503526`, job `113078773574`, passed producer checks:
48,638,456-byte payload, SHA-256
`b98ca6f6ae5f97cc819864390afff49f827b8a9cc32a2e62db34b5d3c82467d0`.
Those attempts remain historical after the prerequisite fix. Exact rerun
identities and subsequent native results are recorded in PR #78; no earlier
producer PASS is promoted into a final-head native PASS.

Fedora image acquisition now uses the confirmed official archive because the
active-release URL returned 404. The archived signed CHECKSUM was verified
against the existing reviewed Fedora 42 fingerprint, and the actual downloaded
532,217,856-byte image matched the unchanged locked SHA-256. The exact platform
and image digest remain the same; acquisition does not prove a native journey.

Windows producer job `113086975910` in run `37707975218` passed on `e2afbc75`
and produced a reproducible 32,891,794-byte Setup, SHA-256
`ea3b707ae73e429ba3638b13493e34820400a6d5288afb2a8933418256004608`.
The actual downloaded archive confirms all producer identity bindings and
`asInvoker`, but consumer `113093002782` failed on a stripped directory prefix
before running acceptance. Its download destination now restores `target/`
without weakening authentication. Windows Server producer/silent scope remains
separate from the required Windows 11 graphical/logon/IPC qualification.

This is the current P045 target ledger. No target is newly accepted by this
document. The scenario JSON and result-v1 schema remain normative. P020/P028
historical records are preserved; P044 process interruption cannot close a
power-cycle gate. P046 remains deferred.

## Exact target and artifact ledger

Linux qualification comes exclusively from
[linux-platforms-v1.json](../../deploy/install/linux-platforms-v1.json).
The profile named `debian-x86_64` qualifies Ubuntu 24.04 only. Windows targets
Windows 11 AMD64; a Windows Server producer is build evidence only.

| Required row | Exact target | Architecture | Scenarios | Result / evidence |
| --- | --- | --- | --- | --- |
| `windows-11-amd64-setup` | Windows 11; tested version/build unavailable | `x86_64` | `INSTALL-JOURNEY-1`, `INSTALL-JOURNEY-6`, `INSTALL-JOURNEY-7`, `INSTALL-JOURNEY-8`, `FIRST-RUN-1`, `FIRST-RUN-2` | BLOCKED; required native evidence unavailable |
| `ubuntu-24.04-x86_64-deb` | Ubuntu 24.04 | `x86_64` | `INSTALL-JOURNEY-2`, `INSTALL-JOURNEY-6`, `INSTALL-JOURNEY-7`, `INSTALL-JOURNEY-8`, `FIRST-RUN-1`, `FIRST-RUN-2` | BLOCKED; required native evidence unavailable |
| `ubuntu-24.04-x86_64-appimage` | Ubuntu 24.04 | `x86_64` | `INSTALL-JOURNEY-4`, `INSTALL-JOURNEY-6`, `INSTALL-JOURNEY-7`, `INSTALL-JOURNEY-8`, `FIRST-RUN-1`, `FIRST-RUN-2` | BLOCKED; required native evidence unavailable |
| `ubuntu-24.04-x86_64-quick-install` | Ubuntu 24.04 | `x86_64` | `INSTALL-JOURNEY-5` | BLOCKED; required native evidence unavailable |
| `fedora-42-x86_64-rpm` | Fedora 42 | `x86_64` | `INSTALL-JOURNEY-3`, `INSTALL-JOURNEY-6`, `INSTALL-JOURNEY-7`, `INSTALL-JOURNEY-8`, `FIRST-RUN-1`, `FIRST-RUN-2` | BLOCKED; required native evidence unavailable |
| `fedora-42-x86_64-appimage` | Fedora 42 | `x86_64` | `INSTALL-JOURNEY-4`, `INSTALL-JOURNEY-6`, `INSTALL-JOURNEY-7`, `INSTALL-JOURNEY-8`, `FIRST-RUN-1`, `FIRST-RUN-2` | BLOCKED; required native evidence unavailable |
| `fedora-42-x86_64-quick-install` | Fedora 42 | `x86_64` | `INSTALL-JOURNEY-5` | BLOCKED; required native evidence unavailable |

Repair/uninstall/interruption and both first-run scenarios appear on each
applicable product format according to the normative families. The quick-install
row uses the same DEB/RPM candidate; its subsequent lifecycle/first-run gates
are listed in the native-package row. Host still requires its qualified runtime
bundle, PostgreSQL and product UI; family applicability alone does not establish
those capabilities. Missing dependencies block rather than create acceptance.

`UPGRADE-TEMPLATE-1` remains blocked/not release-applicable: no version-distinct
v0.2 release candidate exists. Its mechanics fixtures are diagnostic only.

## Machine provenance and blocked execution

| Target | Required provenance | Observed qualifying machine / OS build |
| --- | --- | --- |
| ubuntu-24.04-x86_64 | pinned image SHA-256 `6a81c37564db9b1ee84e141922625e1d7c5b389b99bb3c572e0243607d5bb4d2` | unavailable; pinning authenticates input, not an acceptance PASS |
| fedora-42-x86_64 | pinned image SHA-256 `e401a4db2e5e04d1967b6729774faa96da629bcf3ba90b67d8d9cce9906bec0f` | unavailable; pinning authenticates input, not an acceptance PASS |
| Windows 11 AMD64 | actual disposable interactive VM image, edition/version/build and standard-user identity | unavailable; no Windows build is inferred |

Authoring environment observed on 2026-10-07: Debian GNU/Linux 13 (trixie),
x86_64, no qualified Windows VM or local QEMU/KVM. This host is not a supported
acceptance substitute. GitHub self-hosted runner enumeration returned HTTP 403
(`Resource not accessible by integration`); availability is unknown, not proven
absent. Existing Windows hosted service-session and genuine-logon limitations
remain explicit. No inaccessible runner is selected blindly.

## Candidate and evidence boundaries

Every producer checks out the actual PR head (or dispatch SHA); consumers receive
its frozen bytes rather than rebuild. The Windows producer records detected
OS identity and compiler/linker SHA-256/version, builds Setup twice and compares
bytes. The Linux producer merges DEB/RPM and actual AppImage manifests.

Before observed producer success, artifact ID, filename, source, product version,
size, SHA-256, authenticated manifest identity, exact consumer OS/build, workflow
run/job ID, GUI/IPC probes and cleanup results are **unavailable**, not intended
evidence. Uploaded candidate manifests and run artifacts carry these fields when
produced; the final PR completion record links the observed runs. No hash or
Windows build is invented in this source ledger.

The orchestration workflow reuses the existing Windows and Linux workflows. Four
Linux disposable consumers exercise DEB/RPM and AppImage separately on both
qualified OS targets. Quick install is exercised on the native-package guests.
The aggregate preserves per-row result-v1 records and creates only `contract-valid`
BLOCKED preflight records when a required consumer result is missing. Their
platform facts describe the aggregate host, not a fictitious target execution.

No lower evidence can close a required gate: `ci-native-scoped` silent Setup,
fixture package ownership, unpacked AppDir, offscreen Qt, process SIGKILL and
container restart remain diagnostic. `INSTALL-JOURNEY-8` requires hard VM power
cuts at every named interruption boundary followed by reboot/reconciliation.
Connect (`FIRST-RUN-1`) requires `live-external-dependency`; Host (`FIRST-RUN-2`)
requires its supported installed product path at `native-clean-machine`.

## Current blockers and owning fixes

| Surface | Current observation | Classification / next evidence |
| --- | --- | --- |
| Windows native compiler/linker | Baseline workflow omitted authenticated linker selection; source wiring now uses the active Visual C++ installation and records identities | in-scope candidate build defect; hosted rerun required |
| Windows C++ closure | P044 observed missing MSVCP140.dll; windeployqt can stage an elevated redistributable installer without app-local DLLs | authenticated x64 toolchain CRT is now staged app-locally; exhaustive non-system import audit stays strict; native rerun required |
| Windows Unix-only test imports | Baseline unconditional std::os::unix import reproduced by source inspection | Unix permission/link tests now cfg-gated separately; portable credential tests remain enabled; Windows compilation rerun required |
| AppImage APPIMAGE-8 | Pinned linuxdeploy with a hook wraps custom AppRun into AppRun.wrapped; reproduced at real tool boundary with a diagnostic payload | builder discards only generated cosmetic Qt hook, restores reviewed AppRun and checks byte equality; exact AppImage/native reruns required |
| DEB/RPM desktop reproducibility | Hosted Qt 6.4.2 mismatch traced to allocation-dependent AOT register declarations; raw generator/object differentials reproduced | build-host wrapper canonicalizes only observed independent declarations before compilation and preserves raw provenance; full final-head A/B linked-byte gate must pass |
| Windows/macOS client tests / Linux move intent | baseline failures remain historical observations | broad final-head Rust workflow must rerun; no blanket test disable |
| PostgreSQL crash/restart | historical server failure | separate broad server regression; Host cannot PASS unless its live prerequisites pass |
| P036 production-artifacts identity | acceptance/p036/production-artifacts.json absent at baseline | Host acceptance prerequisite; no synthetic production identity |
| Windows graphical / real logon / installed IPC | hosted service session is insufficient; end-to-end IPC not fully wired | BLOCKED_BY_ENVIRONMENT / missing native implementation; no silent-Setup substitution |
| Linux GUI, trust chain, lifecycle, first run | reviewed guest assertion/action handlers remain incomplete | BLOCKED; adding orchestration does not implement missing product acceptance |
| VM interruption | control-plane power cut exists; no complete product mutation/recovery driver | BLOCKED; final scenario minimum stays interruption-power-cycle |

## Per-scenario evidence and cleanup

The machine-readable row/scenario inventory is
[p045-matrix-v1.json](../../deploy/acceptance/p045-matrix-v1.json).
Run `python3 scripts/install_acceptance.py inventory` for scenario digests and
minimum evidence. Run the P045 validator with `--aggregate`, `--source-commit`,
`--artifact-root`, `--output` and `--require-pass` to emit the actual run ledger.
It rejects ambiguous retries, stale source/scenario digests, changed bytes,
missing target identities, weaker evidence, unsupported versions and invalid
result-v1 structure. It never writes PASS after a missing execution.

No native scenario cleanup has completed in the authoring environment. Missing
executions have `cleanup_result.status=not-run`; actual consumer cleanup is
recorded separately, including failures. The reproduced AppRun diagnostic uses
only scenario-owned files under work/. No unrelated state is removed.

## Unsupported / detected-not-qualified

| Platform | Current status |
| --- | --- |
| Debian (all versions) | detected-not-qualified; no P045 Debian acceptance |
| Linux Mint, Pop!_OS | detected-not-qualified; APT/ID_LIKE does not qualify |
| RHEL, Rocky, AlmaLinux, CentOS, openSUSE | detected-not-qualified; RPM format does not qualify |
| Arch, Manjaro, CachyOS and other Linux distributions | not qualified for P045; AppImage passes on two targets would not prove universal compatibility |
| Ubuntu releases other than 24.04 / Fedora releases other than 42 | not qualified for P045 |
| Windows ARM64, other Windows releases, Linux ARM64/32-bit, macOS/mobile | outside this matrix |

Acceptance remains withheld until every required final-head gate passes at its
declared minimum and the final PR is merged and verified. No release/tag is
created and P046 is not begun.
