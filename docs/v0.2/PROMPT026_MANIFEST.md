# Prompt026 evidence manifest — Windows startup integration

## Identity and inherited state

* Starting SHA: `06add4c87cb82f0ace0ae7888255c7f455600986`.
* Branch: `codex/p026-windows-startup-integration`.
* P024/P025 status inherited from their manifests: source/static implemented;
  final-head hosted native evidence pending. This checkout has no configured
  `origin`, so PR #41/#42 and current hosted runs could not be inspected.
* Directly causal inherited failures fixed: none observed locally.

## Implemented surface

* Preference authority: versioned client-owned non-secret
  `startup-preference.conf`, typed Enabled/Disabled with absence meaning
  preserve/unspecified and malformed input failing closed.
* Installer handoff: absolute installed `synveil-client.exe`, fixed separated
  `--startup-preference enabled|disabled` values, synchronous acknowledgement;
  silent mode requires explicit `/STARTUP=0|1`.
* Profile behavior: no profile is synthesized. Existing/current single desktop
  profile reconciles immediately; otherwise desktop reconciliation follows
  durable profile load.
* Task identity: `\\Synveil\\BackgroundClient\\profile-<ServerProfileId>`.
* Enable: replaces one task and verifies its queried definition. Disable:
  deletes only that profile task and verifies absence. Run: rejects stale tasks.
* Verification checks action, working directory, principal, InteractiveToken,
  LeastPrivilege, LogonTrigger, IgnoreNew, arguments, and password absence.
* Settings and installer use the same Rust preference store and manager.
* P027 cleanup boundary: `--cleanup-startup-integration`; preference and user
  data are preserved.

## Evidence status

Local deterministic Rust/static checks cover persistence, reopen semantics,
manager enable/disable idempotence, XML escaping, canonical paths, and stale
wrong executable/principal/run-level/trigger/argument rejection. Native
standard-user create/run/delete, installed-process readiness, real logon, and
stale-task replacement have not been run in this Linux environment. Workflow
ID and artifact SHA: **not available; no hosted run was started from this
checkout**. These items are pending rather than PASS.

Known blocker: no Git remote is configured, and native Windows Task Scheduler
is unavailable locally. P027 retains full repair/upgrade/uninstall sequencing.
P028 retains native GUI/named-pipe/logon and complete installed-journey
acceptance.

## Files changed

The implementation changes client config/entry/launch authority, desktop
Settings reconciliation, Inno handoff, static validation, P026 documentation,
and the roadmap. No second startup mechanism, service, Run key, Startup-folder
entry, database migration, or credential state was added.
