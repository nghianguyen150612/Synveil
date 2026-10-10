# Upgrade, repair, and uninstall

> **v0.2 lifecycle qualification is still open.** These pages describe the intended policy and verified source boundaries. Use the current v0.1 [package policy](../RELEASE_PACKAGING.md) and [upgrade safety guide](../UPGRADE_SAFETY.md) for v0.1 operations.

## Lifecycle actions

| Action | Intended v0.2 route | Current release status |
| --- | --- | --- |
| Fresh installation | Use the official artifact and the OS-native or Setup flow in [Installation](INSTALLATION.md). | Producer source exists; v0.2 artifacts are not published. |
| Same-version repair | Use Windows Setup's Repair action or the platform's native package maintenance route when explicitly listed by the release. | Source lifecycle rules exist; platform-native repair is not qualified across the matrix. |
| Compatible upgrade | Use only the newer release and source version named as compatible in authenticated release metadata. | Production channel and v0.1-to-v0.2 compatibility evidence are not published; do not assume an upgrade is supported. |
| Interrupted installation | Use the named Setup recovery or native package-manager reconciliation flow. | Recovery must inspect the previous outcome before another mutation; native interruption qualification is incomplete. |
| Ordinary uninstall | Use Windows' registered Synveil uninstaller, the native package application's Remove action, or remove an AppImage and only its opt-in integration. | Preserve-state behavior is the v0.2 contract; full native qualification remains open. |
| Reinstallation | Reinstall only after the supported repair/removal flow identifies the installed state. | Existing user data is not a disposable installation file. |
| Downgrade | No supported downgrade is promised. | Stop if the release does not explicitly list the exact source/target lifecycle. |

Never use a different installer to overwrite an installation whose owner or version is unknown. Never downgrade by installing an older package or restoring only part of a database.

## Application-owned files and user-owned data

| Application-owned files | User-owned or durable data |
| --- | --- |
| Verified Synveil desktop/client binaries, their packaged runtime, shortcuts/menu entries, and installer-created integration recorded as Synveil-owned. | Your libraries and personal files, local profile and sync state, saved credentials, server configuration, database and server object data, external storage, and backup copies. |

Repair may restore only files and integrations whose ownership is known. It must preserve user-owned data and settings. Ordinary uninstall removes the application's known files and integration; it is not a destructive purge. The v0.2 contract requires preservation of user/server data, but native qualification has not completed, so this preview is not a production guarantee. For current v0.1 behavior, follow the v0.1 release policy linked above.

Before a separately supported destructive administrative procedure, identify the exact Synveil-owned data it removes, make and verify the required backup, and confirm that the procedure names that target. Do not treat an ordinary Uninstall button as permission to erase data. No general desktop-data purge is advertised by this guide.

The repository has a package-neutral server lifecycle script with a separate `--purge` operation for its allowlisted server configuration/state paths. It is an administrator-only packaging interface, not a desktop cleanup command; do not run it as part of ordinary uninstall. Its explicit scope and retained external-data boundary are documented in the [package lifecycle reference](../../../deploy/install/README.md).

## If setup was interrupted

A network download can be started again only before installation mutation. If Setup, a package manager, or server setup may already have changed state, use its recovery action to determine what happened. Keep the original installer/journal evidence and do not delete unknown state, package databases, library folders, or database files. If recovery cannot establish a safe state, stop and seek support.

Rollback of application files is not the same as database rollback. Compatible upgrades must be explicitly supported; an older application must not be forced to use a newer schema. Use only a documented, coordinated backup-and-restore procedure for a supported administrator recovery.
