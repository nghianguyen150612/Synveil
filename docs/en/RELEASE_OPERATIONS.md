# Synveil v0.1 operations guide

> Prompt033 publishes no PostgreSQL artifact. Future publication binds exact provenance, inventory, notices and SHA-256; CI packages are fixtures.

This is the user and operator guide for the v0.1 implementation in this
repository. It is narrower than the architecture blueprint: anything marked
deferred or unsupported is not a v0.1 product promise.

## Support matrix

| Area | v0.1 status | Qualification |
|---|---|---|
| Server/API | Supported as the PostgreSQL-backed Rust API and binaries | Deploy with operator-owned PostgreSQL and object storage; desktop packages do not bundle a server or database. |
| Server host | Linux | Requires PostgreSQL, a durable object root, HTTPS termination, and an operator-managed supervisor. |
| Desktop/client | Linux x86_64 | Native package/runtime path and user systemd supervision are provided. |
| Desktop/client | Windows x86_64 | Portable ZIP and Windows code paths are provided; cross-build/compile evidence is not native runtime acceptance. |
| macOS | Deferred; not supported in v0.1 | Do not install or operate v0.1 on macOS. |
| iOS/Android | Deferred; not supported in v0.1 | No mobile client is shipped. |
| Synveil OS | Deferred; not supported in v0.1 | No Synveil OS image is shipped. |

Windows claims are deliberately qualified. Native startup, tray, Task Scheduler,
and filesystem acceptance require a Windows runner. A Linux cross-build is not
Windows runtime validation.

## What is shipped

The v0.1 Linux package contains `synveil-client`, `synveil-desktop`, the
scheduled-maintenance one-shot, package metadata, and a user systemd unit. The
Windows artifact is an unsigned portable ZIP containing
`synveil-client.exe` and `synveil-desktop.exe` plus its Qt runtime closure. It
is not an installer, Windows service, or database bundle.

The workspace also contains `synveil-api`, `synveil-worker`, and
`synveil-scheduled-maintenance-once` binaries. The first two are deployment
components built from the workspace; they are not included in the desktop
package manifest. The API exposes versioned resources below `/api/v1`,
anonymous `/health/live` and `/health/ready` probes, and `/live` and `/ready`
compatibility aliases.

## Installation

### Linux desktop/client

Prerequisites are Linux x86_64, a supported Qt 6 runtime and desktop session,
systemd user management if login supervision is desired, Secret Service/keyring
support for durable device credentials, and a reachable HTTPS server origin.

Build or obtain the Linux artifact from the release process, then install the
DEB or RPM with the host package manager. Package names derive from the
workspace version (`0.1.0` in this checkout):

```sh
./deploy/packages/build.sh --format=all --output-dir=target/packages
sudo dpkg -i target/packages/synveil_0.1.0_amd64.deb
# or on an RPM system:
sudo rpm -Uvh target/packages/synveil-0.1.0-1.x86_64.rpm
```

The package does not enable or start the client silently. Opt into login
supervision explicitly:

```sh
systemctl --user daemon-reload
systemctl --user enable --now synveil-client.service
systemctl --user status synveil-client.service
```

For a staged, non-host install use the repository installer. It requires all
three binaries and never guesses a host root:

```sh
./deploy/install/install.sh --root=/tmp/synveil-root \
  --binary=target/release/synveil-scheduled-maintenance-once \
  --client-binary=target/release/synveil-client \
  --desktop-binary=target/release/synveil-desktop
```

The package-neutral script is packaging/deployment tooling, not the normal
user installation path. Do not run it against `/` unless performing an
administrator-controlled package installation with the explicit host-root
override described in `deploy/install/README.md`.

### Windows desktop/client

Prerequisites are a supported Windows x86_64 host, the Qt/QML closure supplied
by the release ZIP, Windows Credential Manager for device credentials, and a
reachable HTTPS server origin. Extract
`synveil-<version>-windows-x86_64.zip` to a user-owned directory and start
`synveil-desktop.exe`. The ZIP does not require administrator elevation and
does not register a machine-wide service. The client manager may create a
current-user Task Scheduler entry when explicitly enabled. Do not copy Linux
service commands to Windows.

### Server prerequisites

The server side requires Linux, PostgreSQL as the metadata/transaction
authority, a durable object/content root configured as
`SYNVEIL_OBJECT_ROOT`, HTTPS termination, and an operator-owned supervisor and
backup destination. The repository does not provide a guided server installer
or bundled PostgreSQL daemon. Keep PostgreSQL and object storage outside the
desktop package lifecycle.

