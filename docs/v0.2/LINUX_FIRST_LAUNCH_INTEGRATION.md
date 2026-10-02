# Linux first-launch integration

Prompt019 connects the installed desktop to the existing process-management and
AppImage integration boundaries. It does not make the desktop the synchronization
owner and does not add a daemon. On every open the Qt bridge asynchronously calls
`BackgroundClientManager::ensure_running()` for current-session availability.
Closing the desktop does not stop that independently supervised client.

## Consent and state

A versioned, current-user `QSettings` pair (`startupChoiceVersion = 1` and
`startupChoice = enabled|disabled`) records whether the question has been
answered. It contains no account, server, library, token, path, or credential.
An absent or unrecognized version means “never asked”; `disabled` means the user
explicitly declined. The recommendation is selected visually, but opening the
app performs no startup-registration mutation. Mutation begins only when the
user activates **Continue**.

The actual user-manager registration is always authoritative. A saved enabled
choice is history, not permission to repair or re-enable a registration. Both
the first-launch surface and Settings use the same asynchronous typed operation.
Success is saved only after a status read verifies the requested state. Failure
leaves the first-launch choice open and the rest of the desktop usable.

For DEB/RPM installs, the existing manager enables or disables only
`/usr/lib/systemd/user/synveil-client.service`; current-session startup remains a
separate `ensure_running()` operation. User-systemd unavailability is reported
as unavailable and never falls back to cron, a system unit, shell startup, or
XDG autostart.

## AppImage

Portable launch performs no integration. Startup Off records only the preference
when no integration exists. Explicit startup On composes the P016 typed engine:
validate the outer `APPIMAGE`, install current-user desktop/icon/user-unit
surfaces from `APPDIR`, verify a Healthy integration, enable
`synveil-appimage-client.service`, and read `systemctl --user is-enabled`.
Existing stale, incomplete, or repair-needed integration is rejected rather than
blindly enabled. The generated unit continues to name the canonical outer
AppImage and therefore never persists a `/tmp/.mount_*` path. Disabling leaves
the optional integration installed while disabling its startup registration.

All filesystem and process operations execute on Tokio workers (blocking P016
operations use `spawn_blocking`) and results cross the CxxQt queue. QML receives
only bounded state, busy, availability, selection, and generic feedback values;
it supplies neither commands nor executable paths.

## Acceptance boundary

P019 establishes `INSTALLATION_READY → APPLICATION_LAUNCHED` for focused Linux
paths. It does not claim `PROFILE_READY`, `AUTHENTICATED`, `LIBRARY_READY`, or
`SYNC_STARTED`. Full clean-machine, graphical-package, repair, upgrade,
uninstall, and interruption acceptance remains P020.
