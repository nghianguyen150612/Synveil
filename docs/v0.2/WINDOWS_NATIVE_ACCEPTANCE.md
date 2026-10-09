# Windows native acceptance

Status: **in progress / native evidence pending**. The Phase-D readiness marker is withheld.

## Current P045 status

P045 infrastructure/source is implemented; acceptance checkpoint is withheld.
[The current matrix](CROSS_PLATFORM_CLEAN_MACHINE_MATRIX.md) records exact
qualified targets and blocked evidence separately. The historical P020/P028
records below are not rewritten as passed. Graphical, first-run, installed IPC
and genuine power-cycle evidence remain required. P046 is deferred.

## Scope and candidate identity

P028 qualifies the advertised Windows 11 AMD64 per-user product, not x86, ARM64,
or every Windows release. `build-windows-candidate` runs on the pinned GitHub
`windows-2025` image, builds the runtime once, builds Setup twice from identical
inputs, requires byte equality, and uploads the second (identical) file as the
sole candidate. `p028-candidate.json` binds its source commit, Cargo version,
byte size, SHA-256, release-manifest SHA-256, Inno Setup 6.7.3, Qt 6.8.3, MSVC
tools identity, and Windows version/build/architecture. Consumers verify both
the candidate digest and source commit after artifact transfer.

## Clean machine and topology

A qualifying clean machine is a disposable native Windows instance whose tested
identity begins without a Synveil package root, HKCU/HKLM registration,
shortcuts, service, Run key, scheduled task, process, profile, or acceptance
state. It does not mean a Linux cross-build, Wine, or fixture simulation.

The producer performs static contracts, native MSVC/Qt compilation, runtime and
release-manifest production, reproducibility, compiled `asInvoker` inspection,
and exact artifact identification. The consumer is a separate fresh hosted VM,
downloads rather than rebuilds Setup, verifies SHA-256, and invokes the existing
disposable non-administrator harness. Bounded evidence is uploaded even after a
failure. Product operations remain non-elevated; only account provisioning and
disposal may use the runner's administrative harness.

## Evidence surfaces

| Surface | Required method | Current status |
|---|---|---|
| Candidate/build | native MSVC/Qt, two byte-identical Setup builds | `SOURCE_PRESENT`; hosted result pending |
| Standard-user install | disposable non-Administrators identity | `SOURCE_PRESENT`; `CI_NATIVE_SCOPED` pending |
| Installed runtime | closed manifest, sanitized environment, unrelated CWD | `SOURCE_PRESENT`; native result pending |
| Local control | installed desktop/client, profile-scoped named pipe, v1 Ping, no TCP fallback | `BLOCKED`; end-to-end harness not yet wired |
| Startup task | runtime authority, canonical XML, run and disable | `SOURCE_PRESENT`; native result pending |
| Interactive UI | real desktop UI Automation and bounded Welcome/options/install/finish screenshots | `BLOCKED`; GitHub-hosted service sessions are not accepted as graphical evidence |
| Real logon | genuine new user logon, not `schtasks /Run` | `BLOCKED`; hosted runner provides no qualified logon transition |
| Repair | exact production candidate, state and adjacent-file preservation | `SOURCE_PRESENT`; native result pending |
| Upgrade/downgrade | isolated P027 older/newer fixtures only | `SOURCE_PRESENT`; native result pending |
| Uninstall/reinstall | registered uninstaller, preserved durable state | `SOURCE_PRESENT`; native result pending |

`STATIC_VERIFIED` means wiring or policy was checked without executing the
Windows product. `CI_NATIVE_SCOPED` means a bounded native sub-surface ran but
does not by itself satisfy a scenario requiring `NATIVE_CLEAN_MACHINE`.
`PASS`, `FAIL`, and `BLOCKED` apply only to the stated gate and never imply a
stronger evidence level.

## Scenario and lifecycle mapping

INSTALL-JOURNEY-1 maps to exact artifact acquisition, ordinary graphical Setup,
terms/options, install, installed launch, named-pipe IPC, and
`installation_ready`. INSTALL-JOURNEY-6 maps to same-version repair of only
owned payload plus launch and preservation. INSTALL-JOURNEY-7 maps to registered
ordinary uninstall and preservation. UPGRADE-TEMPLATE-1 applies only to the
P027 mechanics fixture and is not historical v0.1 release evidence.

Application configuration, Credential Manager identity, client sync state,
synthetic user library, server state, and unknown adjacent files are preserved.
Ordinary uninstall never means purge. Tests may use only synthetic, non-secret
state; diagnostics are bounded and redacted. Setup, uninstall, desktop, client,
Task Scheduler probes, fixture operations, UI automation, and cleanup require
finite timeouts and independently reported cleanup.

## Gate decisions and limitations

The locked INSTALL-JOURNEY-1 contract requires a graphical session and opening
the supported native graphical installer. Therefore silent Setup is useful
installation evidence but cannot qualify the real four-screen UX. Likewise,
P026 task `/Run` proves scheduler execution only; it is not a LogonTrigger
event. Final startup qualification requires a genuine logon transition.

The dedicated workflow intentionally fails with `BLOCKED_BY_ENVIRONMENT` after
recording the unavailable required surfaces. A self-hosted, disposable Windows
11 AMD64 runner with a trustworthy interactive desktop and disposable logon
capability is required to replace those blockers. Named-pipe end-to-end
acceptance must also be wired and pass. Until all required final-head gates are
`PASS` at `NATIVE_CLEAN_MACHINE` strength, the PR remains open, the roadmap
remains in progress, and no Windows readiness marker may be emitted.
