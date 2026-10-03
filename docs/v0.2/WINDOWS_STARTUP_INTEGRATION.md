# Windows startup integration

## User behavior and preference authority

**Start Synveil when I sign in** controls only future interactive-user logons. It
does not prevent the desktop from directly starting the client in the current
session. The installer and desktop Settings use the same client-owned,
non-secret `startup-preference.conf` state below the platform configuration
root. Its versioned typed values are `Enabled` and `Disabled`; an absent value
means unspecified/preserve, and malformed or unknown data fails closed without
enabling startup.

A fresh interactive install records the checked choice (checked by default).
Silent install requires `/STARTUP=0` or `/STARTUP=1`. An upgrade/repair without
that parameter does not overwrite the existing preference. Setup invokes the
absolute installed `synveil-client.exe` with the fixed
`--startup-preference enabled|disabled` boundary after payload installation,
waits for a bounded result, and never writes application state or Task Scheduler
XML itself.

If no durable profile exists, the client acknowledges the durable preference
without creating a profile or task. When the desktop later loads its one
current authoritative profile, it reconciles that saved choice. This reflects
the current single active desktop-profile model; P026 does not create tasks for
historical profiles or introduce profile selection. Settings writes the same
preference before applying it, then reads actual scheduler state for its shown
result. A failed scheduler operation retains consent so a later Settings or
repair reconciliation can retry.

## Task Scheduler authority

`BackgroundClientManager` and `NativeBackgroundLaunchBackend` in
`crates/client/src/launch.rs` remain the sole production authority. The task is:

* `\\Synveil\\BackgroundClient\\profile-<ServerProfileId>`;
* owned by the current Windows user, with `InteractiveToken`,
  `LeastPrivilege`, and a current-user `LogonTrigger`;
* one `Exec` action for the canonical installed sibling `synveil-client.exe`,
  with its parent as working directory and no arguments;
* `IgnoreNew`, start-when-available, unlimited execution time, battery starts
  allowed, and the existing bounded restart policy;
* free of passwords, credentials, server information, library paths, shells,
  services, Run keys, SYSTEM, and elevation.

The runtime resolves `%SystemRoot%\\System32\\schtasks.exe` rather than PATH and
passes arguments separately. Registration uses a create-new private per-user
temporary file, completes and syncs it before use, and requires cleanup on both
success and failure. Enable replaces the same task (refreshing relocation or
upgrade paths) and verifies the queried XML before reporting Enabled. Disable
is idempotent and verifies absence. Status trusts a task only after checking its
principal, logon type, run level, trigger, executable, working directory,
multiple-instance policy, lack of arguments, and lack of a password. A same-name
stale task is reported non-enabled, is never run, and is replaced only by an
explicit enable/reconciliation. `run_autostart` likewise requires a verified
definition. Current-session launch may fall back directly when registration is
absent, stale, or unavailable; that is distinct from login preference.

## Lifecycle and security boundary

`--cleanup-startup-integration` is the bounded, client-owned deletion boundary
reserved for P027 to call before binaries disappear. It accepts no task name,
profile ID, executable, XML, URL, path, or credential and preserves preference
and all user data. Full repair, upgrade, uninstall sequencing remains P027.
Real logon-session, installed GUI, named-pipe, and complete acceptance remain
P028.

All production operations are current-user and work without elevation. Unit
and static tests cover durable state, malformed fail-closed behavior, XML
escaping, canonical action construction, idempotent manager behavior, and stale
definition rejection. Linux cannot execute native Task Scheduler behavior;
final-head hosted Windows native evidence is pending and is not represented as
a pass here.