## First run

The supported desktop flow is:

1. Install and start `synveil-desktop`; the background `synveil-client` is
   independently started or requested by the desktop manager.
2. Enter the server HTTPS origin and a display label. The client performs one
   bounded anonymous `/health/ready` check before saving the profile.
3. Authenticate through the server flow. Device credentials are stored in the
   operating-system SecretStore; the GUI does not own durable auth secrets.
4. Create a library with a logical name and choose a local folder.
5. Wait for bounded initial observation/sync work to converge. An existing
   ordinary non-empty folder is admitted as initial local content; its entries
   become normal durable create/upload/reconcile work and are not discarded.

The selected folder must be absolute, writable, and non-redirecting. Do not
choose the filesystem root, the home directory itself, the current working
directory, a symlink/junction/reparse redirect, or an overlapping managed
root. A remote-library attach/import flow is not provided.

`.synveil` is reserved for Synveil control data. A missing control directory
may be created. An existing control directory is accepted only when its
bounded regular `root-id` marker proves the same managed root. A marker
symlink, directory, incomplete marker, incompatible marker, or incomplete
staging/quarantine entry fails closed; do not delete it to force setup through.

## Server operation

### Configuration and startup

The server owns PostgreSQL connection, migrations, authentication, API state,
and the configured object root. The API binary defaults to `127.0.0.1:3000`;
set `SYNVEIL_BIND_ADDR` only in service configuration. Set
`SYNVEIL_PUBLIC_ORIGIN` to the canonical external HTTPS origin when an
allowed-origin policy is needed. With PostgreSQL configured, the API requires
`SYNVEIL_REBASELINE_TOKEN_KEY` and runs forward migrations before serving.

For a source/workspace deployment, the composition commands are:

```sh
cargo run --release --locked -p synveil-api --bin synveil-api
cargo run --release --locked -p synveil-api --bin synveil-worker
```

These are operator/developer commands, not a claim that the desktop package
installs or supervises the server. Use a service manager in production and
inject credentials through its secret mechanism. The API source currently
accepts `DATABASE_URL`; do not place a real password in checked-in files or
release documentation. The worker has no listener and requires
`SYNVEIL_OBJECT_ROOT` when enabled.

Probe `GET /health/live` for process liveness and `GET /health/ready` for
cached readiness. Use authenticated `/api/v1/system/health` for the permitted
operational view. Keep API/worker stdout and stderr under the service
manager's journal/log policy. Rust components honor `RUST_LOG`; never enable a
policy that records credentials, cookies, authorization headers, or database
URLs. Stop through the service manager, allow bounded worker cycles to drain,
then start the same matched release again.

The scheduled-maintenance system unit is separate from the API and client. It
is not a general server daemon and is not enabled by package installation.

## Desktop operation

`synveil-client` owns synchronization, local SQLite state, root probes,
SecretStore access, recovery, and local control IPC. `synveil-desktop` is the
native Qt UI/tray/control surface. Closing the GUI stops its controller
connection only; it does not stop an already-running `synveil-client`.

On Linux, the client is a user service at
`/usr/lib/systemd/user/synveil-client.service`, not a root system service. On
Windows, background supervision is current-user Task Scheduler, not a Windows
Service. Reopening the GUI reconnects to the existing client rather than
creating a second sync engine.

### Pause, resume, and Sync Now

- **Pause** persists a user pause state and blocks new periodic, manual,
  network, credential, and filesystem wakeups. Work already in flight is not
  hard-killed.
- Durable SQLite state, pending intents, root binding, conflict records, and
  the pause setting remain intact. Pause does not delete pending work.
- **Resume** clears only the user-pause reason and allows normal scheduling to
  continue; it does not reset state or force a broad destructive rescan.
- **Sync Now** while paused returns a paused scheduling result and does not
  bypass the pause. Resume first, then use Sync Now if a bounded wake is needed.

### Root unavailable safety

**A missing, unmounted, or otherwise unavailable local root is not interpreted
as deletion of every file.** The library remains fenced/deferred as
`RootUnavailable`. Wait for the exact folder or mount to return, then use the
desktop's check/retry or restart the client. Do not create an empty replacement
folder at the same path, delete `.synveil`, or manually remove SQLite state.

### Conflicts and attention

The desktop shows durable attention for supported managed conflicts. Actions are
type-dependent:

- **Accept Remote** accepts the current server decision where the conflict
  exposes that action.
