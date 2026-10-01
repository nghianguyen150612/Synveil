# AppImage runtime integration

## Explicit lifecycle

Opening Synveil normally remains portable: `AppRun` starts the desktop and does
not write integration files. Its fixed internal verbs cover integrate, status,
repair, remove, client enable/disable, and bundled-client dispatch. There is no
generic executable override.

Explicit installation creates one launcher at
`$XDG_DATA_HOME/applications/synveil-appimage.desktop`, one canonical icon at
`$XDG_DATA_HOME/icons/hicolor/scalable/apps/synveil.svg`, schema-1 metadata at
`$XDG_STATE_HOME/synveil/appimage-integration.json`, and a static user unit at
`$XDG_CONFIG_HOME/systemd/user/synveil-appimage-client.service`. Standard XDG
home fallbacks apply. Desktop and systemd commands use their respective
escaping rules; control/non-UTF-8 paths are rejected. Metadata contains only
paths, artifact SHA-256, schema, and the explicit startup choice.

## Inspection, replacement, and removal

Inspection reports `NotIntegrated`, `Healthy`, `StaleAppImagePath`,
`NeedsRepair`, or `Incomplete`. An explicitly opened replacement must be an
executable regular file and is canonicalized; repair atomically rewrites the
singleton owned surfaces without scanning for or deleting the old file.
Missing or divergent effects are never Healthy. Repeated operations are
idempotent.

Writes validate current-user parent ownership and reject symlink/non-regular
destinations. They use an exclusive same-directory temporary file, file fsync,
atomic rename, directory fsync, and byte verification. Removal only removes
the four named integration files. Config, credentials, sync state, libraries,
external data, and AppImages remain untouched.

## Background client and safety boundary

The static user unit invokes the stable AppImage with `--synveil-client`, which
AppRun dispatches to the bundled client. It never records `/tmp/.mount_*`.
Unit installation does not enable it. Explicit enable/disable uses only
`systemctl --user` and returns `UserSystemdUnavailable` without a user manager.

Integration never uses root/system/package-manager paths, changes profiles,
stores secrets, adds TCP IPC, creates portable-home state, or destructively
uninstalls. A non-executable download is rejected and never self-chmodded.
P019 retains the friendly first-launch consent UI.