- **Retry Local** retries the preserved local intent against the current base
  where the conflict exposes that action; it is not an unconditional replay.

Some conflict types expose only one action. There is no automatic merge or
automatic conflict-resolution promise. Resolve from the attention surface and
wait for refreshed authoritative status.

### Ambiguous outcomes

If a response is lost after an operation may have committed, the UI reports
`OutcomeUnknown` and refreshes durable state. Synveil may need to reconcile
whether the operation succeeded before safely retrying. Do not click the same
destructive action repeatedly, delete local state, or invent a second library.

## Recovery guide

| Situation | Automatic/waiting behavior | User action |
|---|---|---|
| Client unavailable | Supervisor/reconnect uses bounded recovery | Start the client or inspect its service status. |
| Authentication required | Runtime waits without discarding the library | Sign in again; do not delete SecretStore entries. |
| Server unavailable | Bounded retry/backoff and refresh | Restore server reachability/TLS, then use Check again. |
| Local root unavailable | Root remains fenced; no mass deletion | Reconnect the exact folder/mount and check again. |
| Pending setup | Durable pending identity is reconciled on restart | Resume setup with the same server/profile/folder; do not duplicate the library. |
| Ambiguous previous request | Durable receipt/fence is refreshed; no blind replay | Wait for reconciliation, then retry only when the UI presents a safe action. |
| Conflict attention | Conflict remains durable | Use only the displayed supported action. |

## Backup and restore

Back up each ownership domain separately:

1. **PostgreSQL data:** use a PostgreSQL-consistent logical or physical backup
   procedure appropriate to the deployment. Do not copy a live PostgreSQL data
   directory as a generic logical backup.
2. **Server configuration:** `/etc/synveil` and service-manager environment or
   secret references, excluding plaintext secrets from ordinary archives.
3. **Server-owned application state:** `/var/lib/synveil` where the deployment
   uses it, plus the configured object/content root and external backup
   destination. These are distinct from PostgreSQL metadata.
4. **Client state:** platform profile/configuration directories and client
   SQLite state. Linux defaults are `$XDG_CONFIG_HOME/synveil` and
   `$XDG_DATA_HOME/synveil`, falling back to `~/.config/synveil` and
   `~/.local/share/synveil`; Windows uses `%APPDATA%\Synveil` and
   `%LOCALAPPDATA%\Synveil`.
5. **Credentials:** device credential bytes in Linux Secret Service or Windows
   Credential Manager. A directory copy does not capture SecretStore data.
6. **Synced user content:** external library roots are user data, not ordinary
   package state. Back them up according to their storage system.

For a conservative manual restore: restore durable PostgreSQL state and the
server-owned object/content roots, restore configuration and credential
references, restore SecretStore entries through the operating system, start
the server and verify readiness, then reconnect clients. If SecretStore
entries cannot be restored, authenticate/enroll the affected device again. No
automated disaster-recovery workflow is claimed.

## Upgrade

Before upgrading, stop the client/API/worker as appropriate and verify a backup
of configuration, PostgreSQL, object roots, and client state. Package upgrade
replaces package-owned files only and preserves durable state. Migrations are
forward-applied by the relevant runtime; packaging does not silently reset a
database.

The frozen v0.1 schema is **36 PostgreSQL migrations**, **7 client SQLite
migrations**, and `LOCAL_SCHEMA_VERSION = 7`. Unknown future schema fails
closed. Automatic downgrade is not guaranteed. Never delete or recreate a
database to make an older binary start; use a matched compatible binary or an
administrator-verified restore.

After upgrade, verify profile identity, library bindings, root availability,
pause state, unresolved conflicts, health/readiness, and client reconnect
before resuming normal work. A partially replaced package payload can be
repaired by reinstalling the intended package; that is not a whole-package
rollback guarantee.

## Uninstall and purge

Ordinary package uninstall removes known PACKAGE artifacts only. It preserves
`/etc/synveil`, credentials, `/var/lib/synveil`, user profiles, PostgreSQL,
external object/backup roots, and synced library content. Explicit purge is
destructive and removes only the allowlisted Synveil-owned `/etc/synveil` and
`/var/lib/synveil` paths after containment checks. It still does not remove
PostgreSQL, external storage, mounted volumes, home data, or the synced library
root.

```sh
./deploy/install/uninstall.sh --root=/tmp/synveil-root --purge
```

Never use a purge path that contains external content or a mounted storage
pool. The script refuses unsafe symlink/parent escapes and does not remove the
`synveil` account automatically.

## Security and networking assumptions

- Device credentials are owned by the OS SecretStore; desktop UI/configuration
  stores references and non-secret profile metadata, not durable auth secrets.
- Local desktop IPC is user-scoped: Unix socket peer credentials on Linux and
  owner-restricted named pipes on Windows. There is no public control TCP
  listener.
- Production profile onboarding requires HTTPS with certificate validation,
  no redirect fallback, and a reachable `/health/ready` endpoint. The API
  process defaults to loopback HTTP; TLS termination belongs to the operator's
  reverse proxy or equivalent deployment boundary.
- The self-hosting operator controls the server host, PostgreSQL, TLS keys,
  object roots, backups, and service permissions. Synveil does not claim
  end-to-end encryption or protection from a compromised host/operator.
- Local root validation rejects redirects, unsafe overlaps, reserved control
  paths, unsupported names, and case/normalization collisions where the target
  filesystem cannot represent them safely. Safe rejection is preferable to
  silent data loss.

## Diagnostics and troubleshooting

Useful non-destructive commands include:

```sh
systemctl --user status synveil-client.service
journalctl --user -u synveil-client.service -n 100 --no-pager
systemctl --user restart synveil-client.service
curl --fail --silent https://server.example/health/live
curl --fail --silent https://server.example/health/ready
```

Use the package manager's installed-version query or the release ZIP manifest
for version reporting. Do not invent Synveil CLI subcommands.

| Symptom | Likely category | Safe next action | Do not do |
|---|---|---|---|
| Desktop cannot reach client | IPC/process lifecycle | Start client, inspect user-service status, then reopen/check again. | Do not expose IPC over TCP or delete profile state. |
| Client not running | Supervisor, install, or config | Check package paths and user-service status; use the packaged sibling. | Do not run a second client against the same profile/root. |
| Server unreachable | Network, TLS, or service | Check health probes, proxy certificate, and server logs. | Do not switch to plaintext HTTP or disable certificate validation. |
| Authentication required | SecretStore/session | Sign in or enroll again through the UI. | Do not copy tokens into logs or config files. |
| Local root missing | Mount/path availability | Restore the exact mount and use Check again. | Do not replace it with an empty folder or delete `.synveil`. |
| Sync paused | User control | Resume, then optionally use Sync Now. | Do not expect Sync Now to bypass Pause. |
| Conflict requires attention | Supported conflict action | Use only displayed Accept Remote or Retry Local. | Do not manually edit SQLite or blindly replay. |
| Package/runtime dependency problem | Qt/systemd/keyring/package | Check package dependencies, Qt runtime, user manager, and Secret Service/Credential Manager. | Do not mix binaries from different releases. |
| Qt startup problem | Native GUI/runtime closure | Run the matching package on its supported host and inspect logs. | Do not copy random Qt DLLs/libraries into the package. |
| Upgrade/schema rejection | Incompatible or unknown durable state | Preserve state, install a compatible release, or restore a verified backup. | Do not drop, wipe, or downgrade the database blindly. |

## Known limitations

- Native Windows runtime acceptance is an environment gate; cross-build or
  MinGW link evidence is not native Windows acceptance.
- macOS, iOS, Android, and Synveil OS are not supported in v0.1.
- The desktop package is not a server/database installer; server deployment,
  PostgreSQL lifecycle, TLS termination, and backup scheduling remain operator
  responsibilities.
- Automatic downgrade and transactional rollback of a partially replaced
  package set are not guaranteed.
- Filesystem filename semantics differ across platforms. Unsupported target
  names, case-folding collisions, reserved names, redirects, and incomplete
  control markers may fail safely rather than sync.
- SecretStore-backed credentials are not captured by copying configuration or
  SQLite directories.
- Native Windows and disposable-PostgreSQL acceptance are separate release
  validation gates and may be blocked by the environment; that is not a
  documentation failure.

## Release-facing source of truth

- Package/install paths: `docs/en/RELEASE_PACKAGING.md` and
  `deploy/install/README.md`.
- Upgrade invariants: `docs/en/UPGRADE_SAFETY.md`.
- Desktop lifecycle and controls: `docs/en/DESKTOP_LAUNCH.md` and
  `docs/en/DESKTOP_CONTROL.md`.
- API/health contract: `docs/en/API_ARCHITECTURE.md`.
- Security boundary: `docs/en/SECURITY.md`.
- v0.1 user-visible summary: `docs/en/RELEASE_NOTES_v0.1.md`.
